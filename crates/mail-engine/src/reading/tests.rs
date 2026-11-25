//! Reading through `AppCore` against the in-memory fake server ([`crate::sync::fake`]) and the
//! real store: opening with and without a stored body, prefetch, offline, links, external
//! images (local counting HTTP server, test switch), attachments.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mail_model::{
    AccountId, AccountSettings, AccountState, BackendError, BodyFormat, BodyUnavailableReason,
    CoreEvent, ErrorCode, LinkWarning, MailboxId, MessageBody, MessageId, MessageListItem,
    MessageProblem, MessageView, OpenAttachmentResult, PrefetchPeriod, PrefetchSettings,
    PrefetchState, RemoteContentScope, RenderToken, Secret, Security, ServerSettings, TextSegment,
    parse_link_placeholder,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;
use tokio::time::Instant;

use super::{ReadingServices, RecordingOpener, RemoteConfig};
use crate::credentials::{Credentials, MemorySecretStore};
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

const WAIT: Duration = Duration::from_secs(10);

fn sync_config() -> SyncConfig {
    SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(50),
            max: Duration::from_millis(400),
            jitter: 0.5,
        },
        idle_section: Duration::from_secs(2),
        poll_interval: Duration::from_secs(600),
        inbox_poll_without_idle: Duration::from_secs(600),
        shutdown_grace: Duration::from_millis(500),
        ..SyncConfig::default()
    }
}

pub(super) struct Harness {
    _dir: TempDir,
    pub(super) core: AppCore<FakeConnector>,
    events: broadcast::Receiver<CoreEvent>,
    pub(super) opener: Arc<RecordingOpener>,
    pub(super) open_dir: PathBuf,
}

impl Harness {
    pub(super) async fn new(server: &Arc<FakeServer>, remote: RemoteConfig) -> Self {
        Self::with_helper(server, remote, None).await
    }

    /// With `preview_helper` set as the shell sets it (C-2, C-4).
    pub(super) async fn with_helper(
        server: &Arc<FakeServer>,
        remote: RemoteConfig,
        preview_helper: Option<PathBuf>,
    ) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let opener = Arc::new(RecordingOpener::default());
        let open_dir = dir.path().join("cache/attachments");
        let services = CoreServices {
            credentials: Credentials::new(MemorySecretStore::new()),
            connector: FakeConnector(Arc::clone(server)),
            clock: Arc::new(crate::clock::SystemClock),
            sync: sync_config(),
            reading: ReadingServices {
                opener: Arc::clone(&opener) as Arc<dyn super::SystemOpener>,
                remote,
                open_dir: Some(open_dir.clone()),
                preview_helper,
            },
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
            _dir: dir,
            core,
            events,
            opener,
            open_dir,
        }
    }

    async fn add(&mut self) -> AccountId {
        let id = self
            .core
            .add_account(settings(), Secret::new(PASSWORD.into()))
            .await
            .expect("add account")
            .id;
        self.event(|e| {
            matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
        })
        .await;
        id
    }

    async fn event(&mut self, pred: impl Fn(&CoreEvent) -> bool) -> CoreEvent {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            let event = tokio::time::timeout_at(deadline, self.events.recv())
                .await
                .unwrap_or_else(|_| panic!("no matching event within {WAIT:?}; saw {seen:#?}"))
                .expect("event stream");
            if pred(&event) {
                return event;
            }
            seen.push(event);
        }
    }

    async fn inbox(&self, account: AccountId) -> MailboxId {
        self.core
            .list_mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == "INBOX")
            .expect("inbox")
            .id
    }

    async fn items(&self, mailbox: MailboxId) -> Vec<MessageListItem> {
        self.core
            .list_messages(mailbox, None, 500)
            .await
            .expect("page")
            .items
    }

    /// Opens a message whose body is not stored and waits until it is.
    async fn open_fetched(&mut self, message: MessageId, format: BodyFormat) -> MessageView {
        let first = self.core.get_message(message, format).await.expect("get");
        if first.body != MessageBody::Loading {
            return first;
        }
        self.event(|e| {
            matches!(e, CoreEvent::MessageBodyAvailable { message_id, .. } if *message_id == message)
        })
        .await;
        self.core.get_message(message, format).await.expect("get")
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

fn text_of(view: &MessageView) -> String {
    match &view.body {
        MessageBody::Text { segments, .. } => segments
            .iter()
            .map(|s| match s {
                TextSegment::Text { text } | TextSegment::Link { text, .. } => text.as_str(),
            })
            .collect(),
        other => panic!("not a text body: {other:?}"),
    }
}

fn token_of(view: &MessageView) -> RenderToken {
    match &view.body {
        MessageBody::Html { render_token, .. } | MessageBody::Text { render_token, .. } => {
            render_token.clone()
        }
        other => panic!("no rendering: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_fetches_once_then_serves_locally_with_a_preview() {
    let server = FakeServer::new();
    server.deliver("INBOX", 3, false);
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let inbox = h.inbox(account).await;
    let items = h.items(inbox).await;
    assert!(
        items.iter().all(|i| i.preview.is_none()),
        "no previews before bodies"
    );
    let message = items[0].id;
    let before = server.read(|s| s.body_fetches);

    // Not stored: `loading` at once, and a second call starts no second fetch.
    let first = h
        .core
        .get_message(message, BodyFormat::Auto)
        .await
        .expect("get");
    assert_eq!(first.body, MessageBody::Loading);
    assert_eq!(first.header.subject, items[0].subject);
    assert_eq!(first.header.from.len(), 1);
    let second = h
        .core
        .get_message(message, BodyFormat::Auto)
        .await
        .expect("get");
    assert_eq!(second.body, MessageBody::Loading);
    h.event(|e| matches!(e, CoreEvent::MessageBodyAvailable { message_id, .. } if *message_id == message))
        .await;
    assert_eq!(
        server.read(|s| s.body_fetches) - before,
        1,
        "one fetch for two calls"
    );

    // Stored: served locally, no further fetch.
    let view = h
        .core
        .get_message(message, BodyFormat::Auto)
        .await
        .expect("get");
    assert_eq!(view.formats, [BodyFormat::Text]);
    assert!(text_of(&view).contains("Body of message"), "{view:?}");
    assert!(text_of(&view).contains("grüße"));
    assert!(view.problems.is_empty());
    assert!(!view.partial);
    let again = h
        .core
        .get_message(message, BodyFormat::Html)
        .await
        .expect("get");
    assert!(
        matches!(again.body, MessageBody::Text { .. }),
        "no HTML: text"
    );
    assert_ne!(
        token_of(&view),
        token_of(&again),
        "a new token per rendering"
    );
    assert_eq!(server.read(|s| s.body_fetches) - before, 1);

    // The list shows the preview now.
    let item = h
        .items(inbox)
        .await
        .into_iter()
        .find(|i| i.id == message)
        .expect("item");
    assert!(
        item.preview
            .as_deref()
            .is_some_and(|p| p.starts_with("Body of message")),
        "{:?}",
        item.preview
    );
    // Reading never writes to the server: every connection was read-only.
    assert!(server.read(|s| s.access.iter().all(|a| *a == mail_model::Access::ReadOnly)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_offline_and_vanished_messages_are_reported() {
    let server = FakeServer::new();
    let uids = server.deliver("INBOX", 3, false);
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let inbox = h.inbox(account).await;
    let items = h.items(inbox).await;

    // The server fails the fetch: reported once by messageBodyFailed with the error.
    server.with(|s| s.faults.bodies = Some(BackendError::Rejected("NO [UNAVAILABLE] busy".into())));
    let failing = items[0].id;
    let view = h
        .core
        .get_message(failing, BodyFormat::Auto)
        .await
        .expect("get");
    assert_eq!(view.body, MessageBody::Loading);
    let event = h
        .event(|e| matches!(e, CoreEvent::MessageBodyFailed { message_id, .. } if *message_id == failing))
        .await;
    let CoreEvent::MessageBodyFailed { error, .. } = event else {
        unreachable!()
    };
    assert_eq!(error.code, ErrorCode::ServerRejected);
    // "Try again" is the next getMessage: a new fetch, which succeeds now.
    server.with(|s| s.faults.bodies = None);
    let before = server.read(|s| s.body_fetches);
    let view = h.open_fetched(failing, BodyFormat::Auto).await;
    assert!(text_of(&view).contains("Body of message"));
    assert_eq!(server.read(|s| s.body_fetches) - before, 1, "one new fetch");

    // Gone from the server (expunged, not synced yet): the fetch reports not_found; trying
    // again fetches again.
    let oldest = items.last().expect("oldest").id;
    server.with(|s| {
        s.mailboxes
            .get_mut("INBOX")
            .expect("inbox")
            .messages
            .remove(&uids[0]);
    });
    for _ in 0..2 {
        assert_eq!(
            h.core
                .get_message(oldest, BodyFormat::Auto)
                .await
                .expect("get")
                .body,
            MessageBody::Loading
        );
        let gone = h
            .event(|e| matches!(e, CoreEvent::MessageBodyFailed { message_id, .. } if *message_id == oldest))
            .await;
        assert!(
            matches!(&gone, CoreEvent::MessageBodyFailed { error, .. } if error.code == ErrorCode::NotFound),
            "{gone:?}"
        );
    }
    // Once the sync removed the location, it is known at once: `notOnServer`.
    h.core.network_changed();
    h.event(|e| matches!(e, CoreEvent::MessagesChanged { .. }))
        .await;
    let mut gone_now = None;
    for _ in 0..50 {
        match h.core.get_message(oldest, BodyFormat::Auto).await {
            Ok(view) if matches!(view.body, MessageBody::Unavailable { .. }) => {
                gone_now = Some(view.body);
                break;
            }
            Err(err) if err.code() == ErrorCode::NotFound => {
                gone_now = Some(MessageBody::Unavailable {
                    reason: BodyUnavailableReason::NotOnServer,
                    error: None,
                });
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert_eq!(
        gone_now,
        Some(MessageBody::Unavailable {
            reason: BodyUnavailableReason::NotOnServer,
            error: None
        }),
        "a message without a location is not on the server (or already deleted)"
    );

    // Offline and not stored: `offline` at once, nothing is fetched.
    let middle = items[1].id;
    server.with(|s| s.faults.connect = Some(BackendError::ConnectionRefused));
    h.core.network_changed();
    h.event(|e| {
        matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Offline)
    })
    .await;
    let before = server.read(|s| s.body_fetches);
    assert_eq!(
        h.core
            .get_message(middle, BodyFormat::Auto)
            .await
            .expect("get")
            .body,
        MessageBody::Unavailable {
            reason: BodyUnavailableReason::Offline,
            error: None
        }
    );
    assert_eq!(server.read(|s| s.body_fetches), before);

    // Unknown message: not_found.
    let err = h
        .core
        .get_message(MessageId(1), BodyFormat::Auto)
        .await
        .expect_err("unknown");
    assert_eq!(err.code(), ErrorCode::NotFound);
}

/// Opens `message` (not stored) and returns how its fetch ended: `messageBodyAvailable` or
/// `messageBodyFailed`.
async fn fetch_outcome(h: &mut Harness, message: MessageId) -> CoreEvent {
    let view = h
        .core
        .get_message(message, BodyFormat::Auto)
        .await
        .expect("get");
    assert_eq!(view.body, MessageBody::Loading);
    h.event(|e| {
        matches!(e,
            CoreEvent::MessageBodyAvailable { message_id, .. }
            | CoreEvent::MessageBodyFailed { message_id, .. } if *message_id == message)
    })
    .await
}

fn failed_code(event: &CoreEvent) -> ErrorCode {
    match event {
        CoreEvent::MessageBodyFailed { error, .. } => error.code,
        other => panic!("expected messageBodyFailed, got {other:?}"),
    }
}

/// Waits until the sync went down and is idle again (after a server restart).
async fn sync_recovered(h: &mut Harness) {
    h.event(|e| {
        matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state != AccountState::Idle)
    })
    .await;
    h.event(|e| {
        matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
    })
    .await;
}

/// After a server restart the reading connection is dead, and it was used too recently for
/// the NOOP check. The next open runs once more on a new connection and succeeds without
/// `messageBodyFailed`. A connection that breaks again is reported after exactly one more
/// try; a refusal is reported at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_open_after_a_server_restart_reconnects_once() {
    let server = FakeServer::new();
    server.deliver("INBOX", 6, false);
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let inbox = h.inbox(account).await;
    let items = h.items(inbox).await;
    // The reading connection is open and was just used.
    h.open_fetched(items[0].id, BodyFormat::Auto).await;

    // The sync notices the restart at once (IDLE) and is back before the user opens the next
    // message; the reading connection is not checked before its first use.
    server.restart();
    sync_recovered(&mut h).await;
    let before = server.read(|s| s.body_fetches);
    let event = fetch_outcome(&mut h, items[1].id).await;
    assert!(
        matches!(event, CoreEvent::MessageBodyAvailable { .. }),
        "{event:?}"
    );
    // EXAMINE failed on the dead connection; the body came over the new one.
    assert_eq!(server.read(|s| s.body_fetches) - before, 1);
    let view = h
        .core
        .get_message(items[1].id, BodyFormat::Auto)
        .await
        .expect("get");
    assert!(text_of(&view).contains("Body of message"));

    // Every connection breaks on its first body fetch: two tries, then the error.
    let cases = [
        (
            BackendError::ConnectionLost("reset by peer".into()),
            ErrorCode::ConnectionLost,
            2,
        ),
        (
            BackendError::Timeout("no data for 30 s".into()),
            ErrorCode::ConnectionTimeout,
            2,
        ),
        (
            BackendError::Rejected("NO [UNAVAILABLE] busy".into()),
            ErrorCode::ServerRejected,
            1,
        ),
    ];
    for ((fault, code, fetches), item) in cases.into_iter().zip(&items[2..]) {
        server.with(|s| s.faults.bodies = Some(fault.clone()));
        let before = server.read(|s| s.body_fetches);
        let event = fetch_outcome(&mut h, item.id).await;
        assert_eq!(failed_code(&event), code, "{fault:?}");
        assert_eq!(
            server.read(|s| s.body_fetches) - before,
            fetches,
            "tries after {fault:?}"
        );
    }
    // "Try again" works once the server does.
    server.with(|s| s.faults.bodies = None);
    let event = fetch_outcome(&mut h, items[2].id).await;
    assert!(
        matches!(event, CoreEvent::MessageBodyAvailable { .. }),
        "{event:?}"
    );
}

/// A dead reading connection whose new connection is refused at login or by the certificate
/// check: reported with that error after one connect, never tried again on its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_and_certificate_errors_are_never_retried() {
    let server = FakeServer::new();
    server.deliver("INBOX", 4, false);
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let inbox = h.inbox(account).await;
    let items = h.items(inbox).await;

    let cases = [
        (
            BackendError::AuthFailed("[AUTHENTICATIONFAILED] Authentication failed.".into()),
            ErrorCode::AuthFailed,
        ),
        (
            BackendError::CertUntrusted("UnknownIssuer".into()),
            ErrorCode::CertUntrusted,
        ),
    ];
    for (n, (fault, code)) in cases.into_iter().enumerate() {
        // A fresh reading connection, then a restart; the sync reconnects before the fault.
        h.open_fetched(items[2 * n].id, BodyFormat::Auto).await;
        server.restart();
        sync_recovered(&mut h).await;
        server.with(|s| s.faults.connect = Some(fault.clone()));
        let (connects, fetches) = server.read(|s| (s.access.len(), s.body_fetches));
        let event = fetch_outcome(&mut h, items[2 * n + 1].id).await;
        assert_eq!(failed_code(&event), code, "{fault:?}");
        assert_eq!(
            server.read(|s| (s.access.len() - connects, s.body_fetches - fetches)),
            (1, 0),
            "one connect, no fetch after {fault:?}"
        );
        server.with(|s| s.faults.connect = None);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prefetch_stores_the_period_and_works_offline() {
    let server = FakeServer::new();
    server.deliver("INBOX", 120, false);
    server.deliver("Sent", 5, true);
    let flagged = server.deliver("Archive", 4, true);
    server.set_flagged("Archive", flagged[0], true);
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    // The fake's mail is from 2023: nothing in the default 30 days.
    assert_eq!(
        h.core
            .get_prefetch_status(account)
            .await
            .expect("status")
            .done,
        0
    );
    assert_eq!(server.read(|s| s.bodies_sent.len()), 0);

    let settings = PrefetchSettings {
        period: PrefetchPeriod::All,
        max_message_size: PrefetchSettings::DEFAULT_MAX_MESSAGE_SIZE,
    };
    assert_eq!(
        h.core.set_prefetch_settings(settings).await.expect("set"),
        settings
    );
    let too_big = PrefetchSettings {
        max_message_size: PrefetchSettings::MAX_MESSAGE_SIZE + 1,
        ..settings
    };
    assert_eq!(
        h.core
            .set_prefetch_settings(too_big)
            .await
            .map_err(|e| e.code()),
        Err(ErrorCode::InvalidArgument)
    );
    // Settings start a run; it reports progress and ends idle with everything stored.
    let mut running = false;
    let done = loop {
        let event = h
            .event(|e| matches!(e, CoreEvent::PrefetchProgress { .. }))
            .await;
        let CoreEvent::PrefetchProgress { status, .. } = event else {
            unreachable!()
        };
        running |= status.state == PrefetchState::Running;
        if status.state == PrefetchState::Idle {
            break status;
        }
    };
    assert!(running);
    // Inbox and Sent whole, from the Archive only the flagged message.
    assert_eq!((done.done, done.total), (126, 126));
    assert!(done.bytes > 0);
    assert_eq!(
        h.core.get_prefetch_status(account).await.expect("status"),
        done
    );
    assert_eq!(
        server.read(|s| s.bodies_sent.len()),
        126,
        "each body fetched once"
    );
    let inbox = h.inbox(account).await;
    let items = h.items(inbox).await;
    assert!(
        items.iter().all(|i| i.preview.is_some()),
        "previews for the list"
    );

    // Offline: prefetched mail opens; the account's prefetch is paused.
    server.with(|s| s.faults.connect = Some(BackendError::ConnectionRefused));
    h.core.network_changed();
    h.event(|e| matches!(e, CoreEvent::PrefetchProgress { status, .. } if status.state == PrefetchState::Paused))
        .await;
    for item in items.iter().take(20) {
        let view = h
            .core
            .get_message(item.id, BodyFormat::Auto)
            .await
            .expect("get");
        assert!(text_of(&view).contains("Body of message"));
    }
    let archive = h
        .core
        .list_mailboxes(account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.path == "Archive")
        .expect("archive");
    let not_prefetched = h
        .items(archive.id)
        .await
        .into_iter()
        .find(|i| !i.flagged)
        .expect("unflagged");
    assert_eq!(
        h.core
            .get_message(not_prefetched.id, BodyFormat::Auto)
            .await
            .expect("get")
            .body,
        MessageBody::Unavailable {
            reason: BodyUnavailableReason::Offline,
            error: None
        }
    );
    let unknown: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
    assert_eq!(
        h.core
            .get_prefetch_status(unknown)
            .await
            .map_err(|e| e.code()),
        Err(ErrorCode::NotFound)
    );
}

const HTML_MAIL: &str = "From: News <News@Shop.example>\r\nTo: alice@fake.test\r\n\
Subject: Links and pictures\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSee https://bank.example/login\r\n\
--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p><a href=\"https://evil.example/login\">https://bank.example/login</a>\
<a href=\"javascript:alert(1)\">run</a> <a href=\"mailto:help@shop.example\">mail us</a>\
<img src=\"http://127.0.0.1:PORT/pixel.png\" alt=\"pixel\"></p>\r\n--b--\r\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn links_are_checked_and_opened_only_through_the_core() {
    let server = FakeServer::new();
    server.deliver_raw("INBOX", HTML_MAIL.replace("PORT", "9").as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    let view = h.open_fetched(message, BodyFormat::Auto).await;
    assert_eq!(view.formats, [BodyFormat::Html, BodyFormat::Text]);
    let MessageBody::Html {
        srcdoc,
        render_token,
        blocked_remote,
        remote_loaded,
        ..
    } = &view.body
    else {
        panic!("html: {view:?}");
    };
    assert!(
        srcdoc.starts_with(&format!(
            "<!DOCTYPE html>\n<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"{}\">",
            mail_content::MAIL_CSP
        )),
        "the mandatory CSP comes first: {srcdoc}"
    );
    assert!(!srcdoc.contains("evil.example") && !srcdoc.contains("127.0.0.1"));
    assert_eq!((*blocked_remote, *remote_loaded), (1, false));
    // Every href is a placeholder of this rendering.
    let placeholders: Vec<(RenderToken, u32)> = srcdoc
        .split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(|href| parse_link_placeholder(href).expect("placeholder"))
        .collect();
    assert!(!placeholders.is_empty());
    assert!(placeholders.iter().all(|(t, _)| t == render_token));

    // The hook reports a click; only known renderings are announced.
    assert!(h.core.link_activated(render_token.clone(), 0));
    h.event(|e| matches!(e, CoreEvent::LinkActivated { index: 0, .. }))
        .await;
    assert!(!h.core.link_activated(RenderToken::from_random(7), 0));

    let first = h.core.describe_link(render_token, 0).expect("describe");
    assert_eq!(first.target, "https://evil.example/login");
    assert!(first.warnings.contains(&LinkWarning::DomainMismatch {
        shown_domain: "bank.example".into(),
        target_domain: "evil.example".into()
    }));
    assert_eq!(
        h.core.open_link(render_token, 0).await.expect("open"),
        mail_model::LinkOpened::Opened
    );
    let script = (0..3)
        .find(|i| {
            h.core
                .describe_link(render_token, *i)
                .is_ok_and(|l| l.scheme == "javascript")
        })
        .expect("javascript link");
    let err = h
        .core
        .open_link(render_token, script)
        .await
        .expect_err("not openable");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let mail = (0..3)
        .find(|i| {
            h.core
                .describe_link(render_token, *i)
                .is_ok_and(|l| l.scheme == "mailto")
        })
        .expect("mailto link");
    // Stage 10: a mailto link opens the app's own composer, not the system's mail app.
    let opened = h
        .core
        .open_link(render_token, mail)
        .await
        .expect("mailto opens");
    let mail_model::LinkOpened::Compose { request } = opened else {
        panic!("{opened:?}");
    };
    assert_eq!(request.to.len(), 1);
    assert_eq!(request.to[0].email, "help@shop.example");
    assert_eq!(h.opener.opened(), ["https://evil.example/login"]);
    let err = h
        .core
        .describe_link(&RenderToken::from_random(1), 0)
        .expect_err("unknown token");
    assert_eq!(err.code(), ErrorCode::NotFound);

    // The text view has its own token and the same check.
    let text = h
        .core
        .get_message(message, BodyFormat::Text)
        .await
        .expect("text");
    let link = match &text.body {
        MessageBody::Text { segments, .. } => segments
            .iter()
            .find_map(|s| match s {
                TextSegment::Link { index, .. } => Some(*index),
                TextSegment::Text { .. } => None,
            })
            .expect("a link"),
        other => panic!("{other:?}"),
    };
    let info = h
        .core
        .describe_link(&token_of(&text), link)
        .expect("text link");
    assert_eq!(info.target, "https://bank.example/login");
    assert!(info.warnings.is_empty());
}

/// Counts TCP connections and answers every request with a PNG.
pub(super) async fn counting_image_server() -> (u16, Arc<AtomicUsize>) {
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, b'I', b'D',
        b'A', b'T', 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50, 0x0f, 0, 0x04, 0x85, 0x01, 0x80,
        0x84, 0xa9, 0x8c, 0x21, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0_u8; 4096];
                let _ = socket.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    PNG.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(PNG).await;
            });
        }
    });
    (port, count)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_images_load_only_when_allowed() {
    let (port, connections) = counting_image_server().await;
    let server = FakeServer::new();
    server.deliver_raw(
        "INBOX",
        HTML_MAIL.replace("PORT", &port.to_string()).as_bytes(),
    );
    // The test switch: the counter server is on loopback.
    let mut h = Harness::new(
        &server,
        RemoteConfig {
            allow_loopback: true,
        },
    )
    .await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    let html = |view: &MessageView| match &view.body {
        MessageBody::Html {
            srcdoc,
            blocked_remote,
            remote_loaded,
            ..
        } => (srcdoc.clone(), *blocked_remote, *remote_loaded),
        other => panic!("{other:?}"),
    };

    let blocked = h.open_fetched(message, BodyFormat::Auto).await;
    let (srcdoc, count, loaded) = html(&blocked);
    assert_eq!((count, loaded), (1, false));
    assert!(!srcdoc.contains("data:image/png"));
    assert_eq!(
        connections.load(Ordering::SeqCst),
        0,
        "blocked means no connection"
    );

    // Once: loaded by the core, this rendering only.
    let once = h
        .core
        .load_remote_content(message, RemoteContentScope::Once)
        .await
        .expect("once");
    let (srcdoc, count, loaded) = html(&once);
    assert_eq!((count, loaded), (0, true));
    // The loaded image, encoded again by the core (C-4).
    assert!(
        srcdoc.contains("<img src=\"data:image/png;base64,")
            || srcdoc.contains("<img src=\"data:image/jpeg;base64,"),
        "{srcdoc}"
    );
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    assert_eq!(
        html(
            &h.core
                .get_message(message, BodyFormat::Auto)
                .await
                .expect("get")
        )
        .1,
        1
    );
    assert!(
        h.core
            .list_remote_content_senders(account)
            .await
            .expect("list")
            .is_empty()
    );

    // Always for the sender: stored normalised, later views load by themselves.
    h.core
        .load_remote_content(message, RemoteContentScope::Sender)
        .await
        .expect("sender");
    assert_eq!(
        h.core
            .list_remote_content_senders(account)
            .await
            .expect("list"),
        ["news@shop.example"]
    );
    let (_, count, loaded) = html(
        &h.core
            .get_message(message, BodyFormat::Auto)
            .await
            .expect("get"),
    );
    assert_eq!((count, loaded), (0, true));
    assert_eq!(connections.load(Ordering::SeqCst), 3);

    // Revoked: blocked again, no connection.
    h.core
        .revoke_remote_content(account, "News@Shop.example")
        .await
        .expect("revoke");
    let (_, count, loaded) = html(
        &h.core
            .get_message(message, BodyFormat::Auto)
            .await
            .expect("get"),
    );
    assert_eq!((count, loaded), (1, false));
    assert_eq!(connections.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_the_test_switch_loopback_images_never_load() {
    let (port, connections) = counting_image_server().await;
    let server = FakeServer::new();
    server.deliver_raw(
        "INBOX",
        HTML_MAIL.replace("PORT", &port.to_string()).as_bytes(),
    );
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    h.open_fetched(message, BodyFormat::Auto).await;
    let view = h
        .core
        .load_remote_content(message, RemoteContentScope::Once)
        .await
        .expect("once");
    let MessageBody::Html {
        blocked_remote,
        remote_loaded,
        srcdoc,
        ..
    } = &view.body
    else {
        panic!("{view:?}");
    };
    assert!(*remote_loaded, "the user asked");
    assert_eq!(*blocked_remote, 1, "but the private target stays blocked");
    assert!(!srcdoc.contains("data:image/png;base64,iVBOR"));
    assert_eq!(
        connections.load(Ordering::SeqCst),
        0,
        "no connection to loopback"
    );
}

/// 300 bytes that are not valid UTF-8, as base64 lines.
pub(super) fn binary_attachment() -> (Vec<u8>, String) {
    let bytes: Vec<u8> = (0..300_u32)
        .map(|i| u8::try_from((i * 7 + 3) % 256).expect("byte"))
        .collect();
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                encoded.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    let lines: Vec<String> = encoded
        .as_bytes()
        .chunks(76)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    (bytes, lines.join("\r\n"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_are_saved_exactly_and_opened_only_after_confirmation() {
    let (bytes, base64) = binary_attachment();
    let raw = format!(
        "From: a@fake.test\r\nTo: alice@fake.test\r\nSubject: Files\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
         --m\r\nContent-Type: text/plain\r\n\r\nTwo files.\r\n\
         --m\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"../../etc/Daten\u{202e}fdp.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{base64}\r\n\
         --m\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"setup.exe\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{base64}\r\n--m--\r\n"
    );
    let server = FakeServer::new();
    server.deliver_raw("INBOX", raw.as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;

    // Before the body is stored, asking for an attachment fetches it first (stage 20, decision
    // 0060: the attachments view lists bodies that may have left the cache since).
    assert_eq!(
        h.core
            .attachment_name(message, 0)
            .await
            .expect("fetched on demand"),
        "Datenfdp.bin"
    );

    let view = h.open_fetched(message, BodyFormat::Auto).await;
    assert_eq!(view.attachments.len(), 2);
    let data = &view.attachments[0];
    assert_eq!(
        data.name,
        h.core.attachment_name(message, 0).await.expect("name")
    );
    assert!(
        !data.name.contains(['/', '\\', '\u{202e}']),
        "{}",
        data.name
    );
    assert!(!data.executable && data.local);
    assert_eq!(data.size, 300);
    assert!(view.attachments[1].executable);

    // Saved: the same bytes as the original.
    let dir = TempDir::new().expect("dir");
    let target = dir.path().join(&data.name);
    let saved = h
        .core
        .save_attachment_to(message, 0, target.clone())
        .await
        .expect("save");
    assert_eq!(saved, target);
    assert_eq!(std::fs::read(&target).expect("read"), bytes);

    // Opening: executables need confirmation and nothing is written before it.
    assert_eq!(
        h.core
            .open_attachment(message, 1, false)
            .await
            .expect("open"),
        OpenAttachmentResult::NeedsConfirmation
    );
    assert!(h.opener.opened().is_empty());
    assert!(!h.open_dir.exists() || std::fs::read_dir(&h.open_dir).expect("dir").count() == 0);
    assert_eq!(
        h.core
            .open_attachment(message, 1, true)
            .await
            .expect("open"),
        OpenAttachmentResult::Opened
    );
    assert_eq!(
        h.core
            .open_attachment(message, 0, false)
            .await
            .expect("open"),
        OpenAttachmentResult::Opened
    );
    let opened = h.opener.opened();
    assert_eq!(opened.len(), 2);
    for path in &opened {
        let path = PathBuf::from(path);
        assert!(path.starts_with(&h.open_dir), "{}", path.display());
        assert!(
            std::fs::metadata(&path)
                .expect("meta")
                .permissions()
                .readonly()
        );
        assert_eq!(std::fs::read(&path).expect("read"), bytes);
    }
    let err = h
        .core
        .open_attachment(message, 9, true)
        .await
        .expect_err("index");
    assert_eq!(err.code(), ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_mail_never_fails_the_reading_pane() {
    let server = FakeServer::new();
    for raw in [
        &b"no header block at all, just text"[..],
        b"Subject: x\r\nContent-Type: multipart/mixed; boundary=\"z\"\r\n\r\n--z\r\nContent-Type: text/plain\r\n\r\ncut",
        b"Subject: y\r\nContent-Type: text/plain; charset=x-unknown\r\n\r\nGr\xfc\xdfe",
        b"",
    ] {
        server.deliver_raw("INBOX", raw);
    }
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let items = h.items(h.inbox(account).await).await;
    assert_eq!(items.len(), 4);
    let mut problems = Vec::new();
    for item in &items {
        for format in [BodyFormat::Auto, BodyFormat::Html, BodyFormat::Text] {
            let view = h.open_fetched(item.id, format).await;
            assert!(
                matches!(
                    view.body,
                    MessageBody::Text { .. } | MessageBody::Html { .. }
                ),
                "{view:?}"
            );
            problems.extend(view.problems);
        }
    }
    assert!(
        problems.contains(&MessageProblem::UnknownCharset),
        "{problems:?}"
    );
    assert!(
        problems.contains(&MessageProblem::NoContent),
        "{problems:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_the_account_stops_its_reading() {
    let server = FakeServer::new();
    server.deliver("INBOX", 2, false);
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    let view = h.open_fetched(message, BodyFormat::Auto).await;
    let token = token_of(&view);
    h.core.remove_account(account).await.expect("remove");
    assert_eq!(
        h.core.describe_link(&token, 0).map_err(|e| e.code()),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        h.core
            .get_message(message, BodyFormat::Auto)
            .await
            .map_err(|e| e.code()),
        Err(ErrorCode::NotFound)
    );
}

// ---------------------------------------------------------------------------------------------
// Stage 16 (decision 0036): preview, save all, pinned attachments

/// A 1×1 PNG.
const PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn part(mime: &str, name: &str, encoding: &str, body: &str) -> String {
    format!(
        "--m\r\nContent-Type: {mime}\r\nContent-Disposition: attachment; filename=\"{name}\"\r\n\
         Content-Transfer-Encoding: {encoding}\r\n\r\n{body}\r\n"
    )
}

fn with_parts(subject: &str, parts: &[String]) -> String {
    format!(
        "From: a@fake.test\r\nTo: alice@fake.test\r\nSubject: {subject}\r\n\
         Message-ID: <{}@fake.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
         --m\r\nContent-Type: text/plain\r\n\r\nSee the files.\r\n{}--m--\r\n",
        subject.replace(' ', "-"),
        parts.concat()
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_are_previewed_by_the_core_and_html_or_svg_never() {
    let raw = with_parts(
        "Preview",
        &[
            part("image/png", "pixel.png", "base64", PIXEL_PNG),
            part(
                "text/plain; charset=utf-8",
                "notes.txt",
                "8bit",
                "Grüße\r\nZeile 2",
            ),
            part(
                "text/html",
                "page.html",
                "7bit",
                "<script>alert(1)</script>",
            ),
            part(
                "image/svg+xml",
                "logo.svg",
                "7bit",
                "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
            ),
            part(
                "application/pdf",
                "broken.pdf",
                "7bit",
                "%PDF-1.4 not really",
            ),
            // Claims to be an image, is HTML: refused by its bytes.
            part(
                "image/png",
                "fake.png",
                "7bit",
                "<html><body>hi</body></html>",
            ),
        ],
    );
    let server = FakeServer::new();
    server.deliver_raw("INBOX", raw.as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    let view = h.open_fetched(message, BodyFormat::Auto).await;
    let kinds: Vec<Option<mail_model::PreviewKind>> =
        view.attachments.iter().map(|a| a.preview).collect();
    assert_eq!(
        kinds,
        vec![
            Some(mail_model::PreviewKind::Image),
            Some(mail_model::PreviewKind::Text),
            None,
            None,
            Some(mail_model::PreviewKind::Pdf),
            Some(mail_model::PreviewKind::Image),
        ]
    );

    let image = h
        .core
        .preview_attachment(message, 0, 0)
        .await
        .expect("image");
    let mail_model::AttachmentPreview::Image {
        data_url,
        width,
        height,
        scaled,
    } = image
    else {
        panic!("{image:?}");
    };
    assert_eq!((width, height, scaled), (1, 1, false));
    assert!(
        data_url.starts_with("data:image/png;base64,")
            || data_url.starts_with("data:image/jpeg;base64,"),
        "encoded by the core"
    );
    assert_ne!(
        data_url.trim_start_matches("data:image/png;base64,"),
        PIXEL_PNG,
        "never the attachment's own bytes"
    );

    assert_eq!(
        h.core
            .preview_attachment(message, 1, 0)
            .await
            .expect("text"),
        mail_model::AttachmentPreview::Text {
            text: "Grüße\nZeile 2".into(),
            truncated: false,
        }
    );
    for index in [2, 3] {
        let err = h
            .core
            .preview_attachment(message, index, 0)
            .await
            .expect_err("active content");
        assert_eq!(err.code(), ErrorCode::PreviewUnsupported, "{index}");
    }
    let err = h
        .core
        .preview_attachment(message, 4, 0)
        .await
        .expect_err("broken pdf");
    assert_eq!(err.code(), ErrorCode::PreviewDamaged);
    let err = h
        .core
        .preview_attachment(message, 5, 0)
        .await
        .expect_err("not an image");
    assert_eq!(err.code(), ErrorCode::PreviewUnsupported);
    let err = h
        .core
        .preview_attachment(message, 9, 0)
        .await
        .expect_err("index");
    assert_eq!(err.code(), ErrorCode::NotFound);
    assert!(h.opener.opened().is_empty(), "a preview opens nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn save_all_writes_every_attachment_and_never_replaces_a_file() {
    let (bytes, base64) = binary_attachment();
    let raw = with_parts(
        "Save all",
        &[
            part("text/plain", "report.txt", "7bit", "first"),
            part("text/plain", "report.txt", "7bit", "second"),
            part("application/octet-stream", "../data.bin", "base64", &base64),
        ],
    );
    let server = FakeServer::new();
    server.deliver_raw("INBOX", raw.as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    h.open_fetched(message, BodyFormat::Auto).await;

    let dir = TempDir::new().expect("dir");
    std::fs::write(dir.path().join("report.txt"), "mine").expect("existing file");
    let (saved, renamed) = h
        .core
        .save_all_attachments_to(message, dir.path().to_path_buf())
        .await
        .expect("save all");
    assert_eq!((saved, renamed), (3, 2));
    let read = |name: &str| std::fs::read(dir.path().join(name)).expect(name);
    assert_eq!(
        read("report.txt"),
        b"mine",
        "an existing file is never replaced"
    );
    assert_eq!(read("report (2).txt"), b"first");
    assert_eq!(read("report (3).txt"), b"second");
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    let data = names
        .iter()
        .find(|n| n.ends_with("data.bin"))
        .expect("the binary file, under a safe name");
    assert!(!data.contains(['/', '\\']), "{data}");
    assert_eq!(read(data), bytes);
    assert_eq!(names.len(), 4, "{names:?}");

    let missing = dir.path().join("gone");
    let err = h
        .core
        .save_all_attachments_to(message, missing)
        .await
        .expect_err("no such folder");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_are_pinned_listed_and_unpinned() {
    let raw = with_parts(
        "Pins",
        &[
            part("application/pdf", "Rechnung.pdf", "7bit", "%PDF-1.4"),
            part("text/plain", "notes.txt", "7bit", "hello"),
        ],
    );
    let server = FakeServer::new();
    server.deliver_raw("INBOX", raw.as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    h.open_fetched(message, BodyFormat::Auto).await;
    assert!(
        h.core
            .get_attachment_pins(message)
            .await
            .expect("pins")
            .is_empty()
    );

    h.core
        .set_attachment_pinned(message, 1, true)
        .await
        .expect("pin notes");
    h.event(|e| matches!(e, CoreEvent::InboxChanged { .. }))
        .await;
    h.core
        .set_attachment_pinned(message, 0, true)
        .await
        .expect("pin invoice");
    // Pinning twice changes nothing.
    h.core
        .set_attachment_pinned(message, 0, true)
        .await
        .expect("again");
    assert_eq!(
        h.core.get_attachment_pins(message).await.expect("pins"),
        vec![0, 1]
    );
    let pinned = h
        .core
        .list_pinned_attachments(mail_model::MarkScope::All)
        .await
        .expect("view");
    assert_eq!(pinned.len(), 2);
    let invoice = pinned.iter().find(|p| p.index == 0).expect("the invoice");
    assert_eq!(invoice.name, "Rechnung.pdf");
    assert_eq!(invoice.mime_type, "application/pdf");
    assert_eq!(invoice.preview, Some(mail_model::PreviewKind::Pdf));
    assert_eq!(invoice.message.id, message);
    assert!(pinned.iter().all(|p| p.message.subject == "Pins"));

    let err = h
        .core
        .set_attachment_pinned(message, 7, true)
        .await
        .expect_err("no such attachment");
    assert_eq!(err.code(), ErrorCode::NotFound);

    h.core
        .set_attachment_pinned(message, 1, false)
        .await
        .expect("unpin");
    assert_eq!(
        h.core.get_attachment_pins(message).await.expect("pins"),
        vec![0]
    );
    let scoped = h
        .core
        .list_pinned_attachments(mail_model::MarkScope::Account {
            account_id: account,
        })
        .await
        .expect("account view");
    assert_eq!(scoped.len(), 1);
    // Nothing went to the server.
    assert_eq!(server.read(|s| s.writer_attempts), 0);
}

/// A mail with an inline PNG, a broken inline GIF and a PNG written into the HTML as a `data:`
/// URL (finding C-4).
fn mail_with_images() -> String {
    format!(
        "From: News <news@shop.example>\r\nTo: alice@fake.test\r\nSubject: Pictures\r\n\
         Message-ID: <pictures@fake.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/related; boundary=\"r\"\r\n\r\n\
         --r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
         <p>Hello</p><img src=\"cid:logo@shop\" alt=\"logo\"><img src=\"cid:broken@shop\" alt=\"broken\">\
         <img src=\"data:image/png;base64,{PIXEL_PNG}\" alt=\"written\">\r\n\
         --r\r\nContent-Type: image/png\r\nContent-ID: <logo@shop>\r\nContent-Transfer-Encoding: base64\r\n\r\n\
         {PIXEL_PNG}\r\n\
         --r\r\nContent-Type: image/gif\r\nContent-ID: <broken@shop>\r\nContent-Transfer-Encoding: base64\r\n\r\n\
         R0lGODlhAQABAICAgICAgICA\r\n--r--\r\n"
    )
}

fn srcdoc_of(view: &MessageView) -> String {
    match &view.body {
        MessageBody::Html { srcdoc, .. } => srcdoc.clone(),
        other => panic!("{other:?}"),
    }
}

/// Finding C-4: the images of a mail reach the web view only encoded again by the core, a
/// broken one not at all, and a second view re-encodes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mail_images_are_shown_re_encoded_and_kept() {
    let server = FakeServer::new();
    server.deliver_raw("INBOX", mail_with_images().as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    let reading = Arc::clone(h.core.reading_ref().expect("reading"));
    let batches_before = reading.image_batches();
    let srcdoc = srcdoc_of(&h.open_fetched(message, BodyFormat::Auto).await);
    assert!(
        !srcdoc.contains(PIXEL_PNG),
        "never the mail's own bytes: {srcdoc}"
    );
    let images: Vec<&str> = srcdoc
        .split("<img ")
        .skip(1)
        .map(|img| img.split('>').next().unwrap_or(""))
        .collect();
    assert_eq!(images.len(), 3, "{srcdoc}");
    // The inline PNG and the written one (the same picture) show as the core's encoding.
    for shown in [images[0], images[2]] {
        assert!(
            shown.starts_with("src=\"data:image/png;base64,")
                || shown.starts_with("src=\"data:image/jpeg;base64,"),
            "{shown}"
        );
    }
    assert_eq!(images[1], "alt=\"broken\"", "the broken GIF has no source");
    assert_eq!(
        reading.image_batches(),
        batches_before + 1,
        "one batch per rendering"
    );

    // Shown again (and as text and HTML once more): nothing is re-encoded.
    let again = srcdoc_of(
        &h.core
            .get_message(message, BodyFormat::Auto)
            .await
            .expect("get"),
    );
    assert_eq!(again.split("<img ").count(), 4);
    assert_eq!(
        again.split("src=\"data:image/").count(),
        srcdoc.split("src=\"data:image/").count()
    );
    assert_eq!(
        reading.image_batches(),
        batches_before + 1,
        "kept in memory"
    );
}

/// Finding C-4: without a working helper the images are not shown, never decoded in the app
/// and never passed on as they are.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_working_helper_mail_images_are_not_shown() {
    let mut helpers = vec![PathBuf::from("/nonexistent/mail-mcp")];
    if cfg!(unix) {
        // Starts, answers nothing, ends with an error.
        helpers.push(PathBuf::from("/usr/bin/false"));
    }
    for helper in helpers {
        let server = FakeServer::new();
        server.deliver_raw("INBOX", mail_with_images().as_bytes());
        let mut h =
            Harness::with_helper(&server, RemoteConfig::default(), Some(helper.clone())).await;
        let account = h.add().await;
        let message = h.items(h.inbox(account).await).await[0].id;
        let srcdoc = srcdoc_of(&h.open_fetched(message, BodyFormat::Auto).await);
        assert!(
            srcdoc.contains("<p>Hello</p>"),
            "the mail is shown: {srcdoc}"
        );
        assert!(
            !srcdoc.contains("data:image/"),
            "{}: no image without the helper: {srcdoc}",
            helper.display()
        );
        assert_eq!(srcdoc.matches("<img alt=").count(), 3, "{srcdoc}");
    }
}

/// Finding C-4: the composer's images (draft inline images, signature pictures) go the same way:
/// re-encoded, never their own bytes; one that cannot be shown is `None`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editor_images_are_re_encoded_too() {
    let server = FakeServer::new();
    let h = Harness::new(&server, RemoteConfig::default()).await;
    let reading = Arc::clone(h.core.reading_ref().expect("reading"));
    let png = crate::compose::snippets::base64_decode(PIXEL_PNG).expect("base64");
    let original = mail_content::image_data_uri(&png).expect("a png");
    let shown = reading
        .editor_images(vec![png.clone().into(), b"GIF89a broken".to_vec().into()])
        .await;
    let first = shown[0].as_deref().expect("re-encoded");
    assert!(mail_content::is_reencoded_data_url(first), "{first}");
    assert_ne!(first, original, "never the original bytes");
    assert_eq!(shown[1], None);
}

/// Like [`counting_image_server`], recording the request path of every connection.
async fn path_recording_image_server() -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, b'I', b'D',
        b'A', b'T', 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50, 0x0f, 0, 0x04, 0x85, 0x01, 0x80,
        0x84, 0xa9, 0x8c, 0x21, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let paths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&paths);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buf = vec![0_u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                seen.lock().expect("paths").push(path);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    PNG.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(PNG).await;
            });
        }
    });
    (port, paths)
}

const TRACKER_MAIL: &str = "From: News <news@shop.example>\r\nTo: alice@fake.test\r\n\
Subject: Our offers\r\nMIME-Version: 1.0\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p><a href=\"https://shop.example/offer?id=7&amp;utm_source=news&amp;utm_medium=email&amp;fbclid=x\">Offer</a></p>\
<img src=\"http://127.0.0.1:PORT/open.gif?u=42\" width=\"1\" height=\"1\" alt=\"\">\
<img src=\"http://127.0.0.1:PORT/banner.png?id=7&amp;utm_campaign=autumn\" width=\"600\" alt=\"Autumn\">\
<img src=\"https://us5.list-manage.com/track/open.php?u=42\">\r\n";

/// Stage 20 (decision 0058): the pixel and the known tracker are counted and never fetched,
/// not even after "load images"; the banner loads without its tracking parameter; the link
/// opens without its tracking parameters. With the settings off, everything is as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trackers_are_counted_blocked_and_links_cleaned() {
    let (port, paths) = path_recording_image_server().await;
    let server = FakeServer::new();
    server.deliver_raw(
        "INBOX",
        TRACKER_MAIL.replace("PORT", &port.to_string()).as_bytes(),
    );
    let mut h = Harness::new(
        &server,
        RemoteConfig {
            allow_loopback: true,
        },
    )
    .await;
    let account = h.add().await;
    let message = h.items(h.inbox(account).await).await[0].id;
    let body = |view: &MessageView| match &view.body {
        MessageBody::Html {
            blocked_remote,
            trackers,
            render_token,
            ..
        } => (*blocked_remote, trackers.clone(), render_token.clone()),
        other => panic!("{other:?}"),
    };

    let (blocked, trackers, token) = body(&h.open_fetched(message, BodyFormat::Auto).await);
    // Only the banner waits for "load images"; the pixel and the known tracker never do.
    assert_eq!(blocked, 1);
    assert_eq!(
        (trackers.pixels, trackers.known_hosts, trackers.parameters),
        (1, 1, 1),
        "{trackers:?}"
    );
    assert_eq!(trackers.total(), 3);
    assert_eq!(trackers.hosts, ["127.0.0.1", "us5.list-manage.com"]);

    let link = h.core.describe_link(&token, 0).expect("describe");
    assert_eq!(link.target, "https://shop.example/offer?id=7");
    assert_eq!(
        link.tracking_removed,
        ["utm_source", "utm_medium", "fbclid"]
    );
    h.core.open_link(&token, 0).await.expect("open");
    assert_eq!(h.opener.opened(), ["https://shop.example/offer?id=7"]);

    let loaded = h
        .core
        .load_remote_content(message, RemoteContentScope::Once)
        .await
        .expect("load");
    let (blocked, trackers, _) = body(&loaded);
    assert_eq!((blocked, trackers.total()), (0, 3));
    assert_eq!(
        *paths.lock().expect("paths"),
        ["/banner.png?id=7"],
        "the pixel never loads, the banner without its parameter"
    );

    // Off: no counting, the link and the images as written.
    let prefs = h.core.get_reading_preferences().await.expect("prefs");
    assert!(prefs.block_trackers && prefs.clean_links, "on by default");
    h.core
        .set_reading_preferences(mail_model::ReadingPreferences {
            block_trackers: false,
            clean_links: false,
            ..prefs
        })
        .await
        .expect("off");
    let (blocked, trackers, token) = body(
        &h.core
            .get_message(message, BodyFormat::Auto)
            .await
            .expect("get"),
    );
    assert_eq!((blocked, trackers.total()), (3, 0));
    let link = h.core.describe_link(&token, 0).expect("describe");
    assert!(link.target.contains("utm_source=news"), "{}", link.target);
    assert!(link.tracking_removed.is_empty());
}

/// Stage 20, task C (decision 0060): the attachments view lists the attachments of stored
/// bodies across the account with the pane's index; saving through the view gives the exact
/// bytes, also after the body left the cache (it is fetched again).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_attachments_view_lists_stored_attachments_and_fetches_on_open() {
    let (bytes, base64) = binary_attachment();
    let first = with_parts(
        "Invoice",
        &[
            part("application/pdf", "Invoice 42.pdf", "base64", &base64),
            part("text/plain", "notes.txt", "7bit", "remember"),
        ],
    );
    let second = with_parts(
        "Photos",
        &[part("image/png", "pixel.png", "base64", PIXEL_PNG)],
    );
    let server = FakeServer::new();
    server.deliver_raw("INBOX", first.as_bytes());
    server.deliver_raw("INBOX", second.as_bytes());
    let mut h = Harness::new(&server, RemoteConfig::default()).await;
    let account = h.add().await;
    let items = h.items(h.inbox(account).await).await;
    for item in &items {
        h.open_fetched(item.id, BodyFormat::Auto).await;
    }
    let invoice = items
        .iter()
        .find(|i| i.subject == "Invoice")
        .expect("invoice")
        .id;

    let filter = |kind, sender: Option<&str>, query: Option<&str>| mail_model::AttachmentFilter {
        scope: mail_model::MarkScope::All,
        kind,
        sender: sender.map(str::to_owned),
        query: query.map(str::to_owned),
    };
    let page = h
        .core
        .list_attachments(filter(None, None, None), None, 100)
        .await
        .expect("view");
    let names: Vec<&str> = page.items.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names.contains(&"Invoice 42.pdf") && names.contains(&"pixel.png"));
    let pdf = page
        .items
        .iter()
        .find(|a| a.name == "Invoice 42.pdf")
        .expect("pdf")
        .clone();
    assert_eq!(pdf.kind, mail_model::AttachmentKind::Pdf);
    assert_eq!(pdf.message_id, invoice);
    assert_eq!(pdf.account_id, account);
    assert!(pdf.local);
    assert_eq!(pdf.preview, Some(mail_model::PreviewKind::Pdf));
    assert_eq!(pdf.subject, "Invoice");

    let only = |page: mail_model::AttachmentPage| {
        page.items.into_iter().map(|a| a.name).collect::<Vec<_>>()
    };
    assert_eq!(
        only(
            h.core
                .list_attachments(
                    filter(Some(mail_model::AttachmentKind::Image), None, None),
                    None,
                    100
                )
                .await
                .expect("images")
        ),
        ["pixel.png"]
    );
    assert_eq!(
        only(
            h.core
                .list_attachments(filter(None, None, Some("  INVOICE ")), None, 100)
                .await
                .expect("by name")
        ),
        ["Invoice 42.pdf"]
    );
    assert_eq!(
        only(
            h.core
                .list_attachments(filter(None, Some("nobody@"), None), None, 100)
                .await
                .expect("by sender")
        ),
        Vec::<String>::new()
    );
    let err = h
        .core
        .list_attachments(filter(None, None, None), None, 0)
        .await
        .expect_err("limit");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);

    // Saved through the view's index: the exact bytes; again after the body left the cache.
    let dir = TempDir::new().expect("dir");
    let saved = h
        .core
        .save_attachment_to(invoice, pdf.index, dir.path().join("a.pdf"))
        .await
        .expect("save");
    assert_eq!(std::fs::read(saved).expect("read"), bytes);
    assert!(
        h.core
            .store_ref()
            .expect("store")
            .delete_body(invoice)
            .await
            .expect("evict")
    );
    let again = h
        .core
        .list_attachments(filter(None, None, Some("invoice")), None, 100)
        .await
        .expect("still listed");
    assert!(
        again.items.first().is_some_and(|a| !a.local),
        "listed, not on this computer"
    );
    let saved = h
        .core
        .save_attachment_to(invoice, pdf.index, dir.path().join("b.pdf"))
        .await
        .expect("fetched and saved");
    assert_eq!(std::fs::read(saved).expect("read"), bytes);
}

