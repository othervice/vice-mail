//! Stage 20, task C (decision 0060): the attachments of stored bodies (`attachment`), for the
//! "Attachments" view across accounts.
//!
//! **Writing.** The rows of a message are replaced in the transaction that indexes its body for
//! the search (`fts::index_bodies`, the job's `fts::write_batch`): the engine's
//! [`crate::BodyTextSource`] lists the attachments the way the reading pane does, so `idx` is
//! the pane's index.
//!
//! **Listing.** Newest message first, keyset by (message id, index): a walk of the primary key
//! backwards, or of `attachment_kind` for one kind; the account, the name and the sender are
//! filters on the walk, the message a primary-key lookup per row. A page stops after `limit`
//! rows; a very selective filter walks further (measured in `attachment_index_tests`).

use mail_model::{AccountId, Address, AttachmentKind, MailboxId, MessageId};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::StoreError;
use crate::accounts::account_id_at;

/// One attachment of a stored body as the reading pane lists it (its position is its index).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexedAttachment {
    /// Sanitised file name.
    pub name: String,
    pub mime_type: String,
    /// Decoded bytes (estimated from the encoded size for a part not stored).
    pub size: u64,
    /// The IMAP part of an attachment of a partial body (`1.2`); `None` in a whole message.
    pub part: Option<String>,
}

/// Longest name kept (characters); longer names are cut for the index only.
const MAX_NAME_CHARS: usize = 1_000;

/// Replaces the attachment rows of `message`.
pub(crate) fn write(
    conn: &Connection,
    message: i64,
    attachments: &[IndexedAttachment],
) -> Result<(), StoreError> {
    conn.prepare_cached("DELETE FROM attachment WHERE message_id = ?1")?
        .execute([message])
        .map_err(StoreError::sqlite("replace attachments"))?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO attachment (message_id, idx, name, name_key, mime_type, kind, size, part)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (index, attachment) in attachments.iter().enumerate() {
        let name: String = attachment.name.chars().take(MAX_NAME_CHARS).collect();
        let kind = AttachmentKind::classify(&name, &attachment.mime_type);
        insert
            .execute(params![
                message,
                i64::try_from(index).unwrap_or(i64::MAX),
                name,
                name.to_lowercase(),
                attachment.mime_type,
                kind.code(),
                i64::try_from(attachment.size).unwrap_or(i64::MAX),
                attachment.part,
            ])
            .map_err(StoreError::sqlite("index attachment"))?;
    }
    Ok(())
}

/// What the view asks for (the engine resolves the scope to accounts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentQuery {
    pub accounts: Vec<AccountId>,
    pub kind: Option<AttachmentKind>,
    /// Lower-case part of the sender's name or address.
    pub sender: Option<String>,
    /// Lower-case part of the file name.
    pub name: Option<String>,
}

/// One row of the view as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAttachment {
    pub message: MessageId,
    pub account: AccountId,
    pub mailbox: MailboxId,
    pub index: u32,
    pub name: String,
    pub mime_type: String,
    pub kind: AttachmentKind,
    pub size: u64,
    /// The bytes are stored (a whole body, or the part of a partial one).
    pub local: bool,
    pub from: Option<Address>,
    pub subject: String,
    /// Unix ms.
    pub date: i64,
}

/// The view's SQL for `query` over `accounts` account rows. Parameters: `?1` cursor message,
/// `?2` cursor index, `?3` limit, `?4` kind, `?5` name, `?6` sender, then the account rows.
pub(crate) fn list_sql(query: &AttachmentQuery, accounts: usize) -> String {
    let accounts: Vec<String> = (0..accounts).map(|i| format!("?{}", i + 7)).collect();
    let (index, kind) = if query.kind.is_some() {
        ("INDEXED BY attachment_kind", "a.kind = ?4 AND ")
    } else {
        ("", "")
    };
    let name = if query.name.is_some() {
        " AND instr(a.name_key, ?5) > 0"
    } else {
        ""
    };
    let sender = if query.sender.is_some() {
        " AND (instr(lower(coalesce(m.from_addr, '')), ?6) > 0
               OR instr(lower(coalesce(m.from_name, '')), ?6) > 0)"
    } else {
        ""
    };
    format!(
        "SELECT * FROM (
           SELECT a.message_id, a.idx, a.name, a.mime_type, a.kind, a.size,
                  (SELECT b.complete FROM message_body b WHERE b.message_id = a.message_id) = 1
                    OR (a.part IS NOT NULL AND EXISTS (
                          SELECT 1 FROM message_body_part p
                          WHERE p.message_id = a.message_id AND p.section = a.part)),
                  (SELECT uuid FROM account x WHERE x.id = m.account_id),
                  m.from_name, m.from_addr, m.subject, m.date,
                  coalesce(
                    (SELECT l.mailbox_id FROM message_location l INDEXED BY loc_message
                       CROSS JOIN mailbox b ON b.id = l.mailbox_id
                     WHERE l.message_id = a.message_id AND b.role = 'inbox' LIMIT 1),
                    (SELECT l.mailbox_id FROM message_location l INDEXED BY loc_message
                       CROSS JOIN mailbox b ON b.id = l.mailbox_id
                     WHERE l.message_id = a.message_id
                       AND (b.role IS NULL OR b.role NOT IN ('trash', 'junk')) LIMIT 1)
                  ) AS shown_in
           FROM attachment a {index} CROSS JOIN message m ON m.id = a.message_id
           WHERE {kind}(a.message_id, a.idx) < (?1, ?2)
             AND m.account_id IN ({accounts}){name}{sender}
           ORDER BY a.message_id DESC, a.idx DESC)
         WHERE shown_in IS NOT NULL LIMIT ?3",
        accounts = if accounts.is_empty() {
            "NULL".to_owned()
        } else {
            accounts.join(", ")
        },
    )
}

/// A page of the view and the cursor (message id, index) of the next one.
pub type AttachmentRows = (Vec<StoredAttachment>, Option<(MessageId, u32)>);

/// One page of the view below `cursor` (message id and index), at most `limit` rows; and the
/// cursor of the next page.
pub(crate) fn list(
    conn: &Connection,
    query: &AttachmentQuery,
    cursor: Option<(MessageId, u32)>,
    limit: u32,
) -> Result<AttachmentRows, StoreError> {
    if query.accounts.is_empty() || limit == 0 {
        return Ok((Vec::new(), None));
    }
    let mut rows_of_accounts: Vec<rusqlite::types::Value> = Vec::new();
    for account in &query.accounts {
        let row: Option<i64> = conn
            .prepare_cached("SELECT id FROM account WHERE uuid = ?1")?
            .query_row([account.to_string()], |r| r.get(0))
            .optional()?;
        if let Some(row) = row {
            rows_of_accounts.push(row.into());
        }
    }
    if rows_of_accounts.is_empty() {
        return Ok((Vec::new(), None));
    }
    let (before_message, before_index) =
        cursor.map_or((i64::MAX, i64::MAX), |(m, i)| (m.0, i64::from(i)));
    let mut values: Vec<rusqlite::types::Value> = vec![
        before_message.into(),
        before_index.into(),
        (i64::from(limit) + 1).into(),
        query.kind.map_or(-1, AttachmentKind::code).into(),
        query.name.clone().unwrap_or_default().into(),
        query.sender.clone().unwrap_or_default().into(),
    ];
    let accounts = rows_of_accounts.len();
    values.extend(rows_of_accounts);
    let mut rows = conn
        .prepare_cached(&list_sql(query, accounts))?
        .query_map(params_from_iter(values), |r| {
            let from_name: Option<String> = r.get(8)?;
            let from_addr: Option<String> = r.get(9)?;
            let date: i64 = r.get(11)?;
            Ok(StoredAttachment {
                message: MessageId(r.get(0)?),
                index: u32::try_from(r.get::<_, i64>(1)?).unwrap_or(u32::MAX),
                name: r.get(2)?,
                mime_type: r.get(3)?,
                kind: AttachmentKind::from_code(r.get(4)?).unwrap_or(AttachmentKind::Other),
                size: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
                local: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
                account: account_id_at(r, 7)?,
                from: from_addr.map(|email| Address {
                    name: from_name,
                    email,
                }),
                subject: r.get(10)?,
                date: date.saturating_mul(1000),
                mailbox: MailboxId(r.get(12)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|r| (r.message, r.index))
    } else {
        None
    };
    Ok((rows, next))
}

impl crate::Store {
    /// Stage 20: one page of the attachments view (see the module docs).
    pub async fn list_attachments(
        &self,
        query: AttachmentQuery,
        cursor: Option<(MessageId, u32)>,
        limit: u32,
    ) -> Result<AttachmentRows, StoreError> {
        self.read(move |c| list(c, &query, cursor, limit)).await
    }
}
