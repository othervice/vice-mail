-- Stage 7: full-text index, threads, account groups, hidden accounts, identity ids
-- (decisions 0003 and 0013).
--
-- Full-text index (class Cache, rebuilt from `message` and the body files):
--   `message_fts` is contentless (`content=''`, `contentless_delete=1`): it holds tokens only,
--   its rowid is `message.id`, so hits come newest first and a date filter is a rowid range.
--   Columns as the search language names them (`mail_model::search::FtsColumn`); `from` and
--   `to` are SQL keywords and quoted here and in INSERTs, never in MATCH expressions. Header
--   columns are written with the message, `body` and `attachments` once a body is stored.
--   Existing messages are indexed in the background after this migration: the job row below
--   tells the engine to fill `message_fts` (target 'message_fts', from `next_id`). A rebuild
--   fills `message_fts_new` the same way and swaps it in at the end.
--
-- Threads (class Cache, rebuilt from `message`):
--   `thread` per account with the newest message and the message count; the unread count is
--   counted when a thread list is read (exact for every flag change, with no hook in the sync,
--   outbox and undo paths; cost per row proportional to the thread size, measured in
--   docs/testing/performance.md). `thread_ref` maps every Message-ID a thread's messages carry
--   or name to the thread (`ThreadLookup::threads_with_ids`). `message.thread_id` (from 0002)
--   is set when a message is stored; existing messages are threaded in the background in
--   ascending id order (date order, the canonical result) through `message_thread`.
--   `message.subject_key` holds the normalized subject only for messages the subject fallback
--   may pair with (not bulk mail, specific subject), so its index stays small.
--
-- Account groups (class sync; refer to accounts by UUID like every synced record) and hidden
-- accounts (class Gerät: hiding is a temporary choice on this device). Identities get a UUID
-- (the settings sync identifies records by UUID) and unique addresses per account.

-- Full text ------------------------------------------------------------------------------------

CREATE VIRTUAL TABLE message_fts USING fts5(
  subject, "from", "to", cc, body, attachments,
  content = '', contentless_delete = 1,
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER message_fts_delete AFTER DELETE ON message BEGIN
  DELETE FROM message_fts WHERE rowid = old.id;
END;

-- At most one indexing job: fill `target` from `next_id` upwards (ascending ids, so FTS writes
-- stay in rowid order). Without a row there is nothing to do.
CREATE TABLE search_index_job (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  target      TEXT    NOT NULL CHECK (target IN ('message_fts', 'message_fts_new')),
  next_id     INTEGER NOT NULL,                       -- first message id not yet indexed
  started_at  INTEGER NOT NULL                        -- Unix ms
) STRICT;

INSERT INTO search_index_job (id, target, next_id, started_at)
SELECT 1, 'message_fts', 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
WHERE EXISTS (SELECT 1 FROM message);

-- Threads --------------------------------------------------------------------------------------

CREATE TABLE thread (
  id               INTEGER PRIMARY KEY,
  account_id       INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  subject_key      TEXT,                              -- normalized subject of the first message
  last_message_id  INTEGER NOT NULL,                  -- newest message (ids are dates)
  message_count    INTEGER NOT NULL CHECK (message_count >= 0)
) STRICT;
-- Foreign key from `account`, and the account's threads by last activity.
CREATE INDEX thread_account ON thread(account_id, last_message_id);

CREATE TABLE thread_ref (
  account_id  INTEGER NOT NULL,                       -- = thread.account_id
  ref_id      TEXT    NOT NULL,                       -- Message-ID without <>
  thread_id   INTEGER NOT NULL REFERENCES thread(id) ON DELETE CASCADE,
  PRIMARY KEY (account_id, ref_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX thread_ref_thread ON thread_ref(thread_id);   -- merges, foreign key

ALTER TABLE message ADD COLUMN subject_key TEXT;
CREATE INDEX message_subject ON message(account_id, subject_key, id)
  WHERE subject_key IS NOT NULL;

-- A deleted message leaves its thread; an empty thread goes with its ids (decision 0013:
-- threads are never split).
CREATE TRIGGER message_thread_delete AFTER DELETE ON message WHEN old.thread_id IS NOT NULL
BEGIN
  UPDATE thread
  SET message_count = message_count - 1,
      last_message_id = CASE WHEN last_message_id = old.id
        THEN coalesce((SELECT max(id) FROM message WHERE thread_id = old.thread_id), 0)
        ELSE last_message_id END
  WHERE id = old.thread_id;
  DELETE FROM thread WHERE id = old.thread_id AND message_count <= 0;
END;

-- Accounts -------------------------------------------------------------------------------------

CREATE TABLE account_group (
  id          INTEGER PRIMARY KEY,
  uuid        TEXT    NOT NULL UNIQUE,                -- lowercase hyphenated
  name        TEXT    NOT NULL,
  position    INTEGER NOT NULL,                       -- order in the sidebar
  created_at  INTEGER NOT NULL                        -- Unix ms
) STRICT;

CREATE TABLE account_group_member (
  group_id      INTEGER NOT NULL REFERENCES account_group(id) ON DELETE CASCADE,
  account_uuid  TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  PRIMARY KEY (group_id, account_uuid)
) STRICT, WITHOUT ROWID;
CREATE INDEX account_group_member_account ON account_group_member(account_uuid);  -- foreign key

CREATE TABLE account_hidden (
  account_uuid  TEXT    PRIMARY KEY REFERENCES account(uuid) ON DELETE CASCADE,
  hidden_at     INTEGER NOT NULL                       -- Unix ms
) STRICT, WITHOUT ROWID;

ALTER TABLE identity ADD COLUMN uuid TEXT;
-- A random UUID v4 for every existing identity.
UPDATE identity SET uuid = lower(
  substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-4' || substr(h, 14, 3) || '-' ||
  substr('89ab', 1 + (abs(random()) % 4), 1) || substr(h, 18, 3) || '-' || substr(h, 21, 12))
FROM (SELECT id AS iid, hex(randomblob(16)) AS h FROM identity) WHERE identity.id = iid;
CREATE UNIQUE INDEX identity_uuid ON identity(uuid);
-- Every new identity gets one too (the insert code stays the same for all schema versions).
CREATE TRIGGER identity_new_uuid AFTER INSERT ON identity WHEN new.uuid IS NULL BEGIN
  UPDATE identity SET uuid = (SELECT lower(
    substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-4' || substr(h, 14, 3) || '-' ||
    substr('89ab', 1 + (abs(random()) % 4), 1) || substr(h, 18, 3) || '-' || substr(h, 21, 12))
    FROM (SELECT hex(randomblob(16)) AS h))
  WHERE id = new.id;
END;
CREATE UNIQUE INDEX identity_email ON identity(account_id, email COLLATE NOCASE);
