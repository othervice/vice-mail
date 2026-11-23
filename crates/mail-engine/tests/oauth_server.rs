//! Stage 14 (decision 0042): sign-in with OAuth 2.0 end to end against the local test server.
//! Stalwart is an OAuth authorization server too (authorization code with PKCE S256, refresh
//! tokens with rotation, RFC 7009 revocation, open dynamic client registration) and accepts
//! the access token over IMAP and SMTP with `XOAUTH2` and `OAUTHBEARER`. A scripted "browser"
//! signs in on Stalwart's login API as alice and follows the redirect to the app's loopback
//! listener, as a person in the system browser would. Microsoft and Google themselves are never
//! contacted: the provider profile here points at Stalwart.
//!
//! Ignored by default; needs the test server (`scripts/testmail.sh start`) and its lock:
//!
//! ```text
//! scripts/testmail.sh lock oauth 60 && scripts/testmail.sh start
//! cargo test -p mail-engine --test oauth_server -- --include-ignored
//! scripts/testmail.sh unlock oauth
//! ```

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::{
    AppCore, BuildInfo, Clock, CoreConfig, CoreServices, OAuthConfig, OAuthService,
    ProviderProfile, SyncConfig, SystemClock, SystemOpener,
};
use mail_imap::test_support::TestEnv;
use mail_imap::{ImapConnector, SmtpSender, WireTap};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, BackendError, CoreError, CoreEvent, Envelope,
    LoginAuth, MailBackend, MailConnector, OAuthPageTexts, OAuthProvider, Secret, Security,
    SendOutcome,
};
use tempfile::TempDir;
use url::Url;

const WAIT: Duration = Duration::from_secs(60);
const SCOPES: &[&str] = &["urn:ietf:params:oauth:scope:mail", "offline_access"];

fn http_port(env: &TestEnv) -> u16 {
    let text =
        std::fs::read_to_string(env.repo.join("testmail/accounts.env")).expect("accounts.env");
    text.lines()
        .find_map(|l| l.trim().strip_prefix("export TESTMAIL_HTTP_PORT="))
        .and_then(|v| v.trim().parse().ok())
        .expect("TESTMAIL_HTTP_PORT")
}

fn http() -> reqwest::Client {
    let tls = mail_imap::tls::client_config(None).expect("tls");
    reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("client")
}

/// POSTs JSON and reads the JSON answer (reqwest has no `json` feature in this workspace).
async fn post_json(url: Url, body: &serde_json::Value) -> serde_json::Value {
    let response = http()
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(body).expect("json"))
        .send()
        .await
        .expect("request");
    let bytes = response.bytes().await.expect("answer");
    serde_json::from_slice(&bytes).expect("json answer")
}

/// Registers a native client with the loopback redirect (RFC 7591); returns its client id.
async fn register_client(base: &Url) -> String {
    let answer = post_json(
        base.join("/auth/register").expect("url"),
        &serde_json::json!({
            "client_name": "Vice Mail tests",
            "redirect_uris": ["http://127.0.0.1/"],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "application_type": "native",
            "scope": SCOPES.join(" "),
        }),
    )
    .await;
    answer["client_id"].as_str().expect("client_id").to_owned()
}

/// The provider profile of the test: Google's form (loopback redirect, revocation), Stalwart's
/// endpoints and scopes, the test server's host as the only IMAP and SMTP server.
fn profile(base: &Url, client_id: &str, host: &str) -> ProviderProfile {
    let mut profile = ProviderProfile::google(client_id, None);
    profile.authorize_url = base.join("/authorize").expect("url");
    profile.token_url = base.join("/auth/token").expect("url");
    profile.revoke_url = Some(base.join("/auth/revoke").expect("url"));
    profile.scopes = SCOPES.iter().map(|s| (*s).to_owned()).collect();
    profile.extra_params = Vec::new();
    profile.imap_hosts = vec![host.to_owned()];
    profile.smtp_hosts = vec![host.to_owned()];
    profile.allow_http_loopback = true;
    profile
}

/// Plays the person in the browser: signs in on Stalwart's login API with the query of the
/// authorization address, gives consent, and follows the redirect to the app. With
/// `forge_first`, a forged answer (other `state`) reaches the app first.
struct StalwartBrowser {
    base: Url,
    user: String,
    password: String,
    forge_first: bool,
    /// Status codes of the app's pages, in order.
    pages: Arc<Mutex<Vec<u16>>>,
}

impl SystemOpener for StalwartBrowser {
    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        let url = Url::parse(url).map_err(|e| CoreError::Internal(e.to_string()))?;
        let base = self.base.clone();
        let (user, password) = (self.user.clone(), self.password.clone());
        let forge_first = self.forge_first;
        let pages = Arc::clone(&self.pages);
        tokio::runtime::Handle::current().spawn(async move {
            let query = url.query().unwrap_or_default().to_owned();
            let client = http();
            let location = login(&base, &user, &password, &query).await;
            let mut visits = Vec::new();
            if forge_first {
                let mut forged = location.clone();
                forged
                    .query_pairs_mut()
                    .clear()
                    .append_pair("code", "forged")
                    .append_pair("state", "forged");
                visits.push(forged);
            }
            visits.push(location);
            for visit in visits {
                if let Ok(response) = client.get(visit).send().await {
                    pages
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(response.status().as_u16());
                }
            }
        });
        Ok(())
    }

    fn open_path(&self, _path: &std::path::Path) -> Result<(), CoreError> {
        Err(CoreError::Internal("not a file browser".into()))
    }
}

fn settings(env: &TestEnv) -> AccountSettings {
    let server = |port: u16, security: &str| {
        serde_json::json!({"host": env.host, "port": port, "security": security,
                           "username": env.alice.address})
    };
    serde_json::from_value(serde_json::json!({
        "email": env.alice.address,
        "displayName": "Alice",
        "imap": server(env.imaps_port, "tls"),
        "smtp": server(env.submission_port, "starttls"),
    }))
    .expect("settings")
}

/// A clock that can jump ahead (the access token's lifetime), otherwise the system's.
#[derive(Default)]
struct LaterClock(AtomicI64);

impl LaterClock {
    fn advance(&self, by: Duration) {
        self.0
            .fetch_add(i64::try_from(by.as_millis()).expect("ms"), Ordering::SeqCst);
    }
}

impl Clock for LaterClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst)
    }
}

/// Waits until the account is idle (synced); fails on an error state.
async fn wait_idle(events: &mut tokio::sync::broadcast::Receiver<CoreEvent>, id: AccountId) {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("account idle in time")
            .expect("events");
        if let CoreEvent::AccountStatusChanged { account_id, status } = event
            && account_id == id
        {
            assert_ne!(status.state, AccountState::Error, "{status:?}");
            if status.state == AccountState::Idle {
                return;
            }
        }
    }
}

/// The refresh token in the (in-memory) keychain: the sign-in entry's JSON.
fn stored_refresh_token(keychain: &MemorySecretStore) -> String {
    let entries = keychain.secrets();
    assert_eq!(entries.len(), 1, "one keychain entry for the account");
    let entry: serde_json::Value =
        serde_json::from_str(entries[0].expose_secret()).expect("sign-in entry");
    entry["refreshToken"]
        .as_str()
        .expect("refreshToken")
        .to_owned()
}

/// Signs in on Stalwart's login API with an authorization query (and consents); returns where
/// Stalwart sends the browser.
async fn login(base: &Url, user: &str, password: &str, query: &str) -> Url {
    let api = base.join("/api/auth").expect("url");
    let mut answer = post_json(
        api.clone(),
        &serde_json::json!({"type": "authCode", "accountName": user,
                            "accountSecret": password, "query": query}),
    )
    .await;
    if answer["type"] == "consentRequired" {
        answer = post_json(
            api,
            &serde_json::json!({"type": "authConsent", "token": answer["token"],
                                "query": query, "consent": true}),
        )
        .await;
    }
    Url::parse(answer["location"].as_str().expect("redirect location")).expect("location")
}

/// A token answer of Stalwart for `form`.
async fn token(base: &Url, form: &[(&str, &str)]) -> (u16, serde_json::Value) {
    let mut body = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in form {
        body.append_pair(key, value);
    }
    let response = http()
        .post(base.join("/auth/token").expect("url"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body.finish())
        .send()
        .await
        .expect("token request");
    let status = response.status().as_u16();
    let bytes = response.bytes().await.expect("answer");
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn sign_in_imap_smtp_refresh_and_revocation_against_stalwart() {
    let env = TestEnv::load().expect("test server settings");
    let base = Url::parse(&format!("http://127.0.0.1:{}", http_port(&env))).expect("base");
    let client_id = register_client(&base).await;
    let pages = Arc::new(Mutex::new(Vec::new()));
    let clock = Arc::new(LaterClock::default());
    let browser = Arc::new(StalwartBrowser {
        base: base.clone(),
        user: env.alice.address.clone(),
        password: env.alice.password.expose_secret().clone(),
        forge_first: true,
        pages: Arc::clone(&pages),
    });
    let oauth = OAuthService::with(
        OAuthConfig {
            microsoft: None,
            google: Some(profile(&base, &client_id, &env.host)),
            sign_in_timeout: Some(Duration::from_secs(30)),
        },
        browser,
        Arc::clone(&clock) as Arc<dyn Clock>,
    );
    let keychain = Arc::new(MemorySecretStore::new());
    let mut services = CoreServices::with_connector(
        Credentials::new(Arc::clone(&keychain)).with_oauth(Arc::new(oauth)),
        ImapConnector::new(),
    );
    services.sync = SyncConfig {
        extra_root_ca: Some(env.extra_root()),
        poll_interval: Duration::from_secs(600),
        ..SyncConfig::default()
    };
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
        services,
    )
    .await;
    let mut events = core.subscribe();

    // 1. Sign-in in the "browser", IMAP login with the token, account stored read-only.
    let account = core
        .add_oauth_account(
            settings(&env),
            OAuthProvider::Google,
            OAuthPageTexts::fallback(),
        )
        .await
        .expect("account added through the sign-in");
    assert_eq!(account.oauth, Some(OAuthProvider::Google));
    assert!(!account.writes_enabled);
    // The forged answer was refused, the real one accepted.
    assert_eq!(
        *pages.lock().unwrap_or_else(PoisonError::into_inner),
        vec![400, 200]
    );

    // 2. The sync logs in with the access token and gets to work.
    wait_idle(&mut events, account.id).await;

    // 3. Only the refresh token is in the keychain (the access token stays in memory).
    let first_refresh = stored_refresh_token(&keychain);
    assert!(
        first_refresh.starts_with("sw1."),
        "a Stalwart refresh token"
    );

    // Two hours later the access token has run out: the next connection renews it at the
    // token endpoint, and Stalwart's rotated refresh token replaces the stored one.
    clock.advance(Duration::from_secs(2 * 3600));
    core.network_changed();
    let deadline = tokio::time::Instant::now() + WAIT;
    while stored_refresh_token(&keychain) == first_refresh {
        assert!(tokio::time::Instant::now() < deadline, "no renewal");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let refresh = stored_refresh_token(&keychain);
    wait_idle(&mut events, account.id).await;

    // A second, independent sign-in gives this test an access token of its own for the adapter
    // checks below, so the app's grant stays untouched.
    let access = own_access_token(&env, &base, &client_id).await;

    // 4. The adapter with the token: XOAUTH2 on STARTTLS (capabilities unknown before login),
    // OAUTHBEARER where XOAUTH2 is hidden, a wrong token is AuthFailed, and SMTP sends.
    let bearer = |security: Security| {
        let mut config = env.config(&env.alice, security, Access::ReadOnly);
        config.password = Secret::new(access.clone());
        config.auth = LoginAuth::Bearer;
        config
    };
    for (security, masked, mechanism) in [
        (Security::Starttls, None, "AUTHENTICATE XOAUTH2"),
        (
            Security::Tls,
            Some("AUTH=XOAUTH2"),
            "AUTHENTICATE OAUTHBEARER",
        ),
    ] {
        let tap = WireTap::new();
        let mut connector = ImapConnector::new().with_wire_tap(tap.clone());
        if let Some(masked) = masked {
            connector = connector.with_masked_capabilities([masked]);
        }
        let backend = connector
            .connect(&bearer(security))
            .await
            .unwrap_or_else(|err| panic!("{mechanism}: {err}"));
        backend.logout().await.expect("logout");
        let lines = tap.lines();
        assert!(
            lines.iter().any(|l| l.contains(mechanism)),
            "{mechanism} not used: {lines:?}"
        );
        assert!(lines.iter().all(|l| !l.contains("LOGIN")), "{lines:?}");
    }
    let mut wrong = bearer(Security::Tls);
    wrong.password = Secret::new("sw1.not-a-token".into());
    let err = ImapConnector::new()
        .connect(&wrong)
        .await
        .expect_err("refused");
    assert!(matches!(err, BackendError::AuthFailed(_)), "{err:?}");

    let mut smtp = env.smtp_config(&env.alice, Security::Starttls);
    smtp.password = Secret::new(access.clone());
    smtp.auth = LoginAuth::Bearer;
    SmtpSender::new()
        .verify(&smtp)
        .await
        .expect("SMTP login with the token");
    let message = format!(
        "From: {a}\r\nTo: {a}\r\nSubject: oauth smtp check\r\nMessage-ID: <oauth-{n}@vicemail.test>\r\n\r\nsent with XOAUTH2\r\n",
        a = env.alice.address,
        n = std::process::id()
    );
    let outcome = SmtpSender::new()
        .send(
            &smtp,
            &Envelope {
                from: env.alice.address.clone(),
                recipients: vec![env.alice.address.clone()],
            },
            message.as_bytes(),
        )
        .await
        .expect("sent");
    assert!(matches!(outcome, SendOutcome::Accepted(_)), "{outcome:?}");

    // 5. Read-only like every new account, also with a sign-in.
    let err = core
        .set_writes_enabled(account.id, true)
        .await
        .map(|a| a.writes_enabled);
    assert_eq!(err.ok(), Some(true), "the user may allow writing");
    core.set_writes_enabled(account.id, false)
        .await
        .expect("off again");

    // 6. Removing the account revokes the grant at the server and deletes the entry.
    assert_eq!(
        stored_refresh_token(&keychain),
        refresh,
        "not renewed meanwhile"
    );
    core.remove_account(account.id).await.expect("removed");
    assert!(keychain.is_empty(), "nothing of the sign-in is left");
    let (status, answer) = token(
        &base,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
        ],
    )
    .await;
    assert_eq!(status, 400, "the revoked refresh token is dead: {answer}");
    assert_eq!(answer["error"], "invalid_grant");
    core.shutdown().await;
}

/// An access token from a sign-in of the test's own (PKCE with a fixed verifier).
async fn own_access_token(env: &TestEnv, base: &Url, client_id: &str) -> String {
    let redirect = "http://127.0.0.1:40998/";
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect)
        .append_pair("scope", &SCOPES.join(" "))
        .append_pair("state", "own")
        .append_pair(
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        )
        .append_pair("code_challenge_method", "S256")
        .finish();
    let location = login(
        base,
        &env.alice.address,
        env.alice.password.expose_secret(),
        &query,
    )
    .await;
    let code: String = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .expect("code");
    let (status, answer) = token(
        base,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", redirect),
            ("client_id", client_id),
            (
                "code_verifier",
                "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ),
        ],
    )
    .await;
    assert_eq!(status, 200, "{answer}");
    answer["access_token"]
        .as_str()
        .expect("access token")
        .to_owned()
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn stalwart_insists_on_the_pkce_verifier() {
    // What the app relies on: a code is only good with the verifier of its challenge.
    let env = TestEnv::load().expect("test server settings");
    let base = Url::parse(&format!("http://127.0.0.1:{}", http_port(&env))).expect("base");
    let client_id = register_client(&base).await;
    let redirect = "http://127.0.0.1:40999/";
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect)
        .append_pair("scope", &SCOPES.join(" "))
        .append_pair("state", "s")
        .append_pair(
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        )
        .append_pair("code_challenge_method", "S256")
        .finish();
    let location = login(
        &base,
        &env.alice.address,
        env.alice.password.expose_secret(),
        &query,
    )
    .await;
    let code: String = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .expect("code");
    let base_form = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect),
        ("client_id", client_id.as_str()),
    ];
    let (status, _) = token(&base, &base_form).await;
    assert_eq!(status, 400, "no token without the verifier");
    let mut wrong = base_form.to_vec();
    wrong.push((
        "code_verifier",
        "wrong-verifier-wrong-verifier-wrong-verifier",
    ));
    let (status, _) = token(&base, &wrong).await;
    assert_eq!(status, 400, "no token with another verifier");
    let mut right = base_form.to_vec();
    right.push(("code_verifier", verifier));
    let (status, answer) = token(&base, &right).await;
    assert_eq!(status, 200, "{answer}");
    let (status, _) = token(&base, &right).await;
    assert_eq!(status, 400, "a code is good once");
}
