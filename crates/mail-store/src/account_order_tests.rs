//! Stage 22 (decision 0078): the user's order of the accounts and their names in the lists.

use mail_model::AccountId;
use rusqlite_migration::M;
use serde_json::json;

use crate::migrations::MIGRATIONS;
use crate::store::Schema;
use crate::test_support::{account_id, db_path, new_account};
use crate::{Store, StoreError, StoreOptions};

async fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    (dir, store)
}

async fn order(store: &Store) -> Vec<AccountId> {
    store
        .accounts()
        .await
        .expect("accounts")
        .into_iter()
        .map(|a| a.id)
        .collect()
}

async fn three(store: &Store) -> [AccountId; 3] {
    for (n, email) in [
        (1, "ann@example.org"),
        (2, "bob@example.org"),
        (3, "cy@example.org"),
    ] {
        store
            .insert_account(new_account(n, email))
            .await
            .expect("account");
    }
    [account_id(1), account_id(2), account_id(3)]
}

#[tokio::test]
async fn accounts_keep_the_order_they_were_added_until_the_user_moves_one() {
    let (_dir, store) = open().await;
    let [a, b, c] = three(&store).await;
    assert_eq!(order(&store).await, [a, b, c]);

    // C to the top: the order holds for every list of accounts.
    let moved = store
        .reorder_accounts(vec![c, a, b])
        .await
        .expect("reorder");
    assert_eq!(moved.iter().map(|x| x.id).collect::<Vec<_>>(), [c, a, b]);
    assert_eq!(order(&store).await, [c, a, b]);
    assert_eq!(
        store
            .diagnostics_stats()
            .await
            .expect("stats")
            .accounts
            .iter()
            .map(|s| s.account)
            .collect::<Vec<_>>(),
        [c, a, b],
        "the diagnostics list them in the same order"
    );

    // Left out: they follow in their order; repeated: counts once.
    store.reorder_accounts(vec![b, b]).await.expect("reorder");
    assert_eq!(order(&store).await, [b, c, a]);

    // An account added later goes to the end, after the placed ones.
    store
        .insert_account(new_account(4, "dee@example.org"))
        .await
        .expect("account");
    assert_eq!(order(&store).await, [b, c, a, account_id(4)]);

    // Unknown: nothing changes.
    let err = store
        .reorder_accounts(vec![a, account_id(9)])
        .await
        .expect_err("unknown");
    assert!(matches!(err, StoreError::NotFound(_)), "{err:?}");
    assert_eq!(order(&store).await, [b, c, a, account_id(4)]);
}

/// Every move gives every account a fresh position above all earlier ones, so each account
/// record changes and the newest order wins as a whole across devices (decision 0078).
#[tokio::test]
async fn every_move_gives_every_account_a_fresh_position() {
    let (_dir, store) = open().await;
    let [a, b, c] = three(&store).await;
    let positions = |store: &Store| {
        let store = store.clone();
        async move {
            store
                .read(|c| {
                    c.prepare("SELECT uuid, position FROM account ORDER BY id")?
                        .query_map([], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(StoreError::from)
                })
                .await
                .expect("positions")
        }
    };
    assert!(
        positions(&store).await.iter().all(|(_, p)| p.is_none()),
        "nobody placed yet"
    );
    store.reorder_accounts(vec![b, a, c]).await.expect("first");
    let first = positions(&store).await;
    store
        .reorder_accounts(vec![b, a, c])
        .await
        .expect("same order again");
    let second = positions(&store).await;
    for ((_, before), (_, after)) in first.iter().zip(&second) {
        assert!(after > before, "{before:?} -> {after:?}");
    }
    assert_eq!(order(&store).await, [b, a, c]);
}

#[tokio::test]
async fn new_accounts_show_their_address_and_keep_the_typed_name_for_sending() {
    let (_dir, store) = open().await;
    let stored = store
        .insert_account(new_account(1, "ann@example.org"))
        .await
        .expect("account");
    assert_eq!(stored.display_name, "", "the lists show the address");
    let identity = store.identities(stored.id).await.expect("identities");
    assert_eq!(
        identity[0].name, "User 1",
        "the sender name is the typed one"
    );
}

#[tokio::test]
async fn renaming_changes_the_list_name_only() {
    let (_dir, store) = open().await;
    let stored = store
        .insert_account(new_account(1, "ann@example.org"))
        .await
        .expect("account");
    let renamed = store
        .rename_account(stored.id, "  Arbeit ".into())
        .await
        .expect("rename");
    assert_eq!(renamed.display_name, "Arbeit", "trimmed");
    assert_eq!(
        store
            .account(stored.id)
            .await
            .expect("read")
            .expect("account")
            .display_name,
        "Arbeit"
    );
    let identity = store.identities(stored.id).await.expect("identities");
    assert_eq!(identity[0].name, "User 1", "the sender name stays");

    // Empty: the address again.
    let cleared = store
        .rename_account(stored.id, "   ".into())
        .await
        .expect("clear");
    assert_eq!(cleared.display_name, "");

    for bad in ["Ar\nbeit", "Tab\there", &"x".repeat(101)] {
        let err = store
            .rename_account(stored.id, bad.to_owned())
            .await
            .expect_err("refused");
        assert!(
            matches!(err, StoreError::InvalidArgument(_)),
            "{bad:?}: {err:?}"
        );
    }
    assert!(
        store
            .rename_account(stored.id, "ü".repeat(100))
            .await
            .is_ok(),
        "100 characters"
    );
    let err = store
        .rename_account(account_id(9), "Privat".into())
        .await
        .expect_err("unknown");
    assert!(matches!(err, StoreError::NotFound(_)), "{err:?}");
}

/// The schema just before migration 0032, wherever it stands in the list.
fn before_0032() -> Schema {
    let at = MIGRATIONS
        .iter()
        .position(|m| *m == M::up(include_str!("../migrations/0032_account_order.sql")))
        .expect("migration 0032 is in the list");
    Schema::from_vec(MIGRATIONS[..at].to_vec())
}

/// Migration 0032 keeps the order of the lists (the order the accounts were added in) and their
/// names; nobody placed an account yet.
#[tokio::test]
async fn the_migration_keeps_the_order_and_the_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let old = Store::open_with(&path, StoreOptions::default(), &before_0032()).expect("old schema");
    for (n, email) in [
        (1, "ann@example.org"),
        (2, "bob@example.org"),
        (3, "cy@example.org"),
    ] {
        old.insert_account(new_account(n, email))
            .await
            .expect("account");
    }
    // Added in the order 2, 3, 1 (by time), with the names of the setup form as before.
    old.write(|tx| {
        tx.execute_batch(&format!(
            "UPDATE account SET created_at = 30, display_name = 'Ann' WHERE uuid = '{}';
             UPDATE account SET created_at = 10, display_name = 'Bob' WHERE uuid = '{}';
             UPDATE account SET created_at = 20, display_name = 'Cy' WHERE uuid = '{}';",
            account_id(1),
            account_id(2),
            account_id(3)
        ))?;
        Ok(())
    })
    .await
    .expect("older accounts");
    drop(old);
    let store = Store::open(&path, StoreOptions::default()).expect("migrated");
    let accounts = store.accounts().await.expect("accounts");
    assert_eq!(
        accounts
            .iter()
            .map(|a| (a.id, a.display_name.as_str()))
            .collect::<Vec<_>>(),
        [
            (account_id(2), "Bob"),
            (account_id(3), "Cy"),
            (account_id(1), "Ann")
        ]
    );
    // Their sync records do not change by themselves: no position until the user moves one.
    let content = store
        .sync_table_content("account", &account_id(2).to_string())
        .await
        .expect("content")
        .expect("account");
    assert_eq!(content.get("position"), None);
    assert_eq!(content["displayName"], json!("Bob"));
}

/// The account record carries the position once the user placed the account; a record from the
/// folder sets it, and one without it keeps this device's.
#[tokio::test]
async fn the_position_travels_with_the_account_record() {
    let (_dir, store) = open().await;
    let [a, b, _c] = three(&store).await;
    store.reorder_accounts(vec![b, a]).await.expect("reorder");
    let content = store
        .sync_table_content("account", &b.to_string())
        .await
        .expect("content")
        .expect("account");
    let first = content["position"].as_i64().expect("placed");
    let a_content = store
        .sync_table_content("account", &a.to_string())
        .await
        .expect("content")
        .expect("account");
    assert_eq!(a_content["position"].as_i64(), Some(first + 1));

    // From the folder: B moves behind A and C.
    let mut moved = content.clone();
    moved["position"] = json!(first + 10);
    moved["displayName"] = json!("Bob (Arbeit)");
    let apply =
        crate::settings_sync_tests::account_apply_for_tests(&store, &b.to_string(), moved).await;
    store.apply_sync(vec![apply], None).await.expect("apply");
    let accounts = store.accounts().await.expect("accounts");
    assert_eq!(accounts.last().map(|x| x.id), Some(b));
    assert_eq!(
        accounts.last().map(|x| x.display_name.as_str()),
        Some("Bob (Arbeit)")
    );

    // A record of an older version (no position) keeps the place this device has.
    let mut older = content;
    older.as_object_mut().expect("object").remove("position");
    let apply =
        crate::settings_sync_tests::account_apply_for_tests(&store, &b.to_string(), older).await;
    store.apply_sync(vec![apply], None).await.expect("apply");
    assert_eq!(order(&store).await.last(), Some(&b));
}
