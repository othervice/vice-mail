-- Stage 22 (decision 0074): search inside words.
--
-- `message_fts_tri` (class Cache, rebuilt from `message` and the body files like `message_fts`):
--   a second contentless full-text index over the same columns and rowids as `message_fts`,
--   tokenized into overlapping groups of three characters. A string of three or more characters
--   matches as a substring there (`"uster"` finds "Muster", `"1234"` a number inside a word).
--   `remove_diacritics 1` folds case and diacritics with the same function as the word index's
--   `unicode61 remove_diacritics 2` (SQLite maps every value but 0 to it); the option needs
--   SQLite 3.45, the bundled one is 3.54.1. `fts::write` writes both indexes in the same
--   transaction, the trigger below deletes with the message.
--
-- `search_index_job.target` gains 'message_fts_tri': fill only the trigram index from `next_id`
--   (existing messages after this migration). 'message_fts' now fills both live indexes and
--   'message_fts_new' both new ones (`message_fts_new`, `message_fts_tri_new`), swapped in
--   together. SQLite cannot change a CHECK constraint, so the table is rebuilt; it holds at most
--   one row and nothing refers to it.
--
-- A job already running: the fill after migration 0005 starts over (it fills the new index too,
--   and rewriting a word row changes nothing). A rebuild cannot get its second new table here (a
--   migration cannot create a table on condition), so it is dropped: searches used the live index
--   all along, which stays complete; the user can start the rebuild again. Without a job, a new
--   one fills the trigram index if there are messages.

CREATE VIRTUAL TABLE message_fts_tri USING fts5(
  subject, "from", "to", cc, body, attachments,
  content = '', contentless_delete = 1,
  tokenize = 'trigram remove_diacritics 1'
);

CREATE TRIGGER message_fts_tri_delete AFTER DELETE ON message BEGIN
  DELETE FROM message_fts_tri WHERE rowid = old.id;
END;

DROP TRIGGER IF EXISTS message_fts_new_delete;
DROP TABLE IF EXISTS message_fts_new;
DROP TABLE IF EXISTS message_fts_old;

CREATE TABLE search_index_job_v29 (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  target      TEXT    NOT NULL
                      CHECK (target IN ('message_fts', 'message_fts_new', 'message_fts_tri')),
  next_id     INTEGER NOT NULL,                       -- first message id not yet indexed
  started_at  INTEGER NOT NULL                        -- Unix ms
) STRICT;

INSERT INTO search_index_job_v29 (id, target, next_id, started_at)
SELECT 1, 'message_fts', 0, started_at FROM search_index_job WHERE target = 'message_fts';

INSERT INTO search_index_job_v29 (id, target, next_id, started_at)
SELECT 1, 'message_fts_tri', 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
WHERE EXISTS (SELECT 1 FROM message) AND NOT EXISTS (SELECT 1 FROM search_index_job_v29);

DROP TABLE search_index_job;
ALTER TABLE search_index_job_v29 RENAME TO search_index_job;
