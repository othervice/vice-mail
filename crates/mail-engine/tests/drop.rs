//! Stage 22 (decision 0081) through `AppCore` against the local test server (Stalwart): rows of
//! "All inboxes" from two accounts dropped on a folder of one of them. The row of that account
//! moves there at once; the row of the other account is copied there first and removed from its
//! inbox once the copy arrived; a row already in the folder stays.
//!
//! Ignored by default. Take the shared lock, start the server (any seed):
//!
//! ```text
//! scripts/testmail.sh lock stage22 && scripts/testmail.sh start
//! cargo test -p mail-engine --test drop -- --include-ignored
//! scripts/testmail.sh stop && scripts/testmail.sh unlock stage22
//! ```
//!
//! Appends its own messages (`<...@drop-core.test>`) to alice's and bob's inboxes and uses a
//! folder `VmDropCore-<pid>` in bob's account; all are removed before and after (the seed stays).

use std::sync::Arc;
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, SyncConfig};
use mail_imap::test_support::{TestAccount, TestEnv};
use mail_imap::{ImapBackend, ImapConnector};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, ErrorCode, MailBackend, MailConnector,
    MessageRef, Security, ServerSettings, TransferMode, UidSet, UnifiedView,
};
use tempfile::TempDir;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(30);
const DOMAIN: &str = "drop-core.test";

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        poll_interval: Duration::from_secs(600),
        extra_root_ca: Some(env.extra_root()),
        settings_sync_auto: false,
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

fn settings(env: &TestEnv, who: &TestAccount) -> AccountSettings {
    let server = |port, security| ServerSettings {
        host: env.host.clone(),
        port,
        security,
        username: who.address.clone(),
    };
    AccountSettings {
        credential: Default::default(),
        protocol: Default::default(),
        email: who.address.clone(),
        display_name: who.address.clone(),
        imap: server(env.imaps_port, Security::Tls),
        smtp: server(env.submission_port, Security::Starttls),
    }
}

async fn admin(env: &TestEnv, who: &TestAccount) -> ImapBackend {
    ImapConnector::new()
        .connect(&env.config(who, Security::Tls, Access::ReadWrite))
        .await
        .expect("admin session")
}

async fn raw(admin: &mut ImapBackend, line: &str) {
    let reply = admin
        .raw_command(line, |_| false)
        .await
        .unwrap_or_else(|err| panic!("{line}: {err}"));
    assert!(reply.ok, "{line}: {}", reply.text);
}

/// Subjects of the test's messages in `folder` (empty if the folder does not exist).
async fn subjects(admin: &mut ImapBackend, folder: &str) -> Vec<String> {
    let Ok(opened) = admin.open_mailbox(folder).await else {
        return Vec::new();
    };
    let uids = admin.search_uids(&opened).await.expect("search");
    let mut found: Vec<String> = if uids.is_empty() {
        Vec::new()
    } else {
        admin
            .fetch_headers(&opened, &uids)
            .await
            .expect("headers")
            .into_iter()
            .filter(|h| h.message_id.as_deref().is_some_and(|m| m.ends_with(DOMAIN)))
            .map(|h| h.subject)
            .collect()
    };
    admin.raw_command("UNSELECT", |_| false).await.ok();
    found.sort();
    found
}

/// The test's messages out of the inbox, its folders gone.
async fn clean(admin: &mut ImapBackend) {
    for remote in admin.list_mailboxes().await.expect("list") {
        let ours = remote.name.starts_with("VmDropCore-");
        if !ours && remote.name != "INBOX" {
            continue;
        }
        if let Ok(opened) = admin.open_mailbox(&remote.name).await {
            let uids = admin.search_uids(&opened).await.expect("search");
            let mine: Vec<_> = if uids.is_empty() {
                Vec::new()
            } else {
                admin
                    .fetch_headers(&opened, &uids)
                    .await
                    .expect("headers")
                    .into_iter()
                    .filter(|h| {
                        ours || h.message_id.as_deref().is_some_and(|m| m.ends_with(DOMAIN))
                    })
                    .map(|h| h.uid)
                    .collect()
            };
            if !mine.is_empty() {
                let set = UidSet::from_uids(mine.iter().copied());
                raw(admin, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")).await;
                raw(admin, &format!("UID EXPUNGE {set}")).await;
            }
            admin.raw_command("UNSELECT", |_| false).await.ok();
        }
        if ours {
            raw(admin, &format!("DELETE \"{}\"", remote.name)).await;
        }
    }
}

async fn append(admin: &mut ImapBackend, folder: &str, subject: &str, id: &str) {
    let body = format!(
        "From: Writer <writer@{DOMAIN}>\r\nTo: someone@vicemail.test\r\nSubject: {subject}\r\n\
         Date: Mon, 23 Nov 2026 10:00:00 +0000\r\nMessage-ID: <{id}@{DOMAIN}>\r\n\r\nDropped.\r\n"
    );
    raw(
        admin,
        &format!("APPEND \"{folder}\" {{{}+}}\r\n{body}", body.len()),
    )
    .await;
}

async fn until<T, F, Fut>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(value) = f().await {
            return value;
        }
        assert!(Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn idle(core: &AppCore<ImapConnector>, account: AccountId) {
    until("account idle", || async {
        core.list_accounts()
            .await
            .ok()?
            .iter()
            .any(|a| a.id == account && a.status.state == AccountState::Idle)
            .then_some(())
    })
    .await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_drop_from_all_inboxes_moves_into_another_accounts_folder() {
    let env = TestEnv::require();
    let pid = std::process::id();
    let folder = format!("VmDropCore-{pid}");
    let from_alice = format!("Dropped from alice {pid}");
    let from_bob = format!("Dropped from bob {pid}");
    let already = format!("Already there {pid}");
    let mut alice = admin(&env, &env.alice).await;
    let mut bob = admin(&env, &env.bob).await;
    clean(&mut alice).await;
    clean(&mut bob).await;
    raw(&mut bob, &format!("CREATE \"{folder}\"")).await;
    append(&mut alice, "INBOX", &from_alice, &format!("a-{pid}")).await;
    append(&mut bob, "INBOX", &from_bob, &format!("b-{pid}")).await;
    append(&mut bob, &folder, &already, &format!("c-{pid}")).await;

    let dir = TempDir::new().expect("temp dir");
    let mut services = CoreServices::with_connector(
        Credentials::new(Arc::new(MemorySecretStore::new())),
        ImapConnector::new(),
    );
    services.sync = sync_config(&env);
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    let mut ids = Vec::new();
    for who in [&env.alice, &env.bob] {
        let id = core
            .add_account(settings(&env, who), who.password.clone())
            .await
            .expect("account")
            .id;
        idle(&core, id).await;
        ids.push(id);
    }
    let (alice_id, bob_id) = (ids[0], ids[1]);
    let target = until("bob's folder", || async {
        core.list_mailboxes(bob_id)
            .await
            .ok()?
            .into_iter()
            .find(|m| m.path == folder)
    })
    .await;
    // The rows as "All inboxes" lists them, each with its account and folder; and the row of
    // the folder itself (a search result, say).
    let rows = until("both rows in All inboxes", || async {
        let page = core
            .list_unified_inbox(UnifiedView::All, None, 200)
            .await
            .ok()?;
        let rows: Vec<_> = page
            .items
            .into_iter()
            .filter(|i| i.subject == from_alice || i.subject == from_bob)
            .collect();
        (rows.len() == 2).then_some(rows)
    })
    .await;
    assert_eq!(
        rows.iter()
            .map(|r| r.account_id)
            .collect::<std::collections::BTreeSet<_>>(),
        [alice_id, bob_id].into_iter().collect(),
        "rows of both accounts"
    );
    let there = until("the row already in the folder", || async {
        core.list_messages(target.id, None, 10)
            .await
            .ok()?
            .items
            .into_iter()
            .find(|i| i.subject == already)
    })
    .await;
    let mut refs: Vec<MessageRef> = rows
        .iter()
        .map(|r| MessageRef {
            mailbox_id: r.mailbox_id,
            message_id: r.id,
        })
        .collect();
    refs.push(MessageRef {
        mailbox_id: target.id,
        message_id: there.id,
    });

    // Read-only accounts refuse, naming the account; nothing is recorded.
    let err = core
        .drop_messages(
            refs.clone(),
            target.id,
            TransferMode::Move,
            "drop-ro".into(),
        )
        .await
        .expect_err("bob may not write yet");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    core.set_writes_enabled(bob_id, true)
        .await
        .expect("bob writes");
    let err = core
        .drop_messages(
            refs.clone(),
            target.id,
            TransferMode::Move,
            "drop-ro2".into(),
        )
        .await
        .expect_err("alice may not write: her row cannot leave");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    core.set_writes_enabled(alice_id, true)
        .await
        .expect("alice writes");

    let result = core
        .drop_messages(
            refs,
            target.id,
            TransferMode::Move,
            uuid::Uuid::new_v4().to_string(),
        )
        .await
        .expect("drop");
    assert_eq!(result.account_id, bob_id);
    assert_eq!(
        (result.moved, result.transferred, result.skipped),
        (1, 1, 1)
    );
    assert_eq!(result.batches.len(), 2, "the transfer and the move");

    let mut expected = vec![already.clone(), from_alice.clone(), from_bob.clone()];
    expected.sort();
    until(
        "all three in bob's folder, none left in the inboxes",
        || async {
            let mut a = admin(&env, &env.alice).await;
            let mut b = admin(&env, &env.bob).await;
            let (alice_inbox, bob_inbox, in_folder) = (
                subjects(&mut a, "INBOX").await,
                subjects(&mut b, "INBOX").await,
                subjects(&mut b, &folder).await,
            );
            a.logout().await.ok();
            b.logout().await.ok();
            (alice_inbox.is_empty() && bob_inbox.is_empty() && in_folder == expected).then_some(())
        },
    )
    .await;
    // The move within bob's account undoes like any move; the transfer from alice's account
    // has reached the server and is not undone.
    let err = core
        .undo_action(result.batches[0])
        .await
        .expect_err("the transfer is on the server");
    assert_eq!(err.code(), ErrorCode::NotUndoable);
    core.undo_action(result.batches[1])
        .await
        .expect("undo the move");
    until("bob's row back in his inbox", || async {
        let mut b = admin(&env, &env.bob).await;
        let inbox = subjects(&mut b, "INBOX").await;
        b.logout().await.ok();
        (inbox == [from_bob.clone()]).then_some(())
    })
    .await;

    core.shutdown().await;
    clean(&mut alice).await;
    clean(&mut bob).await;
    alice.logout().await.ok();
    bob.logout().await.ok();
}
