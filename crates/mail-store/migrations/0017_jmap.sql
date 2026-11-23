-- Stage 14, block D (decision 0041): JMAP as a second protocol. Additive only; IMAP accounts
-- keep every value they had.
--
-- account.protocol (class sync, in the account record only when not 'imap'): how the app talks
--   to the account's server. 'jmap' uses the imap_* columns for the JMAP server (always TLS);
--   smtp_* hold a copy of it. Fixed when the account is set up.
-- jmap_state, jmap_mailbox, jmap_uid (class Cache): the JMAP adapter's own numbering of the
--   messages of each mailbox (JMAP has string ids and state strings, the engine UIDs and
--   mod-sequences). The adapter keeps it in memory and writes every change here, so a restart
--   does not look like a new UIDVALIDITY. Rebuilt from the server when lost.
--   jmap_state: the server-side account (`accountId`) the map belongs to, the Email state it
--     reflects and the change counter (the emulated HIGHESTMODSEQ).
--   jmap_mailbox: per JMAP mailbox id its UIDVALIDITY, the next UID and whether every message
--     has a number yet.
--   jmap_uid: per mailbox and UID the JMAP Email id and the counter value of its last change.

ALTER TABLE account ADD COLUMN protocol TEXT NOT NULL DEFAULT 'imap'
  CHECK (protocol IN ('imap', 'jmap'));

CREATE TABLE jmap_state (
  account_id   INTEGER PRIMARY KEY REFERENCES account(id) ON DELETE CASCADE,
  remote       TEXT    NOT NULL,
  email_state  TEXT,
  modseq       INTEGER NOT NULL CHECK (modseq >= 0)
) STRICT;

CREATE TABLE jmap_mailbox (
  account_id    INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  mailbox       TEXT    NOT NULL,
  uid_validity  INTEGER NOT NULL CHECK (uid_validity BETWEEN 1 AND 4294967295),
  uid_next      INTEGER NOT NULL CHECK (uid_next BETWEEN 1 AND 4294967295),
  complete      INTEGER NOT NULL CHECK (complete IN (0, 1)),
  PRIMARY KEY (account_id, mailbox)
) STRICT, WITHOUT ROWID;

CREATE TABLE jmap_uid (
  account_id  INTEGER NOT NULL,
  mailbox     TEXT    NOT NULL,
  uid         INTEGER NOT NULL CHECK (uid BETWEEN 1 AND 4294967295),
  email       TEXT    NOT NULL,
  modseq      INTEGER NOT NULL CHECK (modseq >= 0),
  PRIMARY KEY (account_id, mailbox, uid),
  FOREIGN KEY (account_id, mailbox) REFERENCES jmap_mailbox(account_id, mailbox) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
