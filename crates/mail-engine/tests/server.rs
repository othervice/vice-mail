//! `AppCore` against the local test server (Stalwart), without a WebView: the acceptance tests
//! of stage 2 for the engine. Ignored by default; they need the server started and seeded:
//!
//! ```text
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000
//! cargo test -p mail-engine -- --include-ignored
//! scripts/testmail.sh stop
//! ```
//!
//! The tests run one at a time (they share alice's mailbox). Tests that change data use their
//! own folder (`VmTest-...`) or mail they delivered themselves, and clean up. The restart test
//! stops and starts the server and only runs with `VICEMAIL_RESTART_TEST=1`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, SyncConfig};
use mail_imap::test_support::{Fault, Proxy, TestEnv, send_mail};
use mail_imap::{ImapBackend, ImapConnector, WireTap};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, AccountStatus, CoreEvent, ErrorCode,
    MailBackend, MailConnector, Mailbox, MailboxId, MailboxRole, Secret, Security, ServerSettings,
    Uid, UidSet,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(60);

/// The tests share alice's account: one at a time.
static SERVER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        poll_interval: Duration::from_secs(1),
        extra_root_ca: Some(env.extra_root()),
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

struct Engine {
    dir: TempDir,
    keychain: Arc<MemorySecretStore>,
    core: AppCore,
    events: broadcast::Receiver<CoreEvent>,
}

impl Engine {
    async fn start(connector: ImapConnector, config: SyncConfig) -> Self {
        Self::reopen(
            TempDir::new().expect("temp dir"),
            Arc::default(),
            connector,
            config,
        )
        .await
    }

    async fn reopen(
        dir: TempDir,
        keychain: Arc<MemorySecretStore>,
        connector: ImapConnector,
        config: SyncConfig,
    ) -> Self {
        let mut services =
            CoreServices::with_connector(Credentials::new(Arc::clone(&keychain)), connector);
        services.sync = config;
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
            keychain,
            core,
            events,
        }
    }

    /// Stops the sync and hands back what a restart needs.
    async fn stop(self) -> (TempDir, Arc<MemorySecretStore>) {
        self.core.shutdown().await;
        (self.dir, self.keychain)
    }

    async fn event(&mut self, pred: impl Fn(&CoreEvent) -> bool) -> (CoreEvent, Vec<CoreEvent>) {
        let mut before = Vec::new();
        let deadline = Instant::now() + WAIT;
        loop {
            let event = match tokio::time::timeout_at(deadline, self.events.recv()).await {
                Ok(Ok(event)) => event,
                Ok(Err(err)) => panic!("event stream: {err}"),
                Err(_) => panic!(
                    "no matching event within {WAIT:?}; last events: {:#?}",
                    &before[before.len().saturating_sub(10)..]
                ),
            };
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
        let CoreEvent::AccountStatusChanged { status, .. } = event else {
            unreachable!()
        };
        (status, before)
    }

    /// Waits until `done` holds, checking again after every event. For changes from outside,
    /// wait for their effect, not for "the next `messagesChanged`": since stage 3 the prefetch
    /// announces the preview of a body it stored with the same event, so the next event for
    /// the inbox may belong to the prefetch and the change may not be stored yet.
    async fn until(&mut self, what: &str, done: impl AsyncFn(&AppCore) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(&self.core).await {
            match tokio::time::timeout_at(deadline, self.events.recv()).await {
                // A lagging receiver lost events, not state: look again.
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(err)) => panic!("event stream: {err}"),
                Err(_) => panic!("{what}: not stored within {WAIT:?}"),
            }
        }
    }

    async fn current(&self) -> AccountStatus {
        let accounts = self.core.list_accounts().await.expect("accounts");
        accounts.first().expect("an account").status.clone()
    }

    /// Waits until the account is idle (at once if it is); returns the events on the way.
    async fn idle(&mut self) -> Vec<CoreEvent> {
        if self.current().await.state == AccountState::Idle {
            return Vec::new();
        }
        self.status(|s| s.state == AccountState::Idle).await.1
    }

    async fn mailbox(&self, account: AccountId, path: &str) -> Mailbox {
        self.core
            .list_mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == path)
            .unwrap_or_else(|| panic!("no mailbox {path}"))
    }

    async fn subjects(&self, mailbox: MailboxId) -> Vec<String> {
        let mut cursor = None;
        let mut subjects = Vec::new();
        loop {
            let page = self
                .core
                .list_messages(mailbox, cursor, 500)
                .await
                .expect("page");
            subjects.extend(page.items.into_iter().map(|m| m.subject));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return subjects,
            }
        }
    }
}

fn settings(env: &TestEnv, port: u16) -> AccountSettings {
    let server = |port, security| ServerSettings {
        host: env.host.clone(),
        port,
        security,
        username: env.alice.address.clone(),
    };
    AccountSettings {
        email: env.alice.address.clone(),
        display_name: "Alice".into(),
        imap: server(port, Security::Tls),
        smtp: server(env.submission_port, Security::Starttls),
    }
}

async fn add_alice(engine: &Engine, env: &TestEnv, port: u16) -> AccountId {
    engine
        .core
        .add_account(settings(env, port), env.alice.password.clone())
        .await
        .expect("add alice")
        .id
}

async fn proxy(env: &TestEnv) -> Arc<Proxy> {
    let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
        .parse()
        .expect("addr");
    Proxy::start(upstream).await.expect("proxy")
}

// ---------------------------------------------------------------------------------------------
// Changes "from outside": a read-write connection of the test, never the engine's.

async fn writer(env: &TestEnv) -> ImapBackend {
    ImapConnector::new()
        .connect(&env.config(&env.alice, Security::Tls, Access::ReadWrite))
        .await
        .expect("read-write connection for test setup")
}

/// A folder of one test run. The name sorts right after the inbox (`A-` before `Archive`), so
/// its sync starts after a small, predictable amount of traffic whatever else is in the account.
fn scratch(test: &str) -> String {
    format!("A-VmTest-engine-{test}-{}", std::process::id())
}

/// Messages in the scratch folders of the cut and stall tests, and where the fault hits: about
/// a third into the folder's first sync with `seed 1000`, a quarter with `seed 10000`.
const FOLDER_SIZE: usize = 400;
const FAULT_AFTER: u64 = 100_000;

fn message(folder: &str, n: usize) -> String {
    format!(
        "From: Test Writer <writer@vicemail.test>\r\nTo: alice@vicemail.test\r\n\
         Subject: {folder} message {n}\r\nDate: Mon, 02 Nov 2026 10:00:{:02} +0000\r\n\
         Message-ID: <{folder}-{n}@vicemail.test>\r\n\r\nBody {n}.\r\n",
        n % 60
    )
}

async fn raw(writer: &mut ImapBackend, line: &str) {
    let reply = writer
        .raw_command(line, |_| false)
        .await
        .unwrap_or_else(|err| panic!("{line}: {err}"));
    assert!(reply.ok, "{line}: {}", reply.text);
}

async fn create_folder(writer: &mut ImapBackend, name: &str, count: usize) {
    let _ = writer
        .raw_command(&format!("DELETE \"{name}\""), |_| false)
        .await;
    raw(writer, &format!("CREATE \"{name}\"")).await;
    for n in 1..=count {
        let body = message(name, n);
        // LITERAL+ (the test server has it): the literal goes out with the command.
        raw(
            writer,
            &format!("APPEND \"{name}\" (\\Seen) {{{}+}}\r\n{body}", body.len()),
        )
        .await;
    }
}

async fn delete_folder(writer: &mut ImapBackend, name: &str) {
    writer.raw_command("UNSELECT", |_| false).await.ok();
    raw(writer, &format!("DELETE \"{name}\"")).await;
}

/// UIDs of the mailbox on the server.
async fn server_uids(writer: &mut ImapBackend, folder: &str) -> UidSet {
    let open = writer.open_mailbox(folder).await.expect("select");
    writer.search_uids(&open).await.expect("search")
}

async fn expunge(writer: &mut ImapBackend, folder: &str, uids: &UidSet) {
    writer.open_mailbox(folder).await.expect("select");
    raw(
        writer,
        &format!("UID STORE {uids} +FLAGS.SILENT (\\Deleted)"),
    )
    .await;
    raw(writer, &format!("UID EXPUNGE {uids}")).await;
}

/// Server message counts by path, for every selectable mailbox.
async fn server_counts(env: &TestEnv) -> Vec<(String, u32, Option<u32>)> {
    let mut reader = ImapConnector::new()
        .connect(&env.config(&env.alice, Security::Tls, Access::ReadOnly))
        .await
        .expect("reader");
    let mut counts: Vec<_> = reader
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        // Stage 11: the settings sync folder (left by its tests) is never shown as mail.
        .filter(|m| !mail_model::is_settings_sync_folder(&m.name, m.delimiter))
        .filter_map(|m| m.status.map(|s| (m.path, s.messages, s.unseen)))
        .collect();
    counts.sort();
    reader.logout().await.ok();
    counts
}

/// The UIDs a `UID FETCH <set> (... ENVELOPE ...)` line asks for; 0 for other lines.
fn header_fetch_size(line: &str) -> u64 {
    if !line.contains("ENVELOPE") {
        return 0;
    }
    let Some(set) = line
        .split("UID FETCH ")
        .nth(1)
        .and_then(|r| r.split(' ').next())
    else {
        return 0;
    };
    let ranges = set.split(',').filter_map(|part| {
        let (a, b) = part.split_once(':').unwrap_or((part, part));
        Some((a.parse::<Uid>().ok()?, b.parse::<Uid>().ok()?))
    });
    UidSet::from_ranges(ranges).len()
}

fn progress(events: &[CoreEvent], mailbox: MailboxId) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::AccountStatusChanged { status, .. } => status
                .progress
                .filter(|p| p.mailbox_id == mailbox)
                .map(|p| p.done),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------

/// First sync of the whole seeded account: folders with roles, every message, counters equal
/// to the server's, the inbox first.
#[tokio::test]
#[ignore = "needs the test server"]
async fn first_sync_matches_the_server() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let mut engine = Engine::start(ImapConnector::new(), sync_config(&env)).await;
    let started = Instant::now();
    let id = add_alice(&engine, &env, env.imaps_port).await;
    let (_, events) = engine.status(|s| s.state == AccountState::Idle).await;
    let took = started.elapsed();

    let mailboxes = engine.core.list_mailboxes(id).await.expect("mailboxes");
    let role = |path: &str| {
        mailboxes
            .iter()
            .find(|m| m.path == path)
            .map(|m| m.role)
            .unwrap_or_else(|| panic!("no {path}"))
    };
    assert_eq!(role("INBOX"), MailboxRole::Inbox);
    assert_eq!(role("Sent"), MailboxRole::Sent);
    assert_eq!(role("Drafts"), MailboxRole::Drafts);
    assert_eq!(role("Trash"), MailboxRole::Trash);
    assert_eq!(role("Junk"), MailboxRole::Junk);
    assert_eq!(role("Archive"), MailboxRole::Archive);

    let mut local: Vec<_> = mailboxes
        .iter()
        .filter(|m| m.selectable)
        .map(|m| (m.path.clone(), m.total, Some(m.unread)))
        .collect();
    local.sort();
    assert_eq!(local, server_counts(&env).await);
    let total: u32 = mailboxes.iter().map(|m| m.total).sum();

    // The inbox is synced before any other folder.
    let inbox = engine.mailbox(id, "INBOX").await;
    let first = events
        .iter()
        .find_map(|e| match e {
            CoreEvent::AccountStatusChanged { status, .. } => status.progress,
            _ => None,
        })
        .expect("progress");
    assert_eq!(first.mailbox_id, inbox.id);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, CoreEvent::NewMail { .. }))
    );
    let windows = events
        .iter()
        .filter(|e| matches!(e, CoreEvent::MessagesChanged { .. }))
        .count();
    println!(
        "first sync: {total} messages in {} mailboxes, {windows} stored windows, {took:?} from add_account to idle",
        mailboxes.len()
    );
    engine.core.shutdown().await;
}

/// New mail, a flag change and a deletion from outside reach the engine through IDLE, without
/// polling (the poll interval is ten minutes here).
#[tokio::test]
#[ignore = "needs the test server"]
async fn new_mail_flag_change_and_deletion_from_outside_arrive_by_idle() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let mut config = sync_config(&env);
    config.poll_interval = Duration::from_secs(600);
    let mut engine = Engine::start(ImapConnector::new(), config).await;
    let id = add_alice(&engine, &env, env.imaps_port).await;
    engine.idle().await;
    let inbox = engine.mailbox(id, "INBOX").await;
    let mut writer = writer(&env).await;
    let before = server_uids(&mut writer, "INBOX").await;
    // Let the IDLE watcher settle (it looks at the inbox once after connecting).
    tokio::time::sleep(Duration::from_millis(500)).await;
    engine.idle().await;

    let subject = format!("engine new mail {}", std::process::id());
    let sent = Instant::now();
    send_mail(&env, &env.bob, &env.alice.address, &subject, "Hello.")
        .await
        .expect("send");
    let accepted = sent.elapsed();
    let (event, _) = engine
        .event(|e| matches!(e, CoreEvent::NewMail { .. }))
        .await;
    let arrived = sent.elapsed();
    assert_eq!(
        event,
        CoreEvent::NewMail {
            account_id: id,
            mailbox_id: inbox.id,
            count: 1
        }
    );
    println!("SMTP accepted after {accepted:?}, newMail event after {arrived:?}");
    assert!(arrived < Duration::from_secs(5), "{arrived:?}");
    engine.idle().await;
    let page = engine
        .core
        .list_messages(inbox.id, None, 5)
        .await
        .expect("page");
    let item = page
        .items
        .iter()
        .find(|m| m.subject == subject)
        .expect("listed");
    assert!(item.unread && !item.flagged);

    let new = server_uids(&mut writer, "INBOX").await.difference(&before);
    assert_eq!(new.len(), 1);
    raw(
        &mut writer,
        &format!("UID STORE {new} +FLAGS (\\Flagged \\Seen)"),
    )
    .await;
    let changed = Instant::now();
    // The prefetch stores the new mail's body meanwhile (its own `messagesChanged`).
    let listed = async |core: &AppCore| {
        core.list_messages(inbox.id, None, 5)
            .await
            .expect("page")
            .items
            .into_iter()
            .find(|m| m.subject == subject)
    };
    engine
        .until("flag change", async |core| {
            listed(core).await.is_some_and(|m| m.flagged)
        })
        .await;
    println!("flag change seen after {:?}", changed.elapsed());
    engine.idle().await;
    let item = listed(&engine.core).await.expect("listed");
    assert!(!item.unread && item.flagged);

    let total = engine.mailbox(id, "INBOX").await.total;
    expunge(&mut writer, "INBOX", &new).await;
    let deleted = Instant::now();
    engine
        .until("deletion", async |core| listed(core).await.is_none())
        .await;
    println!("deletion seen after {:?}", deleted.elapsed());
    engine.idle().await;
    assert_eq!(engine.mailbox(id, "INBOX").await.total, total - 1);
    assert!(!engine.subjects(inbox.id).await.contains(&subject));
    writer.logout().await.ok();
    engine.core.shutdown().await;
}

/// The connection is cut in the middle of a first sync: the account goes offline, reconnects
/// by itself and continues below the lowest stored UID, without fetching stored windows again.
#[tokio::test]
#[ignore = "needs the test server"]
async fn cut_connection_mid_first_sync_resumes_where_it_stopped() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let proxy = proxy(&env).await;
    let folder = scratch("cut");
    let mut config = sync_config(&env);
    config.window = 25;
    config.first_window = 10;

    // Everything else is synced first, so the restart below only has this folder to sync.
    let engine = Engine::start(ImapConnector::new(), config.clone()).await;
    let id = add_alice(&engine, &env, proxy.port()).await;
    let mut engine = engine;
    engine.idle().await;
    let (dir, keychain) = engine.stop().await;
    let mut writer = writer(&env).await;
    create_folder(&mut writer, &folder, FOLDER_SIZE).await;

    // After a restart the worker's connection is the first one; cut it in the folder's sync.
    proxy.arm(Fault::CloseAfter(FAULT_AFTER)).await;
    let tap = WireTap::new();
    let mut engine = Engine::reopen(
        dir,
        keychain,
        ImapConnector::new().with_wire_tap(tap.clone()),
        config,
    )
    .await;
    let (offline, before_cut) = engine.status(|s| s.state == AccountState::Offline).await;
    assert_eq!(
        offline.error.map(|e| e.code),
        Some(ErrorCode::ConnectionLost)
    );
    assert!(proxy.fired_at().await.is_some());
    let after_cut = engine.idle().await;

    let mailbox = engine.mailbox(id, &folder).await;
    let (before, after) = (
        progress(&before_cut, mailbox.id),
        progress(&after_cut, mailbox.id),
    );
    println!("progress before the cut {before:?}, after {after:?}");
    let last = *before
        .last()
        .expect("the cut came during this folder's first sync");
    assert!(last < 400, "cut before the end");
    assert!(
        after.first().is_some_and(|first| *first > last),
        "continued, not restarted"
    );
    assert_eq!(mailbox.total, 400);
    assert_eq!(engine.subjects(mailbox.id).await.len(), 400);
    // At most the window in flight at the cut was fetched twice.
    let fetched: u64 = tap.lines().iter().map(|l| header_fetch_size(l)).sum();
    println!("header UIDs requested after the restart: {fetched}");
    assert!((400..=425).contains(&fetched), "{fetched}");

    delete_folder(&mut writer, &folder).await;
    writer.logout().await.ok();
    engine.core.shutdown().await;
}

/// Messages are deleted on the server while the first sync of their folder is underway (the
/// sync stalls on the network meanwhile): stored ones and ones not fetched yet. Afterwards the
/// local folder equals the server's.
#[tokio::test]
#[ignore = "needs the test server"]
async fn deletion_during_the_first_sync() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let proxy = proxy(&env).await;
    let folder = scratch("delete-during");
    let mut config = sync_config(&env);
    config.window = 25;
    config.first_window = 10;

    let engine = Engine::start(ImapConnector::new(), config.clone()).await;
    let id = add_alice(&engine, &env, proxy.port()).await;
    let mut engine = engine;
    engine.idle().await;
    let (dir, keychain) = engine.stop().await;
    let mut writer = writer(&env).await;
    create_folder(&mut writer, &folder, FOLDER_SIZE).await;

    proxy.arm(Fault::StallAfter(FAULT_AFTER)).await;
    let mut engine = Engine::reopen(dir, keychain, ImapConnector::new(), config).await;
    let deadline = Instant::now() + WAIT;
    while proxy.fired_at().await.is_none() {
        assert!(Instant::now() < deadline, "the stall never fired");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The sync hangs in the middle of this folder's first sync.
    let mailbox = engine.mailbox(id, &folder).await;
    let status = engine.current().await;
    assert_eq!(status.state, AccountState::Syncing);
    let stalled_at = status.progress.expect("first sync in progress");
    assert_eq!(stalled_at.mailbox_id, mailbox.id);
    assert!(stalled_at.done < 400);

    let uids = server_uids(&mut writer, &folder).await;
    let lowest = UidSet::from_uids(uids.iter().take(5));
    let highest = UidSet::from_uids(uids.iter().rev().take(3));
    expunge(&mut writer, &folder, &lowest).await;
    expunge(&mut writer, &folder, &highest).await;
    println!(
        "deleted {lowest} (not fetched yet) and {highest} (stored) at progress {}",
        stalled_at.done
    );

    proxy.release_stalled();
    engine.idle().await;
    // The stored ones disappear in the pass after the reconnect at the latest.
    let deadline = Instant::now() + WAIT;
    loop {
        if engine.mailbox(id, &folder).await.total == 392 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "local folder does not match the server"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut local = engine.subjects(mailbox.id).await;
    local.sort();
    let mut expected: Vec<String> = (6..=397).map(|n| format!("{folder} message {n}")).collect();
    expected.sort();
    assert_eq!(local, expected);

    delete_folder(&mut writer, &folder).await;
    writer.logout().await.ok();
    engine.core.shutdown().await;
}

/// The folder is deleted and created again under the same name with the same messages: a new
/// UIDVALIDITY. The engine drops the old UIDs and rebuilds; local messages keep their ids
/// through the Message-ID.
#[tokio::test]
#[ignore = "needs the test server"]
async fn uidvalidity_change_rebuilds_the_mailbox() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let proxy = proxy(&env).await;
    let folder = scratch("uidvalidity");
    let mut writer = writer(&env).await;
    create_folder(&mut writer, &folder, 20).await;

    let mut engine = Engine::start(ImapConnector::new(), sync_config(&env)).await;
    let id = add_alice(&engine, &env, proxy.port()).await;
    engine.idle().await;
    let mailbox = engine.mailbox(id, &folder).await;
    assert_eq!(mailbox.total, 20);
    let ids =
        |page: mail_model::MessagePage| page.items.into_iter().map(|m| m.id).collect::<Vec<_>>();
    let before = ids(engine
        .core
        .list_messages(mailbox.id, None, 50)
        .await
        .expect("page"));
    let old_validity = writer
        .open_mailbox(&folder)
        .await
        .expect("select")
        .uid_validity;

    // Offline while the folder is recreated, so no pass sees it missing in between.
    proxy.refuse_next(usize::MAX);
    engine.core.network_changed();
    engine
        .status(|s| {
            matches!(s.state, AccountState::Offline | AccountState::Error) && s.error.is_some()
        })
        .await;
    delete_folder(&mut writer, &folder).await;
    create_folder(&mut writer, &folder, 20).await;
    let new_validity = writer
        .open_mailbox(&folder)
        .await
        .expect("select")
        .uid_validity;
    assert_ne!(old_validity, new_validity);

    proxy.refuse_next(0);
    engine.core.retry_account(id).await.expect("retry");
    engine.idle().await;
    let rebuilt = engine.mailbox(id, &folder).await;
    assert_eq!(rebuilt.id, mailbox.id, "same mailbox row");
    assert_eq!(rebuilt.total, 20);
    let after = ids(engine
        .core
        .list_messages(mailbox.id, None, 50)
        .await
        .expect("page"));
    assert_eq!(before, after, "local messages merged back by Message-ID");

    delete_folder(&mut writer, &folder).await;
    writer.logout().await.ok();
    engine.core.shutdown().await;
}

/// A wrong password: setup and the connection test fail with `auth_failed`; nothing is stored.
#[tokio::test]
#[ignore = "needs the test server"]
async fn wrong_password_is_refused_and_never_stored() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let engine = Engine::start(ImapConnector::new(), sync_config(&env)).await;
    let wrong = Secret::new("definitely-wrong-pw".to_owned());
    for result in [
        engine
            .core
            .test_connection(settings(&env, env.imaps_port), wrong.clone())
            .await
            .map(|_| ()),
        engine
            .core
            .add_account(settings(&env, env.imaps_port), wrong.clone())
            .await
            .map(|_| ()),
    ] {
        let err = result.expect_err("wrong password");
        assert_eq!(err.code(), ErrorCode::AuthFailed);
        assert!(!format!("{err} {err:?}").contains("definitely-wrong-pw"));
    }
    assert!(engine.keychain.is_empty());
    assert!(
        engine
            .core
            .list_accounts()
            .await
            .expect("accounts")
            .is_empty()
    );

    // STARTTLS works the same way, and the right password passes.
    let mut starttls = settings(&env, env.imap_port);
    starttls.imap.security = Security::Starttls;
    let result = engine
        .core
        .test_connection(starttls, env.alice.password.clone())
        .await
        .expect("connection test");
    assert!(result.capabilities.idle && result.capabilities.condstore);
}

/// In read-only mode no writing command leaves the adapter during a complete sync with IDLE:
/// the wire tap records every line the engine's connections wrote, and every one is on the
/// allowlist of decision 0007, mailboxes are opened with EXAMINE only, and bodies only with PEEK.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_complete_sync_with_idle_sends_only_allowlisted_commands() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let tap = WireTap::new();
    let mut config = sync_config(&env);
    config.idle_section = Duration::from_secs(1);
    let mut engine = Engine::start(ImapConnector::new().with_wire_tap(tap.clone()), config).await;
    add_alice(&engine, &env, env.imaps_port).await;
    engine.idle().await;
    // A new message wakes IDLE (DONE, a pass), sections end and are renewed, a poll runs.
    let subject = format!("engine read-only {}", std::process::id());
    send_mail(&env, &env.bob, &env.alice.address, &subject, "x")
        .await
        .expect("send");
    engine
        .event(|e| matches!(e, CoreEvent::NewMail { .. }))
        .await;
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    engine.core.shutdown().await;

    let allowed = [
        "CAPABILITY",
        "NOOP",
        "LOGOUT",
        "LOGIN",
        "AUTHENTICATE",
        "ID",
        "ENABLE",
        "LIST",
        "STATUS",
        "EXAMINE",
        "FETCH",
        "UID FETCH",
        "SEARCH",
        "UID SEARCH",
        "IDLE",
        "DONE",
    ];
    let verbs = tap.verbs();
    for verb in &verbs {
        assert!(
            allowed.contains(&verb.as_str()),
            "{verb} on the wire: {:#?}",
            tap.lines()
        );
    }
    for needed in [
        "LOGIN",
        "ENABLE",
        "LIST",
        "EXAMINE",
        "UID SEARCH",
        "UID FETCH",
        "IDLE",
        "DONE",
        "LOGOUT",
    ] {
        assert!(
            verbs.iter().any(|v| v == needed),
            "positive control: {needed} was recorded"
        );
    }
    let lines = tap.lines();
    assert!(
        lines
            .iter()
            .all(|l| !l.to_ascii_uppercase().contains("BODY[")),
        "only BODY.PEEK"
    );
    assert!(lines.iter().all(|l| !l.contains("SELECT ")));
    let idles = verbs.iter().filter(|v| *v == "IDLE").count();
    println!("{} lines on the wire, {idles} IDLE sections", lines.len());

    // Clean up the delivered message (test connection, read-write).
    let mut writer = writer(&env).await;
    let account_inbox = writer.open_mailbox("INBOX").await.expect("select");
    let mut ours = Vec::new();
    for header in writer
        .fetch_headers(
            &account_inbox,
            &UidSet::range(
                account_inbox.uid_next.saturating_sub(5),
                account_inbox.uid_next,
            ),
        )
        .await
        .expect("fetch")
    {
        if header.subject == subject {
            ours.push(header.uid);
        }
    }
    expunge(&mut writer, "INBOX", &UidSet::from_uids(ours)).await;
    writer.logout().await.ok();
}

/// The server stops: the account goes offline and the list stays usable; after the start it
/// recovers by itself (product backoff). Stops and starts the shared test server, so it only
/// runs with `VICEMAIL_RESTART_TEST=1`.
#[tokio::test]
#[ignore = "needs the test server and restarts it"]
async fn recovers_after_a_server_restart() {
    if std::env::var("VICEMAIL_RESTART_TEST").as_deref() != Ok("1") {
        println!("skipped: set VICEMAIL_RESTART_TEST=1 to stop and start the test server");
        return;
    }
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let mut config = sync_config(&env);
    config.backoff = BackoffPolicy::default();
    let mut engine = Engine::start(ImapConnector::new(), config).await;
    let id = add_alice(&engine, &env, env.imaps_port).await;
    engine.idle().await;
    let inbox = engine.mailbox(id, "INBOX").await;

    let script = env.repo.join("scripts/testmail.sh");
    let run = |arg: &str| {
        let status = std::process::Command::new(&script)
            .arg(arg)
            .status()
            .expect("run testmail.sh");
        assert!(status.success(), "testmail.sh {arg}");
    };
    let stopping = Instant::now();
    run("stop");
    let (offline, _) = engine.status(|s| s.state == AccountState::Offline).await;
    println!(
        "offline {:?} after the stop began ({:?})",
        stopping.elapsed(),
        offline.error.map(|e| e.code)
    );
    // The list stays usable while offline.
    assert!(
        !engine
            .core
            .list_messages(inbox.id, None, 50)
            .await
            .expect("page")
            .items
            .is_empty()
    );
    tokio::time::sleep(Duration::from_secs(3)).await;

    let starting = Instant::now();
    run("start");
    let started = starting.elapsed();
    engine.idle().await;
    println!(
        "server start took {started:?}; idle again {:?} after the start began",
        starting.elapsed()
    );
    engine.core.shutdown().await;
}
