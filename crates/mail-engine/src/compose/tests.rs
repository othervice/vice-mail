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
            credential: Default::default(),
            protocol: Default::default(),
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

    // -----------------------------------------------------------------------------------------
    // Stage 16 (decision 0033): send and archive, read receipts (RFC 8098)

    /// Puts `raw` into `mailbox` (unread) in one step. As with [`put_draft`], `serial` must be
    /// unique and the raw message must carry `<serial>@fake.test` as its Message-ID.
    fn put_message(server: &FakeServer, mailbox: &str, serial: u64, raw: Vec<u8>) -> Uid {
        server.with(|s| {
            let mb = s.mailboxes.get_mut(mailbox).expect("mailbox");
            let uid = mb.uid_next;
            mb.uid_next = uid + 1;
            mb.modseq += 1;
            mb.changes += 1;
            mb.messages.insert(
                uid,
                FakeMessage {
                    flags: Flags::default(),
                    modseq: mb.modseq,
                    serial,
                    raw: Some(raw),
                },
            );
            uid
        })
    }

    fn question(serial: u64, subject: &str, receipt: bool) -> Vec<u8> {
        let request = if receipt {
            "Disposition-Notification-To: Bob <bob@friends.example>\r\n"
        } else {
            ""
        };
        format!(
            "Return-Path: <bob@friends.example>\r\n\
             From: Bob <bob@friends.example>\r\n\
             To: Alice <alice@fake.test>\r\n\
             Subject: {subject}\r\n\
             Message-ID: <{serial}@fake.test>\r\n\
             Date: Mon, 23 Nov 2026 08:00:00 +0100\r\n\
             {request}\
             MIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             \r\n\
             What do you think?\r\n"
        )
        .into_bytes()
    }

    fn count_in(server: &FakeServer, mailbox: &str) -> usize {
        server.read(|s| s.mailboxes.get(mailbox).map_or(0, |mb| mb.messages.len()))
    }

    impl Bed {
        async fn folder(&self, role: MailboxRole) -> MailboxId {
            self.core
                .list_mailboxes(self.account)
                .await
                .expect("mailboxes")
                .into_iter()
                .find(|m| m.role == role)
                .expect("the folder")
                .id
        }

        /// Waits for the message with `subject` in the local folder with `role`.
        async fn arrived(&self, role: MailboxRole, subject: &str) -> MessageId {
            let folder = self.folder(role).await;
            self.until(&format!("{subject:?} arrived"), || async {
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

        /// Opens the message in the reading pane until its body is stored.
        async fn opened(&self, message: MessageId) {
            self.until("the body", || async {
                let view = self
                    .core
                    .get_message(message, mail_model::BodyFormat::Text)
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

        fn submitted(&self) -> Vec<String> {
            self.sender
                .sent
                .lock()
                .expect("sent")
                .iter()
                .map(|m| String::from_utf8_lossy(m).into_owned())
                .collect()
        }

        async fn keywords(&self, message: MessageId) -> Vec<String> {
            self.core
                .get_messages_by_ids(vec![message])
                .await
                .expect("item")
                .into_iter()
                .flat_map(|i| i.keywords)
                .collect()
        }
    }

    /// Stage 16: "send and archive". The compose setting turns it on for new replies (not for
    /// new messages). A send that fails and is cancelled archives nothing; once the server
    /// accepted the reply, the original moves from the inbox to Archive through the outbox.
    #[tokio::test]
    async fn send_and_archive_moves_the_original_only_once_the_send_is_accepted() {
        let bed = Bed::new().await;
        bed.core
            .set_compose_settings(ComposeSettings {
                send_delay_seconds: 0,
                archive_on_reply: true,
                ..ComposeSettings::default()
            })
            .await
            .expect("settings");
        put_message(
            &bed.server,
            "INBOX",
            8101,
            question(8101, "Question", false),
        );
        let original = bed.arrived(MailboxRole::Inbox, "Question").await;
        let new = bed.write("Unrelated").await;
        assert!(!new.archive_original, "only replies start with it");
        let reply = bed
            .core
            .create_draft(NewDraft::Reply {
                message_id: original,
                language: mail_model::QuoteLanguage::En,
            })
            .await
            .expect("reply");
        assert!(reply.archive_original, "the compose setting");

        // The send fails (offline) and is cancelled: nothing is archived.
        bed.sender.offline.store(true, Ordering::SeqCst);
        let mail = bed
            .core
            .send_draft(reply.id.clone(), "req-archive-1".into())
            .await
            .expect("send");
        assert_eq!(mail.archive, Some(mail_model::ArchiveAfterSend::Waiting));
        bed.until("a failed attempt", || async {
            let m = bed.outgoing(mail.operation_id).await;
            (m.attempts >= 1 && m.error.is_some()).then_some(())
        })
        .await;
        // Between two attempts (one in flight cannot be cancelled).
        bed.until("the send cancelled", || async {
            bed.core.cancel_send(reply.id.clone()).await.ok()
        })
        .await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        bed.settled().await;
        assert_eq!(count_in(&bed.server, "INBOX"), 1);
        assert_eq!(count_in(&bed.server, "Archive"), 0);
        assert_eq!(
            bed.arrived(MailboxRole::Inbox, "Question").await,
            original,
            "still in the inbox"
        );

        // Accepted: the original goes to Archive afterwards.
        bed.sender.offline.store(false, Ordering::SeqCst);
        let mail = bed
            .core
            .send_draft(reply.id.clone(), "req-archive-2".into())
            .await
            .expect("send again");
        let sent = bed
            .until("the reply sent", || async {
                let m = bed.outgoing(mail.operation_id).await;
                (m.state == SendState::Sent).then_some(m)
            })
            .await;
        assert_eq!(sent.archive, Some(mail_model::ArchiveAfterSend::Queued));
        bed.until("the original in Archive", || async {
            (count_in(&bed.server, "Archive") == 1 && count_in(&bed.server, "INBOX") == 0)
                .then_some(())
        })
        .await;
        bed.settled().await;
        assert_eq!(bed.sender.sent_with("Re: Question"), 1);
        let archived = bed.arrived(MailboxRole::Archive, "Question").await;
        assert!(archived.0 > 0);
    }

    /// Stage 16: without an archive mailbox the send is still sent and says so; the original
    /// stays. The draft's read-receipt request goes out with the identity's address.
    #[tokio::test]
    async fn send_and_archive_without_an_archive_mailbox_keeps_the_original() {
        let server = drafts_server();
        server.with(|s| {
            s.mailboxes.remove("Archive");
        });
        let bed = Bed::on(server).await;
        put_message(&bed.server, "INBOX", 8201, question(8201, "Lunch?", false));
        let original = bed.arrived(MailboxRole::Inbox, "Lunch?").await;
        let reply = bed
            .core
            .create_draft(NewDraft::Reply {
                message_id: original,
                language: mail_model::QuoteLanguage::En,
            })
            .await
            .expect("reply");
        assert!(!reply.archive_original, "off by default");
        let reply = bed
            .core
            .set_draft_send_options(
                reply.id.clone(),
                mail_model::DraftSendOptions {
                    archive_original: true,
                    request_read_receipt: true,
                },
            )
            .await
            .expect("options");
        assert!(reply.archive_original && reply.request_read_receipt);
        let mail = bed
            .core
            .send_draft(reply.id.clone(), "req-no-archive".into())
            .await
            .expect("send");
        let sent = bed
            .until("sent", || async {
                let m = bed.outgoing(mail.operation_id).await;
                (m.state == SendState::Sent).then_some(m)
            })
            .await;
        assert_eq!(
            sent.archive,
            Some(mail_model::ArchiveAfterSend::NoArchiveMailbox)
        );
        bed.settled().await;
        assert_eq!(count_in(&bed.server, "INBOX"), 1, "the original stayed");
        let submitted = bed.submitted();
        assert_eq!(submitted.len(), 1);
        assert!(
            submitted[0].contains("Disposition-Notification-To: <alice@fake.test>")
                || submitted[0].contains("Disposition-Notification-To: alice@fake.test"),
            "{}",
            submitted[0]
        );
    }

    /// Stage 16 (acceptance): an incoming read-receipt request never leads to an SMTP
    /// submission without the user's explicit answer: not when the message arrives, is opened
    /// again, after a restart, nor with the setting `never` (the command is refused). Only
    /// `sendReadReceipt` sends one: a `multipart/report` disposition notification to the
    /// requested address, without a copy in Sent, and `$MDNSent` on the original; pressed
    /// again it sends nothing more.
    #[tokio::test]
    async fn a_read_receipt_request_never_sends_anything_without_the_users_answer() {
        let mut bed = Bed::new().await;
        put_message(
            &bed.server,
            "INBOX",
            8301,
            question(8301, "Please confirm", true),
        );
        let message = bed.arrived(MailboxRole::Inbox, "Please confirm").await;
        bed.opened(message).await;
        let info = bed.core.get_read_receipt(message).await.expect("info");
        let request = info.request.expect("a request");
        assert_eq!(request.to, "bob@friends.example");
        assert!(!request.return_path_differs);
        assert_eq!(request.answer, mail_model::ReadReceiptAnswer::Ask);
        assert!(!info.requested);

        tokio::time::sleep(Duration::from_millis(500)).await;
        bed.opened(message).await;
        bed = bed.restart().await;
        bed.opened(message).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(bed.sender.attempts(), 0, "no SMTP submission at all");

        bed.core
            .set_compose_settings(ComposeSettings {
                send_delay_seconds: 0,
                read_receipts: mail_model::ReadReceiptPolicy::Never,
                ..ComposeSettings::default()
            })
            .await
            .expect("never");
        let info = bed.core.get_read_receipt(message).await.expect("info");
        assert_eq!(
            info.request.map(|r| r.answer),
            Some(mail_model::ReadReceiptAnswer::Off)
        );
        let refused = bed
            .core
            .send_read_receipt(message, mail_model::QuoteLanguage::En, "req-mdn-0".into())
            .await
            .expect_err("refused with never");
        assert_eq!(refused.code(), ErrorCode::InvalidArgument);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(bed.sender.attempts(), 0);

        // The user's explicit answer.
        bed.core
            .set_compose_settings(ComposeSettings {
                send_delay_seconds: 0,
                ..ComposeSettings::default()
            })
            .await
            .expect("ask");
        let mail = bed
            .core
            .send_read_receipt(message, mail_model::QuoteLanguage::En, "req-mdn-1".into())
            .await
            .expect("send the receipt");
        assert!(mail.read_receipt);
        assert_eq!(mail.draft_id, None);
        bed.until("the receipt sent", || async {
            (bed.outgoing(mail.operation_id).await.state == SendState::Sent).then_some(())
        })
        .await;
        bed.settled().await;
        let submitted = bed.submitted();
        assert_eq!(submitted.len(), 1, "exactly one submission");
        let receipt = &submitted[0];
        for needed in [
            "Content-Type: multipart/report",
            "disposition-notification",
            "Content-Type: message/disposition-notification",
            "Final-Recipient: rfc822;alice@fake.test",
            "Original-Message-ID: <8301@fake.test>",
            "Disposition: manual-action/MDN-sent-manually; displayed",
            "Subject: Read: Please confirm",
        ] {
            assert!(receipt.contains(needed), "{needed} missing in {receipt}");
        }
        assert!(!receipt.contains("Disposition-Notification-To"));
        let info = bed.core.get_read_receipt(message).await.expect("info");
        assert_eq!(
            info.request.map(|r| r.answer),
            Some(mail_model::ReadReceiptAnswer::Sent)
        );
        assert!(
            bed.keywords(message)
                .await
                .iter()
                .any(|k| k.eq_ignore_ascii_case("$MDNSent")),
            "the original carries $MDNSent"
        );
        // Pressed again: the same receipt, nothing more sent; no copy in Sent.
        let again = bed
            .core
            .send_read_receipt(message, mail_model::QuoteLanguage::En, "req-mdn-2".into())
            .await
            .expect("again");
        assert_eq!(again.operation_id, mail.operation_id);
        tokio::time::sleep(Duration::from_millis(300)).await;
        bed.settled().await;
        assert_eq!(bed.sender.attempts(), 1);
        assert_eq!(count_in(&bed.server, "Sent"), 0);
    }

    /// Stage 16: declining sends nothing (and sets `$MDNSent`); a receipt that comes back is
    /// shown at the sent message it reports on, and the receipt itself knows what it is for.
    #[tokio::test]
    async fn declined_requests_send_nothing_and_received_receipts_show_at_the_sent_message() {
        let server = drafts_server();
        // A message the user sent with a request (synced at the start: Sent is polled).
        put_message(
            &server,
            "Sent",
            8402,
            b"From: Alice <alice@fake.test>\r\nTo: Bob <bob@friends.example>\r\n\
Subject: Offer\r\nMessage-ID: <8402@fake.test>\r\nDisposition-Notification-To: alice@fake.test\r\n\
Content-Type: text/plain\r\n\r\nThe offer.\r\n"
                .to_vec(),
        );
        let bed = Bed::on(server).await;
        put_message(
            &bed.server,
            "INBOX",
            8401,
            question(8401, "Confirm too", true),
        );
        let message = bed.arrived(MailboxRole::Inbox, "Confirm too").await;
        bed.opened(message).await;
        let info = bed
            .core
            .decline_read_receipt(message, "req-decline-1".into())
            .await
            .expect("decline");
        assert_eq!(
            info.request.map(|r| r.answer),
            Some(mail_model::ReadReceiptAnswer::Declined)
        );
        bed.settled().await;
        assert!(
            bed.keywords(message)
                .await
                .iter()
                .any(|k| k.eq_ignore_ascii_case("$MDNSent"))
        );
        assert_eq!(bed.sender.attempts(), 0);

        // The receipt that came back for the message the user sent with a request.
        put_message(
            &bed.server,
            "INBOX",
            8403,
            b"From: Bob <bob@friends.example>\r\nTo: alice@fake.test\r\n\
Subject: Return Receipt (displayed) - Offer\r\nMessage-ID: <8403@fake.test>\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/report; report-type=disposition-notification;\r\n\tboundary=\"b1\"\r\n\r\n\
--b1\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nThis is a Return Receipt.\r\n\
--b1\r\nContent-Type: message/disposition-notification\r\n\r\n\
Final-Recipient: rfc822;bob@friends.example\r\nOriginal-Message-ID: <8402@fake.test>\r\n\
Disposition: manual-action/MDN-sent-manually; displayed\r\n\r\n--b1--\r\n"
                .to_vec(),
        );
        let sent = bed.arrived(MailboxRole::Sent, "Offer").await;
        let receipt = bed
            .arrived(MailboxRole::Inbox, "Return Receipt (displayed) - Offer")
            .await;
        bed.opened(receipt).await;
        bed.opened(sent).await;
        let info = bed.core.get_read_receipt(sent).await.expect("sent info");
        assert!(info.requested);
        assert_eq!(info.request, None, "the user's own message asks nothing");
        assert_eq!(info.received.len(), 1);
        assert_eq!(
            info.received[0].recipient.as_deref(),
            Some("bob@friends.example")
        );
        assert_eq!(
            info.received[0].disposition,
            mail_model::ReceiptDisposition::Displayed
        );
        let about = bed
            .core
            .get_read_receipt(receipt)
            .await
            .expect("receipt info");
        assert_eq!(about.receipt_for.as_deref(), Some("8402@fake.test"));
        assert_eq!(about.request, None);
        assert_eq!(bed.sender.attempts(), 0, "still nothing sent");
    }

    // -----------------------------------------------------------------------------------------
    // Audit 24.11.2026: inline images of drafts written by other programs.

    /// A 1×1 PNG, as base64 in a message and as bytes.
    const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn png() -> Vec<u8> {
        let raw = format!(
            "Subject: x\r\nMIME-Version: 1.0\r\nContent-Type: image/png\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n{PNG_BASE64}\r\n"
        );
        let part = mail_content::parse(raw.as_bytes())
            .attachments
            .first()
            .map(|a| a.part_id)
            .expect("part");
        mail_content::attachment_bytes(raw.as_bytes(), part).expect("png bytes")
    }

    /// A draft as Apple Mail saves it: text, and HTML in `multipart/related` with an inline
    /// image (`cid`); `extra` goes into the HTML body.
    fn foreign_with_image(subject: &str, serial: u64, cid: &str, extra: &str) -> Vec<u8> {
        format!(
            "From: Alice <alice@fake.test>\r\n\
             To: Bob <bob@friends.example>\r\n\
             Subject: {subject}\r\n\
             Message-ID: <{serial}@fake.test>\r\n\
             Date: Mon, 23 Nov 2026 07:00:00 +0100\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
             \r\n\
             --alt\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             \r\n\
             Hello Bob, see the chart.\r\n\
             --alt\r\n\
             Content-Type: multipart/related; boundary=\"rel\"; type=\"text/html\"\r\n\
             \r\n\
             --rel\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             \r\n\
             <html><body><p>Hello Bob, see the chart:</p>\
             <p><img src=\"cid:{cid}\" width=\"120\" alt=\"Chart\"></p>{extra}</body></html>\r\n\
             --rel\r\n\
             Content-Type: image/png; name=\"chart.png\"\r\n\
             Content-Disposition: inline; filename=\"chart.png\"\r\n\
             Content-ID: <{cid}>\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             {PNG_BASE64}\r\n\
             --rel--\r\n\
             --alt--\r\n"
        )
        .into_bytes()
    }

    /// The image of a foreign draft stays an inline part from opening to sending: the editor
    /// shows it (`cid:` with the part's bytes behind it), it is no attachment, the draft takes
    /// over the copy in the folder, and the sent message carries it in `multipart/related`.
    #[tokio::test]
    async fn inline_images_of_a_foreign_draft_stay_inline_through_editing_and_sending() {
        let server = drafts_server();
        put_draft(
            &server,
            7101,
            foreign_with_image("Chart for Monday", 7101, "chart@other.example", ""),
        );
        let bed = Bed::on(server).await;
        let message = bed.synced("Chart for Monday").await;
        let draft = bed
            .core
            .open_draft_from_message(message)
            .await
            .expect("open");
        assert!(!draft.original_kept, "taken over completely");
        assert_eq!(
            draft.server,
            DraftServerState::Saved,
            "the folder copy is its own"
        );
        assert!(
            draft
                .body_html
                .contains("<img src=\"cid:chart@other.example\" alt=\"Chart\" width=\"120\">"),
            "{}",
            draft.body_html
        );
        assert_eq!(draft.attachments.len(), 1, "{:?}", draft.attachments);
        let image = &draft.attachments[0];
        assert_eq!(image.content_id.as_deref(), Some("chart@other.example"));
        assert_eq!(image.mime, "image/png");
        assert_eq!(image.file_name, "chart.png");
        assert_eq!(image.size, png().len() as u64);
        let shown = bed
            .core
            .inline_image_data(draft.id.clone(), image.id.clone())
            .await
            .expect("the editor's image");
        assert!(shown.starts_with("data:image/png;base64,"), "{shown}");

        // Edited around the image, then sent.
        let edited = format!("<p>Edited first.</p>{}", draft.body_html);
        bed.core
            .save_draft(
                draft.id.clone(),
                DraftUpdate {
                    identity_id: draft.identity_id,
                    to: draft.to.clone(),
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: draft.subject.clone(),
                    format: ComposeFormat::Html,
                    body_html: edited,
                    body_text: String::new(),
                    include_quote: false,
                    quote_position: QuotePosition::Above,
                    include_signature: false,
                    signature_id: None,
                },
            )
            .await
            .expect("save");
        bed.core
            .send_draft(draft.id.clone(), "req-inline-foreign".into())
            .await
            .expect("send");
        let sent = bed
            .until("the message submitted", || async {
                bed.sender
                    .sent
                    .lock()
                    .expect("sent")
                    .iter()
                    .find(|m| String::from_utf8_lossy(m).contains("Chart for Monday"))
                    .cloned()
            })
            .await;
        let text = String::from_utf8_lossy(&sent);
        assert!(text.contains("multipart/related"), "{text}");
        let parsed = mail_content::parse(&sent);
        let html = parsed.html.clone().unwrap_or_default();
        assert!(
            html.contains("Edited first.") && html.contains("cid:chart@other.example"),
            "{html}"
        );
        assert_eq!(parsed.inline.len(), 1);
        assert_eq!(parsed.inline[0].content_id, "chart@other.example");
        assert_eq!(&*parsed.inline[0].bytes, png().as_slice(), "the same image");
        assert!(
            parsed.attachments.iter().all(|a| a.inline),
            "no attachment besides the inline image: {:?}",
            parsed.attachments
        );
    }

    /// What the editor cannot keep (here an image from the web) leaves the original alone: the
    /// draft gets a Message-ID of its own and no server copy, says so, keeps the inline image
    /// it can show, and opening the original again finds the same draft. Discarding it leaves
    /// the original in the folder. An id another program writes (`ii_…`, no `@`) gets a new one.
    #[tokio::test]
    async fn a_foreign_draft_the_editor_cannot_keep_completely_leaves_its_original_alone() {
        let server = drafts_server();
        let uid = put_draft(
            &server,
            7102,
            foreign_with_image(
                "Logo from the web",
                7102,
                "ii_lk3j4h5g0",
                "<p><img src=\"https://cdn.example/logo.png\"></p>",
            ),
        );
        let bed = Bed::on(server).await;
        let store = bed.core.store_for_tests();
        let message = bed.synced("Logo from the web").await;
        let draft = bed
            .core
            .open_draft_from_message(message)
            .await
            .expect("open");
        assert!(draft.original_kept);
        assert_eq!(
            draft.server,
            DraftServerState::Pending,
            "saved next to the original"
        );
        let stored = store
            .draft(draft.id.clone())
            .await
            .expect("store")
            .expect("draft");
        assert!(stored.server_copy.is_none());
        assert_ne!(stored.message_id_header, "7102@fake.test");
        assert_eq!(stored.kept_original.as_deref(), Some("7102@fake.test"));
        // The inline image it can show stays, under a valid id of the sender's domain.
        let image = draft.attachments.first().expect("the inline image");
        let content_id = image.content_id.clone().expect("inline");
        assert!(
            content_id.ends_with("@fake.test") && mail_content::is_valid_content_id(&content_id),
            "{content_id}"
        );
        assert!(
            draft.body_html.contains(&format!("cid:{content_id}")),
            "{}",
            draft.body_html
        );
        assert!(!draft.body_html.contains("cdn.example"));

        let again = bed
            .core
            .open_draft_from_message(message)
            .await
            .expect("open again");
        assert_eq!(again.id, draft.id, "the same draft, not another copy");
        assert_eq!(
            bed.core
                .list_drafts(Some(bed.account))
                .await
                .expect("drafts")
                .len(),
            1
        );
        bed.core
            .delete_draft(draft.id.clone())
            .await
            .expect("discard");
        bed.settled().await;
        assert!(bed.on_server(uid), "the original is never removed");
    }
}
