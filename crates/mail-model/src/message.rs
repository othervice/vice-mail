//! Message header data: what the adapter fetches, what the store keeps, what the list shows.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{AccountId, ListCursor, MailboxId, MessageId, ModSeq, Uid};

/// Largest sort date the store accepts (2^33 seconds, year 2242). Keeps `message.id` exact
/// in JavaScript and leaves room for the "next free number" rule.
pub const MAX_SORT_DATE: i64 = 1 << 33;

/// Seconds a `Date` header may lie after INTERNALDATE and still be trusted (decision 0003).
pub const DATE_HEADER_TOLERANCE: i64 = 24 * 60 * 60;

/// A mailbox address. `email` may be empty for broken headers; groups are flattened.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// Decoded display name (RFC 2047), null if none.
    pub name: Option<String>,
    pub email: String,
}

/// IMAP flags of one message in one mailbox. Flags belong to the location, as in IMAP.
/// `\Recent` is ignored. Serialised only into internal records (outbox payloads).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Flags {
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
    pub draft: bool,
    pub deleted: bool,
    /// Keywords (`$Forwarded`, `$Junk`, user labels). Keep them normalised with
    /// [`Flags::normalize_keywords`]: sorted and unique, both ignoring ASCII case, in the
    /// spelling first seen. Compared case-insensitively, because servers (Stalwart) lower-case them.
    pub keywords: Vec<String>,
}

impl Flags {
    /// Builds flags from IMAP flag atoms as sent by the server (`\Seen`, `$Forwarded`, ...).
    /// Unknown system flags (starting with `\`) are dropped.
    pub fn from_imap<I, S>(atoms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut flags = Self::default();
        for atom in atoms {
            let atom = atom.as_ref().trim();
            match atom.strip_prefix('\\') {
                Some(system) => match system.to_ascii_lowercase().as_str() {
                    "seen" => flags.seen = true,
                    "answered" => flags.answered = true,
                    "flagged" => flags.flagged = true,
                    "draft" => flags.draft = true,
                    "deleted" => flags.deleted = true,
                    _ => {}
                },
                None if !atom.is_empty() => flags.keywords.push(atom.to_owned()),
                None => {}
            }
        }
        flags.normalize_keywords();
        flags
    }

    /// Sorts keywords and removes duplicates, both ignoring ASCII case.
    pub fn normalize_keywords(&mut self) {
        self.keywords.sort_by_cached_key(|k| k.to_ascii_lowercase());
        self.keywords.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    }

    /// Whether `keyword` is set (case-insensitive).
    pub fn has_keyword(&self, keyword: &str) -> bool {
        self.keywords
            .iter()
            .any(|k| k.eq_ignore_ascii_case(keyword))
    }
}

/// Header data of one message, as the adapter returns it after FETCH. Internal, not exported.
/// All text is decoded (RFC 2047, charsets) to Unicode; message ids are without angle brackets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHeader {
    pub uid: Uid,
    /// MODSEQ of the message, if the server has CONDSTORE.
    pub modseq: Option<ModSeq>,
    pub flags: Flags,
    /// RFC822.SIZE in bytes.
    pub size: u32,
    /// INTERNALDATE as Unix seconds (UTC).
    pub internal_date: i64,
    /// `Date` header as Unix seconds, `None` if missing or unreadable.
    pub date: Option<i64>,
    /// Empty if the message has no subject.
    pub subject: String,
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// `Message-ID`, `None` if missing or empty. Such messages are never merged.
    pub message_id: Option<String>,
    /// First message id in `In-Reply-To`.
    pub in_reply_to: Option<String>,
    /// Message ids in `References`, oldest first.
    pub references: Vec<String>,
    /// Raw header values (unfolded, trimmed), `None` if absent.
    pub list_id: Option<String>,
    pub list_unsubscribe: Option<String>,
    pub auto_submitted: Option<String>,
    pub precedence: Option<String>,
    /// From BODYSTRUCTURE: some part has `Content-Disposition: attachment`, or a non-text part
    /// has a file name.
    pub has_attachments: bool,
    /// Stage 9: the raw `HEADER.FIELDS` block the adapter fetched, with the fields the
    /// category rules read (`mail_content::CLASSIFY_HEADER_FIELDS`, decision 0016); empty if
    /// not fetched. The store hands it to its classifier and does not keep it.
    pub category_headers: Vec<u8>,
    /// Stage 14 (decision 0034): `X-GM-MSGID` and `X-GM-LABELS` from Gmail servers
    /// (`X-GM-EXT-1`); `None` for every other server.
    pub gmail: Option<crate::GmailInfo>,
}

impl MessageHeader {
    /// Sort date (decision 0003): the `Date` header if it lies at most 24 hours after
    /// INTERNALDATE, otherwise INTERNALDATE; clamped to `0..=MAX_SORT_DATE`. The store derives
    /// `message.id` from it.
    pub fn sort_date(&self) -> i64 {
        sort_date(self.date, self.internal_date)
    }
}

/// See [`MessageHeader::sort_date`].
pub fn sort_date(date_header: Option<i64>, internal_date: i64) -> i64 {
    let chosen = match date_header {
        Some(date) if date <= internal_date.saturating_add(DATE_HEADER_TOLERANCE) => date,
        _ => internal_date,
    };
    chosen.clamp(0, MAX_SORT_DATE)
}

/// New flags of one message, from a flag sync. Internal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagUpdate {
    pub uid: Uid,
    pub flags: Flags,
    pub modseq: Option<ModSeq>,
    /// Stage 14 (decision 0034): `X-GM-LABELS` read with the flags on Gmail servers; `None`
    /// elsewhere (and when not asked for).
    pub gmail_labels: Option<Vec<String>>,
}

/// Most ids one `getMessagesByIds` asks for (stage 13, decision 0027: the sources of an AI
/// answer, at most a few dozen).
pub const MAX_MESSAGES_BY_IDS: u32 = 200;

/// One row of the message list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageListItem {
    pub id: MessageId,
    pub account_id: AccountId,
    pub mailbox_id: MailboxId,
    /// First `From` address, null if none.
    pub from: Option<Address>,
    /// `To` addresses, for lists of sent mail and drafts.
    pub to: Vec<Address>,
    /// Empty if the message has no subject.
    pub subject: String,
    /// Sort date in Unix milliseconds.
    #[specta(type = Number)]
    pub date: i64,
    pub unread: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
    pub has_attachments: bool,
    /// Keywords as stored (for example `$Forwarded`), see [`Flags::keywords`].
    pub keywords: Vec<String>,
    /// Start of the text, white space collapsed, at most 256 characters; null until the body
    /// was parsed (opened or prefetched).
    pub preview: Option<String>,
    /// Outbox state of a change to this message, for a small symbol in the row. Absent when
    /// nothing is pending or needs the user (most rows), so pages stay small.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub activity: Option<MessageActivity>,
    /// Stage 9: the category after the user's sender rules; null until the message is
    /// classified (treat it as a person's).
    pub category: Option<crate::MessageCategory>,
    /// Stage 9: the sender, domain or list is marked important ("Priority" block).
    pub important: bool,
    /// Stage 9: pinned.
    pub pinned: bool,
    /// Stage 9: snoozed (waiting, or came back). Absent when not snoozed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub snooze: Option<crate::SnoozeMark>,
    /// Stage 9: marked as a task. Absent when not.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub task: Option<crate::TaskMark>,
    /// Stage 16: on the reading list ("Zum Lesen"). Absent when not.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[specta(optional)]
    pub reading_list: bool,
    /// Stage 20 (decision 0059): a reminder if no reply comes. Absent when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub follow_up: Option<crate::FollowUpMark>,
    /// Stage 19: the message's size in bytes (`RFC822.SIZE`), for the table's size column.
    /// Absent when unknown.
    #[serde(skip_serializing_if = "is_zero")]
    #[specta(optional, type = Number)]
    pub size: u32,
    /// Stage 22 (decision 0077): the spam filter suspects it (marked on this device) or moved
    /// it to the junk folder. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub spam: Option<crate::SpamFilterMark>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Outbox state of a message as its list row shows it (stage 4). If several operations touch
/// the message, the one that needs the user wins: `failed`, then `conflict`, then `pending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageActivity {
    /// A change waits for the server (`queued` or `in_flight`); the row already shows it.
    Pending,
    /// A change failed and was reverted locally. The activity view offers retry and discard.
    Failed,
    /// The message was changed elsewhere meanwhile and the server's state was kept. The
    /// activity view explains it.
    Conflict,
}

/// One page of a message list, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub items: Vec<MessageListItem>,
    /// Pass to the next call to continue; null on the last page.
    pub next_cursor: Option<ListCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_from_imap_atoms() {
        let flags = Flags::from_imap([
            "\\Seen",
            "\\FLAGGED",
            "\\Recent",
            "$Forwarded",
            "projekt-x",
            "$forwarded",
            "Projekt-X",
            "",
            "\\Unknown",
        ]);
        assert!(flags.seen && flags.flagged);
        assert!(!flags.answered && !flags.draft && !flags.deleted);
        assert_eq!(flags.keywords, ["$Forwarded", "projekt-x"]);
        assert!(flags.has_keyword("$FORWARDED"));
        assert!(!flags.has_keyword("$Junk"));
    }

    #[test]
    fn sort_date_trusts_the_date_header_within_a_day() {
        let internal = 1_764_000_000;
        // Normal: Date a few seconds before delivery.
        assert_eq!(sort_date(Some(internal - 30), internal), internal - 30);
        // Imported old mail: Date far before INTERNALDATE is right.
        assert_eq!(sort_date(Some(1_000_000_000), internal), 1_000_000_000);
        // Up to 24 h in the future is accepted (time zones, clock skew) ...
        assert_eq!(
            sort_date(Some(internal + DATE_HEADER_TOLERANCE), internal),
            internal + DATE_HEADER_TOLERANCE
        );
        // ... beyond that the header is not trusted.
        assert_eq!(
            sort_date(Some(internal + DATE_HEADER_TOLERANCE + 1), internal),
            internal
        );
        assert_eq!(sort_date(None, internal), internal);
        // Clamped to the range the store accepts.
        assert_eq!(sort_date(Some(-5), internal), 0);
        assert_eq!(sort_date(None, i64::MAX), MAX_SORT_DATE);
    }
}
