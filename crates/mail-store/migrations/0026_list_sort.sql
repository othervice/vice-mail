-- Stage 19, task A (decision 0061): sorting a list by sender, recipient, subject and size.
--
-- message_location gets the sort keys of its message, so a list sorted by one of them is a
-- range of an index of the location table like the newest-first list (keyset-paged, no sort of
-- the whole mailbox, decision 0003):
--   sort_sender     the name the row shows for the sender (display name, else address), folded
--                   (`mail_model::sender_sort_key`: lower case, accents of Latin letters removed,
--                   leading punctuation dropped, at most 40 characters);
--   sort_recipient  the first recipient the same way (Sent and Drafts sort by it);
--   sort_subject    the subject without reply and forward prefixes, folded;
--   size            the message's size in bytes.
-- The keys never change: sender, recipients, subject and size of a message are fixed. They are
-- computed by functions the store registers on its write connection (`sort_keys.rs`); every
-- statement that inserts a location fills them from `message` (`messages::INSERT_LOCATION`,
-- `outbox::INSERT_LOCATION_IF_FREE`). NOT NULL, so keyset comparisons never meet a NULL.
--
-- Each index ends in `message_id`: equal keys sort by date in the same direction, and the
-- cursor `(key, message_id)` is a row-value range of the index.

ALTER TABLE message_location ADD COLUMN sort_sender TEXT NOT NULL DEFAULT '';
ALTER TABLE message_location ADD COLUMN sort_recipient TEXT NOT NULL DEFAULT '';
ALTER TABLE message_location ADD COLUMN sort_subject TEXT NOT NULL DEFAULT '';
ALTER TABLE message_location ADD COLUMN size INTEGER NOT NULL DEFAULT 0 CHECK (size >= 0);

UPDATE message_location
SET sort_sender = vm_sort_name(m.from_name, m.from_addr),
    sort_recipient = vm_sort_recipient(m.to_addrs),
    sort_subject = vm_sort_subject(m.subject),
    size = max(m.size, 0)
FROM message m
WHERE m.id = message_location.message_id;

CREATE INDEX loc_sort_sender ON message_location(mailbox_id, sort_sender, message_id);
CREATE INDEX loc_sort_recipient ON message_location(mailbox_id, sort_recipient, message_id);
CREATE INDEX loc_sort_subject ON message_location(mailbox_id, sort_subject, message_id);
CREATE INDEX loc_sort_size ON message_location(mailbox_id, size, message_id);
