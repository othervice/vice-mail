//! Stage 16 (decision 0033): the reading list, mailbox favourites and colours, blocked senders,
//! and the unread lists. Synchronous functions over a connection and the [`Store`] methods
//! that run them; the sync content of the three synced kinds (`readingList`, `mailboxPref`,
//! `blockedSender`).
//!
//! The reading list refers to a message like tasks (account UUID and Message-ID), mailbox
//! preferences to a mailbox by account UUID and decoded path, blocked senders are normalised
//! addresses (the engine normalises). Nothing here touches the server.

use mail_model::{
    AccountColor, AccountId, ActionError, ActionKind, AppendSource, BlockedSender, ListCursor,
    MAX_BLOCKED_SENDERS, MAX_FAVORITE_MAILBOXES, Mailbox, MailboxId, MessageId, MessagePage,
    MessageRef, OperationPayload, ServerCopy, SourceLocation, TargetMailbox, TransferMode,
    UnreadFirstCursor, UnreadFirstPage, UnreadPhase,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::comfort_api::{InboxMailboxes, resolve};
use crate::inbox::{self, InboxListKind};
use crate::marks::{MessageKey, key_of};
use crate::outbox::{Recorder, account_writes, location_for_plan, repeated, require_writes};
use crate::{OutboxChange, Store, StoreError, StoredOperation, accounts, mailboxes};

// ---------------------------------------------------------------------------------------------
// Reading list

pub(crate) const READING_OF: &str =
    "SELECT added_at FROM reading_list WHERE account_uuid = ?1 AND message_id_hdr = ?2";

/// Puts `message` on the reading list or takes it off. The key, and whether anything changed.
pub(crate) fn set_reading_list(
    conn: &Connection,
    message: MessageId,
    on: bool,
    now: i64,
) -> Result<(MessageKey, bool), StoreError> {
    let key = key_of(conn, message)?;
    let account = key.account.to_string();
    let changed = if on {
        conn.prepare_cached(
            "INSERT OR IGNORE INTO reading_list (account_uuid, message_id_hdr, added_at)
             VALUES (?1, ?2, ?3)",
        )?
        .execute(params![account, key.message_id_hdr, now])?
    } else {
        conn.prepare_cached(
            "DELETE FROM reading_list WHERE account_uuid = ?1 AND message_id_hdr = ?2",
        )?
        .execute(params![account, key.message_id_hdr])?
    };
    Ok((key, changed > 0))
}

// ---------------------------------------------------------------------------------------------
// Mailbox favourites and colours

fn mailbox_key(conn: &Connection, mailbox: MailboxId) -> Result<(AccountId, String), StoreError> {
    let row: Option<(i64, String)> = conn
        .prepare_cached("SELECT account_id, path FROM mailbox WHERE id = ?1")?
        .query_row([mailbox.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let (account_row, path) =
        row.ok_or_else(|| StoreError::NotFound(format!("mailbox {mailbox}")))?;
    let account = conn
        .prepare_cached("SELECT uuid FROM account WHERE id = ?1")?
        .query_row([account_row], |r| crate::accounts::account_id_at(r, 0))?;
    Ok((account, path))
}

/// Clears one of the two: the row goes when the other is not set either.
fn clear_pref(
    conn: &Connection,
    account: &str,
    path: &str,
    column: &str,
    other: &str,
    now: i64,
) -> Result<(), StoreError> {
    conn.prepare_cached(&format!(
        "DELETE FROM mailbox_pref WHERE account_uuid = ?1 AND path = ?2 AND {other} IS NULL"
    ))?
    .execute(params![account, path])?;
    conn.prepare_cached(&format!(
        "UPDATE mailbox_pref SET {column} = NULL, updated_at = ?3
         WHERE account_uuid = ?1 AND path = ?2"
    ))?
    .execute(params![account, path, now])?;
    Ok(())
}

/// Makes `mailbox` a favourite (at the end) or not. The mailbox's account; whether it changed.
pub(crate) fn set_favorite(
    conn: &Connection,
    mailbox: MailboxId,
    favorite: bool,
    now: i64,
) -> Result<(AccountId, bool), StoreError> {
    let (account, path) = mailbox_key(conn, mailbox)?;
    let uuid = account.to_string();
    let current: Option<Option<i64>> = conn
        .prepare_cached(
            "SELECT favorite_position FROM mailbox_pref WHERE account_uuid = ?1 AND path = ?2",
        )?
        .query_row(params![uuid, path], |r| r.get(0))
        .optional()?;
    let is_favorite = current.flatten().is_some();
    if is_favorite == favorite {
        return Ok((account, false));
    }
    if favorite {
        let count: i64 = conn
            .prepare_cached(
                "SELECT count(*) FROM mailbox_pref WHERE favorite_position IS NOT NULL",
            )?
            .query_row([], |r| r.get(0))?;
        if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_FAVORITE_MAILBOXES {
            return Err(StoreError::InvalidArgument(format!(
                "at most {MAX_FAVORITE_MAILBOXES} favourite folders"
            )));
        }
        let next: i64 = conn
            .prepare_cached("SELECT coalesce(max(favorite_position), -1) + 1 FROM mailbox_pref")?
            .query_row([], |r| r.get(0))?;
        conn.prepare_cached(
            "INSERT INTO mailbox_pref (account_uuid, path, favorite_position, color, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4)
             ON CONFLICT (account_uuid, path) DO UPDATE
               SET favorite_position = excluded.favorite_position,
                   updated_at = excluded.updated_at",
        )?
        .execute(params![uuid, path, next, now])?;
    } else {
        clear_pref(conn, &uuid, &path, "favorite_position", "color", now)?;
    }
    Ok((account, true))
}

/// Gives `mailbox` a colour or takes it away. The mailbox's account; whether it changed.
pub(crate) fn set_color(
    conn: &Connection,
    mailbox: MailboxId,
    color: Option<AccountColor>,
    now: i64,
) -> Result<(AccountId, bool), StoreError> {
    let (account, path) = mailbox_key(conn, mailbox)?;
    let uuid = account.to_string();
    let current: Option<Option<u8>> = conn
        .prepare_cached("SELECT color FROM mailbox_pref WHERE account_uuid = ?1 AND path = ?2")?
        .query_row(params![uuid, path], |r| r.get(0))
        .optional()?;
    if current.flatten() == color.map(AccountColor::index) {
        return Ok((account, false));
    }
    match color {
        Some(color) => {
            conn.prepare_cached(
                "INSERT INTO mailbox_pref (account_uuid, path, favorite_position, color, updated_at)
                 VALUES (?1, ?2, NULL, ?3, ?4)
                 ON CONFLICT (account_uuid, path) DO UPDATE
                   SET color = excluded.color, updated_at = excluded.updated_at",
            )?
            .execute(params![uuid, path, color.index(), now])?;
        }
        None => {
            clear_pref(conn, &uuid, &path, "color", "favorite_position", now)?;
        }
    }
    Ok((account, true))
}

/// The favourites in this order (the others after them, in their order). The accounts whose
/// favourites changed.
pub(crate) fn reorder_favorites(
    conn: &Connection,
    order: &[MailboxId],
    now: i64,
) -> Result<Vec<AccountId>, StoreError> {
    let mut keys = Vec::with_capacity(order.len());
    for mailbox in order {
        let key = mailbox_key(conn, *mailbox)?;
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let current: Vec<(String, String)> = conn
        .prepare_cached(
            "SELECT account_uuid, path FROM mailbox_pref WHERE favorite_position IS NOT NULL
             ORDER BY favorite_position, account_uuid, path",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut ordered: Vec<(String, String)> = keys
        .iter()
        .map(|(a, p)| (a.to_string(), p.clone()))
        .filter(|k| current.contains(k))
        .collect();
    for key in current {
        if !ordered.contains(&key) {
            ordered.push(key);
        }
    }
    let mut changed = Vec::new();
    for (position, (account, path)) in (0i64..).zip(&ordered) {
        let n = conn
            .prepare_cached(
                "UPDATE mailbox_pref SET favorite_position = ?3, updated_at = ?4
                 WHERE account_uuid = ?1 AND path = ?2 AND favorite_position IS NOT ?3",
            )?
            .execute(params![account, path, position, now])?;
        if n > 0
            && let Ok(id) = account.parse::<AccountId>()
            && !changed.contains(&id)
        {
            changed.push(id);
        }
    }
    Ok(changed)
}

/// Every favourite mailbox that exists locally, by position (sidebar top, palette).
pub(crate) fn favorites(conn: &Connection) -> Result<Vec<Mailbox>, StoreError> {
    let keys: Vec<(String, String)> = conn
        .prepare_cached(
            "SELECT account_uuid, path FROM mailbox_pref WHERE favorite_position IS NOT NULL
             ORDER BY favorite_position, account_uuid, path",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut by_account: Vec<(AccountId, Vec<Mailbox>)> = Vec::new();
    let mut out = Vec::with_capacity(keys.len());
    for (account, path) in keys {
        let Ok(account) = account.parse::<AccountId>() else {
            continue;
        };
        if !by_account.iter().any(|(a, _)| *a == account) {
            let Ok(row) = accounts::row_id(conn, account) else {
                continue;
            };
            by_account.push((account, mailboxes::list(conn, row, account)?));
        }
        if let Some((_, list)) = by_account.iter().find(|(a, _)| *a == account)
            && let Some(mailbox) = list.iter().find(|m| m.path == path)
        {
            out.push(mailbox.clone());
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Blocked senders

pub(crate) const IS_BLOCKED: &str = "SELECT 1 FROM blocked_sender WHERE address = ?1";

/// Blocks a normalised address; whether it was not blocked before.
pub(crate) fn block(conn: &Connection, address: &str, now: i64) -> Result<bool, StoreError> {
    let count: i64 = conn
        .prepare_cached("SELECT count(*) FROM blocked_sender")?
        .query_row([], |r| r.get(0))?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_BLOCKED_SENDERS {
        return Err(StoreError::InvalidArgument(format!(
            "at most {MAX_BLOCKED_SENDERS} blocked senders"
        )));
    }
    Ok(conn
        .prepare_cached(
            "INSERT OR IGNORE INTO blocked_sender (address, created_at) VALUES (?1, ?2)",
        )?
        .execute(params![address, now])?
        > 0)
}

/// Unblocks; whether it was blocked. Mail hidden for it shows again (trigger).
pub(crate) fn unblock(conn: &Connection, address: &str) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached("DELETE FROM blocked_sender WHERE address = ?1")?
        .execute([address])?
        > 0)
}

pub(crate) fn blocked(conn: &Connection) -> Result<Vec<BlockedSender>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT address, created_at FROM blocked_sender ORDER BY address")?
        .query_map([], |r| {
            Ok(BlockedSender {
                address: r.get(0)?,
                created_at: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn is_blocked(conn: &Connection, address: &str) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached(IS_BLOCKED)?
        .query_row([address], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Hides a message of a blocked sender from the inbox lists (it could not go to junk).
pub(crate) fn hide(
    conn: &Connection,
    message: MessageId,
    sender: &str,
) -> Result<bool, StoreError> {
    let key = key_of(conn, message)?;
    Ok(conn
        .prepare_cached(
            "INSERT OR IGNORE INTO blocked_hidden (account_uuid, message_id_hdr, sender)
             SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM blocked_sender WHERE address = ?3)",
        )?
        .execute(params![key.account.to_string(), key.message_id_hdr, sender])?
        > 0)
}

/// Hidden rows of mailbox `?1` of account `?2` (see [`count_hidden`]). Each row once: a hidden
/// entry has its own Message-ID, and a message lies in a mailbox at most once.
pub(crate) const COUNT_HIDDEN: &str = "
    SELECT count(*) FROM blocked_hidden h
      CROSS JOIN message m
        ON m.account_id = (SELECT id FROM account WHERE uuid = ?2)
       AND m.message_id_hdr = h.message_id_hdr
      CROSS JOIN message_location l ON l.message_id = m.id AND l.mailbox_id = ?1
    WHERE h.account_uuid = ?2";

/// Messages of blocked senders hidden from the inbox lists of the scope (stage 16, decision
/// 0036): rows of the scope's inboxes whose message is in `blocked_hidden`. One probe of the
/// message index per hidden entry of the account; there are few.
pub(crate) fn count_hidden(conn: &Connection, scope: &InboxMailboxes) -> Result<u32, StoreError> {
    let mut total: u32 = 0;
    for (mailbox, account) in resolve(conn, scope)? {
        let count: i64 = conn
            .prepare_cached(COUNT_HIDDEN)?
            .query_row(params![mailbox.0, account.to_string()], |r| r.get(0))?;
        total = total.saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }
    Ok(total)
}

// ---------------------------------------------------------------------------------------------
// Sync content (`settings_sync`)

fn split_scoped(id: &str) -> Option<(&str, &str)> {
    id.split_once('/')
}

/// The content of a stage 16 record as stored here, `None` when there is none.
pub(crate) fn synced_content(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<Option<Value>, StoreError> {
    Ok(match kind {
        "readingList" => {
            let Some((account, msgid)) = split_scoped(id) else {
                return Ok(None);
            };
            conn.prepare_cached(READING_OF)?
                .query_row([account, msgid], |r| r.get::<_, i64>(0))
                .optional()?
                .map(|at| json!({ "addedAtMs": at }))
        }
        "mailboxPref" => {
            let Some((account, path)) = split_scoped(id) else {
                return Ok(None);
            };
            conn.prepare_cached(
                "SELECT favorite_position, color FROM mailbox_pref
                 WHERE account_uuid = ?1 AND path = ?2",
            )?
            .query_row([account, path], |r| {
                Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
            })
            .optional()?
            .map(|(position, color)| {
                let mut value = serde_json::Map::new();
                if let Some(position) = position {
                    value.insert("favoritePosition".into(), json!(position));
                }
                if let Some(color) = color {
                    value.insert("color".into(), json!(color));
                }
                Value::Object(value)
            })
        }
        "blockedSender" => conn
            .prepare_cached(IS_BLOCKED)?
            .query_row([id], |_| Ok(json!({})))
            .optional()?,
        _ => None,
    })
}

/// Writes (`Some`) or deletes (`None`) a stage 16 record's row. Rows changed.
pub(crate) fn apply_synced(
    conn: &Connection,
    kind: &str,
    id: &str,
    value: Option<&Value>,
    now: i64,
) -> Result<usize, StoreError> {
    let bad = |what: &str| StoreError::InvalidArgument(format!("{kind} record: {what}"));
    match (kind, value) {
        ("readingList", Some(value)) => {
            let (account, msgid) = split_scoped(id).ok_or_else(|| bad("id"))?;
            let at = value
                .get("addedAtMs")
                .and_then(Value::as_i64)
                .ok_or_else(|| bad("addedAtMs"))?;
            Ok(conn
                .prepare_cached(
                    "INSERT INTO reading_list (account_uuid, message_id_hdr, added_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT (account_uuid, message_id_hdr) DO UPDATE
                       SET added_at = excluded.added_at",
                )?
                .execute(params![account, msgid, at])?)
        }
        ("readingList", None) => {
            let (account, msgid) = split_scoped(id).ok_or_else(|| bad("id"))?;
            Ok(conn
                .prepare_cached(
                    "DELETE FROM reading_list WHERE account_uuid = ?1 AND message_id_hdr = ?2",
                )?
                .execute(params![account, msgid])?)
        }
        ("mailboxPref", Some(value)) => {
            let (account, path) = split_scoped(id).ok_or_else(|| bad("id"))?;
            let position = value.get("favoritePosition").and_then(Value::as_i64);
            let color = value
                .get("color")
                .and_then(Value::as_u64)
                .map(|c| {
                    u8::try_from(c)
                        .ok()
                        .and_then(AccountColor::new)
                        .ok_or_else(|| bad("color"))
                })
                .transpose()?;
            if position.is_none() && color.is_none() {
                return Err(bad("neither favourite nor colour"));
            }
            Ok(conn
                .prepare_cached(
                    "INSERT INTO mailbox_pref (account_uuid, path, favorite_position, color, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (account_uuid, path) DO UPDATE
                       SET favorite_position = excluded.favorite_position,
                           color = excluded.color, updated_at = excluded.updated_at",
                )?
                .execute(params![
                    account,
                    path,
                    position,
                    color.map(AccountColor::index),
                    now
                ])?)
        }
        ("mailboxPref", None) => {
            let (account, path) = split_scoped(id).ok_or_else(|| bad("id"))?;
            Ok(conn
                .prepare_cached("DELETE FROM mailbox_pref WHERE account_uuid = ?1 AND path = ?2")?
                .execute(params![account, path])?)
        }
        ("blockedSender", Some(_)) => Ok(conn
            .prepare_cached(
                "INSERT OR IGNORE INTO blocked_sender (address, created_at) VALUES (?1, ?2)",
            )?
            .execute(params![id, now])?),
        ("blockedSender", None) => unblock(conn, id).map(usize::from),
        _ => Err(bad("unknown kind")),
    }
}

// ---------------------------------------------------------------------------------------------
// Unread lists

/// One page of "unread first": the unread rows of the scope, then the read ones.
pub(crate) fn unread_first(
    conn: &Connection,
    scope: &InboxMailboxes,
    cursor: Option<UnreadFirstCursor>,
    limit: u32,
) -> Result<UnreadFirstPage, StoreError> {
    let mailboxes = resolve(conn, scope)?;
    let cursor = cursor.unwrap_or(UnreadFirstCursor {
        phase: UnreadPhase::Unread,
        before: None,
    });
    let mut items = Vec::new();
    if cursor.phase == UnreadPhase::Unread {
        let page = inbox::list(
            conn,
            InboxListKind::Unread,
            &mailboxes,
            cursor.before,
            limit,
        )?;
        items = page.items;
        if let Some(before) = page.next_cursor {
            return Ok(UnreadFirstPage {
                items,
                next_cursor: Some(UnreadFirstCursor {
                    phase: UnreadPhase::Unread,
                    before: Some(before),
                }),
            });
        }
    }
    let rest = limit.saturating_sub(u32::try_from(items.len()).unwrap_or(u32::MAX));
    let before = match cursor.phase {
        UnreadPhase::Unread => None,
        UnreadPhase::Read => cursor.before,
    };
    if rest == 0 {
        return Ok(UnreadFirstPage {
            items,
            next_cursor: Some(UnreadFirstCursor {
                phase: UnreadPhase::Read,
                before,
            }),
        });
    }
    let MessagePage {
        items: read,
        next_cursor,
    } = inbox::list(conn, InboxListKind::Read, &mailboxes, before, rest)?;
    items.extend(read);
    Ok(UnreadFirstPage {
        items,
        next_cursor: next_cursor.map(|before: ListCursor| UnreadFirstCursor {
            phase: UnreadPhase::Read,
            before: Some(before),
        }),
    })
}

// ---------------------------------------------------------------------------------------------
// Store API

impl Store {
    /// Puts messages on the reading list (`on`) or takes them off. The keys that changed.
    pub async fn set_reading_list(
        &self,
        messages: Vec<MessageId>,
        on: bool,
        now: i64,
    ) -> Result<Vec<MessageKey>, StoreError> {
        self.write(move |tx| {
            let mut changed = Vec::new();
            for message in messages {
                let (key, did) = set_reading_list(tx, message, on, now)?;
                if did {
                    changed.push(key);
                }
            }
            Ok(changed)
        })
        .await
    }

    pub async fn set_mailbox_favorite(
        &self,
        mailbox: MailboxId,
        favorite: bool,
        now: i64,
    ) -> Result<(AccountId, bool), StoreError> {
        self.write(move |tx| set_favorite(tx, mailbox, favorite, now))
            .await
    }

    pub async fn set_mailbox_color(
        &self,
        mailbox: MailboxId,
        color: Option<AccountColor>,
        now: i64,
    ) -> Result<(AccountId, bool), StoreError> {
        self.write(move |tx| set_color(tx, mailbox, color, now))
            .await
    }

    pub async fn reorder_favorite_mailboxes(
        &self,
        order: Vec<MailboxId>,
        now: i64,
    ) -> Result<Vec<AccountId>, StoreError> {
        self.write(move |tx| reorder_favorites(tx, &order, now))
            .await
    }

    pub async fn favorite_mailboxes(&self) -> Result<Vec<Mailbox>, StoreError> {
        self.read(favorites).await
    }

    /// Blocks a normalised address; whether it was new.
    pub async fn block_sender(&self, address: String, now: i64) -> Result<bool, StoreError> {
        self.write(move |tx| block(tx, &address, now)).await
    }

    pub async fn unblock_sender(&self, address: String) -> Result<bool, StoreError> {
        self.write(move |tx| unblock(tx, &address)).await
    }

    pub async fn blocked_senders(&self) -> Result<Vec<BlockedSender>, StoreError> {
        self.read(blocked).await
    }

    pub async fn is_sender_blocked(&self, address: String) -> Result<bool, StoreError> {
        self.read(move |c| is_blocked(c, &address)).await
    }

    /// Hides a message of a blocked sender from the inbox lists; whether it was hidden now.
    pub async fn hide_blocked_message(
        &self,
        message: MessageId,
        sender: String,
    ) -> Result<bool, StoreError> {
        self.write(move |tx| hide(tx, message, &sender)).await
    }

    /// How many messages of blocked senders the inbox lists of the scope leave out.
    pub async fn count_blocked_hidden(&self, scope: InboxMailboxes) -> Result<u32, StoreError> {
        self.read(move |c| count_hidden(c, &scope)).await
    }

    /// One page of "unread first" over the scope.
    pub async fn unread_first_list(
        &self,
        scope: InboxMailboxes,
        cursor: Option<UnreadFirstCursor>,
        limit: u32,
    ) -> Result<UnreadFirstPage, StoreError> {
        self.read(move |c| unread_first(c, &scope, cursor, limit))
            .await
    }
}

// ---------------------------------------------------------------------------------------------
// Moving and copying between accounts

/// A message about to be copied to another account, as it lies now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferItem {
    pub at: MessageRef,
    pub source: SourceLocation,
    /// Without `<>`; the copy in the target is recognised by it.
    pub message_id_header: String,
}

/// What a transfer checked: the target and the messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPlan {
    pub target: TargetMailbox,
    pub target_account: AccountId,
    pub items: Vec<TransferItem>,
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

/// Checks a copy or move of `refs` to `target` in another account, or (stage 21, decision 0064)
/// a copy to another folder of the same account. Refused before anything is recorded:
/// `read_only` (the target account, and for a move the source accounts, must write), an unknown
/// or unselectable target, a move of a message of the target's own account (that is an ordinary
/// move), a copy into the folder the message lies in, a message without Message-ID (the copy
/// could not be confirmed).
pub(crate) fn check_transfer(
    conn: &Connection,
    refs: &[MessageRef],
    target: MailboxId,
    mode: TransferMode,
) -> Result<TransferPlan, StoreError> {
    if refs.is_empty() {
        return Err(StoreError::Action(ActionError::NoMessages));
    }
    let (target_box, target_row) = target_of(conn, target)?;
    let target_account = require_writes(conn, target_row)?;
    let mut items: Vec<TransferItem> = Vec::with_capacity(refs.len());
    for at in refs {
        if items.iter().any(|i| i.at == *at) {
            continue;
        }
        let planned = location_for_plan(conn, *at)?
            .ok_or(StoreError::Action(ActionError::MessageNotFound(*at)))?;
        if planned.account_row == target_row {
            // Stage 21 (decision 0064): a copy into another folder of the same account takes the
            // same way (the frozen bytes appended there); a move within an account is an
            // ordinary move, and a copy into the message's own folder would only duplicate it.
            if mode == TransferMode::Move {
                return Err(StoreError::InvalidArgument(format!(
                    "message {} already belongs to the target's account; move it within the account",
                    at.message_id
                )));
            }
            if at.mailbox_id == target {
                return Err(StoreError::InvalidArgument(format!(
                    "message {} already lies in the target folder",
                    at.message_id
                )));
            }
        }
        if mode == TransferMode::Move {
            require_writes(conn, planned.account_row)?;
        }
        let source = planned.location.source;
        let message_id_header = source
            .message_id_header
            .clone()
            .filter(|m| !m.is_empty())
            .ok_or_else(|| {
                StoreError::InvalidArgument(format!(
                    "message {} has no Message-ID: its copy in the other account could not be \
                     confirmed",
                    at.message_id
                ))
            })?;
        items.push(TransferItem {
            at: *at,
            source,
            message_id_header,
        });
    }
    Ok(TransferPlan {
        target: target_box,
        target_account,
        items,
    })
}

/// Records the appends of a transfer in the target account, after the engine froze each
/// message's bytes into `files` (relative to the compose directory, by message). The check runs
/// again on the data of this transaction.
pub(crate) fn enqueue_transfer(
    conn: &Connection,
    target: MailboxId,
    mode: TransferMode,
    files: &[(MessageRef, String)],
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let refs: Vec<MessageRef> = files.iter().map(|(at, _)| *at).collect();
    let plan = check_transfer(conn, &refs, target, mode)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(plan.target_account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let target_row = accounts::row_id(conn, plan.target_account)?;
    let action = match mode {
        TransferMode::Copy => ActionKind::Copy,
        TransferMode::Move => ActionKind::Move,
    };
    let mut recorder = Recorder::new(conn, target_row, plan.target_account, action, request, now);
    for item in plan.items {
        let Some((_, file)) = files.iter().find(|(at, _)| *at == item.at) else {
            continue;
        };
        recorder.record(OperationPayload::Append {
            target: plan.target.clone(),
            source: AppendSource::Transfer {
                file: file.clone(),
                flags: item.source.flags.clone(),
                remove: (mode == TransferMode::Move).then(|| Box::new(item.source.clone())),
            },
            message_id_header: item.message_id_header,
            draft_flag: false,
            mark: None,
            replace: None,
        })?;
    }
    Ok(recorder.change)
}

/// A transfer's append was confirmed: for a move, the source is removed now (an `expunge` in
/// the source account, through its outbox), but only if the copy was found in the target
/// (`copy`, by Message-ID above the mark). Without a confirmed copy the source stays.
pub(crate) fn transfer_appended(
    conn: &Connection,
    op: &StoredOperation,
    copy: Option<&ServerCopy>,
    now: i64,
) -> Result<Option<OutboxChange>, StoreError> {
    let OperationPayload::Append {
        source: AppendSource::Transfer {
            remove: Some(source),
            ..
        },
        ..
    } = &op.payload
    else {
        return Ok(None);
    };
    if copy.is_none() {
        tracing::warn!(operation = %op.id, "copy not confirmed in the target; the source stays");
        return Ok(None);
    }
    let at = MessageRef {
        mailbox_id: source.mailbox_id,
        message_id: source.message_id,
    };
    let Some(planned) = location_for_plan(conn, at)? else {
        // Moved or deleted meanwhile: nothing left to remove here.
        return Ok(None);
    };
    let (account, _) = account_writes(conn, planned.account_row)?;
    let request = format!("transfer-{}", op.id);
    let mut recorder = Recorder::new(
        conn,
        planned.account_row,
        account,
        ActionKind::Move,
        &request,
        now,
    );
    recorder.change.batch = Some(op.batch);
    recorder.record(OperationPayload::Expunge {
        source: planned.location.source,
    })?;
    Ok(Some(recorder.change))
}

impl Store {
    /// See [`check_transfer`].
    pub async fn check_transfer(
        &self,
        refs: Vec<MessageRef>,
        target: MailboxId,
        mode: TransferMode,
    ) -> Result<TransferPlan, StoreError> {
        self.read(move |c| check_transfer(c, &refs, target, mode))
            .await
    }

    /// See [`enqueue_transfer`].
    pub async fn enqueue_transfer(
        &self,
        target: MailboxId,
        mode: TransferMode,
        files: Vec<(MessageRef, String)>,
        request: String,
        now: i64,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| enqueue_transfer(tx, target, mode, &files, &request, now))
            .await
    }
}
