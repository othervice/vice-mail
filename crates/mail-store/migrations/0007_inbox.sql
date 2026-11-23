-- Stage 9: inbox comfort (decisions 0016 and 0020). Additive only.
--
-- Categories (class Cache; computed from the headers and the rules):
--   `message.category` (a column since stage 2) is the category after the user's sender rules:
--   0 person, 1 notification, 2 newsletter, NULL not classified yet. `category_auto` and
--   `category_reason` (JSON of `mail_model::CategoryReason`) are the automatic result, kept so
--   a changed rule is applied again without the headers. `important`: a sender rule marks the
--   sender, domain or list important. `sender_key`, `sender_domain` and `list_key` are the
--   normalised From address, its domain and the List-Id the rules compare with.
--   `unsubscribe_one_click`: `List-Unsubscribe-Post: List-Unsubscribe=One-Click` (RFC 8058) was
--   present at arrival; NULL for messages stored before this migration.
--   The partial index finds messages still to classify (every message stored before this
--   migration; the engine classifies them in the background from the stored header data).
--
-- Category and important mark are copied onto every location, so a mailbox can be listed by
--   category (`loc_category`) or its important messages (`loc_important`) per keyset, like
--   the plain list. Triggers keep the copies: a new location takes the message's values, a
--   changed message updates its locations.
--
-- Sender rules (class sync, Datenart `rule` with `"type": "sender"`, stage 11): one row per
--   normalised key; NULL `category` = only the important mark. `category_job.rules_below`:
--   re-apply the rules to messages below this id (newest first), NULL when nothing is to do.
--
-- Snoozes, pins and tasks (class sync, `snooze`/`pin` of `docs/SYNC.md` and a new kind `task`):
--   by account UUID and Message-ID, never by local id. A snooze is waiting while `returned_at`
--   is NULL; then it stands above the inbox until the user clears it. A task is open while
--   `done_at` is NULL. `keyword` remembers that the server got the keyword (to remove it).
--
-- Local rules (class sync, Datenart `rule`): conditions and actions as JSON, run in `position`
--   order.

ALTER TABLE message ADD COLUMN category_auto INTEGER CHECK (category_auto IN (0, 1, 2));
ALTER TABLE message ADD COLUMN category_reason TEXT;
ALTER TABLE message ADD COLUMN important INTEGER NOT NULL DEFAULT 0 CHECK (important IN (0, 1));
ALTER TABLE message ADD COLUMN sender_key TEXT;
ALTER TABLE message ADD COLUMN sender_domain TEXT;
ALTER TABLE message ADD COLUMN list_key TEXT;
ALTER TABLE message ADD COLUMN unsubscribe_one_click INTEGER CHECK (unsubscribe_one_click IN (0, 1));
CREATE INDEX message_category_todo ON message(id) WHERE category IS NULL;

ALTER TABLE message_location ADD COLUMN category INTEGER;
ALTER TABLE message_location ADD COLUMN important INTEGER NOT NULL DEFAULT 0;
CREATE INDEX loc_category ON message_location(mailbox_id, category, message_id);
CREATE INDEX loc_important ON message_location(mailbox_id, message_id) WHERE important = 1;

-- A new location takes the category of its message (the sync inserts the message first).
CREATE TRIGGER loc_category_new AFTER INSERT ON message_location WHEN new.category IS NULL BEGIN
  UPDATE message_location
  SET category = (SELECT m.category FROM message m WHERE m.id = new.message_id),
      important = coalesce((SELECT m.important FROM message m WHERE m.id = new.message_id), 0)
  WHERE mailbox_id = new.mailbox_id AND message_id = new.message_id;
END;

-- A changed category or important mark reaches every location of the message.
CREATE TRIGGER message_category_change AFTER UPDATE OF category, important ON message
WHEN old.category IS NOT new.category OR old.important <> new.important BEGIN
  UPDATE message_location SET category = new.category, important = new.important
  WHERE message_id = new.id;
END;

CREATE TABLE sender_rule (
  id          INTEGER PRIMARY KEY,
  uuid        TEXT    NOT NULL UNIQUE,                     -- lower case
  key_kind    TEXT    NOT NULL CHECK (key_kind IN ('address', 'domain', 'list_id')),
  key_value   TEXT    NOT NULL,                            -- mail_content::normalize_key
  category    INTEGER CHECK (category IN (0, 1, 2)),       -- NULL: no correction
  important   INTEGER NOT NULL DEFAULT 0 CHECK (important IN (0, 1)),
  created_at  INTEGER NOT NULL,                            -- Unix ms
  updated_at  INTEGER NOT NULL,
  UNIQUE (key_kind, key_value),
  CHECK (category IS NOT NULL OR important = 1)
) STRICT;

CREATE TABLE category_job (
  id           INTEGER PRIMARY KEY CHECK (id = 1),
  rules_below  INTEGER                                     -- NULL: nothing to do
) STRICT;
INSERT INTO category_job (id, rules_below) VALUES (1, NULL);

CREATE TABLE snooze (
  id              INTEGER PRIMARY KEY,
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  message_id_hdr  TEXT    NOT NULL,                        -- without <>
  until           INTEGER NOT NULL,                        -- Unix ms
  returned_at     INTEGER,                                 -- Unix ms; NULL while waiting
  keyword         INTEGER NOT NULL DEFAULT 0 CHECK (keyword IN (0, 1)),
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL,
  UNIQUE (account_uuid, message_id_hdr)
) STRICT;
CREATE INDEX snooze_due ON snooze(until) WHERE returned_at IS NULL;

CREATE TABLE pin (
  id              INTEGER PRIMARY KEY,
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  message_id_hdr  TEXT    NOT NULL,
  pinned_at       INTEGER NOT NULL,                        -- Unix ms
  UNIQUE (account_uuid, message_id_hdr)
) STRICT;

CREATE TABLE task (
  id              INTEGER PRIMARY KEY,
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE,
  message_id_hdr  TEXT    NOT NULL,
  done_at         INTEGER,                                 -- Unix ms; NULL while open
  keyword         INTEGER NOT NULL DEFAULT 0 CHECK (keyword IN (0, 1)),
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL,
  UNIQUE (account_uuid, message_id_hdr)
) STRICT;

CREATE TABLE rule (
  id          INTEGER PRIMARY KEY,
  uuid        TEXT    NOT NULL UNIQUE,
  name        TEXT    NOT NULL,
  position    INTEGER NOT NULL,
  enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  definition  TEXT    NOT NULL CHECK (json_valid(definition)),  -- {matchAll, conditions, actions, stop}
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX rule_position ON rule(position);
