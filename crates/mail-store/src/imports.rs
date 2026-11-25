//! Stage 19, block C (decision 0057): messages of an imported mbox file appended to a folder
//! through the outbox. Each message is one `append` with frozen bytes (relative to the compose
//! directory), its flags and its Message-ID (an append that may have arrived before a crash is
//! found by it); recorded as `copy` in the activity view. Only for accounts with writes on.

use mail_model::{
    ActionError, ActionKind, AppendSource, Flags, MailboxId, OperationId, OperationPayload,
    OperationState, TargetMailbox,
};
use rusqlite::{Connection, OptionalExtension};

use crate::outbox::{Recorder, repeated, require_operation, require_writes};
use crate::outbox_api::OutboxChange;
use crate::settings::now_millis;
use crate::{Store, StoreError};

/// One message to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportItem {
    /// The frozen bytes, relative to the compose directory.
    pub file: String,
    pub flags: Flags,
    /// Without angle brackets.
    pub message_id_header: String,
}

fn target_of(conn: &Connection, target: MailboxId) -> Result<(TargetMailbox, i64), StoreError> {
    let row: Option<(String, i64, bool)> = conn
        .prepare_cached("SELECT name, account_id, selectable FROM mailbox WHERE id = ?1")?
        .query_row([target.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    match row {
        Some((name, account_row, true)) => Ok((
            TargetMailbox {
                mailbox_id: target,
                name,
            },
            account_row,
        )),
        _ => Err(StoreError::Action(ActionError::InvalidTarget(target))),
    }
}

/// See [`Store::check_import_target`].
pub(crate) fn check_target(conn: &Connection, target: MailboxId) -> Result<(), StoreError> {
    let (_, account_row) = target_of(conn, target)?;
    require_writes(conn, account_row)?;
    Ok(())
}

/// See [`Store::enqueue_import_appends`].
pub(crate) fn enqueue(
    conn: &Connection,
    target: MailboxId,
    items: &[ImportItem],
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let (target, account_row) = target_of(conn, target)?;
    let account = require_writes(conn, account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let mut recorder = Recorder::new(conn, account_row, account, ActionKind::Copy, request, now);
    for item in items {
        recorder.record(OperationPayload::Append {
            target: target.clone(),
            source: AppendSource::Transfer {
                file: item.file.clone(),
                flags: item.flags.clone(),
                remove: None,
            },
            message_id_header: item.message_id_header.clone(),
            draft_flag: false,
            mark: None,
            replace: None,
        })?;
    }
    Ok(recorder.change)
}

/// See [`Store::cancel_import_appends`].
pub(crate) fn cancel_queued(
    conn: &Connection,
    ids: &[OperationId],
) -> Result<Option<OutboxChange>, StoreError> {
    let mut change: Option<OutboxChange> = None;
    for id in ids {
        let Ok(op) = require_operation(conn, *id) else {
            continue;
        };
        if op.state != OperationState::Queued {
            continue;
        }
        conn.prepare_cached("DELETE FROM operation WHERE id = ?1")?
            .execute([id.0])?;
        change
            .get_or_insert_with(|| OutboxChange::new(op.account))
            .removed
            .push(*id);
    }
    Ok(change)
}

/// See [`Store::operation_states`].
pub(crate) fn states(
    conn: &Connection,
    ids: &[OperationId],
) -> Result<Vec<(OperationId, Option<OperationState>)>, StoreError> {
    let mut stmt = conn.prepare_cached("SELECT state FROM operation WHERE id = ?1")?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let state: Option<String> = stmt.query_row([id.0], |r| r.get(0)).optional()?;
        out.push((*id, state.as_deref().and_then(OperationState::parse)));
    }
    Ok(out)
}

/// See [`Store::mailbox_account`].
pub(crate) fn mailbox_account(
    conn: &Connection,
    mailbox: MailboxId,
) -> Result<Option<String>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT a.uuid FROM mailbox m JOIN account a ON a.id = m.account_id WHERE m.id = ?1",
        )?
        .query_row([mailbox.0], |r| r.get(0))
        .optional()?)
}

impl Store {
    /// Stage 19: `read_only` unless the folder's account may write, `invalid_target` for an
    /// unknown or unselectable folder. Checked before an import reads anything.
    pub async fn check_import_target(&self, target: MailboxId) -> Result<(), StoreError> {
        self.read(move |c| check_target(c, target)).await
    }

    /// Stage 19: records one `append` per item into `target` (one batch, action `copy`).
    pub async fn enqueue_import_appends(
        &self,
        target: MailboxId,
        items: Vec<ImportItem>,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        let now = now_millis();
        self.write(move |tx| enqueue(tx, target, &items, &request, now))
            .await
    }

    /// Stage 19: removes those of `ids` that still wait (`queued`); sent ones stay. `None`
    /// when none waited.
    pub async fn cancel_import_appends(
        &self,
        ids: Vec<OperationId>,
    ) -> Result<Option<OutboxChange>, StoreError> {
        self.write(move |tx| cancel_queued(tx, &ids)).await
    }

    /// Stage 19: the account (UUID) of a folder; `None` for an unknown folder.
    pub async fn mailbox_account(&self, mailbox: MailboxId) -> Result<Option<String>, StoreError> {
        self.read(move |c| mailbox_account(c, mailbox)).await
    }

    /// Stage 19: the state of each operation; `None` once it was removed (discarded).
    pub async fn operation_states(
        &self,
        ids: Vec<OperationId>,
    ) -> Result<Vec<(OperationId, Option<OperationState>)>, StoreError> {
        self.read(move |c| states(c, &ids)).await
    }
}

#[cfg(test)]
mod tests {
    use mail_model::{AccountId, ErrorCode};

    use super::*;
    use crate::test_support::fixture;

    #[tokio::test]
    async fn imports_append_with_flags_only_with_writes_and_can_be_cancelled() {
        let f = fixture().await;
        let item = |n: u32| ImportItem {
            file: format!("outgoing/{n:032x}/import.eml"),
            flags: Flags {
                seen: n.is_multiple_of(2),
                flagged: n == 1,
                ..Flags::default()
            },
            message_id_header: format!("import.{n}@example.org"),
        };
        // Read-only: refused before anything is recorded.
        let err = f
            .store
            .enqueue_import_appends(f.inbox, vec![item(1)], "import-a".into())
            .await
            .expect_err("read only");
        assert!(
            matches!(&err, StoreError::Action(e) if e.code() == ErrorCode::ReadOnly),
            "{err:?}"
        );
        assert!(f.store.check_import_target(f.inbox).await.is_err());
        f.store
            .set_writes_enabled(f.account, true)
            .await
            .expect("writes on");
        f.store
            .check_import_target(f.inbox)
            .await
            .expect("may write");
        let change = f
            .store
            .enqueue_import_appends(f.inbox, vec![item(1), item(2)], "import-b".into())
            .await
            .expect("enqueue");
        assert_eq!(change.operations.len(), 2);
        let OperationPayload::Append {
            source: AppendSource::Transfer { flags, remove, .. },
            message_id_header,
            ..
        } = &change.operations[0].payload
        else {
            panic!("an append");
        };
        assert!(flags.flagged && !flags.seen);
        assert!(remove.is_none());
        assert_eq!(message_id_header, "import.1@example.org");
        let ids: Vec<OperationId> = change.operations.iter().map(|o| o.id).collect();
        let states = f.store.operation_states(ids.clone()).await.expect("states");
        assert!(
            states
                .iter()
                .all(|(_, s)| *s == Some(OperationState::Queued))
        );
        let cancelled = f
            .store
            .cancel_import_appends(ids.clone())
            .await
            .expect("cancel")
            .expect("removed");
        assert_eq!(cancelled.removed, ids);
        let states = f.store.operation_states(ids).await.expect("states");
        assert!(states.iter().all(|(_, s)| s.is_none()));
        let _: AccountId = f.account;
    }
}
