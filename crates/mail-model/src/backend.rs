//! The `MailBackend` trait: what the engine needs from a mail server, shaped after IMAP.
//!
//! One backend object is one authenticated connection. IDLE blocks its connection, so an
//! engine that idles on the inbox while syncing other folders opens a second backend.
//!
//! **Read-only protection is structural, in two layers:**
//! 1. This trait has no writing method at all. Stage 4 adds writes as a separate trait that
//!    extends it; code that only holds a `MailBackend` cannot express a write.
//! 2. The access mode is fixed when connecting ([`ConnectConfig::access`], from the account's
//!    `writes_enabled`). In [`Access::ReadOnly`] the adapter opens mailboxes with EXAMINE and its
//!    command layer only lets an allowlist through: CAPABILITY, NOOP, LOGOUT, STARTTLS, LOGIN,
//!    AUTHENTICATE, ID, ENABLE, LIST, STATUS, EXAMINE, (UID) FETCH without non-PEEK body
//!    sections, (UID) SEARCH, IDLE/DONE. Anything else (STORE, COPY, MOVE, APPEND, EXPUNGE,
//!    SELECT, CLOSE, CREATE, DELETE, RENAME, SUBSCRIBE, ...) fails with
//!    [`BackendError::ReadOnly`] before a byte is sent. An allowlist, not a denylist, so a
//!    command added later is refused until someone classifies it.
//!
//! Futures are `Send`, so the engine can drive a backend from a Tokio task. Implementations may
//! write the methods as `async fn`; the compiler checks the `Send` bound. Dropping a future
//! mid-command leaves the connection unusable (a half-read response is never reused): every
//! later call fails with [`BackendError::ConnectionLost`] and the engine reconnects.

use std::future::Future;
use std::time::Duration;

use crate::{
    BackendError, BodySection, Capabilities, FetchedSection, FlagUpdate, LoginAuth, MessageHeader,
    MessageStructure, ModSeq, RawMessage, RemoteMailbox, Secret, Security, ServerSettings, Uid,
    UidSet, UidValidity,
};

/// Whether the connection may change anything on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    /// EXAMINE, and the adapter refuses every writing command (see module docs).
    ReadOnly,
    /// SELECT; writes become possible in stage 4. Only for accounts with `writes_enabled`.
    ReadWrite,
}

/// Time limits of one connection. A limit counts time without progress, so a large literal
/// that keeps arriving never times out, but a silent connection does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// TCP connect plus TLS handshake.
    pub connect: Duration,
    /// Waiting for the next byte of a response.
    pub read: Duration,
    /// Waiting for the socket to accept the next bytes of a command.
    pub write: Duration,
}

impl Default for Timeouts {
    /// 15 s to connect, 30 s for reads and writes (decision 0002).
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            read: Duration::from_secs(30),
            write: Duration::from_secs(30),
        }
    }
}

/// An additional trusted root certificate (PEM), **only for tests** against the local test
/// server (`TESTMAIL_CA_FILE`, decision 0002). Certificates are still checked against the
/// system store as well. Has no `Deserialize`, so it can never come from the UI or the database.
#[derive(Clone, PartialEq, Eq)]
pub struct ExtraRootCa {
    pem: Vec<u8>,
}

impl ExtraRootCa {
    pub const fn from_pem(pem: Vec<u8>) -> Self {
        Self { pem }
    }

    pub fn pem(&self) -> &[u8] {
        &self.pem
    }
}

impl std::fmt::Debug for ExtraRootCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExtraRootCa({} bytes)", self.pem.len())
    }
}

/// Everything needed to open one authenticated connection.
#[derive(Debug, Clone)]
pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
    /// The password, or the OAuth 2.0 access token when `auth` is [`LoginAuth::Bearer`].
    pub password: Secret<String>,
    /// Stage 14 (decision 0042): how `password` is used to log in.
    pub auth: LoginAuth,
    pub access: Access,
    pub timeouts: Timeouts,
    /// `None` in the product. Set only by tests (the engine's test configuration).
    pub extra_root_ca: Option<ExtraRootCa>,
    /// Stage 14 (decision 0041): which adapter serves the connection, from the account. A
    /// connector for one protocol ignores it; the engine's combined connector picks by it.
    pub protocol: crate::Protocol,
    /// The account the connection is for; `None` while an account is being set up. Adapters
    /// that keep state per account (the JMAP UID map) key it by this.
    pub account: Option<crate::AccountId>,
}

impl ConnectConfig {
    /// Config for `server` with default time limits and no extra root.
    pub fn new(server: &ServerSettings, password: Secret<String>, access: Access) -> Self {
        Self {
            host: server.host.clone(),
            port: server.port,
            security: server.security,
            username: server.username.clone(),
            password,
            auth: LoginAuth::Password,
            access,
            timeouts: Timeouts::default(),
            extra_root_ca: None,
            protocol: crate::Protocol::Imap,
            account: None,
        }
    }
}

/// State of the mailbox opened with [`MailBackend::open_mailbox`]. Mailbox-scoped calls take it
/// back as proof of which mailbox and UIDVALIDITY the caller means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedMailbox {
    /// Wire name, as in [`RemoteMailbox::name`].
    pub name: String,
    pub uid_validity: UidValidity,
    /// UIDNEXT. If the server omits it, the adapter derives it (highest UID + 1, or 1).
    pub uid_next: Uid,
    /// Number of messages (EXISTS).
    pub exists: u32,
    /// HIGHESTMODSEQ; `None` without CONDSTORE or for a `NOMODSEQ` mailbox.
    pub highest_modseq: Option<ModSeq>,
    /// Opened with EXAMINE (or the server answered `[READ-ONLY]`).
    pub read_only: bool,
    /// PERMANENTFLAGS contains `\*`. Only meaningful when opened read-write; EXAMINE reports
    /// no permanent flags.
    pub keywords_allowed: bool,
}

/// Why [`MailBackend::idle`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleOutcome {
    /// The server reported a change to the open mailbox (EXISTS, EXPUNGE, FETCH or VANISHED),
    /// now or since the mailbox was opened. Run an incremental sync.
    Changed,
    /// `max` elapsed without changes. The connection is usable; IDLE again.
    TimedOut,
    /// The `stop` future completed. IDLE was ended with DONE; the connection is usable.
    Stopped,
}

/// One authenticated connection to a mail server, read-only operations: sync (stage 2) and
/// bodies (stage 3; the body methods have defaults that fail with `Unsupported`, so test fakes
/// need not implement them).
///
/// Mailbox-scoped methods take the [`SelectedMailbox`] that [`MailBackend::open_mailbox`]
/// returned. If it is not the currently open mailbox (other name or UIDVALIDITY), they fail with
/// [`BackendError::InvalidState`] instead of applying UIDs to the wrong mailbox.
pub trait MailBackend: Send {
    /// Capabilities after login (and after `ENABLE CONDSTORE`/`QRESYNC`, which the adapter sends
    /// right after login when available).
    fn capabilities(&self) -> &Capabilities;

    /// Access mode fixed at connect time.
    fn access(&self) -> Access;

    /// All mailboxes with SPECIAL-USE attributes and, where possible, STATUS counters
    /// (LIST-STATUS in one round trip, otherwise STATUS per selectable mailbox).
    fn list_mailboxes(
        &mut self,
    ) -> impl Future<Output = Result<Vec<RemoteMailbox>, BackendError>> + Send;

    /// Opens a mailbox by wire name: EXAMINE in read-only mode, SELECT in read-write mode, with
    /// CONDSTORE when available. A mailbox that does not exist fails with
    /// [`BackendError::MailboxNotFound`].
    fn open_mailbox(
        &mut self,
        name: &str,
    ) -> impl Future<Output = Result<SelectedMailbox, BackendError>> + Send;

    /// Every UID in the open mailbox (`UID SEARCH ALL`, ESEARCH when available). Used to plan
    /// the windows of the first sync and to find deleted messages.
    fn search_uids(
        &mut self,
        mailbox: &SelectedMailbox,
    ) -> impl Future<Output = Result<UidSet, BackendError>> + Send;

    /// Header data for the given UIDs (one window, at most a few hundred). UIDs that no longer
    /// exist are simply missing from the result; order is unspecified. Never sets `\Seen`.
    fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> impl Future<Output = Result<Vec<MessageHeader>, BackendError>> + Send;

    /// Flags (and MODSEQ) for the given UIDs. With `changed_since`, only messages whose MODSEQ
    /// is greater (CONDSTORE `CHANGEDSINCE`; the adapter fails with
    /// [`BackendError::Unsupported`] without CONDSTORE). Use `UidSet::range(1, known_max)` with
    /// `changed_since` for the CONDSTORE path, windows without it for the fallback.
    fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        changed_since: Option<ModSeq>,
    ) -> impl Future<Output = Result<Vec<FlagUpdate>, BackendError>> + Send;

    /// Waits for changes in the open mailbox with IDLE, for at most `max` (the engine uses
    /// about 5 minutes, decision 0002), or until `stop` completes. Returns
    /// [`IdleOutcome::Changed`] at once if changes arrived since the mailbox was opened or the
    /// last `idle` returned. Fails with [`BackendError::Unsupported`] without the IDLE
    /// capability; a BYE or a dead connection is [`BackendError::ConnectionLost`] or
    /// [`BackendError::Timeout`] (noticed at the latest after `max` plus the read limit).
    fn idle<S>(
        &mut self,
        mailbox: &SelectedMailbox,
        max: Duration,
        stop: S,
    ) -> impl Future<Output = Result<IdleOutcome, BackendError>> + Send
    where
        S: Future<Output = ()> + Send;

    /// RFC822.SIZE and BODYSTRUCTURE of one message (stage 3), to plan the fetch of a large
    /// message ([`MessageStructure::fetch_plan`]). `None` if the UID does not exist (any more).
    ///
    /// The default fails with [`BackendError::Unsupported`]: for backends (test fakes) that
    /// cannot fetch bodies.
    fn fetch_structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> impl Future<Output = Result<Option<MessageStructure>, BackendError>> + Send {
        let _ = (mailbox, uid);
        std::future::ready(Err(BackendError::Unsupported(
            "fetching message bodies".into(),
        )))
    }

    /// Whole raw messages (`BODY.PEEK[]`, never sets `\Seen`), for opening and prefetching.
    /// The caller keeps a batch small in bytes (RFC822.SIZE is stored), because the result is
    /// held in memory. UIDs that no longer exist are missing from the result; order is
    /// unspecified.
    ///
    /// The default fails with [`BackendError::Unsupported`].
    fn fetch_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> impl Future<Output = Result<Vec<RawMessage>, BackendError>> + Send {
        let _ = (mailbox, uids);
        std::future::ready(Err(BackendError::Unsupported(
            "fetching message bodies".into(),
        )))
    }

    /// Sections of one message in one FETCH (`BODY.PEEK[HEADER]`, `BODY.PEEK[1.2.MIME]`,
    /// `BODY.PEEK[1.2]`, never sets `\Seen`), raw as sent: part contents are still
    /// transfer-encoded. `None` if the UID does not exist (any more); sections the server did not
    /// return are missing from the result.
    ///
    /// The default fails with [`BackendError::Unsupported`].
    fn fetch_sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> impl Future<Output = Result<Option<Vec<FetchedSection>>, BackendError>> + Send {
        let _ = (mailbox, uid, sections);
        std::future::ready(Err(BackendError::Unsupported(
            "fetching message bodies".into(),
        )))
    }

    /// UIDs of the open mailbox matching all `criteria` (`UID SEARCH`, stage 7: "search on the
    /// server"). Read-only (SEARCH is on the allowlist). Strings go as quoted strings, with
    /// `CHARSET UTF-8` when one is not ASCII.
    ///
    /// The default fails with [`BackendError::Unsupported`].
    fn search_text(
        &mut self,
        mailbox: &SelectedMailbox,
        criteria: &[crate::search::ServerCriterion],
    ) -> impl Future<Output = Result<UidSet, BackendError>> + Send {
        let _ = (mailbox, criteria);
        std::future::ready(Err(BackendError::Unsupported(
            "searching on the server".into(),
        )))
    }

    /// Stage 20 (decision 0060): how full the mailbox is, `None` when the server does not
    /// report storage (IMAP: no `QUOTA` capability, or no `STORAGE` resource for the inbox's
    /// quota root; JMAP: no quota for mail). Read-only (`GETQUOTAROOT` is on the allowlist).
    ///
    /// The default reports nothing (test fakes).
    fn quota(
        &mut self,
    ) -> impl Future<Output = Result<Option<crate::StorageQuota>, BackendError>> + Send {
        std::future::ready(Ok(None))
    }

    /// NOOP: checks that the connection is alive and collects pending server notices.
    fn noop(&mut self) -> impl Future<Output = Result<(), BackendError>> + Send;

    /// LOGOUT and close. Errors are only worth logging; the connection is gone either way.
    fn logout(self) -> impl Future<Output = Result<(), BackendError>> + Send
    where
        Self: Sized;
}

/// Opens backends. The IMAP adapter's connector does TCP, TLS or STARTTLS (certificates checked
/// against the system store, plus [`ConnectConfig::extra_root_ca`] in tests), LOGIN, CAPABILITY
/// and ENABLE. A server without IMAP4rev1 fails with [`BackendError::Unsupported`].
pub trait MailConnector: Send + Sync {
    type Backend: MailBackend + 'static;

    fn connect(
        &self,
        config: &ConnectConfig,
    ) -> impl Future<Output = Result<Self::Backend, BackendError>> + Send;

    /// Stage 14 (decision 0041): the engine hands over its store once it is open. Adapters
    /// that number messages themselves (JMAP) keep that numbering there; until then, and for
    /// connections without an account, they keep it in memory. The default ignores it.
    fn attach_uid_map_store(&self, store: std::sync::Arc<dyn crate::UidMapStore>) {
        let _ = store;
    }
}

#[cfg(test)]
mod tests {
    use std::future::{Ready, ready};

    use super::*;

    /// Minimal implementation: proves the trait can be implemented with `async fn` and that the
    /// futures are `Send` (required to drive a backend from `tokio::spawn`).
    struct Fake {
        caps: Capabilities,
    }

    impl MailBackend for Fake {
        fn capabilities(&self) -> &Capabilities {
            &self.caps
        }

        fn access(&self) -> Access {
            Access::ReadOnly
        }

        async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
            Ok(Vec::new())
        }

        async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
            Ok(SelectedMailbox {
                name: name.to_owned(),
                uid_validity: UidValidity(1),
                uid_next: 1,
                exists: 0,
                highest_modseq: None,
                read_only: true,
                keywords_allowed: false,
            })
        }

        async fn search_uids(&mut self, _: &SelectedMailbox) -> Result<UidSet, BackendError> {
            Ok(UidSet::new())
        }

        async fn fetch_headers(
            &mut self,
            _: &SelectedMailbox,
            _: &UidSet,
        ) -> Result<Vec<MessageHeader>, BackendError> {
            Ok(Vec::new())
        }

        async fn fetch_flags(
            &mut self,
            _: &SelectedMailbox,
            _: &UidSet,
            _: Option<ModSeq>,
        ) -> Result<Vec<FlagUpdate>, BackendError> {
            Ok(Vec::new())
        }

        async fn idle<S>(
            &mut self,
            _: &SelectedMailbox,
            _: Duration,
            stop: S,
        ) -> Result<IdleOutcome, BackendError>
        where
            S: Future<Output = ()> + Send,
        {
            stop.await;
            Ok(IdleOutcome::Stopped)
        }

        async fn noop(&mut self) -> Result<(), BackendError> {
            Ok(())
        }

        async fn logout(self) -> Result<(), BackendError> {
            Ok(())
        }
    }

    struct FakeConnector;

    impl MailConnector for FakeConnector {
        type Backend = Fake;

        async fn connect(&self, config: &ConnectConfig) -> Result<Fake, BackendError> {
            if config.password.expose_secret().is_empty() {
                return Err(BackendError::AuthFailed(String::new()));
            }
            Ok(Fake {
                caps: Capabilities::from_names(["IMAP4rev1", "IDLE"]),
            })
        }
    }

    fn assert_send<T: Send>(_: &T) {}

    /// A generic engine function, the way `mail-engine` will use the traits.
    async fn first_pass<C: MailConnector>(
        connector: &C,
        config: &ConnectConfig,
    ) -> Result<usize, BackendError> {
        let mut backend = connector.connect(config).await?;
        let mut count = 0;
        for remote in backend.list_mailboxes().await? {
            let mailbox = backend.open_mailbox(&remote.name).await?;
            let uids = backend.search_uids(&mailbox).await?;
            let (window, _) = uids.split_highest(500);
            count += backend.fetch_headers(&mailbox, &window).await?.len();
        }
        let inbox = backend.open_mailbox("INBOX").await?;
        let stop: Ready<()> = ready(());
        backend.idle(&inbox, Duration::from_secs(300), stop).await?;
        backend.logout().await?;
        Ok(count)
    }

    #[test]
    fn body_methods_default_to_unsupported() {
        let mut fake = Fake {
            caps: Capabilities::from_names(["IMAP4rev1"]),
        };
        let mailbox = SelectedMailbox {
            name: "INBOX".into(),
            uid_validity: UidValidity(1),
            uid_next: 2,
            exists: 1,
            highest_modseq: None,
            read_only: true,
            keywords_allowed: false,
        };
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        let unsupported = |poll: std::task::Poll<Result<(), BackendError>>| {
            matches!(
                poll,
                std::task::Poll::Ready(Err(BackendError::Unsupported(_)))
            )
        };
        {
            let fut = fake.fetch_structure(&mailbox, 1);
            assert_send(&fut);
            let mut fut = std::pin::pin!(fut);
            assert!(unsupported(fut.as_mut().poll(&mut cx).map(|r| r.map(drop))));
        }
        let uids = UidSet::range(1, 1);
        {
            let fut = fake.fetch_messages(&mailbox, &uids);
            assert_send(&fut);
            let mut fut = std::pin::pin!(fut);
            assert!(unsupported(fut.as_mut().poll(&mut cx).map(|r| r.map(drop))));
        }
        let fut = fake.fetch_sections(&mailbox, 1, &[BodySection::Header]);
        assert_send(&fut);
        let mut fut = std::pin::pin!(fut);
        assert!(unsupported(fut.as_mut().poll(&mut cx).map(|r| r.map(drop))));
    }

    #[test]
    fn futures_are_send() {
        let server = ServerSettings {
            host: "127.0.0.1".into(),
            port: 1993,
            security: Security::Tls,
            username: "alice".into(),
        };
        let config = ConnectConfig::new(&server, Secret::new("pw".into()), Access::ReadOnly);
        assert_eq!(config.timeouts, Timeouts::default());
        assert!(config.extra_root_ca.is_none());
        assert_eq!(config.protocol, crate::Protocol::Imap);
        assert!(config.account.is_none());
        let connector = FakeConnector;
        let fut = first_pass(&connector, &config);
        assert_send(&fut);
        // Every fake step is ready at once, so one poll runs the whole pass.
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(result) => assert_eq!(result, Ok(0)),
            std::task::Poll::Pending => panic!("fake backend should not wait"),
        }

        let debug = format!("{config:?}");
        assert!(!debug.contains("\"pw\""), "{debug}");
        let ca = ExtraRootCa::from_pem(b"-----BEGIN CERTIFICATE-----".to_vec());
        assert_eq!(format!("{ca:?}"), "ExtraRootCa(27 bytes)");
    }
}
