//! Decision 0044: the guided first start shows once on a new installation, never on one that
//! already had an account, and its end survives a restart and the removal of every account.

use mail_model::{Security, ServerSettings};
use tempfile::TempDir;

use super::*;
use crate::credentials::{Credentials, MemorySecretStore};
use crate::settings_sync::SYNCED_SETTINGS;
use crate::{BuildInfo, CoreConfig, CoreServices};

const ACCOUNT: &str = "00000000-0000-4000-8000-0000000000aa";

async fn start(dir: &TempDir) -> AppCore {
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.1.0".into(),
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

/// An account straight in the store, as an earlier version would have left it.
async fn add_account(core: &AppCore) {
    let server = ServerSettings {
        host: "mail.example.org".into(),
        port: 993,
        security: Security::Tls,
        username: "alice".into(),
    };
    core.store_for_tests()
        .insert_account(mail_store::NewAccount {
            id: ACCOUNT.parse().expect("id"),
            email: "alice@example.org".into(),
            display_name: "Alice".into(),
            imap: server.clone(),
            smtp: server,
            credential_ref: "x".into(),
        })
        .await
        .expect("account");
}

async fn stored(core: &AppCore) -> Option<String> {
    core.store_for_tests()
        .settings(GLOBAL_SCOPE)
        .await
        .expect("settings")
        .into_iter()
        .find(|s| s.key == FIRST_START_KEY)
        .map(|s| s.value)
}

#[tokio::test]
async fn a_new_installation_shows_it_until_it_is_completed_and_not_after_a_restart() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir).await;
    assert_eq!(
        core.get_first_start().await.expect("get"),
        FirstStart::PENDING
    );
    // Asking does not end it: quitting halfway shows it again on the next start.
    assert_eq!(
        core.get_first_start().await.expect("get"),
        FirstStart::PENDING
    );
    assert_eq!(stored(&core).await, None, "asking stores nothing");

    let done = core
        .finish_first_start(FirstStartEnd::Completed)
        .await
        .expect("finish");
    assert_eq!(done, FirstStart::ended(FirstStartOutcome::Completed));
    assert_eq!(
        core.get_first_start().await.expect("get"),
        FirstStart::ended(FirstStartOutcome::Completed)
    );
    assert_eq!(
        stored(&core).await.as_deref(),
        Some(r#"{"outcome":"completed"}"#)
    );

    core.shutdown().await;
    drop(core);
    let core = start(&dir).await;
    assert_eq!(
        core.get_first_start().await.expect("get after restart"),
        FirstStart::ended(FirstStartOutcome::Completed),
        "shown only once"
    );
}

#[tokio::test]
async fn skipping_ends_it_too_even_without_an_account() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir).await;
    let skipped = core
        .finish_first_start(FirstStartEnd::Skipped)
        .await
        .expect("skip");
    assert_eq!(skipped, FirstStart::ended(FirstStartOutcome::Skipped));
    assert!(
        core.store_for_tests()
            .accounts()
            .await
            .expect("accounts")
            .is_empty()
    );
    assert!(!core.get_first_start().await.expect("get").show);
}

#[tokio::test]
async fn an_installation_with_an_account_never_shows_it_not_even_after_removing_the_account() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir).await;
    add_account(&core).await;
    assert_eq!(
        core.get_first_start().await.expect("get"),
        FirstStart::ended(FirstStartOutcome::NotNeeded)
    );
    assert_eq!(
        stored(&core).await.as_deref(),
        Some(r#"{"outcome":"notNeeded"}"#),
        "recorded the first time it is asked"
    );

    core.remove_account(ACCOUNT.parse().expect("id"))
        .await
        .expect("remove");
    assert!(
        core.store_for_tests()
            .accounts()
            .await
            .expect("accounts")
            .is_empty()
    );
    assert_eq!(
        core.get_first_start().await.expect("get"),
        FirstStart::ended(FirstStartOutcome::NotNeeded),
        "removing the last account does not bring it back"
    );
}

#[tokio::test]
async fn an_account_added_during_the_first_start_does_not_change_how_it_ends() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir).await;
    assert!(core.get_first_start().await.expect("get").show);
    // Step 2 adds the account; the UI keeps showing the steps and asks nothing in between.
    add_account(&core).await;
    let done = core
        .finish_first_start(FirstStartEnd::Completed)
        .await
        .expect("finish");
    assert_eq!(done.outcome, Some(FirstStartOutcome::Completed));

    // Even if something asked in between (and the core took it for an older installation),
    // the end the user chose is what stays.
    let dir = TempDir::new().expect("dir");
    let core = start(&dir).await;
    assert!(core.get_first_start().await.expect("get").show);
    add_account(&core).await;
    assert!(!core.get_first_start().await.expect("get").show);
    core.finish_first_start(FirstStartEnd::Skipped)
        .await
        .expect("skip");
    assert_eq!(
        core.get_first_start().await.expect("get").outcome,
        Some(FirstStartOutcome::Skipped)
    );
}

#[tokio::test]
async fn a_damaged_value_counts_as_ended() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir).await;
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![(
                FIRST_START_KEY.to_owned(),
                r#"{"outcome":"later"}"#.to_owned(),
            )],
        )
        .await
        .expect("put");
    assert_eq!(
        core.get_first_start().await.expect("get"),
        FirstStart::ended(FirstStartOutcome::Completed)
    );
}

#[test]
fn it_stays_on_this_device() {
    assert!(
        !SYNCED_SETTINGS.contains(&FIRST_START_KEY),
        "a second device gets its own first start"
    );
}
