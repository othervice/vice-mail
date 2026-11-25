-- Stage 19, block C (decision 0056): contact groups ("Kontaktgruppen", distribution lists) on
-- the contacts page and in the composer. Class sync (record kind `contactList`, keyed by UUID):
-- the user writes them like names and notes of contacts, so they travel like those.
--
-- contact_list: one group. `name` as the user wrote it (at most 100 characters, checked by the
--   engine), `search` its folded words for the composer's suggestions (' word word …').
-- contact_list_member: one address of a group, in the order added. `address_key` is
--   mail_content::NormalizedAddress::key (one member per address), `email` the address as the
--   user added it, `name` the display name given with it (NULL: the contact's name). Members
--   need no contact row: the directory is a cache, a group is the user's.

CREATE TABLE contact_list (
  id        INTEGER PRIMARY KEY,
  uuid      TEXT    NOT NULL UNIQUE,
  name      TEXT    NOT NULL,
  search    TEXT    NOT NULL,
  created   INTEGER NOT NULL,                     -- Unix ms
  updated   INTEGER NOT NULL                      -- Unix ms
) STRICT;

CREATE TABLE contact_list_member (
  list_id      INTEGER NOT NULL REFERENCES contact_list(id) ON DELETE CASCADE,
  address_key  TEXT    NOT NULL,
  email        TEXT    NOT NULL,
  name         TEXT,
  position     INTEGER NOT NULL,
  PRIMARY KEY (list_id, address_key)
) STRICT, WITHOUT ROWID;

CREATE INDEX contact_list_member_order ON contact_list_member(list_id, position);

-- Change capture (stage 11): the group and its members are one record.

CREATE TRIGGER sync_contact_list_insert AFTER INSERT ON contact_list BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactList' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_list_update AFTER UPDATE OF uuid, name ON contact_list
WHEN old.uuid IS NOT new.uuid OR old.name IS NOT new.name BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactList' AS k, new.uuid AS i UNION SELECT 'contactList' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_list_delete AFTER DELETE ON contact_list BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactList' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- Members belong to their group's record. While a group is deleted, its members go after it
-- (cascade): the group is marked already, and the lookup finds nothing.

CREATE TRIGGER sync_contact_list_member_insert AFTER INSERT ON contact_list_member BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactList' AS k, (SELECT uuid FROM contact_list WHERE id = new.list_id) AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_list_member_update AFTER UPDATE ON contact_list_member BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactList' AS k, (SELECT uuid FROM contact_list WHERE id = new.list_id) AS i UNION SELECT 'contactList' AS k, (SELECT uuid FROM contact_list WHERE id = old.list_id) AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_contact_list_member_delete AFTER DELETE ON contact_list_member BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'contactList' AS k, (SELECT uuid FROM contact_list WHERE id = old.list_id) AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;
