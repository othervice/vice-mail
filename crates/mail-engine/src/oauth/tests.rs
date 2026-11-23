//! The sign-in against a mock authorization server in the test process, with a scripted
//! "browser" in place of the system's (it follows the provider's redirect itself, and can
//! inject forged answers first). The real providers are never contacted.

mod core;
pub(crate) mod mock;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    CoreError, ErrorCode, OAuthPageTexts, OAuthProvider, Secret, Security, ServerSettings,
};

use self::mock::{Browser, MockAuth, Script};
use super::{OAuthConfig, OAuthService, ProviderProfile, RedirectHost, store};
use crate::clock::Clock;
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore};

/// A clock the tests move.
#[derive(Default)]
pub(crate) struct TestClock(Mutex<i64>);

impl TestClock {
    pub(crate) fn advance(&self, ms: i64) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) += ms;
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        1_764_000_000_000 + *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A provider profile pointing at the mock, for `provider`, with the mock's loopback host as
/// its only IMAP and SMTP server.
pub(crate) fn profile(mock: &MockAuth, provider: OAuthProvider) -> ProviderProfile {
    let mut profile = match provider {
        OAuthProvider::Microsoft => ProviderProfile::microsoft("client-ms"),
        OAuthProvider::Google => ProviderProfile::google("client-g", Some("not-secret")),
    };
    profile.authorize_url = mock.url("/authorize");
    profile.token_url = mock.url("/token");
    if profile.revoke_url.is_some() {
        profile.revoke_url = Some(mock.url("/revoke"));
    }
    profile.allow_http_loopback = true;
    profile.imap_hosts = vec!["imap.mock.test".into()];
    profile.smtp_hosts = vec!["smtp.mock.test".into()];
    profile
}

pub(crate) fn service(
    mock: &MockAuth,
    browser: Arc<Browser>,
    clock: Arc<TestClock>,
    timeout: Duration,
) -> Arc<OAuthService> {
    let config = OAuthConfig {
        microsoft: Some(profile(mock, OAuthProvider::Microsoft)),
        google: Some(profile(mock, OAuthProvider::Google)),
        sign_in_timeout: Some(timeout),
    };
    Arc::new(OAuthService::with(config, browser, clock))
}

fn texts() -> OAuthPageTexts {
    OAuthPageTexts::fallback()
}

pub(crate) fn imap_server() -> ServerSettings {
    ServerSettings {
        host: "imap.mock.test".into(),
        port: 993,
        security: Security::Tls,
        username: "alice@mock.test".into(),
    }
}

async fn sign_in(
    script: Script,
    provider: OAuthProvider,
    timeout: Duration,
) -> (
    MockAuth,
    Arc<Browser>,
    Result<super::SignInGrant, CoreError>,
) {
    let mock = MockAuth::start().await;
    mock.state_mut(|s| s.deny = script == Script::Deny);
    let browser = Browser::new(script);
    let service = service(&mock, Arc::clone(&browser), Arc::default(), timeout);
    let result = service.sign_in(provider, "alice@mock.test", texts()).await;
    (mock, browser, result)
}

#[tokio::test]
async fn sign_in_sends_pkce_and_gets_tokens_through_the_loopback_redirect() {
    for provider in OAuthProvider::ALL {
        let (mock, browser, result) =
            sign_in(Script::Normal, provider, Duration::from_secs(10)).await;
        let grant = result.expect("signed in");
        assert_eq!(grant.provider, provider);
        assert!(grant.refresh_token.expose_secret().starts_with("refresh-"));
        assert!(grant.access_token.expose_secret().starts_with("access-"));

        let state = mock.state();
        let authorize = state.authorize.first().expect("authorize request");
        assert_eq!(
            authorize.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            authorize.get("response_type").map(String::as_str),
            Some("code")
        );
        assert_eq!(
            authorize.get("login_hint").map(String::as_str),
            Some("alice@mock.test")
        );
        assert!(!authorize.contains_key("code_verifier"));
        let redirect = authorize.get("redirect_uri").expect("redirect_uri");
        let expected_host = match profile(&mock, provider).redirect_host {
            RedirectHost::Localhost => "http://localhost:",
            RedirectHost::Loopback => "http://127.0.0.1:",
        };
        assert!(redirect.starts_with(expected_host), "{redirect}");
        // The verifier went to the token endpoint and matched the challenge (the mock checks
        // S256 and refuses otherwise).
        let token = state.token.first().expect("token request");
        assert_eq!(
            token.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert_eq!(token.get("code_verifier").map(String::len), Some(43));
        assert_eq!(token.get("redirect_uri"), Some(redirect));
        assert_eq!(
            token.get("client_secret").is_some(),
            provider == OAuthProvider::Google,
            "only Google's installed-app secret is sent"
        );
        assert_eq!(state.verified_pkce, 1);
        // The browser saw the app's page.
        assert!(browser.pages().iter().any(|(status, _)| *status == 200));
    }
}

#[tokio::test]
async fn a_forged_state_is_refused_and_the_real_answer_still_counts() {
    let (mock, browser, result) = sign_in(
        Script::ForgedStateFirst,
        OAuthProvider::Google,
        Duration::from_secs(10),
    )
    .await;
    result.expect("the real answer after the forged one");
    let pages = browser.pages();
    assert_eq!(pages.first().map(|p| p.0), Some(400), "{pages:?}");
    assert_eq!(pages.get(1).map(|p| p.0), Some(200), "{pages:?}");
    // The forged code never reached the token endpoint.
    assert!(
        mock.state()
            .token
            .iter()
            .all(|form| form.get("code").map(String::as_str) != Some("forged-code"))
    );
}

#[tokio::test]
async fn only_a_forged_state_or_another_path_never_finishes_the_sign_in() {
    for script in [
        Script::OnlyForgedState,
        Script::OnlyWrongPath,
        Script::WrongPortOnly,
    ] {
        let (mock, browser, result) =
            sign_in(script, OAuthProvider::Google, Duration::from_millis(1500)).await;
        let err = result.expect_err("no sign-in");
        assert_eq!(err.code(), ErrorCode::OauthTimeout, "{script:?}: {err}");
        assert!(mock.state().token.is_empty(), "{script:?}");
        let pages = browser.pages();
        match script {
            Script::OnlyForgedState => assert_eq!(pages.first().map(|p| p.0), Some(400)),
            Script::OnlyWrongPath => assert_eq!(pages.first().map(|p| p.0), Some(404)),
            _ => assert!(pages.is_empty(), "{pages:?}"),
        }
    }
}

#[tokio::test]
async fn a_browser_that_never_returns_times_out_and_cancel_ends_the_wait() {
    let (_mock, _browser, result) = sign_in(
        Script::Never,
        OAuthProvider::Microsoft,
        Duration::from_millis(300),
    )
    .await;
    assert_eq!(
        result.err().map(|e| e.code()),
        Some(ErrorCode::OauthTimeout)
    );

    let mock = MockAuth::start().await;
    let browser = Browser::new(Script::Never);
    let service = service(&mock, browser, Arc::default(), Duration::from_secs(60));
    let waiting = {
        let service = Arc::clone(&service);
        tokio::spawn(async move { service.sign_in(OAuthProvider::Microsoft, "", texts()).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(service.cancel());
    let result = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("ends at once")
        .expect("task");
    assert_eq!(
        result.err().map(|e| e.code()),
        Some(ErrorCode::OauthCancelled)
    );
    assert!(!service.cancel(), "nothing runs any more");
}

#[tokio::test]
async fn a_refusal_by_the_provider_is_denied_with_its_reason() {
    let (mock, _browser, result) = sign_in(
        Script::Deny,
        OAuthProvider::Microsoft,
        Duration::from_secs(10),
    )
    .await;
    let err = result.expect_err("denied");
    assert_eq!(err.code(), ErrorCode::OauthDenied);
    assert!(err.detail().unwrap_or_default().contains("access_denied"));
    assert!(mock.state().token.is_empty());
}

#[tokio::test]
async fn unavailable_providers_are_named_and_never_start_a_browser() {
    let browser = Browser::new(Script::Normal);
    let service = OAuthService::with(
        OAuthConfig::from_build(Some("ms-id"), Some("google-id"), None),
        browser.clone(),
        Arc::new(TestClock::default()),
    );
    assert!(service.availability().microsoft);
    assert!(
        !service.availability().google,
        "Google needs its installed-app secret too"
    );
    let err = service
        .sign_in(OAuthProvider::Google, "a@gmail.com", texts())
        .await
        .expect_err("unavailable");
    assert_eq!(err.code(), ErrorCode::OauthUnavailable);
    assert!(browser.opened().is_empty());
    let empty = OAuthConfig::from_build(Some("  "), None, Some("x"));
    assert_eq!(
        empty.availability(),
        mail_model::OAuthAvailability::default()
    );
}

/// Keychain with a stored refresh token, credentials with the service.
async fn stored(
    mock: &MockAuth,
    clock: Arc<TestClock>,
) -> (Arc<MemorySecretStore>, Credentials, CredentialRef) {
    let keychain = Arc::new(MemorySecretStore::new());
    let service = service(
        mock,
        Browser::new(Script::Never),
        clock,
        Duration::from_secs(1),
    );
    let credentials = Credentials::from_arc(keychain.clone()).with_oauth(service);
    let key = CredentialRef::for_oauth_account(
        OAuthProvider::Google,
        mail_model::AccountId::from_uuid(uuid::Uuid::new_v4()),
    );
    let refresh = mock.issue_refresh_token();
    store::save(&credentials, &key, &refresh)
        .await
        .expect("save");
    (keychain, credentials, key)
}

#[tokio::test]
async fn tokens_are_renewed_before_they_end_and_after_a_rejection() {
    let mock = MockAuth::start().await;
    let clock = Arc::new(TestClock::default());
    let (_keychain, credentials, key) = stored(&mock, Arc::clone(&clock)).await;

    let first = credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect("token");
    assert_eq!(first.auth, mail_model::LoginAuth::Bearer);
    assert_eq!(mock.refreshes(), 1);
    // Valid for an hour: the same token, no request.
    clock.advance(50 * 60 * 1000);
    let again = credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect("token");
    assert_eq!(again.secret.expose_secret(), first.secret.expose_secret());
    assert_eq!(mock.refreshes(), 1);
    // Five minutes before its end: renewed.
    clock.advance(6 * 60 * 1000);
    let renewed = credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect("token");
    assert_ne!(renewed.secret.expose_secret(), first.secret.expose_secret());
    assert_eq!(mock.refreshes(), 2);
    // A server rejected it: renewed at once.
    assert!(credentials.login_rejected(key.as_str()));
    credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect("token");
    assert_eq!(mock.refreshes(), 3);
    // Every refresh sent the stored refresh token and the client.
    for form in mock.state().token.clone() {
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("refresh_token")
        );
        assert!(
            form.get("refresh_token")
                .is_some_and(|t| t.starts_with("refresh-"))
        );
        assert_eq!(form.get("client_id").map(String::as_str), Some("client-g"));
    }
}

#[tokio::test]
async fn a_rotated_refresh_token_replaces_the_stored_one() {
    let mock = MockAuth::start().await;
    mock.state_mut(|s| s.rotate = true);
    let (_keychain, credentials, key) = stored(&mock, Arc::default()).await;
    let before = store::load(&credentials, &key)
        .await
        .expect("load")
        .expect("some");
    credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect("token");
    let after = store::load(&credentials, &key)
        .await
        .expect("load")
        .expect("some");
    assert_ne!(before.expose_secret(), after.expose_secret());
}

#[tokio::test]
async fn a_dead_refresh_token_means_sign_in_again_and_network_errors_are_transient() {
    let mock = MockAuth::start().await;
    let (_keychain, credentials, key) = stored(&mock, Arc::default()).await;
    mock.state_mut(|s| s.refresh_error = Some("invalid_grant"));
    let err = credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect_err("expired");
    assert_eq!(err.code(), ErrorCode::OauthSignInRequired);
    assert!(!err.is_transient());

    // No sign-in on this device (an account from the settings sync).
    let other = CredentialRef::for_oauth_account(
        OAuthProvider::Google,
        mail_model::AccountId::from_uuid(uuid::Uuid::new_v4()),
    );
    let err = credentials
        .login(other.as_str(), &imap_server(), true)
        .await
        .expect_err("none stored");
    assert_eq!(err.code(), ErrorCode::OauthSignInRequired);

    // The token endpoint is down: the sync retries later.
    mock.stop();
    mock.state_mut(|s| s.refresh_error = None);
    let err = credentials
        .login(key.as_str(), &imap_server(), true)
        .await
        .expect_err("unreachable");
    assert!(err.is_transient(), "{err:?}");
}

#[tokio::test]
async fn tokens_only_go_to_the_providers_servers() {
    let mock = MockAuth::start().await;
    let (_keychain, credentials, key) = stored(&mock, Arc::default()).await;
    let mut evil = imap_server();
    evil.host = "imap.attacker.example".into();
    let err = credentials
        .login(key.as_str(), &evil, true)
        .await
        .expect_err("refused");
    assert_eq!(err.code(), ErrorCode::OauthFailed);
    // IMAP hosts are not SMTP hosts.
    assert!(
        credentials
            .login(key.as_str(), &imap_server(), false)
            .await
            .is_err()
    );
    assert_eq!(mock.refreshes(), 0, "no token was even requested");
    // And the entry is never handed out as a password.
    assert!(credentials.get(&key).await.is_err());
}

#[tokio::test]
async fn revoking_sends_the_refresh_token_where_the_provider_offers_it() {
    let mock = MockAuth::start().await;
    let (keychain, credentials, key) = stored(&mock, Arc::default()).await;
    let token = store::load(&credentials, &key)
        .await
        .expect("load")
        .expect("some");
    super::revoke(&credentials, &key).await;
    assert_eq!(mock.state().revoked, vec![token.expose_secret().clone()]);
    assert!(credentials.delete(&key).await.expect("delete"));
    assert!(keychain.is_empty());

    // Microsoft has no revocation endpoint for public clients: nothing is sent.
    let ms_key = CredentialRef::for_oauth_account(
        OAuthProvider::Microsoft,
        mail_model::AccountId::from_uuid(uuid::Uuid::new_v4()),
    );
    store::save(&credentials, &ms_key, &Secret::new("refresh-ms".into()))
        .await
        .expect("save");
    super::revoke(&credentials, &ms_key).await;
    assert_eq!(mock.state().revoked.len(), 1);
}
