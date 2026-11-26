-- Stage 22 (decision 0072): contacts have no pictures any more. Removes what stage 17 (decision
-- 0050, migration 0020) stored for them:
--
-- contact_picture with its change-capture triggers: the pictures the user chose or imported from
--   vCards, and the copies the helper encoded on this device.
-- sync_record and sync_dirty rows of kind `contactPicture`: this device's state of the picture
--   records of the settings sync, which hold the same JPEGs in base64. The kind is retired: the
--   engine ignores such records in the sync folder without a notice and never writes one, so
--   nothing needs these rows. The entries stay in the folder for devices with an older version.
--
-- Pictures were only ever stored in the database (never as files). The backups taken before a
-- migration (at most three, `backups/`) keep them until newer backups replace them.

DROP TRIGGER IF EXISTS sync_contact_picture_insert;
DROP TRIGGER IF EXISTS sync_contact_picture_update;
DROP TRIGGER IF EXISTS sync_contact_picture_delete;
DROP TABLE contact_picture;

DELETE FROM sync_dirty WHERE kind = 'contactPicture';
DELETE FROM sync_record WHERE kind = 'contactPicture';
