//! Stage 20, task B (decision 0059): reminders when no reply comes.
//!
//! A reminder refers to a message the user sent by account UUID and Message-ID (class sync,
//! kind `followUp`), like a snooze. It is *answered* when the conversation (stage 7 threading:
//! `thread_ref` names the thread of every Message-ID its messages carry or refer to) has a
//! message from someone else than the user (none of the account and identity addresses) that
//! is newer than the sending, in any folder of the account. An answered reminder is removed
//! (and the removal synced); a due one without an answer comes back above the inbox.
//!
//! The answer check is a lookup of the Message-ID in `thread_ref` and a range of
//! `message_thread` per reminder; reminders are few (one per message the user wants an answer
//! to), so [`settle`] looks at all of them.

use std::collections::HashSet;

use mail_model::{AccountId, MailboxId, MessageId, MessageListItem};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde_json::{Value, json};

use crate::StoreError;
use crate::accounts::account_id_at;
use crate::marks::{MessageKey, key_of, messages_of};

/// Most rows of the "Waiting for reply" view.
pub const MAX_FOLLOW_UP_VIEW: u32 = 1_000;

/// A reminder as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFollowUp {
    pub key: MessageKey,
    /// Unix ms.
    pub due: i64,
    /// Unix ms: when the message was sent.
    pub sent_at: i64,
    pub returned_at: Option<i64>,
}

const COLUMNS: &str = "account_uuid, message_id_hdr, due, sent_at, returned_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredFollowUp> {
    Ok(StoredFollowUp {
        key: MessageKey {
            account: account_id_at(r, 0)?,
            message_id_hdr: r.get(1)?,
        },
        due: r.get(2)?,
        sent_at: r.get(3)?,
        returned_at: r.get(4)?,
    })
}

pub(crate) fn of_key(
    conn: &Connection,
    key: &MessageKey,
) -> Result<Option<StoredFollowUp>, StoreError> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {COLUMNS} FROM follow_up WHERE account_uuid = ?1 AND message_id_hdr = ?2"
        ))?
        .query_row(params![key.account.to_string(), key.message_id_hdr], row)
        .optional()?)
}

fn upsert(
    conn: &Connection,
    key: &MessageKey,
    due: i64,
    sent_at: i64,
    now: i64,
) -> Result<(), StoreError> {
    conn.prepare_cached(
        "INSERT INTO follow_up (account_uuid, message_id_hdr, due, sent_at, returned_at,
                                created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5)
         ON CONFLICT (account_uuid, message_id_hdr)
         DO UPDATE SET due = excluded.due, sent_at = excluded.sent_at, returned_at = NULL,
                       updated_at = excluded.updated_at",
    )?
    .execute(params![
        key.account.to_string(),
        key.message_id_hdr,
        due,
        sent_at,
        now
    ])
    .map_err(StoreError::sqlite("store reminder"))?;
    Ok(())
}

/// Sets (or moves) the reminder of a stored message to `due`; it waits again. The sending time
/// is the message's date (kept when the reminder only moves). Returns the reminder and the one
/// before.
pub(crate) fn set(
    conn: &Connection,
    message: MessageId,
    due: i64,
    now: i64,
) -> Result<(StoredFollowUp, Option<StoredFollowUp>), StoreError> {
    let key = key_of(conn, message)?;
    let before = of_key(conn, &key)?;
    let sent_at = match &before {
        Some(before) => before.sent_at,
        None => conn
            .prepare_cached("SELECT date FROM message WHERE id = ?1")?
            .query_row([message.0], |r| r.get::<_, i64>(0))
            .optional()?
            .map_or(now, |date| date.saturating_mul(1000)),
    };
    upsert(conn, &key, due, sent_at, now)?;
    let stored = of_key(conn, &key)?
        .ok_or_else(|| StoreError::NotFound(format!("reminder of message {message}")))?;
    Ok((stored, before))
}

/// Records the reminder of a message that was just sent (the transaction that confirms the
/// send). The message need not be stored yet.
pub(crate) fn record_sent(
    conn: &Connection,
    account_row: i64,
    message_id_hdr: &str,
    due: i64,
    now: i64,
) -> Result<(), StoreError> {
    let account: Option<AccountId> = conn
        .prepare_cached("SELECT uuid FROM account WHERE id = ?1")?
        .query_row([account_row], |r| account_id_at(r, 0))
        .optional()?;
    let Some(account) = account else {
        return Ok(());
    };
    let key = MessageKey {
        account,
        message_id_hdr: message_id_hdr.to_owned(),
    };
    upsert(conn, &key, due, now, now)
}

/// Removes the reminder of a message (waiting or returned); the removed one, if any.
pub(crate) fn remove(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<StoredFollowUp>, StoreError> {
    let key = key_of(conn, message)?;
    let before = of_key(conn, &key)?;
    if before.is_some() {
        remove_key(conn, &key)?;
    }
    Ok(before)
}

fn remove_key(conn: &Connection, key: &MessageKey) -> Result<(), StoreError> {
    conn.prepare_cached("DELETE FROM follow_up WHERE account_uuid = ?1 AND message_id_hdr = ?2")?
        .execute(params![key.account.to_string(), key.message_id_hdr])
        .map_err(StoreError::sqlite("remove reminder"))?;
    Ok(())
}

/// The earliest time a waiting reminder is due, Unix ms.
pub(crate) fn next_due(conn: &Connection) -> Result<Option<i64>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT min(due) FROM follow_up WHERE returned_at IS NULL")?
        .query_row([], |r| r.get(0))?)
}

/// The addresses of the user (accounts and identities), lower case.
fn own_addresses(conn: &Connection) -> Result<HashSet<String>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT lower(trim(email)) FROM account UNION SELECT lower(trim(email)) FROM identity",
        )?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?)
}

pub(crate) const ANSWERS: &str = "
    SELECT r.from_addr FROM thread_ref t CROSS JOIN message r ON r.thread_id = t.thread_id
    WHERE t.account_id = (SELECT a.id FROM account a WHERE a.uuid = ?1) AND t.ref_id = ?2
      AND r.id >= ?3 AND (r.message_id_hdr IS NULL OR r.message_id_hdr != ?2)
    LIMIT 200";

/// Whether the conversation of `follow_up` has an answer: a message from someone else, sent
/// after it (message ids are sort dates in seconds).
fn answered(
    conn: &Connection,
    follow_up: &StoredFollowUp,
    own: &HashSet<String>,
) -> Result<bool, StoreError> {
    let senders = conn
        .prepare_cached(ANSWERS)?
        .query_map(
            params![
                follow_up.key.account.to_string(),
                follow_up.key.message_id_hdr,
                follow_up.sent_at.div_euclid(1000)
            ],
            |r| r.get::<_, Option<String>>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(senders.into_iter().any(|sender| {
        sender.is_some_and(|s| {
            let s = s.trim().to_lowercase();
            !s.is_empty() && !own.contains(&s)
        })
    }))
}

/// What [`settle`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettledFollowUps {
    /// Due without an answer: they came back (at `now`), with the local messages that carry
    /// them (newest first).
    pub returned: Vec<(StoredFollowUp, Vec<MessageId>)>,
    /// An answer came: removed.
    pub answered: Vec<StoredFollowUp>,
}

/// Which reminders [`settle`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleScope {
    /// Only the waiting ones due at `now` (the timer).
    Due,
    /// Every reminder of the account (new mail arrived there).
    Account(AccountId),
    /// Every reminder.
    All,
}

/// Removes the reminders of `scope` whose conversation got an answer, and brings back the
/// waiting ones due at `now` without one.
pub(crate) fn settle(
    conn: &Connection,
    now: i64,
    scope: SettleScope,
) -> Result<SettledFollowUps, StoreError> {
    let all: Vec<StoredFollowUp> = match scope {
        SettleScope::Due => conn
            .prepare_cached(&format!(
                "SELECT {COLUMNS} FROM follow_up INDEXED BY follow_up_due
                 WHERE returned_at IS NULL AND due <= ?1 ORDER BY due LIMIT 500"
            ))?
            .query_map([now], row)?
            .collect::<Result<Vec<_>, _>>()?,
        SettleScope::Account(account) => conn
            .prepare_cached(&format!(
                "SELECT {COLUMNS} FROM follow_up WHERE account_uuid = ?1"
            ))?
            .query_map([account.to_string()], row)?
            .collect::<Result<Vec<_>, _>>()?,
        SettleScope::All => conn
            .prepare_cached(&format!("SELECT {COLUMNS} FROM follow_up"))?
            .query_map([], row)?
            .collect::<Result<Vec<_>, _>>()?,
    };
    let mut out = SettledFollowUps::default();
    if all.is_empty() {
        return Ok(out);
    }
    let own = own_addresses(conn)?;
    for follow_up in all {
        if answered(conn, &follow_up, &own)? {
            remove_key(conn, &follow_up.key)?;
            out.answered.push(follow_up);
        } else if follow_up.returned_at.is_none() && follow_up.due <= now {
            conn.prepare_cached(
                "UPDATE follow_up SET returned_at = ?3, updated_at = ?3
                 WHERE account_uuid = ?1 AND message_id_hdr = ?2",
            )?
            .execute(params![
                follow_up.key.account.to_string(),
                follow_up.key.message_id_hdr,
                now
            ])?;
            let messages = messages_of(conn, &follow_up.key)?;
            out.returned.push((
                StoredFollowUp {
                    returned_at: Some(now),
                    ..follow_up
                },
                messages,
            ));
        }
    }
    Ok(out)
}

/// Whether `account` has any reminder (cheap check before [`settle`] after new mail).
pub(crate) fn any_of(conn: &Connection, account: AccountId) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached("SELECT EXISTS (SELECT 1 FROM follow_up WHERE account_uuid = ?1)")?
        .query_row([account.to_string()], |r| r.get(0))?)
}

/// The Sent mailboxes of `accounts` (where a reminded message is shown when it lies there).
fn sent_mailboxes(conn: &Connection, accounts: &[AccountId]) -> Result<Vec<MailboxId>, StoreError> {
    let mut out = Vec::new();
    let mut stmt = conn.prepare_cached(
        "SELECT m.id FROM mailbox m
         WHERE m.account_id = (SELECT a.id FROM account a WHERE a.uuid = ?1) AND m.role = 'sent'",
    )?;
    for account in accounts {
        for id in stmt.query_map([account.to_string()], |r| r.get::<_, i64>(0))? {
            out.push(MailboxId(id?));
        }
    }
    Ok(out)
}

/// The reminders of `accounts` as list rows, each at its location in Sent where it lies there:
/// `returned` only the ones that came back (most recently first), else all (waiting by due,
/// then the returned ones). Reminders of messages not stored (yet) are left out.
pub(crate) fn rows(
    conn: &Connection,
    accounts: &[AccountId],
    returned: bool,
    limit: u32,
) -> Result<Vec<MessageListItem>, StoreError> {
    if accounts.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..accounts.len()).map(|i| format!("?{}", i + 2)).collect();
    let filter = if returned {
        " AND returned_at IS NOT NULL ORDER BY returned_at DESC, due DESC"
    } else {
        " ORDER BY returned_at IS NOT NULL, due, returned_at DESC"
    };
    let sql = format!(
        "SELECT account_uuid, message_id_hdr FROM follow_up
         WHERE account_uuid IN ({}){filter} LIMIT ?1",
        placeholders.join(", ")
    );
    let mut values: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(i64::from(
        limit.min(MAX_FOLLOW_UP_VIEW),
    ))];
    values.extend(
        accounts
            .iter()
            .map(|a| rusqlite::types::Value::Text(a.to_string())),
    );
    let keys: Vec<MessageKey> = conn
        .prepare_cached(&sql)?
        .query_map(params_from_iter(values), |r| {
            Ok(MessageKey {
                account: account_id_at(r, 0)?,
                message_id_hdr: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let prefer = sent_mailboxes(conn, accounts)?;
    let mut items = Vec::with_capacity(keys.len());
    for key in keys {
        for message in messages_of(conn, &key)? {
            if let Some(entry) = crate::lists::entry(conn, message, &prefer)? {
                items.push(entry.item);
                break;
            }
        }
    }
    Ok(items)
}

/// The mark of a list row: due, sent and whether it came back, as JSON from
/// [`crate::marks::ROW_MARKS`].
pub(crate) fn mark_from_json(json: Option<&str>) -> Option<mail_model::FollowUpMark> {
    let value: Value = serde_json::from_str(json?).ok()?;
    let parts = value.as_array()?;
    Some(mail_model::FollowUpMark {
        due: parts.first()?.as_i64()?,
        sent_at: parts.get(1)?.as_i64()?,
        returned: parts.get(2)?.as_i64()? != 0,
    })
}

// ---------------------------------------------------------------------------------------------
// Settings sync (kind `followUp`, id `<account>/<Message-ID>`)

/// The content of a `followUp` record as stored here, `None` when there is none.
pub(crate) fn synced_content(conn: &Connection, id: &str) -> Result<Option<Value>, StoreError> {
    let Some((account, message_id)) = id.split_once('/') else {
        return Ok(None);
    };
    Ok(conn
        .prepare_cached(
            "SELECT due, sent_at FROM follow_up WHERE account_uuid = ?1 AND message_id_hdr = ?2",
        )?
        .query_row([account, message_id], |r| {
            Ok(json!({ "dueMs": r.get::<_, i64>(0)?, "sentAtMs": r.get::<_, i64>(1)? }))
        })
        .optional()?)
}

/// Writes (`Some`) or deletes (`None`) the row of a `followUp` record. A moved time waits again;
/// the same time keeps whether it came back on this device. Rows changed.
pub(crate) fn apply_synced(
    conn: &Connection,
    id: &str,
    value: Option<&Value>,
    now: i64,
) -> Result<usize, StoreError> {
    let bad = |what: &str| StoreError::InvalidArgument(format!("followUp record: {what}"));
    let (account, message_id) = id.split_once('/').ok_or_else(|| bad("id"))?;
    let Some(value) = value else {
        return Ok(conn
            .prepare_cached(
                "DELETE FROM follow_up WHERE account_uuid = ?1 AND message_id_hdr = ?2",
            )?
            .execute([account, message_id])?);
    };
    let due = value
        .get("dueMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| bad("dueMs"))?;
    let sent_at = value
        .get("sentAtMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| bad("sentAtMs"))?;
    Ok(conn
        .prepare_cached(
            "INSERT INTO follow_up (account_uuid, message_id_hdr, due, sent_at, returned_at,
                                    created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5)
             ON CONFLICT (account_uuid, message_id_hdr) DO UPDATE
               SET returned_at = CASE WHEN follow_up.due = excluded.due
                                      THEN follow_up.returned_at END,
                   due = excluded.due, sent_at = excluded.sent_at,
                   updated_at = excluded.updated_at",
        )?
        .execute(params![account, message_id, due, sent_at, now])?)
}

impl crate::Store {
    /// Sets (or moves) the reminder of a message; it waits again. Returns it and the one
    /// before. `invalid_argument` without a Message-ID.
    pub async fn set_follow_up(
        &self,
        message: MessageId,
        due: i64,
        now: i64,
    ) -> Result<(StoredFollowUp, Option<StoredFollowUp>), StoreError> {
        self.write(move |tx| set(tx, message, due, now)).await
    }

    /// Removes the reminder of a message; the removed one.
    pub async fn remove_follow_up(
        &self,
        message: MessageId,
    ) -> Result<Option<StoredFollowUp>, StoreError> {
        self.write(move |tx| remove(tx, message)).await
    }

    /// When the next waiting reminder is due (Unix ms).
    pub async fn next_follow_up_due(&self) -> Result<Option<i64>, StoreError> {
        self.read(next_due).await
    }

    /// Removes answered reminders of `scope` and brings back the due ones.
    pub async fn settle_follow_ups(
        &self,
        now: i64,
        scope: SettleScope,
    ) -> Result<SettledFollowUps, StoreError> {
        self.write(move |tx| settle(tx, now, scope)).await
    }

    /// Whether the account has any reminder.
    pub async fn has_follow_ups(&self, account: AccountId) -> Result<bool, StoreError> {
        self.read(move |c| any_of(c, account)).await
    }

    /// The reminders of `accounts` as list rows (see `rows`): the returned ones for the top of
    /// the inbox, or all for "Waiting for reply".
    pub async fn follow_up_rows(
        &self,
        accounts: Vec<AccountId>,
        returned: bool,
        limit: u32,
    ) -> Result<Vec<MessageListItem>, StoreError> {
        self.read(move |c| rows(c, &accounts, returned, limit))
            .await
    }

    /// The reminder of a message, if any (by its account and Message-ID).
    pub async fn follow_up_of(
        &self,
        message: MessageId,
    ) -> Result<Option<StoredFollowUp>, StoreError> {
        self.read(move |c| {
            let key = key_of(c, message)?;
            of_key(c, &key)
        })
        .await
    }
}

impl crate::Store {
    /// Stage 20: the reminders that came back for the top of an inbox list: those of the
    /// scope's accounts when the scope is inboxes (a single mailbox only when it is an inbox).
    pub async fn inbox_follow_ups(
        &self,
        scope: crate::InboxMailboxes,
        limit: u32,
    ) -> Result<Vec<MessageListItem>, StoreError> {
        self.read(move |c| {
            let mailboxes = crate::comfort_api::resolve(c, &scope)?;
            let mut accounts: Vec<AccountId> = Vec::new();
            let mut is_inbox =
                c.prepare_cached("SELECT role = 'inbox' FROM mailbox WHERE id = ?1")?;
            for (mailbox, account) in mailboxes {
                let inbox: Option<bool> = is_inbox
                    .query_row([mailbox.0], |r| r.get(0))
                    .optional()?
                    .flatten();
                if inbox == Some(true) && !accounts.contains(&account) {
                    accounts.push(account);
                }
            }
            rows(c, &accounts, true, limit)
        })
        .await
    }
}
