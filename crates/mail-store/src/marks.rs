//! Stage 9: snoozes, pins and tasks (decision 0020), and the marks on list rows.
//!
//! The marks refer to a message by account UUID and Message-ID (class sync), never by the
//! local id: they survive a rebuilt mailbox and travel to other devices in stage 11. All
//! messages of an account with that Message-ID carry the mark. A message without a Message-ID
//! cannot be marked ([`StoreError::InvalidArgument`]).

use mail_model::{AccountId, MailboxId, MessageId, MessageListItem, SnoozeMark, TaskMark};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::StoreError;
use crate::accounts::account_id_at;

/// Most rows of the views of pinned, snoozed and task messages.
pub const MAX_MARK_VIEW: u32 = 1_000;

/// The sync identity of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageKey {
    pub account: AccountId,
    /// Without `<>`.
    pub message_id_hdr: String,
}

pub(crate) const KEY_OF: &str = "
    SELECT (SELECT a.uuid FROM account a WHERE a.id = m.account_id), m.message_id_hdr
    FROM message m WHERE m.id = ?1";

pub(crate) fn key_of(conn: &Connection, message: MessageId) -> Result<MessageKey, StoreError> {
    let (account, msgid): (AccountId, Option<String>) = conn
        .prepare_cached(KEY_OF)?
        .query_row([message.0], |r| Ok((account_id_at(r, 0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("message {message}")))?;
    let message_id_hdr = msgid.filter(|m| !m.is_empty()).ok_or_else(|| {
        StoreError::InvalidArgument(format!(
            "message {message} has no Message-ID and cannot be pinned, snoozed or marked as a task"
        ))
    })?;
    Ok(MessageKey {
        account,
        message_id_hdr,
    })
}

// ---------------------------------------------------------------------------------------------
// Snoozes

/// A snooze as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSnooze {
    pub key: MessageKey,
    /// Unix ms.
    pub until: i64,
    pub returned_at: Option<i64>,
    /// The server got the snooze keyword.
    pub keyword: bool,
}

pub(crate) const SNOOZE_OF: &str = "
    SELECT until, returned_at, keyword FROM snooze WHERE account_uuid = ?1 AND message_id_hdr = ?2";

fn snooze_of(conn: &Connection, key: &MessageKey) -> Result<Option<StoredSnooze>, StoreError> {
    Ok(conn
        .prepare_cached(SNOOZE_OF)?
        .query_row(params![key.account.to_string(), key.message_id_hdr], |r| {
            Ok(StoredSnooze {
                key: key.clone(),
                until: r.get(0)?,
                returned_at: r.get(1)?,
                keyword: r.get(2)?,
            })
        })
        .optional()?)
}

/// Snoozes `message` until `until` (replacing an earlier snooze of it). Returns the stored
/// snooze and the previous one.
pub(crate) fn snooze(
    conn: &Connection,
    message: MessageId,
    until: i64,
    now: i64,
) -> Result<(StoredSnooze, Option<StoredSnooze>), StoreError> {
    let key = key_of(conn, message)?;
    let before = snooze_of(conn, &key)?;
    conn.prepare_cached(
        "INSERT INTO snooze (account_uuid, message_id_hdr, until, returned_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, NULL, ?4, ?4)
         ON CONFLICT (account_uuid, message_id_hdr)
         DO UPDATE SET until = excluded.until, returned_at = NULL, updated_at = excluded.updated_at",
    )?
    .execute(params![key.account.to_string(), key.message_id_hdr, until, now])
    .map_err(StoreError::sqlite("store snooze"))?;
    let stored = snooze_of(conn, &key)?
        .ok_or_else(|| StoreError::NotFound(format!("snooze of message {message}")))?;
    Ok((stored, before))
}

/// Removes the snooze of `message` (waiting or returned); the removed one, if any.
pub(crate) fn unsnooze(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<StoredSnooze>, StoreError> {
    let key = key_of(conn, message)?;
    let before = snooze_of(conn, &key)?;
    if before.is_some() {
        conn.prepare_cached("DELETE FROM snooze WHERE account_uuid = ?1 AND message_id_hdr = ?2")?
            .execute(params![key.account.to_string(), key.message_id_hdr])?;
    }
    Ok(before)
}

pub(crate) fn set_snooze_keyword(
    conn: &Connection,
    key: &MessageKey,
    keyword: bool,
) -> Result<(), StoreError> {
    conn.prepare_cached(
        "UPDATE snooze SET keyword = ?3 WHERE account_uuid = ?1 AND message_id_hdr = ?2",
    )?
    .execute(params![
        key.account.to_string(),
        key.message_id_hdr,
        keyword
    ])?;
    Ok(())
}

/// The earliest time a waiting snooze is due, Unix ms.
pub(crate) fn next_due(conn: &Connection) -> Result<Option<i64>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT min(until) FROM snooze WHERE returned_at IS NULL")?
        .query_row([], |r| r.get(0))?)
}

/// A snooze that came back, with the local messages that carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnedSnooze {
    pub snooze: StoredSnooze,
    /// Newest first; empty if the message is not stored (any more).
    pub messages: Vec<MessageId>,
}

pub(crate) const DUE: &str = "
    SELECT account_uuid, message_id_hdr, until, keyword FROM snooze
    WHERE returned_at IS NULL AND until <= ?1 ORDER BY until LIMIT 500";

/// Marks every snooze due at `now` as returned (at `now`) and returns them.
pub(crate) fn return_due(conn: &Connection, now: i64) -> Result<Vec<ReturnedSnooze>, StoreError> {
    let due: Vec<StoredSnooze> = conn
        .prepare_cached(DUE)?
        .query_map([now], |r| {
            Ok(StoredSnooze {
                key: MessageKey {
                    account: account_id_at(r, 0)?,
                    message_id_hdr: r.get(1)?,
                },
                until: r.get(2)?,
                returned_at: Some(now),
                keyword: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(due.len());
    for snooze in due {
        conn.prepare_cached(
            "UPDATE snooze SET returned_at = ?3, updated_at = ?3
             WHERE account_uuid = ?1 AND message_id_hdr = ?2",
        )?
        .execute(params![
            snooze.key.account.to_string(),
            snooze.key.message_id_hdr,
            now
        ])?;
        let messages = messages_of(conn, &snooze.key)?;
        out.push(ReturnedSnooze { snooze, messages });
    }
    Ok(out)
}

pub(crate) const MESSAGES_OF: &str = "
    SELECT m.id FROM message m
    WHERE m.account_id = (SELECT a.id FROM account a WHERE a.uuid = ?1) AND m.message_id_hdr = ?2
    ORDER BY m.id DESC LIMIT 20";

/// Local messages with this sync identity, newest first.
pub(crate) fn messages_of(
    conn: &Connection,
    key: &MessageKey,
) -> Result<Vec<MessageId>, StoreError> {
    Ok(conn
        .prepare_cached(MESSAGES_OF)?
        .query_map(params![key.account.to_string(), key.message_id_hdr], |r| {
            Ok(MessageId(r.get(0)?))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------------------------------------
// Pins and tasks

/// Pins `message`; whether it was not pinned before.
pub(crate) fn pin(conn: &Connection, message: MessageId, now: i64) -> Result<bool, StoreError> {
    let key = key_of(conn, message)?;
    let added = conn
        .prepare_cached(
            "INSERT OR IGNORE INTO pin (account_uuid, message_id_hdr, pinned_at) VALUES (?1, ?2, ?3)",
        )?
        .execute(params![key.account.to_string(), key.message_id_hdr, now])?;
    Ok(added > 0)
}

/// Unpins `message`; whether it was pinned.
pub(crate) fn unpin(conn: &Connection, message: MessageId) -> Result<bool, StoreError> {
    let key = key_of(conn, message)?;
    let removed = conn
        .prepare_cached("DELETE FROM pin WHERE account_uuid = ?1 AND message_id_hdr = ?2")?
        .execute(params![key.account.to_string(), key.message_id_hdr])?;
    Ok(removed > 0)
}

/// A task as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTask {
    pub key: MessageKey,
    pub done_at: Option<i64>,
    pub keyword: bool,
}

pub(crate) const TASK_OF: &str =
    "SELECT done_at, keyword FROM task WHERE account_uuid = ?1 AND message_id_hdr = ?2";

fn task_of(conn: &Connection, key: &MessageKey) -> Result<Option<StoredTask>, StoreError> {
    Ok(conn
        .prepare_cached(TASK_OF)?
        .query_row(params![key.account.to_string(), key.message_id_hdr], |r| {
            Ok(StoredTask {
                key: key.clone(),
                done_at: r.get(0)?,
                keyword: r.get(1)?,
            })
        })
        .optional()?)
}

/// Marks `message` as an open task (`Some(false)`), as done (`Some(true)`) or removes the mark
/// (`None`). Returns the task before and after.
pub(crate) fn set_task(
    conn: &Connection,
    message: MessageId,
    done: Option<bool>,
    now: i64,
) -> Result<(Option<StoredTask>, Option<StoredTask>), StoreError> {
    let key = key_of(conn, message)?;
    let before = task_of(conn, &key)?;
    let account = key.account.to_string();
    match done {
        None => {
            conn.prepare_cached(
                "DELETE FROM task WHERE account_uuid = ?1 AND message_id_hdr = ?2",
            )?
            .execute(params![account, key.message_id_hdr])?;
        }
        Some(done) => {
            let done_at = done.then_some(now);
            conn.prepare_cached(
                "INSERT INTO task (account_uuid, message_id_hdr, done_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT (account_uuid, message_id_hdr)
                 DO UPDATE SET done_at = CASE WHEN excluded.done_at IS NULL THEN NULL
                                              ELSE coalesce(task.done_at, excluded.done_at) END,
                               updated_at = excluded.updated_at",
            )?
            .execute(params![account, key.message_id_hdr, done_at, now])?;
        }
    }
    let after = task_of(conn, &key)?;
    Ok((before, after))
}

pub(crate) fn set_task_keyword(
    conn: &Connection,
    key: &MessageKey,
    keyword: bool,
) -> Result<(), StoreError> {
    conn.prepare_cached(
        "UPDATE task SET keyword = ?3 WHERE account_uuid = ?1 AND message_id_hdr = ?2",
    )?
    .execute(params![
        key.account.to_string(),
        key.message_id_hdr,
        keyword
    ])?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Marks on list rows

pub(crate) const ROW_MARKS: &str = "
    SELECT m.category, m.important,
           EXISTS (SELECT 1 FROM pin p WHERE p.account_uuid = ?2 AND p.message_id_hdr = m.message_id_hdr),
           (SELECT s.until FROM snooze s WHERE s.account_uuid = ?2 AND s.message_id_hdr = m.message_id_hdr),
           (SELECT s.returned_at IS NOT NULL FROM snooze s
            WHERE s.account_uuid = ?2 AND s.message_id_hdr = m.message_id_hdr),
           (SELECT t.done_at IS NOT NULL FROM task t
            WHERE t.account_uuid = ?2 AND t.message_id_hdr = m.message_id_hdr)
    FROM message m WHERE m.id = ?1";

/// Fills the stage-9 marks of list rows: category, important, pinned, snooze, task.
pub(crate) fn decorate(conn: &Connection, items: &mut [MessageListItem]) -> Result<(), StoreError> {
    if items.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(ROW_MARKS)?;
    for item in items.iter_mut() {
        let marks = stmt
            .query_row(params![item.id.0, item.account_id.to_string()], |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<bool>>(4)?,
                    r.get::<_, Option<bool>>(5)?,
                ))
            })
            .optional()?;
        let Some((category, important, pinned, until, returned, done)) = marks else {
            continue;
        };
        item.category = category.and_then(mail_model::MessageCategory::from_code);
        item.important = important;
        item.pinned = pinned;
        item.snooze = until.map(|until| SnoozeMark {
            until,
            returned: returned.unwrap_or(false),
        });
        item.task = done.map(|done| TaskMark { done });
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Views

/// Which marks a view lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkView {
    /// Waiting snoozes, the next due first.
    Snoozed,
    /// Pins, the newest pin first.
    Pinned,
    /// Tasks (open, done or both), the newest first.
    Tasks(mail_model::TaskFilter),
}

pub(crate) fn view_sql(view: MarkView, accounts: usize) -> String {
    let placeholders: Vec<String> = (0..accounts).map(|i| format!("?{}", i + 2)).collect();
    let accounts = placeholders.join(", ");
    match view {
        MarkView::Snoozed => format!(
            "SELECT account_uuid, message_id_hdr FROM snooze
             WHERE returned_at IS NULL AND account_uuid IN ({accounts}) ORDER BY until LIMIT ?1"
        ),
        MarkView::Pinned => format!(
            "SELECT account_uuid, message_id_hdr FROM pin
             WHERE account_uuid IN ({accounts}) ORDER BY pinned_at DESC, id DESC LIMIT ?1"
        ),
        MarkView::Tasks(filter) => {
            let state = match filter {
                mail_model::TaskFilter::Open => " AND done_at IS NULL",
                mail_model::TaskFilter::Done => " AND done_at IS NOT NULL",
                mail_model::TaskFilter::All => "",
            };
            format!(
                "SELECT account_uuid, message_id_hdr FROM task
                 WHERE account_uuid IN ({accounts}){state} ORDER BY created_at DESC, id DESC LIMIT ?1"
            )
        }
    }
}

/// The marked messages of `accounts` as list rows, each in its preferred mailbox (`prefer`,
/// usually the inboxes) where it lies there. Marks of messages not stored are left out.
pub(crate) fn view(
    conn: &Connection,
    view: MarkView,
    accounts: &[AccountId],
    prefer: &[MailboxId],
    limit: u32,
) -> Result<Vec<MessageListItem>, StoreError> {
    if accounts.is_empty() {
        return Ok(Vec::new());
    }
    let mut values: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(i64::from(
        limit.min(MAX_MARK_VIEW),
    ))];
    values.extend(
        accounts
            .iter()
            .map(|a| rusqlite::types::Value::Text(a.to_string())),
    );
    let keys: Vec<MessageKey> = conn
        .prepare_cached(&view_sql(view, accounts.len()))?
        .query_map(params_from_iter(values), |r| {
            Ok(MessageKey {
                account: account_id_at(r, 0)?,
                message_id_hdr: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut items = Vec::with_capacity(keys.len());
    for key in keys {
        for message in messages_of(conn, &key)? {
            if let Some(entry) = crate::lists::entry(conn, message, prefer)? {
                items.push(entry.item);
                break;
            }
        }
    }
    // `entry` filled the marks.
    Ok(items)
}
