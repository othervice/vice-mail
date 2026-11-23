-- Stage 7 (decision 0026): saved searches and this device's recent searches.
--
-- saved_view (class sync, kind `savedView`): a query of the search language with a name, shown
--   as a view in the sidebar; it searches all accounts (the query narrows with `account:` and
--   `in:`). Names are unique without case only where they are created or renamed here (the
--   engine checks it in the writing transaction): two devices may each save "Unread" and the
--   sync keeps both, so the table has no constraint on it.
-- recent_search (class device): the newest searches of this device, at most
--   `MAX_RECENT_SEARCHES`; the same query again only moves up. Never synchronised.

CREATE TABLE saved_view (
  id          INTEGER PRIMARY KEY,
  uuid        TEXT    NOT NULL UNIQUE,
  name        TEXT    NOT NULL,
  query       TEXT    NOT NULL,
  position    INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,                        -- Unix ms
  updated_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE recent_search (
  query       TEXT    PRIMARY KEY,
  used_at     INTEGER NOT NULL                         -- Unix ms
) STRICT, WITHOUT ROWID;

CREATE INDEX recent_search_used ON recent_search (used_at);

CREATE TRIGGER sync_saved_view_insert AFTER INSERT ON saved_view BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'savedView' AS k, new.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_saved_view_update AFTER UPDATE ON saved_view BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'savedView' AS k, new.uuid AS i UNION SELECT 'savedView' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;

CREATE TRIGGER sync_saved_view_delete AFTER DELETE ON saved_view BEGIN
  INSERT INTO sync_dirty (kind, id) SELECT k, i FROM (SELECT 'savedView' AS k, old.uuid AS i) AS m
   WHERE m.i IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sync_dirty d WHERE d.kind = m.k AND d.id = m.i);
END;
