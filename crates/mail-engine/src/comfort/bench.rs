//! Benchmark of stage 9, ignored by default (`docs/testing/performance.md`):
//!
//! ```text
//! cargo test -p mail-engine --release --lib comfort::bench -- --ignored --nocapture --test-threads 1
//! ```
//!
//! 100,000 headers stored in windows of 500 as the first sync does, once without and once with
//! the classifier (raw category header blocks as the adapter fetches them, a third of them
//! newsletters, a sixth notifications); then the inbox lists of stage 9 through the store
//! (rows without snoozes and pins, a category, the important senders, the top block, the
//! badge count) and the bundling of a page, timed over 200 runs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mail_model::{
    Address, BundleOptions, BundleState, Flags, MessageCategory, MessageHeader, RemoteMailbox,
    Security, ServerSettings, UidValidity, bundle_page,
};
use mail_store::{
    InboxListKind, InboxMailboxes, MailboxBaseline, NewAccount, SenderRuleChange, Store,
    StoreOptions,
};
use tempfile::TempDir;

const TOTAL: u32 = 100_000;
const WINDOW: usize = 500;
const START: i64 = 1_546_300_800;

fn headers() -> Vec<MessageHeader> {
    (1..=TOTAL)
        .map(|uid| {
            let date = START + i64::from(uid) * 2_400;
            let kind = uid % 6;
            let (from, extra) = match kind {
                0 | 1 => (
                    format!("news@shop{}.example", uid % 40),
                    format!(
                        "List-Id: Shop <news.shop{}.example>\r\nList-Unsubscribe: <https://x/u>\r\n\
                         List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\nPrecedence: bulk\r\n",
                        uid % 40
                    ),
                ),
                2 => (
                    "notifications@github.com".to_owned(),
                    "X-GitHub-Reason: mention\r\nList-Id: <repo.owner.github.com>\r\n".to_owned(),
                ),
                _ => (
                    format!("person{}@friends.example", uid % 300),
                    String::new(),
                ),
            };
            let block = format!("From: Sender <{from}>\r\nSubject: Message {uid}\r\n{extra}\r\n");
            MessageHeader {
                uid,
                modseq: None,
                flags: Flags {
                    seen: uid % 9 != 0,
                    ..Flags::default()
                },
                size: 4_000,
                internal_date: date,
                date: Some(date),
                subject: format!("Message {uid}"),
                from: vec![Address {
                    name: Some("Sender".into()),
                    email: from,
                }],
                to: vec![Address {
                    name: None,
                    email: "alice@example.org".into(),
                }],
                cc: Vec::new(),
                message_id: Some(format!("{uid}@bench.example")),
                in_reply_to: None,
                references: Vec::new(),
                list_id: None,
                list_unsubscribe: None,
                auto_submitted: None,
                precedence: None,
                has_attachments: false,
                category_headers: block.into_bytes(),
                gmail: None,
            }
        })
        .collect()
}

async fn store_with(classify: bool) -> (TempDir, Store, mail_model::MailboxId, Duration) {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&dir.path().join("db.sqlite3"), StoreOptions::default()).expect("open");
    if classify {
        store.set_classifier(Arc::new(super::classifier::ContentClassifier));
    }
    let server = ServerSettings {
        host: "h".into(),
        port: 993,
        security: Security::Tls,
        username: "u".into(),
    };
    let account = store
        .insert_account(NewAccount {
            id: "00000000-0000-4000-8000-000000000001".parse().expect("id"),
            email: "alice@example.org".into(),
            display_name: "Alice".into(),
            imap: server.clone(),
            smtp: server,
            credential_ref: "x".into(),
        })
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(
            account,
            vec![RemoteMailbox {
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
    store
        .reset_mailbox(
            inbox,
            MailboxBaseline {
                uid_validity: UidValidity(1),
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    let all = headers();
    let started = Instant::now();
    for window in all.rchunks(WINDOW) {
        store
            .store_headers(inbox, UidValidity(1), window.to_vec(), None)
            .await
            .expect("store");
    }
    (dir, store, inbox, started.elapsed())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

async fn time<F, Fut>(name: &str, runs: usize, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        f().await;
        times.push(started.elapsed());
    }
    times.sort();
    let median = times[times.len() / 2];
    let p95 = times[times.len() * 95 / 100];
    println!(
        "{name:<44} median {:>7.3} ms  p95 {:>7.3} ms  max {:>7.3} ms",
        ms(median),
        ms(p95),
        ms(*times.last().expect("runs"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "benchmark: see the module docs"]
async fn bench_stage_9() {
    let windows = f64::from(TOTAL) / WINDOW as f64;
    let (_plain_dir, _plain, _, plain) = store_with(false).await;
    let (_dir, store, inbox, classified) = store_with(true).await;
    println!(
        "store {TOTAL} headers: without classifier {:.0} ms ({:.2} ms per window of {WINDOW}), \
         with {:.0} ms ({:.2} ms per window)",
        ms(plain),
        ms(plain) / windows,
        ms(classified),
        ms(classified) / windows
    );
    let page = store.list_messages(inbox, None, 50).await.expect("page");
    let newsletters = page
        .items
        .iter()
        .filter(|i| i.category == Some(MessageCategory::Newsletter))
        .count();
    assert!(newsletters > 0, "classified");
    // A few marks and important senders, as a user has them.
    for item in page.items.iter().take(5) {
        store.pin_message(item.id, 1).await.expect("pin");
    }
    for item in page.items.iter().skip(5).take(5) {
        store
            .snooze_message(item.id, i64::MAX / 2, 1)
            .await
            .expect("snooze");
    }
    store
        .put_sender_rule(
            mail_model::SenderKey::address("person7@friends.example"),
            SenderRuleChange {
                category: None,
                important: Some(true),
            },
            1,
        )
        .await
        .expect("rule");
    let started = Instant::now();
    loop {
        let step = store.category_step().await.expect("step");
        if step.done && step.classified == 0 {
            break;
        }
    }
    println!(
        "sender rule applied to {TOTAL} messages in {:.0} ms",
        ms(started.elapsed())
    );

    let scope = || InboxMailboxes::Mailbox(inbox);
    let runs = 200;
    time("list_messages (100, with marks)", runs, || async {
        store.list_messages(inbox, None, 100).await.expect("list");
    })
    .await;
    time("inbox rows (100, no snoozes/pins)", runs, || async {
        store
            .inbox_list(scope(), InboxListKind::Rows, None, 100)
            .await
            .expect("rows");
    })
    .await;
    time("inbox category newsletter (100)", runs, || async {
        store
            .inbox_list(
                scope(),
                InboxListKind::Category(MessageCategory::Newsletter),
                None,
                100,
            )
            .await
            .expect("category");
    })
    .await;
    time("inbox category person (100)", runs, || async {
        store
            .inbox_list(
                scope(),
                InboxListKind::Category(MessageCategory::Person),
                None,
                100,
            )
            .await
            .expect("category");
    })
    .await;
    time("important senders (3) + counts", runs, || async {
        store
            .inbox_list(scope(), InboxListKind::Important, None, 3)
            .await
            .expect("important");
        store.important_counts(scope()).await.expect("counts");
    })
    .await;
    time("top block: pinned + returned", runs, || async {
        store.inbox_top(scope(), false, 100).await.expect("pinned");
        store.inbox_top(scope(), true, 100).await.expect("returned");
    })
    .await;
    time("badge: unread people", runs, || async {
        store.unread_in_inboxes(true).await.expect("badge");
    })
    .await;
    let items = store
        .inbox_list(scope(), InboxListKind::Rows, None, 100)
        .await
        .expect("rows")
        .items;
    let zone = jiff::tz::TimeZone::UTC;
    let now = START * 1_000 + i64::from(TOTAL) * 2_400_000;
    time("bundle a page of 100 (sections, chips)", runs, || {
        let items = items.clone();
        let zone = zone.clone();
        async move {
            let rows: Vec<super::Sectioned> = items
                .into_iter()
                .map(|item| super::Sectioned {
                    section: super::time::section(item.date, now, &zone),
                    unread: item.unread,
                    item,
                })
                .collect();
            let mut state = BundleState::default();
            let _ = bundle_page(rows, &mut state, &BundleOptions::default());
        }
    })
    .await;
}
