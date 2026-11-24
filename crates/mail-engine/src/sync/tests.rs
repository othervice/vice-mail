//! The engine against the in-memory fake server ([`super::fake`]), through `AppCore` with the
//! real store in a temporary directory: windows, events, backoff, panics, removal, wake.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use mail_model::{
    Access, AccountId, AccountSettings, AccountState, AccountStatus, BackendError, CoreEvent,
    ErrorCode, Mailbox, MailboxId, MailboxRole, Secret, Security, ServerSettings,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use super::SyncConfig;
use super::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::clock::{Clock, SystemClock};
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretOp};
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

const WAIT: Duration = Duration::from_secs(10);

/// The system clock plus an offset the test can move (a resume from sleep).
#[derive(Debug, Default)]
struct OffsetClock(AtomicI64);

impl Clock for OffsetClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst)
    }
}

fn sync_config() -> SyncConfig {
    SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(50),
            max: Duration::from_millis(400),
            jitter: 0.5,
        },
        idle_section: Duration::from_secs(2),
        // Long: tests that need a poll set it themselves.
        poll_interval: Duration::from_secs(600),
        inbox_poll_without_idle: Duration::from_secs(600),
        window: 50,
        first_window: 10,
        flag_window: 20,
        wake_check: Duration::from_millis(50),
        wake_threshold: Duration::from_secs(5),
        shutdown_grace: Duration::from_millis(500),
        ..SyncConfig::default()
    }
}

struct Harness {
    dir: TempDir,
    core: AppCore<FakeConnector>,
    keychain: Arc<MemorySecretStore>,
    clock: Arc<OffsetClock>,
    events: broadcast::Receiver<CoreEvent>,
}

impl Harness {
    async fn new(server: Arc<FakeServer>, config: SyncConfig) -> Self {
        Self::reopen(
            TempDir::new().expect("temp dir"),
            server,
            Arc::default(),
            config,
        )
        .await
    }

    async fn reopen(
        dir: TempDir,
        server: Arc<FakeServer>,
        keychain: Arc<MemorySecretStore>,
        config: SyncConfig,
    ) -> Self {
        let clock = Arc::new(OffsetClock::default());
        let services = CoreServices {
            credentials: Credentials::new(Arc::clone(&keychain)),
            connector: FakeConnector(Arc::clone(&server)),
            clock: Arc::clone(&clock) as Arc<dyn Clock>,
            sync: config,
            reading: crate::ReadingServices::default(),
            outbox: None,
        };
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0.0.0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
        .await;
        let events = core.subscribe();
        Self {
            dir,
            core,
            keychain,
            clock,
            events,
        }
    }

    async fn add(&self) -> AccountId {
        self.core
            .add_account(settings("alice@fake.test"), Secret::new(PASSWORD.into()))
            .await
            .expect("add account")
            .id
    }

    /// Waits for an event matching `pred`; returns it and the events seen before it.
    async fn event(&mut self, pred: impl Fn(&CoreEvent) -> bool) -> (CoreEvent, Vec<CoreEvent>) {
        let mut before = Vec::new();
        let deadline = Instant::now() + WAIT;
        loop {
            let event = tokio::time::timeout_at(deadline, self.events.recv())
                .await
                .unwrap_or_else(|_| panic!("no matching event within {WAIT:?}; saw {before:#?}"))
                .expect("event stream");
            if pred(&event) {
                return (event, before);
            }
            before.push(event);
        }
    }

    async fn status(
        &mut self,
        pred: impl Fn(&AccountStatus) -> bool,
    ) -> (AccountStatus, Vec<CoreEvent>) {
        let (event, before) = self
            .event(|e| matches!(e, CoreEvent::AccountStatusChanged { status, .. } if pred(status)))
            .await;
        match event {
            CoreEvent::AccountStatusChanged { status, .. } => (status, before),
            _ => unreachable!(),
        }
    }

    /// Waits until the (only) account is idle; returns at once if it is.
    async fn idle(&mut self) {
        let current = self.core.list_accounts().await.expect("accounts");
        if current
            .first()
            .is_some_and(|a| a.status.state == AccountState::Idle)
        {
            return;
        }
        self.status(|s| s.state == AccountState::Idle).await;
    }

    /// Like [`Harness::idle`] for any state.
    async fn state(&mut self, state: AccountState) -> AccountStatus {
        let current = self.core.list_accounts().await.expect("accounts");
        if let Some(account) = current.into_iter().find(|a| a.status.state == state) {
            return account.status;
        }
        self.status(|s| s.state == state).await.0
    }

    /// Lets the engine settle: no pass is requested for `quiet`.
    async fn settle(&mut self) {
        tokio::time::sleep(Duration::from_millis(150)).await;
        while self.events.try_recv().is_ok() {}
    }

    async fn mailboxes(&self, account: AccountId) -> Vec<Mailbox> {
        self.core.list_mailboxes(account).await.expect("mailboxes")
    }

    async fn mailbox(&self, account: AccountId, path: &str) -> Mailbox {
        self.mailboxes(account)
            .await
            .into_iter()
            .find(|m| m.path == path)
            .unwrap_or_else(|| panic!("no mailbox {path}"))
    }

    async fn count(&self, mailbox: MailboxId) -> usize {
        let mut cursor = None;
        let mut count = 0;
        loop {
            let page = self
                .core
                .list_messages(mailbox, cursor, 500)
                .await
                .expect("page");
            count += page.items.len();
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return count,
            }
        }
    }
}

fn settings(email: &str) -> AccountSettings {
    let server = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "alice".into(),
    };
    AccountSettings {
        credential: Default::default(),
        protocol: Default::default(),
        email: email.into(),
        display_name: "Alice".into(),
        imap: server("imap.fake.test", 993),
        smtp: server("smtp.fake.test", 465),
    }
}

fn progress_of(events: &[CoreEvent]) -> Vec<(MailboxId, u32, u32)> {
    events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::AccountStatusChanged { status, .. } => {
                status.progress.map(|p| (p.mailbox_id, p.done, p.total))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_sync_runs_newest_first_in_windows_inbox_first() {
    let server = FakeServer::new();
    server.deliver("Archive", 30, true);
    server.deliver("INBOX", 125, false);
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    let (_, before_idle) = h.status(|s| s.state == AccountState::Idle).await;

    let inbox = h.mailbox(id, "INBOX").await;
    let archive = h.mailbox(id, "Archive").await;
    assert_eq!(inbox.role, MailboxRole::Inbox);
    assert_eq!(h.mailbox(id, "Sent").await.role, MailboxRole::Sent);
    assert_eq!((inbox.total, inbox.unread), (125, 125));
    assert_eq!((archive.total, archive.unread), (30, 0));
    assert_eq!(h.count(inbox.id).await, 125);

    // Progress: the inbox first (a small first window, then windows of 50), then the archive.
    let progress = progress_of(&before_idle);
    assert_eq!(
        progress,
        [
            (inbox.id, 10, 125),
            (inbox.id, 60, 125),
            (inbox.id, 110, 125),
            (inbox.id, 125, 125),
            (archive.id, 10, 30),
            (archive.id, 30, 30),
        ]
    );
    // One messagesChanged per stored window, never a newMail during the first sync.
    let inbox_changes = before_idle
        .iter()
        .filter(|e| matches!(e, CoreEvent::MessagesChanged { mailbox_id, .. } if *mailbox_id == inbox.id))
        .count();
    assert_eq!(inbox_changes, 4);
    assert!(
        !before_idle
            .iter()
            .any(|e| matches!(e, CoreEvent::NewMail { .. }))
    );
    // Every message was fetched exactly once, read-only.
    assert_eq!(server.read(|s| s.fetched), 155);
    assert!(server.read(|s| s.access.iter().all(|a| *a == Access::ReadOnly)));

    // The first page is the newest mail.
    let page = h.core.list_messages(inbox.id, None, 5).await.expect("page");
    assert_eq!(page.items[0].subject, "Message 155");
    let status = h.core.list_accounts().await.expect("accounts")[0]
        .status
        .clone();
    assert_eq!(status.state, AccountState::Idle);
    assert!(status.last_sync_at.is_some() && status.error.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_mail_flags_and_deletions_from_outside_arrive_without_polling() {
    let server = FakeServer::new();
    let uids = server.deliver("INBOX", 5, true);
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.idle().await;
    h.settle().await;
    let inbox = h.mailbox(id, "INBOX").await;

    // New mail: IDLE wakes the engine; one newMail with the number of unread messages.
    let started = Instant::now();
    server.deliver("INBOX", 2, false);
    let (event, _) = h.event(|e| matches!(e, CoreEvent::NewMail { .. })).await;
    assert_eq!(
        event,
        CoreEvent::NewMail {
            account_id: id,
            mailbox_id: inbox.id,
            count: 2
        }
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    h.idle().await;
    assert_eq!(h.mailbox(id, "INBOX").await.unread, 2);

    // Flag change from outside (CONDSTORE).
    server.set_flagged("INBOX", uids[0], true);
    h.event(|e| matches!(e, CoreEvent::MessagesChanged { .. }))
        .await;
    h.idle().await;
    let page = h
        .core
        .list_messages(inbox.id, None, 50)
        .await
        .expect("page");
    assert_eq!(page.items.iter().filter(|m| m.flagged).count(), 1);

    // Deletion from outside.
    server.expunge("INBOX", uids[1]);
    h.event(|e| matches!(e, CoreEvent::MessagesChanged { .. }))
        .await;
    h.idle().await;
    assert_eq!(h.count(inbox.id).await, 6);
    assert_eq!(h.mailbox(id, "INBOX").await.total, 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_folders_and_servers_without_condstore_are_polled() {
    let server = FakeServer::new();
    server.with(|s| s.capabilities = vec!["IMAP4rev1"]);
    let archived = server.deliver("Archive", 3, true);
    let mut config = sync_config();
    config.inbox_poll_without_idle = Duration::from_millis(200);
    config.poll_interval = Duration::from_millis(300);
    let mut h = Harness::new(Arc::clone(&server), config).await;
    let id = h.add().await;
    h.idle().await;
    let archive = h.mailbox(id, "Archive").await;
    let inbox = h.mailbox(id, "INBOX").await;

    server.deliver("INBOX", 1, false);
    server.expunge("Archive", archived[0]);
    server.set_flagged("Archive", archived[1], true);
    let deadline = Instant::now() + WAIT;
    loop {
        let page = h
            .core
            .list_messages(archive.id, None, 50)
            .await
            .expect("page");
        if h.count(inbox.id).await == 1
            && page.items.len() == 2
            && page.items.iter().any(|m| m.flagged)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "poll did not pick up the changes"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // No IDLE without the capability: one connection only.
    assert_eq!(server.open_connections(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uidvalidity_change_rebuilds_the_mailbox_and_keeps_local_ids() {
    let server = FakeServer::new();
    server.deliver("INBOX", 20, true);
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.idle().await;
    let inbox = h.mailbox(id, "INBOX").await;
    let before = h
        .core
        .list_messages(inbox.id, None, 50)
        .await
        .expect("page");

    server.recreate("INBOX");
    // The IDLE connection breaks (as on a real server); the reconnect rebuilds the mailbox.
    h.status(|s| s.state == AccountState::Syncing).await;
    h.idle().await;
    let after = h
        .core
        .list_messages(inbox.id, None, 50)
        .await
        .expect("page");
    assert_eq!(after.items.len(), 20);
    let ids = |page: &mail_model::MessagePage| page.items.iter().map(|m| m.id).collect::<Vec<_>>();
    assert_eq!(
        ids(&before),
        ids(&after),
        "messages merged back by Message-ID"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cut_connection_mid_first_sync_resumes_where_it_stopped() {
    let server = FakeServer::new();
    server.deliver("INBOX", 200, true);
    server.with(|s| s.faults.cut_after_fetches = Some(2));
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    let (offline, _) = h.status(|s| s.state == AccountState::Offline).await;
    let error = offline.error.expect("error");
    assert_eq!(error.code, ErrorCode::ConnectionLost);
    assert!(offline.retry_at.is_some());
    h.idle().await;
    let inbox = h.mailbox(id, "INBOX").await;
    assert_eq!(h.count(inbox.id).await, 200);
    // Two windows were stored before the cut (10 + 50); nothing was fetched twice.
    assert_eq!(server.read(|s| s.fetched), 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_password_is_never_stored_and_stops_sync_until_the_user_acts() {
    let server = FakeServer::new();
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let err = h
        .core
        .add_account(settings("alice@fake.test"), Secret::new("wrong".into()))
        .await
        .expect_err("wrong password");
    assert_eq!(err.code(), ErrorCode::AuthFailed);
    assert!(h.keychain.is_empty());
    assert!(h.core.list_accounts().await.expect("accounts").is_empty());

    // The password changes on the server while the account runs.
    let id = h.add().await;
    h.idle().await;
    server.with(|s| s.password = "new-pw".into());
    h.core.resume();
    let (status, _) = h.status(|s| s.state == AccountState::Error).await;
    assert_eq!(status.error.map(|e| e.code), Some(ErrorCode::AuthFailed));
    // Neither time nor a network change retries a rejected login.
    let logins = server.read(|s| s.access.len());
    h.core.network_changed();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.read(|s| s.access.len()), logins);

    let err = h
        .core
        .update_password(id, Secret::new("still-wrong".into()))
        .await
        .expect_err("checked first");
    assert_eq!(err.code(), ErrorCode::AuthFailed);
    h.core
        .update_password(id, Secret::new("new-pw".into()))
        .await
        .expect("new password");
    let key = CredentialRef::for_account(id);
    let stored = Credentials::new(Arc::clone(&h.keychain))
        .get(&key)
        .await
        .expect("read");
    assert_eq!(stored.map(Secret::into_exposed).as_deref(), Some("new-pw"));
    h.idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panic_in_the_adapter_is_a_protocol_error_with_backoff() {
    let server = FakeServer::new();
    server.deliver("INBOX", 3, true);
    server.with(|s| s.faults.panic_on_fetch = true);
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    let (status, _) = h.status(|s| s.state == AccountState::Offline).await;
    assert_eq!(status.error.map(|e| e.code), Some(ErrorCode::Protocol));
    assert!(status.retry_at.is_some());
    // It keeps retrying with backoff; the app keeps working.
    h.status(|s| s.state == AccountState::Offline && s.retry_at.is_some())
        .await;
    assert!(h.core.list_accounts().await.is_ok());
    server.with(|s| s.faults.panic_on_fetch = false);
    h.idle().await;
    assert_eq!(h.mailbox(id, "INBOX").await.total, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transient_connect_errors_back_off_and_retry_cuts_it_short() {
    let server = FakeServer::new();
    let mut config = sync_config();
    config.backoff = BackoffPolicy {
        base: Duration::from_secs(30),
        max: Duration::from_secs(60),
        jitter: 0.0,
    };
    let mut h = Harness::new(Arc::clone(&server), config).await;
    let id = h.add().await;
    h.idle().await;
    server.with(|s| s.faults.connect = Some(BackendError::ConnectionRefused));
    h.core.network_changed();
    let (status, _) = h.status(|s| s.state == AccountState::Offline).await;
    assert_eq!(
        status.error.map(|e| e.code),
        Some(ErrorCode::ConnectionRefused)
    );
    let retry_at = status.retry_at.expect("retry time");
    assert!(retry_at - SystemClock.now_ms() > 20_000, "30 s backoff");
    // The server is back; the user does not want to wait.
    server.with(|s| s.faults.connect = None);
    h.core.retry_account(id).await.expect("retry");
    h.idle().await;
    let err = h
        .core
        .retry_account(AccountId::from_uuid(uuid::Uuid::new_v4()))
        .await
        .expect_err("unknown");
    assert_eq!(err.code(), ErrorCode::NotFound);
}

/// Stage 18 (task D): "Reload folders from server" lists the folders again at once: a folder
/// another program created shows without waiting for the next poll; an unknown account is
/// `not_found`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reloading_the_folders_shows_one_another_program_created() {
    let server = FakeServer::new();
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.idle().await;
    let listed = async |h: &Harness| {
        h.core
            .list_mailboxes(id)
            .await
            .expect("mailboxes")
            .iter()
            .any(|m| m.path == "Elsewhere")
    };
    server.add_mailbox("Elsewhere");
    h.settle().await;
    assert!(
        !listed(&h).await,
        "not before a full pass (the poll is 10 min)"
    );
    h.core.reload_folders(id).await.expect("reload");
    h.event(|e| matches!(e, CoreEvent::MailboxesChanged { .. }))
        .await;
    assert!(listed(&h).await, "listed after the reload");
    let err = h
        .core
        .reload_folders(AccountId::from_uuid(uuid::Uuid::new_v4()))
        .await
        .expect_err("unknown");
    assert_eq!(err.code(), ErrorCode::NotFound);
    h.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_an_account_mid_sync_stops_it_and_deletes_everything() {
    let server = FakeServer::new();
    server.deliver("INBOX", 400, false);
    server.with(|s| s.faults.fetch_delay = Duration::from_millis(30));
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.status(|s| s.progress.is_some()).await;
    let inbox = h.mailbox(id, "INBOX").await;
    assert!(server.open_connections() >= 1);

    let started = Instant::now();
    h.core.remove_account(id).await.expect("remove");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    h.event(|e| *e == CoreEvent::AccountsChanged).await;
    // Tasks ended, connections closed, keychain entry and local data gone.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(server.open_connections(), 0);
    assert!(h.keychain.is_empty());
    assert!(h.core.list_accounts().await.expect("accounts").is_empty());
    assert_eq!(
        h.core.list_mailboxes(id).await.expect_err("gone").code(),
        ErrorCode::NotFound
    );
    assert_eq!(
        h.core
            .list_messages(inbox.id, None, 10)
            .await
            .expect_err("gone")
            .code(),
        ErrorCode::NotFound
    );
    // No event of the removed account afterwards.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while let Ok(event) = h.events.try_recv() {
        assert!(
            !matches!(
                event,
                CoreEvent::AccountStatusChanged { .. } | CoreEvent::MessagesChanged { .. }
            ),
            "{event:?}"
        );
    }
    assert_eq!(
        h.core.remove_account(id).await.expect_err("twice").code(),
        ErrorCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keychain_failure_on_removal_keeps_the_account_running() {
    let server = FakeServer::new();
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.idle().await;
    h.keychain.set_failing(SecretOp::Delete, true);
    let err = h.core.remove_account(id).await.expect_err("keychain");
    assert_eq!(err.code(), ErrorCode::Keychain);
    assert_eq!(h.core.list_accounts().await.expect("accounts").len(), 1);
    h.idle().await;
    h.keychain.set_failing(SecretOp::Delete, false);
    h.core.remove_account(id).await.expect("remove");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_account_is_refused_before_connecting() {
    let server = FakeServer::new();
    let h = Harness::new(Arc::clone(&server), sync_config()).await;
    h.add().await;
    // Had the second setup connected, it would fail with this instead.
    server.with(|s| s.faults.connect = Some(BackendError::ConnectionRefused));
    let err = h
        .core
        .add_account(settings("ALICE@fake.test"), Secret::new(PASSWORD.into()))
        .await
        .expect_err("exists");
    assert_eq!(err.code(), ErrorCode::AccountExists);
    let err = h
        .core
        .add_account(settings("not-an-address"), Secret::new(PASSWORD.into()))
        .await
        .expect_err("invalid");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_jump_of_the_wall_clock_reconnects() {
    let server = FakeServer::new();
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    h.add().await;
    h.idle().await;
    h.settle().await;
    let logins = server.read(|s| s.logins);
    // The computer slept for an hour: the wall clock moved, the monotonic clock did not.
    h.clock.0.fetch_add(3_600_000, Ordering::SeqCst);
    h.status(|s| s.state == AccountState::Connecting || s.state == AccountState::Syncing)
        .await;
    h.idle().await;
    assert!(server.read(|s| s.logins) > logins);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accounts_resume_after_a_restart_without_fetching_again() {
    let server = FakeServer::new();
    server.deliver("INBOX", 60, true);
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.idle().await;
    h.core.shutdown().await;
    assert_eq!(server.open_connections(), 0, "shutdown logs out and closes");
    let fetched = server.read(|s| s.fetched);
    let (dir, keychain) = (h.dir, h.keychain);
    drop(h.core);

    server.deliver("INBOX", 1, false);
    let mut h = Harness::reopen(dir, Arc::clone(&server), keychain, sync_config()).await;
    h.idle().await;
    let inbox = h.mailbox(id, "INBOX").await;
    assert_eq!(h.count(inbox.id).await, 61);
    assert_eq!(
        server.read(|s| s.fetched),
        fetched + 1,
        "only the new message"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_keychain_entry_is_an_error_state() {
    let server = FakeServer::new();
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let id = h.add().await;
    h.idle().await;
    h.core.shutdown().await;
    let (dir, keychain) = (h.dir, h.keychain);
    drop(h.core);
    Credentials::new(Arc::clone(&keychain))
        .delete(&CredentialRef::for_account(id))
        .await
        .expect("delete");
    let mut h = Harness::reopen(dir, server, keychain, sync_config()).await;
    let status = h.state(AccountState::Error).await;
    assert_eq!(status.error.map(|e| e.code), Some(ErrorCode::Keychain));
}

/// Stage 14: a JMAP account with an API token. The token is checked before it is stored, kept
/// in the keychain under a `token:` reference, and every connection (setup, sync, after a token
/// change and after a restart) sends it as a bearer token, never as a password. IMAP accounts
/// cannot take a token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_api_token_is_stored_like_a_password_and_always_sent_as_a_bearer_token() {
    const TOKEN: &str = "fmu1-0123-api-token";
    let server = FakeServer::new();
    server.deliver("INBOX", 3, false);
    server.with(|s| s.password = TOKEN.into());
    let mut h = Harness::new(Arc::clone(&server), sync_config()).await;
    let mut jmap = settings("alice@fake.test");
    jmap.protocol = mail_model::Protocol::Jmap;
    jmap.credential = mail_model::CredentialKind::ApiToken;

    let mut imap = settings("alice@fake.test");
    imap.credential = mail_model::CredentialKind::ApiToken;
    let err = h
        .core
        .add_account(imap, Secret::new(TOKEN.into()))
        .await
        .expect_err("IMAP takes no token");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let err = h
        .core
        .add_account(jmap.clone(), Secret::new("fmu1-wrong".into()))
        .await
        .expect_err("wrong token");
    assert_eq!(err.code(), ErrorCode::AuthFailed);
    assert!(h.keychain.is_empty(), "nothing stored for a refused token");

    let account = h
        .core
        .add_account(jmap, Secret::new(TOKEN.into()))
        .await
        .expect("add with the token");
    assert_eq!(account.credential, mail_model::CredentialKind::ApiToken);
    assert_eq!(account.oauth, None);
    let id = account.id;
    let key = CredentialRef::parse(&format!("token:{id}")).expect("reference");
    let stored = Credentials::new(Arc::clone(&h.keychain))
        .get(&key)
        .await
        .expect("read");
    assert_eq!(stored.map(Secret::into_exposed).as_deref(), Some(TOKEN));
    h.idle().await;
    let listed = h.core.list_accounts().await.expect("accounts");
    assert_eq!(listed[0].credential, mail_model::CredentialKind::ApiToken);

    // A new token: checked as a bearer token, stored under the same reference.
    server.with(|s| s.password = "fmu1-new-token".into());
    let err = h
        .core
        .update_password(id, Secret::new(TOKEN.into()))
        .await
        .expect_err("the old token is refused now");
    assert_eq!(err.code(), ErrorCode::AuthFailed);
    h.core
        .update_password(id, Secret::new("fmu1-new-token".into()))
        .await
        .expect("new token");
    let stored = Credentials::new(Arc::clone(&h.keychain))
        .get(&key)
        .await
        .expect("read");
    assert_eq!(
        stored.map(Secret::into_exposed).as_deref(),
        Some("fmu1-new-token")
    );
    h.core.retry_account(id).await.expect("reconnect");
    h.idle().await;

    // After a restart the token comes from the keychain, still as a bearer token.
    h.core.shutdown().await;
    let (dir, keychain) = (h.dir, h.keychain);
    drop(h.core);
    let before = server.read(|s| s.auths.len());
    let mut h = Harness::reopen(dir, Arc::clone(&server), keychain, sync_config()).await;
    h.idle().await;
    let auths = server.read(|s| s.auths.clone());
    assert!(auths.len() > before, "logged in again after the restart");
    assert!(
        auths.iter().all(|a| *a == mail_model::LoginAuth::Bearer),
        "{auths:?}"
    );
}
