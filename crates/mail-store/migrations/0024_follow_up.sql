-- Stage 20, task B (decision 0059): reminders when no reply comes. Additive only.
--
-- follow_up (class sync, kind `followUp`, like `snooze`): "remind me if nobody answers by
--   `due`" for a message the user sent, by account UUID and Message-ID (without <>), never by
--   local id. `sent_at` is when the message was sent (Unix ms): a message of its conversation
--   after that time from someone else is the answer, and the reminder goes. The reminder is
--   waiting while `returned_at` is NULL; once due without an answer it stands above the inbox
--   ("No reply since …") until the user clears it or an answer comes. Whether it came back is
--   each device's own business (only `due` and `sent_at` are synchronised).
--
-- draft.follow_up_at (class Gerät, like the draft): the reminder chosen in the composer; the
--   transaction that confirms the send records it in `follow_up`. NULL: none.

CREATE TABLE follow_up (
  account_uuid    TEXT    NOT NULL REFERENCES account(uuid) ON DELETE CASCADE ON UPDATE CASCADE,
  message_id_hdr  TEXT    NOT NULL,                        -- without <>
  due             INTEGER NOT NULL,                        -- Unix ms
  sent_at         INTEGER NOT NULL,                        -- Unix ms
  returned_at     INTEGER,                                 -- Unix ms; NULL while waiting
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL,
  PRIMARY KEY (account_uuid, message_id_hdr)
) STRICT, WITHOUT ROWID;
CREATE INDEX follow_up_due ON follow_up(due) WHERE returned_at IS NULL;

CREATE TRIGGER sync_follow_up_insert AFTER INSERT ON follow_up BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'followUp' AS k, new.account_uuid || '/' || new.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

-- Coming back changes nothing that is synchronised.
CREATE TRIGGER sync_follow_up_update AFTER UPDATE ON follow_up
WHEN old.due IS NOT new.due OR old.sent_at IS NOT new.sent_at
  OR old.account_uuid IS NOT new.account_uuid BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'followUp' AS k, new.account_uuid || '/' || new.message_id_hdr AS i UNION SELECT 'followUp' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_follow_up_delete AFTER DELETE ON follow_up BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'followUp' AS k, old.account_uuid || '/' || old.message_id_hdr AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

ALTER TABLE draft ADD COLUMN follow_up_at INTEGER;
