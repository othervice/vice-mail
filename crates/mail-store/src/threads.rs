//! Threads in the database (decision 0013): [`ThreadLookup`] over the tables, applying a
//! [`ThreadAssignment`], threading existing messages, the thread lists and a thread's messages.
//! Synchronous functions over a connection; `search_api` runs them on the writer thread or a
//! reader.
//!
//! A new message is threaded in the transaction that stores it (`messages::store_headers`), so
//! a thread never misses one of its messages. Messages stored before stage 7 have no thread and
//! are threaded by [`backfill`] in ascending id order (date order: the canonical result).

use std::collections::{BTreeSet, HashMap};

use mail_model::threading::{
    ThreadAssignment, ThreadConfig, ThreadId, ThreadLookup, ThreadMessage, ThreadTarget,
    assign_thread, is_bulk, is_specific_subject, normalize_subject,
};
use mail_model::{Address, MessageHeader, MessageId};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::StoreError;
use crate::messages::addresses_at;

/// Ids of the subject window are dates; a collision moves an id up by a few, so the id range
/// reaches this far beyond the window (the dates are compared exactly afterwards).
const ID_SLACK: i64 = 24 * 60 * 60;

pub(crate) const REF_THREAD: &str =
    "SELECT thread_id FROM thread_ref WHERE account_id = ?1 AND ref_id = ?2";

pub(crate) const SUBJECTS_BY_MESSAGE_ID: &str =
    "SELECT subject FROM message WHERE account_id = ?1 AND message_id_hdr = ?2";

/// Candidates of the subject fallback: same subject key, in the window, with a thread.
pub(crate) const SUBJECT_CANDIDATES: &str = "
    SELECT id, thread_id, message_id_hdr, in_reply_to, references_hdr, subject, from_addr,
           to_addrs, cc_addrs, date, list_id, list_unsubscribe, precedence, auto_submitted
    FROM message
    WHERE account_id = ?1 AND subject_key = ?2 AND id >= ?3 AND id <= ?4";

const NEW_THREAD: &str = "
    INSERT INTO thread (account_id, subject_key, last_message_id, message_count)
    VALUES (?1, ?2, ?3, 0)";

pub(crate) const MOVE_MESSAGES: &str = "UPDATE message SET thread_id = ?1 WHERE thread_id = ?2";
pub(crate) const MOVE_REFS: &str = "UPDATE thread_ref SET thread_id = ?1 WHERE thread_id = ?2";
pub(crate) const THREAD_COUNTS: &str =
    "SELECT message_count, last_message_id FROM thread WHERE id = ?1";
pub(crate) const DELETE_THREAD: &str = "DELETE FROM thread WHERE id = ?1";

const LINK_ID: &str = "
    INSERT INTO thread_ref (account_id, ref_id, thread_id) VALUES (?1, ?2, ?3)
    ON CONFLICT (account_id, ref_id) DO UPDATE SET thread_id = excluded.thread_id";

pub(crate) const ADD_TO_THREAD: &str = "
    UPDATE thread SET message_count = message_count + ?2,
                      last_message_id = max(last_message_id, ?3)
    WHERE id = ?1";

pub(crate) const OWN_ADDRESSES: &str = "
    SELECT email FROM account WHERE id = ?1
    UNION ALL SELECT email FROM identity WHERE account_id = ?1";

/// Messages without a thread, oldest first (`message_thread` holds NULL first, by rowid).
pub(crate) const UNTHREADED: &str = "
    SELECT id, account_id, message_id_hdr, in_reply_to, references_hdr, subject, from_addr,
           to_addrs, cc_addrs, date, list_id, list_unsubscribe, precedence, auto_submitted
    FROM message WHERE thread_id IS NULL ORDER BY id LIMIT ?1";

pub(crate) const ANY_UNTHREADED: &str = "SELECT 1 FROM message WHERE thread_id IS NULL LIMIT 1";

pub(crate) const SET_THREAD: &str =
    "UPDATE message SET thread_id = ?2, subject_key = ?3 WHERE id = ?1";

/// The subject key to store: only for messages the subject fallback may pair with (decision
/// 0013: never bulk mail, only specific subjects), which keeps `message_subject` small.
fn column_key(message: &ThreadMessage) -> Option<String> {
    if message.bulk {
        return None;
    }
    let key = normalize_subject(&message.subject).key;
    is_specific_subject(&key).then_some(key)
}

/// Read access to one account's threads inside the current transaction.
struct Lookup<'c> {
    conn: &'c Connection,
    account: i64,
}

impl ThreadLookup for Lookup<'_> {
    type Error = StoreError;

    fn threads_with_ids(&self, ids: &[&str]) -> Result<Vec<ThreadId>, StoreError> {
        let mut stmt = self.conn.prepare_cached(REF_THREAD)?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(thread) = stmt
                .query_row(params![self.account, id], |r| r.get::<_, i64>(0))
                .optional()?
            {
                out.push(ThreadId(thread));
            }
        }
        Ok(out)
    }

    fn subjects_with_message_id(&self, id: &str) -> Result<Vec<String>, StoreError> {
        let rows = self
            .conn
            .prepare_cached(SUBJECTS_BY_MESSAGE_ID)?
            .query_map(params![self.account, id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn messages_with_subject(
        &self,
        key: &str,
        from: i64,
        until: i64,
    ) -> Result<Vec<(ThreadId, ThreadMessage)>, StoreError> {
        let rows = self
            .conn
            .prepare_cached(SUBJECT_CANDIDATES)?
            .query_map(
                params![self.account, key, from, until.saturating_add(ID_SLACK)],
                |r| {
                    let thread: Option<i64> = r.get(1)?;
                    Ok(thread.map(ThreadId).zip(Some(message_from_row(r, 0, 2)?)))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .flatten()
            .filter(|(_, m)| (from..=until).contains(&m.date))
            .collect())
    }
}

/// A [`ThreadMessage`] from a `message` row: `id` at `id_col`, then from `first` on
/// `message_id_hdr, in_reply_to, references_hdr, subject, from_addr, to_addrs, cc_addrs, date,
/// list_id, list_unsubscribe, precedence, auto_submitted`.
fn message_from_row(r: &Row<'_>, id_col: usize, first: usize) -> rusqlite::Result<ThreadMessage> {
    let references: Option<String> = r.get(first + 2)?;
    let from: Option<String> = r.get(first + 4)?;
    let to: Vec<Address> = addresses_at(r, first + 5)?;
    let cc: Vec<Address> = addresses_at(r, first + 6)?;
    let list_id: Option<String> = r.get(first + 8)?;
    let list_unsubscribe: Option<String> = r.get(first + 9)?;
    let precedence: Option<String> = r.get(first + 10)?;
    let auto_submitted: Option<String> = r.get(first + 11)?;
    Ok(ThreadMessage {
        message: MessageId(r.get(id_col)?),
        message_id: r.get(first)?,
        in_reply_to: r.get::<_, Option<String>>(first + 1)?.into_iter().collect(),
        references: references
            .map(|refs| refs.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
        subject: r.get(first + 3)?,
        participants: from
            .into_iter()
            .chain(to.into_iter().map(|a| a.email))
            .chain(cc.into_iter().map(|a| a.email))
            .collect(),
        date: r.get(first + 7)?,
        bulk: is_bulk(
            list_id.as_deref(),
            list_unsubscribe.as_deref(),
            precedence.as_deref(),
            auto_submitted.as_deref(),
        ),
    })
}

/// Threads messages of one transaction; caches the accounts' own addresses.
#[derive(Default)]
pub(crate) struct Threader {
    configs: HashMap<i64, ThreadConfig>,
}

impl Threader {
    fn config(&mut self, conn: &Connection, account: i64) -> Result<&ThreadConfig, StoreError> {
        match self.configs.entry(account) {
            std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let addresses = conn
                    .prepare_cached(OWN_ADDRESSES)?
                    .query_map([account], |r| r.get::<_, String>(0))?
                    .collect::<Result<BTreeSet<_>, _>>()?;
                Ok(entry.insert(ThreadConfig::new(addresses)))
            }
        }
    }

    /// Threads a new message that is about to be inserted with id `message` (its row does not
    /// exist yet). Returns the thread and the subject key to store with it.
    pub(crate) fn assign_new(
        &mut self,
        conn: &Connection,
        account: i64,
        message: i64,
        header: &MessageHeader,
    ) -> Result<(i64, Option<String>), StoreError> {
        let thread_message = ThreadMessage::from_header(MessageId(message), header);
        self.assign(conn, account, &thread_message)
    }

    fn assign(
        &mut self,
        conn: &Connection,
        account: i64,
        message: &ThreadMessage,
    ) -> Result<(i64, Option<String>), StoreError> {
        let config = self.config(conn, account)?.clone();
        let lookup = Lookup { conn, account };
        let assignment = assign_thread(&lookup, &config, message)?;
        if assignment.reused_message_id {
            tracing::debug!(
                message = message.message.0,
                "Message-ID reused with another subject; not used to link"
            );
        }
        let thread = apply(conn, account, &assignment, message.message.0)?;
        Ok((thread, column_key(message)))
    }
}

/// Writes `assignment` for message `message`: creates or picks the target thread, merges the
/// absorbed threads into it, registers the ids and counts the message.
fn apply(
    conn: &Connection,
    account: i64,
    assignment: &ThreadAssignment,
    message: i64,
) -> Result<i64, StoreError> {
    let thread = match assignment.target {
        ThreadTarget::Existing(thread) => thread.0,
        ThreadTarget::New => {
            conn.prepare_cached(NEW_THREAD)?
                .execute(params![account, assignment.subject_key, message])
                .map_err(StoreError::sqlite("create thread"))?;
            conn.last_insert_rowid()
        }
    };
    let mut added = 1_i64;
    let mut last = message;
    for absorbed in &assignment.absorbed {
        let counts: Option<(i64, i64)> = conn
            .prepare_cached(THREAD_COUNTS)?
            .query_row([absorbed.0], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        conn.prepare_cached(MOVE_MESSAGES)?
            .execute(params![thread, absorbed.0])
            .map_err(StoreError::sqlite("merge thread messages"))?;
        conn.prepare_cached(MOVE_REFS)?
            .execute(params![thread, absorbed.0])
            .map_err(StoreError::sqlite("merge thread ids"))?;
        conn.prepare_cached(DELETE_THREAD)?
            .execute([absorbed.0])
            .map_err(StoreError::sqlite("delete merged thread"))?;
        if let Some((count, newest)) = counts {
            added += count;
            last = last.max(newest);
        }
    }
    let mut link = conn.prepare_cached(LINK_ID)?;
    for id in &assignment.link_ids {
        link.execute(params![account, id, thread])
            .map_err(StoreError::sqlite("register thread id"))?;
    }
    conn.prepare_cached(ADD_TO_THREAD)?
        .execute(params![thread, added, last])
        .map_err(StoreError::sqlite("count thread message"))?;
    Ok(thread)
}

/// Threads up to `limit` messages that have no thread yet, oldest first. Returns how many.
pub(crate) fn backfill(conn: &Connection, limit: u32) -> Result<usize, StoreError> {
    let rows = conn
        .prepare_cached(UNTHREADED)?
        .query_map([i64::from(limit)], |r| {
            Ok((r.get::<_, i64>(1)?, message_from_row(r, 0, 2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut threader = Threader::default();
    let mut set = conn.prepare_cached(SET_THREAD)?;
    for (account, message) in &rows {
        let (thread, key) = threader.assign(conn, *account, message)?;
        set.execute(params![message.message.0, thread, key])
            .map_err(StoreError::sqlite("set thread"))?;
    }
    Ok(rows.len())
}
