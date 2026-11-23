//! The interactive sign-in: authorization code with PKCE in the system browser (RFC 6749
//! section 4.1, RFC 7636, RFC 8252). See the module docs of `oauth` for the rules.

use std::sync::Arc;

use mail_model::{CoreError, OAuthError, OAuthPageTexts, OAuthProvider, Secret};
use tokio::time::Instant;

use super::loopback::{Callback, Listener};
use super::{OAuthService, ProviderProfile, RedirectHost, pkce, token_http, tokens};

/// A finished sign-in: the tokens, in memory until the account is stored.
pub struct SignInGrant {
    pub provider: OAuthProvider,
    pub(crate) refresh_token: Secret<String>,
    pub(crate) access_token: Secret<String>,
    /// When the access token is renewed (Unix milliseconds).
    pub(crate) renew_at: i64,
}

impl std::fmt::Debug for SignInGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInGrant")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

impl OAuthService {
    /// Signs in with `provider` in the system browser. `login_hint` (the address) preselects
    /// the account on the provider's page. A second sign-in cancels a running one.
    pub(crate) async fn sign_in(
        &self,
        provider: OAuthProvider,
        login_hint: &str,
        texts: OAuthPageTexts,
    ) -> Result<SignInGrant, CoreError> {
        let profile = self.profile(provider)?.clone();
        if !profile.allows_endpoint(&profile.authorize_url)
            || !profile.allows_endpoint(&profile.token_url)
        {
            return Err(OAuthError::Failed("the sign-in addresses are not allowed".into()).into());
        }
        let (id, cancel) = self.begin();
        tracing::info!(provider = provider.as_str(), "sign-in started");
        let result = self
            .run(&profile, login_hint, &texts.cleaned(), cancel)
            .await;
        self.end(id);
        match &result {
            Ok(_) => tracing::info!(provider = provider.as_str(), "sign-in finished"),
            Err(err) => {
                tracing::info!(provider = provider.as_str(), code = ?err.code(), "sign-in ended without tokens");
            }
        }
        result
    }

    async fn run(
        &self,
        profile: &ProviderProfile,
        login_hint: &str,
        texts: &OAuthPageTexts,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<SignInGrant, CoreError> {
        let verifier = pkce::random_token()?;
        let state = pkce::random_token()?;
        let listener = Listener::bind(profile.redirect_host == RedirectHost::Localhost).await?;
        // Exactly as registered at the provider, plus the port (which loopback redirects may
        // choose freely, RFC 8252 section 7.3): Microsoft registers `http://localhost`, Google's
        // and RFC 8252's form is the loopback address with the root path.
        let redirect_uri = match profile.redirect_host {
            RedirectHost::Loopback => format!("http://127.0.0.1:{}/", listener.port()),
            RedirectHost::Localhost => format!("http://localhost:{}", listener.port()),
        };
        let url = authorize_url(profile, &redirect_uri, &state, &verifier, login_hint);

        // The system's default browser, never the WebView (the same opener as links).
        let browser = Arc::clone(&self.browser);
        let target = Secret::new(url.to_string());
        tokio::task::spawn_blocking(move || browser.open_url(target.expose_secret()))
            .await
            .map_err(|err| OAuthError::Failed(format!("the browser could not be opened: {err}")))?
            .map_err(|err| OAuthError::Failed(format!("the browser could not be opened: {err}")))?;

        let deadline = Instant::now() + self.sign_in_timeout();
        let callback = listener.wait(&state, texts, cancel, deadline).await?;
        let code = match callback {
            Callback::Code(code) => code,
            Callback::Error { error, description } => {
                let detail = match description {
                    Some(description) => format!("{error}: {description}"),
                    None => error.clone(),
                };
                return Err(match error.as_str() {
                    "access_denied" | "consent_required" | "unauthorized_client" => {
                        OAuthError::Denied(detail)
                    }
                    _ => OAuthError::Failed(detail),
                }
                .into());
            }
        };
        let response = token_http::exchange_code(profile, &code, &verifier, &redirect_uri).await?;
        let refresh_token = response
            .refresh_token
            .ok_or_else(|| OAuthError::Failed("the provider sent no refresh token".into()))?;
        Ok(SignInGrant {
            provider: profile.provider,
            refresh_token,
            access_token: response.access_token,
            renew_at: tokens::renew_at(self.now_ms(), response.expires_in_secs),
        })
    }
}

/// The provider's authorization address with this sign-in's parameters.
fn authorize_url(
    profile: &ProviderProfile,
    redirect_uri: &str,
    state: &Secret<String>,
    verifier: &Secret<String>,
    login_hint: &str,
) -> url::Url {
    let mut url = profile.authorize_url.clone();
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &profile.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &profile.scopes.join(" "))
            .append_pair("state", state.expose_secret())
            .append_pair("code_challenge", &pkce::challenge(verifier))
            .append_pair("code_challenge_method", "S256");
        let hint = login_hint.trim();
        if !hint.is_empty() && hint.len() <= 254 {
            query.append_pair("login_hint", hint);
        }
        for (key, value) in &profile.extra_params {
            query.append_pair(key, value);
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorization_address_carries_pkce_state_and_scopes_but_never_the_verifier() {
        let profile = ProviderProfile::microsoft("client-123");
        let state = Secret::new("st".to_owned());
        let verifier = Secret::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_owned());
        let url = authorize_url(
            &profile,
            "http://localhost:50123",
            &state,
            &verifier,
            "alice@outlook.com",
        );
        assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
        assert_eq!(url.path(), "/common/oauth2/v2.0/authorize");
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("client_id"), Some("client-123"));
        assert_eq!(get("response_type"), Some("code"));
        assert_eq!(get("redirect_uri"), Some("http://localhost:50123"));
        assert_eq!(
            get("scope"),
            Some(
                "https://outlook.office.com/IMAP.AccessAsUser.All \
                 https://outlook.office.com/SMTP.Send offline_access"
            )
        );
        assert_eq!(get("state"), Some("st"));
        assert_eq!(
            get("code_challenge"),
            Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        );
        assert_eq!(get("code_challenge_method"), Some("S256"));
        assert_eq!(get("login_hint"), Some("alice@outlook.com"));
        assert!(!url.as_str().contains(verifier.expose_secret()));

        let google = ProviderProfile::google("id.apps.googleusercontent.com", Some("GOCSPX-x"));
        let url = authorize_url(&google, "http://127.0.0.1:1", &state, &verifier, "");
        assert!(
            url.as_str()
                .starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
        );
        assert!(
            url.as_str()
                .contains("scope=https%3A%2F%2Fmail.google.com%2F")
        );
        assert!(url.as_str().contains("access_type=offline"));
        assert!(url.as_str().contains("prompt=consent"));
        assert!(!url.as_str().contains("login_hint"));
        // The client "secret" is only for the token endpoint.
        assert!(!url.as_str().contains("GOCSPX"));
    }
}
