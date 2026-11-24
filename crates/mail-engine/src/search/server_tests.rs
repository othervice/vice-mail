//! Search on the server against the local test server (Stalwart) with the seed. Ignored by
//! default; needs the server reset, started and seeded:
//!
//! ```text
//! scripts/testmail.sh lock search
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 300
//! cargo test -p mail-engine --lib search::server_tests -- --ignored --nocapture --test-threads 1
//! scripts/testmail.sh stop; scripts/testmail.sh unlock search
//! ```
//!
//! - a word from the body of a message whose body is not stored is not found locally, but by
//!   "search on the server", over the read-only reading connection;
//! - only messages without a complete local body come back, each a stored message;
//! - non-ASCII words (`CHARSET UTF-8`), `has:attachment` (local only, reported) and an unknown
//!   folder behave as documented.

use std::sync::Arc;
use std::time::Duration;

use mail_imap::ImapConnector;
use mail_imap::test_support::TestEnv;
use mail_model::{
    Access, AccountSettings, AccountState, ConnectConfig, CoreEvent, MailBackend, MailConnector,
    MailboxRole, SearchProblemCode, SearchRequest, SearchScope, Security, ServerSettings, UidSet,
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

fn request(query: &str, scope: SearchScope) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        scope,
        cursor: None,
        limit: 100,
        typing: false,
        month_first: false,
    }
}

/// The longest plain word of the text part of `raw` (after the header), lower case.
fn body_word(raw: &[u8]) -> Option<String> {
    let parsed = mail_content::parse(raw);
    let text = parsed.display_text().into_owned();
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| w.len() >= 9)
        .max_by_key(|w| w.len())
        .map(str::to_lowercase)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the seeded test server: see the module docs"]
async fn server_search_finds_what_is_not_stored_locally() {
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
                credential: Default::default(),
                protocol: Default::default(),
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

    // A message of the inbox whose body is not stored, and a word from its body (read with a
    // connection of the test's own, read-only).
    let inbox = core
        .list_mailboxes(account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == MailboxRole::Inbox)
        .expect("inbox");
    let mut config = ConnectConfig::new(
        &server(env.imaps_port, Security::Tls),
        env.alice.password.clone(),
        Access::ReadOnly,
    );
    config.extra_root_ca = Some(env.extra_root());
    let mut backend = ImapConnector::new()
        .connect(&config)
        .await
        .expect("connect");
    let selected = backend.open_mailbox("INBOX").await.expect("examine");
    let locations = store.mailbox_sync_states(account).await.expect("states");
    assert!(locations.iter().any(|s| s.mailbox_id == inbox.id));
    let mut target = None;
    let mut cursor = None;
    'find: loop {
        let page = core
            .list_messages(inbox.id, cursor, 100)
            .await
            .expect("page");
        for item in &page.items {
            if store.body_info(item.id).await.expect("body").is_some() {
                continue;
            }
            let locs = store.message_locations(item.id).await.expect("locations");
            let Some(loc) = locs.iter().find(|l| l.mailbox_id == inbox.id) else {
                continue;
            };
            let raw = backend
                .fetch_messages(&selected, &UidSet::from_uids([loc.uid]))
                .await
                .expect("fetch");
            if let Some(word) = raw.first().and_then(|r| body_word(&r.data)) {
                target = Some((item.id, word));
                break 'find;
            }
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    let (message, word) = target.expect("a message without a local body and with text");
    let _ = backend.logout().await;
    println!("searching for {word:?} (message {message})");

    let local = core
        .search(request(
            &word,
            SearchScope::Account {
                account_id: account,
            },
        ))
        .await
        .expect("local");
    assert!(
        local.hits.iter().all(|h| h.message.id != message),
        "the body is not local, so the local index cannot know the word"
    );
    let started = Instant::now();
    let found = core
        .search_on_server(request(
            &word,
            SearchScope::Account {
                account_id: account,
            },
        ))
        .await
        .expect("server search");
    println!(
        "server search: {} hits in {} mailboxes, {:?}",
        found.hits.len(),
        found.searched.len(),
        started.elapsed()
    );
    assert!(found.failed.is_empty(), "{:?}", found.failed);
    assert!(found.searched.contains(&inbox.id));
    assert!(
        found.hits.iter().any(|h| h.message.id == message),
        "the server finds the message"
    );
    assert!(found.hits.iter().all(|h| !h.body_local));
    assert!(found.problems.is_empty());

    // Limited to the inbox, with a date filter that excludes everything.
    let none = core
        .search_on_server(request(
            &format!("{word} in:inbox before:1.1.1990"),
            SearchScope::All,
        ))
        .await
        .expect("dated");
    assert!(none.hits.is_empty());
    assert_eq!(none.searched, [inbox.id]);

    // Non-ASCII goes with CHARSET UTF-8; the server answers (with or without hits).
    let umlaut = core
        .search_on_server(request(
            "Grüße",
            SearchScope::Account {
                account_id: account,
            },
        ))
        .await
        .expect("umlaut");
    assert!(
        umlaut.failed.is_empty(),
        "UTF-8 search refused: {:?}",
        umlaut.failed
    );

    // Terms only the local data can answer are reported; unknown folders match nothing.
    let attachment = core
        .search_on_server(request(
            &format!("{word} has:attachment"),
            SearchScope::Account {
                account_id: account,
            },
        ))
        .await
        .expect("has:attachment");
    assert!(
        attachment
            .problems
            .iter()
            .any(|p| p.code == SearchProblemCode::NotOnServer)
    );
    let unknown = core
        .search_on_server(request(&format!("{word} in:gibtsnicht"), SearchScope::All))
        .await
        .expect("unknown folder");
    assert!(unknown.hits.is_empty() && unknown.searched.is_empty());
    core.shutdown().await;
}
