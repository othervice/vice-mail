//! Stage 16 (decision 0033): moving and copying between accounts against the local test server
//! (Stalwart), with the fault-injecting proxy in front of the target account. Ignored by
//! default; take the shared lock first:
//!
//! ```text
//! scripts/testmail.sh lock stage16 && scripts/testmail.sh start
//! cargo test -p mail-engine outbox::transfer_server_tests -- --include-ignored --test-threads=1
//! scripts/testmail.sh stop && scripts/testmail.sh unlock stage16
//! ```
//!
//! Source is a scratch folder `VmTransfer-...` in alice's account (her seed stays untouched),
//! target one in bob's; the messages are the tests' own (`Message-ID: <...@transfer.test>`),
//! removed with the folders before and after each test.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mail_imap::test_support::{Proxy, TestAccount, TestEnv};
use mail_imap::{ImapBackend, ImapConnector, WireTap};
use mail_model::{
    Access, AccountId, MailBackend, MailConnector, MailboxId, MessageRef, OperationKind,
    OperationState, Security, ServerSettings, TransferMode, Uid, UidSet,
};
use mail_store::{MailboxBaseline, NewAccount, Store, StoreOptions, StoredOperation};
use tempfile::TempDir;
use tokio::time::Instant;

use super::{Outbox, OutboxConfig};
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretStore};
use crate::sync_state::BackoffPolicy;

const WAIT: Duration = Duration::from_secs(20);
const DOMAIN: &str = "transfer.test";

// One lock with the other outbox server tests: both use bob's account (and alice's).
use super::server_tests::SERVER;

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

async fn raw(admin: &mut ImapBackend, line: &str) {
    let reply = admin
        .raw_command(line, |_| false)
        .await
        .unwrap_or_else(|err| panic!("{line}: {err}"));
    assert!(reply.ok, "{line}: {}", reply.text);
}

fn message(run: &str, n: usize) -> String {
    format!(
        "From: Transfer Test <writer@{DOMAIN}>\r\nTo: alice@vicemail.test\r\n\
         Subject: {run} message {n}\r\nDate: Mon, 02 Nov 2026 10:{:02}:00 +0000\r\n\
         Message-ID: <{run}-{n}@{DOMAIN}>\r\n\r\nBody {n}.\r\n",
        n % 60
    )
}

/// The test's messages in every folder of the session: Message-ID → [(folder, UID)].
async fn places(admin: &mut ImapBackend) -> BTreeMap<String, Vec<(String, Uid)>> {
    let mut found: BTreeMap<String, Vec<(String, Uid)>> = BTreeMap::new();
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
            if let Some(id) = header.message_id.filter(|id| id.ends_with(DOMAIN)) {
                found
                    .entry(id)
                    .or_default()
                    .push((remote.name.clone(), header.uid));
            }
        }
    }
    admin.raw_command("UNSELECT", |_| false).await.ok();
    found
}

async fn clean(admin: &mut ImapBackend) {
    let mut by_folder: BTreeMap<String, Vec<Uid>> = BTreeMap::new();
    for (_, at) in places(admin).await {
        for (folder, uid) in at {
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
        if remote.name.starts_with("VmTransfer-") {
            raw(admin, &format!("DELETE \"{}\"", remote.name)).await;
        }
    }
}

struct Side {
    account: AccountId,
    admin: ImapBackend,
    who: TestAccount,
}

struct Bed {
    env: TestEnv,
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    credentials: Credentials,
    proxy: Arc<Proxy>,
    tap: WireTap,
    outbox: Outbox,
    source: Side,
    target: Side,
    folder: String,
    run: String,
}

impl Bed {
    /// Alice's scratch folder with `count` messages (source, direct), bob's empty scratch
    /// folder (target, through the proxy); both accounts may write; the folders synced into a
    /// fresh store.
    async fn new(test: &str, count: usize) -> Self {
        let env = TestEnv::require();
        let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
            .parse()
            .expect("addr");
        let proxy = Proxy::start(upstream).await.expect("proxy");
        let run = format!("tr{}-{test}", std::process::id());
        let folder = format!("VmTransfer-{run}");
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("mail.sqlite3");
        let store = Store::open(&path, StoreOptions::default()).expect("store");
        let keychain = MemorySecretStore::new();
        let mut sides = Vec::new();
        for (who, port) in [
            (env.alice.clone(), env.imaps_port),
            (env.bob.clone(), proxy.port()),
        ] {
            let mut admin = ImapConnector::new()
                .connect(&env.config(&who, Security::Tls, Access::ReadWrite))
                .await
                .expect("admin session");
            clean(&mut admin).await;
            raw(&mut admin, &format!("CREATE \"{folder}\"")).await;
            let account = AccountId::from_uuid(uuid::Uuid::new_v4());
            let key = CredentialRef::for_account(account);
            keychain.set(&key, &who.password).expect("keychain");
            let server = |port| ServerSettings {
                host: "127.0.0.1".into(),
                port,
                security: Security::Tls,
                username: who.address.clone(),
            };
            store
                .insert_account(NewAccount {
                    id: account,
                    email: who.address.clone(),
                    display_name: who.address.clone(),
                    imap: server(port),
                    smtp: server(env.submission_port),
                    credential_ref: key.to_string(),
                })
                .await
                .expect("account");
            store
                .set_writes_enabled(account, true)
                .await
                .expect("writes on");
            sides.push(Side {
                account,
                admin,
                who,
            });
        }
        let target = sides.pop().expect("bob");
        let mut source = sides.pop().expect("alice");
        for n in 1..=count {
            let body = message(&run, n);
            raw(
                &mut source.admin,
                &format!("APPEND \"{folder}\" () {{{}+}}\r\n{body}", body.len()),
            )
            .await;
        }
        let credentials = Credentials::new(keychain);
        let tap = WireTap::new();
        let outbox = Outbox::new(
            store.clone(),
            Arc::new(ImapConnector::new().with_wire_tap(tap.clone())),
            credentials.clone(),
            config(&env),
        );
        let bed = Self {
            env,
            _dir: dir,
            path,
            store,
            credentials,
            proxy,
            tap,
            outbox,
            source,
            target,
            folder,
            run,
        };
        bed.resync(true).await;
        bed.resync(false).await;
        bed
    }

    /// The folder list of one side and the headers of its scratch folder, as the sync does.
    async fn resync(&self, source: bool) {
        let side = if source { &self.source } else { &self.target };
        let mut reader = ImapConnector::new()
            .connect(&self.env.config(&side.who, Security::Tls, Access::ReadOnly))
            .await
            .expect("reader");
        let remote = reader.list_mailboxes().await.expect("list");
        self.store
            .sync_mailboxes(side.account, remote)
            .await
            .expect("mailboxes");
        let state = self
            .store
            .mailbox_sync_states(side.account)
            .await
            .expect("states")
            .into_iter()
            .find(|s| s.name == self.folder)
            .expect("scratch folder");
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
        reader.logout().await.ok();
    }

    async fn folder_id(&self, account: AccountId) -> MailboxId {
        self.store
            .mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == self.folder)
            .map(|m| m.id)
            .expect("scratch folder")
    }

    async fn source_ref(&self, n: usize) -> MessageRef {
        let mailbox = self.folder_id(self.source.account).await;
        let subject = format!("{} message {n}", self.run);
        let page = self
            .store
            .list_messages(mailbox, None, 100)
            .await
            .expect("list");
        let item = page
            .items
            .into_iter()
            .find(|i| i.subject == subject)
            .expect("source message");
        MessageRef {
            mailbox_id: mailbox,
            message_id: item.id,
        }
    }

    /// Freezes message `n`'s bytes as the engine does and records the transfer.
    async fn transfer(&self, n: usize, mode: TransferMode) -> StoredOperation {
        let at = self.source_ref(n).await;
        let root = crate::compose::files::compose_dir(&self.store);
        let dir = mail_model::DraftId::from_random(uuid::Uuid::new_v4().as_u128());
        let relative = crate::compose::files::outgoing_path(&dir, "transfer.eml");
        let file = crate::compose::files::resolve(&root, &relative).expect("path");
        crate::compose::files::write_atomically(&file, message(&self.run, n).as_bytes())
            .expect("frozen bytes");
        let target = self.folder_id(self.target.account).await;
        let change = self
            .store
            .enqueue_transfer(
                target,
                mode,
                vec![(at, relative)],
                format!("{}-{n}", self.run),
                1,
            )
            .await
            .expect("recorded");
        assert_eq!(change.account, self.target.account);
        let op = change.operations[0].clone();
        assert_eq!(op.payload.kind(), OperationKind::Append);
        self.outbox.recorded_result(&change);
        op
    }

    /// A fresh outbox (new connector and tap) on the same database, as after a restart.
    async fn restart(&mut self) {
        self.outbox.shutdown().await;
        let store = Store::open(&self.path, StoreOptions::default()).expect("reopen");
        self.store = store.clone();
        self.tap = WireTap::new();
        self.outbox = Outbox::new(
            store,
            Arc::new(ImapConnector::new().with_wire_tap(self.tap.clone())),
            self.credentials.clone(),
            config(&self.env),
        );
    }

    async fn wait_state(
        &self,
        id: mail_model::OperationId,
        state: OperationState,
    ) -> StoredOperation {
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

    /// Waits until the source account has no pending operation; returns its operations.
    async fn source_settled(&self) -> Vec<StoredOperation> {
        let deadline = Instant::now() + WAIT;
        loop {
            let pending = self
                .store
                .pending_operations(self.source.account)
                .await
                .expect("pending");
            if pending.is_empty() {
                return self
                    .store
                    .operations(self.source.account, None, 100)
                    .await
                    .expect("operations");
            }
            assert!(
                Instant::now() < deadline,
                "source still pending: {pending:#?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Where message `n` lies: (alice's folders, bob's folders).
    async fn where_is(&mut self, n: usize) -> (Vec<String>, Vec<String>) {
        let id = format!("{}-{n}@{DOMAIN}", self.run);
        let mut out = Vec::new();
        for admin in [&mut self.source.admin, &mut self.target.admin] {
            let found = places(admin).await;
            out.push(
                found
                    .get(&id)
                    .map(|at| at.iter().map(|(f, _)| f.clone()).collect())
                    .unwrap_or_default(),
            );
        }
        let bob = out.pop().unwrap_or_default();
        let alice = out.pop().unwrap_or_default();
        (alice, bob)
    }

    async fn finish(mut self) {
        self.outbox.shutdown().await;
        for admin in [&mut self.source.admin, &mut self.target.admin] {
            clean(admin).await;
        }
        self.source.admin.logout().await.ok();
        self.target.admin.logout().await.ok();
    }
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_move_between_accounts_removes_the_source_only_after_the_copy_is_confirmed() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("move", 2).await;

    // Copy: the original stays, one copy in the target.
    let op = bed.transfer(1, TransferMode::Copy).await;
    bed.wait_state(op.id, OperationState::Confirmed).await;
    let (alice, bob) = bed.where_is(1).await;
    assert_eq!(
        alice,
        [bed.folder.clone()],
        "a copy leaves the source alone"
    );
    assert_eq!(bob, [bed.folder.clone()]);
    assert!(
        bed.source_settled().await.is_empty(),
        "nothing recorded in the source"
    );

    // Move: appended, then removed from the source.
    let op = bed.transfer(2, TransferMode::Move).await;
    bed.wait_state(op.id, OperationState::Confirmed).await;
    let source_ops = bed.source_settled().await;
    assert_eq!(source_ops.len(), 1, "{source_ops:#?}");
    assert_eq!(source_ops[0].payload.kind(), OperationKind::Expunge);
    assert_eq!(source_ops[0].state, OperationState::Confirmed);
    assert_eq!(
        source_ops[0].batch, op.batch,
        "one batch for the activity view"
    );
    let (alice, bob) = bed.where_is(2).await;
    assert!(alice.is_empty(), "removed from the source: {alice:?}");
    assert_eq!(bob, [bed.folder.clone()], "exactly one copy");
    bed.finish().await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_connection_cut_after_append_leaves_exactly_one_copy_and_the_source_until_confirmed() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("cut", 1).await;
    let proxy = Arc::clone(&bed.proxy);
    bed.tap.on_write("APPEND", move || {
        proxy.refuse_next(1_000_000);
        proxy.cut_responses(Duration::from_millis(300));
    });
    let op = bed.transfer(1, TransferMode::Move).await;
    // Sent, the answer lost: in flight, and the target cannot be reached.
    bed.wait_state(op.id, OperationState::InFlight).await;
    let deadline = Instant::now() + WAIT;
    while bed.proxy.cuts() == 0 {
        assert!(Instant::now() < deadline, "APPEND was never sent");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        bed.store
            .operation(op.id)
            .await
            .expect("read")
            .expect("op")
            .state,
        OperationState::InFlight
    );
    let (alice, bob) = bed.where_is(1).await;
    assert_eq!(alice, [bed.folder.clone()], "the source is not touched yet");
    assert_eq!(bob, [bed.folder.clone()], "the server got the APPEND");
    assert!(
        bed.store
            .operations(bed.source.account, None, 10)
            .await
            .expect("operations")
            .is_empty(),
        "no removal before the confirmation"
    );

    // Restart: the in-flight append is checked against the target (found by Message-ID above
    // the mark), not sent again; only then is the source removed.
    bed.restart().await;
    bed.proxy.refuse_next(0);
    bed.outbox.start().await.expect("start");
    let confirmed = bed.wait_state(op.id, OperationState::Confirmed).await;
    assert_eq!(confirmed.attempts, 1, "checked, not appended again");
    assert!(
        !bed.tap.verbs().contains(&"APPEND".to_owned()),
        "{:?}",
        bed.tap.verbs()
    );
    let source_ops = bed.source_settled().await;
    assert_eq!(source_ops.len(), 1, "{source_ops:#?}");
    assert_eq!(source_ops[0].state, OperationState::Confirmed);
    let (alice, bob) = bed.where_is(1).await;
    assert!(
        alice.is_empty(),
        "removed after the confirmation: {alice:?}"
    );
    assert_eq!(bob, [bed.folder.clone()], "never two copies");
    bed.finish().await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_target_that_refuses_fails_the_transfer_and_the_source_stays() {
    let _server = SERVER.lock().await;
    let mut bed = Bed::new("refused", 1).await;
    // The target folder disappears on the server before the outbox gets there.
    bed.proxy.refuse_next(1_000_000);
    let folder = bed.folder.clone();
    raw(&mut bed.target.admin, &format!("DELETE \"{folder}\"")).await;
    let op = bed.transfer(1, TransferMode::Move).await;
    bed.proxy.refuse_next(0);
    bed.outbox.wake(bed.target.account);
    let failed = bed.wait_state(op.id, OperationState::Failed).await;
    assert!(failed.last_error.is_some());
    assert!(
        bed.store
            .operations(bed.source.account, None, 10)
            .await
            .expect("operations")
            .is_empty(),
        "nothing removed"
    );
    let (alice, bob) = bed.where_is(1).await;
    assert_eq!(alice, [bed.folder.clone()], "never lost");
    assert!(bob.is_empty());
    bed.finish().await;
}
