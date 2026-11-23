use std::path::PathBuf;
use std::sync::{Arc, Weak};

use mail_imap::ImapConnector;
use mail_model::{
    Account, AccountId, AccountSettings, AccountStatus, ActionResult, AppInfo, BatchId, BodyFormat,
    Capabilities, ConnectionTestResult, CoreError, CoreEvent, DiscoveryResult, LinkInfo,
    ListCursor, MailAction, MailBackend, MailConnector, Mailbox, MailboxId, MailboxRole, MessageId,
    MessagePage, MessageRef, MessageView, OpenAttachmentResult, OperationId, OperationInfo,
    OutboxCounts, Platform, Preferences, PrefetchSettings, PrefetchStatus, ReadingPreferences,
    RemoteContentScope, RenderToken, Secret, ServerSettings, WriteConnector,
};
use mail_store::{GLOBAL_SCOPE, NewAccount, Store, StoreOptions, StoredAccount};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::clock::{Clock, SystemClock};
use crate::compose::Compose;
use crate::credentials::{CredentialRef, Credentials};
use crate::discovery;
use crate::outbox::{Outbox, OutboxConfig, SyncHook};
use crate::reading::{Reading, ReadingServices};
use crate::sync::account::{self, Control, Registry};
use crate::sync::{Env, OnConnected, SyncConfig, emit, wake};

/// File name of the database inside the app's data directory.
pub const DATABASE_FILE: &str = "mail.sqlite3";

const THEME_KEY: &str = "ui.theme";
const LANGUAGE_KEY: &str = "ui.language";
const MARK_READ_KEY: &str = "reading.mark_read_on_open";

/// Longest request id of an action (the UI sends a UUID).
const MAX_REQUEST_ID: usize = 64;

/// Events buffered per subscriber before a slow subscriber starts missing events.
const EVENT_CAPACITY: usize = 1024;

/// Facts about the build, supplied by the shell (it owns the package metadata and build script).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: String,
    pub commit: Option<String>,
    pub build_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Directory for the database and its backups (the app's data directory).
    pub data_dir: PathBuf,
    pub build: BuildInfo,
}

/// What the core needs from the mail adapter: read-only connections for the sync, IDLE and
/// reading ([`MailConnector`]), writer connections for the outbox ([`WriteConnector`]) and,
/// since stage 11, the settings sync connection that may write to its folder only
/// ([`mail_model::SyncFolderConnector`]).
pub trait Connector:
    MailConnector + WriteConnector + mail_model::SyncFolderConnector + 'static
{
}

impl<T: MailConnector + WriteConnector + mail_model::SyncFolderConnector + 'static> Connector
    for T
{
}

/// What the core is given instead of reaching for it: the keychain, the mail connector, the
/// clock and the sync tunables. Tests inject an in-memory keychain, a fake or instrumented
/// connector, a movable clock and short intervals.
pub struct CoreServices<C: MailConnector = ImapConnector> {
    pub credentials: Credentials,
    pub connector: C,
    pub clock: Arc<dyn Clock>,
    pub sync: SyncConfig,
    /// Opening links and attachments, external images, the directory for opened attachments
    /// (stage 3). Tests record instead of opening.
    pub reading: ReadingServices,
    /// Tunables of the outbox (stage 4); `None` derives them from `sync`
    /// ([`OutboxConfig::from_sync`]), as the product does.
    pub outbox: Option<OutboxConfig>,
}

impl CoreServices<ImapConnector> {
    /// The product services: the IMAP adapter, the system clock, the default sync settings.
    pub fn new(credentials: Credentials) -> Self {
        Self::with_connector(credentials, ImapConnector::new())
    }
}

impl<C: MailConnector> CoreServices<C> {
    pub fn with_connector(credentials: Credentials, connector: C) -> Self {
        Self {
            credentials,
            connector,
            clock: Arc::new(SystemClock),
            sync: SyncConfig::default(),
            reading: ReadingServices::default(),
            outbox: None,
        }
    }
}

/// A weak reference to the core, for background tasks (stage 9): they end once the core is
/// gone instead of keeping it (and the database) open.
pub(crate) struct WeakCore<C: Connector>(Weak<Inner<C>>);

impl<C: Connector> WeakCore<C> {
    pub(crate) fn upgrade(&self) -> Option<AppCore<C>> {
        self.0.upgrade().map(|inner| AppCore { inner })
    }
}

/// Facade of the core. The shell forwards every command here; everything is testable without
/// a WebView. Cheap to clone. Generic over the mail connector so tests can use a fake server.
pub struct AppCore<C: Connector = ImapConnector> {
    inner: Arc<Inner<C>>,
}

impl<C: Connector> Clone for AppCore<C> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<C: Connector> {
    info: AppInfo,
    /// The app's data directory (database, bodies, backups); stage 15 reports its size.
    data_dir: PathBuf,
    /// The store, or the error that kept it from opening (for example a database written by a
    /// newer app version). Storage-backed calls return that error, so the UI can explain it.
    store: Result<Store, CoreError>,
    events: broadcast::Sender<CoreEvent>,
    connector: Arc<C>,
    credentials: Credentials,
    clock: Arc<dyn Clock>,
    sync: Arc<SyncConfig>,
    accounts: Arc<Registry>,
    /// Serialises adding, removing and re-keying accounts (keychain, database and sync task
    /// change together).
    admin: tokio::sync::Mutex<()>,
    wake_detector: Option<JoinHandle<()>>,
    /// Reading messages (stage 3); `None` if the store did not open.
    reading: Option<Arc<Reading<C>>>,
    /// Writing actions (stage 4); `None` if the store did not open.
    outbox: Option<Outbox<C>>,
    /// Handed to every sync task: wakes the account's outbox when the sync connected.
    on_connected: OnConnected,
    /// Stage 7: the index job and the threading of old messages in the background.
    search: crate::search::SearchState,
    /// Stage 5: drafts, sending, contacts; `None` if the store did not open.
    compose: Option<Compose<C>>,
    /// Stage 5: submits messages and checks SMTP logins (account setup).
    sender: std::sync::Arc<dyn mail_model::MailSender>,
    /// Stage 9: categories, snoozes, rules and notifications in the background.
    comfort: crate::comfort::ComfortState,
    /// Stage 12: AI access over MCP (host, settings, access log).
    mcp: crate::mcp::McpState<C>,
    /// Stage 11: the settings sync (capture hook, clock, worker, folder connection).
    settings_sync: crate::settings_sync::service::SettingsSync<C>,
    /// Stage 13: AI in the app (running requests, settings changes).
    ai: crate::ai::AiState,
    /// Stage 15 (decision 0037): in-app updates (source, state, background task).
    updates: crate::updates::UpdatesState,
}

impl<C: Connector> Drop for Inner<C> {
    /// Without an orderly [`AppCore::shutdown`] the sync and outbox tasks are aborted:
    /// connections close without LOGOUT, stored data is consistent anyway (one transaction per
    /// window; in-flight operations are checked at the next start).
    fn drop(&mut self) {
        if let Some(task) = &self.wake_detector {
            task.abort();
        }
        self.accounts.abort_all();
        if let Some(outbox) = &self.outbox {
            outbox.abort_all();
        }
        if let Some(compose) = &self.compose {
            compose.shutdown();
        }
        self.comfort.stop();
        self.mcp.abort();
        self.settings_sync.stop();
        self.ai.cancel_all();
        self.updates.stop();
    }
}

/// The outbox's [`SyncHook`]: asks the account's sync for a pass over the mailboxes the outbox
/// just changed on the server, so the sync confirms the change at once.
struct CoreSyncHook {
    accounts: Weak<Registry>,
}

impl SyncHook for CoreSyncHook {
    fn sync_requested(&self, account: AccountId, mailboxes: &[MailboxId]) {
        if let Some(accounts) = self.accounts.upgrade()
            && !accounts.send(account, Control::SyncMailboxes(mailboxes.to_vec()))
        {
            tracing::debug!(%account, "sync requested for an account without a sync task");
        }
    }
}

impl<C: Connector> Inner<C> {
    fn stop_reading(&self) {
        if let Some(reading) = &self.reading {
            reading.shutdown();
        }
    }
}

impl<C: Connector> AppCore<C> {
    /// Opens the database in `config.data_dir`, starts the core and the sync of every stored
    /// account.
    ///
    /// Never fails: if the database cannot be opened, the core still starts, logs the error,
    /// reports it through [`AppCore::startup_error`] and returns it from every call that needs
    /// storage. That way the UI can show a clear message instead of the app vanishing.
    pub async fn start(config: CoreConfig, services: CoreServices<C>) -> Self {
        let path = config.data_dir.join(DATABASE_FILE);
        let opened =
            tokio::task::spawn_blocking(move || Store::open(&path, StoreOptions::default()))
                .await
                .map_err(|err| CoreError::Internal(format!("database open task failed: {err}")))
                .and_then(|result| result.map_err(CoreError::from));

        match &opened {
            Ok(store) => {
                let report = store.open_report();
                tracing::info!(
                    schema_version = report.version,
                    migrated_from = report.previous_version,
                    backup = report.backup.as_ref().map(|p| p.display().to_string()),
                    "database ready"
                );
            }
            Err(err) => {
                tracing::error!(code = ?err.code(), error = %err, "database could not be opened");
            }
        }

        // Stage 11 first: the capture hook must be set before anything writes.
        let settings_sync = crate::settings_sync::service::SettingsSync::new(
            opened.as_ref().ok(),
            Arc::clone(&services.clock),
        )
        .await;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let accounts = Arc::new(Registry::default());
        let sync = Arc::new(services.sync);
        let connector = Arc::new(services.connector);
        // Stage 9: the one-click unsubscribe client trusts what the sync trusts (the test CA in
        // tests) and reaches loopback only where external images may (tests).
        let comfort = crate::comfort::ComfortState::new(
            opened.as_ref().ok(),
            sync.extra_root_ca.as_ref(),
            services.reading.remote.allow_loopback,
        );
        let reading = opened.as_ref().ok().map(|store| {
            Reading::start(
                store.clone(),
                events.clone(),
                Arc::clone(&connector),
                services.credentials.clone(),
                Arc::clone(&sync),
                Arc::clone(&accounts),
                services.reading,
                &config.data_dir,
            )
        });
        let outbox_config = services
            .outbox
            .unwrap_or_else(|| OutboxConfig::from_sync(&sync));
        let sender = Arc::clone(&outbox_config.sender.0);
        let outbox = opened.as_ref().ok().map(|store| {
            let config = outbox_config.clone();
            let outbox = Outbox::with_clock(
                store.clone(),
                Arc::clone(&connector),
                services.credentials.clone(),
                config,
                Arc::clone(&services.clock),
            );
            outbox.forward_to(events.clone());
            outbox.set_sync_hook(Arc::new(CoreSyncHook {
                accounts: Arc::downgrade(&accounts),
            }));
            outbox
        });
        let on_connected: OnConnected = match &outbox {
            Some(outbox) => {
                let outbox = outbox.clone();
                Arc::new(move |account| outbox.wake(account))
            }
            None => Arc::new(|_| {}),
        };
        let wake_detector = Some(wake::spawn(
            Arc::downgrade(&accounts),
            Arc::clone(&services.clock),
            sync.wake_check,
            sync.wake_threshold,
        ));
        let search = crate::search::SearchState::start(opened.as_ref().ok()).await;
        let compose = match (opened.as_ref().ok(), &outbox, &reading) {
            (Some(store), Some(outbox), Some(reading)) => Some(Compose::new(
                store.clone(),
                outbox.clone(),
                Arc::clone(reading),
                events.clone(),
                Arc::clone(&services.clock),
            )),
            _ => None,
        };
        let core = Self {
            inner: Arc::new(Inner {
                info: AppInfo {
                    version: config.build.version,
                    commit: config.build.commit,
                    build_date: config.build.build_date,
                    platform: Platform::current(),
                },
                data_dir: config.data_dir.clone(),
                store: opened,
                events,
                connector,
                credentials: services.credentials,
                clock: services.clock,
                sync,
                accounts,
                admin: tokio::sync::Mutex::new(()),
                wake_detector,
                reading,
                outbox,
                on_connected,
                search,
                compose,
                sender,
                comfort,
                mcp: crate::mcp::McpState::new(config.data_dir.clone()),
                settings_sync,
                ai: crate::ai::AiState::default(),
                updates: crate::updates::UpdatesState::default(),
            }),
        };
        if let Some(reading) = &core.inner.reading {
            reading.clear_open_dir().await;
        }
        // Stage 9 before the sync starts, so no arrival is missed.
        let categories_pending = crate::comfort::start(&core).await;
        // Stage 10: scheduled sends missed while the app was off are asked about, not sent;
        // before anything can start the outbox (a connected account wakes it).
        if let Some(compose) = &core.inner.compose {
            compose.check_missed_sends().await;
        }
        core.start_stored_accounts().await;
        if let Some(outbox) = &core.inner.outbox
            && let Err(err) = outbox.start().await
        {
            // The queue stays stored; the next action or wake starts the worker.
            tracing::error!(code = ?err.code(), error = %err, "the outbox could not resume");
        }
        if let Some(compose) = &core.inner.compose {
            compose.start().await;
        }
        // Stage 12: the MCP host (it listens only while AI access is on).
        crate::mcp::start(&core).await;
        // Stage 11: the settings sync's worker (it only connects while the sync is on).
        crate::settings_sync::service::start(&core);
        // Stages 7 and 9: index, thread and category work found at start begins now, after
        // the start's own writes (decision 0029).
        core.inner.search.started();
        if categories_pending {
            core.inner.comfort.kick_categories();
        }
        core
    }

    async fn start_stored_accounts(&self) {
        let Ok(store) = self.store() else { return };
        match store.accounts().await {
            Ok(accounts) => {
                for stored in accounts {
                    self.spawn_sync(stored, None);
                }
            }
            Err(err) => {
                tracing::error!(error = %err, "stored accounts could not be read; no sync started");
            }
        }
    }

    pub fn app_info(&self) -> AppInfo {
        self.inner.info.clone()
    }

    /// The error that kept the database from opening, if any.
    pub fn startup_error(&self) -> Option<&CoreError> {
        self.inner.store.as_ref().err()
    }

    /// Subscribes to the event stream. Events sent before subscribing are not replayed.
    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.inner.events.subscribe()
    }

    // -----------------------------------------------------------------------------------------
    // Preferences
    // -----------------------------------------------------------------------------------------

    pub async fn get_preferences(&self) -> Result<Preferences, CoreError> {
        let settings = self.store()?.settings(GLOBAL_SCOPE).await?;
        let mut prefs = Preferences::default();
        for setting in settings {
            match setting.key.as_str() {
                THEME_KEY => prefs.theme = decode(&setting.key, &setting.value),
                LANGUAGE_KEY => prefs.language = decode(&setting.key, &setting.value),
                _ => {}
            }
        }
        Ok(prefs)
    }

    /// Stores `preferences`, then emits [`CoreEvent::PreferencesChanged`] and returns them.
    pub async fn set_preferences(
        &self,
        preferences: Preferences,
    ) -> Result<Preferences, CoreError> {
        let entries = vec![
            (THEME_KEY.to_owned(), encode(&preferences.theme)?),
            (LANGUAGE_KEY.to_owned(), encode(&preferences.language)?),
        ];
        self.store()?.put_settings(GLOBAL_SCOPE, entries).await?;
        tracing::info!(theme = ?preferences.theme, language = ?preferences.language, "preferences stored");
        self.emit(CoreEvent::PreferencesChanged { preferences });
        Ok(preferences)
    }

    /// Reading preferences (stage 4): when an opened message is marked as read. Defaults when
    /// nothing is stored.
    pub async fn get_reading_preferences(&self) -> Result<ReadingPreferences, CoreError> {
        let settings = self.store()?.settings(GLOBAL_SCOPE).await?;
        let mut prefs = ReadingPreferences::default();
        for setting in settings {
            if setting.key == MARK_READ_KEY {
                prefs.mark_read_on_open = decode(&setting.key, &setting.value);
            }
        }
        Ok(prefs)
    }

    /// Stores the reading preferences and returns them.
    pub async fn set_reading_preferences(
        &self,
        preferences: ReadingPreferences,
    ) -> Result<ReadingPreferences, CoreError> {
        let entries = vec![(
            MARK_READ_KEY.to_owned(),
            encode(&preferences.mark_read_on_open)?,
        )];
        self.store()?.put_settings(GLOBAL_SCOPE, entries).await?;
        tracing::info!(mark_read_on_open = ?preferences.mark_read_on_open, "reading preferences stored");
        Ok(preferences)
    }

    // -----------------------------------------------------------------------------------------
    // Accounts
    // -----------------------------------------------------------------------------------------

    /// All accounts, oldest first, with their live sync status.
    pub async fn list_accounts(&self) -> Result<Vec<Account>, CoreError> {
        let stored = self.store()?.accounts().await?;
        Ok(stored
            .into_iter()
            .map(|account| {
                let status = self.status_of(account.id);
                account.into_account(status)
            })
            .collect())
    }

    /// Server settings from the built-in provider table. No network.
    pub fn discover_known_provider(&self, email: &str) -> Result<DiscoveryResult, CoreError> {
        discovery::discover_known(email)
    }

    /// DNS SRV, then the domain's autoconfig file. Only after the user asked for it.
    pub async fn discover_online(&self, email: &str) -> Result<DiscoveryResult, CoreError> {
        discovery::discover_online(email).await
    }

    /// Validates the settings, logs in to IMAP read-only and out again, then to SMTP (log in
    /// and say goodbye; nothing is sent). Stores nothing. An SMTP failure carries `SMTP` in
    /// its detail, so the UI can tell which server it was.
    pub async fn test_connection(
        &self,
        settings: AccountSettings,
        password: Secret<String>,
    ) -> Result<ConnectionTestResult, CoreError> {
        let settings = settings.validated()?;
        let capabilities = self.check_login(&settings.imap, password.clone()).await?;
        self.check_smtp(&settings.smtp, password).await?;
        Ok(ConnectionTestResult { capabilities })
    }

    /// Adds an account: same checks as [`AppCore::test_connection`] (a wrong password is never
    /// stored), password to the keychain, account row with `writes_enabled = false`, then its
    /// sync starts. Emits `accountsChanged`.
    pub async fn add_account(
        &self,
        settings: AccountSettings,
        password: Secret<String>,
    ) -> Result<Account, CoreError> {
        let settings = settings.validated()?;
        let store = self.store()?;
        if store
            .accounts()
            .await?
            .iter()
            .any(|a| a.email.eq_ignore_ascii_case(&settings.email))
        {
            return Err(CoreError::AccountExists(settings.email));
        }
        self.check_login(&settings.imap, password.clone()).await?;

        let _admin = self.inner.admin.lock().await;
        let id = AccountId::from_uuid(uuid::Uuid::new_v4());
        let key = CredentialRef::for_account(id);
        self.inner.credentials.set(&key, password.clone()).await?;
        let new = NewAccount {
            id,
            email: settings.email,
            display_name: settings.display_name,
            imap: settings.imap,
            smtp: settings.smtp,
            credential_ref: key.to_string(),
        };
        let stored = match store.insert_account(new).await {
            Ok(stored) => stored,
            Err(err) => {
                if let Err(cleanup) = self.inner.credentials.delete(&key).await {
                    tracing::error!(account = %id, error = %cleanup, "keychain entry of a failed account insert could not be removed");
                }
                return Err(err.into());
            }
        };
        tracing::info!(account = %id, "account added");
        self.spawn_sync(stored.clone(), Some(password));
        self.emit(CoreEvent::AccountsChanged);
        Ok(stored.into_account(AccountStatus::connecting()))
    }

    /// Stops the account's sync, deletes its keychain entry (a missing one is fine) and all its
    /// local data. Emits `accountsChanged`. If the keychain refuses, nothing is deleted and the
    /// sync starts again.
    pub async fn remove_account(&self, id: AccountId) -> Result<(), CoreError> {
        let store = self.store()?;
        let _admin = self.inner.admin.lock().await;
        let account = store
            .account(id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {id}")))?;
        if let Some(handle) = self.inner.accounts.remove(id) {
            handle.stop(self.inner.sync.shutdown_grace).await;
        }
        if let Some(outbox) = &self.inner.outbox {
            // Unfinished operations go with the account's rows; nothing is sent any more.
            outbox.stop_account(id).await;
        }
        if let Some(reading) = &self.inner.reading {
            reading.forget_account(id);
        }
        let key = CredentialRef::parse(&account.credential_ref)?;
        if let Err(err) = self.inner.credentials.delete(&key).await {
            self.spawn_sync(account, None);
            return Err(err);
        }
        if let Err(err) = store.delete_account(id).await {
            // The password is gone; the account shows that until it is removed again.
            self.spawn_sync(account, None);
            return Err(err.into());
        }
        tracing::info!(account = %id, "account removed");
        // Stage 12: its AI access went with it (`mcp_account` cascades).
        if let Err(err) = self.mcp_reload().await {
            tracing::warn!(error = %err, "MCP settings could not be republished");
        }
        // Stage 11: removing the sync account turns the settings sync off. Not under the admin
        // lock: a settings pass may be waiting for it (it restarts accounts it changed).
        drop(_admin);
        self.sync_account_removed(id).await;
        self.emit(CoreEvent::AccountsChanged);
        Ok(())
    }

    /// Reconnects now: cuts a backoff short and leaves the `error` state.
    pub async fn retry_account(&self, id: AccountId) -> Result<(), CoreError> {
        self.store()?;
        if self.inner.accounts.send(id, Control::RetryNow) {
            Ok(())
        } else {
            Err(CoreError::NotFound(format!("account {id}")))
        }
    }

    /// Checks the login with the new password, replaces the keychain entry and reconnects.
    pub async fn update_password(
        &self,
        id: AccountId,
        password: Secret<String>,
    ) -> Result<(), CoreError> {
        let store = self.store()?;
        let account = store
            .account(id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {id}")))?;
        self.check_login(&account.imap, password.clone()).await?;
        let _admin = self.inner.admin.lock().await;
        let key = CredentialRef::parse(&account.credential_ref)?;
        self.inner.credentials.set(&key, password.clone()).await?;
        if let Some(reading) = &self.inner.reading {
            reading.credentials_changed(id);
        }
        if !self
            .inner
            .accounts
            .send(id, Control::CredentialsChanged(password.clone()))
        {
            self.spawn_sync(account, Some(password));
        }
        if let Some(outbox) = &self.inner.outbox {
            // It reads the new password from the keychain on its next connect.
            outbox.wake(id);
        }
        // Stage 11: an account taken over from another device has its password now.
        if let Err(err) = store.clear_sync_adopted(id).await {
            tracing::warn!(account = %id, error = %err, "adopted mark not cleared");
        }
        tracing::info!(account = %id, "password replaced");
        Ok(())
    }

    /// Turns writing on or off for the account (the user's switch in the settings; new
    /// accounts start off). Off fails the queued operations with `read_only` and reverts them.
    /// The sync stays read-only either way. Emits `accountsChanged` and returns the account.
    pub async fn set_writes_enabled(
        &self,
        id: AccountId,
        enabled: bool,
    ) -> Result<Account, CoreError> {
        let store = self.store()?;
        if store.account(id).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {id}")));
        }
        self.outbox()?.set_writes_enabled(id, enabled).await?;
        let account = store
            .account(id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {id}")))?;
        self.emit(CoreEvent::AccountsChanged);
        Ok(account.into_account(self.status_of(id)))
    }

    // -----------------------------------------------------------------------------------------
    // Mailboxes and messages
    // -----------------------------------------------------------------------------------------

    /// Mailboxes of an account, ordered by path.
    pub async fn list_mailboxes(&self, account: AccountId) -> Result<Vec<Mailbox>, CoreError> {
        let store = self.store()?;
        if store.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        Ok(store.mailboxes(account).await?)
    }

    /// Sets a role chosen by the user; returns all mailboxes of the account and emits
    /// `mailboxesChanged`.
    pub async fn set_mailbox_role(
        &self,
        mailbox: MailboxId,
        role: MailboxRole,
    ) -> Result<Vec<Mailbox>, CoreError> {
        let mailboxes = self.store()?.set_mailbox_role(mailbox, role).await?;
        if let Some(first) = mailboxes.first() {
            self.emit(CoreEvent::MailboxesChanged {
                account_id: first.account_id,
            });
        }
        Ok(mailboxes)
    }

    /// One keyset page of a mailbox, newest first. `limit` 1..=500.
    pub async fn list_messages(
        &self,
        mailbox: MailboxId,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, CoreError> {
        Ok(self.store()?.list_messages(mailbox, cursor, limit).await?)
    }

    /// Stage 13 (decision 0027): the list rows of `ids` in the order asked (the AI panel names
    /// and opens the sources of an answer). Unknown ids are left out; at most
    /// [`mail_model::MAX_MESSAGES_BY_IDS`] ids, else `invalid_argument`.
    pub async fn get_messages_by_ids(
        &self,
        ids: Vec<MessageId>,
    ) -> Result<Vec<mail_model::MessageListItem>, CoreError> {
        Ok(self.store()?.message_items(ids).await?)
    }

    // -----------------------------------------------------------------------------------------
    // Actions and the outbox (stage 4, see `crate::outbox`)
    // -----------------------------------------------------------------------------------------

    /// Applies `action` to the messages (one account, at most
    /// [`mail_model::MAX_ACTION_MESSAGES`]): the local state changes at once, the operations
    /// are recorded and sent to the server in order. `request_id` names this user action (a
    /// UUID): the same id twice changes nothing. Returns the batch for
    /// [`AppCore::undo_action`]. Events: `operationsChanged`, `messagesChanged`,
    /// `mailboxesChanged`; later the operations' new states.
    ///
    /// Refused before anything changes: `read_only` (writes off for the account),
    /// `missing_mailbox` (no archive, trash or junk folder), `confirmation_required` (delete in
    /// the trash: send `DeletePermanently`), `invalid_keyword`, `not_found` (the list was out
    /// of date), `invalid_argument`.
    pub async fn apply_action(
        &self,
        action: MailAction,
        messages: Vec<MessageRef>,
        request_id: String,
    ) -> Result<ActionResult, CoreError> {
        let outbox = self.outbox()?;
        if request_id.is_empty()
            || request_id.len() > MAX_REQUEST_ID
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(CoreError::InvalidArgument(
                "the request id must be 1 to 64 letters, digits, '-' or '_'".into(),
            ));
        }
        outbox.apply_request(action, messages, request_id).await
    }

    /// Undoes a batch (the notice's "Undo"): cancels what is still queued, sends
    /// counter-operations for the rest. `not_undoable` if a part cannot be undone; then
    /// nothing changes.
    pub async fn undo_action(&self, batch: BatchId) -> Result<ActionResult, CoreError> {
        self.outbox()?.undo(batch).await
    }

    /// Queues the creation of a folder (decoded path, the server's delimiter) that gets `role`,
    /// for example `archive` after `missing_mailbox`. It appears in the folder list once the
    /// server created it (`mailboxesChanged`).
    pub async fn create_mailbox(
        &self,
        account: AccountId,
        path: String,
        role: Option<MailboxRole>,
    ) -> Result<ActionResult, CoreError> {
        self.outbox()?.create_mailbox(account, path, role).await
    }

    /// The account's operations, newest first (activity view), keyset-paged below `before`.
    /// `limit` 1..=500.
    pub async fn list_operations(
        &self,
        account: AccountId,
        before: Option<OperationId>,
        limit: u32,
    ) -> Result<Vec<OperationInfo>, CoreError> {
        self.outbox()?.operations(account, before, limit).await
    }

    /// Queues a failed or conflicting operation again (activity view, "Retry").
    pub async fn retry_operation(&self, id: OperationId) -> Result<ActionResult, CoreError> {
        self.outbox()?.retry(id).await
    }

    /// Removes a finished operation from the activity view ("Discard").
    pub async fn discard_operation(&self, id: OperationId) -> Result<(), CoreError> {
        self.outbox()?.discard(id).await
    }

    /// How many of the account's operations are pending, failed or in conflict.
    pub async fn outbox_counts(&self, account: AccountId) -> Result<OutboxCounts, CoreError> {
        self.outbox()?.counts(account).await
    }

    // -----------------------------------------------------------------------------------------
    // Reading (stage 3, see `crate::reading`)
    // -----------------------------------------------------------------------------------------

    /// The reading pane's data. Served from the stored body when there is one; otherwise
    /// `loading` and a fetch starts (`messageBodyAvailable`/`messageBodyFailed` follow), or
    /// `unavailable` offline.
    pub async fn get_message(
        &self,
        message: MessageId,
        format: BodyFormat,
    ) -> Result<MessageView, CoreError> {
        self.reading()?.get_message(message, format).await
    }

    /// Renders again with the external images, loaded by the core; `sender` also allows them
    /// for the sender from now on.
    pub async fn load_remote_content(
        &self,
        message: MessageId,
        scope: RemoteContentScope,
    ) -> Result<MessageView, CoreError> {
        self.reading()?.load_remote_content(message, scope).await
    }

    /// Senders of the account whose external content always loads, sorted.
    pub async fn list_remote_content_senders(
        &self,
        account: AccountId,
    ) -> Result<Vec<String>, CoreError> {
        let store = self.store()?;
        if store.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        Ok(store.remote_content_senders(account).await?)
    }

    /// Takes a sender off the account's allowlist (nothing happens if it was not on it).
    pub async fn revoke_remote_content(
        &self,
        account: AccountId,
        sender: &str,
    ) -> Result<(), CoreError> {
        self.store()?.revoke_remote_content(account, sender).await?;
        Ok(())
    }

    /// The real target of link `index` of a rendering, with warnings; `not_found` for a token
    /// that is unknown or no longer kept.
    pub fn describe_link(&self, token: &RenderToken, index: u32) -> Result<LinkInfo, CoreError> {
        self.reading()?.describe_link(token, index)
    }

    /// Opens link `index` of a rendering with the system (`http`, `https`); a `mailto:` link
    /// comes back as a compose request for the app's own composer (stage 10). Anything else is
    /// `invalid_argument`.
    pub async fn open_link(
        &self,
        token: &RenderToken,
        index: u32,
    ) -> Result<mail_model::LinkOpened, CoreError> {
        self.reading()?.open_link(token, index).await
    }

    /// The shell's navigation hook saw a click on a link placeholder: emits `linkActivated` if
    /// the rendering is known. Returns whether it was.
    pub fn link_activated(&self, token: RenderToken, index: u32) -> bool {
        self.reading()
            .is_ok_and(|reading| reading.link_activated(token, index))
    }

    /// The sanitised name of an attachment, as the save dialog suggests it.
    pub async fn attachment_name(
        &self,
        message: MessageId,
        index: u32,
    ) -> Result<String, CoreError> {
        self.reading()?.attachment_name(message, index).await
    }

    /// Writes an attachment to `path`, which the user chose in the system dialog (atomically;
    /// fetched first if only part of the message is stored).
    pub async fn save_attachment_to(
        &self,
        message: MessageId,
        index: u32,
        path: PathBuf,
    ) -> Result<PathBuf, CoreError> {
        self.reading()?
            .save_attachment_to(message, index, path)
            .await
    }

    /// Stage 16 (decision 0036): the preview of an attachment (`page` of a PDF, 0-based):
    /// re-encoded images, rasterised PDF pages or plain text, decoded by the core. Typed
    /// errors `preview_unsupported`, `preview_too_large`, `preview_damaged`.
    pub async fn preview_attachment(
        &self,
        message: MessageId,
        index: u32,
        page: u32,
    ) -> Result<mail_model::AttachmentPreview, CoreError> {
        self.reading()?
            .preview_attachment(message, index, page)
            .await
    }

    /// Stage 16 (decision 0036): every attachment into `folder`, which the user chose in the
    /// system dialog; how many were written, and how many of them under another name (an
    /// existing file is never replaced).
    pub async fn save_all_attachments_to(
        &self,
        message: MessageId,
        folder: PathBuf,
    ) -> Result<(u32, u32), CoreError> {
        self.reading()?
            .save_all_attachments_to(message, folder)
            .await
    }

    /// Opens a read-only copy of an attachment with the system's default app. Executable types
    /// answer `needsConfirmation` unless `confirmed_executable`; nothing is written then.
    pub async fn open_attachment(
        &self,
        message: MessageId,
        index: u32,
        confirmed_executable: bool,
    ) -> Result<OpenAttachmentResult, CoreError> {
        self.reading()?
            .open_attachment(message, index, confirmed_executable)
            .await
    }

    pub async fn get_prefetch_settings(&self) -> Result<PrefetchSettings, CoreError> {
        Ok(self.store()?.prefetch_settings().await?)
    }

    /// Stores the prefetch settings (`invalid_argument` for a size limit above 256 MiB) and
    /// starts a new prefetch run for every account.
    pub async fn set_prefetch_settings(
        &self,
        settings: PrefetchSettings,
    ) -> Result<PrefetchSettings, CoreError> {
        self.reading()?.set_prefetch_settings(settings).await
    }

    /// The prefetch state of an account; `prefetchProgress` events follow.
    pub async fn get_prefetch_status(
        &self,
        account: AccountId,
    ) -> Result<PrefetchStatus, CoreError> {
        let reading = self.reading()?;
        if self.store()?.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        Ok(reading.prefetch_status(account))
    }

    // -----------------------------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------------------------

    /// The network changed (new Wi-Fi, VPN up or down): every account reconnects now instead
    /// of waiting for its connections to time out. For a shell with an OS signal.
    pub fn network_changed(&self) {
        tracing::info!("network changed; reconnecting all accounts");
        self.inner.accounts.wake_all();
        self.wake_outbox();
    }

    /// The computer woke from sleep: like [`AppCore::network_changed`]. The core also notices
    /// sleep by itself (`sync::wake`).
    pub fn resume(&self) {
        tracing::info!("resumed; reconnecting all accounts");
        self.inner.accounts.wake_all();
        self.wake_outbox();
        // Stage 9: snoozes due while the computer slept come back now.
        self.inner.comfort.wake_snoozes();
    }

    /// Stops every sync task and the outbox; connections get a moment to log out. Call when the
    /// app quits. Operations still in flight are checked against the server at the next start.
    pub async fn shutdown(&self) {
        // First the MCP host: no AI client may start a tool call while the rest stops.
        crate::mcp::shutdown(&self.inner.mcp).await;
        self.inner.ai.cancel_all();
        self.inner.updates.stop();
        let grace = self.inner.sync.shutdown_grace;
        let handles = self.inner.accounts.take_all();
        let mut stopping = tokio::task::JoinSet::new();
        for handle in handles {
            stopping.spawn(handle.stop(grace));
        }
        if let Some(outbox) = self.inner.outbox.clone() {
            stopping.spawn(async move { outbox.shutdown().await });
        }
        if let Some(compose) = &self.inner.compose {
            compose.shutdown();
        }
        while stopping.join_next().await.is_some() {}
        if let Some(task) = &self.inner.wake_detector {
            task.abort();
        }
        self.inner.stop_reading();
        self.inner.search.stop();
        self.inner.comfort.stop();
        self.inner.settings_sync.stop();
        if let Some(folder) = self.inner.settings_sync.take_connection().await {
            use mail_model::SyncFolder;
            if let Err(err) = folder.logout().await {
                tracing::debug!(error = %err, "LOGOUT of the settings sync connection failed");
            }
        }
        tracing::info!("sync stopped");
    }

    // -----------------------------------------------------------------------------------------

    fn status_of(&self, id: AccountId) -> AccountStatus {
        self.inner
            .accounts
            .status(id)
            .unwrap_or_else(AccountStatus::connecting)
    }

    /// The outbox of every account retries now (the network may be back).
    fn wake_outbox(&self) {
        if let Some(outbox) = &self.inner.outbox {
            outbox.wake_all();
        }
    }

    fn spawn_sync(&self, account: StoredAccount, password: Option<Secret<String>>) {
        let Ok(store) = self.store() else { return };
        let env = Env {
            store: store.clone(),
            connector: Arc::clone(&self.inner.connector),
            credentials: self.inner.credentials.clone(),
            clock: Arc::clone(&self.inner.clock),
            config: Arc::clone(&self.inner.sync),
            events: self.inner.events.clone(),
            on_connected: Arc::clone(&self.inner.on_connected),
            on_arrival: self.inner.comfort.on_arrival(),
            sync_signals: Arc::clone(&self.inner.settings_sync.signals),
        };
        let id = account.id;
        let handle = account::spawn(env, account, password);
        self.inner.accounts.insert(id, handle);
    }

    /// Logs in read-only and out again. Runs in its own task, so a panic in the adapter is a
    /// `protocol` error for this call, not a hanging command.
    async fn check_login(
        &self,
        server: &ServerSettings,
        password: Secret<String>,
    ) -> Result<Capabilities, CoreError> {
        // Always read-only, whatever the account allows later.
        let config = self.inner.sync.connect_config(server, password);
        let connector = Arc::clone(&self.inner.connector);
        let task = tokio::spawn(async move {
            let backend = connector.connect(&config).await?;
            let capabilities = backend.capabilities().clone();
            if let Err(err) = backend.logout().await {
                tracing::debug!(error = %err, "LOGOUT after the login check failed");
            }
            Ok::<_, CoreError>(capabilities)
        });
        match task.await {
            Ok(result) => result,
            Err(err) => Err(CoreError::Mail(mail_model::BackendError::Protocol(
                format!("the login check failed unexpectedly: {err}"),
            ))),
        }
    }

    /// SMTP login check of the account setup (decision 0007: added in stage 5). Own task, as
    /// for IMAP.
    async fn check_smtp(
        &self,
        server: &ServerSettings,
        password: Secret<String>,
    ) -> Result<(), CoreError> {
        let mut config =
            mail_model::ConnectConfig::new(server, password, mail_model::Access::ReadOnly);
        config.timeouts = self.inner.sync.timeouts;
        config.extra_root_ca = self.inner.sync.extra_root_ca.clone();
        let sender = Arc::clone(&self.inner.sender);
        let task = tokio::spawn(async move { sender.verify(&config).await });
        match task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(CoreError::Mail(smtp_detail(err))),
            Err(err) => Err(CoreError::Mail(mail_model::BackendError::Protocol(
                format!("SMTP: the login check failed unexpectedly: {err}"),
            ))),
        }
    }

    fn store(&self) -> Result<&Store, CoreError> {
        self.inner.store.as_ref().map_err(Clone::clone)
    }

    fn compose(&self) -> Result<&Compose<C>, CoreError> {
        self.store()?;
        self.inner
            .compose
            .as_ref()
            .ok_or_else(|| CoreError::Internal("composing is not available".into()))
    }

    fn outbox(&self) -> Result<&Outbox<C>, CoreError> {
        self.store()?;
        self.inner
            .outbox
            .as_ref()
            .ok_or_else(|| CoreError::Internal("the outbox is not available".into()))
    }

    fn reading(&self) -> Result<&Arc<Reading<C>>, CoreError> {
        self.store()?;
        self.inner
            .reading
            .as_ref()
            .ok_or_else(|| CoreError::Internal("reading is not available".into()))
    }

    /// For tests inside the crate: the store.
    #[cfg(test)]
    pub(crate) fn store_for_tests(&self) -> &Store {
        self.inner.store.as_ref().expect("store")
    }

    fn emit(&self, event: CoreEvent) {
        emit(&self.inner.events, event);
    }

    // Stage 7 (`crate::search`) works through these.

    pub(crate) fn store_ref(&self) -> Result<&Store, CoreError> {
        self.store()
    }

    pub(crate) fn reading_ref(&self) -> Result<&Arc<Reading<C>>, CoreError> {
        self.reading()
    }

    /// Stage 16 (`crate::parity`): transfers between accounts record through the outbox.
    pub(crate) fn outbox_ref(&self) -> Result<&Outbox<C>, CoreError> {
        self.outbox()
    }

    /// Stage 15: the data directory the core was started with.
    pub(crate) fn data_dir(&self) -> &std::path::Path {
        &self.inner.data_dir
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.inner.clock.now_ms()
    }

    pub(crate) fn emit_event(&self, event: CoreEvent) {
        self.emit(event);
    }

    pub(crate) fn search_state(&self) -> &crate::search::SearchState {
        &self.inner.search
    }

    // Stage 12 (`crate::mcp`) works through these.

    pub(crate) fn ai(&self) -> &crate::ai::AiState {
        &self.inner.ai
    }

    pub(crate) fn mcp(&self) -> &crate::mcp::McpState<C> {
        &self.inner.mcp
    }

    pub(crate) fn compose_ref(&self) -> Result<&Compose<C>, CoreError> {
        self.compose()
    }

    pub(crate) fn credentials_ref(&self) -> &Credentials {
        &self.inner.credentials
    }

    // Stage 11 (`crate::settings_sync::service`) works through these.

    pub(crate) fn settings_sync(&self) -> &crate::settings_sync::service::SettingsSync<C> {
        &self.inner.settings_sync
    }

    pub(crate) fn sync_config(&self) -> Arc<SyncConfig> {
        Arc::clone(&self.inner.sync)
    }

    pub(crate) fn connector_ref(&self) -> &Arc<C> {
        &self.inner.connector
    }

    pub(crate) fn wall_clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.inner.clock)
    }

    /// An account taken over from another device: its sync starts (and waits for the
    /// password), and the UI hears of it.
    pub(crate) async fn start_synced_account(&self, id: AccountId) {
        let Ok(store) = self.store() else { return };
        match store.account(id).await {
            Ok(Some(account)) => {
                tracing::info!(account = %id, "account taken over from another device");
                self.spawn_sync(account, None);
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(account = %id, error = %err, "adopted account unreadable"),
        }
    }

    /// The account's settings changed through the settings sync: its sync reconnects with them.
    pub(crate) async fn restart_account_sync(&self, id: AccountId) {
        let Ok(store) = self.store() else { return };
        let Ok(Some(account)) = store.account(id).await else {
            return;
        };
        let _admin = self.inner.admin.lock().await;
        if let Some(handle) = self.inner.accounts.remove(id) {
            handle.stop(self.inner.sync.shutdown_grace).await;
        }
        if let Some(reading) = &self.inner.reading {
            reading.credentials_changed(id);
        }
        self.spawn_sync(account, None);
    }

    /// A local account took the id of the same account in the settings sync folder: its tasks
    /// run under the new id from now on (the keychain entry and the local data stay).
    pub(crate) async fn rekeyed_account(&self, old: AccountId, new: AccountId) {
        let _admin = self.inner.admin.lock().await;
        if let Some(handle) = self.inner.accounts.remove(old) {
            handle.stop(self.inner.sync.shutdown_grace).await;
        }
        if let Some(outbox) = &self.inner.outbox {
            outbox.stop_account(old).await;
        }
        if let Some(reading) = &self.inner.reading {
            reading.forget_account(old);
        }
        let Ok(store) = self.store() else { return };
        match store.account(new).await {
            Ok(Some(account)) => {
                tracing::info!(%old, %new, "account matched with the settings sync");
                self.spawn_sync(account, None);
                if let Some(outbox) = &self.inner.outbox {
                    outbox.wake(new);
                }
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(account = %new, error = %err, "matched account unreadable"),
        }
    }

    // Stage 9 (`crate::comfort`) works through these.

    pub(crate) fn comfort(&self) -> &crate::comfort::ComfortState {
        &self.inner.comfort
    }

    /// Stage 15 (`crate::updates`, decision 0037).
    pub(crate) fn updates(&self) -> &crate::updates::UpdatesState {
        &self.inner.updates
    }

    /// A reference that does not keep the core alive (for background tasks).
    pub(crate) fn downgrade(&self) -> WeakCore<C> {
        WeakCore(Arc::downgrade(&self.inner))
    }

    // -----------------------------------------------------------------------------------------
    // Composing and sending (stage 5, see `crate::compose`)
    // -----------------------------------------------------------------------------------------

    /// A new local draft: empty, or a reply, reply to all or forward of a stored message
    /// (recipients, identity, subject, References and the quote prepared; a forward takes the
    /// attachments). Emits `draftsChanged`.
    pub async fn create_draft(
        &self,
        request: mail_model::NewDraft,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.create(request).await
    }

    pub async fn get_draft(&self, id: mail_model::DraftId) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.get(id).await
    }

    /// Saves the composer's fields locally (call every few seconds while typing); the server
    /// copy follows after a quiet moment. `send_in_progress` while the draft is in the outbox.
    pub async fn save_draft(
        &self,
        id: mail_model::DraftId,
        update: mail_model::DraftUpdate,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.save(id, update).await
    }

    /// The composer closed: the newest version goes to the Drafts folder now.
    pub async fn close_draft(
        &self,
        id: mail_model::DraftId,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.close(id).await
    }

    /// Discards a draft (local, files, and its copy in the Drafts folder with writes on).
    pub async fn delete_draft(&self, id: mail_model::DraftId) -> Result<(), CoreError> {
        self.compose()?.delete(id).await
    }

    /// Local drafts (also after a crash), newest change first; all accounts without `account`.
    pub async fn list_drafts(
        &self,
        account: Option<AccountId>,
    ) -> Result<Vec<mail_model::DraftSummary>, CoreError> {
        self.compose()?.list(account).await
    }

    /// Opens a message of the Drafts folder as a draft (the app's own, or one of another
    /// program made into a local draft).
    pub async fn open_draft_from_message(
        &self,
        message: MessageId,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.open_from_message(message).await
    }

    /// Copies files into the draft. The shell passes only paths from the system file dialog or
    /// from a drag-and-drop event of the window.
    pub async fn add_draft_attachments(
        &self,
        id: mail_model::DraftId,
        paths: Vec<PathBuf>,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.add_attachments(id, paths).await
    }

    pub async fn remove_draft_attachment(
        &self,
        id: mail_model::DraftId,
        attachment: String,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.remove_attachment(id, attachment).await
    }

    /// Freezes the draft and queues it for sending after the undo delay. `request_id` names
    /// the press of "send" (a UUID): the same id twice sends once. Refused before anything
    /// changes: `read_only`, `invalid_address`, `attachment_too_large`, `send_in_progress`.
    pub async fn send_draft(
        &self,
        id: mail_model::DraftId,
        request_id: String,
    ) -> Result<mail_model::OutgoingMail, CoreError> {
        self.compose()?.send(id, request_id).await
    }

    /// Undo of a send within the delay, or "edit" of a failed or unclear one: the draft is
    /// editable again; nothing was sent. `send_in_progress` once the submission started.
    pub async fn cancel_send(
        &self,
        id: mail_model::DraftId,
    ) -> Result<mail_model::Draft, CoreError> {
        self.compose()?.cancel_send(id).await
    }

    /// The outbox view: the account's sends, newest first, below `before`. `limit` 1..=500.
    pub async fn list_outgoing(
        &self,
        account: AccountId,
        before: Option<OperationId>,
        limit: u32,
    ) -> Result<Vec<mail_model::OutgoingMail>, CoreError> {
        self.compose()?.outgoing(account, before, limit).await
    }

    /// The user's decision on an unclear send (`sendNeedsDecision`).
    pub async fn resolve_unclear_send(
        &self,
        operation: OperationId,
        decision: mail_model::UnclearDecision,
    ) -> Result<mail_model::OutgoingMail, CoreError> {
        self.compose()?.resolve_unclear(operation, decision).await
    }

    pub async fn get_compose_settings(&self) -> Result<mail_model::ComposeSettings, CoreError> {
        self.compose()?.compose_settings().await
    }

    /// Stores the undo delay (0 to 30 seconds; `invalid_argument` otherwise).
    pub async fn set_compose_settings(
        &self,
        settings: mail_model::ComposeSettings,
    ) -> Result<mail_model::ComposeSettings, CoreError> {
        self.compose()?.set_compose_settings(settings).await
    }

    pub async fn get_send_settings(
        &self,
        account: AccountId,
    ) -> Result<mail_model::AccountSendSettings, CoreError> {
        self.compose()?.send_settings(account).await
    }

    /// Who files sent mail of the account (`auto`: Gmail does it itself).
    pub async fn set_sent_copy_mode(
        &self,
        account: AccountId,
        mode: mail_model::SentCopyMode,
    ) -> Result<mail_model::AccountSendSettings, CoreError> {
        self.compose()?.set_sent_copy(account, mode).await
    }

    /// Identities with their text signatures, for the composer's sender choice.
    pub async fn list_compose_identities(
        &self,
        account: AccountId,
    ) -> Result<Vec<mail_model::ComposeIdentity>, CoreError> {
        self.compose()?.identities(account).await
    }

    /// Sets or removes (`None`) the identity's text signature. Emits `accountsChanged`.
    pub async fn set_identity_signature(
        &self,
        identity: mail_model::IdentityId,
        signature: Option<String>,
    ) -> Result<mail_model::ComposeIdentity, CoreError> {
        self.compose()?.set_signature(identity, signature).await
    }

    /// Autocomplete from addresses of earlier mail: every word of `query` starts a word of the
    /// name or the address; ordered by how often and how recently the user wrote to them.
    pub async fn search_contacts(
        &self,
        query: String,
        limit: u32,
    ) -> Result<Vec<mail_model::ContactSuggestion>, CoreError> {
        self.compose()?.search_contacts(query, limit).await
    }
}

/// Marks an SMTP failure of the setup check as such in its detail.
fn smtp_detail(err: mail_model::BackendError) -> mail_model::BackendError {
    use mail_model::BackendError as B;
    let tag = |d: String| format!("SMTP: {d}");
    match err {
        B::Dns(d) => B::Dns(tag(d)),
        B::Timeout(d) => B::Timeout(tag(d)),
        B::ConnectionLost(d) => B::ConnectionLost(tag(d)),
        B::CertUntrusted(d) => B::CertUntrusted(tag(d)),
        B::Tls(d) => B::Tls(tag(d)),
        B::AuthFailed(d) => B::AuthFailed(tag(d)),
        B::Unsupported(d) => B::Unsupported(tag(d)),
        B::Protocol(d) => B::Protocol(tag(d)),
        B::Rejected(d) => B::Rejected(tag(d)),
        other => other,
    }
}

fn encode<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value)
        .map_err(|err| CoreError::Internal(format!("encode setting: {err}")))
}

/// A stored value this version does not understand falls back to the default, so the app still
/// starts; the problem is logged.
fn decode<T: DeserializeOwned + Default>(key: &str, json: &str) -> T {
    serde_json::from_str(json).unwrap_or_else(|err| {
        tracing::warn!(key, %err, "stored setting has an unknown value, using the default");
        T::default()
    })
}
