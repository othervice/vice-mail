//! IPC types of stage 3: the reading pane, remote content, links, attachments and prefetch
//! settings (stage 3 IPC contract, decision 0009).
//!
//! The commands that use them are wired by the engine and shell session of stage 3. Until a
//! command or event references a type, tauri-specta does not export it, so `bindings.ts` is
//! unchanged. Wire format: camelCase fields; tagged unions carry `kind`.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{AccountId, Address, CoreError, IpcError, MessageId};

/// Which rendering of the body the UI asks `getMessage` for.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum BodyFormat {
    /// HTML if the message has an HTML part, otherwise text.
    #[default]
    Auto,
    Html,
    Text,
}

/// Opaque id of one rendering of a body: 32 lowercase hex digits (128 random bits), made by the
/// core. Link placeholders of that rendering carry it (`https://link.invalid/<token>/<index>`);
/// the real targets stay in the core under it. Unguessable, so a mail cannot point at the links
/// of another rendering.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct RenderToken(String);

impl RenderToken {
    pub const LEN: usize = 32;

    /// `None` unless `text` is exactly 32 lowercase hex digits.
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == Self::LEN
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(text.to_owned()))
    }

    /// From 128 random bits (the engine draws them; this crate has no randomness).
    pub fn from_random(bits: u128) -> Self {
        Self(format!("{bits:032x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Host of the link placeholders the sanitizer writes into every `href` (decision 0005).
pub const LINK_PLACEHOLDER_HOST: &str = "link.invalid";

/// The placeholder URL of link `index` in the rendering `token`.
pub fn link_placeholder(token: &RenderToken, index: u32) -> String {
    format!("https://{LINK_PLACEHOLDER_HOST}/{}/{index}", token.as_str())
}

/// The navigation hook's question: is `url` a link placeholder, and of which rendering and link?
/// Exact form only (`https`, no port, user info, query or fragment); anything else is `None`
/// and the hook just cancels the navigation.
pub fn parse_link_placeholder(url: &str) -> Option<(RenderToken, u32)> {
    let rest = url.strip_prefix("https://")?;
    let rest = rest
        .strip_prefix(LINK_PLACEHOLDER_HOST)?
        .strip_prefix('/')?;
    let (token, index) = rest.split_once('/')?;
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) || index.len() > 9 {
        return None;
    }
    Some((RenderToken::parse(token)?, index.parse().ok()?))
}

/// What `getMessage` returns: everything the reading pane shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: MessageId,
    /// For the account color.
    pub account_id: AccountId,
    pub header: MessageViewHeader,
    pub body: MessageBody,
    /// Formats the message can be shown in (`html`, `text`, or both). Switching is another
    /// `getMessage` with that format; it is served locally.
    pub formats: Vec<BodyFormat>,
    /// Real attachments (not the inline images the HTML shows), in message order.
    pub attachments: Vec<AttachmentInfo>,
    /// A large message of which only the text parts are stored; attachments are fetched when
    /// saved or opened.
    pub partial: bool,
    /// Defects found while parsing; the UI shows a hint. Empty for well-formed mail.
    pub problems: Vec<MessageProblem>,
}

/// Header of the reading pane. Addresses decoded (RFC 2047), groups flattened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageViewHeader {
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Only known for mail the user sent (Sent, Drafts).
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    /// Sort date in Unix milliseconds, as in the list.
    #[specta(type = Number)]
    pub date: i64,
    /// Empty if the message has no subject.
    pub subject: String,
}

/// The body as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MessageBody {
    /// Sanitized HTML for `<iframe sandbox="allow-same-origin" srcdoc=...>`. Starts with the
    /// mandatory meta CSP; every `href` is a placeholder of `render_token`; external images are
    /// replaced unless remote content was loaded.
    Html {
        srcdoc: String,
        render_token: RenderToken,
        /// External images and styles that were blocked (0 when `remote_loaded`).
        blocked_remote: u32,
        remote_loaded: bool,
    },
    /// Plain text for the app's own theme. Links are separate segments, opened through the same
    /// check as HTML links (`describeLink` with this `render_token`).
    Text {
        segments: Vec<TextSegment>,
        render_token: RenderToken,
    },
    /// Not stored locally; a fetch is running. `messageBodyAvailable` or `messageBodyFailed`
    /// follows; then ask again.
    Loading,
    /// Not stored locally and cannot be fetched now.
    Unavailable {
        reason: BodyUnavailableReason,
        /// Details for `fetchFailed`, null otherwise.
        error: Option<IpcError>,
    },
}

/// A piece of a text body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TextSegment {
    Text {
        text: String,
    },
    /// A URL or address found in the text; `index` for `describeLink`/`openLink`.
    Link {
        text: String,
        index: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BodyUnavailableReason {
    /// The account is not connected and the body was never stored.
    Offline,
    /// The message no longer exists on the server (moved or deleted meanwhile).
    NotOnServer,
    /// The fetch failed; see `error`.
    FetchFailed,
}

/// One attachment in the reading pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    /// Position for `saveAttachment`/`openAttachment`; stable for the message.
    pub index: u32,
    /// Sanitized file name (no path separators, reserved names, control or bidi characters);
    /// never empty.
    pub name: String,
    /// Lower-case MIME type as declared.
    pub mime_type: String,
    /// Decoded size in bytes; estimated from the encoded size while `local` is false.
    pub size: u32,
    /// Opening it can run code (by extension or type); `openAttachment` asks for confirmation.
    pub executable: bool,
    /// The content is stored locally (false for attachments of a partial message).
    pub local: bool,
}

/// Defects of a message that the reading pane mentions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MessageProblem {
    /// The MIME structure is damaged (missing boundary, truncated); shown as far as readable.
    BrokenStructure,
    /// A text part declared an unknown charset; decoded with replacement characters.
    UnknownCharset,
    /// A part's transfer encoding is broken; it is shown raw or left out.
    UndecodablePart,
    /// No readable text part (only attachments, or empty).
    NoContent,
    /// The message could not be parsed at all and is shown as raw text.
    RawFallback,
}

/// Scope of `loadRemoteContent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum RemoteContentScope {
    /// This rendering only; nothing is stored.
    Once,
    /// Always for this sender address in this account (stored, synced in stage 11).
    Sender,
}

/// Answer of `describeLink`: what the user confirms before the system browser opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    /// The real target, complete, for display.
    pub target: String,
    /// Lower-case scheme (`https`, `http`, `mailto`, ...).
    pub scheme: String,
    /// Host in Unicode form; null for `mailto` and hostless URLs.
    pub host: Option<String>,
    /// Visible link text, white space collapsed, at most 200 characters; empty for images.
    pub text: String,
    pub warnings: Vec<LinkWarning>,
    /// Only `http`, `https` and `mailto` can be opened.
    pub openable: bool,
}

/// Why a link deserves a second look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LinkWarning {
    /// The link text names another domain than the target.
    DomainMismatch {
        shown_domain: String,
        target_domain: String,
    },
    /// The host mixes scripts or uses look-alike characters (IDN homograph); `ascii_host` is
    /// its punycode form.
    ConfusableHost { ascii_host: String },
    /// The host is a bare IP address.
    IpAddressHost,
    /// The URL carries a user name or password (`https://bank.example@evil.example/`).
    CredentialsInUrl,
    /// A scheme other than http, https and mailto; the link cannot be opened.
    UnsupportedScheme,
}

/// Result of `saveAttachment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveAttachmentResult {
    /// Written to the path the user chose in the system dialog.
    Saved { path: String },
    /// The user closed the dialog.
    Cancelled,
}

/// Result of `openAttachment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OpenAttachmentResult {
    /// Handed to the system's default app.
    Opened,
    /// Executable and not confirmed: show the warning, then call again with
    /// `confirmedExecutable: true`. Nothing was written or opened.
    NeedsConfirmation,
}

/// How far back bodies are prefetched (Inbox, Sent, Drafts and flagged mail).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum PrefetchPeriod {
    Days7,
    #[default]
    Days30,
    Days90,
    Days365,
    All,
}

impl PrefetchPeriod {
    /// Days covered; `None` for everything.
    pub const fn days(self) -> Option<u32> {
        match self {
            Self::Days7 => Some(7),
            Self::Days30 => Some(30),
            Self::Days90 => Some(90),
            Self::Days365 => Some(365),
            Self::All => None,
        }
    }

    /// Oldest sort date (Unix seconds) inside the period at `now` (Unix seconds); `None` for
    /// everything.
    pub fn since(self, now: i64) -> Option<i64> {
        self.days()
            .map(|days| now.saturating_sub(i64::from(days) * 86_400))
    }
}

/// Prefetch settings (device class: disk space and connection differ per device; not synced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchSettings {
    pub period: PrefetchPeriod,
    /// Messages up to this RFC822.SIZE (bytes) are prefetched whole; larger ones only with
    /// their text parts, so they can still be read offline. 0 prefetches text only.
    pub max_message_size: u32,
}

impl PrefetchSettings {
    /// Upper bound of [`PrefetchSettings::max_message_size`] (256 MiB, beyond any server limit).
    pub const MAX_MESSAGE_SIZE: u32 = 256 * 1024 * 1024;
    /// Default size limit: 5 MiB (most attachments, not the occasional video).
    pub const DEFAULT_MAX_MESSAGE_SIZE: u32 = 5 * 1024 * 1024;

    pub fn validated(self) -> Result<Self, CoreError> {
        if self.max_message_size > Self::MAX_MESSAGE_SIZE {
            return Err(CoreError::InvalidArgument(format!(
                "prefetch size limit {} is above {}",
                self.max_message_size,
                Self::MAX_MESSAGE_SIZE
            )));
        }
        Ok(self)
    }
}

impl Default for PrefetchSettings {
    fn default() -> Self {
        Self {
            period: PrefetchPeriod::default(),
            max_message_size: Self::DEFAULT_MAX_MESSAGE_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PrefetchState {
    /// Everything in the period is stored.
    Idle,
    Running,
    /// The account is offline; continues when it reconnects.
    Paused,
}

/// Progress of the prefetch of one account (event `prefetchProgress`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchStatus {
    pub state: PrefetchState,
    /// Bodies stored in the current run, and the run's total.
    pub done: u32,
    pub total: u32,
    /// Bytes fetched in the current run (below 2^53).
    #[specta(type = Number)]
    pub bytes: u64,
}

/// The sender address as the remote-content allowlist stores and compares it: angle brackets
/// and surrounding white space removed, ASCII lower case. `None` unless it is one address with
/// a non-empty local part and domain, without white space or control characters, at most 320
/// bytes long.
pub fn normalize_sender(address: &str) -> Option<String> {
    let trimmed = address.trim();
    let bare = trimmed
        .strip_prefix('<')
        .and_then(|a| a.strip_suffix('>'))
        .unwrap_or(trimmed)
        .trim();
    let (local, domain) = bare.rsplit_once('@')?;
    let valid = !local.is_empty()
        && !domain.is_empty()
        && bare.len() <= 320
        && !bare
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ','))
        && bare.bytes().filter(|&b| b == b'@').count() == 1;
    valid.then(|| bare.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ErrorCode;

    fn token() -> RenderToken {
        RenderToken::from_random(0x0123_4567_89ab_cdef_0011_2233_4455_6677)
    }

    #[test]
    fn render_tokens_and_link_placeholders() {
        let t = token();
        assert_eq!(t.as_str(), "0123456789abcdef0011223344556677");
        assert_eq!(RenderToken::from_random(1).as_str().len(), RenderToken::LEN);
        assert_eq!(RenderToken::parse(t.as_str()), Some(t.clone()));
        for bad in [
            "",
            "0123456789ABCDEF0011223344556677",
            "0123",
            "g123456789abcdef0011223344556677",
        ] {
            assert_eq!(RenderToken::parse(bad), None, "{bad:?}");
        }

        let url = link_placeholder(&t, 42);
        assert_eq!(
            url,
            "https://link.invalid/0123456789abcdef0011223344556677/42"
        );
        assert_eq!(parse_link_placeholder(&url), Some((t.clone(), 42)));
        for bad in [
            "http://link.invalid/0123456789abcdef0011223344556677/42",
            "https://link.invalid:443/0123456789abcdef0011223344556677/42",
            "https://user@link.invalid/0123456789abcdef0011223344556677/42",
            "https://link.invalid/0123456789abcdef0011223344556677/42?x",
            "https://link.invalid/0123456789abcdef0011223344556677/42#x",
            "https://link.invalid/0123456789abcdef0011223344556677/",
            "https://link.invalid/0123456789abcdef0011223344556677/+1",
            "https://link.invalid/0123456789abcdef0011223344556677/1/2",
            "https://link.invalid/42",
            "https://link.invalid.evil.example/0123456789abcdef0011223344556677/42",
            "https://example.org/",
            "about:srcdoc",
        ] {
            assert_eq!(parse_link_placeholder(bad), None, "{bad}");
        }
    }

    #[test]
    fn message_view_wire_format() {
        let view = MessageView {
            id: MessageId(1_764_000_000),
            account_id: "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id"),
            header: MessageViewHeader {
                from: vec![Address {
                    name: Some("Jürgen".into()),
                    email: "j@example.org".into(),
                }],
                to: vec![],
                cc: vec![],
                bcc: vec![],
                reply_to: vec![],
                date: 1_764_000_000_000,
                subject: "Grüße".into(),
            },
            body: MessageBody::Html {
                srcdoc:
                    "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'\">"
                        .into(),
                render_token: token(),
                blocked_remote: 3,
                remote_loaded: false,
            },
            formats: vec![BodyFormat::Html, BodyFormat::Text],
            attachments: vec![AttachmentInfo {
                index: 0,
                name: "Rechnung.pdf".into(),
                mime_type: "application/pdf".into(),
                size: 12_345,
                executable: false,
                local: true,
            }],
            partial: false,
            problems: vec![MessageProblem::UnknownCharset],
        };
        let wire = serde_json::to_value(&view).expect("serialize");
        assert_eq!(
            wire,
            json!({
                "id": 1_764_000_000,
                "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                "header": { "from": [{ "name": "Jürgen", "email": "j@example.org" }], "to": [],
                            "cc": [], "bcc": [], "replyTo": [], "date": 1_764_000_000_000_i64,
                            "subject": "Grüße" },
                "body": { "kind": "html",
                          "srcdoc": "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'\">",
                          "renderToken": "0123456789abcdef0011223344556677",
                          "blockedRemote": 3, "remoteLoaded": false },
                "formats": ["html", "text"],
                "attachments": [{ "index": 0, "name": "Rechnung.pdf", "mimeType": "application/pdf",
                                  "size": 12_345, "executable": false, "local": true }],
                "partial": false,
                "problems": ["unknownCharset"]
            })
        );
        let back: MessageView = serde_json::from_value(wire).expect("deserialize");
        assert_eq!(back, view);
    }

    #[test]
    fn body_variants_wire_format() {
        let cases = [
            (
                MessageBody::Text {
                    segments: vec![
                        TextSegment::Text {
                            text: "See ".into(),
                        },
                        TextSegment::Link {
                            text: "https://example.org".into(),
                            index: 0,
                        },
                    ],
                    render_token: token(),
                },
                json!({ "kind": "text",
                        "segments": [{ "kind": "text", "text": "See " },
                                     { "kind": "link", "text": "https://example.org", "index": 0 }],
                        "renderToken": "0123456789abcdef0011223344556677" }),
            ),
            (MessageBody::Loading, json!({ "kind": "loading" })),
            (
                MessageBody::Unavailable {
                    reason: BodyUnavailableReason::Offline,
                    error: None,
                },
                json!({ "kind": "unavailable", "reason": "offline", "error": null }),
            ),
            (
                MessageBody::Unavailable {
                    reason: BodyUnavailableReason::FetchFailed,
                    error: Some(IpcError {
                        code: ErrorCode::ConnectionLost,
                        message: "The connection to the server was lost.".into(),
                        detail: None,
                    }),
                },
                json!({ "kind": "unavailable", "reason": "fetchFailed",
                        "error": { "code": "connection_lost",
                                   "message": "The connection to the server was lost.",
                                   "detail": null } }),
            ),
        ];
        for (body, wire) in cases {
            assert_eq!(serde_json::to_value(&body).expect("serialize"), wire);
            assert_eq!(
                serde_json::from_value::<MessageBody>(wire).expect("deserialize"),
                body
            );
        }
        assert_eq!(
            serde_json::to_value(BodyUnavailableReason::NotOnServer).expect("serialize"),
            "notOnServer"
        );
        assert_eq!(
            serde_json::to_value(BodyFormat::default()).expect("serialize"),
            "auto"
        );
    }

    #[test]
    fn link_and_attachment_results_wire_format() {
        let info = LinkInfo {
            target: "https://evil.example/login".into(),
            scheme: "https".into(),
            host: Some("evil.example".into()),
            text: "https://bank.example".into(),
            warnings: vec![
                LinkWarning::DomainMismatch {
                    shown_domain: "bank.example".into(),
                    target_domain: "evil.example".into(),
                },
                LinkWarning::ConfusableHost {
                    ascii_host: "xn--pple-43d.example".into(),
                },
                LinkWarning::IpAddressHost,
                LinkWarning::CredentialsInUrl,
                LinkWarning::UnsupportedScheme,
            ],
            openable: true,
        };
        assert_eq!(
            serde_json::to_value(&info).expect("serialize"),
            json!({
                "target": "https://evil.example/login", "scheme": "https", "host": "evil.example",
                "text": "https://bank.example",
                "warnings": [
                    { "kind": "domainMismatch", "shownDomain": "bank.example", "targetDomain": "evil.example" },
                    { "kind": "confusableHost", "asciiHost": "xn--pple-43d.example" },
                    { "kind": "ipAddressHost" },
                    { "kind": "credentialsInUrl" },
                    { "kind": "unsupportedScheme" }
                ],
                "openable": true
            })
        );
        assert_eq!(
            serde_json::to_value(SaveAttachmentResult::Saved {
                path: "/Users/a/Downloads/x.pdf".into()
            })
            .expect("serialize"),
            json!({ "kind": "saved", "path": "/Users/a/Downloads/x.pdf" })
        );
        assert_eq!(
            serde_json::to_value(SaveAttachmentResult::Cancelled).expect("serialize"),
            json!({ "kind": "cancelled" })
        );
        assert_eq!(
            serde_json::to_value(OpenAttachmentResult::NeedsConfirmation).expect("serialize"),
            json!({ "kind": "needsConfirmation" })
        );
        assert_eq!(
            serde_json::to_value(RemoteContentScope::Sender).expect("serialize"),
            "sender"
        );
    }

    #[test]
    fn prefetch_settings_and_status_wire_format() {
        let settings = PrefetchSettings::default();
        assert_eq!(
            serde_json::to_value(settings).expect("serialize"),
            json!({ "period": "days30", "maxMessageSize": 5_242_880 })
        );
        let periods = [
            (PrefetchPeriod::Days7, "days7", Some(7)),
            (PrefetchPeriod::Days30, "days30", Some(30)),
            (PrefetchPeriod::Days90, "days90", Some(90)),
            (PrefetchPeriod::Days365, "days365", Some(365)),
            (PrefetchPeriod::All, "all", None),
        ];
        for (period, wire, days) in periods {
            assert_eq!(serde_json::to_value(period).expect("serialize"), wire);
            assert_eq!(period.days(), days);
        }
        assert_eq!(PrefetchPeriod::Days7.since(1_000_000), Some(395_200));
        assert_eq!(PrefetchPeriod::All.since(1_000_000), None);

        assert!(settings.validated().is_ok());
        let too_big = PrefetchSettings {
            max_message_size: PrefetchSettings::MAX_MESSAGE_SIZE + 1,
            ..settings
        };
        assert_eq!(
            too_big.validated().map_err(|e| e.code()),
            Err(ErrorCode::InvalidArgument)
        );
        assert!(
            serde_json::from_value::<PrefetchSettings>(
                json!({ "period": "days14", "maxMessageSize": 1 })
            )
            .is_err()
        );

        let status = PrefetchStatus {
            state: PrefetchState::Running,
            done: 12,
            total: 340,
            bytes: 1_234_567,
        };
        assert_eq!(
            serde_json::to_value(status).expect("serialize"),
            json!({ "state": "running", "done": 12, "total": 340, "bytes": 1_234_567 })
        );
    }

    #[test]
    fn sender_addresses_are_normalized() {
        assert_eq!(
            normalize_sender("  <News@Example.ORG> ").as_deref(),
            Some("news@example.org")
        );
        assert_eq!(
            normalize_sender("Jürgen@Firma.example").as_deref(),
            Some("jürgen@firma.example"),
            "only ASCII is lower-cased"
        );
        for bad in [
            "",
            "news",
            "@example.org",
            "news@",
            "a b@example.org",
            "a@b@example.org",
            "a@example.org, b@example.org",
            "a\u{0}@example.org",
            "\"Name\" <a@example.org>",
        ] {
            assert_eq!(normalize_sender(bad), None, "{bad:?}");
        }
        let long = format!("{}@example.org", "a".repeat(320));
        assert_eq!(normalize_sender(&long), None);
    }
}
