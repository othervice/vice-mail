-- Stage 16: feature parity (decision 0033). Additive only.
--
-- Reading list (class sync, kind `readingList`): a local mark like tasks, by account UUID and
--   Message-ID, never by local id. Nothing goes to the server.
--
-- Mailbox preferences (class sync, kind `mailboxPref`): favourite (with its position in the
--   favourites) and colour (an index into the account colour palette, 0-7) per mailbox, by account UUID
--   and decoded path, the way local rules name a mailbox across devices. A row exists only
--   while one of them is set.
--
-- Blocked senders (class sync, kind `blockedSender`): normalised addresses
--   (`mail_content::normalize_address`). New mail from them goes to the junk mailbox where the
--   account may write; on a read-only account (or without a junk mailbox) it is hidden from the
--   inbox lists through `blocked_hidden` (class Gerät), one probe per row of its unique index.

CREATE TABLE reading_list (
  id              INTEGER PRIMARY KEY,
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  message_id_hdr  TEXT    NOT NULL,                        -- without <>
  added_at        INTEGER NOT NULL,                        -- Unix ms
  UNIQUE (account_uuid, message_id_hdr)
) STRICT;
CREATE INDEX reading_list_added ON reading_list(added_at);

CREATE TABLE mailbox_pref (
  account_uuid       TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  path               TEXT    NOT NULL,                     -- decoded path, as `mailbox.path`
  favorite_position  INTEGER,                              -- NULL: not a favourite
  color              INTEGER CHECK (color BETWEEN 0 AND 7), -- AccountColor index; NULL: none
  updated_at         INTEGER NOT NULL,
  PRIMARY KEY (account_uuid, path),
  CHECK (favorite_position IS NOT NULL OR color IS NOT NULL)
) STRICT, WITHOUT ROWID;

CREATE TABLE blocked_sender (
  address     TEXT    PRIMARY KEY,                         -- normalised address
  created_at  INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE blocked_hidden (
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  message_id_hdr  TEXT    NOT NULL,
  sender          TEXT    NOT NULL,                        -- normalised address
  PRIMARY KEY (account_uuid, message_id_hdr)
) STRICT, WITHOUT ROWID;
CREATE INDEX blocked_hidden_sender ON blocked_hidden(sender);

CREATE TRIGGER sync_reading_list_insert AFTER INSERT ON reading_list BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'readingList' AS k, new.account_uuid || '/' || new.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_reading_list_update AFTER UPDATE ON reading_list BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'readingList' AS k, new.account_uuid || '/' || new.message_id_hdr AS i UNION SELECT 'readingList' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_reading_list_delete AFTER DELETE ON reading_list BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'readingList' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_mailbox_pref_insert AFTER INSERT ON mailbox_pref BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'mailboxPref' AS k, new.account_uuid || '/' || new.path AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_mailbox_pref_update AFTER UPDATE ON mailbox_pref BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'mailboxPref' AS k, new.account_uuid || '/' || new.path AS i UNION SELECT 'mailboxPref' AS k, old.account_uuid || '/' || old.path AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_mailbox_pref_delete AFTER DELETE ON mailbox_pref BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'mailboxPref' AS k, old.account_uuid || '/' || old.path AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_blocked_sender_insert AFTER INSERT ON blocked_sender BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'blockedSender' AS k, new.address AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_blocked_sender_update AFTER UPDATE ON blocked_sender BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'blockedSender' AS k, new.address AS i UNION SELECT 'blockedSender' AS k, old.address AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_blocked_sender_delete AFTER DELETE ON blocked_sender BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'blockedSender' AS k, old.address AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- Unblocking a sender shows what was hidden for it again.
CREATE TRIGGER blocked_sender_unhide AFTER DELETE ON blocked_sender BEGIN
  DELETE FROM blocked_hidden WHERE sender = old.address;
END;

-- ---------------------------------------------------------------------------------------------
-- Templates with recipients (stage 16, task 7): To, Cc and Bcc a template prefills, as JSON
--   `[{"name", "email"}]` like `draft.to_addrs`. Existing templates get none. The sync record
--   of kind `template` carries them (`to`, `cc`, `bcc`, left out when empty); the existing
--   triggers capture the change.

ALTER TABLE template ADD COLUMN to_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(to_addrs));
ALTER TABLE template ADD COLUMN cc_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(cc_addrs));
ALTER TABLE template ADD COLUMN bcc_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(bcc_addrs));

-- ---------------------------------------------------------------------------------------------
-- Send and archive, read receipts (stage 16, tasks 5 and 8; RFC 8098, RFC 3503).
--
-- Drafts: `archive_original` ("Senden und archivieren": archive the answered original once the
--   send is accepted) and `request_receipt` (ask for a read receipt).
-- `receipt_answer` (class Gerät): the user's answer to a read-receipt request of an incoming
--   message, by account UUID and Message-ID: 'queued' (with the send operation), 'sent' once
--   the server accepted it, 'declined'. The original also gets `$MDNSent` where the account may
--   write, so other programs and devices do not ask again.
-- `receipt_received` (class Cache): read receipts that came back, recorded when their body is
--   stored, by account UUID and the Message-ID they report on; one row per reported recipient
--   (the newest wins). Rebuilt when the bodies are fetched again.
-- Both refer to the account UUID with ON UPDATE CASCADE, so a re-keyed account (settings sync,
--   decision 0022) takes its rows along.

ALTER TABLE draft ADD COLUMN archive_original INTEGER NOT NULL DEFAULT 0 CHECK (archive_original IN (0, 1));
ALTER TABLE draft ADD COLUMN request_receipt INTEGER NOT NULL DEFAULT 0 CHECK (request_receipt IN (0, 1));

CREATE TABLE receipt_answer (
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE ON UPDATE CASCADE,
  message_id_hdr  TEXT    NOT NULL,                        -- without <>
  state           TEXT    NOT NULL CHECK (state IN ('queued', 'sent', 'declined')),
  operation_id    INTEGER,                                 -- the send of a queued receipt
  updated_at      INTEGER NOT NULL,                        -- Unix ms
  PRIMARY KEY (account_uuid, message_id_hdr)
) STRICT, WITHOUT ROWID;

CREATE TABLE receipt_received (
  account_uuid        TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE ON UPDATE CASCADE,
  original_msgid_hdr  TEXT    NOT NULL,                    -- the reported message, without <>
  recipient           TEXT    NOT NULL,                    -- Final-Recipient; '' when not given
  disposition         TEXT    NOT NULL
                      CHECK (disposition IN ('displayed', 'deleted', 'dispatched', 'processed', 'other')),
  received_at         INTEGER NOT NULL,                    -- the receipt's sort date, Unix ms
  PRIMARY KEY (account_uuid, original_msgid_hdr, recipient)
) STRICT, WITHOUT ROWID;
