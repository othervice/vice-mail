-- Stage 13: AI in the app (decision 0024). Additive only. Both tables are class Gerät and never
-- leave the device through the settings sync. The AI settings themselves (switch, provider
-- profiles, choice per function, cloud permission per account, consent) are one JSON value in
-- `setting` (`global`, `ai.settings`), a device setting; the API keys are in the keychain,
-- never in the database.
--
-- `ai_request`: the request log, metadata only (when, which function, which profile and host,
--   local or not, model, number of messages, bytes sent, outcome, error code, duration). Never
--   a question, a mail text, an answer or a key. Kept 30 days and at most 10,000 entries
--   (pruned when entries are written).
-- `open_reply_dismissal`: open-reply hints the user dismissed; they do not come back for that
--   message. Gone with the message.

CREATE TABLE ai_request (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  at           INTEGER NOT NULL,                    -- Unix ms
  function     TEXT    NOT NULL CHECK (function IN ('summary', 'reply_suggestion',
                                                    'writing_help', 'search')),
  profile_name TEXT    NOT NULL,
  host         TEXT    NOT NULL,
  locality     TEXT    NOT NULL CHECK (locality IN ('this_device', 'local_network', 'internet')),
  model        TEXT    NOT NULL,
  mails        INTEGER NOT NULL DEFAULT 0 CHECK (mails >= 0),
  bytes        INTEGER NOT NULL DEFAULT 0 CHECK (bytes >= 0),
  outcome      TEXT    NOT NULL CHECK (outcome IN ('ok', 'cancelled', 'failed')),
  error        TEXT,                                -- ErrorCode (snake_case) for 'failed'
  duration_ms  INTEGER NOT NULL DEFAULT 0 CHECK (duration_ms >= 0)
) STRICT;

CREATE INDEX ai_request_at ON ai_request(at);

CREATE TABLE open_reply_dismissal (
  message_id    INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  dismissed_at  INTEGER NOT NULL                    -- Unix ms
) STRICT;
