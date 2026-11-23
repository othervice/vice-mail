-- Stage 5: drafts, sending through the outbox, contacts, text signatures, the Sent-copy setting
-- per account (decision 0018).
--
-- Drafts (class Gerät; the server's Drafts folder is the copy other devices see):
--   One row per local draft, keyed by a UUID. `message_id_hdr` is made when the draft is
--   created and kept for every save and every send attempt. The composer's three parts stay
--   apart (decision 0016): what the user wrote (`body_html`/`body_text`), the quote of a reply
--   or forward (`quote_html`/`quote_text`) and whether the identity's signature goes below.
--   `attachments` lists the core's own copies of attached files (under `compose/drafts/<uuid>/`
--   next to the database), never the user's original paths. `revision` grows with every save;
--   `server_revision` is the revision last appended to the Drafts folder, `server_*` where
--   that copy lies (an own copy: only it is ever removed, after checking its Message-ID).
--   `state` 'sending' freezes the draft while its `send` operation is in the outbox.
--
-- Operations: `kind` gains 'send', 'append' and 'remove_copy', `notice` gains 'no_sent_copy',
--   and `not_before` delays an operation (the undo delay of a send, the next look into Sent
--   after an unclear send). SQLite cannot change a CHECK constraint in place, so the table is
--   rebuilt with the same columns, rows, ids, AUTOINCREMENT counter and indexes; nothing
--   refers to `operation`, so the rebuild touches no other table.
--
-- Contacts (class Cache; rebuilt from the stored headers): one row per address, as
--   `mail_content::ContactBook` keeps them (decision 0016). `contact_group` joins addresses the
--   user merged (stage 10; until then every row is its own group). `rank` is the decay-free
--   sort key of `mail_content::Rank` with an index for autocomplete; `search` holds the folded
--   words of names and address (space-separated, with a leading space) for prefix matching.
--   `message.contacts_seen` marks messages whose addresses went into the directory; the
--   partial index finds the rest (new mail, and every message stored before this migration).
--
-- Identities get a plain-text signature; accounts the choice who files sent mail.

CREATE TABLE draft (
  id                   INTEGER PRIMARY KEY,
  uuid                 TEXT    NOT NULL UNIQUE,          -- DraftId, lower case
  account_id           INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  identity_id          INTEGER REFERENCES identity(id) ON DELETE SET NULL,
  kind                 TEXT    NOT NULL CHECK (kind IN ('new', 'reply', 'reply_all', 'forward')),
  message_id_hdr       TEXT    NOT NULL,                 -- without <>, fixed at creation
  original_message_id  INTEGER REFERENCES message(id) ON DELETE SET NULL,
  original_msgid_hdr   TEXT,                             -- the answered or forwarded message
  in_reply_to          TEXT,
  references_hdr       TEXT,                             -- ids without <>, space-separated
  to_addrs             TEXT    NOT NULL CHECK (json_valid(to_addrs)),   -- [{"name", "email"}]
  cc_addrs             TEXT    NOT NULL CHECK (json_valid(cc_addrs)),
  bcc_addrs            TEXT    NOT NULL CHECK (json_valid(bcc_addrs)),
  subject              TEXT    NOT NULL,
  format               TEXT    NOT NULL CHECK (format IN ('html', 'text')),
  body_html            TEXT    NOT NULL,
  body_text            TEXT    NOT NULL,
  quote_html           TEXT,
  quote_text           TEXT,
  include_quote        INTEGER NOT NULL DEFAULT 1 CHECK (include_quote IN (0, 1)),
  quote_position       TEXT    NOT NULL DEFAULT 'above' CHECK (quote_position IN ('above', 'below')),
  include_signature    INTEGER NOT NULL DEFAULT 1 CHECK (include_signature IN (0, 1)),
  attachments          TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(attachments)),
  state                TEXT    NOT NULL DEFAULT 'editing' CHECK (state IN ('editing', 'sending')),
  revision             INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  server_revision      INTEGER,                          -- revision of the copy on the server
  server_mailbox_id    INTEGER REFERENCES mailbox(id) ON DELETE SET NULL,
  server_mailbox       TEXT,                             -- wire name of the Drafts folder
  server_uidvalidity   INTEGER,
  server_uid           INTEGER CHECK (server_uid IS NULL OR server_uid BETWEEN 1 AND 4294967295),
  created_at           INTEGER NOT NULL,                 -- Unix ms
  updated_at           INTEGER NOT NULL
) STRICT;
-- The account's drafts, newest first, and the foreign key from `account`.
CREATE INDEX draft_account ON draft(account_id, updated_at);
-- Opening a message of the Drafts folder finds its local draft (and the foreign key lookups).
CREATE INDEX draft_msgid ON draft(account_id, message_id_hdr);
CREATE INDEX draft_identity ON draft(identity_id);
CREATE INDEX draft_original ON draft(original_message_id);
CREATE INDEX draft_server_mailbox ON draft(server_mailbox_id);

-- Operations, rebuilt -------------------------------------------------------------------------

CREATE TABLE operation_v6 (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id         INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  batch              INTEGER NOT NULL,
  action             TEXT    NOT NULL,
  kind               TEXT    NOT NULL CHECK (kind IN ('set_flag', 'move', 'expunge', 'create_mailbox',
                                                     'send', 'append', 'remove_copy')),
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
  not_before         INTEGER,                          -- Unix ms; NULL = at once
  draft_id           INTEGER REFERENCES draft(id) ON DELETE SET NULL
) STRICT;

INSERT INTO operation_v6 (id, account_id, batch, action, kind, mailbox_id, message_id, uid,
                          target_mailbox_id, payload, idempotency_key, state, attempts,
                          last_error, conflict, notice, created_at, updated_at)
SELECT id, account_id, batch, action, kind, mailbox_id, message_id, uid, target_mailbox_id,
       payload, idempotency_key, state, attempts, last_error, conflict, notice, created_at,
       updated_at
FROM operation;

-- Keep the AUTOINCREMENT counter: ids of purged operations must not come back.
UPDATE sqlite_sequence
SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'operation')
WHERE name = 'operation_v6'
  AND (SELECT seq FROM sqlite_sequence WHERE name = 'operation') > seq;
INSERT INTO sqlite_sequence (name, seq)
SELECT 'operation_v6', seq FROM sqlite_sequence
WHERE name = 'operation'
  AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'operation_v6');

DROP TABLE operation;
ALTER TABLE operation_v6 RENAME TO operation;

CREATE INDEX operation_account ON operation(account_id, id);
CREATE INDEX operation_pending ON operation(account_id, id) WHERE state IN ('queued', 'in_flight');
CREATE INDEX operation_location ON operation(mailbox_id, uid);
CREATE INDEX operation_message ON operation(message_id);
CREATE INDEX operation_target ON operation(target_mailbox_id);
CREATE INDEX operation_batch ON operation(batch);
-- Stage 5: the outbox view (sends of an account, newest first) and the operations of a draft.
CREATE INDEX operation_send ON operation(account_id, id) WHERE kind = 'send';
CREATE INDEX operation_draft ON operation(draft_id);

-- Contacts -------------------------------------------------------------------------------------

CREATE TABLE contact (
  id               INTEGER PRIMARY KEY,
  address_key      TEXT    NOT NULL UNIQUE,            -- mail_content::NormalizedAddress::key
  local_part       TEXT    NOT NULL,                   -- as last written (case kept)
  domain           TEXT    NOT NULL,                   -- ASCII (IDNA)
  contact_group    INTEGER,                            -- stage 10: merged contacts share it
  sent             INTEGER NOT NULL DEFAULT 0 CHECK (sent >= 0),
  received         INTEGER NOT NULL DEFAULT 0 CHECK (received >= 0),
  last_sent        INTEGER,                            -- Unix s
  last_received    INTEGER,
  rank             REAL    NOT NULL,                   -- mail_content::Rank::key; -1e308 = none
  names            TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(names)),  -- [{name, count, lastSeen}]
  user_name        TEXT,
  search           TEXT    NOT NULL,                   -- ' word word …' folded
  updated_at       INTEGER NOT NULL                    -- Unix ms
) STRICT;
CREATE INDEX contact_rank ON contact(rank DESC);
CREATE INDEX contact_group ON contact(contact_group) WHERE contact_group IS NOT NULL;

ALTER TABLE message ADD COLUMN contacts_seen INTEGER;
CREATE INDEX message_contacts_todo ON message(id) WHERE contacts_seen IS NULL;

-- Identities and accounts ----------------------------------------------------------------------

ALTER TABLE identity ADD COLUMN signature TEXT;           -- plain text, without '-- '
ALTER TABLE account ADD COLUMN sent_copy TEXT CHECK (sent_copy IN ('app', 'server'));  -- NULL = auto
