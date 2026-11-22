-- Stage 4: the outbox (decision 0012). Class: Gerät (device only, never synced).
--
-- One row per operation: one message and one change (a flag, a move, a permanent delete), or a
-- mailbox to create. The local effect is applied in the same transaction that inserts the row;
-- the outbox executes rows in `id` order per account and records the outcome here.
--
-- `id` is AUTOINCREMENT so ids (and batch ids, which are the id of a batch's first operation)
-- are never reused after confirmed operations are purged: the UI may still hold them.
-- `payload` holds everything needed to execute, check and revert the operation (wire names,
-- UIDVALIDITY, UID, the Message-ID header and the flags it replaced); the id columns are for
-- lookups and become NULL when the local row goes (foreign keys `ON DELETE SET NULL`).
-- `idempotency_key` is `<request id>:<index>`: the same request recorded twice (an IPC call
-- repeated by the UI) is recognised and not applied again.

CREATE TABLE operation (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id         INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  batch              INTEGER NOT NULL,
  action             TEXT    NOT NULL,                -- ActionKind, for the activity view
  kind               TEXT    NOT NULL CHECK (kind IN ('set_flag', 'move', 'expunge', 'create_mailbox')),
  mailbox_id         INTEGER REFERENCES mailbox(id) ON DELETE SET NULL,   -- source mailbox
  message_id         INTEGER REFERENCES message(id) ON DELETE SET NULL,
  uid                INTEGER,                         -- source UID (with the payload's UIDVALIDITY)
  target_mailbox_id  INTEGER REFERENCES mailbox(id) ON DELETE SET NULL,
  payload            TEXT    NOT NULL CHECK (json_valid(payload)),
  idempotency_key    TEXT    NOT NULL UNIQUE,
  state              TEXT    NOT NULL DEFAULT 'queued'
                             CHECK (state IN ('queued', 'in_flight', 'confirmed', 'failed', 'conflict')),
  attempts           INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),  -- times the command was sent
  last_error         TEXT    CHECK (last_error IS NULL OR json_valid(last_error)),  -- IpcError JSON
  conflict           TEXT    CHECK (conflict IN ('gone', 'modified', 'mailbox_reset', 'mailbox_gone')),
  notice             TEXT    CHECK (notice IN ('left_marked_deleted')),
  created_at         INTEGER NOT NULL,                -- Unix ms
  updated_at         INTEGER NOT NULL
) STRICT;

-- Activity view per account, newest first, and the foreign key from `account`.
CREATE INDEX operation_account ON operation(account_id, id);
-- What the outbox still has to do, in order. Small: only queued and in-flight rows.
CREATE INDEX operation_pending ON operation(account_id, id) WHERE state IN ('queued', 'in_flight');
-- Pending flag changes of one location, re-applied after a sync write (local changes win until
-- the server confirmed them), and the foreign key from `mailbox`.
CREATE INDEX operation_location ON operation(mailbox_id, uid);
CREATE INDEX operation_message ON operation(message_id);          -- foreign key, orphan guard
CREATE INDEX operation_target ON operation(target_mailbox_id);    -- foreign key
CREATE INDEX operation_batch ON operation(batch);                 -- undo
