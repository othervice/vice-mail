-- Stage 20, task C (decision 0060): the attachments of stored bodies, for the "Attachments" view
-- across accounts. Additive only.
--
-- attachment (class Cache): one row per attachment of a message whose body (or, for a large
--   message, its structure and text) is stored, with the index the reading pane uses (`idx`),
--   written in the transaction that indexes the body for the search (`BodyTextSource`), like
--   the attachment names of `message_fts`. `name_key` is the name in lower case for the search;
--   `kind` the filter (0 image, 1 PDF, 2 document, 3 spreadsheet, 4 archive, 5 other,
--   `mail_model::AttachmentKind`); `part` the IMAP part of an attachment of a partial body
--   (whether its bytes are stored is looked up in `message_body_part`), NULL in a whole one.
--   A body removed from the cache keeps its rows (opening fetches it again), as the search
--   keeps its text; a deleted message takes them along.
--
-- Bodies stored before this migration: the search index job runs once over every message and
-- writes the rows of those with a body (the same pass as a rebuild of the live index).

CREATE TABLE attachment (
  message_id  INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  idx         INTEGER NOT NULL CHECK (idx >= 0),
  name        TEXT    NOT NULL,
  name_key    TEXT    NOT NULL,
  mime_type   TEXT    NOT NULL,
  kind        INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 5),
  size        INTEGER NOT NULL CHECK (size >= 0),
  part        TEXT,
  PRIMARY KEY (message_id, idx)
) STRICT, WITHOUT ROWID;
-- The view filtered by kind, newest first.
CREATE INDEX attachment_kind ON attachment(kind, message_id);

INSERT OR IGNORE INTO search_index_job (id, target, next_id, started_at)
SELECT 1, 'message_fts', 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
WHERE EXISTS (SELECT 1 FROM message_body);
