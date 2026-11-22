-- Stage 2: accounts, identities, mailboxes and message headers, after decision 0003
-- ("Schemaentwurf"). No FTS, threads or attachments yet (stages 3 and 7). `message.id` is the
-- rowid, so the contentless FTS table of stage 7 takes it as its rowid unchanged.
-- Booleans are 0/1 INTEGERs.
-- Deviations from 0003, with reasons:
--   account     server settings and the keychain reference are columns (stage 2 needs them);
--               the email is unique (error account_exists).
--   mailbox     `name` is the wire name (identity on the server), `path` the decoded name;
--               `attributes`, `selectable`, `role_source` and `sync_low_uid` are new;
--               `parent_id` is ON DELETE SET NULL, not CASCADE: a vanished parent must not
--               take children with it that still exist (the list refresh recomputes parents).
--               `mailbox_role` enforces one mailbox per role and account; `mailbox_path`
--               gives the sidebar order without a sort.
--   message     header columns for later stages (threads, categories, unsubscribe), so they
--               never have to be fetched again.
--   message_location  CHECKs on UID and flags (no storage cost).

CREATE TABLE account (
  id              INTEGER PRIMARY KEY,
  uuid            TEXT    NOT NULL UNIQUE,                 -- AccountId, lowercase hyphenated
  email           TEXT    NOT NULL UNIQUE COLLATE NOCASE,
  display_name    TEXT    NOT NULL,
  color           INTEGER NOT NULL CHECK (color BETWEEN 0 AND 7),
  imap_host       TEXT    NOT NULL,
  imap_port       INTEGER NOT NULL CHECK (imap_port BETWEEN 1 AND 65535),
  imap_security   TEXT    NOT NULL CHECK (imap_security IN ('tls', 'starttls')),
  imap_username   TEXT    NOT NULL,
  smtp_host       TEXT    NOT NULL,
  smtp_port       INTEGER NOT NULL CHECK (smtp_port BETWEEN 1 AND 65535),
  smtp_security   TEXT    NOT NULL CHECK (smtp_security IN ('tls', 'starttls')),
  smtp_username   TEXT    NOT NULL,
  credential_ref  TEXT    NOT NULL,                        -- keychain entry name, never the secret
  writes_enabled  INTEGER NOT NULL DEFAULT 0 CHECK (writes_enabled IN (0, 1)),
  created_at      INTEGER NOT NULL                         -- Unix ms
) STRICT;

CREATE TABLE identity (
  id          INTEGER PRIMARY KEY,
  account_id  INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  email       TEXT    NOT NULL,
  name        TEXT    NOT NULL,
  reply_to    TEXT,
  is_default  INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1))
) STRICT;
CREATE INDEX identity_account ON identity(account_id);
CREATE UNIQUE INDEX identity_default ON identity(account_id) WHERE is_default = 1;

CREATE TABLE mailbox (
  id             INTEGER PRIMARY KEY,
  account_id     INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  name           TEXT    NOT NULL,                  -- wire name (modified UTF-7), for EXAMINE
  path           TEXT    NOT NULL,                  -- decoded display path
  delimiter      TEXT,                              -- one character, NULL = flat namespace
  parent_id      INTEGER REFERENCES mailbox(id) ON DELETE SET NULL,
  attributes     TEXT    NOT NULL DEFAULT '',       -- LIST attributes, space-separated, as sent
  selectable     INTEGER NOT NULL DEFAULT 1 CHECK (selectable IN (0, 1)),
  role           TEXT    CHECK (role IN ('inbox', 'sent', 'drafts', 'archive', 'trash', 'junk',
                                         'all', 'flagged')),   -- NULL = custom
  role_source    TEXT    NOT NULL DEFAULT 'none'
                         CHECK (role_source IN ('server', 'heuristic', 'user', 'none')),
  uidvalidity    INTEGER,                           -- generation the stored UIDs belong to
  uidnext        INTEGER,                           -- UIDNEXT when the last new-mail pass began
  highestmodseq  INTEGER,                           -- HIGHESTMODSEQ when the last flag pass began
  sync_low_uid   INTEGER,                           -- first sync: every server message with
                                                    -- sync_low_uid <= UID < uidnext is stored;
                                                    -- 1 = first sync complete, NULL = not begun
  UNIQUE (account_id, name)
) STRICT;
CREATE INDEX mailbox_parent ON mailbox(parent_id);
CREATE INDEX mailbox_path ON mailbox(account_id, path);
CREATE UNIQUE INDEX mailbox_role ON mailbox(account_id, role) WHERE role IS NOT NULL;

-- One row per mail, even if it lies in several mailboxes.
-- id = sort date in Unix seconds; if taken, the next free number above (assigned on the writer
-- thread). Larger = newer, below 2^33 + a few, exact in JavaScript. Never changes.
CREATE TABLE message (
  id                INTEGER PRIMARY KEY,
  account_id        INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  message_id_hdr    TEXT,                           -- without <>, NULL if missing (never merged)
  thread_id         INTEGER,                        -- stage 7
  date              INTEGER NOT NULL,               -- sort date (Unix seconds), for display
  subject           TEXT    NOT NULL,               -- '' if none
  from_name         TEXT,                           -- first From address; both NULL if none
  from_addr         TEXT,
  to_addrs          TEXT,                           -- JSON [{"name": str|null, "email": str}],
  cc_addrs          TEXT,                           -- NULL if empty
  in_reply_to       TEXT,
  references_hdr    TEXT,                           -- message ids without <>, space-separated
  list_id           TEXT,
  list_unsubscribe  TEXT,
  auto_submitted    TEXT,
  precedence        TEXT,
  size              INTEGER NOT NULL,
  preview           TEXT,                           -- stage 3
  has_attachments   INTEGER NOT NULL DEFAULT 0 CHECK (has_attachments IN (0, 1)),
  category          INTEGER,                        -- stage 9
  body_ref          TEXT                            -- stage 3
) STRICT;
CREATE INDEX message_msgid  ON message(account_id, message_id_hdr);  -- merge, threads, FK
CREATE INDEX message_thread ON message(thread_id);

-- Where a mail lies. Flags belong to the location, as in IMAP.
CREATE TABLE message_location (
  mailbox_id  INTEGER NOT NULL REFERENCES mailbox(id) ON DELETE CASCADE,
  message_id  INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  uid         INTEGER NOT NULL CHECK (uid BETWEEN 1 AND 4294967295),
  seen        INTEGER NOT NULL DEFAULT 0 CHECK (seen IN (0, 1)),
  flagged     INTEGER NOT NULL DEFAULT 0 CHECK (flagged IN (0, 1)),
  answered    INTEGER NOT NULL DEFAULT 0 CHECK (answered IN (0, 1)),
  draft       INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0, 1)),
  deleted     INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
  keywords    TEXT,                                 -- space-separated (IMAP atoms), NULL = none
  modseq      INTEGER,
  PRIMARY KEY (mailbox_id, message_id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX loc_uid ON message_location(mailbox_id, uid);                     -- sync by UID
CREATE INDEX loc_message ON message_location(message_id);                              -- FK cascade
CREATE INDEX loc_unseen  ON message_location(mailbox_id, message_id) WHERE seen = 0;   -- counters
