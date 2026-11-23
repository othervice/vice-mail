//! Stage 5 through `AppCore` against the local test server (Stalwart), without a WebView:
//! writing, sending, receiving, answering in the same thread, archiving; attachments and
//! Unicode; blind copies; the undo delay; an unclear send after a cut connection; drafts that
//! survive quitting and a crash, with only the newest version in the Drafts folder.
//!
//! Ignored by default. Take the shared lock, start a fresh server (no seed needed):
//!
//! ```text
//! scripts/testmail.sh lock compose && scripts/testmail.sh reset && scripts/testmail.sh start
//! cargo test -p mail-engine --test compose -- --include-ignored --test-threads=1
//! scripts/testmail.sh stop && scripts/testmail.sh unlock compose
//! ```
//!
//! bob and alice send to each other (admin receives blind copies). Every message has a fresh
//! Message-ID and a subject starting with `VMC `; the tests remove those messages from all
//! folders afterwards. Stalwart silently drops a second delivery with the same Message-ID
//! (decision 0001), so "never two" is proved by counting SMTP connections and end-of-data
//! markers at a proxy and a wire tap, not only by the recipient's mailbox.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mail_engine::credentials::{CredentialRef, Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{
    AppCore, BuildInfo, ComposeConfig, CoreConfig, CoreServices, OutboxConfig, SenderHandle,
    SyncConfig,
};
use mail_imap::test_support::{Proxy, TestAccount, TestEnv};
use mail_imap::{ImapBackend, ImapConnector, SmtpSender, WireTap};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, Address, ComposeFormat, ComposeSettings,
    CoreEvent, Draft, DraftServerState, DraftState, DraftUpdate, ErrorCode, MailAction,
    MailBackend, MailConnector, MailboxId, MailboxRole, MessageListItem, MessageRef, NewDraft,
    OperationState, QuoteLanguage, QuotePosition, Security, SendState, SentCopyMode,
    ServerSettings, Uid, UidSet, UnclearDecision,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(45);
const MARK: &str = "VMC ";

/// The tests share bob's and alice's accounts: one at a time.
static SERVER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        poll_interval: Duration::from_secs(2),
        extra_root_ca: Some(env.extra_root()),
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

fn outbox_config(env: &TestEnv, sender: SmtpSender) -> OutboxConfig {
    let mut config = OutboxConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        extra_root_ca: Some(env.extra_root()),
        linger: Duration::from_millis(500),
        sender: SenderHandle(Arc::new(sender)),
        compose: ComposeConfig {
            draft_save_delay: Duration::from_millis(400),
            sent_checks: vec![Duration::from_millis(300); 3],
            contacts_delay: Duration::from_millis(50),
        },
        ..OutboxConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

/// A run id for subjects.
fn run_id() -> String {
    format!(
        "{:x}{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or_default()
    )
}

async fn until<T, F, Fut>(what: &str, mut probe: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(value) = probe().await {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct Bed {
    env: TestEnv,
    dir: TempDir,
    keychain: Arc<MemorySecretStore>,
    core: AppCore<ImapConnector>,
    events: broadcast::Receiver<CoreEvent>,
    bob: AccountId,
    alice: AccountId,
}

async fn start_core(
    env: &TestEnv,
    data: &Path,
    keychain: &Arc<MemorySecretStore>,
    sender: SmtpSender,
) -> AppCore<ImapConnector> {
    let mut services =
        CoreServices::with_connector(Credentials::new(Arc::clone(keychain)), ImapConnector::new());
    services.sync = sync_config(env);
    services.outbox = Some(outbox_config(env, sender));
    AppCore::start(
        CoreConfig {
            data_dir: data.to_owned(),
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

fn settings(
    env: &TestEnv,
    account: &TestAccount,
    name: &str,
    smtp: (String, u16, Security),
) -> AccountSettings {
    let server = |host: String, port, security| ServerSettings {
        host,
        port,
        security,
        username: account.address.clone(),
    };
    AccountSettings {
        protocol: Default::default(),
        email: account.address.clone(),
        display_name: name.to_owned(),
        imap: server(env.host.clone(), env.imaps_port, Security::Tls),
        smtp: server(smtp.0, smtp.1, smtp.2),
    }
}

impl Bed {
    /// A core with bob and alice, writes on for both, both synced once. `bob_smtp` overrides
    /// bob's SMTP server (a proxy).
    async fn new(sender: SmtpSender, bob_smtp: Option<(String, u16, Security)>) -> Self {
        let env = TestEnv::require();
        clean_server(&env).await;
        let dir = TempDir::new().expect("temp dir");
        let keychain: Arc<MemorySecretStore> = Arc::default();
        let core = start_core(&env, &dir.path().join("data"), &keychain, sender).await;
        let mut events = core.subscribe();
        let submission = (env.host.clone(), env.submission_port, Security::Starttls);
        let bob = core
            .add_account(
                settings(
                    &env,
                    &env.bob,
                    "Bob Bäcker 🍞",
                    bob_smtp.unwrap_or_else(|| submission.clone()),
                ),
                env.bob.password.clone(),
            )
            .await
            .expect("add bob")
            .id;
        let alice = core
            .add_account(
                settings(&env, &env.alice, "Alice Ärger", submission),
                env.alice.password.clone(),
            )
            .await
            .expect("add alice")
            .id;
        for account in [bob, alice] {
            core.set_writes_enabled(account, true)
                .await
                .expect("writes on");
        }
        let mut idle = std::collections::BTreeSet::new();
        let deadline = Instant::now() + WAIT;
        while idle.len() < 2 {
            match tokio::time::timeout_at(deadline, events.recv()).await {
                Ok(Ok(CoreEvent::AccountStatusChanged { account_id, status }))
                    if status.state == AccountState::Idle =>
                {
                    idle.insert(account_id);
                }
                Ok(Ok(_)) => {}
                Ok(Err(err)) => panic!("events: {err}"),
                Err(_) => panic!("accounts did not sync"),
            }
        }
        core.set_compose_settings(ComposeSettings {
            send_delay_seconds: 0,
            ..ComposeSettings::default()
        })
        .await
        .expect("no delay");
        Self {
            env,
            dir,
            keychain,
            core,
            events,
            bob,
            alice,
        }
    }

    async fn mailbox(&self, account: AccountId, role: MailboxRole) -> MailboxId {
        self.core
            .list_mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == role)
            .unwrap_or_else(|| panic!("no {role:?} mailbox"))
            .id
    }

    async fn find(
        &self,
        account: AccountId,
        role: MailboxRole,
        subject: &str,
    ) -> Option<MessageListItem> {
        let mailbox = self.mailbox(account, role).await;
        self.core
            .list_messages(mailbox, None, 500)
            .await
            .expect("list")
            .items
            .into_iter()
            .find(|i| i.subject == subject)
    }

    async fn arrived(
        &self,
        account: AccountId,
        role: MailboxRole,
        subject: &str,
    ) -> MessageListItem {
        until(&format!("{subject:?} in {role:?}"), || async {
            self.find(account, role, subject).await
        })
        .await
    }

    async fn new_draft(&self, account: AccountId) -> Draft {
        self.core
            .create_draft(NewDraft::New {
                account_id: account,
                identity_id: None,
                to: Vec::new(),
            })
            .await
            .expect("new draft")
    }

    async fn write(
        &self,
        draft: &Draft,
        to: Vec<Address>,
        bcc: Vec<Address>,
        subject: &str,
        html: &str,
    ) -> Draft {
        self.core
            .save_draft(
                draft.id.clone(),
                DraftUpdate {
                    identity_id: draft.identity_id,
                    to,
                    cc: Vec::new(),
                    bcc,
                    subject: subject.to_owned(),
                    format: ComposeFormat::Html,
                    body_html: html.to_owned(),
                    body_text: String::new(),
                    include_quote: true,
                    quote_position: QuotePosition::Above,
                    include_signature: true,
                    signature_id: None,
                },
            )
            .await
            .expect("save draft")
    }

    async fn sent(
        &self,
        account: AccountId,
        operation: mail_model::OperationId,
    ) -> mail_model::OutgoingMail {
        until("the send to finish", || async {
            self.core
                .list_outgoing(account, None, 50)
                .await
                .expect("outgoing")
                .into_iter()
                .find(|m| m.operation_id == operation && m.state == SendState::Sent)
        })
        .await
    }

    /// Waits until the account has no pending operations (follow-ups done).
    async fn settled(&self, account: AccountId) {
        until("the outbox to settle", || async {
            let counts = self.core.outbox_counts(account).await.expect("counts");
            (counts.pending == 0).then_some(())
        })
        .await;
    }
}

fn person(name: &str, email: &str) -> Address {
    Address {
        name: Some(name.to_owned()),
        email: email.to_owned(),
    }
}

async fn admin_session(env: &TestEnv, account: &TestAccount) -> ImapBackend {
    ImapConnector::new()
        .connect(&env.config(account, Security::Tls, Access::ReadWrite))
        .await
        .expect("admin session")
}

/// Raw copies of messages with `message_id` in `folder` of `account`.
async fn copies(
    env: &TestEnv,
    account: &TestAccount,
    folder: &str,
    message_id: &str,
) -> Vec<(Uid, Vec<u8>)> {
    let mut imap = admin_session(env, account).await;
    let opened = imap.open_mailbox(folder).await.expect("open");
    let uids = imap.search_uids(&opened).await.expect("search");
    let mut found = Vec::new();
    if !uids.is_empty() {
        let headers = imap.fetch_headers(&opened, &uids).await.expect("headers");
        let matching = UidSet::from_uids(
            headers
                .iter()
                .filter(|h| h.message_id.as_deref() == Some(message_id))
                .map(|h| h.uid),
        );
        if !matching.is_empty() {
            for raw in imap
                .fetch_messages(&opened, &matching)
                .await
                .expect("fetch")
            {
                found.push((raw.uid, raw.data));
            }
        }
    }
    imap.logout().await.ok();
    found
}

/// Removes every message with a `VMC ` subject from all folders of bob, alice and admin.
async fn clean_server(env: &TestEnv) {
    for account in [&env.bob, &env.alice, &env.admin] {
        let mut imap = admin_session(env, account).await;
        for remote in imap.list_mailboxes().await.expect("list") {
            if !remote.is_selectable() {
                continue;
            }
            let opened = imap.open_mailbox(&remote.name).await.expect("select");
            let uids = imap.search_uids(&opened).await.expect("search");
            if uids.is_empty() {
                continue;
            }
            let ours: Vec<Uid> = imap
                .fetch_headers(&opened, &uids)
                .await
                .expect("headers")
                .iter()
                .filter(|h| h.subject.starts_with(MARK) || h.subject.contains(&format!(": {MARK}")))
                .map(|h| h.uid)
                .collect();
            if ours.is_empty() {
                continue;
            }
            let set = UidSet::from_uids(ours);
            for line in [
                format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)"),
                format!("UID EXPUNGE {set}"),
            ] {
                let reply = imap.raw_command(&line, |_| false).await.expect(&line);
                assert!(reply.ok, "{line}: {}", reply.text);
            }
        }
        imap.logout().await.ok();
    }
}

/// The first message with `subject` in `folder` of `account`, raw.
async fn server_message(
    env: &TestEnv,
    account: &TestAccount,
    folder: &str,
    subject: &str,
) -> Option<(Uid, Vec<u8>)> {
    let mut imap = admin_session(env, account).await;
    let opened = imap.open_mailbox(folder).await.ok()?;
    let uids = imap.search_uids(&opened).await.ok()?;
    if uids.is_empty() {
        imap.logout().await.ok();
        return None;
    }
    let uid = imap
        .fetch_headers(&opened, &uids)
        .await
        .ok()?
        .into_iter()
        .find(|h| h.subject == subject)?
        .uid;
    let raw = imap
        .fetch_messages(&opened, &UidSet::from_uids([uid]))
        .await
        .ok()?
        .into_iter()
        .next()?
        .data;
    imap.logout().await.ok();
    Some((uid, raw))
}

fn header_block(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.split("\r\n\r\n").next().unwrap_or_default().to_owned()
}

fn message_id_of(raw: &[u8]) -> String {
    mail_content::parse(raw)
        .headers
        .message_id
        .expect("message id")
}

/// Schreiben, senden, beim zweiten Konto ankommen, dort antworten, Antwort im selben Thread,
/// archivieren. Also: `\Answered` on the original, the Sent copies, contacts for autocomplete.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_message_goes_out_is_answered_in_the_same_thread_and_archived() {
    let _serial = SERVER.lock().await;
    let bed = Bed::new(SmtpSender::new(), None).await;
    let env = &bed.env;
    let subject = format!("{MARK}Durchgehend {}", run_id());

    // bob writes to alice and sends.
    let draft = bed.new_draft(bed.bob).await;
    assert_eq!(draft.state, DraftState::Editing);
    let draft = bed
        .write(
            &draft,
            vec![
                person("Alice Ärger", &env.alice.address),
                person("Petra Verwaltung", &env.admin.address),
            ],
            Vec::new(),
            &subject,
            "<p>Hallo Alice, <strong>schöne</strong> Grüße 👋</p>",
        )
        .await;
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("e2e-1"))
        .await
        .expect("send");
    let sent = bed.sent(bed.bob, outgoing.operation_id).await;
    assert!(sent.draft_id.is_none(), "{sent:?}");
    bed.settled(bed.bob).await;
    assert!(
        bed.core.get_draft(draft.id.clone()).await.is_err(),
        "the sent draft is gone"
    );

    // It arrives at alice; bob's Sent folder has the app's copy.
    let original = bed.arrived(bed.alice, MailboxRole::Inbox, &subject).await;
    let bob_sent = bed.arrived(bed.bob, MailboxRole::Sent, &subject).await;

    // alice answers: identity, recipient, subject, References, quote.
    let reply = bed
        .core
        .create_draft(NewDraft::Reply {
            message_id: original.id,
            language: QuoteLanguage::De,
        })
        .await
        .expect("reply");
    assert_eq!(reply.to.len(), 1);
    assert_eq!(reply.to[0].email, env.bob.address);
    assert_eq!(reply.subject, format!("Re: {subject}"));
    let quote = reply.quote_html.clone().expect("quote");
    assert!(quote.contains("schrieb"), "{quote}");
    assert!(quote.contains("Grüße"), "{quote}");
    let identities = bed
        .core
        .list_compose_identities(bed.alice)
        .await
        .expect("identities");
    assert_eq!(
        reply.identity_id,
        Some(identities[0].id),
        "the identity the mail went to"
    );
    let reply = bed
        .write(
            &reply,
            reply.to.clone(),
            Vec::new(),
            &reply.subject.clone(),
            "<p>Danke, Bob!</p>",
        )
        .await;
    let answer = bed
        .core
        .send_draft(reply.id.clone(), uuid_like("e2e-2"))
        .await
        .expect("send reply");
    bed.sent(bed.alice, answer.operation_id).await;
    bed.settled(bed.alice).await;

    // The original is answered at alice's (after the outbox's own sync of the INBOX).
    until("\\Answered on the original", || async {
        bed.find(bed.alice, MailboxRole::Inbox, &subject)
            .await
            .filter(|m| m.answered)
    })
    .await;

    // bob gets the answer, in the thread of his sent message.
    let answer_row = bed
        .arrived(bed.bob, MailboxRole::Inbox, &format!("Re: {subject}"))
        .await;
    let thread = until("the answer in bob's thread", || async {
        let view = bed.core.get_thread(answer_row.id, None).await.ok()?;
        (view.messages.len() == 2).then_some(view)
    })
    .await;
    let ids: Vec<_> = thread.messages.iter().map(|e| e.message.id).collect();
    assert!(
        ids.contains(&bob_sent.id) && ids.contains(&answer_row.id),
        "{ids:?}"
    );
    let inbox = bed.mailbox(bed.bob, MailboxRole::Inbox).await;
    // The answer names bob's message in In-Reply-To and References.
    let (_, sent_raw) = server_message(env, &env.bob, "Sent", &subject)
        .await
        .expect("bob's Sent copy");
    let original_msgid = message_id_of(&sent_raw);
    assert_eq!(
        copies(env, &env.bob, "Sent", &original_msgid).await.len(),
        1,
        "one Sent copy"
    );
    let (_, answer_raw) = server_message(env, &env.bob, "INBOX", &format!("Re: {subject}"))
        .await
        .expect("the answer on the server");
    let answer_headers = mail_content::parse(&answer_raw).headers;
    assert_eq!(answer_headers.in_reply_to, vec![original_msgid.clone()]);
    assert!(answer_headers.references.contains(&original_msgid));

    // bob archives the answer.
    let archived = bed
        .core
        .apply_action(
            MailAction::Archive,
            vec![MessageRef {
                mailbox_id: inbox,
                message_id: answer_row.id,
            }],
            uuid_like("e2e-archive"),
        )
        .await
        .expect("archive");
    assert_eq!(archived.queued, 1);
    bed.arrived(bed.bob, MailboxRole::Archive, &format!("Re: {subject}"))
        .await;
    assert!(
        bed.find(bed.bob, MailboxRole::Inbox, &format!("Re: {subject}"))
            .await
            .is_none()
    );

    // Autocomplete knows the other recipient from bob's sent mail (alice and bob are own
    // addresses of this app and never contacts).
    let suggestions = until("the recipient in the contacts", || async {
        let found = bed
            .core
            .search_contacts("petra verw".into(), 10)
            .await
            .expect("contacts");
        found
            .iter()
            .any(|c| c.email == env.admin.address && c.sent >= 1)
            .then_some(found)
    })
    .await;
    assert_eq!(
        suggestions[0].name.as_deref(),
        Some("Petra Verwaltung"),
        "{suggestions:?}"
    );
    assert!(
        bed.core
            .search_contacts("ali".into(), 10)
            .await
            .expect("contacts")
            .iter()
            .all(|c| c.email != env.alice.address),
        "own addresses are no contacts"
    );

    bed.core.shutdown().await;
    clean_server(env).await;
}

/// A UUID-shaped request id (letters, digits and dashes are what the core accepts).
fn uuid_like(tag: &str) -> String {
    format!("{tag}-{}", run_id())
}

/// Anhang mit gleicher Prüfsumme; Umlaute und Emoji in Betreff, Namen und Dateinamen.
#[tokio::test]
#[ignore = "needs the test server"]
async fn attachments_keep_their_checksum_and_unicode_survives() {
    let _serial = SERVER.lock().await;
    let bed = Bed::new(SmtpSender::new(), None).await;
    let env = &bed.env;
    let subject = format!("{MARK}Grüße 🎉 Übermorgen {}", run_id());
    let files = TempDir::new().expect("files");
    let name = "Prüfbericht 🧾 Ärger.pdf";
    let path = files.path().join(name);
    let bytes: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    std::fs::write(&path, &bytes).expect("write attachment");
    let expected = sha256(&bytes);

    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice Ärger 🌸", &env.alice.address)],
            Vec::new(),
            &subject,
            "<p>Anbei der Bericht. Schöne Grüße 😀</p>",
        )
        .await;
    let draft = bed
        .core
        .add_draft_attachments(draft.id.clone(), vec![path.clone()])
        .await
        .expect("attach");
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].file_name, name);
    assert_eq!(draft.attachments[0].size, 300_000);
    // The core keeps its own copy: the original may go.
    std::fs::remove_file(&path).expect("remove original");
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("att"))
        .await
        .expect("send");
    bed.sent(bed.bob, outgoing.operation_id).await;
    bed.arrived(bed.alice, MailboxRole::Inbox, &subject).await;

    let mut imap = admin_session(env, &env.alice).await;
    let opened = imap.open_mailbox("INBOX").await.expect("inbox");
    let uids = imap.search_uids(&opened).await.expect("uids");
    let uid = imap
        .fetch_headers(&opened, &uids)
        .await
        .expect("headers")
        .into_iter()
        .find(|h| h.subject == subject)
        .map(|h| h.uid)
        .expect("on the server");
    let raw = imap
        .fetch_messages(&opened, &UidSet::from_uids([uid]))
        .await
        .expect("fetch")
        .remove(0)
        .data;
    imap.logout().await.ok();
    let parsed = mail_content::parse(&raw);
    assert_eq!(parsed.headers.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(
        parsed.headers.from[0].name.as_deref(),
        Some("Bob Bäcker 🍞")
    );
    assert_eq!(parsed.headers.to[0].name.as_deref(), Some("Alice Ärger 🌸"));
    assert!(
        parsed
            .html
            .as_deref()
            .is_some_and(|h| h.contains("Grüße 😀"))
    );
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].file_name, name);
    let received = mail_content::attachment_bytes(&raw, parsed.attachments[0].part_id)
        .expect("attachment bytes");
    assert_eq!(sha256(&received), expected, "same checksum");
    // No header names the program.
    let head = header_block(&raw).to_ascii_lowercase();
    assert!(
        !head.contains("user-agent") && !head.contains("x-mailer"),
        "{head}"
    );

    bed.core.shutdown().await;
    clean_server(env).await;
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// BCC-Empfänger erhält die Mail, sein Name steht nicht in den Kopfzeilen der anderen; die
/// Kopie in „Gesendet“ nennt ihn.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_blind_copy_arrives_and_is_named_nowhere_else() {
    let _serial = SERVER.lock().await;
    let bed = Bed::new(SmtpSender::new(), None).await;
    let env = &bed.env;
    let subject = format!("{MARK}Blindkopie {}", run_id());
    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice Ärger", &env.alice.address)],
            vec![person("Geheime Verwaltung", &env.admin.address)],
            &subject,
            "<p>Nur für Alice sichtbar adressiert.</p>",
        )
        .await;
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("bcc"))
        .await
        .expect("send");
    assert_eq!(outgoing.recipient_count, 2);
    bed.sent(bed.bob, outgoing.operation_id).await;
    bed.settled(bed.bob).await;
    bed.arrived(bed.alice, MailboxRole::Inbox, &subject).await;
    let sent = bed.arrived(bed.bob, MailboxRole::Sent, &subject).await;
    drop(sent);

    // Find the Message-ID from bob's Sent copy, then look everywhere.
    let mut imap = admin_session(env, &env.bob).await;
    let opened = imap.open_mailbox("Sent").await.expect("sent");
    let uids = imap.search_uids(&opened).await.expect("uids");
    let message_id = imap
        .fetch_headers(&opened, &uids)
        .await
        .expect("headers")
        .into_iter()
        .find(|h| h.subject == subject)
        .and_then(|h| h.message_id)
        .expect("Sent copy");
    imap.logout().await.ok();

    let admin_copy = until("the blind copy at admin", || async {
        let found = copies(env, &env.admin, "INBOX", &message_id).await;
        (!found.is_empty()).then_some(found)
    })
    .await;
    assert_eq!(admin_copy.len(), 1);
    let alice_copy = copies(env, &env.alice, "INBOX", &message_id).await;
    assert_eq!(alice_copy.len(), 1);
    // The others never see the blind copy; the blind recipient's own copy only names them in
    // its server's delivery header (`Delivered-To`), not in a header the sender wrote.
    let alice_head = header_block(&alice_copy[0].1);
    assert!(!alice_head.contains(&env.admin.address), "{alice_head}");
    assert!(!alice_head.contains("Geheime"), "{alice_head}");
    for (who, raw) in [("alice", &alice_copy[0].1), ("admin", &admin_copy[0].1)] {
        let head = header_block(raw);
        assert!(!head.contains("Geheime"), "{who}: {head}");
        assert!(
            !head.to_ascii_lowercase().contains("\nbcc:"),
            "{who}: {head}"
        );
        let parsed = mail_content::parse(raw);
        assert!(parsed.headers.bcc.is_empty(), "{who}");
        assert!(
            parsed
                .headers
                .to
                .iter()
                .chain(&parsed.headers.cc)
                .all(|a| a.email != env.admin.address),
            "{who}: {head}"
        );
    }
    let sent_copy = copies(env, &env.bob, "Sent", &message_id).await;
    assert_eq!(sent_copy.len(), 1);
    let parsed = mail_content::parse(&sent_copy[0].1);
    assert_eq!(
        parsed.headers.bcc.len(),
        1,
        "the Sent copy names the blind copy"
    );
    assert_eq!(parsed.headers.bcc[0].email, env.admin.address);

    bed.core.shutdown().await;
    clean_server(env).await;
}

async fn smtp_proxy(env: &TestEnv) -> Arc<Proxy> {
    let upstream: SocketAddr = format!("{}:{}", env.host, env.submission_port)
        .parse()
        .expect("upstream");
    Proxy::start(upstream).await.expect("proxy")
}

/// Abbrechen innerhalb der Verzögerung sendet nichts: no SMTP connection at all (proxy
/// counter), with a positive control that the counter counts.
#[tokio::test]
#[ignore = "needs the test server"]
async fn cancelling_within_the_delay_sends_nothing() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let proxy = smtp_proxy(&env).await;
    let bed = Bed::new(
        SmtpSender::new(),
        Some(("127.0.0.1".into(), proxy.port(), Security::Starttls)),
    )
    .await;
    bed.core
        .set_compose_settings(ComposeSettings {
            send_delay_seconds: 3,
            ..ComposeSettings::default()
        })
        .await
        .expect("delay");
    let subject = format!("{MARK}Rückgängig {}", run_id());
    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice", &env.alice.address)],
            Vec::new(),
            &subject,
            "<p>Doch nicht.</p>",
        )
        .await;
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("undo"))
        .await
        .expect("send");
    assert_eq!(outgoing.state, SendState::Scheduled);
    assert!(
        outgoing.send_at > outgoing.created_at + 2_000,
        "{outgoing:?}"
    );
    let frozen = bed.core.get_draft(draft.id.clone()).await.expect("draft");
    assert_eq!(frozen.state, DraftState::Sending);
    let refused = bed
        .core
        .save_draft(
            draft.id.clone(),
            DraftUpdate {
                identity_id: frozen.identity_id,
                to: frozen.to.clone(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "changed".into(),
                format: ComposeFormat::Html,
                body_html: String::new(),
                body_text: String::new(),
                include_quote: true,
                quote_position: QuotePosition::Above,
                include_signature: true,
                signature_id: None,
            },
        )
        .await
        .expect_err("frozen");
    assert_eq!(refused.code(), ErrorCode::SendInProgress);

    tokio::time::sleep(Duration::from_millis(1_000)).await;
    let back = bed
        .core
        .cancel_send(draft.id.clone())
        .await
        .expect("cancel");
    assert_eq!(back.state, DraftState::Editing);
    assert_eq!(back.subject, subject, "the draft is as it was");
    // Past the delay: still nothing.
    tokio::time::sleep(Duration::from_millis(4_000)).await;
    assert_eq!(proxy.accepted(), 0, "no SMTP connection");
    assert_eq!(proxy.bytes_to_server(), 0);
    assert!(
        bed.core
            .list_outgoing(bed.bob, None, 10)
            .await
            .expect("outgoing")
            .is_empty()
    );
    assert!(
        bed.find(bed.alice, MailboxRole::Inbox, &subject)
            .await
            .is_none()
    );

    // Positive control: sending again (no delay) goes through the proxy.
    bed.core
        .set_compose_settings(ComposeSettings {
            send_delay_seconds: 0,
            ..ComposeSettings::default()
        })
        .await
        .expect("no delay");
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("undo-2"))
        .await
        .expect("send");
    bed.sent(bed.bob, outgoing.operation_id).await;
    assert_eq!(proxy.accepted(), 1, "the counter counts");
    bed.arrived(bed.alice, MailboxRole::Inbox, &subject).await;

    bed.core.shutdown().await;
    clean_server(&bed.env).await;
}

/// Fehlerinjektion: the connection is cut right after the end of the data. Not sent again by
/// itself (also not after a restart); alice has exactly one copy; the app asks. Then the
/// variant of a server that files sent mail itself: the copy in Sent settles it.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_cut_after_the_data_is_unclear_and_never_sent_twice() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    let proxy = smtp_proxy(&env).await;
    let tap = WireTap::new();
    let sender = SmtpSender::new().with_wire_tap(tap.clone());
    let mut bed = Bed::new(
        sender,
        Some(("127.0.0.1".into(), proxy.port(), Security::Starttls)),
    )
    .await;
    let cut = Arc::clone(&proxy);
    tap.on_write("\r\n.\r\n", move || {
        cut.cut_responses(Duration::from_millis(500))
    });
    let subject = format!("{MARK}Unklar {}", run_id());
    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice", &env.alice.address)],
            Vec::new(),
            &subject,
            "<p>Kam das an?</p>",
        )
        .await;
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("cut"))
        .await
        .expect("send");
    let operation = outgoing.operation_id;

    // The app asks.
    let deadline = Instant::now() + WAIT;
    let asked = loop {
        match tokio::time::timeout_at(deadline, bed.events.recv()).await {
            Ok(Ok(CoreEvent::SendNeedsDecision { mail, .. })) if mail.operation_id == operation => {
                break mail;
            }
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(err)) => panic!("events: {err}"),
            Err(_) => panic!("no question to the user"),
        }
    };
    assert_eq!(asked.state, SendState::Unclear);
    assert_eq!(
        asked.error.as_ref().map(|e| e.code),
        Some(ErrorCode::SendUnclear)
    );
    let data_markers = |tap: &WireTap| {
        let bytes = tap.bytes();
        bytes.windows(5).filter(|w| w == b"\r\n.\r\n").count()
    };
    assert_eq!(proxy.accepted(), 1, "one SMTP connection");
    assert_eq!(data_markers(&tap), 1, "one end of data");

    // Exactly one copy at alice (the server had the whole message).
    let message_id = {
        let delivered = until("the message at alice", || async {
            bed.find(bed.alice, MailboxRole::Inbox, &subject).await
        })
        .await;
        drop(delivered);
        let mut imap = admin_session(&env, &env.alice).await;
        let opened = imap.open_mailbox("INBOX").await.expect("inbox");
        let uids = imap.search_uids(&opened).await.expect("uids");
        let id = imap
            .fetch_headers(&opened, &uids)
            .await
            .expect("headers")
            .into_iter()
            .find(|h| h.subject == subject)
            .and_then(|h| h.message_id)
            .expect("id");
        imap.logout().await.ok();
        id
    };
    assert_eq!(
        copies(&env, &env.alice, "INBOX", &message_id).await.len(),
        1
    );

    // Retry is refused without a decision; a restart sends nothing either.
    let err = bed
        .core
        .retry_operation(operation)
        .await
        .expect_err("needs a decision");
    assert_eq!(err.code(), ErrorCode::ConfirmationRequired);
    bed.core.shutdown().await;
    let data = bed.dir.path().join("data");
    bed.core = start_core(
        &env,
        &data,
        &bed.keychain,
        SmtpSender::new().with_wire_tap(tap.clone()),
    )
    .await;
    bed.events = bed.core.subscribe();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let still = bed
        .core
        .list_outgoing(bed.bob, None, 10)
        .await
        .expect("outgoing")
        .into_iter()
        .find(|m| m.operation_id == operation)
        .expect("still listed");
    assert_eq!(still.state, SendState::Unclear);
    assert_eq!(proxy.accepted(), 1, "never sent again by itself");
    assert_eq!(data_markers(&tap), 1);

    // The user checked: it arrived. The app finishes it (Sent copy), nothing is submitted.
    let done = bed
        .core
        .resolve_unclear_send(operation, UnclearDecision::TreatAsSent)
        .await
        .expect("decide");
    assert_eq!(done.state, SendState::Sent);
    bed.settled(bed.bob).await;
    assert_eq!(copies(&env, &env.bob, "Sent", &message_id).await.len(), 1);
    assert_eq!(proxy.accepted(), 1);
    assert_eq!(
        copies(&env, &env.alice, "INBOX", &message_id).await.len(),
        1,
        "never two"
    );

    // A server that files sent mail itself: the test plays Gmail and files the copy; the app
    // finds it in Sent and does not ask. A fresh tap (the old one ends with the marker).
    bed.core.shutdown().await;
    let tap = WireTap::new();
    bed.core = start_core(
        &env,
        &data,
        &bed.keychain,
        SmtpSender::new().with_wire_tap(tap.clone()),
    )
    .await;
    bed.events = bed.core.subscribe();
    bed.core
        .set_sent_copy_mode(bed.bob, SentCopyMode::Server)
        .await
        .expect("server files");
    let cut = Arc::clone(&proxy);
    tap.on_write("\r\n.\r\n", move || {
        cut.cut_responses(Duration::from_millis(500))
    });
    let subject2 = format!("{MARK}Unklar mit Ablage {}", run_id());
    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice", &env.alice.address)],
            Vec::new(),
            &subject2,
            "<p>Und diesmal?</p>",
        )
        .await;
    let second = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("cut-2"))
        .await
        .expect("send");
    let verifying = until("the check of Sent", || async {
        bed.core
            .list_outgoing(bed.bob, None, 10)
            .await
            .ok()?
            .into_iter()
            .find(|m| {
                m.operation_id == second.operation_id
                    && m.state != SendState::Sending
                    && m.state != SendState::Queued
            })
    })
    .await;
    assert_eq!(verifying.state, SendState::Verifying, "{verifying:?}");
    // "Gmail" files it now.
    let delivered = until("the second message at alice", || async {
        let mut imap = admin_session(&env, &env.alice).await;
        let opened = imap.open_mailbox("INBOX").await.ok()?;
        let uids = imap.search_uids(&opened).await.ok()?;
        let headers = imap.fetch_headers(&opened, &uids).await.ok()?;
        imap.logout().await.ok();
        headers.into_iter().find(|h| h.subject == subject2)
    })
    .await;
    let id2 = delivered.message_id.expect("id");
    let raw = copies(&env, &env.alice, "INBOX", &id2).await.remove(0).1;
    let mut bob_imap = admin_session(&env, &env.bob).await;
    let reply = bob_imap
        .raw_command(
            &format!(
                "APPEND \"Sent\" (\\Seen) {{{}+}}\r\n{}",
                raw.len(),
                String::from_utf8_lossy(&raw)
            ),
            |_| false,
        )
        .await
        .expect("append");
    assert!(reply.ok, "{}", reply.text);
    bob_imap.logout().await.ok();
    let sent = bed.sent(bed.bob, second.operation_id).await;
    assert!(sent.error.is_none() || sent.state == SendState::Sent);
    bed.settled(bed.bob).await;
    assert_eq!(proxy.accepted(), 2, "one connection per message");
    assert_eq!(data_markers(&tap), 1);
    assert_eq!(copies(&env, &env.alice, "INBOX", &id2).await.len(), 1);
    assert_eq!(
        copies(&env, &env.bob, "Sent", &id2).await.len(),
        1,
        "the app appended no second copy"
    );

    bed.core.shutdown().await;
    clean_server(&env).await;
}

/// Entwurf überlebt Beenden und Absturz; im Entwürfe-Ordner liegt immer nur die neueste
/// Fassung. The crash is real: a child process saves and aborts.
#[tokio::test]
#[ignore = "needs the test server"]
async fn drafts_survive_quitting_and_a_crash_and_only_the_newest_is_on_the_server() {
    let _serial = SERVER.lock().await;
    let mut bed = Bed::new(SmtpSender::new(), None).await;
    let env = TestEnv::require();
    let run = run_id();
    let subject = |n: u32| format!("{MARK}Entwurf {run} v{n}");
    let draft = bed.new_draft(bed.bob).await;
    let message_id = {
        // The Message-ID is fixed at creation; read it from the first server copy.
        let saved = bed
            .write(
                &draft,
                vec![person("Alice", &env.alice.address)],
                Vec::new(),
                &subject(1),
                "<p>Eins</p>",
            )
            .await;
        assert_eq!(saved.revision, 2);
        let copy = until("version 1 in Drafts", || async {
            let mut imap = admin_session(&env, &env.bob).await;
            let opened = imap.open_mailbox("Drafts").await.ok()?;
            let uids = imap.search_uids(&opened).await.ok()?;
            let headers = if uids.is_empty() {
                Vec::new()
            } else {
                imap.fetch_headers(&opened, &uids).await.ok()?
            };
            imap.logout().await.ok();
            headers.into_iter().find(|h| h.subject == subject(1))
        })
        .await;
        assert!(copy.flags.draft, "\\Draft");
        copy.message_id.expect("id")
    };
    let only_newest = |n: u32| {
        let env = &env;
        let message_id = message_id.clone();
        let wanted = subject(n);
        async move {
            until(&format!("only version {n} in Drafts"), || async {
                let found = copies(env, &env.bob, "Drafts", &message_id).await;
                let subjects: Vec<String> = found
                    .iter()
                    .filter_map(|(_, raw)| mail_content::parse(raw).headers.subject)
                    .collect();
                (subjects == vec![wanted.clone()]).then_some(())
            })
            .await;
        }
    };
    only_newest(1).await;
    let info = until("the draft saved on the server", || async {
        let d = bed.core.get_draft(draft.id.clone()).await.ok()?;
        (d.server == DraftServerState::Saved).then_some(d)
    })
    .await;
    assert_eq!(info.revision, 2);

    // Version 2 after the quiet delay: appended, version 1 removed.
    bed.write(
        &draft,
        vec![person("Alice", &env.alice.address)],
        Vec::new(),
        &subject(2),
        "<p>Zwei</p>",
    )
    .await;
    only_newest(2).await;
    // Version 3 with closing: at once.
    bed.write(
        &draft,
        vec![person("Alice", &env.alice.address)],
        Vec::new(),
        &subject(3),
        "<p>Drei</p>",
    )
    .await;
    bed.core.close_draft(draft.id.clone()).await.expect("close");
    only_newest(3).await;
    bed.settled(bed.bob).await;

    // Quit and start again: the draft is there.
    bed.core.shutdown().await;
    let data = bed.dir.path().join("data");
    bed.core = start_core(&env, &data, &bed.keychain, SmtpSender::new()).await;
    let reopened = bed
        .core
        .get_draft(draft.id.clone())
        .await
        .expect("after quitting");
    assert_eq!(reopened.subject, subject(3));
    assert!(
        bed.core
            .list_drafts(Some(bed.bob))
            .await
            .expect("drafts")
            .iter()
            .any(|d| d.id == draft.id)
    );
    bed.core.shutdown().await;

    // A crash: a child process saves version 4 and aborts at once (no shutdown, no close).
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "crash_child_saves_a_draft_and_aborts",
            "--exact",
            "--include-ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("VMC_CRASH_DIR", &data)
        .env("VMC_CRASH_ACCOUNT", bed.bob.to_string())
        .env("VMC_CRASH_DRAFT", draft.id.as_str())
        .env("VMC_CRASH_SUBJECT", subject(4))
        .status()
        .expect("child");
    assert!(!status.success(), "the child aborted: {status:?}");
    bed.core = start_core(&env, &data, &bed.keychain, SmtpSender::new()).await;
    let recovered = bed
        .core
        .get_draft(draft.id.clone())
        .await
        .expect("after the crash");
    assert_eq!(
        recovered.subject,
        subject(4),
        "the save before the crash is kept"
    );
    assert_eq!(recovered.body_html, "<p>Vier</p>");
    // After the restart the unsaved version goes to the server; still only one copy.
    only_newest(4).await;

    // Discarding removes the server copy too.
    bed.core
        .delete_draft(draft.id.clone())
        .await
        .expect("delete");
    bed.settled(bed.bob).await;
    until("no copy in Drafts", || async {
        copies(&env, &env.bob, "Drafts", &message_id)
            .await
            .is_empty()
            .then_some(())
    })
    .await;
    bed.core.shutdown().await;
    clean_server(&env).await;
}

/// The child of the crash test (does nothing unless started by it).
#[tokio::test]
#[ignore = "child process of the crash test"]
async fn crash_child_saves_a_draft_and_aborts() {
    let Some(data) = std::env::var_os("VMC_CRASH_DIR").map(PathBuf::from) else {
        return;
    };
    let env = TestEnv::require();
    let account: AccountId = std::env::var("VMC_CRASH_ACCOUNT")
        .expect("account")
        .parse()
        .expect("account id");
    let draft = mail_model::DraftId::parse(&std::env::var("VMC_CRASH_DRAFT").expect("draft"))
        .expect("draft id");
    let subject = std::env::var("VMC_CRASH_SUBJECT").expect("subject");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    Credentials::new(Arc::clone(&keychain))
        .set(
            &CredentialRef::for_account(account),
            env.bob.password.clone(),
        )
        .await
        .expect("password");
    let core = start_core(&env, &data, &keychain, SmtpSender::new()).await;
    let current = core.get_draft(draft.clone()).await.expect("draft");
    core.save_draft(
        draft,
        DraftUpdate {
            identity_id: current.identity_id,
            to: current.to,
            cc: Vec::new(),
            bcc: Vec::new(),
            subject,
            format: ComposeFormat::Html,
            body_html: "<p>Vier</p>".into(),
            body_text: String::new(),
            include_quote: true,
            quote_position: QuotePosition::Above,
            include_signature: true,
            signature_id: None,
        },
    )
    .await
    .expect("save");
    std::process::abort();
}

/// A read-only account keeps drafts on the device and refuses to send with `read_only`.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_read_only_account_keeps_drafts_local_and_refuses_to_send() {
    let _serial = SERVER.lock().await;
    let bed = Bed::new(SmtpSender::new(), None).await;
    let env = &bed.env;
    bed.core
        .set_writes_enabled(bed.bob, false)
        .await
        .expect("writes off");
    let subject = format!("{MARK}Nur lesen {}", run_id());
    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice", &env.alice.address)],
            Vec::new(),
            &subject,
            "<p>x</p>",
        )
        .await;
    assert_eq!(draft.server, DraftServerState::LocalOnly);
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    let closed = bed.core.close_draft(draft.id.clone()).await.expect("close");
    assert_eq!(closed.server, DraftServerState::LocalOnly);
    let err = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("ro"))
        .await
        .expect_err("read-only");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    let still = bed.core.get_draft(draft.id.clone()).await.expect("draft");
    assert_eq!(still.state, DraftState::Editing);
    let ops = bed
        .core
        .list_operations(bed.bob, None, 50)
        .await
        .expect("operations");
    assert!(
        ops.iter().all(|op| op.state != OperationState::Queued),
        "nothing queued: {ops:?}"
    );
    let mut imap = admin_session(env, &env.bob).await;
    let opened = imap.open_mailbox("Drafts").await.expect("drafts");
    let uids = imap.search_uids(&opened).await.expect("uids");
    let on_server = if uids.is_empty() {
        Vec::new()
    } else {
        imap.fetch_headers(&opened, &uids).await.expect("headers")
    };
    imap.logout().await.ok();
    assert!(
        on_server.iter().all(|h| h.subject != subject),
        "nothing on the server"
    );
    bed.core.shutdown().await;
    clean_server(env).await;
}

/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Stage 10: an inline image goes out as a `cid:` part and is shown by the second account's
/// reading pane (as `data:` in the sandboxed document), not listed as an attachment.
#[tokio::test]
#[ignore = "needs the test server"]
async fn an_inline_image_arrives_and_is_shown_at_the_second_account() {
    let _serial = SERVER.lock().await;
    let bed = Bed::new(SmtpSender::new(), None).await;
    let env = &bed.env;
    let subject = format!("{MARK}Inline image {}", run_id());
    let files = TempDir::new().expect("files");
    let path = files.path().join("Diagramm.png");
    std::fs::write(&path, PNG).expect("png");

    let draft = bed.new_draft(bed.alice).await;
    let added = bed
        .core
        .add_inline_images(draft.id.clone(), vec![path])
        .await
        .expect("inline image");
    let content_id = added.added[0].content_id.clone().expect("content id");
    let html = format!(
        "<p>Hallo Bob, hier das Diagramm:</p><p><img src=\"cid:{content_id}\" width=\"120\" \
         alt=\"Diagramm\"></p>"
    );
    let draft = bed
        .write(
            &added.draft,
            vec![person("Bob", &env.bob.address)],
            Vec::new(),
            &subject,
            &html,
        )
        .await;
    let outgoing = bed
        .core
        .send_draft(draft.id.clone(), uuid_like("inline"))
        .await
        .expect("send");
    bed.sent(bed.alice, outgoing.operation_id).await;
    let item = bed.arrived(bed.bob, MailboxRole::Inbox, &subject).await;

    // On the server: one related part with the image, unchanged.
    let (_, raw) = server_message(env, &env.bob, "INBOX", &subject)
        .await
        .expect("on the server");
    let parsed = mail_content::parse(&raw);
    assert_eq!(parsed.inline.len(), 1, "{:?}", parsed.inline);
    assert_eq!(parsed.inline[0].content_id, content_id);
    assert_eq!(&*parsed.inline[0].bytes, PNG);
    assert!(
        String::from_utf8_lossy(&raw).contains("multipart/related"),
        "{}",
        header_block(&raw)
    );

    // Bob's reading pane resolves it.
    let view = until("bob's view of the message", || async {
        let view = bed
            .core
            .get_message(item.id, mail_model::BodyFormat::Html)
            .await
            .expect("view");
        matches!(view.body, mail_model::MessageBody::Html { .. }).then_some(view)
    })
    .await;
    let mail_model::MessageBody::Html { srcdoc, .. } = &view.body else {
        unreachable!("checked above");
    };
    assert!(srcdoc.contains("src=\"data:image/png;base64,"), "{srcdoc}");
    assert!(!srcdoc.contains("cid:"), "{srcdoc}");
    assert!(
        view.attachments.is_empty(),
        "an inline image is not an attachment: {:?}",
        view.attachments
    );
    clean_server(env).await;
}

/// Base64 in lines of 76 characters, as mail programs write attachments.
fn base64_lines(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out.as_bytes()
        .chunks(76)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Entwürfe anderer Programme aus dem Entwürfe-Ordner öffnen: a draft that another program
/// (here: as Thunderbird writes it) put into bob's Drafts folder opens as a local draft, with
/// the HTML through the allowlist and the attachment byte for byte. Edited and closed, it
/// replaces the foreign copy (same Message-ID, exactly one copy, the new content, the
/// attachment kept); discarding it removes the copy.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_draft_of_another_program_opens_and_its_server_copy_is_replaced() {
    let _serial = SERVER.lock().await;
    let bed = Bed::new(SmtpSender::new(), None).await;
    let env = &bed.env;
    let run = run_id();
    let subject = format!("{MARK}Fremder Entwurf {run}");
    let edited = format!("{MARK}Fremder Entwurf {run} bearbeitet");
    let message_id = format!("foreign-{run}@other.example");
    let agenda: Vec<u8> = (0..20_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect();
    let encoded = base64_lines(&agenda);
    let raw = format!(
        "From: Bob <{bob}>\r\n\
         To: Alice <{alice}>\r\n\
         Cc: Admin <{admin}>\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{message_id}>\r\n\
         Date: Mon, 23 Nov 2026 07:00:00 +0100\r\n\
         User-Agent: Mozilla Thunderbird\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"mixed\"\r\n\
         \r\n\
         --mixed\r\n\
         Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
         \r\n\
         --alt\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         Hallo Alice, die Tagesordnung f\u{fc}r Montag.\r\n\
         --alt\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         \r\n\
         <p onclick=\"steal()\">Hallo <b>Alice</b>, die Tagesordnung f\u{fc}r Montag.</p>\
         <script>alert(1)</script>\r\n\
         --alt--\r\n\
         --mixed\r\n\
         Content-Type: application/pdf; name=\"Tagesordnung.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"Tagesordnung.pdf\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {encoded}\r\n\
         --mixed--\r\n",
        bob = env.bob.address,
        alice = env.alice.address,
        admin = env.admin.address,
    );
    let mut imap = admin_session(env, &env.bob).await;
    let reply = imap
        .raw_command(
            &format!(
                "APPEND \"Drafts\" (\\Draft \\Seen) {{{}+}}\r\n{raw}",
                raw.len()
            ),
            |_| false,
        )
        .await
        .expect("append the foreign draft");
    assert!(reply.ok, "{}", reply.text);
    imap.logout().await.ok();
    assert_eq!(copies(env, &env.bob, "Drafts", &message_id).await.len(), 1);

    let item = bed.arrived(bed.bob, MailboxRole::Drafts, &subject).await;
    let draft = bed
        .core
        .open_draft_from_message(item.id)
        .await
        .expect("open the foreign draft");
    assert_eq!(draft.account_id, bed.bob);
    assert_eq!(draft.subject, subject);
    assert_eq!(draft.to, vec![person("Alice", &env.alice.address)]);
    assert_eq!(draft.cc, vec![person("Admin", &env.admin.address)]);
    assert_eq!(draft.format, ComposeFormat::Html);
    assert!(
        draft.body_html.contains("Tagesordnung für Montag"),
        "{}",
        draft.body_html
    );
    for hostile in ["script", "alert", "onclick", "steal"] {
        assert!(
            !draft.body_html.contains(hostile),
            "{hostile} left in {}",
            draft.body_html
        );
    }
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].file_name, "Tagesordnung.pdf");
    assert_eq!(draft.attachments[0].size, 20_000);
    assert_eq!(
        draft.server,
        DraftServerState::Saved,
        "the foreign copy counts as the first version"
    );
    // Opened again: the same draft.
    let again = bed
        .core
        .open_draft_from_message(item.id)
        .await
        .expect("open again");
    assert_eq!(again.id, draft.id);

    // Edited and closed: the new version replaces the foreign copy.
    let saved = bed
        .write(
            &draft,
            vec![person("Alice", &env.alice.address)],
            Vec::new(),
            &edited,
            "<p>Hallo Alice, die neue Tagesordnung.</p>",
        )
        .await;
    assert!(saved.revision > draft.revision);
    bed.core.close_draft(draft.id.clone()).await.expect("close");
    until("only the edited version in Drafts", || async {
        let found = copies(env, &env.bob, "Drafts", &message_id).await;
        let subjects: Vec<String> = found
            .iter()
            .filter_map(|(_, raw)| mail_content::parse(raw).headers.subject)
            .collect();
        (subjects == vec![edited.clone()]).then_some(())
    })
    .await;
    bed.settled(bed.bob).await;
    let found = copies(env, &env.bob, "Drafts", &message_id).await;
    assert_eq!(found.len(), 1, "exactly one copy");
    let parsed = mail_content::parse(&found[0].1);
    assert_eq!(
        parsed.headers.message_id.as_deref(),
        Some(message_id.as_str())
    );
    assert!(
        parsed
            .html
            .as_deref()
            .is_some_and(|h| h.contains("die neue Tagesordnung") && !h.contains("script")),
        "{:?}",
        parsed.html
    );
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].file_name, "Tagesordnung.pdf");
    let bytes = mail_content::attachment_bytes(&found[0].1, parsed.attachments[0].part_id)
        .expect("attachment bytes");
    assert_eq!(sha256(&bytes), sha256(&agenda), "the attachment is kept");
    let info = bed.core.get_draft(draft.id.clone()).await.expect("draft");
    assert_eq!(info.server, DraftServerState::Saved);

    // Discarding removes the copy.
    bed.core
        .delete_draft(draft.id.clone())
        .await
        .expect("discard");
    bed.settled(bed.bob).await;
    until("no copy in Drafts", || async {
        copies(env, &env.bob, "Drafts", &message_id)
            .await
            .is_empty()
            .then_some(())
    })
    .await;
    bed.core.shutdown().await;
    clean_server(env).await;
}

/// Stage 16 (decision 0033, acceptance): a read-receipt request never leads to an SMTP
/// submission without the user's answer. bob asks alice for a receipt; alice's core receives
/// and shows the message, opens it again, and the SMTP wire tap sees no new `MAIL FROM`. Only
/// `sendReadReceipt` submits one: a disposition report the server accepts, which arrives at bob
/// and shows at his sent message.
#[tokio::test]
#[ignore = "needs the test server"]
async fn a_read_receipt_goes_out_only_on_the_users_answer_smtp_capture() {
    let _serial = SERVER.lock().await;
    let tap = WireTap::new();
    let bed = Bed::new(SmtpSender::new().with_wire_tap(tap.clone()), None).await;
    let env = &bed.env;
    let subject = format!("{MARK}Lesebestätigung {}", run_id());
    let submissions = |tap: &WireTap| {
        tap.lines()
            .iter()
            .filter(|l| l.to_ascii_uppercase().starts_with("MAIL FROM:"))
            .count()
    };

    // bob asks for a read receipt.
    let draft = bed.new_draft(bed.bob).await;
    let draft = bed
        .write(
            &draft,
            vec![person("Alice", &env.alice.address)],
            Vec::new(),
            &subject,
            "<p>Bitte bestätigen.</p>",
        )
        .await;
    bed.core
        .set_draft_send_options(
            draft.id.clone(),
            mail_model::DraftSendOptions {
                archive_original: false,
                request_read_receipt: true,
            },
        )
        .await
        .expect("ask for a receipt");
    let mail = bed
        .core
        .send_draft(draft.id.clone(), "req-mdn-server-1".into())
        .await
        .expect("send");
    bed.sent(bed.bob, mail.operation_id).await;
    bed.settled(bed.bob).await;
    assert_eq!(submissions(&tap), 1, "bob's message");

    // alice gets it and opens it: the request shows, nothing is submitted.
    let at_alice = bed.arrived(bed.alice, MailboxRole::Inbox, &subject).await;
    opened(&bed.core, at_alice.id).await;
    let info = bed
        .core
        .get_read_receipt(at_alice.id)
        .await
        .expect("receipt info");
    let request = info.request.expect("alice sees the request");
    assert_eq!(request.to, env.bob.address.to_ascii_lowercase());
    assert_eq!(request.answer, mail_model::ReadReceiptAnswer::Ask);
    tokio::time::sleep(Duration::from_secs(3)).await;
    opened(&bed.core, at_alice.id).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        submissions(&tap),
        1,
        "no SMTP submission without the user's answer"
    );

    // alice answers "send".
    let receipt = bed
        .core
        .send_read_receipt(at_alice.id, QuoteLanguage::De, "req-mdn-server-2".into())
        .await
        .expect("send the receipt");
    assert!(receipt.read_receipt);
    bed.sent(bed.alice, receipt.operation_id).await;
    bed.settled(bed.alice).await;
    assert_eq!(submissions(&tap), 2, "exactly one receipt");
    let wire = String::from_utf8_lossy(&tap.bytes()).into_owned();
    assert!(wire.contains("multipart/report"), "a disposition report");
    assert!(wire.contains("message/disposition-notification"));

    // It arrives at bob and shows at his sent message.
    let receipt_subject = format!("Gelesen: {subject}");
    let at_bob = bed
        .arrived(bed.bob, MailboxRole::Inbox, &receipt_subject)
        .await;
    opened(&bed.core, at_bob.id).await;
    let sent = bed.arrived(bed.bob, MailboxRole::Sent, &subject).await;
    opened(&bed.core, sent.id).await;
    let info = until("the receipt at bob's message", || async {
        let info = bed.core.get_read_receipt(sent.id).await.ok()?;
        (!info.received.is_empty()).then_some(info)
    })
    .await;
    assert!(info.requested);
    assert_eq!(
        info.received[0].disposition,
        mail_model::ReceiptDisposition::Displayed
    );
    assert_eq!(
        info.received[0].recipient.as_deref(),
        Some(env.alice.address.to_ascii_lowercase().as_str())
    );
    let answered = bed
        .core
        .get_read_receipt(at_alice.id)
        .await
        .expect("alice's answer");
    assert_eq!(
        answered.request.map(|r| r.answer),
        Some(mail_model::ReadReceiptAnswer::Sent)
    );
    bed.core.shutdown().await;
    clean_server(env).await;
}

/// Opens the message in the reading pane until its body is stored.
async fn opened(core: &AppCore<ImapConnector>, id: mail_model::MessageId) {
    until("the body", || async {
        let view = core
            .get_message(id, mail_model::BodyFormat::Text)
            .await
            .ok()?;
        (!matches!(
            view.body,
            mail_model::MessageBody::Loading | mail_model::MessageBody::Unavailable { .. }
        ))
        .then_some(())
    })
    .await;
}
