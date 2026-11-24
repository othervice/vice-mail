//! [`JmapConnector`]: opens [`JmapBackend`]s and [`JmapWriter`]s.

use std::sync::Arc;

use mail_model::{
    Access, AccountId, BackendError, ConnectConfig, ExtraRootCa, MailConnector,
    SyncFolderConnector, UidMapStore, WriteConnector,
};

use crate::backend::JmapBackend;
use crate::client::Client;
use crate::map::Maps;
use crate::session;
use crate::sync_folder::JmapSyncFolder;
use crate::writer::JmapWriter;

/// Builds the TLS client configuration (system trust store, plus the test CA in tests). The
/// engine passes `mail_imap::tls::client_config`, so every connection of the app trusts the
/// same roots with the same provider.
pub type TlsConfigFn = Arc<
    dyn Fn(Option<&ExtraRootCa>) -> Result<Arc<rustls::ClientConfig>, BackendError> + Send + Sync,
>;

/// Opens JMAP connections. Shares one UID map per account between all of them.
#[derive(Clone)]
pub struct JmapConnector {
    tls: TlsConfigFn,
    maps: Arc<Maps>,
    insecure_loopback: bool,
    log: Option<crate::client::MethodLog>,
}

impl JmapConnector {
    pub fn new(tls: TlsConfigFn) -> Self {
        Self {
            tls,
            maps: Arc::new(Maps::default()),
            insecure_loopback: false,
            log: None,
        }
    }

    /// Plain HTTP to loopback addresses, for this crate's scripted test server only.
    /// Records the methods its connections send (test support).
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_method_log(mut self, log: crate::client::MethodLog) -> Self {
        self.log = Some(log);
        self
    }

    #[cfg(test)]
    pub(crate) fn insecure_loopback_for_tests(mut self) -> Self {
        self.insecure_loopback = true;
        self
    }

    /// A read connection for `account` (its UID map is shared with the account's other
    /// connections and stored); `None` for a connection without account (setup), whose map is
    /// its own and not stored.
    pub async fn connect_for(
        &self,
        config: &ConnectConfig,
        account: Option<AccountId>,
    ) -> Result<JmapBackend, BackendError> {
        let client = Client::new(config, &self.tls, self.insecure_loopback, self.log.clone())?;
        let session = session::fetch(&client, config).await?;
        let map = self.maps.for_account(account);
        map.lock().await.prepare(&session.account_id).await?;
        tracing::debug!(
            account = ?account,
            push = session.event_source_url.is_some(),
            submission = session.submission,
            "JMAP session open"
        );
        Ok(JmapBackend::new(client, session, map))
    }
}

impl JmapConnector {
    /// A writer for `account`; a read-only configuration is refused before any request.
    pub async fn connect_writer_for(
        &self,
        config: &ConnectConfig,
        account: Option<AccountId>,
    ) -> Result<JmapWriter, BackendError> {
        if config.access != Access::ReadWrite {
            return Err(BackendError::ReadOnly("writer connection".into()));
        }
        JmapWriter::new(self.connect_for(config, account).await?)
    }
}

impl MailConnector for JmapConnector {
    type Backend = JmapBackend;

    /// A connection for `config.account` (see [`JmapConnector::connect_for`]).
    async fn connect(&self, config: &ConnectConfig) -> Result<JmapBackend, BackendError> {
        self.connect_for(config, config.account).await
    }

    fn attach_uid_map_store(&self, store: Arc<dyn UidMapStore>) {
        self.maps.attach(store);
    }
}

impl WriteConnector for JmapConnector {
    type Writer = JmapWriter;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<JmapWriter, BackendError> {
        self.connect_writer_for(config, config.account).await
    }
}

impl SyncFolderConnector for JmapConnector {
    type Folder = JmapSyncFolder;

    /// Stage 14: the settings sync folder on a JMAP account. The connection is read-only
    /// whatever the configuration says; only the folder's own writes pass (see
    /// `sync_folder`). It has no UID map of the account.
    async fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        folder: &str,
    ) -> Result<JmapSyncFolder, BackendError> {
        let mut config = config.clone();
        config.access = Access::ReadOnly;
        let client = Client::new(&config, &self.tls, self.insecure_loopback, self.log.clone())?;
        let session = session::fetch(&client, &config).await?;
        JmapSyncFolder::new(client, session, folder)
    }
}
