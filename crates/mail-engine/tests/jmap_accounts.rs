//! Stage 14 follow-ups of the JMAP adapter (decision 0041) through `AppCore` with the product
//! connector ([`AnyConnector`]) against the local test server (Stalwart):
//!
//! - **API token:** a JMAP account that signs in with a token (Fastmail's way) sends it as
//!   `Authorization: Bearer`. Stalwart has no Fastmail-style API tokens; an OAuth access token
//!   it issued for bob stands in for one (any bearer token of the account works there).
//! - **Several providers at once:** alice over IMAP and bob over JMAP in one app. The unified
//!   inbox shows both, every message belongs to its own account, actions go to the account's
//!   own server over its own protocol, and read-only holds per account.
//! - **Settings sync on a JMAP account:** two devices keep their settings in bob's
//!   `Settings-Sync` mailbox over JMAP; the account stays read-only, and the folder is checked
//!   over IMAP.
//!
//! Ignored by default. Take the shared lock, start the server (any seed):
//!
//! ```text
//! scripts/testmail.sh lock jmap 60 && scripts/testmail.sh start
//! cargo test -p mail-engine --test jmap_accounts -- --include-ignored --test-threads=1
//! scripts/testmail.sh unlock jmap
//! ```
//!
//! Works with messages `<...@jmap-accounts.test>` it appends itself and with bob's
//! `Settings-Sync` folder; they are removed before and after.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_engine::credentials::{CredentialRef, Credentials, MemorySecretStore};
use mail_engine::protocols::{AnyBackend, AnyConnector, AnyWriter};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, OutboxConfig, SyncConfig};
use mail_imap::test_support::{TestAccount, TestEnv};
use mail_imap::{ImapBackend, ImapConnector};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, BackendError, ConnectConfig, CoreEvent,
    CredentialKind, ErrorCode, IpcError, LanguagePreference, MailAction, MailBackend,
    MailConnector, MessageListItem, MessageRef, OperationState, Preferences, Protocol, Secret,
    Security, ServerSettings, SettingsSyncState, SyncFolder, SyncFolderConnector, ThemePreference,
    UidSet, UnifiedView, WriteConnector,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;
use url::Url;

const WAIT: Duration = Duration::from_secs(60);
const DOMAIN: &str = "jmap-accounts.test";

fn run() -> String {
    format!("ja{}", std::process::id())
}

// ---------------------------------------------------------------------------------------------
// The product connector, recording which protocol and user every connection was for.

/// One connection the engine opened: `read` or `write`, its protocol and its user name.
type Opened = (&'static str, Protocol, String);

#[derive(Clone, Default)]
struct Recording {
    inner: AnyConnector,
    opened: Arc<Mutex<Vec<Opened>>>,
}

impl Recording {
    fn note(&self, what: &'static str, config: &ConnectConfig) {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((what, config.protocol, config.username.clone()));
    }

    fn writers(&self) -> Vec<(Protocol, String)> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(what, _, _)| *what == "write")
            .map(|(_, protocol, user)| (*protocol, user.clone()))
            .collect()
    }

    fn readers(&self) -> Vec<(Protocol, String)> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(what, _, _)| *what == "read")
            .map(|(_, protocol, user)| (*protocol, user.clone()))
            .collect()
    }
}

impl MailConnector for Recording {
    type Backend = AnyBackend;

    async fn connect(&self, config: &ConnectConfig) -> Result<AnyBackend, BackendError> {
        self.note("read", config);
        self.inner.connect(config).await
    }

    fn attach_uid_map_store(&self, store: Arc<dyn mail_model::UidMapStore>) {
        self.inner.attach_uid_map_store(store);
    }
}

impl WriteConnector for Recording {
    type Writer = AnyWriter;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<AnyWriter, BackendError> {
        self.note("write", config);
        self.inner.connect_writer(config).await
    }
}

impl SyncFolderConnector for Recording {
    type Folder = <AnyConnector as SyncFolderConnector>::Folder;

    async fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        folder: &str,
    ) -> Result<Self::Folder, BackendError> {
        self.inner.connect_sync_folder(config, folder).await
    }
}

// ---------------------------------------------------------------------------------------------
// The server, seen over IMAP

async fn imap(env: &TestEnv, who: &TestAccount) -> ImapBackend {
    ImapConnector::new()
        .connect(&env.config(who, Security::Tls, Access::ReadWrite))
        .await
        .expect("IMAP session")
}

async fn raw(admin: &mut ImapBackend, line: &str) {
    let reply = admin
        .raw_command(line, |_| false)
        .await
        .unwrap_or_else(|err| panic!("{line}: {err}"));
    assert!(reply.ok, "{line}: {}", reply.text);
}

/// Appends a message for `to` dated now, so it is near the top of the unified inbox.
async fn append(admin: &mut ImapBackend, to: &str, subject: &str, id: &str) {
    let date = jiff::Timestamp::now()
        .strftime("%a, %d %b %Y %H:%M:%S +0000")
        .to_string();
    let body = format!(
        "From: Mixed Accounts <writer@{DOMAIN}>\r\nTo: {to}\r\nSubject: {subject}\r\n\
         Date: {date}\r\nMessage-ID: <{id}>\r\n\r\nFor {to}.\r\n"
    );
    raw(
        admin,
        &format!("APPEND \"INBOX\" () {{{}+}}\r\n{body}", body.len()),
    )
    .await;
}

/// The flags of the message in INBOX over IMAP: (seen, flagged); `None` if it is not there.
async fn flags(admin: &mut ImapBackend, id: &str) -> Option<(bool, bool)> {
    let opened = admin.open_mailbox("INBOX").await.expect("select");
    let uids = admin.search_uids(&opened).await.expect("search");
    if uids.is_empty() {
        return None;
    }
    admin
        .fetch_headers(&opened, &uids)
        .await
        .expect("fetch")
        .into_iter()
        .find(|h| h.message_id.as_deref() == Some(id))
        .map(|h| (h.flags.seen, h.flags.flagged))
}

async fn clean(admin: &mut ImapBackend) {
    let opened = admin.open_mailbox("INBOX").await.expect("select");
    let uids = admin.search_uids(&opened).await.expect("search");
    if uids.is_empty() {
        return;
    }
    let mine: Vec<u32> = admin
        .fetch_headers(&opened, &uids)
        .await
        .expect("fetch")
        .into_iter()
        .filter(|h| {
            h.message_id
                .as_deref()
                .is_some_and(|id| id.ends_with(&format!("@{DOMAIN}")))
        })
        .map(|h| h.uid)
        .collect();
    if !mine.is_empty() {
        let set = UidSet::from_uids(mine);
        raw(admin, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")).await;
        raw(admin, &format!("UID EXPUNGE {set}")).await;
    }
}

// ---------------------------------------------------------------------------------------------
// The app

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        poll_interval: Duration::from_secs(600),
        extra_root_ca: Some(env.extra_root()),
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

async fn start(
    env: &TestEnv,
    dir: &TempDir,
    keychain: &Arc<MemorySecretStore>,
    connector: Recording,
) -> AppCore<Recording> {
    let mut services =
        CoreServices::with_connector(Credentials::new(Arc::clone(keychain)), connector);
    services.sync = sync_config(env);
    let mut outbox = OutboxConfig::from_sync(&services.sync);
    outbox.linger = Duration::from_millis(500);
    services.outbox = Some(outbox);
    AppCore::start(
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
    .await
}

fn jmap_settings(env: &TestEnv, who: &TestAccount, username: &str) -> AccountSettings {
    let server = ServerSettings {
        host: env.tls_name.clone(),
        port: env.https_port,
        security: Security::Tls,
        username: username.to_owned(),
    };
    AccountSettings {
        email: who.address.clone(),
        display_name: String::new(),
        imap: server.clone(),
        smtp: server,
        protocol: Protocol::Jmap,
        credential: CredentialKind::Password,
    }
}

fn imap_settings(env: &TestEnv, who: &TestAccount) -> AccountSettings {
    let server = |port: u16, security: Security| ServerSettings {
        host: env.host.clone(),
        port,
        security,
        username: who.address.clone(),
    };
    AccountSettings {
        email: who.address.clone(),
        display_name: String::new(),
        imap: server(env.imaps_port, Security::Tls),
        smtp: server(env.submission_port, Security::Starttls),
        protocol: Protocol::Imap,
        credential: CredentialKind::Password,
    }
}

/// Waits until every account in `ids` is idle (synced) at the same time; fails on an error.
async fn all_idle(core: &AppCore<Recording>, ids: &[AccountId]) {
    let deadline = Instant::now() + WAIT;
    loop {
        let accounts = core.list_accounts().await.expect("accounts");
        for account in &accounts {
            assert_ne!(
                account.status.state,
                AccountState::Error,
                "{}: {:?}",
                account.email,
                account.status
            );
        }
        if ids.iter().all(|id| {
            accounts
                .iter()
                .any(|a| a.id == *id && a.status.state == AccountState::Idle)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "idle: {accounts:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The unified inbox, all pages.
async fn unified(core: &AppCore<Recording>) -> Vec<MessageListItem> {
    let mut items = Vec::new();
    let mut cursor = None;
    loop {
        let page = core
            .list_unified_inbox(UnifiedView::All, cursor, 500)
            .await
            .expect("unified inbox");
        items.extend(page.items);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return items,
        }
    }
}

/// Polls the unified inbox until `subject` is there and `pred` holds for it.
async fn until(
    core: &AppCore<Recording>,
    subject: &str,
    pred: impl Fn(&MessageListItem) -> bool,
) -> MessageListItem {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(item) = unified(core)
            .await
            .into_iter()
            .find(|i| i.subject == subject && pred(i))
        {
            return item;
        }
        assert!(Instant::now() < deadline, "{subject} as expected");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn settled(events: &mut broadcast::Receiver<CoreEvent>, batch: mail_model::BatchId) {
    let deadline = Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("operation settled in time");
        if let Ok(CoreEvent::OperationsChanged { operations, .. }) = event
            && let Some(op) = operations.iter().find(|o| o.batch == batch)
        {
            match op.state {
                OperationState::Confirmed => return,
                OperationState::Conflict | OperationState::Failed => {
                    panic!("operation ended {:?}: {op:?}", op.state)
                }
                _ => {}
            }
        }
    }
}

fn at(item: &MessageListItem) -> MessageRef {
    MessageRef {
        mailbox_id: item.mailbox_id,
        message_id: item.id,
    }
}

// ---------------------------------------------------------------------------------------------
// Stalwart's OAuth, for a bearer token of bob

fn http_port(env: &TestEnv) -> u16 {
    let text =
        std::fs::read_to_string(env.repo.join("testmail/accounts.env")).expect("accounts.env");
    text.lines()
        .find_map(|l| l.trim().strip_prefix("export TESTMAIL_HTTP_PORT="))
        .and_then(|v| v.trim().parse().ok())
        .expect("TESTMAIL_HTTP_PORT")
}

async fn post(url: Url, content_type: &str, body: String) -> (u16, serde_json::Value) {
    let tls = mail_imap::tls::client_config(None).expect("tls");
    let response = reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("client")
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body)
        .send()
        .await
        .expect("request");
    let status = response.status().as_u16();
    let bytes = response.bytes().await.expect("answer");
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

/// An access token Stalwart issues for `who` (authorization code with PKCE through its login
/// API, as a person in the browser would). Stalwart accepts it as a bearer token over JMAP.
async fn bearer_token(env: &TestEnv, who: &TestAccount) -> Secret<String> {
    let base = Url::parse(&format!("http://127.0.0.1:{}", http_port(env))).expect("base");
    let scope = "urn:ietf:params:oauth:scope:mail offline_access";
    let redirect = "http://127.0.0.1:40997/";
    let json = "application/json";
    let (_, registered) = post(
        base.join("/auth/register").expect("url"),
        json,
        serde_json::json!({
            "client_name": "Vice Mail API token test",
            "redirect_uris": [redirect],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "application_type": "native",
            "scope": scope,
        })
        .to_string(),
    )
    .await;
    let client_id = registered["client_id"]
        .as_str()
        .unwrap_or_else(|| panic!("client registration: {registered}"))
        .to_owned();
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect)
        .append_pair("scope", scope)
        .append_pair("state", "api-token")
        // RFC 7636, appendix B.
        .append_pair(
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        )
        .append_pair("code_challenge_method", "S256")
        .finish();
    let api = base.join("/api/auth").expect("url");
    let (_, mut answer) = post(
        api.clone(),
        json,
        serde_json::json!({"type": "authCode", "accountName": who.address,
                           "accountSecret": who.password.expose_secret(), "query": query})
        .to_string(),
    )
    .await;
    if answer["type"] == "consentRequired" {
        answer = post(
            api,
            json,
            serde_json::json!({"type": "authConsent", "token": answer["token"],
                               "query": query, "consent": true})
            .to_string(),
        )
        .await
        .1;
    }
    let location = Url::parse(
        answer["location"]
            .as_str()
            .unwrap_or_else(|| panic!("login: {answer}")),
    )
    .expect("location");
    let code = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .expect("code");
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("redirect_uri", redirect)
        .append_pair("client_id", &client_id)
        .append_pair(
            "code_verifier",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
        )
        .finish();
    let (status, token) = post(
        base.join("/auth/token").expect("url"),
        "application/x-www-form-urlencoded",
        form,
    )
    .await;
    assert_eq!(status, 200, "token answer {token}");
    Secret::new(
        token["access_token"]
            .as_str()
            .expect("access token")
            .to_owned(),
    )
}

// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_jmap_account_signs_in_with_an_api_token_as_a_bearer_token() {
    let env = TestEnv::require();
    let run = run();
    let mut bob = imap(&env, &env.bob).await;
    clean(&mut bob).await;
    let subject = format!("{run} token");
    let id = format!("{run}-token@{DOMAIN}");
    append(&mut bob, &env.bob.address, &subject, &id).await;
    let token = bearer_token(&env, &env.bob).await;

    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let connector = Recording::default();
    let core = start(&env, &dir, &keychain, connector.clone()).await;
    let mut events = core.subscribe();
    // A bearer token does not need the user name: one that does not exist proves the token
    // signed in, not a user name and password.
    let mut settings = jmap_settings(&env, &env.bob, "nobody@vicemail.test");
    settings.credential = CredentialKind::ApiToken;

    // The password is not a token, and a token is not a password.
    let err = core
        .test_connection(settings.clone(), env.bob.password.clone())
        .await
        .expect_err("a password as a bearer token");
    assert_eq!(err.code(), ErrorCode::AuthFailed, "{err}");
    let mut as_password = jmap_settings(&env, &env.bob, &env.bob.address);
    as_password.credential = CredentialKind::Password;
    let err = core
        .test_connection(as_password, token.clone())
        .await
        .expect_err("a token as a password");
    assert_eq!(err.code(), ErrorCode::AuthFailed, "{err}");
    assert!(
        !format!("{err:?} {err}").contains(token.expose_secret().as_str()),
        "the token never shows in an error"
    );

    let tested = core
        .test_connection(settings.clone(), token.clone())
        .await
        .expect("test with the token");
    assert!(tested.capabilities.jmap && tested.capabilities.idle);
    let account = core
        .add_account(settings, token.clone())
        .await
        .expect("add with the token");
    assert_eq!(account.credential, CredentialKind::ApiToken);
    assert_eq!(account.protocol, Protocol::Jmap);
    assert!(!account.writes_enabled);
    let key = CredentialRef::for_api_token_account(account.id);
    assert_eq!(
        Credentials::new(Arc::clone(&keychain))
            .get(&key)
            .await
            .expect("keychain")
            .map(Secret::into_exposed)
            .as_deref(),
        Some(token.expose_secret().as_str()),
        "the token is kept in the keychain like a password"
    );
    all_idle(&core, &[account.id]).await;
    let item = until(&core, &subject, |_| true).await;
    assert_eq!(item.account_id, account.id);

    // Writing with the token: through the outbox to the server.
    core.set_writes_enabled(account.id, true)
        .await
        .expect("writes on");
    let flagged = core
        .apply_action(MailAction::Flag, vec![at(&item)], "flag".into())
        .await
        .expect("flag");
    settled(&mut events, flagged.batch.expect("batch")).await;
    assert_eq!(flags(&mut bob, &id).await, Some((false, true)));
    assert!(
        connector
            .writers()
            .iter()
            .all(|(p, user)| *p == Protocol::Jmap && user == "nobody@vicemail.test"),
        "{:?}",
        connector.writers()
    );

    core.shutdown().await;
    clean(&mut bob).await;
    bob.logout().await.ok();
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn imap_and_jmap_accounts_share_the_unified_inbox_each_on_its_own_server() {
    let env = TestEnv::require();
    let run = run();
    let mut alice = imap(&env, &env.alice).await;
    let mut bob = imap(&env, &env.bob).await;
    clean(&mut alice).await;
    clean(&mut bob).await;
    let (alice_subject, alice_id) = (format!("{run} for alice"), format!("{run}-a@{DOMAIN}"));
    let (bob_subject, bob_id) = (format!("{run} for bob"), format!("{run}-b@{DOMAIN}"));
    append(&mut alice, &env.alice.address, &alice_subject, &alice_id).await;
    append(&mut bob, &env.bob.address, &bob_subject, &bob_id).await;

    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let connector = Recording::default();
    let core = start(&env, &dir, &keychain, connector.clone()).await;
    let mut events = core.subscribe();
    let over_imap = core
        .add_account(imap_settings(&env, &env.alice), env.alice.password.clone())
        .await
        .expect("alice over IMAP");
    let over_jmap = core
        .add_account(
            jmap_settings(&env, &env.bob, &env.bob.address),
            env.bob.password.clone(),
        )
        .await
        .expect("bob over JMAP");
    assert_eq!(over_imap.protocol, Protocol::Imap);
    assert_eq!(over_jmap.protocol, Protocol::Jmap);
    all_idle(&core, &[over_imap.id, over_jmap.id]).await;

    // 1. The unified inbox shows both, each message with its own account and that account's
    //    inbox.
    let a = until(&core, &alice_subject, |_| true).await;
    let b = until(&core, &bob_subject, |_| true).await;
    assert_eq!(a.account_id, over_imap.id);
    assert_eq!(b.account_id, over_jmap.id);
    for (item, account) in [(&a, over_imap.id), (&b, over_jmap.id)] {
        let inbox = core
            .list_mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == "INBOX")
            .expect("inbox");
        assert_eq!(item.mailbox_id, inbox.id, "{}", item.subject);
    }
    let both = unified(&core).await;
    assert!(both.iter().any(|i| i.account_id == over_imap.id));
    assert!(both.iter().any(|i| i.account_id == over_jmap.id));
    // Every connection so far was the account's own: alice over IMAP, bob over JMAP.
    for (protocol, user) in connector.readers() {
        match protocol {
            Protocol::Imap => assert_eq!(user, env.alice.address),
            Protocol::Jmap => assert_eq!(user, env.bob.address),
        }
    }
    assert!(connector.writers().is_empty(), "both read-only");

    // 2. Read-only per account: writing on for bob only. Alice's message is refused before
    //    anything is sent; bob's reaches his server over JMAP.
    core.set_writes_enabled(over_jmap.id, true)
        .await
        .expect("bob writes");
    let err = core
        .apply_action(MailAction::Flag, vec![at(&a)], "alice-ro".into())
        .await
        .expect_err("alice is read-only");
    assert_eq!(IpcError::from(&err).code, ErrorCode::ReadOnly, "{err}");
    let flagged = core
        .apply_action(MailAction::Flag, vec![at(&b)], "bob-flag".into())
        .await
        .expect("flag bob's");
    settled(&mut events, flagged.batch.expect("batch")).await;
    assert_eq!(flags(&mut bob, &bob_id).await, Some((false, true)));
    assert_eq!(flags(&mut alice, &alice_id).await, Some((false, false)));
    assert_eq!(
        connector.writers(),
        [(Protocol::Jmap, env.bob.address.clone())],
        "only bob's server was written to"
    );

    // 3. Writing on for alice too: marking both read (an action is for one account) reaches
    //    both servers, each over its own protocol.
    core.set_writes_enabled(over_imap.id, true)
        .await
        .expect("alice writes");
    for (item, request) in [(&a, "read-alice"), (&b, "read-bob")] {
        let read = core
            .apply_action(MailAction::MarkRead, vec![at(item)], request.into())
            .await
            .unwrap_or_else(|err| panic!("mark {} read: {err}", item.subject));
        settled(&mut events, read.batch.expect("batch")).await;
    }
    let deadline = Instant::now() + WAIT;
    loop {
        let (fa, fb) = (
            flags(&mut alice, &alice_id).await,
            flags(&mut bob, &bob_id).await,
        );
        if fa == Some((true, false)) && fb == Some((true, true)) {
            break;
        }
        assert!(Instant::now() < deadline, "alice {fa:?}, bob {fb:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let writers = connector.writers();
    assert!(writers.contains(&(Protocol::Imap, env.alice.address.clone())));
    for (protocol, user) in &writers {
        match protocol {
            Protocol::Imap => assert_eq!(user, &env.alice.address),
            Protocol::Jmap => assert_eq!(user, &env.bob.address),
        }
    }
    until(&core, &alice_subject, |i| {
        !i.unread && i.account_id == over_imap.id
    })
    .await;
    until(&core, &bob_subject, |i| {
        !i.unread && i.flagged && i.account_id == over_jmap.id
    })
    .await;

    core.shutdown().await;
    clean(&mut alice).await;
    clean(&mut bob).await;
    alice.logout().await.ok();
    bob.logout().await.ok();
}

/// The entries of bob's settings sync folder over IMAP: (id, version) per entry, or `None` when
/// there is no folder.
async fn folder_over_imap(env: &TestEnv) -> Option<Vec<(String, String)>> {
    let config = env.config(&env.bob, Security::Tls, Access::ReadOnly);
    let mut folder = ImapConnector::new()
        .connect_sync_folder(&config, "Settings-Sync")
        .await
        .expect("IMAP folder connection");
    folder.open(false).await.expect("open")?;
    let fields = ["X-Settings-Sync-Id", "X-Settings-Sync-Version"];
    let entries = folder
        .list(&fields)
        .await
        .expect("list")
        .into_iter()
        .map(|e| {
            let text = String::from_utf8_lossy(&e.header).into_owned();
            let value = |name: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(&format!("{name}:")))
                    .map(|v| v.trim().to_owned())
                    .unwrap_or_default()
            };
            (value(fields[0]), value(fields[1]))
        })
        .collect();
    folder.logout().await.ok();
    Some(entries)
}

/// Removes bob's settings sync folder (a run before may have left it).
async fn drop_folder(bob: &mut ImapBackend) {
    let reply = bob
        .raw_command("DELETE \"Settings-Sync\"", |_| false)
        .await
        .expect("DELETE");
    // NO when there is none.
    let _ = reply.ok;
}

async fn inbox_count(bob: &mut ImapBackend) -> u64 {
    let opened = bob.open_mailbox("INBOX").await.expect("select");
    bob.search_uids(&opened).await.expect("search").len()
}

/// One device: its own database and keychain, bob over JMAP.
async fn device(env: &TestEnv) -> (TempDir, AppCore<Recording>, AccountId) {
    let dir = TempDir::new().expect("temp dir");
    let keychain: Arc<MemorySecretStore> = Arc::default();
    let core = start(env, &dir, &keychain, Recording::default()).await;
    let account = core
        .add_account(
            jmap_settings(env, &env.bob, &env.bob.address),
            env.bob.password.clone(),
        )
        .await
        .expect("bob over JMAP");
    all_idle(&core, &[account.id]).await;
    (dir, core, account.id)
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn the_settings_sync_keeps_its_folder_in_a_jmap_account() {
    let env = TestEnv::require();
    let mut bob = imap(&env, &env.bob).await;
    drop_folder(&mut bob).await;
    let inbox_before = inbox_count(&mut bob).await;
    let passphrase = || Secret::new("correct horse battery staple".to_owned());

    // 1. Device A turns the sync on with bob's JMAP account: the folder is created over JMAP.
    let (_a_dir, a, a_bob) = device(&env).await;
    let status = a
        .enable_settings_sync(a_bob, passphrase())
        .await
        .expect("enable on a JMAP account");
    assert_eq!(status.state, SettingsSyncState::Active, "{status:?}");
    a.set_preferences(Preferences {
        theme: ThemePreference::Dark,
        language: LanguagePreference::De,
    })
    .await
    .expect("preferences");
    let status = a.sync_settings_now().await.expect("pass");
    assert_eq!(status.state, SettingsSyncState::Active, "{status:?}");
    let entries = folder_over_imap(&env).await.expect("the folder exists");
    assert!(entries.len() >= 2, "key record and settings: {entries:?}");
    assert!(
        entries
            .iter()
            .all(|(id, version)| !id.is_empty() && !version.is_empty()),
        "{entries:?}"
    );
    // The account stays read-only; nothing outside the folder changed.
    let accounts = a.list_accounts().await.expect("accounts");
    assert!(accounts.iter().all(|x| !x.writes_enabled));
    assert_eq!(inbox_count(&mut bob).await, inbox_before);

    // 2. Device B sees the folder in bob's JMAP mailboxes and joins with the passphrase.
    let (_b_dir, b, b_bob) = device(&env).await;
    let status = b.get_settings_sync_status().await.expect("status");
    assert!(
        status.detected_accounts.contains(&b_bob),
        "the folder was detected over JMAP: {status:?}"
    );
    b.preview_settings_sync_join(b_bob, passphrase())
        .await
        .expect("preview");
    let status = b.join_settings_sync().await.expect("join");
    assert_eq!(status.state, SettingsSyncState::Active, "{status:?}");
    assert_eq!(
        b.get_preferences().await.expect("preferences"),
        Preferences {
            theme: ThemePreference::Dark,
            language: LanguagePreference::De,
        },
        "A's settings arrived over JMAP"
    );

    // 3. A change on B replaces its entry (import the new one, destroy the old one) and
    //    reaches A.
    b.set_preferences(Preferences {
        theme: ThemePreference::Light,
        language: LanguagePreference::De,
    })
    .await
    .expect("preferences");
    b.sync_settings_now().await.expect("pass B");
    a.sync_settings_now().await.expect("pass A");
    assert_eq!(
        a.get_preferences().await.expect("preferences").theme,
        ThemePreference::Light
    );
    let entries = folder_over_imap(&env).await.expect("the folder exists");
    let mut ids: Vec<&String> = entries.iter().map(|(id, _)| id).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "one entry per record: {entries:?}");
    assert_eq!(inbox_count(&mut bob).await, inbox_before);

    a.shutdown().await;
    b.shutdown().await;
    drop_folder(&mut bob).await;
    bob.logout().await.ok();
}
