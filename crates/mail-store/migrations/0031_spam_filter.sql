-- Stage 22 (decision 0077): the learning spam filter. Everything here is class Device: it is
-- learned on this device, never travels via the settings sync and cannot be rebuilt from the
-- mail server. Only the switch (`setting` `spam.filter`) is synchronised. "Forget what was
-- learned" empties all four tables.
--
-- spam_token: per token (a 64-bit FNV-1a hash of its text, stored as a signed integer; the
--   database holds no words of mails) how many learned spam and ham mails had it.
-- spam_model: one row: how many mails were learned as spam and as ham.
-- spam_trained: per learned message its class and the exact tokens learned (8 bytes each,
--   little endian), so that it can be unlearned (moved spam → ham, undo) even when its body
--   changed meanwhile. `batch`: the outbox batch of the user's "Spam"/"Not spam" that taught it,
--   for "Undo". When the message row goes (deleted on the server), the record goes; what was
--   learned from it stays in the counts.
-- spam_verdict: what the filter did with a new message: `moved` to the junk folder (`batch` of
--   that move, for "Undo") or marked as `suspected` on this device (read-only account or no junk
--   folder). The list rows and the reading pane show it.

CREATE TABLE spam_token (
  hash  INTEGER PRIMARY KEY,
  spam  INTEGER NOT NULL DEFAULT 0 CHECK (spam >= 0),
  ham   INTEGER NOT NULL DEFAULT 0 CHECK (ham >= 0)
) STRICT;

CREATE TABLE spam_model (
  id    INTEGER PRIMARY KEY CHECK (id = 1),
  spam  INTEGER NOT NULL DEFAULT 0 CHECK (spam >= 0),
  ham   INTEGER NOT NULL DEFAULT 0 CHECK (ham >= 0)
) STRICT;

CREATE TABLE spam_trained (
  message_id  INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  class       TEXT    NOT NULL CHECK (class IN ('spam', 'ham')),
  tokens      BLOB    NOT NULL,
  batch       INTEGER,
  trained_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX spam_trained_batch ON spam_trained(batch) WHERE batch IS NOT NULL;

CREATE TABLE spam_verdict (
  message_id  INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  mark        TEXT    NOT NULL CHECK (mark IN ('moved', 'suspected')),
  score       REAL    NOT NULL CHECK (score BETWEEN 0 AND 1),
  batch       INTEGER,
  at          INTEGER NOT NULL
) STRICT;
CREATE INDEX spam_verdict_batch ON spam_verdict(batch) WHERE batch IS NOT NULL;
