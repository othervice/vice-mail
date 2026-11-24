//! Executes one group of operations on a writer connection, and checks operations that were
//! in flight when a connection broke or the app stopped.
//!
//! **Before anything is sent** the outbox looks at the server: the source mailbox must have the
//! UIDVALIDITY the operation was planned for (otherwise `conflict: mailbox_reset`) and the
//! message must still be there (otherwise `conflict: gone` — moved or deleted by another
//! client; the server wins). A flag that is already as asked is confirmed without a STORE.
//!
//! **Flags** go out as `UID STORE ... (UNCHANGEDSINCE m)` on CONDSTORE mailboxes, where `m` is
//! the message's MODSEQ as the app last saw it (recorded by the sync, or by the outbox from the
//! answer to its own earlier STORE). Without a known MODSEQ (a location the outbox added after
//! a move) the STORE is unconditional. Messages the server reports as `[MODIFIED]` are read
//! again: if the server already has the asked state, confirmed; otherwise `conflict: modified`
//! and the server's flags are written locally.
//!
//! **Moves** record the target's UIDNEXT and UIDVALIDITY (STATUS) before the command is sent.
//! Whatever arrives in the target at or above that UIDNEXT came after this moment, so a later
//! check can tell this operation's copy from an older copy of the same message. Checking an
//! in-flight move ([`Step::verify_move`]): copy in the target and original gone → confirmed;
//! copy and original both there (the COPY fallback broke before EXPUNGE) → only the expunge is
//! repeated, never the copy; no copy and original there → sent again; neither → conflict.
//!
//! **Errors:** a refusal (NO/BAD, missing target) fails the operation and reverts its local
//! change; a broken connection ends the step, operations stay in flight and are checked after
//! reconnecting. Outcomes collected before a connection broke are stored anyway.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use mail_model::{
    AccountId, BackendError, ConflictReason, CoreError, ExpungeOutcome, Flag, FlagUpdate, IpcError,
    MailWriter, MailboxId, MessageHeader, ModSeq, MoveMethod, OperationId, OperationNotice,
    OperationPayload, OperationState, SelectedMailbox, SourceLocation, StoreMode, TargetMailbox,
    TargetMark, Uid, UidSet, UidValidity,
};
use mail_store::{OperationResult, OutboxChange, Store, StoreError, StoredOperation};

pub(crate) mod compose;

/// Most operations sent in one command.
pub(crate) const MAX_GROUP: usize = 500;

/// Why a step ended early.
#[derive(Debug)]
pub(crate) enum StepError {
    /// The connection is unusable; reconnect. In-flight operations are checked then.
    Connection(BackendError),
    Store(StoreError),
}

impl From<StoreError> for StepError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

/// What a step did.
#[derive(Debug, Default)]
pub(crate) struct StepOutcome {
    pub(crate) change: Option<OutboxChange>,
    /// Mailboxes the step changed on the server (for the sync hook).
    pub(crate) server_mailboxes: Vec<MailboxId>,
    /// A mailbox was created and the folder list stored again.
    pub(crate) list_changed: bool,
}

/// A writer connection and the mailbox it has open.
pub(crate) struct Connection<W> {
    pub(crate) writer: W,
    selected: Option<SelectedMailbox>,
}

impl<W: MailWriter> Connection<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self {
            writer,
            selected: None,
        }
    }

    /// SELECT, unless the mailbox is open already.
    async fn open(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        if let Some(open) = &self.selected
            && open.name == name
        {
            return Ok(open.clone());
        }
        self.selected = None;
        let opened = self.writer.open_mailbox(name).await?;
        self.selected = Some(opened.clone());
        Ok(opened)
    }
}

/// The next group: consecutive operations of the same batch, state class, kind, source mailbox
/// and parameters (flag and value, or target), at most [`MAX_GROUP`]. Mailbox creation alone.
pub(crate) fn next_group(pending: &[StoredOperation]) -> Vec<StoredOperation> {
    let Some(first) = pending.first() else {
        return Vec::new();
    };
    let key = |op: &StoredOperation| -> Option<String> {
        let source = op.payload.source()?;
        let params = match &op.payload {
            // Stage 14: Gmail labels exactly as named; flags and keywords ignoring case.
            OperationPayload::SetFlag {
                flag: Flag::Label(label),
                value,
                ..
            } => format!("label:{label}={value}"),
            OperationPayload::SetFlag { flag, value, .. } => {
                format!("{}={value}", flag.imap().to_ascii_lowercase())
            }
            OperationPayload::Move { target, .. } => target.name.clone(),
            OperationPayload::Expunge { .. }
            | OperationPayload::CreateMailbox { .. }
            | OperationPayload::Send(_)
            | OperationPayload::Append { .. }
            | OperationPayload::RemoveCopy { .. }
            | OperationPayload::RenameMailbox { .. }
            | OperationPayload::DeleteMailbox { .. } => String::new(),
        };
        Some(format!(
            "{}|{:?}|{}|{}|{}|{params}",
            op.batch,
            op.state,
            op.payload.kind().as_str(),
            source.mailbox_name,
            source.uid_validity
        ))
    };
    let Some(want) = key(first) else {
        return vec![first.clone()];
    };
    pending
        .iter()
        .take_while(|op| key(op).as_ref() == Some(&want))
        .take(MAX_GROUP)
        .cloned()
        .collect()
}

/// A refusal becomes the operation's error; anything else means the connection is gone.
fn refusal(err: BackendError) -> Result<IpcError, StepError> {
    match err {
        BackendError::Rejected(_)
        | BackendError::InvalidState(_)
        | BackendError::Unsupported(_)
        | BackendError::ReadOnly(_)
        | BackendError::MailboxNotFound(_) => Ok(IpcError::from(CoreError::Mail(err))),
        other => Err(StepError::Connection(other)),
    }
}

fn lost(err: BackendError) -> StepError {
    StepError::Connection(err)
}

fn source_of(op: &StoredOperation) -> Option<&SourceLocation> {
    op.payload.source()
}

fn uids_of(ops: &[StoredOperation]) -> UidSet {
    UidSet::from_uids(ops.iter().filter_map(|op| source_of(op).map(|s| s.uid)))
}

/// One step: executes or checks one group and stores the outcomes.
pub(crate) async fn run_step<W: MailWriter>(
    conn: &mut Connection<W>,
    store: &Store,
    account: AccountId,
    max_attempts: u32,
    sent_checks: &[Duration],
    group: Vec<StoredOperation>,
) -> Result<StepOutcome, StepError> {
    let mut step = Step {
        conn,
        store,
        account,
        max_attempts,
        sent_checks,
        results: Vec::new(),
        server_mailboxes: BTreeSet::new(),
        list_changed: false,
    };
    let ran = step.run(group).await;
    // Outcomes known before a failure are kept.
    let results = std::mem::take(&mut step.results);
    let change = if results.is_empty() {
        None
    } else {
        store.finish_operations(results).await?
    };
    ran?;
    Ok(StepOutcome {
        change,
        server_mailboxes: step.server_mailboxes.into_iter().collect(),
        list_changed: step.list_changed,
    })
}

struct Step<'a, W> {
    conn: &'a mut Connection<W>,
    store: &'a Store,
    account: AccountId,
    max_attempts: u32,
    /// Stage 5: delays between the looks into Sent after an unclear send.
    sent_checks: &'a [Duration],
    results: Vec<(OperationId, OperationResult)>,
    server_mailboxes: BTreeSet<MailboxId>,
    list_changed: bool,
}

impl<W: MailWriter> Step<'_, W> {
    async fn run(&mut self, group: Vec<StoredOperation>) -> Result<(), StepError> {
        let Some(first) = group.first() else {
            return Ok(());
        };
        match &first.payload {
            OperationPayload::SetFlag { .. } => self.set_flags(group).await,
            OperationPayload::Move { .. } if first.state == OperationState::InFlight => {
                for op in group {
                    self.verify_move(op, None).await?;
                }
                Ok(())
            }
            OperationPayload::Move { .. } => self.moves(group).await,
            OperationPayload::Expunge { .. } => self.expunge(group).await,
            OperationPayload::CreateMailbox { .. } => {
                for op in group {
                    self.create(op).await?;
                }
                Ok(())
            }
            OperationPayload::RenameMailbox { .. } => {
                for op in group {
                    self.rename_folder(op).await?;
                }
                Ok(())
            }
            OperationPayload::DeleteMailbox { .. } => {
                for op in group {
                    self.delete_folder(op).await?;
                }
                Ok(())
            }
            OperationPayload::Send(_) => {
                let checks = self.sent_checks;
                for op in group {
                    self.verify_send(op, checks).await?;
                }
                Ok(())
            }
            OperationPayload::Append { .. } => {
                for op in group {
                    self.append(op).await?;
                }
                Ok(())
            }
            OperationPayload::RemoveCopy { .. } => {
                for op in group {
                    self.remove_copy(op).await?;
                }
                Ok(())
            }
        }
    }

    // -- outcomes ------------------------------------------------------------------------

    fn confirm(
        &mut self,
        op: &StoredOperation,
        flags: Option<FlagUpdate>,
        moved_to: Option<(UidValidity, Uid)>,
        notice: Option<OperationNotice>,
    ) {
        if let Some(source) = source_of(op) {
            self.server_mailboxes.insert(source.mailbox_id);
        }
        if let Some(target) = op.payload.target() {
            self.server_mailboxes.insert(target.mailbox_id);
        }
        self.results.push((
            op.id,
            OperationResult::Confirmed {
                flags,
                moved_to,
                notice,
            },
        ));
    }

    fn fail(&mut self, op: &StoredOperation, error: IpcError) {
        tracing::warn!(account = %self.account, operation = %op.id, code = ?error.code, "operation failed");
        self.results
            .push((op.id, OperationResult::Failed { error }));
    }

    fn conflict(
        &mut self,
        op: &StoredOperation,
        reason: ConflictReason,
        flags: Option<FlagUpdate>,
    ) {
        tracing::info!(account = %self.account, operation = %op.id, ?reason, "operation conflicts with the server; the server wins");
        if let Some(source) = source_of(op) {
            self.server_mailboxes.insert(source.mailbox_id);
        }
        self.results
            .push((op.id, OperationResult::Conflict { reason, flags }));
    }

    fn gave_up(&self, op: &StoredOperation) -> IpcError {
        IpcError::from(CoreError::Mail(BackendError::Protocol(format!(
            "gave up after {} attempts",
            op.attempts
        ))))
    }

    /// Operations that may be sent once more; the rest fail.
    fn within_attempts(&mut self, ops: Vec<StoredOperation>) -> Vec<StoredOperation> {
        let mut keep = Vec::with_capacity(ops.len());
        for op in ops {
            if op.attempts >= self.max_attempts {
                let error = self.gave_up(&op);
                self.fail(&op, error);
            } else {
                keep.push(op);
            }
        }
        keep
    }

    /// Marks the operations in flight (with their payloads as given); returns those that were
    /// still pending.
    async fn begin(
        &mut self,
        ops: Vec<StoredOperation>,
    ) -> Result<Vec<StoredOperation>, StepError> {
        let begun = self
            .store
            .begin_operations(ops.iter().map(|op| (op.id, op.payload.clone())).collect())
            .await?;
        Ok(ops
            .into_iter()
            .filter(|op| begun.contains(&op.id))
            .collect())
    }

    /// Opens the source mailbox of a group. `None` after recording why it cannot be used.
    async fn open_source(
        &mut self,
        ops: &[StoredOperation],
        source: &SourceLocation,
    ) -> Result<Option<SelectedMailbox>, StepError> {
        let outcome = match self.conn.open(&source.mailbox_name).await {
            Ok(opened) if opened.uid_validity == source.uid_validity => return Ok(Some(opened)),
            Ok(_) => Err(ConflictReason::MailboxReset),
            Err(BackendError::MailboxNotFound(_)) => Err(ConflictReason::MailboxGone),
            Err(err) => Ok(refusal(err)?),
        };
        for op in ops {
            match &outcome {
                Err(reason) => self.conflict(op, *reason, None),
                Ok(error) => self.fail(op, error.clone()),
            }
        }
        Ok(None)
    }

    async fn current_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<HashMap<Uid, FlagUpdate>, StepError> {
        if uids.is_empty() {
            return Ok(HashMap::new());
        }
        Ok(self
            .conn
            .writer
            .fetch_flags(mailbox, uids, None)
            .await
            .map_err(lost)?
            .into_iter()
            .map(|update| (update.uid, update))
            .collect())
    }

    // -- flags -----------------------------------------------------------------------------

    async fn set_flags(&mut self, ops: Vec<StoredOperation>) -> Result<(), StepError> {
        let Some(OperationPayload::SetFlag {
            source,
            flag,
            value,
            ..
        }) = ops.first().map(|op| op.payload.clone())
        else {
            return Ok(());
        };
        let Some(mailbox) = self.open_source(&ops, &source).await? else {
            return Ok(());
        };
        // Stage 14 (decision 0034): a Gmail label is always stored (idempotent, and Gmail may
        // leave the selected folder's own label out of X-GM-LABELS), without UNCHANGEDSINCE,
        // and confirmed without flags: its local effect is on the message, not the location.
        let label = matches!(flag, Flag::Label(_));
        let current = self.current_flags(&mailbox, &uids_of(&ops)).await?;
        let mut todo = Vec::new();
        for op in ops {
            let Some(uid) = source_of(&op).map(|s| s.uid) else {
                continue;
            };
            match current.get(&uid) {
                None => self.conflict(&op, ConflictReason::Gone, None),
                Some(now) if !label && flag.is_set(&now.flags) == value => {
                    self.confirm(&op, Some(now.clone()), None, None);
                }
                Some(_) => todo.push(op),
            }
        }
        let todo = self.within_attempts(todo);

        // One STORE per known MODSEQ: UNCHANGEDSINCE must not be newer than what the app saw
        // of each message.
        let condstore = mailbox.highest_modseq.is_some() && !label;
        let mut by_modseq: BTreeMap<Option<ModSeq>, Vec<StoredOperation>> = BTreeMap::new();
        for op in todo {
            let known = source_of(&op)
                .and_then(|s| s.modseq)
                .max(op.current_modseq)
                .filter(|_| condstore);
            by_modseq.entry(known).or_default().push(op);
        }
        let mode = if value {
            StoreMode::Add
        } else {
            StoreMode::Remove
        };
        for (since, ops) in by_modseq {
            let begun = self.begin(ops).await?;
            if begun.is_empty() {
                continue;
            }
            let uids = uids_of(&begun);
            let stored = self
                .conn
                .writer
                .store_flags(&mailbox, &uids, mode, std::slice::from_ref(&flag), since)
                .await;
            let outcome = match stored {
                Ok(outcome) => outcome,
                Err(err) => {
                    let error = refusal(err)?;
                    for op in &begun {
                        self.fail(op, error.clone());
                    }
                    continue;
                }
            };
            let reported: HashMap<Uid, FlagUpdate> =
                outcome.updates.into_iter().map(|u| (u.uid, u)).collect();
            let mut recheck = Vec::new();
            for op in begun {
                let Some(uid) = source_of(&op).map(|s| s.uid) else {
                    continue;
                };
                if outcome.modified.contains(uid) {
                    recheck.push(op);
                    continue;
                }
                let flags = reported.get(&uid).cloned().or_else(|| {
                    current.get(&uid).map(|before| {
                        let mut after = before.clone();
                        flag.apply(&mut after.flags, value);
                        after.modseq = None;
                        after
                    })
                });
                self.confirm(&op, flags.filter(|_| !label), None, None);
            }
            if recheck.is_empty() {
                continue;
            }
            let now = self.current_flags(&mailbox, &uids_of(&recheck)).await?;
            for op in recheck {
                let uid = source_of(&op).map_or(0, |s| s.uid);
                match now.get(&uid) {
                    None => self.conflict(&op, ConflictReason::Gone, None),
                    Some(server) if flag.is_set(&server.flags) == value => {
                        self.confirm(&op, Some(server.clone()), None, None);
                    }
                    Some(server) => {
                        self.conflict(&op, ConflictReason::Modified, Some(server.clone()));
                    }
                }
            }
        }
        Ok(())
    }

    // -- moves -----------------------------------------------------------------------------

    async fn moves(&mut self, ops: Vec<StoredOperation>) -> Result<(), StepError> {
        let Some(OperationPayload::Move { source, target, .. }) =
            ops.first().map(|op| op.payload.clone())
        else {
            return Ok(());
        };
        let Some(mailbox) = self.open_source(&ops, &source).await? else {
            return Ok(());
        };
        let present = self.current_flags(&mailbox, &uids_of(&ops)).await?;
        let mut todo = Vec::new();
        for op in ops {
            if source_of(&op).is_some_and(|s| present.contains_key(&s.uid)) {
                todo.push(op);
            } else {
                self.conflict(&op, ConflictReason::Gone, None);
            }
        }
        let todo = self.within_attempts(todo);
        if todo.is_empty() {
            return Ok(());
        }

        let mark = match self.target_mark(&target).await {
            Ok(mark) => mark,
            Err(err) => {
                let error = refusal(err)?;
                for op in &todo {
                    self.fail(op, error.clone());
                }
                return Ok(());
            }
        };
        // STATUS may have needed a SELECT of the target: back to the source.
        let Some(mailbox) = self.open_source(&todo, &source).await? else {
            return Ok(());
        };
        let marked: Vec<StoredOperation> = todo
            .into_iter()
            .map(|mut op| {
                if let OperationPayload::Move { mark: m, .. } = &mut op.payload {
                    *m = Some(mark);
                }
                op
            })
            .collect();
        let begun = self.begin(marked).await?;
        if begun.is_empty() {
            return Ok(());
        }
        let uids = uids_of(&begun);
        match self
            .conn
            .writer
            .move_messages(&mailbox, &uids, &target.name)
            .await
        {
            Ok(outcome) => {
                let notice = (outcome.method == MoveMethod::CopyLeftDeleted)
                    .then_some(OperationNotice::LeftMarkedDeleted);
                for op in &begun {
                    let moved_to = outcome.copy_uid.as_ref().and_then(|copy| {
                        let uid = source_of(op)?.uid;
                        Some((copy.uid_validity, copy.target_of(uid)?))
                    });
                    self.confirm(op, None, moved_to, notice);
                }
            }
            Err(err) => {
                // Refused, possibly half way (the COPY fallback): look at what happened, as
                // after a crash, and only then decide.
                let error = refusal(err)?;
                for op in begun {
                    self.verify_move(op, Some(error.clone())).await?;
                }
            }
        }
        Ok(())
    }

    /// UIDNEXT and UIDVALIDITY of the target right now (STATUS; a SELECT if the server leaves
    /// them out).
    async fn target_mark(&mut self, target: &TargetMailbox) -> Result<TargetMark, BackendError> {
        let status = self.conn.writer.mailbox_status(&target.name).await?;
        if let (Some(uid_next), Some(uid_validity)) = (status.uid_next, status.uid_validity) {
            return Ok(TargetMark {
                uid_validity,
                uid_next,
            });
        }
        let opened = self.conn.open(&target.name).await?;
        Ok(TargetMark {
            uid_validity: opened.uid_validity,
            uid_next: opened.uid_next,
        })
    }

    /// The copy this move made in the target: a message that arrived at or above the recorded
    /// UIDNEXT with the same Message-ID (or, without one, the same size).
    async fn find_copy(
        &mut self,
        target: &TargetMailbox,
        mark: TargetMark,
        source: &SourceLocation,
    ) -> Result<Option<Uid>, StepError> {
        let opened = match self.conn.open(&target.name).await {
            Ok(opened) => opened,
            Err(err) => {
                refusal(err)?;
                return Ok(None);
            }
        };
        if opened.uid_validity != mark.uid_validity || opened.uid_next <= mark.uid_next {
            return Ok(None);
        }
        let arrived: Vec<MessageHeader> = self
            .conn
            .writer
            .fetch_headers(&opened, &UidSet::range(mark.uid_next, Uid::MAX))
            .await
            .map_err(lost)?;
        let same = |h: &MessageHeader| match &source.message_id_header {
            Some(id) => h.message_id.as_deref() == Some(id.as_str()),
            None => h.size == source.size,
        };
        Ok(arrived.iter().filter(|h| same(h)).map(|h| h.uid).min())
    }

    /// The source mailbox, if the message is still in it.
    async fn source_with(
        &mut self,
        source: &SourceLocation,
    ) -> Result<Option<SelectedMailbox>, StepError> {
        let opened = match self.conn.open(&source.mailbox_name).await {
            Ok(opened) if opened.uid_validity == source.uid_validity => opened,
            Ok(_) => return Ok(None),
            Err(err) => {
                refusal(err)?;
                return Ok(None);
            }
        };
        let present = self
            .current_flags(&opened, &UidSet::from_uids([source.uid]))
            .await?;
        Ok(present.contains_key(&source.uid).then_some(opened))
    }

    /// Decides an in-flight (or half refused) move by what the server shows; see the module
    /// docs. `refusal`: the server refused it just now, so "nothing happened" means failed
    /// rather than "send again".
    async fn verify_move(
        &mut self,
        op: StoredOperation,
        refused: Option<IpcError>,
    ) -> Result<(), StepError> {
        let OperationPayload::Move {
            source,
            target,
            mark,
        } = op.payload.clone()
        else {
            return Ok(());
        };
        let copy = match mark {
            Some(mark) => self
                .find_copy(&target, mark, &source)
                .await?
                .map(|uid| (mark.uid_validity, uid)),
            None => None,
        };
        let original = self.source_with(&source).await?;
        match (copy, original) {
            (Some(copy), Some(mailbox)) => {
                tracing::info!(account = %self.account, operation = %op.id, "move had copied but not removed the original; finishing it");
                let uids = UidSet::from_uids([source.uid]);
                match self.conn.writer.expunge_messages(&mailbox, &uids).await {
                    Ok(outcome) => {
                        let notice = (outcome == ExpungeOutcome::LeftDeleted)
                            .then_some(OperationNotice::LeftMarkedDeleted);
                        self.confirm(&op, None, Some(copy), notice);
                    }
                    Err(err) => {
                        let error = refusal(err)?;
                        self.fail(&op, error);
                    }
                }
            }
            (Some(copy), None) => self.confirm(&op, None, Some(copy), None),
            (None, Some(_)) => match refused {
                Some(error) => self.fail(&op, error),
                None => {
                    tracing::info!(account = %self.account, operation = %op.id, "move did not happen; sending it again");
                    Box::pin(self.moves(vec![op])).await?;
                }
            },
            (None, None) => self.conflict(&op, ConflictReason::Gone, None),
        }
        Ok(())
    }

    // -- permanent delete ------------------------------------------------------------------

    async fn expunge(&mut self, ops: Vec<StoredOperation>) -> Result<(), StepError> {
        let Some(source) = ops.first().and_then(|op| source_of(op).cloned()) else {
            return Ok(());
        };
        let Some(mailbox) = self.open_source(&ops, &source).await? else {
            return Ok(());
        };
        let present = self.current_flags(&mailbox, &uids_of(&ops)).await?;
        let mut todo = Vec::new();
        for op in ops {
            let here = source_of(&op).is_some_and(|s| present.contains_key(&s.uid));
            match (here, op.state) {
                (true, _) => todo.push(op),
                // Sent before and gone now: this operation removed it.
                (false, OperationState::InFlight) => self.confirm(&op, None, None, None),
                (false, _) => self.conflict(&op, ConflictReason::Gone, None),
            }
        }
        let todo = self.within_attempts(todo);
        let begun = self.begin(todo).await?;
        if begun.is_empty() {
            return Ok(());
        }
        match self
            .conn
            .writer
            .expunge_messages(&mailbox, &uids_of(&begun))
            .await
        {
            Ok(outcome) => {
                let notice = (outcome == ExpungeOutcome::LeftDeleted)
                    .then_some(OperationNotice::LeftMarkedDeleted);
                for op in &begun {
                    self.confirm(op, None, None, notice);
                }
            }
            Err(err) => {
                let error = refusal(err)?;
                for op in &begun {
                    self.fail(op, error.clone());
                }
            }
        }
        Ok(())
    }

    // -- create ----------------------------------------------------------------------------

    async fn create(&mut self, op: StoredOperation) -> Result<(), StepError> {
        let OperationPayload::CreateMailbox { path, role } = op.payload.clone() else {
            return Ok(());
        };
        let mut ops = self.within_attempts(vec![op]);
        let Some(op) = ops.pop() else {
            return Ok(());
        };
        let Some(op) = self.begin(vec![op]).await?.pop() else {
            return Ok(());
        };
        let name = match self.conn.writer.create_mailbox(&path, role).await {
            Ok(name) => name,
            Err(err) => {
                let error = refusal(err)?;
                self.fail(&op, error);
                return Ok(());
            }
        };
        // The folder list from the server, so the new mailbox gets its local row now.
        let remote = self.conn.writer.list_mailboxes().await.map_err(lost)?;
        self.store.sync_mailboxes(self.account, remote).await?;
        self.list_changed = true;
        if let Some(role) = role {
            let states = self.store.mailbox_sync_states(self.account).await?;
            if let Some(created) = states.iter().find(|s| s.name == name)
                && created.role != role
                && let Err(err) = self.store.set_mailbox_role(created.mailbox_id, role).await
            {
                tracing::warn!(account = %self.account, error = %err, "role of the new mailbox not set");
            }
        }
        self.confirm(&op, None, None, None);
        Ok(())
    }

    // -- rename and delete (audit 24.11.2026) -------------------------------------------------

    /// Whether the server lists a mailbox with this wire name.
    async fn listed(&mut self, name: &str) -> Result<bool, StepError> {
        let remote = self.conn.writer.list_mailboxes().await.map_err(lost)?;
        Ok(remote.iter().any(|m| m.name == name))
    }

    /// The folder list from the server, into the store (the renamed or deleted folder's rows).
    async fn refresh_folders(&mut self) -> Result<(), StepError> {
        let remote = self.conn.writer.list_mailboxes().await.map_err(lost)?;
        self.store.sync_mailboxes(self.account, remote).await?;
        self.list_changed = true;
        Ok(())
    }

    /// RENAME; after a crash (in flight) or a refusal it counts as done when the old name is
    /// gone and the new one exists. The local rows take the new name before the folder list is
    /// read again, so the folder keeps its messages.
    async fn rename_folder(&mut self, op: StoredOperation) -> Result<(), StepError> {
        let OperationPayload::RenameMailbox {
            mailbox, new_path, ..
        } = op.payload.clone()
        else {
            return Ok(());
        };
        let Some(op) = self.within_attempts(vec![op]).pop() else {
            return Ok(());
        };
        let Some(op) = self.begin(vec![op]).await?.pop() else {
            return Ok(());
        };
        // The connection must not keep the folder open while it changes.
        self.conn.selected = None;
        let new_name = match self
            .conn
            .writer
            .rename_mailbox(&mailbox.name, &new_path)
            .await
        {
            Ok(name) => name,
            Err(err) => {
                let error = refusal(err)?;
                let remote = self.conn.writer.list_mailboxes().await.map_err(lost)?;
                let done = remote
                    .iter()
                    .find(|m| m.path == new_path)
                    .map(|m| m.name.clone());
                match done {
                    Some(name) if !remote.iter().any(|m| m.name == mailbox.name) => name,
                    _ => {
                        self.fail(&op, error);
                        return Ok(());
                    }
                }
            }
        };
        self.store
            .rename_mailbox_rows(mailbox.mailbox_id, new_name, new_path)
            .await?;
        self.refresh_folders().await?;
        tracing::info!(account = %self.account, operation = %op.id, "folder renamed");
        self.confirm(&op, None, None, None);
        Ok(())
    }

    /// DELETE; a folder that is gone already counts as deleted (a repeat after a crash).
    async fn delete_folder(&mut self, op: StoredOperation) -> Result<(), StepError> {
        let OperationPayload::DeleteMailbox { mailbox, .. } = op.payload.clone() else {
            return Ok(());
        };
        let Some(op) = self.within_attempts(vec![op]).pop() else {
            return Ok(());
        };
        let Some(op) = self.begin(vec![op]).await?.pop() else {
            return Ok(());
        };
        self.conn.selected = None;
        if let Err(err) = self.conn.writer.delete_mailbox(&mailbox.name).await {
            let error = refusal(err)?;
            if self.listed(&mailbox.name).await? {
                self.fail(&op, error);
                return Ok(());
            }
        }
        self.refresh_folders().await?;
        tracing::info!(account = %self.account, operation = %op.id, "folder deleted");
        self.confirm(&op, None, None, None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mail_model::{ActionKind, BatchId, Flag, Flags, MessageId};

    use super::*;

    fn op(
        id: i64,
        batch: i64,
        state: OperationState,
        payload: OperationPayload,
    ) -> StoredOperation {
        StoredOperation {
            id: OperationId(id),
            account: "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id"),
            batch: BatchId(batch),
            action: ActionKind::MarkRead,
            payload,
            state,
            attempts: 0,
            last_error: None,
            conflict: None,
            notice: None,
            mailbox_id: None,
            message_id: None,
            target_mailbox_id: None,
            current_modseq: None,
            created_at: 0,
            updated_at: 0,
            not_before: None,
        }
    }

    fn source(mailbox: &str, uid: Uid) -> SourceLocation {
        SourceLocation {
            mailbox_id: MailboxId(1),
            mailbox_name: mailbox.into(),
            uid_validity: UidValidity(3),
            uid,
            message_id: MessageId(i64::from(uid)),
            message_id_header: None,
            size: 1,
            flags: Flags::default(),
            modseq: None,
        }
    }

    fn seen(mailbox: &str, uid: Uid, value: bool) -> OperationPayload {
        OperationPayload::SetFlag {
            source: source(mailbox, uid),
            flag: Flag::Seen,
            value,
            before: !value,
        }
    }

    #[test]
    fn groups_are_consecutive_operations_of_one_kind_mailbox_and_value() {
        use OperationState::{InFlight, Queued};
        let pending = vec![
            op(1, 1, InFlight, seen("INBOX", 1, true)),
            op(2, 1, Queued, seen("INBOX", 2, true)),
            op(3, 1, Queued, seen("INBOX", 3, true)),
            op(4, 1, Queued, seen("Other", 4, true)),
            op(5, 2, Queued, seen("INBOX", 5, true)),
        ];
        let ids = |g: Vec<StoredOperation>| g.iter().map(|o| o.id.0).collect::<Vec<_>>();
        assert_eq!(
            ids(next_group(&pending)),
            [1],
            "in flight apart from queued"
        );
        assert_eq!(ids(next_group(&pending[1..])), [2, 3]);
        assert_eq!(ids(next_group(&pending[3..])), [4]);
        let mixed = vec![
            op(1, 1, Queued, seen("INBOX", 1, true)),
            op(2, 1, Queued, seen("INBOX", 2, false)),
        ];
        assert_eq!(
            ids(next_group(&mixed)),
            [1],
            "another value is another STORE"
        );
        let create = vec![
            op(
                1,
                1,
                Queued,
                OperationPayload::CreateMailbox {
                    path: "A".into(),
                    role: None,
                },
            ),
            op(
                2,
                1,
                Queued,
                OperationPayload::CreateMailbox {
                    path: "B".into(),
                    role: None,
                },
            ),
        ];
        assert_eq!(ids(next_group(&create)), [1]);
        let many: Vec<_> = (1..=MAX_GROUP + 10)
            .map(|n| {
                let uid = Uid::try_from(n).unwrap_or(1);
                op(i64::from(uid), 1, Queued, seen("INBOX", uid, true))
            })
            .collect();
        assert_eq!(next_group(&many).len(), MAX_GROUP);
        assert!(next_group(&[]).is_empty());
    }

    #[test]
    fn refusals_fail_the_operation_and_broken_connections_end_the_step() {
        assert!(refusal(BackendError::Rejected("NO".into())).is_ok());
        assert!(refusal(BackendError::MailboxNotFound("X".into())).is_ok());
        assert!(matches!(
            refusal(BackendError::ConnectionLost("eof".into())),
            Err(StepError::Connection(_))
        ));
        assert!(matches!(
            refusal(BackendError::Timeout("read".into())),
            Err(StepError::Connection(_))
        ));
    }
}
