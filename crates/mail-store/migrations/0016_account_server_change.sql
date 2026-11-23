-- Finding F1 of the stage 15 security review (decision 0040). Additive only.
--
-- account_server_change (class Gerät): server settings (IMAP and SMTP host, port, security,
--   user name) that arrived for an existing account through the settings sync and differ from
--   the ones this device uses. They are not applied: the stored password would go to the new
--   server. The account keeps its settings until the user confirms the change (the new settings
--   are written, writing is switched off and the password is removed from the keychain) or
--   dismisses it (`dismissed` = 1: this device keeps its settings, the row stays so the change
--   is not offered again and a later change of other fields does not re-version the servers).
--   `imap` and `smtp` are the server objects of the sync record (`{host, port, security,
--   username}`). While a row exists, the sync sees these values as the account's servers, so a
--   local change of another field never sends this device's servers back to the folder.

CREATE TABLE account_server_change (
  account_uuid  TEXT    PRIMARY KEY REFERENCES account(uuid) ON DELETE CASCADE ON UPDATE CASCADE,
  imap          TEXT    NOT NULL CHECK (json_valid(imap)),
  smtp          TEXT    NOT NULL CHECK (json_valid(smtp)),
  dismissed     INTEGER NOT NULL DEFAULT 0 CHECK (dismissed IN (0, 1)),
  received_at   INTEGER NOT NULL                         -- Unix ms
) STRICT, WITHOUT ROWID;
