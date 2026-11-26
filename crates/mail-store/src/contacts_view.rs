//! Stage 17, block B (decision 0050): what the contacts view adds to the directory of stage 10.
//!
//! - **Notes** on the contact's head row (`contact.note`), synchronised in the `contact` record.
//! - **No pictures**: the pictures of stage 17 were removed in stage 22 (decision 0072); migration
//!   0028 dropped their table.
//! - **Kinds** (`contact.kind`): computed by the engine, recomputed for every row once after the
//!   update (rows of stage 10 that the backfill never touches).
//! - **A contact's mail**: messages from its addresses (`message_sender`), for the contacts
//!   view and for moving them to junk when the contact is marked as spam.

use mail_model::{AccountId, MailboxId, MessageId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::account_id_at;
use crate::settings::now_millis;
use crate::{Store, StoreError};

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
