//! Stage 12 in the core: AI access over MCP (decisions 0010, 0015, 0021).
//!
//! - **Settings** live in the store (class Gerät: switch, shares, clients, access log) and in
//!   the keychain (one token per client, `CredentialRef::for_mcp_client`). The core publishes
//!   them to the host as a `watch` channel ([`McpSettings`]); every change takes effect at the
//!   host's next step: switching off (the emergency stop) closes the endpoint and every open
//!   connection, revoking a client closes its connections, hiding an account removes it from
//!   every tool.
//! - **Host.** One [`McpHost`] per core, created at start. Its task starts the first time MCP
//!   is on (at start, or when the user switches it on) and runs until [`AppCore::shutdown`];
//!   while MCP is off it holds no socket or pipe. The endpoint comes from [`place`].
//! - **Tools** run against [`access::CoreAccess`], which holds the core weakly.
//! - **Access log** ([`log`]): every tool call and refused connection, written in batches,
//!   announced as `mcpAccess`. Host state changes are announced as `mcpStatusChanged`.
//! - **Test connection** ([`probe`]): starts the helper like an AI client, with a temporary
//!   client that is never stored or logged.

pub(crate) mod access;
mod log;
mod place;
mod probe;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use mail_mcp::config::{ClientKind, Setup, TOKEN_PLACEHOLDER, client_config};
use mail_mcp::{
    AccessLog, AccountAccess, ClientId, ClientKey, ClientRecord, ClientToken, Endpoint, HostStatus,
    McpHost, McpSettings,
};
use mail_model::{
    AccountId, CoreError, CoreEvent, MAX_MCP_ACCESS_PAGE, MAX_MCP_CLIENT_NAME_CHARS, McpAccessId,
    McpAccessPage, McpAccountAccess, McpAccountShare, McpClientConfig, McpClientCreated,
    McpClientInfo, McpClientKind, McpConnectionTest, McpHelperInfo, McpHostState, McpSettingsInfo,
    McpSetup, McpStatus, Secret,
};
use mail_store::{StoredMcpClient, StoredMcpState};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::core::{AppCore, Connector};
use crate::credentials::{CredentialRef, Credentials};

use access::CoreAccess;

/// Name of the temporary client of "test connection" in the host (never stored or logged).
const PROBE_NAME: &str = "connection test";

/// How long [`AppCore::shutdown`] waits for the host to close its connections.
const STOP_WAIT: Duration = Duration::from_secs(3);

/// The core's part of AI access. Lives in the core; the host and its tasks hold it weakly.
pub(crate) struct McpState<C: Connector> {
    data_dir: PathBuf,
    /// What the host checks every request against.
    settings: watch::Sender<McpSettings>,
    /// Keys of the registered clients, read from the keychain while MCP is on.
    keys: Mutex<HashMap<ClientId, ClientKey>>,
    /// The temporary clients of "test connection".
    probes: Arc<Mutex<HashMap<ClientId, ClientKey>>>,
    sink: Arc<log::Sink>,
    log_rx: Mutex<Option<mpsc::UnboundedReceiver<mail_mcp::AccessEvent>>>,
    host: OnceLock<McpHost<CoreAccess<C>>>,
    endpoint: OnceLock<Result<Endpoint, String>>,
    helper: Mutex<Option<PathBuf>>,
    run: Mutex<Option<JoinHandle<()>>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    stop: watch::Sender<bool>,
    /// Serialises changes: store, keychain and the published settings change together.
    admin: tokio::sync::Mutex<()>,
}

impl<C: Connector> McpState<C> {
    pub(crate) fn new(data_dir: PathBuf) -> Self {
        let probes: Arc<Mutex<HashMap<ClientId, ClientKey>>> = Arc::default();
        let (sink, log_rx) = log::Sink::new(Arc::clone(&probes));
        Self {
            data_dir,
            settings: watch::Sender::new(McpSettings::default()),
            keys: Mutex::default(),
            probes,
            sink: Arc::new(sink),
            log_rx: Mutex::new(Some(log_rx)),
            host: OnceLock::new(),
            endpoint: OnceLock::new(),
            helper: Mutex::new(place::default_helper()),
            run: Mutex::default(),
            tasks: Mutex::default(),
            stop: watch::Sender::new(false),
            admin: tokio::sync::Mutex::new(()),
        }
    }

    /// Without an orderly shutdown: the host and its connections end now.
    pub(crate) fn abort(&self) {
        if let Some(run) = lock(&self.run).take() {
            run.abort();
        }
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
    }

    fn access_of(&self, account: &AccountId) -> AccountAccess {
        self.settings.borrow().access(account)
    }

    /// Publishes `stored` (with the cached keys and the probes) to the host.
    fn publish(&self, stored: &StoredMcpState) {
        let keys = lock(&self.keys);
        let mut clients: Vec<ClientRecord> = stored
            .clients
            .iter()
            .filter(|client| client.active())
            .filter_map(|client| {
                let id: ClientId = client.client_id.parse().ok()?;
                keys.get(&id).map(|key| ClientRecord {
                    id,
                    name: client.name.clone(),
                    key: key.clone(),
                })
            })
            .collect();
        clients.extend(lock(&self.probes).iter().map(|(id, key)| ClientRecord {
            id: *id,
            name: PROBE_NAME.to_owned(),
            key: key.clone(),
        }));
        let settings = McpSettings {
            enabled: stored.enabled,
            accounts: stored
                .accounts
                .iter()
                .map(|(id, access)| (*id, to_host(*access)))
                .collect(),
            clients,
        };
        self.settings.send_replace(settings);
    }

    /// Reads the keys of active clients that are not cached yet. A missing or unreadable entry
    /// leaves that client unable to connect (logged); the others work.
    async fn load_keys(&self, credentials: &Credentials, clients: &[StoredMcpClient]) {
        for client in clients.iter().filter(|c| c.active()) {
            let Ok(id) = client.client_id.parse::<ClientId>() else {
                tracing::warn!("stored MCP client id is not valid");
                continue;
            };
            if lock(&self.keys).contains_key(&id) {
                continue;
            }
            let key = match CredentialRef::parse(&client.credential_ref) {
                Ok(key) => key,
                Err(err) => {
                    tracing::warn!(error = %err, "MCP client has a bad keychain reference");
                    continue;
                }
            };
            match credentials.get(&key).await {
                Ok(Some(secret)) => match ClientToken::parse(secret.expose_secret()) {
                    Ok(token) if token.id == id => {
                        lock(&self.keys).insert(id, token.key);
                    }
                    _ => {
                        tracing::warn!(client = %id, "keychain entry of an MCP client is not its token")
                    }
                },
                Ok(None) => {
                    tracing::warn!(client = %id, "keychain entry of an MCP client is missing")
                }
                Err(err) => {
                    tracing::warn!(client = %id, error = %err, "keychain entry of an MCP client unreadable")
                }
            }
        }
    }

    fn helper_info(&self) -> McpHelperInfo {
        let path = lock(&self.helper).clone();
        McpHelperInfo {
            found: path.as_ref().is_some_and(|p| p.is_file()),
            unstable_location: path.as_deref().is_some_and(place::unstable),
            path: path.map(|p| p.to_string_lossy().into_owned()),
        }
    }

    fn endpoint(&self) -> Option<&Result<Endpoint, String>> {
        self.endpoint.get()
    }

    fn status(&self) -> McpStatus {
        let enabled = self.settings.borrow().enabled;
        let host = self
            .host
            .get()
            .map(|host| host.status().borrow().clone())
            .unwrap_or(HostStatus::Stopped);
        let (state, mut error) = match host {
            HostStatus::Stopped => (McpHostState::Stopped, None),
            HostStatus::Listening => (McpHostState::Listening, None),
            HostStatus::Failed(err) => (McpHostState::Failed, Some(err)),
        };
        let endpoint = match self.endpoint() {
            Some(Ok(endpoint)) => Some(endpoint.as_str().to_owned()),
            Some(Err(err)) => {
                error.get_or_insert_with(|| err.clone());
                None
            }
            None => None,
        };
        let state = if enabled && endpoint.is_none() && error.is_some() {
            McpHostState::Failed
        } else {
            state
        };
        McpStatus {
            enabled,
            state,
            endpoint,
            error,
            helper: self.helper_info(),
        }
    }

    /// Starts the host task if it does not run yet.
    fn ensure_running(&self) {
        let (Some(host), Some(Ok(endpoint))) = (self.host.get(), self.endpoint()) else {
            return;
        };
        let mut run = lock(&self.run);
        if run.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let host = host.clone();
        let endpoint = endpoint.clone();
        let mut stop = self.stop.subscribe();
        *run = Some(tokio::spawn(async move {
            host.run(endpoint, async move {
                let _ = stop.wait_for(|stop| *stop).await;
            })
            .await;
            tracing::info!("MCP host ended");
        }));
    }

    fn configs(&self, token: &str) -> Vec<McpClientConfig> {
        let Some(Ok(endpoint)) = self.endpoint() else {
            return Vec::new();
        };
        let helper = lock(&self.helper)
            .clone()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let setup = Setup {
            helper: &helper,
            endpoint,
            token,
        };
        ClientKind::ALL
            .into_iter()
            .map(|kind| {
                let config = client_config(kind, &setup);
                McpClientConfig {
                    kind: match kind {
                        ClientKind::ClaudeDesktop => McpClientKind::ClaudeDesktop,
                        ClientKind::ClaudeCode => McpClientKind::ClaudeCode,
                        ClientKind::Cursor => McpClientKind::Cursor,
                        ClientKind::LmStudio => McpClientKind::LmStudio,
                    },
                    location: config.location.to_owned(),
                    content: config.content,
                    source: config.source.to_owned(),
                }
            })
            .collect()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

const fn to_host(access: McpAccountAccess) -> AccountAccess {
    match access {
        McpAccountAccess::Hidden => AccountAccess::Hidden,
        McpAccountAccess::Read => AccountAccess::Read,
        McpAccountAccess::ReadAndDrafts => AccountAccess::ReadAndDrafts,
    }
}

/// At the end of [`AppCore::start`]: endpoint, host, log and status tasks; the host task if MCP
/// is on.
pub(crate) async fn start<C: Connector>(core: &AppCore<C>) {
    let Ok(store) = core.store_ref() else { return };
    let state = core.mcp();
    let endpoint = place::resolve(store, &state.data_dir, core.now_ms()).await;
    if let Err(err) = &endpoint {
        tracing::error!(error = %err, "no MCP endpoint; AI access cannot be switched on");
    }
    let _ = state.endpoint.set(endpoint);
    let sink: Arc<dyn AccessLog> = Arc::clone(&state.sink) as Arc<dyn AccessLog>;
    let host = McpHost::new(
        Arc::new(CoreAccess(core.downgrade())),
        state.settings.subscribe(),
        sink,
    );
    let mut status = host.status();
    let _ = state.host.set(host);

    let mut tasks = Vec::new();
    if let Some(rx) = lock(&state.log_rx).take() {
        tasks.push(tokio::spawn(log::run(core.downgrade(), rx)));
    }
    let weak = core.downgrade();
    tasks.push(tokio::spawn(async move {
        while status.changed().await.is_ok() {
            let Some(core) = weak.upgrade() else { return };
            let status = core.mcp().status();
            core.emit_event(CoreEvent::McpStatusChanged { status });
        }
    }));
    lock(&state.tasks).extend(tasks);

    match core.mcp_reload().await {
        Ok(stored) if stored.enabled => state.ensure_running(),
        Ok(_) => {}
        Err(err) => tracing::error!(error = %err, "MCP settings could not be read"),
    }
}

/// In [`AppCore::shutdown`]: closes the endpoint and every connection.
pub(crate) async fn shutdown<C: Connector>(state: &McpState<C>) {
    state.stop.send_replace(true);
    let run = lock(&state.run).take();
    if let Some(run) = run
        && tokio::time::timeout(STOP_WAIT, run).await.is_err()
    {
        tracing::warn!("the MCP host did not stop in time");
    }
}

fn client_name(name: &str) -> Result<String, CoreError> {
    let name = name.trim();
    let chars = name.chars().count();
    if chars == 0
        || chars > usize::try_from(MAX_MCP_CLIENT_NAME_CHARS).unwrap_or(usize::MAX)
        || name.chars().any(char::is_control)
    {
        return Err(CoreError::InvalidArgument(format!(
            "a client name has 1 to {MAX_MCP_CLIENT_NAME_CHARS} characters and no control characters"
        )));
    }
    Ok(name.to_owned())
}

impl<C: Connector> AppCore<C> {
    /// Reads the stored settings, loads the keys while MCP is on and publishes them.
    pub(crate) async fn mcp_reload(&self) -> Result<StoredMcpState, CoreError> {
        let stored = self.store_ref()?.mcp_state().await?;
        if stored.enabled {
            self.mcp()
                .load_keys(self.credentials_ref(), &stored.clients)
                .await;
        }
        self.mcp().publish(&stored);
        Ok(stored)
    }

    /// The account's access level for AI clients right now.
    pub(crate) fn mcp_access_of(&self, account: &AccountId) -> AccountAccess {
        self.mcp().access_of(account)
    }

    /// The settings page "AI over MCP": switch, every account's access level, the clients and
    /// the endpoint's state.
    pub async fn get_mcp_settings(&self) -> Result<McpSettingsInfo, CoreError> {
        let store = self.store_ref()?;
        let stored = store.mcp_state().await?;
        let accounts = store
            .accounts()
            .await?
            .into_iter()
            .map(|account| McpAccountShare {
                account_id: account.id,
                access: stored
                    .accounts
                    .iter()
                    .find(|(id, _)| *id == account.id)
                    .map_or(McpAccountAccess::Hidden, |(_, access)| *access),
            })
            .collect();
        Ok(McpSettingsInfo {
            enabled: stored.enabled,
            accounts,
            clients: stored.clients.iter().map(StoredMcpClient::info).collect(),
            status: self.mcp().status(),
        })
    }

    /// Switches AI access on or off. Off is the emergency stop: the endpoint closes and every
    /// open connection is closed at once (a running tool call ends too); the clients stay
    /// registered. Emits `mcpStatusChanged`.
    pub async fn set_mcp_enabled(&self, enabled: bool) -> Result<McpSettingsInfo, CoreError> {
        {
            let _admin = self.mcp().admin.lock().await;
            self.store_ref()?
                .set_mcp_enabled(enabled, self.now_ms())
                .await?;
            self.mcp_reload().await?;
            if enabled {
                self.mcp().ensure_running();
            }
        }
        tracing::info!(enabled, "AI access over MCP switched");
        self.emit_event(CoreEvent::McpStatusChanged {
            status: self.mcp().status(),
        });
        self.get_mcp_settings().await
    }

    /// What AI clients may do with an account (`hidden`, `read`, `read_and_drafts`). Takes
    /// effect at the next request of every client.
    pub async fn set_mcp_account_access(
        &self,
        account: AccountId,
        access: McpAccountAccess,
    ) -> Result<McpSettingsInfo, CoreError> {
        {
            let _admin = self.mcp().admin.lock().await;
            let store = self.store_ref()?;
            if store.account(account).await?.is_none() {
                return Err(CoreError::NotFound(format!("account {account}")));
            }
            store
                .set_mcp_account_access(account, access, self.now_ms())
                .await?;
            self.mcp_reload().await?;
        }
        tracing::info!(%account, ?access, "AI access of an account changed");
        self.get_mcp_settings().await
    }

    /// Registers an AI client: a new token in the keychain, the client in the store. Returns
    /// the token and the ready-made configurations with it, once.
    pub async fn create_mcp_client(&self, name: String) -> Result<McpClientCreated, CoreError> {
        let name = client_name(&name)?;
        let token = ClientToken::generate().map_err(|err| CoreError::Internal(err.to_string()))?;
        let key = CredentialRef::for_mcp_client(&token.id);
        let text = token.expose_secret();
        let client = {
            let _admin = self.mcp().admin.lock().await;
            let store = self.store_ref()?;
            self.credentials_ref()
                .set(&key, Secret::new(text.clone()))
                .await?;
            let client = match store
                .insert_mcp_client(token.id.to_string(), name, key.to_string(), self.now_ms())
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    if let Err(cleanup) = self.credentials_ref().delete(&key).await {
                        tracing::error!(error = %cleanup, "keychain entry of a failed MCP client could not be removed");
                    }
                    return Err(err.into());
                }
            };
            lock(&self.mcp().keys).insert(token.id, token.key.clone());
            self.mcp_reload().await?;
            client
        };
        tracing::info!(client = %token.id, "AI client registered");
        Ok(McpClientCreated {
            client: client.info(),
            configs: self.mcp().configs(&text),
            token: text,
        })
    }

    /// Revokes a client: its token stops working and its open connections close at once; its
    /// key leaves the keychain. It stays listed (and in the access log) as revoked.
    pub async fn revoke_mcp_client(&self, client_id: String) -> Result<McpSettingsInfo, CoreError> {
        let id: ClientId = client_id
            .parse()
            .map_err(|_| CoreError::InvalidArgument("not an MCP client id".into()))?;
        {
            let _admin = self.mcp().admin.lock().await;
            let (client, revoked) = self
                .store_ref()?
                .revoke_mcp_client(id.to_string(), self.now_ms())
                .await?
                .ok_or_else(|| CoreError::NotFound(format!("MCP client {id}")))?;
            lock(&self.mcp().keys).remove(&id);
            self.mcp_reload().await?;
            if revoked {
                match CredentialRef::parse(&client.credential_ref) {
                    Ok(key) => {
                        if let Err(err) = self.credentials_ref().delete(&key).await {
                            // Revoked anyway: the store and the host no longer know the client.
                            tracing::error!(client = %id, error = %err, "keychain entry of a revoked MCP client could not be removed");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "revoked MCP client has a bad keychain reference")
                    }
                }
                tracing::info!(client = %id, "AI client revoked");
            }
        }
        self.get_mcp_settings().await
    }

    /// Registered and revoked clients, newest first.
    pub async fn list_mcp_clients(&self) -> Result<Vec<McpClientInfo>, CoreError> {
        Ok(self
            .store_ref()?
            .mcp_clients()
            .await?
            .iter()
            .map(StoredMcpClient::info)
            .collect())
    }

    /// The setup help without a token: helper path, endpoint and the configurations for
    /// Claude Desktop, Claude Code, Cursor and LM Studio with the placeholder `<token>`.
    pub fn get_mcp_setup(&self) -> McpSetup {
        let state = self.mcp();
        McpSetup {
            helper: state.helper_info(),
            endpoint: match state.endpoint() {
                Some(Ok(endpoint)) => Some(endpoint.as_str().to_owned()),
                _ => None,
            },
            configs: state.configs(TOKEN_PLACEHOLDER),
        }
    }

    /// The access log, newest first, below `before`. `limit` 1..=500.
    pub async fn list_mcp_access(
        &self,
        before: Option<McpAccessId>,
        limit: u32,
    ) -> Result<McpAccessPage, CoreError> {
        if limit == 0 || limit > MAX_MCP_ACCESS_PAGE {
            return Err(CoreError::InvalidArgument(format!(
                "the limit {limit} is outside 1..={MAX_MCP_ACCESS_PAGE}"
            )));
        }
        Ok(self.store_ref()?.mcp_access(before, limit).await?)
    }

    /// Whether AI access is on, the endpoint's state and the helper.
    pub fn get_mcp_status(&self) -> McpStatus {
        self.mcp().status()
    }

    /// "Test connection": starts the helper like an AI client and runs the handshake and two
    /// requests through it (see [`probe`]).
    pub async fn test_mcp_connection(&self) -> Result<McpConnectionTest, CoreError> {
        probe::run(self).await
    }

    /// Where the helper program is. The default is next to the app's executable; tests and
    /// unusual layouts set it.
    pub fn set_mcp_helper(&self, path: Option<PathBuf>) {
        *lock(&self.mcp().helper) = path;
    }
}

#[cfg(test)]
mod tests;
