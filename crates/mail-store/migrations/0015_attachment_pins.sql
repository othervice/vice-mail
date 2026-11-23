-- Stage 16: pinned attachments (decision 0036). Additive only.
--
-- attachment_pin (class sync, kind `attachmentPin`): an attachment the user pinned, by account
--   UUID, Message-ID (without <>) and the attachment's index in the reading view. Name, type
--   and size are kept as they were when it was pinned, so "Pinned attachments" lists it without
--   the body. Refers to the account UUID with ON UPDATE CASCADE like the other stage 16 tables
--   (a re-keyed account takes its rows along, decision 0022).

CREATE TABLE attachment_pin (
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE ON UPDATE CASCADE,
  message_id_hdr  TEXT    NOT NULL,                        -- without <>
  part_index      INTEGER NOT NULL CHECK (part_index >= 0),
  name            TEXT    NOT NULL,                        -- sanitised file name
  mime_type       TEXT    NOT NULL,
  size            INTEGER NOT NULL CHECK (size >= 0),
  pinned_at       INTEGER NOT NULL,                        -- Unix ms
  PRIMARY KEY (account_uuid, message_id_hdr, part_index)
) STRICT, WITHOUT ROWID;
CREATE INDEX attachment_pin_at ON attachment_pin(pinned_at);

CREATE TRIGGER sync_attachment_pin_insert AFTER INSERT ON attachment_pin BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'attachmentPin' AS k, new.account_uuid || '/' || new.message_id_hdr || '/' || new.part_index AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_attachment_pin_update AFTER UPDATE ON attachment_pin BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'attachmentPin' AS k, new.account_uuid || '/' || new.message_id_hdr || '/' || new.part_index AS i UNION SELECT 'attachmentPin' AS k, old.account_uuid || '/' || old.message_id_hdr || '/' || old.part_index AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_attachment_pin_delete AFTER DELETE ON attachment_pin BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'attachmentPin' AS k, old.account_uuid || '/' || old.message_id_hdr || '/' || old.part_index AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;
