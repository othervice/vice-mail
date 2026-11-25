//! Public API of the store for the outbox (stage 4, decision 0012). Each method is one
//! `BEGIN IMMEDIATE` transaction on the writer thread (or a read on a reader): the local change
//! and the recorded operation are written together or not at all. SQL and rules in `outbox`.
//!
//! Errors the user can act on are [`StoreError::Action`] with a typed [`ActionError`]: an
//! account without write access ([`ActionError::ReadOnly`], checked before anything is
//! recorded), a missing archive, trash or junk mailbox, a delete in the trash without the
//! explicit action, an invalid keyword, messages the list showed but the store no longer has.

use mail_model::{
    AccountId, ActionError, ActionKind, BatchId, ConflictReason, DraftId, FlagUpdate, IpcError,
    MailAction, MailboxId, MailboxRole, MessageId, MessageRef, ModSeq, OperationId, OperationInfo,
    OperationNotice, OperationPayload, OperationState, OutboxCounts, ServerCopy, Uid, UidValidity,
};

use crate::settings::now_millis;
use crate::{Store, StoreError, outbox};

/// Largest page of [`Store::operations`].
pub const MAX_OPERATIONS_PAGE: u32 = 500;

/// One row of the `operation` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOperation {
    pub id: OperationId,
    pub account: AccountId,
    pub batch: BatchId,
    pub action: ActionKind,
    pub payload: OperationPayload,
    pub state: OperationState,
    /// Times the command was sent.
    pub attempts: u32,
    pub last_error: Option<IpcError>,
    pub conflict: Option<ConflictReason>,
    pub notice: Option<OperationNotice>,
    /// Source mailbox, message and move target; `None` once the local row is gone.
    pub mailbox_id: Option<MailboxId>,
    pub message_id: Option<MessageId>,
    pub target_mailbox_id: Option<MailboxId>,
    /// MODSEQ of the source location as stored now (it grows when the outbox records the
    /// server's answer to an earlier STORE). `None` if the location is gone or has none.
    pub current_modseq: Option<ModSeq>,
    /// Unix milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
    /// Stage 5: not to be executed before this time (Unix ms): the undo delay of a send, the
    /// next look into Sent after an unclear one. `None`: at once.
    pub not_before: Option<i64>,
}

impl StoredOperation {
    /// Whether the outbox may execute it at `now` (Unix ms).
    pub fn is_due(&self, now: i64) -> bool {
        self.not_before.is_none_or(|at| at <= now)
    }

    /// The form the activity view shows.
    pub fn info(&self) -> OperationInfo {
        OperationInfo {
            id: self.id,
            batch: self.batch,
            account_id: self.account,
            action: self.action,
            kind: self.payload.kind(),
            state: self.state,
            mailbox_id: self.mailbox_id,
            message_id: self.message_id,
            target_mailbox_id: self.target_mailbox_id,
            message_count: self.payload.message_count(),
            attempts: self.attempts,
            error: self.last_error.clone(),
            conflict: self.conflict,
            notice: self.notice,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// How an operation ended on the server, for [`Store::finish_operations`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationResult {
    /// Done. `flags`: the message's flags as the server reported them afterwards (STORE);
    /// `moved_to`: the target's UIDVALIDITY and the new UID (COPYUID), so the target location
    /// is known before the next sync.
    Confirmed {
        flags: Option<FlagUpdate>,
        moved_to: Option<(UidValidity, Uid)>,
        notice: Option<OperationNotice>,
    },
    /// Not done; the local change is reverted.
    Failed { error: IpcError },
    /// The server state changed meanwhile; the server wins. For flags, `flags` is the server's
    /// current state if known (written locally), otherwise the local change is reverted. For
    /// moves and deletes the location stays removed locally: the message is elsewhere or gone,
    /// and the next sync shows where.
    Conflict {
        reason: ConflictReason,
        flags: Option<FlagUpdate>,
    },
    /// Stage 5: the server accepted a send (or the user treated an unclear one as sent):
    /// confirmed, and its follow-ups are recorded in the same transaction: the copy into Sent
    /// (unless `copy_on_server`, or the server files it itself), removing the draft's copy in
    /// Drafts, `\Answered` or `$Forwarded` on the original; the draft is deleted.
    Sent { copy_on_server: bool },
    /// Stage 5: an append is done; `copy` is where the new copy lies (if known). A draft's
    /// version becomes its server copy (or is removed again if the draft is gone meanwhile).
    Appended { copy: Option<ServerCopy> },
    /// Stage 5: not done yet. Back to `queued` (nothing went out), or kept `in_flight` with
    /// `keep_in_flight` (an unclear send that must not be submitted again), with the error to
    /// show and the time of the next try; `payload` replaces the stored one.
    Later {
        keep_in_flight: bool,
        error: Option<IpcError>,
        not_before: Option<i64>,
        payload: Option<Box<OperationPayload>>,
    },
}

/// What a call changed, so the engine can emit events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxChange {
    pub account: AccountId,
    /// The batch recorded (or found, with `repeated`).
    pub batch: Option<BatchId>,
    /// Operations recorded or changed, in their new state.
    pub operations: Vec<StoredOperation>,
    /// Operations removed from the outbox (cancelled).
    pub removed: Vec<OperationId>,
    /// Mailboxes whose local messages changed.
    pub mailboxes: Vec<MailboxId>,
    /// The request id was recorded before; nothing was applied again.
    pub repeated: bool,
    /// Stage 5: drafts whose state changed (frozen, released, server copy).
    pub drafts: Vec<DraftId>,
    /// Stage 5: drafts deleted (sent or discarded); their files can go.
    pub drafts_removed: Vec<DraftId>,
    /// Stage 16: what the change recorded in other accounts (a move between accounts removes
    /// its source once the copy is confirmed in the target).
    pub others: Vec<OutboxChange>,
}

impl OutboxChange {
    pub fn new(account: AccountId) -> Self {
        Self {
            account,
            batch: None,
            operations: Vec::new(),
            removed: Vec::new(),
            mailboxes: Vec::new(),
            repeated: false,
            drafts: Vec::new(),
            drafts_removed: Vec::new(),
            others: Vec::new(),
        }
    }

    pub(crate) fn draft_changed(&mut self, draft: &DraftId) {
        if !self.drafts.contains(draft) {
            self.drafts.push(draft.clone());
        }
    }
}

impl Store {
    /// Plans `action` on the messages (one account), applies its local effect and records the
    /// operations, in one transaction. Flag operations that a still queued one cancels out are
    /// removed instead (`removed`); flags that are already as asked record nothing.
    ///
    /// `request` identifies the call (a UUID from the engine): recording the same request
    /// again returns the first batch with `repeated` and changes nothing.
    ///
    /// Refused with [`StoreError::Action`] before anything is written:
    /// [`ActionError::ReadOnly`] (account without `writes_enabled`), [`ActionError::NoMessages`],
    /// [`ActionError::MessageNotFound`], [`ActionError::MixedAccounts`] and the planner's errors
    /// ([`mail_model::plan_action`]).
    pub async fn enqueue_action(
        &self,
        action: MailAction,
        messages: Vec<MessageRef>,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| outbox::enqueue_action(tx, &action, &messages, &request, now_millis()))
            .await
    }

    /// Records the creation of a mailbox (decoded path with the server's delimiter) that gets
    /// `role` once the server confirmed it. No local mailbox row before that: the folder list
    /// comes from the server.
    pub async fn enqueue_create_mailbox(
        &self,
        account: AccountId,
        path: String,
        role: Option<MailboxRole>,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| {
            outbox::enqueue_create_mailbox(tx, account, &path, role, &request, now_millis())
        })
        .await
    }

    /// Queued and in-flight operations of the account, in execution order.
    pub async fn pending_operations(
        &self,
        account: AccountId,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        self.read(move |conn| outbox::pending(conn, account)).await
    }

    /// Marks operations as in flight right before their command is sent, counts the attempt
    /// and stores the payload as given (a move records its target mark here). Returns the ids
    /// that were still pending; the others were cancelled meanwhile and must not be sent.
    pub async fn begin_operations(
        &self,
        operations: Vec<(OperationId, OperationPayload)>,
    ) -> Result<Vec<OperationId>, StoreError> {
        self.write(move |tx| outbox::begin(tx, &operations, now_millis()))
            .await
    }

    /// Records outcomes with their local effects (see [`OperationResult`]). Operations that
    /// are no longer pending are skipped. `None` if nothing changed.
    pub async fn finish_operations(
        &self,
        results: Vec<(OperationId, OperationResult)>,
    ) -> Result<Option<OutboxChange>, StoreError> {
        self.write(move |tx| outbox::finish(tx, &results, now_millis()))
            .await
    }

    pub async fn operation(&self, id: OperationId) -> Result<Option<StoredOperation>, StoreError> {
        self.read(move |conn| outbox::operation(conn, id)).await
    }

    /// The account's operations, newest first, keyset-paged below `before`. `limit` is
    /// 1..=[`MAX_OPERATIONS_PAGE`].
    pub async fn operations(
        &self,
        account: AccountId,
        before: Option<OperationId>,
        limit: u32,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        if limit == 0 || limit > MAX_OPERATIONS_PAGE {
            return Err(StoreError::InvalidArgument(format!(
                "operations limit {limit} is outside 1..={MAX_OPERATIONS_PAGE}"
            )));
        }
        self.read(move |conn| outbox::of_account(conn, account, before, limit))
            .await
    }

    /// How many of the account's operations are pending, failed or in conflict (stage 4,
    /// status badge). `NotFound` for an unknown account.
    pub async fn outbox_counts(&self, account: AccountId) -> Result<OutboxCounts, StoreError> {
        self.read(move |conn| outbox::counts(conn, account)).await
    }

    /// Undoes a batch: queued operations are cancelled (local change reverted, row removed),
    /// confirmed or in-flight flag changes get counter-operations, confirmed moves a move back
    /// from where the message is now. Refused as a whole with [`ActionError::NotUndoable`] if
    /// one part cannot be undone (a permanent delete, a created folder, a move that is in
    /// flight or not yet in the target locally).
    pub async fn undo_batch(
        &self,
        batch: BatchId,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| outbox::undo(tx, batch, &request, now_millis()))
            .await
    }

    /// Queues a failed or conflicting operation again, with its local effect, as a new
    /// operation at the end of the queue (same batch; the old row is in `removed`), so it runs
    /// after what the user did since. The message must still be where the operation found it
    /// ([`ActionError::MessageNotFound`] otherwise). A flag that is already as asked records
    /// nothing new.
    pub async fn retry_operation(
        &self,
        id: OperationId,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| outbox::retry(tx, id, &request, now_millis()))
            .await
    }

    /// Removes a finished operation (failed, conflict or confirmed) from the outbox. Pending
    /// ones are refused ([`ActionError::WrongState`]); a queued one is cancelled with
    /// [`Store::undo_batch`].
    pub async fn discard_operation(&self, id: OperationId) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| outbox::discard(tx, id)).await
    }

    /// Turns writing on or off for the account. Turning it off fails every queued operation
    /// with `read_only` and reverts its local change (in-flight ones wait until writes are on
    /// again, then they are checked against the server). New accounts start with writes off.
    pub async fn set_writes_enabled(
        &self,
        account: AccountId,
        enabled: bool,
    ) -> Result<OutboxChange, StoreError> {
        let change = self
            .write(move |tx| outbox::set_writes(tx, account, enabled, now_millis()))
            .await?;
        tracing::info!(%account, enabled, "write access changed");
        Ok(change)
    }

    /// Deletes confirmed operations last changed before `before` (Unix ms). Failed and
    /// conflicting ones stay until the user discards them. Returns the count.
    pub async fn purge_operations(
        &self,
        account: AccountId,
        before: i64,
    ) -> Result<usize, StoreError> {
        self.write(move |tx| outbox::purge(tx, account, before))
            .await
    }
}

impl From<ActionError> for StoreError {
    fn from(err: ActionError) -> Self {
        Self::Action(err)
    }
}
