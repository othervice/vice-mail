//! IPC types of stage 7: search results, threads, the unified inbox, account groups, hidden
//! accounts and identities. All cross the bridge (camelCase, error-like codes snake_case).
//!
//! Positions in text (search problems, chips, highlights) are UTF-16 code units, the indices of
//! JavaScript strings ([`crate::search::Span::to_utf16`]).

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::search::{Filter, SearchErrorKind, Status, Term, TextField};
use crate::{AccountId, Identity, IdentityId, ListCursor, MailboxId, MessageListItem};

/// Largest page of [`SearchPage`], [`ThreadPage`] and the unified inbox.
pub const MAX_SEARCH_PAGE: u32 = 200;

/// Where a search looks. Hidden accounts are left out of `all` and `group` (as from the
/// unified inbox); searching one account or mailbox explicitly always works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchScope {
    /// Every visible account.
    All,
    #[serde(rename_all = "camelCase")]
    Account { account_id: AccountId },
    #[serde(rename_all = "camelCase")]
    Mailbox { mailbox_id: MailboxId },
    /// The visible accounts of a group.
    #[serde(rename_all = "camelCase")]
    Group { group_id: AccountGroupId },
}

/// One search call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// As typed, at most 2048 bytes are read.
    pub query: String,
    pub scope: SearchScope,
    /// `nextCursor` of the previous page; null for the first page.
    pub cursor: Option<ListCursor>,
    /// 1..=200.
    pub limit: u32,
    /// Search as you type: the word the input ends in matches as a prefix (`rech` finds
    /// "Rechnung"). Off for a submitted search.
    pub typing: bool,
    /// The user's region writes `01/02/2026` month first (United States). Dates with dots,
    /// ISO dates and month names do not depend on it.
    #[serde(default)]
    pub month_first: bool,
}

/// A range of UTF-16 code units (`start..end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

/// What a problem in the query is. The parser's error kinds plus what only the engine knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SearchProblemCode {
    UnclosedQuote,
    MissingValue,
    UnknownFilter,
    UnknownValue,
    InvalidDate,
    InvalidAge,
    NothingToSearch,
    PrefixTooShort,
    MissingTerm,
    MisplacedOr,
    OrNeedsText,
    OrWithExclusion,
    TooLong,
    TooManyTerms,
    /// The term contradicts another one (`is:unread is:read`); the query finds nothing.
    Conflict,
    /// `account:` names no account (address, name or id).
    UnknownAccount,
    /// `in:` names no mailbox of the accounts searched.
    UnknownMailbox,
    /// The server cannot search for this term (`has:attachment`, `is:snoozed`); it is applied
    /// to the local data of the results only (server search).
    NotOnServer,
}

impl From<SearchErrorKind> for SearchProblemCode {
    fn from(kind: SearchErrorKind) -> Self {
        match kind {
            SearchErrorKind::UnclosedQuote => Self::UnclosedQuote,
            SearchErrorKind::MissingValue => Self::MissingValue,
            SearchErrorKind::UnknownFilter => Self::UnknownFilter,
            SearchErrorKind::UnknownValue => Self::UnknownValue,
            SearchErrorKind::InvalidDate => Self::InvalidDate,
            SearchErrorKind::InvalidAge => Self::InvalidAge,
            SearchErrorKind::NothingToSearch => Self::NothingToSearch,
            SearchErrorKind::PrefixTooShort => Self::PrefixTooShort,
            SearchErrorKind::MissingTerm => Self::MissingTerm,
            SearchErrorKind::MisplacedOr => Self::MisplacedOr,
            SearchErrorKind::OrNeedsText => Self::OrNeedsText,
            SearchErrorKind::OrWithExclusion => Self::OrWithExclusion,
            SearchErrorKind::TooLong => Self::TooLong,
            SearchErrorKind::TooManyTerms => Self::TooManyTerms,
        }
    }
}

/// A problem at a place in the query; the UI marks the range and shows a text for the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct SearchProblem {
    pub code: SearchProblemCode,
    pub range: TextRange,
}

/// What a term of the query is, for chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SearchTermKind {
    Text,
    From,
    To,
    Cc,
    Subject,
    Filename,
    In,
    Account,
    HasAttachment,
    IsUnread,
    IsRead,
    IsFlagged,
    IsSnoozed,
    Before,
    After,
    OlderThan,
    NewerThan,
}

impl SearchTermKind {
    pub fn of(filter: &Filter) -> Self {
        match filter {
            Filter::Text(text) => match text.field {
                TextField::Any => Self::Text,
                TextField::From => Self::From,
                TextField::To => Self::To,
                TextField::Cc => Self::Cc,
                TextField::Subject => Self::Subject,
                TextField::Filename => Self::Filename,
            },
            Filter::In(_) => Self::In,
            Filter::Account(_) => Self::Account,
            Filter::HasAttachment => Self::HasAttachment,
            Filter::Is(Status::Unread) => Self::IsUnread,
            Filter::Is(Status::Read) => Self::IsRead,
            Filter::Is(Status::Flagged) => Self::IsFlagged,
            Filter::Is(Status::Snoozed) => Self::IsSnoozed,
            Filter::Before(_) => Self::Before,
            Filter::After(_) => Self::After,
            Filter::OlderThan(_) => Self::OlderThan,
            Filter::NewerThan(_) => Self::NewerThan,
        }
    }
}

/// One term as typed, for a chip: kind, whether it was excluded with `-`, the whole term and
/// its value (without key and quotes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchTermInfo {
    pub kind: SearchTermKind,
    pub negated: bool,
    pub range: TextRange,
    pub value_range: TextRange,
    /// Part of an `OR` group (`a OR b`).
    pub alternative: bool,
}

impl SearchTermInfo {
    pub fn from_term(term: &Term, input: &str, alternative: bool) -> Self {
        Self {
            kind: SearchTermKind::of(&term.filter),
            negated: term.negated,
            range: utf16_range(term.span, input),
            value_range: utf16_range(term.value_span, input),
            alternative,
        }
    }
}

/// `span` of `input` in UTF-16 code units.
pub fn utf16_range(span: crate::search::Span, input: &str) -> TextRange {
    let (start, end) = span.to_utf16(input);
    TextRange {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    }
}

/// Where the query's words occur in the fields the list shows. Computed from the stored text
/// (the index holds no text, decision 0003); a match deep in the body has no range here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHighlights {
    pub subject: Vec<TextRange>,
    /// In the sender's display name, or in the address when there is no name.
    pub from: Vec<TextRange>,
    pub preview: Vec<TextRange>,
}

/// One result: the message as a list row (one of its locations), every mailbox it lies in, and
/// the highlighted ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub message: MessageListItem,
    /// All locations, ascending; `message.mailboxId` is one of them.
    pub mailbox_ids: Vec<MailboxId>,
    pub highlights: SearchHighlights,
    /// The whole message is stored locally, so its text was searched. Otherwise only headers
    /// (and the text parts of a large message) were: "search on the server" can find more.
    pub body_local: bool,
}

/// State of the full-text index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SearchIndexState {
    /// Every stored message is indexed.
    Ready,
    /// Existing messages are being indexed (after the update to stage 7); results may be
    /// missing older messages until it is done.
    Building,
    /// A new index is being built in the background; searches use the old one meanwhile.
    Rebuilding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStatus {
    pub state: SearchIndexState,
    /// 0..=100, by message dates (the index is built from old to new).
    pub percent: u8,
}

/// One page of results, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    /// Null on the last page. A page can be shorter than the limit and still have a next one.
    pub next_cursor: Option<ListCursor>,
    /// Parser errors, contradictions and unknown accounts or mailboxes, sorted by position.
    pub problems: Vec<SearchProblem>,
    /// Every term of the query, for chips.
    pub terms: Vec<SearchTermInfo>,
    /// The query cannot match anything (a contradiction, an unknown account or mailbox, an
    /// empty date range); `hits` is empty.
    pub matches_nothing: bool,
    pub index: SearchIndexStatus,
}

/// Result of searching on the server (IMAP SEARCH over the read-only connection) for messages
/// whose content is not stored locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerSearchResult {
    /// Messages the server found whose whole body is not stored locally, newest first, at most
    /// the request's limit. Every hit is a stored message (headers are always local).
    pub hits: Vec<SearchHit>,
    /// More hits than returned.
    pub truncated: bool,
    /// Mailboxes searched.
    pub searched: Vec<MailboxId>,
    /// Mailboxes whose search failed (offline, refused); the rest still counts.
    pub failed: Vec<MailboxId>,
    /// Server hits not stored locally yet (first sync still running).
    #[specta(type = Number)]
    pub not_synced: u32,
    /// Problems of the query, including terms the server cannot search (`notOnServer`).
    pub problems: Vec<SearchProblem>,
}

// ---------------------------------------------------------------------------------------------
// Threads

/// One conversation in a list: the newest of its messages in the listed mailbox, how many it
/// has (in all mailboxes of the account) and how many of those are unread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListItem {
    /// The newest message of the thread in this mailbox; the row shows it.
    pub message: MessageListItem,
    /// Local thread id; null while the message is not threaded yet (right after the update to
    /// stage 7). Changes when threads merge: open a thread by `message.id` (`getThread`).
    #[specta(type = Option<Number>)]
    pub thread_id: Option<i64>,
    #[specta(type = Number)]
    pub message_count: u32,
    /// Messages of the thread unread in at least one mailbox.
    #[specta(type = Number)]
    pub unread_count: u32,
    /// Sort date of the thread's newest message anywhere (Unix milliseconds), for example a
    /// reply in Sent.
    #[specta(type = Number)]
    pub last_activity: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPage {
    pub items: Vec<ThreadListItem>,
    /// Null on the last page.
    pub next_cursor: Option<ListCursor>,
}

/// One message of an open thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEntry {
    /// As a list row of one of its locations (the requested mailbox if it lies there).
    pub message: MessageListItem,
    /// All locations, ascending.
    pub mailbox_ids: Vec<MailboxId>,
}

/// Every message of a thread over all mailboxes of its account, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    #[specta(type = Option<Number>)]
    pub thread_id: Option<i64>,
    pub messages: Vec<ThreadEntry>,
    /// Messages in the thread; more than `messages.length` if the thread was cut to the newest
    /// [`MAX_THREAD_MESSAGES`].
    #[specta(type = Number)]
    pub total: u32,
}

/// Most messages [`ThreadView`] returns (the newest ones).
pub const MAX_THREAD_MESSAGES: u32 = 1000;

// ---------------------------------------------------------------------------------------------
// Unified inbox, account groups, hidden accounts

/// Stable id of an account group (UUID v4, lowercase hyphenated); synced like accounts.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct AccountGroupId(pub String);

impl std::fmt::Display for AccountGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Which inboxes a unified view mixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UnifiedView {
    /// The inboxes of every visible account.
    All,
    /// The inboxes of the group's visible accounts.
    #[serde(rename_all = "camelCase")]
    Group { group_id: AccountGroupId },
}

/// A user-defined group of accounts ("Arbeit", "Privat").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountGroup {
    pub id: AccountGroupId,
    pub name: String,
    /// Members in account order (oldest account first).
    pub account_ids: Vec<AccountId>,
    /// Order in the sidebar, ascending.
    #[specta(type = Number)]
    pub position: i64,
}

/// Longest group name in characters.
pub const MAX_GROUP_NAME_CHARS: usize = 100;

// ---------------------------------------------------------------------------------------------
// Identities

/// A sender identity of an account over IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    pub id: IdentityId,
    pub account_id: AccountId,
    pub email: String,
    pub name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
}

impl From<Identity> for IdentityInfo {
    fn from(identity: Identity) -> Self {
        Self {
            id: identity.id,
            account_id: identity.account_id,
            email: identity.email,
            name: identity.name,
            reply_to: identity.reply_to,
            is_default: identity.is_default,
        }
    }
}

/// Fields of a new or changed identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInput {
    pub email: String,
    pub name: String,
    pub reply_to: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format() {
        let scope: SearchScope =
            serde_json::from_value(json!({ "kind": "mailbox", "mailboxId": 7 })).expect("scope");
        assert_eq!(
            scope,
            SearchScope::Mailbox {
                mailbox_id: MailboxId(7)
            }
        );
        let view: UnifiedView =
            serde_json::from_value(json!({ "kind": "group", "groupId": "g-1" })).expect("view");
        assert_eq!(
            view,
            UnifiedView::Group {
                group_id: AccountGroupId("g-1".into())
            }
        );
        assert_eq!(
            serde_json::to_value(SearchProblem {
                code: SearchProblemCode::UnknownAccount,
                range: TextRange { start: 0, end: 4 },
            })
            .expect("problem"),
            json!({ "code": "unknown_account", "range": { "start": 0, "end": 4 } })
        );
        assert_eq!(
            serde_json::to_value(SearchIndexStatus {
                state: SearchIndexState::Rebuilding,
                percent: 40
            })
            .expect("status"),
            json!({ "state": "rebuilding", "percent": 40 })
        );
    }
}
