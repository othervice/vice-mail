//! Stage 14, block D (decision 0041): one connector and one sender for both protocols. The
//! engine stays generic over one [`crate::Connector`]; [`AnyConnector`] opens an IMAP or a JMAP
//! connection depending on [`ConnectConfig::protocol`], which every connection of an account
//! takes from the account ([`for_account`]). [`AnySender`] does the same for sending (SMTP or
//! JMAP submission).
//!
//! The settings sync keeps its records in an IMAP folder; a JMAP account cannot hold it
//! ([`BackendError::Unsupported`]).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use mail_imap::{ImapBackend, ImapConnector, ImapSyncFolder, ImapWriter, SmtpSender};
use mail_jmap::{JmapBackend, JmapConnector, JmapSender, JmapWriter, TlsConfigFn};
use mail_model::search::ServerCriterion;
use mail_model::{
    Access, AppendUid, BackendError, BodySection, Capabilities, ConnectConfig, Envelope,
    ExpungeOutcome, FetchedSection, Flag, FlagUpdate, IdleOutcome, MailBackend, MailConnector,
    MailSender, MailWriter, MailboxRole, MailboxStatus, MessageHeader, MessageStructure, ModSeq,
    MoveOutcome, Protocol, RawMessage, RemoteMailbox, SelectedMailbox, SendFuture, SendOutcome,
    SmtpLimits, StoreMode, StoreOutcome, SyncFolderConnector, Uid, UidMapStore, UidSet,
    WriteConnector,
};
use mail_store::StoredAccount;

/// The app's TLS setup for the JMAP adapter (`mail_imap::tls`, one place for all connections).
pub(crate) fn tls() -> TlsConfigFn {
    Arc::new(mail_imap::tls::client_config)
}

/// Marks a connection as one of `account`'s: its protocol and its id (the JMAP UID map).
pub(crate) fn for_account(config: &mut ConnectConfig, account: &StoredAccount) {
    config.protocol = account.protocol;
    config.account = Some(account.id);
}

/// IMAP or JMAP, by [`ConnectConfig::protocol`].
#[derive(Clone)]
pub struct AnyConnector {
    imap: ImapConnector,
    jmap: JmapConnector,
}

impl Default for AnyConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl AnyConnector {
    pub fn new() -> Self {
        Self {
            imap: ImapConnector::new(),
            jmap: JmapConnector::new(tls()),
        }
    }
}

/// A connection of either adapter. One per connection and long-lived, so the size difference
/// of the variants does not matter.
#[allow(clippy::large_enum_variant)]
pub enum AnyBackend {
    Imap(ImapBackend),
    Jmap(JmapBackend),
}

/// A writer of either adapter (see [`AnyBackend`] on the variants' sizes).
#[allow(clippy::large_enum_variant)]
pub enum AnyWriter {
    Imap(ImapWriter),
    Jmap(JmapWriter),
}

macro_rules! both {
    ($self:ident, $b:ident => $e:expr) => {
        match $self {
            Self::Imap($b) => $e,
            Self::Jmap($b) => $e,
        }
    };
}

macro_rules! backend_methods {
    () => {
        fn capabilities(&self) -> &Capabilities {
            both!(self, b => b.capabilities())
        }

        fn access(&self) -> Access {
            both!(self, b => b.access())
        }

        async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
            both!(self, b => b.list_mailboxes().await)
        }

        async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
            both!(self, b => b.open_mailbox(name).await)
        }

        async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
            both!(self, b => b.search_uids(mailbox).await)
        }

        async fn fetch_headers(
            &mut self,
            mailbox: &SelectedMailbox,
            uids: &UidSet,
        ) -> Result<Vec<MessageHeader>, BackendError> {
            both!(self, b => b.fetch_headers(mailbox, uids).await)
        }

        async fn fetch_flags(
            &mut self,
            mailbox: &SelectedMailbox,
            uids: &UidSet,
            changed_since: Option<ModSeq>,
        ) -> Result<Vec<FlagUpdate>, BackendError> {
            both!(self, b => b.fetch_flags(mailbox, uids, changed_since).await)
        }

        async fn idle<S>(
            &mut self,
            mailbox: &SelectedMailbox,
            max: Duration,
            stop: S,
        ) -> Result<IdleOutcome, BackendError>
        where
            S: Future<Output = ()> + Send,
        {
            both!(self, b => b.idle(mailbox, max, stop).await)
        }

        async fn fetch_structure(
            &mut self,
            mailbox: &SelectedMailbox,
            uid: Uid,
        ) -> Result<Option<MessageStructure>, BackendError> {
            both!(self, b => b.fetch_structure(mailbox, uid).await)
        }

        async fn fetch_messages(
            &mut self,
            mailbox: &SelectedMailbox,
            uids: &UidSet,
        ) -> Result<Vec<RawMessage>, BackendError> {
            both!(self, b => b.fetch_messages(mailbox, uids).await)
        }

        async fn fetch_sections(
            &mut self,
            mailbox: &SelectedMailbox,
            uid: Uid,
            sections: &[BodySection],
        ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
            both!(self, b => b.fetch_sections(mailbox, uid, sections).await)
        }

        async fn search_text(
            &mut self,
            mailbox: &SelectedMailbox,
            criteria: &[ServerCriterion],
        ) -> Result<UidSet, BackendError> {
            both!(self, b => b.search_text(mailbox, criteria).await)
        }

        async fn noop(&mut self) -> Result<(), BackendError> {
            both!(self, b => b.noop().await)
        }

        async fn logout(self) -> Result<(), BackendError> {
            both!(self, b => b.logout().await)
        }
    };
}

impl MailBackend for AnyBackend {
    backend_methods!();
}

impl MailBackend for AnyWriter {
    backend_methods!();
}

impl MailWriter for AnyWriter {
    async fn mailbox_status(&mut self, name: &str) -> Result<MailboxStatus, BackendError> {
        both!(self, w => w.mailbox_status(name).await)
    }

    async fn store_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<ModSeq>,
    ) -> Result<StoreOutcome, BackendError> {
        both!(self, w => w.store_flags(mailbox, uids, mode, flags, unchanged_since).await)
    }

    async fn move_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        target: &str,
    ) -> Result<MoveOutcome, BackendError> {
        both!(self, w => w.move_messages(mailbox, uids, target).await)
    }

    async fn expunge_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<ExpungeOutcome, BackendError> {
        both!(self, w => w.expunge_messages(mailbox, uids).await)
    }

    async fn create_mailbox(
        &mut self,
        path: &str,
        role: Option<MailboxRole>,
    ) -> Result<String, BackendError> {
        both!(self, w => w.create_mailbox(path, role).await)
    }

    async fn append(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        message: &[u8],
    ) -> Result<Option<AppendUid>, BackendError> {
        both!(self, w => w.append(mailbox, flags, message).await)
    }

    async fn find_message_id(
        &mut self,
        mailbox: &SelectedMailbox,
        message_id: &str,
    ) -> Result<UidSet, BackendError> {
        both!(self, w => w.find_message_id(mailbox, message_id).await)
    }
}

impl MailConnector for AnyConnector {
    type Backend = AnyBackend;

    async fn connect(&self, config: &ConnectConfig) -> Result<AnyBackend, BackendError> {
        match config.protocol {
            Protocol::Imap => self.imap.connect(config).await.map(AnyBackend::Imap),
            Protocol::Jmap => self.jmap.connect(config).await.map(AnyBackend::Jmap),
        }
    }

    fn attach_uid_map_store(&self, store: Arc<dyn UidMapStore>) {
        self.jmap.attach_uid_map_store(store);
    }
}

impl WriteConnector for AnyConnector {
    type Writer = AnyWriter;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<AnyWriter, BackendError> {
        match config.protocol {
            Protocol::Imap => self.imap.connect_writer(config).await.map(AnyWriter::Imap),
            Protocol::Jmap => self.jmap.connect_writer(config).await.map(AnyWriter::Jmap),
        }
    }
}

impl SyncFolderConnector for AnyConnector {
    type Folder = ImapSyncFolder;

    async fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        folder: &str,
    ) -> Result<ImapSyncFolder, BackendError> {
        match config.protocol {
            Protocol::Imap => self.imap.connect_sync_folder(config, folder).await,
            Protocol::Jmap => Err(BackendError::Unsupported(
                "the settings sync on a JMAP account (choose an IMAP account for it)".into(),
            )),
        }
    }
}

/// SMTP or JMAP submission, by [`ConnectConfig::protocol`].
pub struct AnySender {
    smtp: SmtpSender,
    jmap: JmapSender,
}

impl Default for AnySender {
    fn default() -> Self {
        Self {
            smtp: SmtpSender::new(),
            jmap: JmapSender::new(tls()),
        }
    }
}

impl MailSender for AnySender {
    fn send<'a>(
        &'a self,
        config: &'a ConnectConfig,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> SendFuture<'a, SendOutcome> {
        match config.protocol {
            Protocol::Imap => MailSender::send(&self.smtp, config, envelope, message),
            Protocol::Jmap => MailSender::send(&self.jmap, config, envelope, message),
        }
    }

    fn verify<'a>(&'a self, config: &'a ConnectConfig) -> SendFuture<'a, ()> {
        match config.protocol {
            Protocol::Imap => MailSender::verify(&self.smtp, config),
            Protocol::Jmap => MailSender::verify(&self.jmap, config),
        }
    }

    fn limits<'a>(&'a self, config: &'a ConnectConfig) -> SendFuture<'a, SmtpLimits> {
        match config.protocol {
            Protocol::Imap => MailSender::limits(&self.smtp, config),
            Protocol::Jmap => MailSender::limits(&self.jmap, config),
        }
    }
}

#[cfg(test)]
mod tests {
    use mail_model::{Secret, Security, ServerSettings};

    use super::*;

    fn config(protocol: Protocol, access: Access) -> ConnectConfig {
        let server = ServerSettings {
            host: "127.0.0.1".into(),
            // Nothing listens here: a connection attempt fails, a refusal comes first.
            port: 9,
            security: Security::Tls,
            username: "u".into(),
        };
        let mut config = ConnectConfig::new(&server, Secret::new("pw".into()), access);
        config.protocol = protocol;
        config.timeouts.connect = Duration::from_secs(2);
        config
    }

    #[tokio::test]
    async fn jmap_refuses_read_only_writers_and_the_settings_sync_before_connecting() {
        let connector = AnyConnector::new();
        for protocol in [Protocol::Imap, Protocol::Jmap] {
            let Err(err) = connector
                .connect_writer(&config(protocol, Access::ReadOnly))
                .await
            else {
                panic!("{protocol:?}: a read-only writer was opened");
            };
            assert!(
                matches!(err, BackendError::ReadOnly(_)),
                "{protocol:?}: {err:?}"
            );
        }
        let err = connector
            .connect_sync_folder(&config(Protocol::Jmap, Access::ReadOnly), "Settings-Sync")
            .await
            .expect_err("refused");
        assert!(matches!(err, BackendError::Unsupported(_)), "{err:?}");
        let sender = AnySender::default();
        let envelope = Envelope {
            from: "a@example.test".into(),
            recipients: vec!["b@example.test".into()],
        };
        let err = sender
            .send(&config(Protocol::Jmap, Access::ReadOnly), &envelope, b"x")
            .await
            .expect_err("refused");
        assert!(matches!(err, BackendError::ReadOnly(_)), "{err:?}");
        // The JMAP side really tries JMAP (HTTPS to the port), not IMAP.
        let err = connector
            .connect(&config(Protocol::Jmap, Access::ReadOnly))
            .await
            .err()
            .expect("nothing listens");
        assert!(
            matches!(
                err,
                BackendError::ConnectionRefused | BackendError::ConnectTimeout
            ),
            "{err:?}"
        );
    }
}
