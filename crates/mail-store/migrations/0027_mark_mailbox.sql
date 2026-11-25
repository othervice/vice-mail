-- Stage 21 (decision 0063): "Mark folder as read" through the outbox.
--
-- Operations: `kind` gains 'mark_mailbox': `\Seen` set or cleared on a stored set of UIDs of one
--   folder (the payload holds the set). `mailbox_id` holds that folder; `message_id` and `uid`
--   stay NULL, so the folder's pending marks are a range of `operation_location`
--   (`mailbox_id = ? AND uid IS NULL`). SQLite cannot change a CHECK constraint in place, so the
--   table is rebuilt as in 0006 and 0019: the same columns, rows, ids, AUTOINCREMENT counter and
--   indexes; nothing refers to `operation`, so the rebuild touches no other table.

CREATE TABLE operation_v27 (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id         INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  batch              INTEGER NOT NULL,
  action             TEXT    NOT NULL,
  kind               TEXT    NOT NULL CHECK (kind IN ('set_flag', 'move', 'expunge', 'create_mailbox',
                                                     'send', 'append', 'remove_copy',
                                                     'rename_mailbox', 'delete_mailbox',
                                                     'mark_mailbox')),
  mailbox_id         INTEGER REFERENCES mailbox(id) ON DELETE SET NULL,
  message_id         INTEGER REFERENCES message(id) ON DELETE SET NULL,
  uid                INTEGER,
  target_mailbox_id  INTEGER REFERENCES mailbox(id) ON DELETE SET NULL,
  payload            TEXT    NOT NULL CHECK (json_valid(payload)),
  idempotency_key    TEXT    NOT NULL UNIQUE,
  state              TEXT    NOT NULL DEFAULT 'queued'
                             CHECK (state IN ('queued', 'in_flight', 'confirmed', 'failed', 'conflict')),
  attempts           INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  last_error         TEXT    CHECK (last_error IS NULL OR json_valid(last_error)),
  conflict           TEXT    CHECK (conflict IN ('gone', 'modified', 'mailbox_reset', 'mailbox_gone')),
  notice             TEXT    CHECK (notice IN ('left_marked_deleted', 'no_sent_copy')),
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL,
  not_before         INTEGER,
  draft_id           INTEGER REFERENCES draft(id) ON DELETE SET NULL
) STRICT;

INSERT INTO operation_v27 (id, account_id, batch, action, kind, mailbox_id, message_id, uid,
                           target_mailbox_id, payload, idempotency_key, state, attempts,
                           last_error, conflict, notice, created_at, updated_at, not_before,
                           draft_id)
SELECT id, account_id, batch, action, kind, mailbox_id, message_id, uid, target_mailbox_id,
       payload, idempotency_key, state, attempts, last_error, conflict, notice, created_at,
       updated_at, not_before, draft_id
FROM operation;

-- Keep the AUTOINCREMENT counter: ids of purged operations must not come back.
UPDATE sqlite_sequence
SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'operation')
WHERE name = 'operation_v27'
  AND (SELECT seq FROM sqlite_sequence WHERE name = 'operation') > seq;
INSERT INTO sqlite_sequence (name, seq)
SELECT 'operation_v27', seq FROM sqlite_sequence
WHERE name = 'operation'
  AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'operation_v27');

DROP TABLE operation;
ALTER TABLE operation_v27 RENAME TO operation;

CREATE INDEX operation_account ON operation(account_id, id);
CREATE INDEX operation_pending ON operation(account_id, id) WHERE state IN ('queued', 'in_flight');
CREATE INDEX operation_location ON operation(mailbox_id, uid);
CREATE INDEX operation_message ON operation(message_id);
CREATE INDEX operation_target ON operation(target_mailbox_id);
CREATE INDEX operation_batch ON operation(batch);
CREATE INDEX operation_send ON operation(account_id, id) WHERE kind = 'send';
CREATE INDEX operation_draft ON operation(draft_id);
