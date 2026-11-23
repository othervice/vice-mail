//! The MCP wiring without a mail server: clients and the keychain, shares, the switch and the
//! socket, the probe's early answers. `tests/mcp.rs` runs the real helper over the real socket
//! against the test server.

use std::sync::Arc;
use std::time::Duration;

use mail_mcp::ClientToken;
use mail_model::{
    AccountId, CoreEvent, ErrorCode, IpcError, McpAccountAccess, McpHostState, McpTestProblem,
    Security, ServerSettings,
};
use mail_store::NewAccount;
use tempfile::TempDir;
use tokio::time::Instant;

use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretOp, SecretStore};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

async fn start(dir: &TempDir, keychain: &Arc<MemorySecretStore>) -> AppCore {
    AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::new(Credentials::new(Arc::clone(keychain))),
    )
    .await
}

fn code(err: &mail_model::CoreError) -> ErrorCode {
    IpcError::from(err).code
}

async fn until_state(core: &AppCore, state: McpHostState) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while core.get_mcp_status().state != state {
        assert!(
            Instant::now() < deadline,
            "{:?}, expected {state:?}",
            core.get_mcp_status()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn clients_live_in_the_keychain_and_revoking_removes_them() {
    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let core = start(&dir, &keychain).await;

    let created = core
        .create_mcp_client("  Claude Desktop ".into())
        .await
        .expect("client");
    assert_eq!(created.client.name, "Claude Desktop");
    let token = ClientToken::parse(&created.token).expect("a token");
    let key = CredentialRef::for_mcp_client(&token.id);
    assert!(key.is_mcp_client());
    let stored = keychain.get(&key).expect("keychain").expect("entry");
    assert_eq!(stored.expose_secret(), &created.token);
    assert_eq!(created.configs.len(), 4);
    assert!(
        created
            .configs
            .iter()
            .all(|c| c.content.contains(&created.token))
    );
    let setup = core.get_mcp_setup();
    assert_eq!(setup.configs.len(), 4);
    assert!(
        setup
            .configs
            .iter()
            .all(|c| c.content.contains("<token>") && !c.content.contains(&created.token))
    );
    assert!(
        !format!("{created:?}").contains(&created.token),
        "Debug redacts"
    );

    let settings = core.get_mcp_settings().await.expect("settings");
    assert!(!settings.enabled);
    assert_eq!(settings.clients.len(), 1);
    assert!(settings.clients[0].revoked_at.is_none());

    let after = core
        .revoke_mcp_client(created.client.client_id.clone())
        .await
        .expect("revoke");
    assert!(after.clients[0].revoked_at.is_some());
    assert!(!keychain.contains(&key), "the key left the keychain");
    core.revoke_mcp_client(created.client.client_id.clone())
        .await
        .expect("revoking twice is fine");
    let unknown = core
        .revoke_mcp_client("ab".repeat(16))
        .await
        .expect_err("unknown");
    assert_eq!(code(&unknown), ErrorCode::NotFound);
    let bad = core
        .revoke_mcp_client("not an id".into())
        .await
        .expect_err("bad id");
    assert_eq!(code(&bad), ErrorCode::InvalidArgument);

    for name in ["", "   ", "a\nb", &"x".repeat(101)] {
        let err = core
            .create_mcp_client(name.to_owned())
            .await
            .expect_err("bad name");
        assert_eq!(code(&err), ErrorCode::InvalidArgument, "{name:?}");
    }
    assert_eq!(core.list_mcp_clients().await.expect("clients").len(), 1);
}

#[tokio::test]
async fn a_failed_keychain_write_registers_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let core = start(&dir, &keychain).await;
    keychain.set_failing(SecretOp::Set, true);
    let err = core
        .create_mcp_client("Cursor".into())
        .await
        .expect_err("keychain refuses");
    assert_eq!(code(&err), ErrorCode::Keychain);
    assert!(core.list_mcp_clients().await.expect("clients").is_empty());
    assert!(keychain.is_empty());
}

#[tokio::test]
async fn account_access_needs_an_account_and_hidden_is_the_default() {
    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let core = start(&dir, &keychain).await;
    let missing = AccountId::from_uuid(uuid::Uuid::from_u128(7));
    let err = core
        .set_mcp_account_access(missing, McpAccountAccess::Read)
        .await
        .expect_err("no such account");
    assert_eq!(code(&err), ErrorCode::NotFound);

    let id = AccountId::from_uuid(uuid::Uuid::from_u128(8));
    let server = |port| ServerSettings {
        host: "mail.example.org".into(),
        port,
        security: Security::Tls,
        username: "a@example.org".into(),
    };
    core.store_for_tests()
        .insert_account(NewAccount {
            id,
            email: "a@example.org".into(),
            display_name: "A".into(),
            imap: server(993),
            smtp: server(465),
            credential_ref: id.to_string(),
        })
        .await
        .expect("account");
    let settings = core.get_mcp_settings().await.expect("settings");
    assert_eq!(settings.accounts.len(), 1);
    assert_eq!(settings.accounts[0].access, McpAccountAccess::Hidden);
    for access in [
        McpAccountAccess::ReadAndDrafts,
        McpAccountAccess::Read,
        McpAccountAccess::Hidden,
    ] {
        let settings = core
            .set_mcp_account_access(id, access)
            .await
            .expect("share");
        assert_eq!(settings.accounts[0].access, access);
        let expected = match access {
            McpAccountAccess::Hidden => mail_mcp::AccountAccess::Hidden,
            McpAccountAccess::Read => mail_mcp::AccountAccess::Read,
            McpAccountAccess::ReadAndDrafts => mail_mcp::AccountAccess::ReadAndDrafts,
        };
        assert_eq!(core.mcp_access_of(&id), expected, "published to the host");
    }
}

/// Whether the socket file exists (Unix); a named pipe has no file, so `expected` holds there.
fn socket_exists(endpoint: &str, expected: bool) -> bool {
    if cfg!(unix) {
        std::path::Path::new(endpoint).exists()
    } else {
        expected
    }
}

#[tokio::test]
async fn the_switch_opens_and_closes_the_socket_and_survives_a_restart() {
    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let endpoint = {
        let core = start(&dir, &keychain).await;
        let mut events = core.subscribe();
        let status = core.get_mcp_status();
        assert!(!status.enabled);
        assert_eq!(status.state, McpHostState::Stopped);
        let endpoint = status.endpoint.expect("endpoint");
        if cfg!(unix) {
            assert!(endpoint.ends_with("/mcp/mail-mcp.sock"), "{endpoint}");
        }
        assert!(!socket_exists(&endpoint, false));

        core.set_mcp_enabled(true).await.expect("on");
        until_state(&core, McpHostState::Listening).await;
        assert!(socket_exists(&endpoint, true));
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(CoreEvent::McpStatusChanged { status }) = events.recv().await
                    && status.enabled
                {
                    return status;
                }
            }
        })
        .await
        .expect("mcpStatusChanged");
        assert!(event.enabled);

        core.set_mcp_enabled(false).await.expect("off");
        until_state(&core, McpHostState::Stopped).await;
        assert!(
            !socket_exists(&endpoint, false),
            "the emergency stop removes the socket"
        );
        core.set_mcp_enabled(true).await.expect("on again");
        until_state(&core, McpHostState::Listening).await;
        core.shutdown().await;
        assert!(!socket_exists(&endpoint, false), "shutdown closes it");
        endpoint
    };
    // Stored: the next start listens right away, on the same path.
    let core = start(&dir, &keychain).await;
    assert!(core.get_mcp_status().enabled);
    until_state(&core, McpHostState::Listening).await;
    assert_eq!(
        core.get_mcp_status().endpoint.as_deref(),
        Some(endpoint.as_str())
    );
    core.shutdown().await;
}

#[tokio::test]
async fn the_probe_explains_what_is_missing() {
    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let core = start(&dir, &keychain).await;
    core.set_mcp_helper(Some(dir.path().join("no-such-helper")));
    let probe = core.test_mcp_connection().await.expect("probe");
    assert!(!probe.ok);
    assert_eq!(probe.problem, Some(McpTestProblem::HelperMissing));
    assert!(!core.get_mcp_status().helper.found);

    // Any existing file counts as found; MCP is off.
    let file = dir.path().join("helper");
    std::fs::write(&file, b"").expect("file");
    core.set_mcp_helper(Some(file));
    assert!(core.get_mcp_status().helper.found);
    let probe = core.test_mcp_connection().await.expect("probe");
    assert_eq!(probe.problem, Some(McpTestProblem::Disabled));
}
