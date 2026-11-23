//! Accounts that sign in with a provider, through [`AppCore`]: the fake IMAP server accepts the
//! mock's access token as its "password", the mock authorization server plays the provider.

use std::sync::Arc;
use std::time::Duration;

use mail_model::{
    AccountId, AccountSettings, AccountState, AccountStatus, CoreEvent, ErrorCode, LoginAuth,
    MailAction, MessageRef, OAuthPageTexts, OAuthProvider, Secret,
};
use tempfile::TempDir;
use tokio::sync::broadcast;

use super::mock::{Browser, MockAuth, Script};
use super::{TestClock, service};
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore};
use crate::sync::fake::{FakeConnector, FakeServer};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, ReadingServices, SyncConfig};

const TOKEN: &str = "access-from-mock";
const WAIT: Duration = Duration::from_secs(10);

struct Harness {
    _dir: TempDir,
    server: Arc<FakeServer>,
    mock: MockAuth,
    keychain: Arc<MemorySecretStore>,
    core: AppCore<FakeConnector>,
    events: broadcast::Receiver<CoreEvent>,
}

/// Through JSON, so fields a later `AccountSettings` adds keep their defaults.
fn settings() -> AccountSettings {
    serde_json::from_value(serde_json::json!({
        "email": "alice@mock.test",
        "displayName": "Alice",
        "imap": {"host": "imap.mock.test", "port": 993, "security": "tls",
                 "username": "alice@mock.test"},
        "smtp": {"host": "smtp.mock.test", "port": 465, "security": "tls",
                 "username": "alice@mock.test"}
    }))
    .expect("settings")
}

impl Harness {
    async fn new(script: Script) -> Self {
        let server = FakeServer::new();
        server.deliver("INBOX", 2, false);
        server.with(|s| s.password = TOKEN.into());
        let mock = MockAuth::start().await;
        mock.state_mut(|s| {
            s.fixed_access = Some(TOKEN.into());
            s.deny = script == Script::Deny;
        });
        let keychain = Arc::new(MemorySecretStore::new());
        let oauth = service(
            &mock,
            Browser::new(script),
            Arc::new(TestClock::default()),
            Duration::from_secs(5),
        );
        let dir = TempDir::new().expect("temp dir");
        let services = CoreServices {
            credentials: Credentials::from_arc(keychain.clone()).with_oauth(oauth),
            connector: FakeConnector(Arc::clone(&server)),
            clock: Arc::new(crate::clock::SystemClock),
            sync: SyncConfig {
                shutdown_grace: Duration::from_millis(300),
                poll_interval: Duration::from_secs(600),
                ..SyncConfig::default()
            },
            reading: ReadingServices::default(),
            outbox: None,
        };
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
        let events = core.subscribe();
        Self {
            _dir: dir,
            server,
            mock,
            keychain,
            core,
            events,
        }
    }

    async fn add(&self) -> AccountId {
        self.core
            .add_oauth_account(
                settings(),
                OAuthProvider::Google,
                OAuthPageTexts::fallback(),
            )
            .await
            .expect("account added")
            .id
    }

    async fn status(&mut self, id: AccountId, pred: impl Fn(&AccountStatus) -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let event = tokio::time::timeout_at(deadline, self.events.recv())
                .await
                .expect("status in time")
                .expect("events");
            if let CoreEvent::AccountStatusChanged { account_id, status } = event
                && account_id == id
                && pred(&status)
            {
                return;
            }
        }
    }
}

#[tokio::test]
async fn an_account_signs_in_syncs_with_the_token_and_stays_read_only() {
    let mut h = Harness::new(Script::Normal).await;
    let id = h.add().await;

    let account = h
        .core
        .list_accounts()
        .await
        .expect("accounts")
        .into_iter()
        .find(|a| a.id == id)
        .expect("listed");
    assert_eq!(account.oauth, Some(OAuthProvider::Google));
    assert!(!account.writes_enabled, "new accounts are read-only");
    h.status(id, |s| s.state == AccountState::Idle).await;

    // Every login used the access token as a bearer token, never as a password.
    let auths = h.server.read(|s| s.auths.clone());
    assert!(!auths.is_empty());
    assert!(auths.iter().all(|a| *a == LoginAuth::Bearer), "{auths:?}");
    // The keychain holds the refresh token under the account's sign-in entry, nothing else.
    let key = CredentialRef::for_oauth_account(OAuthProvider::Google, id);
    assert!(h.keychain.contains(&key));
    assert_eq!(h.keychain.len(), 1);
    assert!(
        h.keychain
            .secrets()
            .iter()
            .all(|s| !s.expose_secret().contains(TOKEN)),
        "the access token is only in memory"
    );

    // Read-only like any new account: actions are refused before anything is recorded.
    let inbox = h
        .core
        .list_mailboxes(id)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == mail_model::MailboxRole::Inbox)
        .expect("inbox");
    let page = h
        .core
        .list_messages(inbox.id, None, 10)
        .await
        .expect("messages");
    let at: Vec<MessageRef> = page
        .items
        .iter()
        .map(|i| MessageRef {
            mailbox_id: inbox.id,
            message_id: i.id,
        })
        .collect();
    assert!(!at.is_empty());
    let err = h
        .core
        .apply_action(MailAction::MarkRead, at, uuid::Uuid::new_v4().to_string())
        .await
        .expect_err("read-only");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);

    // Such an account has no password to change.
    let err = h
        .core
        .update_password(id, Secret::new("guess".into()))
        .await
        .expect_err("no password");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert!(h.keychain.contains(&key));
}

#[tokio::test]
async fn servers_that_are_not_the_providers_are_refused_before_the_browser() {
    let h = Harness::new(Script::Normal).await;
    let mut evil = settings();
    evil.imap.host = "imap.attacker.example".into();
    let err = h
        .core
        .add_oauth_account(evil, OAuthProvider::Google, OAuthPageTexts::fallback())
        .await
        .expect_err("refused");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert!(h.mock.state().authorize.is_empty(), "no browser step");
    assert!(h.core.list_accounts().await.expect("accounts").is_empty());
}

#[tokio::test]
async fn a_cancelled_or_refused_sign_in_adds_nothing() {
    let h = Harness::new(Script::Deny).await;
    let err = h
        .core
        .add_oauth_account(
            settings(),
            OAuthProvider::Google,
            OAuthPageTexts::fallback(),
        )
        .await
        .expect_err("denied");
    assert_eq!(err.code(), ErrorCode::OauthDenied);
    assert!(h.core.list_accounts().await.expect("accounts").is_empty());
    assert!(h.keychain.is_empty());

    let h = Harness::new(Script::Never).await;
    let core = h.core.clone();
    let waiting = tokio::spawn(async move {
        core.add_oauth_account(
            settings(),
            OAuthProvider::Google,
            OAuthPageTexts::fallback(),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(h.core.cancel_oauth_sign_in());
    let err = waiting.await.expect("task").expect_err("cancelled");
    assert_eq!(err.code(), ErrorCode::OauthCancelled);
    assert!(h.core.list_accounts().await.expect("accounts").is_empty());
    assert!(h.keychain.is_empty());
}

#[tokio::test]
async fn an_ended_permission_stops_the_sync_until_the_user_signs_in_again() {
    let mut h = Harness::new(Script::Normal).await;
    let id = h.add().await;
    h.status(id, |s| s.state == AccountState::Idle).await;

    // The provider revoked the refresh token and the server rejects the old access token.
    h.mock
        .state_mut(|s| s.refresh_error = Some("invalid_grant"));
    h.server
        .with(|s| s.password = "the-token-that-was-revoked".into());
    h.core.network_changed();
    h.status(id, |s| {
        s.state == AccountState::Error
            && s.error.as_ref().map(|e| e.code) == Some(ErrorCode::OauthSignInRequired)
    })
    .await;
    // It tried exactly one renewal after the rejection, not a loop.
    assert_eq!(h.mock.refreshes(), 1);

    // Signing in again stores the new refresh token and the sync recovers.
    h.mock.state_mut(|s| s.refresh_error = None);
    h.server.with(|s| s.password = TOKEN.into());
    h.core
        .sign_in_again(id, OAuthPageTexts::fallback())
        .await
        .expect("signed in again");
    h.status(id, |s| s.state == AccountState::Idle).await;
}

#[tokio::test]
async fn removing_the_account_revokes_and_deletes_the_sign_in() {
    let h = Harness::new(Script::Normal).await;
    let id = h.add().await;
    let key = CredentialRef::for_oauth_account(OAuthProvider::Google, id);
    let refresh = crate::oauth::store::load(h.core.credentials_ref(), &key)
        .await
        .expect("load")
        .expect("stored");
    h.core.remove_account(id).await.expect("removed");
    assert_eq!(
        h.mock.state().revoked,
        vec![refresh.expose_secret().clone()]
    );
    assert!(h.keychain.is_empty(), "no piece of the sign-in is left");
}
