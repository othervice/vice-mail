-- Stage 10: several signatures (HTML with images and links), templates, default signatures per
-- identity for new mails and for replies, the signature a draft uses (decision 0023).
--
-- signature (class sync, kind `signature`): a pool shared by all identities. `html` is the
--   allowlisted editor HTML (`mail_content::Signature::from_html`), NULL for a signature from
--   stage 5 that only has text (the engine derives the HTML from `text`). `images` are the
--   pictures the HTML shows as `cid:<contentId>`: `[{id, contentId, fileName, mime, dataBase64,
--   width, height}]`, small rasters the core read from files the user picked or dropped.
-- template (class sync, kind `template`): subject and editor HTML with placeholders.
-- identity.signature_new / signature_reply: the default signature (UUID) of new mails and of
--   replies and forwards. Not a foreign key: the sync applies identities before signatures, and
--   a default that points nowhere means "none". Deleting a signature leaves the defaults as they
--   are: an edit on another device beats the delete and comes back as the default it was.
-- draft.signature_uuid: the signature the composer switched to; NULL = the identity's default.
--
-- The text signature of stage 5 (`identity.signature`) becomes a signature whose UUID is the
-- identity's UUID, so its sync record (`signature/<identity uuid>`) keeps its id; both defaults
-- of the identity point to it. `identity.signature` stays as an unused column.

CREATE TABLE signature (
  id          INTEGER PRIMARY KEY,
  uuid        TEXT    NOT NULL UNIQUE,
  name        TEXT    NOT NULL,
  html        TEXT,
  text        TEXT    NOT NULL,
  images      TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(images)),
  position    INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,                        -- Unix ms
  updated_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE template (
  id          INTEGER PRIMARY KEY,
  uuid        TEXT    NOT NULL UNIQUE,
  name        TEXT    NOT NULL,
  subject     TEXT    NOT NULL,
  html        TEXT    NOT NULL,
  position    INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,                        -- Unix ms
  updated_at  INTEGER NOT NULL
) STRICT;

ALTER TABLE identity ADD COLUMN signature_new TEXT;
ALTER TABLE identity ADD COLUMN signature_reply TEXT;
ALTER TABLE draft ADD COLUMN signature_uuid TEXT;

-- Change capture (decision 0022): an identity and a signature are separate records now; the
-- identity's record carries its defaults.

DROP TRIGGER sync_identity_insert;
DROP TRIGGER sync_identity_update;
DROP TRIGGER sync_identity_delete;

CREATE TRIGGER sync_identity_insert AFTER INSERT ON identity BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'identity' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_identity_update AFTER UPDATE ON identity BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'identity' AS k, new.uuid AS i UNION SELECT 'identity' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_identity_delete AFTER DELETE ON identity BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'identity' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_signature_insert AFTER INSERT ON signature BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'signature' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_signature_update AFTER UPDATE ON signature BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'signature' AS k, new.uuid AS i UNION SELECT 'signature' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_signature_delete AFTER DELETE ON signature BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'signature' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_template_insert AFTER INSERT ON template BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'template' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_template_update AFTER UPDATE ON template BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'template' AS k, new.uuid AS i UNION SELECT 'template' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_template_delete AFTER DELETE ON template BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'template' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- The text signatures of stage 5 (the triggers above mark the new rows and the identities, so
-- the next start versions both).
INSERT INTO signature (uuid, name, html, text, images, position, created_at, updated_at)
SELECT uuid, email, NULL, signature, '[]', id,
       CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000
FROM identity
WHERE uuid IS NOT NULL AND signature IS NOT NULL AND trim(signature) != '';

UPDATE identity SET signature_new = uuid, signature_reply = uuid, signature = NULL
WHERE uuid IS NOT NULL AND signature IS NOT NULL AND trim(signature) != '';
UPDATE identity SET signature = NULL WHERE signature IS NOT NULL;

-- Contacts ------------------------------------------------------------------------------------
-- Stage 10: contacts the user merged, and duplicate suggestions the user dismissed
-- (decisions 0016, 0023). Additive, except that the contact triggers of 0009 are replaced.
--
-- Merges: the rows of merged addresses share `contact.contact_group` (column of 0006): the id of
--   the row of the contact that was kept (its "head"; the head's own `contact_group` is its id).
--   A row without a group is a contact of its own. Merges are synchronised as part of the
--   `contact` record of each merged address: `{mergedInto: <address key of the head>}`.
-- contact_dismissed (class Gerät): pairs of contact keys the user said are not the same person
--   (`a_key <= b_key`; a key paired with itself dismisses "one address, different names").
--   They never come back as suggestions on this device.

CREATE TABLE contact_dismissed (
  a_key       TEXT    NOT NULL,
  b_key       TEXT    NOT NULL,
  created_at  INTEGER NOT NULL,                          -- Unix ms
  PRIMARY KEY (a_key, b_key),
  CHECK (a_key <= b_key)
) STRICT, WITHOUT ROWID;

-- Change capture: a contact record now holds the name the user gave and the merge.

DROP TRIGGER sync_contact_insert;
DROP TRIGGER sync_contact_update;
DROP TRIGGER sync_contact_delete;

CREATE TRIGGER sync_contact_insert AFTER INSERT ON contact
WHEN new.user_name IS NOT NULL OR new.contact_group IS NOT NULL BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, new.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_update AFTER UPDATE OF user_name, address_key, contact_group ON contact
WHEN old.user_name IS NOT new.user_name OR old.address_key IS NOT new.address_key
  OR old.contact_group IS NOT new.contact_group BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, new.address_key AS i UNION SELECT 'contact' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_delete AFTER DELETE ON contact
WHEN old.user_name IS NOT NULL OR old.contact_group IS NOT NULL BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;
