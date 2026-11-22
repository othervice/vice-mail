use std::path::PathBuf;
use std::sync::Arc;

use mail_imap::ImapConnector;
use mail_model::{
    Account, AccountId, AccountSettings, AccountStatus, AppInfo, Capabilities,
    ConnectionTestResult, CoreError, CoreEvent, DiscoveryResult, ListCursor, MailBackend,
    MailConnector, Mailbox, MailboxId, MailboxRole, MessagePage, Platform, Preferences, Secret,
    ServerSettings,
};
use mail_store::{GLOBAL_SCOPE, NewAccount, Store, StoreOptions, StoredAccount};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::clock::{Clock, SystemClock};
use crate::credentials::{CredentialRef, Credentials};
use crate::discovery;
use crate::sync::account::{self, Control, Registry};
use crate::sync::{Env, SyncConfig, emit, wake};

/// File name of the database inside the app's data directory.
pub const DATABASE_FILE: &str = "mail.sqlite3";

const THEME_KEY: &str = "ui.theme";
const LANGUAGE_KEY: &str = "ui.language";

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

/// What the core is given instead of reaching for it: the keychain, the mail connector, the
/// clock and the sync tunables. Tests inject an in-memory keychain, a fake or instrumented
/// connector, a movable clock and short intervals.
pub struct CoreServices<C: MailConnector = ImapConnector> {
    pub credentials: Credentials,
    pub connector: C,
    pub clock: Arc<dyn Clock>,
    pub sync: SyncConfig,
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
        }
    }
}

/// Facade of the core. The shell forwards every command here; everything is testable without
/// a WebView. Cheap to clone. Generic over the mail connector so tests can use a fake server.
pub struct AppCore<C: MailConnector + 'static = ImapConnector> {
    inner: Arc<Inner<C>>,
}

impl<C: MailConnector + 'static> Clone for AppCore<C> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<C> {
    info: AppInfo,
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
}

impl<C> Drop for Inner<C> {
    /// Without an orderly [`AppCore::shutdown`] the sync tasks are aborted: connections close
    /// without LOGOUT, stored data is consistent anyway (one transaction per window).
    fn drop(&mut self) {
        if let Some(task) = &self.wake_detector {
            task.abort();
        }
        self.accounts.abort_all();
    }
}

impl<C: MailConnector + 'static> AppCore<C> {
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

        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let accounts = Arc::new(Registry::default());
        let sync = Arc::new(services.sync);
        let wake_detector = Some(wake::spawn(
            Arc::downgrade(&accounts),
            Arc::clone(&services.clock),
            sync.wake_check,
            sync.wake_threshold,
        ));
        let core = Self {
            inner: Arc::new(Inner {
                info: AppInfo {
                    version: config.build.version,
                    commit: config.build.commit,
                    build_date: config.build.build_date,
                    platform: Platform::current(),
                },
                store: opened,
                events,
                connector: Arc::new(services.connector),
                credentials: services.credentials,
                clock: services.clock,
                sync,
                accounts,
                admin: tokio::sync::Mutex::new(()),
                wake_detector,
            }),
        };
        core.start_stored_accounts().await;
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

    /// Validates the settings, logs in to IMAP read-only and out again. Stores nothing.
    pub async fn test_connection(
        &self,
        settings: AccountSettings,
        password: Secret<String>,
    ) -> Result<ConnectionTestResult, CoreError> {
        let settings = settings.validated()?;
        let capabilities = self.check_login(&settings.imap, password).await?;
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
        if !self
            .inner
            .accounts
            .send(id, Control::CredentialsChanged(password.clone()))
        {
            self.spawn_sync(account, Some(password));
        }
        tracing::info!(account = %id, "password replaced");
        Ok(())
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

    // -----------------------------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------------------------

    /// The network changed (new Wi-Fi, VPN up or down): every account reconnects now instead
    /// of waiting for its connections to time out. For a shell with an OS signal.
    pub fn network_changed(&self) {
        tracing::info!("network changed; reconnecting all accounts");
        self.inner.accounts.wake_all();
    }

    /// The computer woke from sleep: like [`AppCore::network_changed`]. The core also notices
    /// sleep by itself (`sync::wake`).
    pub fn resume(&self) {
        tracing::info!("resumed; reconnecting all accounts");
        self.inner.accounts.wake_all();
    }

    /// Stops every sync task; connections get a moment to log out. Call when the app quits.
    pub async fn shutdown(&self) {
        let grace = self.inner.sync.shutdown_grace;
        let handles = self.inner.accounts.take_all();
        let mut stopping = tokio::task::JoinSet::new();
        for handle in handles {
            stopping.spawn(handle.stop(grace));
        }
        while stopping.join_next().await.is_some() {}
        if let Some(task) = &self.inner.wake_detector {
            task.abort();
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

    fn spawn_sync(&self, account: StoredAccount, password: Option<Secret<String>>) {
        let Ok(store) = self.store() else { return };
        let env = Env {
            store: store.clone(),
            connector: Arc::clone(&self.inner.connector),
            credentials: self.inner.credentials.clone(),
            clock: Arc::clone(&self.inner.clock),
            config: Arc::clone(&self.inner.sync),
            events: self.inner.events.clone(),
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
        let config = self.inner.sync.connect_config(server, password, false);
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

    fn store(&self) -> Result<&Store, CoreError> {
        self.inner.store.as_ref().map_err(Clone::clone)
    }

    fn emit(&self, event: CoreEvent) {
        emit(&self.inner.events, event);
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
