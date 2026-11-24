//! The outbox (stage 4, decision 0012): every writing action changes the local state at once,
//! is recorded as operations in the store (same transaction) and executed on the server in
//! order, per account on its own writer connection.
//!
//! - **Recording** ([`Outbox::apply`]): the store plans the action ([`mail_model::plan_action`]),
//!   applies the local effect, cancels queued opposites and records the operations. An account
//!   without `writes_enabled` refuses with [`mail_model::ActionError::ReadOnly`] before anything is
//!   recorded; so do missing archive, trash or junk mailboxes ([`mail_model::ActionError::MissingMailbox`],
//!   the UI asks which mailbox to use or creates one with [`Outbox::create_mailbox`]) and a
//!   delete in the trash without [`MailAction::DeletePermanently`].
//! - **Executing** (`worker`, `exec`): one worker per account with its own connection,
//!   independent of the sync and IDLE. Operations run in recorded order; consecutive ones of
//!   one batch, mailbox and change go out as one command. After each step the
//!   [`SyncHook`] asks for a sync of the mailboxes that changed on the server.
//! - **Crash safety:** before a command is sent its operations are marked `in_flight`. After a
//!   restart ([`Outbox::start`]) or a lost connection they are checked against the server and
//!   repeated only if they demonstrably did not happen (`exec` module docs).
//! - **Failures and conflicts:** a refusal reverts the local change (`failed`, the user can
//!   [`Outbox::retry`] or [`Outbox::discard`]); if the message was moved, deleted or modified
//!   elsewhere meanwhile, the server wins (`conflict`).
//! - **Undo** ([`Outbox::undo`]) cancels what is still queued and sends counter-operations for
//!   the rest.
//!
//! Events: [`OutboxEvent`] (one per operation) on [`Outbox::subscribe`]. For the UI,
//! [`Outbox::forward_to`] sends each change of the outbox straight into the core's stream as
//! one `CoreEvent::OperationsChanged` (all operations of the change), then one
//! `messagesChanged` per mailbox and one `mailboxesChanged`: an action on thousands of messages
//! is a handful of events, never a flood that could overrun a subscriber.

mod exec;
mod worker;

#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_server_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use crate::protocols::{AnyConnector, AnySender};
use mail_model::{
    AccountId, ActionResult, BatchId, CoreError, CoreEvent, ExtraRootCa, MAX_ACTION_MESSAGES,
    MailAction, MailSender, MailboxId, MailboxRole, MessageRef, OperationId, OperationInfo,
    OperationKind, OperationState, OutboxCounts, OutboxEvent, SendState, Timeouts, WriteConnector,
};
use mail_store::{OutboxChange, Store};
use tokio::sync::{Notify, broadcast, watch};
use tokio::task::JoinHandle;

use crate::clock::{Clock, SystemClock};
use crate::credentials::Credentials;
use crate::sync::SyncConfig;
use crate::sync_state::BackoffPolicy;

/// Events buffered per subscriber.
const EVENT_CAPACITY: usize = 1024;

/// Tunables of the outbox. The defaults are the product values; tests shorten them.
#[derive(Debug, Clone)]
pub struct OutboxConfig {
    /// Retry delay while the server cannot be reached (or a step failed).
    pub backoff: BackoffPolicy,
    pub timeouts: Timeouts,
    /// Additional trusted root (tests only), as in [`SyncConfig::extra_root_ca`].
    pub extra_root_ca: Option<ExtraRootCa>,
    /// How long the writer connection stays open after the queue ran empty.
    pub linger: Duration,
    /// Times one operation is sent before it is given up as `failed`.
    pub max_attempts: u32,
    /// Confirmed operations are kept this long (activity view, undo), then purged.
    pub keep_confirmed: Duration,
    /// Stage 5: how messages are submitted (the SMTP adapter); tests hand in an instrumented
    /// one.
    pub sender: SenderHandle,
    /// Stage 5: tunables of drafts and sending.
    pub compose: ComposeConfig,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            backoff: BackoffPolicy::default(),
            timeouts: Timeouts::default(),
            extra_root_ca: None,
            linger: Duration::from_secs(60),
            max_attempts: 5,
            keep_confirmed: Duration::from_secs(24 * 60 * 60),
            sender: SenderHandle::default(),
            compose: ComposeConfig::default(),
        }
    }
}

/// The [`MailSender`] the outbox submits with. The product's is [`AnySender`] (SMTP, or JMAP
/// submission for JMAP accounts, decision 0041).
#[derive(Clone)]
pub struct SenderHandle(pub Arc<dyn MailSender>);

impl Default for SenderHandle {
    fn default() -> Self {
        Self(Arc::new(AnySender::default()))
    }
}

impl std::fmt::Debug for SenderHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SenderHandle")
    }
}

/// Tunables of drafts and sending (stage 5). The defaults are the product values.
#[derive(Debug, Clone)]
pub struct ComposeConfig {
    /// A draft goes to the server's Drafts folder after this long without a save (and at
    /// once when the composer closes).
    pub draft_save_delay: Duration,
    /// After an unclear send on a server that files sent mail itself: waits before each look
    /// into Sent (the server may take a moment to file it). Then the user is asked.
    pub sent_checks: Vec<Duration>,
    /// The contact directory reads new messages this long after they arrived (batched).
    pub contacts_delay: Duration,
}

impl Default for ComposeConfig {
    fn default() -> Self {
        Self {
            draft_save_delay: Duration::from_secs(30),
            sent_checks: vec![
                Duration::from_secs(5),
                Duration::from_secs(20),
                Duration::from_secs(60),
            ],
            contacts_delay: Duration::from_secs(2),
        }
    }
}

impl OutboxConfig {
    /// Backoff, time limits and extra root of the sync, the outbox defaults for the rest.
    pub fn from_sync(sync: &SyncConfig) -> Self {
        Self {
            backoff: sync.backoff,
            timeouts: sync.timeouts,
            extra_root_ca: sync.extra_root_ca.clone(),
            ..Self::default()
        }
    }
}

/// Called after the outbox changed mailboxes on the server, so their sync runs soon (the
/// integration of stage 4 wires it to the account's sync task). Must not block.
pub trait SyncHook: Send + Sync + 'static {
    fn sync_requested(&self, account: AccountId, mailboxes: &[MailboxId]);
}

/// A [`SyncHook`] that does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSyncHook;

impl SyncHook for NoSyncHook {
    fn sync_requested(&self, _account: AccountId, _mailboxes: &[MailboxId]) {}
}

/// Where the outbox's events go: its own per-operation stream, and the core's stream if
/// [`Outbox::forward_to`] was called.
pub(crate) struct Emitter {
    pub(crate) events: broadcast::Sender<OutboxEvent>,
    core: RwLock<Option<broadcast::Sender<CoreEvent>>>,
    /// Stage 5: for removing the files of sent and discarded drafts.
    store: Store,
    /// Held while compose files are written before their operation is recorded, so the sweep
    /// never removes them in between.
    pub(crate) files_lock: Arc<tokio::sync::Mutex<()>>,
    /// Stage 16: wakes the worker of another account that a change recorded operations for
    /// (the source of a move between accounts).
    waker: RwLock<Option<AccountWaker>>,
}

type AccountWaker = Arc<dyn Fn(AccountId) + Send + Sync>;

impl Emitter {
    fn new(store: Store) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            events,
            core: RwLock::new(None),
            store,
            files_lock: Arc::new(tokio::sync::Mutex::new(())),
            waker: RwLock::new(None),
        }
    }

    /// Stage 5: events of drafts and sends, and the removal of files nothing needs any more.
    fn compose_change(&self, change: &OutboxChange, send: &dyn Fn(CoreEvent)) {
        if !change.drafts.is_empty() || !change.drafts_removed.is_empty() {
            send(CoreEvent::DraftsChanged {
                account_id: change.account,
                changed: change
                    .drafts
                    .iter()
                    .filter(|d| !change.drafts_removed.contains(d))
                    .cloned()
                    .collect(),
                removed: change.drafts_removed.clone(),
            });
        }
        let now = crate::clock::Clock::now_ms(&crate::clock::SystemClock);
        let mails: Vec<_> = change
            .operations
            .iter()
            .filter_map(|op| crate::compose::outgoing::outgoing_mail(op, now))
            .collect();
        if !mails.is_empty() {
            for mail in &mails {
                if mail.state == SendState::Unclear {
                    send(CoreEvent::SendNeedsDecision {
                        account_id: change.account,
                        mail: mail.clone(),
                    });
                }
            }
            send(CoreEvent::OutgoingChanged {
                account_id: change.account,
                mails,
            });
        }
        let compose_files_may_be_free = !change.drafts_removed.is_empty()
            || !change.removed.is_empty()
            || change.operations.iter().any(|op| {
                matches!(
                    op.payload.kind(),
                    OperationKind::Send | OperationKind::Append
                ) && matches!(op.state, OperationState::Confirmed | OperationState::Failed)
            });
        if compose_files_may_be_free && tokio::runtime::Handle::try_current().is_ok() {
            let store = self.store.clone();
            let lock = Arc::clone(&self.files_lock);
            tokio::spawn(async move {
                let _held = lock.lock().await;
                crate::compose::files::sweep(&store).await;
            });
        }
    }

    fn core(&self) -> Option<broadcast::Sender<CoreEvent>> {
        self.core
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sends the events of a store change: per operation on the outbox stream, coalesced on
    /// the core's.
    pub(crate) fn change(&self, change: &OutboxChange) {
        for other in &change.others {
            self.change(other);
            let waker = self
                .waker
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(wake) = waker
                && !other.operations.is_empty()
            {
                wake(other.account);
            }
        }
        emit_change(&self.events, change);
        let core = self.core();
        let send = |event| {
            if let Some(core) = &core
                && core.send(event).is_err()
            {
                tracing::trace!("core event dropped: no subscribers");
            }
        };
        self.compose_change(change, &send);
        if core.is_none() {
            return;
        }
        if !change.operations.is_empty() || !change.removed.is_empty() {
            send(CoreEvent::OperationsChanged {
                account_id: change.account,
                operations: change.operations.iter().map(|op| op.info()).collect(),
                removed: change.removed.clone(),
            });
        }
        for mailbox in &change.mailboxes {
            send(CoreEvent::MessagesChanged {
                account_id: change.account,
                mailbox_id: *mailbox,
            });
        }
        if !change.mailboxes.is_empty() {
            send(CoreEvent::MailboxesChanged {
                account_id: change.account,
            });
        }
    }

    /// The folder list of the account changed (a mailbox was created).
    pub(crate) fn mailboxes_changed(&self, account: AccountId) {
        let event = OutboxEvent::MailboxesChanged {
            account_id: account,
        };
        if let Some(core) = self.core()
            && let Some(core_event) = event.to_core_event()
            && core.send(core_event).is_err()
        {
            tracing::trace!("core event dropped: no subscribers");
        }
        if self.events.send(event).is_err() {
            tracing::trace!("outbox event dropped: no subscribers");
        }
    }
}

/// What the workers share.
pub(crate) struct Shared<C> {
    pub(crate) store: Store,
    pub(crate) connector: Arc<C>,
    pub(crate) credentials: Credentials,
    pub(crate) config: OutboxConfig,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) emitter: Emitter,
    hook: RwLock<Arc<dyn SyncHook>>,
}

impl<C> Shared<C> {
    pub(crate) fn hook(&self) -> Arc<dyn SyncHook> {
        Arc::clone(&self.hook.read().unwrap_or_else(PoisonError::into_inner))
    }
}

struct WorkerHandle {
    kick: Arc<Notify>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// The outbox. Cheap to clone. Generic over the writer connector, so tests can use any
/// [`WriteConnector`]; the product uses [`AnyConnector`].
pub struct Outbox<C: WriteConnector + 'static = AnyConnector> {
    shared: Arc<Shared<C>>,
    workers: Arc<Mutex<HashMap<AccountId, WorkerHandle>>>,
}

impl<C: WriteConnector + 'static> Clone for Outbox<C> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            workers: Arc::clone(&self.workers),
        }
    }
}

/// Sends the per-operation events of a store change on the outbox stream.
fn emit_change(events: &broadcast::Sender<OutboxEvent>, change: &OutboxChange) {
    let send = |event| {
        if events.send(event).is_err() {
            tracing::trace!("outbox event dropped: no subscribers");
        }
    };
    for id in &change.removed {
        send(OutboxEvent::OperationRemoved {
            account_id: change.account,
            id: *id,
        });
    }
    for op in &change.operations {
        send(OutboxEvent::OperationChanged {
            operation: op.info(),
        });
    }
    for mailbox in &change.mailboxes {
        send(OutboxEvent::MessagesChanged {
            account_id: change.account,
            mailbox_id: *mailbox,
        });
    }
    // Counters (unread, total) come with the folder list, as for the sync's events.
    if !change.mailboxes.is_empty() {
        send(OutboxEvent::MailboxesChanged {
            account_id: change.account,
        });
    }
}

fn result_of(change: &OutboxChange) -> ActionResult {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    ActionResult {
        batch: change.batch,
        queued: if change.repeated {
            0
        } else {
            count(change.operations.len())
        },
        cancelled: count(change.removed.len()),
    }
}

fn request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl<C: WriteConnector + 'static> Outbox<C> {
    /// An outbox over `store`. Nothing runs until [`Outbox::start`] or the first action.
    pub fn new(
        store: Store,
        connector: Arc<C>,
        credentials: Credentials,
        config: OutboxConfig,
    ) -> Self {
        Self::with_clock(store, connector, credentials, config, Arc::new(SystemClock))
    }

    /// Like [`Outbox::new`] with the core's clock: when delayed and scheduled operations are
    /// due (stage 10; tests move the clock).
    pub fn with_clock(
        store: Store,
        connector: Arc<C>,
        credentials: Credentials,
        config: OutboxConfig,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let store_for_emitter = store.clone();
        let outbox = Self {
            shared: Arc::new(Shared {
                store,
                connector,
                credentials,
                config,
                clock,
                emitter: Emitter::new(store_for_emitter),
                hook: RwLock::new(Arc::new(NoSyncHook)),
            }),
            workers: Arc::new(Mutex::new(HashMap::new())),
        };
        // Weak, so the waker never keeps the outbox alive.
        let shared = Arc::downgrade(&outbox.shared);
        let workers = Arc::downgrade(&outbox.workers);
        let waker: AccountWaker = Arc::new(move |account| {
            if let (Some(shared), Some(workers)) = (shared.upgrade(), workers.upgrade()) {
                Self { shared, workers }.kick(account);
            }
        });
        *outbox
            .shared
            .emitter
            .waker
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(waker);
        outbox
    }

    /// Also sends every change into the core's event stream, coalesced (see the module docs).
    pub fn forward_to(&self, core: broadcast::Sender<CoreEvent>) {
        *self
            .shared
            .emitter
            .core
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(core);
    }

    /// Replaces the hook called after operations changed mailboxes on the server.
    pub fn set_sync_hook(&self, hook: Arc<dyn SyncHook>) {
        *self
            .shared
            .hook
            .write()
            .unwrap_or_else(PoisonError::into_inner) = hook;
    }

    /// Subscribes to the outbox events. Events sent before subscribing are not replayed.
    pub fn subscribe(&self) -> broadcast::Receiver<OutboxEvent> {
        self.shared.emitter.events.subscribe()
    }

    /// Starts a worker for every account with queued or in-flight operations. In-flight ones
    /// (the app stopped while they were sent) are checked against the server first.
    pub async fn start(&self) -> Result<(), CoreError> {
        for account in self.shared.store.accounts().await? {
            let pending = self.shared.store.pending_operations(account.id).await?;
            if !pending.is_empty() {
                let in_flight = pending
                    .iter()
                    .filter(|op| op.state == mail_model::OperationState::InFlight)
                    .count();
                tracing::info!(account = %account.id, pending = pending.len(), in_flight, "outbox resumes");
                self.kick(account.id);
            }
        }
        Ok(())
    }

    /// Applies `action` to the messages (all of one account) and queues it for the server.
    /// See the module docs for what is refused. Returns the batch for [`Outbox::undo`].
    pub async fn apply(
        &self,
        action: MailAction,
        messages: Vec<MessageRef>,
    ) -> Result<ActionResult, CoreError> {
        self.apply_request(action, messages, request_id()).await
    }

    /// Like [`Outbox::apply`] with the caller's request id (a UUID per user action): repeating
    /// the call with the same id changes nothing (an IPC call sent twice). At most
    /// [`MAX_ACTION_MESSAGES`] messages (`invalid_argument` otherwise).
    pub async fn apply_request(
        &self,
        action: MailAction,
        messages: Vec<MessageRef>,
        request: String,
    ) -> Result<ActionResult, CoreError> {
        if messages.len() > MAX_ACTION_MESSAGES {
            return Err(CoreError::InvalidArgument(format!(
                "{} messages in one action; at most {MAX_ACTION_MESSAGES}",
                messages.len()
            )));
        }
        let kind = action.kind();
        let change = self
            .shared
            .store
            .enqueue_action(action, messages, request)
            .await?;
        tracing::info!(account = %change.account, action = kind.as_str(), queued = change.operations.len(), cancelled = change.removed.len(), "action recorded");
        Ok(self.recorded(&change))
    }

    /// Queues the creation of a mailbox (decoded path, the server's delimiter) that gets
    /// `role` (for example `archive`, after [`mail_model::ActionError::MissingMailbox`]). The mailbox
    /// appears in the folder list once the server created it (`MailboxesChanged`).
    pub async fn create_mailbox(
        &self,
        account: AccountId,
        path: String,
        role: Option<MailboxRole>,
    ) -> Result<ActionResult, CoreError> {
        let change = self
            .shared
            .store
            .enqueue_create_mailbox(account, path, role, request_id())
            .await?;
        Ok(self.recorded(&change))
    }

    /// Stage 18: queues the creation of the folder `name` below `parent`, or at the top of the
    /// account; the store checks the name and computes the path with the server's delimiter
    /// ([`mail_store::Store::enqueue_create_folder`]). The folder appears in the folder list once
    /// the server created it (`MailboxesChanged`).
    pub async fn create_folder(
        &self,
        account: AccountId,
        parent: Option<mail_model::MailboxId>,
        name: String,
    ) -> Result<ActionResult, CoreError> {
        let change = self
            .shared
            .store
            .enqueue_create_folder(account, parent, name, request_id())
            .await?;
        tracing::info!(%account, below = parent.is_some(), "folder creation recorded");
        Ok(self.recorded(&change))
    }

    /// Audit 24.11.2026: queues the rename of a folder of the user's own to `name` within its
    /// parent. The folder list changes once the server renamed it (`MailboxesChanged`).
    pub async fn rename_mailbox(
        &self,
        account: AccountId,
        mailbox: mail_model::MailboxId,
        name: String,
    ) -> Result<ActionResult, CoreError> {
        let change = self
            .shared
            .store
            .enqueue_rename_mailbox(account, mailbox, name, request_id())
            .await?;
        tracing::info!(%account, "folder rename recorded");
        Ok(self.recorded(&change))
    }

    /// Audit 24.11.2026: queues the deletion of a folder of the user's own with its messages.
    pub async fn delete_mailbox(
        &self,
        account: AccountId,
        mailbox: mail_model::MailboxId,
    ) -> Result<ActionResult, CoreError> {
        let change = self
            .shared
            .store
            .enqueue_delete_mailbox(account, mailbox, request_id())
            .await?;
        tracing::info!(%account, "folder deletion recorded");
        Ok(self.recorded(&change))
    }

    /// Undoes a batch (the "undo" of the notice after an action): cancels what is still
    /// queued, queues counter-operations for what was sent. Refused as a whole with
    /// [`mail_model::ActionError::NotUndoable`] if a part cannot be undone.
    pub async fn undo(&self, batch: BatchId) -> Result<ActionResult, CoreError> {
        let change = self.shared.store.undo_batch(batch, request_id()).await?;
        tracing::info!(account = %change.account, %batch, cancelled = change.removed.len(), queued = change.operations.len(), "batch undone");
        Ok(self.recorded(&change))
    }

    /// Queues a failed or conflicting operation again (activity view, "retry"). An unclear
    /// send is refused (`confirmation_required`): it is only sent again through the user's
    /// explicit decision ([`mail_model::UnclearDecision::SendAgain`]).
    pub async fn retry(&self, id: OperationId) -> Result<ActionResult, CoreError> {
        let change = self.shared.store.retry_operation(id, request_id()).await?;
        Ok(self.recorded(&change))
    }

    /// Removes a finished operation from the activity view ("discard").
    pub async fn discard(&self, id: OperationId) -> Result<(), CoreError> {
        let change = self.shared.store.discard_operation(id).await?;
        self.shared.emitter.change(&change);
        Ok(())
    }

    /// How many of the account's operations are pending, failed or in conflict.
    pub async fn counts(&self, account: AccountId) -> Result<OutboxCounts, CoreError> {
        Ok(self.shared.store.outbox_counts(account).await?)
    }

    /// The account's operations, newest first (activity view), keyset-paged below `before`.
    pub async fn operations(
        &self,
        account: AccountId,
        before: Option<OperationId>,
        limit: u32,
    ) -> Result<Vec<OperationInfo>, CoreError> {
        Ok(self
            .shared
            .store
            .operations(account, before, limit)
            .await?
            .iter()
            .map(mail_store::StoredOperation::info)
            .collect())
    }

    /// Turns writing on or off for the account (the user's switch; new accounts start off).
    /// Off fails queued operations with `read_only` and reverts them. The sync is unaffected:
    /// it stays read-only either way.
    pub async fn set_writes_enabled(
        &self,
        account: AccountId,
        enabled: bool,
    ) -> Result<(), CoreError> {
        let change = self
            .shared
            .store
            .set_writes_enabled(account, enabled)
            .await?;
        self.shared.emitter.change(&change);
        if enabled {
            // In-flight operations left from before can be checked now.
            self.kick(account);
        }
        Ok(())
    }

    /// The account can probably reach its server again (the sync reconnected, the network
    /// changed): retry at once instead of waiting out the backoff.
    pub fn wake(&self, account: AccountId) {
        let workers = self.lock();
        if let Some(worker) = workers.get(&account) {
            worker.kick.notify_one();
        }
    }

    /// [`Outbox::wake`] for every account with a worker.
    pub fn wake_all(&self) {
        for worker in self.lock().values() {
            worker.kick.notify_one();
        }
    }

    /// Stops the account's worker (the account is being removed); its connection gets a moment
    /// to log out. A later action starts a new one.
    pub async fn stop_account(&self, account: AccountId) {
        let worker = self.lock().remove(&account);
        if let Some(worker) = worker {
            let _ = worker.stop.send(true);
            let mut task = worker.task;
            if tokio::time::timeout(Duration::from_secs(3), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
    }

    /// Aborts every worker at once (the core is dropped without [`Outbox::shutdown`]):
    /// connections close without LOGOUT; in-flight operations are checked at the next start.
    pub(crate) fn abort_all(&self) {
        for worker in self.lock().values() {
            worker.task.abort();
        }
    }

    /// Stops every worker; open connections get a moment to log out.
    pub async fn shutdown(&self) {
        let workers: Vec<WorkerHandle> = self.lock().drain().map(|(_, w)| w).collect();
        for worker in &workers {
            let _ = worker.stop.send(true);
        }
        for worker in workers {
            let mut task = worker.task;
            if tokio::time::timeout(Duration::from_secs(3), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
    }

    /// Stage 5: a change the compose module recorded in the store (a send, a draft's copy, a
    /// discarded draft): emits its events and wakes the account's worker.
    pub(crate) fn changed(&self, change: &OutboxChange) {
        self.shared.emitter.change(change);
        if !change.operations.is_empty() {
            self.kick(change.account);
        }
    }

    /// Stage 10: what the account's SMTP server announces about message sizes (SIZE), asked
    /// over a connection of its own that sends nothing.
    pub(crate) async fn smtp_limits(
        &self,
        account: &mail_store::StoredAccount,
    ) -> Result<mail_model::SmtpLimits, CoreError> {
        // Stage 14: a password or an access token (renewed once if the server rejects it).
        self.shared
            .credentials
            .with_login(&account.credential_ref, &account.smtp, false, |login| {
                let mut config = login.config(&account.smtp, mail_model::Access::ReadOnly);
                crate::protocols::for_account(&mut config, account);
                config.timeouts = self.shared.config.timeouts;
                config.extra_root_ca = self.shared.config.extra_root_ca.clone();
                let sender = std::sync::Arc::clone(&self.shared.config.sender.0);
                async move { sender.limits(&config).await }
            })
            .await
    }

    pub(crate) fn compose_config(&self) -> &ComposeConfig {
        &self.shared.config.compose
    }

    /// Held while compose files are written ahead of their operation.
    pub(crate) fn files_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(&self.shared.emitter.files_lock)
    }

    /// Stage 16: a change recorded elsewhere (a transfer between accounts): emits its events,
    /// wakes the account's worker and returns the result for the UI.
    pub(crate) fn recorded_result(&self, change: &OutboxChange) -> ActionResult {
        self.recorded(change)
    }

    fn recorded(&self, change: &OutboxChange) -> ActionResult {
        self.shared.emitter.change(change);
        if !change.operations.is_empty() {
            self.kick(change.account);
        }
        result_of(change)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<AccountId, WorkerHandle>> {
        self.workers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wakes the account's worker, starting it if needed.
    fn kick(&self, account: AccountId) {
        let mut workers = self.lock();
        if let Some(worker) = workers.get(&account)
            && !worker.task.is_finished()
        {
            worker.kick.notify_one();
            return;
        }
        let kick = Arc::new(Notify::new());
        let (stop, stop_rx) = watch::channel(false);
        let task = tokio::spawn(worker::run(
            Arc::clone(&self.shared),
            account,
            Arc::clone(&kick),
            stop_rx,
        ));
        workers.insert(account, WorkerHandle { kick, stop, task });
    }
}
