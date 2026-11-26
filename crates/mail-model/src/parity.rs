//! Stage 16 (decision 0033): the reading list, unread lists, mailbox favourites and colours,
//! blocked senders, and moving or copying messages between accounts.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{ListCursor, MessageListItem};

/// Most rows of the reading list view (as the other mark views).
pub const MAX_READING_LIST_VIEW: u32 = 1_000;

/// Most blocked senders.
pub const MAX_BLOCKED_SENDERS: usize = 10_000;

/// Most favourite mailboxes over all accounts.
pub const MAX_FAVORITE_MAILBOXES: usize = 200;

/// Most list views that group unread messages on top, and the longest view key.
pub const MAX_UNREAD_FIRST_VIEWS: usize = 200;
pub const MAX_VIEW_KEY_CHARS: usize = 200;

/// A sender whose new mail goes to the junk folder (or is hidden on read-only accounts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BlockedSender {
    /// Normalised address (lower case, IDNA domain).
    pub address: String,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub created_at: i64,
}

/// Which part of an "unread first" list a page or cursor is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum UnreadPhase {
    /// The unread messages, newest first.
    Unread,
    /// Then the read ones, newest first.
    Read,
}

/// Where an "unread first" list continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadFirstCursor {
    pub phase: UnreadPhase,
    /// Within the phase: messages with a smaller id. Null: the phase starts from the top.
    pub before: Option<ListCursor>,
}

/// One page of an "unread first" list: the unread rows come before the read ones (each row's
/// `unread` tells where the group ends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadFirstPage {
    pub items: Vec<MessageListItem>,
    /// Null on the last page.
    pub next_cursor: Option<UnreadFirstCursor>,
}

/// Decision 0039: "unread first" as conversations: the conversations with an unread message in
/// the listed mailboxes, then the others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadFirstThreadPage {
    pub items: Vec<crate::ThreadListItem>,
    /// Null on the last page.
    pub next_cursor: Option<UnreadFirstCursor>,
}

/// Copy or move to a mailbox of another account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TransferMode {
    /// The original stays.
    Copy,
    /// The original is removed from the source once the copy is confirmed in the target.
    Move,
}

/// Stage 22 (decision 0081): what a drop of messages on a folder recorded (`dropMessages`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DropResult {
    /// The account of the folder they were dropped on.
    pub account_id: crate::AccountId,
    /// Messages of that account moved there at once (an ordinary move).
    pub moved: u32,
    /// Messages copied there, or moved there from other accounts: copy first, the source
    /// removed only once the copy arrived (decision 0033).
    pub transferred: u32,
    /// Messages already in the folder, left as they were.
    pub skipped: u32,
    /// The batches recorded, newest last, for "Undo" and the failure notices. A move within the
    /// account undoes like any move; a transfer only while nothing of it reached the server.
    pub batches: Vec<crate::BatchId>,
}
