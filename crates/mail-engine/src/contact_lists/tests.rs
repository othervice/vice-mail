//! Stage 19 (decision 0056): contact groups through `AppCore` with the real store.

use mail_model::{Address, ContactListId, CoreEvent, ErrorCode, MAX_CONTACT_LIST_NAME_CHARS};
use tempfile::TempDir;

use super::*;
use crate::credentials::{Credentials, MemorySecretStore};
use crate::settings_sync::record::ContactListMemberContent;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

async fn core(dir: &TempDir) -> AppCore {
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::new(Credentials::new(MemorySecretStore::new())),
    )
    .await;
    assert!(core.startup_error().is_none());
    core
}

fn person(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.to_owned(),
    }
}

#[tokio::test]
async fn groups_are_created_renamed_filled_and_suggested() {
    let dir = TempDir::new().expect("temp dir");
    let core = core(&dir).await;
    let mut events = core.subscribe();
    let list = core
        .create_contact_list(
            "  Team   Süd ".into(),
            vec![
                person(Some("Ada Lovelace"), "ada@example.org"),
                person(None, "Grace@Example.org"),
            ],
        )
        .await
        .expect("create");
    assert_eq!(list.name, "Team Süd");
    assert_eq!(
        list.recipients(),
        vec![
            person(Some("Ada Lovelace"), "ada@example.org"),
            person(None, "Grace@Example.org")
        ]
    );
    assert!(matches!(
        events.recv().await.expect("event"),
        CoreEvent::ContactsChanged
    ));

    // Suggestions: every word starts a word of the name, without accents or case.
    for query in ["team", "sud", "TEAM s", "süd"] {
        let found = core
            .suggest_contact_lists(query.into(), 5)
            .await
            .expect("suggest");
        assert_eq!(found.len(), 1, "{query}");
        assert_eq!(found[0].member_count, 2);
    }
    assert!(
        core.suggest_contact_lists("nord".into(), 5)
            .await
            .expect("suggest")
            .is_empty()
    );
    assert!(
        core.suggest_contact_lists("  ".into(), 5)
            .await
            .expect("suggest")
            .is_empty()
    );

    let list = core
        .add_contact_list_members(
            list.id.clone(),
            vec![
                person(None, "grace@example.org"),
                person(Some("Linus"), "linus@example.org"),
            ],
        )
        .await
        .expect("add");
    assert_eq!(list.members.len(), 3, "the same address once");
    let list = core
        .remove_contact_list_member(list.id.clone(), "GRACE@example.org".into())
        .await
        .expect("remove");
    let emails: Vec<&str> = list.members.iter().map(|m| m.email.as_str()).collect();
    assert_eq!(emails, ["ada@example.org", "linus@example.org"]);
    let renamed = core
        .rename_contact_list(list.id.clone(), "Team Nord".into())
        .await
        .expect("rename");
    assert_eq!(renamed.name, "Team Nord");
    assert_eq!(core.list_contact_lists().await.expect("lists").len(), 1);
    core.delete_contact_list(list.id.clone())
        .await
        .expect("delete");
    let gone = core
        .get_contact_list(list.id.clone())
        .await
        .expect_err("gone");
    assert_eq!(gone.code(), ErrorCode::NotFound);
}

#[tokio::test]
async fn names_and_addresses_are_checked() {
    let dir = TempDir::new().expect("temp dir");
    let core = core(&dir).await;
    for name in [
        String::new(),
        "  ".into(),
        "x".repeat(MAX_CONTACT_LIST_NAME_CHARS as usize + 1),
        "a\u{202e}b".into(),
    ] {
        let err = core
            .create_contact_list(name.clone(), Vec::new())
            .await
            .expect_err("bad name");
        assert_eq!(err.code(), ErrorCode::InvalidArgument, "{name:?}");
    }
    let err = core
        .create_contact_list("Ok".into(), vec![person(None, "not an address")])
        .await
        .expect_err("bad address");
    assert_eq!(err.code(), ErrorCode::InvalidAddress);
    assert!(core.list_contact_lists().await.expect("lists").is_empty());
    let unknown = ContactListId::parse("0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01").expect("id");
    let err = core
        .rename_contact_list(unknown, "x".into())
        .await
        .expect_err("unknown");
    assert_eq!(err.code(), ErrorCode::NotFound);
}

#[test]
fn records_from_another_device_must_be_what_this_device_stores() {
    let good = ContactListContent {
        name: "Team Süd".into(),
        search: " team sud".into(),
        members: vec![ContactListMemberContent {
            email: "Ada@Example.org".into(),
            name: Some("Ada Lovelace".into()),
            key: "ada@example.org".into(),
        }],
    };
    assert_eq!(check_synced(&good), Ok(()));
    let mut wrong_search = good.clone();
    wrong_search.search = " other".into();
    assert!(check_synced(&wrong_search).is_err());
    let mut wrong_key = good.clone();
    wrong_key.members[0].key = "eve@example.org".into();
    assert!(check_synced(&wrong_key).is_err());
    let mut bad_name = good.clone();
    bad_name.name = "Team\nSüd".into();
    assert!(check_synced(&bad_name).is_err());
    let mut bad_address = good;
    bad_address.members[0].email = "nobody".into();
    assert!(check_synced(&bad_address).is_err());
}
