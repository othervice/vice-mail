//! Stage 16 (decision 0036): pinned attachments. A local mark by account UUID, Message-ID and
//! the attachment's index, synced as kind `attachmentPin` (id `<account>/<Message-ID>/<index>`,
//! content `{pinnedAtMs, name, mimeType, size}`). Name, type and size are kept from pinning
//! time, so the view lists an attachment without its body. Nothing touches the server.

use mail_model::{AccountId, MAX_PINNED_ATTACHMENTS, MailboxId, MessageId, MessageListItem};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde_json::{Value, json};

use crate::accounts::account_id_at;
use crate::marks::{MessageKey, key_of, messages_of};
use crate::{Store, StoreError};

/// What a pin keeps of the attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPinInput {
    pub index: u32,
    /// Sanitised file name.
    pub name: String,
    pub mime_type: String,
    pub size: u32,
}

/// A row of "Pinned attachments".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedAttachmentRow {
    pub message: MessageListItem,
    pub index: u32,
    pub name: String,
    pub mime_type: String,
    pub size: u32,
    pub pinned_at: i64,
}

/// Longest name a pin keeps (sanitised names are far shorter; bounds a synced record).
const MAX_PIN_NAME_BYTES: usize = 1024;
/// Longest MIME type a pin keeps.
const MAX_PIN_MIME_BYTES: usize = 255;

fn check_input(input: &AttachmentPinInput) -> Result<(), StoreError> {
    if input.name.is_empty()
        || input.name.len() > MAX_PIN_NAME_BYTES
        || input.name.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidArgument(
            "the attachment name is empty, too long or holds controls".into(),
        ));
    }
    if input.mime_type.len() > MAX_PIN_MIME_BYTES || input.mime_type.chars().any(char::is_control) {
        return Err(StoreError::InvalidArgument(
            "the attachment type is too long or holds controls".into(),
        ));
    }
    Ok(())
}

/// Pins attachment `input.index` of `message` (`Some`) or takes the pin of `index` off
/// (`None`). The message's key and whether anything changed.
pub(crate) fn set_pin(
    conn: &Connection,
    message: MessageId,
    index: u32,
    input: Option<&AttachmentPinInput>,
    now: i64,
) -> Result<(MessageKey, bool), StoreError> {
    let key = key_of(conn, message)?;
    let account = key.account.to_string();
    let changed = match input {
        Some(input) => {
            check_input(input)?;
            let exists = conn
                .prepare_cached(
                    "SELECT 1 FROM attachment_pin
                     WHERE account_uuid = ?1 AND message_id_hdr = ?2 AND part_index = ?3",
                )?
                .query_row(params![account, key.message_id_hdr, index], |_| Ok(()))
                .optional()?
                .is_some();
            if exists {
                0
            } else {
                let count: i64 = conn
                    .prepare_cached("SELECT count(*) FROM attachment_pin")?
                    .query_row([], |r| r.get(0))?;
                if count >= i64::from(MAX_PINNED_ATTACHMENTS) {
                    return Err(StoreError::InvalidArgument(format!(
                        "at most {MAX_PINNED_ATTACHMENTS} pinned attachments"
                    )));
                }
                conn.prepare_cached(
                    "INSERT INTO attachment_pin
                       (account_uuid, message_id_hdr, part_index, name, mime_type, size, pinned_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?
                .execute(params![
                    account,
                    key.message_id_hdr,
                    index,
                    input.name,
                    input.mime_type,
                    input.size,
                    now
                ])?
            }
        }
        None => conn
            .prepare_cached(
                "DELETE FROM attachment_pin
                 WHERE account_uuid = ?1 AND message_id_hdr = ?2 AND part_index = ?3",
            )?
            .execute(params![account, key.message_id_hdr, index])?,
    };
    Ok((key, changed > 0))
}

/// The pinned attachment indexes of a message (of every copy with its Message-ID).
pub(crate) fn pins_of(conn: &Connection, message: MessageId) -> Result<Vec<u32>, StoreError> {
    let Ok(key) = key_of(conn, message) else {
        // No Message-ID: nothing can be pinned. An unknown message is still `not_found`.
        if conn
            .prepare_cached("SELECT 1 FROM message WHERE id = ?1")?
            .query_row([message.0], |_| Ok(()))
            .optional()?
            .is_none()
        {
            return Err(StoreError::NotFound(format!("message {message}")));
        }
        return Ok(Vec::new());
    };
    Ok(conn
        .prepare_cached(
            "SELECT part_index FROM attachment_pin
             WHERE account_uuid = ?1 AND message_id_hdr = ?2 ORDER BY part_index",
        )?
        .query_map(params![key.account.to_string(), key.message_id_hdr], |r| {
            r.get(0)
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// The pinned attachments of `accounts`, the newest pin first, each with its message as a list
/// row in its preferred mailbox (`prefer`, the inboxes) where it lies there. Pins of messages
/// not stored here are left out.
pub(crate) fn view(
    conn: &Connection,
    accounts: &[AccountId],
    prefer: &[MailboxId],
    limit: u32,
) -> Result<Vec<PinnedAttachmentRow>, StoreError> {
    if accounts.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..accounts.len()).map(|i| format!("?{}", i + 2)).collect();
    let sql = format!(
        "SELECT account_uuid, message_id_hdr, part_index, name, mime_type, size, pinned_at
         FROM attachment_pin WHERE account_uuid IN ({})
         ORDER BY pinned_at DESC, account_uuid, message_id_hdr, part_index LIMIT ?1",
        placeholders.join(", ")
    );
    let mut values: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(i64::from(
        limit.min(MAX_PINNED_ATTACHMENTS),
    ))];
    values.extend(
        accounts
            .iter()
            .map(|a| rusqlite::types::Value::Text(a.to_string())),
    );
    let pins: Vec<(MessageKey, u32, String, String, u32, i64)> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(values), |r| {
            Ok((
                MessageKey {
                    account: account_id_at(r, 0)?,
                    message_id_hdr: r.get(1)?,
                },
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::with_capacity(pins.len());
    for (key, index, name, mime_type, size, pinned_at) in pins {
        for message in messages_of(conn, &key)? {
            if let Some(entry) = crate::lists::entry(conn, message, prefer)? {
                rows.push(PinnedAttachmentRow {
                    message: entry.item,
                    index,
                    name,
                    mime_type,
                    size,
                    pinned_at,
                });
                break;
            }
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------------------------
// Sync content (`settings_sync`)

/// `<account>/<Message-ID>/<index>` into its parts (the Message-ID may hold `/`).
pub(crate) fn split_id(id: &str) -> Option<(&str, &str, u32)> {
    let (account, rest) = id.split_once('/')?;
    let (message_id, index) = rest.rsplit_once('/')?;
    if message_id.is_empty() {
        return None;
    }
    Some((account, message_id, index.parse().ok()?))
}

/// The content of an `attachmentPin` record as stored here, `None` when there is none.
pub(crate) fn synced_content(conn: &Connection, id: &str) -> Result<Option<Value>, StoreError> {
    let Some((account, message_id, index)) = split_id(id) else {
        return Ok(None);
    };
    Ok(conn
        .prepare_cached(
            "SELECT name, mime_type, size, pinned_at FROM attachment_pin
             WHERE account_uuid = ?1 AND message_id_hdr = ?2 AND part_index = ?3",
        )?
        .query_row(params![account, message_id, index], |r| {
            Ok(json!({
                "name": r.get::<_, String>(0)?,
                "mimeType": r.get::<_, String>(1)?,
                "size": r.get::<_, u32>(2)?,
                "pinnedAtMs": r.get::<_, i64>(3)?,
            }))
        })
        .optional()?)
}

/// Writes (`Some`) or deletes (`None`) the row of an `attachmentPin` record. Rows changed.
pub(crate) fn apply_synced(
    conn: &Connection,
    id: &str,
    value: Option<&Value>,
) -> Result<usize, StoreError> {
    let bad = |what: &str| StoreError::InvalidArgument(format!("attachmentPin record: {what}"));
    let (account, message_id, index) = split_id(id).ok_or_else(|| bad("id"))?;
    let Some(value) = value else {
        return Ok(conn
            .prepare_cached(
                "DELETE FROM attachment_pin
                 WHERE account_uuid = ?1 AND message_id_hdr = ?2 AND part_index = ?3",
            )?
            .execute(params![account, message_id, index])?);
    };
    let input = AttachmentPinInput {
        index,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("name"))?
            .to_owned(),
        mime_type: value
            .get("mimeType")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("mimeType"))?
            .to_owned(),
        size: value
            .get("size")
            .and_then(Value::as_u64)
            .and_then(|s| u32::try_from(s).ok())
            .ok_or_else(|| bad("size"))?,
    };
    check_input(&input)?;
    let at = value
        .get("pinnedAtMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| bad("pinnedAtMs"))?;
    Ok(conn
        .prepare_cached(
            "INSERT INTO attachment_pin
               (account_uuid, message_id_hdr, part_index, name, mime_type, size, pinned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (account_uuid, message_id_hdr, part_index) DO UPDATE
               SET name = excluded.name, mime_type = excluded.mime_type, size = excluded.size,
                   pinned_at = excluded.pinned_at",
        )?
        .execute(params![
            account,
            message_id,
            index,
            input.name,
            input.mime_type,
            input.size,
            at
        ])?)
}

// ---------------------------------------------------------------------------------------------
// Store API

impl Store {
    /// Pins an attachment (`Some`, with what the pin keeps) or takes the pin of `index` off.
    /// The message's key and whether anything changed.
    pub async fn set_attachment_pin(
        &self,
        message: MessageId,
        index: u32,
        input: Option<AttachmentPinInput>,
        now: i64,
    ) -> Result<(MessageKey, bool), StoreError> {
        self.write(move |tx| set_pin(tx, message, index, input.as_ref(), now))
            .await
    }

    /// The pinned attachment indexes of a message.
    pub async fn attachment_pins_of(&self, message: MessageId) -> Result<Vec<u32>, StoreError> {
        self.read(move |c| pins_of(c, message)).await
    }

    /// "Pinned attachments" of `accounts`, newest first (at most [`MAX_PINNED_ATTACHMENTS`]).
    pub async fn pinned_attachments(
        &self,
        accounts: Vec<AccountId>,
        prefer: Vec<MailboxId>,
        limit: u32,
    ) -> Result<Vec<PinnedAttachmentRow>, StoreError> {
        self.read(move |c| view(c, &accounts, &prefer, limit)).await
    }
}

#[cfg(test)]
mod tests {
    use super::split_id;

    #[test]
    fn ids_split_at_the_first_and_the_last_slash() {
        assert_eq!(
            split_id("0b5c1f9e-0000-4000-8000-000000000001/a/b@x/3"),
            Some(("0b5c1f9e-0000-4000-8000-000000000001", "a/b@x", 3))
        );
        assert_eq!(split_id("acc/msg@x/x"), None);
        assert_eq!(split_id("acc//1"), None);
        assert_eq!(split_id("acc/msg@x"), None);
    }
}
