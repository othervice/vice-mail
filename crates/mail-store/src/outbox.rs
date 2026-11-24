//! The outbox (stage 4, decision 0012): operations and their local effects. Synchronous
//! functions over a connection; `outbox_api` runs them in one writer transaction each, so the
//! local change and the recorded operation always go together.
//!
//! **Local effects.** A flag operation sets the flag on the location at once. A move or a
//! permanent delete removes the location at once (the message row stays: messages referenced
//! by a pending or confirmed operation are never deleted as orphans); the payload keeps UID and
//! flags to put it back. A confirmed move with COPYUID inserts the location in the target, so
//! the target's next sync finds the UID known; without COPYUID the sync merges the message back
//! by Message-ID (decision 0003).
//!
//! **Sync writes do not undo pending changes.** After `store_headers` and `update_flags` wrote
//! the server's flags, [`reapply_pending_in_mailbox`] applies every pending flag operation of
//! the mailbox again; until the server confirmed it, the user's change wins.
//!
//! **Cancelling.** A new flag operation whose value equals the `before` of a still queued one
//! on the same location and flag removes that one instead of queueing a second (read, then
//! unread before the server saw either: nothing to send).

use std::collections::BTreeSet;

use mail_model::{
    AccountId, ActionError, ActionKind, BatchId, ConflictReason, CoreError, Flag, FlagUpdate,
    Flags, IpcError, MailAction, MailboxId, MailboxRole, MessageId, MessageRef, ModSeq,
    OperationId, OperationNotice, OperationPayload, OperationState, OutboxCounts, PlanLocation,
    PlanMailbox, SourceLocation, TargetMailbox, Uid, UidValidity, plan_action,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::StoreError;
use crate::accounts::{self, account_id_at, conversion_error};
use crate::outbox_api::{OperationResult, OutboxChange, StoredOperation};

// ---------------------------------------------------------------------------------------------
// SQL (every statement has a plan test)

/// A location with what planning needs: flags, mailbox (name, generation, role) and message.
pub(crate) const LOCATION_FOR_PLAN: &str = "
    SELECT l.uid, l.seen, l.flagged, l.answered, l.draft, l.deleted, l.keywords, l.modseq,
           b.account_id, b.name, b.uidvalidity, b.role, m.message_id_hdr, m.size
    FROM message_location l CROSS JOIN mailbox b ON b.id = l.mailbox_id
         CROSS JOIN message m ON m.id = l.message_id
    WHERE l.mailbox_id = ?1 AND l.message_id = ?2";

pub(crate) const ACCOUNT_WRITES: &str = "SELECT uuid, writes_enabled FROM account WHERE id = ?1";

pub(crate) const SET_WRITES: &str = "UPDATE account SET writes_enabled = ?2 WHERE id = ?1";

pub(crate) const PLAN_MAILBOXES: &str =
    "SELECT id, name, role, selectable FROM mailbox WHERE account_id = ?1";

pub(crate) const MAILBOX_GENERATION: &str = "SELECT uidvalidity FROM mailbox WHERE id = ?1";

pub(crate) const MESSAGE_EXISTS: &str = "SELECT 1 FROM message WHERE id = ?1";

pub(crate) const KEY_BATCH: &str = "SELECT batch FROM operation WHERE idempotency_key = ?1";

pub(crate) const INSERT_OPERATION: &str = "
    INSERT INTO operation (account_id, batch, action, kind, mailbox_id, message_id, uid,
                           target_mailbox_id, payload, idempotency_key, state, created_at,
                           updated_at, not_before, draft_id)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued', ?11, ?11, ?12,
            (SELECT d.id FROM draft d WHERE d.uuid = ?13))";

pub(crate) const SET_BATCH: &str = "UPDATE operation SET batch = id WHERE id = ?1";

/// Pending flag operations of one location, in order (`operation_location` holds the rowid, so
/// the order needs no sort).
pub(crate) const PENDING_FLAGS_AT: &str = "
    SELECT id, state, payload FROM operation
    WHERE mailbox_id = ?1 AND uid = ?2 AND kind = 'set_flag'
      AND state IN ('queued', 'in_flight')
    ORDER BY uid, id";

/// Pending flag operations of a mailbox, by UID and then in order.
pub(crate) const PENDING_FLAGS_IN: &str = "
    SELECT id, uid, payload FROM operation
    WHERE mailbox_id = ?1 AND kind = 'set_flag' AND state IN ('queued', 'in_flight')
    ORDER BY uid, id";

pub(crate) const LOCATION_FLAGS: &str = "
    SELECT uid, seen, flagged, answered, draft, deleted, keywords, modseq
    FROM message_location WHERE mailbox_id = ?1 AND message_id = ?2";

pub(crate) const WRITE_FLAGS: &str = "
    UPDATE message_location
    SET seen = ?3, flagged = ?4, answered = ?5, draft = ?6, deleted = ?7, keywords = ?8
    WHERE mailbox_id = ?1 AND message_id = ?2";

pub(crate) const WRITE_SERVER_FLAGS: &str = "
    UPDATE message_location
    SET seen = ?3, flagged = ?4, answered = ?5, draft = ?6, deleted = ?7, keywords = ?8,
        modseq = coalesce(?9, modseq)
    WHERE mailbox_id = ?1 AND uid = ?2";

pub(crate) const DELETE_LOCATION: &str =
    "DELETE FROM message_location WHERE mailbox_id = ?1 AND message_id = ?2";

/// Puts a location back (revert) or adds it in a move's target (COPYUID). A location that is
/// there already (same message or same UID) wins: the sync was faster.
pub(crate) const INSERT_LOCATION_IF_FREE: &str = "
    INSERT OR IGNORE INTO message_location (mailbox_id, message_id, uid, seen, flagged,
                                            answered, draft, deleted, keywords, modseq)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)";

macro_rules! select_operations {
    ($rest:literal) => {
        concat!(
            "SELECT o.id, (SELECT a.uuid FROM account a WHERE a.id = o.account_id), o.batch,
                    o.action, o.payload, o.state, o.attempts, o.last_error, o.conflict,
                    o.notice, o.mailbox_id, o.message_id, o.target_mailbox_id,
                    (SELECT l.modseq FROM message_location l
                      WHERE l.mailbox_id = o.mailbox_id AND l.message_id = o.message_id),
                    o.created_at, o.updated_at, o.not_before
             FROM operation o ",
            $rest
        )
    };
}

pub(crate) const PENDING_OPERATIONS: &str = select_operations!(
    "WHERE o.account_id = ?1 AND o.state IN ('queued', 'in_flight') ORDER BY o.id"
);

pub(crate) const OPERATION_BY_ID: &str = select_operations!("WHERE o.id = ?1");

pub(crate) const OPERATIONS_OF_BATCH: &str = select_operations!("WHERE o.batch = ?1 ORDER BY o.id");

pub(crate) const OPERATIONS_OF_ACCOUNT: &str =
    select_operations!("WHERE o.account_id = ?1 AND o.id < ?2 ORDER BY o.id DESC LIMIT ?3");

/// Stage 5: the account's sends, newest first (`operation_send`), for the outbox view.
pub(crate) const SENDS_OF_ACCOUNT: &str = select_operations!(
    "WHERE o.account_id = ?1 AND o.kind = 'send' AND o.id < ?2 ORDER BY o.id DESC LIMIT ?3"
);

pub(crate) const BEGIN_OPERATION: &str = "
    UPDATE operation SET state = 'in_flight', attempts = attempts + 1, payload = ?2,
                         updated_at = ?3
    WHERE id = ?1 AND state IN ('queued', 'in_flight')";

pub(crate) const FINISH_OPERATION: &str = "
    UPDATE operation SET state = ?2, last_error = ?3, conflict = ?4, notice = ?5, updated_at = ?6
    WHERE id = ?1";

pub(crate) const DELETE_OPERATION: &str = "DELETE FROM operation WHERE id = ?1";

/// Stage 5: an operation that is not done yet goes back to `queued` (or stays `in_flight`),
/// with the error to show, the time of the next try and possibly a new payload.
pub(crate) const LATER_OPERATION: &str = "
    UPDATE operation SET state = ?2, last_error = ?3, not_before = ?4,
                         payload = coalesce(?5, payload), updated_at = ?6
    WHERE id = ?1";

pub(crate) const SET_PAYLOAD: &str = "UPDATE operation SET payload = ?2 WHERE id = ?1";

/// Counts of the account's unfinished operations (status badge, stage 4). Reads the account's
/// rows through `operation_account`; confirmed ones are purged after a day, so they are few.
pub(crate) const OUTBOX_COUNTS: &str = "
    SELECT count(*) FILTER (WHERE state IN ('queued', 'in_flight')),
           count(*) FILTER (WHERE state = 'failed'),
           count(*) FILTER (WHERE state = 'conflict')
    FROM operation WHERE account_id = ?1";

pub(crate) const PURGE_CONFIRMED: &str = "
    DELETE FROM operation WHERE account_id = ?1 AND state = 'confirmed' AND updated_at < ?2";

// ---------------------------------------------------------------------------------------------
// Rows

pub(crate) fn json_error(context: &'static str) -> impl FnOnce(serde_json::Error) -> StoreError {
    move |err| StoreError::Config(format!("{context}: {err}"))
}

pub(crate) fn payload_text(payload: &OperationPayload) -> Result<String, StoreError> {
    serde_json::to_string(payload).map_err(json_error("encode operation payload"))
}

pub(crate) fn payload_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<OperationPayload> {
    let text: String = r.get(idx)?;
    serde_json::from_str(&text).map_err(|err| conversion_error(idx, Type::Text, err.to_string()))
}

fn opt_u32(r: &Row<'_>, idx: usize) -> rusqlite::Result<Option<u32>> {
    r.get(idx)
}

fn modseq_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<Option<ModSeq>> {
    r.get::<_, Option<i64>>(idx)?
        .map(|v| {
            u64::try_from(v)
                .map(ModSeq)
                .map_err(|_| conversion_error(idx, Type::Integer, format!("MODSEQ {v}")))
        })
        .transpose()
}

/// Flags from the columns `seen, flagged, answered, draft, deleted, keywords` at `first`.
fn flags_at(r: &Row<'_>, first: usize) -> rusqlite::Result<Flags> {
    let keywords: Option<String> = r.get(first + 5)?;
    let mut flags = Flags {
        seen: r.get(first)?,
        flagged: r.get(first + 1)?,
        answered: r.get(first + 2)?,
        draft: r.get(first + 3)?,
        deleted: r.get(first + 4)?,
        keywords: keywords
            .map(|k| k.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
    };
    flags.normalize_keywords();
    Ok(flags)
}

fn keywords_text(flags: &Flags) -> Option<String> {
    (!flags.keywords.is_empty()).then(|| flags.keywords.join(" "))
}

pub(crate) fn operation_from_row(r: &Row<'_>) -> rusqlite::Result<StoredOperation> {
    let action: String = r.get(3)?;
    let state: String = r.get(5)?;
    let last_error: Option<String> = r.get(7)?;
    let conflict: Option<String> = r.get(8)?;
    let notice: Option<String> = r.get(9)?;
    let attempts: i64 = r.get(6)?;
    Ok(StoredOperation {
        id: OperationId(r.get(0)?),
        account: account_id_at(r, 1)?,
        batch: BatchId(r.get(2)?),
        action: ActionKind::parse(&action)
            .ok_or_else(|| conversion_error(3, Type::Text, format!("action {action:?}")))?,
        payload: payload_at(r, 4)?,
        state: OperationState::parse(&state)
            .ok_or_else(|| conversion_error(5, Type::Text, format!("state {state:?}")))?,
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        last_error: last_error
            .map(|text| {
                serde_json::from_str::<IpcError>(&text)
                    .map_err(|err| conversion_error(7, Type::Text, err.to_string()))
            })
            .transpose()?,
        conflict: conflict.as_deref().and_then(ConflictReason::parse),
        notice: notice.as_deref().and_then(OperationNotice::parse),
        mailbox_id: r.get::<_, Option<i64>>(10)?.map(MailboxId),
        message_id: r.get::<_, Option<i64>>(11)?.map(MessageId),
        target_mailbox_id: r.get::<_, Option<i64>>(12)?.map(MailboxId),
        current_modseq: modseq_at(r, 13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        not_before: r.get(16)?,
    })
}

pub(crate) fn operation(
    conn: &Connection,
    id: OperationId,
) -> Result<Option<StoredOperation>, StoreError> {
    Ok(conn
        .prepare_cached(OPERATION_BY_ID)?
        .query_row([id.0], operation_from_row)
        .optional()?)
}

pub(crate) fn require_operation(
    conn: &Connection,
    id: OperationId,
) -> Result<StoredOperation, StoreError> {
    operation(conn, id)?.ok_or(StoreError::Action(ActionError::OperationNotFound(id)))
}

pub(crate) fn pending(
    conn: &Connection,
    account: AccountId,
) -> Result<Vec<StoredOperation>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    Ok(conn
        .prepare_cached(PENDING_OPERATIONS)?
        .query_map([row], operation_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn of_account(
    conn: &Connection,
    account: AccountId,
    before: Option<OperationId>,
    limit: u32,
) -> Result<Vec<StoredOperation>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    let before = before.map_or(i64::MAX, |id| id.0);
    Ok(conn
        .prepare_cached(OPERATIONS_OF_ACCOUNT)?
        .query_map(params![row, before, limit], operation_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

/// See [`crate::Store::outbox_counts`].
pub(crate) fn counts(conn: &Connection, account: AccountId) -> Result<OutboxCounts, StoreError> {
    let row = accounts::row_id(conn, account)?;
    let count = |r: &Row<'_>, idx: usize| -> rusqlite::Result<u32> {
        let n: i64 = r.get(idx)?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    };
    Ok(conn.prepare_cached(OUTBOX_COUNTS)?.query_row([row], |r| {
        Ok(OutboxCounts {
            account_id: account,
            pending: count(r, 0)?,
            failed: count(r, 1)?,
            conflict: count(r, 2)?,
        })
    })?)
}

fn of_batch(conn: &Connection, batch: BatchId) -> Result<Vec<StoredOperation>, StoreError> {
    Ok(conn
        .prepare_cached(OPERATIONS_OF_BATCH)?
        .query_map([batch.0], operation_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------------------------------------
// Locations

/// A location read for planning, with its account row.
pub(crate) struct Planned {
    pub(crate) account_row: i64,
    pub(crate) location: PlanLocation,
}

pub(crate) fn location_for_plan(
    conn: &Connection,
    at: MessageRef,
) -> Result<Option<Planned>, StoreError> {
    let found = conn
        .prepare_cached(LOCATION_FOR_PLAN)?
        .query_row(params![at.mailbox_id.0, at.message_id.0], |r| {
            let role: Option<String> = r.get(11)?;
            Ok((
                opt_u32(r, 10)?,
                Planned {
                    account_row: r.get(8)?,
                    location: PlanLocation {
                        source: SourceLocation {
                            mailbox_id: at.mailbox_id,
                            mailbox_name: r.get(9)?,
                            uid_validity: UidValidity(0),
                            uid: r.get(0)?,
                            message_id: at.message_id,
                            message_id_header: r.get(12)?,
                            size: r.get(13)?,
                            flags: flags_at(r, 1)?,
                            // Only the message's own MODSEQ: a location the outbox inserted
                            // after a move has none, and the mailbox's HIGHESTMODSEQ from the
                            // last sync may be older than the moved message.
                            modseq: modseq_at(r, 7)?,
                        },
                        role: MailboxRole::from_db(role.as_deref()),
                    },
                },
            ))
        })
        .optional()?;
    // A location exists only in a mailbox with a generation (`reset_mailbox` sets it first).
    Ok(found.and_then(|(uid_validity, mut planned)| {
        planned.location.source.uid_validity = UidValidity(uid_validity?);
        Some(planned)
    }))
}

/// A location as it is stored now: UID, flags, MODSEQ.
struct Current {
    uid: Uid,
    flags: Flags,
    modseq: Option<ModSeq>,
}

fn location_flags(
    conn: &Connection,
    mailbox: MailboxId,
    message: MessageId,
) -> Result<Option<Current>, StoreError> {
    Ok(conn
        .prepare_cached(LOCATION_FLAGS)?
        .query_row(params![mailbox.0, message.0], |r| {
            Ok(Current {
                uid: r.get(0)?,
                flags: flags_at(r, 1)?,
                modseq: modseq_at(r, 7)?,
            })
        })
        .optional()?)
}

fn write_flags(
    conn: &Connection,
    mailbox: MailboxId,
    message: MessageId,
    flags: &Flags,
) -> Result<(), StoreError> {
    conn.prepare_cached(WRITE_FLAGS)?
        .execute(params![
            mailbox.0,
            message.0,
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft,
            flags.deleted,
            keywords_text(flags),
        ])
        .map_err(StoreError::sqlite("write local flags"))?;
    Ok(())
}

pub(crate) fn generation(
    conn: &Connection,
    mailbox: MailboxId,
) -> Result<Option<UidValidity>, StoreError> {
    Ok(conn
        .prepare_cached(MAILBOX_GENERATION)?
        .query_row([mailbox.0], |r| opt_u32(r, 0))
        .optional()?
        .flatten()
        .map(UidValidity))
}

/// The location of `source` as it is stored now, if it is still the same message at the same
/// UID in the same generation of the mailbox.
fn current_location(
    conn: &Connection,
    source: &SourceLocation,
) -> Result<Option<Current>, StoreError> {
    if generation(conn, source.mailbox_id)? != Some(source.uid_validity) {
        return Ok(None);
    }
    Ok(location_flags(conn, source.mailbox_id, source.message_id)?
        .filter(|current| current.uid == source.uid))
}

fn same_location(conn: &Connection, source: &SourceLocation) -> Result<Option<Flags>, StoreError> {
    Ok(current_location(conn, source)?.map(|current| current.flags))
}

/// `source` refreshed from the stored location: its flags now, and the newest MODSEQ known
/// (an undo copies the source of an operation that was sent since). `None` if it is gone.
fn refreshed(
    conn: &Connection,
    source: &SourceLocation,
) -> Result<Option<SourceLocation>, StoreError> {
    Ok(
        current_location(conn, source)?.map(|current| SourceLocation {
            flags: current.flags,
            modseq: source.modseq.max(current.modseq),
            ..source.clone()
        }),
    )
}

/// Sets `flag` on the location of `source` (if it is still there). Returns whether it changed.
fn set_local_flag(
    conn: &Connection,
    source: &SourceLocation,
    flag: &Flag,
    value: bool,
) -> Result<bool, StoreError> {
    // Stage 14: a Gmail label belongs to the message, not to the location (decision 0034).
    if let Flag::Label(label) = flag {
        return crate::gmail::set_local_label(conn, source, label, value);
    }
    let Some(mut flags) = same_location(conn, source)? else {
        return Ok(false);
    };
    if flag.is_set(&flags) == value {
        return Ok(false);
    }
    flag.apply(&mut flags, value);
    write_flags(conn, source.mailbox_id, source.message_id, &flags)?;
    Ok(true)
}

/// Puts the location of a move or delete back, with the flags it had. Only into the same
/// generation of the mailbox, and only if neither the message nor the UID is there already.
pub(crate) fn reinsert_location(
    conn: &Connection,
    source: &SourceLocation,
) -> Result<bool, StoreError> {
    if generation(conn, source.mailbox_id)? != Some(source.uid_validity) {
        return Ok(false);
    }
    insert_location_if_free(
        conn,
        source.mailbox_id,
        source.message_id,
        source.uid,
        &source.flags,
    )
}

fn insert_location_if_free(
    conn: &Connection,
    mailbox: MailboxId,
    message: MessageId,
    uid: Uid,
    flags: &Flags,
) -> Result<bool, StoreError> {
    let exists: Option<i64> = conn
        .prepare_cached(MESSAGE_EXISTS)?
        .query_row([message.0], |r| r.get(0))
        .optional()?;
    if exists.is_none() {
        return Ok(false);
    }
    let inserted = conn
        .prepare_cached(INSERT_LOCATION_IF_FREE)?
        .execute(params![
            mailbox.0,
            message.0,
            uid,
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft,
            flags.deleted,
            keywords_text(flags),
        ])
        .map_err(StoreError::sqlite("insert location"))?;
    Ok(inserted > 0)
}

/// Applies the pending flag operations of one location again, in order, skipping `except`.
fn reapply_pending_at(
    conn: &Connection,
    mailbox: MailboxId,
    uid: Uid,
    except: Option<OperationId>,
) -> Result<bool, StoreError> {
    let ops = conn
        .prepare_cached(PENDING_FLAGS_AT)?
        .query_map(params![mailbox.0, uid], |r| {
            Ok((OperationId(r.get(0)?), payload_at(r, 2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = false;
    for (id, payload) in ops {
        if Some(id) == except {
            continue;
        }
        if let OperationPayload::SetFlag {
            source,
            flag,
            value,
            ..
        } = &payload
        {
            changed |= set_local_flag(conn, source, flag, *value)?;
        }
    }
    Ok(changed)
}

/// After a sync write: the pending flag operations of `mailbox` win over what the server said
/// until it confirmed them. One index probe when nothing is pending. Returns whether a location
/// changed.
pub(crate) fn reapply_pending_in_mailbox(
    conn: &Connection,
    mailbox: MailboxId,
) -> Result<bool, StoreError> {
    let ops = conn
        .prepare_cached(PENDING_FLAGS_IN)?
        .query_map([mailbox.0], |r| payload_at(r, 2))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = false;
    for payload in ops {
        if let OperationPayload::SetFlag {
            source,
            flag,
            value,
            ..
        } = &payload
        {
            changed |= set_local_flag(conn, source, flag, *value)?;
        }
    }
    Ok(changed)
}

/// Writes flags the server reported (after STORE or a conflict check) to the location with
/// that UID, then the other pending operations of the location on top.
fn adopt_server_flags(
    conn: &Connection,
    source: &SourceLocation,
    update: &FlagUpdate,
    except: OperationId,
) -> Result<(), StoreError> {
    if generation(conn, source.mailbox_id)? != Some(source.uid_validity) {
        return Ok(());
    }
    let modseq = update
        .modseq
        .map(|m| i64::try_from(m.0))
        .transpose()
        .map_err(|_| StoreError::InvalidArgument("MODSEQ out of range".into()))?;
    let mut flags = update.flags.clone();
    flags.normalize_keywords();
    conn.prepare_cached(WRITE_SERVER_FLAGS)?
        .execute(params![
            source.mailbox_id.0,
            update.uid,
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft,
            flags.deleted,
            keywords_text(&flags),
            modseq,
        ])
        .map_err(StoreError::sqlite("write server flags"))?;
    reapply_pending_at(conn, source.mailbox_id, update.uid, Some(except))?;
    Ok(())
}

/// After a confirmed STORE: the other pending flag operations of the message know its new
/// MODSEQ, so their `UNCHANGEDSINCE` does not mistake the outbox's own change for someone
/// else's. Kept in the payloads, because the location may be gone locally (a move queued
/// behind them) and the answer must survive a restart.
fn advance_pending_modseq(
    conn: &Connection,
    source: &SourceLocation,
    modseq: ModSeq,
    except: OperationId,
) -> Result<(), StoreError> {
    let ops = conn
        .prepare_cached(PENDING_FLAGS_AT)?
        .query_map(params![source.mailbox_id.0, source.uid], |r| {
            Ok((OperationId(r.get(0)?), payload_at(r, 2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, mut payload) in ops {
        if id == except {
            continue;
        }
        let OperationPayload::SetFlag { source: other, .. } = &mut payload else {
            continue;
        };
        if other.uid_validity != source.uid_validity || other.modseq >= Some(modseq) {
            continue;
        }
        other.modseq = Some(modseq);
        conn.prepare_cached(SET_PAYLOAD)?
            .execute(params![id.0, payload_text(&payload)?])
            .map_err(StoreError::sqlite("update operation payload"))?;
    }
    Ok(())
}

/// Takes back the local effect of an operation that did not happen (failed, cancelled).
fn revert(conn: &Connection, op: &StoredOperation) -> Result<Option<MailboxId>, StoreError> {
    match &op.payload {
        OperationPayload::SetFlag {
            source,
            flag,
            before,
            ..
        } => {
            let changed = set_local_flag(conn, source, flag, *before)?;
            let reapplied = reapply_pending_at(conn, source.mailbox_id, source.uid, Some(op.id))?;
            Ok((changed || reapplied).then_some(source.mailbox_id))
        }
        OperationPayload::Move { source, .. } | OperationPayload::Expunge { source } => {
            Ok(reinsert_location(conn, source)?.then_some(source.mailbox_id))
        }
        // A failed send keeps its draft frozen (retry sends exactly what failed); cancelling
        // releases it (`compose::release_draft`).
        OperationPayload::CreateMailbox { .. }
        | OperationPayload::Send(_)
        | OperationPayload::Append { .. }
        | OperationPayload::RemoveCopy { .. }
        | OperationPayload::RenameMailbox { .. }
        | OperationPayload::DeleteMailbox { .. } => Ok(None),
    }
}

// ---------------------------------------------------------------------------------------------
// Recording

/// What one recording transaction did.
pub(crate) struct Recorder<'a> {
    conn: &'a Connection,
    account_row: i64,
    action: ActionKind,
    request: &'a str,
    now: i64,
    index: usize,
    pub(crate) change: OutboxChange,
    /// Stage 5: the operations recorded next wait until this time (Unix ms).
    pub(crate) not_before: Option<i64>,
}

impl<'a> Recorder<'a> {
    pub(crate) fn new(
        conn: &'a Connection,
        account_row: i64,
        account: AccountId,
        action: ActionKind,
        request: &'a str,
        now: i64,
    ) -> Self {
        Self {
            conn,
            account_row,
            action,
            request,
            now,
            index: 0,
            change: OutboxChange::new(account),
            not_before: None,
        }
    }

    pub(crate) fn touched(&mut self, mailbox: MailboxId) {
        if !self.change.mailboxes.contains(&mailbox) {
            self.change.mailboxes.push(mailbox);
        }
    }

    pub(crate) fn insert(&mut self, payload: &OperationPayload) -> Result<OperationId, StoreError> {
        let key = format!("{}:{}", self.request, self.index);
        self.index += 1;
        let source = payload.source();
        let batch = self.change.batch.map_or(-1, |b| b.0);
        self.conn
            .prepare_cached(INSERT_OPERATION)?
            .execute(params![
                self.account_row,
                batch,
                self.action.as_str(),
                payload.kind().as_str(),
                // A renamed or deleted folder stands in the source column, for its pending state.
                source
                    .map(|s| s.mailbox_id.0)
                    .or_else(|| payload.changed_mailbox().map(|m| m.mailbox_id.0)),
                source.map(|s| s.message_id.0),
                source.map(|s| s.uid),
                payload.target().map(|t| t.mailbox_id.0),
                payload_text(payload)?,
                key,
                self.now,
                self.not_before,
                payload.draft().map(|d| d.as_str().to_owned()),
            ])
            .map_err(StoreError::sqlite("insert operation"))?;
        let id = OperationId(self.conn.last_insert_rowid());
        if self.change.batch.is_none() {
            self.conn
                .prepare_cached(SET_BATCH)?
                .execute([id.0])
                .map_err(StoreError::sqlite("set batch"))?;
            self.change.batch = Some(BatchId(id.0));
        }
        if let Some(op) = operation(self.conn, id)? {
            self.change.operations.push(op);
        }
        Ok(id)
    }

    /// Records one planned operation with its local effect, or cancels a queued opposite.
    pub(crate) fn record(&mut self, payload: OperationPayload) -> Result<(), StoreError> {
        match payload {
            OperationPayload::SetFlag {
                source,
                flag,
                value,
                ..
            } => self.record_flag(source, flag, value),
            OperationPayload::Move { ref source, .. }
            | OperationPayload::Expunge { ref source } => {
                let mailbox = source.mailbox_id;
                let message = source.message_id;
                self.insert(&payload)?;
                self.conn
                    .prepare_cached(DELETE_LOCATION)?
                    .execute(params![mailbox.0, message.0])
                    .map_err(StoreError::sqlite("remove location"))?;
                self.touched(mailbox);
                Ok(())
            }
            // Stage 5: their local effects (the frozen draft) are written by `compose`. Renamed
            // and deleted folders change locally once the server confirmed.
            OperationPayload::CreateMailbox { .. }
            | OperationPayload::Send(_)
            | OperationPayload::Append { .. }
            | OperationPayload::RemoveCopy { .. }
            | OperationPayload::RenameMailbox { .. }
            | OperationPayload::DeleteMailbox { .. } => {
                self.insert(&payload)?;
                Ok(())
            }
        }
    }

    pub(crate) fn record_flag(
        &mut self,
        mut source: SourceLocation,
        flag: Flag,
        value: bool,
    ) -> Result<(), StoreError> {
        // A queued operation on the same flag: cancel it if this one undoes it.
        let queued = self
            .conn
            .prepare_cached(PENDING_FLAGS_AT)?
            .query_map(params![source.mailbox_id.0, source.uid], |r| {
                let state: String = r.get(1)?;
                Ok((OperationId(r.get(0)?), state, payload_at(r, 2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, state, payload) in queued {
            if state != OperationState::Queued.as_str() {
                continue;
            }
            let OperationPayload::SetFlag {
                flag: other,
                before,
                ..
            } = &payload
            else {
                continue;
            };
            if !other.same(&flag) {
                continue;
            }
            if *before == value {
                self.conn
                    .prepare_cached(DELETE_OPERATION)?
                    .execute([id.0])
                    .map_err(StoreError::sqlite("cancel operation"))?;
                self.change.removed.push(id);
                if set_local_flag(self.conn, &source, &flag, value)? {
                    self.touched(source.mailbox_id);
                }
            }
            // Otherwise the queued one asks for the same value already.
            return Ok(());
        }

        let Some(current) = refreshed(self.conn, &source)? else {
            return Ok(());
        };
        source = current;
        let before = match &flag {
            Flag::Label(label) => crate::gmail::label_state(self.conn, &source, label)?,
            _ => flag.is_set(&source.flags),
        };
        if before == value {
            return Ok(());
        }
        self.insert(&OperationPayload::SetFlag {
            source: source.clone(),
            flag: flag.clone(),
            value,
            before,
        })?;
        if set_local_flag(self.conn, &source, &flag, value)? {
            self.touched(source.mailbox_id);
        }
        Ok(())
    }
}

pub(crate) fn account_writes(
    conn: &Connection,
    account_row: i64,
) -> Result<(AccountId, bool), StoreError> {
    conn.prepare_cached(ACCOUNT_WRITES)?
        .query_row([account_row], |r| Ok((account_id_at(r, 0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("account row {account_row}")))
}

pub(crate) fn require_writes(conn: &Connection, account_row: i64) -> Result<AccountId, StoreError> {
    let (account, writes) = account_writes(conn, account_row)?;
    if !writes {
        return Err(StoreError::Action(ActionError::ReadOnly(account)));
    }
    Ok(account)
}

pub(crate) fn plan_mailboxes(
    conn: &Connection,
    account_row: i64,
) -> Result<Vec<PlanMailbox>, StoreError> {
    let mut boxes = conn
        .prepare_cached(PLAN_MAILBOXES)?
        .query_map([account_row], |r| {
            let role: Option<String> = r.get(2)?;
            Ok(PlanMailbox {
                id: MailboxId(r.get(0)?),
                name: r.get(1)?,
                role: MailboxRole::from_db(role.as_deref()),
                selectable: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    boxes.sort_by_key(|m| m.id);
    Ok(boxes)
}

/// The batch recorded earlier under this request id, if any.
pub(crate) fn repeated(conn: &Connection, request: &str) -> Result<Option<BatchId>, StoreError> {
    Ok(conn
        .prepare_cached(KEY_BATCH)?
        .query_row([format!("{request}:0")], |r| r.get(0))
        .optional()?
        .map(BatchId))
}

/// See [`crate::Store::enqueue_action`].
pub(crate) fn enqueue_action(
    conn: &Connection,
    action: &MailAction,
    refs: &[MessageRef],
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let mut unique: Vec<MessageRef> = Vec::with_capacity(refs.len());
    for r in refs {
        if !unique.contains(r) {
            unique.push(*r);
        }
    }
    let Some(first) = unique.first().copied() else {
        return Err(StoreError::Action(ActionError::NoMessages));
    };

    let mut locations = Vec::with_capacity(unique.len());
    let mut account_row = None;
    for at in &unique {
        let planned = location_for_plan(conn, *at)?
            .ok_or(StoreError::Action(ActionError::MessageNotFound(*at)))?;
        match account_row {
            None => account_row = Some(planned.account_row),
            Some(row) if row != planned.account_row => {
                return Err(StoreError::Action(ActionError::MixedAccounts));
            }
            Some(_) => {}
        }
        locations.push(planned.location);
    }
    let account_row = account_row.ok_or(StoreError::Action(ActionError::MessageNotFound(first)))?;
    let account = require_writes(conn, account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let mailboxes = plan_mailboxes(conn, account_row)?;
    // Stage 14: labels and archive on Gmail (decision 0034).
    let payloads = match crate::gmail::plan(conn, account_row, action, &locations, &mailboxes)? {
        Some(payloads) => payloads,
        None => plan_action(action, &locations, &mailboxes).map_err(StoreError::Action)?,
    };

    let mut recorder = Recorder::new(conn, account_row, account, action.kind(), request, now);
    for payload in payloads {
        recorder.record(payload)?;
    }
    Ok(recorder.change)
}

/// See [`crate::Store::enqueue_create_mailbox`].
pub(crate) fn enqueue_create_mailbox(
    conn: &Connection,
    account: AccountId,
    path: &str,
    role: Option<MailboxRole>,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let path = path.trim();
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(StoreError::InvalidArgument(format!(
            "mailbox path {path:?} is not valid"
        )));
    }
    if role == Some(MailboxRole::Inbox) {
        return Err(StoreError::InvalidArgument(
            "the inbox role cannot be given to a new mailbox".into(),
        ));
    }
    let account_row = accounts::row_id(conn, account)?;
    require_writes(conn, account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let mut recorder = Recorder::new(
        conn,
        account_row,
        account,
        ActionKind::CreateMailbox,
        request,
        now,
    );
    recorder.record(OperationPayload::CreateMailbox {
        path: path.to_owned(),
        role: role.filter(|r| *r != MailboxRole::Custom),
    })?;
    Ok(recorder.change)
}

// ---------------------------------------------------------------------------------------------
// Execution

/// See [`crate::Store::begin_operations`].
pub(crate) fn begin(
    conn: &Connection,
    ops: &[(OperationId, OperationPayload)],
    now: i64,
) -> Result<Vec<OperationId>, StoreError> {
    let mut begun = Vec::with_capacity(ops.len());
    for (id, payload) in ops {
        let updated = conn
            .prepare_cached(BEGIN_OPERATION)?
            .execute(params![id.0, payload_text(payload)?, now])
            .map_err(StoreError::sqlite("begin operation"))?;
        if updated > 0 {
            begun.push(*id);
        }
    }
    Ok(begun)
}

pub(crate) fn error_text(error: &IpcError) -> Result<String, StoreError> {
    serde_json::to_string(error).map_err(json_error("encode operation error"))
}

/// See [`crate::Store::finish_operations`].
pub(crate) fn finish(
    conn: &Connection,
    results: &[(OperationId, OperationResult)],
    now: i64,
) -> Result<Option<OutboxChange>, StoreError> {
    let mut change: Option<OutboxChange> = None;
    let mut touched = BTreeSet::new();
    let mut finished = Vec::new();
    // Stage 5: what the follow-ups of a send recorded (new operations, drafts).
    let mut extra: Option<OutboxChange> = None;
    for (id, result) in results {
        let Some(op) = operation(conn, *id)? else {
            continue;
        };
        if !op.state.is_pending() {
            continue;
        }
        change.get_or_insert_with(|| OutboxChange::new(op.account));
        if let OperationResult::Later {
            keep_in_flight,
            error,
            not_before,
            payload,
        } = result
        {
            let state = if *keep_in_flight {
                OperationState::InFlight
            } else {
                OperationState::Queued
            };
            conn.prepare_cached(LATER_OPERATION)?
                .execute(params![
                    id.0,
                    state.as_str(),
                    error.as_ref().map(error_text).transpose()?,
                    not_before,
                    payload.as_deref().map(payload_text).transpose()?,
                    now,
                ])
                .map_err(StoreError::sqlite("postpone operation"))?;
            finished.push(*id);
            continue;
        }
        let (state, error, conflict, notice) = match result {
            OperationResult::Confirmed {
                flags,
                moved_to,
                notice,
            } => {
                match &op.payload {
                    OperationPayload::SetFlag { source, .. } => {
                        if let Some(update) = flags {
                            adopt_server_flags(conn, source, update, op.id)?;
                            if let Some(modseq) = update.modseq {
                                advance_pending_modseq(conn, source, modseq, op.id)?;
                            }
                            touched.insert(source.mailbox_id);
                        }
                    }
                    OperationPayload::Move { source, target, .. } => {
                        if let Some((uv, uid)) = moved_to
                            && generation(conn, target.mailbox_id)? == Some(*uv)
                            && insert_location_if_free(
                                conn,
                                target.mailbox_id,
                                source.message_id,
                                *uid,
                                &source.flags,
                            )?
                        {
                            touched.insert(target.mailbox_id);
                        }
                    }
                    OperationPayload::Expunge { .. }
                    | OperationPayload::CreateMailbox { .. }
                    | OperationPayload::Send(_)
                    | OperationPayload::Append { .. }
                    | OperationPayload::RemoveCopy { .. }
                    | OperationPayload::RenameMailbox { .. }
                    | OperationPayload::DeleteMailbox { .. } => {}
                }
                (OperationState::Confirmed, None, None, *notice)
            }
            OperationResult::Sent { copy_on_server } => {
                let extra = extra.get_or_insert_with(|| OutboxChange::new(op.account));
                let notice =
                    crate::compose::send_followups(conn, &op, *copy_on_server, now, extra)?;
                (OperationState::Confirmed, None, None, notice)
            }
            OperationResult::Appended { copy } => {
                let extra = extra.get_or_insert_with(|| OutboxChange::new(op.account));
                crate::compose::appended(conn, &op, copy.as_ref(), now, extra)?;
                // Stage 16: a move between accounts removes its source now.
                if let Some(other) =
                    crate::parity::transfer_appended(conn, &op, copy.as_ref(), now)?
                {
                    extra.others.push(other);
                }
                (OperationState::Confirmed, None, None, None)
            }
            OperationResult::Later { .. } => continue,
            OperationResult::Failed { error } => {
                if let Some(mailbox) = revert(conn, &op)? {
                    touched.insert(mailbox);
                }
                (OperationState::Failed, Some(error.clone()), None, None)
            }
            OperationResult::Conflict { reason, flags } => {
                if let OperationPayload::SetFlag { source, .. } = &op.payload {
                    match flags {
                        Some(update) => adopt_server_flags(conn, source, update, op.id)?,
                        None => {
                            revert(conn, &op)?;
                        }
                    }
                    touched.insert(source.mailbox_id);
                }
                // Moves and deletes: the location stays removed, the server has it elsewhere
                // or not at all; the next sync shows where.
                (OperationState::Conflict, None, Some(*reason), None)
            }
        };
        conn.prepare_cached(FINISH_OPERATION)?
            .execute(params![
                id.0,
                state.as_str(),
                error.as_ref().map(error_text).transpose()?,
                conflict.map(ConflictReason::as_str),
                notice.map(OperationNotice::as_str),
                now,
            ])
            .map_err(StoreError::sqlite("finish operation"))?;
        finished.push(*id);
    }
    let Some(mut change) = change else {
        return Ok(None);
    };
    for id in finished {
        if let Some(op) = operation(conn, id)? {
            change.operations.push(op);
        }
    }
    if let Some(extra) = extra {
        touched.extend(extra.mailboxes.iter().copied());
        change.operations.extend(extra.operations);
        change.removed.extend(extra.removed);
        change.drafts.extend(extra.drafts);
        change.drafts_removed.extend(extra.drafts_removed);
        change.others.extend(extra.others);
    }
    change.mailboxes = touched.into_iter().collect();
    Ok(Some(change))
}

// ---------------------------------------------------------------------------------------------
// Undo, retry, discard, writes

/// See [`crate::Store::undo_batch`].
pub(crate) fn undo(
    conn: &Connection,
    batch: BatchId,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let ops = of_batch(conn, batch)?;
    let Some(first) = ops.first() else {
        return Err(StoreError::Action(ActionError::OperationNotFound(
            OperationId(batch.0),
        )));
    };
    let account = first.account;
    let account_row = accounts::row_id(conn, account)?;
    if let Some(earlier) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(earlier);
        change.repeated = true;
        return Ok(change);
    }

    // Plan first: one operation that cannot be undone refuses the whole undo.
    enum Step {
        Cancel(StoredOperation),
        Inverse(OperationPayload),
    }
    let mut steps = Vec::new();
    for op in ops.iter().rev() {
        match (op.state, &op.payload) {
            (OperationState::Failed | OperationState::Conflict, _) => {}
            (OperationState::Queued, _) => steps.push(Step::Cancel(op.clone())),
            (
                OperationState::InFlight | OperationState::Confirmed,
                OperationPayload::SetFlag {
                    source,
                    flag,
                    value,
                    before,
                },
            ) => {
                if value != before {
                    // Stage 14: undoing a Gmail archive goes through "All Mail".
                    let source = crate::gmail::inverse_source(conn, source, flag, *value)?
                        .unwrap_or_else(|| source.clone());
                    steps.push(Step::Inverse(OperationPayload::SetFlag {
                        source,
                        flag: flag.clone(),
                        value: *before,
                        before: *value,
                    }));
                }
            }
            (OperationState::Confirmed, OperationPayload::Move { source, target, .. }) => {
                let at = MessageRef {
                    mailbox_id: target.mailbox_id,
                    message_id: source.message_id,
                };
                let planned = location_for_plan(conn, at)?.ok_or_else(|| {
                    StoreError::Action(ActionError::NotUndoable(
                        "the moved message is not in the target folder yet; try again after \
                         the next sync"
                            .into(),
                    ))
                })?;
                if generation(conn, source.mailbox_id)?.is_none() {
                    return Err(StoreError::Action(ActionError::NotUndoable(
                        "the original folder no longer exists".into(),
                    )));
                }
                steps.push(Step::Inverse(OperationPayload::Move {
                    source: planned.location.source,
                    target: TargetMailbox {
                        mailbox_id: source.mailbox_id,
                        name: source.mailbox_name.clone(),
                    },
                    mark: None,
                }));
            }
            (OperationState::InFlight, OperationPayload::Move { .. }) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "the move is being sent right now; try again in a moment".into(),
                )));
            }
            (_, OperationPayload::Expunge { .. }) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "the message was deleted for good".into(),
                )));
            }
            (_, OperationPayload::CreateMailbox { .. }) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "a created folder is not removed automatically".into(),
                )));
            }
            (_, OperationPayload::RenameMailbox { .. }) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "a renamed folder is not renamed back automatically".into(),
                )));
            }
            (_, OperationPayload::DeleteMailbox { .. }) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "a deleted folder cannot be brought back".into(),
                )));
            }
            (_, OperationPayload::Send(_)) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "the message is being sent or was sent".into(),
                )));
            }
            (_, OperationPayload::Append { .. } | OperationPayload::RemoveCopy { .. }) => {
                return Err(StoreError::Action(ActionError::NotUndoable(
                    "a copy on the server is not changed back".into(),
                )));
            }
        }
    }

    let has_inverse = steps.iter().any(|s| matches!(s, Step::Inverse(_)));
    if has_inverse {
        require_writes(conn, account_row)?;
    }
    let mut recorder = Recorder::new(conn, account_row, account, ActionKind::Undo, request, now);
    for step in steps {
        match step {
            Step::Cancel(op) => {
                if let Some(mailbox) = revert(conn, &op)? {
                    recorder.touched(mailbox);
                }
                conn.prepare_cached(DELETE_OPERATION)?
                    .execute([op.id.0])
                    .map_err(StoreError::sqlite("cancel operation"))?;
                recorder.change.removed.push(op.id);
                crate::compose::release_draft(conn, &op, now, &mut recorder.change)?;
            }
            Step::Inverse(payload) => recorder.record(payload)?,
        }
    }
    Ok(recorder.change)
}

/// See [`crate::Store::retry_operation`].
pub(crate) fn retry(
    conn: &Connection,
    id: OperationId,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let op = require_operation(conn, id)?;
    if !matches!(op.state, OperationState::Failed | OperationState::Conflict) {
        return Err(StoreError::Action(ActionError::WrongState(id, op.state)));
    }
    let account_row = accounts::row_id(conn, op.account)?;
    require_writes(conn, account_row)?;
    let gone = |source: &SourceLocation| {
        StoreError::Action(ActionError::MessageNotFound(MessageRef {
            mailbox_id: source.mailbox_id,
            message_id: source.message_id,
        }))
    };
    // The message must still be where the operation found it; the payload is refreshed.
    let payload = match op.payload.clone() {
        OperationPayload::SetFlag {
            source,
            flag,
            value,
            ..
        } => {
            let source = refreshed(conn, &source)?.ok_or_else(|| gone(&source))?;
            OperationPayload::SetFlag {
                before: flag.is_set(&source.flags),
                source,
                flag,
                value,
            }
        }
        OperationPayload::Move {
            mut source, target, ..
        } => {
            source.flags = same_location(conn, &source)?.ok_or_else(|| gone(&source))?;
            OperationPayload::Move {
                source,
                target,
                mark: None,
            }
        }
        OperationPayload::Expunge { mut source } => {
            source.flags = same_location(conn, &source)?.ok_or_else(|| gone(&source))?;
            OperationPayload::Expunge { source }
        }
        create @ OperationPayload::CreateMailbox { .. } => create,
        folder @ (OperationPayload::RenameMailbox { .. }
        | OperationPayload::DeleteMailbox { .. }) => folder,
        OperationPayload::Send(mut job) => {
            // Unclear sends, and scheduled sends missed while the app was off (stage 10), are
            // only sent again on the user's explicit decision.
            let unclear = op.last_error.as_ref().is_some_and(|e| {
                matches!(
                    e.code,
                    mail_model::ErrorCode::SendUnclear | mail_model::ErrorCode::SendMissed
                )
            });
            if unclear {
                return Err(StoreError::Draft(mail_model::DraftError::NeedsDecision(id)));
            }
            // Stage 16: a read receipt has no draft.
            if job.receipt_for.is_none() {
                crate::compose::require_frozen(conn, &job.draft)?;
            }
            job.phase = mail_model::SendPhase::Submit;
            job.checks = 0;
            OperationPayload::Send(job)
        }
        OperationPayload::Append {
            target,
            source,
            message_id_header,
            draft_flag,
            ..
        } => OperationPayload::Append {
            target,
            source: match source {
                mail_model::AppendSource::Draft { draft, .. } => mail_model::AppendSource::Draft {
                    draft,
                    revision: None,
                },
                file @ (mail_model::AppendSource::File { .. }
                | mail_model::AppendSource::Transfer { .. }) => file,
            },
            message_id_header,
            draft_flag,
            mark: None,
            replace: None,
        },
        remove @ OperationPayload::RemoveCopy { .. } => remove,
    };
    // Queued again at the end, so it runs after what the user did since; same batch.
    conn.prepare_cached(DELETE_OPERATION)?
        .execute([id.0])
        .map_err(StoreError::sqlite("replace operation"))?;
    let mut recorder = Recorder::new(conn, account_row, op.account, op.action, request, now);
    recorder.change.batch = Some(op.batch);
    recorder.change.removed.push(id);
    match payload {
        OperationPayload::SetFlag {
            source,
            flag,
            value,
            before,
        } => {
            if before != value {
                recorder.insert(&OperationPayload::SetFlag {
                    source: source.clone(),
                    flag: flag.clone(),
                    value,
                    before,
                })?;
                if set_local_flag(conn, &source, &flag, value)? {
                    recorder.touched(source.mailbox_id);
                }
            }
        }
        other => recorder.record(other)?,
    }
    Ok(recorder.change)
}

/// See [`crate::Store::discard_operation`].
pub(crate) fn discard(conn: &Connection, id: OperationId) -> Result<OutboxChange, StoreError> {
    let op = require_operation(conn, id)?;
    if op.state.is_pending() {
        return Err(StoreError::Action(ActionError::WrongState(id, op.state)));
    }
    conn.prepare_cached(DELETE_OPERATION)?
        .execute([id.0])
        .map_err(StoreError::sqlite("discard operation"))?;
    let mut change = OutboxChange::new(op.account);
    change.removed.push(id);
    // A discarded send gives its draft back for editing.
    if !matches!(op.state, OperationState::Confirmed) {
        crate::compose::release_draft(conn, &op, now_ms(), &mut change)?;
    }
    Ok(change)
}

fn now_ms() -> i64 {
    crate::settings::now_millis()
}

/// See [`crate::Store::set_writes_enabled`].
pub(crate) fn set_writes(
    conn: &Connection,
    account: AccountId,
    enabled: bool,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let row = accounts::row_id(conn, account)?;
    conn.prepare_cached(SET_WRITES)?
        .execute(params![row, enabled])
        .map_err(StoreError::sqlite("set writes_enabled"))?;
    if enabled {
        return Ok(OutboxChange::new(account));
    }
    let error = IpcError::from(CoreError::Action(ActionError::ReadOnly(account)));
    let queued: Vec<(OperationId, OperationResult)> = pending(conn, account)?
        .into_iter()
        .filter(|op| op.state == OperationState::Queued)
        .map(|op| {
            (
                op.id,
                OperationResult::Failed {
                    error: error.clone(),
                },
            )
        })
        .collect();
    Ok(finish(conn, &queued, now)?.unwrap_or_else(|| OutboxChange::new(account)))
}

/// See [`crate::Store::purge_operations`].
pub(crate) fn purge(
    conn: &Connection,
    account: AccountId,
    before: i64,
) -> Result<usize, StoreError> {
    let row = accounts::row_id(conn, account)?;
    conn.prepare_cached(PURGE_CONFIRMED)?
        .execute(params![row, before])
        .map_err(StoreError::sqlite("purge operations"))
}
