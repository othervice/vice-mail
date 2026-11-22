//! Benchmark, ignored by default: 100,000 headers stored in windows of 500 as the first sync
//! does, then list pages, counters and the sync queries timed through the public API (channel
//! to the database threads included). Target from decision 0003: a list page well below 20 ms.
//!
//! ```text
//! cargo test -p mail-store --release -- --ignored --nocapture bench_
//! ```

use std::time::{Duration, Instant};

use mail_model::{FlagUpdate, ListCursor, MailboxId, MessageHeader, MessageId, UidSet};

use crate::test_support::{UV, fixture, header};
use crate::{Store, StoreReport};

const TOTAL: u32 = 100_000;
const WINDOW: u64 = 500;
const START: i64 = 1_546_300_800; // 01.01.2019
const END: i64 = 1_795_305_600; // 22.11.2026

/// SplitMix64: deterministic data without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Headers of one mailbox, UID order = date order (as delivered), with Message-IDs.
fn mailbox_headers(rng: &mut Rng, n: u32, first_msgid: u64, seen_pct: u64) -> Vec<MessageHeader> {
    let span = u64::try_from(END - START).expect("positive span");
    let mut dates: Vec<i64> = (0..n)
        .map(|_| START + i64::try_from(rng.below(span)).expect("fits"))
        .collect();
    dates.sort_unstable();
    dates
        .into_iter()
        .zip(1u32..)
        .map(|(date, uid)| {
            // 2 % without Message-ID.
            let msgid =
                (rng.below(100) >= 2).then(|| format!("{}@bench", first_msgid + u64::from(uid)));
            let mut h = header(uid, date, msgid.as_deref());
            h.flags.seen = rng.below(100) < seen_pct;
            h.flags.flagged = rng.below(100) < 2;
            h.size = 2_000 + u32::try_from(rng.below(100_000)).expect("fits");
            h
        })
        .collect()
}

/// Stores the headers from the highest UID down in windows of 500, like the first sync.
async fn first_sync(store: &Store, mailbox: MailboxId, headers: &[MessageHeader]) -> StoreReport {
    let max = u32::try_from(headers.len()).expect("fits");
    let mut rest = UidSet::range(1, max);
    let mut total = StoreReport::default();
    while !rest.is_empty() {
        let (window, remaining) = rest.split_highest(WINDOW);
        let batch: Vec<_> = window
            .iter()
            .map(|uid| headers[usize::try_from(uid - 1).expect("fits")].clone())
            .collect();
        let r = store
            .store_headers(mailbox, UV, batch, window.min())
            .await
            .expect("store window");
        total.inserted += r.inserted;
        total.merged += r.merged;
        total.updated += r.updated;
        rest = remaining;
    }
    total
}

struct Timing {
    median: Duration,
    p95: Duration,
    max: Duration,
}

async fn time<F, Fut>(runs: usize, mut f: F) -> Timing
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        f().await;
        samples.push(start.elapsed());
    }
    samples.sort();
    Timing {
        median: samples[samples.len() / 2],
        p95: samples[samples.len() * 95 / 100],
        max: samples[samples.len() - 1],
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn report(name: &str, t: &Timing) {
    println!(
        "| {name:<42} | {:>8.3} | {:>8.3} | {:>8.3} |",
        ms(t.median),
        ms(t.p95),
        ms(t.max)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "benchmark: cargo test -p mail-store --release -- --ignored --nocapture bench_"]
async fn bench_100k_headers_and_list_pages() {
    let f = fixture().await;
    let store = &f.store;
    let mut rng = Rng(0x5eed);

    // 60 % inbox, 25 % archive, 15 % sent; 8 % of the inbox also lies in the archive (merge).
    let inbox_n = TOTAL * 60 / 100;
    let archive_n = TOTAL * 25 / 100;
    let sent_n = TOTAL - inbox_n - archive_n;
    let inbox = mailbox_headers(&mut rng, inbox_n, 0, 90);
    let mut archive = mailbox_headers(&mut rng, archive_n, 1_000_000, 100);
    let sent = mailbox_headers(&mut rng, sent_n, 2_000_000, 100);
    let copies = archive_n * 8 / 100;
    for i in 0..copies {
        let src = &inbox[usize::try_from(rng.below(u64::from(inbox_n))).expect("fits")];
        let slot = &mut archive[usize::try_from(i).expect("fits")];
        slot.message_id.clone_from(&src.message_id);
    }

    let start = Instant::now();
    let mut stored = StoreReport::default();
    for (mailbox, headers) in [(f.inbox, &inbox), (f.archive, &archive), (f.sent, &sent)] {
        let r = first_sync(store, mailbox, headers).await;
        stored.inserted += r.inserted;
        stored.merged += r.merged;
    }
    let elapsed = start.elapsed();
    let windows = u64::from(TOTAL).div_ceil(WINDOW);
    println!(
        "\nstored {TOTAL} headers ({} rows, {} merged) in {windows} windows: {:.2} s, {:.0} mails/s, {:.1} ms per window",
        stored.inserted,
        stored.merged,
        elapsed.as_secs_f64(),
        f64::from(TOTAL) / elapsed.as_secs_f64(),
        ms(elapsed) / windows as f64,
    );
    store
        .write(|tx| Ok(tx.execute_batch("PRAGMA optimize")?))
        .await
        .expect("optimize");

    let all: Vec<i64> = {
        let mut ids = Vec::new();
        let mut cursor = None;
        loop {
            let page = store
                .list_messages(f.inbox, cursor, 500)
                .await
                .expect("page");
            ids.extend(page.items.iter().map(|i| i.id.0));
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        ids
    };
    assert_eq!(all.len(), usize::try_from(inbox_n).expect("fits"));
    let middle = ListCursor::after(MessageId(all[all.len() / 2]));
    let last = ListCursor::after(MessageId(all[all.len() - 50]));

    println!("\n| query (200 runs, through the API)          | median ms |   p95 ms |   max ms |");
    println!("|---|---:|---:|---:|");
    let runs = 200;
    let first_page = time(runs, || async {
        store.list_messages(f.inbox, None, 100).await.expect("page");
    })
    .await;
    report("inbox (60k) page 1, 100 rows", &first_page);
    let mid = time(runs, || async {
        store
            .list_messages(f.inbox, Some(middle), 100)
            .await
            .expect("page");
    })
    .await;
    report("inbox keyset from the middle", &mid);
    let end = time(runs, || async {
        store
            .list_messages(f.inbox, Some(last), 100)
            .await
            .expect("page");
    })
    .await;
    report("inbox keyset, last page", &end);
    let big = time(runs, || async {
        store.list_messages(f.inbox, None, 500).await.expect("page");
    })
    .await;
    report("inbox page 1, 500 rows", &big);
    let boxes = time(runs, || async {
        store.mailboxes(f.account).await.expect("mailboxes");
    })
    .await;
    report("mailboxes with unread and total", &boxes);
    let unread = time(runs, || async {
        store.unread_counts(f.account).await.expect("counts");
    })
    .await;
    report("unread counts", &unread);
    let uids = time(20, || async {
        store.known_uids(f.inbox).await.expect("uids");
    })
    .await;
    report("known UIDs of the inbox (20 runs)", &uids);

    let mut toggle = false;
    let flags = time(20, || {
        toggle = !toggle;
        let updates: Vec<FlagUpdate> = (1..=500)
            .map(|uid| FlagUpdate {
                uid,
                flags: mail_model::Flags {
                    seen: toggle,
                    ..mail_model::Flags::default()
                },
                modseq: None,
            })
            .collect();
        async move {
            store
                .update_flags(f.inbox, UV, updates)
                .await
                .expect("flags");
        }
    })
    .await;
    report("500 flag changes, one transaction (20 runs)", &flags);

    let start = Instant::now();
    let removed = store
        .remove_uids(f.inbox, UV, UidSet::range(1, 5_000))
        .await
        .expect("remove");
    let orphans = store
        .delete_orphan_messages(f.account)
        .await
        .expect("orphans");
    println!(
        "\nremove 5,000 UIDs ({removed} locations) + delete {orphans} orphans: {:.1} ms",
        ms(start.elapsed())
    );
    let start = Instant::now();
    assert!(store.delete_account(f.account).await.expect("delete"));
    println!(
        "delete the account with all remaining messages: {:.1} ms",
        ms(start.elapsed())
    );

    assert!(
        first_page.median < Duration::from_millis(20) && end.median < Duration::from_millis(20),
        "list page target of decision 0003 missed"
    );
}
