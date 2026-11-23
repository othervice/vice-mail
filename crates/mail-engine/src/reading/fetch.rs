//! The reading connection of an account and fetching bodies over it.
//!
//! Each account gets one more connection for reading, besides the sync and IDLE connections of
//! stage 2: opening a message must not wait for a sync pass, and the sync must not wait for a
//! large attachment. It is always read-only (EXAMINE, `BODY.PEEK[...]`; the adapter's allowlist
//! refuses anything else), whatever the account allows. It is opened on first use, checked with
//! NOOP after [`STALE_AFTER`] without use (servers log idle clients out), dropped after a
//! connection-level error and opened again on the next use. Opening a message and the prefetch
//! share it; the lock is fair, so an open waits for at most one prefetch batch.
//!
//! A connection used within [`STALE_AFTER`] is not checked, so after a server restart or a
//! network change the next fetch runs into the dead connection. Fetching for the user (opening
//! a message, an attachment) therefore runs once more on a new connection after a
//! connection-level failure ([`retry_on_new_connection`]) before it reports the error; never
//! after a refusal, a login or certificate error, and never more than once.
//!
//! Where to fetch from: the message's locations (`message_location`), each checked against the
//! UIDVALIDITY the server reports on EXAMINE. A location whose UIDVALIDITY changed is skipped
//! (the next sync rebuilds it); a UID the server no longer has is "not on the server".

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    AccountId, BackendError, BodyFetchPlan, BodySection, CoreError, FetchedSection,
    LARGE_MESSAGE_THRESHOLD, MailBackend, MailConnector, MessageId, MessageStructure,
    SelectedMailbox, Uid, UidSet,
};
use mail_store::{MessageLocation, Store, StoredAccount};
use tokio::sync::OwnedMutexGuard;
use tokio::time::Instant;

use crate::credentials::Credentials;
use crate::sync::SyncConfig;

/// A reading connection unused for this long is checked with NOOP before it is used again.
pub const STALE_AFTER: Duration = Duration::from_secs(60);

/// One account's reading connection.
pub(crate) struct ReadConnection<B> {
    backend: Option<B>,
    last_used: Option<Instant>,
}

impl<B> Default for ReadConnection<B> {
    fn default() -> Self {
        Self {
            backend: None,
            last_used: None,
        }
    }
}

/// One account's reading connection behind its (fair) lock.
type Slot<B> = Arc<tokio::sync::Mutex<ReadConnection<B>>>;

/// The reading connections of all accounts.
pub(crate) struct Connections<C: MailConnector> {
    connector: Arc<C>,
    credentials: Credentials,
    config: Arc<SyncConfig>,
    by_account: Mutex<HashMap<AccountId, Slot<C::Backend>>>,
}

/// A connected reading connection, held exclusively until dropped.
pub(crate) struct Held<B> {
    guard: OwnedMutexGuard<ReadConnection<B>>,
}

impl<C: MailConnector> Connections<C> {
    pub fn new(connector: Arc<C>, credentials: Credentials, config: Arc<SyncConfig>) -> Self {
        Self {
            connector,
            credentials,
            config,
            by_account: Mutex::default(),
        }
    }

    /// Waits for the account's reading connection and makes sure it is connected.
    pub async fn hold(&self, account: &StoredAccount) -> Result<Held<C::Backend>, CoreError> {
        let slot = {
            let mut map = self
                .by_account
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            Arc::clone(map.entry(account.id).or_default())
        };
        let mut guard = slot.lock_owned().await;
        let stale = guard
            .last_used
            .is_none_or(|used| used.elapsed() >= STALE_AFTER);
        if stale
            && let Some(backend) = guard.backend.as_mut()
            && let Err(err) = backend.noop().await
        {
            tracing::debug!(account = %account.id, error = %err, "reading connection went stale; reconnecting");
            guard.backend = None;
        }
        self.connect_if_closed(&mut guard, account).await?;
        Ok(Held { guard })
    }

    /// After `cause` ([`retry_on_new_connection`]): closes the held connection and opens a new
    /// one, keeping the lock, so the caller can run its step once more.
    async fn reconnect(
        &self,
        held: &mut Held<C::Backend>,
        account: &StoredAccount,
        cause: &CoreError,
    ) -> Result<(), CoreError> {
        tracing::info!(account = %account.id, error = %cause, "reading connection failed; trying once more on a new connection");
        held.guard.backend = None;
        self.connect_if_closed(&mut held.guard, account).await
    }

    async fn connect_if_closed(
        &self,
        connection: &mut ReadConnection<C::Backend>,
        account: &StoredAccount,
    ) -> Result<(), CoreError> {
        if connection.backend.is_none() {
            // Always read-only, whatever the account allows. Stage 14: a password or an
            // access token (renewed once if the server rejects it).
            let backend = self
                .credentials
                .with_login(&account.credential_ref, &account.imap, true, |login| {
                    let mut config = self.config.connect_config(&account.imap, login.secret);
                    config.auth = login.auth;
                    let connector = &self.connector;
                    async move { connector.connect(&config).await }
                })
                .await?;
            tracing::debug!(account = %account.id, "reading connection open");
            connection.backend = Some(backend);
        }
        connection.last_used = Some(Instant::now());
        Ok(())
    }

    /// Closes the account's reading connection (account removed, password changed).
    pub fn forget(&self, account: AccountId) {
        let slot = self
            .by_account
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&account);
        if let Some(slot) = slot {
            // Dropped without LOGOUT if it is busy; otherwise it logs out in the background.
            tokio::spawn(async move {
                let backend = slot.lock().await.backend.take();
                if let Some(backend) = backend
                    && let Err(err) = tokio::time::timeout(Duration::from_secs(2), backend.logout())
                        .await
                        .unwrap_or(Ok(()))
                {
                    tracing::debug!(error = %err, "LOGOUT of the reading connection failed");
                }
            });
        }
    }
}

impl<B: MailBackend> Held<B> {
    fn backend(&mut self) -> Result<&mut B, BackendError> {
        self.guard
            .backend
            .as_mut()
            .ok_or_else(|| BackendError::ConnectionLost("reading connection closed".into()))
    }

    /// Runs one step; a connection-level failure closes the connection (it may hold a
    /// half-read response), so the next use reconnects.
    fn checked<T>(&mut self, result: Result<T, BackendError>) -> Result<T, BackendError> {
        if let Err(err) = &result
            && !matches!(
                err,
                BackendError::Rejected(_) | BackendError::MailboxNotFound(_)
            )
        {
            self.guard.backend = None;
        }
        self.guard.last_used = Some(Instant::now());
        result
    }

    /// EXAMINE of the location's mailbox; `None` if the mailbox is gone or its UIDVALIDITY is
    /// not the stored one (the UID would mean another message).
    pub async fn open(
        &mut self,
        location: &MessageLocation,
    ) -> Result<Option<SelectedMailbox>, BackendError> {
        let opened = self.backend()?.open_mailbox(&location.mailbox_name).await;
        match self.checked(opened) {
            Ok(selected) if Some(selected.uid_validity) == location.uid_validity => {
                Ok(Some(selected))
            }
            Ok(selected) => {
                tracing::debug!(
                    mailbox = location.mailbox_id.0,
                    stored = location.uid_validity.map(|v| v.0),
                    server = selected.uid_validity.0,
                    "UIDVALIDITY changed; location skipped until the next sync"
                );
                Ok(None)
            }
            Err(BackendError::MailboxNotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<MessageStructure>, BackendError> {
        let result = self.backend()?.fetch_structure(mailbox, uid).await;
        self.checked(result)
    }

    pub async fn messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<mail_model::RawMessage>, BackendError> {
        let result = self.backend()?.fetch_messages(mailbox, uids).await;
        self.checked(result)
    }

    pub async fn sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
        let result = self.backend()?.fetch_sections(mailbox, uid, sections).await;
        self.checked(result)
    }

    /// `UID SEARCH` with criteria in the open mailbox (stage 7, search on the server).
    pub async fn search(
        &mut self,
        mailbox: &SelectedMailbox,
        criteria: &[mail_model::search::ServerCriterion],
    ) -> Result<UidSet, BackendError> {
        let result = self.backend()?.search_text(mailbox, criteria).await;
        self.checked(result)
    }
}

/// What a fetch got from the server.
pub(crate) enum Fetched {
    Whole(Vec<u8>),
    Sections(MessageStructure, Vec<FetchedSection>),
    /// No location has the message any more.
    NotOnServer,
}

/// Locations worth trying, Inbox, Sent and Drafts first (they are synced most often).
pub(crate) fn usable_locations(mut locations: Vec<MessageLocation>) -> Vec<MessageLocation> {
    use mail_model::MailboxRole;
    locations.retain(|l| l.selectable && l.uid_validity.is_some());
    locations.sort_by_key(|l| {
        let primary = matches!(
            l.role,
            MailboxRole::Inbox | MailboxRole::Sent | MailboxRole::Drafts
        );
        (!primary, l.mailbox_id)
    });
    locations
}

/// Fetches the body of one message as opening it needs: whole up to `limit` (RFC822.SIZE),
/// otherwise header and readable parts ([`MessageStructure::fetch_plan`]).
pub(crate) async fn fetch_body<C: MailConnector>(
    connections: &Connections<C>,
    store: &Store,
    account: &StoredAccount,
    message: MessageId,
    size: u32,
    limit: u32,
) -> Result<Fetched, CoreError> {
    let locations = usable_locations(store.message_locations(message).await?);
    if locations.is_empty() {
        return Ok(Fetched::NotOnServer);
    }
    let mut held = connections.hold(account).await?;
    match body_from(&mut held, &locations, size, limit).await {
        Err(err) if retry_on_new_connection(&err) => {
            connections.reconnect(&mut held, account, &err).await?;
            body_from(&mut held, &locations, size, limit).await
        }
        result => result,
    }
}

async fn body_from<B: MailBackend>(
    held: &mut Held<B>,
    locations: &[MessageLocation],
    size: u32,
    limit: u32,
) -> Result<Fetched, CoreError> {
    for location in locations {
        let Some(selected) = held.open(location).await? else {
            continue;
        };
        if size > limit {
            let Some(structure) = held.structure(&selected, location.uid).await? else {
                continue;
            };
            match structure.fetch_plan(limit) {
                BodyFetchPlan::Whole => {}
                BodyFetchPlan::Sections(sections) => {
                    let Some(fetched) = held.sections(&selected, location.uid, &sections).await?
                    else {
                        continue;
                    };
                    return Ok(Fetched::Sections(structure, fetched));
                }
            }
        }
        let raws = held
            .messages(&selected, &UidSet::from_uids([location.uid]))
            .await?;
        if let Some(raw) = raws.into_iter().find(|r| r.uid == location.uid) {
            return Ok(Fetched::Whole(raw.data));
        }
    }
    Ok(Fetched::NotOnServer)
}

/// Fetches more sections of a partial body (an attachment: `n.MIME` and `n`).
pub(crate) async fn fetch_sections<C: MailConnector>(
    connections: &Connections<C>,
    store: &Store,
    account: &StoredAccount,
    message: MessageId,
    sections: &[BodySection],
) -> Result<Option<Vec<FetchedSection>>, CoreError> {
    let locations = usable_locations(store.message_locations(message).await?);
    let mut held = connections.hold(account).await?;
    match sections_from(&mut held, &locations, sections).await {
        Err(err) if retry_on_new_connection(&err) => {
            connections.reconnect(&mut held, account, &err).await?;
            sections_from(&mut held, &locations, sections).await
        }
        result => result,
    }
}

async fn sections_from<B: MailBackend>(
    held: &mut Held<B>,
    locations: &[MessageLocation],
    sections: &[BodySection],
) -> Result<Option<Vec<FetchedSection>>, CoreError> {
    for location in locations {
        let Some(selected) = held.open(location).await? else {
            continue;
        };
        if let Some(fetched) = held.sections(&selected, location.uid, sections).await? {
            return Ok(Some(fetched));
        }
    }
    Ok(None)
}

/// Whether a failed fetch gets one more try on a new connection: the connection broke (closed,
/// reset, aborted: `ConnectionLost`; silent past the read limit: `Timeout`), as a reused
/// connection does after a server restart or a network change. Not after a refusal, a login,
/// certificate or TLS error, or a failure to connect: a new connection gets the same answer
/// (and repeated logins can lock an account).
fn retry_on_new_connection(err: &CoreError) -> bool {
    matches!(
        err,
        CoreError::Mail(BackendError::ConnectionLost(_) | BackendError::Timeout(_))
    )
}

/// The limit above which opening a message fetches it in parts.
pub(crate) const OPEN_LIMIT: u32 = LARGE_MESSAGE_THRESHOLD;
