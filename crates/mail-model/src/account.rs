//! Accounts, identities, server settings and the per-account sync status shown in the UI.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{AccountId, Capabilities, CoreError, IdentityId, IpcError, MailboxId};

/// Transport security of a server connection. There is deliberately no plaintext option: a
/// password never travels unencrypted, and the test server offers TLS and STARTTLS as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Tls,
    /// Plain connect, then `STARTTLS` before anything else (IMAP 143, SMTP 587). If the server
    /// does not offer STARTTLS, the connection fails; it never continues unencrypted.
    Starttls,
}

impl Security {
    /// Text form in the database (`'tls'`, `'starttls'`), same as on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::Starttls => "starttls",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tls" => Some(Self::Tls),
            "starttls" => Some(Self::Starttls),
            _ => None,
        }
    }
}

/// How the app talks to an account's server (stage 14, block D, decision 0041). Fixed when the
/// account is set up.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// IMAP for reading, SMTP for sending: the account's `imap` and `smtp` servers.
    #[default]
    Imap,
    /// JMAP (RFC 8620, RFC 8621) over HTTPS for reading, changes and sending: the account's
    /// `imap` server settings name the JMAP server (host, port, user name; always TLS), whose
    /// session resource is `https://<host>[:<port>]/.well-known/jmap`. `smtp` holds the same
    /// server and is not used.
    Jmap,
}

impl Protocol {
    /// Text form in the database and on the wire (`'imap'`, `'jmap'`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Jmap => "jmap",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "imap" => Some(Self::Imap),
            "jmap" => Some(Self::Jmap),
            _ => None,
        }
    }

    pub const fn is_imap(&self) -> bool {
        matches!(self, Self::Imap)
    }
}

/// What the secret of an account is (stage 14). A password (or an app password) by default;
/// JMAP servers may take an API token instead, which the adapter sends as
/// `Authorization: Bearer` (Fastmail's JMAP API wants one). Fixed when the account is set up;
/// kept in the keychain like a password, under a reference that starts with
/// [`API_TOKEN_CREDENTIAL_PREFIX`].
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    #[default]
    Password,
    /// An API token for a JMAP server (only with [`Protocol::Jmap`]).
    ApiToken,
}

impl CredentialKind {
    pub const fn is_password(&self) -> bool {
        matches!(self, Self::Password)
    }

    /// Text form in settings sync records (`password`, `apiToken`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::ApiToken => "apiToken",
        }
    }

    /// The kind of the secret behind a keychain reference.
    pub fn of_credential_ref(credential_ref: &str) -> Self {
        if is_api_token_credential_ref(credential_ref) {
            Self::ApiToken
        } else {
            Self::Password
        }
    }
}

/// Keychain references of accounts whose secret is an API token start with this, followed by
/// the account's own part: `token:<uuid>`, or `token:synced:<hex>` for an account taken over
/// from the settings sync.
pub const API_TOKEN_CREDENTIAL_PREFIX: &str = "token:";

/// Whether a keychain reference holds an API token (see [`CredentialKind::ApiToken`]).
pub fn is_api_token_credential_ref(credential_ref: &str) -> bool {
    credential_ref
        .strip_prefix(API_TOKEN_CREDENTIAL_PREFIX)
        .is_some_and(|rest| !rest.is_empty())
}

/// Where and how to reach one server (IMAP or SMTP). No password: that lives in the keychain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
}

/// What the user enters to set up an account (input of `testConnection` and `addAccount`).
/// The password is a separate command argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettings {
    pub email: String,
    /// Sender name; may be empty.
    pub display_name: String,
    pub imap: ServerSettings,
    /// Stored for stage 5 (sending). Stage 2 checks it for plausibility only.
    pub smtp: ServerSettings,
    /// Stage 14 (decision 0041): IMAP and SMTP unless the user chose JMAP in the advanced
    /// options. With JMAP, `imap` names the JMAP server and `smtp` is replaced by a copy of it.
    #[serde(default, skip_serializing_if = "Protocol::is_imap")]
    #[specta(optional)]
    pub protocol: Protocol,
    /// Stage 14: what the secret argument is. An API token only for JMAP; left out for a
    /// password.
    #[serde(default, skip_serializing_if = "CredentialKind::is_password")]
    #[specta(optional)]
    pub credential: CredentialKind,
}

impl AccountSettings {
    /// Plausibility check before anything is stored or connected. Returns the settings with
    /// surrounding whitespace removed from every text field.
    pub fn validated(mut self) -> Result<Self, CoreError> {
        fn trim(s: &mut String) {
            let t = s.trim();
            if t.len() != s.len() {
                *s = t.to_owned();
            }
        }
        trim(&mut self.email);
        trim(&mut self.display_name);
        if self.protocol == Protocol::Jmap {
            // One server for everything; JMAP runs over HTTPS only.
            if self.imap.security != Security::Tls {
                return Err(CoreError::InvalidArgument(
                    "JMAP needs TLS (HTTPS), not STARTTLS".into(),
                ));
            }
            self.smtp = self.imap.clone();
        } else if self.credential == CredentialKind::ApiToken {
            return Err(CoreError::InvalidArgument(
                "an API token only works with JMAP; IMAP and SMTP take a password".into(),
            ));
        }
        let at = self.email.find('@');
        if !at.is_some_and(|i| i > 0 && i + 1 < self.email.len()) || self.email.contains(' ') {
            return Err(CoreError::InvalidArgument(format!(
                "email address {:?} is not valid",
                self.email
            )));
        }
        for (label, server) in [("IMAP", &mut self.imap), ("SMTP", &mut self.smtp)] {
            trim(&mut server.host);
            trim(&mut server.username);
            // A colon means "host:port" typed into the host field; IPv6 literals are bracketed.
            let colon = server.host.contains(':') && !server.host.starts_with('[');
            if server.host.is_empty() || server.host.contains([' ', '/']) || colon {
                return Err(CoreError::InvalidArgument(format!(
                    "{label} host {:?} is not valid",
                    server.host
                )));
            }
            if server.port == 0 {
                return Err(CoreError::InvalidArgument(format!("{label} port is 0")));
            }
            if server.username.is_empty() {
                return Err(CoreError::InvalidArgument(format!(
                    "{label} user name is empty"
                )));
            }
        }
        Ok(self)
    }
}

/// Index 0–7 into the eight account color tokens (`docs/DESIGN.md`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct AccountColor(u8);

impl AccountColor {
    pub const COUNT: u8 = 8;

    pub const fn new(index: u8) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn index(self) -> u8 {
        self.0
    }

    /// Color for a new account: the lowest index no account uses, otherwise the least used one.
    pub fn next_free(used: &[AccountColor]) -> Self {
        let mut counts = [0usize; Self::COUNT as usize];
        for color in used {
            if let Some(n) = counts.get_mut(usize::from(color.0)) {
                *n += 1;
            }
        }
        let mut best = 0u8;
        for (i, n) in (0u8..).zip(counts) {
            if counts.get(usize::from(best)).is_some_and(|b| n < *b) {
                best = i;
            }
        }
        Self(best)
    }
}

/// A configured account as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: AccountId,
    pub email: String,
    pub display_name: String,
    pub color: AccountColor,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
    /// Stage 14 (decision 0042): the provider this account signs in with instead of a
    /// password (Microsoft, Google); `null` for a password.
    pub oauth: Option<crate::OAuthProvider>,
    /// Stage 14: `apiToken` for a JMAP account that signs in with an API token; left out for a
    /// password (and for accounts that sign in with a provider).
    #[serde(skip_serializing_if = "CredentialKind::is_password")]
    #[specta(optional)]
    pub credential: CredentialKind,
    /// `false` for every new account. Only the user turns it on (stage 4); until then the
    /// adapter opens mailboxes read-only and refuses every writing command.
    pub writes_enabled: bool,
    pub status: AccountStatus,
    /// Stage 14 (decision 0041): IMAP/SMTP or JMAP; left out for IMAP.
    #[serde(skip_serializing_if = "Protocol::is_imap")]
    #[specta(optional)]
    pub protocol: Protocol,
}

/// A sender identity of an account. Stage 2 creates one default identity per account
/// (address and display name of the account). Not exported to the UI before stage 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: IdentityId,
    pub account_id: AccountId,
    pub email: String,
    pub name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
}

/// Coarse sync state of an account, for the status indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum AccountState {
    /// First connection attempt after start or after the user asked to retry.
    Connecting,
    /// A sync pass is running; `progress` is set during the first sync of a mailbox.
    Syncing,
    /// Connected and up to date; waiting for changes (IDLE or poll interval).
    Idle,
    /// Not connected; retrying with backoff. `error` says why, `retryAt` when.
    Offline,
    /// Stopped until the user acts (wrong password, untrusted certificate, unsupported server).
    Error,
}

/// Progress of the first sync of one mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub mailbox_id: MailboxId,
    /// Messages of this mailbox stored locally so far.
    pub done: u32,
    /// Messages in this mailbox on the server (EXISTS when the pass started).
    pub total: u32,
}

/// Sync status of one account. Invariants by `state`:
/// `syncing` may carry `progress`; `offline` carries `error` (null only before the first
/// failure) and `retryAt` (null while a reconnect is running); `error` always carries `error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub state: AccountState,
    pub progress: Option<SyncProgress>,
    pub error: Option<IpcError>,
    /// Unix milliseconds of the next reconnect attempt.
    #[specta(type = Option<Number>)]
    pub retry_at: Option<i64>,
    /// Unix milliseconds when the last complete sync pass finished, in any state.
    #[specta(type = Option<Number>)]
    pub last_sync_at: Option<i64>,
}

impl AccountStatus {
    /// Status of an account whose sync task has not reported anything yet.
    pub const fn connecting() -> Self {
        Self {
            state: AccountState::Connecting,
            progress: None,
            error: None,
            retry_at: None,
            last_sync_at: None,
        }
    }
}

/// Where proposed server settings came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DiscoverySource {
    /// Built-in table of well-known providers (no network).
    KnownProvider,
    /// DNS SRV records (`_imaps._tcp`, `_submission._tcp`, RFC 6186).
    Srv,
    /// The domain's own autoconfig file (`autoconfig.<domain>`).
    Autoconfig,
    /// Nothing found; the user fills in the form.
    None,
}

/// Provider-specific advice for the setup form. The UI shows one note per hint; the server
/// settings are filled in either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ProviderHint {
    /// Needs an app password (Google account with 2-step verification).
    Gmail,
    /// Needs an app-specific password (Apple ID).
    Icloud,
    /// Needs an app password (Yahoo account security settings).
    Yahoo,
    /// Needs an app password (Fastmail settings, Privacy & Security).
    Fastmail,
    /// Outlook.com, Hotmail, Live: Microsoft only accepts OAuth sign-in, which the app supports
    /// from stage 14. A password login is expected to fail.
    Outlook,
    /// Proton Mail works only through Proton Mail Bridge on this computer (paid plan): use the
    /// user name and password that Bridge shows, and trust Bridge's certificate in the system.
    Proton,
    /// GMX: IMAP access must be turned on first (web mail settings, POP3/IMAP).
    Gmx,
    /// WEB.DE: IMAP access must be turned on first (web mail settings, POP3/IMAP).
    Webde,
    /// Telekom (t-online.de, magenta.de): needs the separate email password from the Telekom
    /// customer center, not the Telekom login password.
    Tonline,
}

/// Result of server discovery for an email address. Either server may be unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult {
    pub source: DiscoverySource,
    pub imap: Option<ServerSettings>,
    pub smtp: Option<ServerSettings>,
    pub provider_hint: Option<ProviderHint>,
    /// Stage 14 (decision 0041): a JMAP server found for the address (its session resource
    /// answered at `https://<host>/.well-known/jmap`, or the domain's autoconfig lists one).
    /// The setup form offers it as a protocol choice; IMAP stays the default. Left out when
    /// none was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub jmap: Option<ServerSettings>,
    /// Security review N-4 (RFC 6186, section 6): found servers that deserve a second look
    /// before the account is added. Only the online search sets them, never the built-in table.
    /// Left out when there is none.
    #[serde(skip_serializing_if = "ServerWarnings::is_empty")]
    #[specta(optional)]
    pub warnings: ServerWarnings,
}

/// Why a server the online search found deserves a second look (security review N-4,
/// RFC 6186 section 6): DNS and autoconfig answers are not verified, so a network attacker
/// during setup could propose his own server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ServerWarning {
    /// The host is neither the address's domain nor one of its subdomains (and not a server of
    /// the built-in provider table).
    OutsideDomain,
    /// The host is an IP address (IPv4 or IPv6), not a name.
    IpAddress,
}

/// At most one [`ServerWarning`] per server of a [`DiscoveryResult`]; an IP address is not
/// also reported as outside the domain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerWarnings {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub imap: Option<ServerWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub smtp: Option<ServerWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub jmap: Option<ServerWarning>,
}

impl ServerWarnings {
    /// No warning for any server.
    pub const NONE: Self = Self {
        imap: None,
        smtp: None,
        jmap: None,
    };

    pub const fn is_empty(&self) -> bool {
        self.imap.is_none() && self.smtp.is_none() && self.jmap.is_none()
    }
}

/// A successful connection test. Failures arrive as `IpcError` with a specific code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    /// What the IMAP server offers after login, for hints such as "no IDLE: new mail appears
    /// with a delay".
    pub capabilities: Capabilities,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn settings() -> AccountSettings {
        let server = |host: &str, port, security| ServerSettings {
            host: host.into(),
            port,
            security,
            username: " alice@example.org ".into(),
        };
        AccountSettings {
            credential: Default::default(),
            email: " alice@example.org".into(),
            display_name: "Alice ".into(),
            imap: server("imap.example.org", 993, Security::Tls),
            smtp: server("smtp.example.org ", 587, Security::Starttls),
            protocol: Protocol::Imap,
        }
    }

    #[test]
    fn validation_trims_and_rejects_implausible_values() {
        let ok = settings().validated().expect("valid");
        assert_eq!(ok.email, "alice@example.org");
        assert_eq!(ok.display_name, "Alice");
        assert_eq!(ok.smtp.host, "smtp.example.org");
        assert_eq!(ok.imap.username, "alice@example.org");

        let cases: [fn(&mut AccountSettings); 6] = [
            |s| s.email = "alice".into(),
            |s| s.email = "@example.org".into(),
            |s| s.email = "alice@".into(),
            |s| s.imap.host = "imap.example.org:993".into(),
            |s| s.smtp.port = 0,
            |s| s.imap.username = "  ".into(),
        ];
        for break_it in cases {
            let mut s = settings();
            break_it(&mut s);
            let err = s.validated().expect_err("invalid");
            assert_eq!(err.code(), crate::ErrorCode::InvalidArgument, "{err}");
        }
    }

    #[test]
    fn jmap_settings_use_one_tls_server_and_imap_stays_the_default() {
        let mut jmap = settings();
        jmap.protocol = Protocol::Jmap;
        jmap.imap.host = " jmap.example.org ".into();
        jmap.imap.port = 443;
        let ok = jmap.clone().validated().expect("valid");
        assert_eq!(ok.smtp, ok.imap, "the JMAP server stands in for SMTP");
        assert_eq!(ok.imap.host, "jmap.example.org");
        jmap.imap.security = Security::Starttls;
        let err = jmap.validated().expect_err("STARTTLS");
        assert_eq!(err.code(), crate::ErrorCode::InvalidArgument);

        // Old clients and records without the field are IMAP; IMAP is not written out.
        let mut value = serde_json::to_value(settings()).expect("serialize");
        assert!(value.get("protocol").is_none());
        let parsed: AccountSettings = serde_json::from_value(value.clone()).expect("parse");
        assert_eq!(parsed.protocol, Protocol::Imap);
        value["protocol"] = json!("jmap");
        let parsed: AccountSettings = serde_json::from_value(value).expect("parse");
        assert_eq!(parsed.protocol, Protocol::Jmap);
        assert_eq!(Protocol::parse("jmap"), Some(Protocol::Jmap));
        assert_eq!(
            Protocol::parse(Protocol::Imap.as_str()),
            Some(Protocol::Imap)
        );
        assert_eq!(Protocol::parse("pop3"), None);
    }

    #[test]
    fn api_tokens_are_for_jmap_only_and_named_by_their_keychain_reference() {
        let mut token = settings();
        token.credential = CredentialKind::ApiToken;
        let err = token.clone().validated().expect_err("IMAP with a token");
        assert_eq!(err.code(), crate::ErrorCode::InvalidArgument);
        token.protocol = Protocol::Jmap;
        assert_eq!(
            token.clone().validated().expect("JMAP").credential,
            CredentialKind::ApiToken
        );
        // Left out for a password, so old records and clients are unchanged.
        let value = serde_json::to_value(settings()).expect("serialize");
        assert!(value.get("credential").is_none());
        let value = serde_json::to_value(&token).expect("serialize");
        assert_eq!(value["credential"], json!("apiToken"));
        let parsed: AccountSettings = serde_json::from_value(value).expect("parse");
        assert_eq!(parsed.credential, CredentialKind::ApiToken);

        assert!(is_api_token_credential_ref("token:0000-1"));
        assert!(is_api_token_credential_ref("token:synced:abcd"));
        assert!(!is_api_token_credential_ref("token:"));
        assert!(!is_api_token_credential_ref("0000-1"));
        assert!(!is_api_token_credential_ref("oauth:google:0000-1"));
        assert_eq!(
            CredentialKind::of_credential_ref("token:x"),
            CredentialKind::ApiToken
        );
        assert_eq!(
            CredentialKind::of_credential_ref("synced:x"),
            CredentialKind::Password
        );
    }

    #[test]
    fn next_free_color_prefers_unused_then_least_used() {
        let c = |i| AccountColor::new(i).expect("color");
        assert_eq!(AccountColor::next_free(&[]), c(0));
        assert_eq!(AccountColor::next_free(&[c(0), c(1), c(3)]), c(2));
        let all: Vec<_> = (0..8).map(c).collect();
        assert_eq!(AccountColor::next_free(&all), c(0));
        let mut more = all.clone();
        more.extend([c(0), c(1)]);
        assert_eq!(AccountColor::next_free(&more), c(2));
        assert!(AccountColor::new(8).is_none());
    }

    #[test]
    fn status_wire_format() {
        let status = AccountStatus {
            state: AccountState::Offline,
            progress: None,
            error: None,
            retry_at: Some(1_764_000_000_000),
            last_sync_at: None,
        };
        assert_eq!(
            serde_json::to_value(&status).expect("serialize"),
            json!({ "state": "offline", "progress": null, "error": null,
                    "retryAt": 1_764_000_000_000_i64, "lastSyncAt": null })
        );
        assert_eq!(
            serde_json::to_value(Security::Starttls).expect("serialize"),
            json!("starttls")
        );
        assert_eq!(Security::parse("tls"), Some(Security::Tls));
        assert_eq!(Security::parse("plain"), None);
    }
}
