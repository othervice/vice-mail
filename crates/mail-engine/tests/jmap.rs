//! Stage 14, block D (decision 0041) through `AppCore` against the local test server over JMAP
//! (Stalwart, HTTPS), without a WebView: the same scenarios as the IMAP server tests. A JMAP
//! account is set up, synced, gets new mail by push and changes made elsewhere; actions go
//! through the outbox and reach the server; a read-only account refuses them before anything
//! is sent; a message changed or deleted elsewhere ends as a conflict; sending goes through JMAP
//! submission; a restart keeps the UIDs. Every step is checked over IMAP on the same mailbox.
//!
//! Ignored by default. Take the shared lock, start the server (any seed):
//!
//! ```text
//! scripts/testmail.sh lock jmap && scripts/testmail.sh start
//! cargo test -p mail-engine --test jmap -- --include-ignored --test-threads=1
//! scripts/testmail.sh unlock jmap
//! ```
//!
//! Works in bob's account (alice sends) with messages `<...@jmap-engine.test>` it appends
//! itself; they are removed before and after.

use std::sync::Arc;
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, OutboxConfig, SyncConfig};
use mail_imap::test_support::{TestEnv, send_mail};
use mail_imap::{ImapBackend, ImapConnector, ImapSyncFolder};
use mail_jmap::{JmapBackend, JmapConnector, JmapWriter, MethodLog};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, Address, BackendError, ComposeFormat,
    ComposeSettings, ConnectConfig, CoreEvent, DraftUpdate, ErrorCode, IpcError, MailAction,
    MailBackend, MailConnector, MailboxId, MessageListItem, MessageRef, NewDraft, OperationState,
    Protocol, QuotePosition, Security, SendState, ServerSettings, SyncFolderConnector, UidSet,
    WriteConnector,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(40);
const DOMAIN: &str = "jmap-engine.test";

/// JMAP for everything, with one method log for the read connections (sync, push, reading)
/// and one for the outbox. Both share one UID map, as in the product.
struct Split {
    read: JmapConnector,
    write: JmapConnector,
}

impl MailConnector for Split {
    type Backend = JmapBackend;

    async fn connect(&self, config: &ConnectConfig) -> Result<JmapBackend, BackendError> {
        assert_eq!(
            config.protocol,
            Protocol::Jmap,
            "the account's protocol travels along"
        );
        self.read.connect(config).await
    }

    fn attach_uid_map_store(&self, store: Arc<dyn mail_model::UidMapStore>) {
        self.read.attach_uid_map_store(store);
    }
}

impl WriteConnector for Split {
    type Writer = JmapWriter;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<JmapWriter, BackendError> {
        self.write.connect_writer(config).await
    }
}

impl SyncFolderConnector for Split {
    type Folder = ImapSyncFolder;

    async fn connect_sync_folder(
        &self,
        _config: &ConnectConfig,
        _folder: &str,
    ) -> Result<ImapSyncFolder, BackendError> {
        Err(BackendError::Unsupported("settings sync over JMAP".into()))
    }
}

fn run() -> String {
    format!("je{}", std::process::id())
}

async fn imap(env: &TestEnv) -> ImapBackend {
    ImapConnector::new()
        .connect(&env.config(&env.bob, Security::Tls, Access::ReadWrite))
        .await
        .expect("IMAP session")
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
        "From: JMAP Engine <writer@{DOMAIN}>\r\nTo: bob@vicemail.test\r\n\
         Subject: {run} message {n}\r\nDate: Mon, 23 Nov 2026 11:{:02}:00 +0000\r\n\
         Message-ID: <{run}-{n}@{DOMAIN}>\r\n\r\nBody {n}.\r\n",
        n % 60
    );
    raw(
        admin,
        &format!("APPEND \"{folder}\" () {{{}+}}\r\n{body}", body.len()),
    )
    .await;
}

/// Where the message lies over IMAP: (folder, flagged).
async fn places(admin: &mut ImapBackend, id: &str) -> Vec<(String, bool)> {
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
                found.push((remote.name.clone(), header.flags.flagged));
            }
        }
    }
    found
}

/// IMAP UID of the message in `folder`.
async fn imap_uid(admin: &mut ImapBackend, folder: &str, id: &str) -> Option<u32> {
    let opened = admin.open_mailbox(folder).await.expect("select");
    let uids = admin.search_uids(&opened).await.expect("search");
    if uids.is_empty() {
        return None;
    }
    admin
        .fetch_headers(&opened, &uids)
        .await
        .expect("fetch")
        .into_iter()
        .find(|h| h.message_id.as_deref() == Some(id))
        .map(|h| h.uid)
}

async fn clean(admin: &mut ImapBackend) {
    for remote in admin.list_mailboxes().await.expect("list") {
        if !remote.is_selectable() {
            continue;
        }
        let opened = admin.open_mailbox(&remote.name).await.expect("select");
        let uids = admin.search_uids(&opened).await.expect("search");
        if uids.is_empty() {
            continue;
        }
        let mine: Vec<u32> = admin
            .fetch_headers(&opened, &uids)
            .await
            .expect("fetch")
            .into_iter()
            .filter(|h| {
                h.message_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with(&format!("@{DOMAIN}")))
                    || h.subject.starts_with("je")
            })
            .map(|h| h.uid)
            .collect();
        if !mine.is_empty() {
            let set = UidSet::from_uids(mine);
            raw(admin, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")).await;
            raw(admin, &format!("UID EXPUNGE {set}")).await;
        }
    }
}

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        // Long: new mail within the wait can only come by push (or the outbox's hook).
        poll_interval: Duration::from_secs(600),
        extra_root_ca: Some(env.extra_root()),
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

struct Engine {
    core: AppCore<Split>,
    events: broadcast::Receiver<CoreEvent>,
    account: AccountId,
}

impl Engine {
    async fn start(
        env: &TestEnv,
        dir: &TempDir,
        keychain: &Arc<MemorySecretStore>,
        read: &MethodLog,
        write: &MethodLog,
    ) -> AppCore<Split> {
        let base = JmapConnector::new(Arc::new(mail_imap::tls::client_config));
        let connector = Split {
            read: base.clone().with_method_log(read.clone()),
            write: base.with_method_log(write.clone()),
        };
        let mut services =
            CoreServices::with_connector(Credentials::new(Arc::clone(keychain)), connector);
        services.sync = sync_config(env);
        let mut outbox = OutboxConfig::from_sync(&services.sync);
        outbox.linger = Duration::from_millis(500);
        services.outbox = Some(outbox);
        AppCore::start(
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
        .await
    }

    async fn event(&mut self, what: &str, pred: impl Fn(&CoreEvent) -> bool) -> CoreEvent {
        let deadline = Instant::now() + WAIT;
        loop {
            match tokio::time::timeout_at(deadline, self.events.recv()).await {
                Ok(Ok(event)) if pred(&event) => return event,
                Ok(Ok(CoreEvent::AccountStatusChanged { status, .. })) => {
                    eprintln!("status while waiting for {what}: {status:?}");
                }
                Ok(Ok(_)) => {}
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(err)) => panic!("event stream: {err}"),
                Err(_) => panic!("{what}: no matching event within {WAIT:?}"),
            }
        }
    }

    async fn settled(&mut self, batch: mail_model::BatchId) -> OperationState {
        let event = self
            .event("operation settled", |e| match e {
                CoreEvent::OperationsChanged { operations, .. } => operations.iter().any(|o| {
                    o.batch == batch
                        && matches!(
                            o.state,
                            OperationState::Confirmed
                                | OperationState::Conflict
                                | OperationState::Failed
                        )
                }),
                _ => false,
            })
            .await;
        match event {
            CoreEvent::OperationsChanged { operations, .. } => operations
                .into_iter()
                .find(|o| o.batch == batch)
                .map(|o| o.state)
                .expect("the operation"),
            _ => unreachable!("matched above"),
        }
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

    async fn find(&self, path: &str, subject: &str) -> Option<MessageListItem> {
        let mailbox = self.mailbox(path).await;
        let mut cursor = None;
        loop {
            let page = self
                .core
                .list_messages(mailbox, cursor, 500)
                .await
                .expect("page");
            if let Some(found) = page.items.into_iter().find(|i| i.subject == subject) {
                return Some(found);
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return None,
            }
        }
    }

    /// Polls until the message is in the folder's list and `pred` holds for it.
    async fn until(
        &self,
        path: &str,
        subject: &str,
        pred: impl Fn(&MessageListItem) -> bool,
    ) -> MessageListItem {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(item) = self.find(path, subject).await
                && pred(&item)
            {
                return item;
            }
            assert!(Instant::now() < deadline, "{subject} in {path} as expected");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn until_gone(&self, path: &str, subject: &str) {
        let deadline = Instant::now() + WAIT;
        while self.find(path, subject).await.is_some() {
            assert!(Instant::now() < deadline, "{subject} gone from {path}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn at(mailbox: MailboxId, item: &MessageListItem) -> MessageRef {
    MessageRef {
        mailbox_id: mailbox,
        message_id: item.id,
    }
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_jmap_account_syncs_acts_sends_and_keeps_its_numbers() {
    let env = TestEnv::require();
    let run = run();
    let subject = |n: usize| format!("{run} message {n}");
    let id = |n: usize| format!("{run}-{n}@{DOMAIN}");
    let mut admin = imap(&env).await;
    clean(&mut admin).await;
    for n in 1..=4 {
        append(&mut admin, "INBOX", &run, n).await;
    }

    let dir = TempDir::new().expect("temp dir");
    let read_log = MethodLog::new();
    let write_log = MethodLog::new();
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let core = Engine::start(&env, &dir, &keychain, &read_log, &write_log).await;
    let events = core.subscribe();
    let jmap_server = ServerSettings {
        host: env.tls_name.clone(),
        port: env.https_port,
        security: Security::Tls,
        username: env.bob.address.clone(),
    };
    let settings = AccountSettings {
        credential: Default::default(),
        email: env.bob.address.clone(),
        display_name: "Bob".into(),
        imap: jmap_server.clone(),
        // Replaced by the JMAP server.
        smtp: ServerSettings {
            port: 1,
            ..jmap_server.clone()
        },
        protocol: Protocol::Jmap,
    };
    let tested = core
        .test_connection(settings.clone(), env.bob.password.clone())
        .await
        .expect("test connection");
    assert!(tested.capabilities.jmap && tested.capabilities.idle);
    let added = core
        .add_account(settings, env.bob.password.clone())
        .await
        .expect("add bob over JMAP");
    assert_eq!(added.protocol, Protocol::Jmap);
    assert_eq!(added.smtp, jmap_server, "one server for everything");
    assert!(!added.writes_enabled, "read-only at first");
    let mut engine = Engine {
        core,
        events,
        account: added.id,
    };
    engine
        .event("idle", |e| {
            matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
        })
        .await;

    // 1. The first sync over JMAP lists the messages with their folders.
    let inbox = engine.mailbox("INBOX").await;
    for n in 1..=4 {
        engine.until("INBOX", &subject(n), |_| true).await;
    }
    for path in ["Archive", "Sent", "Drafts", "Trash", "Junk"] {
        engine.mailbox(path).await;
    }

    // 2. Read-only: refused through the core, nothing written.
    let first = engine.find("INBOX", &subject(1)).await.expect("1");
    let err = engine
        .core
        .apply_action(MailAction::Flag, vec![at(inbox, &first)], "ro".into())
        .await
        .expect_err("read-only");
    assert_eq!(IpcError::from(&err).code, ErrorCode::ReadOnly, "{err}");
    assert!(
        write_log.entries().is_empty(),
        "no writer for a read-only account"
    );

    // 3. New mail by push (the poll interval is ten minutes).
    send_mail(&env, &env.alice, &env.bob.address, &subject(5), "Pushed.")
        .await
        .expect("SMTP");
    engine
        .event(
            "newMail",
            |e| matches!(e, CoreEvent::NewMail { mailbox_id, .. } if *mailbox_id == inbox),
        )
        .await;
    engine.until("INBOX", &subject(5), |i| i.unread).await;

    // 4. A change elsewhere (IMAP) arrives by push too.
    let uid4 = imap_uid(&mut admin, "INBOX", &id(4))
        .await
        .expect("4 over IMAP");
    raw(
        &mut admin,
        &format!("UID STORE {uid4} +FLAGS.SILENT (\\Seen)"),
    )
    .await;
    engine.until("INBOX", &subject(4), |i| !i.unread).await;

    // 5. Writes on: flag, archive, delete go through the outbox and reach the server.
    engine
        .core
        .set_writes_enabled(engine.account, true)
        .await
        .expect("writes on");
    let flagged = engine
        .core
        .apply_action(MailAction::Flag, vec![at(inbox, &first)], "flag".into())
        .await
        .expect("flag");
    assert_eq!(
        engine.settled(flagged.batch.expect("batch")).await,
        OperationState::Confirmed
    );
    assert_eq!(
        places(&mut admin, &id(1)).await,
        [("INBOX".to_owned(), true)]
    );
    engine
        .until("INBOX", &subject(1), |i| i.flagged && i.activity.is_none())
        .await;

    let second = engine.find("INBOX", &subject(2)).await.expect("2");
    let archived = engine
        .core
        .apply_action(
            MailAction::Archive,
            vec![at(inbox, &second)],
            "archive".into(),
        )
        .await
        .expect("archive");
    assert_eq!(
        engine.settled(archived.batch.expect("batch")).await,
        OperationState::Confirmed
    );
    assert_eq!(
        places(&mut admin, &id(2)).await,
        [("Archive".to_owned(), false)]
    );
    engine.until("Archive", &subject(2), |_| true).await;
    engine.until_gone("INBOX", &subject(2)).await;

    let third = engine.find("INBOX", &subject(3)).await.expect("3");
    let deleted = engine
        .core
        .apply_action(MailAction::Delete, vec![at(inbox, &third)], "delete".into())
        .await
        .expect("delete");
    assert_eq!(
        engine.settled(deleted.batch.expect("batch")).await,
        OperationState::Confirmed
    );
    assert_eq!(
        places(&mut admin, &id(3)).await,
        [("Trash".to_owned(), false)]
    );

    // 6. Conflict: deleted elsewhere before the outbox gets to it; the server wins.
    let fourth = engine.find("INBOX", &subject(4)).await.expect("4");
    let uid4 = imap_uid(&mut admin, "INBOX", &id(4))
        .await
        .expect("4 over IMAP");
    raw(
        &mut admin,
        &format!("UID STORE {uid4} +FLAGS.SILENT (\\Deleted)"),
    )
    .await;
    raw(&mut admin, &format!("UID EXPUNGE {uid4}")).await;
    let gone = engine
        .core
        .apply_action(
            MailAction::Flag,
            vec![at(inbox, &fourth)],
            "conflict".into(),
        )
        .await
        .expect("flag");
    assert_eq!(
        engine.settled(gone.batch.expect("batch")).await,
        OperationState::Conflict
    );
    engine.until_gone("INBOX", &subject(4)).await;

    // 7. Sending: JMAP submission, the app's copy in Sent, received over IMAP.
    engine
        .core
        .set_compose_settings(ComposeSettings {
            send_delay_seconds: 0,
            ..ComposeSettings::default()
        })
        .await
        .expect("no delay");
    let draft = engine
        .core
        .create_draft(NewDraft::New {
            account_id: engine.account,
            identity_id: None,
            to: Vec::new(),
        })
        .await
        .expect("draft");
    let sent_subject = format!("{run} sent over JMAP");
    let draft = engine
        .core
        .save_draft(
            draft.id.clone(),
            DraftUpdate {
                identity_id: draft.identity_id,
                to: vec![Address {
                    name: Some("Bob".into()),
                    email: env.bob.address.clone(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: sent_subject.clone(),
                format: ComposeFormat::Html,
                body_html: "<p>Über JMAP gesendet.</p>".into(),
                body_text: String::new(),
                include_quote: false,
                quote_position: QuotePosition::Above,
                include_signature: false,
                signature_id: None,
            },
        )
        .await
        .expect("save");
    let outgoing = engine
        .core
        .send_draft(draft.id.clone(), format!("{run}-send"))
        .await
        .expect("send");
    let deadline = Instant::now() + WAIT;
    loop {
        let list = engine
            .core
            .list_outgoing(engine.account, None, 50)
            .await
            .expect("outgoing");
        if list
            .iter()
            .any(|m| m.operation_id == outgoing.operation_id && m.state == SendState::Sent)
        {
            break;
        }
        assert!(Instant::now() < deadline, "sent: {list:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    engine.until("INBOX", &sent_subject, |_| true).await;
    engine.until("Sent", &sent_subject, |_| true).await;

    // 8. The read connections only read; the outbox wrote.
    let read = read_log.entries();
    assert!(
        read.iter().any(|m| m == "Email/changes"),
        "positive control"
    );
    assert!(read.iter().any(|m| m == "eventsource"), "push used");
    for method in &read {
        assert!(
            [
                "Mailbox/get",
                "Email/get",
                "Email/query",
                "Email/changes",
                "Core/echo",
                "download",
                "eventsource"
            ]
            .contains(&method.as_str()),
            "{method} on a read connection"
        );
    }
    let written = write_log.entries();
    assert!(written.iter().any(|m| m == "Email/set"), "{written:?}");
    assert!(
        written.iter().any(|m| m == "Email/import"),
        "the Sent copy: {written:?}"
    );

    // 9. A restart keeps the numbers: nothing is listed again.
    let before = engine.find("INBOX", &subject(1)).await.expect("1");
    engine.core.shutdown().await;
    drop(engine);
    let queries = read_log
        .entries()
        .iter()
        .filter(|m| *m == "Email/query")
        .count();
    let core = Engine::start(&env, &dir, &keychain, &read_log, &write_log).await;
    let events = core.subscribe();
    let engine = Engine {
        core,
        events,
        account: added.id,
    };
    // The account may be idle before this test subscribes: ask for its status.
    let deadline = Instant::now() + WAIT;
    loop {
        let accounts = engine.core.list_accounts().await.expect("accounts");
        if accounts
            .iter()
            .any(|a| a.id == engine.account && a.status.state == AccountState::Idle)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "idle after the restart: {accounts:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let after = engine
        .find("INBOX", &subject(1))
        .await
        .expect("still listed");
    assert_eq!(after.id, before.id, "the same message row");
    assert_eq!(
        read_log
            .entries()
            .iter()
            .filter(|m| *m == "Email/query")
            .count(),
        queries,
        "no mailbox listed again after the restart"
    );
    engine.core.shutdown().await;
    clean(&mut admin).await;
    admin.logout().await.ok();
}
