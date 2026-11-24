//! Stage 17, block B (decision 0050): what the contacts view adds to the directory of stage 10.
//!
//! - **Notes** on the contact's head row (`contact.note`), synchronised in the `contact` record.
//! - **Pictures** (`contact_picture`), one per address that has one: the contact's picture is
//!   that of its first address that has one (head first). `image` is the synchronised JPEG,
//!   `display` the one encoded on this device (the same bytes for a picture chosen here; for a
//!   picture from the settings sync `NULL` until the engine had the helper encode it again,
//!   decision 0046). Synchronised as kind `contactPicture` (only `image`).
//! - **Kinds** (`contact.kind`): computed by the engine, recomputed for every row once after the
//!   update (rows of stage 10 that the backfill never touches).
//! - **A contact's mail**: messages from its addresses (`message_sender`), for the contacts
//!   view and for moving them to junk when the contact is marked as spam.

use mail_model::{AccountId, MailboxId, MessageId};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::accounts::account_id_at;
use crate::settings::now_millis;
use crate::{Store, StoreError};

/// A contact picture as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPicture {
    /// The address it is stored for.
    pub key: String,
    /// The synchronised JPEG.
    pub image: Vec<u8>,
    /// The JPEG encoded on this device; `None` until a picture from the sync was encoded here.
    pub display: Option<Vec<u8>>,
    pub revision: i64,
}

/// A message from an address, in one folder (for moving it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenderLocation {
    pub account: AccountId,
    pub mailbox: MailboxId,
    pub message: MessageId,
}

/// Roles whose messages "mark as spam" leaves where they are: junk already, trash, the user's
/// own (sent, drafts) and Gmail's "All Mail" (every message is there).
const KEPT_ROLES: &str = "('junk', 'trash', 'sent', 'drafts', 'all')";

pub(crate) fn sender_locations_sql() -> String {
    format!(
        "SELECT (SELECT a.uuid FROM account a WHERE a.id = b.account_id), l.mailbox_id, m.id
         FROM message m INDEXED BY message_sender
         CROSS JOIN message_location l INDEXED BY loc_message ON l.message_id = m.id
         CROSS JOIN mailbox b ON b.id = l.mailbox_id
         WHERE m.sender_key = ?1 AND m.sender_key IS NOT NULL
           AND (b.role IS NULL OR b.role NOT IN {KEPT_ROLES})
         ORDER BY m.id DESC"
    )
}

pub(crate) const SENDER_MESSAGES: &str = "
    SELECT id FROM message INDEXED BY message_sender
    WHERE sender_key = ?1 AND sender_key IS NOT NULL
    ORDER BY id DESC LIMIT ?2";

pub(crate) const PICTURE_BY_KEY: &str =
    "SELECT address_key, image, display, revision FROM contact_picture WHERE address_key = ?1";

fn picture(conn: &Connection, key: &str) -> Result<Option<StoredPicture>, StoreError> {
    Ok(conn
        .prepare_cached(PICTURE_BY_KEY)?
        .query_row([key], |r| {
            Ok(StoredPicture {
                key: r.get(0)?,
                image: r.get(1)?,
                display: r.get(2)?,
                revision: r.get(3)?,
            })
        })
        .optional()?)
}

/// The picture of the first of `keys` that has one.
pub(crate) fn picture_of(
    conn: &Connection,
    keys: &[String],
) -> Result<Option<StoredPicture>, StoreError> {
    for key in keys {
        if let Some(found) = picture(conn, key)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// A new revision: now, but always above the previous one.
fn next_revision(conn: &Connection, key: &str, now: i64) -> Result<i64, StoreError> {
    let previous: Option<i64> = conn
        .prepare_cached("SELECT revision FROM contact_picture WHERE address_key = ?1")?
        .query_row([key], |r| r.get(0))
        .optional()?;
    Ok(previous.map_or(now, |p| now.max(p + 1)))
}

/// Stores the picture chosen here for the address `key`; the other addresses of its contact
/// (`others`) lose theirs. Returns the revision.
pub(crate) fn set_picture(
    conn: &Connection,
    key: &str,
    others: &[String],
    jpeg: &[u8],
    now: i64,
) -> Result<i64, StoreError> {
    let revision = next_revision(conn, key, now)?;
    conn.prepare_cached(
        "INSERT INTO contact_picture (address_key, image, display, revision)
         VALUES (?1, ?2, ?2, ?3)
         ON CONFLICT (address_key) DO UPDATE SET image = excluded.image,
           display = excluded.display, revision = excluded.revision",
    )?
    .execute(params![key, jpeg, revision])
    .map_err(StoreError::sqlite("store contact picture"))?;
    let mut remove = conn.prepare_cached("DELETE FROM contact_picture WHERE address_key = ?1")?;
    for other in others.iter().filter(|o| o.as_str() != key) {
        remove.execute([other])?;
    }
    Ok(revision)
}

/// Removes the pictures of `keys`; returns how many there were.
pub(crate) fn remove_pictures(conn: &Connection, keys: &[String]) -> Result<usize, StoreError> {
    let mut remove = conn.prepare_cached("DELETE FROM contact_picture WHERE address_key = ?1")?;
    let mut removed = 0;
    for key in keys {
        removed += remove
            .execute([key])
            .map_err(StoreError::sqlite("remove contact picture"))?;
    }
    Ok(removed)
}

/// Keeps what the helper encoded on this device for the synchronised picture of `key`, if the
/// picture is still that revision. Not synchronised (`image` stays).
pub(crate) fn set_display(
    conn: &Connection,
    key: &str,
    revision: i64,
    display: &[u8],
) -> Result<bool, StoreError> {
    let n = conn
        .prepare_cached(
            "UPDATE contact_picture SET display = ?3 WHERE address_key = ?1 AND revision = ?2",
        )?
        .execute(params![key, revision, display])
        .map_err(StoreError::sqlite("store contact picture"))?;
    Ok(n > 0)
}

/// Every picture: address key and revision.
pub(crate) fn picture_list(conn: &Connection) -> Result<Vec<(String, i64)>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT address_key, revision FROM contact_picture ORDER BY address_key")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Sets the note of the contact with the address `key` (its head row). `false` without one.
pub(crate) fn set_note(
    conn: &Connection,
    key: &str,
    note: Option<&str>,
    now: i64,
) -> Result<bool, StoreError> {
    let Some((row, group)) = conn
        .prepare_cached("SELECT id, contact_group FROM contact WHERE address_key = ?1")?
        .query_row([key], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .optional()?
    else {
        return Ok(false);
    };
    let n = conn
        .prepare_cached("UPDATE contact SET note = ?2, updated_at = ?3 WHERE id = ?1")?
        .execute(params![group.unwrap_or(row), note, now])
        .map_err(StoreError::sqlite("set contact note"))?;
    Ok(n > 0)
}

/// One row for recomputing kinds: id, address key, personal, sent, stored kind.
pub type KindRow = (i64, String, bool, u32, i64);

pub(crate) fn kind_rows(
    conn: &Connection,
    after: i64,
    limit: u32,
) -> Result<Vec<KindRow>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT id, address_key, personal, sent, kind FROM contact
             WHERE id > ?1 ORDER BY id LIMIT ?2",
        )?
        .query_map(params![after, limit], |r| {
            let sent: i64 = r.get(3)?;
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                u32::try_from(sent.max(0)).unwrap_or(u32::MAX),
                r.get(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn set_kinds(conn: &Connection, kinds: &[(i64, i64)]) -> Result<(), StoreError> {
    let mut update = conn.prepare_cached("UPDATE contact SET kind = ?2 WHERE id = ?1")?;
    for (row, kind) in kinds {
        update
            .execute(params![row, kind])
            .map_err(StoreError::sqlite("set contact kind"))?;
    }
    Ok(())
}

pub(crate) fn sender_locations(
    conn: &Connection,
    key: &str,
) -> Result<Vec<SenderLocation>, StoreError> {
    Ok(conn
        .prepare_cached(&sender_locations_sql())?
        .query_map([key], |r| {
            Ok(SenderLocation {
                account: account_id_at(r, 0)?,
                mailbox: MailboxId(r.get(1)?),
                message: MessageId(r.get(2)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn sender_messages(
    conn: &Connection,
    key: &str,
    limit: u32,
) -> Result<Vec<MessageId>, StoreError> {
    Ok(conn
        .prepare_cached(SENDER_MESSAGES)?
        .query_map(params![key, limit], |r| Ok(MessageId(r.get(0)?)))?
        .collect::<Result<Vec<_>, _>>()?)
}

// -- settings sync ----------------------------------------------------------------------------

/// The content of the `contactPicture` record of `key`: `{"jpeg": <base64>}`.
pub(crate) fn synced_picture(conn: &Connection, key: &str) -> Result<Option<Value>, StoreError> {
    Ok(picture(conn, key)?.map(|p| json!({ "jpeg": base64(&p.image) })))
}

/// Writes (`Some`) or deletes (`None`) the picture of a `contactPicture` record. The engine
/// checked the content (base64 of a JPEG within the size limit). A picture from the sync is
/// shown only after this device's helper encoded it again (`display` stays `NULL`).
pub(crate) fn apply_synced_picture(
    conn: &Connection,
    key: &str,
    value: Option<&Value>,
) -> Result<usize, StoreError> {
    let Some(value) = value else {
        return remove_pictures(conn, &[key.to_owned()]);
    };
    let bad = |what: &str| StoreError::InvalidArgument(format!("contactPicture record: {what}"));
    let jpeg = value
        .get("jpeg")
        .and_then(Value::as_str)
        .and_then(unbase64)
        .ok_or_else(|| bad("no picture"))?;
    if jpeg.len() > mail_model::MAX_CONTACT_PICTURE_BYTES {
        return Err(bad("too large"));
    }
    if picture(conn, key)?.is_some_and(|p| p.image == jpeg) {
        return Ok(0);
    }
    let revision = next_revision(conn, key, now_millis())?;
    Ok(conn
        .prepare_cached(
            "INSERT INTO contact_picture (address_key, image, display, revision)
             VALUES (?1, ?2, NULL, ?3)
             ON CONFLICT (address_key) DO UPDATE SET image = excluded.image, display = NULL,
               revision = excluded.revision",
        )?
        .execute(params![key, jpeg, revision])?)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        let symbols = chunk.len() + 1;
        for i in 0..4 {
            out.push(if i < symbols {
                char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

/// Strict standard base64 (padding required, nothing else).
pub(crate) fn unbase64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| ALPHABET.iter().position(|a| *a == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let padding = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk[..4 - padding].iter().enumerate() {
            n |= value(*c)? << (18 - 6 * i);
        }
        for i in 0..3 - padding {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

// -- the store's API ---------------------------------------------------------------------------

impl Store {
    /// Stage 17: sets or clears the note of the contact with the address `key`.
    pub async fn set_contact_note(
        &self,
        key: String,
        note: Option<String>,
    ) -> Result<bool, StoreError> {
        let now = now_millis();
        self.write(move |tx| set_note(tx, &key, note.as_deref(), now))
            .await
    }

    /// Stage 17: stores a picture the helper encoded for `key`; the contact's other addresses
    /// (`others`) lose theirs. Returns the revision.
    pub async fn set_contact_picture(
        &self,
        key: String,
        others: Vec<String>,
        jpeg: Vec<u8>,
    ) -> Result<i64, StoreError> {
        let now = now_millis();
        self.write(move |tx| set_picture(tx, &key, &others, &jpeg, now))
            .await
    }

    /// Stage 17: removes the pictures of these addresses.
    pub async fn remove_contact_pictures(&self, keys: Vec<String>) -> Result<usize, StoreError> {
        self.write(move |tx| remove_pictures(tx, &keys)).await
    }

    /// Stage 17: the picture of the first of `keys` that has one.
    pub async fn contact_picture_of(
        &self,
        keys: Vec<String>,
    ) -> Result<Option<StoredPicture>, StoreError> {
        self.read(move |conn| picture_of(conn, &keys)).await
    }

    /// Stage 17: keeps the JPEG this device encoded for a synchronised picture.
    pub async fn set_contact_picture_display(
        &self,
        key: String,
        revision: i64,
        display: Vec<u8>,
    ) -> Result<bool, StoreError> {
        self.write(move |tx| set_display(tx, &key, revision, &display))
            .await
    }

    /// Stage 17: every stored picture (address key, revision).
    pub async fn contact_pictures(&self) -> Result<Vec<(String, i64)>, StoreError> {
        self.read(picture_list).await
    }

    /// Stage 17: contact rows after row id `after` for recomputing their kinds.
    pub async fn contact_kind_rows(
        &self,
        after: i64,
        limit: u32,
    ) -> Result<Vec<KindRow>, StoreError> {
        self.read(move |conn| kind_rows(conn, after, limit)).await
    }

    /// Stage 17: stores recomputed kinds (row id, kind code).
    pub async fn set_contact_kinds(&self, kinds: Vec<(i64, i64)>) -> Result<(), StoreError> {
        self.write(move |tx| set_kinds(tx, &kinds)).await
    }

    /// Stage 17: messages from the address `key` outside junk, trash, sent, drafts and "All
    /// Mail", each folder it lies in, newest first.
    pub async fn sender_locations(&self, key: String) -> Result<Vec<SenderLocation>, StoreError> {
        self.read(move |conn| sender_locations(conn, &key)).await
    }

    /// Stage 17: the newest `limit` messages from the address `key`.
    pub async fn sender_messages(
        &self,
        key: String,
        limit: u32,
    ) -> Result<Vec<MessageId>, StoreError> {
        self.read(move |conn| sender_messages(conn, &key, limit))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{base64, unbase64};

    #[test]
    fn base64_round_trips_and_refuses_what_is_not_strict() {
        for bytes in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"\xff\xd8\xff\x00\x10",
        ] {
            let text = base64(bytes);
            assert_eq!(unbase64(&text).as_deref(), Some(bytes), "{text}");
        }
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fo"), "Zm8=");
        for bad in ["Zm8", "Zm=8", "Zm 8=", "Z===", "Zm8=Zm8="] {
            assert_eq!(unbase64(bad), None, "{bad}");
        }
    }
}
