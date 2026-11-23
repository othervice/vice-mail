//! Stage 4 through `AppCore` against the local test server (Stalwart), without a WebView: an
//! action shows in the list at once, its events arrive, the server catches up on the outbox's
//! own connection, and the sync confirms it right away (the outbox's hook); undo; a read-only
//! account refuses with `read_only`; with writes on, the sync still only sends EXAMINE.
//!
//! Ignored by default. Take the shared lock, start the server (any seed):
//!
//! ```text
//! scripts/testmail.sh lock actions && scripts/testmail.sh start
//! cargo test -p mail-engine --test actions -- --include-ignored --test-threads=1
//! scripts/testmail.sh stop && scripts/testmail.sh unlock actions
//! ```
//!
//! Works in bob's account (alice's seed stays untouched) with a folder `VmActions-<pid>` and
//! messages `<...@actions.test>` it appends itself; both are removed before and after. The
//! sync's connections and the outbox's go through separate wire taps, so the read side can be
//! checked on its own.

use std::collections::BTreeMap;
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, SyncConfig};
use mail_imap::test_support::TestEnv;
use mail_imap::{ImapBackend, ImapConnector, ImapWriter, TlsTransport, WireTap};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, BackendError, ConnectConfig, CoreEvent,
    ErrorCode, IpcError, MailAction, MailBackend, MailConnector, MailboxId, MessageListItem,
    MessageRef, OperationState, Security, ServerSettings, Uid, UidSet, WriteConnector,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(30);
const DOMAIN: &str = "actions.test";

/// The sync, IDLE and reading connect through `read`, the outbox through `write`: two taps.
struct Split {
    read: ImapConnector,
    write: ImapConnector,
}

impl MailConnector for Split {
    type Backend = ImapBackend<TlsTransport>;

    async fn connect(&self, config: &ConnectConfig) -> Result<Self::Backend, BackendError> {
        self.read.connect(config).await
    }
}

impl WriteConnector for Split {
    type Writer = ImapWriter<TlsTransport>;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<Self::Writer, BackendError> {
        self.write.connect_writer(config).await
    }
}

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        // Long: a pass over the scratch folder within the wait can only come from the hook
        // (IDLE watches the inbox).
        poll_interval: Duration::from_secs(600),
        extra_root_ca: Some(env.extra_root()),
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

async fn raw(admin: &mut ImapBackend, line: &str) {
    let reply = admin
        .raw_command(line, |_| false)
        .await
        .unwrap_or_else(|err| panic!("{line}: {err}"));
    assert!(reply.ok, "{line}: {}", reply.text);
}

async fn append(admin: &mut ImapBackend, folder: &str, run: &str, n: usize) {
    let body = format!(
        "From: Actions Test <writer@{DOMAIN}>\r\nTo: bob@vicemail.test\r\n\
         Subject: {run} message {n}\r\nDate: Mon, 02 Nov 2026 11:{:02}:00 +0000\r\n\
         Message-ID: <{run}-{n}@{DOMAIN}>\r\n\r\nBody {n}.\r\n",
        n % 60
    );
    raw(
        admin,
        &format!("APPEND \"{folder}\" () {{{}+}}\r\n{body}", body.len()),
    )
    .await;
}

/// Where the message `<run-n@actions.test>` lies on the server: (folder, UID, flagged).
async fn places(admin: &mut ImapBackend, id: &str) -> Vec<(String, Uid, bool)> {
    let mut found = Vec::new();
    for remote in admin.list_mailboxes().await.expect("list") {
        if !remote.is_selectable() {
            continue;
        }
        let opened = admin.open_mailbox(&remote.name).await.expect("select");
        let uids = admin.search_uids(&opened).await.expect("search");
        if uids.is_empty() {
            continue;
        }
        for header in admin.fetch_headers(&opened, &uids).await.expect("fetch") {
            if header.message_id.as_deref() == Some(id) {
                found.push((remote.name.clone(), header.uid, header.flags.flagged));
            }
        }
    }
    admin.raw_command("UNSELECT", |_| false).await.ok();
    found
}

/// Removes this test's messages and folders (also left over from an aborted run).
async fn clean_server(admin: &mut ImapBackend) {
    let mut by_folder: BTreeMap<String, Vec<Uid>> = BTreeMap::new();
    for remote in admin.list_mailboxes().await.expect("list") {
        if !remote.is_selectable() {
            continue;
        }
        let opened = admin.open_mailbox(&remote.name).await.expect("select");
        let uids = admin.search_uids(&opened).await.expect("search");
        if uids.is_empty() {
            continue;
        }
        for header in admin.fetch_headers(&opened, &uids).await.expect("fetch") {
            if header
                .message_id
                .as_deref()
                .is_some_and(|id| id.ends_with(&format!("@{DOMAIN}")))
            {
                by_folder
                    .entry(remote.name.clone())
                    .or_default()
                    .push(header.uid);
            }
        }
    }
    for (folder, uids) in by_folder {
        let set = UidSet::from_uids(uids);
        admin.open_mailbox(&folder).await.expect("select");
        raw(admin, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")).await;
        raw(admin, &format!("UID EXPUNGE {set}")).await;
    }
    admin.raw_command("UNSELECT", |_| false).await.ok();
    for remote in admin.list_mailboxes().await.expect("list") {
        if remote.name.starts_with("VmActions-") {
            raw(admin, &format!("DELETE \"{}\"", remote.name)).await;
        }
    }
}

struct Engine {
    _dir: TempDir,
    core: AppCore<Split>,
    events: broadcast::Receiver<CoreEvent>,
    account: AccountId,
}

impl Engine {
    async fn event(&mut self, what: &str, pred: impl Fn(&CoreEvent) -> bool) -> CoreEvent {
        let deadline = Instant::now() + WAIT;
        loop {
            match tokio::time::timeout_at(deadline, self.events.recv()).await {
                Ok(Ok(event)) if pred(&event) => return event,
                Ok(Ok(_)) => {}
                Ok(Err(err)) => panic!("event stream: {err}"),
                Err(_) => panic!("{what}: no matching event within {WAIT:?}"),
            }
        }
    }

    async fn confirmed(&mut self, batch: mail_model::BatchId) {
        self.event("confirmed", |e| match e {
            CoreEvent::OperationsChanged { operations, .. } => operations
                .iter()
                .any(|o| o.batch == batch && o.state == OperationState::Confirmed),
            _ => false,
        })
        .await;
    }

    async fn mailbox(&self, path: &str) -> MailboxId {
        self.core
            .list_mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == path)
            .unwrap_or_else(|| panic!("no mailbox {path}"))
            .id
    }

    async fn items(&self, mailbox: MailboxId) -> Vec<MessageListItem> {
        let mut cursor = None;
        let mut items = Vec::new();
        loop {
            let page = self
                .core
                .list_messages(mailbox, cursor, 500)
                .await
                .expect("page");
            items.extend(page.items);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return items,
            }
        }
    }

    async fn find(&self, mailbox: MailboxId, subject: &str) -> Option<MessageListItem> {
        self.items(mailbox)
            .await
            .into_iter()
            .find(|i| i.subject == subject)
    }

    /// Polls until the message is (or is not) in the folder's list.
    async fn until_listed(&self, path: &str, subject: &str, listed: bool) {
        let mailbox = self.mailbox(path).await;
        let deadline = Instant::now() + WAIT;
        while self.find(mailbox, subject).await.is_some() != listed {
            assert!(
                Instant::now() < deadline,
                "{subject} listed in {path}: expected {listed}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// `EXAMINE` lines of a folder on the tap (`A0007 EXAMINE "name" (CONDSTORE)`).
fn examines(tap: &WireTap, folder: &str) -> usize {
    tap.lines()
        .iter()
        .filter(|line| {
            let mut words = line.split(' ');
            words.any(|w| w == "EXAMINE")
                && words.next().map(|name| name.trim_matches('"')) == Some(folder)
        })
        .count()
}

/// The sync's flag fetches (`UID FETCH ... (CHANGEDSINCE m)`). Nothing else sends them: the
/// prefetch (which also opens folders with flagged mail) and IDLE do not.
fn flag_syncs(tap: &WireTap) -> usize {
    tap.lines()
        .iter()
        .filter(|line| line.contains("(CHANGEDSINCE "))
        .count()
}

async fn until_flags_synced(tap: &WireTap, more_than: usize) {
    let deadline = Instant::now() + WAIT;
    while flag_syncs(tap) <= more_than {
        assert!(
            Instant::now() < deadline,
            "the sync fetched the changed flags after the outbox stored them"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn until_examined(tap: &WireTap, folder: &str, more_than: usize) {
    let deadline = Instant::now() + WAIT;
    while examines(tap, folder) <= more_than {
        assert!(
            Instant::now() < deadline,
            "the sync examined {folder} after the outbox changed it"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn actions_through_the_core_reach_the_server_and_the_sync_confirms_them() {
    let env = TestEnv::require();
    let mut admin = ImapConnector::new()
        .connect(&env.config(&env.bob, Security::Tls, Access::ReadWrite))
        .await
        .expect("admin session");
    clean_server(&mut admin).await;
    let run = format!("act{}", std::process::id());
    let scratch = format!("VmActions-{run}");
    raw(&mut admin, &format!("CREATE \"{scratch}\"")).await;
    for n in 1..=3 {
        append(&mut admin, &scratch, &run, n).await;
    }
    let subject = |n: usize| format!("{run} message {n}");
    let message_id = |n: usize| format!("{run}-{n}@{DOMAIN}");

    let read_tap = WireTap::new();
    let write_tap = WireTap::new();
    let connector = Split {
        read: ImapConnector::new().with_wire_tap(read_tap.clone()),
        write: ImapConnector::new().with_wire_tap(write_tap.clone()),
    };
    let dir = TempDir::new().expect("temp dir");
    let mut services =
        CoreServices::with_connector(Credentials::new(MemorySecretStore::new()), connector);
    services.sync = sync_config(&env);
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
    let server = |port, security| ServerSettings {
        host: env.host.clone(),
        port,
        security,
        username: env.bob.address.clone(),
    };
    let account = core
        .add_account(
            AccountSettings {
                email: env.bob.address.clone(),
                display_name: "Bob".into(),
                imap: server(env.imaps_port, Security::Tls),
                smtp: server(env.submission_port, Security::Starttls),
            },
            env.bob.password.clone(),
        )
        .await
        .expect("add bob")
        .id;
    let mut engine = Engine {
        _dir: dir,
        core,
        events,
        account,
    };
    engine
        .event("idle", |e| {
            matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
        })
        .await;
    let folder = engine.mailbox(&scratch).await;
    let at = |item: &MessageListItem| MessageRef {
        mailbox_id: folder,
        message_id: item.id,
    };
    let first = engine.find(folder, &subject(1)).await.expect("message 1");
    let second = engine.find(folder, &subject(2)).await.expect("message 2");

    // 1. Read-only (every new account): refused through the core, with `read_only`.
    let err = engine
        .core
        .apply_action(MailAction::Flag, vec![at(&first)], "ro-1".into())
        .await
        .expect_err("read-only");
    assert_eq!(IpcError::from(&err).code, ErrorCode::ReadOnly, "{err}");
    assert!(
        !engine
            .find(folder, &subject(1))
            .await
            .expect("listed")
            .flagged
    );
    assert!(
        write_tap.is_empty(),
        "no writer connection for a read-only account"
    );

    // 2. Writes on.
    let enabled = engine
        .core
        .set_writes_enabled(account, true)
        .await
        .expect("writes on");
    assert!(enabled.writes_enabled);
    engine
        .event("accountsChanged", |e| {
            matches!(e, CoreEvent::AccountsChanged)
        })
        .await;

    // 3. Flag: local at once, then the server, then the sync's pass over the folder (only
    //    the folder changed, so its flag fetch is the only one).
    let passes = flag_syncs(&read_tap);
    let started = Instant::now();
    let flagged = engine
        .core
        .apply_action(MailAction::Flag, vec![at(&first)], "flag-1".into())
        .await
        .expect("flag");
    let batch = flagged.batch.expect("batch");
    assert!(
        engine
            .find(folder, &subject(1))
            .await
            .expect("listed")
            .flagged,
        "local at once"
    );
    engine.confirmed(batch).await;
    let confirmed_after = started.elapsed();
    let on_server = places(&mut admin, &message_id(1)).await;
    assert_eq!(on_server.len(), 1);
    assert!(on_server[0].2, "flagged on the server: {on_server:?}");
    until_flags_synced(&read_tap, passes).await;
    let synced_after = started.elapsed();
    let row = engine.find(folder, &subject(1)).await.expect("listed");
    assert!(row.flagged && row.activity.is_none(), "{row:?}");

    // 4. Archive: out of the folder at once, in the archive once the server moved it.
    let archive_passes = examines(&read_tap, "Archive");
    let archived = engine
        .core
        .apply_action(MailAction::Archive, vec![at(&second)], "archive-1".into())
        .await
        .expect("archive");
    let archive_batch = archived.batch.expect("batch");
    assert!(
        engine.find(folder, &subject(2)).await.is_none(),
        "gone from the folder at once"
    );
    engine
        .event(
            "messagesChanged of the folder",
            |e| matches!(e, CoreEvent::MessagesChanged { mailbox_id, .. } if *mailbox_id == folder),
        )
        .await;
    engine.confirmed(archive_batch).await;
    assert_eq!(
        places(&mut admin, &message_id(2))
            .await
            .iter()
            .map(|p| p.0.as_str())
            .collect::<Vec<_>>(),
        ["Archive"]
    );
    engine.until_listed("Archive", &subject(2), true).await;
    until_examined(&read_tap, "Archive", archive_passes).await;

    // 5. Undo: back where it was, on the server and in the lists.
    let undone = engine.core.undo_action(archive_batch).await.expect("undo");
    assert_eq!(undone.queued, 1);
    engine.until_listed(&scratch, &subject(2), true).await;
    engine.until_listed("Archive", &subject(2), false).await;
    let deadline = Instant::now() + WAIT;
    loop {
        let now = places(&mut admin, &message_id(2)).await;
        if now.len() == 1 && now[0].0 == scratch {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "moved back on the server: {now:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        engine
            .core
            .outbox_counts(account)
            .await
            .expect("counts")
            .pending,
        0
    );
    engine.core.shutdown().await;

    // 6. The sync, IDLE and reading stayed on the read-only allowlist with writes on; only
    //    the outbox's own connection wrote.
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
    let verbs = read_tap.verbs();
    for verb in &verbs {
        assert!(
            allowed.contains(&verb.as_str()),
            "{verb} on a read connection: {:#?}",
            read_tap.lines()
        );
    }
    assert!(verbs.iter().any(|v| v == "EXAMINE"), "positive control");
    assert!(read_tap.lines().iter().all(|l| !l.contains(" SELECT ")));
    let written = write_tap.verbs();
    for needed in ["SELECT", "UID STORE"] {
        assert!(
            written.iter().any(|v| v == needed),
            "{needed} on the outbox connection: {written:?}"
        );
    }
    assert!(
        written.iter().any(|v| v == "UID MOVE" || v == "UID COPY"),
        "a move on the outbox connection: {written:?}"
    );
    println!(
        "flag confirmed after {confirmed_after:?}, folder synced after {synced_after:?}; \
         {} read lines, {} write lines",
        read_tap.lines().len(),
        write_tap.lines().len()
    );

    clean_server(&mut admin).await;
    admin.logout().await.ok();
}
