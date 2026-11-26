//! Stage 22 (decision 0074) in the store: the trigram index for search inside words. Queries
//! compiled by `mail_model::search` run against both indexes (substrings, the word index for
//! short and quoted terms, both for terms of several words, contacts, exclusions, paging, FTS5
//! injection); a single word finds the same in the trigram index alone as in both; migration
//! 0029 fills the trigram index in the background and drops a rebuild it cannot continue.

use std::collections::BTreeSet;
use std::sync::Arc;

use mail_model::search::{
    CompileOptions, FixedOffset, SearchContext, SlashDateOrder, Substrings, TextMatch,
    contact_match, parse,
};
use mail_model::{Address, ListCursor, MessageHeader, MessageId};
use rusqlite_migration::M;

use crate::migrations::MIGRATIONS;
use crate::stage7_tests::{
    TestText, assert_index_matches, raw, run_index_job, search_ids, with_text,
};
use crate::store::Schema;
use crate::test_support::{UV, db_path, header, new_account, remote};
use crate::{BodyTextSource, IndexStep, MailboxBaseline, SearchFilter, Store, StoreOptions, fts};

const D: i64 = 1_790_000_000;

fn ctx() -> SearchContext<'static> {
    SearchContext {
        now: D,
        time_zone: &FixedOffset(0),
        slash_dates: SlashDateOrder::DayFirst,
    }
}

fn text(input: &str, substrings: Substrings, typing: bool) -> TextMatch {
    parse(input, &ctx())
        .compile(
            &ctx(),
            CompileOptions {
                prefix_last_word: typing,
                substrings,
            },
        )
        .text
}

fn filter(text: TextMatch) -> SearchFilter {
    SearchFilter {
        text,
        ..SearchFilter::default()
    }
}

fn person(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.into(),
    }
}

/// One test message: uid, subject, from, to, cc, body.
type Mail = (
    u32,
    &'static str,
    Address,
    Address,
    Option<Address>,
    Option<&'static str>,
);

/// The messages of the tests.
fn mails() -> Vec<Mail> {
    let me = person(None, "me@example.org");
    vec![
        (
            1,
            "Muster für dich",
            person(Some("Max Mustermann"), "max.mustermann@firma.example"),
            me.clone(),
            None,
            Some("Hallo"),
        ),
        (
            2,
            "Bestellung",
            person(Some("Shop"), "shop@laden.example"),
            me.clone(),
            None,
            Some("Ihre Bestellnummer AB1234XY ist da, ab morgen"),
        ),
        (
            3,
            "Re: Termin",
            me.clone(),
            person(Some("Jan"), "jan@firma.de"),
            None,
            None,
        ),
        (
            4,
            "Grüße aus Jürgens Garten",
            me.clone(),
            person(None, "an@firma.de"),
            None,
            None,
        ),
        (
            5,
            "Kunde und Hund",
            person(None, "kunde@laden.example"),
            me.clone(),
            None,
            Some("und so weiter"),
        ),
        (
            6,
            "vielen dank",
            person(None, "eve@example.net"),
            me.clone(),
            Some(person(Some("Bob"), "bob@example.com")),
            Some("Vielen\nDank für alles"),
        ),
        (
            7,
            "Rechnung",
            person(None, "buchhaltung@laden.example"),
            me,
            None,
            None,
        ),
    ]
}

fn id(uid: u32) -> i64 {
    D + i64::from(uid) * 100
}

fn headers() -> Vec<MessageHeader> {
    mails()
        .into_iter()
        .map(|(uid, subject, from, to, cc, _)| {
            let mut h = header(uid, id(uid), Some(&format!("{uid}@sub")));
            h.subject = subject.into();
            h.from = vec![from];
            h.to = vec![to];
            h.cc = cc.into_iter().collect();
            h.has_attachments = uid == 7;
            h
        })
        .collect()
}

/// The test messages with their bodies (and an attachment for 7), stored through the API.
async fn stored() -> crate::test_support::Fixture {
    let f = with_text().await;
    f.store
        .store_headers(f.inbox, UV, headers(), None)
        .await
        .expect("headers");
    for (uid, _, _, _, _, body) in mails() {
        let body = match (uid, body) {
            (7, _) => raw("Anbei", &["Rechnung_2026-00042.pdf"]),
            (_, Some(text)) => raw(text, &[]),
            (_, None) => continue,
        };
        f.store
            .store_body(MessageId(id(uid)), body)
            .await
            .expect("body");
    }
    f
}

fn uids(ids: &[i64]) -> Vec<u32> {
    ids.iter()
        .map(|i| u32::try_from((i - D) / 100).expect("uid"))
        .collect()
}

#[tokio::test]
async fn terms_match_inside_words_as_the_language_says() {
    let f = stored().await;
    let store = &f.store;
    // (input, typing, uids newest first); the same with the index being filled and complete.
    let cases: &[(&str, bool, &[u32])] = &[
        ("uster", false, &[1]),
        ("USTER", false, &[1]),
        ("1234", false, &[2]),
        ("1234xy", false, &[2]),
        ("an@firma", false, &[4, 3]),
        ("@firma", false, &[4, 3, 1]),
        ("urgen", false, &[4]),
        ("jurgen", false, &[4]),
        ("jürgens", false, &[4]),
        // Short terms are words: "ab" the word, not inside "AB1234XY" or "aber".
        ("ab", false, &[2]),
        ("ab 1234", false, &[2]),
        ("ab uster", false, &[]),
        // Quoted: whole words only.
        ("\"und\"", false, &[5]),
        ("\"uster\"", false, &[]),
        ("und", false, &[5]),
        // Several words, and a phrase across a line break (word index).
        ("vielen dank", false, &[6]),
        ("\"vielen dank\"", false, &[6]),
        ("ob@example.co", false, &[6]),
        ("bob@example.com", false, &[6]),
        ("cc:example.com", false, &[6]),
        ("from:uster", false, &[1]),
        ("to:uster", false, &[]),
        ("rech", true, &[7]),
        ("echnun", false, &[7]),
        ("filename:2026-000", false, &[7]),
        ("filename:00042", false, &[7]),
        ("uster OR 1234", false, &[2, 1]),
        ("uster OR ab", false, &[2, 1]),
        ("-uster", false, &[7, 6, 5, 4, 3, 2]),
        ("-ab -uster -firma", false, &[7, 6, 5]),
        ("firma -an@firma", false, &[1]),
        ("@firma -jan", false, &[4, 1]),
    ];
    let mut failures = Vec::new();
    for (input, typing, want) in cases {
        for substrings in [Substrings::Filling, Substrings::Complete] {
            let got = uids(&search_ids(store, filter(text(input, substrings, *typing))).await);
            if got != *want {
                failures.push(format!(
                    "{input:?} ({substrings:?}): want {want:?}, got {got:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    // Without the trigram index (as before decision 0074) "uster" finds nothing.
    assert!(
        search_ids(store, filter(text("uster", Substrings::Off, false)))
            .await
            .is_empty()
    );

    // Contacts: every address a phrase in sender, recipients and copy; `an@firma.de` is not
    // `jan@firma.de`.
    let contact = |addresses: &[&str]| {
        let mut text = TextMatch::default();
        text.require_words(
            contact_match(
                &addresses
                    .iter()
                    .map(|a| (*a).to_owned())
                    .collect::<Vec<_>>(),
            )
            .expect("expression"),
        );
        filter(text)
    };
    assert_eq!(
        uids(&search_ids(store, contact(&["an@firma.de"])).await),
        [4]
    );
    assert_eq!(
        uids(
            &search_ids(
                store,
                contact(&["jan@firma.de", "max.mustermann@firma.example"])
            )
            .await
        ),
        [3, 1]
    );
    assert_eq!(
        uids(&search_ids(store, contact(&["BOB@Example.com"])).await),
        [6]
    );
    let mut without = text("firma", Substrings::Complete, false);
    without.exclude_words(contact_match(&["an@firma.de".into()]).expect("expression"));
    assert_eq!(uids(&search_ids(store, filter(without)).await), [3, 1]);
}

/// Pages through a search with one hit per page; newest first, each once.
async fn paged(store: &Store, filter: SearchFilter) -> Vec<i64> {
    let mut out = Vec::new();
    let mut cursor: Option<ListCursor> = None;
    for _ in 0..100 {
        let page = store
            .search(filter.clone(), cursor, 1)
            .await
            .expect("search");
        out.extend(page.hits.iter().map(|h| h.item.id.0));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return out,
        }
    }
    panic!("the search did not end");
}

#[tokio::test]
async fn both_indexes_page_newest_first_and_inputs_never_break_the_query() {
    let f = stored().await;
    let store = &f.store;
    // A term in either index walks both, merged by date.
    for substrings in [Substrings::Filling, Substrings::Complete] {
        assert_eq!(
            uids(&paged(store, filter(text("@firma", substrings, false))).await),
            [4, 3, 1]
        );
        assert_eq!(
            uids(&paged(store, filter(text("-uster", substrings, false))).await),
            [7, 6, 5, 4, 3, 2]
        );
    }
    // Dates and the cursor bound both indexes.
    let mut dated = filter(text("@firma", Substrings::Complete, false));
    dated.dates.from = Some(id(2));
    dated.dates.until = Some(id(4));
    assert_eq!(uids(&search_ids(store, dated).await), [3]);

    // Whatever is typed: no FTS5 or SQL error, in any mode.
    let inputs = [
        "{from}:x",
        "{from to}: abc",
        "NEAR(abc def)",
        "\"abc\" OR \"def",
        "x\"y\"z",
        "^start",
        "a*b",
        "abc*",
        "\"abc\"*",
        "col:xyz",
        "-\"",
        "\"\" \"\"",
        "AND OR NOT",
        "abc OR",
        "OR abc",
        "\u{0}abc\u{7f}",
        "🎉🎉🎉",
        "e\u{301}e\u{301}e\u{301}",
        "-abc -\"de f\" -ghi*",
        "from:\"x OR y\" to:* cc:-abc",
        "abc AND -abc",
        "'; DROP TABLE message; --",
        "contact:\"a OR b\"",
        "a:b:c",
        "***",
        "-*",
        "\"",
    ];
    for input in inputs {
        for substrings in [Substrings::Off, Substrings::Filling, Substrings::Complete] {
            for typing in [false, true] {
                let compiled = text(input, substrings, typing);
                store
                    .search(filter(compiled.clone()), None, 50)
                    .await
                    .unwrap_or_else(|err| panic!("{input:?} ({substrings:?}): {err}"));
                let mut with_contact = compiled;
                if let Some(expr) = contact_match(&[input.to_owned()]) {
                    with_contact.require_words(expr);
                }
                store
                    .search(filter(with_contact), None, 50)
                    .await
                    .unwrap_or_else(|err| panic!("contact {input:?}: {err}"));
            }
        }
    }
}

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[usize::try_from(self.next() % items.len() as u64).unwrap_or(0)]
    }
}

/// Decision 0074: a single word in the trigram index alone finds exactly what it finds in both
/// indexes (each word hit is a substring hit of the same folded text), also with case,
/// diacritics, digits, prefixes and decomposed characters.
#[tokio::test]
async fn a_single_word_finds_the_same_in_the_trigram_index_alone() {
    let f = with_text().await;
    const WORDS: &[&str] = &[
        "Müller",
        "MULLER",
        "muller",
        "Straße",
        "STRASSE",
        "Grüße",
        "Mu\u{308}ller",
        "Rechnung",
        "Zurechnung",
        "rechnen",
        "Ärger",
        "arger",
        "Œuvre",
        "İstanbul",
        "istanbul",
        "Ελλάδα",
        "ελλαδα",
        "東京都",
        "AB1234XY",
        "1234",
        "x12345",
        "naïve",
        "NAÏVE",
        "café",
        "Café-Bar",
        "e-mail",
        "E‑Mail",
        "ﬁnance",
    ];
    let mut rng = Rng(0x5eed_0074);
    let mut headers = Vec::new();
    for uid in 1..=150_u32 {
        let mut h = header(uid, D + i64::from(uid) * 60, Some(&format!("{uid}@w")));
        let words: Vec<&str> = (0..4).map(|_| rng.pick(WORDS)).collect();
        h.subject = words.join(if uid % 2 == 0 { " " } else { ", " });
        h.from = vec![person(Some(rng.pick(WORDS)), "x@example.org")];
        headers.push(h);
    }
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("headers");
    let mut queries: BTreeSet<String> = BTreeSet::new();
    for word in WORDS {
        let chars: Vec<char> = word.chars().collect();
        for len in 3..=chars.len() {
            for start in [0, chars.len() - len] {
                let part: String = chars[start..start + len].iter().collect();
                if part.chars().all(char::is_alphanumeric) {
                    queries.insert(part);
                }
            }
        }
    }
    for query in &queries {
        for typing in [false, true] {
            let alone = text(query, Substrings::Complete, typing);
            let both = text(query, Substrings::Filling, typing);
            let words_only = text(query, Substrings::Off, typing);
            assert!(
                alone.all.iter().all(|c| c.words.is_none()),
                "{query:?}: {alone:?}"
            );
            let alone = search_ids(&f.store, filter(alone)).await;
            let both = search_ids(&f.store, filter(both)).await;
            assert_eq!(alone, both, "{query:?} (typing {typing})");
            let words: BTreeSet<i64> = search_ids(&f.store, filter(words_only))
                .await
                .into_iter()
                .collect();
            let alone: BTreeSet<i64> = alone.into_iter().collect();
            assert!(
                words.is_subset(&alone),
                "{query:?} (typing {typing}): word hits {:?} missing",
                words.difference(&alone).collect::<Vec<_>>()
            );
        }
    }
}

/// The schema just before migration 0029, wherever it stands in the list.
fn before_0029() -> Schema {
    let at = MIGRATIONS
        .iter()
        .position(|m| *m == M::up(include_str!("../migrations/0029_search_substrings.sql")))
        .expect("migration 0029 is in the list");
    Schema::from_vec(MIGRATIONS[..at].to_vec())
}

/// A database before migration 0029 with the test messages, their headers in the word index
/// (no job), and the inbox; `rebuilding`: a rebuild of the word index was running.
async fn old_database(dir: &tempfile::TempDir, rebuilding: bool) -> mail_model::MailboxId {
    let path = db_path(dir);
    let store =
        Store::open_with(&path, StoreOptions::default(), &before_0029()).expect("before 0029");
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(account, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let inbox = store.mailbox_sync_states(account).await.expect("mailboxes")[0].mailbox_id;
    store
        .reset_mailbox(
            inbox,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    store
        .write(move |tx| {
            let headers = headers();
            crate::test_support::stage2_store_headers(tx, inbox, &headers)?;
            let mut rows: Vec<(i64, fts::Row)> = Vec::new();
            for h in &headers {
                let (row, _) = fts::stored_row(tx, h.sort_date())?.expect("stored");
                rows.push((h.sort_date(), row));
            }
            fts::write(tx, &[fts::Table::Words], &mut rows)?;
            assert!(fts::job(tx)?.is_none());
            if rebuilding {
                // As `start_rebuild` did before 0029: one new table, its trigger, the job.
                tx.execute_batch(
                    "CREATE VIRTUAL TABLE message_fts_new USING fts5(
                       subject, \"from\", \"to\", cc, body, attachments,
                       content = '', contentless_delete = 1,
                       tokenize = 'unicode61 remove_diacritics 2');
                     CREATE TRIGGER message_fts_new_delete AFTER DELETE ON message BEGIN
                       DELETE FROM message_fts_new WHERE rowid = old.id;
                     END;
                     INSERT INTO search_index_job (id, target, next_id, started_at)
                     VALUES (1, 'message_fts_new', 0, 0);",
                )?;
            }
            Ok(())
        })
        .await
        .expect("old rows");
    inbox
}

#[tokio::test]
async fn migration_0029_fills_the_trigram_index_in_the_background() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let inbox = old_database(&dir, false).await;
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("migrate");
    store.set_body_text_source(Arc::new(TestText));
    let status = store.search_index_status().await.expect("status");
    assert_eq!(status.state, mail_model::SearchIndexState::Building);

    // While it fills, both indexes together still find every word (the engine compiles with
    // `Filling`); substrings only where the trigram index has the message.
    assert_eq!(
        uids(
            &search_ids(
                &store,
                filter(text("Mustermann", Substrings::Filling, false))
            )
            .await
        ),
        [1]
    );
    assert!(
        search_ids(
            &store,
            filter(text("ustermann", Substrings::Filling, false))
        )
        .await
        .is_empty()
    );
    // Written meanwhile: a new message and a body, into both indexes at once.
    let mut late = header(8, id(8), Some("8@sub"));
    late.subject = "Nachzügler".into();
    store
        .store_headers(inbox, UV, vec![late], None)
        .await
        .expect("new message");
    store
        .store_body(MessageId(id(2)), raw("Ihre Bestellnummer AB1234XY", &[]))
        .await
        .expect("body");
    assert_eq!(
        uids(&search_ids(&store, filter(text("chzügl", Substrings::Filling, false))).await),
        [8]
    );

    assert_eq!(run_index_job(&store).await, 1);
    assert_eq!(
        store.search_index_status().await.expect("status").state,
        mail_model::SearchIndexState::Ready
    );
    assert_eq!(
        store.search_index_step().await.expect("step"),
        IndexStep::Idle
    );
    let mut texts = std::collections::BTreeMap::new();
    texts.insert(
        id(2),
        TestText.complete(&raw("Ihre Bestellnummer AB1234XY", &[])),
    );
    assert_index_matches(&store, &texts, "after migration 0029").await;
    for substrings in [Substrings::Filling, Substrings::Complete] {
        assert_eq!(
            uids(&search_ids(&store, filter(text("ustermann", substrings, false))).await),
            [1]
        );
        assert_eq!(
            uids(&search_ids(&store, filter(text("1234", substrings, false))).await),
            [2]
        );
    }
}

#[tokio::test]
async fn migration_0029_drops_a_rebuild_it_cannot_continue() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    old_database(&dir, true).await;
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("migrate");
    store.set_body_text_source(Arc::new(TestText));
    let leftovers: Vec<String> = store
        .read(|c| {
            Ok(c.prepare(
                "SELECT name FROM sqlite_schema WHERE name LIKE 'message_fts_new%'
                 OR name = 'message_fts_new_delete'",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?)
        })
        .await
        .expect("schema");
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let status = store.search_index_status().await.expect("status");
    assert_eq!(status.state, mail_model::SearchIndexState::Building);
    run_index_job(&store).await;
    assert_index_matches(&store, &std::collections::BTreeMap::new(), "after 0029").await;
    // A rebuild now renews both indexes.
    assert!(store.rebuild_search_index().await.expect("rebuild"));
    run_index_job(&store).await;
    assert_index_matches(
        &store,
        &std::collections::BTreeMap::new(),
        "after a rebuild",
    )
    .await;
    assert_eq!(
        uids(
            &search_ids(
                &store,
                filter(text("ustermann", Substrings::Complete, false))
            )
            .await
        ),
        [1]
    );
}
