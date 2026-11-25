//! Stage 19, task A in the store: sorted and filtered list views (decisions 0061, 0062), the
//! sort keys of every location (migration 0026) and the plans of the view queries.

use mail_model::search::{RowidRange, TextMatch};
use mail_model::{
    AccountId, Address, ListOrder, MailboxId, MessageHeader, MessageId, SortKey, ViewCursor,
    sender_sort_key, subject_sort_key,
};
use rusqlite::Connection;

use crate::test_support::{Fixture, UV, db_path, fixture, header, new_account, remote};
use crate::views::{ResolvedView, ViewFilter, ViewQuery, statement};
use crate::{InboxMailboxes, MailboxBaseline, SearchFilter, Store, StoreOptions};

const D: i64 = 1_790_000_000;
const NOW: i64 = D * 1000;

const ORDERS: [SortKey; 5] = [
    SortKey::Date,
    SortKey::Sender,
    SortKey::Recipient,
    SortKey::Subject,
    SortKey::Size,
];

fn order(key: SortKey, ascending: bool) -> ListOrder {
    ListOrder { key, ascending }
}

fn person(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.into(),
    }
}

/// The 24 inbox messages: senders with case, umlauts, quotes, no name and no sender at all;
/// subjects with prefixes and none; sizes with ties; flags, tags and attachments.
fn varied(k: u32) -> MessageHeader {
    let mut h = header(k + 1, D + i64::from(k) * 60, Some(&format!("{k}@v")));
    let senders: [Option<Address>; 8] = [
        Some(person(Some("Anna Weber"), "anna@example.org")),
        Some(person(Some("anna weber"), "anna2@example.org")),
        Some(person(Some("Älter, Bernd"), "bernd@example.org")),
        Some(person(Some("Zoë Ångström"), "zoe@example.org")),
        Some(person(None, "Carl@Example.org")),
        None,
        Some(person(Some("\"Quote\" Q"), "q@example.org")),
        Some(person(Some("Bob"), "bob@example.org")),
    ];
    h.from = senders[k as usize % senders.len()]
        .iter()
        .cloned()
        .collect();
    let subjects = [
        "Re: Angebot",
        "Angebot",
        "",
        "AW: Über uns",
        "zebra",
        "Rechnung 12",
    ];
    h.subject = subjects[k as usize % subjects.len()].into();
    let recipients = ["Xaver", "maria", "Émile", "Bert"];
    h.to = vec![person(
        Some(recipients[k as usize % recipients.len()]),
        "to@example.org",
    )];
    h.size = 1_000 + (k % 5) * 100;
    h.flags.seen = !k.is_multiple_of(3);
    h.flags.flagged = k.is_multiple_of(4);
    if k.is_multiple_of(5) {
        h.flags.keywords = vec![
            if k.is_multiple_of(10) {
                "Projekt"
            } else {
                "PROJEKT"
            }
            .into(),
        ];
    }
    h.has_attachments = k.is_multiple_of(6);
    h
}

/// The keys a header sorts by, as the store computes them.
fn key_of(h: &MessageHeader, key: SortKey) -> (String, i64) {
    match key {
        SortKey::Date => (String::new(), 0),
        SortKey::Sender => {
            let from = h.from.first();
            (
                sender_sort_key(
                    from.and_then(|a| a.name.as_deref()),
                    from.map(|a| a.email.as_str()),
                ),
                0,
            )
        }
        SortKey::Recipient => {
            let to = h.to.first();
            (
                sender_sort_key(
                    to.and_then(|a| a.name.as_deref()),
                    to.map(|a| a.email.as_str()),
                ),
                0,
            )
        }
        SortKey::Subject => (subject_sort_key(&h.subject), 0),
        SortKey::Size => (String::new(), i64::from(h.size)),
    }
}

/// The expected order of `headers` (ids are their dates): by key, ties by id, one direction.
fn expected(headers: &[MessageHeader], o: ListOrder) -> Vec<i64> {
    let mut rows: Vec<((String, i64), i64)> = headers
        .iter()
        .map(|h| (key_of(h, o.key), h.sort_date()))
        .collect();
    rows.sort();
    if !o.ascending {
        rows.reverse();
    }
    rows.into_iter().map(|(_, id)| id).collect()
}

fn query(scope: InboxMailboxes, o: ListOrder, filter: ViewFilter) -> ViewQuery {
    ViewQuery {
        scope,
        order: o,
        filter,
    }
}

/// Every row of a view, one row per message, paging with `limit`.
async fn all_messages(store: &Store, q: &ViewQuery, limit: u32) -> Vec<i64> {
    let mut out = Vec::new();
    let mut cursor: Option<ViewCursor> = None;
    for _ in 0..1_000 {
        let page = store
            .view_messages(q.clone(), cursor.clone(), limit)
            .await
            .expect("view page");
        assert!(page.items.len() <= limit as usize);
        out.extend(page.items.iter().map(|i| i.id.0));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return out,
        }
    }
    panic!("the view did not end");
}

/// Every row of a view, one row per conversation.
async fn all_conversations(store: &Store, q: &ViewQuery, limit: u32) -> Vec<(i64, u32)> {
    let mut out = Vec::new();
    let mut cursor: Option<ViewCursor> = None;
    for _ in 0..1_000 {
        let page = store
            .view_conversations(q.clone(), cursor.clone(), limit)
            .await
            .expect("view page");
        out.extend(page.items.iter().map(|t| (t.message.id.0, t.message_count)));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return out,
        }
    }
    panic!("the view did not end");
}

async fn store_in(f: &Fixture, mailbox: MailboxId, headers: Vec<MessageHeader>) {
    f.store
        .store_headers(mailbox, UV, headers, None)
        .await
        .expect("store headers");
}

async fn varied_inbox() -> (Fixture, Vec<MessageHeader>) {
    let f = fixture().await;
    let headers: Vec<MessageHeader> = (0..24).map(varied).collect();
    store_in(&f, f.inbox, headers.clone()).await;
    (f, headers)
}

#[tokio::test]
async fn every_order_pages_through_every_message_once_in_both_directions() {
    let (f, headers) = varied_inbox().await;
    for key in ORDERS {
        for ascending in [true, false] {
            let o = order(key, ascending);
            let want = expected(&headers, o);
            for limit in [1, 5, 24, 500] {
                let q = query(InboxMailboxes::Mailbox(f.inbox), o, ViewFilter::default());
                let got = all_messages(&f.store, &q, limit).await;
                assert_eq!(got, want, "{o:?}, limit {limit}");
            }
        }
    }
    // Date descending is the folder's own list.
    let q = query(
        InboxMailboxes::Mailbox(f.inbox),
        ListOrder::default(),
        ViewFilter::default(),
    );
    assert_eq!(
        all_messages(&f.store, &q, 7).await,
        crate::test_support::all_ids(&f.store, f.inbox, 7).await
    );
}

#[tokio::test]
async fn keys_fold_case_and_accents_and_empty_values_sort_first() {
    let (f, headers) = varied_inbox().await;
    let q = query(
        InboxMailboxes::Mailbox(f.inbox),
        order(SortKey::Sender, true),
        ViewFilter::default(),
    );
    let ids = all_messages(&f.store, &q, 500).await;
    let sender = |id: i64| {
        headers
            .iter()
            .find(|h| h.sort_date() == id)
            .and_then(|h| h.from.first())
            .map(|a| a.name.clone().unwrap_or_else(|| a.email.clone()))
            .unwrap_or_default()
    };
    let names: Vec<String> = ids.iter().map(|id| sender(*id)).collect();
    let mut runs: Vec<String> = Vec::new();
    for name in names {
        if runs.last() != Some(&name) {
            runs.push(name);
        }
    }
    // No sender first; `Älter` with the a's, `anna weber` and `Anna Weber` one run by date,
    // the quotes of `"Quote" Q` do not count.
    assert_eq!(runs.first().map(String::as_str), Some(""));
    let order_of = |name: &str| runs.iter().position(|r| r == name).expect(name);
    assert!(order_of("Älter, Bernd") < order_of("Anna Weber"));
    assert!(order_of("Bob") < order_of("Carl@Example.org"));
    assert!(order_of("Carl@Example.org") < order_of("\"Quote\" Q"));
    assert!(order_of("\"Quote\" Q") < order_of("Zoë Ångström"));

    // Empty subjects (no subject, nothing but a prefix) first ascending, last descending.
    let q = query(
        InboxMailboxes::Mailbox(f.inbox),
        order(SortKey::Subject, true),
        ViewFilter::default(),
    );
    let first = all_messages(&f.store, &q, 500).await[0];
    let subject = |id: i64| {
        headers
            .iter()
            .find(|h| h.sort_date() == id)
            .map(|h| h.subject.clone())
            .unwrap_or_default()
    };
    assert_eq!(subject(first), "");
    let q = query(
        InboxMailboxes::Mailbox(f.inbox),
        order(SortKey::Subject, false),
        ViewFilter::default(),
    );
    let last = *all_messages(&f.store, &q, 500).await.last().expect("rows");
    assert_eq!(subject(last), "");
}

#[tokio::test]
async fn a_cursor_of_another_order_is_refused() {
    let (f, _) = varied_inbox().await;
    let q = query(
        InboxMailboxes::Mailbox(f.inbox),
        order(SortKey::Sender, true),
        ViewFilter::default(),
    );
    let wrong = ViewCursor {
        text: None,
        number: Some(5),
        id: MessageId(D),
    };
    let err = f
        .store
        .view_messages(q, Some(wrong), 10)
        .await
        .expect_err("size cursor for a sender order");
    assert!(
        matches!(err, crate::StoreError::InvalidArgument(_)),
        "{err}"
    );
    let q = query(
        InboxMailboxes::Mailbox(f.inbox),
        ListOrder::default(),
        ViewFilter::default(),
    );
    for limit in [0, crate::MAX_PAGE_SIZE + 1] {
        assert!(f.store.view_messages(q.clone(), None, limit).await.is_err());
    }
}

/// `want` ids of `headers` that pass `keep`, in date order (newest first).
fn ids_where(headers: &[MessageHeader], keep: impl Fn(u32, &MessageHeader) -> bool) -> Vec<i64> {
    let mut ids: Vec<i64> = headers
        .iter()
        .enumerate()
        .filter(|(k, h)| keep(u32::try_from(*k).expect("k"), h))
        .map(|(_, h)| h.sort_date())
        .collect();
    ids.sort_unstable();
    ids.reverse();
    ids
}

#[tokio::test]
async fn quick_filters_combine_and_count() {
    let (f, headers) = varied_inbox().await;
    // Contacts: Anna (written to) and Bob (named by the user); Zoë only seen.
    f.store
        .write(|tx| {
            tx.execute_batch(
                "UPDATE message SET sender_key = lower(from_addr);
                 INSERT INTO contact (address_key, local_part, domain, rank, search, updated_at, sent)
                 VALUES ('anna@example.org', 'anna', 'example.org', 0, ' anna', 0, 3),
                        ('zoe@example.org', 'zoe', 'example.org', 0, ' zoe', 0, 0);
                 INSERT INTO contact (address_key, local_part, domain, rank, search, updated_at, user_name)
                 VALUES ('bob@example.org', 'bob', 'example.org', 0, ' bob', 0, 'Robert');",
            )?;
            Ok(())
        })
        .await
        .expect("contacts");
    let scope = InboxMailboxes::Mailbox(f.inbox);
    let check = |name: &'static str, filter: ViewFilter, want: Vec<i64>| {
        let store = f.store.clone();
        let scope = scope.clone();
        async move {
            let q = query(scope, ListOrder::default(), filter);
            assert_eq!(all_messages(&store, &q, 3).await, want, "{name}");
            assert_eq!(
                store.view_count(q).await.expect("count"),
                want.len() as u64,
                "{name} count"
            );
        }
    };
    let search = |s: SearchFilter| ViewFilter {
        search: s,
        ..ViewFilter::default()
    };
    check(
        "unread",
        search(SearchFilter {
            unread: Some(true),
            ..SearchFilter::default()
        }),
        ids_where(&headers, |k, _| k % 3 == 0),
    )
    .await;
    check(
        "unread and flagged",
        search(SearchFilter {
            unread: Some(true),
            flagged: Some(true),
            ..SearchFilter::default()
        }),
        ids_where(&headers, |k, _| k % 3 == 0 && k % 4 == 0),
    )
    .await;
    check(
        "attachment",
        search(SearchFilter {
            has_attachment: Some(true),
            ..SearchFilter::default()
        }),
        ids_where(&headers, |k, _| k % 6 == 0),
    )
    .await;
    check(
        "tag in any case",
        ViewFilter {
            keyword: Some("projekt".into()),
            ..ViewFilter::default()
        },
        ids_where(&headers, |k, _| k % 5 == 0),
    )
    .await;
    check(
        "a tag nobody has",
        ViewFilter {
            keyword: Some("proj".into()),
            ..ViewFilter::default()
        },
        Vec::new(),
    )
    .await;
    check(
        "contacts",
        ViewFilter {
            contacts: true,
            ..ViewFilter::default()
        },
        ids_where(&headers, |_, h| {
            h.from
                .first()
                .is_some_and(|a| a.email == "anna@example.org" || a.email == "bob@example.org")
        }),
    )
    .await;
    check(
        "text in the subject",
        search(SearchFilter {
            text: TextMatch::Match("\"angebot\"".into()),
            ..SearchFilter::default()
        }),
        ids_where(&headers, |_, h| h.subject.contains("Angebot")),
    )
    .await;
    check(
        "text without the excluded word",
        search(SearchFilter {
            text: TextMatch::Exclude("\"angebot\"".into()),
            ..SearchFilter::default()
        }),
        ids_where(&headers, |_, h| !h.subject.contains("Angebot")),
    )
    .await;
    let from = D + 10 * 60;
    let until = D + 20 * 60;
    check(
        "text, tag, unread and a date range",
        ViewFilter {
            search: SearchFilter {
                text: TextMatch::Match("\"angebot\"".into()),
                unread: Some(true),
                dates: RowidRange {
                    from: Some(from),
                    until: Some(until),
                },
                ..SearchFilter::default()
            },
            keyword: Some("PROJEKT".into()),
            contacts: false,
        },
        ids_where(&headers, |k, h| {
            h.subject.contains("Angebot")
                && k % 3 == 0
                && k % 5 == 0
                && (from..until).contains(&h.sort_date())
        }),
    )
    .await;
    check(
        "an empty set of accounts",
        search(SearchFilter {
            accounts: Some(Vec::new()),
            ..SearchFilter::default()
        }),
        Vec::new(),
    )
    .await;
    // Filtered and sorted at once.
    let filter = search(SearchFilter {
        unread: Some(true),
        ..SearchFilter::default()
    });
    let q = query(scope.clone(), order(SortKey::Subject, true), filter);
    let unread: Vec<MessageHeader> = headers
        .iter()
        .enumerate()
        .filter(|(k, _)| k % 3 == 0)
        .map(|(_, h)| h.clone())
        .collect();
    assert_eq!(
        all_messages(&f.store, &q, 2).await,
        expected(&unread, order(SortKey::Subject, true))
    );
    // Tags of the view: once, `$` keywords left out.
    f.store
        .write(move |tx| {
            tx.execute(
                "UPDATE message_location SET keywords = '$Forwarded Rechnung' WHERE message_id = ?1",
                [D + 60],
            )?;
            Ok(())
        })
        .await
        .expect("keywords");
    assert_eq!(
        f.store.view_tags(scope, 200).await.expect("tags"),
        ["Projekt", "Rechnung"]
    );
}

fn reply(mut h: MessageHeader, to: &str) -> MessageHeader {
    h.in_reply_to = Some(to.into());
    h.references = vec![to.into()];
    h
}

fn from(mut h: MessageHeader, name: &str) -> MessageHeader {
    h.from = vec![person(
        Some(name),
        &format!("{}@example.org", name.to_lowercase()),
    )];
    h
}

#[tokio::test]
async fn conversations_sort_by_their_newest_matching_message() {
    let f = fixture().await;
    // Thread A: Anna (unread), then Zed (read) in the inbox, then the user's reply in Sent.
    // Thread B: Bob alone. Thread C: Carl, then Yves.
    let a1 = from(header(1, D, Some("a1@t")), "Anna");
    let a2 = crate::test_support::seen(from(reply(header(2, D + 60, Some("a2@t")), "a1@t"), "Zed"));
    let b1 = from(header(3, D + 120, Some("b1@t")), "Bob");
    let c1 = from(header(4, D + 180, Some("c1@t")), "Carl");
    let c2 = from(reply(header(5, D + 240, Some("c2@t")), "c1@t"), "Yves");
    store_in(&f, f.inbox, vec![a1, a2, b1, c1, c2]).await;
    let a3 = from(reply(header(1, D + 300, Some("a3@t")), "a2@t"), "Alice");
    store_in(&f, f.sent, vec![a3]).await;
    let scope = InboxMailboxes::Mailbox(f.inbox);
    let rows = |o: ListOrder, filter: ViewFilter| {
        let store = f.store.clone();
        let scope = scope.clone();
        async move { all_conversations(&store, &query(scope, o, filter), 1).await }
    };
    // By sender A to Z: Bob (B), Yves (C, its newest), Zed (A, its newest in the inbox; the
    // reply in Sent does not count). Counts are the whole conversation.
    assert_eq!(
        rows(order(SortKey::Sender, true), ViewFilter::default()).await,
        [(D + 120, 1), (D + 240, 2), (D + 60, 3)]
    );
    assert_eq!(
        rows(order(SortKey::Sender, false), ViewFilter::default()).await,
        [(D + 60, 3), (D + 240, 2), (D + 120, 1)]
    );
    // Oldest first by date: the conversations by their newest message.
    assert_eq!(
        rows(order(SortKey::Date, true), ViewFilter::default()).await,
        [(D + 60, 3), (D + 120, 1), (D + 240, 2)]
    );
    // Unread only: Zed is read, so Anna stands for thread A and moves to the front.
    let unread = ViewFilter {
        search: SearchFilter {
            unread: Some(true),
            ..SearchFilter::default()
        },
        ..ViewFilter::default()
    };
    assert_eq!(
        rows(order(SortKey::Sender, true), unread.clone()).await,
        [(D, 3), (D + 120, 1), (D + 240, 2)]
    );
    // Stage 20's `-larger:` (size below 1,002 bytes) on the newer message of a thread too: Zed's
    // (1,002) does not match, so Anna's (1,001) stands for thread A; B and C have none.
    let small = ViewFilter {
        search: SearchFilter {
            size_below: Some(1_002),
            ..SearchFilter::default()
        },
        ..ViewFilter::default()
    };
    assert_eq!(rows(order(SortKey::Sender, true), small).await, [(D, 3)]);
    // The count is of messages, not conversations.
    assert_eq!(
        f.store
            .view_count(query(scope.clone(), ListOrder::default(), unread))
            .await
            .expect("count"),
        4
    );
}

/// A second account with its own inbox.
async fn second_account(store: &Store) -> (AccountId, MailboxId) {
    let account = store
        .insert_account(new_account(2, "bob@example.net"))
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(account, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let inbox = store.mailboxes(account).await.expect("mailboxes")[0].id;
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
    (account, inbox)
}

#[tokio::test]
async fn unified_views_merge_accounts_and_leave_out_what_the_inbox_hides() {
    let f = fixture().await;
    let (_, other) = second_account(&f.store).await;
    let mine: Vec<MessageHeader> = ["Mia", "Carl", "Emil", "Hans"]
        .iter()
        .enumerate()
        .map(|(i, n)| {
            from(
                header(
                    u32::try_from(i).expect("i") + 1,
                    D + i64::try_from(i).expect("i") * 120,
                    Some(&format!("m{i}@u")),
                ),
                n,
            )
        })
        .collect();
    let theirs: Vec<MessageHeader> = ["Bert", "Dora", "Fritz"]
        .iter()
        .enumerate()
        .map(|(i, n)| {
            from(
                header(
                    u32::try_from(i).expect("i") + 1,
                    D + 60 + i64::try_from(i).expect("i") * 120,
                    Some(&format!("t{i}@u")),
                ),
                n,
            )
        })
        .collect();
    store_in(&f, f.inbox, mine).await;
    let store = f.store.clone();
    store
        .store_headers(other, UV, theirs, None)
        .await
        .expect("store");
    let unified = InboxMailboxes::Unified(None);
    let by_sender = query(
        unified.clone(),
        order(SortKey::Sender, true),
        ViewFilter::default(),
    );
    let names = |ids: Vec<i64>| {
        let store = f.store.clone();
        async move {
            let mut out = Vec::new();
            for id in ids {
                let name: String =
                    store
                        .read(move |c| {
                            Ok(c.query_row(
                                "SELECT from_name FROM message WHERE id = ?1",
                                [id],
                                |r| r.get(0),
                            )?)
                        })
                        .await
                        .expect("name");
                out.push(name);
            }
            out
        }
    };
    assert_eq!(
        names(all_messages(&f.store, &by_sender, 2).await).await,
        ["Bert", "Carl", "Dora", "Emil", "Fritz", "Hans", "Mia"]
    );
    // A waiting snooze and mail of a blocked sender leave the inbox; a pinned message stays.
    f.store
        .snooze_message(MessageId(D + 120), NOW + 3_600_000, NOW)
        .await
        .expect("snooze Carl");
    f.store
        .block_sender("dora@example.org".into(), NOW)
        .await
        .expect("block Dora");
    f.store
        .hide_blocked_message(MessageId(D + 60 + 120), "dora@example.org".into())
        .await
        .expect("hide Dora");
    f.store
        .pin_message(MessageId(D + 360), NOW)
        .await
        .expect("pin Hans");
    assert_eq!(
        names(all_messages(&f.store, &by_sender, 2).await).await,
        ["Bert", "Emil", "Fritz", "Hans", "Mia"]
    );
    assert_eq!(
        f.store.view_count(by_sender.clone()).await.expect("count"),
        5
    );
    // A folder that is not an inbox has no such rules: the archive keeps a snoozed message.
    let archived = from(header(1, D + 1_000, Some("arch@u")), "Olga");
    store_in(&f, f.archive, vec![archived]).await;
    f.store
        .snooze_message(MessageId(D + 1_000), NOW + 3_600_000, NOW)
        .await
        .expect("snooze");
    let archive = query(
        InboxMailboxes::Mailbox(f.archive),
        order(SortKey::Sender, true),
        ViewFilter::default(),
    );
    assert_eq!(all_messages(&f.store, &archive, 10).await, [D + 1_000]);
    // Conversations of a unified view, by subject descending, page by page.
    let conv = query(
        unified,
        order(SortKey::Subject, false),
        ViewFilter::default(),
    );
    let rows = all_conversations(&f.store, &conv, 1).await;
    assert_eq!(rows.len(), 5);
}

#[tokio::test]
async fn every_location_gets_its_sort_keys() {
    let f = fixture().await;
    let mut h = header(1, D, Some("same@x"));
    h.from = vec![person(Some("Örjan"), "o@example.org")];
    h.to = vec![person(None, "Zed@Example.org")];
    h.subject = "Fwd: Bericht".into();
    h.size = 4_321;
    store_in(&f, f.inbox, vec![h.clone()]).await;
    // The same message in the archive: a merged location.
    let mut copy = h.clone();
    copy.uid = 1;
    store_in(&f, f.archive, vec![copy]).await;
    // A location put back by the outbox (a reverted move).
    f.store
        .write(move |tx| {
            crate::outbox::insert_location_if_free(
                tx,
                MailboxId(f.sent.0),
                MessageId(D),
                7,
                &mail_model::Flags::default(),
            )?;
            Ok(())
        })
        .await
        .expect("put back");
    let rows: Vec<(String, String, String, i64)> = f
        .store
        .read(|c| {
            let mut s = c.prepare(
                "SELECT sort_sender, sort_recipient, sort_subject, size FROM message_location
                 ORDER BY mailbox_id",
            )?;
            let rows = s
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .expect("rows");
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(
            row,
            (
                "orjan".into(),
                "zed@example.org".into(),
                "bericht".into(),
                4_321
            )
        );
    }
}

#[tokio::test]
async fn the_migration_fills_the_keys_of_existing_locations() {
    use crate::migrations::MIGRATIONS;
    use crate::store::Schema;
    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = db_path(&dir);
    let before = Schema::from_vec(MIGRATIONS[..MIGRATIONS.len() - 1].to_vec());
    let store = Store::open_with(&path, StoreOptions::default(), &before).expect("old schema");
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
    let headers: Vec<MessageHeader> = (0..30).map(varied).collect();
    let h2 = headers.clone();
    store
        .write(move |tx| crate::test_support::stage2_store_headers(tx, inbox, &h2))
        .await
        .expect("old headers");
    // The old code stored recipients too.
    let to: Vec<(i64, String)> = headers
        .iter()
        .map(|h| (h.sort_date(), serde_json::to_string(&h.to).expect("json")))
        .collect();
    store
        .write(move |tx| {
            for (id, json) in &to {
                tx.execute(
                    "UPDATE message SET to_addrs = ?2 WHERE id = ?1",
                    rusqlite::params![id, json],
                )?;
            }
            Ok(())
        })
        .await
        .expect("recipients");
    drop(store);
    let store = Store::open(&path, StoreOptions::default()).expect("migrated");
    for key in ORDERS {
        for ascending in [true, false] {
            let o = order(key, ascending);
            let q = query(InboxMailboxes::Mailbox(inbox), o, ViewFilter::default());
            assert_eq!(
                all_messages(&store, &q, 7).await,
                expected(&headers, o),
                "{o:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Plans

fn plan(conn: &Connection, sql: &str, params: &[rusqlite::types::Value]) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare EXPLAIN QUERY PLAN");
    stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        r.get::<_, String>(3)
    })
    .expect("query plan")
    .collect::<Result<Vec<_>, _>>()
    .expect("plan rows")
}

fn filters() -> Vec<(&'static str, ViewFilter)> {
    vec![
        ("none", ViewFilter::default()),
        (
            "unread",
            ViewFilter {
                search: SearchFilter {
                    unread: Some(true),
                    ..SearchFilter::default()
                },
                ..ViewFilter::default()
            },
        ),
        (
            "text, tag, contacts, attachment",
            ViewFilter {
                search: SearchFilter {
                    text: TextMatch::Match("\"angebot\"".into()),
                    has_attachment: Some(true),
                    ..SearchFilter::default()
                },
                contacts: true,
                keyword: Some("projekt".into()),
            },
        ),
    ]
}

/// Every view query walks the index of its order (never `SCAN l`, never a sort of a branch);
/// the newer-message check of conversations uses `message_thread`.
fn check_plans(conn: &Connection, mailboxes: &[(MailboxId, AccountId)], stage: &str) {
    for key in ORDERS {
        for ascending in [true, false] {
            for (name, filter) in filters() {
                for conversations in [false, true] {
                    for inbox_rules in [false, true] {
                        let view = ResolvedView {
                            mailboxes,
                            inbox_rules,
                            order: order(key, ascending),
                            filter: &filter,
                            account_rows: None,
                        };
                        let cursor = ViewCursor {
                            text: matches!(
                                key,
                                SortKey::Sender | SortKey::Recipient | SortKey::Subject
                            )
                            .then(|| "m".to_owned()),
                            number: matches!(key, SortKey::Size).then_some(1_100),
                            id: MessageId(D + 600),
                        };
                        for cursor in [None, Some(&cursor)] {
                            let (sql, params) =
                                statement(&view, conversations, cursor, 100).expect("statement");
                            let lines = plan(conn, &sql, &params);
                            let context = format!(
                                "{stage}: {key:?} asc={ascending} filter={name} conversations={conversations} rules={inbox_rules} cursor={}\n{}",
                                cursor.is_some(),
                                lines.join("\n")
                            );
                            assert!(
                                !lines.iter().any(|l| ["SCAN l", "SCAN m"]
                                    .iter()
                                    .any(|t| l == t || l.starts_with(&format!("{t} ")))),
                                "{context}"
                            );
                            assert!(
                                lines.iter().any(|l| l
                                    .starts_with("SEARCH m USING INTEGER PRIMARY KEY (rowid=?)")),
                                "{context}"
                            );
                            let walks = match key {
                                SortKey::Date => lines.iter().filter(|l| {
                                    l.starts_with("SEARCH l USING PRIMARY KEY (mailbox_id=?")
                                        || l.starts_with("SEARCH l USING INDEX loc_unseen")
                                        || l.starts_with("SEARCH l USING INDEX loc_flagged")
                                        // The text's matches in id order, like the search.
                                        || (matches!(filter.search.text, TextMatch::Match(_))
                                            && l.starts_with("SEARCH l USING INDEX loc_message (message_id=? AND mailbox_id=?)"))
                                }).count(),
                                _ => {
                                    let index = match key {
                                        SortKey::Sender => "loc_sort_sender",
                                        SortKey::Recipient => "loc_sort_recipient",
                                        SortKey::Subject => "loc_sort_subject",
                                        _ => "loc_sort_size",
                                    };
                                    lines
                                        .iter()
                                        .filter(|l| l.starts_with(&format!("SEARCH l USING INDEX {index} (mailbox_id=?")))
                                        .count()
                                }
                            };
                            assert_eq!(walks, mailboxes.len(), "{context}");
                            // No branch sorts its index walk. Several branches merge
                            // (`MERGE (UNION ALL)`), each sorting only its own result of at
                            // most `limit + 1` rows, as the unified inbox does (decision 0003).
                            for (i, line) in lines.iter().enumerate() {
                                if line.contains("USE TEMP B-TREE FOR ORDER BY") {
                                    assert!(
                                        mailboxes.len() > 1
                                            && i > 0
                                            && lines[i - 1].starts_with("SCAN (subquery-"),
                                        "{context}"
                                    );
                                }
                            }
                            if mailboxes.len() > 1 {
                                assert!(
                                    lines.first().is_some_and(|l| l == "MERGE (UNION ALL)"),
                                    "{context}"
                                );
                            }
                            if conversations {
                                assert!(
                                    lines
                                        .iter()
                                        .any(|l| l.contains("INDEX message_thread (thread_id=?")),
                                    "{context}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn every_view_query_uses_the_index_of_its_order() {
    let f = fixture().await;
    let (other_account, other) = second_account(&f.store).await;
    let headers: Vec<MessageHeader> = (0..2_000).map(varied).collect();
    for window in headers.chunks(500) {
        store_in(&f, f.inbox, window.to_vec()).await;
    }
    let mut theirs: Vec<MessageHeader> = (0..600).map(varied).collect();
    for h in &mut theirs {
        h.message_id = h.message_id.as_ref().map(|m| format!("o{m}"));
    }
    f.store
        .store_headers(other, UV, theirs, None)
        .await
        .expect("other");
    let one = vec![(f.inbox, f.account)];
    let two = vec![(f.inbox, f.account), (other, other_account)];
    let check = |stage: &'static str| {
        let (one, two) = (one.clone(), two.clone());
        f.store.read(move |c| {
            check_plans(c, &one, stage);
            check_plans(c, &two, stage);
            Ok(())
        })
    };
    check("fresh statistics").await.expect("plans");
    f.store
        .write(|tx| {
            tx.execute_batch("ANALYZE; PRAGMA optimize;")?;
            Ok(())
        })
        .await
        .expect("analyze");
    let Fixture { _dir, store, .. } = f;
    drop(store);
    let reopened = Store::open(&db_path(&_dir), StoreOptions::default()).expect("reopen");
    reopened
        .read(move |c| {
            check_plans(c, &one, "after ANALYZE");
            check_plans(c, &two, "after ANALYZE");
            Ok(())
        })
        .await
        .expect("plans");
}
