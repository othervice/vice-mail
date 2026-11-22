//! Schema draft and connection setup.

use rusqlite::{Connection, OpenFlags, Result};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tokenizer {
    Unicode61,
    Trigram,
}

impl Tokenizer {
    pub fn spec(self) -> &'static str {
        match self {
            Tokenizer::Unicode61 => "unicode61 remove_diacritics 2",
            Tokenizer::Trigram => "trigram remove_diacritics 1",
        }
    }
}

pub const BASE_SCHEMA: &str = r#"
CREATE TABLE account (
  id             INTEGER PRIMARY KEY,
  uuid           TEXT NOT NULL UNIQUE,
  address        TEXT NOT NULL,
  display_name   TEXT NOT NULL,
  color          TEXT,
  writes_enabled INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE mailbox (
  id            INTEGER PRIMARY KEY,
  account_id    INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  delimiter     TEXT,
  parent_id     INTEGER REFERENCES mailbox(id) ON DELETE CASCADE,
  role          TEXT,              -- inbox, sent, drafts, archive, trash, junk, all; NULL = custom
  uidvalidity   INTEGER,
  uidnext       INTEGER,
  highestmodseq INTEGER,
  UNIQUE (account_id, name)
) STRICT;
CREATE INDEX mailbox_parent ON mailbox(parent_id);

-- One row per mail, even if it lives in several folders.
-- id is time-ordered: the sort date in seconds, +1 on a collision. Newer mail => larger id, so
-- "ORDER BY id DESC" is "newest first" for lists and for FTS5 (rowid = id).
CREATE TABLE message (
  id              INTEGER PRIMARY KEY,
  account_id      INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  message_id_hdr  TEXT,
  thread_id       INTEGER,
  date            INTEGER NOT NULL,  -- unix seconds, the sort date encoded in id
  subject         TEXT NOT NULL,
  from_name       TEXT,
  from_addr       TEXT,
  to_addrs        TEXT,
  cc_addrs        TEXT,
  size            INTEGER NOT NULL,
  preview         TEXT,
  has_attachments INTEGER NOT NULL DEFAULT 0,
  category        INTEGER,
  body_ref        TEXT               -- path of the compressed body file, NULL = not cached
) STRICT;
CREATE INDEX message_msgid  ON message(account_id, message_id_hdr);
CREATE INDEX message_thread ON message(thread_id);

-- Where a mail lives: one row per (folder, mail). Flags belong to the location, as in IMAP.
-- Clustered on (mailbox_id, message_id) so a list page is one contiguous range read.
CREATE TABLE message_location (
  mailbox_id  INTEGER NOT NULL REFERENCES mailbox(id) ON DELETE CASCADE,
  message_id  INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  uid         INTEGER NOT NULL,
  seen        INTEGER NOT NULL DEFAULT 0,
  flagged     INTEGER NOT NULL DEFAULT 0,
  answered    INTEGER NOT NULL DEFAULT 0,
  draft       INTEGER NOT NULL DEFAULT 0,
  deleted     INTEGER NOT NULL DEFAULT 0,
  keywords    TEXT,
  modseq      INTEGER,
  PRIMARY KEY (mailbox_id, message_id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX loc_uid ON message_location(mailbox_id, uid);
CREATE INDEX loc_message ON message_location(message_id);
CREATE INDEX loc_unseen  ON message_location(mailbox_id, message_id) WHERE seen = 0;

CREATE TABLE attachment (
  id          INTEGER PRIMARY KEY,
  message_id  INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  part        TEXT NOT NULL,
  filename    TEXT,
  mime_type   TEXT,
  size        INTEGER,
  content_id  TEXT
) STRICT;
CREATE INDEX attachment_message ON attachment(message_id);
"#;

pub fn fts_schema(tok: Tokenizer) -> String {
    format!(
        r#"
-- Contentless: the text lives in message / body files, the index only holds tokens.
-- contentless_delete=1 allows DELETE and INSERT OR REPLACE by rowid (re-index when a body arrives).
CREATE VIRTUAL TABLE message_fts USING fts5(
  subject, sender, recipients, body, attachments,
  content='', contentless_delete=1,
  tokenize="{}"
);
CREATE TRIGGER message_fts_delete AFTER DELETE ON message BEGIN
  DELETE FROM message_fts WHERE rowid = old.id;
END;
"#,
        tok.spec()
    )
}

/// Pragmas for the single writer connection.
pub const WRITER_PRAGMAS: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -65536;
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;
PRAGMA journal_size_limit = 67108864;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA busy_timeout = 5000;
"#;

/// Pragmas for each reader connection of the pool.
pub const READER_PRAGMAS: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -32768;
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;
PRAGMA busy_timeout = 5000;
PRAGMA query_only = 1;
"#;

pub fn open_writer(path: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.execute_batch("PRAGMA page_size = 4096;")?;
    c.execute_batch(WRITER_PRAGMAS)?;
    if let Some(mb) = std::env::var("SPIKE_CACHE_MB").ok().and_then(|s| s.parse::<i64>().ok()) {
        c.execute_batch(&format!("PRAGMA cache_size = -{};", mb * 1024))?;
    }
    Ok(c)
}

pub fn open_reader(path: &Path, mmap: bool) -> Result<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    c.execute_batch(READER_PRAGMAS)?;
    if let Some(mb) = std::env::var("SPIKE_READER_CACHE_MB").ok().and_then(|s| s.parse::<i64>().ok()) {
        c.execute_batch(&format!("PRAGMA cache_size = -{};", mb * 1024))?;
    }
    if !mmap {
        c.execute_batch("PRAGMA mmap_size = 0;")?;
    }
    Ok(c)
}
