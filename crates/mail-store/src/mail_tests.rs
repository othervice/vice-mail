//! Tests of the stage 2 schema and API: migration, accounts, mailboxes, storing headers, merge
//! rule, id allocation, flags, removals, UIDVALIDITY, keyset lists, counters and cascades.

use std::collections::HashSet;

use mail_model::{
    AccountColor, AccountStatus, Address, CoreError, ErrorCode, FlagUpdate, Flags, ListCursor,
    MailboxId, MailboxRole, MessageId, ModSeq, RoleSource, UidSet, UidValidity,
};
use tempfile::TempDir;

use crate::migrations::MIGRATIONS;
use crate::store::Schema;
use crate::test_support::{
    UV, account_id, all_ids, count, db_path, fixture, header, inspect, new_account, remote, seen,
};
use crate::{
    GLOBAL_SCOPE, MAX_PAGE_SIZE, MailboxBaseline, SCHEMA_VERSION, Store, StoreError, StoreOptions,
    StoreReport, backup,
};

const D: i64 = 1_764_000_000; // 24.11.2025

const MAIL_TABLES: [&str; 5] = [
    "account",
    "identity",
    "mailbox",
    "message",
    "message_location",
];

fn baseline(uid_validity: u32, uid_next: u32) -> MailboxBaseline {
    MailboxBaseline {
        uid_validity: UidValidity(uid_validity),
        uid_next,
        highest_modseq: None,
    }
}

fn flags(seen: bool, keywords: &[&str]) -> Flags {
    let mut f = Flags {
        seen,
        keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
        ..Flags::default()
    };
    f.normalize_keywords();
    f
}

// ---------------------------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn migrates_a_stage_1_database_and_backs_it_up() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);
    let stage1 = Schema::from_vec(MIGRATIONS[..1].to_vec());
    let store = Store::open_with(&path, StoreOptions::default(), &stage1).expect("stage 1");
    store
        .put_settings(GLOBAL_SCOPE, vec![("ui.theme".into(), "\"dark\"".into())])
        .await
        .expect("write setting");
    drop(store);
    assert_eq!(inspect(&path).0, 1);

    let store = Store::open(&path, StoreOptions::default()).expect("migrate to stage 2");
    let report = store.open_report().clone();
    assert_eq!(SCHEMA_VERSION, 2);
    assert_eq!((report.previous_version, report.version), (1, 2));

    // The backup is the stage 1 database, with its data and without the new tables.
    let copy = report.backup.expect("backup before migration");
    assert!(copy.to_string_lossy().ends_with("-v1.sqlite3"));
    assert_eq!(backup::list(&path), std::slice::from_ref(&copy));
    let (version, names) = inspect(&copy);
    assert_eq!(version, 1);
    assert!(names.contains(&"setting".to_owned()));
    for table in MAIL_TABLES {
        assert!(!names.contains(&table.to_owned()), "{table} in backup");
    }

    // The migrated database kept the settings and works with the new API.
    let settings = store.settings(GLOBAL_SCOPE).await.expect("settings");
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].value, "\"dark\"");
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("insert account");
    assert_eq!(store.accounts().await.expect("accounts"), [account]);
    drop(store);

    let (version, names) = inspect(&path);
    assert_eq!(version, 2);
    for table in MAIL_TABLES {
        assert!(names.contains(&table.to_owned()), "{table} missing");
    }
}

#[tokio::test]
async fn creates_the_mail_schema_in_an_empty_database() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);
    let store = Store::open(&path, StoreOptions::default()).expect("open");
    assert_eq!(store.open_report().previous_version, 0);
    assert_eq!(store.open_report().backup, None);
    drop(store);

    let (version, names) = inspect(&path);
    assert_eq!(version, SCHEMA_VERSION);
    let expected = [
        "account",
        "identity",
        "identity_account",
        "identity_default",
        "loc_message",
        "loc_uid",
        "loc_unseen",
        "mailbox",
        "mailbox_parent",
        "mailbox_path",
        "mailbox_role",
        "message",
        "message_location",
        "message_msgid",
        "message_thread",
        "setting",
    ];
    for name in expected {
        assert!(
            names.contains(&name.to_owned()),
            "{name} missing: {names:?}"
        );
    }
    assert!(
        !names.iter().any(|n| n.contains("fts")),
        "no FTS before stage 7"
    );
}

// ---------------------------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn accounts_get_a_color_an_identity_and_a_unique_email() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");

    let alice = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("alice");
    assert_eq!(alice.color, AccountColor::new(0).expect("color"));
    assert!(!alice.writes_enabled, "new accounts are read-only");
    assert!(alice.created_at > 0);

    let err = store
        .insert_account(new_account(2, "ALICE@Example.org"))
        .await
        .expect_err("same address, other case");
    assert!(matches!(err, StoreError::AccountExists(_)), "{err:?}");
    assert_eq!(CoreError::from(err).code(), ErrorCode::AccountExists);

    let bob = store
        .insert_account(new_account(3, "bob@example.org"))
        .await
        .expect("bob");
    assert_eq!(bob.color, AccountColor::new(1).expect("color"));

    assert_eq!(
        store.accounts().await.expect("accounts"),
        [alice.clone(), bob.clone()]
    );
    assert_eq!(
        store.account(alice.id).await.expect("account"),
        Some(alice.clone())
    );
    assert_eq!(store.account(account_id(99)).await.expect("account"), None);

    let identities = store.identities(alice.id).await.expect("identities");
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].email, "alice@example.org");
    assert_eq!(identities[0].name, alice.display_name);
    assert_eq!(identities[0].account_id, alice.id);
    assert!(identities[0].is_default);
    assert!(matches!(
        store.identities(account_id(99)).await,
        Err(StoreError::NotFound(_))
    ));

    let ui = alice.clone().into_account(AccountStatus::connecting());
    assert_eq!((ui.id, ui.writes_enabled), (alice.id, false));

    assert!(store.delete_account(bob.id).await.expect("delete"));
    assert!(!store.delete_account(bob.id).await.expect("delete again"));
    // The address is free again, and the freed color is reused.
    let again = store
        .insert_account(new_account(4, "bob@example.org"))
        .await
        .expect("re-add");
    assert_eq!(again.color, AccountColor::new(1).expect("color"));
}

// ---------------------------------------------------------------------------------------------
// Mailboxes and roles
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn mailbox_list_sync_adds_updates_and_removes() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("account")
        .id;

    let list = vec![
        remote("INBOX", &[]),
        remote("INBOX/Sub", &["\\HasNoChildren"]),
        remote("Gesendet", &[]),
        remote("Projekte", &["\\Noselect", "\\HasChildren"]),
        remote("Projekte/2026", &[]),
        remote("Spam", &[]),
    ];
    let report = store
        .sync_mailboxes(account, list.clone())
        .await
        .expect("sync");
    assert_eq!(report.added.len(), 6);
    assert!(report.removed.is_empty() && report.changed);

    let boxes = store.mailboxes(account).await.expect("mailboxes");
    let paths: Vec<&str> = boxes.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "Gesendet",
            "INBOX",
            "INBOX/Sub",
            "Projekte",
            "Projekte/2026",
            "Spam"
        ]
    );
    let get = |path: &str| {
        boxes
            .iter()
            .find(|m| m.path == path)
            .cloned()
            .expect("mailbox")
    };
    let inbox = get("INBOX");
    assert_eq!(
        (inbox.role, inbox.role_source),
        (MailboxRole::Inbox, RoleSource::Server)
    );
    assert_eq!(get("INBOX/Sub").parent_id, Some(inbox.id));
    assert_eq!(get("INBOX/Sub").name, "Sub");
    assert_eq!(
        (get("Gesendet").role, get("Gesendet").role_source),
        (MailboxRole::Sent, RoleSource::Heuristic)
    );
    assert_eq!(get("Spam").role, MailboxRole::Junk);
    let projekte = get("Projekte");
    assert!(!projekte.selectable);
    assert_eq!(
        (projekte.role, projekte.role_source),
        (MailboxRole::Custom, RoleSource::None)
    );
    assert_eq!(get("Projekte/2026").parent_id, Some(projekte.id));
    assert_eq!(get("Projekte/2026").delimiter.as_deref(), Some("/"));
    assert_eq!((inbox.unread, inbox.total), (0, 0));

    // The same list again changes nothing.
    let report = store
        .sync_mailboxes(account, list.clone())
        .await
        .expect("sync again");
    assert_eq!(report, crate::MailboxListReport::default());

    // A message in Spam, which then disappears from the server.
    let spam = get("Spam").id;
    store
        .reset_mailbox(spam, baseline(1, 2))
        .await
        .expect("reset");
    store
        .store_headers(spam, UidValidity(1), vec![header(1, D, Some("s@x"))], None)
        .await
        .expect("store");

    // Projekte and Spam vanish, Sent with SPECIAL-USE appears, Gesendet loses the guessed role.
    let list = vec![
        remote("INBOX", &[]),
        remote("INBOX/Sub", &["\\HasNoChildren"]),
        remote("Gesendet", &[]),
        remote("Projekte/2026", &["\\HasNoChildren"]),
        remote("Sent", &["\\Sent"]),
    ];
    let report = store.sync_mailboxes(account, list).await.expect("sync");
    assert_eq!(report.removed.len(), 2);
    assert!(report.removed.contains(&projekte.id) && report.removed.contains(&spam));
    assert_eq!(report.added.len(), 1);
    assert!(report.changed);

    let boxes = store.mailboxes(account).await.expect("mailboxes");
    let get = |path: &str| boxes.iter().find(|m| m.path == path).cloned();
    assert_eq!(
        get("Projekte/2026").expect("child kept").parent_id,
        None,
        "a vanished parent does not take its children"
    );
    assert_eq!(get("Sent").expect("sent").role, MailboxRole::Sent);
    assert_eq!(get("Gesendet").expect("gesendet").role, MailboxRole::Custom);
    assert_eq!(
        count(&store, "SELECT count(*) FROM message_location").await,
        0,
        "locations go with the mailbox"
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM message").await,
        1,
        "the message stays as an orphan until the cleanup"
    );
}

#[tokio::test]
async fn a_role_set_by_the_user_is_unique_per_account() {
    let f = fixture().await;
    let store = &f.store;
    store
        .sync_mailboxes(
            f.account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
                remote("Old Archive", &[]),
                remote("Notes", &[]),
                remote("Folder", &["\\Noselect"]),
            ],
        )
        .await
        .expect("sync");
    let id_of = |boxes: &[mail_model::Mailbox], path: &str| {
        boxes
            .iter()
            .find(|m| m.path == path)
            .map(|m| m.id)
            .expect("mailbox")
    };
    let boxes = store.mailboxes(f.account).await.expect("mailboxes");
    let (old, notes, folder) = (
        id_of(&boxes, "Old Archive"),
        id_of(&boxes, "Notes"),
        id_of(&boxes, "Folder"),
    );
    let with_role = |boxes: &[mail_model::Mailbox], role| -> Vec<(String, RoleSource)> {
        boxes
            .iter()
            .filter(|m| m.role == role)
            .map(|m| (m.path.clone(), m.role_source))
            .collect()
    };

    // The user's choice beats SPECIAL-USE; the former archive loses the role.
    let boxes = store
        .set_mailbox_role(old, MailboxRole::Archive)
        .await
        .expect("set role");
    assert_eq!(
        with_role(&boxes, MailboxRole::Archive),
        [("Old Archive".to_owned(), RoleSource::User)]
    );
    let archive = boxes.iter().find(|m| m.path == "Archive").expect("archive");
    assert_eq!(
        (archive.role, archive.role_source),
        (MailboxRole::Custom, RoleSource::None)
    );

    // A second user choice of the same role moves it; the first mailbox falls back to automatic.
    let boxes = store
        .set_mailbox_role(notes, MailboxRole::Archive)
        .await
        .expect("move role");
    assert_eq!(
        with_role(&boxes, MailboxRole::Archive),
        [("Notes".to_owned(), RoleSource::User)]
    );
    let old_box = boxes.iter().find(|m| m.id == old).expect("old");
    assert_eq!(old_box.role_source, RoleSource::None);

    // "custom" by the user blocks the automatic role, also across list refreshes.
    let boxes = store
        .set_mailbox_role(f.sent, MailboxRole::Custom)
        .await
        .expect("custom");
    assert!(with_role(&boxes, MailboxRole::Sent).is_empty());
    let report = store
        .sync_mailboxes(
            f.account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
                remote("Old Archive", &[]),
                remote("Notes", &[]),
                remote("Folder", &["\\Noselect"]),
            ],
        )
        .await
        .expect("refresh");
    assert!(!report.changed, "user roles survive a refresh");
    let boxes = store.mailboxes(f.account).await.expect("mailboxes");
    assert_eq!(
        with_role(&boxes, MailboxRole::Archive),
        [("Notes".to_owned(), RoleSource::User)]
    );
    let sent = boxes.iter().find(|m| m.id == f.sent).expect("sent");
    assert_eq!(
        (sent.role, sent.role_source),
        (MailboxRole::Custom, RoleSource::User)
    );

    // Refused: the inbox role, the inbox itself, placeholders, unknown mailboxes.
    for (mailbox, role) in [
        (notes, MailboxRole::Inbox),
        (f.inbox, MailboxRole::Archive),
        (f.inbox, MailboxRole::Custom),
        (folder, MailboxRole::Trash),
    ] {
        let err = store
            .set_mailbox_role(mailbox, role)
            .await
            .expect_err("refused");
        assert!(matches!(err, StoreError::InvalidArgument(_)), "{err:?}");
    }
    assert!(matches!(
        store
            .set_mailbox_role(MailboxId(9_999), MailboxRole::Trash)
            .await,
        Err(StoreError::NotFound(_))
    ));

    // The schema itself refuses a second mailbox with the same role.
    let notes_id = notes.0;
    let err = store
        .write(move |tx| {
            Ok(tx.execute(
                "UPDATE mailbox SET role = 'inbox' WHERE id = ?1",
                [notes_id],
            )?)
        })
        .await
        .expect_err("unique index mailbox_role");
    assert!(err.to_string().contains("UNIQUE"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Storing headers, merge rule, ids
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn storing_the_same_window_twice_creates_no_duplicates() {
    let f = fixture().await;
    let window: Vec<_> = (1..=50)
        .map(|uid| header(uid, D + i64::from(uid) * 60, Some(&format!("m{uid}@x"))))
        .collect();

    let first = f
        .store
        .store_headers(f.inbox, UV, window.clone(), Some(1))
        .await
        .expect("store");
    assert_eq!(
        first,
        StoreReport {
            inserted: 50,
            merged: 0,
            updated: 0
        }
    );
    let ids = all_ids(&f.store, f.inbox, 500).await;

    let second = f
        .store
        .store_headers(f.inbox, UV, window, Some(1))
        .await
        .expect("store again");
    assert_eq!(
        second,
        StoreReport {
            inserted: 0,
            merged: 0,
            updated: 50
        }
    );
    assert_eq!(all_ids(&f.store, f.inbox, 500).await, ids);
    assert_eq!(count(&f.store, "SELECT count(*) FROM message").await, 50);
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM message_location").await,
        50
    );
}

#[tokio::test]
async fn the_same_mail_in_two_mailboxes_is_one_message_with_two_locations() {
    let f = fixture().await;
    let report = f
        .store
        .store_headers(f.inbox, UV, vec![header(1, D, Some("a@x"))], None)
        .await
        .expect("inbox");
    assert_eq!(report.inserted, 1);
    // The archive copy has another UID, other flags and another INTERNALDATE.
    let report = f
        .store
        .store_headers(
            f.archive,
            UV,
            vec![seen(header(7, D + 3_600, Some("a@x")))],
            None,
        )
        .await
        .expect("archive");
    assert_eq!(
        report,
        StoreReport {
            inserted: 0,
            merged: 1,
            updated: 0
        }
    );

    assert_eq!(count(&f.store, "SELECT count(*) FROM message").await, 1);
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM message_location").await,
        2
    );
    let inbox = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("inbox");
    let archive = f
        .store
        .list_messages(f.archive, None, 10)
        .await
        .expect("archive");
    assert_eq!(
        inbox.items[0].id,
        MessageId(D),
        "id stays the first sort date"
    );
    assert_eq!(archive.items[0].id, MessageId(D));
    assert!(inbox.items[0].unread, "flags belong to the location");
    assert!(!archive.items[0].unread);
    assert_eq!(
        f.store.known_uids(f.archive).await.expect("uids"),
        UidSet::from_uids([7])
    );
}

#[tokio::test]
async fn a_duplicate_message_id_within_one_mailbox_gets_its_own_row() {
    let f = fixture().await;
    // A mailing list copy and a direct copy: same Message-ID, both in the inbox.
    let report = f
        .store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, Some("dup@x")), header(2, D + 5, Some("dup@x"))],
            None,
        )
        .await
        .expect("inbox");
    assert_eq!(report.inserted, 2);

    // Copies in the archive pair up with them one by one, a third gets a row of its own.
    let report = f
        .store
        .store_headers(
            f.archive,
            UV,
            vec![
                header(1, D, Some("dup@x")),
                header(2, D + 5, Some("dup@x")),
                header(3, D + 9, Some("dup@x")),
            ],
            None,
        )
        .await
        .expect("archive");
    assert_eq!(
        report,
        StoreReport {
            inserted: 1,
            merged: 2,
            updated: 0
        }
    );
    assert_eq!(count(&f.store, "SELECT count(*) FROM message").await, 3);
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM message_location").await,
        5
    );
    // The schema rule holds: no message twice in one mailbox.
    assert_eq!(
        all_ids(&f.store, f.archive, 500).await,
        [D + 9, D + 5, D],
        "each archive copy has its own message"
    );
}

#[tokio::test]
async fn mails_without_message_id_are_never_merged() {
    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, None), header(2, D, Some(""))],
            None,
        )
        .await
        .expect("inbox");
    let report = f
        .store
        .store_headers(f.archive, UV, vec![header(1, D, None)], None)
        .await
        .expect("archive");
    assert_eq!(report.inserted, 1);
    assert_eq!(count(&f.store, "SELECT count(*) FROM message").await, 3);
    assert_eq!(
        count(
            &f.store,
            "SELECT count(*) FROM message WHERE message_id_hdr IS NULL"
        )
        .await,
        3,
        "an empty Message-ID is stored as missing"
    );
}

#[tokio::test]
async fn the_message_id_is_the_sort_date() {
    let f = fixture().await;
    let mut within = header(1, D, Some("1@x"));
    within.date = Some(D + 3_600); // Date header one hour after delivery: trusted
    let mut beyond = header(2, D + 100, Some("2@x"));
    beyond.date = Some(D + 100 + 2 * 86_400); // two days in the future: INTERNALDATE
    let mut missing = header(3, D + 200, Some("3@x"));
    missing.date = None;
    let mut imported = header(4, D + 300, Some("4@x"));
    imported.date = Some(1_000_000_000); // old mail imported later: the Date header

    f.store
        .store_headers(f.inbox, UV, vec![within, beyond, missing, imported], None)
        .await
        .expect("store");
    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    let got: Vec<(i64, i64)> = page.items.iter().map(|i| (i.id.0, i.date)).collect();
    assert_eq!(
        got,
        [
            (D + 3_600, (D + 3_600) * 1000),
            (D + 200, (D + 200) * 1000),
            (D + 100, (D + 100) * 1000),
            (1_000_000_000, 1_000_000_000_000),
        ]
    );
}

#[tokio::test]
async fn many_mails_with_the_same_date_get_unique_ids_in_a_stable_order() {
    let f = fixture().await;
    let n: u32 = 1_500;
    // As the first sync does: windows of 500, from the highest UID down.
    let all = UidSet::range(1, n);
    let mut rest = all;
    while !rest.is_empty() {
        let (window, remaining) = rest.split_highest(500);
        let headers = window.iter().map(|uid| header(uid, D, None)).collect();
        let report = f
            .store
            .store_headers(f.inbox, UV, headers, window.min())
            .await
            .expect("window");
        assert_eq!(report.inserted, 500);
        rest = remaining;
    }

    let ids = all_ids(&f.store, f.inbox, 100).await;
    let expected: Vec<i64> = (D..D + i64::from(n)).rev().collect();
    assert_eq!(ids, expected, "dense, unique ids from the shared date up");

    // Within a window the later arrival (higher UID) gets the higher id.
    let inbox = f.inbox.0;
    let by_uid = f
        .store
        .read(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT message_id FROM message_location WHERE mailbox_id = ?1 ORDER BY uid",
            )?;
            let rows = stmt.query_map([inbox], |r| r.get::<_, i64>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
        .expect("ids by uid");
    for window in by_uid.chunks(500) {
        assert!(
            window.windows(2).all(|w| w[0] < w[1]),
            "ascending in a window"
        );
    }

    // A later mail whose date is taken goes to the next free number above the run; one with
    // a free date keeps it.
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(2_000, D + 10, None), header(2_001, D - 1, None)],
            None,
        )
        .await
        .expect("more");
    let again = all_ids(&f.store, f.inbox, 100).await;
    assert_eq!(again.len(), 1_502);
    assert_eq!(again[0], D + i64::from(n), "next free above the run");
    assert_eq!(*again.last().expect("last"), D - 1);
    assert_eq!(&again[1..=1_500], &expected[..], "existing ids never move");
    assert_eq!(
        again.iter().collect::<HashSet<_>>().len(),
        again.len(),
        "unique"
    );
    assert_eq!(
        all_ids(&f.store, f.inbox, 37).await,
        again,
        "the order is the same with every page size"
    );
}

// ---------------------------------------------------------------------------------------------
// Flags, removals, orphans
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn flag_updates_count_real_changes_only() {
    let f = fixture().await;
    let mut third = header(3, D + 3, Some("3@x"));
    third.flags = flags(false, &["$Forwarded"]);
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![
                header(1, D + 1, Some("1@x")),
                header(2, D + 2, Some("2@x")),
                third,
            ],
            None,
        )
        .await
        .expect("store");

    let updates = vec![
        // seen: a change
        FlagUpdate {
            uid: 1,
            flags: flags(true, &[]),
            modseq: Some(ModSeq(10)),
        },
        // same flags, new MODSEQ: stored, not counted
        FlagUpdate {
            uid: 2,
            flags: flags(false, &[]),
            modseq: Some(ModSeq(11)),
        },
        // keyword in other case (Stalwart lower-cases): no change, stored spelling kept
        FlagUpdate {
            uid: 3,
            flags: flags(false, &["$forwarded"]),
            modseq: None,
        },
        // not stored: ignored
        FlagUpdate {
            uid: 99,
            flags: flags(true, &[]),
            modseq: None,
        },
    ];
    let changed = f
        .store
        .update_flags(f.inbox, UV, updates.clone())
        .await
        .expect("update");
    assert_eq!(changed, 1);
    assert_eq!(
        f.store
            .update_flags(f.inbox, UV, updates)
            .await
            .expect("again"),
        0,
        "idempotent"
    );

    let changed = f
        .store
        .update_flags(
            f.inbox,
            UV,
            vec![FlagUpdate {
                uid: 3,
                flags: Flags {
                    flagged: true,
                    answered: true,
                    keywords: vec!["projekt".into(), "$Forwarded".into(), "bad word".into()],
                    ..Flags::default()
                },
                modseq: Some(ModSeq(12)),
            }],
        )
        .await
        .expect("update");
    assert_eq!(changed, 1);

    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    let item = |uid_date: i64| {
        page.items
            .iter()
            .find(|i| i.id.0 == uid_date)
            .cloned()
            .expect("item")
    };
    assert!(!item(D + 1).unread);
    assert!(item(D + 2).unread);
    let third = item(D + 3);
    assert!(third.flagged && third.answered && third.unread);
    assert_eq!(
        third.keywords,
        ["$Forwarded", "projekt"],
        "normalised; not an atom dropped"
    );

    let modseqs = f
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT modseq FROM message_location ORDER BY uid")?;
            let rows = stmt.query_map([], |r| r.get::<_, Option<i64>>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
        .expect("modseqs");
    assert_eq!(modseqs, [Some(10), Some(11), Some(12)]);
}

#[tokio::test]
async fn removed_uids_leave_orphans_until_the_cleanup() {
    let f = fixture().await;
    let inbox: Vec<_> = (1..=4)
        .map(|uid| header(uid, D + i64::from(uid), Some(&format!("m{uid}@x"))))
        .collect();
    f.store
        .store_headers(f.inbox, UV, inbox, None)
        .await
        .expect("inbox");
    f.store
        .store_headers(f.archive, UV, vec![header(1, D + 1, Some("m1@x"))], None)
        .await
        .expect("archive");

    // Another account's orphan must survive Alice's cleanup.
    let bob = f
        .store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("bob")
        .id;
    f.store
        .sync_mailboxes(bob, vec![remote("INBOX", &[])])
        .await
        .expect("bob mailboxes");
    let bob_inbox = f.store.mailboxes(bob).await.expect("boxes")[0].id;
    f.store
        .reset_mailbox(bob_inbox, baseline(5, 2))
        .await
        .expect("reset");
    f.store
        .store_headers(
            bob_inbox,
            UidValidity(5),
            vec![header(1, D, Some("b@x"))],
            None,
        )
        .await
        .expect("bob mail");
    f.store
        .remove_uids(bob_inbox, UidValidity(5), UidSet::all())
        .await
        .expect("bob remove");

    let removed = f
        .store
        .remove_uids(f.inbox, UV, UidSet::from_uids([1, 2, 77]))
        .await
        .expect("remove");
    assert_eq!(removed, 2);
    assert_eq!(
        f.store.known_uids(f.inbox).await.expect("uids"),
        UidSet::from_uids([3, 4])
    );
    assert_eq!(count(&f.store, "SELECT count(*) FROM message").await, 5);

    // m2 lies nowhere now; m1 still lies in the archive.
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("cleanup"),
        1
    );
    assert_eq!(
        all_ids(&f.store, f.archive, 10).await,
        [D + 1],
        "a message with another location stays"
    );

    assert_eq!(
        f.store
            .remove_uids(f.inbox, UV, UidSet::all())
            .await
            .expect("remove all"),
        2
    );
    assert!(f.store.known_uids(f.inbox).await.expect("uids").is_empty());
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("cleanup"),
        2
    );
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM message").await,
        2,
        "m1 of Alice and Bob's orphan"
    );
    assert_eq!(f.store.delete_orphan_messages(bob).await.expect("bob"), 1);
}

// ---------------------------------------------------------------------------------------------
// UIDVALIDITY
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_uidvalidity_change_resets_the_mailbox_and_voids_stale_writes() {
    let f = fixture().await;
    let mut window: Vec<_> = (1..=3)
        .map(|uid| header(uid, D + i64::from(uid), Some(&format!("m{uid}@x"))))
        .collect();
    window.push(header(4, D + 4, None));
    f.store
        .store_headers(f.inbox, UV, window.clone(), Some(1))
        .await
        .expect("store");
    let ids_before = all_ids(&f.store, f.inbox, 10).await;

    // Writes computed for another UIDVALIDITY fail and write nothing.
    let stale = UidValidity(999);
    let mismatch = |err: StoreError, stored: u32, given: u32| {
        assert!(
            matches!(err, StoreError::UidValidityMismatch { stored: s, given: g, .. }
                     if s == Some(stored) && g == given),
            "{err:?}"
        );
    };
    mismatch(
        f.store
            .store_headers(f.inbox, stale, vec![header(9, D + 9, None)], Some(1))
            .await
            .expect_err("stale store"),
        UV.0,
        999,
    );
    mismatch(
        f.store
            .update_flags(
                f.inbox,
                stale,
                vec![FlagUpdate {
                    uid: 1,
                    flags: flags(true, &[]),
                    modseq: None,
                }],
            )
            .await
            .expect_err("stale flags"),
        UV.0,
        999,
    );
    mismatch(
        f.store
            .remove_uids(f.inbox, stale, UidSet::all())
            .await
            .expect_err("stale remove"),
        UV.0,
        999,
    );
    mismatch(
        f.store
            .set_uid_next(f.inbox, stale, 50)
            .await
            .expect_err("stale uidnext"),
        UV.0,
        999,
    );
    mismatch(
        f.store
            .set_highest_modseq(f.inbox, stale, Some(ModSeq(5)))
            .await
            .expect_err("stale modseq"),
        UV.0,
        999,
    );
    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    assert_eq!(page.items.len(), 4);
    assert!(page.items.iter().all(|i| i.unread), "no flag written");

    // The server reports a new UIDVALIDITY: the mapping is dropped, the messages stay.
    let removed = f
        .store
        .reset_mailbox(
            f.inbox,
            MailboxBaseline {
                uid_validity: UidValidity(2_000),
                uid_next: 10,
                highest_modseq: Some(ModSeq(77)),
            },
        )
        .await
        .expect("reset");
    assert_eq!(removed, 4);
    assert!(f.store.known_uids(f.inbox).await.expect("uids").is_empty());
    assert_eq!(count(&f.store, "SELECT count(*) FROM message").await, 4);
    let state = f
        .store
        .mailbox_sync_states(f.account)
        .await
        .expect("states")
        .into_iter()
        .find(|s| s.mailbox_id == f.inbox)
        .expect("inbox state");
    assert_eq!(state.uid_validity, Some(UidValidity(2_000)));
    assert_eq!(state.uid_next, Some(10));
    assert_eq!(state.highest_modseq, Some(ModSeq(77)));
    assert_eq!(state.sync_low_uid, Some(10), "nothing stored yet");
    assert!(!state.first_sync_complete());

    // Writes for the old generation are void now.
    mismatch(
        f.store
            .store_headers(f.inbox, UV, window, None)
            .await
            .expect_err("old generation"),
        2_000,
        UV.0,
    );

    // The re-sync merges the old messages back by Message-ID: same ids, local data kept.
    let resync: Vec<_> = (1..=3)
        .map(|n| header(10 + n, D + i64::from(n), Some(&format!("m{n}@x"))))
        .chain([header(14, D + 4, None)])
        .collect();
    let report = f
        .store
        .store_headers(f.inbox, UidValidity(2_000), resync, Some(1))
        .await
        .expect("resync");
    assert_eq!(
        report,
        StoreReport {
            inserted: 1,
            merged: 3,
            updated: 0
        }
    );
    let ids_after = all_ids(&f.store, f.inbox, 10).await;
    assert_eq!(ids_after.len(), 4);
    assert_eq!(
        &ids_after[1..],
        &ids_before[1..],
        "merged messages keep their ids"
    );
    assert_ne!(
        ids_after[0], ids_before[0],
        "the mail without Message-ID is a new row"
    );
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("cleanup"),
        1,
        "only the old copy of the mail without Message-ID"
    );

    // A mailbox that never had a sync rejects sync writes until its first reset.
    let boxes = f
        .store
        .sync_mailboxes(
            f.account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
                remote("New", &[]),
            ],
        )
        .await
        .expect("sync");
    let new = boxes.added[0];
    let err = f
        .store
        .store_headers(new, UV, vec![], None)
        .await
        .expect_err("not reset yet");
    assert!(
        matches!(err, StoreError::UidValidityMismatch { stored: None, .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn sync_progress_is_recorded() {
    let f = fixture().await;
    f.store
        .store_headers(f.inbox, UV, vec![header(5, D, None)], Some(5))
        .await
        .expect("window");
    f.store.set_uid_next(f.inbox, UV, 6).await.expect("uidnext");
    f.store
        .set_highest_modseq(f.inbox, UV, Some(ModSeq(1 << 62)))
        .await
        .expect("modseq");
    let state = |states: Vec<crate::MailboxSyncState>| {
        states
            .into_iter()
            .find(|s| s.mailbox_id == f.inbox)
            .expect("inbox")
    };
    let s = state(
        f.store
            .mailbox_sync_states(f.account)
            .await
            .expect("states"),
    );
    assert_eq!(
        (s.uid_validity, s.uid_next, s.highest_modseq, s.sync_low_uid),
        (Some(UV), Some(6), Some(ModSeq(1 << 62)), Some(5))
    );
    assert_eq!((s.name.as_str(), s.path.as_str()), ("INBOX", "INBOX"));
    assert!(s.selectable && s.role == MailboxRole::Inbox);
    assert!(!s.first_sync_complete());

    f.store
        .store_headers(f.inbox, UV, vec![], Some(1))
        .await
        .expect("last window");
    f.store
        .set_highest_modseq(f.inbox, UV, None)
        .await
        .expect("no modseq");
    let s = state(
        f.store
            .mailbox_sync_states(f.account)
            .await
            .expect("states"),
    );
    assert!(s.first_sync_complete());
    assert_eq!(s.highest_modseq, None);

    let err = f
        .store
        .set_highest_modseq(f.inbox, UV, Some(ModSeq(u64::MAX)))
        .await
        .expect_err("beyond RFC 7162");
    assert!(matches!(err, StoreError::InvalidArgument(_)), "{err:?}");
}

// ---------------------------------------------------------------------------------------------
// Lists and counters
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn keyset_pages_are_complete_and_free_of_duplicates_while_mail_arrives() {
    let f = fixture().await;
    // 1,000 mails; every tenth shares its date with the previous one (collisions).
    let originals: Vec<_> = (1..=1_000u32)
        .map(|uid| {
            let slot = i64::from(uid - uid / 10);
            header(uid, D + slot * 60, None)
        })
        .collect();
    for window in originals.chunks(500) {
        f.store
            .store_headers(f.inbox, UV, window.to_vec(), None)
            .await
            .expect("store");
    }
    let original_ids: HashSet<i64> = all_ids(&f.store, f.inbox, 500).await.into_iter().collect();
    assert_eq!(original_ids.len(), 1_000);

    let mut seen_ids = Vec::new();
    let mut older_inserted = Vec::new();
    let mut cursor: Option<ListCursor> = None;
    let mut uid = 10_000;
    loop {
        let page = f
            .store
            .list_messages(f.inbox, cursor, 64)
            .await
            .expect("page");
        seen_ids.extend(page.items.iter().map(|i| i.id.0));
        let Some(next) = page.next_cursor else { break };
        cursor = Some(next);

        // Between two pages: a new mail on top, one older than everything, and one dated
        // just above the cursor (already passed). Only the older one belongs to later pages.
        let older = D - 1_000_000 - i64::from(uid);
        older_inserted.push(older);
        f.store
            .store_headers(
                f.inbox,
                UV,
                vec![
                    header(uid, D + 10_000_000 + i64::from(uid), None),
                    header(uid + 1, older, None),
                    header(uid + 2, next.before().0 + 1, None),
                ],
                None,
            )
            .await
            .expect("arrive");
        uid += 3;
    }

    let unique: HashSet<i64> = seen_ids.iter().copied().collect();
    assert_eq!(unique.len(), seen_ids.len(), "no duplicates");
    assert!(
        seen_ids.windows(2).all(|w| w[0] > w[1]),
        "strictly newest first across pages"
    );
    assert!(original_ids.is_subset(&unique), "no mail skipped");
    for older in &older_inserted {
        assert!(
            unique.contains(older),
            "older arrivals appear on later pages"
        );
    }

    // A fresh pass sees everything that is stored.
    let total = count(&f.store, "SELECT count(*) FROM message_location").await;
    assert_eq!(
        i64::try_from(all_ids(&f.store, f.inbox, 64).await.len()).expect("len"),
        total
    );
}

#[tokio::test]
async fn list_items_carry_the_header_data_and_bad_requests_are_refused() {
    let f = fixture().await;
    let mut full = header(1, D, Some("full@x"));
    full.subject = String::new();
    full.from = vec![
        Address {
            name: Some("Jörg Müller".into()),
            email: "joerg@example.de".into(),
        },
        Address {
            name: None,
            email: "second@example.de".into(),
        },
    ];
    full.to = vec![
        Address {
            name: Some("Me".into()),
            email: "me@example.org".into(),
        },
        Address {
            name: None,
            email: "team@example.org".into(),
        },
    ];
    full.cc = vec![Address {
        name: None,
        email: "cc@example.org".into(),
    }];
    full.flags = Flags {
        seen: true,
        flagged: true,
        answered: true,
        draft: true,
        ..Flags::default()
    };
    full.has_attachments = true;
    let mut anonymous = header(2, D - 10, None);
    anonymous.from.clear();
    anonymous.to.clear();
    f.store
        .store_headers(f.inbox, UV, vec![full.clone(), anonymous], None)
        .await
        .expect("store");

    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    assert_eq!(page.next_cursor, None, "a single page");
    let item = &page.items[0];
    assert_eq!(item.id, MessageId(D));
    assert_eq!(item.account_id, f.account);
    assert_eq!(item.mailbox_id, f.inbox);
    assert_eq!(item.from, Some(full.from[0].clone()), "first From address");
    assert_eq!(item.to, full.to);
    assert_eq!(item.subject, "");
    assert_eq!(item.date, D * 1000);
    assert!(!item.unread && item.flagged && item.answered && item.draft);
    assert!(item.has_attachments);
    assert!(item.keywords.is_empty());
    assert_eq!(page.items[1].from, None);
    assert!(page.items[1].to.is_empty());

    for limit in [0, MAX_PAGE_SIZE + 1] {
        let err = f
            .store
            .list_messages(f.inbox, None, limit)
            .await
            .expect_err("limit");
        assert!(matches!(err, StoreError::InvalidArgument(_)), "{err:?}");
        assert_eq!(CoreError::from(err).code(), ErrorCode::InvalidArgument);
    }
    f.store
        .list_messages(f.inbox, None, MAX_PAGE_SIZE)
        .await
        .expect("largest page");
    assert!(matches!(
        f.store.list_messages(MailboxId(9_999), None, 10).await,
        Err(StoreError::NotFound(_))
    ));
    let empty = f
        .store
        .list_messages(f.sent, None, 10)
        .await
        .expect("empty");
    assert!(empty.items.is_empty() && empty.next_cursor.is_none());

    // Page boundaries: an exactly full last page has no cursor.
    let p1 = f.store.list_messages(f.inbox, None, 1).await.expect("p1");
    assert_eq!(p1.next_cursor, Some(ListCursor::after(MessageId(D))));
    let p2 = f
        .store
        .list_messages(f.inbox, p1.next_cursor, 1)
        .await
        .expect("p2");
    assert_eq!(p2.items.len(), 1);
    assert_eq!(p2.next_cursor, None);
    assert_eq!(
        f.store
            .list_messages(f.inbox, None, 2)
            .await
            .expect("p")
            .next_cursor,
        None
    );
}

#[tokio::test]
async fn unread_counts_follow_the_flags() {
    let f = fixture().await;
    let inbox = (1..=5)
        .map(|uid| {
            let h = header(uid, D + i64::from(uid), Some(&format!("{uid}@x")));
            if uid <= 2 { seen(h) } else { h }
        })
        .collect();
    f.store
        .store_headers(f.inbox, UV, inbox, None)
        .await
        .expect("inbox");
    let archive = (1..=3)
        .map(|uid| seen(header(uid, D + 100 + i64::from(uid), None)))
        .collect();
    f.store
        .store_headers(f.archive, UV, archive, None)
        .await
        .expect("archive");

    let counts = f.store.unread_counts(f.account).await.expect("counts");
    assert_eq!(counts, [(f.archive, 0), (f.inbox, 3), (f.sent, 0)]);
    let boxes = f.store.mailboxes(f.account).await.expect("mailboxes");
    let totals: Vec<(MailboxId, u32, u32)> =
        boxes.iter().map(|m| (m.id, m.unread, m.total)).collect();
    assert_eq!(totals, [(f.archive, 0, 3), (f.inbox, 3, 5), (f.sent, 0, 0)]);

    f.store
        .update_flags(
            f.inbox,
            UV,
            vec![
                FlagUpdate {
                    uid: 3,
                    flags: flags(true, &[]),
                    modseq: None,
                },
                FlagUpdate {
                    uid: 1,
                    flags: flags(false, &[]),
                    modseq: None,
                },
                FlagUpdate {
                    uid: 4,
                    flags: flags(true, &[]),
                    modseq: None,
                },
            ],
        )
        .await
        .expect("flags");
    let counts = f.store.unread_counts(f.account).await.expect("counts");
    assert_eq!(counts[1], (f.inbox, 2));
    assert!(matches!(
        f.store.unread_counts(account_id(77)).await,
        Err(StoreError::NotFound(_))
    ));
}

// ---------------------------------------------------------------------------------------------
// Removing an account
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn deleting_an_account_deletes_all_its_local_data() {
    let f = fixture().await;
    let window: Vec<_> = (1..=20)
        .map(|uid| header(uid, D + i64::from(uid), Some(&format!("{uid}@x"))))
        .collect();
    f.store
        .store_headers(f.inbox, UV, window.clone(), None)
        .await
        .expect("inbox");
    f.store
        .store_headers(f.archive, UV, window[..5].to_vec(), None)
        .await
        .expect("archive");

    let bob = f
        .store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("bob")
        .id;
    f.store
        .sync_mailboxes(bob, vec![remote("INBOX", &[]), remote("Sub", &[])])
        .await
        .expect("bob mailboxes");
    let bob_inbox = f
        .store
        .mailboxes(bob)
        .await
        .expect("boxes")
        .into_iter()
        .find(|m| m.role == MailboxRole::Inbox)
        .expect("inbox")
        .id;
    f.store
        .reset_mailbox(bob_inbox, baseline(3, 1))
        .await
        .expect("reset");
    // Same Message-IDs as Alice's: never merged across accounts.
    let report = f
        .store
        .store_headers(bob_inbox, UidValidity(3), window[..7].to_vec(), None)
        .await
        .expect("bob mail");
    assert_eq!(report.inserted, 7);

    assert!(f.store.delete_account(f.account).await.expect("delete"));

    for (table, expected) in [
        ("SELECT count(*) FROM account", 1),
        ("SELECT count(*) FROM identity", 1),
        ("SELECT count(*) FROM mailbox", 2),
        ("SELECT count(*) FROM message", 7),
        ("SELECT count(*) FROM message_location", 7),
    ] {
        assert_eq!(count(&f.store, table).await, expected, "{table}");
    }
    let dangling = f
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
            let n = stmt.query_map([], |_| Ok(()))?.count();
            Ok(n)
        })
        .await
        .expect("foreign_key_check");
    assert_eq!(dangling, 0);

    assert!(matches!(
        f.store.list_messages(f.inbox, None, 10).await,
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        f.store.mailboxes(f.account).await,
        Err(StoreError::NotFound(_))
    ));
    assert_eq!(all_ids(&f.store, bob_inbox, 100).await.len(), 7);
    assert_eq!(f.store.accounts().await.expect("accounts").len(), 1);
}

#[tokio::test]
async fn unknown_ids_are_reported_as_not_found() {
    let f = fixture().await;
    let ghost = MailboxId(4_242);
    let nobody = account_id(4_242);
    let errors = [
        f.store
            .store_headers(ghost, UV, vec![], None)
            .await
            .expect_err("store"),
        f.store
            .update_flags(ghost, UV, vec![])
            .await
            .expect_err("flags"),
        f.store
            .remove_uids(ghost, UV, UidSet::all())
            .await
            .expect_err("remove"),
        f.store.known_uids(ghost).await.expect_err("uids"),
        f.store
            .set_uid_next(ghost, UV, 1)
            .await
            .expect_err("uidnext"),
        f.store
            .set_highest_modseq(ghost, UV, None)
            .await
            .expect_err("modseq"),
        f.store
            .reset_mailbox(ghost, baseline(1, 1))
            .await
            .expect_err("reset"),
        f.store
            .sync_mailboxes(nobody, vec![])
            .await
            .expect_err("sync"),
        f.store.mailboxes(nobody).await.expect_err("mailboxes"),
        f.store
            .mailbox_sync_states(nobody)
            .await
            .expect_err("states"),
        f.store
            .delete_orphan_messages(nobody)
            .await
            .expect_err("orphans"),
    ];
    for err in errors {
        assert!(matches!(err, StoreError::NotFound(_)), "{err:?}");
        assert_eq!(CoreError::from(err).code(), ErrorCode::NotFound);
    }
}
