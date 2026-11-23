//! Unit tests of the compose service: its pure parts, and (in `with_core`) opening drafts of
//! other programs and sending while offline through `AppCore` with the fake server.

use super::*;

#[test]
fn gmail_files_sent_mail_itself() {
    assert!(files_sent_itself("imap.gmail.com"));
    assert!(files_sent_itself("IMAP.googlemail.com."));
    assert!(!files_sent_itself("imap.example.org"));
    assert!(!files_sent_itself("notgmail.com"));
}

#[test]
fn request_ids_and_mime_types() {
    assert!(valid_request_id("6f9619ff-8b86-4d11-b42d-00c04fc964ff").is_ok());
    assert!(valid_request_id("").is_err());
    assert!(valid_request_id("a b").is_err());
    assert_eq!(mime_of("Rechnung.PDF"), "application/pdf");
    assert_eq!(mime_of("no-extension"), "application/octet-stream");
}

#[test]
fn saved_content_is_cleaned() {
    let update = DraftUpdate {
        identity_id: None,
        to: vec![
            Address {
                name: Some(" Bob\n ".into()),
                email: " bob@example.org ".into(),
            },
            Address {
                name: None,
                email: "   ".into(),
            },
        ],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Hallo\r\nBcc: x@example.org".into(),
        format: ComposeFormat::Html,
        body_html: "<p>x</p>".into(),
        body_text: String::new(),
        include_quote: true,
        quote_position: QuotePosition::Above,
        include_signature: true,
        signature_id: None,
    };
    let content = content_of(&update).expect("content");
    assert_eq!(
        content.to,
        vec![Address {
            name: Some("Bob".into()),
            email: "bob@example.org".into()
        }]
    );
    assert!(!content.subject.contains(['\r', '\n']));
}

// ---------------------------------------------------------------------------------------------
// Through `AppCore` with the fake IMAP server and a recording SMTP sender (stage 5 gaps): a
// draft of another program opened from the Drafts folder; sending at once while offline.

mod with_core {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use mail_model::{
        AccountId, AccountSettings, AccountState, Address, BackendError, ComposeFormat,
        ComposeSettings, ConnectConfig, CoreEvent, Draft, DraftKind, DraftServerState, DraftState,
        DraftUpdate, Envelope, ErrorCode, Flags, MailSender, MailboxId, MailboxRole, MessageId,
        NewDraft, QuotePosition, Secret, Security, SendFuture, SendOutcome, SendState,
        ServerSettings, SmtpLimits, SmtpReply, Uid,
    };
    use tempfile::TempDir;
    use tokio::sync::broadcast;
    use tokio::time::Instant;

    use super::super::files;
    use crate::credentials::{Credentials, MemorySecretStore};
    use crate::outbox::{ComposeConfig, OutboxConfig, SenderHandle};
    use crate::sync::SyncConfig;
    use crate::sync::fake::{FakeConnector, FakeMailbox, FakeMessage, FakeServer, PASSWORD};
    use crate::sync_state::BackoffPolicy;
    use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

    const WAIT: Duration = Duration::from_secs(15);

    /// Records what it submits; can be offline (nothing transmitted).
    #[derive(Debug, Default)]
    struct RecordingSender {
        sent: Mutex<Vec<Vec<u8>>>,
        offline: AtomicBool,
        attempts: AtomicI64,
    }

    impl RecordingSender {
        fn sent_with(&self, subject: &str) -> usize {
            let needle = format!("Subject: {subject}\r\n");
            self.sent
                .lock()
                .expect("sent")
                .iter()
                .filter(|m| String::from_utf8_lossy(m).contains(&needle))
                .count()
        }

        fn attempts(&self) -> i64 {
            self.attempts.load(Ordering::SeqCst)
        }
    }

    impl MailSender for RecordingSender {
        fn send<'a>(
            &'a self,
            _config: &'a ConnectConfig,
            _envelope: &'a Envelope,
            message: &'a [u8],
        ) -> SendFuture<'a, SendOutcome> {
            Box::pin(async move {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                if self.offline.load(Ordering::SeqCst) {
                    return Err(BackendError::ConnectionRefused);
                }
                self.sent.lock().expect("sent").push(message.to_vec());
                Ok(SendOutcome::Accepted(SmtpReply {
                    code: 250,
                    enhanced: Some("2.0.0".into()),
                    text: "queued".into(),
                }))
            })
        }

        fn verify<'a>(&'a self, _config: &'a ConnectConfig) -> SendFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn limits<'a>(&'a self, _config: &'a ConnectConfig) -> SendFuture<'a, SmtpLimits> {
            Box::pin(async move {
                if self.offline.load(Ordering::SeqCst) {
                    return Err(BackendError::ConnectionRefused);
                }
                Ok(SmtpLimits {
                    max_message_bytes: None,
                })
            })
        }
    }

    fn settings() -> AccountSettings {
        let server = |host: &str, port| ServerSettings {
            host: host.into(),
            port,
            security: Security::Tls,
            username: "alice".into(),
        };
        AccountSettings {
            email: "alice@fake.test".into(),
            display_name: "Alice".into(),
            imap: server("imap.fake.test", 993),
            smtp: server("smtp.fake.test", 465),
        }
    }

    struct Bed {
        dir: TempDir,
        server: Arc<FakeServer>,
        keychain: Arc<MemorySecretStore>,
        sender: Arc<RecordingSender>,
        core: AppCore<FakeConnector>,
        events: broadcast::Receiver<CoreEvent>,
        account: AccountId,
    }

    impl Bed {
        /// One account with writes on, no undo delay; the fake server has a Drafts folder.
        async fn new() -> Self {
            Self::on(drafts_server()).await
        }

        /// Like [`Bed::new`] with this server; what lies on it is synced before this returns.
        async fn on(server: Arc<FakeServer>) -> Self {
            let dir = TempDir::new().expect("temp dir");
            let mut bed = Self::start(dir, server, Arc::default(), Arc::default()).await;
            bed.account = bed
                .core
                .add_account(settings(), Secret::new(PASSWORD.into()))
                .await
                .expect("add account")
                .id;
            bed.core
                .set_writes_enabled(bed.account, true)
                .await
                .expect("writes on");
            bed.core
                .set_compose_settings(ComposeSettings {
                    send_delay_seconds: 0,
                    ..ComposeSettings::default()
                })
                .await
                .expect("no undo delay");
            bed.idle().await;
            bed
        }

        async fn start(
            dir: TempDir,
            server: Arc<FakeServer>,
            keychain: Arc<MemorySecretStore>,
            sender: Arc<RecordingSender>,
        ) -> Self {
            let mut services = CoreServices::with_connector(
                Credentials::new(Arc::clone(&keychain)),
                FakeConnector(Arc::clone(&server)),
            );
            let backoff = BackoffPolicy {
                base: Duration::from_millis(50),
                max: Duration::from_millis(300),
                jitter: 0.5,
            };
            services.sync = SyncConfig {
                backoff,
                poll_interval: Duration::from_secs(600),
                wake_check: Duration::from_millis(50),
                wake_threshold: Duration::from_secs(3_600 * 24 * 365),
                settings_sync_auto: false,
                ..SyncConfig::default()
            };
            services.outbox = Some(OutboxConfig {
                backoff,
                linger: Duration::from_millis(200),
                sender: SenderHandle(Arc::clone(&sender) as Arc<dyn MailSender>),
                compose: ComposeConfig {
                    draft_save_delay: Duration::from_secs(600),
                    sent_checks: vec![Duration::from_millis(50)],
                    contacts_delay: Duration::from_millis(50),
                },
                ..OutboxConfig::default()
            });
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
            let account = core
                .list_accounts()
                .await
                .ok()
                .and_then(|a| a.first().map(|a| a.id))
                .unwrap_or_else(|| "00000000-0000-4000-8000-000000000000".parse().expect("id"));
            Self {
                dir,
                server,
                keychain,
                sender,
                core,
                events,
                account,
            }
        }

        /// The same data, server, keychain and sender after quitting and starting again.
        async fn restart(self) -> Self {
            let Self {
                dir,
                server,
                keychain,
                sender,
                core,
                ..
            } = self;
            core.shutdown().await;
            drop(core);
            let mut bed = Self::start(dir, server, keychain, sender).await;
            bed.idle().await;
            bed
        }

        async fn idle(&mut self) {
            let deadline = Instant::now() + WAIT;
            loop {
                let accounts = self.core.list_accounts().await.expect("accounts");
                if accounts
                    .iter()
                    .any(|a| a.id == self.account && a.status.state == AccountState::Idle)
                {
                    return;
                }
                assert!(Instant::now() < deadline, "the account did not sync");
                let _ = tokio::time::timeout(Duration::from_millis(100), self.events.recv()).await;
            }
        }

        async fn until<T, F, Fut>(&self, what: &str, mut probe: F) -> T
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
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        async fn drafts_folder(&self) -> MailboxId {
            self.core
                .list_mailboxes(self.account)
                .await
                .expect("mailboxes")
                .into_iter()
                .find(|m| m.role == MailboxRole::Drafts)
                .expect("the Drafts folder")
                .id
        }

        fn on_server(&self, uid: Uid) -> bool {
            self.server.read(|s| {
                s.mailboxes
                    .get("Drafts")
                    .is_some_and(|mb| mb.messages.contains_key(&uid))
            })
        }

        /// Waits for the message with `subject` in the local Drafts folder.
        async fn synced(&self, subject: &str) -> MessageId {
            let folder = self.drafts_folder().await;
            self.until(&format!("{subject:?} in Drafts"), || async {
                self.core
                    .list_messages(folder, None, 50)
                    .await
                    .ok()?
                    .items
                    .into_iter()
                    .find(|i| i.subject == subject)
                    .map(|i| i.id)
            })
            .await
        }

        async fn settled(&self) {
            self.until("the outbox to settle", || async {
                let counts = self.core.outbox_counts(self.account).await.ok()?;
                (counts.pending == 0).then_some(())
            })
            .await;
        }

        async fn write(&self, subject: &str) -> Draft {
            let draft = self
                .core
                .create_draft(NewDraft::New {
                    account_id: self.account,
                    identity_id: None,
                    to: Vec::new(),
                })
                .await
                .expect("draft");
            self.core
                .save_draft(
                    draft.id.clone(),
                    DraftUpdate {
                        identity_id: draft.identity_id,
                        to: vec![Address {
                            name: Some("Bob".into()),
                            email: "bob@friends.example".into(),
                        }],
                        cc: Vec::new(),
                        bcc: Vec::new(),
                        subject: subject.to_owned(),
                        format: ComposeFormat::Html,
                        body_html: "<p>Hi</p>".into(),
                        body_text: String::new(),
                        include_quote: true,
                        quote_position: QuotePosition::Above,
                        include_signature: true,
                        signature_id: None,
                    },
                )
                .await
                .expect("save")
        }

        async fn outgoing(&self, operation: mail_model::OperationId) -> mail_model::OutgoingMail {
            self.core
                .list_outgoing(self.account, None, 100)
                .await
                .expect("outgoing")
                .into_iter()
                .find(|m| m.operation_id == operation)
                .expect("the send")
        }
    }

    /// A fake server with INBOX, Sent, Archive and Drafts (`\Drafts`).
    fn drafts_server() -> Arc<FakeServer> {
        let server = FakeServer::new();
        server.with(|s| {
            s.mailboxes.insert(
                "Drafts".into(),
                FakeMailbox {
                    uid_validity: 900,
                    uid_next: 1,
                    modseq: 1,
                    attributes: vec!["\\Drafts".into()],
                    messages: BTreeMap::new(),
                    changes: 0,
                },
            );
        });
        server
    }

    /// Puts `raw` into the fake Drafts folder (`\Draft \Seen`) in one step, so a sync never
    /// sees half of it. `serial` must be unique: the fake reports `<serial>@fake.test` as the
    /// Message-ID of the header, so the raw message must carry that one too.
    fn put_draft(server: &FakeServer, serial: u64, raw: Vec<u8>) -> Uid {
        server.with(|s| {
            let mb = s.mailboxes.get_mut("Drafts").expect("Drafts");
            let uid = mb.uid_next;
            mb.uid_next = uid + 1;
            mb.modseq += 1;
            mb.changes += 1;
            mb.messages.insert(
                uid,
                FakeMessage {
                    flags: Flags {
                        seen: true,
                        draft: true,
                        ..Flags::default()
                    },
                    modseq: mb.modseq,
                    serial,
                    raw: Some(raw),
                },
            );
            uid
        })
    }

    /// The attachment of the foreign draft, and the same bytes in base64.
    const AGENDA: &[u8] = b"%PDF-1.4\n% Agenda \xc3\xa4\xc3\xb6\xc3\xbc\n1 0 obj << >> endobj\n";
    const AGENDA_BASE64: &str = "JVBERi0xLjQKJSBBZ2VuZGEgw6TDtsO8CjEgMCBvYmogPDwgPj4gZW5kb2JqCg==";

    /// A draft as Thunderbird saves it: text and HTML (with a script and an event handler), an
    /// attachment, Cc; with or without a Message-ID.
    fn foreign_draft(subject: &str, message_id: Option<&str>) -> Vec<u8> {
        let id_line = message_id.map_or_else(String::new, |id| format!("Message-ID: <{id}>\r\n"));
        format!(
            "From: Alice <alice@fake.test>\r\n\
             To: Bob <bob@friends.example>\r\n\
             Cc: Carol <carol@work.example>\r\n\
             Subject: {subject}\r\n\
             {id_line}\
             Date: Mon, 23 Nov 2026 07:00:00 +0100\r\n\
             User-Agent: Mozilla Thunderbird\r\n\
             X-Mozilla-Draft-Info: internal/draft; vcard=0; receipt=0; DSN=0; uuencode=0\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"mixed\"\r\n\
             \r\n\
             --mixed\r\n\
             Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
             \r\n\
             --alt\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             \r\n\
             Hello Bob, the agenda for Monday.\r\n\
             --alt\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             \r\n\
             <html><body><p onclick=\"steal()\">Hello <b>Bob</b>, the agenda for Monday.</p>\
             <script>alert(1)</script></body></html>\r\n\
             --alt--\r\n\
             --mixed\r\n\
             Content-Type: application/pdf; name=\"Agenda.pdf\"\r\n\
             Content-Disposition: attachment; filename=\"Agenda.pdf\"\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             {AGENDA_BASE64}\r\n\
             --mixed--\r\n"
        )
        .into_bytes()
    }

    /// Stage 5: a draft of another mail program in the Drafts folder opens as a local draft:
    /// the fields, HTML through the allowlist, the attachment copied byte for byte. It keeps its
    /// Message-ID and takes the copy in the folder as its server copy (the first save replaces
    /// it); opening it again gives the same draft. Without a Message-ID it gets a fresh one and
    /// the foreign copy stays (it is not the app's to delete).
    #[tokio::test]
    async fn a_draft_of_another_program_opens_cleaned_and_keeps_its_message_id() {
        let server = drafts_server();
        let message_id = "7001@fake.test";
        let uid = put_draft(
            &server,
            7001,
            foreign_draft("Agenda for Monday", Some(message_id)),
        );
        let anonymous_uid = put_draft(&server, 7002, foreign_draft("Without an id", None));
        let bed = Bed::on(server).await;
        let store = bed.core.store_for_tests();
        let message = bed.synced("Agenda for Monday").await;

        let draft = bed
            .core
            .open_draft_from_message(message)
            .await
            .expect("open the foreign draft");
        assert_eq!(draft.account_id, bed.account);
        assert_eq!(draft.kind, DraftKind::New);
        assert_eq!(draft.state, DraftState::Editing);
        assert_eq!(draft.subject, "Agenda for Monday");
        assert_eq!(
            draft.to,
            vec![Address {
                name: Some("Bob".into()),
                email: "bob@friends.example".into()
            }]
        );
        assert_eq!(
            draft.cc,
            vec![Address {
                name: Some("Carol".into()),
                email: "carol@work.example".into()
            }]
        );
        let identities = bed.core.list_identities(bed.account).await.expect("ids");
        assert_eq!(
            draft.identity_id,
            identities.iter().find(|i| i.is_default).map(|i| i.id),
            "the identity of the From address"
        );
        assert_eq!(draft.format, ComposeFormat::Html);
        assert!(draft.body_html.contains("Hello"), "{}", draft.body_html);
        assert!(
            draft.body_html.contains("<strong>Bob</strong>"),
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
        // The signature is part of the foreign text; there is no quote to add.
        assert!(!draft.include_signature);
        assert!(!draft.include_quote);
        assert_eq!(draft.attachments.len(), 1);
        let attachment = &draft.attachments[0];
        assert_eq!(attachment.file_name, "Agenda.pdf");
        assert_eq!(attachment.mime, "application/pdf");
        assert_eq!(attachment.size, AGENDA.len() as u64);
        // The copy in the folder is this draft's revision 1.
        assert_eq!(draft.server, DraftServerState::Saved);

        let stored = store
            .draft(draft.id.clone())
            .await
            .expect("store")
            .expect("stored draft");
        assert_eq!(stored.message_id_header, message_id, "the Message-ID stays");
        let copy = stored.server_copy.clone().expect("the folder copy is ours");
        assert_eq!(copy.uid, uid);
        assert_eq!(copy.message_id_header, message_id);
        assert_eq!(copy.mailbox.mailbox_id, bed.drafts_folder().await);
        assert_eq!(stored.attachments[0].sha256, files::sha256_hex(AGENDA));
        let root = files::compose_dir(store);
        assert_eq!(
            files::read(&root, &stored.attachments[0].file).expect("copied file"),
            AGENDA,
            "the attachment was copied into the draft"
        );

        // Opening it again is the same draft, not a second one.
        let again = bed
            .core
            .open_draft_from_message(message)
            .await
            .expect("open again");
        assert_eq!(again.id, draft.id);
        assert_eq!(
            bed.core
                .list_drafts(Some(bed.account))
                .await
                .expect("drafts")
                .len(),
            1
        );

        // Without a Message-ID: a new one in the sender's domain, no server copy.
        let anonymous = bed.synced("Without an id").await;
        let other = bed
            .core
            .open_draft_from_message(anonymous)
            .await
            .expect("open the draft without an id");
        assert_ne!(other.id, draft.id);
        assert_eq!(other.subject, "Without an id");
        assert_eq!(other.attachments.len(), 1);
        assert_eq!(other.server, DraftServerState::Pending);
        let stored_other = store
            .draft(other.id.clone())
            .await
            .expect("store")
            .expect("stored draft");
        assert!(stored_other.server_copy.is_none());
        assert!(
            stored_other.message_id_header.ends_with("@fake.test")
                && stored_other.message_id_header != "7002@fake.test"
                && stored_other.message_id_header != message_id,
            "a fresh Message-ID: {}",
            stored_other.message_id_header
        );

        // Discarding: the copy known by its Message-ID goes, the anonymous foreign copy stays.
        bed.core
            .delete_draft(draft.id.clone())
            .await
            .expect("discard");
        bed.core
            .delete_draft(other.id.clone())
            .await
            .expect("discard the other");
        bed.until("the folder copy removed", || async {
            (!bed.on_server(uid)).then_some(())
        })
        .await;
        bed.settled().await;
        assert!(
            bed.on_server(anonymous_uid),
            "a copy without Message-ID is not removed"
        );
        assert!(
            bed.core
                .list_drafts(Some(bed.account))
                .await
                .expect("drafts")
                .is_empty()
        );
    }

    /// Stage 5: "send" without a delay while offline. Nothing is transmitted, the send waits
    /// queued with the connection error (also across a restart), and goes out exactly once
    /// when the network is back.
    #[tokio::test]
    async fn sending_at_once_while_offline_waits_and_goes_out_once_online() {
        let mut bed = Bed::new().await;
        bed.sender.offline.store(true, Ordering::SeqCst);
        let draft = bed.write("Offline right now").await;
        let mail = bed
            .core
            .send_draft(draft.id.clone(), "req-offline-now".into())
            .await
            .expect("send");
        let operation = mail.operation_id;
        let waiting = bed
            .until("a failed attempt", || async {
                let m = bed.outgoing(operation).await;
                (m.attempts >= 1 && m.error.is_some()).then_some(m)
            })
            .await;
        assert_eq!(waiting.state, SendState::Queued);
        assert_eq!(
            waiting.error.as_ref().map(|e| e.code),
            Some(ErrorCode::ConnectionRefused)
        );
        assert_eq!(bed.sender.sent_with("Offline right now"), 0);
        let frozen = bed.core.get_draft(draft.id.clone()).await.expect("draft");
        assert_eq!(frozen.state, DraftState::Sending, "frozen while queued");
        assert_eq!(frozen.outgoing, Some(operation));

        // Quit and start again, still offline: tried again, still queued, nothing sent.
        bed = bed.restart().await;
        let before = bed.sender.attempts();
        bed.until("an attempt after the restart", || async {
            (bed.sender.attempts() > before).then_some(())
        })
        .await;
        let after_restart = bed.outgoing(operation).await;
        assert!(
            matches!(after_restart.state, SendState::Queued | SendState::Sending),
            "{after_restart:?}"
        );
        assert_eq!(bed.sender.sent_with("Offline right now"), 0);

        // Online again: it goes out, once.
        bed.sender.offline.store(false, Ordering::SeqCst);
        bed.core.network_changed();
        bed.until("the send after going online", || async {
            (bed.outgoing(operation).await.state == SendState::Sent).then_some(())
        })
        .await;
        assert_eq!(bed.sender.sent_with("Offline right now"), 1);
        // Later wakes and backoff rounds send nothing more.
        bed.core.network_changed();
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(bed.sender.sent_with("Offline right now"), 1, "exactly once");
        let err = bed
            .core
            .get_draft(draft.id.clone())
            .await
            .expect_err("the draft is gone after sending");
        assert_eq!(err.code(), ErrorCode::NotFound);
    }
}
