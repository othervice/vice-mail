//! Stage 14, blocks B and C (decision 0042): accounts that sign in with Microsoft or Google.
//!
//! - **Sign-in** ([`flow`]): OAuth 2.0 authorization code with PKCE (S256, RFC 7636) in the
//!   system browser and a loopback redirect (RFC 8252): the core listens on
//!   `127.0.0.1:<random port>` ([`loopback`]), opens the provider's page with the system's
//!   default browser (the same opener as links, never the WebView), and accepts exactly one
//!   answer that carries the random `state` it sent. Answers on other paths or with another
//!   `state` are refused and the wait goes on; it ends after a time limit or on cancel.
//! - **Tokens** ([`store`], [`tokens`]): the refresh token lives in the system keychain under
//!   the account's keychain reference (`oauth:<provider>:<id>`), the access token only in
//!   memory. A connection gets a token that is valid for at least five more minutes; a refresh
//!   that the provider refuses with `invalid_grant` is [`OAuthError::SignInRequired`] ("sign in
//!   again"). A rejected login refreshes once. Tokens never reach a log, an error text, the
//!   database, the settings sync or a diagnostics export.
//! - **Profiles** ([`ProviderProfile`]): endpoints, scopes and the servers a token may go to.
//!   A token is only handed to a connection whose host belongs to the provider, so a changed
//!   server setting can never send it elsewhere. The client ID (and Google's installed-app
//!   "secret", which Google says is not confidential) come from the build; without them the
//!   provider is unavailable in this build.

mod flow;
mod loopback;
mod pkce;
pub(crate) mod store;
mod token_http;
pub(crate) mod tokens;

mod core;

#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{OAuthAvailability, OAuthError, OAuthProvider};
use url::Url;

use crate::clock::{Clock, SystemClock};
use crate::credentials::{CredentialRef, Credentials};
use crate::reading::{DesktopOpener, SystemOpener};

pub use self::flow::SignInGrant;

/// How long the app waits for the browser to come back.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Microsoft identity platform, authority `common`: work and school accounts and personal
/// Microsoft accounts (Outlook.com, Hotmail, Live).
const MICROSOFT_AUTHORIZE: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const MICROSOFT_TOKEN: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
/// IMAP and SMTP for Exchange Online and Outlook.com, plus a refresh token.
const MICROSOFT_SCOPES: &[&str] = &[
    "https://outlook.office.com/IMAP.AccessAsUser.All",
    "https://outlook.office.com/SMTP.Send",
    "offline_access",
];
/// The servers Microsoft names for IMAP and SMTP with OAuth (Exchange Online and Outlook.com).
const MICROSOFT_IMAP_HOSTS: &[&str] = &["outlook.office365.com", "outlook.office.com"];
const MICROSOFT_SMTP_HOSTS: &[&str] = &[
    "smtp.office365.com",
    "smtp-mail.outlook.com",
    "outlook.office365.com",
];

const GOOGLE_AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE: &str = "https://oauth2.googleapis.com/revoke";
/// Full IMAP and SMTP access to Gmail (a "restricted" scope; see `docs/PROVIDERS.md`).
const GOOGLE_SCOPES: &[&str] = &["https://mail.google.com/"];
const GOOGLE_IMAP_HOSTS: &[&str] = &["imap.gmail.com"];
const GOOGLE_SMTP_HOSTS: &[&str] = &["smtp.gmail.com"];

/// The host name in the redirect URI. The listener is always on the loopback address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectHost {
    /// `http://127.0.0.1:<port>/` (RFC 8252, section 7.3; Google's desktop clients).
    Loopback,
    /// `http://localhost:<port>`: Microsoft Entra registers `http://localhost` for desktop
    /// apps and accepts any port with it. The listener answers on 127.0.0.1 and ::1.
    Localhost,
}

/// One provider's endpoints, scopes and client, and the servers its tokens may go to.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderProfile {
    pub provider: OAuthProvider,
    /// Public identifier of the app at the provider; not a secret.
    pub client_id: String,
    /// Google's "client secret" of an installed app. Google states it cannot be kept secret in
    /// an installed app and is not treated as one; it is sent because Google's token endpoint
    /// asks for it. `None` for Microsoft (a public client has none).
    pub client_secret: Option<String>,
    pub authorize_url: Url,
    pub token_url: Url,
    /// RFC 7009 revocation; `None` where the provider offers none for this client (Microsoft).
    pub revoke_url: Option<Url>,
    pub scopes: Vec<String>,
    /// Further authorization parameters (Google: `access_type=offline`, `prompt=consent`, so a
    /// refresh token always comes back).
    pub extra_params: Vec<(String, String)>,
    pub redirect_host: RedirectHost,
    /// Lowercase host names of IMAP and SMTP servers this provider's tokens may go to.
    pub imap_hosts: Vec<String>,
    pub smtp_hosts: Vec<String>,
    /// Plain HTTP to loopback endpoints. Only the tests' and the desktop test build's own
    /// authorization servers; the provider endpoints are HTTPS.
    pub allow_http_loopback: bool,
}

impl std::fmt::Debug for ProviderProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderProfile")
            .field("provider", &self.provider)
            .field("authorize_url", &self.authorize_url.as_str())
            .field("token_url", &self.token_url.as_str())
            .finish_non_exhaustive()
    }
}

fn url(text: &str) -> Url {
    // The constants above are valid URLs (`tests::provider_profiles_use_the_documented_…`).
    Url::parse(text).unwrap_or_else(|_| Url::parse("https://invalid.invalid/").expect("valid"))
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

impl ProviderProfile {
    /// Microsoft 365 and Outlook.com with the client ID of an app registered in Microsoft
    /// Entra as a public client ("Mobile and desktop applications", redirect `http://localhost`).
    pub fn microsoft(client_id: &str) -> Self {
        Self {
            provider: OAuthProvider::Microsoft,
            client_id: client_id.trim().to_owned(),
            client_secret: None,
            authorize_url: url(MICROSOFT_AUTHORIZE),
            token_url: url(MICROSOFT_TOKEN),
            revoke_url: None,
            scopes: strings(MICROSOFT_SCOPES),
            extra_params: Vec::new(),
            redirect_host: RedirectHost::Localhost,
            imap_hosts: strings(MICROSOFT_IMAP_HOSTS),
            smtp_hosts: strings(MICROSOFT_SMTP_HOSTS),
            allow_http_loopback: false,
        }
    }

    /// Gmail with the client ID (and the non-confidential secret) of a "Desktop app" OAuth
    /// client in a Google Cloud project.
    pub fn google(client_id: &str, client_secret: Option<&str>) -> Self {
        Self {
            provider: OAuthProvider::Google,
            client_id: client_id.trim().to_owned(),
            client_secret: client_secret
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            authorize_url: url(GOOGLE_AUTHORIZE),
            token_url: url(GOOGLE_TOKEN),
            revoke_url: Some(url(GOOGLE_REVOKE)),
            scopes: strings(GOOGLE_SCOPES),
            extra_params: vec![
                ("access_type".into(), "offline".into()),
                ("prompt".into(), "consent".into()),
            ],
            redirect_host: RedirectHost::Loopback,
            imap_hosts: strings(GOOGLE_IMAP_HOSTS),
            smtp_hosts: strings(GOOGLE_SMTP_HOSTS),
            allow_http_loopback: false,
        }
    }

    /// Whether a token of this provider may go to `host` for IMAP (`imap`) or SMTP.
    pub fn allows_host(&self, host: &str, imap: bool) -> bool {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        let list = if imap {
            &self.imap_hosts
        } else {
            &self.smtp_hosts
        };
        list.contains(&host)
    }

    /// Whether `url` may be contacted: HTTPS, or HTTP to a loopback address in tests.
    fn allows_endpoint(&self, url: &Url) -> bool {
        match url.scheme() {
            "https" => true,
            "http" => self.allow_http_loopback && token_http::is_loopback(url),
            _ => false,
        }
    }
}

/// The providers this build can sign in with: a profile per provider that has a client ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OAuthConfig {
    pub microsoft: Option<ProviderProfile>,
    pub google: Option<ProviderProfile>,
    /// How long the browser may take; [`SIGN_IN_TIMEOUT`] in the product.
    pub sign_in_timeout: Option<Duration>,
}

impl OAuthConfig {
    /// From the build configuration: a provider is available when its client ID is set and not
    /// empty. Google also needs the installed-app secret its token endpoint asks for.
    pub fn from_build(
        microsoft_client_id: Option<&str>,
        google_client_id: Option<&str>,
        google_client_secret: Option<&str>,
    ) -> Self {
        fn set(value: Option<&str>) -> Option<&str> {
            value.map(str::trim).filter(|v| !v.is_empty())
        }
        Self {
            microsoft: set(microsoft_client_id).map(ProviderProfile::microsoft),
            google: set(google_client_id).and_then(|id| {
                set(google_client_secret).map(|secret| ProviderProfile::google(id, Some(secret)))
            }),
            sign_in_timeout: None,
        }
    }

    pub fn profile(&self, provider: OAuthProvider) -> Option<&ProviderProfile> {
        match provider {
            OAuthProvider::Microsoft => self.microsoft.as_ref(),
            OAuthProvider::Google => self.google.as_ref(),
        }
    }

    pub fn availability(&self) -> OAuthAvailability {
        OAuthAvailability {
            microsoft: self.microsoft.is_some(),
            google: self.google.is_some(),
        }
    }
}

/// The sign-in service: configuration, the browser, the clock, the running sign-in and the
/// in-memory access tokens. Shared through [`crate::credentials::Credentials`].
pub struct OAuthService {
    config: OAuthConfig,
    browser: Arc<dyn SystemOpener>,
    clock: Arc<dyn Clock>,
    /// The running sign-in, if any: its number and its cancel switch. A new sign-in cancels
    /// the one before.
    active: Mutex<Option<(u64, tokio::sync::watch::Sender<bool>)>>,
    next_sign_in: std::sync::atomic::AtomicU64,
    pub(crate) tokens: tokens::TokenCache,
}

impl std::fmt::Debug for OAuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthService")
            .field("availability", &self.config.availability())
            .finish_non_exhaustive()
    }
}

impl OAuthService {
    /// The product service: the system's default browser and clock.
    pub fn new(config: OAuthConfig) -> Self {
        Self::with(config, Arc::new(DesktopOpener), Arc::new(SystemClock))
    }

    /// With another "browser" (tests drive the provider's page themselves) and clock.
    pub fn with(
        config: OAuthConfig,
        browser: Arc<dyn SystemOpener>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            browser,
            clock,
            active: Mutex::new(None),
            next_sign_in: std::sync::atomic::AtomicU64::new(1),
            tokens: tokens::TokenCache::default(),
        }
    }

    pub fn availability(&self) -> OAuthAvailability {
        self.config.availability()
    }

    pub(crate) fn profile(&self, provider: OAuthProvider) -> Result<&ProviderProfile, OAuthError> {
        self.config
            .profile(provider)
            .ok_or(OAuthError::Unavailable(provider))
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    fn sign_in_timeout(&self) -> Duration {
        self.config.sign_in_timeout.unwrap_or(SIGN_IN_TIMEOUT)
    }

    /// Registers a new sign-in and cancels the one before. Returns its number and cancel flag.
    fn begin(&self) -> (u64, tokio::sync::watch::Receiver<bool>) {
        let id = self
            .next_sign_in
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = tokio::sync::watch::channel(false);
        let previous = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace((id, tx));
        if let Some((_, previous)) = previous {
            let _ = previous.send(true);
        }
        (id, rx)
    }

    /// Forgets sign-in `id` once it ended (a newer one stays).
    fn end(&self, id: u64) {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if active.as_ref().is_some_and(|(current, _)| *current == id) {
            *active = None;
        }
    }

    /// Cancels the running sign-in; `false` if none was running.
    pub fn cancel(&self) -> bool {
        match self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            Some((_, tx)) => {
                let _ = tx.send(true);
                true
            }
            None => false,
        }
    }
}

/// Asks the provider to revoke the account's refresh token, where it offers that (Google;
/// Microsoft has no revocation for a public client, its tokens end with their lifetime or when
/// the user removes the app in the Microsoft account). Best effort before the account's entry is
/// deleted: a failure is logged and the removal goes on. Nothing for a password account.
pub(crate) async fn revoke(credentials: &Credentials, key: &CredentialRef) {
    let Some(provider) = key.oauth_provider() else {
        return;
    };
    let Some(profile) = credentials
        .oauth()
        .and_then(|service| service.profile(provider).ok())
    else {
        return;
    };
    if profile.revoke_url.is_none() {
        tracing::info!(
            provider = provider.as_str(),
            "the provider offers no revocation; the sign-in is deleted locally"
        );
        return;
    }
    match store::load(credentials, key).await {
        Ok(Some(token)) => match token_http::revoke(profile, &token).await {
            Ok(_) => tracing::info!(
                provider = provider.as_str(),
                "sign-in revoked at the provider"
            ),
            Err(err) => {
                tracing::warn!(provider = provider.as_str(), code = ?err.code(), error = %err, "revoking the sign-in failed; it is deleted locally");
            }
        },
        Ok(None) => {}
        Err(err) => tracing::warn!(code = ?err.code(), "stored sign-in unreadable; not revoked"),
    }
}
