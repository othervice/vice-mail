//! Stage 10 through `AppCore` with the fake IMAP server, a recording SMTP sender and a clock
//! the test moves: sending later (on time, offline at the time, the app off below and above an
//! hour, the three answers to a missed send), the SIZE limit, inline images, forwarding as an
//! attachment, pasted HTML.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mail_model::{
    AccountId, AccountSettings, AccountState, Address, BackendError, ComposeFormat,
    ComposeSettings, ConnectConfig, CoreEvent, Draft, DraftState, DraftUpdate, Envelope, ErrorCode,
    MailSender, MissedSendDecision, NewDraft, QuoteLanguage, QuotePosition, Secret, Security,
    SendFuture, SendOutcome, SendState, ServerSettings, SmtpLimits, SmtpReply,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::clock::{Clock, SystemClock};
use crate::credentials::{Credentials, MemorySecretStore};
use crate::outbox::{ComposeConfig, OutboxConfig, SenderHandle};
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

const WAIT: Duration = Duration::from_secs(15);
const MIN: i64 = 60 * 1000;

#[derive(Debug, Default)]
struct OffsetClock(AtomicI64);

impl Clock for OffsetClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst)
    }
}

/// Records what it submits; can be offline and announce a SIZE limit.
#[derive(Debug, Default)]
struct RecordingSender {
    sent: Mutex<Vec<Vec<u8>>>,
    offline: AtomicBool,
    attempts: AtomicI64,
    limit: Mutex<Option<u64>>,
}

impl RecordingSender {
    fn sent(&self) -> Vec<Vec<u8>> {
        self.sent.lock().expect("sent").clone()
    }

    fn sent_with(&self, subject: &str) -> Vec<Vec<u8>> {
        let needle = format!("Subject: {subject}\r\n");
        self.sent()
            .into_iter()
            .filter(|m| String::from_utf8_lossy(m).contains(&needle))
            .collect()
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
        let max_message_bytes = *self.limit.lock().expect("limit");
        Box::pin(async move {
            if self.offline.load(Ordering::SeqCst) {
                return Err(BackendError::ConnectionRefused);
            }
            Ok(SmtpLimits { max_message_bytes })
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
    clock: Arc<OffsetClock>,
    sender: Arc<RecordingSender>,
    core: AppCore<FakeConnector>,
    events: broadcast::Receiver<CoreEvent>,
    account: AccountId,
}

impl Bed {
    async fn new() -> Self {
        let server = FakeServer::new();
        let dir = TempDir::new().expect("temp dir");
        let mut bed =
            Self::start(dir, server, Arc::default(), Arc::default(), Arc::default()).await;
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
        clock: Arc<OffsetClock>,
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
        services.clock = Arc::clone(&clock) as Arc<dyn Clock>;
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
            clock,
            sender,
            core,
            events,
            account,
        }
    }

    /// The same data, server, keychain and sender after the app was off for `off_ms`.
    async fn restart(self, off_ms: i64) -> Self {
        let Self {
            dir,
            server,
            keychain,
            clock,
            sender,
            core,
            ..
        } = self;
        core.shutdown().await;
        drop(core);
        clock.0.fetch_add(off_ms, Ordering::SeqCst);
        let mut bed = Self::start(dir, server, keychain, clock, sender).await;
        bed.idle().await;
        bed
    }

    fn now(&self) -> i64 {
        self.clock.now_ms()
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

    async fn draft(&self, subject: &str, html: &str) -> Draft {
        let draft = self
            .core
            .create_draft(NewDraft::New {
                account_id: self.account,
                identity_id: None,
                to: Vec::new(),
            })
            .await
            .expect("draft");
        self.write(&draft, subject, html).await
    }

    async fn write(&self, draft: &Draft, subject: &str, html: &str) -> Draft {
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
                    body_html: html.to_owned(),
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

    async fn state_of(&self, operation: mail_model::OperationId) -> Option<SendState> {
        self.core
            .list_outgoing(self.account, None, 100)
            .await
            .expect("outgoing")
            .into_iter()
            .find(|m| m.operation_id == operation)
            .map(|m| m.state)
    }
}

fn date_header(raw: &[u8]) -> Option<i64> {
    mail_content::parse(raw).headers.date
}

#[tokio::test]
async fn a_scheduled_send_goes_out_on_time_and_not_before() {
    let bed = Bed::new().await;
    let draft = bed.draft("Later on time", "<p>See you</p>").await;
    let at = bed.now() + 1_500;
    let mail = bed
        .core
        .schedule_send(draft.id.clone(), "req-on-time".into(), at)
        .await
        .expect("schedule");
    assert_eq!(mail.state, SendState::Scheduled);
    assert!(mail.scheduled);
    assert_eq!(mail.send_at, at);
    let listed = bed.core.list_scheduled_sends().await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].operation_id, mail.operation_id);
    // Frozen like any send.
    let frozen = bed.core.get_draft(draft.id.clone()).await.expect("draft");
    assert_eq!(frozen.state, DraftState::Sending);

    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        bed.sender.sent_with("Later on time").is_empty(),
        "sent too early"
    );
    let sent = bed
        .until("the scheduled send", || async {
            let sent = bed.sender.sent_with("Later on time");
            (!sent.is_empty()).then_some(sent)
        })
        .await;
    assert!(bed.now() >= at);
    assert_eq!(sent.len(), 1);
    // Dated with the scheduled time, not the time of the click.
    assert_eq!(date_header(&sent[0]), Some(at / 1000));
    bed.until("sent", || async {
        (bed.state_of(mail.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    assert!(
        bed.core
            .list_scheduled_sends()
            .await
            .expect("list")
            .is_empty()
    );

    // A time in the past is refused, nothing is queued.
    let other = bed.draft("Past", "<p>x</p>").await;
    let err = bed
        .core
        .schedule_send(other.id.clone(), "req-past".into(), bed.now() - 1)
        .await
        .expect_err("past");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let other = bed.core.get_draft(other.id).await.expect("draft");
    assert_eq!(other.state, DraftState::Editing);
}

#[tokio::test]
async fn offline_at_the_scheduled_time_it_goes_out_once_online() {
    let bed = Bed::new().await;
    bed.sender.offline.store(true, Ordering::SeqCst);
    let draft = bed.draft("Later offline", "<p>Hi</p>").await;
    let at = bed.now() + 600;
    let mail = bed
        .core
        .schedule_send(draft.id.clone(), "req-offline".into(), at)
        .await
        .expect("schedule");
    // Due, tried, nothing transmitted: queued with the error, retried with backoff.
    let waiting = bed
        .until("a failed attempt", || async {
            bed.core
                .list_outgoing(bed.account, None, 10)
                .await
                .expect("outgoing")
                .into_iter()
                // The queued state between attempts, not the next attempt under load.
                .find(|m| {
                    m.operation_id == mail.operation_id
                        && m.attempts >= 1
                        && m.error.is_some()
                        && m.state == SendState::Queued
                })
        })
        .await;
    assert_eq!(waiting.state, SendState::Queued);
    assert_eq!(
        waiting.error.as_ref().map(|e| e.code),
        Some(ErrorCode::ConnectionRefused)
    );
    assert!(bed.sender.sent().is_empty());
    // Back online: it goes out, once.
    bed.sender.offline.store(false, Ordering::SeqCst);
    bed.core.network_changed();
    bed.until("the send after going online", || async {
        (bed.state_of(mail.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    assert_eq!(bed.sender.sent_with("Later offline").len(), 1);
}

#[tokio::test]
async fn the_app_off_at_the_scheduled_time_below_an_hour_sends_above_an_hour_asks() {
    let mut bed = Bed::new().await;
    let now = bed.now();
    // Due 10 minutes from now, missed by 2 h 20 min after the restart: asked about (three of
    // them, one per answer). Due in 2 hours, 30 minutes late after the restart: sent.
    let mut asked = Vec::new();
    for (n, subject) in ["Missed now", "Missed draft", "Missed later"]
        .iter()
        .enumerate()
    {
        let draft = bed.draft(subject, "<p>Hi</p>").await;
        let mail = bed
            .core
            .schedule_send(draft.id.clone(), format!("req-missed-{n}"), now + 10 * MIN)
            .await
            .expect("schedule");
        asked.push((draft.id, mail.operation_id));
    }
    let late_draft = bed.draft("Half an hour late", "<p>Hi</p>").await;
    let late = bed
        .core
        .schedule_send(late_draft.id.clone(), "req-late".into(), now + 120 * MIN)
        .await
        .expect("schedule");
    bed.events = bed.core.subscribe();

    // The app is off for two and a half hours.
    bed = bed.restart(150 * MIN).await;

    // Under an hour late: sent at the start, no question.
    bed.until("the send half an hour late", || async {
        (bed.state_of(late.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    assert_eq!(bed.sender.sent_with("Half an hour late").len(), 1);

    // Over an hour late: nothing sent, the user decides.
    let scheduled = bed.core.list_scheduled_sends().await.expect("list");
    for (_, op) in &asked {
        let mail = scheduled
            .iter()
            .find(|m| m.operation_id == *op)
            .unwrap_or_else(|| panic!("missed send {op} listed: {scheduled:#?}"));
        assert_eq!(mail.state, SendState::Missed);
        assert_eq!(
            mail.error.as_ref().map(|e| e.code),
            Some(ErrorCode::SendMissed)
        );
        assert_eq!(mail.attempts, 0);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    for subject in ["Missed now", "Missed draft", "Missed later"] {
        assert!(
            bed.sender.sent_with(subject).is_empty(),
            "{subject} was sent"
        );
    }

    // A plain retry does not count as a decision.
    let err = bed
        .core
        .retry_operation(asked[0].1)
        .await
        .expect_err("needs a decision");
    assert_eq!(err.code(), ErrorCode::ConfirmationRequired);

    // Send now.
    let (_, now_op) = &asked[0];
    let mail = bed
        .core
        .resolve_missed_send(*now_op, MissedSendDecision::SendNow)
        .await
        .expect("send now")
        .expect("still a send");
    assert!(!mail.scheduled);
    bed.until("send now", || async {
        (bed.state_of(*now_op).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    let sent = bed.sender.sent_with("Missed now");
    assert_eq!(sent.len(), 1);
    // Dated when it really went out, not at the missed time.
    let dated = date_header(&sent[0]).expect("date");
    assert!(dated * 1000 >= bed.now() - MIN, "{dated}");

    // Keep as draft: editable again, nothing sent.
    let (draft_id, draft_op) = &asked[1];
    let none = bed
        .core
        .resolve_missed_send(*draft_op, MissedSendDecision::KeepAsDraft)
        .await
        .expect("keep");
    assert!(none.is_none());
    let draft = bed.core.get_draft(draft_id.clone()).await.expect("draft");
    assert_eq!(draft.state, DraftState::Editing);
    assert!(draft.outgoing.is_none());

    // Schedule again: waits for the new time, then goes out.
    let (_, later_op) = &asked[2];
    let at = bed.now() + 800;
    let mail = bed
        .core
        .resolve_missed_send(*later_op, MissedSendDecision::Reschedule { send_at: at })
        .await
        .expect("reschedule")
        .expect("still a send");
    assert_eq!(mail.state, SendState::Scheduled);
    assert_eq!(mail.send_at, at);
    bed.until("the rescheduled send", || async {
        (bed.state_of(*later_op).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    let sent = bed.sender.sent_with("Missed later");
    assert_eq!(sent.len(), 1);
    assert_eq!(date_header(&sent[0]), Some(at / 1000));
    assert!(bed.sender.sent_with("Missed draft").is_empty());

    // A second answer to the same question is refused.
    let err = bed
        .core
        .resolve_missed_send(*later_op, MissedSendDecision::SendNow)
        .await
        .expect_err("decided already");
    assert_eq!(err.code(), ErrorCode::SendInProgress);
}

#[tokio::test]
async fn a_scheduled_send_can_be_moved_or_cancelled_until_it_starts() {
    let bed = Bed::new().await;
    let identity = bed
        .core
        .list_compose_identities(bed.account)
        .await
        .expect("identities")[0]
        .id;
    bed.core
        .set_identity_signature(identity, Some("Alice from the tests".into()))
        .await
        .expect("signature");
    let draft = bed.draft("Moved", "<p>Hi</p>").await;
    let mail = bed
        .core
        .schedule_send(draft.id.clone(), "req-move".into(), bed.now() + 60 * MIN)
        .await
        .expect("schedule");
    let at = bed.now() + 700;
    let moved = bed
        .core
        .reschedule_send(mail.operation_id, Some(at))
        .await
        .expect("move");
    assert_eq!(moved.send_at, at);
    assert!(moved.scheduled);
    bed.until("the moved send", || async {
        (bed.state_of(mail.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    let sent = bed.sender.sent_with("Moved");
    assert_eq!(sent.len(), 1);
    assert_eq!(date_header(&sent[0]), Some(at / 1000));
    // Rebuilt for the new time with everything the draft had, the signature too.
    let text = mail_content::parse(&sent[0]).text.expect("text");
    assert!(text.contains("-- \nAlice from the tests"), "{text}");
    // Sent: can no longer be moved.
    let err = bed
        .core
        .reschedule_send(mail.operation_id, Some(bed.now() + MIN))
        .await
        .expect_err("sent");
    assert_eq!(err.code(), ErrorCode::SendInProgress);

    // Cancelled: back to the draft, nothing sent.
    let other = bed.draft("Cancelled later", "<p>Hi</p>").await;
    bed.core
        .schedule_send(other.id.clone(), "req-cancel".into(), bed.now() + 60 * MIN)
        .await
        .expect("schedule");
    let back = bed.core.cancel_send(other.id).await.expect("cancel");
    assert_eq!(back.state, DraftState::Editing);
    assert!(
        bed.core
            .list_scheduled_sends()
            .await
            .expect("list")
            .is_empty()
    );
    assert!(bed.sender.sent_with("Cancelled later").is_empty());
}

#[tokio::test]
async fn the_server_size_limit_warns_and_refuses_too_large_messages() {
    let bed = Bed::new().await;
    let dir = TempDir::new().expect("dir");
    let file = dir.path().join("big.bin");
    std::fs::write(&file, vec![7u8; 6_000]).expect("file");
    let draft = bed.draft("Big", "<p>Attached</p>").await;
    bed.core
        .add_draft_attachments(draft.id.clone(), vec![file])
        .await
        .expect("attach");

    // Offline: the limit is unknown, sending is not blocked by it.
    bed.sender.offline.store(true, Ordering::SeqCst);
    let check = bed
        .core
        .check_draft_size(draft.id.clone())
        .await
        .expect("check");
    assert!(!check.limit_known);
    assert!(!check.exceeds_server_limit);
    assert!(check.size > 8_000, "base64 makes it larger: {}", check.size);
    bed.sender.offline.store(false, Ordering::SeqCst);

    // The server announces 5,000 bytes.
    *bed.sender.limit.lock().expect("limit") = Some(5_000);
    let check = bed
        .core
        .check_draft_size(draft.id.clone())
        .await
        .expect("check");
    assert!(check.limit_known);
    assert_eq!(check.server_limit, Some(5_000));
    assert!(check.exceeds_server_limit);
    assert!(!check.large);
    let err = bed
        .core
        .send_draft(draft.id.clone(), "req-big".into())
        .await
        .expect_err("too large");
    assert_eq!(err.code(), ErrorCode::MessageTooLarge);
    let draft = bed.core.get_draft(draft.id).await.expect("draft");
    assert_eq!(draft.state, DraftState::Editing, "nothing queued");
    assert!(bed.sender.sent().is_empty());

    // A small message fits.
    let small = bed.draft("Small", "<p>Hi</p>").await;
    let check = bed.core.check_draft_size(small.id).await.expect("check");
    assert!(!check.exceeds_server_limit, "{check:?}");
}

/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[tokio::test]
async fn inline_images_are_copied_checked_and_sent_as_related_parts() {
    let bed = Bed::new().await;
    let dir = TempDir::new().expect("dir");
    let png = dir.path().join("chart.png");
    std::fs::write(&png, PNG).expect("png");
    let fake = dir.path().join("evil.png");
    std::fs::write(&fake, b"<svg onload=alert(1)>").expect("fake");
    let draft = bed.draft("Inline", "<p>x</p>").await;

    let err = bed
        .core
        .add_inline_images(draft.id.clone(), vec![fake])
        .await
        .expect_err("not an image");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);

    let added = bed
        .core
        .add_inline_images(draft.id.clone(), vec![png.clone()])
        .await
        .expect("add");
    assert_eq!(added.added.len(), 1);
    let image = &added.added[0];
    let cid = image.content_id.clone().expect("content id");
    assert!(cid.ends_with("@fake.test"), "{cid}");
    assert_eq!(image.mime, "image/png");
    // The core copied the file: removing the original changes nothing.
    std::fs::remove_file(&png).expect("remove original");
    let data = bed
        .core
        .inline_image_data(draft.id.clone(), image.id.clone())
        .await
        .expect("data");
    assert!(data.starts_with("data:image/png;base64,"), "{data}");

    let html = format!("<p>The chart:</p><p><img src=\"cid:{cid}\" width=\"240\"></p>");
    let draft = bed.write(&added.draft, "Inline", &html).await;
    let check = bed
        .core
        .check_draft_size(draft.id.clone())
        .await
        .expect("size");
    assert!(check.size > 0);
    let mail = bed
        .core
        .send_draft(draft.id.clone(), "req-inline".into())
        .await
        .expect("send");
    bed.until("the inline send", || async {
        (bed.state_of(mail.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    let raw = bed.sender.sent_with("Inline").pop().expect("sent");
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(text.contains("multipart/related"), "{text}");
    assert!(text.contains(&format!("Content-ID: <{cid}>")), "{text}");
    let parsed = mail_content::parse(&raw);
    assert_eq!(parsed.inline.len(), 1);
    assert_eq!(parsed.inline[0].content_id, cid);
    assert_eq!(&*parsed.inline[0].bytes, PNG);
    assert!(
        parsed.html.expect("html").contains("width=\"240\""),
        "the size the user chose"
    );
}

#[tokio::test]
async fn a_message_is_forwarded_as_an_attachment() {
    let mut bed = Bed::new().await;
    let original = b"From: Carol <carol@work.example>\r\nTo: alice@fake.test\r\n\
Subject: Contract draft\r\nMessage-ID: <contract-1@work.example>\r\n\
Date: Mon, 23 Nov 2026 08:00:00 +0000\r\nContent-Type: text/plain\r\n\r\nPlease review.\r\n";
    bed.server.deliver_raw("INBOX", original);
    bed.core.retry_account(bed.account).await.expect("sync");
    let inbox = bed
        .core
        .list_mailboxes(bed.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == mail_model::MailboxRole::Inbox)
        .expect("inbox")
        .id;
    let item = bed
        .until("the original", || async {
            bed.core
                .list_messages(inbox, None, 50)
                .await
                .ok()?
                .items
                .into_iter()
                .find(|i| i.subject == "Contract draft")
        })
        .await;
    bed.idle().await;
    let draft = bed
        .core
        .create_draft(NewDraft::ForwardAsAttachment {
            message_id: item.id,
            language: QuoteLanguage::En,
        })
        .await
        .expect("forward");
    assert!(draft.quote_html.is_none() && draft.quote_text.is_none());
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].mime, "message/rfc822");
    assert_eq!(draft.attachments[0].file_name, "Contract draft.eml");
    assert!(
        draft.subject.contains("Contract draft"),
        "{}",
        draft.subject
    );
    let draft = bed
        .write(&draft, &draft.subject.clone(), "<p>FYI</p>")
        .await;
    let mail = bed
        .core
        .send_draft(draft.id.clone(), "req-fwd".into())
        .await
        .expect("send");
    bed.until("the forward", || async {
        (bed.state_of(mail.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    let raw = bed.sender.sent().pop().expect("sent");
    let parsed = mail_content::parse(&raw);
    let part = parsed
        .attachments
        .iter()
        .find(|a| a.mime == "message/rfc822")
        .unwrap_or_else(|| panic!("{:?}", parsed.attachments));
    let inner = mail_content::attachment_bytes(&raw, part.part_id).expect("bytes");
    let inner = mail_content::parse(&inner);
    assert_eq!(
        inner.headers.message_id.as_deref(),
        Some("contract-1@work.example")
    );
    assert!(String::from_utf8_lossy(&raw).contains("Content-Transfer-Encoding: 7bit"));
}

#[tokio::test]
async fn pasted_html_comes_back_clean() {
    let bed = Bed::new().await;
    let cleaned = bed
        .core
        .clean_pasted_html(
            "<p onclick=\"x()\">Hi <img src=\"https://t.example/p.gif\"><script>bad()</script>\
             <a href=\"https://ok.example\">link</a></p>"
                .into(),
        )
        .await
        .expect("clean");
    assert_eq!(
        cleaned.html,
        "<p>Hi <a href=\"https://ok.example/\">link</a></p>\n"
    );
    assert_eq!(cleaned.images_removed, 1);
    let err = bed
        .core
        .clean_pasted_html("x".repeat(3 << 20))
        .await
        .expect_err("too large");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn a_compose_request_fills_every_field_and_threads_the_reply() {
    let bed = Bed::new().await;
    let request = mail_model::ComposeRequest {
        to: vec![Address {
            name: Some("Bob".into()),
            email: "bob@friends.example".into(),
        }],
        cc: vec![Address {
            name: None,
            email: "carol@friends.example".into(),
        }],
        bcc: vec![Address {
            name: None,
            email: "dave@friends.example".into(),
        }],
        subject: Some("Re: Lunch".into()),
        body: Some("Hi Bob,\n\n<b>not bold</b> & more".into()),
        in_reply_to: Some("lunch-1@friends.example".into()),
        notices: Vec::new(),
    };
    let draft = bed
        .core
        .create_draft(NewDraft::Compose {
            account_id: bed.account,
            identity_id: None,
            request: request.clone(),
        })
        .await
        .expect("draft");
    assert_eq!(draft.to, request.to);
    assert_eq!(draft.cc, request.cc);
    assert_eq!(draft.bcc, request.bcc);
    assert_eq!(draft.subject, "Re: Lunch");
    assert!(
        draft
            .body_html
            .contains("&lt;b&gt;not bold&lt;/b&gt; &amp; more"),
        "{}",
        draft.body_html
    );
    let mail = bed
        .core
        .send_draft(draft.id.clone(), "req-compose".into())
        .await
        .expect("send");
    bed.until("the send", || async {
        (bed.state_of(mail.operation_id).await == Some(SendState::Sent)).then_some(())
    })
    .await;
    let raw = bed.sender.sent_with("Re: Lunch").pop().expect("sent");
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(
        text.contains("In-Reply-To: <lunch-1@friends.example>"),
        "{text}"
    );
    assert!(
        text.contains("References: <lunch-1@friends.example>"),
        "{text}"
    );
    assert!(
        text.contains("Cc: carol@friends.example") || text.contains("Cc: <carol@friends.example>"),
        "{text}"
    );
    assert!(
        !text.contains("dave@friends.example"),
        "no Bcc header in what is sent"
    );

    let mut bad = request;
    bad.in_reply_to = Some("not a message id".into());
    let err = bed
        .core
        .create_draft(NewDraft::Compose {
            account_id: bed.account,
            identity_id: None,
            request: bad,
        })
        .await
        .expect_err("refused");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
}

/// Stage 8 (decision 0025): new replies and forwards take the app-wide quote position; it is
/// stored with the other compose settings and survives a restart.
#[tokio::test]
async fn replies_and_forwards_start_with_the_default_quote_position() {
    let mut bed = Bed::new().await;
    assert_eq!(
        bed.core
            .get_compose_settings()
            .await
            .expect("settings")
            .default_quote_position,
        QuotePosition::Above,
        "the default keeps the behaviour before"
    );
    let original = b"From: Carol <carol@work.example>\r\nTo: alice@fake.test\r\n\
Subject: Quote test\r\nMessage-ID: <quote-1@work.example>\r\n\
Date: Mon, 23 Nov 2026 08:00:00 +0000\r\nContent-Type: text/plain\r\n\r\nOriginal text.\r\n";
    bed.server.deliver_raw("INBOX", original);
    bed.core.retry_account(bed.account).await.expect("sync");
    let inbox = bed
        .core
        .list_mailboxes(bed.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == mail_model::MailboxRole::Inbox)
        .expect("inbox")
        .id;
    let item = bed
        .until("the original", || async {
            bed.core
                .list_messages(inbox, None, 50)
                .await
                .ok()?
                .items
                .into_iter()
                .find(|i| i.subject == "Quote test")
        })
        .await;
    bed.idle().await;
    let reply = |id| NewDraft::Reply {
        message_id: id,
        language: QuoteLanguage::En,
    };
    let before = bed.core.create_draft(reply(item.id)).await.expect("reply");
    assert_eq!(before.quote_position, QuotePosition::Above);

    let stored = bed
        .core
        .set_compose_settings(ComposeSettings {
            send_delay_seconds: 0,
            default_quote_position: QuotePosition::Below,
            ..ComposeSettings::default()
        })
        .await
        .expect("store");
    assert_eq!(stored.default_quote_position, QuotePosition::Below);
    let reply = bed.core.create_draft(reply(item.id)).await.expect("reply");
    assert_eq!(reply.quote_position, QuotePosition::Below);
    let forward = bed
        .core
        .create_draft(NewDraft::Forward {
            message_id: item.id,
            language: QuoteLanguage::En,
        })
        .await
        .expect("forward");
    assert_eq!(forward.quote_position, QuotePosition::Below);
    // A new message has no quote; it keeps the type's default.
    let new = bed
        .core
        .create_draft(NewDraft::New {
            account_id: bed.account,
            identity_id: None,
            to: Vec::new(),
        })
        .await
        .expect("new");
    assert_eq!(new.quote_position, QuotePosition::Above);

    let bed = bed.restart(0).await;
    assert_eq!(
        bed.core.get_compose_settings().await.expect("settings"),
        ComposeSettings {
            send_delay_seconds: 0,
            default_quote_position: QuotePosition::Below,
            ..ComposeSettings::default()
        }
    );
}
