//! Public API of the store for stage 7: search, the full-text index job, threads, the unified
//! inbox, account groups, hidden accounts and identities. The SQL lives in `fts`, `search`,
//! `threads`, `lists` and `groups`; like `mail_api`, every method only moves its arguments to
//! the writer thread or a reader (file work to the blocking pool) and awaits the result.

use std::collections::{HashMap, HashSet};

use mail_model::{
    AccountGroup, AccountGroupId, AccountId, BodySection, FetchedSection, Identity, IdentityId,
    IdentityInput, ListCursor, MailboxId, MessageId, MessagePage, SearchIndexState,
    SearchIndexStatus, ThreadPage, ThreadView, UidSet, UidValidity,
};
use rusqlite::{OptionalExtension, params};

use crate::fts::{self, BatchItem, BodyText};
use crate::search::{self, SearchFilter, StoredHitPage};
use crate::{Store, StoreError, bodies, body_files, groups, lists, threads};

/// Messages per index job batch and per threading batch (one writer transaction each).
pub const INDEX_BATCH: u32 = 500;

/// What one step of the index job did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexStep {
    /// No job.
    Idle,
    /// A batch of this many messages was written; call again.
    Progress(usize),
    /// The job ended (a rebuild swapped its table in).
    Finished,
}

impl Store {
    // ---------------------------------------------------------------------------------------
    // Search
    // ---------------------------------------------------------------------------------------

    /// One page of messages matching `filter`, newest first (by `message.id`, the sort date),
    /// keyset-paged below `cursor`. `limit` 1..=[`search::MAX_SEARCH_LIMIT`]. A page can be
    /// shorter than `limit` and still have a next one (a message found in two of the searched
    /// mailboxes counts once). Each hit is a list row of one of its locations (in the first
    /// mailbox set if given) with all its mailboxes.
    pub async fn search(
        &self,
        filter: SearchFilter,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<StoredHitPage, StoreError> {
        self.read(move |c| search::search(c, &filter, cursor, limit))
            .await
    }

    /// State of the full-text index (see [`SearchIndexStatus`]).
    pub async fn search_index_status(&self) -> Result<SearchIndexStatus, StoreError> {
        self.read(|c| {
            let Some(job) = fts::job(c)? else {
                return Ok(SearchIndexStatus {
                    state: SearchIndexState::Ready,
                    percent: 100,
                });
            };
            let (low, high) = fts::id_range(c)?;
            let percent = match (low, high) {
                (Some(low), Some(high)) if high > low => {
                    let done = (job.next_id.clamp(low, high) - low) as f64 / (high - low) as f64;
                    (done * 100.0).floor().clamp(0.0, 99.0) as u8
                }
                _ => 0,
            };
            Ok(SearchIndexStatus {
                state: match job.target {
                    fts::Target::Live | fts::Target::Trigrams => SearchIndexState::Building,
                    fts::Target::New => SearchIndexState::Rebuilding,
                },
                percent,
            })
        })
        .await
    }

    /// Stage 22 (decision 0074): bytes of the database pages that hold the word index and the
    /// trigram index (their shadow tables, `dbstat`), for the measurements of
    /// `docs/testing/performance.md`.
    pub async fn full_text_sizes(&self) -> Result<(u64, u64), StoreError> {
        self.read(|c| {
            let size = |table: &str| -> Result<u64, StoreError> {
                let names: Vec<String> = ["data", "idx", "docsize", "config", "content"]
                    .iter()
                    .map(|suffix| format!("'{table}_{suffix}'"))
                    .collect();
                let bytes: i64 = c.query_row(
                    &format!(
                        "SELECT coalesce(sum(pgsize), 0) FROM dbstat WHERE name IN ({})",
                        names.join(", ")
                    ),
                    [],
                    |r| r.get(0),
                )?;
                Ok(u64::try_from(bytes).unwrap_or(0))
            };
            Ok((
                size(crate::search::fts_table(false))?,
                size(crate::search::fts_table(true))?,
            ))
        })
        .await
    }

    /// Starts a rebuild of the full-text index into a new table; the job (run by
    /// [`Store::search_index_step`]) swaps it in at the end, searches use the old one until
    /// then. Returns `false` if an index job runs already.
    pub async fn rebuild_search_index(&self) -> Result<bool, StoreError> {
        let now = crate::settings::now_millis();
        let started = self.write(move |tx| fts::start_rebuild(tx, now)).await?;
        if started {
            tracing::info!("full-text index rebuild started");
        }
        Ok(started)
    }

    /// One batch of the index job: the next [`INDEX_BATCH`] messages by id, their body texts
    /// made on the blocking pool (one read per body file), written in one transaction.
    pub async fn search_index_step(&self) -> Result<IndexStep, StoreError> {
        let (job, items) = self
            .read(|c| match fts::job(c)? {
                None => Ok((None, Vec::new())),
                Some(job) => Ok((Some(job), fts::batch(c, job.next_id, INDEX_BATCH)?)),
            })
            .await?;
        let Some(job) = job else {
            return Ok(IndexStep::Idle);
        };
        let Some(last) = items.last().map(|i| i.id) else {
            let swapped = self.write(move |tx| fts::finish(tx, job)).await?;
            if swapped {
                self.write(|tx| {
                    tx.execute_batch(fts::DROP_OLD)
                        .map_err(StoreError::sqlite("drop the old index"))
                })
                .await?;
                tracing::info!("rebuilt full-text index swapped in");
            } else {
                tracing::info!(?job.target, "existing messages indexed");
            }
            return Ok(IndexStep::Finished);
        };
        let texts = self.batch_texts(&items).await?;
        let entries: Vec<(BatchItem, Option<BodyText>)> = items
            .into_iter()
            .map(|item| {
                let text = texts.get(&item.id).cloned();
                (item, text)
            })
            .collect();
        let count = entries.len();
        let written = self
            .write(move |tx| fts::write_batch(tx, job, &entries, last.saturating_add(1)))
            .await?;
        Ok(if written {
            IndexStep::Progress(count)
        } else {
            IndexStep::Progress(0)
        })
    }

    /// Body texts of the batch items that have a body. Files are read and decompressed once
    /// each; a missing or damaged file just leaves its messages without text (reading them
    /// fetches the body again).
    async fn batch_texts(&self, items: &[BatchItem]) -> Result<HashMap<i64, BodyText>, StoreError> {
        let Some(source) = self.body_text_source() else {
            return Ok(HashMap::new());
        };
        let with_body: Vec<(i64, bool)> = items
            .iter()
            .filter_map(|i| i.body.map(|complete| (i.id, complete)))
            .collect();
        if with_body.is_empty() {
            return Ok(HashMap::new());
        }
        // Per message: the parts to read (whole message, or the text sections) and the
        // structure of a partial body.
        let plan = self
            .read(move |c| {
                let mut plan = Vec::new();
                for (id, complete) in with_body {
                    let message = MessageId(id);
                    let parts = bodies::parts(c, message)?;
                    if complete {
                        let whole: Vec<_> =
                            parts.into_iter().filter(|p| p.section.is_none()).collect();
                        plan.push((id, None, whole));
                    } else {
                        let Some(structure) = bodies::info(c, message)?.and_then(|i| i.structure)
                        else {
                            continue;
                        };
                        let text: Vec<_> = parts
                            .into_iter()
                            .filter(|p| match &p.section {
                                Some(BodySection::Header) => true,
                                Some(BodySection::Part(path) | BodySection::Mime(path)) => {
                                    structure.part(path).is_some_and(|m| m.is_body_text())
                                }
                                None => false,
                            })
                            .collect();
                        plan.push((id, Some(structure), text));
                    }
                }
                Ok(plan)
            })
            .await?;
        let root = self.bodies_dir().to_owned();
        tokio::task::spawn_blocking(move || {
            let mut files: HashMap<String, Option<Vec<u8>>> = HashMap::new();
            let mut out = HashMap::new();
            for (id, structure, parts) in plan {
                let mut sections = Vec::new();
                let mut whole = None;
                for part in parts {
                    let data = files.entry(part.file.clone()).or_insert_with(|| {
                        body_files::read(&root, &part.file)
                            .ok()
                            .and_then(|bytes| body_files::decompress(&bytes).ok())
                    });
                    let Some(bytes) = data
                        .as_deref()
                        .and_then(|d| d.get(part.offset..part.offset.saturating_add(part.length)))
                    else {
                        continue;
                    };
                    match part.section {
                        None => whole = Some(bytes.to_vec()),
                        Some(section) => sections.push(FetchedSection {
                            section,
                            data: bytes.to_vec(),
                        }),
                    }
                }
                let text = match (&structure, whole) {
                    (None, Some(raw)) => Some(source.complete(&raw)),
                    (Some(structure), _) if !sections.is_empty() => {
                        Some(source.partial(structure, &sections))
                    }
                    _ => None,
                };
                if let Some(text) = text {
                    out.insert(id, text);
                }
                // Packs are written in id order, so a file is rarely needed again later.
                if files.len() > 16 {
                    files.clear();
                }
            }
            out
        })
        .await
        .map_err(|err| StoreError::JobPanicked(format!("index text task: {err}")))
    }

    // ---------------------------------------------------------------------------------------
    // Threads
    // ---------------------------------------------------------------------------------------

    /// Whether background work is left: an index job, or messages without a thread.
    pub async fn background_work_pending(&self) -> Result<bool, StoreError> {
        self.read(|c| {
            if fts::job(c)?.is_some() {
                return Ok(true);
            }
            Ok(c.prepare_cached(threads::ANY_UNTHREADED)?
                .query_row([], |_| Ok(()))
                .optional()?
                .is_some())
        })
        .await
    }

    /// Threads up to [`INDEX_BATCH`] messages stored without a thread (before stage 7), oldest
    /// first, in one transaction. Returns how many; 0 when all are threaded.
    pub async fn thread_backfill_step(&self) -> Result<usize, StoreError> {
        self.write(|tx| threads::backfill(tx, INDEX_BATCH)).await
    }

    /// One page of threads of a mailbox: per thread the newest of its messages in the mailbox,
    /// newest first, keyset-paged by that message's id. `limit` 1..=[`crate::MAX_PAGE_SIZE`].
    pub async fn list_threads(
        &self,
        mailbox: MailboxId,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, StoreError> {
        self.read(move |c| {
            let account = lists::mailbox_account(c, mailbox)?;
            lists::threads(c, &[(mailbox, account)], cursor, limit)
        })
        .await
    }

    /// Threads of the unified inbox (`group`: of the group's accounts); hidden accounts are
    /// left out.
    pub async fn list_unified_threads(
        &self,
        group: Option<AccountGroupId>,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, StoreError> {
        self.read(move |c| {
            let inboxes = groups::unified_inboxes(c, group.as_ref())?;
            lists::threads(c, &inboxes, cursor, limit)
        })
        .await
    }

    /// Every message of the thread of `message` over all mailboxes of its account, oldest
    /// first (at most the newest [`mail_model::MAX_THREAD_MESSAGES`]). Each entry shows its
    /// location in `prefer` if it lies there.
    pub async fn thread(
        &self,
        message: MessageId,
        prefer: Option<MailboxId>,
    ) -> Result<ThreadView, StoreError> {
        self.read(move |c| lists::thread_view(c, message, prefer, mail_model::MAX_THREAD_MESSAGES))
            .await
    }

    // ---------------------------------------------------------------------------------------
    // Unified inbox, groups, hidden accounts
    // ---------------------------------------------------------------------------------------

    /// One page of the unified inbox: the inboxes of all visible accounts (or of the group's
    /// visible accounts) mixed by time, newest first, `UNION ALL` of one keyset branch per
    /// inbox. Each item carries its own account and mailbox.
    pub async fn list_unified(
        &self,
        group: Option<AccountGroupId>,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, StoreError> {
        self.read(move |c| {
            let inboxes = groups::unified_inboxes(c, group.as_ref())?;
            lists::unified(c, &inboxes, cursor, limit)
        })
        .await
    }

    /// Stage 23 (decision 0087): one page of "Sent" over all visible accounts (or the group's
    /// visible accounts): their Sent folders mixed by time, newest first, like
    /// [`Store::list_unified`]. Each item carries its own account and mailbox.
    pub async fn list_unified_sent(
        &self,
        group: Option<AccountGroupId>,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, StoreError> {
        self.read(move |c| {
            let sent = groups::unified_sent(c, group.as_ref())?;
            lists::unified(c, &sent, cursor, limit)
        })
        .await
    }

    /// Stage 23 (decision 0087): the threads of those Sent folders, like
    /// [`Store::list_unified_threads`].
    pub async fn list_unified_sent_threads(
        &self,
        group: Option<AccountGroupId>,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, StoreError> {
        self.read(move |c| {
            let sent = groups::unified_sent(c, group.as_ref())?;
            lists::threads(c, &sent, cursor, limit)
        })
        .await
    }

    /// Names and roles of the account's mailboxes, without counters (for `in:`).
    pub async fn mailbox_names(
        &self,
        account: AccountId,
    ) -> Result<Vec<groups::MailboxName>, StoreError> {
        self.read(move |c| groups::mailbox_names(c, account)).await
    }

    /// The account a mailbox belongs to; [`StoreError::NotFound`] if there is none.
    pub async fn mailbox_owner(&self, mailbox: MailboxId) -> Result<AccountId, StoreError> {
        self.read(move |c| lists::mailbox_account(c, mailbox)).await
    }

    /// The visible accounts (oldest first), or the group's visible members.
    pub async fn visible_accounts(
        &self,
        group: Option<AccountGroupId>,
    ) -> Result<Vec<AccountId>, StoreError> {
        self.read(move |c| {
            Ok(groups::visible_accounts(c, group.as_ref())?
                .into_iter()
                .map(|(_, a)| a)
                .collect())
        })
        .await
    }

    /// All account groups in sidebar order.
    pub async fn account_groups(&self) -> Result<Vec<AccountGroup>, StoreError> {
        self.read(groups::groups).await
    }

    /// Creates a group (name 1..=100 characters, trimmed) with these accounts, last in order.
    pub async fn create_account_group(
        &self,
        name: String,
        accounts: Vec<AccountId>,
    ) -> Result<AccountGroup, StoreError> {
        self.write(move |tx| {
            let id = groups::create_group(tx, &name, &accounts)?;
            find_group(tx, &id)
        })
        .await
    }

    /// Renames a group and replaces its members.
    pub async fn update_account_group(
        &self,
        id: AccountGroupId,
        name: String,
        accounts: Vec<AccountId>,
    ) -> Result<AccountGroup, StoreError> {
        self.write(move |tx| {
            groups::update_group(tx, &id, &name, &accounts)?;
            find_group(tx, &id)
        })
        .await
    }

    /// Deletes a group (not its accounts). Returns `false` if it did not exist.
    pub async fn delete_account_group(&self, id: AccountGroupId) -> Result<bool, StoreError> {
        self.write(move |tx| groups::delete_group(tx, &id)).await
    }

    /// Hidden accounts (this device only).
    pub async fn hidden_accounts(&self) -> Result<HashSet<AccountId>, StoreError> {
        self.read(groups::hidden).await
    }

    /// Hides an account from the unified inbox, groups and "search everywhere", or shows it
    /// again. Returns whether anything changed.
    pub async fn set_account_hidden(
        &self,
        account: AccountId,
        hidden: bool,
    ) -> Result<bool, StoreError> {
        self.write(move |tx| groups::set_hidden(tx, account, hidden))
            .await
    }

    // ---------------------------------------------------------------------------------------
    // Identities
    // ---------------------------------------------------------------------------------------

    /// Adds an identity (not the default). [`StoreError::InvalidArgument`] for an address that
    /// is not one, or one the account has already (ignoring case).
    pub async fn add_identity(
        &self,
        account: AccountId,
        input: IdentityInput,
    ) -> Result<Identity, StoreError> {
        self.write(move |tx| {
            let id = groups::add_identity(tx, account, &input)?;
            groups::identity(tx, id)
        })
        .await
    }

    pub async fn update_identity(
        &self,
        id: IdentityId,
        input: IdentityInput,
    ) -> Result<Identity, StoreError> {
        self.write(move |tx| {
            groups::update_identity(tx, id, &input)?;
            groups::identity(tx, id)
        })
        .await
    }

    /// Makes the identity its account's default; returns the account's identities.
    pub async fn set_default_identity(&self, id: IdentityId) -> Result<Vec<Identity>, StoreError> {
        self.write(move |tx| {
            let account = groups::set_default_identity(tx, id)?;
            crate::accounts::identities(tx, account)
        })
        .await
    }

    /// Deletes an identity that is not the default; returns its account.
    pub async fn delete_identity(&self, id: IdentityId) -> Result<AccountId, StoreError> {
        self.write(move |tx| groups::delete_identity(tx, id)).await
    }

    // ---------------------------------------------------------------------------------------
    // Server search
    // ---------------------------------------------------------------------------------------

    /// The stored messages behind these UIDs of the mailbox (server search results), and how
    /// many UIDs are not stored (yet). Nothing if the mailbox's UIDVALIDITY is another one.
    pub async fn messages_for_uids(
        &self,
        mailbox: MailboxId,
        uid_validity: UidValidity,
        uids: UidSet,
    ) -> Result<(Vec<MessageId>, usize), StoreError> {
        self.read(move |c| {
            let stored: Option<Option<u32>> = c
                .prepare_cached("SELECT uidvalidity FROM mailbox WHERE id = ?1")?
                .query_row([mailbox.0], |r| r.get(0))
                .optional()?;
            if stored.flatten() != Some(uid_validity.0) {
                return Ok((
                    Vec::new(),
                    usize::try_from(uids.len()).unwrap_or(usize::MAX),
                ));
            }
            let mut stmt = c.prepare_cached(lists::MESSAGE_BY_UID)?;
            let mut found = Vec::new();
            let mut missing = 0;
            for uid in uids.iter() {
                match stmt
                    .query_row(params![mailbox.0, uid], |r| r.get::<_, i64>(0))
                    .optional()?
                {
                    Some(id) => found.push(MessageId(id)),
                    None => missing += 1,
                }
            }
            Ok((found, missing))
        })
        .await
    }
}

fn find_group(
    conn: &rusqlite::Connection,
    id: &AccountGroupId,
) -> Result<AccountGroup, StoreError> {
    groups::groups(conn)?
        .into_iter()
        .find(|g| &g.id == id)
        .ok_or_else(|| StoreError::NotFound(format!("account group {id}")))
}
