-- Stage 3: message bodies, preview, prefetch and remote content (decision 0009).
--
-- Bodies are zstd-compressed files under <data dir>/bodies/<account uuid>/; the database holds
-- only the reference. A body is either complete (the whole raw message, BODY[]) or partial
-- (a large message: header, text parts and whatever was fetched on request, each section
-- addressed by its IMAP section). Class: Cache (rebuilt from the server).
--
-- `message.preview` (from 0002) gets its text with the parser; `message.body_ref` (0002, never
-- used) is replaced by `message_body`.

ALTER TABLE message DROP COLUMN body_ref;

CREATE TABLE message_body (
  message_id   INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  complete     INTEGER NOT NULL CHECK (complete IN (0, 1)),
  structure    TEXT    CHECK (structure IS NULL OR json_valid(structure)),
                                               -- JSON MessageStructure of a partial body
  raw_size     INTEGER NOT NULL CHECK (raw_size >= 0),     -- bytes of all stored sections
  stored_size  INTEGER NOT NULL CHECK (stored_size >= 0),  -- bytes of the files on disk
  fetched_at   INTEGER NOT NULL                            -- Unix ms of the last store
) STRICT;

-- Where each stored section lies. What is written together shares one file (one fsync): the
-- sections of one fetch, or several small complete bodies packed by the prefetch. A complete
-- body is the section ''. A file is removed when no row refers to it any more.
CREATE TABLE message_body_part (
  message_id  INTEGER NOT NULL REFERENCES message_body(message_id) ON DELETE CASCADE,
  section     TEXT    NOT NULL,                  -- '' = whole message, else 'HEADER', '1.2', '1.2.MIME'
  file        TEXT    NOT NULL,                  -- relative to the bodies directory
  byte_offset INTEGER NOT NULL CHECK (byte_offset >= 0),  -- in the decompressed file
  byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
  stored_bytes INTEGER NOT NULL CHECK (stored_bytes >= 0), -- its share of the file on disk
  PRIMARY KEY (message_id, section)
) STRICT, WITHOUT ROWID;
CREATE INDEX body_part_file ON message_body_part(file);  -- "is this file still referenced?"

-- Flagged mail is prefetched in every mailbox; few rows, so a partial index.
CREATE INDEX loc_flagged ON message_location(mailbox_id, message_id) WHERE flagged = 1;

-- "Always load remote content from this sender" per account. Class: sync (stage 11 registers
-- it); refers to the account by UUID like every synced record, and goes with the account.
CREATE TABLE remote_content_sender (
  account_uuid  TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  sender        TEXT    NOT NULL,                -- normalised address (mail_model::normalize_sender)
  created_at    INTEGER NOT NULL,                -- Unix ms
  PRIMARY KEY (account_uuid, sender)
) STRICT, WITHOUT ROWID;
