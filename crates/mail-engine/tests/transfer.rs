//! Stage 16 (decision 0033) through `AppCore` against the local test server (Stalwart): a move
//! from alice's account to bob's freezes the message's bytes, appends them to bob's folder
//! through his outbox and removes the original from alice's only afterwards. The crash cases
//! (a connection cut after APPEND, a refusing target) are in
//! `src/outbox/transfer_server_tests.rs`.
//!
//! Ignored by default. Take the shared lock, start the server (any seed):
//!
//! ```text
//! scripts/testmail.sh lock stage16 && scripts/testmail.sh start
//! cargo test -p mail-engine --test transfer -- --include-ignored
//! scripts/testmail.sh stop && scripts/testmail.sh unlock stage16
//! ```
//!
//! Uses folders `VmTransferCore-<pid>` in both accounts (alice's seed stays untouched) with
//! messages `<...@transfer-core.test>` it appends itself; both are removed before and after.

use std::sync::Arc;
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, SyncConfig};
use mail_imap::test_support::{TestAccount, TestEnv};
use mail_imap::{ImapBackend, ImapConnector};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, MailBackend, MailConnector, MessageRef,
    Security, ServerSettings, TransferMode, UidSet,
};
use tempfile::TempDir;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(30);
const DOMAIN: &str = "transfer-core.test";

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
    if uids.is_empty() {
        return Vec::new();
    }
    admin
        .fetch_headers(&opened, &uids)
        .await
        .expect("headers")
        .into_iter()
        .filter(|h| h.message_id.as_deref().is_some_and(|m| m.ends_with(DOMAIN)))
        .map(|h| h.subject)
        .collect()
}

async fn clean(admin: &mut ImapBackend) {
    for remote in admin.list_mailboxes().await.expect("list") {
        if remote.name.starts_with("VmTransferCore-") {
            if let Ok(opened) = admin.open_mailbox(&remote.name).await {
                let uids = admin.search_uids(&opened).await.expect("search");
                if !uids.is_empty() {
                    let set = UidSet::from_uids(uids.iter());
                    raw(admin, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")).await;
                    raw(admin, &format!("UID EXPUNGE {set}")).await;
                }
                admin.raw_command("UNSELECT", |_| false).await.ok();
            }
            raw(admin, &format!("DELETE \"{}\"", remote.name)).await;
        }
    }
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
async fn a_move_to_another_account_through_the_core_appends_then_removes_the_source() {
    let env = TestEnv::require();
    let folder = format!("VmTransferCore-{}", std::process::id());
    let subject = format!("Transfer through the core {}", std::process::id());
    let mut alice = admin(&env, &env.alice).await;
    let mut bob = admin(&env, &env.bob).await;
    clean(&mut alice).await;
    clean(&mut bob).await;
    raw(&mut alice, &format!("CREATE \"{folder}\"")).await;
    raw(&mut bob, &format!("CREATE \"{folder}\"")).await;
    let body = format!(
        "From: Writer <writer@{DOMAIN}>\r\nTo: alice@vicemail.test\r\nSubject: {subject}\r\n\
         Date: Mon, 02 Nov 2026 10:00:00 +0000\r\nMessage-ID: <core-1@{DOMAIN}>\r\n\r\nMoved.\r\n"
    );
    raw(
        &mut alice,
        &format!(
            "APPEND \"{folder}\" (\\Flagged) {{{}+}}\r\n{body}",
            body.len()
        ),
    )
    .await;

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
        core.set_writes_enabled(id, true).await.expect("writes");
        ids.push(id);
    }
    let (source_account, target_account) = (ids[0], ids[1]);
    let folder_of = |account: AccountId| {
        let core = core.clone();
        let folder = folder.clone();
        async move {
            core.list_mailboxes(account)
                .await
                .ok()?
                .into_iter()
                .find(|m| m.path == folder)
        }
    };
    let source = until("alice's folder", || folder_of(source_account)).await;
    let target = until("bob's folder", || folder_of(target_account)).await;
    let item = until("the message in alice's folder", || async {
        core.list_messages(source.id, None, 10)
            .await
            .ok()?
            .items
            .into_iter()
            .find(|i| i.subject == subject)
    })
    .await;

    let result = core
        .transfer_messages(
            vec![MessageRef {
                mailbox_id: source.id,
                message_id: item.id,
            }],
            target.id,
            TransferMode::Move,
            uuid::Uuid::new_v4().to_string(),
        )
        .await
        .expect("transfer");
    assert_eq!(result.queued, 1);
    until("moved on the server", || async {
        let (a, b) = (
            subjects(&mut admin(&env, &env.alice).await, &folder).await,
            subjects(&mut admin(&env, &env.bob).await, &folder).await,
        );
        (a.is_empty() && b == [subject.clone()]).then_some(())
    })
    .await;
    // The flag went with it; the activity view of both accounts names the transfer.
    let opened = bob.open_mailbox(&folder).await.expect("select");
    let uids = bob.search_uids(&opened).await.expect("search");
    let flags = bob.fetch_headers(&opened, &uids).await.expect("headers")[0]
        .flags
        .clone();
    assert!(flags.flagged, "{flags:?}");
    bob.raw_command("UNSELECT", |_| false).await.ok();
    let target_ops = core
        .list_operations(target_account, None, 10)
        .await
        .expect("target operations");
    assert!(
        target_ops
            .iter()
            .any(|o| o.kind == mail_model::OperationKind::Append)
    );
    let source_ops = core
        .list_operations(source_account, None, 10)
        .await
        .expect("source operations");
    assert!(
        source_ops
            .iter()
            .any(|o| o.kind == mail_model::OperationKind::Expunge)
    );
    core.shutdown().await;
    clean(&mut alice).await;
    clean(&mut bob).await;
    alice.logout().await.ok();
    bob.logout().await.ok();
}
