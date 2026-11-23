//! Running one tool call against [`MailAccess`] for one [`Grant`]: argument checks, permission
//! checks before and after the call, cleaning and limits, and the untrusted data block.

use mail_model::search::{Filter, FixedOffset, SearchContext, SlashDateOrder};
use mail_model::{
    AccountId, Address, CoreError, ListCursor, Mailbox, MailboxId, MailboxRole, MessageListItem,
    SearchProblemCode,
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::access::{
    ListFilter, ListQuery, MailAccess, MessageContent, NewDraft, SearchQuery, SearchResults,
};
use crate::limits;
use crate::output::{Cleaned, OutputError, clean_line, clean_text, iso_utc, untrusted};
use crate::permissions::Grant;
use crate::tools::{Args, ToolError, ToolName, required};

/// The answer to one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub text: String,
    pub is_error: bool,
    /// Messages whose data the result contains (for the access log).
    pub messages: u32,
}

struct Done {
    text: String,
    messages: u32,
}

enum Failure {
    Tool(ToolError),
    Core(CoreError),
    Output(OutputError),
}

impl From<ToolError> for Failure {
    fn from(err: ToolError) -> Self {
        Self::Tool(err)
    }
}

impl From<CoreError> for Failure {
    fn from(err: CoreError) -> Self {
        Self::Core(err)
    }
}

impl From<OutputError> for Failure {
    fn from(err: OutputError) -> Self {
        Self::Output(err)
    }
}

const NO_ACCOUNTS: &str = "No account is shared with this client. The user can share accounts \
in the mail app's settings under AI access.";
const NOT_SHARED: &str = "No shared account has this id; use an account_id from list_accounts.";
const NO_MESSAGE: &str = "No message with this id in the shared accounts.";

/// Runs `tool`. Never panics; every failure is a result with `is_error`.
pub(crate) async fn execute<A: MailAccess>(
    access: &A,
    grant: &Grant,
    tool: ToolName,
    arguments: Map<String, Value>,
) -> Outcome {
    let result = match Args::new(tool, arguments) {
        Ok(args) => match tool {
            ToolName::ListAccounts => list_accounts(access, grant).await,
            ToolName::ListMailboxes => list_mailboxes(access, grant, &args).await,
            ToolName::SearchMessages => search_messages(access, grant, &args).await,
            ToolName::ListMessages => list_messages(access, grant, &args).await,
            ToolName::ReadMessage => read_message(access, grant, &args).await,
            ToolName::ReadThread => read_thread(access, grant, &args).await,
            ToolName::CreateDraft => create_draft(access, grant, &args).await,
        },
        Err(err) => Err(Failure::Tool(err)),
    };
    match result {
        Ok(done) => Outcome {
            text: done.text,
            is_error: false,
            messages: done.messages,
        },
        Err(Failure::Tool(ToolError(text))) => Outcome {
            text,
            is_error: true,
            messages: 0,
        },
        Err(Failure::Core(err)) => {
            tracing::warn!(%tool, error = %err, "MCP tool call failed in the app");
            let code = serde_json::to_value(err.code())
                .ok()
                .and_then(|code| code.as_str().map(str::to_owned))
                .unwrap_or_else(|| "internal".to_owned());
            Outcome {
                text: format!("The mail app could not do this (error code: {code})."),
                is_error: true,
                messages: 0,
            }
        }
        Err(Failure::Output(err)) => {
            tracing::error!(%tool, error = %err, "MCP tool result could not be built");
            Outcome {
                text: format!("The result could not be built: {err}."),
                is_error: true,
                messages: 0,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Output types: what a client sees. Field names are snake_case like the tool arguments.

#[derive(Serialize)]
struct AccountOut {
    account_id: String,
    name: String,
    email: String,
    drafts_allowed: bool,
}

#[derive(Serialize)]
struct MailboxOut {
    mailbox_id: i64,
    path: String,
    role: &'static str,
    unread: u32,
    total: u32,
}

#[derive(Serialize)]
struct AddressOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    email: String,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Serialize)]
struct HitOut {
    message_id: i64,
    account_id: String,
    mailbox_id: i64,
    date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<AddressOut>,
    to: Vec<AddressOut>,
    #[serde(skip_serializing_if = "is_zero")]
    to_more: usize,
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
    unread: bool,
    flagged: bool,
    answered: bool,
    has_attachments: bool,
}

#[derive(Serialize)]
struct PageOut {
    messages: Vec<HitOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    problems: Vec<SearchProblemCode>,
}

#[derive(Serialize)]
struct AttachmentOut {
    name: String,
    #[serde(rename = "type")]
    mime_type: String,
    size: u64,
}

#[derive(Serialize)]
struct MessageOut {
    message_id: i64,
    account_id: String,
    mailbox_ids: Vec<i64>,
    date: String,
    subject: String,
    from: Vec<AddressOut>,
    to: Vec<AddressOut>,
    #[serde(skip_serializing_if = "is_zero")]
    to_more: usize,
    cc: Vec<AddressOut>,
    #[serde(skip_serializing_if = "is_zero")]
    cc_more: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reply_to: Vec<AddressOut>,
    unread: bool,
    flagged: bool,
    answered: bool,
    attachments: Vec<AttachmentOut>,
    #[serde(skip_serializing_if = "is_zero")]
    attachments_more: usize,
    /// `text`, `html_as_text`, `empty`, or `unavailable` (not stored locally, not fetched).
    body_format: &'static str,
    body_chars: usize,
    body_truncated: bool,
    /// Invisible characters (tag characters, bidi overrides, zero-width spaces) removed from
    /// the subject and body.
    #[serde(skip_serializing_if = "is_zero")]
    hidden_characters_removed: usize,
    body: String,
}

#[derive(Serialize)]
struct MessageResult {
    message: MessageOut,
}

#[derive(Serialize)]
struct AccountsOut {
    accounts: Vec<AccountOut>,
}

#[derive(Serialize)]
struct MailboxesOut {
    mailboxes: Vec<MailboxOut>,
}

#[derive(Serialize)]
struct ThreadOut {
    total: u32,
    #[serde(skip_serializing_if = "is_zero")]
    older_not_shown: usize,
    messages: Vec<MessageOut>,
}

fn address(address: &Address) -> AddressOut {
    AddressOut {
        name: address
            .name
            .as_deref()
            .map(|name| clean_line(name, limits::LINE_CHARS))
            .filter(|name| !name.is_empty()),
        email: clean_line(&address.email, limits::LINE_CHARS),
    }
}

/// At most `max` addresses, and how many were left out.
fn addresses(list: &[Address], max: usize) -> (Vec<AddressOut>, usize) {
    (
        list.iter().take(max).map(address).collect(),
        list.len().saturating_sub(max),
    )
}

fn role_name(role: MailboxRole) -> &'static str {
    role.as_db().unwrap_or("custom")
}

fn hit(item: &MessageListItem) -> HitOut {
    let (to, to_more) = addresses(&item.to, 5);
    HitOut {
        message_id: item.id.0,
        account_id: item.account_id.to_string(),
        mailbox_id: item.mailbox_id.0,
        date: iso_utc(item.date),
        from: item.from.as_ref().map(address),
        to,
        to_more,
        subject: clean_line(&item.subject, limits::LINE_CHARS),
        preview: item
            .preview
            .as_deref()
            .map(|preview| clean_line(preview, limits::PREVIEW_CHARS)),
        unread: item.unread,
        flagged: item.flagged,
        answered: item.answered,
        has_attachments: item.has_attachments,
    }
}

/// The body as text: HTML (what the reading pane shows first) through `html_to_text`, else the
/// text part. Never HTML.
fn body_text(message: &MessageContent, max_chars: usize) -> (&'static str, Cleaned) {
    let Some(parts) = &message.body else {
        return ("unavailable", Cleaned::default());
    };
    if let Some(html) = parts.html.as_deref().filter(|h| !h.trim().is_empty()) {
        let text = mail_content::html_to_text(html);
        return ("html_as_text", clean_text(&text, max_chars));
    }
    match parts.text.as_deref().filter(|t| !t.trim().is_empty()) {
        Some(text) => ("text", clean_text(text, max_chars)),
        None => ("empty", Cleaned::default()),
    }
}

fn message_out(message: &MessageContent, max_chars: usize) -> MessageOut {
    let (body_format, body) = body_text(message, max_chars);
    let (to, to_more) = addresses(&message.to, limits::ADDRESSES_PER_FIELD);
    let (cc, cc_more) = addresses(&message.cc, limits::ADDRESSES_PER_FIELD);
    let (reply_to, _) = addresses(&message.reply_to, limits::ADDRESSES_PER_FIELD);
    let (from, _) = addresses(&message.from, limits::ADDRESSES_PER_FIELD);
    let attachments = message
        .body
        .as_ref()
        .map(|parts| parts.attachments.as_slice())
        .unwrap_or_default();
    let subject_hidden = message
        .subject
        .chars()
        .filter(|c| crate::output::is_hidden_char(*c))
        .count();
    MessageOut {
        message_id: message.id.0,
        account_id: message.account_id.to_string(),
        mailbox_ids: message.mailbox_ids.iter().map(|id| id.0).collect(),
        date: iso_utc(message.date),
        subject: clean_line(&message.subject, limits::LINE_CHARS),
        from,
        to,
        to_more,
        cc,
        cc_more,
        reply_to,
        unread: message.unread,
        flagged: message.flagged,
        answered: message.answered,
        attachments: attachments
            .iter()
            .take(limits::ATTACHMENTS_PER_MESSAGE)
            .map(|part| AttachmentOut {
                name: clean_line(&part.name, limits::LINE_CHARS),
                mime_type: clean_line(&part.mime_type, 100),
                size: part.size,
            })
            .collect(),
        attachments_more: attachments
            .len()
            .saturating_sub(limits::ATTACHMENTS_PER_MESSAGE),
        body_format,
        body_chars: body.chars,
        body_truncated: body.truncated,
        hidden_characters_removed: body.hidden_removed + subject_hidden,
        body: body.text,
    }
}

// ---------------------------------------------------------------------------------------------
// Permission helpers

fn ensure_visible(grant: &Grant, account: &AccountId) -> Result<(), ToolError> {
    if grant.can_read(account) {
        Ok(())
    } else {
        Err(ToolError::new(NOT_SHARED))
    }
}

fn visible_or_error(grant: &Grant) -> Result<Vec<AccountId>, ToolError> {
    let visible = grant.visible();
    if visible.is_empty() {
        Err(ToolError::new(NO_ACCOUNTS))
    } else {
        Ok(visible)
    }
}

/// The accounts and mailbox a search or list covers: one shared account, the account of a
/// shared mailbox, or every shared account.
async fn scope<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<(Vec<AccountId>, Option<MailboxId>), Failure> {
    let account = args.account_id("account_id")?;
    let mailbox = args.mailbox_id("mailbox_id")?;
    let accounts = match account {
        Some(account) => {
            ensure_visible(grant, &account)?;
            vec![account]
        }
        None => visible_or_error(grant)?,
    };
    let Some(mailbox) = mailbox else {
        return Ok((accounts, None));
    };
    for account in &accounts {
        let mailboxes = access.mailboxes(*account).await?;
        if mailboxes
            .iter()
            .any(|m| m.id == mailbox && m.account_id == *account)
        {
            return Ok((vec![*account], Some(mailbox)));
        }
    }
    Err(ToolError::new(
        "No mailbox with this id in the shared accounts; use a mailbox_id from list_mailboxes.",
    )
    .into())
}

fn page(grant: &Grant, results: SearchResults, limit: u32) -> Result<Done, Failure> {
    let hits: Vec<HitOut> = results
        .hits
        .iter()
        .filter(|item| grant.can_read(&item.account_id))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(hit)
        .collect();
    let count = u32::try_from(hits.len()).unwrap_or(u32::MAX);
    let next_cursor = results.next_cursor.map(|cursor| cursor.0);
    let note = match (count, next_cursor) {
        (0, _) => Some("No messages found.".to_owned()),
        (_, Some(cursor)) => Some(format!("More messages: call again with cursor {cursor}.")),
        _ => None,
    };
    let data = PageOut {
        messages: hits,
        next_cursor,
        // An unknown account can only come from `account:`, which is refused before; never
        // pass on anything that could tell apart hidden and missing accounts.
        problems: results
            .problems
            .into_iter()
            .filter(|code| *code != SearchProblemCode::UnknownAccount)
            .collect(),
    };
    Ok(Done {
        text: untrusted(&data, note.as_deref())?,
        messages: count,
    })
}

fn paging(args: &Args) -> Result<(u32, Option<ListCursor>), ToolError> {
    let limit = args
        .integer("limit", 1, i64::from(limits::MAX_PAGE))?
        .and_then(|limit| u32::try_from(limit).ok())
        .unwrap_or(limits::DEFAULT_PAGE);
    let cursor = args.integer("cursor", 0, i64::MAX)?.map(ListCursor);
    Ok((limit, cursor))
}

// ---------------------------------------------------------------------------------------------
// Tools

async fn list_accounts<A: MailAccess>(access: &A, grant: &Grant) -> Result<Done, Failure> {
    let accounts: Vec<AccountOut> = access
        .accounts()
        .await?
        .iter()
        .filter(|account| grant.can_read(&account.id))
        .map(|account| AccountOut {
            account_id: account.id.to_string(),
            name: clean_line(&account.display_name, limits::LINE_CHARS),
            email: clean_line(&account.email, limits::LINE_CHARS),
            drafts_allowed: grant.can_draft(&account.id),
        })
        .collect();
    let note = accounts.is_empty().then_some(NO_ACCOUNTS);
    Ok(Done {
        text: untrusted(&AccountsOut { accounts }, note)?,
        messages: 0,
    })
}

async fn list_mailboxes<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<Done, Failure> {
    let account = required(args.account_id("account_id")?, "account_id")?;
    ensure_visible(grant, &account)?;
    let mailboxes: Vec<Mailbox> = access
        .mailboxes(account)
        .await?
        .into_iter()
        .filter(|m| m.selectable && m.account_id == account)
        .collect();
    let omitted = mailboxes.len().saturating_sub(limits::MAX_MAILBOXES);
    let shown: Vec<MailboxOut> = mailboxes
        .iter()
        .take(limits::MAX_MAILBOXES)
        .map(|m| MailboxOut {
            mailbox_id: m.id.0,
            path: clean_line(&m.path, limits::LINE_CHARS),
            role: role_name(m.role),
            unread: m.unread,
            total: m.total,
        })
        .collect();
    let note = (omitted > 0).then(|| format!("{omitted} more mailboxes are not listed."));
    Ok(Done {
        text: untrusted(&MailboxesOut { mailboxes: shown }, note.as_deref())?,
        messages: 0,
    })
}

/// Refuses `account:` terms: accounts are chosen with `account_id`, so a query can never name
/// (or probe for) an account that is not shared.
fn check_query(query: &str) -> Result<(), ToolError> {
    if query.len() > limits::MAX_QUERY_BYTES {
        return Err(ToolError::new(format!(
            "query is longer than {} bytes.",
            limits::MAX_QUERY_BYTES
        )));
    }
    if query.trim().is_empty() {
        return Err(ToolError::new("query is empty."));
    }
    let utc = FixedOffset(0);
    let context = SearchContext {
        now: 0,
        time_zone: &utc,
        slash_dates: SlashDateOrder::DayFirst,
    };
    let parsed = mail_model::search::parse(query, &context);
    if parsed
        .terms()
        .any(|term| matches!(term.filter, Filter::Account(_)))
    {
        return Err(ToolError::new(
            "account: is not supported here; pass account_id (from list_accounts) instead.",
        ));
    }
    Ok(())
}

async fn search_messages<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<Done, Failure> {
    let query = required(args.string("query", limits::MAX_QUERY_BYTES)?, "query")?;
    check_query(&query)?;
    let (limit, cursor) = paging(args)?;
    let (accounts, mailbox) = scope(access, grant, args).await?;
    let results = access
        .search(SearchQuery {
            query,
            accounts,
            mailbox,
            limit,
            cursor,
        })
        .await?;
    page(grant, results, limit)
}

async fn list_messages<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<Done, Failure> {
    let filter = match required(args.string("filter", 16)?, "filter")?.as_str() {
        "unread" => ListFilter::Unread,
        "flagged" => ListFilter::Flagged,
        _ => return Err(ToolError::new("filter must be \"unread\" or \"flagged\".").into()),
    };
    let (limit, cursor) = paging(args)?;
    let (accounts, mailbox) = scope(access, grant, args).await?;
    let results = access
        .list_messages(ListQuery {
            filter,
            accounts,
            mailbox,
            limit,
            cursor,
        })
        .await?;
    page(grant, results, limit)
}

async fn read_message<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<Done, Failure> {
    let id = required(args.message_id("message_id")?, "message_id")?;
    let visible = visible_or_error(grant)?;
    let message = access
        .message(id, visible)
        .await?
        .filter(|message| grant.can_read(&message.account_id))
        .ok_or_else(|| ToolError::new(NO_MESSAGE))?;
    let data = MessageResult {
        message: message_out(&message, limits::MESSAGE_BODY_CHARS),
    };
    Ok(Done {
        text: untrusted(&data, None)?,
        messages: 1,
    })
}

async fn read_thread<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<Done, Failure> {
    let id = required(args.message_id("message_id")?, "message_id")?;
    let visible = visible_or_error(grant)?;
    let thread = access
        .thread(id, limits::THREAD_MESSAGES, visible)
        .await?
        .ok_or_else(|| ToolError::new(NO_MESSAGE))?;
    let messages: Vec<&MessageContent> = thread
        .messages
        .iter()
        .filter(|message| grant.can_read(&message.account_id))
        .collect();
    let keep = usize::try_from(limits::THREAD_MESSAGES).unwrap_or(usize::MAX);
    let messages = &messages[messages.len().saturating_sub(keep)..];
    if messages.is_empty() {
        return Err(ToolError::new(NO_MESSAGE).into());
    }
    // Newest first get the body budget; older ones get what is left.
    let mut budget = limits::THREAD_TOTAL_BODY_CHARS;
    let mut out: Vec<MessageOut> = messages
        .iter()
        .rev()
        .map(|message| {
            let message = message_out(message, budget.min(limits::THREAD_BODY_CHARS));
            budget = budget.saturating_sub(message.body.chars().count());
            message
        })
        .collect();
    out.reverse();
    let total = thread
        .total
        .max(u32::try_from(out.len()).unwrap_or(u32::MAX));
    let count = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let data = ThreadOut {
        total,
        older_not_shown: usize::try_from(total.saturating_sub(count)).unwrap_or(0),
        messages: out,
    };
    Ok(Done {
        text: untrusted(&data, None)?,
        messages: count,
    })
}

async fn create_draft<A: MailAccess>(
    access: &A,
    grant: &Grant,
    args: &Args,
) -> Result<Done, Failure> {
    let account = required(args.account_id("account_id")?, "account_id")?;
    ensure_visible(grant, &account)?;
    if !grant.can_draft(&account) {
        return Err(
            ToolError::new("The user allows only reading in this account, not drafts.").into(),
        );
    }
    let normalize = |list: Vec<String>| -> Result<Vec<String>, ToolError> {
        list.iter()
            .map(|address| {
                mail_content::normalize_email(address).map_err(|_| {
                    ToolError::new(format!(
                        "\"{}\" is not a valid e-mail address.",
                        clean_line(address, 100)
                    ))
                })
            })
            .collect()
    };
    let to = normalize(args.strings("to", limits::DRAFT_RECIPIENTS, 320)?)?;
    let cc = normalize(args.strings("cc", limits::DRAFT_RECIPIENTS, 320)?)?;
    if to.len() + cc.len() > limits::DRAFT_RECIPIENTS {
        return Err(ToolError::new(format!(
            "A draft may have at most {} recipients.",
            limits::DRAFT_RECIPIENTS
        ))
        .into());
    }
    // One line (no header injection), no hidden characters.
    let subject = clean_line(
        &args
            .string("subject", limits::DRAFT_SUBJECT_CHARS)?
            .unwrap_or_default(),
        limits::DRAFT_SUBJECT_CHARS,
    );
    let body = clean_text(
        &args
            .string("body", limits::DRAFT_BODY_CHARS)?
            .unwrap_or_default(),
        limits::DRAFT_BODY_CHARS,
    )
    .text;
    let in_reply_to = args.message_id("in_reply_to")?;
    if let Some(id) = in_reply_to {
        access
            .message(id, vec![account])
            .await?
            .filter(|message| message.account_id == account)
            .ok_or_else(|| ToolError::new("in_reply_to is not a message of this account."))?;
    }
    if to.is_empty()
        && cc.is_empty()
        && in_reply_to.is_none()
        && subject.is_empty()
        && body.trim().is_empty()
    {
        return Err(ToolError::new("The draft is empty.").into());
    }
    access
        .create_draft(NewDraft {
            account_id: account,
            to,
            cc,
            subject,
            body,
            in_reply_to,
        })
        .await?;
    Ok(Done {
        text: format!(
            "Draft saved in the Drafts folder of account {account}. It has not been sent; the \
             user can review, change and send it in the mail app."
        ),
        messages: 1,
    })
}
