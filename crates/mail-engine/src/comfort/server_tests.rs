//! Stage 9 against the local test server (Stalwart) with the seed. Ignored by default; needs
//! the server reset, started and seeded:
//!
//! ```text
//! scripts/testmail.sh lock comfort
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 300
//! cargo test -p mail-engine --lib comfort::server_tests -- --ignored --nocapture --test-threads 1
//! scripts/testmail.sh stop; scripts/testmail.sh unlock comfort
//! ```
//!
//! - the category headers come through the real `HEADER.FIELDS` fetch: the seed corpus is
//!   sorted as a person would sort it (hit rate printed; the misses decision 0016 names are
//!   allowed);
//! - snooze and task set their keywords on the server and remove them again, and nothing is
//!   moved: the message stays in the INBOX with its UID;
//! - the calendar invitation of the seed is read from the fetched body.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mail_imap::ImapConnector;
use mail_imap::test_support::TestEnv;
use mail_model::{
    AccountSettings, AccountState, CoreEvent, MailboxRole, MessageCategory, Security,
    ServerSettings, SnoozeChoice, TaskMark,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::sync::SyncConfig;
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, ReadingServices, RecordingOpener};

const WAIT: Duration = Duration::from_secs(120);

async fn wait(events: &mut broadcast::Receiver<CoreEvent>, pred: impl Fn(&CoreEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) if pred(&event) => return,
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(err)) => panic!("event stream: {err}"),
            Err(_) => panic!("no matching event within {WAIT:?}"),
        }
    }
}

/// What a person expects for a seed fixture (as `mail-content/tests/classify_seed.rs`).
fn expected(name: &str) -> Option<MessageCategory> {
    use MessageCategory::{Newsletter, Notification, Person};
    Some(match name {
        "html_broken-markup.eml" | "html_external-everything.eml" | "html_hostile.eml" => {
            Notification
        }
        "mime_digest.eml" => Newsletter,
        _ if name.starts_with("auto_") => Notification,
        _ if name.starts_with("html_newsletter-") || name.starts_with("list_") => Newsletter,
        _ if name.starts_with("broken_")
            || name.starts_with("charset_")
            || name.starts_with("mime_") =>
        {
            Person
        }
        _ => return None,
    })
}

/// Decision 0016, limits: a phishing mail without any program header.
const KNOWN_MISSES: &[&str] = &["html_hostile.eml"];

/// Message-ID → fixture name of the seed corpus.
fn corpus() -> HashMap<String, String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testmail/corpus/seed");
    let mut out = HashMap::new();
    for entry in std::fs::read_dir(&dir).expect("seed corpus") {
        let path = entry.expect("entry").path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        let raw = std::fs::read(&path).expect("fixture");
        if let Some(id) = mail_content::parse(&raw).headers.message_id {
            out.insert(id, name);
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the seeded test server: see the module docs"]
async fn categories_marks_and_invitations_on_the_test_server() {
    let env = TestEnv::require();
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
    let store = core.store_for_tests().clone();
    let mailboxes = core.list_mailboxes(account).await.expect("mailboxes");
    let inbox = mailboxes
        .iter()
        .find(|m| m.role == MailboxRole::Inbox)
        .expect("inbox")
        .id;

    // Every stored message of the corpus, wherever it lies.
    let names = corpus();
    let mut hits = 0;
    let mut checked = 0;
    let mut misses = Vec::new();
    let mut by_name: HashMap<String, (mail_model::MessageId, mail_model::MailboxId)> =
        HashMap::new();
    for mailbox in &mailboxes {
        let mut cursor = None;
        loop {
            let page = core
                .list_messages(mailbox.id, cursor, 500)
                .await
                .expect("page");
            for item in &page.items {
                let Some(summary) = store.message_summary(item.id).await.expect("summary") else {
                    continue;
                };
                let Some(name) = summary
                    .message_id_header
                    .as_ref()
                    .and_then(|id| names.get(id))
                else {
                    continue;
                };
                let Some(want) = expected(name) else { continue };
                if by_name
                    .insert(name.clone(), (item.id, mailbox.id))
                    .is_some()
                {
                    continue;
                }
                checked += 1;
                if item.category == Some(want) {
                    hits += 1;
                } else {
                    misses.push(format!("{name}: {:?}, expected {want:?}", item.category));
                }
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
    }
    println!(
        "seed corpus over IMAP: {hits} of {checked} as expected ({:.1} %)",
        f64::from(hits) * 100.0 / f64::from(checked.max(1))
    );
    for miss in &misses {
        println!("  miss: {miss}");
    }
    assert!(checked >= 60, "too few fixtures found: {checked}");
    let unexpected: Vec<&String> = misses
        .iter()
        .filter(|m| !KNOWN_MISSES.iter().any(|k| m.starts_with(k)))
        .collect();
    assert!(unexpected.is_empty(), "unexpected misses: {unexpected:#?}");

    // Snooze and task on a writable account: keywords on the server, nothing moved.
    let (target, _) = core
        .list_messages(inbox, None, 50)
        .await
        .expect("inbox")
        .items
        .iter()
        .find(|i| !i.draft)
        .map(|i| (i.id, i.mailbox_id))
        .expect("an inbox message");
    let before = store.message_locations(target).await.expect("locations");
    core.set_writes_enabled(account, true)
        .await
        .expect("writes");
    core.snooze_messages(vec![target], SnoozeChoice::NextWeek)
        .await
        .expect("snooze");
    core.set_task(vec![target], Some(TaskMark { done: false }))
        .await
        .expect("task");
    core.set_pinned(vec![target], true).await.expect("pin");
    let has = |keywords: &[String], k: &str| keywords.iter().any(|w| w.eq_ignore_ascii_case(k));
    let deadline = Instant::now() + WAIT;
    loop {
        let item = store.message_locations(target).await.expect("locations");
        let row = core
            .list_messages(inbox, None, 500)
            .await
            .expect("inbox")
            .items
            .into_iter()
            .find(|i| i.id == target);
        if let Some(row) = row
            && has(&row.keywords, super::SNOOZE_KEYWORD)
            && has(&row.keywords, super::TASK_KEYWORD)
        {
            assert_eq!(item, before, "same mailbox and UID: nothing moved");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "keywords never reached the server"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    core.unsnooze_messages(vec![target])
        .await
        .expect("unsnooze");
    core.set_task(vec![target], Some(TaskMark { done: true }))
        .await
        .expect("done");
    core.set_pinned(vec![target], false).await.expect("unpin");
    let deadline = Instant::now() + WAIT;
    loop {
        let row = core
            .list_messages(inbox, None, 500)
            .await
            .expect("inbox")
            .items
            .into_iter()
            .find(|i| i.id == target)
            .expect("still in the inbox");
        if !has(&row.keywords, super::SNOOZE_KEYWORD) && !has(&row.keywords, super::TASK_KEYWORD) {
            break;
        }
        assert!(Instant::now() < deadline, "keywords never removed");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        store.message_locations(target).await.expect("locations"),
        before,
        "nothing moved"
    );
    core.set_writes_enabled(account, false)
        .await
        .expect("writes off");

    // The invitation of the seed, from the fetched body.
    if let Some((invite, _)) = by_name.get("mime_calendar-invite.eml").copied() {
        let _ = core
            .get_message(invite, mail_model::BodyFormat::Text)
            .await
            .expect("open");
        let deadline = Instant::now() + WAIT;
        let extras = loop {
            let extras = core.get_message_extras(invite).await.expect("extras");
            if extras.body_complete {
                break extras;
            }
            assert!(Instant::now() < deadline, "body never arrived");
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        assert!(!extras.invites.is_empty(), "the invitation is read");
        let event = &extras.invites[0].invite.events[0];
        println!("invitation: {:?} at {:?}", event.summary, event.start);
        assert!(event.start.is_some());
    } else {
        panic!("the seed's invitation is missing");
    }
    core.shutdown().await;
}
