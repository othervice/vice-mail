//! Passwords in the system keychain, since stage 12 the tokens of AI clients (MCP), and since
//! stage 11 the settings sync key.
//!
//! The database only holds a [`CredentialRef`] (column `account.credential_ref`); the secret
//! itself lives in the macOS keychain or the Windows Credential Manager under the app's service
//! name (the bundle id, passed in by the shell) and the account's UUID as the account name.
//! Accounts taken over from the settings sync get a random name of their own (`synced:<hex>`,
//! decision 0040), so they never find a password this device holds under their UUID.
//!
//! [`SecretStore`] is the blocking interface with two implementations: [`SystemKeychain`] for
//! the product and [`MemorySecretStore`] for tests (automatic tests never touch the real
//! keychain, which can show system dialogs). The engine uses the [`Credentials`] handle, which
//! runs every call on a blocking thread.
//!
//! Stage 22 (decision 0075): on macOS [`SystemKeychain`] keeps every secret in one keychain item
//! (the vault, [`vault`]), so an update of the ad-hoc signed app asks once instead of once per
//! secret; the names above are keys inside it. Windows keeps one Credential Manager entry per
//! secret ([`KeychainItems`]): it shows no dialogs, and an entry holds at most 2,560 bytes.
//!
//! Stage 14 (decision 0042): accounts that sign in with Microsoft or Google have a reference
//! `oauth:<provider>:<id>`; their entry holds the refresh token (in pieces if it is long, see
//! `oauth::store`), never a password. [`Credentials::get`] refuses such entries, so a token can
//! never be sent as a password; connections ask [`Credentials::login`], which hands out an
//! access token for the provider's own servers only.
//!
//! Stage 14: JMAP accounts that sign in with an API token (Fastmail) have a reference
//! `token:<id>`; the entry holds the token like a password, and [`Credentials::login`] hands it
//! out as a bearer token ([`LoginAuth::Bearer`]), never as a password.
//!
//! No secret ever reaches a log line or an error text: values are only handled as
//! [`Secret<String>`], and keychain errors are mapped to [`CoreError::Keychain`] without the
//! data some platform errors carry (see [`keychain_error`]).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use mail_model::{
    AccountId, CoreError, LoginAuth, OAuthError, OAuthProvider, Secret, ServerSettings,
};

use crate::oauth::OAuthService;

pub mod vault;

pub use vault::{ItemStore, MigrationReport, VAULT_ITEM, VaultStore};

/// Longest accepted [`CredentialRef`]. Windows allows 32767 characters for a generic target
/// name and 513 for the user name; an account UUID has 36.
const MAX_REF_LEN: usize = 256;

/// Prefix of the keychain entries of AI clients (stage 12).
const MCP_CLIENT_PREFIX: &str = "mcp-client:";

/// Prefix of the keychain entries of AI provider keys (stage 13).
const AI_PROFILE_PREFIX: &str = "ai-profile:";

/// Name of the keychain entry with the settings sync key (stage 11).
const SETTINGS_SYNC_KEY: &str = "settings-sync:key";

/// Name of a keychain entry under the app's service. For accounts it is the account UUID.
/// Stored in the database instead of the secret.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialRef(String);

impl CredentialRef {
    /// The entry that holds the password of an account.
    pub fn for_account(id: AccountId) -> Self {
        Self(id.to_string())
    }

    /// Stage 14 (decision 0042): the entry of an account that signs in with `provider`,
    /// `oauth:<provider>:<uuid>`. It holds the refresh token.
    pub fn for_oauth_account(provider: OAuthProvider, id: AccountId) -> Self {
        Self(format!(
            "{}{}:{id}",
            mail_model::OAUTH_CREDENTIAL_PREFIX,
            provider.as_str()
        ))
    }

    /// Stage 14: the entry of a JMAP account whose secret is an API token, `token:<uuid>`.
    pub fn for_api_token_account(id: AccountId) -> Self {
        Self(format!("{}{id}", mail_model::API_TOKEN_CREDENTIAL_PREFIX))
    }

    /// The provider whose sign-in this entry holds; `None` for a password or another secret.
    pub fn oauth_provider(&self) -> Option<OAuthProvider> {
        mail_model::oauth_provider_of_credential_ref(&self.0)
    }

    /// Piece `n` (1…) of a long refresh token (`oauth::store`).
    pub(crate) fn piece(&self, n: usize) -> Self {
        Self(format!("{}#{n}", self.0))
    }

    /// Stage 12: the entry that holds the token of an AI client (MCP), `mcp-client:<id>`. The
    /// prefix keeps it apart from account entries (UUIDs) under the same service.
    pub fn for_mcp_client(id: &mail_mcp::ClientId) -> Self {
        Self(format!("{MCP_CLIENT_PREFIX}{id}"))
    }

    /// Stage 13: the entry that holds the API key of an AI provider profile,
    /// `ai-profile:<id>`.
    pub fn for_ai_profile(id: &str) -> Self {
        Self(format!("{AI_PROFILE_PREFIX}{id}"))
    }

    /// Stage 11: the entry that holds the settings sync key (`AGE-SECRET-KEY-1…`, never the
    /// passphrase; decision 0014). One per device: the sync has one folder.
    pub fn for_settings_sync() -> Self {
        Self(SETTINGS_SYNC_KEY.to_owned())
    }

    /// Whether this is the entry of an AI client rather than an account password.
    pub fn is_mcp_client(&self) -> bool {
        self.0.starts_with(MCP_CLIENT_PREFIX)
    }

    /// Checks a reference read back from the database.
    pub fn parse(stored: &str) -> Result<Self, CoreError> {
        if stored.is_empty() || stored.len() > MAX_REF_LEN || stored.chars().any(char::is_control) {
            return Err(CoreError::InvalidArgument(format!(
                "keychain reference {stored:?} is not valid"
            )));
        }
        Ok(Self(stored.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A store for secrets, keyed by [`CredentialRef`].
///
/// The calls block: the operating system may take a while or ask the user (keychain dialog).
/// Call them from a blocking thread; [`Credentials`] does that.
pub trait SecretStore: Send + Sync + 'static {
    /// Stores or replaces the secret.
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError>;

    /// The stored secret, or `None` if there is no entry.
    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError>;

    /// Removes the entry. Returns `false` if there was none (not an error).
    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError>;

    /// Stage 22: moves secrets kept the old way (one keychain item each) into the store's own
    /// form; `names` are every entry the database knows of. Nothing to do for stores without
    /// another form (the default).
    fn migrate(&self, names: &[CredentialRef]) -> Result<MigrationReport, CoreError> {
        let _ = names;
        Ok(MigrationReport::default())
    }
}

/// The engine's handle to the secret store: cheap to clone, async, runs each call on a
/// blocking thread and logs the outcome (never the secret).
#[derive(Clone)]
pub struct Credentials {
    store: Arc<dyn SecretStore>,
    /// Stage 14: sign-in with Microsoft and Google; `None` where no provider is configured.
    oauth: Option<Arc<OAuthService>>,
}

/// What a connection logs in with: a password or an access token (`Debug` shows neither).
#[derive(Debug)]
pub(crate) struct Login {
    pub secret: Secret<String>,
    pub auth: LoginAuth,
}

impl Login {
    /// The connection settings for `server` with this login (default limits, no extra root).
    pub(crate) fn config(
        self,
        server: &ServerSettings,
        access: mail_model::Access,
    ) -> mail_model::ConnectConfig {
        let mut config = mail_model::ConnectConfig::new(server, self.secret, access);
        config.auth = self.auth;
        config
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials").finish_non_exhaustive()
    }
}

impl Credentials {
    pub fn new(store: impl SecretStore) -> Self {
        Self {
            store: Arc::new(store),
            oauth: None,
        }
    }

    pub fn from_arc(store: Arc<dyn SecretStore>) -> Self {
        Self { store, oauth: None }
    }

    /// Stage 14: with sign-in through these providers (the shell passes the build's profiles).
    #[must_use]
    pub fn with_oauth(mut self, service: Arc<OAuthService>) -> Self {
        self.oauth = Some(service);
        self
    }

    pub(crate) fn oauth(&self) -> Option<&Arc<OAuthService>> {
        self.oauth.as_ref()
    }

    /// Stage 14: what a connection to `server` logs in with. A password account gets its
    /// password; an account that signs in with a provider gets a valid access token, and only
    /// for that provider's IMAP (`imap`) or SMTP servers.
    pub(crate) async fn login(
        &self,
        credential_ref: &str,
        server: &ServerSettings,
        imap: bool,
    ) -> Result<Login, CoreError> {
        let key = CredentialRef::parse(credential_ref)?;
        let Some(provider) = key.oauth_provider() else {
            let password = self.get(&key).await?.ok_or_else(|| {
                CoreError::Keychain("no password stored for this account; enter it again".into())
            })?;
            return Ok(Login {
                secret: password,
                auth: secret_auth(credential_ref),
            });
        };
        let service = self
            .oauth
            .as_ref()
            .ok_or(OAuthError::Unavailable(provider))?;
        let profile = service.profile(provider)?;
        if !profile.allows_host(&server.host, imap) {
            return Err(OAuthError::Failed(format!(
                "{} is not a {} server; the sign-in is only used with the provider's servers",
                server.host,
                provider.name()
            ))
            .into());
        }
        let token = crate::oauth::tokens::access_token(service, self, &key, provider).await?;
        Ok(Login {
            secret: token,
            auth: LoginAuth::Bearer,
        })
    }

    /// Stage 14: runs `connect` with the account's login; when the server rejects an access
    /// token, renews it and runs `connect` once more (the token may have been revoked or ended
    /// early). A rejected password is returned as it is.
    pub(crate) async fn with_login<T, F, Fut>(
        &self,
        credential_ref: &str,
        server: &ServerSettings,
        imap: bool,
        mut connect: F,
    ) -> Result<T, CoreError>
    where
        F: FnMut(Login) -> Fut,
        Fut: std::future::Future<Output = Result<T, mail_model::BackendError>>,
    {
        let login = self.login(credential_ref, server, imap).await?;
        let bearer = login.auth == LoginAuth::Bearer;
        match connect(login).await {
            Err(mail_model::BackendError::AuthFailed(_))
                if bearer && self.login_rejected(credential_ref) =>
            {
                tracing::info!("the server rejected the access token; renewing it once");
                let login = self.login(credential_ref, server, imap).await?;
                Ok(connect(login).await?)
            }
            other => Ok(other?),
        }
    }

    /// Stage 14: a server rejected the access token of `credential_ref`; the next
    /// [`Credentials::login`] renews it. Nothing happens for a password.
    pub(crate) fn login_rejected(&self, credential_ref: &str) -> bool {
        let Ok(key) = CredentialRef::parse(credential_ref) else {
            return false;
        };
        match (&self.oauth, key.oauth_provider()) {
            (Some(service), Some(_)) => {
                service.tokens.reject(&key);
                true
            }
            _ => false,
        }
    }

    /// Stores or replaces the secret of `key`. Not for sign-in entries (`oauth::store`).
    pub async fn set(&self, key: &CredentialRef, secret: Secret<String>) -> Result<(), CoreError> {
        if key.oauth_provider().is_some() {
            return Err(CoreError::InvalidArgument(
                "this account signs in with its provider; it has no password".into(),
            ));
        }
        self.set_raw(key, secret).await
    }

    /// [`Credentials::set`] for any entry, sign-in entries included.
    pub(crate) async fn set_raw(
        &self,
        key: &CredentialRef,
        secret: Secret<String>,
    ) -> Result<(), CoreError> {
        let result = self
            .run(key, move |store, key| store.set(key, &secret))
            .await;
        match &result {
            Ok(()) => tracing::debug!(credential = %key, "secret stored in the keychain"),
            Err(err) => {
                tracing::warn!(credential = %key, error = %err, "storing a secret in the keychain failed");
            }
        }
        result
    }

    /// The secret of `key`, `None` if the keychain has no entry for it. Refuses the entry of
    /// an account that signs in with a provider: its refresh token is never a password.
    pub async fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        if key.oauth_provider().is_some() {
            return Err(CoreError::Keychain(
                "this account signs in with its provider and has no password".into(),
            ));
        }
        self.get_raw(key).await
    }

    /// [`Credentials::get`] for any entry (`oauth::store` reads its own).
    pub(crate) async fn get_raw(
        &self,
        key: &CredentialRef,
    ) -> Result<Option<Secret<String>>, CoreError> {
        let result = self.run(key, |store, key| store.get(key)).await;
        match &result {
            Ok(found) => {
                tracing::debug!(credential = %key, found = found.is_some(), "keychain read");
            }
            Err(err) => {
                tracing::warn!(credential = %key, error = %err, "reading a secret from the keychain failed");
            }
        }
        result
    }

    /// Removes the entry of `key`; `false` if there was none. For a sign-in entry also its
    /// pieces and the access token in memory (the provider is not asked here; see
    /// `AppCore::remove_account`, which revokes first).
    pub async fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        if key.oauth_provider().is_some() {
            for n in 1..crate::oauth::store::MAX_PIECES {
                self.delete_raw(&key.piece(n)).await?;
            }
            if let Some(service) = &self.oauth {
                service.tokens.forget(key);
            }
        }
        self.delete_raw(key).await
    }

    /// Removes exactly the entry of `key`.
    pub(crate) async fn delete_raw(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        let result = self.run(key, |store, key| store.delete(key)).await;
        match &result {
            Ok(existed) => tracing::debug!(credential = %key, existed, "keychain entry deleted"),
            Err(err) => {
                tracing::warn!(credential = %key, error = %err, "deleting a keychain entry failed");
            }
        }
        result
    }

    /// Stage 22 (decision 0075): moves the entries of `names` into the store's own form (the
    /// vault on macOS); see [`SecretStore::migrate`]. Logs the outcome, never a secret.
    pub async fn migrate(&self, names: Vec<CredentialRef>) -> Result<MigrationReport, CoreError> {
        let store = Arc::clone(&self.store);
        let result = tokio::task::spawn_blocking(move || store.migrate(&names))
            .await
            .map_err(|err| CoreError::Internal(format!("keychain task failed: {err}")))?;
        match &result {
            Ok(report) if *report == MigrationReport::default() => {}
            Ok(report) => tracing::info!(
                moved = report.moved,
                kept_old = report.kept_old,
                failed = report.failed,
                "old keychain entries moved into one"
            ),
            Err(err) => {
                tracing::warn!(error = %err, "moving old keychain entries failed; they stay")
            }
        }
        result
    }

    async fn run<T, F>(&self, key: &CredentialRef, op: F) -> Result<T, CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&dyn SecretStore, &CredentialRef) -> Result<T, CoreError> + Send + 'static,
    {
        let store = Arc::clone(&self.store);
        let key = key.clone();
        tokio::task::spawn_blocking(move || op(store.as_ref(), &key))
            .await
            .map_err(|err| CoreError::Internal(format!("keychain task failed: {err}")))?
    }
}

/// How the secret stored under `credential_ref` is sent: an API token (stage 14) as a bearer
/// token, anything else as a password. Sign-in entries never get here as a secret.
pub(crate) fn secret_auth(credential_ref: &str) -> LoginAuth {
    if mail_model::is_api_token_credential_ref(credential_ref) {
        LoginAuth::Bearer
    } else {
        LoginAuth::Password
    }
}

/// The operating system's credential store as it is, one item per secret: macOS keychain
/// (login keychain, generic password) or Windows Credential Manager (generic credential,
/// persisted on this computer only). Service is the app's bundle id, account the entry's name.
pub struct KeychainItems {
    service: String,
    store: Arc<keyring_core::CredentialStore>,
    /// Store-specific options for every entry (Windows: persistence).
    modifiers: HashMap<&'static str, &'static str>,
}

impl fmt::Debug for KeychainItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeychainItems")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl KeychainItems {
    /// Opens the platform's store for `service` (the app's bundle id). Does not touch the
    /// keychain yet, so it never shows a dialog.
    pub fn new(service: &str) -> Result<Self, CoreError> {
        let (store, modifiers) = platform_store()?;
        Self::with_store(service, store, modifiers)
    }

    /// Uses the given keyring-core store (tests use `keyring_core::mock`).
    pub fn with_store(
        service: &str,
        store: Arc<keyring_core::CredentialStore>,
        modifiers: HashMap<&'static str, &'static str>,
    ) -> Result<Self, CoreError> {
        let service = service.trim();
        if service.is_empty() || service.chars().any(char::is_control) {
            return Err(CoreError::InvalidArgument(format!(
                "keychain service name {service:?} is not valid"
            )));
        }
        Ok(Self {
            service: service.to_owned(),
            store,
            modifiers,
        })
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    fn entry(&self, name: &str) -> Result<keyring_core::Entry, CoreError> {
        let entry = if self.modifiers.is_empty() {
            keyring_core::Entry::new_in_store(&self.store, &self.service, name)
        } else {
            keyring_core::Entry::new_in_store_with_modifiers(
                &self.store,
                &self.service,
                name,
                &self.modifiers,
            )
        };
        entry.map_err(keychain_error)
    }
}

impl ItemStore for KeychainItems {
    fn read(&self, name: &str) -> Result<Option<Secret<String>>, CoreError> {
        match self.entry(name)?.get_password() {
            Ok(password) => Ok(Some(Secret::new(password))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(err) => Err(keychain_error(err)),
        }
    }

    fn write(&self, name: &str, value: &Secret<String>) -> Result<(), CoreError> {
        if value.expose_secret().is_empty() {
            // macOS rejects empty passwords; a server login with one makes no sense either.
            return Err(CoreError::InvalidArgument("the password is empty".into()));
        }
        self.entry(name)?
            .set_password(value.expose_secret())
            .map_err(keychain_error)
    }

    fn remove(&self, name: &str) -> Result<bool, CoreError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring_core::Error::NoEntry) => Ok(false),
            Err(err) => Err(keychain_error(err)),
        }
    }

    /// The accounts of every item under the service (macOS: attributes only, no dialog).
    fn names(&self) -> Result<Vec<String>, CoreError> {
        let spec = HashMap::from([("service", self.service.as_str())]);
        let entries =
            keyring_core::Entry::search_in_store(&self.store, &spec).map_err(keychain_error)?;
        Ok(entries
            .iter()
            .filter_map(keyring_core::Entry::get_specifiers)
            .filter(|(service, _)| *service == self.service)
            .map(|(_, name)| name)
            .collect())
    }
}

impl SecretStore for KeychainItems {
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError> {
        self.write(key.as_str(), secret)
    }

    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        self.read(key.as_str())
    }

    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        self.remove(key.as_str())
    }
}

/// The app's secret store: on macOS the vault in one keychain item (stage 22, decision 0075),
/// elsewhere one entry per secret ([`KeychainItems`]).
pub struct SystemKeychain {
    service: String,
    store: Box<dyn SecretStore>,
}

impl fmt::Debug for SystemKeychain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemKeychain")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl SystemKeychain {
    /// Opens the platform's store for `service` (the app's bundle id). Does not touch the
    /// keychain yet, so it never shows a dialog.
    pub fn new(service: &str) -> Result<Self, CoreError> {
        let items = KeychainItems::new(service)?;
        let service = items.service().to_owned();
        let store: Box<dyn SecretStore> = if cfg!(target_os = "macos") {
            Box::new(VaultStore::new(items))
        } else {
            Box::new(items)
        };
        Ok(Self { service, store })
    }

    pub fn service(&self) -> &str {
        &self.service
    }
}

impl SecretStore for SystemKeychain {
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError> {
        self.store.set(key, secret)
    }

    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        self.store.get(key)
    }

    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        self.store.delete(key)
    }

    fn migrate(&self, names: &[CredentialRef]) -> Result<MigrationReport, CoreError> {
        self.store.migrate(names)
    }
}

type PlatformStore = (
    Arc<keyring_core::CredentialStore>,
    HashMap<&'static str, &'static str>,
);

#[cfg(target_os = "macos")]
fn platform_store() -> Result<PlatformStore, CoreError> {
    let store = apple_native_keyring_store::keychain::Store::new().map_err(keychain_error)?;
    Ok((store, HashMap::new()))
}

#[cfg(target_os = "windows")]
fn platform_store() -> Result<PlatformStore, CoreError> {
    let store = windows_native_keyring_store::Store::new().map_err(keychain_error)?;
    // `Local`: stays on this computer. The store's default (`Enterprise`) would roam with a
    // domain profile; passwords of synced accounts are entered per device instead.
    Ok((store, HashMap::from([("persistence", "Local")])))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_store() -> Result<PlatformStore, CoreError> {
    Err(CoreError::Keychain(
        "no supported system keychain on this platform".into(),
    ))
}

/// Maps a keyring error to [`CoreError::Keychain`]. Variants that carry stored data
/// (`BadEncoding`, `BadDataFormat`) are reduced to a fixed text, so a secret read back from the
/// keychain can never end up in a message. Platform errors are OS status codes and texts.
pub fn keychain_error(err: keyring_core::Error) -> CoreError {
    use keyring_core::Error as E;
    let text = match err {
        E::PlatformFailure(inner) => format!("platform failure: {inner}"),
        E::NoStorageAccess(inner) => format!("no access to the keychain: {inner}"),
        E::NoEntry => "no keychain entry".to_owned(),
        E::BadEncoding(_) => "the stored password is not valid UTF-8".to_owned(),
        E::BadDataFormat(_, _) => "the stored secret has an unexpected format".to_owned(),
        E::BadStoreFormat(_) => "the keychain has an unexpected format".to_owned(),
        E::TooLong(name, limit) => format!("{name} is longer than the keychain allows ({limit})"),
        E::Invalid(name, reason) => format!("invalid {name}: {reason}"),
        E::Ambiguous(entries) => format!("{} matching keychain entries", entries.len()),
        E::NoDefaultStore => "no keychain store available".to_owned(),
        E::NotSupportedByStore(what) => format!("not supported by the keychain: {what}"),
        _ => "unknown keychain error".to_owned(),
    };
    CoreError::Keychain(text)
}

/// Which call of a [`MemorySecretStore`] should fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretOp {
    Set,
    Get,
    Delete,
}

/// In-memory [`SecretStore`] for tests, with failure injection. Not for the product.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<CredentialRef, Secret<String>>>,
    failing: Mutex<HashSet<SecretOp>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// From now on, `op` fails with a [`CoreError::Keychain`] (until turned off again).
    pub fn set_failing(&self, op: SecretOp, failing: bool) {
        let mut set = self.failing.lock().unwrap_or_else(PoisonError::into_inner);
        if failing {
            set.insert(op);
        } else {
            set.remove(&op);
        }
    }

    /// Number of stored entries.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    pub fn contains(&self, key: &CredentialRef) -> bool {
        self.lock().contains_key(key)
    }

    /// Length in characters of the longest stored secret (stage 14: pieces of long tokens).
    pub fn longest(&self) -> usize {
        self.lock()
            .values()
            .map(|s| s.expose_secret().chars().count())
            .max()
            .unwrap_or(0)
    }

    /// Every stored secret, for leak checks in tests.
    pub fn secrets(&self) -> Vec<Secret<String>> {
        self.lock().values().cloned().collect()
    }

    fn check(&self, op: SecretOp) -> Result<(), CoreError> {
        let failing = self.failing.lock().unwrap_or_else(PoisonError::into_inner);
        if failing.contains(&op) {
            return Err(CoreError::Keychain(format!("injected {op:?} failure")));
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CredentialRef, Secret<String>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError> {
        self.check(SecretOp::Set)?;
        if secret.expose_secret().is_empty() {
            return Err(CoreError::InvalidArgument("the password is empty".into()));
        }
        self.lock().insert(key.clone(), secret.clone());
        Ok(())
    }

    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        self.check(SecretOp::Get)?;
        Ok(self.lock().get(key).cloned())
    }

    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        self.check(SecretOp::Delete)?;
        Ok(self.lock().remove(key).is_some())
    }
}

/// Safe to share as the shell's store: an `Arc` of a store is a store.
impl<S: SecretStore + ?Sized> SecretStore for Arc<S> {
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError> {
        (**self).set(key, secret)
    }

    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        (**self).get(key)
    }

    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        (**self).delete(key)
    }

    fn migrate(&self, names: &[CredentialRef]) -> Result<MigrationReport, CoreError> {
        (**self).migrate(names)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use mail_model::{ErrorCode, IpcError};
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;
    use crate::logging;

    const PLAIN: &str = "correct horse battery staple 7%";
    const SERVICE: &str = "org.vicemail.desktop.test";

    fn account_ref() -> CredentialRef {
        let id: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        CredentialRef::for_account(id)
    }

    fn secret() -> Secret<String> {
        Secret::new(PLAIN.to_owned())
    }

    /// Every text a [`CoreError`] can turn into on its way to a log line or the UI.
    fn texts(err: &CoreError) -> Vec<String> {
        let ipc = IpcError::from(err);
        vec![
            err.to_string(),
            format!("{err:?}"),
            err.detail().unwrap_or_default(),
            format!("{ipc:?}"),
            serde_json::to_string(&ipc).expect("serialize"),
        ]
    }

    #[test]
    fn credential_ref_is_the_account_uuid_and_parses_back() {
        let key = account_ref();
        assert_eq!(key.as_str(), "6f9619ff-8b86-4d11-b42d-00c04fc964ff");
        assert_eq!(key.to_string(), key.as_str());
        assert_eq!(CredentialRef::parse(key.as_str()).expect("parse"), key);
        for bad in ["", "a\nb", &"x".repeat(MAX_REF_LEN + 1)] {
            let err = CredentialRef::parse(bad).expect_err("invalid");
            assert_eq!(err.code(), ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn memory_store_sets_replaces_reads_and_deletes() {
        let store = MemorySecretStore::new();
        let key = account_ref();
        assert!(store.get(&key).expect("get").is_none());
        assert!(!store.delete(&key).expect("delete missing is fine"));

        store.set(&key, &Secret::new("first".into())).expect("set");
        store.set(&key, &secret()).expect("replace");
        assert_eq!(store.len(), 1);
        let read = store.get(&key).expect("get").expect("present");
        assert_eq!(read.expose_secret(), PLAIN);

        assert!(store.delete(&key).expect("delete"));
        assert!(store.is_empty());
        assert!(store.get(&key).expect("get").is_none());

        let err = store
            .set(&key, &Secret::new(String::new()))
            .expect_err("empty");
        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[test]
    fn memory_store_injects_typed_failures() {
        let store = MemorySecretStore::new();
        let key = account_ref();
        store.set_failing(SecretOp::Set, true);
        let err = store.set(&key, &secret()).expect_err("injected");
        assert_eq!(err.code(), ErrorCode::Keychain);
        assert!(store.is_empty());
        store.set_failing(SecretOp::Set, false);
        store.set(&key, &secret()).expect("set");

        store.set_failing(SecretOp::Delete, true);
        assert_eq!(
            store.delete(&key).expect_err("injected").code(),
            ErrorCode::Keychain
        );
        assert!(store.contains(&key));
    }

    /// The real code path against keyring-core's in-memory mock store: entries are created
    /// under service and account, a missing entry is `None`/`false`, errors are typed.
    #[test]
    fn system_keychain_maps_entries_and_errors() {
        let mock: Arc<keyring_core::CredentialStore> =
            keyring_core::mock::Store::new().expect("mock store");
        let keychain =
            KeychainItems::with_store(SERVICE, mock.clone(), HashMap::new()).expect("keychain");
        let key = account_ref();

        assert!(keychain.get(&key).expect("get").is_none());
        assert!(!keychain.delete(&key).expect("delete missing"));
        keychain.set(&key, &secret()).expect("set");
        let read = keychain.get(&key).expect("get").expect("present");
        assert_eq!(read.expose_secret(), PLAIN);

        // Same service and account name as the platform stores would use.
        let raw = keyring_core::Entry::new_in_store(&mock, SERVICE, key.as_str()).expect("entry");
        assert_eq!(raw.get_password().expect("raw read"), PLAIN);

        // A failing platform call becomes a typed `keychain` error.
        let cred: &keyring_core::mock::Cred = raw.as_any().downcast_ref().expect("mock cred");
        cred.set_error(keyring_core::Error::NoStorageAccess(Box::new(
            io::Error::other("user denied access"),
        )));
        let err = keychain.get(&key).expect_err("denied");
        assert_eq!(err.code(), ErrorCode::Keychain);
        assert!(err.to_string().contains("user denied access"), "{err}");

        assert!(keychain.delete(&key).expect("delete"));
        assert!(keychain.get(&key).expect("get").is_none());

        let err = keychain
            .set(&key, &Secret::new(String::new()))
            .expect_err("empty");
        assert_eq!(err.code(), ErrorCode::InvalidArgument);

        // The names under the service, from the store's search (the vault's listing; the
        // mock lists every entry it was asked for, the platform stores only existing ones).
        keychain.set(&key, &secret()).expect("set again");
        let other = KeychainItems::with_store("org.vicemail.other", mock.clone(), HashMap::new())
            .expect("other");
        other
            .set(&CredentialRef::for_settings_sync(), &secret())
            .expect("other service");
        assert_eq!(keychain.names().expect("names"), [key.as_str()]);

        let err = KeychainItems::with_store(" ", mock, HashMap::new()).expect_err("no service");
        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    /// Keyring errors that carry the stored bytes must not leak them into any error text.
    #[test]
    fn secret_never_appears_in_keychain_errors() {
        use keyring_core::Error as E;
        let bytes = PLAIN.as_bytes().to_vec();
        let errors = [
            E::BadEncoding(bytes.clone()),
            E::BadDataFormat(bytes.clone(), Box::new(io::Error::other(PLAIN))),
            E::BadStoreFormat(PLAIN.to_owned()),
            E::NoEntry,
            E::Ambiguous(Vec::new()),
            E::TooLong("password".into(), 2560),
            E::NoDefaultStore,
        ];
        for err in errors {
            let core = keychain_error(err);
            assert_eq!(core.code(), ErrorCode::Keychain);
            for text in texts(&core) {
                assert!(!text.contains(PLAIN), "secret leaked: {text}");
                assert!(
                    !text.contains("99, 111, 114"),
                    "secret bytes leaked: {text}"
                );
            }
        }
    }

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
                .into_owned()
        }
    }

    impl io::Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Store, read, fail and delete through the engine's handle with the production log
    /// layer at debug level: the log shows the entry name (positive control), never the secret.
    #[tokio::test]
    async fn secret_never_appears_in_logs() {
        let buffer = Buffer::default();
        let _guard =
            tracing::subscriber::set_default(logging::test_subscriber(buffer.clone(), true));

        let store = Arc::new(MemorySecretStore::new());
        let credentials = Credentials::new(Arc::clone(&store));
        let key = account_ref();

        credentials.set(&key, secret()).await.expect("set");
        let read = credentials.get(&key).await.expect("get").expect("present");
        tracing::info!(password = ?read, "read back"); // a careless log line stays redacted
        assert_eq!(read.expose_secret(), PLAIN);

        store.set_failing(SecretOp::Set, true);
        let err = credentials.set(&key, secret()).await.expect_err("injected");
        assert_eq!(err.code(), ErrorCode::Keychain);
        store.set_failing(SecretOp::Set, false);

        assert!(credentials.delete(&key).await.expect("delete"));
        assert!(!credentials.delete(&key).await.expect("delete again"));
        assert!(credentials.get(&key).await.expect("get").is_none());

        let log = buffer.text();
        assert!(
            log.contains(key.as_str()),
            "positive control, log was: {log}"
        );
        assert!(
            log.contains("storing a secret in the keychain failed"),
            "{log}"
        );
        assert!(!log.contains(PLAIN), "secret leaked into the log: {log}");
    }

    /// Stage 22 (decision 0075): the vault in the real keychain, under the desktop tests'
    /// service only. Ignored, and a no-op without `VICEMAIL_REAL_KEYCHAIN=1`. On the Mac mini
    /// run it in the GUI session (as `scripts/e2e-gui.sh` does; SSH sessions have the login
    /// keychain locked) with the test-server lock held. One build creates and reads every
    /// item, so macOS shows no dialog. Refuses to start while the service holds anything and
    /// leaves nothing behind: items as 0.1.2 stored them are moved into the vault, the old ones
    /// deleted, a change writes the next generation, and emptying it removes the item.
    #[test]
    #[ignore = "touches the real system keychain"]
    fn real_keychain_vault_round_trip() {
        if std::env::var("VICEMAIL_REAL_KEYCHAIN").as_deref() != Ok("1") {
            println!("skipped: set VICEMAIL_REAL_KEYCHAIN=1 to use the real keychain");
            return;
        }
        const E2E_SERVICE: &str = "org.vicemail.desktop.e2e";
        struct Cleanup(KeychainItems);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for name in self.0.names().unwrap_or_default() {
                    let _ = self.0.remove(&name);
                }
            }
        }
        let items = KeychainItems::new(E2E_SERVICE).expect("keychain");
        assert!(
            items.names().expect("names").is_empty(),
            "{E2E_SERVICE} holds items already; not touching them"
        );
        let _cleanup = Cleanup(KeychainItems::new(E2E_SERVICE).expect("keychain"));
        let old = [
            ("00000000-0000-4000-8000-00000000c0de", PLAIN),
            ("mcp-client:selftest", "token \"quoted\" ünïcödé"),
            ("settings-sync:key", "AGE-SECRET-KEY-1SELFTESTNOTAREALKEY"),
        ];
        for (name, value) in old {
            items
                .write(name, &Secret::new(value.to_owned()))
                .expect("old item");
        }
        let names: Vec<CredentialRef> = old
            .iter()
            .map(|(n, _)| CredentialRef::parse(n).expect("ref"))
            .collect();
        let vault = VaultStore::new(KeychainItems::new(E2E_SERVICE).expect("keychain"));
        let report = vault.migrate(&names).expect("migrate");
        assert_eq!((report.moved, report.kept_old, report.failed), (3, 0, 0));
        assert_eq!(items.names().expect("names"), [VAULT_ITEM]);
        for (key, (_, value)) in names.iter().zip(old) {
            let read = vault.get(key).expect("get").expect("present");
            assert_eq!(read.expose_secret(), value);
        }
        let new = CredentialRef::parse("ai-profile:selftest").expect("ref");
        vault.set(&new, &secret()).expect("set");
        assert_eq!(items.names().expect("names"), ["vault.2"]);
        let restarted = VaultStore::new(KeychainItems::new(E2E_SERVICE).expect("keychain"));
        let read = restarted.get(&new).expect("get").expect("present");
        assert_eq!(read.expose_secret(), PLAIN);
        for key in names.iter().chain([&new]) {
            assert!(restarted.delete(key).expect("delete"));
        }
        assert!(items.names().expect("names").is_empty(), "nothing left");
    }

    /// Real keychain round trip (macOS keychain or Windows Credential Manager). Ignored, and a
    /// no-op without `VICEMAIL_REAL_KEYCHAIN=1` (so `--include-ignored` for the test server never
    /// touches it): the system may show a dialog. Run by hand:
    /// `VICEMAIL_REAL_KEYCHAIN=1 cargo test -p mail-engine credentials::tests::real_keychain -- --ignored`.
    /// Uses its own service name and removes the entry again, also when an assertion fails.
    #[test]
    #[ignore = "touches the real system keychain"]
    fn real_keychain_round_trip() {
        if std::env::var("VICEMAIL_REAL_KEYCHAIN").as_deref() != Ok("1") {
            println!("skipped: set VICEMAIL_REAL_KEYCHAIN=1 to use the real keychain");
            return;
        }
        struct Cleanup<'a>(&'a SystemKeychain, &'a CredentialRef);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = self.0.delete(self.1);
            }
        }

        let keychain =
            SystemKeychain::new("org.vicemail.desktop.selftest").expect("system keychain");
        let id: AccountId = "00000000-0000-4000-8000-00000000c0de".parse().expect("id");
        let key = CredentialRef::for_account(id);
        let _cleanup = Cleanup(&keychain, &key);

        let _ = keychain.delete(&key);
        assert!(keychain.get(&key).expect("get").is_none());
        keychain
            .set(&key, &Secret::new("first".into()))
            .expect("set");
        keychain.set(&key, &secret()).expect("replace");
        let read = keychain.get(&key).expect("get").expect("present");
        assert_eq!(read.expose_secret(), PLAIN);
        assert!(keychain.delete(&key).expect("delete"));
        assert!(keychain.get(&key).expect("get").is_none());
        assert!(!keychain.delete(&key).expect("delete missing"));
    }
}
