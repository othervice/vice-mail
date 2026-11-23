//! Composing and sending (stage 5, decision 0018): drafts, the outbox view of sent messages
//! ("Ausgang"), compose settings, identities with their signature and contact suggestions.
//!
//! IPC types cross the bridge through the stage 5 commands (`app/src-tauri/src/commands/compose.rs`).
//! [`SendJob`] and [`ServerCopy`] are internal: they live in outbox payloads.
//!
//! - A **draft** is local first: the composer saves it every few seconds ([`DraftUpdate`]); the
//!   core puts a copy into the account's Drafts folder after a quiet period and when the composer
//!   closes (a new version appended, the old one removed). Its Message-ID is fixed when it is
//!   created and kept for every save and every send attempt.
//! - **Sending** is an outbox operation (`send`): the message is frozen (built once, the bytes
//!   kept), waits out the undo delay ([`ComposeSettings::send_delay_seconds`]) and is then
//!   submitted. [`OutgoingMail`] is its state for the UI. An unclear outcome (the connection broke
//!   after the whole message went out) is never sent again automatically: the core looks for the
//!   message in the Sent folder where the server files it itself, and otherwise asks the user
//!   ([`SendState::Unclear`], decided with [`UnclearDecision`]).

use std::fmt;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{
    AccountId, Address, ErrorCode, IdentityId, IpcError, MessageId, OperationId, OperationNotice,
};

/// Default delay between "send" and the submission, in which sending can be cancelled.
pub const DEFAULT_SEND_DELAY_SECS: u32 = 10;
/// Longest send delay the user can choose.
pub const MAX_SEND_DELAY_SECS: u32 = 30;
/// Most bytes of all attachments of one draft together (decoded). Larger drafts are refused
/// with [`ErrorCode::AttachmentTooLarge`]; most providers refuse far less (see below).
/// Stage 10: a scheduled send that was due while the app was off goes out at the next start if
/// at most this much time has passed (Unix ms); after longer the user is asked.
pub const MISSED_SEND_GRACE_MS: i64 = 60 * 60 * 1000;
/// Stage 10: how far ahead a message can be scheduled (one year and a day, in ms).
pub const MAX_SCHEDULE_AHEAD_MS: i64 = 366 * 24 * 60 * 60 * 1000;

pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
/// Above this total the composer warns: many providers refuse messages over 20 to 25 MB (the
/// encoded message is a third larger than its attachments).
pub const LARGE_ATTACHMENT_WARNING_BYTES: u64 = 15 * 1024 * 1024;
/// Most attachments of one draft.
pub const MAX_ATTACHMENTS: usize = 100;
/// Most recipients (To, Cc and Bcc together) of one message.
pub const MAX_RECIPIENTS: usize = 500;
/// Longest subject in characters; longer ones are cut when the draft is saved.
pub const MAX_SUBJECT_CHARS: usize = 998;
/// Largest editor content (HTML and text together) of a draft.
pub const MAX_DRAFT_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Longest text signature of an identity, in bytes.
pub const MAX_TEXT_SIGNATURE_BYTES: usize = 8 * 1024;
/// Most contact suggestions of one autocomplete request.
pub const MAX_CONTACT_SUGGESTIONS: u32 = 50;

/// Id of a local draft: a UUID, lower case. Stable for the life of the draft.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct DraftId(String);

impl DraftId {
    /// `None` unless `text` is a hyphenated UUID (any case; stored in lower case).
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 36
            && bytes.iter().enumerate().all(|(i, b)| match i {
                8 | 13 | 18 | 23 => *b == b'-',
                _ => b.is_ascii_hexdigit(),
            });
        shaped.then(|| Self(text.to_ascii_lowercase()))
    }

    /// From 128 random bits (the engine draws them; this crate has no randomness), as a UUID v4.
    pub fn from_random(bits: u128) -> Self {
        let bits = (bits & !(0xf << 76) & !(0x3 << 62)) | (0x4 << 76) | (0x2 << 62);
        let hex = format!("{bits:032x}");
        Self(format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DraftId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a draft was started as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    New,
    Reply,
    ReplyAll,
    Forward,
}

impl DraftKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Reply => "reply",
            Self::ReplyAll => "reply_all",
            Self::Forward => "forward",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::New, Self::Reply, Self::ReplyAll, Self::Forward]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

/// Rich text (HTML from the editor, sent as `multipart/alternative`) or plain text only.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ComposeFormat {
    #[default]
    Html,
    Text,
}

impl ComposeFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Html, Self::Text]
            .into_iter()
            .find(|f| f.as_str() == s)
    }
}

/// Where the answer goes relative to the quoted original.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum QuotePosition {
    /// Answer and signature above the quote (the common default).
    #[default]
    Above,
    /// The quote first, then the answer and the signature.
    Below,
}

impl QuotePosition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Below => "below",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Above, Self::Below]
            .into_iter()
            .find(|p| p.as_str() == s)
    }
}

/// Language of the attribution line ("On … wrote:") and the forward header of a new reply or
/// forward: the UI's resolved language.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum QuoteLanguage {
    #[default]
    En,
    De,
}

/// A file attached to a draft. The core keeps its own copy of the bytes (taken when the file
/// was added), so moving or changing the original file afterwards changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftAttachment {
    /// Unique within the draft.
    pub id: String,
    /// Sanitised file name as it will be sent.
    pub file_name: String,
    pub mime: String,
    /// Decoded bytes.
    #[specta(type = Number)]
    pub size: u64,
    /// Set for an inline image (stage 10): the editor shows it as `<img src="cid:…">`, it goes
    /// out as a part of `multipart/related`, and the attachment list leaves it out.
    pub content_id: Option<String>,
}

/// Whether the local draft is a draft or already on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DraftState {
    /// Being written; saves are accepted.
    Editing,
    /// Frozen in the outbox (waiting for the undo delay, sending, failed or unclear); see
    /// [`Draft::outgoing`]. Saves are refused with `send_in_progress` until the user cancels
    /// the send or edits the failed one.
    Sending,
}

impl DraftState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Editing => "editing",
            Self::Sending => "sending",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Editing, Self::Sending]
            .into_iter()
            .find(|state| state.as_str() == s)
    }
}

/// Where the draft's copy in the server's Drafts folder stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DraftServerState {
    /// Kept on this device only: writes are off for the account, or it has no Drafts folder.
    LocalOnly,
    /// Changed since the last copy on the server; a copy is saved after a quiet moment.
    Pending,
    /// The outbox is putting the newest version on the server.
    Saving,
    /// The server has the newest version.
    Saved,
}

/// A local draft, as the composer opens it.
///
/// The composer keeps three parts apart (decision 0016): what the user writes (`bodyHtml` or
/// `bodyText`), the identity's signature (`includeSignature`, switched with the identity) and
/// the quote of a reply or forward (`quoteHtml`/`quoteText`, read-only in stage 5). The core
/// puts them together when it saves the draft to the server and when it sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: DraftId,
    pub account_id: AccountId,
    /// The sending identity; null when it was deleted (the composer asks for another one).
    pub identity_id: Option<IdentityId>,
    pub kind: DraftKind,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub format: ComposeFormat,
    /// Editor HTML of what the user wrote (`format: html`).
    pub body_html: String,
    /// Plain text of what the user wrote (`format: text`).
    pub body_text: String,
    /// The quoted original as editor HTML (allowlisted) and as plain text with `> `.
    pub quote_html: Option<String>,
    pub quote_text: Option<String>,
    pub include_quote: bool,
    pub quote_position: QuotePosition,
    /// The identity's text signature goes below what the user wrote.
    pub include_signature: bool,
    /// The signature the composer switched to (stage 10); null = the identity's default for
    /// this kind of draft (new mail or reply/forward).
    pub signature_id: Option<crate::SignatureId>,
    pub attachments: Vec<DraftAttachment>,
    /// The message answered or forwarded, while it is still stored locally.
    pub original_message_id: Option<MessageId>,
    /// Grows with every save.
    #[specta(type = Number)]
    pub revision: i64,
    pub state: DraftState,
    pub server: DraftServerState,
    /// The send operation while `state` is `sending`.
    pub outgoing: Option<OperationId>,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub created_at: i64,
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// A draft in a list (restoring drafts after a crash, the local drafts of an account).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftSummary {
    pub id: DraftId,
    pub account_id: AccountId,
    pub kind: DraftKind,
    pub subject: String,
    /// The first To recipients (at most five).
    pub to: Vec<Address>,
    pub state: DraftState,
    pub attachment_count: u32,
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// What the composer saves: every editable field. Attachments have their own commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftUpdate {
    pub identity_id: Option<IdentityId>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub format: ComposeFormat,
    pub body_html: String,
    pub body_text: String,
    pub include_quote: bool,
    pub quote_position: QuotePosition,
    pub include_signature: bool,
    /// The signature chosen in the composer (stage 10); null or left out = the identity's
    /// default for this kind of draft.
    #[serde(default)]
    pub signature_id: Option<crate::SignatureId>,
}

/// How a new draft starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NewDraft {
    /// An empty message from the account (its default identity unless `identityId`), optionally
    /// to someone already (a `mailto:` click, "write to" in a contact).
    New {
        account_id: AccountId,
        identity_id: Option<IdentityId>,
        to: Vec<Address>,
    },
    /// An answer to the sender of `messageId`, from the identity the message went to.
    Reply {
        message_id: MessageId,
        language: QuoteLanguage,
    },
    /// An answer to the sender and every other recipient except the user's own addresses.
    ReplyAll {
        message_id: MessageId,
        language: QuoteLanguage,
    },
    /// The message with its attachments, to new recipients.
    Forward {
        message_id: MessageId,
        language: QuoteLanguage,
    },
    /// Stage 10: a new message with every field of a [`crate::ComposeRequest`] (a `mailto:`
    /// link, `openLink` → `compose`): recipients, subject, plain-text body, and `In-Reply-To`
    /// (with `References`) for threading. From the account's default identity unless
    /// `identityId`. The request's notices are the UI's to show; they are not stored.
    Compose {
        account_id: AccountId,
        identity_id: Option<IdentityId>,
        request: crate::ComposeRequest,
    },
    /// Stage 10: the whole message as an attachment (`message/rfc822`, `<subject>.eml`), to
    /// new recipients; nothing quoted. Needs the complete original (`invalid_argument` for a
    /// large message stored only in part: forward it inline instead).
    ForwardAsAttachment {
        message_id: MessageId,
        language: QuoteLanguage,
    },
}

/// Where a sent message stands, for the outbox view ("Ausgang").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SendState {
    /// Waiting until `sendAt`: the undo delay, or (`scheduled`) the time the user chose
    /// ("send later", stage 10). Can be cancelled (back to the draft) or, when scheduled,
    /// moved (`rescheduleSend`). It is sent only if the app runs and is online at that time.
    Scheduled,
    /// Due, waiting for the server (offline, or a temporary refusal); `error` says why.
    Queued,
    /// Being submitted right now; can no longer be cancelled.
    Sending,
    /// The connection broke after the whole message went out: the core looks for it in the
    /// Sent folder (only where the server files sent mail itself).
    Verifying,
    /// It stayed unclear whether the message went out. The user decides
    /// ([`UnclearDecision`]); nothing is sent again by itself.
    Unclear,
    /// Refused (`error`); nothing was delivered. Retry, or edit the draft again.
    Failed,
    /// Stage 10: scheduled for a time when the app was not running, and more than an hour has
    /// passed (`error.code: send_missed`). Nothing was sent; the user decides
    /// ([`MissedSendDecision`]).
    Missed,
    /// The server accepted it.
    Sent,
}

/// A message on its way, for the outbox view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMail {
    pub operation_id: OperationId,
    pub account_id: AccountId,
    /// The frozen draft while it exists (until the server accepted the message).
    pub draft_id: Option<DraftId>,
    pub subject: String,
    /// The first recipients (at most five) and how many there are in all.
    pub to: Vec<Address>,
    pub recipient_count: u32,
    pub state: SendState,
    /// When the submission starts (Unix ms): the end of the undo delay, or the time the user
    /// chose (`scheduled`).
    #[specta(type = Number)]
    pub send_at: i64,
    /// Stage 10: the user chose the time ("send later"); otherwise `sendAt` is the end of the
    /// undo delay.
    pub scheduled: bool,
    /// Why it failed, is unclear or waits (the last temporary error).
    pub error: Option<IpcError>,
    /// Times the submission was started.
    pub attempts: u32,
    /// `noSentCopy` when no copy went into Sent (no Sent folder).
    pub notice: Option<OperationNotice>,
    #[specta(type = Number)]
    pub created_at: i64,
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// The user's answer to an unclear send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UnclearDecision {
    /// It arrived (the user checked): finish as sent (copy into Sent, draft removed).
    TreatAsSent,
    /// Submit it once more with the same Message-ID. The user confirmed that recipients may get
    /// it twice.
    SendAgain,
}

/// Stage 10: a time chosen for "send later".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SendLater {
    /// Unix milliseconds; in the future, at most a year ahead.
    #[specta(type = Number)]
    pub send_at: i64,
}

/// Stage 10: pasted HTML as the composer gets it back (`cleanPastedHtml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CleanedPaste {
    /// Editor HTML: the mail allowlist, no script, no style, no image, no external reference;
    /// links only `http`, `https`, `mailto`.
    pub html: String,
    /// Images that were dropped (web addresses, local files, `data:`, other mails' `cid:`).
    pub images_removed: u32,
    /// Colours, fonts, sizes, tables or headings were simplified.
    pub formatting_simplified: bool,
}

/// Stage 10: largest single inline image of a draft (the reading side shows images up to
/// 8 MiB).
pub const MAX_INLINE_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// Stage 10: what adding inline images to a draft did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InlineImagesAdded {
    pub draft: Draft,
    /// The new images in the order of the files: the editor inserts
    /// `<img src="cid:{contentId}">` for each and shows it with `inlineImageData`.
    pub added: Vec<DraftAttachment>,
}

/// Stage 10: the draft's size against what the account's SMTP server accepts (extension SIZE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftSizeCheck {
    /// Size of the message as it would go out, in bytes (attachments and inline images
    /// base64-encoded, text, headers): an estimate within a few kilobytes.
    #[specta(type = Number)]
    pub size: u64,
    /// The server's limit (SIZE); null when the server announces none or it could not be
    /// read (offline: then `limitKnown` is false).
    #[specta(type = Option<Number>)]
    pub server_limit: Option<u64>,
    /// The server answered (now or earlier in this session).
    pub limit_known: bool,
    /// `size` is above `serverLimit`: the server would refuse the message; sending is refused.
    pub exceeds_server_limit: bool,
    /// Above [`LARGE_ATTACHMENT_WARNING_BYTES`] of attachments: many providers refuse such
    /// messages on the receiving side even when this server accepts them.
    pub large: bool,
}

/// Stage 10: the user's answer to a scheduled send that was missed while the app was off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MissedSendDecision {
    /// Send it now (after the usual undo delay).
    SendNow,
    /// Schedule it again for this time (Unix ms, in the future).
    Reschedule {
        #[specta(type = Number)]
        send_at: i64,
    },
    /// Take it out of the outbox; it becomes an editable draft again.
    KeepAsDraft,
}

/// App-wide compose settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposeSettings {
    /// Undo delay before a message is submitted, 0 to [`MAX_SEND_DELAY_SECS`].
    pub send_delay_seconds: u32,
}

impl Default for ComposeSettings {
    fn default() -> Self {
        Self {
            send_delay_seconds: DEFAULT_SEND_DELAY_SECS,
        }
    }
}

/// Who puts the copy of a sent message into the Sent folder.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum SentCopyMode {
    /// Recognised from the server: Gmail files sent mail itself, everyone else needs the app.
    #[default]
    Auto,
    /// The app appends the copy.
    App,
    /// The server files it itself; the app appends nothing.
    Server,
}

impl SentCopyMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::App => "app",
            Self::Server => "server",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Auto, Self::App, Self::Server]
            .into_iter()
            .find(|m| m.as_str() == s)
    }
}

/// Sending settings of one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountSendSettings {
    pub account_id: AccountId,
    pub sent_copy: SentCopyMode,
    /// What `sentCopy: auto` amounts to for this account: the server files sent mail itself.
    pub server_saves_sent: bool,
}

/// An identity for the composer: the sender choice and its text signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposeIdentity {
    pub id: IdentityId,
    pub account_id: AccountId,
    pub email: String,
    pub name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
    /// Plain text without the `-- ` separator (added when the message is put together): the
    /// text of the default signature for new mails (stage 5 field, kept).
    pub signature: Option<String>,
    /// Default signature of new mails (stage 10); null = none.
    pub signature_new: Option<crate::SignatureId>,
    /// Default signature of replies and forwards (stage 10); null = none.
    pub signature_reply: Option<crate::SignatureId>,
}

/// An autocomplete suggestion from addresses of earlier mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactSuggestion {
    /// The name to show and to write (the user's choice, else the most used one).
    pub name: Option<String>,
    /// The address to write to.
    pub email: String,
    /// Messages the user sent to this address.
    pub sent: u32,
}

/// Why a compose request was refused; nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DraftError {
    #[error("draft {0} not found")]
    NotFound(DraftId),

    /// The draft is in the outbox; saving or deleting it needs the send to be cancelled (or
    /// the failed send to be edited) first.
    #[error("draft {0} is being sent")]
    Sending(DraftId),

    /// The undo delay is over: the submission started and cannot be stopped.
    #[error("operation {0} is already being sent")]
    TooLate(OperationId),

    #[error("the message has no recipient")]
    NoRecipients,

    #[error("not a valid email address: {0}")]
    InvalidAddress(String),

    #[error("attachments of {size} bytes; at most {limit}")]
    AttachmentTooLarge { size: u64, limit: u64 },

    /// An unclear send is only sent again through `resolveUnclearSend` with `sendAgain`.
    #[error("operation {0} may have been sent; decide explicitly")]
    NeedsDecision(OperationId),

    /// The account has no identity to send from (or the chosen one belongs to another account).
    #[error("no usable identity: {0}")]
    NoIdentity(String),

    /// Stage 10: the message is larger than the SMTP server's SIZE limit.
    #[error("the message has about {size} bytes; the server accepts at most {limit}")]
    TooLargeForServer { size: u64, limit: u64 },

    /// Stage 10: a send time in the past or too far ahead.
    #[error("not a usable send time: {0}")]
    InvalidSendTime(String),

    /// Stage 10: a scheduled send was due while the app was not running, more than
    /// [`MISSED_SEND_GRACE_MS`] ago; it was not sent (the error of the missed send).
    #[error("operation {0} was scheduled for a time when the app was not running")]
    Missed(OperationId),

    /// Stage 10: the operation is not a scheduled (or missed) send that can be moved.
    #[error("operation {0} is not waiting for a scheduled time")]
    NotScheduled(OperationId),
}

impl DraftError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Sending(_) | Self::TooLate(_) => ErrorCode::SendInProgress,
            Self::NoRecipients | Self::InvalidAddress(_) => ErrorCode::InvalidAddress,
            Self::AttachmentTooLarge { .. } => ErrorCode::AttachmentTooLarge,
            Self::NeedsDecision(_) => ErrorCode::ConfirmationRequired,
            Self::NoIdentity(_) => ErrorCode::InvalidArgument,
            Self::TooLargeForServer { .. } => ErrorCode::MessageTooLarge,
            Self::Missed(_) => ErrorCode::SendMissed,
            Self::InvalidSendTime(_) => ErrorCode::InvalidArgument,
            Self::NotScheduled(_) => ErrorCode::SendInProgress,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "The draft no longer exists.",
            Self::Sending(_) => "The message is being sent; cancel sending to change it.",
            Self::TooLate(_) => "Sending has started and can no longer be cancelled.",
            Self::NoRecipients => "The message has no recipient.",
            Self::InvalidAddress(_) => "An email address is not valid.",
            Self::AttachmentTooLarge { .. } => "The attachments are too large.",
            Self::NeedsDecision(_) => "The message may have been sent already; decide explicitly.",
            Self::NoIdentity(_) => "Choose a sender address for this message.",
            Self::TooLargeForServer { .. } => "The message is larger than the server accepts.",
            Self::Missed(_) => {
                "The app was not running at the scheduled time; the message was not sent."
            }
            Self::InvalidSendTime(_) => "Choose a time in the future.",
            Self::NotScheduled(_) => "This message is not waiting for a scheduled time.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        Some(self.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Internal: outbox payloads (never cross the bridge)

/// A message copy on the server: where it lies and what Message-ID it must carry. The outbox
/// deletes a copy only after it checked that the UID still holds this Message-ID, so it never
/// deletes someone else's mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCopy {
    pub mailbox: crate::TargetMailbox,
    pub uid_validity: crate::UidValidity,
    pub uid: crate::Uid,
    /// Without angle brackets.
    pub message_id_header: String,
}

/// The phase of a `send` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendPhase {
    /// Submit (again) over SMTP.
    #[default]
    Submit,
    /// The outcome is unclear: look for the Message-ID in Sent; never submit again by itself.
    Verify,
}

/// What a `send` operation submits; frozen when the user pressed send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendJob {
    /// The draft (UUID) the message was built from.
    pub draft: DraftId,
    /// Without angle brackets; the same for every attempt.
    pub message_id_header: String,
    /// Envelope sender and recipients (To, Cc, Bcc).
    pub from: String,
    pub recipients: Vec<String>,
    /// The bytes to submit (no `Bcc`), relative to the compose directory.
    pub file: String,
    /// The copy for Sent (with `Bcc`), relative to the compose directory.
    pub sent_file: String,
    /// For the outbox view.
    pub subject: String,
    pub to: Vec<Address>,
    pub recipient_count: u32,
    /// The server files sent mail itself (decided when the user pressed send).
    pub server_saves_sent: bool,
    /// The answered (`reply`) or forwarded message: gets `\Answered` or `$Forwarded`.
    pub original: Option<OriginalRef>,
    #[serde(default)]
    pub phase: SendPhase,
    /// Searches of the Sent folder after an unclear outcome so far.
    #[serde(default)]
    pub checks: u32,
    /// Stage 10: the user chose the send time (`not_before` of the operation); a scheduled
    /// send missed by more than [`MISSED_SEND_GRACE_MS`] while the app was off is not sent
    /// but asked about.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scheduled: bool,
}

/// An attachment of a draft as stored (`draft.attachments`): the core's own copy of the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftFile {
    pub id: String,
    pub file_name: String,
    pub mime: String,
    pub size: u64,
    /// SHA-256 of the bytes, lower-case hex.
    pub sha256: String,
    /// Path of the copy relative to the compose directory (`drafts/<draft>/<id>`).
    pub file: String,
    /// An inline image of the HTML (stage 10), referred to as `cid:<content_id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
}

impl DraftFile {
    pub fn info(&self) -> DraftAttachment {
        DraftAttachment {
            id: self.id.clone(),
            file_name: self.file_name.clone(),
            mime: self.mime.clone(),
            size: self.size,
            content_id: self.content_id.clone(),
        }
    }
}

/// A display name seen for a contact, as stored (`contact.names`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactNameRecord {
    pub name: String,
    pub count: u32,
    pub last_seen: i64,
}

/// The message a reply answers or a forward passes on, by Message-ID (local ids may change).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginalRef {
    pub message_id_header: String,
    pub forwarded: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn draft_ids_are_uuids() {
        let id = DraftId::from_random(u128::MAX);
        assert_eq!(id.as_str(), "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert_eq!(DraftId::parse(id.as_str()), Some(id.clone()));
        assert_eq!(
            DraftId::parse("6F9619FF-8B86-4D11-B42D-00C04FC964FF").map(|d| d.0),
            Some("6f9619ff-8b86-4d11-b42d-00c04fc964ff".to_owned())
        );
        for bad in [
            "",
            "x",
            "6f9619ff8b864d11b42d00c04fc964ff",
            "../../etc/passwd/xxxxxxxxxxxxxxxxxxxx",
        ] {
            assert!(DraftId::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(
            DraftId::from_random(0).as_str(),
            "00000000-0000-4000-8000-000000000000"
        );
    }

    #[test]
    fn wire_format() {
        let request: NewDraft = serde_json::from_value(json!({
            "kind": "reply_all", "messageId": 7, "language": "de"
        }))
        .expect("request");
        assert_eq!(
            request,
            NewDraft::ReplyAll {
                message_id: MessageId(7),
                language: QuoteLanguage::De
            }
        );
        assert_eq!(
            serde_json::to_value(ComposeSettings::default()).expect("settings"),
            json!({ "sendDelaySeconds": 10 })
        );
        assert_eq!(
            serde_json::to_value(SendState::Verifying).expect("state"),
            json!("verifying")
        );
        assert_eq!(
            serde_json::to_value(UnclearDecision::TreatAsSent).expect("decision"),
            json!("treat_as_sent")
        );
        for code in [
            DraftError::Sending(DraftId::from_random(1)).code(),
            DraftError::TooLate(OperationId(1)).code(),
        ] {
            assert_eq!(code, ErrorCode::SendInProgress);
        }
        assert_eq!(DraftError::NoRecipients.code(), ErrorCode::InvalidAddress);
    }
}
