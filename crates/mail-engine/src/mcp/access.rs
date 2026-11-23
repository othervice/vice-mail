//! [`MailAccess`] over [`AppCore`]: what the MCP tools read and the one thing they write.
//!
//! - **Only reading**, except `create_draft`. Bodies come from the local store or are fetched
//!   over the account's read-only reading connection (`BODY.PEEK`); nothing is marked read, the
//!   core never marks on open (the UI does, after its own setting).
//! - **Exactly the given accounts.** Search and lists run in the accounts the host passes (the
//!   shared ones), never in "all" (the app's hidden accounts are a view setting, not a
//!   permission). Without a mailbox, Trash and Junk are left out.
//! - **Bodies as parsed** ([`BodyParts`]): text, HTML and the reading pane's attachment list.
//!   The host turns HTML into the text a reader sees (`mail_content::html_to_visible_text`,
//!   HTML first like the reading pane) and counts the hidden text it left out.
//! - **Drafts** through the drafts of stage 5: a new draft from the account's default identity,
//!   or a reply (identity, recipients, subject and quote as "Reply" prepares them); saved, then
//!   closed, so the outbox puts it into the Drafts folder at once. Accounts without writes keep
//!   it on this device (`local_only`).

use mail_mcp::{
    AccountAccess, AttachmentMeta, BodyParts, DraftCreated, ListFilter, ListQuery, MailAccess,
    MessageContent, NewDraft, SearchQuery, SearchResults, ThreadContent,
};
use mail_model::{
    Account, AccountId, Address, AttachmentInfo, ComposeFormat, CoreError, DraftServerState,
    DraftUpdate, LanguagePreference, Mailbox, MailboxId, MessageId, MessageListItem, QuoteLanguage,
};
use mail_store::{SearchFilter, Store, StoredMessage};

use crate::core::{AppCore, Connector, WeakCore};

/// The app side of the MCP tools. Holds the core weakly: the host must not keep it alive.
pub(crate) struct CoreAccess<C: Connector>(pub(crate) WeakCore<C>);

impl<C: Connector> CoreAccess<C> {
    fn core(&self) -> Result<AppCore<C>, CoreError> {
        self.0
            .upgrade()
            .ok_or_else(|| CoreError::Internal("the app is shutting down".into()))
    }
}

impl<C: Connector> MailAccess for CoreAccess<C> {
    async fn accounts(&self) -> Result<Vec<Account>, CoreError> {
        self.core()?.list_accounts().await
    }

    async fn mailboxes(&self, account: AccountId) -> Result<Vec<Mailbox>, CoreError> {
        self.core()?.list_mailboxes(account).await
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResults, CoreError> {
        let page = self
            .core()?
            .search_exactly(
                &query.query,
                &query.accounts,
                query.mailbox,
                query.cursor,
                query.limit,
            )
            .await?;
        Ok(SearchResults {
            hits: page.hits,
            next_cursor: page.next_cursor,
            problems: page.problems,
        })
    }

    async fn list_messages(&self, query: ListQuery) -> Result<SearchResults, CoreError> {
        let page = self
            .core()?
            .list_exactly(
                query.filter == ListFilter::Flagged,
                &query.accounts,
                query.mailbox,
                query.cursor,
                query.limit,
            )
            .await?;
        Ok(SearchResults {
            hits: page.hits,
            next_cursor: page.next_cursor,
            problems: Vec::new(),
        })
    }

    async fn message(
        &self,
        id: MessageId,
        accounts: Vec<AccountId>,
    ) -> Result<Option<MessageContent>, CoreError> {
        let core = self.core()?;
        let store = core.store_ref()?;
        let Some(summary) = store.message_summary(id).await? else {
            return Ok(None);
        };
        if !accounts.contains(&summary.account_id) {
            return Ok(None);
        }
        let Some((row, mailboxes)) = list_row(store, id, summary.account_id).await? else {
            return Ok(None);
        };
        let body = body_of(&core, &summary).await;
        Ok(Some(content(&summary, &row, mailboxes, body)))
    }

    async fn thread(
        &self,
        id: MessageId,
        newest: u32,
        accounts: Vec<AccountId>,
    ) -> Result<Option<ThreadContent>, CoreError> {
        let core = self.core()?;
        let store = core.store_ref()?;
        let Some(summary) = store.message_summary(id).await? else {
            return Ok(None);
        };
        if !accounts.contains(&summary.account_id) {
            return Ok(None);
        }
        let view = store.thread(id, None).await?;
        let keep = usize::try_from(newest).unwrap_or(usize::MAX);
        let start = view.messages.len().saturating_sub(keep);
        let mut messages = Vec::new();
        for entry in view.messages.get(start..).unwrap_or_default() {
            if entry.message.account_id != summary.account_id {
                continue;
            }
            let Some(member) = store.message_summary(entry.message.id).await? else {
                continue;
            };
            let body = body_of(&core, &member).await;
            messages.push(content(
                &member,
                &entry.message,
                entry.mailbox_ids.clone(),
                body,
            ));
        }
        Ok(Some(ThreadContent {
            messages,
            total: view.total,
        }))
    }

    async fn create_draft(&self, draft: NewDraft) -> Result<DraftCreated, CoreError> {
        let core = self.core()?;
        // The host checked this; the core checks it again against the current settings.
        if core.mcp_access_of(&draft.account_id) != AccountAccess::ReadAndDrafts {
            return Err(CoreError::InvalidArgument(
                "the account does not allow drafts from AI clients".into(),
            ));
        }
        core.create_assistant_draft(draft).await
    }
}

/// The message as a list row (flags) with all its mailboxes.
async fn list_row(
    store: &Store,
    id: MessageId,
    account: AccountId,
) -> Result<Option<(MessageListItem, Vec<MailboxId>)>, CoreError> {
    let filter = SearchFilter {
        only: Some(vec![id]),
        accounts: Some(vec![account]),
        ..SearchFilter::default()
    };
    let page = store.search(filter, None, 1).await?;
    Ok(page
        .hits
        .into_iter()
        .next()
        .map(|hit| (hit.item, hit.mailbox_ids)))
}

type Body = Option<(mail_content::ParsedMessage, Vec<AttachmentInfo>)>;

/// The body, or `None` (not stored and not fetched). A failed fetch is not the tool's failure:
/// the client still gets the headers, with `body_format: unavailable`.
async fn body_of<C: Connector>(core: &AppCore<C>, summary: &StoredMessage) -> Body {
    let reading = match core.reading_ref() {
        Ok(reading) => reading,
        Err(err) => {
            tracing::warn!(error = %err, "no reading for MCP");
            return None;
        }
    };
    match reading.content_for_model(summary).await {
        Ok(body) => body,
        Err(err) => {
            tracing::warn!(message = summary.id.0, code = ?err.code(), error = %err, "MCP: body not available");
            None
        }
    }
}

fn content(
    summary: &StoredMessage,
    row: &MessageListItem,
    mailbox_ids: Vec<MailboxId>,
    body: Body,
) -> MessageContent {
    let headers = body.as_ref().map(|(parsed, _)| &parsed.headers);
    let or_stored = |parsed: Option<&Vec<Address>>, stored: &[Address]| -> Vec<Address> {
        match parsed {
            Some(list) if !list.is_empty() => list.clone(),
            _ => stored.to_vec(),
        }
    };
    let stored_from: Vec<Address> = summary.from.iter().cloned().collect();
    MessageContent {
        id: summary.id,
        account_id: summary.account_id,
        mailbox_ids,
        date: row.date,
        subject: summary.subject.clone(),
        from: or_stored(headers.map(|h| &h.from), &stored_from),
        to: or_stored(headers.map(|h| &h.to), &summary.to),
        cc: or_stored(headers.map(|h| &h.cc), &summary.cc),
        reply_to: headers.map(|h| h.reply_to.clone()).unwrap_or_default(),
        unread: row.unread,
        flagged: row.flagged,
        answered: row.answered,
        body: body.map(|(parsed, attachments)| BodyParts {
            text: parsed.text,
            html: parsed.html,
            attachments: attachments
                .into_iter()
                .map(|a| AttachmentMeta {
                    name: a.name,
                    mime_type: a.mime_type,
                    size: u64::from(a.size),
                })
                .collect(),
        }),
    }
}

impl<C: Connector> AppCore<C> {
    /// `create_draft` of the MCP server: a new draft (default identity) or a reply, with the
    /// client's plain text, saved and closed so the outbox takes it to the Drafts folder now.
    pub(crate) async fn create_assistant_draft(
        &self,
        request: NewDraft,
    ) -> Result<DraftCreated, CoreError> {
        let compose = self.compose_ref()?;
        let language = match self.get_preferences().await.map(|p| p.language) {
            Ok(LanguagePreference::De) => QuoteLanguage::De,
            _ => QuoteLanguage::En,
        };
        let draft = match request.in_reply_to {
            Some(original) => {
                compose
                    .create(mail_model::NewDraft::Reply {
                        message_id: original,
                        language,
                    })
                    .await?
            }
            None => {
                compose
                    .create(mail_model::NewDraft::New {
                        account_id: request.account_id,
                        identity_id: None,
                        to: Vec::new(),
                    })
                    .await?
            }
        };
        if draft.account_id != request.account_id {
            compose.delete(draft.id).await?;
            return Err(CoreError::InvalidArgument(
                "in_reply_to is not a message of this account".into(),
            ));
        }
        let addresses = |list: Vec<String>| -> Vec<Address> {
            list.into_iter()
                .map(|email| Address { name: None, email })
                .collect()
        };
        let reply = request.in_reply_to.is_some();
        let to = if reply && request.to.is_empty() {
            draft.to.clone()
        } else {
            addresses(request.to)
        };
        let cc = if reply && request.cc.is_empty() {
            draft.cc.clone()
        } else {
            addresses(request.cc)
        };
        let subject = if request.subject.trim().is_empty() {
            draft.subject.clone()
        } else {
            request.subject
        };
        let update = DraftUpdate {
            identity_id: draft.identity_id,
            to,
            cc,
            bcc: Vec::new(),
            subject,
            format: ComposeFormat::Text,
            body_html: String::new(),
            body_text: request.body,
            include_quote: draft.include_quote,
            quote_position: draft.quote_position,
            include_signature: draft.include_signature,
        };
        if let Err(err) = compose.save(draft.id.clone(), update).await {
            if let Err(cleanup) = compose.delete(draft.id.clone()).await {
                tracing::warn!(error = %cleanup, "an MCP draft that failed to save could not be removed");
            }
            return Err(err);
        }
        let closed = compose.close(draft.id).await?;
        let local_only = closed.server == DraftServerState::LocalOnly;
        tracing::info!(account = %closed.account_id, local_only, "draft created for an AI client");
        Ok(DraftCreated {
            message_id: None,
            local_only,
        })
    }
}
