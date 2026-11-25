//! Stage 19, task A through [`AppCore`] with the real store: the quick filter compiled like a
//! search (text, operators, switches, contradictions), sorted pages of a folder and of the
//! unified inbox, counts, tags, and the stored orders and table settings.

use mail_model::{
    AccountId, Address, CoreError, Flags, InboxScope, ListOrder, ListPresentation,
    ListTableSettings, ListViewRequest, MailboxId, MailboxRole, MessageHeader, QuickFilter,
    RemoteMailbox, Security, ServerSettings, SortKey, TableColumnId, UidValidity, UnifiedView,
    ViewCursor,
};
use mail_store::{GLOBAL_SCOPE, MailboxBaseline, NewAccount};
use tempfile::TempDir;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

const UV: UidValidity = UidValidity(7);
const D: i64 = 1_790_000_000;

struct Fixture {
    _dir: TempDir,
    core: AppCore,
    /// (account, inbox, archive) of two accounts.
    accounts: Vec<(AccountId, MailboxId, MailboxId)>,
}

fn header(uid: u32, date: i64, subject: &str, from: (&str, &str)) -> MessageHeader {
    MessageHeader {
        uid,
        modseq: None,
        flags: Flags::default(),
        size: 1_000 + uid,
        internal_date: date,
        date: Some(date),
        subject: subject.into(),
        from: vec![Address {
            name: Some(from.0.into()),
            email: from.1.into(),
        }],
        to: vec![Address {
            name: None,
            email: "me@example.org".into(),
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

async fn fixture() -> Fixture {
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
    for (n, email) in [(1, "alice@example.org"), (2, "bob@firma.example")] {
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
                display_name: email.into(),
                imap: server(993),
                smtp: server(465),
                credential_ref: format!("test:{id}"),
            })
            .await
            .expect("account");
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
                vec![remote("INBOX", &[]), remote("Archiv", &["\\Archive"])],
            )
            .await
            .expect("mailboxes");
        let boxes = store.mailboxes(id).await.expect("mailboxes");
        let by = |role| boxes.iter().find(|m| m.role == role).expect("role").id;
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
        accounts.push((id, by(MailboxRole::Inbox), by(MailboxRole::Archive)));
    }
    Fixture {
        _dir: dir,
        core,
        accounts,
    }
}

/// Alice's inbox: five messages, the last two read, one with an attachment, one flagged and
/// tagged; Bob's inbox: two.
async fn filled() -> Fixture {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let (_, a_inbox, a_archive) = f.accounts[0];
    let (_, b_inbox, _) = f.accounts[1];
    let mut alice = vec![
        header(
            1,
            D,
            "Rechnung März",
            ("Müller GmbH", "rechnung@mueller.example"),
        ),
        header(2, D + 60, "Termin morgen", ("Anna", "anna@example.org")),
        header(3, D + 120, "Re: Angebot", ("Zoe", "zoe@example.org")),
        header(4, D + 180, "Rechnung April", ("Bernd", "bernd@example.org")),
        header(5, D + 240, "Urlaub", ("Carla", "carla@example.org")),
    ];
    alice[2].has_attachments = true;
    alice[3].flags.flagged = true;
    alice[3].flags.keywords = vec!["Steuer".into()];
    alice[3].flags.seen = true;
    alice[4].flags.seen = true;
    store
        .store_headers(a_inbox, UV, alice, None)
        .await
        .expect("alice");
    store
        .store_headers(
            a_archive,
            UV,
            vec![header(
                1,
                D + 300,
                "Rechnung Mai",
                ("Bernd", "bernd@example.org"),
            )],
            None,
        )
        .await
        .expect("archive");
    store
        .store_headers(
            b_inbox,
            UV,
            vec![
                header(1, D + 30, "Projekt", ("Dora", "dora@firma.example")),
                header(2, D + 90, "Rechnung Juni", ("Emil", "emil@firma.example")),
            ],
            None,
        )
        .await
        .expect("bob");
    f
}

fn request(
    scope: InboxScope,
    key: SortKey,
    ascending: bool,
    filter: QuickFilter,
) -> ListViewRequest {
    ListViewRequest {
        scope,
        order: ListOrder { key, ascending },
        filter,
    }
}

async fn subjects(core: &AppCore, request: ListViewRequest) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor: Option<ViewCursor> = None;
    loop {
        let page = core
            .list_view_messages(request.clone(), cursor, 2)
            .await
            .expect("page");
        out.extend(page.items.into_iter().map(|i| i.subject));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return out,
        }
    }
}

#[tokio::test]
async fn a_folder_sorts_and_filters_like_a_search_typed_in_it() {
    let f = filled().await;
    let (_, inbox, _) = f.accounts[0];
    let scope = InboxScope::Mailbox { mailbox_id: inbox };
    let core = &f.core;
    let by = |key, ascending, filter| request(scope.clone(), key, ascending, filter);
    assert_eq!(
        subjects(core, by(SortKey::Sender, true, QuickFilter::default())).await,
        [
            "Termin morgen",
            "Rechnung April",
            "Urlaub",
            "Rechnung März",
            "Re: Angebot"
        ]
    );
    assert_eq!(
        subjects(core, by(SortKey::Subject, true, QuickFilter::default())).await,
        [
            "Re: Angebot",
            "Rechnung April",
            "Rechnung März",
            "Termin morgen",
            "Urlaub"
        ]
    );
    // Text as in the search: words, the last one as a prefix, operators.
    let text = |t: &str| QuickFilter {
        text: t.into(),
        ..QuickFilter::default()
    };
    assert_eq!(
        subjects(core, by(SortKey::Date, false, text("rechn"))).await,
        ["Rechnung April", "Rechnung März"],
        "the folder only, not the archive"
    );
    assert_eq!(
        subjects(core, by(SortKey::Date, false, text("from:anna"))).await,
        ["Termin morgen"]
    );
    // Switches combine with each other and with the text.
    let unread_rechnung = QuickFilter {
        unread: true,
        text: "rechnung".into(),
        ..QuickFilter::default()
    };
    assert_eq!(
        subjects(core, by(SortKey::Date, false, unread_rechnung.clone())).await,
        ["Rechnung März"]
    );
    assert_eq!(
        core.count_list_view(by(SortKey::Date, false, unread_rechnung))
            .await
            .expect("count"),
        1
    );
    let flagged_tag = QuickFilter {
        flagged: true,
        tag: Some("steuer".into()),
        ..QuickFilter::default()
    };
    assert_eq!(
        subjects(core, by(SortKey::Date, false, flagged_tag)).await,
        ["Rechnung April"]
    );
    let attachment = QuickFilter {
        attachment: true,
        ..QuickFilter::default()
    };
    assert_eq!(
        subjects(core, by(SortKey::Date, false, attachment)).await,
        ["Re: Angebot"]
    );
    // "Unread" and `is:read` contradict each other: nothing matches.
    let contradiction = QuickFilter {
        unread: true,
        text: "is:read".into(),
        ..QuickFilter::default()
    };
    assert!(
        subjects(core, by(SortKey::Date, false, contradiction.clone()))
            .await
            .is_empty()
    );
    assert_eq!(
        core.count_list_view(by(SortKey::Date, false, contradiction))
            .await
            .expect("count"),
        0
    );
    // An unknown folder in the text matches nothing; `in:` another folder narrows to both.
    assert!(
        subjects(core, by(SortKey::Date, false, text("in:nirgendwo")))
            .await
            .is_empty()
    );
    assert!(
        subjects(core, by(SortKey::Date, false, text("in:archiv")))
            .await
            .is_empty(),
        "no message of this inbox lies in the archive too"
    );
    // Conversations of the same view (each message is its own here).
    let page = core
        .list_view_conversations(by(SortKey::Size, true, QuickFilter::default()), None, 10)
        .await
        .expect("conversations");
    let subjects: Vec<&str> = page
        .items
        .iter()
        .map(|t| t.message.subject.as_str())
        .collect();
    assert_eq!(
        subjects,
        [
            "Rechnung März",
            "Termin morgen",
            "Re: Angebot",
            "Rechnung April",
            "Urlaub"
        ],
        "smallest first (the test sizes grow with the UID)"
    );
    assert!(page.next_cursor.is_none());
    assert_eq!(
        core.list_view_tags(scope.clone()).await.expect("tags"),
        ["Steuer"]
    );
}

#[tokio::test]
async fn the_unified_inbox_sorts_over_every_account() {
    let f = filled().await;
    let scope = InboxScope::Unified {
        view: UnifiedView::All,
    };
    let filter = QuickFilter {
        text: "rechnung".into(),
        ..QuickFilter::default()
    };
    assert_eq!(
        subjects(
            &f.core,
            request(scope.clone(), SortKey::Sender, false, filter.clone())
        )
        .await,
        ["Rechnung März", "Rechnung Juni", "Rechnung April"]
    );
    assert_eq!(
        f.core
            .count_list_view(request(scope.clone(), SortKey::Date, false, filter))
            .await
            .expect("count"),
        3
    );
    // `account:` in the text narrows the unified view.
    let bob = QuickFilter {
        text: "account:bob@firma.example".into(),
        ..QuickFilter::default()
    };
    assert_eq!(
        subjects(&f.core, request(scope, SortKey::Subject, true, bob)).await,
        ["Projekt", "Rechnung Juni"]
    );
}

#[tokio::test]
async fn invalid_filters_are_refused() {
    let f = filled().await;
    let (_, inbox, _) = f.accounts[0];
    let scope = InboxScope::Mailbox { mailbox_id: inbox };
    let bad_tag = QuickFilter {
        tag: Some("zwei wörter".into()),
        ..QuickFilter::default()
    };
    let err = f
        .core
        .list_view_messages(
            request(scope.clone(), SortKey::Date, false, bad_tag),
            None,
            10,
        )
        .await
        .expect_err("tag with a space");
    assert!(matches!(err, CoreError::InvalidArgument(_)), "{err:?}");
    let err = f
        .core
        .list_view_messages(
            request(
                InboxScope::Mailbox {
                    mailbox_id: MailboxId(999_999),
                },
                SortKey::Date,
                false,
                QuickFilter::default(),
            ),
            None,
            10,
        )
        .await
        .expect_err("unknown mailbox");
    assert!(matches!(err, CoreError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn orders_are_stored_per_view_and_the_default_is_not() {
    let f = fixture().await;
    let core = &f.core;
    assert!(core.get_list_orders().await.expect("orders").is_empty());
    let by_sender = ListOrder {
        key: SortKey::Sender,
        ascending: true,
    };
    let orders = core
        .set_list_order("mailbox:4".into(), Some(by_sender))
        .await
        .expect("set");
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].view, "mailbox:4");
    assert_eq!(orders[0].order, by_sender);
    // Another view, then the first one changed: one entry per view.
    core.set_list_order(
        " unified:all ".into(),
        Some(ListOrder {
            key: SortKey::Size,
            ascending: false,
        }),
    )
    .await
    .expect("set");
    let orders = core
        .set_list_order(
            "mailbox:4".into(),
            Some(ListOrder {
                key: SortKey::Subject,
                ascending: false,
            }),
        )
        .await
        .expect("set");
    assert_eq!(orders.len(), 2);
    assert!(orders.iter().any(|o| o.view == "unified:all"));
    // The default order and `None` remove the entry.
    let orders = core
        .set_list_order("mailbox:4".into(), Some(ListOrder::default()))
        .await
        .expect("default");
    assert_eq!(orders.len(), 1);
    let orders = core
        .set_list_order("unified:all".into(), None)
        .await
        .expect("none");
    assert!(orders.is_empty());
    assert!(core.get_list_orders().await.expect("orders").is_empty());
    for bad in ["", " ", "a\nb"] {
        assert!(
            core.set_list_order(bad.into(), Some(by_sender))
                .await
                .is_err(),
            "{bad:?}"
        );
    }
}

#[tokio::test]
async fn table_settings_are_stored_checked_and_normalized() {
    let f = fixture().await;
    let core = &f.core;
    assert_eq!(
        core.get_list_table().await.expect("defaults"),
        ListTableSettings::default()
    );
    let mut settings = ListTableSettings {
        presentation: ListPresentation::Table,
        ..ListTableSettings::default()
    };
    if let Some(size) = settings
        .columns
        .iter_mut()
        .find(|c| c.id == TableColumnId::Size)
    {
        size.visible = false;
        size.width = 120;
    }
    let stored = core.set_list_table(settings.clone()).await.expect("set");
    assert_eq!(stored, settings);
    assert_eq!(core.get_list_table().await.expect("get"), settings);
    let mut wrong = settings.clone();
    wrong.columns[2].width = 5;
    assert!(core.set_list_table(wrong).await.is_err());
    // A stored value from elsewhere (an older version, an edit) is normalized on reading.
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![(
                super::LIST_TABLE_KEY.to_owned(),
                r#"{"presentation":"table","columns":[{"id":"date","visible":false,"width":9999}]}"#
                    .to_owned(),
            )],
        )
        .await
        .expect("put");
    let read = core.get_list_table().await.expect("get");
    assert_eq!(read.presentation, ListPresentation::Table);
    assert_eq!(read.columns.len(), TableColumnId::ALL.len());
    let date = read
        .columns
        .iter()
        .find(|c| c.id == TableColumnId::Date)
        .expect("date");
    assert!(!date.visible);
    assert_eq!(date.width, TableColumnId::Date.default_width());
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![(
                super::LIST_TABLE_KEY.to_owned(),
                r#"{"presentation": 3}"#.to_owned(),
            )],
        )
        .await
        .expect("put");
    assert_eq!(
        core.get_list_table().await.expect("get"),
        ListTableSettings::default()
    );
}
