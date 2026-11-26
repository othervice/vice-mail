//! Stage 7 in the core: search (local and on the server), threads, the unified inbox, account
//! groups, hidden accounts and identities, as methods of [`AppCore`]; and the background work
//! that indexes and threads messages stored before stage 7.
//!
//! **Search.** The query is parsed and compiled by `mail_model::search` with the user's time
//! zone ([`LocalZone`], the system zone through jiff) and date order; this module resolves
//! `account:` (address, name, UUID or domain) and `in:` (role first, else full path or last
//! part of the path, ignoring case) against the stored accounts and mailboxes of the scope,
//! and hands the store a [`SearchFilter`]. Problems (parser errors, contradictions, unknown
//! accounts or mailboxes) come back with UTF-16 ranges; a query that cannot match returns no
//! hits without asking the store. Highlights are computed here from the shown fields (the index
//! is contentless, decision 0003).
//!
//! **Background.** [`SearchState`] runs the index job of the store (fill after the migration,
//! or a rebuild) and the threading of old messages in small transactions, one after another,
//! yielding in between so sync windows and user actions interleave.

#[cfg(test)]
mod bench;
pub(crate) mod resolve;
mod saved;
mod server;
#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod stage22_tests;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, PoisonError, Weak};

use mail_model::search::{
    CompileOptions, HighlightField, HighlightTerm, SlashDateOrder, highlight,
};
use mail_model::{
    AccountGroup, AccountGroupId, AccountId, CoreError, CoreEvent, Identity, IdentityId,
    IdentityInfo, IdentityInput, ListCursor, MAX_SEARCH_PAGE, MailboxId, MessageId, MessagePage,
    SearchHighlights, SearchHit, SearchIndexStatus, SearchPage, SearchRequest, ServerSearchResult,
    ThreadPage, ThreadView, UnifiedView, utf16_range,
};
use mail_store::{IndexStep, Store, StoredHit};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::core::{AppCore, Connector};
use crate::reading::index_text::ContentText;

pub use resolve::LocalZone;

/// The background work of stage 7: index job and threading of old messages.
///
/// The worker holds the store only weakly and only during a step, so dropping the core closes
/// the database at once (the last connection checkpoints the WAL), as without stage 7.
pub(crate) struct SearchState {
    wake: Arc<Notify>,
    /// Keeps the store the worker reaches through its weak reference.
    _store: Option<Arc<Store>>,
    task: Mutex<Option<JoinHandle<()>>>,
    /// Work found at start, begun by [`SearchState::started`].
    pending_at_start: std::sync::atomic::AtomicBool,
}

impl SearchState {
    /// Gives the store its body text source and looks whether there is work left (an index
    /// job, messages without a thread). The worker begins with [`SearchState::started`] at the
    /// end of [`AppCore::start`] (decision 0029), so its first step does not hold the writer
    /// while the start's own writes wait.
    pub(crate) async fn start(store: Option<&Store>) -> Self {
        let wake = Arc::new(Notify::new());
        let store = store.map(|store| {
            store.set_body_text_source(Arc::new(ContentText));
            Arc::new(store.clone())
        });
        let mut pending = false;
        if let Some(store) = &store {
            match store.background_work_pending().await {
                Ok(found) => pending = found,
                Err(err) => {
                    tracing::error!(error = %err, "background indexing could not start");
                }
            }
        }
        Self {
            wake,
            _store: store,
            task: Mutex::new(None),
            pending_at_start: std::sync::atomic::AtomicBool::new(pending),
        }
    }

    /// The core has started: begins the work found at start, if any.
    pub(crate) fn started(&self) {
        if self
            .pending_at_start
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.kick();
        }
    }

    /// New work (a rebuild was started): wakes the worker, or starts it.
    fn kick(&self) {
        let Some(store) = &self._store else { return };
        let mut task = self.task.lock().unwrap_or_else(PoisonError::into_inner);
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            self.wake.notify_one();
        } else {
            *task = Some(tokio::spawn(background(
                Arc::downgrade(store),
                Arc::clone(&self.wake),
            )));
        }
    }

    pub(crate) fn stop(&self) {
        if let Some(task) = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

impl Drop for SearchState {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One step of the work, `None` when the core is gone.
enum Step {
    Index(Result<IndexStep, mail_store::StoreError>),
    Thread(Result<usize, mail_store::StoreError>),
}

async fn step(store: &Weak<Store>, index: bool) -> Option<Step> {
    let store = store.upgrade()?;
    Some(if index {
        Step::Index(store.search_index_step().await)
    } else {
        Step::Thread(store.thread_backfill_step().await)
    })
}

/// Indexes and threads until nothing is left, then waits for [`SearchState::kick`]. Ends when
/// the core is gone.
async fn background(store: Weak<Store>, wake: Arc<Notify>) {
    loop {
        let mut indexed = 0usize;
        loop {
            let Some(next) = step(&store, true).await else {
                return;
            };
            match next {
                Step::Index(Ok(IndexStep::Progress(n))) => indexed += n,
                Step::Index(Ok(IndexStep::Finished | IndexStep::Idle)) => break,
                Step::Index(Err(err)) => {
                    tracing::error!(error = %err, "indexing stopped; it resumes at the next start");
                    break;
                }
                Step::Thread(_) => break,
            }
            tokio::task::yield_now().await;
        }
        if indexed > 0 {
            tracing::info!(indexed, "full-text index job done");
        }
        let mut threaded = 0usize;
        loop {
            let Some(next) = step(&store, false).await else {
                return;
            };
            match next {
                Step::Thread(Ok(0)) => break,
                Step::Thread(Ok(n)) => threaded += n,
                Step::Thread(Err(err)) => {
                    tracing::error!(error = %err, "threading of stored messages stopped");
                    break;
                }
                Step::Index(_) => break,
            }
            tokio::task::yield_now().await;
        }
        if threaded > 0 {
            tracing::info!(threaded, "stored messages threaded");
        }
        wake.notified().await;
    }
}

/// A page of [`AppCore::search_exactly`] or [`AppCore::list_exactly`].
#[derive(Debug, Clone, Default)]
pub(crate) struct ExactPage {
    pub(crate) hits: Vec<mail_model::MessageListItem>,
    pub(crate) next_cursor: Option<ListCursor>,
    pub(crate) problems: Vec<mail_model::SearchProblemCode>,
}

fn check_limit(limit: u32) -> Result<(), CoreError> {
    if limit == 0 || limit > MAX_SEARCH_PAGE {
        return Err(CoreError::InvalidArgument(format!(
            "the limit {limit} is outside 1..={MAX_SEARCH_PAGE}"
        )));
    }
    Ok(())
}

/// A stored hit with its highlights.
fn hit(stored: StoredHit, terms: &[HighlightTerm]) -> SearchHit {
    let marks = |text: &str, field| {
        highlight(text, field, terms)
            .into_iter()
            .map(|span| utf16_range(span, text))
            .collect::<Vec<_>>()
    };
    let item = stored.item;
    let from = item
        .from
        .as_ref()
        .map(|a| {
            a.name
                .as_deref()
                .filter(|n| !n.is_empty())
                .unwrap_or(&a.email)
        })
        .unwrap_or_default();
    let highlights = SearchHighlights {
        subject: marks(&item.subject, HighlightField::Subject),
        from: marks(from, HighlightField::From),
        preview: item
            .preview
            .as_deref()
            .map(|p| marks(p, HighlightField::Preview))
            .unwrap_or_default(),
    };
    SearchHit {
        message: item,
        mailbox_ids: stored.mailbox_ids,
        highlights,
        body_local: stored.body_complete,
        thread_id: stored.thread_id,
        thread_message_count: stored.thread_message_count,
        conversation_subject: stored.conversation_subject,
    }
}

impl<C: Connector> AppCore<C> {
    // -----------------------------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------------------------

    /// Searches the local messages (see the module docs): newest first, keyset-paged, with
    /// highlights, the query's problems and terms (for chips) and the index state. An empty
    /// query gives no hits.
    pub async fn search(&self, request: SearchRequest) -> Result<SearchPage, CoreError> {
        check_limit(request.limit)?;
        let store = self.store_ref()?;
        let index = store.search_index_status().await?;
        let options = CompileOptions {
            prefix_last_word: request.typing,
            substrings: resolve::substrings(&index),
        };
        let compiled = {
            let zone = LocalZone::system();
            let context = self.search_context(&zone, &request);
            resolve::compile(&request, &context, options)
        };
        let plan = resolve::plan(store, &request, compiled).await?;
        let terms = plan.parsed.highlight_terms(options);
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            problems: plan.problems.clone(),
            terms: plan.chips.clone(),
            matches_nothing: plan.matches_nothing,
            index,
        };
        if plan.matches_nothing || plan.empty {
            return Ok(page);
        }
        let found = store
            .search(plan.filter, request.cursor, request.limit)
            .await?;
        page.hits = found.hits.into_iter().map(|h| hit(h, &terms)).collect();
        page.next_cursor = found.next_cursor;
        Ok(page)
    }

    /// Searches on the server for messages whose body is not stored locally: IMAP SEARCH over
    /// each account's read-only reading connection, in the mailboxes of the scope (the
    /// mailboxes of `in:`; else every selectable mailbox but trash and junk, or only "All
    /// Mail" where there is one). The found messages are filtered locally like a local search
    /// (dates exactly, `has:attachment`, mailboxes) and returned newest first, at most
    /// `request.limit` (`truncated` if there are more; `request.cursor` continues below a hit).
    pub async fn search_on_server(
        &self,
        request: SearchRequest,
    ) -> Result<ServerSearchResult, CoreError> {
        check_limit(request.limit)?;
        let (compiled, server) = {
            let zone = LocalZone::system();
            let context = self.search_context(&zone, &request);
            let compiled = resolve::compile(&request, &context, CompileOptions::default());
            let server = compiled.0.server_query(&context);
            (compiled, server)
        };
        server::search(self, &request, compiled, server).await
    }

    /// Stage 12 (AI access over MCP): a submitted search in exactly `accounts`, or in one of
    /// their mailboxes. Without a mailbox and without a positive `in:`, Trash and Junk are left
    /// out. Hidden accounts of the app do not matter (the caller decides what is shared).
    pub(crate) async fn search_exactly(
        &self,
        query: &str,
        accounts: &[AccountId],
        mailbox: Option<MailboxId>,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ExactPage, CoreError> {
        check_limit(limit)?;
        let store = self.store_ref()?;
        let request = SearchRequest {
            query: query.to_owned(),
            scope: mail_model::SearchScope::All,
            cursor,
            limit,
            typing: false,
            month_first: false,
        };
        let options = CompileOptions {
            prefix_last_word: false,
            substrings: resolve::substrings_now(store).await?,
        };
        let compiled = {
            let zone = LocalZone::system();
            let context = self.search_context(&zone, &request);
            resolve::compile(&request, &context, options)
        };
        let plan = resolve::plan_with(
            store,
            &request,
            compiled,
            resolve::Base::Exactly { accounts, mailbox },
        )
        .await?;
        let mut page = ExactPage {
            problems: plan.problems.iter().map(|p| p.code).collect(),
            ..ExactPage::default()
        };
        if plan.matches_nothing || plan.empty || accounts.is_empty() {
            return Ok(page);
        }
        let found = store.search(plan.filter, cursor, limit).await?;
        page.hits = found.hits.into_iter().map(|h| h.item).collect();
        page.next_cursor = found.next_cursor;
        Ok(page)
    }

    /// Stage 12: unread or flagged messages in exactly `accounts` (or one of their mailboxes),
    /// newest first; without a mailbox Trash and Junk are left out.
    pub(crate) async fn list_exactly(
        &self,
        flagged: bool,
        accounts: &[AccountId],
        mailbox: Option<MailboxId>,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ExactPage, CoreError> {
        /// Up to this many mailboxes are one `UNION ALL` branch each (driven by `loc_unseen`);
        /// more are filtered per message (SQLite allows 500 compound terms).
        const MAX_BRANCHES: usize = 100;
        check_limit(limit)?;
        let store = self.store_ref()?;
        if accounts.is_empty() {
            return Ok(ExactPage::default());
        }
        let mut boxes = Vec::new();
        let mut skipped = Vec::new();
        match mailbox {
            Some(mailbox) => {
                if !accounts.contains(&store.mailbox_owner(mailbox).await?) {
                    return Err(CoreError::NotFound(format!("mailbox {}", mailbox.0)));
                }
                boxes.push(mailbox);
            }
            None => {
                for account in accounts {
                    for name in store.mailbox_names(*account).await? {
                        if matches!(
                            name.role,
                            mail_model::MailboxRole::Trash | mail_model::MailboxRole::Junk
                        ) {
                            skipped.push(name.id);
                        } else {
                            boxes.push(name.id);
                        }
                    }
                }
            }
        }
        let mut filter = mail_store::SearchFilter {
            accounts: Some(accounts.to_vec()),
            ..Default::default()
        };
        if flagged {
            filter.flagged = Some(true);
        } else {
            filter.unread = Some(true);
        }
        if boxes.len() <= MAX_BRANCHES {
            filter.mailbox_sets = vec![boxes];
        } else {
            filter.excluded_mailboxes = skipped;
        }
        let found = store.search(filter, cursor, limit).await?;
        Ok(ExactPage {
            hits: found.hits.into_iter().map(|h| h.item).collect(),
            next_cursor: found.next_cursor,
            problems: Vec::new(),
        })
    }

    /// State of the full-text index.
    pub async fn get_search_index_status(&self) -> Result<SearchIndexStatus, CoreError> {
        Ok(self.store_ref()?.search_index_status().await?)
    }

    /// Rebuilds the full-text index in the background; searches keep working on the old one
    /// until the new one replaces it. `invalid_argument` if an index job runs already.
    pub async fn rebuild_search_index(&self) -> Result<SearchIndexStatus, CoreError> {
        let store = self.store_ref()?;
        if !store.rebuild_search_index().await? {
            return Err(CoreError::InvalidArgument(
                "the search index is being built already".into(),
            ));
        }
        self.search_state().kick();
        Ok(store.search_index_status().await?)
    }

    pub(crate) fn search_context<'z>(
        &self,
        zone: &'z LocalZone,
        request: &SearchRequest,
    ) -> mail_model::search::SearchContext<'z> {
        mail_model::search::SearchContext {
            now: self.now_ms().div_euclid(1000),
            time_zone: zone,
            slash_dates: if request.month_first {
                SlashDateOrder::MonthFirst
            } else {
                SlashDateOrder::DayFirst
            },
        }
    }

    // -----------------------------------------------------------------------------------------
    // Threads and the unified inbox
    // -----------------------------------------------------------------------------------------

    /// Threads of a mailbox, newest first: per thread its newest message in the mailbox, the
    /// message count over all mailboxes and the unread count. `limit` 1..=500.
    pub async fn list_threads(
        &self,
        mailbox: MailboxId,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, CoreError> {
        Ok(self
            .store_ref()?
            .list_threads(mailbox, cursor, limit)
            .await?)
    }

    /// Threads of the unified inbox (or of a group's inboxes); hidden accounts left out.
    pub async fn list_unified_threads(
        &self,
        view: UnifiedView,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, CoreError> {
        Ok(self
            .store_ref()?
            .list_unified_threads(group_of(view), cursor, limit)
            .await?)
    }

    /// The thread of `message`: all its messages in all mailboxes of the account, oldest first.
    /// `mailbox` is where it was opened from (entries show that location when they lie there).
    pub async fn get_thread(
        &self,
        message: MessageId,
        mailbox: Option<MailboxId>,
    ) -> Result<ThreadView, CoreError> {
        Ok(self.store_ref()?.thread(message, mailbox).await?)
    }

    /// One page of the unified inbox: the inboxes of all visible accounts (or of a group's)
    /// mixed by time, newest first; every item carries its account. `limit` 1..=500.
    pub async fn list_unified_inbox(
        &self,
        view: UnifiedView,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, CoreError> {
        Ok(self
            .store_ref()?
            .list_unified(group_of(view), cursor, limit)
            .await?)
    }

    /// Stage 23 (decision 0087): one page of "Sent" over all visible accounts (or a group's):
    /// their Sent folders mixed by time, newest first; every item carries its account.
    /// `limit` 1..=500.
    pub async fn list_unified_sent(
        &self,
        view: UnifiedView,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, CoreError> {
        Ok(self
            .store_ref()?
            .list_unified_sent(group_of(view), cursor, limit)
            .await?)
    }

    /// Stage 23 (decision 0087): the threads of those Sent folders. `limit` 1..=500.
    pub async fn list_unified_sent_threads(
        &self,
        view: UnifiedView,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, CoreError> {
        Ok(self
            .store_ref()?
            .list_unified_sent_threads(group_of(view), cursor, limit)
            .await?)
    }

    // -----------------------------------------------------------------------------------------
    // Account groups and hidden accounts
    // -----------------------------------------------------------------------------------------

    pub async fn list_account_groups(&self) -> Result<Vec<AccountGroup>, CoreError> {
        Ok(self.store_ref()?.account_groups().await?)
    }

    /// Creates a group; emits `accountsChanged`.
    pub async fn create_account_group(
        &self,
        name: String,
        accounts: Vec<AccountId>,
    ) -> Result<AccountGroup, CoreError> {
        let group = self
            .store_ref()?
            .create_account_group(name, accounts)
            .await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(group)
    }

    /// Renames a group and sets its members; emits `accountsChanged`.
    pub async fn update_account_group(
        &self,
        id: AccountGroupId,
        name: String,
        accounts: Vec<AccountId>,
    ) -> Result<AccountGroup, CoreError> {
        let group = self
            .store_ref()?
            .update_account_group(id, name, accounts)
            .await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(group)
    }

    /// Deletes a group (`not_found` if there is none); emits `accountsChanged`.
    pub async fn delete_account_group(&self, id: AccountGroupId) -> Result<(), CoreError> {
        if !self.store_ref()?.delete_account_group(id.clone()).await? {
            return Err(CoreError::NotFound(format!("account group {id}")));
        }
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(())
    }

    /// Accounts hidden on this device, in account order.
    pub async fn list_hidden_accounts(&self) -> Result<Vec<AccountId>, CoreError> {
        let store = self.store_ref()?;
        let hidden = store.hidden_accounts().await?;
        Ok(store
            .accounts()
            .await?
            .into_iter()
            .map(|a| a.id)
            .filter(|id| hidden.contains(id))
            .collect())
    }

    /// Hides an account from the unified inbox, groups and "search everywhere" (this device
    /// only), or shows it again. Its sync keeps running. Emits `accountsChanged` on a change.
    pub async fn set_account_hidden(
        &self,
        account: AccountId,
        hidden: bool,
    ) -> Result<(), CoreError> {
        if self
            .store_ref()?
            .set_account_hidden(account, hidden)
            .await?
        {
            tracing::info!(%account, hidden, "account visibility changed");
            self.emit_event(CoreEvent::AccountsChanged);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Identities
    // -----------------------------------------------------------------------------------------

    /// Identities of an account, the default first.
    pub async fn list_identities(
        &self,
        account: AccountId,
    ) -> Result<Vec<IdentityInfo>, CoreError> {
        let store = self.store_ref()?;
        if store.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        Ok(store
            .identities(account)
            .await?
            .into_iter()
            .map(IdentityInfo::from)
            .collect())
    }

    /// Adds a sender address to the account; emits `accountsChanged`.
    pub async fn add_identity(
        &self,
        account: AccountId,
        input: IdentityInput,
    ) -> Result<IdentityInfo, CoreError> {
        let identity = self.store_ref()?.add_identity(account, input).await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(identity.into())
    }

    pub async fn update_identity(
        &self,
        id: IdentityId,
        input: IdentityInput,
    ) -> Result<IdentityInfo, CoreError> {
        let identity = self.store_ref()?.update_identity(id, input).await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(identity.into())
    }

    /// Makes an identity its account's default; returns the account's identities.
    pub async fn set_default_identity(
        &self,
        id: IdentityId,
    ) -> Result<Vec<IdentityInfo>, CoreError> {
        let identities = self.store_ref()?.set_default_identity(id).await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(identities.into_iter().map(IdentityInfo::from).collect())
    }

    /// Deletes an identity; the default one cannot be deleted (`invalid_argument`).
    pub async fn delete_identity(&self, id: IdentityId) -> Result<(), CoreError> {
        self.store_ref()?.delete_identity(id).await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(())
    }

    /// The identity a reply to `message` is sent from: the address the mail went to (From for
    /// one's own mail, then To and Cc, also without `+tag`), else the default
    /// (`mail_content::choose_identity`).
    pub async fn reply_identity(&self, message: MessageId) -> Result<IdentityInfo, CoreError> {
        let store = self.store_ref()?;
        let summary = store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let identities: Vec<Identity> = store.identities(summary.account_id).await?;
        let headers = mail_content::MessageHeaders {
            from: summary.from.into_iter().collect(),
            to: summary.to,
            cc: summary.cc,
            ..mail_content::MessageHeaders::default()
        };
        mail_content::choose_identity(&identities, &headers, &[])
            .cloned()
            .map(IdentityInfo::from)
            .ok_or_else(|| {
                CoreError::NotFound(format!("an identity of account {}", summary.account_id))
            })
    }
}

fn group_of(view: UnifiedView) -> Option<AccountGroupId> {
    match view {
        UnifiedView::All => None,
        UnifiedView::Group { group_id } => Some(group_id),
    }
}
