//! Acceptance tests of stage 3 for the engine, against the local test server (Stalwart) with
//! the seed. Ignored by default; they need the server reset, started and seeded:
//!
//! ```text
//! scripts/testmail.sh lock reading
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000
//! cargo test -p mail-engine --lib reading::server_tests -- --ignored --nocapture --test-threads 1
//! VICEMAIL_RESTART_TEST=1 cargo test -p mail-engine --lib reading::server_tests::prefetched -- --ignored --nocapture
//! scripts/testmail.sh stop; scripts/testmail.sh unlock reading
//! ```
//!
//! - every message of the seed opens from the stored body in well under 100 ms (median, p95
//!   and maximum over all messages, the large HTML newsletters included; measured in the core:
//!   `getMessage` from call to answer, parsing, sanitising and rendering included);
//! - every `broken/*` message of the seed gives a readable view or a clear problem report;
//! - a saved attachment has exactly the bytes of the original, for a complete message (the
//!   1.5 MB PDF) and for a partial one whose attachment is fetched on demand (the 5 MB ZIP);
//! - prefetched messages open while the test server is stopped (`VICEMAIL_RESTART_TEST=1`,
//!   because it stops and starts the shared server).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mail_imap::ImapConnector;
use mail_imap::test_support::TestEnv;
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, BodyFormat, BodySection,
    BodyUnavailableReason, CoreEvent, MailBackend, MailConnector, MessageBody, MessageId,
    MessageView, PrefetchPeriod, PrefetchSettings, PrefetchState, Security, ServerSettings, UidSet,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::sync::SyncConfig;
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, ReadingServices, RecordingOpener};

const WAIT: Duration = Duration::from_secs(120);

struct Engine {
    _dir: TempDir,
    core: AppCore,
    events: broadcast::Receiver<CoreEvent>,
    account: AccountId,
}

impl Engine {
    async fn start(env: &TestEnv) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let mut services = CoreServices::with_connector(
            Credentials::new(MemorySecretStore::new()),
            ImapConnector::new(),
        );
        services.sync = SyncConfig {
            backoff: BackoffPolicy {
                base: Duration::from_millis(100),
                max: Duration::from_secs(1),
                jitter: 0.5,
            },
            extra_root_ca: Some(env.extra_root()),
            ..SyncConfig::default()
        };
        services.reading = ReadingServices {
            opener: Arc::new(RecordingOpener::default()),
            open_dir: Some(dir.path().join("open")),
            ..ReadingServices::default()
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
        let mut events = core.subscribe();
        let server = |port, security| ServerSettings {
            host: env.host.clone(),
            port,
            security,
            username: env.alice.address.clone(),
        };
        let started = Instant::now();
        let account = core
            .add_account(
                AccountSettings {
                    email: env.alice.address.clone(),
                    display_name: "Alice".into(),
                    imap: server(env.imaps_port, Security::Tls),
                    smtp: server(env.submission_port, Security::Starttls),
                },
                env.alice.password.clone(),
            )
            .await
            .expect("add alice")
            .id;
        wait(&mut events, |e| {
            matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
        })
        .await;
        println!("first sync: {:?}", started.elapsed());
        Self {
            _dir: dir,
            core,
            events,
            account,
        }
    }

    /// Prefetch everything (the seed spans more than 30 days), with the default size limit.
    async fn prefetch_all(&mut self) {
        let started = Instant::now();
        self.core
            .set_prefetch_settings(PrefetchSettings {
                period: PrefetchPeriod::All,
                max_message_size: PrefetchSettings::DEFAULT_MAX_MESSAGE_SIZE,
            })
            .await
            .expect("settings");
        loop {
            let event = wait(&mut self.events, |e| {
                matches!(e, CoreEvent::PrefetchProgress { .. })
            })
            .await;
            if let CoreEvent::PrefetchProgress { status, .. } = event
                && status.state == PrefetchState::Idle
                && status.done > 0
            {
                println!(
                    "prefetch: {} bodies, {} bytes in {:?}",
                    status.done,
                    status.bytes,
                    started.elapsed()
                );
                break;
            }
        }
        let left = self
            .core
            .store_for_tests()
            .prefetch_candidates(self.account, None, None, 10)
            .await
            .expect("candidates");
        assert!(
            left.is_empty(),
            "everything in the period is stored: {left:?}"
        );
    }

    /// Every message of the account: (mailbox path, id, flagged).
    async fn messages(&self) -> Vec<(String, MessageId, bool)> {
        let mut out = Vec::new();
        for mailbox in self
            .core
            .list_mailboxes(self.account)
            .await
            .expect("mailboxes")
        {
            let mut cursor = None;
            loop {
                let page = self
                    .core
                    .list_messages(mailbox.id, cursor, 500)
                    .await
                    .expect("page");
                out.extend(
                    page.items
                        .iter()
                        .map(|i| (mailbox.path.clone(), i.id, i.flagged)),
                );
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        out
    }

    /// The `X-Vicemail-Seed` key of a stored message.
    async fn seed_key(&self, message: MessageId) -> Option<String> {
        let store = self.core.store_for_tests();
        let header = match store.load_message(message).await.expect("load") {
            Some(raw) => raw,
            None => {
                store
                    .load_sections(message, Some(vec![BodySection::Header]))
                    .await
                    .expect("sections")
                    .into_iter()
                    .next()?
                    .data
            }
        };
        let text = String::from_utf8_lossy(&header);
        let head = text.split("\r\n\r\n").next().unwrap_or_default();
        head.lines()
            .find_map(|l| l.strip_prefix("X-Vicemail-Seed: "))
            .map(|k| k.trim().to_owned())
    }

    async fn open(&mut self, message: MessageId, format: BodyFormat) -> MessageView {
        let view = self.core.get_message(message, format).await.expect("get");
        if view.body != MessageBody::Loading {
            return view;
        }
        let event = wait(&mut self.events, |e| {
            matches!(e, CoreEvent::MessageBodyAvailable { message_id, .. } | CoreEvent::MessageBodyFailed { message_id, .. } if *message_id == message)
        })
        .await;
        assert!(
            matches!(event, CoreEvent::MessageBodyAvailable { .. }),
            "{event:?}"
        );
        self.core.get_message(message, format).await.expect("get")
    }
}

async fn wait(
    events: &mut broadcast::Receiver<CoreEvent>,
    pred: impl Fn(&CoreEvent) -> bool,
) -> CoreEvent {
    let deadline = Instant::now() + WAIT;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) if pred(&event) => return event,
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(err)) => panic!("event stream: {err}"),
            Err(_) => panic!("no matching event within {WAIT:?}"),
        }
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the test server with `seed 1000`"]
async fn stored_messages_open_in_under_100_ms_and_broken_ones_never_fail() {
    let env = TestEnv::require();
    let mut engine = Engine::start(&env).await;
    engine.prefetch_all().await;
    let messages = engine.messages().await;
    assert!(messages.len() >= 1000, "seeded: {}", messages.len());

    // Bodies not prefetched (unflagged mail outside Inbox, Sent and Drafts) are fetched once
    // first; the measurement is about stored bodies.
    let mut fetched = 0;
    for (_, id, _) in &messages {
        if engine
            .core
            .store_for_tests()
            .body_info(*id)
            .await
            .expect("info")
            .is_none()
        {
            engine.open(*id, BodyFormat::Auto).await;
            fetched += 1;
        }
    }
    println!("fetched on open: {fetched}");

    let mut keys = HashMap::new();
    for (_, id, _) in &messages {
        if let Some(key) = engine.seed_key(*id).await {
            keys.insert(*id, key);
        }
    }

    let mut all = Vec::new();
    let mut newsletters = Vec::new();
    let mut by_kind: HashMap<&'static str, usize> = HashMap::new();
    for _round in 0..2 {
        for (_, id, _) in &messages {
            let started = Instant::now();
            let view = engine
                .core
                .get_message(*id, BodyFormat::Auto)
                .await
                .expect("get");
            let took = started.elapsed();
            let kind = match &view.body {
                MessageBody::Html { .. } => "html",
                MessageBody::Text { .. } => "text",
                MessageBody::Loading => "loading",
                MessageBody::Unavailable { .. } => "unavailable",
            };
            *by_kind.entry(kind).or_default() += 1;
            assert!(
                matches!(
                    view.body,
                    MessageBody::Html { .. } | MessageBody::Text { .. }
                ),
                "stored message {id:?} ({:?}) did not render: {:?}",
                keys.get(id),
                view.body
            );
            all.push(took);
            if keys
                .get(id)
                .is_some_and(|k| k.starts_with("html/newsletter"))
            {
                newsletters.push((keys[id].clone(), took));
            }
        }
    }
    all.sort();
    let (median, p95, p99, max) = (
        percentile(&all, 0.5),
        percentile(&all, 0.95),
        percentile(&all, 0.99),
        *all.last().expect("timings"),
    );
    println!(
        "getMessage(auto) over {} openings of {} stored messages: median {median:?}, p95 {p95:?}, p99 {p99:?}, max {max:?}; {by_kind:?}",
        all.len(),
        messages.len()
    );
    newsletters.sort();
    for (key, took) in &newsletters {
        println!("  {key}: {took:?}");
    }
    assert!(!newsletters.is_empty(), "the seed's newsletters were found");
    assert!(p95 < Duration::from_millis(100), "p95 {p95:?}");
    assert!(max < Duration::from_millis(100), "max {max:?}");

    // Every broken message: a view in every format, never an error.
    let broken: Vec<(MessageId, String)> = keys
        .iter()
        .filter(|(_, k)| k.starts_with("broken/"))
        .map(|(id, k)| (*id, k.clone()))
        .collect();
    let mut names: Vec<&str> = broken.iter().map(|(_, k)| k.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    // 26 broken mails in the seed; `broken/duplicate-message-id` shares its Message-ID with
    // `thread/offsite-03` in another folder and is stored as one message with two locations
    // (merge rule of decision 0003), so its body is that of the thread message.
    assert_eq!(names.len(), 25, "{names:?}");
    assert!(!names.contains(&"broken/duplicate-message-id"));
    for (id, key) in &broken {
        for format in [BodyFormat::Auto, BodyFormat::Html, BodyFormat::Text] {
            let view = engine.core.get_message(*id, format).await.expect("get");
            let readable = match &view.body {
                MessageBody::Text { segments, .. } => {
                    !segments.is_empty() || !view.problems.is_empty()
                }
                MessageBody::Html { srcdoc, .. } => !srcdoc.is_empty(),
                other => panic!("{key}: {other:?}"),
            };
            assert!(
                readable,
                "{key} ({format:?}): neither content nor a problem report"
            );
        }
        let view = engine
            .core
            .get_message(*id, BodyFormat::Auto)
            .await
            .expect("get");
        println!(
            "  {key}: formats {:?}, problems {:?}",
            view.formats, view.problems
        );
    }
    let mut with_problems = 0;
    for (id, _) in &broken {
        let view = engine
            .core
            .get_message(*id, BodyFormat::Auto)
            .await
            .expect("get");
        with_problems += usize::from(!view.problems.is_empty());
    }
    println!(
        "broken messages with a problem report: {with_problems} of {}",
        broken.len()
    );
    assert!(with_problems > 0);
}

/// The original bytes of an attachment: the whole message over a second connection of our
/// own, decoded with mail-content (independent of the core's stored body and its sections).
async fn original_attachment(
    env: &TestEnv,
    mailbox: &str,
    subject_key: &str,
    file_name: &str,
) -> Vec<u8> {
    let mut backend = ImapConnector::new()
        .connect(&env.config(&env.alice, Security::Tls, Access::ReadOnly))
        .await
        .expect("connect");
    let selected = backend.open_mailbox(mailbox).await.expect("examine");
    let uids = backend.search_uids(&selected).await.expect("search");
    let all: Vec<_> = uids.iter().collect();
    for chunk in all.chunks(100) {
        let raws = backend
            .fetch_messages(&selected, &UidSet::from_uids(chunk.iter().copied()))
            .await
            .expect("fetch");
        for raw in raws {
            let text = String::from_utf8_lossy(&raw.data[..raw.data.len().min(8192)]).into_owned();
            if !text.contains(&format!("X-Vicemail-Seed: {subject_key}")) {
                continue;
            }
            let parsed = mail_content::parse(&raw.data);
            let part = parsed
                .attachments
                .iter()
                .find(|a| a.file_name == file_name)
                .unwrap_or_else(|| panic!("{subject_key}: no attachment {file_name}"));
            let bytes = mail_content::attachment_bytes(&raw.data, part.part_id).expect("decode");
            let _ = backend.logout().await;
            return bytes;
        }
    }
    panic!("{subject_key} not found in {mailbox}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the test server with `seed 1000`"]
async fn saved_attachments_have_the_bytes_of_the_original() {
    let env = TestEnv::require();
    let mut engine = Engine::start(&env).await;
    let keep = std::env::var_os("VICEMAIL_KEEP_ATTACHMENTS").map(std::path::PathBuf::from);
    let target_dir = TempDir::new().expect("dir");
    let target_dir = keep
        .clone()
        .unwrap_or_else(|| target_dir.path().to_path_buf());
    std::fs::create_dir_all(&target_dir).expect("dir");

    let messages = engine.messages().await;
    for (mailbox, key, name, expect_partial) in [
        ("INBOX", "large/pdf-1mb", "large-document.pdf", false),
        ("Archive", "large/zip-5mb", "large-archive.zip", true),
    ] {
        // Find the message by opening the candidates of its mailbox by subject.
        let subject = if key.ends_with("zip-5mb") {
            "Large attachment (zip)"
        } else {
            "Large attachment (pdf)"
        };
        let mut found = None;
        for (path, id, _) in &messages {
            if path != mailbox {
                continue;
            }
            let view = engine
                .core
                .get_message(*id, BodyFormat::Auto)
                .await
                .expect("get");
            if view.header.subject == subject {
                found = Some(*id);
                break;
            }
        }
        let id = found.unwrap_or_else(|| panic!("{key} not in {mailbox}"));
        let started = Instant::now();
        let view = engine.open(id, BodyFormat::Auto).await;
        println!(
            "{key}: opened in {:?}, partial {}",
            started.elapsed(),
            view.partial
        );
        assert_eq!(view.partial, expect_partial, "{key}");
        let attachment = view
            .attachments
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("{key}: attachments {:?}", view.attachments));
        assert_eq!(attachment.local, !expect_partial);

        let target = target_dir.join(&attachment.name);
        let started = Instant::now();
        engine
            .core
            .save_attachment_to(id, attachment.index, target.clone())
            .await
            .expect("save");
        println!("{key}: saved in {:?}", started.elapsed());
        let saved = std::fs::read(&target).expect("read saved");
        let original = original_attachment(&env, mailbox, key, name).await;
        if let Some(dir) = &keep {
            std::fs::write(dir.join(format!("{name}.original")), &original).expect("keep");
        }
        println!(
            "{key}: {} bytes saved, {} bytes original",
            saved.len(),
            original.len()
        );
        assert!(
            saved == original,
            "{key}: the saved file differs from the original"
        );
        if expect_partial {
            // Now stored as a section: shown as local, saved again without the server.
            let again = engine
                .core
                .get_message(id, BodyFormat::Auto)
                .await
                .expect("get");
            assert!(again.attachments.iter().any(|a| a.name == name && a.local));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the test server with `seed 1000`; stops and starts it"]
async fn prefetched_messages_open_while_the_server_is_stopped() {
    if std::env::var("VICEMAIL_RESTART_TEST").as_deref() != Ok("1") {
        println!("skipped: set VICEMAIL_RESTART_TEST=1 to stop and start the test server");
        return;
    }
    let env = TestEnv::require();
    let mut engine = Engine::start(&env).await;
    engine.prefetch_all().await;
    let messages = engine.messages().await;
    let mut prefetched = Vec::new();
    let mut not_stored = None;
    for (path, id, _) in &messages {
        if engine
            .core
            .store_for_tests()
            .body_info(*id)
            .await
            .expect("info")
            .is_some()
        {
            prefetched.push((path.clone(), *id));
        } else if not_stored.is_none() {
            not_stored = Some(*id);
        }
    }
    let not_stored = not_stored.expect("a message outside the prefetch");
    println!("prefetched: {} of {}", prefetched.len(), messages.len());

    let script = env.repo.join("scripts/testmail.sh");
    let run = |arg: &str| {
        let status = std::process::Command::new(&script)
            .arg(arg)
            .status()
            .expect("run testmail.sh");
        assert!(status.success(), "testmail.sh {arg}");
    };
    run("stop");
    wait(&mut engine.events, |e| {
        matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Offline)
    })
    .await;
    let mut opened = 0;
    for (path, id) in &prefetched {
        let view = engine
            .core
            .get_message(*id, BodyFormat::Auto)
            .await
            .expect("get");
        assert!(
            matches!(
                view.body,
                MessageBody::Html { .. } | MessageBody::Text { .. }
            ),
            "{path}/{id:?} offline: {:?}",
            view.body
        );
        opened += 1;
    }
    let offline = engine
        .core
        .get_message(not_stored, BodyFormat::Auto)
        .await
        .expect("get");
    let status = engine
        .core
        .get_prefetch_status(engine.account)
        .await
        .expect("status");
    run("start");
    println!(
        "opened offline: {opened}; prefetch state while offline: {:?}",
        status.state
    );
    assert_eq!(
        offline.body,
        MessageBody::Unavailable {
            reason: BodyUnavailableReason::Offline,
            error: None
        }
    );
    assert_eq!(status.state, PrefetchState::Paused);
    // Back online, the account syncs again.
    wait(&mut engine.events, |e| {
        matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
    })
    .await;
}
