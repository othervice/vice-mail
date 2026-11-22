//! Prefetching bodies in the background (stage 3, decision 0009): every message of the period
//! in Inbox, Sent and Drafts, and flagged messages elsewhere, so they can be read offline.
//!
//! - One task per account, woken after every completed sync pass (the account's status turns
//!   `idle`: after the first sync, and after passes that brought new mail) and when the settings
//!   change. A wake during a run starts another run afterwards.
//! - A run asks the store for candidates (newest first), fetches messages up to the size limit
//!   whole in batches of at most [`BATCH_BYTES`] / [`BATCH_MESSAGES`] and stores each batch
//!   with one `store_bodies` (one file, one fsync); larger messages only with their readable
//!   parts. Then the list previews of the batch, one `messagesChanged` per batch and mailbox,
//!   and `prefetchProgress` at most about once a second.
//! - Interruptible between batches: the account went offline (state `paused` until it is idle
//!   again), the settings changed (a new run starts), the account was removed (the task ends).
//! - A message that fails is skipped for the rest of the session; a connection failure ends the
//!   run (the next pass starts a new one).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mail_model::{
    AccountId, BodyFetchPlan, CoreError, CoreEvent, MailConnector, MailboxId, MessageId,
    PrefetchState, PrefetchStatus, UidSet,
};
use mail_store::{MAX_PREFETCH_BATCH, PrefetchCandidate, StoredAccount};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::Reading;
use crate::sync::emit;

/// Raw bytes (RFC822.SIZE) per FETCH of whole messages.
pub const BATCH_BYTES: u64 = 2 * 1024 * 1024;
/// Messages per FETCH of whole messages.
pub const BATCH_MESSAGES: usize = 50;
/// Shortest interval between two `prefetchProgress` events of a running prefetch.
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// The prefetch tasks of all accounts.
#[derive(Default)]
pub(crate) struct Prefetcher {
    accounts: Mutex<HashMap<AccountId, AccountPrefetch>>,
}

struct AccountPrefetch {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

/// What the task of one account shares with the rest of the core.
struct Shared {
    wake: Notify,
    /// Set while the account is offline; the run stops at the next batch.
    paused: AtomicBool,
    /// Set when the settings changed; the run stops and a new one starts.
    restart: AtomicBool,
    status: Mutex<PrefetchStatus>,
    /// Messages that failed in this session; not tried again.
    failed: Mutex<HashSet<MessageId>>,
}

impl Shared {
    fn status(&self) -> PrefetchStatus {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_status(&self, status: PrefetchStatus) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }

    fn interrupted(&self) -> bool {
        self.paused.load(Ordering::SeqCst) || self.restart.load(Ordering::SeqCst)
    }
}

const IDLE: PrefetchStatus = PrefetchStatus {
    state: PrefetchState::Idle,
    done: 0,
    total: 0,
    bytes: 0,
};

impl Prefetcher {
    /// The account's status; idle if it never ran.
    pub fn status(&self, account: AccountId) -> PrefetchStatus {
        self.lock()
            .get(&account)
            .map_or(IDLE, |a| a.shared.status())
    }

    /// Starts a run for the account (starting its task on first use).
    pub fn wake<C: MailConnector + 'static>(&self, reading: &Arc<Reading<C>>, account: AccountId) {
        let shared = self.shared(reading, account);
        shared.paused.store(false, Ordering::SeqCst);
        shared.wake.notify_one();
    }

    /// The account is offline: a running prefetch stops at the next batch; the state is
    /// `paused` until the account is idle again.
    pub fn pause<C: MailConnector + 'static>(&self, reading: &Arc<Reading<C>>, account: AccountId) {
        let shared = self.shared(reading, account);
        shared.paused.store(true, Ordering::SeqCst);
        let status = PrefetchStatus {
            state: PrefetchState::Paused,
            ..shared.status()
        };
        if shared.status() != status {
            shared.set_status(status);
            reading.emit(CoreEvent::PrefetchProgress {
                account_id: account,
                status,
            });
        }
    }

    /// New settings: every account starts over (and forgets which messages failed).
    pub fn restart_all(&self) {
        for prefetch in self.lock().values() {
            prefetch.shared.restart.store(true, Ordering::SeqCst);
            prefetch
                .shared
                .failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            if !prefetch.shared.paused.load(Ordering::SeqCst) {
                prefetch.shared.wake.notify_one();
            }
        }
    }

    /// The account was removed: its task ends.
    pub fn forget(&self, account: AccountId) {
        if let Some(prefetch) = self.lock().remove(&account) {
            prefetch.task.abort();
        }
    }

    pub fn abort_all(&self) {
        for (_, prefetch) in self.lock().drain() {
            prefetch.task.abort();
        }
    }

    fn shared<C: MailConnector + 'static>(
        &self,
        reading: &Arc<Reading<C>>,
        account: AccountId,
    ) -> Arc<Shared> {
        let mut accounts = self.lock();
        let entry = accounts.entry(account).or_insert_with(|| {
            let shared = Arc::new(Shared {
                wake: Notify::new(),
                paused: AtomicBool::new(false),
                restart: AtomicBool::new(false),
                status: Mutex::new(IDLE),
                failed: Mutex::default(),
            });
            let task = tokio::spawn(account_task(
                Arc::downgrade(reading),
                account,
                Arc::clone(&shared),
            ));
            AccountPrefetch { shared, task }
        });
        Arc::clone(&entry.shared)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<AccountId, AccountPrefetch>> {
        self.accounts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

async fn account_task<C: MailConnector + 'static>(
    reading: Weak<Reading<C>>,
    account: AccountId,
    shared: Arc<Shared>,
) {
    loop {
        shared.wake.notified().await;
        let Some(reading) = reading.upgrade() else {
            return;
        };
        if shared.paused.load(Ordering::SeqCst) {
            continue;
        }
        shared.restart.store(false, Ordering::SeqCst);
        let mut run = Run::new(&reading, account, &shared);
        let outcome = run.execute().await;
        let state = if shared.paused.load(Ordering::SeqCst) {
            PrefetchState::Paused
        } else {
            PrefetchState::Idle
        };
        if let Err(err) = outcome {
            tracing::warn!(account = %account, code = ?err.code(), error = %err, "prefetch stopped");
        }
        run.report(state, true);
        if shared.restart.load(Ordering::SeqCst) {
            shared.wake.notify_one();
        }
    }
}

/// One prefetch run of one account.
struct Run<'a, C: MailConnector> {
    reading: &'a Arc<Reading<C>>,
    account: AccountId,
    shared: &'a Shared,
    status: PrefetchStatus,
    last_report: Option<Instant>,
}

impl<'a, C: MailConnector + 'static> Run<'a, C> {
    fn new(reading: &'a Arc<Reading<C>>, account: AccountId, shared: &'a Shared) -> Self {
        Self {
            reading,
            account,
            shared,
            status: PrefetchStatus {
                state: PrefetchState::Running,
                done: 0,
                total: 0,
                bytes: 0,
            },
            last_report: None,
        }
    }

    /// Publishes the status; while running at most once per [`PROGRESS_INTERVAL`].
    fn report(&mut self, state: PrefetchState, force: bool) {
        self.status.state = state;
        let due = self
            .last_report
            .is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
        let changed = self.shared.status() != self.status;
        self.shared.set_status(self.status);
        if changed && (force || due) {
            self.last_report = Some(Instant::now());
            self.reading.emit(CoreEvent::PrefetchProgress {
                account_id: self.account,
                status: self.status,
            });
        }
    }

    async fn execute(&mut self) -> Result<(), CoreError> {
        let store = self.reading.store.clone();
        let Some(account) = store.account(self.account).await? else {
            return Ok(());
        };
        let settings = store.prefetch_settings().await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let since = settings.period.since(now);
        let mut before: Option<MessageId> = None;
        let mut started = false;
        loop {
            if self.shared.interrupted() {
                return Ok(());
            }
            let candidates = store
                .prefetch_candidates(self.account, since, before, MAX_PREFETCH_BATCH)
                .await?;
            let Some(oldest) = candidates.iter().map(|c| c.message_id).min() else {
                return Ok(());
            };
            before = Some(oldest);
            let failed = self
                .shared
                .failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let candidates: Vec<PrefetchCandidate> = candidates
                .into_iter()
                .filter(|c| !failed.contains(&c.message_id))
                .collect();
            self.status.total = self
                .status
                .done
                .saturating_add(u32::try_from(candidates.len()).unwrap_or(u32::MAX));
            if candidates.is_empty() {
                continue;
            }
            if !started {
                started = true;
                tracing::info!(account = %self.account, candidates = candidates.len(), "prefetch started");
                self.report(PrefetchState::Running, true);
            }
            self.mailboxes(&account, candidates, settings.max_message_size)
                .await?;
        }
    }

    /// Fetches the candidates mailbox by mailbox.
    async fn mailboxes(
        &mut self,
        account: &StoredAccount,
        candidates: Vec<PrefetchCandidate>,
        max_size: u32,
    ) -> Result<(), CoreError> {
        let store = self.reading.store.clone();
        let states: HashMap<MailboxId, mail_store::MailboxSyncState> = store
            .mailbox_sync_states(self.account)
            .await?
            .into_iter()
            .map(|s| (s.mailbox_id, s))
            .collect();
        let mut by_mailbox: Vec<(MailboxId, Vec<PrefetchCandidate>)> = Vec::new();
        for candidate in candidates {
            match by_mailbox
                .iter_mut()
                .find(|(m, _)| *m == candidate.mailbox_id)
            {
                Some((_, list)) => list.push(candidate),
                None => by_mailbox.push((candidate.mailbox_id, vec![candidate])),
            }
        }
        for (mailbox, list) in by_mailbox {
            let Some(state) = states.get(&mailbox) else {
                continue;
            };
            let location = |c: &PrefetchCandidate| mail_store::MessageLocation {
                mailbox_id: mailbox,
                mailbox_name: state.name.clone(),
                role: state.role,
                selectable: state.selectable,
                uid_validity: state.uid_validity,
                uid: c.uid,
            };
            let (whole, large): (Vec<_>, Vec<_>) =
                list.into_iter().partition(|c| c.size <= max_size);
            for batch in batches(whole) {
                if self.shared.interrupted() {
                    return Ok(());
                }
                let Some(first) = batch.first() else { continue };
                self.whole_batch(account, &location(first), &batch).await?;
            }
            for candidate in large {
                if self.shared.interrupted() {
                    return Ok(());
                }
                self.large(account, &location(&candidate), candidate, max_size)
                    .await?;
            }
        }
        Ok(())
    }

    async fn whole_batch(
        &mut self,
        account: &StoredAccount,
        location: &mail_store::MessageLocation,
        batch: &[PrefetchCandidate],
    ) -> Result<(), CoreError> {
        let uids = UidSet::from_uids(batch.iter().map(|c| c.uid));
        let fetched = {
            let mut held = self.reading.connections.hold(account).await?;
            let Some(selected) = held.open(location).await? else {
                self.fail(batch.iter().map(|c| c.message_id));
                return Ok(());
            };
            match held.messages(&selected, &uids).await {
                Ok(raws) => raws,
                Err(err @ mail_model::BackendError::Rejected(_)) => {
                    tracing::warn!(account = %self.account, error = %err, "prefetch batch refused; skipped");
                    self.fail(batch.iter().map(|c| c.message_id));
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            }
        };
        let by_uid: HashMap<mail_model::Uid, MessageId> =
            batch.iter().map(|c| (c.uid, c.message_id)).collect();
        let mut bodies = Vec::with_capacity(fetched.len());
        let mut bytes = 0_u64;
        for raw in fetched {
            if let Some(&message) = by_uid.get(&raw.uid) {
                bytes += raw.data.len() as u64;
                bodies.push((message, raw.data));
            }
        }
        let got: HashSet<MessageId> = bodies.iter().map(|(m, _)| *m).collect();
        self.fail(
            batch
                .iter()
                .map(|c| c.message_id)
                .filter(|m| !got.contains(m)),
        );
        let previews = self.reading.store_whole_bodies(bodies).await?;
        self.stored(location.mailbox_id, previews, bytes);
        Ok(())
    }

    async fn large(
        &mut self,
        account: &StoredAccount,
        location: &mail_store::MessageLocation,
        candidate: PrefetchCandidate,
        max_size: u32,
    ) -> Result<(), CoreError> {
        let fetched = {
            let mut held = self.reading.connections.hold(account).await?;
            let Some(selected) = held.open(location).await? else {
                self.fail([candidate.message_id]);
                return Ok(());
            };
            let Some(structure) = held.structure(&selected, candidate.uid).await? else {
                self.fail([candidate.message_id]);
                return Ok(());
            };
            match structure.fetch_plan(max_size) {
                BodyFetchPlan::Whole => held
                    .messages(&selected, &UidSet::from_uids([candidate.uid]))
                    .await?
                    .into_iter()
                    .find(|r| r.uid == candidate.uid)
                    .map(|r| super::fetch::Fetched::Whole(r.data)),
                BodyFetchPlan::Sections(sections) => held
                    .sections(&selected, candidate.uid, &sections)
                    .await?
                    .map(|s| super::fetch::Fetched::Sections(structure, s)),
            }
        };
        let Some(fetched) = fetched else {
            self.fail([candidate.message_id]);
            return Ok(());
        };
        let bytes = match &fetched {
            super::fetch::Fetched::Whole(data) => data.len() as u64,
            super::fetch::Fetched::Sections(_, sections) => {
                sections.iter().map(|s| s.data.len() as u64).sum()
            }
            super::fetch::Fetched::NotOnServer => 0,
        };
        match self
            .reading
            .store_fetched(candidate.message_id, fetched)
            .await
        {
            Ok(previews) => self.stored(location.mailbox_id, previews, bytes),
            Err(err) => {
                tracing::warn!(message = candidate.message_id.0, error = %err, "prefetch of a large message failed; skipped");
                self.fail([candidate.message_id]);
            }
        }
        Ok(())
    }

    fn fail(&self, messages: impl IntoIterator<Item = MessageId>) {
        let mut failed = self
            .shared
            .failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        failed.extend(messages);
    }

    fn stored(&mut self, mailbox: MailboxId, stored: usize, bytes: u64) {
        self.status.done = self
            .status
            .done
            .saturating_add(u32::try_from(stored).unwrap_or(u32::MAX));
        self.status.total = self.status.total.max(self.status.done);
        self.status.bytes = self.status.bytes.saturating_add(bytes);
        if stored > 0 {
            emit(
                &self.reading.events,
                CoreEvent::MessagesChanged {
                    account_id: self.account,
                    mailbox_id: mailbox,
                },
            );
        }
        self.report(PrefetchState::Running, false);
    }
}

/// Whole messages in FETCH batches by size and count; one larger than the byte limit alone.
fn batches(candidates: Vec<PrefetchCandidate>) -> Vec<Vec<PrefetchCandidate>> {
    let mut out: Vec<Vec<PrefetchCandidate>> = Vec::new();
    let mut bytes = 0_u64;
    for candidate in candidates {
        let size = u64::from(candidate.size);
        let fits = out
            .last()
            .is_some_and(|b| b.len() < BATCH_MESSAGES && bytes + size <= BATCH_BYTES);
        if fits {
            bytes += size;
            if let Some(last) = out.last_mut() {
                last.push(candidate);
            }
        } else {
            bytes = size;
            out.push(vec![candidate]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use mail_model::Uid;

    use super::*;

    fn candidate(id: i64, size: u32) -> PrefetchCandidate {
        PrefetchCandidate {
            message_id: MessageId(id),
            mailbox_id: MailboxId(1),
            uid: Uid::try_from(id).expect("uid"),
            size,
        }
    }

    #[test]
    fn batches_respect_bytes_and_count() {
        let sizes = [100_000_u32; 120];
        let list: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(i, s)| candidate(i64::try_from(i).expect("i") + 1, *s))
            .collect();
        let out = batches(list);
        assert!(out.iter().all(|b| b.len() <= BATCH_MESSAGES));
        assert!(
            out.iter()
                .all(|b| b.iter().map(|c| u64::from(c.size)).sum::<u64>() <= BATCH_BYTES)
        );
        assert_eq!(out.iter().map(Vec::len).sum::<usize>(), 120);

        let big = batches(vec![
            candidate(1, 10),
            candidate(2, 3 * 1024 * 1024),
            candidate(3, 10),
        ]);
        assert_eq!(big.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 1]);
    }
}
