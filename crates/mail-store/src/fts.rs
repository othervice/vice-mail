//! The full-text index `message_fts` (decision 0003, stage 7). Synchronous functions over a
//! connection; `search_api` runs them on the writer thread or a reader.
//!
//! **What is indexed.** Every message has one row, rowid = `message.id`. The header columns
//! (`subject`, `from`, `to`, `cc`) are written in the transaction that stores the message; once a
//! body is stored, the row is replaced with the same headers plus `body` (the text the reading
//! pane shows, at most [`MAX_BODY_TEXT_BYTES`]) and `attachments` (file names), in the
//! transaction that records the body. The text comes from the engine's [`BodyTextSource`] (the
//! store does not parse mail). A body removed from the cache leaves its text in the index: it
//! was local once, and opening the message fetches it again. Deleting the message deletes the
//! row (trigger `message_fts_delete`).
//!
//! **Order.** FTS5 starts a new segment whenever a rowid is smaller than the previous one in a
//! transaction, so every transaction writes its rows sorted by rowid (decision 0003).
//!
//! **The job.** Existing messages (after the migration) and a rebuild are filled in by a job
//! (`search_index_job`) in batches of ascending ids: a reader lists the next ids with their
//! body state, the body texts are made on the blocking pool, and one writer transaction writes
//! the rows and moves the job on. While the job targets `message_fts_new` (a rebuild), every
//! write above goes to both tables, so the new table misses nothing that happens meanwhile; at
//! the end it replaces `message_fts` in one transaction. Searches keep using the old table until
//! then. A batch row is skipped when the message's body changed after the batch was read (the
//! live write has the newer text) or the message is gone.

use mail_model::{Address, FetchedSection, MessageHeader, MessageStructure};
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;

/// The live index.
pub(crate) const LIVE: &str = "message_fts";
/// The table a rebuild fills.
pub(crate) const NEW: &str = "message_fts_new";

/// Longest body text indexed per message, in bytes (cut at a character boundary). Long
/// newsletters and logs would otherwise grow the index with little use for search.
pub const MAX_BODY_TEXT_BYTES: usize = 256 * 1024;

/// Text of a stored body for the index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BodyText {
    /// What the reading pane shows as text (the text part, else the text of the HTML part).
    pub body: String,
    /// File names of the attachments.
    pub attachments: Vec<String>,
    /// Stage 16: the message is a read receipt (RFC 8098); recorded for the message it reports
    /// on when a complete body is stored.
    pub receipt: Option<crate::ReceivedReceipt>,
}

/// Makes [`BodyText`] from stored bodies. The engine implements it with `mail-content`; the
/// store calls it on Tokio's blocking pool. It must not panic (a panic fails the store call).
pub trait BodyTextSource: Send + Sync {
    /// From a whole raw message.
    fn complete(&self, raw: &[u8]) -> BodyText;
    /// From the stored sections of a large message (text parts and headers) and its structure.
    fn partial(&self, structure: &MessageStructure, sections: &[FetchedSection]) -> BodyText;
}

const INSERT_LIVE: &str = "
    INSERT OR REPLACE INTO message_fts (rowid, subject, \"from\", \"to\", cc, body, attachments)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";
const INSERT_NEW: &str = "
    INSERT OR REPLACE INTO message_fts_new (rowid, subject, \"from\", \"to\", cc, body, attachments)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

pub(crate) const JOB: &str = "SELECT target, next_id FROM search_index_job WHERE id = 1";

/// The header columns of a stored message and its body state, for a body or job write.
pub(crate) const MESSAGE_FOR_INDEX: &str = "
    SELECT m.subject, m.from_name, m.from_addr, m.to_addrs, m.cc_addrs,
           (SELECT b.fetched_at FROM message_body b WHERE b.message_id = m.id)
    FROM message m WHERE m.id = ?1";

/// The next ids of a job with the state of their bodies. Scalar subqueries, not a LEFT JOIN:
/// with statistics of a nearly empty `message_body` the planner scanned it for every row.
pub(crate) const JOB_BATCH: &str = "
    SELECT m.id,
           (SELECT b.complete FROM message_body b WHERE b.message_id = m.id),
           (SELECT b.fetched_at FROM message_body b WHERE b.message_id = m.id)
    FROM message m
    WHERE m.id >= ?1 ORDER BY m.id LIMIT ?2";

/// Two subqueries: `min` and `max` together in one SELECT scan the table.
pub(crate) const ID_RANGE: &str =
    "SELECT (SELECT min(id) FROM message), (SELECT max(id) FROM message)";

const SET_NEXT: &str = "UPDATE search_index_job SET next_id = ?1 WHERE id = 1";

const CREATE_NEW: &str = "
    CREATE VIRTUAL TABLE message_fts_new USING fts5(
      subject, \"from\", \"to\", cc, body, attachments,
      content = '', contentless_delete = 1,
      tokenize = 'unicode61 remove_diacritics 2');
    CREATE TRIGGER message_fts_new_delete AFTER DELETE ON message BEGIN
      DELETE FROM message_fts_new WHERE rowid = old.id;
    END;";

/// The swap at the end of a rebuild: the new table takes the live name, the old one is dropped
/// in the next transaction. Triggers are recreated, not renamed (a rename would rewrite them to
/// point at the renamed tables).
const SWAP: &str = "
    DROP TRIGGER message_fts_new_delete;
    DROP TRIGGER message_fts_delete;
    ALTER TABLE message_fts RENAME TO message_fts_old;
    ALTER TABLE message_fts_new RENAME TO message_fts;
    CREATE TRIGGER message_fts_delete AFTER DELETE ON message BEGIN
      DELETE FROM message_fts WHERE rowid = old.id;
    END;
    DELETE FROM search_index_job;";

pub(crate) const DROP_OLD: &str = "DROP TABLE IF EXISTS message_fts_old";

/// Which table a job fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Existing messages after the migration, into the live table.
    Live,
    /// A rebuild into `message_fts_new`.
    New,
}

impl Target {
    fn parse(name: &str) -> Option<Self> {
        match name {
            LIVE => Some(Self::Live),
            NEW => Some(Self::New),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Live => LIVE,
            Self::New => NEW,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Job {
    pub(crate) target: Target,
    pub(crate) next_id: i64,
}

pub(crate) fn job(conn: &Connection) -> Result<Option<Job>, StoreError> {
    let row: Option<(String, i64)> = conn
        .prepare_cached(JOB)?
        .query_row([], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(row
        .and_then(|(target, next_id)| Target::parse(&target).map(|target| Job { target, next_id })))
}

/// One row of the index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) subject: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) cc: String,
    pub(crate) body: String,
    pub(crate) attachments: String,
}

impl Row {
    /// Header columns of a message as the adapter fetched it.
    pub(crate) fn from_header(h: &MessageHeader) -> Self {
        Self {
            subject: h.subject.clone(),
            // The list shows the first sender; the index takes them all.
            from: addresses_text(&h.from),
            to: addresses_text(&h.to),
            cc: addresses_text(&h.cc),
            body: String::new(),
            attachments: String::new(),
        }
    }

    fn with_text(mut self, text: &BodyText) -> Self {
        self.body = cut(&text.body, MAX_BODY_TEXT_BYTES).to_owned();
        self.attachments = text.attachments.join("\n");
        self
    }
}

/// `Name email` per address, one per line.
fn addresses_text(addresses: &[Address]) -> String {
    let mut out = String::new();
    for a in addresses {
        if !out.is_empty() {
            out.push('\n');
        }
        if let Some(name) = a.name.as_deref().filter(|n| !n.is_empty()) {
            out.push_str(name);
            out.push(' ');
        }
        out.push_str(&a.email);
    }
    out
}

fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Tables every index write goes to: the live one, and the new one while a rebuild runs.
pub(crate) fn targets(conn: &Connection) -> Result<Vec<Target>, StoreError> {
    Ok(match job(conn)? {
        Some(Job {
            target: Target::New,
            ..
        }) => vec![Target::Live, Target::New],
        _ => vec![Target::Live],
    })
}

/// Writes `rows` (sorted here by rowid) to `targets`, replacing existing rows.
pub(crate) fn write(
    conn: &Connection,
    targets: &[Target],
    rows: &mut [(i64, Row)],
) -> Result<(), StoreError> {
    rows.sort_by_key(|(id, _)| *id);
    for target in targets {
        let sql = match target {
            Target::Live => INSERT_LIVE,
            Target::New => INSERT_NEW,
        };
        let mut stmt = conn.prepare_cached(sql)?;
        for (id, row) in rows.iter() {
            stmt.execute(params![
                id,
                row.subject,
                row.from,
                row.to,
                row.cc,
                row.body,
                row.attachments
            ])
            .map_err(StoreError::sqlite("write full-text index"))?;
        }
    }
    Ok(())
}

/// The stored header columns of `message` and the `fetched_at` of its body; `None` if the
/// message does not exist.
pub(crate) fn stored_row(
    conn: &Connection,
    message: i64,
) -> Result<Option<(Row, Option<i64>)>, StoreError> {
    let row = conn
        .prepare_cached(MESSAGE_FOR_INDEX)?
        .query_row([message], |r| {
            let from_name: Option<String> = r.get(1)?;
            let from_addr: Option<String> = r.get(2)?;
            let from: Vec<Address> = from_addr
                .map(|email| Address {
                    name: from_name,
                    email,
                })
                .into_iter()
                .collect();
            Ok((
                r.get::<_, String>(0)?,
                from,
                crate::messages::addresses_at(r, 3)?,
                crate::messages::addresses_at(r, 4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })
        .optional()?;
    Ok(row.map(|(subject, from, to, cc, fetched)| {
        (
            Row {
                subject,
                from: addresses_text(&from),
                to: addresses_text(&to),
                cc: addresses_text(&cc),
                body: String::new(),
                attachments: String::new(),
            },
            fetched,
        )
    }))
}

/// Re-indexes messages whose body was just stored, with their texts, into every target.
pub(crate) fn index_bodies(conn: &Connection, texts: &[(i64, BodyText)]) -> Result<(), StoreError> {
    if texts.is_empty() {
        return Ok(());
    }
    let targets = targets(conn)?;
    let mut rows = Vec::with_capacity(texts.len());
    for (message, text) in texts {
        if let Some((row, _)) = stored_row(conn, *message)? {
            rows.push((*message, row.with_text(text)));
        }
    }
    write(conn, &targets, &mut rows)
}

/// One id of a job batch as a reader saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchItem {
    pub(crate) id: i64,
    /// `Some(complete)` if a body was stored.
    pub(crate) body: Option<bool>,
    /// `fetched_at` of that body.
    pub(crate) fetched_at: Option<i64>,
}

pub(crate) fn batch(
    conn: &Connection,
    from: i64,
    limit: u32,
) -> Result<Vec<BatchItem>, StoreError> {
    let rows = conn
        .prepare_cached(JOB_BATCH)?
        .query_map(params![from, i64::from(limit)], |r| {
            Ok(BatchItem {
                id: r.get(0)?,
                body: r.get(1)?,
                fetched_at: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Writes one batch of `job` (items in ascending id order, with the texts made from the bodies
/// the reader saw) and moves the job to `next_id`. Returns `false` (nothing written) if the job
/// is not the one the batch was read for any more.
pub(crate) fn write_batch(
    conn: &Connection,
    expected: Job,
    items: &[(BatchItem, Option<BodyText>)],
    next_id: i64,
) -> Result<bool, StoreError> {
    if job(conn)? != Some(expected) {
        return Ok(false);
    }
    let mut rows = Vec::with_capacity(items.len());
    for (item, text) in items {
        let Some((row, fetched_now)) = stored_row(conn, item.id)? else {
            continue;
        };
        // The body changed since the batch was read: the live write indexed the new one.
        if fetched_now.is_some() && fetched_now != item.fetched_at {
            continue;
        }
        let row = match (fetched_now, text) {
            (Some(_), Some(text)) => row.with_text(text),
            _ => row,
        };
        rows.push((item.id, row));
    }
    write(conn, &[expected.target], &mut rows)?;
    conn.prepare_cached(SET_NEXT)?
        .execute([next_id])
        .map_err(StoreError::sqlite("advance the index job"))?;
    Ok(true)
}

/// Ends `expected`: a rebuild swaps tables (returns `true`: drop the old table next), the
/// initial fill just ends.
pub(crate) fn finish(conn: &Connection, expected: Job) -> Result<bool, StoreError> {
    if job(conn)? != Some(expected) {
        return Ok(false);
    }
    match expected.target {
        Target::New => {
            conn.execute_batch(SWAP)
                .map_err(StoreError::sqlite("swap in the rebuilt index"))?;
            Ok(true)
        }
        Target::Live => {
            conn.execute("DELETE FROM search_index_job", [])
                .map_err(StoreError::sqlite("end the index job"))?;
            Ok(false)
        }
    }
}

/// Starts a rebuild into `message_fts_new`. Returns `false` if a job runs already.
pub(crate) fn start_rebuild(conn: &Connection, now_ms: i64) -> Result<bool, StoreError> {
    if job(conn)?.is_some() {
        return Ok(false);
    }
    conn.execute_batch(DROP_OLD)
        .map_err(StoreError::sqlite("drop a left-over index"))?;
    conn.execute_batch(CREATE_NEW)
        .map_err(StoreError::sqlite("create the rebuilt index"))?;
    conn.execute(
        "INSERT INTO search_index_job (id, target, next_id, started_at) VALUES (1, ?1, 0, ?2)",
        params![Target::New.name(), now_ms],
    )
    .map_err(StoreError::sqlite("start the index job"))?;
    Ok(true)
}

/// Lowest and highest message id.
pub(crate) fn id_range(conn: &Connection) -> Result<(Option<i64>, Option<i64>), StoreError> {
    Ok(conn
        .prepare_cached(ID_RANGE)?
        .query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?)
}
