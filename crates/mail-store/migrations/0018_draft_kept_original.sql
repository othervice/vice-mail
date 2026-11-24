-- Audit 24.11.2026: drafts of other programs whose original stays in the Drafts folder.
--
-- draft.kept_original (class Gerät, like the draft): set when the draft was made from a copy
--   another program saved in the Drafts folder that the editor could not take over completely
--   (an image from the web, a part it cannot show, a message stored only in part). That copy is
--   never replaced or removed; the draft got a Message-ID of its own and is saved next to it.
--   The value is the original's Message-ID without <> ('' when it had none), so opening the
--   original again finds this draft instead of making another one. NULL for every other draft.

ALTER TABLE draft ADD COLUMN kept_original TEXT;

-- Opening the original again: one lookup per account and Message-ID; only such drafts.
CREATE INDEX draft_kept_original ON draft(account_id, kept_original) WHERE kept_original IS NOT NULL;
