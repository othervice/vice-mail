-- Key-value settings (docs/ARCHITECTURE.md, Datenmodell: key, value, scope, version).
--   scope       'global' for app-wide settings; later stages add per-account scopes.
--   value       JSON text.
--   version     counts changes of the row, starting at 1. Stage 11 (settings sync) may turn it
--               into a hybrid logical clock.
--   updated_at  Unix milliseconds of the last change.
CREATE TABLE setting (
  scope      TEXT    NOT NULL,
  key        TEXT    NOT NULL,
  value      TEXT    NOT NULL CHECK (json_valid(value)),
  version    INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (scope, key)
) STRICT, WITHOUT ROWID;
