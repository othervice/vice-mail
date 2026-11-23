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

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use mail_imap::ImapConnector;
use mail_model::{
    AccountId, ActionResult, BatchId, CoreError, CoreEvent, ExtraRootCa, MAX_ACTION_MESSAGES,
    MailAction, MailboxId, MailboxRole, MessageRef, OperationId, OperationInfo, OutboxCounts,
    OutboxEvent, Timeouts, WriteConnector,
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
}

impl Emitter {
    fn new() -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            events,
            core: RwLock::new(None),
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
        emit_change(&self.events, change);
        let Some(core) = self.core() else { return };
        let send = |event| {
            if core.send(event).is_err() {
                tracing::trace!("core event dropped: no subscribers");
            }
        };
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
/// [`WriteConnector`]; the product uses [`ImapConnector`].
pub struct Outbox<C: WriteConnector + 'static = ImapConnector> {
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
        Self {
            shared: Arc::new(Shared {
                store,
                connector,
                credentials,
                config,
                clock: Arc::new(SystemClock),
                emitter: Emitter::new(),
                hook: RwLock::new(Arc::new(NoSyncHook)),
            }),
            workers: Arc::new(Mutex::new(HashMap::new())),
        }
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

    /// Undoes a batch (the "undo" of the notice after an action): cancels what is still
    /// queued, queues counter-operations for what was sent. Refused as a whole with
    /// [`mail_model::ActionError::NotUndoable`] if a part cannot be undone.
    pub async fn undo(&self, batch: BatchId) -> Result<ActionResult, CoreError> {
        let change = self.shared.store.undo_batch(batch, request_id()).await?;
        tracing::info!(account = %change.account, %batch, cancelled = change.removed.len(), queued = change.operations.len(), "batch undone");
        Ok(self.recorded(&change))
    }

    /// Queues a failed or conflicting operation again (activity view, "retry").
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
