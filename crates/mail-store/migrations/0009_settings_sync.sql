-- Stage 11: settings sync (docs/SYNC.md, decisions 0014 and 0022). Additive only.
--
-- sync_record (class Gerät): one row per synchronised record of this device, its current
--   version (content, or a tombstone with the version it deleted) and the version last known
--   to be in the sync folder (`synced_version`, NULL = never synced). `content` is the
--   record's JSON as in the encrypted payload; account records carry their per-field
--   versions inside it. Versions are hybrid logical clock values in their fixed-width text
--   form, so they compare as text. A partial index finds the records still to upload.
-- sync_dirty: records changed by the running write transaction. The triggers below fill it;
--   the store's capture hook turns every mark into a new version in `sync_record` and empties
--   it before the transaction commits (decision 0022), so every change of a synchronised table
--   updates the sync state in the same transaction.
-- sync_state (class Gerät): one row: this device's random id in the sync, the highest version
--   it issued or observed (the clock continues after it), whether the sync is on, its account
--   and folder, and when the last complete pass and the last full check ran. `folder_state`
--   is the folder's UIDVALIDITY, UIDNEXT, EXISTS and HIGHESTMODSEQ at the start of the last
--   pass: unchanged, and nothing local to upload, means there is nothing to do.
-- sync_device (class sync, kind `device`): the devices of the sync with name, system and when
--   they were last seen (each device writes its own row, at most daily).
-- sync_adopted (class Gerät): accounts taken over from another device that have no password on
--   this one yet ("password missing").

CREATE TABLE sync_record (
  kind             TEXT    NOT NULL,
  id               TEXT    NOT NULL,
  version          TEXT    NOT NULL,
  synced_version   TEXT,
  content          TEXT    CHECK (content IS NULL OR json_valid(content)),
  deleted_version  TEXT,
  updated_at       INTEGER NOT NULL,                     -- Unix ms
  PRIMARY KEY (kind, id),
  CHECK ((content IS NULL) <> (deleted_version IS NULL))
) STRICT, WITHOUT ROWID;
CREATE INDEX sync_record_dirty ON sync_record(kind, id) WHERE synced_version IS NOT version;

CREATE TABLE sync_dirty (
  kind  TEXT NOT NULL,
  id    TEXT NOT NULL,
  PRIMARY KEY (kind, id)
) STRICT, WITHOUT ROWID;

CREATE TABLE sync_state (
  id                     INTEGER PRIMARY KEY CHECK (id = 1),
  device_id              TEXT    NOT NULL,
  hlc_last               TEXT,
  enabled                INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  account_uuid           TEXT    REFERENCES account(uuid) ON DELETE SET NULL,
  folder                 TEXT,
  last_complete_sync_ms  INTEGER,
  last_full_check_ms     INTEGER,
  folder_state           TEXT,
  updated_at             INTEGER NOT NULL                -- Unix ms
) STRICT;

-- A random UUID v4 as this device's id.
INSERT INTO sync_state (id, device_id, updated_at)
SELECT 1, lower(
  substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-4' || substr(h, 14, 3) || '-' ||
  substr('89ab', 1 + (abs(random()) % 4), 1) || substr(h, 18, 3) || '-' || substr(h, 21, 12)), 0
FROM (SELECT hex(randomblob(16)) AS h);

CREATE TABLE sync_device (
  device_id     TEXT    PRIMARY KEY,
  name          TEXT    NOT NULL,
  system        TEXT    NOT NULL,
  last_seen_ms  INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE sync_adopted (
  account_uuid  TEXT    PRIMARY KEY REFERENCES account(uuid) ON DELETE CASCADE,
  adopted_at    INTEGER NOT NULL                         -- Unix ms
) STRICT, WITHOUT ROWID;

-- Change capture ------------------------------------------------------------------------------
-- Coarse on purpose: every insert, update and delete marks the record; the capture hook
-- compares the content and keeps the version when nothing synchronised changed (for example
-- `writes_enabled`, a snooze coming back, a keyword mirror). Only contacts filter here: their
-- counters change with every sync window, and only the name the user set is synchronised.
-- `NOT EXISTS` instead of `INSERT OR IGNORE`: the conflict clause of the statement that fires
-- a trigger (an upsert, `INSERT OR REPLACE`) would override the trigger's own.

CREATE TRIGGER sync_setting_insert AFTER INSERT ON setting BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'setting' AS k, new.scope || '/' || new.key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_setting_update AFTER UPDATE ON setting BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'setting' AS k, new.scope || '/' || new.key AS i UNION SELECT 'setting' AS k, old.scope || '/' || old.key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_setting_delete AFTER DELETE ON setting BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'setting' AS k, old.scope || '/' || old.key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_account_insert AFTER INSERT ON account BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'account' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_account_update AFTER UPDATE ON account BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'account' AS k, new.uuid AS i UNION SELECT 'account' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_account_delete AFTER DELETE ON account BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'account' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- An identity and its signature are two records (the signature merges with "edit beats
-- delete" on its own). New identities get their UUID from `identity_new_uuid` right after the
-- insert; that update marks them.

CREATE TRIGGER sync_identity_insert AFTER INSERT ON identity BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'identity' AS k, new.uuid AS i UNION SELECT 'signature' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_identity_update AFTER UPDATE ON identity BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'identity' AS k, new.uuid AS i UNION SELECT 'signature' AS k, new.uuid AS i UNION SELECT 'identity' AS k, old.uuid AS i UNION SELECT 'signature' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_identity_delete AFTER DELETE ON identity BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'identity' AS k, old.uuid AS i UNION SELECT 'signature' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_sender_rule_insert AFTER INSERT ON sender_rule BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'senderRule' AS k, new.key_kind || ':' || new.key_value AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_sender_rule_update AFTER UPDATE ON sender_rule BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'senderRule' AS k, new.key_kind || ':' || new.key_value AS i UNION SELECT 'senderRule' AS k, old.key_kind || ':' || old.key_value AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_sender_rule_delete AFTER DELETE ON sender_rule BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'senderRule' AS k, old.key_kind || ':' || old.key_value AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_rule_insert AFTER INSERT ON rule BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'rule' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_rule_update AFTER UPDATE ON rule BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'rule' AS k, new.uuid AS i UNION SELECT 'rule' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_rule_delete AFTER DELETE ON rule BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'rule' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_snooze_insert AFTER INSERT ON snooze BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'snooze' AS k, new.account_uuid || '/' || new.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_snooze_update AFTER UPDATE ON snooze BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'snooze' AS k, new.account_uuid || '/' || new.message_id_hdr AS i UNION SELECT 'snooze' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_snooze_delete AFTER DELETE ON snooze BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'snooze' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_pin_insert AFTER INSERT ON pin BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'pin' AS k, new.account_uuid || '/' || new.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_pin_update AFTER UPDATE ON pin BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'pin' AS k, new.account_uuid || '/' || new.message_id_hdr AS i UNION SELECT 'pin' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_pin_delete AFTER DELETE ON pin BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'pin' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_task_insert AFTER INSERT ON task BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'task' AS k, new.account_uuid || '/' || new.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_task_update AFTER UPDATE ON task BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'task' AS k, new.account_uuid || '/' || new.message_id_hdr AS i UNION SELECT 'task' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_task_delete AFTER DELETE ON task BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'task' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_group_insert AFTER INSERT ON account_group BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'accountGroup' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_group_update AFTER UPDATE ON account_group BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'accountGroup' AS k, new.uuid AS i UNION SELECT 'accountGroup' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_group_delete AFTER DELETE ON account_group BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'accountGroup' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- Members belong to their group's record. While a group is deleted, its members go after it
-- (cascade): the group is marked already, and the lookup finds nothing.

CREATE TRIGGER sync_group_member_insert AFTER INSERT ON account_group_member BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'accountGroup' AS k, (SELECT uuid FROM account_group WHERE id = new.group_id) AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_group_member_update AFTER UPDATE ON account_group_member BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'accountGroup' AS k, (SELECT uuid FROM account_group WHERE id = new.group_id) AS i UNION SELECT 'accountGroup' AS k, (SELECT uuid FROM account_group WHERE id = old.group_id) AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_group_member_delete AFTER DELETE ON account_group_member BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'accountGroup' AS k, (SELECT uuid FROM account_group WHERE id = old.group_id) AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_remote_content_insert AFTER INSERT ON remote_content_sender BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'remoteContent' AS k, new.account_uuid || '/' || new.sender AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_remote_content_update AFTER UPDATE ON remote_content_sender BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'remoteContent' AS k, new.account_uuid || '/' || new.sender AS i UNION SELECT 'remoteContent' AS k, old.account_uuid || '/' || old.sender AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_remote_content_delete AFTER DELETE ON remote_content_sender BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'remoteContent' AS k, old.account_uuid || '/' || old.sender AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_insert AFTER INSERT ON contact WHEN new.user_name IS NOT NULL BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, new.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_update AFTER UPDATE OF user_name, address_key ON contact
WHEN old.user_name IS NOT new.user_name OR old.address_key IS NOT new.address_key BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, new.address_key AS i UNION SELECT 'contact' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_delete AFTER DELETE ON contact WHEN old.user_name IS NOT NULL BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_device_insert AFTER INSERT ON sync_device BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'device' AS k, new.device_id AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_device_update AFTER UPDATE ON sync_device BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'device' AS k, new.device_id AS i UNION SELECT 'device' AS k, old.device_id AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_device_delete AFTER DELETE ON sync_device BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'device' AS k, old.device_id AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- Data stored before this migration: captured at the next start (the engine flushes the marks
-- once its hook is set).
INSERT INTO sync_dirty (kind, id)
  SELECT 'setting', scope || '/' || key FROM setting
  UNION ALL SELECT 'account', uuid FROM account
  UNION ALL SELECT 'identity', uuid FROM identity WHERE uuid IS NOT NULL
  UNION ALL SELECT 'signature', uuid FROM identity WHERE uuid IS NOT NULL AND signature IS NOT NULL
  UNION ALL SELECT 'senderRule', key_kind || ':' || key_value FROM sender_rule
  UNION ALL SELECT 'rule', uuid FROM rule
  UNION ALL SELECT 'snooze', account_uuid || '/' || message_id_hdr FROM snooze
  UNION ALL SELECT 'pin', account_uuid || '/' || message_id_hdr FROM pin
  UNION ALL SELECT 'task', account_uuid || '/' || message_id_hdr FROM task
  UNION ALL SELECT 'accountGroup', uuid FROM account_group
  UNION ALL SELECT 'remoteContent', account_uuid || '/' || sender FROM remote_content_sender
  UNION ALL SELECT 'contact', address_key FROM contact WHERE user_name IS NOT NULL;
