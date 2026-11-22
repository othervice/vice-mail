//! Body rows (`message_body`, `message_body_part`). Synchronous functions over a connection;
//! `body_api` runs them on the writer thread or a reader, and does the file work in between.
//!
//! A file can hold several sections and several messages (what was written together). It may
//! be removed only when no row refers to it any more: every function that drops rows returns
//! the files that became unreferenced, checked through the index `body_part_file` in the same
//! transaction.

use std::collections::BTreeSet;

use mail_model::{BodySection, MessageId, MessageStructure};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::accounts::conversion_error;

/// Section text of a complete body (the whole message).
pub(crate) const WHOLE: &str = "";

/// Account UUID of a message, for its body directory. A scalar subquery (see
/// `messages::MAILBOX_ACCOUNT` for why not a join).
pub(crate) const ACCOUNT_OF_MESSAGE: &str = "
    SELECT (SELECT a.uuid FROM account a WHERE a.id = m.account_id) FROM message m WHERE m.id = ?1";

pub(crate) const BODY_ROW: &str = "
    SELECT complete, structure, raw_size, stored_size, fetched_at
    FROM message_body WHERE message_id = ?1";

pub(crate) const BODY_PARTS: &str = "
    SELECT section, file, byte_offset, byte_length
    FROM message_body_part WHERE message_id = ?1 ORDER BY section";

/// Files of a message, one row per section (callers dedupe; DISTINCT would need a temp B-tree).
pub(crate) const BODY_FILES: &str = "SELECT file FROM message_body_part WHERE message_id = ?1";

/// Whether any row still refers to a file.
pub(crate) const FILE_REFERENCED: &str = "SELECT 1 FROM message_body_part WHERE file = ?1 LIMIT 1";

const DELETE_BODY: &str = "DELETE FROM message_body WHERE message_id = ?1";

const INSERT_BODY: &str = "
    INSERT INTO message_body (message_id, complete, structure, raw_size, stored_size, fetched_at)
    VALUES (?1, ?2, ?3, 0, 0, ?4)";

const UPDATE_PARTIAL: &str =
    "UPDATE message_body SET structure = ?2, fetched_at = ?3 WHERE message_id = ?1";

const UPSERT_PART: &str = "
    INSERT INTO message_body_part (message_id, section, file, byte_offset, byte_length,
                                   stored_bytes)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
    ON CONFLICT (message_id, section) DO UPDATE
      SET file = excluded.file, byte_offset = excluded.byte_offset,
          byte_length = excluded.byte_length, stored_bytes = excluded.stored_bytes";

/// Sizes of a body from its parts.
const UPDATE_SIZES: &str = "
    UPDATE message_body SET
      raw_size = (SELECT coalesce(sum(byte_length), 0) FROM message_body_part
                  WHERE message_id = ?1),
      stored_size = (SELECT coalesce(sum(stored_bytes), 0) FROM message_body_part
                     WHERE message_id = ?1)
    WHERE message_id = ?1";

const DELETE_PARTS_IN_FILE: &str =
    "DELETE FROM message_body_part WHERE message_id = ?1 AND file = ?2";

const PART_COUNT: &str = "SELECT count(*) FROM message_body_part WHERE message_id = ?1";

/// Files of the account's messages that lie in no mailbox (deleted with them by
/// `messages::DELETE_ORPHANS`); one row per section.
pub(crate) const ORPHAN_FILES: &str = "
    SELECT p.file
    FROM message m CROSS JOIN message_body_part p ON p.message_id = m.id
    WHERE m.account_id = ?1
      AND NOT EXISTS (SELECT 1 FROM message_location l WHERE l.message_id = m.id)
      AND NOT EXISTS (SELECT 1 FROM operation o WHERE o.message_id = m.id
                      AND o.state IN ('queued', 'in_flight', 'confirmed'))";

/// Every referenced file, for the sweep: a scan of the covering index `body_part_file` on
/// purpose (once at start).
pub(crate) const ALL_FILES: &str = "SELECT DISTINCT file FROM message_body_part";

/// Stored body of a message (see [`crate::Store::body_info`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyInfo {
    pub message: MessageId,
    /// The whole raw message is stored. Otherwise only some sections of a large message.
    pub complete: bool,
    /// MIME structure of a partial body (from BODYSTRUCTURE); `None` for a complete one, whose
    /// structure the parser reads from the message itself.
    pub structure: Option<MessageStructure>,
    /// Stored sections of a partial body, sorted; empty for a complete one.
    pub sections: Vec<BodySection>,
    /// Uncompressed bytes of everything stored.
    pub raw_size: u64,
    /// Bytes on disk: its share of the body files (several small bodies can share one).
    pub stored_size: u64,
    /// Unix milliseconds of the last store.
    pub fetched_at: i64,
}

/// One stored section and where it lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartRow {
    /// `None` for the whole message.
    pub(crate) section: Option<BodySection>,
    pub(crate) file: String,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

/// Where a piece written to a new file lies, and its share of the file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) offset: usize,
    pub(crate) length: usize,
    pub(crate) stored_bytes: u64,
}

/// A section about to be recorded.
pub(crate) struct NewPart {
    pub(crate) section: BodySection,
    pub(crate) placement: Placement,
}

/// Result of a body write: the new state and the files no row refers to any more.
pub(crate) struct Recorded {
    pub(crate) info: Option<BodyInfo>,
    pub(crate) unreferenced: Vec<String>,
}

pub(crate) fn account_of(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<String>, StoreError> {
    Ok(conn
        .prepare_cached(ACCOUNT_OF_MESSAGE)?
        .query_row([message.0], |r| r.get::<_, String>(0))
        .optional()?)
}

fn message_exists(conn: &Connection, message: MessageId) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached("SELECT id FROM message WHERE id = ?1")?
        .query_row([message.0], |r| r.get::<_, i64>(0))
        .optional()?
        .is_some())
}

fn files_of(conn: &Connection, message: MessageId) -> Result<BTreeSet<String>, StoreError> {
    let files = conn
        .prepare_cached(BODY_FILES)?
        .query_map([message.0], |r| r.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(files)
}

/// Those of `candidates` that no row refers to.
pub(crate) fn unreferenced(
    conn: &Connection,
    candidates: impl IntoIterator<Item = String>,
) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare_cached(FILE_REFERENCED)?;
    let mut out = Vec::new();
    for file in candidates.into_iter().collect::<BTreeSet<_>>() {
        if stmt.query_row([&file], |_| Ok(())).optional()?.is_none() {
            out.push(file);
        }
    }
    Ok(out)
}

fn to_i64(value: usize, what: &str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::InvalidArgument(format!("{what} {value}")))
}

fn upsert_part(
    conn: &Connection,
    message: MessageId,
    section: &str,
    file: &str,
    placement: Placement,
) -> Result<(), StoreError> {
    conn.prepare_cached(UPSERT_PART)?
        .execute(params![
            message.0,
            section,
            file,
            to_i64(placement.offset, "section offset")?,
            to_i64(placement.length, "section length")?,
            i64::try_from(placement.stored_bytes).unwrap_or(i64::MAX),
        ])
        .map_err(StoreError::sqlite("insert body part"))?;
    Ok(())
}

fn update_sizes(conn: &Connection, message: MessageId) -> Result<(), StoreError> {
    conn.prepare_cached(UPDATE_SIZES)?
        .execute([message.0])
        .map_err(StoreError::sqlite("body sizes"))?;
    Ok(())
}

/// Records a complete body at `placement` in `file` (already written), replacing whatever was
/// stored. `info: None` if the message does not exist (any more); then nothing is recorded.
/// The returned files may include `file` itself only if nothing else refers to it.
pub(crate) fn record_complete(
    conn: &Connection,
    message: MessageId,
    file: &str,
    placement: Placement,
    now_ms: i64,
) -> Result<Recorded, StoreError> {
    if !message_exists(conn, message)? {
        return Ok(Recorded {
            info: None,
            unreferenced: Vec::new(),
        });
    }
    let before = files_of(conn, message)?;
    conn.prepare_cached(DELETE_BODY)?
        .execute([message.0])
        .map_err(StoreError::sqlite("replace body"))?;
    conn.prepare_cached(INSERT_BODY)?
        .execute(params![message.0, true, Option::<String>::None, now_ms])
        .map_err(StoreError::sqlite("insert body"))?;
    upsert_part(conn, message, WHOLE, file, placement)?;
    update_sizes(conn, message)?;
    Ok(Recorded {
        info: info(conn, message)?,
        unreferenced: unreferenced(conn, before)?,
    })
}

/// Adds sections stored in `file` (already written) to the partial body of `message`,
/// replacing earlier copies of the same sections. A complete body wins: then nothing changes.
/// `info: None` if the message does not exist.
pub(crate) fn record_sections(
    conn: &Connection,
    message: MessageId,
    structure: &MessageStructure,
    file: &str,
    parts: &[NewPart],
    now_ms: i64,
) -> Result<Recorded, StoreError> {
    let complete = info(conn, message)?.filter(|i| i.complete);
    if complete.is_some() || !message_exists(conn, message)? {
        return Ok(Recorded {
            info: complete,
            unreferenced: unreferenced(conn, [file.to_owned()])?,
        });
    }
    let json = serde_json::to_string(structure)
        .map_err(|err| StoreError::InvalidArgument(format!("message structure: {err}")))?;
    let before = files_of(conn, message)?;
    let updated = conn
        .prepare_cached(UPDATE_PARTIAL)?
        .execute(params![message.0, json, now_ms])
        .map_err(StoreError::sqlite("update body"))?;
    if updated == 0 {
        conn.prepare_cached(INSERT_BODY)?
            .execute(params![message.0, false, json, now_ms])
            .map_err(StoreError::sqlite("insert body"))?;
    }
    for part in parts {
        upsert_part(conn, message, &part.section.spec(), file, part.placement)?;
    }
    update_sizes(conn, message)?;
    let mut candidates = before;
    // Every section empty: nothing may point at the new file.
    candidates.insert(file.to_owned());
    Ok(Recorded {
        info: info(conn, message)?,
        unreferenced: unreferenced(conn, candidates)?,
    })
}

/// The body state of `message`, `None` if nothing is stored.
pub(crate) fn info(conn: &Connection, message: MessageId) -> Result<Option<BodyInfo>, StoreError> {
    let row = conn
        .prepare_cached(BODY_ROW)?
        .query_row([message.0], |r| {
            let structure: Option<String> = r.get(1)?;
            let structure = structure
                .map(|json| {
                    serde_json::from_str::<MessageStructure>(&json)
                        .map_err(|err| conversion_error(1, Type::Text, err.to_string()))
                })
                .transpose()?;
            Ok(BodyInfo {
                message,
                complete: r.get(0)?,
                structure,
                sections: Vec::new(),
                raw_size: r.get::<_, i64>(2)?.try_into().unwrap_or(0),
                stored_size: r.get::<_, i64>(3)?.try_into().unwrap_or(0),
                fetched_at: r.get(4)?,
            })
        })
        .optional()?;
    let Some(mut info) = row else {
        return Ok(None);
    };
    info.sections = parts(conn, message)?
        .into_iter()
        .filter_map(|p| p.section)
        .collect();
    info.sections.sort();
    Ok(Some(info))
}

/// Every stored section of `message` with its file.
pub(crate) fn parts(conn: &Connection, message: MessageId) -> Result<Vec<PartRow>, StoreError> {
    let rows = conn
        .prepare_cached(BODY_PARTS)?
        .query_map([message.0], |r| {
            let section: String = r.get(0)?;
            let section = if section == WHOLE {
                None
            } else {
                Some(
                    section
                        .parse::<BodySection>()
                        .map_err(|err| conversion_error(0, Type::Text, err))?,
                )
            };
            let offset: i64 = r.get(2)?;
            let length: i64 = r.get(3)?;
            Ok(PartRow {
                section,
                file: r.get(1)?,
                offset: usize::try_from(offset)
                    .map_err(|err| conversion_error(2, Type::Integer, err.to_string()))?,
                length: usize::try_from(length)
                    .map_err(|err| conversion_error(3, Type::Integer, err.to_string()))?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Deletes the body of `message`. Returns whether there was one, and the files that became
/// unreferenced (to remove after the commit).
pub(crate) fn delete(
    conn: &Connection,
    message: MessageId,
) -> Result<(bool, Vec<String>), StoreError> {
    let files = files_of(conn, message)?;
    let deleted = conn
        .prepare_cached(DELETE_BODY)?
        .execute([message.0])
        .map_err(StoreError::sqlite("delete body"))?;
    Ok((deleted > 0, unreferenced(conn, files)?))
}

/// Forgets the sections of `message` that lie in `file` (missing or damaged on disk). A body
/// left without sections goes entirely. Returns whether anything was forgotten (a concurrent
/// re-fetch may have replaced the file already) and whether `file` is now unreferenced (other
/// messages packed into it find out on their own read).
pub(crate) fn forget_file(
    conn: &Connection,
    message: MessageId,
    file: &str,
) -> Result<(bool, bool), StoreError> {
    let removed = conn
        .prepare_cached(DELETE_PARTS_IN_FILE)?
        .execute(params![message.0, file])
        .map_err(StoreError::sqlite("forget body file"))?;
    if removed == 0 {
        return Ok((false, false));
    }
    let left: i64 = conn
        .prepare_cached(PART_COUNT)?
        .query_row([message.0], |r| r.get(0))?;
    if left == 0 {
        conn.prepare_cached(DELETE_BODY)?
            .execute([message.0])
            .map_err(StoreError::sqlite("delete body"))?;
    } else {
        update_sizes(conn, message)?;
    }
    Ok((true, !unreferenced(conn, [file.to_owned()])?.is_empty()))
}

/// Files of the account's orphan messages, before deleting them in the same transaction.
pub(crate) fn orphan_files(
    conn: &Connection,
    account_row: i64,
) -> Result<BTreeSet<String>, StoreError> {
    let files = conn
        .prepare_cached(ORPHAN_FILES)?
        .query_map([account_row], |r| r.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(files)
}

pub(crate) fn all_files(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let files = conn
        .prepare_cached(ALL_FILES)?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(files)
}
