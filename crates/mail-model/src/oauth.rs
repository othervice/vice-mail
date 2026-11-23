//! Stage 14, blocks B and C (decision 0042): sign-in with Microsoft and Google (OAuth 2.0 with
//! PKCE in the system browser) for IMAP and SMTP. The types here cross the IPC bridge or tell
//! the adapter how to log in; the flow, the tokens and the provider profiles live in
//! `mail-engine` (`oauth/`).

use serde::{Deserialize, Serialize};

use crate::{CoreError, ErrorCode};

/// A provider whose accounts sign in through its own login page (OAuth 2.0) instead of a
/// password. Stored as part of the account's keychain reference (`oauth:<provider>:…`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum OAuthProvider {
    /// Microsoft 365 and Outlook.com (Microsoft Entra, authority `common`).
    Microsoft,
    /// Gmail and Google Workspace.
    Google,
}

impl OAuthProvider {
    pub const ALL: [Self; 2] = [Self::Microsoft, Self::Google];

    /// Text form (keychain reference, settings sync record): `microsoft`, `google`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Microsoft => "microsoft",
            Self::Google => "google",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }

    /// Name for logs and error details (not shown as UI text).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Microsoft => "Microsoft",
            Self::Google => "Google",
        }
    }
}

/// Keychain references of accounts that sign in with a provider start with this, followed by
/// the provider and the account's own part: `oauth:microsoft:<uuid>`. The entry holds the
/// refresh token, never a password.
pub const OAUTH_CREDENTIAL_PREFIX: &str = "oauth:";

/// The provider of an account, read from its keychain reference; `None` for a password.
pub fn oauth_provider_of_credential_ref(credential_ref: &str) -> Option<OAuthProvider> {
    let rest = credential_ref.strip_prefix(OAUTH_CREDENTIAL_PREFIX)?;
    let (provider, id) = rest.split_once(':')?;
    if id.is_empty() {
        return None;
    }
    OAuthProvider::parse(provider)
}

/// How a connection proves who it is. With [`LoginAuth::Bearer`] the `password` field of
/// [`crate::ConnectConfig`] holds an OAuth 2.0 access token, sent with SASL `XOAUTH2` or
/// `OAUTHBEARER` (RFC 7628), never with LOGIN or PLAIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoginAuth {
    #[default]
    Password,
    Bearer,
}

/// Which sign-ins this build offers: a provider needs a client ID configured when the app was
/// built. Without one the setup hides the button and says that this build cannot sign in there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAvailability {
    pub microsoft: bool,
    pub google: bool,
}

impl OAuthAvailability {
    pub const fn has(self, provider: OAuthProvider) -> bool {
        match provider {
            OAuthProvider::Microsoft => self.microsoft,
            OAuthProvider::Google => self.google,
        }
    }
}

/// Longest text of [`OAuthPageTexts`] in characters.
pub const MAX_OAUTH_PAGE_TEXT_CHARS: usize = 300;

/// The texts of the page the browser shows when it comes back to the app (the UI's
/// translations; the core escapes them). Plain text, no markup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OAuthPageTexts {
    /// Title of the page (the app's name, for example).
    pub title: String,
    /// Shown when the sign-in went through: "You can close this window and return to the app."
    pub done: String,
    /// Shown when the provider refused or something failed.
    pub failed: String,
}

impl OAuthPageTexts {
    /// Plain English texts, for tests and when the UI sends empty ones.
    pub fn fallback() -> Self {
        Self {
            title: "Sign-in".into(),
            done: "You are signed in. You can close this window and return to the app.".into(),
            failed: "The sign-in did not work. Return to the app for details.".into(),
        }
    }

    /// Trimmed and shortened texts; empty ones fall back to English.
    pub fn cleaned(self) -> Self {
        let fallback = Self::fallback();
        let clean = |text: String, default: String| {
            let text: String = text
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_OAUTH_PAGE_TEXT_CHARS)
                .collect();
            if text.is_empty() { default } else { text }
        };
        Self {
            title: clean(self.title, fallback.title),
            done: clean(self.done, fallback.done),
            failed: clean(self.failed, fallback.failed),
        }
    }
}

/// A sign-in or a token refresh failed. Texts never contain a token, a code or the `state`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OAuthError {
    /// No client ID for this provider in this build.
    #[error("sign-in with {} is not available in this build", .0.name())]
    Unavailable(OAuthProvider),
    /// The user cancelled the sign-in in the app.
    #[error("the sign-in was cancelled")]
    Cancelled,
    /// The browser did not come back within the time limit.
    #[error("no answer from the browser within the time limit")]
    TimedOut,
    /// The provider or the user refused (`access_denied`, missing consent, blocked app).
    #[error("the sign-in was refused: {0}")]
    Denied(String),
    /// Anything else: the browser could not be opened, the provider's answer was not
    /// understood, the token endpoint refused, or the account's server is not the provider's.
    #[error("the sign-in failed: {0}")]
    Failed(String),
    /// The stored permission has expired or was revoked (`invalid_grant`): the user has to
    /// sign in again. The account stops syncing until then.
    #[error("the sign-in has expired or was revoked: {0}")]
    SignInRequired(String),
}

impl OAuthError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Unavailable(_) => ErrorCode::OauthUnavailable,
            Self::Cancelled => ErrorCode::OauthCancelled,
            Self::TimedOut => ErrorCode::OauthTimeout,
            Self::Denied(_) => ErrorCode::OauthDenied,
            Self::Failed(_) => ErrorCode::OauthFailed,
            Self::SignInRequired(_) => ErrorCode::OauthSignInRequired,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "Signing in with this provider is not available in this build.",
            Self::Cancelled => "The sign-in was cancelled.",
            Self::TimedOut => "The sign-in in the browser was not finished in time.",
            Self::Denied(_) => "The provider did not allow the sign-in.",
            Self::Failed(_) => "The sign-in failed.",
            Self::SignInRequired(_) => {
                "The permission for this account has expired or was revoked. Sign in again."
            }
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Unavailable(provider) => Some(provider.name().to_owned()),
            Self::Cancelled | Self::TimedOut => None,
            Self::Denied(d) | Self::Failed(d) | Self::SignInRequired(d) => {
                (!d.is_empty()).then(|| d.clone())
            }
        }
    }
}

impl From<OAuthError> for CoreError {
    fn from(err: OAuthError) -> Self {
        Self::OAuth(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IpcError;

    #[test]
    fn providers_round_trip_their_text_form() {
        for provider in OAuthProvider::ALL {
            assert_eq!(OAuthProvider::parse(provider.as_str()), Some(provider));
            assert_eq!(
                serde_json::to_value(provider).expect("serialize"),
                provider.as_str()
            );
        }
        assert_eq!(OAuthProvider::parse("Microsoft"), None);
        assert_eq!(OAuthProvider::parse(""), None);
    }

    #[test]
    fn keychain_references_name_the_provider() {
        assert_eq!(
            oauth_provider_of_credential_ref("oauth:microsoft:7d1c"),
            Some(OAuthProvider::Microsoft)
        );
        assert_eq!(
            oauth_provider_of_credential_ref("oauth:google:synced:ab12"),
            Some(OAuthProvider::Google)
        );
        for password in [
            "7d1c-uuid",
            "synced:ab12",
            "oauth:microsoft:",
            "oauth:yahoo:1",
            "oauth:",
            "mcp-client:x",
        ] {
            assert_eq!(
                oauth_provider_of_credential_ref(password),
                None,
                "{password}"
            );
        }
    }

    #[test]
    fn errors_cross_the_bridge_with_their_codes() {
        let cases = [
            (
                OAuthError::Unavailable(OAuthProvider::Google),
                "oauth_unavailable",
            ),
            (OAuthError::Cancelled, "oauth_cancelled"),
            (OAuthError::TimedOut, "oauth_timeout"),
            (OAuthError::Denied("access_denied".into()), "oauth_denied"),
            (OAuthError::Failed("x".into()), "oauth_failed"),
            (
                OAuthError::SignInRequired("invalid_grant".into()),
                "oauth_sign_in_required",
            ),
        ];
        for (err, code) in cases {
            let ipc = IpcError::from(CoreError::from(err));
            assert_eq!(serde_json::to_value(ipc.code).expect("code"), code);
            assert!(!ipc.message.is_empty());
        }
        assert!(!CoreError::from(OAuthError::SignInRequired(String::new())).is_transient());
    }

    #[test]
    fn page_texts_are_cleaned() {
        let texts = OAuthPageTexts {
            title: "  Mail\u{7} ".into(),
            done: String::new(),
            failed: "x".repeat(1000),
        }
        .cleaned();
        assert_eq!(texts.title, "Mail");
        assert_eq!(texts.done, OAuthPageTexts::fallback().done);
        assert_eq!(texts.failed.chars().count(), MAX_OAUTH_PAGE_TEXT_CHARS);
    }
}
