//! [`MailAccess`]: what the host needs from the app. `AppCore` implements it later; the tests
//! use a fake. The host checks permissions before and after every call (an account that is not
//! visible to the client is never asked for and never returned); implementations still get the
//! visible accounts and must stay inside them.
//!
//! Rules for implementations:
//! - Only reading, except [`MailAccess::create_draft`]. Reading never changes the server:
//!   no `\Seen` (fetch with `BODY.PEEK`), no "mark read on open", no moves.
//! - Bodies are handed over as the parser produced them ([`BodyParts`]); the host turns HTML
//!   into text itself, so no raw HTML ever leaves the app.
//! - Errors are the engine's [`CoreError`]; the host shows the client only the error code,
//!   never the message (it may contain server responses or paths).

use std::future::Future;

use mail_model::{
    Account, AccountId, Address, CoreError, ListCursor, Mailbox, MailboxId, MessageId,
    MessageListItem, SearchProblemCode,
};

/// The app side of the MCP tools.
pub trait MailAccess: Send + Sync + 'static {
    /// Every account of the app; the host keeps only the visible ones.
    fn accounts(&self) -> impl Future<Output = Result<Vec<Account>, CoreError>> + Send;

    /// The mailboxes of one visible account.
    fn mailboxes(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Mailbox>, CoreError>> + Send;

    /// A submitted search (not search-as-you-type) in exactly `query.accounts`.
    fn search(
        &self,
        query: SearchQuery,
    ) -> impl Future<Output = Result<SearchResults, CoreError>> + Send;

    /// Unread or flagged messages, newest first, in exactly `query.accounts`.
    fn list_messages(
        &self,
        query: ListQuery,
    ) -> impl Future<Output = Result<SearchResults, CoreError>> + Send;

    /// One message with its body, or `None` if it does not exist, lies outside `accounts`, or
    /// lies only in Trash or Junk and not in `named` ([`readable_in`], decision 0045): the
    /// client cannot tell such a message from a missing one.
    fn message(
        &self,
        id: MessageId,
        accounts: Vec<AccountId>,
        named: Option<MailboxId>,
    ) -> impl Future<Output = Result<Option<MessageContent>, CoreError>> + Send;

    /// The thread of a message: its newest `newest` messages, oldest first, or `None` when
    /// [`MailAccess::message`] would give `None`. Members that lie only in Trash or Junk and
    /// not in `named` are left out of the messages and of the total.
    fn thread(
        &self,
        id: MessageId,
        newest: u32,
        accounts: Vec<AccountId>,
        named: Option<MailboxId>,
    ) -> impl Future<Output = Result<Option<ThreadContent>, CoreError>> + Send;

    /// Saves a draft in the account's Drafts mailbox through the outbox (it appears in the app
    /// like any draft). Never sends. The host has checked that the account allows drafts.
    fn create_draft(
        &self,
        draft: NewDraft,
    ) -> impl Future<Output = Result<DraftCreated, CoreError>> + Send;
}

/// Whether the tools may read a message that lies in `locations` (decision 0045): somewhere
/// outside Trash and Junk (`trash_or_junk`, the account's mailboxes with these roles), or in
/// the mailbox the call names. A message only in Trash or Junk is read only when the call names
/// that mailbox, as the searches leave both out unless the call names them.
pub fn readable_in(
    locations: &[MailboxId],
    trash_or_junk: &[MailboxId],
    named: Option<MailboxId>,
) -> bool {
    locations
        .iter()
        .any(|mailbox| !trash_or_junk.contains(mailbox) || Some(*mailbox) == named)
}

/// [`MailAccess::search`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// The search language of stage 7 (`mail_model::search`); never contains `account:`
    /// (the host refuses such queries; the accounts are below).
    pub query: String,
    /// Non-empty, all visible.
    pub accounts: Vec<AccountId>,
    /// A mailbox of one of `accounts`.
    pub mailbox: Option<MailboxId>,
    /// 1..=[`crate::limits::MAX_PAGE`].
    pub limit: u32,
    pub cursor: Option<ListCursor>,
}

/// Which messages [`MailAccess::list_messages`] lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListFilter {
    Unread,
    Flagged,
}

/// [`MailAccess::list_messages`]. Without a mailbox: every mailbox of the accounts except
/// Trash and Junk (as `is:unread` in the unified search).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery {
    pub filter: ListFilter,
    pub accounts: Vec<AccountId>,
    pub mailbox: Option<MailboxId>,
    pub limit: u32,
    pub cursor: Option<ListCursor>,
}

/// One page of messages, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResults {
    pub hits: Vec<MessageListItem>,
    pub next_cursor: Option<ListCursor>,
    /// Problems of the query (unknown mailbox, invalid date, ...).
    pub problems: Vec<SearchProblemCode>,
}

/// A message with its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageContent {
    pub id: MessageId,
    pub account_id: AccountId,
    /// Every mailbox it lies in.
    pub mailbox_ids: Vec<MailboxId>,
    /// Sort date, Unix milliseconds.
    pub date: i64,
    pub subject: String,
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub unread: bool,
    pub flagged: bool,
    pub answered: bool,
    /// `None` when the body is not stored locally and could not be fetched.
    pub body: Option<BodyParts>,
}

/// The parsed body, as `mail_content::ParsedMessage` has it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyParts {
    /// The text/plain body.
    pub text: Option<String>,
    /// The HTML body, unsanitised. The host only ever passes it through
    /// `mail_content::html_to_visible_text`.
    pub html: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
}

impl BodyParts {
    /// Text, HTML and attachment metadata of a parsed message.
    pub fn from_parsed(parsed: &mail_content::ParsedMessage) -> Self {
        Self {
            text: parsed.text.clone(),
            html: parsed.html.clone(),
            attachments: parsed
                .attachments
                .iter()
                .map(|part| AttachmentMeta {
                    name: part.file_name.clone(),
                    mime_type: part.mime.clone(),
                    size: part.size,
                })
                .collect(),
        }
    }
}

/// What a client learns about an attachment: never its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMeta {
    /// Sanitised file name (`mail_content::sanitize_filename`).
    pub name: String,
    pub mime_type: String,
    /// Bytes.
    pub size: u64,
}

/// [`MailAccess::thread`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadContent {
    /// Oldest first; the newest ones if the thread is longer.
    pub messages: Vec<MessageContent>,
    /// Messages in the whole thread.
    pub total: u32,
}

/// [`MailAccess::create_draft`]. Checked by the host: the account allows drafts, addresses are
/// normalised (`mail_content::normalize_email`), lengths are within [`crate::limits`], and a
/// replied-to message is visible, of the same account and not only in Trash or Junk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDraft {
    pub account_id: AccountId,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    /// Plain text.
    pub body: String,
    /// Reply to this message: `In-Reply-To`/`References` from it, and its reply recipients
    /// when `to` is empty.
    pub in_reply_to: Option<MessageId>,
}

/// Result of [`MailAccess::create_draft`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftCreated {
    /// The local id once the draft is stored, if already known.
    pub message_id: Option<MessageId>,
    /// The draft stays in the app on this device: the user has not allowed changes on the
    /// server for this account (or it has no Drafts folder). Otherwise it goes to the Drafts
    /// folder.
    pub local_only: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_only_in_trash_or_junk_needs_its_mailbox_named() {
        let (inbox, trash, junk) = (MailboxId(1), MailboxId(2), MailboxId(3));
        let hidden = [trash, junk];
        assert!(readable_in(&[inbox], &hidden, None));
        assert!(
            readable_in(&[inbox, trash], &hidden, None),
            "also elsewhere"
        );
        assert!(!readable_in(&[trash], &hidden, None));
        assert!(!readable_in(&[trash, junk], &hidden, None));
        assert!(readable_in(&[trash], &hidden, Some(trash)));
        assert!(!readable_in(&[trash], &hidden, Some(junk)), "another one");
        assert!(!readable_in(&[trash], &hidden, Some(inbox)));
        assert!(readable_in(&[trash, junk], &hidden, Some(junk)));
        assert!(!readable_in(&[], &hidden, Some(inbox)), "no location");
    }
}
