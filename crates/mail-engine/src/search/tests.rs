//! Stage 7 through [`AppCore`], without a server: search with scope, `account:`, `in:`,
//! problems, chips, highlights and body text through the real content parser; the unified
//! inbox with groups and hidden accounts; identities; the background rebuild.

use std::time::Duration;

use mail_model::{
    AccountId, Address, CoreError, CoreEvent, Flags, IdentityInput, MailboxRole, MessageHeader,
    MessageId, RemoteMailbox, SearchIndexState, SearchProblemCode, SearchRequest, SearchScope,
    SearchTermKind, Security, ServerSettings, TextRange, UidValidity, UnifiedView,
};
use mail_store::{MailboxBaseline, NewAccount};
use tempfile::TempDir;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

pub(super) const UV: UidValidity = UidValidity(7);
const D: i64 = 1_790_000_000;

pub(super) struct Fixture {
    _dir: TempDir,
    pub(super) core: AppCore,
    /// (account, inbox, archive)
    pub(super) accounts: Vec<(AccountId, mail_model::MailboxId, mail_model::MailboxId)>,
}

pub(super) fn header(
    uid: u32,
    date: i64,
    subject: &str,
    from: (&str, &str),
    to: &str,
) -> MessageHeader {
    MessageHeader {
        uid,
        modseq: None,
        flags: Flags::default(),
        size: 1_000,
        internal_date: date,
        date: Some(date),
        subject: subject.into(),
        from: vec![Address {
            name: Some(from.0.into()),
            email: from.1.into(),
        }],
        to: vec![Address {
            name: None,
            email: to.into(),
        }],
        cc: Vec::new(),
        message_id: Some(format!("{uid}-{date}@t")),
        in_reply_to: None,
        references: Vec::new(),
        list_id: None,
        list_unsubscribe: None,
        auto_submitted: None,
        precedence: None,
        has_attachments: false,
        category_headers: Vec::new(),
        gmail: None,
    }
}

pub(super) async fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::new(Credentials::new(MemorySecretStore::new())),
    )
    .await;
    let store = core.store_for_tests().clone();
    let mut accounts = Vec::new();
    for (n, email, name) in [
        (1, "alice@example.org", "Alice"),
        (2, "bob@firma.example", "Bob Arbeit"),
    ] {
        let server = |port| ServerSettings {
            host: "imap.invalid".into(),
            port,
            security: Security::Tls,
            username: email.into(),
        };
        let id: AccountId = format!("00000000-0000-4000-8000-{n:012}")
            .parse()
            .expect("id");
        store
            .insert_account(NewAccount {
                id,
                email: email.into(),
                display_name: name.into(),
                imap: server(993),
                smtp: server(465),
                credential_ref: format!("test:{id}"),
            })
            .await
            .expect("account");
        // Stage 22 (decision 0078): `account:` finds the account by its name in the lists.
        store
            .rename_account(id, name.into())
            .await
            .expect("list name");
        let remote = |name: &str, attrs: &[&str]| RemoteMailbox {
            name: name.into(),
            path: name.into(),
            delimiter: Some('/'),
            attributes: attrs.iter().map(|a| (*a).to_owned()).collect(),
            status: None,
        };
        store
            .sync_mailboxes(
                id,
                vec![
                    remote("INBOX", &[]),
                    remote("Archiv", &["\\Archive"]),
                    remote("Projekte", &[]),
                ],
            )
            .await
            .expect("mailboxes");
        let boxes = store.mailboxes(id).await.expect("mailboxes");
        let by = |role| boxes.iter().find(|m| m.role == role).expect("role").id;
        let (inbox, archive) = (by(MailboxRole::Inbox), by(MailboxRole::Archive));
        for mailbox in boxes.iter().map(|m| m.id) {
            store
                .reset_mailbox(
                    mailbox,
                    MailboxBaseline {
                        uid_validity: UV,
                        uid_next: 1,
                        highest_modseq: None,
                    },
                )
                .await
                .expect("reset");
        }
        accounts.push((id, inbox, archive));
    }
    Fixture {
        _dir: dir,
        core,
        accounts,
    }
}

pub(super) fn request(query: &str, scope: SearchScope) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        scope,
        cursor: None,
        limit: 50,
        typing: false,
        month_first: false,
    }
}

pub(super) fn subjects(page: &mail_model::SearchPage) -> Vec<&str> {
    page.hits
        .iter()
        .map(|h| h.message.subject.as_str())
        .collect()
}

#[tokio::test]
async fn search_resolves_scope_accounts_and_mailboxes() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let (alice, a_inbox, a_archive) = f.accounts[0];
    let (bob, b_inbox, _) = f.accounts[1];
    store
        .store_headers(
            a_inbox,
            UV,
            vec![
                header(
                    1,
                    D,
                    "Rechnung März",
                    ("Müller GmbH", "rechnung@mueller.example"),
                    "alice@example.org",
                ),
                header(
                    2,
                    D + 60,
                    "Termin morgen",
                    ("Anna", "anna@example.org"),
                    "alice@example.org",
                ),
            ],
            None,
        )
        .await
        .expect("alice inbox");
    store
        .store_headers(
            a_archive,
            UV,
            vec![header(
                1,
                D + 120,
                "Rechnung April",
                ("Müller GmbH", "rechnung@mueller.example"),
                "alice@example.org",
            )],
            None,
        )
        .await
        .expect("alice archive");
    store
        .store_headers(
            b_inbox,
            UV,
            vec![header(
                1,
                D + 180,
                "Rechnung Mai",
                ("Lieferant", "x@lieferant.example"),
                "bob@firma.example",
            )],
            None,
        )
        .await
        .expect("bob inbox");

    // Everywhere, newest first, with highlights (UTF-16 ranges) and chips.
    let page = f
        .core
        .search(request("rechnung müller", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Rechnung April", "Rechnung März"]);
    assert_eq!(
        page.hits[0].highlights.subject,
        [TextRange { start: 0, end: 8 }]
    );
    assert_eq!(
        page.hits[0].highlights.from,
        [TextRange { start: 0, end: 6 }]
    );
    assert!(!page.hits[0].body_local);
    assert_eq!(page.index.state, SearchIndexState::Ready);
    assert_eq!(
        page.terms.iter().map(|t| t.kind).collect::<Vec<_>>(),
        [SearchTermKind::Text, SearchTermKind::Text]
    );
    assert!(page.problems.is_empty());

    // account: by address, name or domain; unknown accounts are a problem at their place.
    let page = f
        .core
        .search(request("rechnung account:firma.example", SearchScope::All))
        .await
        .expect("bob");
    assert_eq!(subjects(&page), ["Rechnung Mai"]);
    assert_eq!(page.hits[0].message.account_id, bob);
    let page = f
        .core
        .search(request("rechnung -konto:\"Bob Arbeit\"", SearchScope::All))
        .await
        .expect("not bob");
    assert_eq!(page.hits.len(), 2);
    let page = f
        .core
        .search(request("äh account:nobody", SearchScope::All))
        .await
        .expect("unknown");
    assert!(page.matches_nothing && page.hits.is_empty());
    assert_eq!(page.problems.len(), 1);
    assert_eq!(page.problems[0].code, SearchProblemCode::UnknownAccount);
    // "äh " is three UTF-16 units: the term starts at 3.
    assert_eq!(page.problems[0].range, TextRange { start: 3, end: 17 });

    // in: by role (any language) and by name; scope of one mailbox.
    let page = f
        .core
        .search(request("rechnung in:archiv", SearchScope::All))
        .await
        .expect("archive");
    assert_eq!(subjects(&page), ["Rechnung April"]);
    assert_eq!(page.hits[0].message.mailbox_id, a_archive);
    let page = f
        .core
        .search(request("in:projekte", SearchScope::All))
        .await
        .expect("empty folder");
    assert!(page.hits.is_empty() && !page.matches_nothing);
    let page = f
        .core
        .search(request("in:gibtsnicht rechnung", SearchScope::All))
        .await
        .expect("unknown folder");
    assert!(page.matches_nothing);
    assert_eq!(page.problems[0].code, SearchProblemCode::UnknownMailbox);
    let page = f
        .core
        .search(request(
            "rechnung",
            SearchScope::Mailbox {
                mailbox_id: a_inbox,
            },
        ))
        .await
        .expect("inbox only");
    assert_eq!(subjects(&page), ["Rechnung März"]);

    // Hidden accounts are left out of "all", not out of their own scope.
    f.core.set_account_hidden(bob, true).await.expect("hide");
    let page = f
        .core
        .search(request("rechnung", SearchScope::All))
        .await
        .expect("all");
    assert!(page.hits.iter().all(|h| h.message.account_id == alice));
    let page = f
        .core
        .search(request(
            "rechnung",
            SearchScope::Account { account_id: bob },
        ))
        .await
        .expect("bob scope");
    assert_eq!(subjects(&page), ["Rechnung Mai"]);

    // Typing: the last word is a prefix; contradictions match nothing; empty finds nothing.
    let mut typing = request("rech", SearchScope::Account { account_id: alice });
    typing.typing = true;
    let page = f.core.search(typing).await.expect("typing");
    assert_eq!(page.hits.len(), 2);
    assert_eq!(
        page.hits[0].highlights.subject,
        [TextRange { start: 0, end: 8 }]
    );
    let page = f
        .core
        .search(request("is:unread is:read", SearchScope::All))
        .await
        .expect("conflict");
    assert!(page.matches_nothing);
    assert_eq!(page.problems[0].code, SearchProblemCode::Conflict);
    let page = f
        .core
        .search(request("  ", SearchScope::All))
        .await
        .expect("empty");
    assert!(page.hits.is_empty() && !page.matches_nothing);
    // Dates in the user's zone: after the day of D finds everything on and after it.
    let page = f
        .core
        .search(request(
            "rechnung after:1.1.2026 before:1.1.2030",
            SearchScope::All,
        ))
        .await
        .expect("dates");
    assert_eq!(page.hits.len(), 2);
    assert!(matches!(
        f.core
            .search(SearchRequest {
                limit: 0,
                ..request("x", SearchScope::All)
            })
            .await,
        Err(CoreError::InvalidArgument(_))
    ));
}

#[tokio::test]
async fn body_text_and_attachment_names_are_indexed_through_the_parser() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let (_, inbox, _) = f.accounts[0];
    store
        .store_headers(
            inbox,
            UV,
            vec![header(
                1,
                D,
                "Unterlagen",
                ("Bank", "bank@example.org"),
                "alice@example.org",
            )],
            None,
        )
        .await
        .expect("header");
    let id = MessageId(D);
    let raw = b"From: bank@example.org\r\nSubject: Unterlagen\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p>Ihr <b>Kontoauszug</b> f&uuml;r Oktober</p>\r\n--b\r\nContent-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"Auszug_2026-10.pdf\"\r\n\r\nJVBERi0=\r\n--b--\r\n";
    store.store_body(id, raw.to_vec()).await.expect("body");
    for query in [
        "kontoauszug",
        "für oktober",
        "filename:auszug_2026",
        "datei:pdf",
    ] {
        let page = f
            .core
            .search(request(query, SearchScope::All))
            .await
            .expect("search");
        assert_eq!(page.hits.len(), 1, "{query}");
        assert!(page.hits[0].body_local);
    }
    // HTML markup is not text.
    let page = f
        .core
        .search(request("uuml", SearchScope::All))
        .await
        .expect("entity");
    assert!(page.hits.is_empty());
}

#[tokio::test]
async fn unified_inbox_groups_and_hidden_accounts_through_the_core() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    for (k, (_, inbox, _)) in f.accounts.iter().enumerate() {
        let k = i64::try_from(k).expect("small");
        let headers: Vec<_> = (1..=5)
            .map(|uid| {
                header(
                    uid,
                    D + i64::from(uid) * 100 + k,
                    "Hallo",
                    ("X", "x@example.org"),
                    "me@example.org",
                )
            })
            .collect();
        store
            .store_headers(*inbox, UV, headers, None)
            .await
            .expect("headers");
    }
    let mut events = f.core.subscribe();
    let page = f
        .core
        .list_unified_inbox(UnifiedView::All, None, 100)
        .await
        .expect("unified");
    assert_eq!(page.items.len(), 10);
    for item in &page.items {
        let (account, inbox, _) = f
            .accounts
            .iter()
            .find(|(a, _, _)| *a == item.account_id)
            .expect("known account");
        assert_eq!(item.mailbox_id, *inbox, "{account}");
    }
    let (bob, _, _) = f.accounts[1];
    let group = f
        .core
        .create_account_group("Arbeit".into(), vec![bob])
        .await
        .expect("group");
    assert!(matches!(
        events.recv().await,
        Ok(CoreEvent::AccountsChanged)
    ));
    let page = f
        .core
        .list_unified_inbox(
            UnifiedView::Group {
                group_id: group.id.clone(),
            },
            None,
            100,
        )
        .await
        .expect("group view");
    assert!(page.items.iter().all(|i| i.account_id == bob));
    assert_eq!(page.items.len(), 5);
    f.core.set_account_hidden(bob, true).await.expect("hide");
    assert!(matches!(
        events.recv().await,
        Ok(CoreEvent::AccountsChanged)
    ));
    assert_eq!(f.core.list_hidden_accounts().await.expect("hidden"), [bob]);
    let page = f
        .core
        .list_unified_threads(UnifiedView::All, None, 100)
        .await
        .expect("threads");
    assert_eq!(page.items.len(), 5);
    assert!(page.items.iter().all(|t| t.message.account_id != bob));
    assert!(matches!(
        f.core
            .delete_account_group(mail_model::AccountGroupId("nope".into()))
            .await,
        Err(CoreError::NotFound(_))
    ));
    f.core.delete_account_group(group.id).await.expect("delete");
}

/// Stage 23 (decision 0087): "Sent" of "All accounts" through the core: the Sent folders of the
/// visible accounts (or a group's), each row with its own account; the page size is checked.
#[tokio::test]
async fn unified_sent_through_the_core() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let mut sent = Vec::new();
    for (k, (account, _, _)) in f.accounts.iter().enumerate() {
        let remote = |name: &str, attrs: &[&str]| RemoteMailbox {
            name: name.into(),
            path: name.into(),
            delimiter: Some('/'),
            attributes: attrs.iter().map(|a| (*a).to_owned()).collect(),
            status: None,
        };
        store
            .sync_mailboxes(
                *account,
                vec![
                    remote("INBOX", &[]),
                    remote("Archiv", &["\\Archive"]),
                    remote("Projekte", &[]),
                    remote("Sent", &["\\Sent"]),
                ],
            )
            .await
            .expect("mailboxes");
        let mailbox = store
            .mailboxes(*account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == MailboxRole::Sent)
            .expect("sent")
            .id;
        store
            .reset_mailbox(
                mailbox,
                MailboxBaseline {
                    uid_validity: UV,
                    uid_next: 1,
                    highest_modseq: None,
                },
            )
            .await
            .expect("reset");
        let k = i64::try_from(k).expect("small");
        let headers: Vec<_> = (1..=4)
            .map(|uid| {
                header(
                    uid,
                    D + i64::from(uid) * 100 + k,
                    "Angebot",
                    ("Ich", "me@example.org"),
                    "x@example.org",
                )
            })
            .collect();
        store
            .store_headers(mailbox, UV, headers, None)
            .await
            .expect("headers");
        sent.push((*account, mailbox));
    }
    let page = f
        .core
        .list_unified_sent(UnifiedView::All, None, 100)
        .await
        .expect("unified sent");
    assert_eq!(page.items.len(), 8);
    assert!(page.items.windows(2).all(|w| w[0].date >= w[1].date));
    for item in &page.items {
        assert!(
            sent.contains(&(item.account_id, item.mailbox_id)),
            "{item:?}"
        );
    }
    let (bob, bob_sent) = sent[1];
    let group = f
        .core
        .create_account_group("Arbeit".into(), vec![bob])
        .await
        .expect("group");
    let page = f
        .core
        .list_unified_sent_threads(
            UnifiedView::Group {
                group_id: group.id.clone(),
            },
            None,
            100,
        )
        .await
        .expect("group threads");
    assert_eq!(page.items.len(), 4);
    assert!(page.items.iter().all(|t| t.message.mailbox_id == bob_sent));
    assert!(matches!(
        f.core.list_unified_sent(UnifiedView::All, None, 0).await,
        Err(CoreError::InvalidArgument(_))
    ));
}

#[tokio::test]
async fn identities_and_the_reply_identity() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let (alice, inbox, _) = f.accounts[0];
    let work = f
        .core
        .add_identity(
            alice,
            IdentityInput {
                email: "alice@work.example".into(),
                name: "Alice (Arbeit)".into(),
                reply_to: None,
            },
        )
        .await
        .expect("add");
    store
        .store_headers(
            inbox,
            UV,
            vec![
                header(
                    1,
                    D,
                    "An die Arbeit",
                    ("Chef", "chef@work.example"),
                    "alice+projekt@work.example",
                ),
                header(
                    2,
                    D + 1,
                    "Privat",
                    ("Mama", "mama@example.org"),
                    "alice@example.org",
                ),
            ],
            None,
        )
        .await
        .expect("headers");
    let to_work = f.core.reply_identity(MessageId(D)).await.expect("reply");
    assert_eq!(to_work.id, work.id, "plus tag stripped");
    let private = f
        .core
        .reply_identity(MessageId(D + 1))
        .await
        .expect("reply");
    assert!(private.is_default);
    let all = f.core.set_default_identity(work.id).await.expect("default");
    assert_eq!(all[0].id, work.id);
    assert_eq!(f.core.list_identities(alice).await.expect("list").len(), 2);
    assert!(matches!(
        f.core.delete_identity(work.id).await,
        Err(CoreError::InvalidArgument(_))
    ));
}

#[tokio::test]
async fn a_rebuild_runs_in_the_background_and_search_keeps_working() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let (_, inbox, _) = f.accounts[0];
    let headers: Vec<_> = (1..=1_200)
        .map(|uid| {
            header(
                uid,
                D + i64::from(uid),
                "Bestellung",
                ("Shop", "shop@example.org"),
                "alice@example.org",
            )
        })
        .collect();
    for window in headers.chunks(500) {
        store
            .store_headers(inbox, UV, window.to_vec(), None)
            .await
            .expect("headers");
    }
    let status = f.core.rebuild_search_index().await.expect("rebuild");
    assert_ne!(status.state, SearchIndexState::Ready);
    assert!(matches!(
        f.core.rebuild_search_index().await,
        Err(CoreError::InvalidArgument(_)) | Ok(_)
    ));
    // Search works during the rebuild.
    let page = f
        .core
        .search(request("bestellung", SearchScope::All))
        .await
        .expect("during");
    assert_eq!(page.hits.len(), 50);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = f.core.get_search_index_status().await.expect("status");
        if status.state == SearchIndexState::Ready {
            assert_eq!(status.percent, 100);
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "rebuild did not finish"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let page = f
        .core
        .search(request("bestellung", SearchScope::All))
        .await
        .expect("after");
    assert_eq!(page.hits.len(), 50);
    assert!(page.next_cursor.is_some());
}

/// An index job left by the previous run is found at start and done once the start is over
/// (decision 0029: the work found at start does not hold the writer during the start).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_index_job_found_at_start_runs_after_the_start() {
    let f = fixture().await;
    let store = f.core.store_for_tests().clone();
    let inbox = f.accounts[0].1;
    let headers: Vec<MessageHeader> = (1..=300)
        .map(|uid| {
            header(
                uid,
                D + i64::from(uid),
                "Bestellung",
                ("Shop", "shop@example.org"),
                "alice@example.org",
            )
        })
        .collect();
    store
        .store_headers(inbox, UV, headers, None)
        .await
        .expect("headers");
    // Through the store, so the running core is not told: the job waits for the next start.
    assert!(store.rebuild_search_index().await.expect("rebuild"));
    drop(store);
    let Fixture {
        _dir: dir, core, ..
    } = f;
    core.shutdown().await;
    drop(core);

    let server = crate::sync::fake::FakeServer::new();
    server.with(|s| s.faults.connect = Some(mail_model::BackendError::ConnectionRefused));
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::with_connector(
            Credentials::new(MemorySecretStore::new()),
            crate::sync::fake::FakeConnector(server),
        ),
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = core.get_search_index_status().await.expect("status");
        if status.state == SearchIndexState::Ready {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the index job found at start did not run"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let page = core
        .search(request("bestellung", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(page.hits.len(), 50);
    core.shutdown().await;
}
