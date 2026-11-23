//! The provider's token endpoint (RFC 6749, sections 4.1.3 and 6) and revocation (RFC 7009),
//! over the app's HTTP rules: the app's rustls setup with the system trust store, HTTP/1.1, no
//! redirects, no proxy, no cookies, no retries, a neutral user agent, time and size limits.
//! Only the profile's own endpoints; HTTPS, or HTTP to a loopback address in tests.
//!
//! Tokens and codes travel only in request bodies and are never part of an error text: errors
//! carry the provider's `error` code and a shortened `error_description`.

use std::sync::Arc;
use std::time::Duration;

use mail_model::{BackendError, CoreError, OAuthError, Secret};
use url::Url;

use super::ProviderProfile;
use crate::discovery::autoconfig::NEUTRAL_USER_AGENT;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Revocation runs while an account is removed; it must not hold that up for long.
const REVOKE_TIMEOUT: Duration = Duration::from_secs(10);
/// A token response is a few kilobytes.
const MAX_RESPONSE: usize = 64 * 1024;
const MAX_DESCRIPTION: usize = 300;
/// Access tokens without `expires_in` count as valid for an hour (both providers send it).
const DEFAULT_LIFETIME_SECS: u64 = 3600;

/// A successful token response.
pub(crate) struct TokenResponse {
    pub access_token: Secret<String>,
    /// `None` when the provider keeps the old one (Google on refresh).
    pub refresh_token: Option<Secret<String>>,
    pub expires_in_secs: u64,
}

/// Which request a refusal belongs to (it decides what `invalid_grant` means).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grant {
    Code,
    Refresh,
}

pub(crate) fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn client(profile: &ProviderProfile, timeout: Duration) -> Result<reqwest::Client, CoreError> {
    let tls = mail_imap::tls::client_config(None).map_err(CoreError::Mail)?;
    reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
        .https_only(!profile.allow_http_loopback)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .retry(reqwest::retry::never())
        .user_agent(NEUTRAL_USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT.min(timeout))
        .timeout(timeout)
        .build()
        .map_err(|err| CoreError::Internal(format!("HTTP client for the sign-in: {err}")))
}

/// Exchanges the authorization code (with the PKCE verifier) for tokens.
pub(crate) async fn exchange_code(
    profile: &ProviderProfile,
    code: &Secret<String>,
    verifier: &Secret<String>,
    redirect_uri: &str,
) -> Result<TokenResponse, CoreError> {
    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code.expose_secret()),
        ("redirect_uri", redirect_uri),
        ("client_id", &profile.client_id),
        ("code_verifier", verifier.expose_secret()),
    ];
    if let Some(secret) = &profile.client_secret {
        pairs.push(("client_secret", secret));
    }
    let body = form(&pairs);
    let response = post_token(profile, body, Grant::Code).await?;
    if response.refresh_token.is_none() {
        // Without one the account would stop working after an hour.
        return Err(OAuthError::Failed(
            "the provider sent no refresh token (offline access was not granted)".into(),
        )
        .into());
    }
    Ok(response)
}

/// A new access token for the refresh token.
pub(crate) async fn refresh(
    profile: &ProviderProfile,
    refresh_token: &Secret<String>,
) -> Result<TokenResponse, CoreError> {
    let scope = profile.scopes.join(" ");
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.expose_secret()),
        ("client_id", &profile.client_id),
        ("scope", &scope),
    ];
    if let Some(secret) = &profile.client_secret {
        pairs.push(("client_secret", secret));
    }
    let body = form(&pairs);
    post_token(profile, body, Grant::Refresh).await
}

/// Asks the provider to revoke `token` (the refresh token; Google revokes the whole grant).
/// `Ok(false)` where the profile has no revocation endpoint.
pub(crate) async fn revoke(
    profile: &ProviderProfile,
    token: &Secret<String>,
) -> Result<bool, CoreError> {
    let Some(url) = &profile.revoke_url else {
        return Ok(false);
    };
    if !profile.allows_endpoint(url) {
        return Err(OAuthError::Failed("the revocation address is not allowed".into()).into());
    }
    // RFC 7009, section 2.1: a public client identifies itself with its client id.
    let mut pairs = vec![
        ("token", token.expose_secret().as_str()),
        ("token_type_hint", "refresh_token"),
        ("client_id", &profile.client_id),
    ];
    if let Some(secret) = &profile.client_secret {
        pairs.push(("client_secret", secret));
    }
    let body = form(&pairs);
    let response = client(profile, REVOKE_TIMEOUT)?
        .post(url.clone())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body.expose_secret().clone())
        .send()
        .await
        .map_err(network)?;
    let status = response.status();
    if status.is_success() {
        Ok(true)
    } else {
        let bytes = read_limited(response).await.unwrap_or_default();
        Err(OAuthError::Failed(format!(
            "revocation answered {status}{}",
            provider_error(&bytes).map_or_else(String::new, |(e, _)| format!(": {e}"))
        ))
        .into())
    }
}

/// An `application/x-www-form-urlencoded` body (it holds a code or a token).
fn form(pairs: &[(&str, &str)]) -> Secret<String> {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        form.append_pair(key, value);
    }
    Secret::new(form.finish())
}

async fn post_token(
    profile: &ProviderProfile,
    body: Secret<String>,
    grant: Grant,
) -> Result<TokenResponse, CoreError> {
    if !profile.allows_endpoint(&profile.token_url) {
        return Err(OAuthError::Failed("the token address is not allowed".into()).into());
    }
    let response = client(profile, REQUEST_TIMEOUT)?
        .post(profile.token_url.clone())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .body(body.expose_secret().clone())
        .send()
        .await
        .map_err(network)?;
    let status = response.status();
    let bytes = read_limited(response).await?;
    if status.is_server_error() {
        return Err(CoreError::Mail(BackendError::ConnectionLost(format!(
            "the sign-in server answered {status}"
        ))));
    }
    if !status.is_success() {
        let (error, description) =
            provider_error(&bytes).unwrap_or_else(|| (format!("status {}", status.as_u16()), None));
        return Err(refusal(grant, &error, description.as_deref()).into());
    }
    parse_tokens(&bytes)
}

/// What a refused token request means (RFC 6749, section 5.2).
fn refusal(grant: Grant, error: &str, description: Option<&str>) -> OAuthError {
    let detail = match description {
        Some(description) => format!("{error}: {description}"),
        None => error.to_owned(),
    };
    match (grant, error) {
        // The refresh token expired, was revoked, or the consent was withdrawn.
        (Grant::Refresh, "invalid_grant" | "interaction_required" | "consent_required") => {
            OAuthError::SignInRequired(detail)
        }
        (_, "access_denied" | "unauthorized_client") => OAuthError::Denied(detail),
        _ => OAuthError::Failed(detail),
    }
}

fn parse_tokens(bytes: &[u8]) -> Result<TokenResponse, CoreError> {
    let unreadable = || OAuthError::Failed("the token answer was not understood".into());
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| unreadable())?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let access_token = text("access_token").ok_or_else(unreadable)?;
    let token_type = text("token_type").unwrap_or_default();
    if !token_type.eq_ignore_ascii_case("bearer") {
        return Err(OAuthError::Failed(format!(
            "the provider sent a token of type {:?}, not Bearer",
            token_type.chars().take(20).collect::<String>()
        ))
        .into());
    }
    let expires_in_secs = match value.get("expires_in") {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(DEFAULT_LIFETIME_SECS),
        // Some servers send it as a string.
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(DEFAULT_LIFETIME_SECS),
        _ => DEFAULT_LIFETIME_SECS,
    };
    Ok(TokenResponse {
        access_token: Secret::new(access_token),
        refresh_token: text("refresh_token").map(Secret::new),
        expires_in_secs,
    })
}

/// `error` and a shortened `error_description` from an error answer.
fn provider_error(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let error = value.get("error")?.as_str()?;
    let clean = |s: &str, max: usize| -> String {
        s.chars().filter(|c| !c.is_control()).take(max).collect()
    };
    let description = value
        .get("error_description")
        .and_then(serde_json::Value::as_str)
        .map(|d| clean(d, MAX_DESCRIPTION));
    Some((clean(error, 64), description))
}

async fn read_limited(mut response: reqwest::Response) -> Result<Vec<u8>, CoreError> {
    let too_large = || CoreError::from(OAuthError::Failed("the token answer is too large".into()));
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network)? {
        if body.len() + chunk.len() > MAX_RESPONSE {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A transport failure: transient, like a mail server that cannot be reached.
fn network(err: reqwest::Error) -> CoreError {
    let err = err.without_url();
    let mut text = err.to_string();
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    let text = format!("sign-in server: {text}");
    CoreError::Mail(if err.is_timeout() {
        BackendError::Timeout(text)
    } else {
        BackendError::ConnectionLost(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_answers_are_read_strictly() {
        let ok = parse_tokens(
            br#"{"access_token":"at","token_type":"Bearer","expires_in":3599,"refresh_token":"rt"}"#,
        )
        .expect("tokens");
        assert_eq!(ok.access_token.expose_secret(), "at");
        assert_eq!(ok.expires_in_secs, 3599);
        assert!(ok.refresh_token.is_some());
        let google =
            parse_tokens(br#"{"access_token":"at","token_type":"bearer","expires_in":"60"}"#)
                .expect("tokens");
        assert_eq!(google.expires_in_secs, 60);
        assert!(google.refresh_token.is_none());
        for bad in [
            &br#"{"token_type":"Bearer"}"#[..],
            br#"{"access_token":"at","token_type":"mac"}"#,
            br#"{"access_token":"","token_type":"Bearer"}"#,
            b"not json",
        ] {
            let err = parse_tokens(bad).err().expect("refused");
            assert_eq!(err.code(), mail_model::ErrorCode::OauthFailed, "{err}");
            assert!(!err.to_string().contains("\"at\""));
        }
    }

    #[test]
    fn refusals_mean_sign_in_again_only_for_a_dead_refresh_token() {
        assert!(matches!(
            refusal(Grant::Refresh, "invalid_grant", Some("AADSTS70008: expired")),
            OAuthError::SignInRequired(d) if d.contains("AADSTS70008")
        ));
        assert!(matches!(
            refusal(Grant::Code, "invalid_grant", None),
            OAuthError::Failed(_)
        ));
        assert!(matches!(
            refusal(Grant::Code, "unauthorized_client", None),
            OAuthError::Denied(_)
        ));
        assert!(matches!(
            refusal(Grant::Refresh, "invalid_client", None),
            OAuthError::Failed(_)
        ));
        assert_eq!(
            provider_error(br#"{"error":"invalid_grant","error_description":"Bad\nRequest"}"#),
            Some(("invalid_grant".into(), Some("BadRequest".into())))
        );
    }

    #[test]
    fn loopback_addresses() {
        for (url, loopback) in [
            ("http://127.0.0.1:8080/token", true),
            ("http://[::1]/token", true),
            ("http://localhost:1/x", true),
            ("http://192.168.1.1/token", false),
            ("https://login.microsoftonline.com/x", false),
        ] {
            assert_eq!(
                is_loopback(&Url::parse(url).expect("url")),
                loopback,
                "{url}"
            );
        }
    }
}
