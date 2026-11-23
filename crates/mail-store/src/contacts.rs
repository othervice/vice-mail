//! The contact directory (stage 5 autocomplete, decision 0016), identity signatures and the
//! payloads the compose sweep needs. Synchronous functions over a connection; `compose_api`
//! runs them.
//!
//! The directory logic (normalising, names, ranking, folding) lives in `mail_content`; the store
//! keeps its rows: one per address, `rank` with an index, `search` with the folded words
//! (` word word …`, a leading space so `instr(search, ' ' || ?)` is a word-prefix match).
//! A search walks `contact_rank` from the top and stops after `limit` hits.

use mail_model::{
    AccountId, Address, ContactNameRecord, Identity, IdentityId, MAX_TEXT_SIGNATURE_BYTES,
    MessageId, OperationPayload,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::StoreError;
use crate::accounts::{self, account_id_at, conversion_error};
use crate::compose_api::{ContactSource, StoredContact};
use crate::outbox::{json_error, payload_at};

/// Most words of one contact search.
pub const MAX_SEARCH_WORDS: usize = 8;

/// Stored for "no messages" (a REAL column cannot sort `-inf` reliably everywhere).
const NO_RANK: f64 = -1.0e308;

pub(crate) const CONTACT_COLUMNS: &str = "address_key, local_part, domain, contact_group, sent, \
     received, last_sent, last_received, rank, names, user_name, search";

pub(crate) const CONTACT_BY_KEY: &str = "
    SELECT address_key, local_part, domain, contact_group, sent, received, last_sent,
           last_received, rank, names, user_name, search
    FROM contact WHERE address_key = ?1";

pub(crate) const UPSERT_CONTACT: &str = "
    INSERT INTO contact (address_key, local_part, domain, contact_group, sent, received,
                         last_sent, last_received, rank, names, user_name, search, updated_at)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
    ON CONFLICT (address_key) DO UPDATE SET
      local_part = excluded.local_part, domain = excluded.domain, sent = excluded.sent,
      received = excluded.received, last_sent = excluded.last_sent,
      last_received = excluded.last_received, rank = excluded.rank, names = excluded.names,
      search = excluded.search, updated_at = excluded.updated_at";

/// Messages not yet read into the directory (`message_contacts_todo`), oldest id first.
pub(crate) const CONTACTS_TODO: &str = "
    SELECT m.id, (SELECT a.uuid FROM account a WHERE a.id = m.account_id), m.from_name,
           m.from_addr, m.to_addrs, m.cc_addrs, m.subject, m.list_id, m.list_unsubscribe,
           m.auto_submitted, m.precedence, m.date
    FROM message m INDEXED BY message_contacts_todo
    WHERE m.contacts_seen IS NULL ORDER BY m.id LIMIT ?1";

pub(crate) const MARK_CONTACTS_SEEN: &str = "UPDATE message SET contacts_seen = 1 WHERE id = ?1";

/// Stage 10: the text of the identity's default signature for new mails stands in for the
/// stage 5 text signature.
pub(crate) const COMPOSE_IDENTITIES: &str = "
    SELECT id, email, name, reply_to, is_default,
           (SELECT s.text FROM signature s WHERE s.uuid = identity.signature_new)
    FROM identity
    WHERE account_id = ?1 ORDER BY is_default DESC, id";

/// Unfinished sends and appends, for the sweep of compose files (`operation_pending` covers the
/// pending ones; failed and conflicting ones are few, the scan of `operation` is the table
/// of one device's outbox).
pub(crate) const UNFINISHED_COMPOSE: &str = "
    SELECT payload FROM operation
    WHERE kind IN ('send', 'append') AND state != 'confirmed'";

/// The search query for `words` words: every word must start a word of `search`.
pub(crate) fn search_sql(words: usize) -> String {
    let mut sql = format!("SELECT {CONTACT_COLUMNS} FROM contact INDEXED BY contact_rank WHERE ");
    for i in 0..words.max(1) {
        if i > 0 {
            sql.push_str(" AND ");
        }
        sql.push_str(&format!("instr(search, ' ' || ?{}) > 0", i + 1));
    }
    sql.push_str(&format!(" ORDER BY rank DESC LIMIT ?{}", words.max(1) + 1));
    sql
}

fn names_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<Vec<ContactNameRecord>> {
    let text: String = r.get(idx)?;
    serde_json::from_str(&text).map_err(|err| conversion_error(idx, Type::Text, err.to_string()))
}

fn count_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<u32> {
    let n: i64 = r.get(idx)?;
    Ok(u32::try_from(n.max(0)).unwrap_or(u32::MAX))
}

fn contact_from_row(r: &Row<'_>) -> rusqlite::Result<StoredContact> {
    contact_at(r, 0)
}

/// A contact from [`CONTACT_COLUMNS`] starting at column `o`.
pub(crate) fn contact_at(r: &Row<'_>, o: usize) -> rusqlite::Result<StoredContact> {
    let rank: f64 = r.get(o + 8)?;
    Ok(StoredContact {
        key: r.get(o)?,
        local_part: r.get(o + 1)?,
        domain: r.get(o + 2)?,
        group: r.get(o + 3)?,
        sent: count_at(r, o + 4)?,
        received: count_at(r, o + 5)?,
        last_sent: r.get(o + 6)?,
        last_received: r.get(o + 7)?,
        rank: if rank <= NO_RANK {
            f64::NEG_INFINITY
        } else {
            rank
        },
        names: names_at(r, o + 9)?,
        user_name: r.get(o + 10)?,
        search: r.get(o + 11)?,
    })
}

pub(crate) fn by_keys(
    conn: &Connection,
    keys: &[String],
) -> Result<Vec<StoredContact>, StoreError> {
    let mut stmt = conn.prepare_cached(CONTACT_BY_KEY)?;
    let mut out = Vec::new();
    for key in keys {
        if let Some(contact) = stmt.query_row([key], contact_from_row).optional()? {
            out.push(contact);
        }
    }
    Ok(out)
}

pub(crate) fn record(
    conn: &Connection,
    contacts: &[StoredContact],
    seen: &[MessageId],
    now: i64,
) -> Result<(), StoreError> {
    let mut upsert = conn.prepare_cached(UPSERT_CONTACT)?;
    for c in contacts {
        let names = serde_json::to_string(&c.names).map_err(json_error("encode contact names"))?;
        let rank = if c.rank.is_finite() { c.rank } else { NO_RANK };
        upsert
            .execute(params![
                c.key,
                c.local_part,
                c.domain,
                c.group,
                c.sent,
                c.received,
                c.last_sent,
                c.last_received,
                rank,
                names,
                c.user_name,
                c.search,
                now,
            ])
            .map_err(StoreError::sqlite("write contact"))?;
    }
    // Stage 10: a name or merge synchronised from another device before this device saw the
    // address applies now (the directory is a cache; the record waited in `sync_record`).
    for c in contacts
        .iter()
        .filter(|c| c.user_name.is_none() && c.group.is_none())
    {
        crate::contacts_dir::apply_waiting_record(conn, &c.key)?;
    }
    let mut mark = conn.prepare_cached(MARK_CONTACTS_SEEN)?;
    for message in seen {
        mark.execute([message.0])
            .map_err(StoreError::sqlite("mark contacts seen"))?;
    }
    Ok(())
}

pub(crate) fn search(
    conn: &Connection,
    words: &[String],
    limit: u32,
) -> Result<Vec<StoredContact>, StoreError> {
    let words: Vec<&String> = words
        .iter()
        .filter(|w| !w.is_empty())
        .take(MAX_SEARCH_WORDS)
        .collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let sql = search_sql(words.len());
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut values: Vec<&dyn rusqlite::ToSql> =
        words.iter().map(|w| *w as &dyn rusqlite::ToSql).collect();
    values.push(&limit);
    Ok(stmt
        .query_map(values.as_slice(), contact_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn addresses_json(text: Option<String>) -> Vec<Address> {
    text.and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(crate) fn todo(conn: &Connection, limit: u32) -> Result<Vec<ContactSource>, StoreError> {
    Ok(conn
        .prepare_cached(CONTACTS_TODO)?
        .query_map([limit], |r| {
            let from_name: Option<String> = r.get(2)?;
            let from_addr: Option<String> = r.get(3)?;
            Ok(ContactSource {
                message: MessageId(r.get(0)?),
                account: account_id_at(r, 1)?,
                from: from_addr.map(|email| Address {
                    name: from_name,
                    email,
                }),
                to: addresses_json(r.get(4)?),
                cc: addresses_json(r.get(5)?),
                subject: r.get(6)?,
                list_id: r.get(7)?,
                list_unsubscribe: r.get(8)?,
                auto_submitted: r.get(9)?,
                precedence: r.get(10)?,
                date: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn compose_identities(
    conn: &Connection,
    account: AccountId,
) -> Result<Vec<(Identity, Option<String>)>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    Ok(conn
        .prepare_cached(COMPOSE_IDENTITIES)?
        .query_map([row], |r| {
            Ok((
                Identity {
                    id: IdentityId(r.get(0)?),
                    account_id: account,
                    email: r.get(1)?,
                    name: r.get(2)?,
                    reply_to: r.get(3)?,
                    is_default: r.get(4)?,
                },
                r.get(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// The stage 5 text signature: since stage 10 a signature with the identity's UUID that both
/// defaults of the identity name (`crate::signatures::set_text_signature`).
pub(crate) fn set_signature(
    conn: &Connection,
    identity: IdentityId,
    signature: Option<&str>,
) -> Result<AccountId, StoreError> {
    let signature = signature
        .map(|s| s.replace("\r\n", "\n"))
        .filter(|s| !s.trim().is_empty());
    if signature
        .as_ref()
        .is_some_and(|s| s.len() > MAX_TEXT_SIGNATURE_BYTES)
    {
        return Err(StoreError::InvalidArgument(format!(
            "a signature has at most {MAX_TEXT_SIGNATURE_BYTES} bytes"
        )));
    }
    crate::signatures::set_text_signature(
        conn,
        identity,
        None,
        signature.as_deref(),
        crate::settings::now_millis(),
    )
}

pub(crate) fn unfinished_payloads(conn: &Connection) -> Result<Vec<OperationPayload>, StoreError> {
    Ok(conn
        .prepare_cached(UNFINISHED_COMPOSE)?
        .query_map([], |r| payload_at(r, 0))?
        .collect::<Result<Vec<_>, _>>()?)
}
