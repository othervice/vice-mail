//! The outbox against the local test server (Stalwart): the acceptance tests of stage 4's core.
//! Ignored by default; they need the server running (any seed). Take the shared lock first:
//!
//! ```text
//! scripts/testmail.sh lock outbox && scripts/testmail.sh start
//! cargo test -p mail-engine outbox::server_tests -- --include-ignored --test-threads=1
//! scripts/testmail.sh stop && scripts/testmail.sh unlock outbox
//! ```
//!
//! They work in bob's account (alice's seed stays untouched) with messages they append
//! themselves (`Message-ID: <...@outbox.test>`) and folders named `VmOutbox-...`, and remove
//! both before and after each test. Server state is checked through a separate IMAP session;
//! the outbox talks to the server through a fault-injecting proxy, which plays "offline".

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_imap::test_support::{Proxy, TestEnv};
use mail_imap::{ImapBackend, ImapConnector, WireTap};
use mail_model::{
    Access, AccountId, ActionError, ConflictReason, CoreError, Flags, MailAction, MailBackend,
    MailConnector, MailboxId, MailboxRole, MessageRef, OperationId, OperationKind, OperationNotice,
    OperationPayload, OperationState, OutboxEvent, Security, ServerSettings, Uid, UidSet,
    WriteConnector,
};
use mail_store::{MailboxBaseline, NewAccount, Store, StoreOptions, StoredOperation};
use tempfile::TempDir;
use tokio::time::Instant;

use super::{Outbox, OutboxConfig, SyncHook};
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretStore};
use crate::sync_state::{BackoffPolicy, Jitter, SplitMix64};

const WAIT: Duration = Duration::from_secs(20);
const DOMAIN: &str = "outbox.test";
const KEYWORD: &str = "$VmLabel";

/// The tests share bob's account: one at a time, together with `transfer_server_tests`, whose
/// folders this file's tests would otherwise see appear and vanish while they list bob's folders.
pub(super) static SERVER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn config(env: &TestEnv) -> OutboxConfig {
    let mut config = OutboxConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(20),
            max: Duration::from_millis(200),
            jitter: 0.5,
        },
        extra_root_ca: Some(env.extra_root()),
        linger: Duration::from_millis(200),
        ..OutboxConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

/// Records what the outbox asked the sync to look at.
#[derive(Default)]
struct Hook(Mutex<Vec<MailboxId>>);

impl SyncHook for Hook {
    fn sync_requested(&self, _account: AccountId, mailboxes: &[MailboxId]) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(mailboxes);
    }
}

impl Hook {
    fn seen(&self) -> Vec<MailboxId> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Where a message lies on the server: folder, UID, flags.
type Places = Vec<(String, Uid, Flags)>;

struct Bed {
    env: TestEnv,
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    account: AccountId,
    credentials: Credentials,
    proxy: Arc<Proxy>,
    tap: WireTap,
    hook: Arc<Hook>,
    outbox: Outbox<ImapConnector>,
    /// Direct read-write session to the server for setup and checks (not through the proxy).
    admin: ImapBackend,
    run: String,
    scratch: String,
}

impl Bed {
    /// Bob's account with `count` new unread messages in the INBOX and an empty scratch
    /// folder, synced into a fresh store; writes on; the outbox through the proxy.
    async fn new(test: &str, count: usize, connector: ImapConnector) -> Self {
        let env = TestEnv::require();
        let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
            .parse()
            .expect("addr");
        let proxy = Proxy::start(upstream).await.expect("proxy");
        let mut admin = ImapConnector::new()
            .connect(&env.config(&env.bob, Security::Tls, Access::ReadWrite))
            .await
            .expect("admin session");
        clean_server(&mut admin).await;
        let run = format!("ob{}-{test}", std::process::id());
        let scratch = format!("VmOutbox-{run}");
        raw(&mut admin, &format!("CREATE \"{scratch}\"")).await;
        for n in 1..=count {
            append(&mut admin, "INBOX", &run, n).await;
        }

        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("mail.sqlite3");
        let store = Store::open(&path, StoreOptions::default()).expect("store");
        let account = AccountId::from_uuid(uuid::Uuid::new_v4());
        let key = CredentialRef::for_account(account);
        let keychain = MemorySecretStore::new();
        keychain.set(&key, &env.bob.password).expect("keychain");
        let credentials = Credentials::new(keychain);
        let server = |port| ServerSettings {
            host: "127.0.0.1".into(),
            port,
            security: Security::Tls,
            username: env.bob.address.clone(),
        };
        store
            .insert_account(NewAccount {
                id: account,
                email: env.bob.address.clone(),
                display_name: "Bob".into(),
                imap: server(proxy.port()),
                smtp: server(env.submission_port),
                credential_ref: key.to_string(),
            })
            .await
            .expect("account");
        store
            .set_writes_enabled(account, true)
            .await
            .expect("writes on");
        let tap = WireTap::new();
        let hook = Arc::new(Hook::default());
        let outbox = Outbox::new(
            store.clone(),
            Arc::new(connector.with_wire_tap(tap.clone())),
            credentials.clone(),
            config(&env),
        );
        outbox.set_sync_hook(hook.clone());
        let bed = Self {
            env,
            _dir: dir,
            path,
            store,
            account,
            credentials,
            proxy,
            tap,
            hook,
            outbox,
            admin,
            run,
            scratch,
        };
        bed.resync().await;
        bed
    }

    /// A fresh outbox (new connector and tap) on the same database, as after a restart.
    async fn restart(&mut self, connector: ImapConnector) {
        self.outbox.shutdown().await;
        let store = Store::open(&self.path, StoreOptions::default()).expect("reopen");
        self.store = store.clone();
        self.tap = WireTap::new();
        self.outbox = Outbox::new(
            store,
            Arc::new(connector.with_wire_tap(self.tap.clone())),
            self.credentials.clone(),
            config(&self.env),
        );
        self.outbox.set_sync_hook(self.hook.clone());
    }

    async fn finish(mut self) {
        self.outbox.shutdown().await;
        clean_server(&mut self.admin).await;
        self.admin.logout().await.ok();
    }

    /// Nothing reaches the server: new connections are refused, open ones cut.
    fn offline(&self) {
        self.proxy.refuse_next(1_000_000);
        self.proxy.cut_responses(Duration::ZERO);
    }

    fn online(&self) {
        self.proxy.refuse_next(0);
        self.outbox.wake(self.account);
    }

    /// Rebuilds the local state of every mailbox from the server (what the sync does, done
    /// plainly): folder list, then per mailbox all headers. Only while the outbox is idle.
    async fn resync(&self) {
        let mut reader = ImapConnector::new()
            .connect(
                &self
                    .env
                    .config(&self.env.bob, Security::Tls, Access::ReadOnly),
            )
            .await
            .expect("reader");
        let remote = reader.list_mailboxes().await.expect("list");
        self.store
            .sync_mailboxes(self.account, remote)
            .await
            .expect("mailboxes");
        for state in self
            .store
            .mailbox_sync_states(self.account)
            .await
            .expect("states")
        {
            if !state.selectable {
                continue;
            }
            let opened = reader.open_mailbox(&state.name).await.expect("examine");
            self.store
                .reset_mailbox(
                    state.mailbox_id,
                    MailboxBaseline {
                        uid_validity: opened.uid_validity,
                        uid_next: opened.uid_next,
                        highest_modseq: opened.highest_modseq,
                    },
                )
                .await
                .expect("reset");
            let uids = reader.search_uids(&opened).await.expect("search");
            let headers = if uids.is_empty() {
                Vec::new()
            } else {
                reader.fetch_headers(&opened, &uids).await.expect("headers")
            };
            self.store
                .store_headers(state.mailbox_id, opened.uid_validity, headers, Some(1))
                .await
                .expect("store");
        }
        reader.logout().await.ok();
    }

    async fn mailbox(&self, role: MailboxRole) -> MailboxId {
        self.store
            .mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == role)
            .map(|m| m.id)
            .unwrap_or_else(|| panic!("no {role:?} mailbox"))
    }

    async fn mailbox_named(&self, path: &str) -> MailboxId {
        self.store
            .mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == path)
            .map(|m| m.id)
            .unwrap_or_else(|| panic!("no mailbox {path}"))
    }

    fn subject(&self, n: usize) -> String {
        format!("{} message {n}", self.run)
    }

    fn message_id(&self, n: usize) -> String {
        format!("{}-{n}@{DOMAIN}", self.run)
    }

    /// Local locations of message `n`: (mailbox, ref, unread, flagged, keywords).
    async fn local(&self, n: usize) -> Vec<(MailboxId, MessageRef, bool, bool, Vec<String>)> {
        let subject = self.subject(n);
        let mut found = Vec::new();
        for mailbox in self.store.mailboxes(self.account).await.expect("mailboxes") {
            let page = self
                .store
                .list_messages(mailbox.id, None, 500)
                .await
                .expect("list");
            for item in page.items.into_iter().filter(|i| i.subject == subject) {
                found.push((
                    mailbox.id,
                    MessageRef {
                        mailbox_id: mailbox.id,
                        message_id: item.id,
                    },
                    item.unread,
                    item.flagged,
                    item.keywords,
                ));
            }
        }
        found
    }

    async fn only_ref(&self, n: usize) -> MessageRef {
        let local = self.local(n).await;
        assert_eq!(local.len(), 1, "message {n} locally: {local:?}");
        local[0].1
    }

    /// Where the test's messages lie on the server, by Message-ID.
    async fn server(&mut self) -> BTreeMap<String, Places> {
        let suffix = format!("@{DOMAIN}");
        let mut found: BTreeMap<String, Places> = BTreeMap::new();
        for (name, uid, header) in all_headers(&mut self.admin).await {
            if let Some(id) = header.message_id.filter(|id| id.ends_with(&suffix)) {
                found.entry(id).or_default().push((name, uid, header.flags));
            }
        }
        found
    }

    async fn server_places(&mut self, n: usize) -> Places {
        let id = self.message_id(n);
        self.server().await.remove(&id).unwrap_or_default()
    }

    async fn apply(&self, action: MailAction, at: Vec<MessageRef>) -> mail_model::ActionResult {
        self.outbox
            .apply(action.clone(), at)
            .await
            .unwrap_or_else(|err| panic!("{action:?}: {err}"))
    }

    async fn batch_ops(&self, batch: mail_model::BatchId) -> Vec<StoredOperation> {
        self.store
            .operations(self.account, None, 500)
            .await
            .expect("operations")
            .into_iter()
            .filter(|op| op.batch == batch)
            .collect()
    }

    /// Waits until no operation of the account is pending; returns all operations.
    async fn settled(&self) -> Vec<StoredOperation> {
        let deadline = Instant::now() + WAIT;
        loop {
            let pending = self
                .store
                .pending_operations(self.account)
                .await
                .expect("pending");
            if pending.is_empty() {
                return self
                    .store
                    .operations(self.account, None, 500)
                    .await
                    .expect("operations");
            }
            assert!(
                Instant::now() < deadline,
                "operations still pending: {:#?}",
                pending
                    .iter()
                    .map(|op| (op.id, op.state, op.attempts, op.last_error.clone()))
                    .collect::<Vec<_>>()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Waits until the operation is in `state`.
    async fn wait_state(&self, id: OperationId, state: OperationState) -> StoredOperation {
        let deadline = Instant::now() + WAIT;
        loop {
            let op = self
                .store
                .operation(id)
                .await
                .expect("read")
                .expect("exists");
            if op.state == state {
                return op;
            }
            assert!(
                Instant::now() < deadline,
                "operation {id} stays {:?}: {op:?}",
                op.state
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
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
        "From: Outbox Test <writer@{DOMAIN}>\r\nTo: bob@vicemail.test\r\n\
         Subject: {run} message {n}\r\nDate: Mon, 02 Nov 2026 10:{:02}:00 +0000\r\n\
         Message-ID: <{run}-{n}@{DOMAIN}>\r\n\r\nBody {n}.\r\n",
        n % 60
    );
    raw(
        admin,
        &format!("APPEND \"{folder}\" () {{{}+}}\r\n{body}", body.len()),
    )
    .await;
}

/// Every message of every selectable folder: (folder, UID, header).
async fn all_headers(admin: &mut ImapBackend) -> Vec<(String, Uid, mail_model::MessageHeader)> {
    let mut all = Vec::new();
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
            all.push((remote.name.clone(), header.uid, header));
        }
    }
    all
}

/// Removes every test message and folder of the outbox tests (leftovers of aborted runs too).
async fn clean_server(admin: &mut ImapBackend) {
    let suffix = format!("@{DOMAIN}>");
    let suffix_plain = format!("@{DOMAIN}");
    let mut by_folder: BTreeMap<String, Vec<Uid>> = BTreeMap::new();
    for (folder, uid, header) in all_headers(admin).await {
        if header
            .message_id
            .as_deref()
            .is_some_and(|id| id.ends_with(&suffix_plain) || id.ends_with(&suffix))
        {
            by_folder.entry(folder).or_default().push(uid);
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
        if remote.name.starts_with("VmOutbox-") {
            raw(admin, &format!("DELETE \"{}\"", remote.name)).await;
        }
    }
}

fn folders(places: &Places) -> Vec<&str> {
    places.iter().map(|(f, _, _)| f.as_str()).collect()
}

// ---------------------------------------------------------------------------------------------
// Acceptance: every action online, and recorded offline then executed after connecting

#[tokio::test]
#[ignore = "needs the test server"]
async fn every_action_online_and_offline() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("actions", 30, ImapConnector::new()).await;
    let mut events = bed.outbox.subscribe();
    let inbox = bed.mailbox(MailboxRole::Inbox).await;
    let archive = bed.mailbox(MailboxRole::Archive).await;
    let trash = bed.mailbox(MailboxRole::Trash).await;
    let junk = bed.mailbox(MailboxRole::Junk).await;
    let scratch = bed.mailbox_named(&bed.scratch.clone()).await;
    let scratch_name = bed.scratch.clone();

    // Prepared state per message, set on the server before syncing.
    enum Prep {
        Nothing,
        Seen,
        Flagged,
        Keyword,
        InJunk,
    }
    #[derive(Clone, Copy)]
    enum Expect {
        Seen(bool),
        Flagged(bool),
        Keyword(bool),
        In(&'static str),
        Nowhere,
    }
    let cases: Vec<(MailAction, Prep, Expect)> = vec![
        (MailAction::MarkRead, Prep::Nothing, Expect::Seen(true)),
        (MailAction::MarkUnread, Prep::Seen, Expect::Seen(false)),
        (MailAction::Flag, Prep::Nothing, Expect::Flagged(true)),
        (MailAction::Unflag, Prep::Flagged, Expect::Flagged(false)),
        (
            MailAction::AddKeyword {
                keyword: KEYWORD.into(),
            },
            Prep::Nothing,
            Expect::Keyword(true),
        ),
        (
            MailAction::RemoveKeyword {
                keyword: KEYWORD.into(),
            },
            Prep::Keyword,
            Expect::Keyword(false),
        ),
        (MailAction::Archive, Prep::Nothing, Expect::In("Archive")),
        (MailAction::Delete, Prep::Nothing, Expect::In("Trash")),
        (
            MailAction::DeletePermanently,
            Prep::Nothing,
            Expect::Nowhere,
        ),
        (
            MailAction::Move { target: scratch },
            Prep::Nothing,
            Expect::In("scratch"),
        ),
        (MailAction::Spam, Prep::Nothing, Expect::In("Junk")),
        (MailAction::NotSpam, Prep::InJunk, Expect::In("INBOX")),
    ];

    // Prepare: message 2k-1 for the online run, 2k for the offline run.
    for (k, (_, prep, _)) in cases.iter().enumerate() {
        for n in [2 * k + 1, 2 * k + 2] {
            let uid = bed.server_places(n).await[0].1;
            bed.admin.open_mailbox("INBOX").await.expect("select");
            match prep {
                Prep::Nothing => {}
                Prep::Seen => {
                    raw(&mut bed.admin, &format!("UID STORE {uid} +FLAGS (\\Seen)")).await
                }
                Prep::Flagged => {
                    raw(
                        &mut bed.admin,
                        &format!("UID STORE {uid} +FLAGS (\\Flagged)"),
                    )
                    .await;
                }
                Prep::Keyword => {
                    raw(
                        &mut bed.admin,
                        &format!("UID STORE {uid} +FLAGS ({KEYWORD})"),
                    )
                    .await;
                }
                Prep::InJunk => raw(&mut bed.admin, &format!("UID MOVE {uid} \"Junk\"")).await,
            }
        }
    }
    bed.resync().await;
    let target_name = |e: &'static str| -> String {
        if e == "scratch" {
            scratch_name.clone()
        } else {
            e.to_owned()
        }
    };

    for (k, (action, _, expect)) in cases.iter().enumerate() {
        for offline in [false, true] {
            let n = 2 * k + 1 + usize::from(offline);
            let at = bed.only_ref(n).await;
            if offline {
                bed.offline();
            }
            let result = bed.apply(action.clone(), vec![at]).await;
            let batch = result
                .batch
                .unwrap_or_else(|| panic!("{action:?}: nothing recorded"));
            assert_eq!(result.queued, 1, "{action:?}");

            // The local change is there at once.
            let local = bed.local(n).await;
            match expect {
                Expect::Seen(v) => assert_eq!(local[0].2, !v, "{action:?} unread locally"),
                Expect::Flagged(v) => assert_eq!(local[0].3, *v, "{action:?} flagged locally"),
                Expect::Keyword(v) => assert_eq!(
                    local[0].4.iter().any(|k| k.eq_ignore_ascii_case(KEYWORD)),
                    *v,
                    "{action:?} keyword locally"
                ),
                Expect::In(_) | Expect::Nowhere => {
                    // Gone from where it was (online, the target may show it already).
                    let source = if matches!(action, MailAction::NotSpam) {
                        junk
                    } else {
                        inbox
                    };
                    assert!(
                        local.iter().all(|(m, ..)| *m != source),
                        "{action:?}: still listed in the source {local:?}"
                    );
                }
            }

            if offline {
                // Recorded, not sent: queued and the server unchanged.
                tokio::time::sleep(Duration::from_millis(300)).await;
                let ops = bed.batch_ops(batch).await;
                assert_eq!(ops.len(), 1);
                assert_eq!(ops[0].state, OperationState::Queued, "{action:?} offline");
                assert_eq!(ops[0].attempts, 0, "{action:?}: nothing was sent");
                let places = bed.server_places(n).await;
                assert_eq!(
                    folders(&places),
                    [if matches!(action, MailAction::NotSpam) {
                        "Junk"
                    } else {
                        "INBOX"
                    }],
                    "{action:?}: server unchanged while offline"
                );
                bed.online();
            }
            bed.settled().await;
            let ops = bed.batch_ops(batch).await;
            assert_eq!(
                ops[0].state,
                OperationState::Confirmed,
                "{action:?} (offline {offline}): {:?}",
                ops[0]
            );

            let places = bed.server_places(n).await;
            match expect {
                Expect::Seen(v) => {
                    assert_eq!(folders(&places), ["INBOX"]);
                    assert_eq!(places[0].2.seen, *v, "{action:?} on the server");
                }
                Expect::Flagged(v) => assert_eq!(places[0].2.flagged, *v, "{action:?}"),
                Expect::Keyword(v) => {
                    assert_eq!(places[0].2.has_keyword(KEYWORD), *v, "{action:?}");
                }
                Expect::In(folder) => {
                    assert_eq!(
                        folders(&places),
                        [target_name(folder)],
                        "{action:?} (offline {offline})"
                    );
                    // COPYUID: the target location is known locally before any sync.
                    let local = bed.local(n).await;
                    let want = match *folder {
                        "Archive" => archive,
                        "Trash" => trash,
                        "Junk" => junk,
                        "INBOX" => inbox,
                        _ => scratch,
                    };
                    assert_eq!(local.len(), 1, "{action:?}: {local:?}");
                    assert_eq!(local[0].0, want, "{action:?}");
                }
                Expect::Nowhere => assert!(places.is_empty(), "{action:?}: {places:?}"),
            }
        }
    }

    // Events: every operation was announced queued and then confirmed; lists changed.
    let mut states: BTreeMap<OperationId, Vec<OperationState>> = BTreeMap::new();
    let mut lists = 0;
    while let Ok(event) = events.try_recv() {
        match event {
            OutboxEvent::OperationChanged { operation } => {
                states
                    .entry(operation.id)
                    .or_default()
                    .push(operation.state);
            }
            OutboxEvent::MessagesChanged { .. } => lists += 1,
            _ => {}
        }
    }
    assert_eq!(states.len(), cases.len() * 2);
    for (id, seen) in &states {
        assert_eq!(
            seen.first(),
            Some(&OperationState::Queued),
            "{id}: {seen:?}"
        );
        assert_eq!(
            seen.last(),
            Some(&OperationState::Confirmed),
            "{id}: {seen:?}"
        );
    }
    assert!(lists >= cases.len() * 2);
    // The sync was asked to look at what changed on the server.
    let asked: BTreeSet<MailboxId> = bed.hook.seen().into_iter().collect();
    for mailbox in [inbox, archive, trash, junk, scratch] {
        assert!(asked.contains(&mailbox), "{mailbox} not synced: {asked:?}");
    }
    // Only the command layer's writing commands, all through UID; CONDSTORE servers get
    // UNCHANGEDSINCE.
    let verbs: BTreeSet<String> = bed.tap.verbs().into_iter().collect();
    assert!(
        verbs.contains("UID MOVE") && verbs.contains("UID STORE") && verbs.contains("UID EXPUNGE"),
        "{verbs:?}"
    );
    assert!(
        !verbs.contains("EXPUNGE") && !verbs.contains("STORE"),
        "{verbs:?}"
    );
    assert!(
        bed.tap.lines().iter().any(|l| l.contains("UNCHANGEDSINCE")),
        "no conditional STORE"
    );
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Acceptance: a second session moves the message while the operation waits

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_message_moved_elsewhere_meanwhile_is_a_conflict_and_exists_once() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("conflict", 3, ImapConnector::new()).await;
    let scratch = bed.scratch.clone();

    // 1: archive waits offline; the other session moves the message to the scratch folder.
    // 2: "read" waits offline; the other session moves it away too.
    // 3: "read" waits offline; the other session flags it (MODSEQ grows): UNCHANGEDSINCE.
    let one = bed.only_ref(1).await;
    let two = bed.only_ref(2).await;
    let three = bed.only_ref(3).await;
    bed.offline();
    let archived = bed.apply(MailAction::Archive, vec![one]).await;
    let read = bed.apply(MailAction::MarkRead, vec![two]).await;
    let read_three = bed.apply(MailAction::MarkRead, vec![three]).await;
    let uid = |places: &Places| places[0].1;
    let (u1, u2, u3) = (
        uid(&bed.server_places(1).await),
        uid(&bed.server_places(2).await),
        uid(&bed.server_places(3).await),
    );
    bed.admin.open_mailbox("INBOX").await.expect("select");
    raw(&mut bed.admin, &format!("UID MOVE {u1},{u2} \"{scratch}\"")).await;
    raw(
        &mut bed.admin,
        &format!("UID STORE {u3} +FLAGS (\\Flagged)"),
    )
    .await;
    bed.online();
    bed.settled().await;

    let op = &bed.batch_ops(archived.batch.expect("batch")).await[0];
    assert_eq!(op.state, OperationState::Conflict);
    assert_eq!(op.conflict, Some(ConflictReason::Gone));
    let places = bed.server_places(1).await;
    assert_eq!(
        folders(&places),
        [scratch.as_str()],
        "the server wins, exactly once"
    );
    assert!(
        bed.local(1).await.is_empty(),
        "not shown in the inbox any more"
    );

    let op = &bed.batch_ops(read.batch.expect("batch")).await[0];
    assert_eq!(
        (op.state, op.conflict),
        (OperationState::Conflict, Some(ConflictReason::Gone))
    );
    assert_eq!(folders(&bed.server_places(2).await), [scratch.as_str()]);
    assert!(
        !bed.server_places(2).await[0].2.seen,
        "nothing was stored on the moved mail"
    );

    let op = &bed.batch_ops(read_three.batch.expect("batch")).await[0];
    assert_eq!(
        (op.state, op.conflict),
        (OperationState::Conflict, Some(ConflictReason::Modified)),
        "{op:?}"
    );
    let places = bed.server_places(3).await;
    assert!(
        places[0].2.flagged && !places[0].2.seen,
        "server state kept: {places:?}"
    );
    let local = bed.local(3).await;
    assert!(
        local[0].2 && local[0].3,
        "local shows the server's state: {local:?}"
    );
    assert!(
        bed.tap
            .lines()
            .iter()
            .any(|l| l.contains("UNCHANGEDSINCE") && l.contains(&u3.to_string())),
        "{:?}",
        bed.tap.lines()
    );
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Acceptance: the connection is cut right after the command; after a restart nothing is
// applied twice

async fn cut_after(
    bed: &mut Bed,
    command: &'static str,
    action: MailAction,
    n: usize,
) -> OperationId {
    let proxy = Arc::clone(&bed.proxy);
    bed.tap.on_write(command, move || {
        proxy.refuse_next(1_000_000);
        proxy.cut_responses(Duration::from_millis(300));
    });
    let at = bed.only_ref(n).await;
    let result = bed.apply(action, vec![at]).await;
    let id = bed.batch_ops(result.batch.expect("batch")).await[0].id;
    // Sent, the answer lost: in flight, and the outbox cannot reconnect.
    bed.wait_state(id, OperationState::InFlight).await;
    let deadline = Instant::now() + WAIT;
    while bed.proxy.cuts() == 0 {
        assert!(Instant::now() < deadline, "{command} was never sent");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        bed.store
            .operation(id)
            .await
            .expect("read")
            .expect("op")
            .state,
        OperationState::InFlight
    );
    id
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_connection_cut_after_the_command_is_not_applied_twice() {
    let _server = SERVER.lock().await;

    // UID MOVE: the server moved it; the check after the restart sees that.
    let mut bed = Bed::new("cut-move", 3, ImapConnector::new()).await;
    let id = cut_after(&mut bed, "UID MOVE", MailAction::Archive, 1).await;
    assert_eq!(
        folders(&bed.server_places(1).await),
        ["Archive"],
        "the server did it"
    );
    bed.restart(ImapConnector::new()).await;
    bed.proxy.refuse_next(0);
    bed.outbox.start().await.expect("start");
    let op = bed.wait_state(id, OperationState::Confirmed).await;
    assert_eq!(op.attempts, 1, "checked, not sent again");
    assert_eq!(
        folders(&bed.server_places(1).await),
        ["Archive"],
        "exactly once"
    );
    assert!(
        !bed.tap.verbs().contains(&"UID MOVE".to_owned()),
        "{:?}",
        bed.tap.verbs()
    );

    // UID STORE: checked, the flag is there, not stored again.
    let id = cut_after(&mut bed, "UID STORE", MailAction::Flag, 2).await;
    bed.restart(ImapConnector::new()).await;
    bed.proxy.refuse_next(0);
    bed.outbox.start().await.expect("start");
    bed.wait_state(id, OperationState::Confirmed).await;
    assert!(bed.server_places(2).await[0].2.flagged);
    assert!(
        !bed.tap.verbs().contains(&"UID STORE".to_owned()),
        "{:?}",
        bed.tap.verbs()
    );
    bed.finish().await;

    // Without MOVE: cut after UID COPY. The copy exists, the original too; after the restart
    // only the original is removed, the copy is never made twice.
    let no_move = || ImapConnector::new().with_masked_capabilities(["MOVE"]);
    let mut bed = Bed::new("cut-copy", 2, no_move()).await;
    let id = cut_after(&mut bed, "UID COPY", MailAction::Archive, 1).await;
    let mut places = folders(&bed.server_places(1).await)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    places.sort();
    assert_eq!(places, ["Archive", "INBOX"], "copied, original still there");
    bed.restart(no_move()).await;
    bed.proxy.refuse_next(0);
    bed.outbox.start().await.expect("start");
    let op = bed.wait_state(id, OperationState::Confirmed).await;
    assert_eq!(op.attempts, 1);
    assert_eq!(
        folders(&bed.server_places(1).await),
        ["Archive"],
        "exactly once"
    );
    let verbs = bed.tap.verbs();
    assert!(!verbs.contains(&"UID COPY".to_owned()), "{verbs:?}");
    assert!(verbs.contains(&"UID EXPUNGE".to_owned()), "{verbs:?}");
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Acceptance: moving without MOVE; EXPUNGE never touches other deleted mail

#[tokio::test]
#[ignore = "needs the test server"]
async fn moving_without_move_leaves_other_deleted_mail_alone() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new(
        "no-move",
        4,
        ImapConnector::new().with_masked_capabilities(["MOVE"]),
    )
    .await;
    let scratch_id = bed.mailbox_named(&bed.scratch.clone()).await;
    let scratch = bed.scratch.clone();
    // Message 4 is marked deleted by another client.
    let foreign = bed.server_places(4).await[0].1;
    bed.admin.open_mailbox("INBOX").await.expect("select");
    raw(
        &mut bed.admin,
        &format!("UID STORE {foreign} +FLAGS (\\Deleted)"),
    )
    .await;

    // With UIDPLUS: COPY, \Deleted, UID EXPUNGE of exactly the moved message.
    let one = bed.only_ref(1).await;
    bed.apply(MailAction::Move { target: scratch_id }, vec![one])
        .await;
    let ops = bed.settled().await;
    assert_eq!(ops[0].state, OperationState::Confirmed);
    assert!(ops[0].notice.is_none());
    assert_eq!(folders(&bed.server_places(1).await), [scratch.as_str()]);
    let other = bed.server_places(4).await;
    assert_eq!(
        folders(&other),
        ["INBOX"],
        "the other deleted mail is still there"
    );
    assert!(other[0].2.deleted);
    let verbs = bed.tap.verbs();
    assert!(verbs.contains(&"UID COPY".to_owned()) && verbs.contains(&"UID EXPUNGE".to_owned()));
    assert!(!verbs.contains(&"UID MOVE".to_owned()) && !verbs.contains(&"EXPUNGE".to_owned()));

    // Without UIDPLUS either: a plain EXPUNGE would remove message 4, so none is sent. The
    // copy is made, the original stays marked deleted, the user is told.
    bed.restart(ImapConnector::new().with_masked_capabilities(["MOVE", "UIDPLUS"]))
        .await;
    let two = bed.only_ref(2).await;
    let result = bed
        .apply(MailAction::Move { target: scratch_id }, vec![two])
        .await;
    bed.settled().await;
    let op = &bed.batch_ops(result.batch.expect("batch")).await[0];
    assert_eq!(op.state, OperationState::Confirmed);
    assert_eq!(op.notice, Some(OperationNotice::LeftMarkedDeleted));
    let mut places = bed.server_places(2).await;
    places.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(folders(&places), ["INBOX", scratch.as_str()]);
    assert!(places[0].2.deleted, "the original is marked deleted");
    assert!(bed.server_places(4).await[0].2.deleted, "untouched");
    assert!(
        !bed.tap.verbs().contains(&"EXPUNGE".to_owned()),
        "{:?}",
        bed.tap.verbs()
    );
    // Without COPYUID the target location is not known locally; the sync merges it later.
    assert!(bed.local(2).await.is_empty());

    // Once no other message is marked deleted, a plain EXPUNGE is safe. (The other client
    // takes its mark back; the original left behind above is expunged by hand.)
    let original = places[0].1;
    bed.admin.open_mailbox("INBOX").await.expect("select");
    raw(
        &mut bed.admin,
        &format!("UID STORE {foreign} -FLAGS (\\Deleted)"),
    )
    .await;
    raw(&mut bed.admin, &format!("UID EXPUNGE {original}")).await;
    bed.resync().await;
    let three = bed.only_ref(3).await;
    bed.apply(MailAction::Move { target: scratch_id }, vec![three])
        .await;
    bed.settled().await;
    assert_eq!(folders(&bed.server_places(3).await), [scratch.as_str()]);
    assert_eq!(
        bed.tap.verbs().iter().filter(|v| *v == "EXPUNGE").count(),
        1,
        "{:?}",
        bed.tap.verbs()
    );
    assert!(!bed.server_places(4).await[0].2.deleted);
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Acceptance: accounts without write access refuse everything

#[tokio::test]
#[ignore = "needs the test server"]
async fn accounts_without_write_access_refuse_everything() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("read-only", 2, ImapConnector::new()).await;
    bed.outbox
        .set_writes_enabled(bed.account, false)
        .await
        .expect("writes off");
    let at = bed.only_ref(1).await;
    let scratch = bed.mailbox_named(&bed.scratch.clone()).await;
    let accepted = bed.proxy.accepted();
    for action in [
        MailAction::MarkRead,
        MailAction::MarkUnread,
        MailAction::Flag,
        MailAction::Unflag,
        MailAction::AddKeyword {
            keyword: KEYWORD.into(),
        },
        MailAction::RemoveKeyword {
            keyword: KEYWORD.into(),
        },
        MailAction::Archive,
        MailAction::Delete,
        MailAction::DeletePermanently,
        MailAction::Move { target: scratch },
        MailAction::Spam,
        MailAction::NotSpam,
    ] {
        let err = bed
            .outbox
            .apply(action.clone(), vec![at])
            .await
            .expect_err("read-only");
        assert_eq!(
            err,
            CoreError::Action(ActionError::ReadOnly(bed.account)),
            "{action:?}"
        );
        assert_eq!(err.code(), mail_model::ErrorCode::ReadOnly);
    }
    let err = bed
        .outbox
        .create_mailbox(bed.account, "VmOutbox-never".into(), None)
        .await
        .expect_err("read-only");
    assert_eq!(err.code(), mail_model::ErrorCode::ReadOnly);
    assert!(
        bed.store
            .operations(bed.account, None, 10)
            .await
            .expect("ops")
            .is_empty(),
        "nothing recorded"
    );
    assert!(bed.local(1).await[0].2, "still unread locally");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        bed.proxy.accepted(),
        accepted,
        "no connection was even opened"
    );
    assert!(bed.tap.is_empty(), "nothing written");

    // The connector refuses a writer for a read-only configuration before connecting.
    let config = bed.env.config_at(
        &bed.env.bob,
        "127.0.0.1",
        bed.proxy.port(),
        Security::Tls,
        Access::ReadOnly,
    );
    let err = ImapConnector::new()
        .connect_writer(&config)
        .await
        .expect_err("read-only config");
    assert!(
        matches!(err, mail_model::BackendError::ReadOnly(_)),
        "{err}"
    );
    assert_eq!(bed.proxy.accepted(), accepted);

    // Turned on, the same action goes through.
    bed.outbox
        .set_writes_enabled(bed.account, true)
        .await
        .expect("writes on");
    bed.apply(MailAction::MarkRead, vec![at]).await;
    bed.settled().await;
    assert!(bed.server_places(1).await[0].2.seen);
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Archive without an archive folder: a typed error, then creating one is an operation

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_missing_archive_is_a_typed_error_and_creating_one_an_operation() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("create", 1, ImapConnector::new()).await;
    let mut events = bed.outbox.subscribe();
    // Locally the account has no archive (the user cleared the role).
    let archive = bed.mailbox(MailboxRole::Archive).await;
    bed.store
        .set_mailbox_role(archive, MailboxRole::Custom)
        .await
        .expect("clear role");
    let at = bed.only_ref(1).await;
    let err = bed
        .outbox
        .apply(MailAction::Archive, vec![at])
        .await
        .expect_err("no archive");
    assert_eq!(
        err,
        CoreError::Action(ActionError::MissingMailbox(MailboxRole::Archive))
    );

    let path = format!("VmOutbox-{}-Ablage", bed.run);
    let created = bed
        .outbox
        .create_mailbox(bed.account, path.clone(), Some(MailboxRole::Archive))
        .await
        .expect("create");
    bed.settled().await;
    let ops = bed.batch_ops(created.batch.expect("batch")).await;
    assert_eq!(ops[0].payload.kind(), OperationKind::CreateMailbox);
    assert_eq!(ops[0].state, OperationState::Confirmed, "{:?}", ops[0]);
    let new = bed.mailbox(MailboxRole::Archive).await;
    assert_eq!(
        new,
        bed.mailbox_named(&path).await,
        "the new folder is the archive"
    );
    let mut list_event = false;
    while let Ok(event) = events.try_recv() {
        list_event |= matches!(event, OutboxEvent::MailboxesChanged { .. });
    }
    assert!(list_event);

    bed.apply(MailAction::Archive, vec![at]).await;
    bed.settled().await;
    assert_eq!(folders(&bed.server_places(1).await), [path.as_str()]);
    // Repeating the creation is harmless (the folder exists).
    bed.outbox
        .create_mailbox(bed.account, path.clone(), None)
        .await
        .expect("again");
    let ops = bed.settled().await;
    assert!(
        ops.iter().all(|op| op.state == OperationState::Confirmed),
        "{ops:?}"
    );
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Audit 24.11.2026: renaming and deleting folders of the user's own (RENAME, DELETE)

#[tokio::test]
#[ignore = "needs the test server"]
async fn folders_are_renamed_with_their_mail_and_subfolders_and_deleted() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("folders", 1, ImapConnector::new()).await;
    let scratch = bed.scratch.clone();
    let child = format!("{scratch}/Unter");
    raw(&mut bed.admin, &format!("CREATE \"{child}\"")).await;
    append(&mut bed.admin, &scratch, &bed.run.clone(), 2).await;
    bed.resync().await;
    let folder = bed.mailbox_named(&scratch).await;
    let sub = bed.mailbox_named(&child).await;

    // Renamed: the folder keeps its row, its message and its subfolder.
    let renamed = format!("{scratch}-neu");
    let result = bed
        .outbox
        .rename_mailbox(bed.account, folder, renamed.clone())
        .await
        .expect("rename");
    bed.settled().await;
    let ops = bed.batch_ops(result.batch.expect("batch")).await;
    assert_eq!(ops[0].payload.kind(), OperationKind::RenameMailbox);
    assert_eq!(ops[0].state, OperationState::Confirmed, "{:?}", ops[0]);
    assert_eq!(bed.mailbox_named(&renamed).await, folder, "the same row");
    assert_eq!(
        bed.mailbox_named(&format!("{renamed}/Unter")).await,
        sub,
        "the subfolder came along"
    );
    assert_eq!(folders(&bed.server_places(2).await), [renamed.as_str()]);
    let listed: Vec<String> = bed
        .admin
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .map(|m| m.path)
        .collect();
    assert!(
        listed.contains(&renamed) && !listed.contains(&scratch),
        "{listed:?}"
    );

    // Deleted: refused while it has a subfolder; the subfolder, then the folder with its mail.
    let err = bed
        .outbox
        .delete_mailbox(bed.account, folder)
        .await
        .expect_err("subfolder");
    assert_eq!(
        err,
        CoreError::Action(ActionError::MailboxHasSubfolders(folder))
    );
    bed.outbox
        .delete_mailbox(bed.account, sub)
        .await
        .expect("delete the subfolder");
    bed.settled().await;
    bed.outbox
        .delete_mailbox(bed.account, folder)
        .await
        .expect("delete");
    let ops = bed.settled().await;
    assert!(
        ops.iter().all(|op| op.state == OperationState::Confirmed),
        "{ops:?}"
    );
    let listed: Vec<String> = bed
        .admin
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .map(|m| m.path)
        .collect();
    assert!(
        !listed.iter().any(|p| p.starts_with(&renamed)),
        "{listed:?}"
    );
    assert!(
        bed.server_places(2).await.is_empty(),
        "its message went with it"
    );
    assert!(
        !bed.store
            .mailboxes(bed.account)
            .await
            .expect("mailboxes")
            .iter()
            .any(|m| m.id == folder || m.id == sub)
    );
    bed.finish().await;
}

// ---------------------------------------------------------------------------------------------
// Acceptance: for any sequence of actions the Message-IDs over all folders stay the same,
// except for permanent deletes (random sequences with a fixed seed)

#[tokio::test]
#[ignore = "needs the test server"]
async fn random_action_sequences_keep_every_message() {
    let _server = SERVER.lock().await;
    for (seed, connector) in [
        (0x5eed_0001_u64, ImapConnector::new()),
        (
            0x5eed_0002_u64,
            ImapConnector::new().with_masked_capabilities(["MOVE"]),
        ),
        (
            0x5eed_0003_u64,
            ImapConnector::new().with_masked_capabilities(["MOVE", "UIDPLUS"]),
        ),
        (0x5eed_0004_u64, ImapConnector::new()),
    ] {
        random_sequence(seed, connector).await;
    }
}

async fn random_sequence(seed: u64, connector: ImapConnector) {
    const MESSAGES: usize = 12;
    const STEPS: usize = 150;
    let mut bed = Bed::new(&format!("prop{seed:x}"), MESSAGES, connector).await;
    let mut rng = SplitMix64::seeded(seed);
    let mut pick = |n: usize| -> usize {
        let unit = rng.unit();
        ((unit * n as f64) as usize).min(n.saturating_sub(1))
    };
    let scratch = bed.mailbox_named(&bed.scratch.clone()).await;
    let before: BTreeSet<String> = bed.server().await.into_keys().collect();
    assert_eq!(before.len(), MESSAGES);
    let mut batches = Vec::new();
    let mut offline = false;
    let mut log = Vec::new();

    for step in 0..STEPS {
        // Every message we still see locally, wherever it is.
        let mut visible = Vec::new();
        for n in 1..=MESSAGES {
            for (_, at, _, _, _) in bed.local(n).await {
                visible.push(at);
            }
        }
        let roll = pick(100);
        if roll < 8 {
            offline = !offline;
            if offline {
                bed.offline();
            } else {
                bed.online();
            }
            log.push(format!("{step}: offline={offline}"));
            continue;
        }
        if roll < 16 && !batches.is_empty() {
            let batch = batches[pick(batches.len())];
            let outcome = bed.outbox.undo(batch).await;
            log.push(format!("{step}: undo {batch} -> {outcome:?}"));
            if let Err(err) = outcome {
                assert!(
                    matches!(err, CoreError::Action(_)),
                    "only typed refusals: {err}"
                );
            }
            continue;
        }
        if visible.is_empty() {
            continue;
        }
        let action = match pick(13) {
            0 => MailAction::MarkRead,
            1 => MailAction::MarkUnread,
            2 => MailAction::Flag,
            3 => MailAction::Unflag,
            4 => MailAction::AddKeyword {
                keyword: KEYWORD.into(),
            },
            5 => MailAction::RemoveKeyword {
                keyword: KEYWORD.into(),
            },
            6 => MailAction::Archive,
            7 => MailAction::Delete,
            8 => MailAction::Move { target: scratch },
            9 => MailAction::Spam,
            10 => MailAction::NotSpam,
            11 => MailAction::Move {
                target: bed.mailbox(MailboxRole::Inbox).await,
            },
            // Rare: deleting for good is the one action that may shrink the set.
            _ if pick(3) == 0 => MailAction::DeletePermanently,
            _ => MailAction::MarkRead,
        };
        let count = 1 + pick(3);
        let mut targets = Vec::new();
        for _ in 0..count {
            let at = visible[pick(visible.len())];
            if !targets.contains(&at) {
                targets.push(at);
            }
        }
        let outcome = bed.outbox.apply(action.clone(), targets.clone()).await;
        log.push(format!("{step}: {action:?} {targets:?} -> {outcome:?}"));
        match outcome {
            Ok(result) => {
                if let Some(batch) = result.batch {
                    batches.push(batch);
                }
            }
            // Delete in the trash needs the explicit action: a typed refusal, nothing changed.
            Err(CoreError::Action(ActionError::ConfirmationRequired)) => {}
            Err(err) => panic!("step {step}: {action:?}: {err}\n{}", log.join("\n")),
        }
        if !offline && pick(4) == 0 {
            bed.settled().await;
            // Now and then the sync runs (without COPYUID it is what brings moved mail back).
            if pick(3) == 0 {
                bed.resync().await;
                log.push(format!("{step}: sync"));
            }
        }
    }

    bed.online();
    let ops = bed.settled().await;
    for op in &ops {
        assert!(
            matches!(op.state, OperationState::Confirmed),
            "seed {seed:x}: {op:#?}\n{}",
            log.join("\n")
        );
    }
    let expunged: BTreeSet<String> = ops
        .iter()
        .filter(|op| op.payload.kind() == OperationKind::Expunge)
        .filter_map(|op| match &op.payload {
            OperationPayload::Expunge { source } => source.message_id_header.clone(),
            _ => None,
        })
        .collect();
    let after = bed.server().await;
    let expected: BTreeSet<String> = before.difference(&expunged).cloned().collect();
    let found: BTreeSet<String> = after.keys().cloned().collect();
    assert_eq!(
        found,
        expected,
        "seed {seed:x}: Message-IDs over all folders\n{}",
        log.join("\n")
    );
    for (id, places) in &after {
        let live: Vec<_> = places.iter().filter(|(_, _, f)| !f.deleted).collect();
        assert_eq!(
            live.len(),
            1,
            "seed {seed:x}: {id} on the server: {places:?}"
        );
    }
    // The local state agrees with the server once synced.
    bed.resync().await;
    let mut local = BTreeSet::new();
    for n in 1..=MESSAGES {
        let here = bed.local(n).await;
        assert!(
            here.len() <= 1,
            "seed {seed:x}: message {n} twice locally: {here:?}"
        );
        if !here.is_empty() {
            local.insert(bed.message_id(n));
        }
    }
    assert_eq!(local, expected, "seed {seed:x}: local after sync");
    println!(
        "seed {seed:x}: {} operations, {} deleted for good, {} batches",
        ops.len(),
        expunged.len(),
        batches.len()
    );
    bed.finish().await;
}
