-- Stage 12: AI access over MCP (decisions 0010, 0015, 0021). Additive only. Every table is
-- class Gerät: AI access is set up per device (the client keys live in this device's keychain,
-- the helper path and the endpoint are local) and never leaves it through the settings sync.
--
-- `mcp_state`: one row. `enabled` is the master switch (off: no socket, every connection
--   closed). `endpoint`: the Windows pipe name, made once and reused on every start (it is
--   in the client configurations); NULL on macOS, where the socket path follows from the data
--   directory.
-- `mcp_account`: the access level of a shared account; no row = hidden (the default).
-- `mcp_client`: registered AI clients. The key is in the keychain under `credential_ref`,
--   never here. A revoked client keeps its row (name for the access log, `revoked_at`), its
--   keychain entry is deleted.
-- `mcp_access`: the access log, metadata only (client, tool, time, number of messages,
--   outcome, reason of a refused connection); never arguments or content. Kept 30 days and at
--   most 10,000 entries (pruned when entries are written).

CREATE TABLE mcp_state (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  enabled     INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  endpoint    TEXT,
  updated_at  INTEGER NOT NULL                      -- Unix ms
) STRICT;

INSERT INTO mcp_state (id, enabled, endpoint, updated_at) VALUES (1, 0, NULL, 0);

CREATE TABLE mcp_account (
  account_id  INTEGER PRIMARY KEY REFERENCES account(id) ON DELETE CASCADE,
  access      TEXT    NOT NULL CHECK (access IN ('read', 'read_and_drafts')),
  updated_at  INTEGER NOT NULL                      -- Unix ms
) STRICT;

CREATE TABLE mcp_client (
  id              INTEGER PRIMARY KEY,
  client_id       TEXT    NOT NULL UNIQUE CHECK (length(client_id) = 32),
  name            TEXT    NOT NULL,
  credential_ref  TEXT    NOT NULL,                 -- keychain entry name, never the key
  created_at      INTEGER NOT NULL,                 -- Unix ms
  revoked_at      INTEGER,
  last_used_at    INTEGER
) STRICT;

CREATE TABLE mcp_access (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  at           INTEGER NOT NULL,                    -- Unix ms
  client_id    TEXT,
  client_name  TEXT,
  tool         TEXT,                                -- NULL: a refused connection
  messages     INTEGER NOT NULL DEFAULT 0 CHECK (messages >= 0),
  outcome      TEXT    NOT NULL CHECK (outcome IN ('ok', 'error', 'refused')),
  refusal      TEXT
) STRICT;

CREATE INDEX mcp_access_at ON mcp_access(at);
