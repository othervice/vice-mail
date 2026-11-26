//! Stage 22, item 6 (decision 0077): the learning spam filter, as methods of [`AppCore`].
//!
//! **Learning.** The user's "Spam" and "Not spam" ([`AppCore::apply_action`] with
//! [`MailAction::Spam`] / [`MailAction::NotSpam`]) teach the filter the messages as spam or ham
//! (from the stored body, else the stored header data; no network), recorded with the action's
//! outbox batch: "Undo" ([`AppCore::undo_action`]) unlearns them. A message learned again in
//! the other class is unlearned first. The filter's own moves and the moves for blocked senders
//! teach nothing. On request ([`AppCore::learn_spam_from_mail`]) it learns from existing mail in
//! the background: the newest of the junk folders as spam, read inbox mail (replied first) as
//! ham, at most [`MAX_LEARN_SPAM`] and [`MAX_LEARN_HAM`], with progress and cancel. Because more
//! learned spam than ham makes it suspect good mail (measured on a corpus, decision 0077), it
//! tops up ham from read inbox mail when the user taught more than half as much spam.
//!
//! **Looking at new mail** ([`AppCore::handle_spam_arrivals`]): new mail in an inbox (never the
//! first sync), after blocked senders and before the local rules and notifications, while the
//! app runs, for accounts the setting covers and once the filter learned
//! [`MIN_LEARNED_SPAM`] and [`MIN_LEARNED_HAM`] mails. Never spam: the user's own addresses,
//! contacts the user wrote to (or named, noted, merged, grouped), replies to the user's mail,
//! senders whose images the user allowed, senders marked important. The body is fetched for it
//! as for opening when not stored (read-only); if it cannot be read, the stored header data
//! decides (fewer tokens: a score nearer 0.5). From [`SPAM_THRESHOLD`] on the messages go to
//! the junk folder through the outbox in one batch, where the account may write and has one;
//! otherwise they are marked as suspected spam on this device only. Either way they run no
//! rules and announce nothing; `spamFiltered` tells the UI (with the batch for "Undo", which
//! also learns them as ham).
//!
//! Everything learned stays on this device (`spam_*` tables, class Device); only the switch is
//! synchronised (setting `spam.filter`).

mod learn;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use mail_content::{SpamCounts, SpamFields, SpamScore, SpamTokens, SpamTotals};
use mail_model::{
    ActionResult, BatchId, CoreError, CoreEvent, MAX_LEARN_HAM, MAX_LEARN_SPAM,
    MAX_SPAM_EXCLUDED_ACCOUNTS, MAX_SPAM_FEEDBACK_MESSAGES, MIN_LEARNED_HAM, MIN_LEARNED_SPAM,
    MailAction, MessageId, MessageRef, SPAM_THRESHOLD, SpamFilterMark, SpamFilterSettings,
    SpamFilterStatus, SpamVerdict,
};
use mail_store::{ArrivedMessage, GLOBAL_SCOPE, SpamClass};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::AppCore;
use crate::core::{Connector, WeakCore};
use crate::reading::SpamFetch;

/// Setting key (global, synchronised, JSON [`SpamFilterSettings`]).
pub(crate) const SETTING_KEY: &str = "spam.filter";

/// Learning from read inbox mail skips mails the filter scores at least this high: they might
/// be spam the user read and left.
const HAM_DOUBT: f64 = 0.9;
/// The filter keeps at least this many learned ham mails per learned spam mail (decision 0077).
const HAM_PER_SPAM: u32 = 2;
/// At most this many ham mails per top-up.
const MAX_TOP_UP: u32 = 200;

/// Why a new message is never spam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exempt {
    OwnAddress,
    Contact,
    Important,
    ReplyToOwn,
    ImagesAllowed,
}

/// Work of the filter's worker, in order (an undo never overtakes the learning it undoes).
enum Job {
    /// Learn messages as `class`; `batch`: the user's outbox action; `clear`: remove the
    /// filter's verdicts on them (the user decided through the server).
    Learn {
        messages: Vec<MessageId>,
        class: SpamClass,
        batch: Option<BatchId>,
        clear: bool,
    },
    /// An outbox batch was undone.
    Undone { batch: BatchId },
    /// Answered once every job before it is done.
    Flush(oneshot::Sender<()>),
}

/// The filter's state in the core.
pub(crate) struct SpamState {
    jobs: mpsc::UnboundedSender<Job>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<Job>>>,
    pub(crate) learning: Mutex<Option<Arc<learn::Run>>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Learned spam and ham the filter needs before it acts (lower only in tests).
    minimums: Mutex<(u32, u32)>,
}

impl Default for SpamState {
    fn default() -> Self {
        let (jobs, receiver) = mpsc::unbounded_channel();
        Self {
            jobs,
            receiver: Mutex::new(Some(receiver)),
            learning: Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
            minimums: Mutex::new((MIN_LEARNED_SPAM, MIN_LEARNED_HAM)),
        }
    }
}

impl SpamState {
    fn send(&self, job: Job) {
        // Fails only while the core shuts down.
        let _ = self.jobs.send(job);
    }

    fn minimums(&self) -> (u32, u32) {
        *lock(&self.minimums)
    }

    /// Tests teach a handful of mails, not hundreds.
    #[cfg(test)]
    pub(crate) fn set_minimums(&self, spam: u32, ham: u32) {
        *lock(&self.minimums) = (spam, ham);
    }

    pub(crate) fn stop(&self) {
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
        if let Some(run) = lock(&self.learning).as_ref() {
            run.cancel();
        }
    }
}

impl Drop for SpamState {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts the filter's worker (it holds the core only weakly).
pub(crate) fn start<C: Connector>(core: &AppCore<C>) {
    let state = core.spam();
    if let Some(receiver) = lock(&state.receiver).take() {
        let task = tokio::spawn(worker(core.downgrade(), receiver));
        lock(&state.tasks).push(task);
    }
}

async fn worker<C: Connector>(weak: WeakCore<C>, mut receiver: mpsc::UnboundedReceiver<Job>) {
    while let Some(job) = receiver.recv().await {
        let Some(core) = weak.upgrade() else { return };
        let result = match job {
            Job::Learn {
                messages,
                class,
                batch,
                clear,
            } => core.learn_now(messages, class, batch, clear).await,
            Job::Undone { batch } => core.batch_undone(batch).await,
            Job::Flush(done) => {
                let _ = done.send(());
                Ok(())
            }
        };
        if let Err(err) = result {
            tracing::error!(code = ?err.code(), error = %err, "spam filter: learning failed");
        }
    }
}

fn class_of(action: &MailAction) -> Option<SpamClass> {
    match action {
        MailAction::Spam => Some(SpamClass::Spam),
        MailAction::NotSpam => Some(SpamClass::Ham),
        _ => None,
    }
}

fn totals(spam: u32, ham: u32) -> SpamTotals {
    SpamTotals { spam, ham }
}

impl<C: Connector> AppCore<C> {
    // -----------------------------------------------------------------------------------------
    // Settings and status

    /// The setting as stored; the default (off) when missing or unreadable.
    pub(crate) async fn spam_settings(&self) -> Result<SpamFilterSettings, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(settings
            .into_iter()
            .find(|s| s.key == SETTING_KEY)
            .and_then(|s| match serde_json::from_str(&s.value) {
                Ok(value) => Some(value),
                Err(err) => {
                    tracing::warn!(%err, "spam filter setting unreadable; off");
                    None
                }
            })
            .unwrap_or_default())
    }

    /// What the filter learned and does (Settings).
    pub async fn get_spam_filter(&self) -> Result<SpamFilterStatus, CoreError> {
        let settings = self.spam_settings().await?;
        let size = self.store_ref()?.spam_model_size().await?;
        let learning = lock(&self.spam().learning)
            .as_ref()
            .map(|run| run.snapshot());
        let (min_spam, min_ham) = self.spam().minimums();
        Ok(SpamFilterStatus {
            settings,
            learned_spam: size.spam,
            learned_ham: size.ham,
            tokens: size.tokens,
            ready: size.spam >= min_spam && size.ham >= min_ham,
            min_spam,
            min_ham,
            learning,
        })
    }

    pub(crate) async fn spam_status_changed(&self) {
        match self.get_spam_filter().await {
            Ok(status) => self.emit_event(CoreEvent::SpamFilterChanged { status }),
            Err(err) => tracing::warn!(code = ?err.code(), "spam filter status unreadable"),
        }
    }

    /// Stores the setting (synchronised to other devices) and returns the status. Turning the
    /// filter off stops a running learning; what was learned stays. `invalid_argument` for more
    /// than [`MAX_SPAM_EXCLUDED_ACCOUNTS`] excluded accounts.
    pub async fn set_spam_filter(
        &self,
        mut settings: SpamFilterSettings,
    ) -> Result<SpamFilterStatus, CoreError> {
        let mut seen = HashSet::new();
        settings.excluded_accounts.retain(|a| seen.insert(*a));
        if settings.excluded_accounts.len() > MAX_SPAM_EXCLUDED_ACCOUNTS {
            return Err(CoreError::InvalidArgument(format!(
                "at most {MAX_SPAM_EXCLUDED_ACCOUNTS} excluded accounts"
            )));
        }
        let value = serde_json::to_string(&settings)
            .map_err(|err| CoreError::Internal(format!("spam filter setting: {err}")))?;
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(SETTING_KEY.to_owned(), value)])
            .await?;
        tracing::info!(
            enabled = settings.enabled,
            excluded = settings.excluded_accounts.len(),
            "spam filter setting stored"
        );
        if !settings.enabled {
            self.cancel_spam_learning();
        }
        let status = self.get_spam_filter().await?;
        self.emit_event(CoreEvent::SpamFilterChanged {
            status: status.clone(),
        });
        Ok(status)
    }

    /// Forgets everything the filter learned and every mark it set (mail it moved stays in the
    /// junk folder). Stops a running learning first.
    pub async fn forget_spam_filter(&self) -> Result<SpamFilterStatus, CoreError> {
        self.cancel_spam_learning();
        self.spam_flush().await;
        self.store_ref()?.reset_spam_filter().await?;
        tracing::info!("spam filter: everything learned forgotten");
        self.emit_event(CoreEvent::InboxChanged { account_id: None });
        let status = self.get_spam_filter().await?;
        self.emit_event(CoreEvent::SpamFilterChanged {
            status: status.clone(),
        });
        Ok(status)
    }

    /// The filter's verdict on a message (the reading pane's note), if it set one.
    pub async fn get_spam_verdict(
        &self,
        message: MessageId,
    ) -> Result<Option<SpamVerdict>, CoreError> {
        Ok(self.store_ref()?.spam_verdict(message).await?)
    }

    // -----------------------------------------------------------------------------------------
    // Learning from the user's actions

    /// Called by [`AppCore::apply_action`] after the outbox recorded a user's action: "Spam" and
    /// "Not spam" teach the filter (in the background, in order).
    pub(crate) fn spam_action_applied(
        &self,
        action: &MailAction,
        messages: &[MessageRef],
        result: &ActionResult,
    ) {
        let Some(class) = class_of(action) else {
            return;
        };
        let mut ids: Vec<MessageId> = messages.iter().map(|m| m.message_id).collect();
        ids.sort_unstable();
        ids.dedup();
        self.spam().send(Job::Learn {
            messages: ids,
            class,
            batch: result.batch,
            clear: true,
        });
    }

    /// Called by [`AppCore::undo_action`] after the outbox undid a batch.
    pub(crate) fn spam_batch_undone(&self, batch: BatchId) {
        self.spam().send(Job::Undone { batch });
    }

    /// Waits until the worker did every job sent before.
    pub(crate) async fn spam_flush(&self) {
        let (done, wait) = oneshot::channel();
        self.spam().send(Job::Flush(done));
        let _ = wait.await;
    }

    /// "Spam" / "Not spam" without the server, for mail the filter marked on an account that
    /// may not write or has no junk folder: `spam` marks the messages as suspected and learns
    /// them as spam; otherwise the mark goes and they are learned as ham. Returns how many
    /// marks changed. `invalid_argument` for no or more than [`MAX_SPAM_FEEDBACK_MESSAGES`]
    /// messages.
    pub async fn mark_spam_locally(
        &self,
        messages: Vec<MessageId>,
        spam: bool,
    ) -> Result<u32, CoreError> {
        if messages.is_empty() || messages.len() > MAX_SPAM_FEEDBACK_MESSAGES {
            return Err(CoreError::InvalidArgument(format!(
                "1 to {MAX_SPAM_FEEDBACK_MESSAGES} messages"
            )));
        }
        let store = self.store_ref()?;
        let mut accounts = HashSet::new();
        let mut changed = 0u32;
        let mut known = Vec::with_capacity(messages.len());
        for message in messages {
            let summary = store
                .message_summary(message)
                .await?
                .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
            accounts.insert(summary.account_id);
            known.push(message);
        }
        if spam {
            let verdicts = known.iter().map(|m| (*m, 1.0)).collect();
            store
                .set_spam_verdicts(verdicts, SpamFilterMark::Suspected, None, self.now_ms())
                .await?;
            changed = u32::try_from(known.len()).unwrap_or(u32::MAX);
        } else {
            for mark in store.clear_spam_verdicts(known.clone()).await? {
                if mark.is_some() {
                    changed += 1;
                }
            }
        }
        self.spam().send(Job::Learn {
            messages: known,
            class: if spam {
                SpamClass::Spam
            } else {
                SpamClass::Ham
            },
            batch: None,
            clear: false,
        });
        for account in accounts {
            self.emit_event(CoreEvent::InboxChanged {
                account_id: Some(account),
            });
        }
        Ok(changed)
    }

    /// The worker's part of a learn job: tokens from what is stored (no network: the body when
    /// stored, else the header data), learned in one transaction; with `clear` the filter's
    /// verdicts on these messages go (the user decided).
    async fn learn_now(
        &self,
        messages: Vec<MessageId>,
        class: SpamClass,
        batch: Option<BatchId>,
        clear: bool,
    ) -> Result<(), CoreError> {
        let settings = self.spam_settings().await?;
        let store = self.store_ref()?;
        if clear {
            store.clear_spam_verdicts(messages.clone()).await?;
        }
        if !settings.enabled {
            return Ok(());
        }
        let mut items = Vec::with_capacity(messages.len());
        for message in messages {
            let Some(summary) = store.message_summary(message).await? else {
                continue;
            };
            if !settings.applies_to(summary.account_id) {
                continue;
            }
            if let Some(tokens) = self.spam_tokens_of(message, SpamFetch::No).await? {
                items.push((message, tokens.hashes().to_vec()));
            }
        }
        if items.is_empty() {
            return Ok(());
        }
        let count = items.len();
        store.learn_spam(items, class, batch, self.now_ms()).await?;
        tracing::info!(count, ?class, "spam filter learned from the user");
        self.spam_status_changed().await;
        if class == SpamClass::Spam {
            self.top_up_ham().await;
        }
        Ok(())
    }

    /// The worker's part of an undone batch: what the user's action taught is unlearned; mail
    /// the filter moved in it is learned as ham ("that was not spam").
    async fn batch_undone(&self, batch: BatchId) -> Result<(), CoreError> {
        let store = self.store_ref()?;
        let unlearned = store.unlearn_spam_batch(batch).await?;
        let moved = store.spam_moved_in_batch(batch).await?;
        if !unlearned.is_empty() {
            tracing::info!(
                count = unlearned.len(),
                "spam filter: undone action unlearned"
            );
        }
        if !moved.is_empty() {
            tracing::info!(count = moved.len(), "spam filter: its move was undone");
            self.learn_now(moved, SpamClass::Ham, None, true).await?;
        } else if !unlearned.is_empty() {
            self.spam_status_changed().await;
        }
        Ok(())
    }

    /// Learns more ham from read inbox mail while the filter knows fewer than
    /// [`HAM_PER_SPAM`] ham mails per spam mail.
    async fn top_up_ham(&self) {
        let Ok(store) = self.store_ref() else { return };
        let Ok(size) = store.spam_model_size().await else {
            return;
        };
        let wanted = size.spam.saturating_mul(HAM_PER_SPAM);
        if size.ham >= wanted {
            return;
        }
        let missing = (wanted - size.ham).min(MAX_TOP_UP);
        if let Err(err) = self.start_spam_learning(0, missing).await {
            tracing::debug!(code = ?err.code(), "spam filter: no top-up");
        }
    }

    // -----------------------------------------------------------------------------------------
    // Learning from existing mail

    /// Starts learning from existing mail in the background (the junk folders as spam, read
    /// inbox mail as ham) and returns the status; already running: the status. Progress comes as
    /// `spamFilterChanged`. `invalid_argument` while the filter is off.
    pub async fn learn_spam_from_mail(&self) -> Result<SpamFilterStatus, CoreError> {
        self.start_spam_learning(MAX_LEARN_SPAM, MAX_LEARN_HAM)
            .await?;
        self.get_spam_filter().await
    }

    /// Stops learning from existing mail; what was learned so far stays.
    pub fn cancel_spam_learning(&self) {
        if let Some(run) = lock(&self.spam().learning).as_ref() {
            run.cancel();
        }
    }

    // -----------------------------------------------------------------------------------------
    // New mail

    /// The tokens of a message: from its body (stored, or fetched as `fetch` allows), else from
    /// the stored header data; `None` when the message is gone.
    pub(crate) async fn spam_tokens_of(
        &self,
        message: MessageId,
        fetch: SpamFetch,
    ) -> Result<Option<SpamTokens>, CoreError> {
        match self.reading_ref()?.spam_tokens(message, fetch).await {
            Ok(Some(tokens)) => return Ok(Some(tokens)),
            Ok(None) => {}
            Err(CoreError::NotFound(_)) => return Ok(None),
            Err(err) => {
                tracing::debug!(code = ?err.code(), "spam filter: body unreadable; header data");
            }
        }
        let Some(fields) = self.store_ref()?.spam_fields(message).await? else {
            return Ok(None);
        };
        Ok(Some(mail_content::spam_tokens_of_fields(&SpamFields {
            from: fields.from.as_ref(),
            reply_to: &fields.reply_to,
            recipients: fields.recipients,
            subject: &fields.subject,
            list_id: fields.list_id.as_deref(),
            preview: fields.preview.as_deref(),
        })))
    }

    /// The score of tokens under what the filter learned; `None` before it learned both classes.
    pub(crate) async fn spam_score_of(
        &self,
        tokens: &SpamTokens,
    ) -> Result<Option<SpamScore>, CoreError> {
        let lookup = self
            .store_ref()?
            .spam_counts(tokens.hashes().to_vec())
            .await?;
        Ok(mail_content::spam_score(
            totals(lookup.totals.0, lookup.totals.1),
            lookup.tokens.iter().map(|(spam, ham)| SpamCounts {
                spam: *spam,
                ham: *ham,
            }),
        ))
    }

    /// Why a message is never spam, if it is not to be judged.
    pub(crate) async fn spam_exempt(
        &self,
        message: MessageId,
        own: &[String],
    ) -> Result<Option<Exempt>, CoreError> {
        let store = self.store_ref()?;
        let Some(facts) = store.spam_sender_facts(message).await? else {
            return Ok(None);
        };
        let is_own = |address: &str| crate::comfort::classifier::is_own(address, own);
        if facts.from.as_deref().is_some_and(is_own) {
            return Ok(Some(Exempt::OwnAddress));
        }
        if facts.known_contact {
            return Ok(Some(Exempt::Contact));
        }
        if facts.important {
            return Ok(Some(Exempt::Important));
        }
        if facts.referenced_senders.iter().any(|s| is_own(s)) {
            return Ok(Some(Exempt::ReplyToOwn));
        }
        if let (Some(account), Some(from)) = (facts.account, facts.from.as_deref())
            && store.remote_content_allowed(account, from).await?
        {
            return Ok(Some(Exempt::ImagesAllowed));
        }
        Ok(None)
    }

    /// New mail in an inbox (after blocked senders): the messages the filter takes for spam
    /// go to the junk folder or are marked (see the module docs). Returns their ids; the caller
    /// runs neither rules nor notifications for them.
    pub(crate) async fn handle_spam_arrivals(
        &self,
        account: &mail_store::StoredAccount,
        messages: &[ArrivedMessage],
    ) -> Result<HashSet<MessageId>, CoreError> {
        let settings = self.spam_settings().await?;
        if messages.is_empty() || !settings.applies_to(account.id) {
            return Ok(HashSet::new());
        }
        let store = self.store_ref()?;
        let size = store.spam_model_size().await?;
        let (min_spam, min_ham) = self.spam().minimums();
        if size.spam < min_spam || size.ham < min_ham {
            tracing::debug!(
                spam = size.spam,
                ham = size.ham,
                "spam filter: not learned enough yet"
            );
            return Ok(HashSet::new());
        }
        let own = store.own_addresses().await?;
        let started = Instant::now();
        let mut spam: Vec<(MessageId, f64, mail_model::MailboxId)> = Vec::new();
        let mut looked = 0usize;
        for message in messages {
            if let Some(reason) = self.spam_exempt(message.id, &own).await? {
                tracing::debug!(message = message.id.0, ?reason, "spam filter: never spam");
                continue;
            }
            let Some(tokens) = self.spam_tokens_of(message.id, SpamFetch::Notify).await? else {
                continue;
            };
            looked += 1;
            let Some(score) = self.spam_score_of(&tokens).await? else {
                continue;
            };
            if score.score >= SPAM_THRESHOLD {
                spam.push((message.id, score.score, message.mailbox_id));
            }
        }
        tracing::debug!(
            looked,
            spam = spam.len(),
            ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "spam filter: new mail looked at"
        );
        if spam.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: HashSet<MessageId> = spam.iter().map(|(id, _, _)| *id).collect();
        let verdicts: Vec<(MessageId, f64)> = spam.iter().map(|(id, s, _)| (*id, *s)).collect();
        // Moved: the move's batch (`None` inside when the outbox had nothing to do).
        let mut moved_in: Option<Option<BatchId>> = None;
        if account.writes_enabled {
            let refs: Vec<MessageRef> = spam
                .iter()
                .map(|(id, _, mailbox)| MessageRef {
                    mailbox_id: *mailbox,
                    message_id: *id,
                })
                .collect();
            let first = spam.first().map_or(0, |(id, _, _)| id.0);
            let request = format!("spam-filter-{}-{first}", account.id.as_uuid().simple());
            match self
                .apply_action_quietly(MailAction::Spam, refs, request)
                .await
            {
                Ok(result) => moved_in = Some(result.batch),
                Err(err) => {
                    tracing::info!(code = ?err.code(), "spam filter: not moved; marked instead");
                }
            }
        }
        let now = self.now_ms();
        let (moved, marked) = match moved_in {
            Some(batch) => {
                store
                    .set_spam_verdicts(verdicts, SpamFilterMark::Moved, batch, now)
                    .await?;
                (ids.len(), 0)
            }
            None => {
                store
                    .set_spam_verdicts(verdicts, SpamFilterMark::Suspected, None, now)
                    .await?;
                (0, ids.len())
            }
        };
        tracing::info!(moved, marked, "spam filter: new mail taken for spam");
        self.emit_event(CoreEvent::SpamFiltered {
            account_id: account.id,
            moved: u32::try_from(moved).unwrap_or(u32::MAX),
            marked: u32::try_from(marked).unwrap_or(u32::MAX),
            batch: moved_in.flatten(),
        });
        self.emit_event(CoreEvent::InboxChanged {
            account_id: Some(account.id),
        });
        Ok(ids)
    }
}
