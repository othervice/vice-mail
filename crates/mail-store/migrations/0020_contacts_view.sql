-- Stage 17, block B (decision 0050): every address becomes a contact, contacts get a kind, a
-- note and a picture. Additive, except that the contact triggers of 0010 are replaced.
--
-- message.reply_to_addrs: Reply-To of a message, JSON [{"name", "email"}] like `to_addrs`,
--   from the header block the adapter fetches for the category rules; NULL when it was not
--   fetched or names nobody.
--
-- message.contacts_seen: 1 = read into the directory by the rules of stage 10 (the sender of a
--   mail from a person, the recipients of sent mail), 2 = by the rules of stage 17 (every
--   address of From, To, Cc and Reply-To). The partial index finds the messages read before,
--   for the background backfill that adds only what stage 10 left out (newest last, it never
--   blocks the start). It stays empty afterwards (queries name it with INDEXED BY).
--
-- contact (class Cache, except the user's fields):
--   seen, last_seen   other messages the address was in (copied, or as Reply-To; no rank);
--   personal          it was seen in the user's own mail or in mail from a person (every row
--                     of stage 10 was);
--   kind              0 person, 1 list (newsletters, notifications, list mail only),
--                     2 system (nobody reads it, or a program sends from it): computed by the
--                     engine (`mail_content::contact_kind`), filtered in the contacts view;
--   note              the user's note (class sync, part of the `contact` record).
--
-- contact_picture (class sync, kind `contactPicture`): the picture the user chose for the
--   contact with this address (its preferred address when it was set), a JPEG of 256 × 256 px
--   and at most 40 KiB written by the core's helper process (decision 0046). `display` is what
--   the UI is shown: the same bytes for a picture chosen here; for one from the settings sync
--   NULL until the helper encoded it again on this device. `revision` (Unix ms) changes with
--   every new picture; the UI caches by it. Only changes of `image` are synchronised.

ALTER TABLE message ADD COLUMN reply_to_addrs TEXT;

CREATE INDEX message_contacts_backfill ON message(id) WHERE contacts_seen = 1;

-- Existing mail from an address (stage 17: "mark as spam" moves it, the contact shows it).
CREATE INDEX message_sender ON message(sender_key, id) WHERE sender_key IS NOT NULL;

ALTER TABLE contact ADD COLUMN seen INTEGER NOT NULL DEFAULT 0 CHECK (seen >= 0);
ALTER TABLE contact ADD COLUMN last_seen INTEGER;
ALTER TABLE contact ADD COLUMN personal INTEGER NOT NULL DEFAULT 1 CHECK (personal IN (0, 1));
ALTER TABLE contact ADD COLUMN kind INTEGER NOT NULL DEFAULT 0 CHECK (kind IN (0, 1, 2));
ALTER TABLE contact ADD COLUMN note TEXT;

CREATE TABLE contact_picture (
  id           INTEGER PRIMARY KEY,
  address_key  TEXT    NOT NULL UNIQUE,                 -- mail_content::NormalizedAddress::key
  image        BLOB    NOT NULL,                        -- JPEG, the synchronised bytes
  display      BLOB,                                    -- JPEG encoded on this device
  revision     INTEGER NOT NULL                         -- Unix ms
) STRICT;

-- Change capture: a contact record now also holds the note.

DROP TRIGGER sync_contact_insert;
DROP TRIGGER sync_contact_update;
DROP TRIGGER sync_contact_delete;

CREATE TRIGGER sync_contact_insert AFTER INSERT ON contact
WHEN new.user_name IS NOT NULL OR new.contact_group IS NOT NULL OR new.note IS NOT NULL BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, new.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_update AFTER UPDATE OF user_name, address_key, contact_group, note ON contact
WHEN old.user_name IS NOT new.user_name OR old.address_key IS NOT new.address_key
  OR old.contact_group IS NOT new.contact_group OR old.note IS NOT new.note BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, new.address_key AS i UNION SELECT 'contact' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_delete AFTER DELETE ON contact
WHEN old.user_name IS NOT NULL OR old.contact_group IS NOT NULL OR old.note IS NOT NULL BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contact' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_picture_insert AFTER INSERT ON contact_picture BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactPicture' AS k, new.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_picture_update AFTER UPDATE OF image, address_key ON contact_picture
WHEN old.image IS NOT new.image OR old.address_key IS NOT new.address_key BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactPicture' AS k, new.address_key AS i UNION SELECT 'contactPicture' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_picture_delete AFTER DELETE ON contact_picture BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactPicture' AS k, old.address_key AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;
