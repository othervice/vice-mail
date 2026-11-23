//! Stage 9 through `AppCore` against the in-memory fake server, with the real store in a
//! temporary directory: categories at arrival and corrections, the inbox with bundles and the
//! blocks on top, snoozes with a movable clock (sleep, closed app), pins and tasks that move
//! nothing on the server, local rules on new mail (also on read-only accounts), notifications
//! through a recording sink, `mailto:` links, one-click unsubscribing only after a click
//! (counting HTTPS server), `.eml` export and the badge.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mail_model::{
    AccountId, AccountSettings, AccountState, CategoryReason, CoreEvent, ExtraRootCa, InboxFilter,
    InboxRow, InboxScope, MailboxId, MailboxRole, MessageCategory, MessageId, MessageListItem,
    NotificationSettings, RuleAction, RuleCondition, RuleInput, RuleSkipReason, Secret, Security,
    SenderKey, ServerSettings, SnoozeChoice, TaskFilter, TaskMark, TextMatchOp, UnifiedView,
    UnsubscribeMethod, UnsubscribeOutcome,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use super::{NotificationKind, RecordingSink, SNOOZE_KEYWORD, TASK_KEYWORD};
use crate::clock::{Clock, SystemClock};
use crate::credentials::{Credentials, MemorySecretStore};
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, ReadingServices, RecordingOpener};

const WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
struct OffsetClock(AtomicI64);

impl Clock for OffsetClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst)
    }
}

impl OffsetClock {
    fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

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
        wake_check: Duration::from_millis(50),
        wake_threshold: Duration::from_secs(3_600 * 24 * 365),
        shutdown_grace: Duration::from_millis(500),
        ..SyncConfig::default()
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

const PERSON: &[u8] = b"From: Bob Example <bob@friends.example>\r\nTo: alice@fake.test\r\n\
Subject: Lunch tomorrow?\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHi Alice\r\n";

fn newsletter(unsubscribe: &str) -> Vec<u8> {
    format!(
        "From: Shop News <news@shop.example>\r\nTo: alice@fake.test\r\nSubject: Deals of the week\r\n\
         List-Id: Shop News <weekly.shop.example>\r\nList-Unsubscribe: <{unsubscribe}>, <mailto:leave@shop.example?subject=unsubscribe>\r\n\
         List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\nPrecedence: bulk\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\nBuy now\r\n"
    )
    .into_bytes()
}

const GITHUB: &[u8] = b"From: GitHub <notifications@github.com>\r\nTo: alice@fake.test\r\n\
Subject: [repo] New issue\r\nX-GitHub-Reason: mention\r\nList-Id: repo <repo.owner.github.com>\r\n\
Content-Type: text/plain\r\n\r\nSomeone mentioned you\r\n";

const URGENT: &[u8] = b"From: Carol <carol@work.example>\r\nTo: alice@fake.test\r\n\
Subject: URGENT: contract\r\nContent-Type: text/plain\r\n\r\nPlease sign\r\n";

struct Harness {
    dir: TempDir,
    server: Arc<FakeServer>,
    keychain: Arc<MemorySecretStore>,
    clock: Arc<OffsetClock>,
    core: AppCore<FakeConnector>,
    events: broadcast::Receiver<CoreEvent>,
    account: AccountId,
    sink: Arc<RecordingSink>,
    opener: Arc<RecordingOpener>,
}

impl Harness {
    async fn new(server: Arc<FakeServer>) -> Self {
        Self::with(server, None).await
    }

    async fn with(server: Arc<FakeServer>, extra: Option<(ExtraRootCa, bool)>) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let keychain = Arc::new(MemorySecretStore::new());
        let clock = Arc::new(OffsetClock::default());
        let mut h = Self::start(dir, server, keychain, clock, extra).await;
        h.account = h
            .core
            .add_account(settings(), Secret::new(PASSWORD.into()))
            .await
            .expect("add account")
            .id;
        h.idle().await;
        h
    }

    async fn start(
        dir: TempDir,
        server: Arc<FakeServer>,
        keychain: Arc<MemorySecretStore>,
        clock: Arc<OffsetClock>,
        extra: Option<(ExtraRootCa, bool)>,
    ) -> Self {
        let opener = Arc::new(RecordingOpener::default());
        let mut sync = sync_config();
        let mut reading = ReadingServices {
            opener: Arc::clone(&opener) as Arc<dyn crate::reading::SystemOpener>,
            ..ReadingServices::default()
        };
        if let Some((ca, loopback)) = extra {
            sync.extra_root_ca = Some(ca);
            reading.remote.allow_loopback = loopback;
        }
        let mut services = CoreServices::with_connector(
            Credentials::new(Arc::clone(&keychain)),
            FakeConnector(Arc::clone(&server)),
        );
        services.clock = Arc::clone(&clock) as Arc<dyn Clock>;
        services.sync = sync;
        services.reading = reading;
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
        core.set_time_zone(jiff::tz::TimeZone::UTC);
        let events = core.subscribe();
        let sink = Arc::new(RecordingSink::default());
        core.set_notification_sink(Arc::clone(&sink) as Arc<dyn super::NotificationSink>);
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
            core,
            events,
            account,
            sink,
            opener,
        }
    }

    /// The same data directory, server and keychain after the app was closed.
    async fn restart(self, advance_ms: i64) -> Self {
        let Self {
            dir,
            server,
            keychain,
            clock,
            core,
            ..
        } = self;
        core.shutdown().await;
        drop(core);
        clock.advance(advance_ms);
        let mut h = Self::start(dir, server, keychain, clock, None).await;
        h.idle().await;
        h
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

    async fn idle(&mut self) {
        let accounts = self.core.list_accounts().await.expect("accounts");
        if accounts
            .iter()
            .any(|a| a.id == self.account && a.status.state == AccountState::Idle)
        {
            return;
        }
        self.event(|e| {
            matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
        })
        .await;
    }

    async fn mailbox(&self, role: MailboxRole) -> MailboxId {
        self.core
            .list_mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == role)
            .map(|m| m.id)
            .expect("mailbox with role")
    }

    async fn inbox_items(&self) -> Vec<MessageListItem> {
        let inbox = self.mailbox(MailboxRole::Inbox).await;
        self.core
            .list_messages(inbox, None, 100)
            .await
            .expect("list")
            .items
    }

    async fn by_subject(&self, subject: &str) -> MessageListItem {
        self.inbox_items()
            .await
            .into_iter()
            .find(|i| i.subject == subject)
            .unwrap_or_else(|| panic!("no message {subject:?} in the inbox"))
    }

    /// Delivers to the INBOX after the first sync and waits until it is stored.
    async fn deliver(&mut self, raw: &[u8]) -> MessageListItem {
        let subject = raw_subject(raw);
        self.server.deliver_raw("INBOX", raw);
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(item) = self
                .inbox_items()
                .await
                .into_iter()
                .find(|i| i.subject == subject)
            {
                return item;
            }
            assert!(Instant::now() < deadline, "{subject:?} never arrived");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn writes(&mut self, enabled: bool) {
        self.core
            .set_writes_enabled(self.account, enabled)
            .await
            .expect("writes");
    }

    fn scope(&self, inbox: MailboxId) -> InboxScope {
        let _ = self;
        InboxScope::Mailbox { mailbox_id: inbox }
    }
}

fn raw_subject(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .lines()
        .find_map(|l| l.strip_prefix("Subject: "))
        .unwrap_or_default()
        .to_owned()
}

/// Polls `cond` until it holds (at most [`WAIT`]).
async fn eventually<F, Fut>(what: &str, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + WAIT;
    while !cond().await {
        assert!(Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn server_with(messages: &[&[u8]]) -> Arc<FakeServer> {
    let server = FakeServer::new();
    for raw in messages {
        server.deliver_raw("INBOX", raw);
    }
    server
}

// ---------------------------------------------------------------------------------------------
// Categories, corrections, the inbox

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn categories_bundles_and_corrections() {
    let news = newsletter("https://news.shop.example/u");
    let h = Harness::new(server_with(&[PERSON, &news, GITHUB])).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;

    let person = h.by_subject("Lunch tomorrow?").await;
    let shop = h.by_subject("Deals of the week").await;
    let github = h.by_subject("[repo] New issue").await;
    assert_eq!(person.category, Some(MessageCategory::Person));
    assert_eq!(shop.category, Some(MessageCategory::Newsletter));
    assert_eq!(github.category, Some(MessageCategory::Notification));

    // "Why here?" names the rule and offers the keys for "always as …".
    let why = h.core.get_message_category(github.id).await.expect("why");
    assert!(matches!(
        why.decision.as_ref().map(|d| &d.reason),
        Some(CategoryReason::KnownSystem { .. })
    ));
    assert_eq!(
        why.address.as_ref().map(|k| k.value.as_str()),
        Some("notifications@github.com")
    );
    assert_eq!(
        why.list.as_ref().map(|k| k.value.as_str()),
        Some("repo.owner.github.com")
    );

    // The inbox: the person a row, the others bundled (one date section).
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    assert!(page.categories);
    let kinds: Vec<&str> = page
        .rows
        .iter()
        .map(|r| match r {
            InboxRow::Message { .. } => "message",
            InboxRow::Bundle { .. } => "bundle",
            InboxRow::More { .. } => "more",
        })
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "message").count(),
        1,
        "{kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "bundle").count(),
        2,
        "{kinds:?}"
    );
    let top = page.top.expect("first page has the top block");
    assert!(top.pinned.is_empty() && top.returned.is_empty() && top.priority.is_empty());

    let newsletters = h
        .core
        .list_inbox_filtered(
            h.scope(inbox),
            InboxFilter::Category {
                category: MessageCategory::Newsletter,
            },
            None,
            50,
        )
        .await
        .expect("filtered");
    assert_eq!(newsletters.items.len(), 1);
    assert_eq!(newsletters.items[0].id, shop.id);

    // The user's correction goes before every automatic rule, also before X-GitHub-Reason.
    h.core
        .set_sender_category(
            SenderKey::address("Notifications@GitHub.com"),
            Some(MessageCategory::Person),
        )
        .await
        .expect("correct")
        .expect("rule");
    eventually("the correction applies", || async {
        h.by_subject("[repo] New issue").await.category == Some(MessageCategory::Person)
    })
    .await;
    let why = h.core.get_message_category(github.id).await.expect("why");
    assert!(matches!(
        why.decision.as_ref().map(|d| &d.reason),
        Some(CategoryReason::UserRule { key }) if key.value == "notifications@github.com"
    ));

    // An important domain: the newsletter stays a newsletter, but stands in "Priority" and is
    // never bundled.
    h.core
        .set_sender_important(SenderKey::domain("SHOP.example"), true)
        .await
        .expect("important");
    eventually("the important mark applies", || async {
        h.by_subject("Deals of the week").await.important
    })
    .await;
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    let top = page.top.expect("top");
    assert_eq!(
        top.priority.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![shop.id]
    );
    assert_eq!((top.priority_total, top.priority_unread), (1, 1));
    assert!(
        page.rows
            .iter()
            .all(|r| !matches!(r, InboxRow::Bundle { .. })),
        "nothing bundled now"
    );

    // Rules removed: back to the automatic categories.
    for rule in h.core.list_sender_rules().await.expect("rules") {
        h.core.delete_sender_rule(rule.id).await.expect("delete");
    }
    eventually("rules removed", || async {
        let github = h.by_subject("[repo] New issue").await;
        let shop = h.by_subject("Deals of the week").await;
        github.category == Some(MessageCategory::Notification) && !shop.important
    })
    .await;

    // Categories off: purely chronological.
    let mut off = h.core.get_inbox_settings().await.expect("settings");
    off.categories = false;
    h.core.set_inbox_settings(off).await.expect("off");
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    assert!(!page.categories);
    assert_eq!(page.rows.len(), 3);
    assert!(
        page.rows
            .iter()
            .all(|r| matches!(r, InboxRow::Message { .. }))
    );
    let ids: Vec<i64> = page
        .rows
        .iter()
        .filter_map(|r| match r {
            InboxRow::Message { item, .. } => Some(item.id.0),
            _ => None,
        })
        .collect();
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "newest first");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bundles_continue_across_pages() {
    let news = newsletter("https://news.shop.example/u");
    let mut raws: Vec<Vec<u8>> = Vec::new();
    for i in 0..7 {
        raws.push(if i % 3 == 0 {
            PERSON.to_vec()
        } else {
            news.clone()
        });
    }
    let refs: Vec<&[u8]> = raws.iter().map(Vec::as_slice).collect();
    let h = Harness::new(server_with(&refs)).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    // Pages of two messages: the newsletter bundle starts on one page and continues.
    let mut cursor = None;
    let mut members = 0;
    let mut bundles = 0;
    let mut more = 0;
    let mut pages = 0;
    loop {
        let page = h
            .core
            .list_inbox(h.scope(inbox), cursor.clone(), 2)
            .await
            .expect("page");
        pages += 1;
        for row in &page.rows {
            match row {
                InboxRow::Bundle { members: m, .. } => {
                    bundles += 1;
                    members += m.len();
                }
                InboxRow::More {
                    members: m,
                    summary,
                } => {
                    more += 1;
                    members += m.len();
                    assert_eq!(
                        summary.count as usize, members,
                        "the summary counts every member so far"
                    );
                }
                InboxRow::Message { .. } => {}
            }
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(pages, 4);
    assert_eq!(bundles, 1, "one bundle for the one date section");
    assert!(more >= 1);
    assert_eq!(members, 4);
}

/// A database at `path` with three messages stored before a classifier existed (as by an
/// older version); its store and inbox.
async fn unclassified_store(path: &std::path::Path) -> (mail_store::Store, mail_model::MailboxId) {
    use mail_model::{Address, Flags, MessageHeader};
    let store = mail_store::Store::open(path, mail_store::StoreOptions::default()).expect("open");
    let account = store
        .insert_account(mail_store::NewAccount {
            id: "00000000-0000-4000-8000-000000000001".parse().expect("id"),
            email: "alice@example.org".into(),
            display_name: "Alice".into(),
            imap: settings().imap,
            smtp: settings().smtp,
            credential_ref: "x".into(),
        })
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(
            account,
            vec![mail_model::RemoteMailbox {
                name: "INBOX".into(),
                path: "INBOX".into(),
                delimiter: Some('/'),
                attributes: Vec::new(),
                status: None,
            }],
        )
        .await
        .expect("mailboxes");
    let inbox = store.mailboxes(account).await.expect("boxes")[0].id;
    let baseline = mail_store::MailboxBaseline {
        uid_validity: mail_model::UidValidity(1),
        uid_next: 1,
        highest_modseq: None,
    };
    store.reset_mailbox(inbox, baseline).await.expect("reset");
    let header = |uid: u32, from: &str, list: Option<&str>| MessageHeader {
        uid,
        modseq: None,
        flags: Flags::default(),
        size: 100,
        internal_date: 1_764_000_000 + i64::from(uid),
        date: Some(1_764_000_000 + i64::from(uid)),
        subject: format!("S {uid}"),
        from: vec![Address {
            name: None,
            email: from.into(),
        }],
        to: Vec::new(),
        cc: Vec::new(),
        message_id: Some(format!("{uid}@x")),
        in_reply_to: None,
        references: Vec::new(),
        list_id: list.map(str::to_owned),
        list_unsubscribe: None,
        auto_submitted: None,
        precedence: None,
        has_attachments: false,
        category_headers: Vec::new(),
        gmail: None,
    };
    store
        .store_headers(
            inbox,
            mail_model::UidValidity(1),
            vec![
                header(1, "bob@friends.example", None),
                header(2, "noreply@shop.example", None),
                header(3, "x@lists.example", Some("<l.lists.example>")),
            ],
            None,
        )
        .await
        .expect("store");
    (store, inbox)
}

async fn assert_classified(store: &mail_store::Store, inbox: mail_model::MailboxId) {
    let page = store.list_messages(inbox, None, 10).await.expect("list");
    let by_subject: HashMap<String, Option<MessageCategory>> = page
        .items
        .iter()
        .map(|i| (i.subject.clone(), i.category))
        .collect();
    assert_eq!(by_subject["S 1"], Some(MessageCategory::Person));
    assert_eq!(by_subject["S 2"], Some(MessageCategory::Notification));
    assert_eq!(by_subject["S 3"], Some(MessageCategory::Newsletter));
    assert!(!store.category_work_pending().await.expect("pending"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stored_messages_are_classified_in_the_background() {
    let dir = TempDir::new().expect("temp dir");
    let (store, inbox) = unclassified_store(&dir.path().join("db.sqlite3")).await;
    let page = store.list_messages(inbox, None, 10).await.expect("list");
    assert!(page.items.iter().all(|i| i.category.is_none()));
    assert!(store.category_work_pending().await.expect("pending"));
    store.set_classifier(Arc::new(super::classifier::ContentClassifier));
    loop {
        let step = store.category_step().await.expect("step");
        if step.done && step.classified == 0 {
            break;
        }
    }
    assert_classified(&store, inbox).await;
}

/// The core finds the unclassified messages at start and classifies them once its start is
/// done (decision 0029: the backfill does not hold the writer during the start).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_core_classifies_older_messages_after_its_start() {
    let dir = TempDir::new().expect("temp dir");
    let data = dir.path().join("data");
    let (store, inbox) = unclassified_store(&data.join(crate::DATABASE_FILE)).await;
    drop(store);
    let server = FakeServer::new();
    server.with(|s| s.faults.connect = Some(mail_model::BackendError::ConnectionRefused));
    let core = AppCore::start(
        CoreConfig {
            data_dir: data,
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::with_connector(
            Credentials::new(MemorySecretStore::new()),
            FakeConnector(server),
        ),
    )
    .await;
    let store = core.store_for_tests().clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.category_work_pending().await.expect("pending") {
        assert!(Instant::now() < deadline, "the backfill did not run");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_classified(&store, inbox).await;
    core.shutdown().await;
}

// ---------------------------------------------------------------------------------------------
// Snooze, pins, tasks

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snooze_hides_and_returns_on_top_with_a_notification() {
    let mut h = Harness::new(server_with(&[PERSON, URGENT])).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    let lunch = h.by_subject("Lunch tomorrow?").await;
    let now = h.clock.now_ms();
    let until = h
        .core
        .snooze_messages(
            vec![lunch.id],
            SnoozeChoice::At {
                at: now + 2 * 3_600_000,
            },
        )
        .await
        .expect("snooze");
    assert_eq!(until, now + 2 * 3_600_000);
    h.event(|e| matches!(e, CoreEvent::InboxChanged { .. }))
        .await;

    // Gone from the inbox rows, listed under "Später", found by `is:snoozed`.
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    let ids = row_ids(&page.rows);
    assert!(!ids.contains(&lunch.id), "snoozed message left the inbox");
    let snoozed = h
        .core
        .list_snoozed(mail_model::MarkScope::All)
        .await
        .expect("later");
    assert_eq!(
        snoozed.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![lunch.id]
    );
    assert_eq!(
        snoozed[0].snooze.map(|s| (s.until, s.returned)),
        Some((until, false))
    );
    let found = h.core.search(search("is:snoozed")).await.expect("search");
    assert_eq!(
        found.hits.iter().map(|h| h.message.id).collect::<Vec<_>>(),
        vec![lunch.id]
    );
    let not = h.core.search(search("-is:snoozed")).await.expect("search");
    assert!(!not.hits.iter().any(|h| h.message.id == lunch.id));

    // The computer sleeps over the time: the clock jumps, the app resumes.
    h.clock.advance(3 * 3_600_000);
    h.core.resume();
    h.event(|e| matches!(e, CoreEvent::InboxChanged { .. }))
        .await;
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    let top = page.top.expect("top");
    assert_eq!(
        top.returned.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![lunch.id]
    );
    assert!(
        top.returned[0].snooze.is_some_and(|s| s.returned),
        "marked as returned"
    );
    assert!(
        !row_ids(&page.rows).contains(&lunch.id),
        "on top, not repeated below"
    );
    let shown = h.sink.shown();
    let due: Vec<_> = shown
        .iter()
        .filter(|n| n.kind == NotificationKind::SnoozeDue)
        .collect();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].body, "Lunch tomorrow?");
    assert_eq!(due[0].message_id, Some(lunch.id));
    assert!(
        h.core
            .list_snoozed(mail_model::MarkScope::All)
            .await
            .expect("later")
            .is_empty()
    );

    // Clearing the mark puts it back where its date is.
    h.core
        .unsnooze_messages(vec![lunch.id])
        .await
        .expect("clear");
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    assert!(page.top.expect("top").returned.is_empty());
    assert!(row_ids(&page.rows).contains(&lunch.id));
    assert!(h.by_subject("Lunch tomorrow?").await.snooze.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snoozes_due_while_the_app_was_closed_come_back_at_start() {
    let h = Harness::new(server_with(&[PERSON])).await;
    let lunch = h.by_subject("Lunch tomorrow?").await;
    h.core
        .snooze_messages(vec![lunch.id], SnoozeChoice::Tomorrow)
        .await
        .expect("snooze");
    // Closed for two days.
    let h = h.restart(2 * 24 * 3_600_000).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    eventually("the snooze came back", || async {
        h.core
            .list_inbox(h.scope(inbox), None, 10)
            .await
            .ok()
            .and_then(|p| p.top)
            .is_some_and(|t| t.returned.iter().any(|i| i.id == lunch.id))
    })
    .await;
    // Announced (a notification from before the shell set its sink waits in the core).
    assert!(
        h.sink
            .shown()
            .iter()
            .any(|n| n.kind == NotificationKind::SnoozeDue)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snooze_pin_and_task_move_nothing_on_the_server() {
    let mut h = Harness::new(server_with(&[PERSON, URGENT])).await;
    let lunch = h.by_subject("Lunch tomorrow?").await;
    let urgent = h.by_subject("URGENT: contract").await;
    let on_server = |h: &Harness| {
        h.server.read(|s| {
            s.mailboxes
                .iter()
                .map(|(name, mb)| {
                    (
                        name.clone(),
                        mb.messages
                            .values()
                            .map(|m| m.flags.keywords.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        })
    };

    // Read-only account: everything local, no connection that could write.
    h.core
        .snooze_messages(vec![lunch.id], SnoozeChoice::NextWeek)
        .await
        .expect("snooze");
    h.core.set_pinned(vec![urgent.id], true).await.expect("pin");
    h.core
        .set_task(vec![urgent.id], Some(TaskMark { done: false }))
        .await
        .expect("task");
    let item = h.by_subject("URGENT: contract").await;
    assert!(item.pinned && item.task == Some(TaskMark { done: false }));
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);
    let pinned = h
        .core
        .list_pinned(mail_model::MarkScope::All)
        .await
        .expect("pinned");
    assert_eq!(
        pinned.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![urgent.id]
    );
    let tasks = h
        .core
        .list_tasks(mail_model::MarkScope::All, TaskFilter::Open)
        .await
        .expect("tasks");
    assert_eq!(
        tasks.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![urgent.id]
    );

    // Pinned messages stand on top and not in the rows.
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    assert_eq!(
        page.top
            .expect("top")
            .pinned
            .iter()
            .map(|i| i.id)
            .collect::<Vec<_>>(),
        vec![urgent.id]
    );
    assert!(!row_ids(&page.rows).contains(&urgent.id));

    // With writing on: only keywords go to the server, nothing is moved or deleted.
    h.core
        .unsnooze_messages(vec![lunch.id])
        .await
        .expect("unsnooze");
    h.core
        .set_task(vec![urgent.id], None)
        .await
        .expect("untask");
    h.writes(true).await;
    h.core
        .snooze_messages(vec![lunch.id], SnoozeChoice::NextWeek)
        .await
        .expect("snooze");
    h.core
        .set_task(vec![urgent.id], Some(TaskMark { done: false }))
        .await
        .expect("task");
    eventually("keywords reach the server", || async {
        let keywords: Vec<String> = on_server(&h)
            .into_iter()
            .flat_map(|(_, k)| k.into_iter().flatten())
            .collect();
        keywords.iter().any(|k| k == SNOOZE_KEYWORD) && keywords.iter().any(|k| k == TASK_KEYWORD)
    })
    .await;
    h.core
        .set_task(vec![urgent.id], Some(TaskMark { done: true }))
        .await
        .expect("done");
    let done = h
        .core
        .list_tasks(mail_model::MarkScope::All, TaskFilter::Done)
        .await
        .expect("done");
    assert_eq!(
        done.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![urgent.id]
    );
    h.core
        .unsnooze_messages(vec![lunch.id])
        .await
        .expect("unsnooze");
    eventually("keywords removed again", || async {
        on_server(&h).into_iter().all(|(_, k)| {
            k.iter()
                .all(|k| !k.iter().any(|w| w == SNOOZE_KEYWORD || w == TASK_KEYWORD))
        })
    })
    .await;
    let writes = h.server.read(|s| s.writes.clone());
    assert!(!writes.is_empty());
    assert!(
        writes.iter().all(|w| *w == "STORE"),
        "only STORE: {writes:?}"
    );
    let boxes = on_server(&h);
    let inbox_count = boxes
        .iter()
        .find(|(name, _)| name == "INBOX")
        .map_or(0, |(_, m)| m.len());
    assert_eq!(inbox_count, 2, "both messages still in the INBOX");
    assert!(
        boxes
            .iter()
            .filter(|(name, _)| name != "INBOX")
            .all(|(_, m)| m.is_empty()),
        "nothing anywhere else"
    );
}

// ---------------------------------------------------------------------------------------------
// Local rules and notifications

fn rule(name: &str, conditions: Vec<RuleCondition>, actions: Vec<RuleAction>) -> RuleInput {
    RuleInput {
        name: name.into(),
        enabled: true,
        match_all: true,
        conditions,
        actions,
        stop: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rules_run_on_new_mail_in_order() {
    let mut h = Harness::new(FakeServer::new()).await;
    h.writes(true).await;
    let account = h.account;
    // 1: the shop's mail goes to Archive, read, and is announced anyway.
    h.core
        .create_rule(rule(
            "Shop",
            vec![RuleCondition::From {
                op: TextMatchOp::Domain,
                value: "shop.example".into(),
            }],
            vec![
                RuleAction::MarkRead,
                RuleAction::Move {
                    account_id: account,
                    path: "Archive".into(),
                },
                RuleAction::Notify { enabled: true },
            ],
        ))
        .await
        .expect("rule 1");
    // 2: urgent mail is pinned and a person's, and later rules do not run.
    let mut urgent = rule(
        "Urgent",
        vec![RuleCondition::Subject {
            op: TextMatchOp::Contains,
            value: "urgent".into(),
        }],
        vec![
            RuleAction::Pin,
            RuleAction::SetCategory {
                category: MessageCategory::Person,
            },
        ],
    );
    urgent.stop = true;
    h.core.create_rule(urgent).await.expect("rule 2");
    // 3: would silence everything; never reached for urgent mail.
    h.core
        .create_rule(rule(
            "Quiet",
            vec![],
            vec![RuleAction::Notify { enabled: false }],
        ))
        .await
        .expect("rule 3");

    let news = newsletter("https://news.shop.example/u");
    h.server.deliver_raw("INBOX", &news);
    // Moved by the rule: it lands in Archive on the server, read.
    eventually("the shop's mail is archived", || async {
        h.server.read(|s| {
            s.mailboxes["Archive"].messages.len() == 1
                && s.mailboxes["Archive"]
                    .messages
                    .values()
                    .all(|m| m.flags.seen)
                && s.mailboxes["INBOX"].messages.is_empty()
        })
    })
    .await;
    eventually("announced although a newsletter", || async {
        h.sink.shown().iter().any(|n| n.body == "Deals of the week")
    })
    .await;

    let item = h.deliver(URGENT).await;
    eventually("pinned by the rule", || async {
        h.by_subject("URGENT: contract").await.pinned
    })
    .await;
    let why = h.core.get_message_category(item.id).await.expect("why");
    assert!(matches!(
        why.decision.map(|d| d.reason),
        Some(CategoryReason::LocalRule { rule_name }) if rule_name == "Urgent"
    ));
    eventually("announced (rule 3 not reached)", || async {
        h.sink.shown().iter().any(|n| n.body == "URGENT: contract")
    })
    .await;

    // A person's mail: rule 3 silences it.
    let before = h.sink.shown().len();
    h.deliver(PERSON).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.sink.shown().len(), before, "rule 3 silenced it");

    // Reorder: "Quiet" first silences the urgent mail too.
    let rules = h.core.list_rules().await.expect("rules");
    let ids: Vec<_> = rules.iter().rev().map(|r| r.id.clone()).collect();
    let reordered = h.core.reorder_rules(ids).await.expect("reorder");
    assert_eq!(reordered[0].name, "Quiet");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rules_on_read_only_accounts_are_skipped_and_reported() {
    let mut h = Harness::new(FakeServer::new()).await;
    let account = h.account;
    h.core
        .create_rule(rule(
            "Archive everything",
            vec![],
            vec![
                RuleAction::MarkRead,
                RuleAction::Move {
                    account_id: account,
                    path: "Archive".into(),
                },
                RuleAction::Pin,
            ],
        ))
        .await
        .expect("rule");
    h.server.deliver_raw("INBOX", PERSON);
    let skipped = h
        .event(|e| {
            matches!(
                e,
                CoreEvent::RuleSkipped {
                    reason: RuleSkipReason::ReadOnly,
                    ..
                }
            )
        })
        .await;
    let CoreEvent::RuleSkipped {
        rule_name, count, ..
    } = skipped
    else {
        unreachable!()
    };
    assert_eq!(rule_name, "Archive everything");
    assert_eq!(count, 1);
    // Local parts still ran; nothing was written.
    eventually("pinned anyway", || async {
        h.by_subject("Lunch tomorrow?").await.pinned
    })
    .await;
    let item = h.by_subject("Lunch tomorrow?").await;
    assert!(item.unread);
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);
    assert_eq!(h.server.read(|s| s.mailboxes["INBOX"].messages.len()), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notifications_follow_the_settings() {
    let mut h = Harness::new(FakeServer::new()).await;
    // People: yes. Newsletters: not by default.
    h.deliver(PERSON).await;
    eventually("person announced", || async {
        h.sink
            .shown()
            .iter()
            .any(|n| n.title == "Bob Example" && n.body == "Lunch tomorrow?")
    })
    .await;
    let news = newsletter("https://news.shop.example/u");
    h.deliver(&news).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!h.sink.shown().iter().any(|n| n.body == "Deals of the week"));
    // The first sync never notifies: the harness's own start produced nothing else.
    assert_eq!(h.sink.shown().len(), 1);

    // Hidden content, translated texts.
    h.core
        .set_notification_texts(mail_model::NotificationTexts {
            new_message: "Neue Nachricht".into(),
            ..mail_model::NotificationTexts::default()
        });
    h.core
        .set_notification_settings(NotificationSettings {
            hide_content: true,
            ..NotificationSettings::default()
        })
        .await
        .expect("settings");
    h.deliver(URGENT).await;
    eventually("hidden content", || async {
        h.sink
            .shown()
            .iter()
            .any(|n| n.title == "Neue Nachricht" && n.body.is_empty())
    })
    .await;

    // Stage 16: notifications play the default sound unless the account is silent; a sound
    // name outside the platform's list is refused.
    assert!(
        h.sink
            .shown()
            .iter()
            .all(|n| n.sound.as_deref() == Some("Default"))
    );
    let mut silent = NotificationSettings::default();
    silent.sound.windows = "Ping".into();
    assert!(matches!(
        h.core.set_notification_settings(silent.clone()).await,
        Err(mail_model::CoreError::InvalidArgument(_))
    ));
    silent.sound.windows = "Mail".into();
    silent.sound.silent_accounts = vec![h.account];
    h.core
        .set_notification_settings(silent)
        .await
        .expect("silent account");
    let before = h.sink.shown().len();
    h.deliver(b"From: Eve <eve@x.example>\r\nSubject: Quietly\r\n\r\nx\r\n")
        .await;
    eventually("silent notification", || async {
        h.sink
            .shown()
            .iter()
            .skip(before)
            .any(|n| n.body == "Quietly" && n.sound.is_none())
    })
    .await;

    // Quiet hours around now (UTC in these tests): dropped.
    let minute = super::time::minute_of_day(h.clock.now_ms(), &jiff::tz::TimeZone::UTC);
    h.core
        .set_notification_settings(NotificationSettings {
            quiet_hours: mail_model::QuietHours {
                enabled: true,
                start_minute: minute.saturating_sub(5) % 1440,
                end_minute: (minute + 5) % 1440,
            },
            ..NotificationSettings::default()
        })
        .await
        .expect("settings");
    let before = h.sink.shown().len();
    h.deliver(GITHUB).await;
    let mut person2 = PERSON.to_vec();
    person2.extend_from_slice(b"again\r\n");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.sink.shown().len(), before);

    // A muted account.
    h.core
        .set_notification_settings(NotificationSettings {
            muted_accounts: vec![h.account],
            ..NotificationSettings::default()
        })
        .await
        .expect("settings");
    h.server.deliver_raw(
        "INBOX",
        b"From: Dan <dan@x.example>\r\nSubject: Muted\r\n\r\nx\r\n",
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(h.sink.shown().len(), before);

    // A click opens the message.
    h.core.notification_clicked(h.account, None, MessageId(42));
    h.event(|e| matches!(e, CoreEvent::OpenMessageRequested { message_id, .. } if *message_id == MessageId(42)))
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_sends_are_announced_once() {
    let h = Harness::new(FakeServer::new()).await;
    let mail = mail_model::OutgoingMail {
        operation_id: mail_model::OperationId(7),
        account_id: h.account,
        draft_id: None,
        subject: "Quarterly report".into(),
        to: Vec::new(),
        recipient_count: 1,
        state: mail_model::SendState::Failed,
        send_at: 0,
        scheduled: false,
        error: None,
        attempts: 1,
        notice: None,
        archive: None,
        read_receipt: false,
        created_at: 0,
        updated_at: 0,
    };
    for _ in 0..2 {
        h.core.emit_event(CoreEvent::OutgoingChanged {
            account_id: h.account,
            mails: vec![mail.clone()],
        });
    }
    eventually("announced", || async {
        h.sink
            .shown()
            .iter()
            .any(|n| n.kind == NotificationKind::SendFailed)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let failed: Vec<_> = h
        .sink
        .shown()
        .into_iter()
        .filter(|n| n.kind == NotificationKind::SendFailed)
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(
        (failed[0].title.as_str(), failed[0].body.as_str()),
        ("Message not sent", "Quarterly report")
    );
}

// ---------------------------------------------------------------------------------------------
// mailto:, export, badge

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mailto_links_ask_for_the_composer() {
    let mut h = Harness::new(FakeServer::new()).await;
    let request = h
        .core
        .open_url("mailto:bob@example.org?subject=Gr%C3%BC%C3%9Fe&body=Zeile%201%0D%0AZeile%202&bcc=evil@example.net")
        .expect("mailto");
    assert_eq!(request.subject.as_deref(), Some("Grüße"));
    assert_eq!(request.body.as_deref(), Some("Zeile 1\nZeile 2"));
    assert_eq!(request.bcc.len(), 1);
    let event = h
        .event(|e| matches!(e, CoreEvent::ComposeRequested { .. }))
        .await;
    let CoreEvent::ComposeRequested { request: sent } = event else {
        unreachable!()
    };
    assert_eq!(sent, request);
    assert!(matches!(
        sent.notices.first(),
        Some(mail_model::MailtoNotice::BccPresent)
    ));
    assert!(h.core.open_url("https://example.org").is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn export_writes_the_message_as_on_the_server_and_counts_the_badge() {
    let h = Harness::new(server_with(&[PERSON, URGENT])).await;
    let lunch = h.by_subject("Lunch tomorrow?").await;
    assert_eq!(
        h.core.export_file_name(lunch.id).await.expect("name"),
        "Lunch tomorrow_.eml"
    );
    let path = h.dir.path().join("out.eml");
    h.core
        .export_message(lunch.id, path.clone())
        .await
        .expect("export");
    assert_eq!(std::fs::read(&path).expect("file"), PERSON);

    assert_eq!(h.core.badge_count().await.expect("badge"), 2);
    let settings = NotificationSettings {
        badge: mail_model::BadgeMode::Off,
        ..NotificationSettings::default()
    };
    h.core
        .set_notification_settings(settings)
        .await
        .expect("off");
    assert_eq!(h.core.badge_count().await.expect("badge"), 0);
}

fn row_ids(rows: &[InboxRow]) -> Vec<MessageId> {
    rows.iter()
        .flat_map(|r| match r {
            InboxRow::Message { item, .. } => vec![item.id],
            InboxRow::Bundle { members, .. } | InboxRow::More { members, .. } => {
                members.iter().map(|m| m.id).collect()
            }
        })
        .collect()
}

fn search(query: &str) -> mail_model::SearchRequest {
    mail_model::SearchRequest {
        query: query.into(),
        scope: mail_model::SearchScope::All,
        cursor: None,
        limit: 50,
        typing: false,
        month_first: false,
    }
}

// ---------------------------------------------------------------------------------------------
// Unsubscribing: nothing without a click (counting HTTPS server)

struct Pki {
    ca_pem: String,
    server: Arc<rustls::ServerConfig>,
}

fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        i32::try_from(year).expect("year"),
        u8::try_from(month).expect("month"),
        u8::try_from(day).expect("day"),
    )
}

fn pki() -> Pki {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer,
        KeyPair, KeyUsagePurpose, SanType,
    };
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    let today = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs()
            / 86_400,
    )
    .expect("days");
    let day = |offset: i64| {
        let (y, m, d) = civil_from_days(today + offset);
        rcgen::date_time_ymd(y, m, d)
    };
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Unsubscribe test CA");
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    ca_params.not_before = day(-1);
    ca_params.not_after = day(30);
    let ca_key = KeyPair::generate().expect("key");
    let ca = ca_params.self_signed(&ca_key).expect("ca");
    let issuer = Issuer::new(ca_params, ca_key);
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
    params.subject_alt_names = vec![SanType::IpAddress(std::net::IpAddr::V4(
        std::net::Ipv4Addr::LOCALHOST,
    ))];
    params
        .distinguished_name
        .push(DnType::CommonName, "127.0.0.1");
    params.use_authority_key_identifier_extension = true;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = day(-1);
    params.not_after = day(30);
    let key = KeyPair::generate().expect("key");
    let cert = params.signed_by(&key, &issuer).expect("cert");
    let server = rustls::ServerConfig::builder_with_provider(mail_imap::tls::provider())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone(), ca.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .expect("server config");
    Pki {
        ca_pem: ca.pem(),
        server: Arc::new(server),
    }
}

/// A request as the counting server saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: String,
}

/// HTTPS on 127.0.0.1 that counts TCP connections and records requests; answers 200.
struct CountingServer {
    addr: SocketAddr,
    connections: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl CountingServer {
    async fn start(tls: Arc<rustls::ServerConfig>) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let acceptor = tokio_rustls::TlsAcceptor::from(tls);
        let (count, log) = (Arc::clone(&connections), Arc::clone(&seen));
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                count.fetch_add(1, Ordering::SeqCst);
                let (acceptor, log) = (acceptor.clone(), Arc::clone(&log));
                tokio::spawn(async move {
                    let Ok(mut stream) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 2048];
                    let head_end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                    let mut lines = head.split("\r\n");
                    let first = lines.next().unwrap_or_default().to_owned();
                    let headers: HashMap<String, String> = lines
                        .filter_map(|l| l.split_once(':'))
                        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
                        .collect();
                    let length: usize = headers
                        .get("content-length")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    while buf.len() < head_end + length {
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
                    let mut parts = first.split(' ');
                    log.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(Seen {
                            method: parts.next().unwrap_or_default().to_owned(),
                            target: parts.next().unwrap_or_default().to_owned(),
                            headers,
                            body,
                        });
                    let _ = stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                        )
                        .await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            addr,
            connections,
            seen,
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsubscribe_requests_go_out_only_after_a_click() {
    let pki = pki();
    let counter = CountingServer::start(Arc::clone(&pki.server)).await;
    let url = format!(
        "https://127.0.0.1:{}/unsubscribe?id=42",
        counter.addr.port()
    );
    let news = newsletter(&url);
    let mut h = Harness::with(
        server_with(&[&news]),
        Some((ExtraRootCa::from_pem(pki.ca_pem.clone().into_bytes()), true)),
    )
    .await;
    let message = h.by_subject("Deals of the week").await;

    // Opening the message and showing what it offers sends nothing.
    let _ = h
        .core
        .get_message(message.id, mail_model::BodyFormat::Html)
        .await
        .expect("open");
    h.event(|e| matches!(e, CoreEvent::MessageBodyAvailable { .. }))
        .await;
    let extras = h.core.get_message_extras(message.id).await.expect("extras");
    assert_eq!(
        extras.unsubscribe.one_click_host.as_deref(),
        Some("127.0.0.1")
    );
    assert_eq!(
        extras.unsubscribe.mailto.as_deref(),
        Some("leave@shop.example")
    );
    assert!(extras.body_complete);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(counter.connections(), 0, "no request without a click");

    // The click: one POST, nothing else of the message, no cookies, no referrer.
    let outcome = h
        .core
        .unsubscribe(message.id, UnsubscribeMethod::OneClick)
        .await
        .expect("unsubscribe");
    assert_eq!(outcome, UnsubscribeOutcome::Requested { status: 200 });
    assert_eq!(
        counter.connections(),
        1,
        "positive control: the counter counts"
    );
    let seen = counter.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].target, "/unsubscribe?id=42");
    assert_eq!(seen[0].body, "List-Unsubscribe=One-Click");
    assert!(!seen[0].headers.contains_key("cookie"));
    assert!(!seen[0].headers.contains_key("referer"));
    assert_eq!(
        seen[0].headers.get("user-agent").map(String::as_str),
        Some("Mozilla/5.0")
    );

    // mailto: the composer gets the request; the web page goes to the browser.
    let outcome = h
        .core
        .unsubscribe(message.id, UnsubscribeMethod::Mailto)
        .await
        .expect("mailto");
    let UnsubscribeOutcome::Compose { request } = outcome else {
        panic!("compose expected")
    };
    assert_eq!(request.to[0].email, "leave@shop.example");
    assert_eq!(request.subject.as_deref(), Some("unsubscribe"));
    h.core
        .unsubscribe(message.id, UnsubscribeMethod::Web)
        .await
        .expect("web");
    assert_eq!(h.opener.opened(), vec![url.clone()]);
    assert_eq!(
        counter.connections(),
        1,
        "the browser page is not fetched by the app"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unified_scope_and_groups() {
    let h = Harness::new(server_with(&[PERSON])).await;
    let page = h
        .core
        .list_inbox(
            InboxScope::Unified {
                view: UnifiedView::All,
            },
            None,
            10,
        )
        .await
        .expect("unified");
    assert_eq!(row_ids(&page.rows).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notifications_wait_for_the_shell_s_sink() {
    let dir = TempDir::new().expect("temp dir");
    let services = CoreServices::with_connector(
        Credentials::new(MemorySecretStore::new()),
        FakeConnector(FakeServer::new()),
    );
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
    let account: AccountId = "00000000-0000-4000-8000-000000000001".parse().expect("id");
    let note = |n: i64| super::Notification {
        kind: NotificationKind::SnoozeDue,
        title: format!("t{n}"),
        body: String::new(),
        account_id: account,
        mailbox_id: None,
        message_id: Some(MessageId(n)),
        sound: None,
    };
    core.notify(note(1));
    core.notify(note(2));
    let sink = Arc::new(RecordingSink::default());
    core.set_notification_sink(Arc::clone(&sink) as Arc<dyn super::NotificationSink>);
    assert_eq!(sink.shown().len(), 2, "kept until the sink was set");
    core.notify(note(3));
    let titles: Vec<String> = sink.shown().into_iter().map(|n| n.title).collect();
    assert_eq!(titles, ["t1", "t2", "t3"]);
    core.shutdown().await;
}

// Stage 16 (decision 0033): a child module, so it shares the harness.
#[path = "parity_tests.rs"]
mod parity;
