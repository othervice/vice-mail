//! Performance of stage 7 (`docs/testing/performance.md`): generated mailboxes over five
//! accounts, stored through the store API as a first sync stores them (windows of 500, newest
//! first, threads and index written with each window), bodies for a realistic share, then
//! timed through [`AppCore`]: list pages, thread lists, the unified inbox, first search hits,
//! opening locally stored messages, start until the first list page, memory when idle.
//!
//! ```text
//! cargo test -p mail-engine --release --lib search::bench -- --ignored --nocapture --test-threads 1
//! ```
//!
//! `bench_100k` and `bench_500k` print markdown rows. "Start" and memory are measured in a
//! child process (this test binary again, `child_measure`), so the generator's memory does not
//! count: it starts the core on the generated database, lists the largest inbox, runs a few
//! searches and thread lists, waits two seconds and reads its resident size (`ps`). It fails
//! when the start alone makes more than [`START_READ_LIMIT_MB`] resident (the database is
//! mapped, so that means the start read the database broadly, as the category check of stage 9
//! once did) or when start, first page or idle memory miss the targets of `docs/SPEC.md`.
//! `small_data_meets_generous_thresholds` runs in every `cargo test` with 3,000 messages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mail_model::{
    AccountId, Address, BackendError, BodyFormat, Flags, ListCursor, MailboxId, MailboxRole,
    MessageHeader, MessageId, RemoteMailbox, SearchRequest, SearchScope, Security, ServerSettings,
    UidSet, UidValidity, UnifiedView,
};
use mail_store::{MailboxBaseline, NewAccount, Store, StoreOptions};
use tempfile::TempDir;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::reading::index_text::ContentText;
use crate::sync::fake::{FakeConnector, FakeServer};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, DATABASE_FILE};

const UV: UidValidity = UidValidity(1);
const START: i64 = 1_546_300_800; // 01.01.2019
const END: i64 = 1_795_305_600; // 22.11.2026
const DAY: i64 = 86_400;

/// SplitMix64.
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
        self.next() % n.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[usize::try_from(self.below(items.len() as u64)).unwrap_or(0)]
    }
}

/// Zipf-distributed indices into `0..n` (s = 1), by inverse CDF.
struct Zipf {
    cdf: Vec<f64>,
}

impl Zipf {
    fn new(n: usize) -> Self {
        let mut cdf = Vec::with_capacity(n);
        let mut total = 0.0;
        for k in 1..=n {
            total += 1.0 / k as f64;
            cdf.push(total);
        }
        for c in &mut cdf {
            *c /= total;
        }
        Self { cdf }
    }

    fn sample(&self, rng: &mut Rng) -> usize {
        let u = (rng.next() >> 11) as f64 / (1u64 << 53) as f64;
        self.cdf.partition_point(|c| *c < u).min(self.cdf.len() - 1)
    }
}

const COMMON: &[&str] = &[
    "und",
    "der",
    "die",
    "das",
    "the",
    "and",
    "for",
    "mit",
    "für",
    "von",
    "bitte",
    "danke",
    "rechnung",
    "bestellung",
    "termin",
    "projekt",
    "angebot",
    "meeting",
    "update",
    "frage",
    "grüße",
    "über",
    "müller",
    "straße",
    "lieferung",
    "vertrag",
    "kunde",
    "report",
    "invoice",
    "order",
    "schedule",
    "please",
    "thanks",
    "budget",
    "planung",
    "zahlung",
    "konto",
];

struct Vocabulary {
    words: Vec<String>,
    zipf: Zipf,
}

impl Vocabulary {
    fn new(rng: &mut Rng, n: usize) -> Self {
        const SYL: &[&str] = &[
            "ka", "lo", "mer", "ti", "sa", "ber", "un", "ge", "zu", "ver", "an", "ist", "ra", "mo",
            "ne", "wa", "ch", "ü", "ä", "ö", "sch", "pro", "ten", "lin", "dor", "fe",
        ];
        let mut words: Vec<String> = COMMON.iter().map(|w| (*w).to_owned()).collect();
        while words.len() < n {
            let parts = 2 + rng.below(3);
            let word: String = (0..parts).map(|_| *rng.pick(SYL)).collect();
            words.push(word);
        }
        Self {
            zipf: Zipf::new(words.len()),
            words,
        }
    }

    fn word(&self, rng: &mut Rng) -> &str {
        &self.words[self.zipf.sample(rng)]
    }

    fn text(&self, rng: &mut Rng, n: usize) -> String {
        let mut out = String::new();
        for i in 0..n {
            if i > 0 {
                out.push(if rng.chance(8) { '\n' } else { ' ' });
            }
            out.push_str(self.word(rng));
        }
        out
    }
}

/// Mailboxes of a generated account with their share of messages (percent).
const FOLDERS: &[(&str, &[&str], u64)] = &[
    ("INBOX", &[], 55),
    ("Archive", &["\\Archive"], 20),
    ("Sent", &["\\Sent"], 12),
    ("Newsletter", &[], 10),
    ("Trash", &["\\Trash"], 3),
];

struct Generated {
    message: MessageHeader,
    folder: usize,
    body: Option<Vec<u8>>,
}

struct Conversation {
    subject: String,
    ids: Vec<String>,
}

/// One account's messages in date order.
fn account_messages(
    rng: &mut Rng,
    vocab: &Vocabulary,
    senders: &[(String, String)],
    sender_zipf: &Zipf,
    account: usize,
    email: &str,
    n: usize,
) -> Vec<Generated> {
    let span = u64::try_from(END - START).unwrap_or(1);
    let mut dates: Vec<i64> = (0..n)
        .map(|_| {
            // Denser towards the present: the square root of a uniform number.
            let u = (rng.next() >> 11) as f64 / (1u64 << 53) as f64;
            START + (u.sqrt() * span as f64) as i64
        })
        .collect();
    dates.sort_unstable();
    let mut conversations: Vec<Conversation> = Vec::new();
    let mut list_thread = Conversation {
        subject: format!("[dev] {}", vocab.text(rng, 4)),
        ids: Vec::new(),
    };
    let mut out = Vec::with_capacity(n);
    for (i, date) in dates.into_iter().enumerate() {
        let roll = rng.below(100);
        let folder = {
            let mut acc = 0;
            FOLDERS
                .iter()
                .position(|(_, _, share)| {
                    acc += share;
                    roll < acc
                })
                .unwrap_or(0)
        };
        let id = format!("{account}.{i}@gen.example");
        let sender = &senders[sender_zipf.sample(rng)];
        let mut header = MessageHeader {
            uid: 0,
            modseq: None,
            flags: Flags {
                seen: rng.chance(if date > END - 30 * DAY { 60 } else { 96 }),
                flagged: rng.chance(2),
                ..Flags::default()
            },
            size: 2_000 + u32::try_from(rng.below(60_000)).unwrap_or(0),
            internal_date: date,
            date: Some(date),
            subject: String::new(),
            from: vec![Address {
                name: Some(sender.0.clone()),
                email: sender.1.clone(),
            }],
            to: vec![Address {
                name: None,
                email: email.to_owned(),
            }],
            cc: Vec::new(),
            message_id: Some(id.clone()),
            in_reply_to: None,
            references: Vec::new(),
            list_id: None,
            list_unsubscribe: None,
            auto_submitted: None,
            precedence: None,
            has_attachments: rng.chance(15),
            category_headers: Vec::new(),
            gmail: None,
        };
        if FOLDERS[folder].0 == "Sent" {
            header.from = vec![Address {
                name: Some("Ich".into()),
                email: email.to_owned(),
            }];
            header.to = vec![Address {
                name: Some(sender.0.clone()),
                email: sender.1.clone(),
            }];
        }
        if FOLDERS[folder].0 == "Newsletter" {
            // One long list thread, the rest single newsletters.
            header.list_id = Some("dev.lists.example".into());
            if rng.chance(30) {
                header.subject = if list_thread.ids.is_empty() {
                    list_thread.subject.clone()
                } else {
                    format!("Re: {}", list_thread.subject)
                };
                if let Some(last) = list_thread.ids.last() {
                    header.in_reply_to = Some(last.clone());
                    header.references = list_thread
                        .ids
                        .iter()
                        .rev()
                        .take(20)
                        .rev()
                        .cloned()
                        .collect();
                }
                list_thread.ids.push(id.clone());
            } else {
                header.subject = format!("Newsletter {}", vocab.text(rng, 4));
                header.list_unsubscribe = Some("<mailto:leave@lists.example>".into());
            }
        } else if !conversations.is_empty() && rng.chance(40) {
            // A reply to one of the recent conversations.
            let back = usize::try_from(rng.below(conversations.len().min(300) as u64)).unwrap_or(0);
            let index = conversations.len() - 1 - back;
            let conv = &mut conversations[index];
            header.subject = format!(
                "{} {}",
                if rng.chance(50) { "Re:" } else { "AW:" },
                conv.subject
            );
            header.in_reply_to = conv.ids.last().cloned();
            header.references = conv.ids.iter().rev().take(10).rev().cloned().collect();
            conv.ids.push(id.clone());
        } else {
            let words = 2 + usize::try_from(rng.below(5)).unwrap_or(0);
            let subject = vocab.text(rng, words);
            header.subject = subject.clone();
            conversations.push(Conversation {
                subject,
                ids: vec![id.clone()],
            });
        }
        // Bodies: recent mail of Inbox and Sent, flagged mail, 15 % of the rest.
        let recent = date > END - 30 * DAY && matches!(FOLDERS[folder].0, "INBOX" | "Sent");
        let body = (recent || header.flags.flagged || rng.chance(15))
            .then(|| raw_body(rng, vocab, &header));
        out.push(Generated {
            message: header,
            folder,
            body,
        });
    }
    out
}

fn raw_body(rng: &mut Rng, vocab: &Vocabulary, h: &MessageHeader) -> Vec<u8> {
    let words = 20 + usize::try_from(rng.below(400)).unwrap_or(0);
    let text = vocab.text(rng, words);
    let from = &h.from[0];
    let mut raw = format!(
        "From: {} <{}>\r\nTo: {}\r\nSubject: {}\r\nMessage-ID: <{}>\r\nMIME-Version: 1.0\r\n",
        from.name.as_deref().unwrap_or(""),
        from.email,
        h.to[0].email,
        h.subject,
        h.message_id.as_deref().unwrap_or("x@y"),
    );
    let html = rng.chance(30);
    let part = if html {
        format!(
            "Content-Type: text/html; charset=utf-8\r\n\r\n<html><body><p>{}</p><p>Viele Grüße</p></body></html>\r\n",
            text.replace('\n', "</p><p>")
        )
    } else {
        format!("Content-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n\r\nViele Grüße\r\n")
    };
    if h.has_attachments {
        raw.push_str("Content-Type: multipart/mixed; boundary=gen\r\n\r\n--gen\r\n");
        raw.push_str(&part);
        raw.push_str(&format!(
            "--gen\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"Rechnung_{}-{:05}.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjcK\r\n--gen--\r\n",
            2019 + rng.below(8),
            rng.below(100_000)
        ));
    } else {
        raw.push_str(&part);
    }
    raw.into_bytes()
}

/// Account id, address, mailboxes with their roles (in `FOLDERS` order).
type GeneratedAccount = (AccountId, String, Vec<(MailboxId, MailboxRole)>);

struct Dataset {
    dir: TempDir,
    accounts: Vec<GeneratedAccount>,
    largest_inbox: MailboxId,
    messages: usize,
    bodies: usize,
    store_secs: f64,
    windows: usize,
    body_secs: f64,
    frequent: Vec<String>,
    rare: String,
    sender: String,
}

fn data_path(dir: &TempDir) -> PathBuf {
    dir.path().join("data").join(DATABASE_FILE)
}

/// Generates `total` messages over five accounts (50 / 24 / 16 / 8 / 2 %) into a new database.
async fn generate(total: usize) -> Dataset {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&data_path(&dir), StoreOptions::default()).expect("open");
    store.set_body_text_source(Arc::new(ContentText));
    let mut rng = Rng(0x5eed_0007);
    let vocab = Vocabulary::new(&mut rng, 30_000);
    let senders: Vec<(String, String)> = (0..3_000)
        .map(|i| {
            let first = vocab.words[COMMON.len() + i % 500].clone();
            let last = vocab.words[COMMON.len() + 500 + (i * 7) % 2_000].clone();
            (
                format!("{first} {last}"),
                format!("{first}.{last}{i}@sender.example"),
            )
        })
        .collect();
    let sender_zipf = Zipf::new(senders.len());
    let shares = [50, 24, 16, 8, 2];
    let mut accounts = Vec::new();
    let mut store_time = Duration::ZERO;
    let mut body_time = Duration::ZERO;
    let mut windows = 0;
    let mut bodies = 0;
    let mut largest_inbox = (0, MailboxId(0));
    for (a, share) in shares.iter().enumerate() {
        let email = format!("konto{a}@bench.example");
        let id: AccountId = format!("00000000-0000-4000-8000-{:012}", a + 1)
            .parse()
            .expect("id");
        let server = |port| ServerSettings {
            host: "imap.invalid".into(),
            port,
            security: Security::Tls,
            username: email.clone(),
        };
        store
            .insert_account(NewAccount {
                id,
                email: email.clone(),
                display_name: format!("Konto {a}"),
                imap: server(993),
                smtp: server(465),
                credential_ref: format!("bench:{id}"),
            })
            .await
            .expect("account");
        store
            .sync_mailboxes(
                id,
                FOLDERS
                    .iter()
                    .map(|(name, attrs, _)| RemoteMailbox {
                        name: (*name).into(),
                        path: (*name).into(),
                        delimiter: Some('/'),
                        attributes: attrs.iter().map(|s| (*s).to_owned()).collect(),
                        status: None,
                    })
                    .collect(),
            )
            .await
            .expect("mailboxes");
        let boxes = store.mailboxes(id).await.expect("mailboxes");
        let ids: Vec<MailboxId> = FOLDERS
            .iter()
            .map(|(name, _, _)| boxes.iter().find(|b| b.path == *name).expect("folder").id)
            .collect();
        for mailbox in &ids {
            store
                .reset_mailbox(
                    *mailbox,
                    MailboxBaseline {
                        uid_validity: UV,
                        uid_next: 1,
                        highest_modseq: None,
                    },
                )
                .await
                .expect("reset");
        }
        let n = total * share / 100;
        let messages = account_messages(&mut rng, &vocab, &senders, &sender_zipf, a, &email, n);
        // Per folder in date order, UIDs ascending; stored newest first in windows of 500.
        let mut by_folder: Vec<Vec<Generated>> = (0..FOLDERS.len()).map(|_| Vec::new()).collect();
        for g in messages {
            by_folder[g.folder].push(g);
        }
        for (f, list) in by_folder.iter_mut().enumerate() {
            for (uid, g) in list.iter_mut().enumerate() {
                g.message.uid = u32::try_from(uid + 1).expect("uid");
            }
            let max = u32::try_from(list.len()).expect("count");
            let mut rest = UidSet::range(1, max.max(1));
            if list.is_empty() {
                continue;
            }
            while !rest.is_empty() {
                let (window, remaining) = rest.split_highest(500);
                let batch: Vec<MessageHeader> = window
                    .iter()
                    .map(|uid| {
                        list[usize::try_from(uid - 1).expect("index")]
                            .message
                            .clone()
                    })
                    .collect();
                let started = Instant::now();
                store
                    .store_headers(ids[f], UV, batch, window.min())
                    .await
                    .expect("window");
                store_time += started.elapsed();
                windows += 1;
                rest = remaining;
            }
            if f == 0 && list.len() > largest_inbox.0 {
                largest_inbox = (list.len(), ids[0]);
            }
            // Bodies after the headers (prefetch and opening store them later in reality).
            let with_body: Vec<(u32, Vec<u8>)> = list
                .iter()
                .filter_map(|g| g.body.clone().map(|b| (g.message.uid, b)))
                .collect();
            let page_ids = all_ids(&store, ids[f]).await;
            // ids newest first, uids ascending by date: uid k is page_ids[len - k].
            let mut batch = Vec::new();
            for (uid, body) in with_body {
                let index = page_ids.len() - usize::try_from(uid).expect("uid");
                batch.push((MessageId(page_ids[index]), body));
                if batch.len() == 200 {
                    let started = Instant::now();
                    bodies += store
                        .store_bodies(std::mem::take(&mut batch))
                        .await
                        .expect("bodies")
                        .len();
                    body_time += started.elapsed();
                }
            }
            if !batch.is_empty() {
                let started = Instant::now();
                bodies += store.store_bodies(batch).await.expect("bodies").len();
                body_time += started.elapsed();
            }
        }
        accounts.push((
            id,
            email,
            ids.iter()
                .zip(FOLDERS)
                .map(|(id, (name, _, _))| {
                    let role = boxes
                        .iter()
                        .find(|b| b.path == *name)
                        .map(|b| b.role)
                        .unwrap_or(MailboxRole::Custom);
                    (*id, role)
                })
                .collect(),
        ));
    }
    drop(store);
    Dataset {
        dir,
        accounts,
        largest_inbox: largest_inbox.1,
        messages: total,
        bodies,
        store_secs: store_time.as_secs_f64(),
        windows,
        body_secs: body_time.as_secs_f64(),
        frequent: vocab.words[..COMMON.len()].to_vec(),
        rare: vocab.words[25_000].clone(),
        sender: senders[3]
            .0
            .split(' ')
            .next_back()
            .unwrap_or_default()
            .to_owned(),
    }
}

async fn all_ids(store: &Store, mailbox: MailboxId) -> Vec<i64> {
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list_messages(mailbox, cursor, 500)
            .await
            .expect("page");
        ids.extend(page.items.iter().map(|i| i.id.0));
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return ids,
        }
    }
}

/// The core on the generated data, without network: every connection is refused.
async fn start_core(data_dir: &Path) -> AppCore<FakeConnector> {
    let server = FakeServer::new();
    server.with(|s| {
        s.faults.connect = Some(BackendError::ConnectionRefused);
    });
    AppCore::start(
        CoreConfig {
            data_dir: data_dir.to_owned(),
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
    .await
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
        let started = Instant::now();
        f().await;
        samples.push(started.elapsed());
    }
    samples.sort();
    Timing {
        median: samples[samples.len() / 2],
        p95: samples[(samples.len() * 95 / 100).min(samples.len() - 1)],
        max: samples[samples.len() - 1],
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn row(name: &str, t: &Timing) {
    println!(
        "| {name} | {:.2} | {:.2} | {:.2} |",
        ms(t.median),
        ms(t.p95),
        ms(t.max)
    );
}

fn search_request(query: &str, typing: bool) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        scope: SearchScope::All,
        cursor: None,
        limit: 50,
        typing,
        month_first: false,
    }
}

/// The measured queries: (name, query, typing).
fn queries(data: &Dataset) -> Vec<(String, String, bool)> {
    let f = &data.frequent;
    let smallest = &data.accounts[4].1;
    vec![
        ("häufiges Wort".into(), "rechnung".into(), false),
        ("sehr häufiges Wort".into(), "und".into(), false),
        ("seltenes Wort".into(), data.rare.clone(), false),
        ("beim Tippen (Präfix)".into(), "rech".into(), true),
        ("Phrase".into(), "\"viele grüße\"".into(), false),
        ("zwei Wörter".into(), format!("{} {}", f[12], f[13]), false),
        ("from:".into(), format!("from:{}", data.sender), false),
        (
            "Wort + account: (kleinstes Konto)".into(),
            format!("rechnung account:{smallest}"),
            false,
        ),
        (
            "Wort + in:inbox is:unread".into(),
            "rechnung in:inbox is:unread".into(),
            false,
        ),
        (
            "Wort + newer_than:1y".into(),
            "rechnung newer_than:1y".into(),
            false,
        ),
        (
            "filename: + has:attachment".into(),
            "filename:rechnung has:attachment".into(),
            false,
        ),
        (
            "ohne Text: is:unread in:inbox".into(),
            "is:unread in:inbox".into(),
            false,
        ),
        ("nur Ausschluss: -und".into(), "-und".into(), false),
        ("OR".into(), "rechnung OR invoice".into(), false),
    ]
}

/// Everything measured in the parent process; prints markdown rows.
async fn measure(data: &Dataset, runs: usize) -> HashMap<&'static str, Duration> {
    let core = start_core(&data.dir.path().join("data")).await;
    let mut worst = HashMap::new();
    println!("\n| Messung | Median ms | p95 ms | max ms |\n|---|---:|---:|---:|");
    let inbox = data.largest_inbox;
    let t = time(runs, || async {
        core.list_messages(inbox, None, 100).await.expect("page");
    })
    .await;
    row("Listenseite größter Posteingang (100)", &t);
    worst.insert("list", t.median);
    let ids = {
        let store = core.store_for_tests();
        all_ids(store, inbox).await
    };
    let middle = ListCursor::after(MessageId(ids[ids.len() / 2]));
    let t = time(runs, || async {
        core.list_messages(inbox, Some(middle), 100)
            .await
            .expect("page");
    })
    .await;
    row("… Keyset ab Mitte", &t);
    let t = time(runs, || async {
        core.list_unified_inbox(UnifiedView::All, None, 100)
            .await
            .expect("unified");
    })
    .await;
    row("Vereinter Posteingang (5 Konten, 100)", &t);
    worst.insert("unified", t.median);
    let t = time(runs, || async {
        core.list_threads(inbox, None, 100).await.expect("threads");
    })
    .await;
    row("Thread-Liste größter Posteingang (100)", &t);
    worst.insert("threads", t.median);
    let t = time(runs, || async {
        core.list_threads(inbox, Some(middle), 100)
            .await
            .expect("threads");
    })
    .await;
    row("… Keyset ab Mitte", &t);
    let t = time(runs, || async {
        core.list_unified_threads(UnifiedView::All, None, 100)
            .await
            .expect("threads");
    })
    .await;
    row("Thread-Liste vereinter Posteingang (100)", &t);
    let newsletter = data.accounts[0].2[3].0;
    let t = time(runs, || async {
        core.list_threads(newsletter, None, 100)
            .await
            .expect("threads");
    })
    .await;
    row("Thread-Liste Newsletter (langer Listen-Thread)", &t);
    // Decision 0039: the sorted inbox as conversations (rows, top and priority on the first
    // page, bundles), next to the same inbox as messages.
    let scope = mail_model::InboxScope::Mailbox { mailbox_id: inbox };
    let unified = mail_model::InboxScope::Unified {
        view: UnifiedView::All,
    };
    let t = time(runs, || async {
        core.list_inbox(scope.clone(), None, 100)
            .await
            .expect("inbox");
    })
    .await;
    row("Sortierter Posteingang, Nachrichten (100)", &t);
    let t = time(runs, || async {
        core.list_inbox_conversations(scope.clone(), None, 100)
            .await
            .expect("conversations");
    })
    .await;
    row("Sortierter Posteingang, Konversationen (100)", &t);
    worst.insert("conversations", t.median);
    let from_middle = mail_model::InboxCursor {
        before: middle,
        bundles: None,
    };
    let t = time(runs, || async {
        core.list_inbox_conversations(scope.clone(), Some(from_middle.clone()), 100)
            .await
            .expect("conversations");
    })
    .await;
    row("… Keyset ab Mitte", &t);
    let t = time(runs, || async {
        core.list_inbox_conversations(unified.clone(), None, 100)
            .await
            .expect("conversations");
    })
    .await;
    row("Vereinter Posteingang, Konversationen (5 Konten, 100)", &t);
    worst.insert("unified conversations", t.median);
    let t = time(runs, || async {
        core.list_inbox_conversations_filtered(
            scope.clone(),
            mail_model::InboxFilter::Unread,
            None,
            100,
        )
        .await
        .expect("unread conversations");
    })
    .await;
    row("Nur ungelesene, Konversationen (100)", &t);
    let t = time(runs, || async {
        core.list_unread_first_conversations(scope.clone(), None, 100)
            .await
            .expect("unread first");
    })
    .await;
    row("Ungelesene oben, Konversationen (100)", &t);
    let first_rows: Vec<mail_model::MessageRef> = core
        .list_threads(inbox, None, 100)
        .await
        .expect("threads")
        .items
        .iter()
        .map(|t| mail_model::MessageRef {
            mailbox_id: inbox,
            message_id: t.message.id,
        })
        .collect();
    let t = time(runs, || async {
        core.conversation_targets(
            first_rows.clone(),
            mail_model::ConversationIntent::Action {
                action: mail_model::MailAction::Archive,
            },
        )
        .await
        .expect("targets");
    })
    .await;
    row("Ziele einer Aktion auf 100 Konversationen", &t);
    // The longest thread, opened.
    let longest = core
        .list_threads(newsletter, None, 100)
        .await
        .expect("threads")
        .items
        .into_iter()
        .max_by_key(|t| t.message_count)
        .expect("a thread");
    let t = time(runs.min(50), || async {
        core.get_thread(longest.message.id, Some(newsletter))
            .await
            .expect("thread");
    })
    .await;
    row(
        &format!("Thread öffnen ({} Mails)", longest.message_count),
        &t,
    );

    // Stage 19 (decisions 0061, 0062): the inbox and the unified inbox sorted by other keys,
    // with the quick filter, and the counts of the filter bar.
    {
        use mail_model::{ListOrder, ListViewRequest, QuickFilter, SortKey};
        let view =
            |scope: &mail_model::InboxScope, key, ascending, filter: QuickFilter| ListViewRequest {
                scope: scope.clone(),
                order: ListOrder { key, ascending },
                filter,
            };
        let filter = |f: fn(&mut QuickFilter)| {
            let mut q = QuickFilter::default();
            f(&mut q);
            q
        };
        for (name, key, ascending) in [
            ("Datum, älteste zuerst", SortKey::Date, true),
            ("Absender A–Z", SortKey::Sender, true),
            ("Absender Z–A", SortKey::Sender, false),
            ("Empfänger A–Z", SortKey::Recipient, true),
            ("Betreff A–Z", SortKey::Subject, true),
            ("Größe, größte zuerst", SortKey::Size, false),
        ] {
            let request = view(&scope, key, ascending, QuickFilter::default());
            let t = time(runs, || async {
                core.list_view_messages(request.clone(), None, 100)
                    .await
                    .expect("view");
            })
            .await;
            row(&format!("Stufe 19: Posteingang nach {name} (100)"), &t);
            worst.insert(
                "view",
                worst.get("view").copied().unwrap_or_default().max(t.median),
            );
        }
        // Keyset from the middle: the cursor after half of the inbox, by sender.
        let by_sender = view(&scope, SortKey::Sender, true, QuickFilter::default());
        let mut cursor = None;
        let mut seen = 0usize;
        while seen < ids.len() / 2 {
            let page = core
                .list_view_messages(by_sender.clone(), cursor.clone(), 500)
                .await
                .expect("page");
            seen += page.items.len();
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        let t = time(runs, || async {
            core.list_view_messages(by_sender.clone(), cursor.clone(), 100)
                .await
                .expect("view");
        })
        .await;
        row("Stufe 19: … nach Absender, Keyset ab Mitte", &t);
        let t = time(runs, || async {
            core.list_view_conversations(by_sender.clone(), None, 100)
                .await
                .expect("view");
        })
        .await;
        row(
            "Stufe 19: Posteingang nach Absender, Konversationen (100)",
            &t,
        );
        worst.insert("view conversations", t.median);
        let unified_by_sender = view(&unified, SortKey::Sender, true, QuickFilter::default());
        let t = time(runs, || async {
            core.list_view_messages(unified_by_sender.clone(), None, 100)
                .await
                .expect("view");
        })
        .await;
        row(
            "Stufe 19: Vereinter Posteingang nach Absender (5 Konten, 100)",
            &t,
        );
        let t = time(runs, || async {
            core.list_view_conversations(unified_by_sender.clone(), None, 100)
                .await
                .expect("view");
        })
        .await;
        row(
            "Stufe 19: Vereinter Posteingang nach Absender, Konversationen (100)",
            &t,
        );
        let rare = data.rare.clone();
        let filters: Vec<(String, SortKey, QuickFilter)> = vec![
            (
                "Ungelesen, Datum".into(),
                SortKey::Date,
                filter(|q| q.unread = true),
            ),
            (
                "Ungelesen, Absender".into(),
                SortKey::Sender,
                filter(|q| q.unread = true),
            ),
            (
                "Markiert, Datum".into(),
                SortKey::Date,
                filter(|q| q.flagged = true),
            ),
            (
                "Markiert, Absender".into(),
                SortKey::Sender,
                filter(|q| q.flagged = true),
            ),
            (
                "Mit Anhang, Betreff".into(),
                SortKey::Subject,
                filter(|q| q.attachment = true),
            ),
            (
                "Von Kontakten (keiner im Generator: ganzer Ordner)".into(),
                SortKey::Date,
                filter(|q| q.contacts = true),
            ),
            (
                "Schlagwort ohne Treffer (ganzer Ordner)".into(),
                SortKey::Sender,
                filter(|q| q.tag = Some("nirgends".into())),
            ),
            (
                "Text `rechnung`, Datum".into(),
                SortKey::Date,
                filter(|q| q.text = "rechnung".into()),
            ),
            (
                "Text `rechnung`, Absender".into(),
                SortKey::Sender,
                filter(|q| q.text = "rechnung".into()),
            ),
            (
                "Text beim Tippen `rech`, Datum".into(),
                SortKey::Date,
                filter(|q| q.text = "rech".into()),
            ),
            (
                format!("seltenes Wort `{rare}`, Datum"),
                SortKey::Date,
                QuickFilter {
                    text: rare.clone(),
                    ..QuickFilter::default()
                },
            ),
            (
                format!("seltenes Wort `{rare}`, Absender"),
                SortKey::Sender,
                QuickFilter {
                    text: rare.clone(),
                    ..QuickFilter::default()
                },
            ),
            (
                "Ungelesen + Anhang + `rechnung`, Datum".into(),
                SortKey::Date,
                filter(|q| {
                    q.unread = true;
                    q.attachment = true;
                    q.text = "rechnung".into();
                }),
            ),
        ];
        let mut slowest_filter = Duration::ZERO;
        let mut slowest_count = Duration::ZERO;
        for (name, key, quick) in filters {
            let request = view(&scope, key, false, quick);
            let t = time(runs, || async {
                core.list_view_messages(request.clone(), None, 100)
                    .await
                    .expect("view");
            })
            .await;
            let rows = core
                .list_view_messages(request.clone(), None, 100)
                .await
                .expect("view")
                .items
                .len();
            row(
                &format!("Stufe 19: Schnellfilter {name} ({rows} Zeilen)"),
                &t,
            );
            slowest_filter = slowest_filter.max(t.median);
            let t = time(runs.min(20), || async {
                core.count_list_view(request.clone()).await.expect("count");
            })
            .await;
            let count = core.count_list_view(request.clone()).await.expect("count");
            row(&format!("Stufe 19: … Trefferzahl ({count})"), &t);
            slowest_count = slowest_count.max(t.median);
        }
        worst.insert("view filter", slowest_filter);
        worst.insert("view count", slowest_count);
        let t = time(runs.min(20), || async {
            core.list_view_tags(scope.clone()).await.expect("tags");
        })
        .await;
        row("Stufe 19: Schlagwörter des Posteingangs (Menü)", &t);
    }

    let mut slowest = Duration::ZERO;
    for (name, query, typing) in queries(data) {
        let t = time(runs, || async {
            core.search(search_request(&query, typing))
                .await
                .expect("search");
        })
        .await;
        let hits = core
            .search(search_request(&query, typing))
            .await
            .expect("search")
            .hits
            .len();
        row(&format!("Suche: {name} (`{query}`, {hits} Treffer)"), &t);
        slowest = slowest.max(t.median);
    }
    worst.insert("search", slowest);

    // Switching between locally stored messages: getMessage of stored bodies (parse, sanitise,
    // render), HTML and text.
    let store = core.store_for_tests().clone();
    let mut local = Vec::new();
    for id in &ids {
        if store
            .body_info(MessageId(*id))
            .await
            .expect("info")
            .is_some_and(|i| i.complete)
        {
            local.push(MessageId(*id));
            if local.len() == 200 {
                break;
            }
        }
    }
    let mut next = 0usize;
    let t = time(local.len(), || {
        let id = local[next % local.len()];
        next += 1;
        let core = core.clone();
        async move {
            core.get_message(id, BodyFormat::Html)
                .await
                .expect("message");
        }
    })
    .await;
    row(
        &format!("Mail öffnen, Body lokal ({} Mails)", local.len()),
        &t,
    );
    worst.insert("open", t.p95);
    core.shutdown().await;
    worst
}

/// The background jobs that still have work, by name (empty when none).
async fn background_pending(store: &Store) -> String {
    let mut pending = Vec::new();
    if store.category_work_pending().await.expect("categories") {
        pending.push("categories");
    }
    if !store.contacts_todo(1).await.expect("contacts").is_empty() {
        pending.push("contacts");
    }
    if store.background_work_pending().await.expect("index") {
        pending.push("index and threads");
    }
    pending.join(", ")
}

/// Starts the core on `data` and waits until its background jobs have nothing left (at most
/// ten minutes); how long that took and what was left.
async fn settle(data: &Dataset) -> (f64, String) {
    let started = Instant::now();
    let core = start_core(&data.dir.path().join("data")).await;
    let store = core.store_for_tests().clone();
    let mut left = background_pending(&store).await;
    while !left.is_empty() && started.elapsed() < Duration::from_secs(600) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        left = background_pending(&store).await;
    }
    drop(store);
    core.shutdown().await;
    (started.elapsed().as_secs_f64(), left)
}

/// Most resident memory the start alone may add (database pages read through the mapping,
/// caches, threads). Measured 8 MB at 100,000 and 500,000 messages; a start that reads the
/// `message` table once adds about as much as the table (80 MB at 100,000 messages).
const START_READ_LIMIT_MB: f64 = 40.0;

/// Starts a child process that measures start and idle memory on `data`.
fn child(data: &Dataset) -> String {
    let exe = std::env::current_exe().expect("test binary");
    let out = std::process::Command::new(exe)
        .args([
            "--ignored",
            "--exact",
            "search::bench::child_measure",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("VICEMAIL_BENCH_DATA", data.dir.path().join("data"))
        .env("VICEMAIL_BENCH_INBOX", data.largest_inbox.0.to_string())
        .output()
        .expect("child");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // libtest prints `test … ... ` and the test's output on the same line.
    let lines: Vec<String> = text
        .lines()
        .filter_map(|l| l.find("CHILD ").map(|i| l[i + 6..].to_owned()))
        .collect();
    assert!(
        out.status.success() && !lines.is_empty(),
        "child measurement failed: {text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    lines.join("\n")
}

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .map(|kb| kb / 1024.0)
        .unwrap_or(f64::NAN)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "child of the benchmarks"]
async fn child_measure() {
    let Some(data) = std::env::var_os("VICEMAIL_BENCH_DATA") else {
        return;
    };
    let inbox = MailboxId(
        std::env::var("VICEMAIL_BENCH_INBOX")
            .expect("inbox")
            .parse()
            .expect("inbox id"),
    );
    let before = rss_mb();
    let started = Instant::now();
    let core = start_core(Path::new(&data)).await;
    let opened = started.elapsed();
    core.list_messages(inbox, None, 100)
        .await
        .expect("first page");
    let usable = started.elapsed();
    let after_start = rss_mb();
    // What a user does first: threads, the unified inbox, a few searches.
    core.list_threads(inbox, None, 100).await.expect("threads");
    core.list_unified_inbox(UnifiedView::All, None, 100)
        .await
        .expect("unified");
    for q in ["rechnung", "rech", "\"viele grüße\"", "is:unread in:inbox"] {
        core.search(search_request(q, q == "rech"))
            .await
            .expect("search");
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let idle = rss_mb();
    let left = background_pending(core.store_for_tests()).await;
    println!(
        "CHILD start: core started {:.0} ms, first list page {:.0} ms after start; resident memory {before:.0} MB before, {after_start:.0} MB after start and first page, {idle:.0} MB idle after loading; background work left: {}",
        ms(opened),
        ms(usable),
        if left.is_empty() { "none" } else { &left }
    );
    core.shutdown().await;
    assert!(
        after_start - before < START_READ_LIMIT_MB,
        "the start made {:.0} MB resident (limit {START_READ_LIMIT_MB} MB): it reads the database broadly",
        after_start - before
    );
    assert!(
        usable < Duration::from_secs(1),
        "first list page after {usable:?}"
    );
    assert!(idle < 250.0, "{idle:.0} MB idle");
}

fn file_mb(path: &Path) -> f64 {
    std::fs::metadata(path)
        .map(|m| m.len() as f64 / 1e6)
        .unwrap_or(0.0)
}

fn dir_mb(path: &Path) -> f64 {
    fn walk(path: &Path) -> u64 {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| {
                        let p = e.path();
                        if p.is_dir() {
                            walk(&p)
                        } else {
                            e.metadata().map(|m| m.len()).unwrap_or(0)
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    walk(path) as f64 / 1e6
}

async fn report(total: usize, runs: usize) {
    let started = Instant::now();
    let data = generate(total).await;
    println!(
        "\ngenerated {} messages over 5 accounts in {:.1} s; store_headers {:.2} s in {} windows ({:.1} ms per window of 500, {:.0} mails/s, threads and index included); {} bodies stored in {:.1} s ({:.0} bodies/s, index text included)",
        data.messages,
        started.elapsed().as_secs_f64(),
        data.store_secs,
        data.windows,
        data.store_secs * 1000.0 / data.windows as f64,
        data.messages as f64 / data.store_secs,
        data.bodies,
        data.body_secs,
        data.bodies as f64 / data.body_secs,
    );
    let path = data_path(&data.dir);
    let store = Store::open(&path, StoreOptions::default()).expect("open");
    let sizes = store.search_index_status().await.expect("status");
    drop(store);
    println!(
        "database {:.0} MB, bodies {:.0} MB, index state {:?}",
        file_mb(&path),
        dir_mb(&data.dir.path().join("data").join("bodies")),
        sizes.state
    );
    measure(&data, runs).await;
    // The generator stores as stage 7 did, without the classifier: the categories are left to
    // catch up as after an upgrade from before stage 9, the contact directory as after every
    // first sync. The child measures the warm start after that one-time work, like stage 7
    // (which had no such work).
    let (caught_up, still) = settle(&data).await;
    println!(
        "background catch-up after the measurements: {caught_up:.1} s ({})",
        if still.is_empty() { "done" } else { &still }
    );
    println!("{}", child(&data));

    // The rebuild in the background: the whole index again, bodies parsed again.
    let store = Store::open(&path, StoreOptions::default()).expect("open");
    store.set_body_text_source(Arc::new(ContentText));
    let started = Instant::now();
    assert!(store.rebuild_search_index().await.expect("rebuild"));
    while let mail_store::IndexStep::Progress(_) = store.search_index_step().await.expect("step") {}
    println!(
        "index rebuild of {} messages ({} bodies parsed again): {:.1} s",
        data.messages,
        data.bodies,
        started.elapsed().as_secs_f64()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "benchmark: cargo test -p mail-engine --release --lib search::bench -- --ignored --nocapture --test-threads 1"]
async fn bench_100k() {
    report(100_000, 100).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "benchmark: cargo test -p mail-engine --release --lib search::bench -- --ignored --nocapture --test-threads 1"]
async fn bench_500k() {
    report(500_000, 50).await;
}

/// Runs with every `cargo test` (debug build, small data): far above the targets, so only a
/// real regression (a scan, a sort over a whole mailbox) fails it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn small_data_meets_generous_thresholds() {
    let data = generate(3_000).await;
    let worst = measure(&data, 5).await;
    let limit = |name: &str, max_ms: u64| {
        let got = worst[name];
        assert!(
            got < Duration::from_millis(max_ms),
            "{name}: {got:?} is above the generous limit of {max_ms} ms"
        );
    };
    limit("list", 50);
    limit("unified", 100);
    limit("threads", 200);
    limit("conversations", 200);
    limit("unified conversations", 300);
    limit("search", 200);
    limit("open", 500);
    // Stage 19: sorted views, filters (a filter without matches walks the whole inbox) and
    // the counts of the filter bar.
    limit("view", 100);
    limit("view conversations", 300);
    limit("view filter", 300);
    limit("view count", 300);
}
