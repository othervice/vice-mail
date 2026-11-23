-- Stage 14, block A (decision 0034): Gmail over IMAP. Additive only; nothing changes for other
-- servers (their columns stay NULL and the table below has no row for them).
--
-- message.gm_msgid (class Cache): X-GM-MSGID, the same number for a message in every label
--   (folder) it lies in. The sync merges locations by it instead of the Message-ID header, so a
--   mail in the inbox, two labels and "All Mail" is one row with four locations, also without
--   a Message-ID. Stored as the signed 64-bit pattern of Gmail's unsigned number.
-- message.gm_labels (class Cache): X-GM-LABELS as last read from the server (JSON array of
--   decoded names, system labels as `\Inbox`), completed with the label of the folder it was
--   read in; NULL for other servers.
-- account_server (class Gerät): what the last login told about the server. `gmail` is
--   X-GM-EXT-1. The planner reads it offline (archive on Gmail removes the inbox label) and the
--   sent copy setting `auto` follows it. Not on `account`, whose update trigger feeds the
--   settings sync.

ALTER TABLE message ADD COLUMN gm_msgid INTEGER;
ALTER TABLE message ADD COLUMN gm_labels TEXT CHECK (gm_labels IS NULL OR json_valid(gm_labels));
CREATE INDEX message_gm_msgid ON message(account_id, gm_msgid) WHERE gm_msgid IS NOT NULL;

CREATE TABLE account_server (
  account_id  INTEGER PRIMARY KEY REFERENCES account(id) ON DELETE CASCADE,
  gmail       INTEGER NOT NULL DEFAULT 0 CHECK (gmail IN (0, 1)),
  updated_at  INTEGER NOT NULL                             -- Unix ms
) STRICT;
