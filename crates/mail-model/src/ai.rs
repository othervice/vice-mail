//! AI in the app (stage 13, decisions 0010, 0019 and 0024) as the UI sees it: settings with
//! provider profiles, the choice per function, the cloud permission per account, consent,
//! the transparency events, the request log, the results of summary, reply suggestion,
//! writing help and questions, and the open-reply hints (which work without AI).
//!
//! API keys never cross the bridge towards the UI: a profile only says whether it has one
//! (`hasKey`). The request log holds metadata only.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{AccountId, Address, ErrorCode, MessageId};

/// Most entries one `listAiRequests` page returns.
pub const MAX_AI_REQUEST_PAGE: u32 = 500;
/// The request log keeps entries this many days ...
pub const AI_REQUEST_RETENTION_DAYS: u32 = 30;
/// ... and at most this many entries.
pub const AI_REQUEST_MAX_ENTRIES: u32 = 10_000;
/// Most provider profiles.
pub const MAX_AI_PROFILES: u32 = 20;
/// Longest profile name in characters.
pub const MAX_AI_PROFILE_NAME_CHARS: u32 = 100;
/// Longest request id the UI may choose (it sends a UUID).
pub const MAX_AI_REQUEST_ID_CHARS: u32 = 64;
/// Longest question for `aiAsk`, in characters.
pub const MAX_AI_QUESTION_CHARS: u32 = 1_000;
/// Longest passage for `aiWritingHelp`, in characters.
pub const MAX_AI_PASSAGE_CHARS: u32 = 12_000;

/// Wire protocol of a profile (decision 0019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    /// `POST {base}/chat/completions`: Ollama, LM Studio, llama.cpp server, OpenRouter,
    /// OpenAI. The base URL includes `/v1`.
    OpenaiCompatible,
    /// Anthropic Messages API (`POST {base}/v1/messages`).
    Anthropic,
}

/// Where a profile's base URL points. `thisDevice` and `localNetwork` are shown as "local":
/// the transport only ever connects to such addresses for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiLocality {
    ThisDevice,
    LocalNetwork,
    /// A cloud provider: consent first, and only accounts with cloud permission.
    Internet,
}

impl AiLocality {
    pub const fn is_local(self) -> bool {
        !matches!(self, Self::Internet)
    }
}

/// The functions that can use a provider. The open-reply hints are not one of them: they work
/// without AI (decision 0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiFunction {
    Summary,
    ReplySuggestion,
    WritingHelp,
    Search,
}

impl AiFunction {
    pub const ALL: [Self; 4] = [
        Self::Summary,
        Self::ReplySuggestion,
        Self::WritingHelp,
        Self::Search,
    ];

    /// Stable name for the settings and the request log.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::ReplySuggestion => "reply_suggestion",
            Self::WritingHelp => "writing_help",
            Self::Search => "search",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == text)
    }
}

/// A provider profile as the settings show it. The key is in the system keychain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiProfileInfo {
    /// 32 lowercase hex digits.
    pub id: String,
    pub name: String,
    pub kind: AiProviderKind,
    pub base_url: String,
    /// Empty until the user picks one.
    pub model: String,
    /// Ask for the JSON schema of the answer; off for servers or models that reject it.
    pub structured_output: bool,
    pub max_output_tokens: Option<u32>,
    /// A key is stored in the keychain (never shown).
    pub has_key: bool,
    /// Host and port of the base URL ("an wen").
    pub host: String,
    pub locality: AiLocality,
    /// When the user agreed to send data to this provider (Unix ms). Always `null` for local
    /// profiles, which need no consent. Changing the base URL's host withdraws it.
    #[specta(type = Option<Number>)]
    pub consent_at: Option<i64>,
    /// A cloud profile without consent: the first request fails with `ai_consent_required`.
    pub needs_consent: bool,
}

/// How a profile's key changes when it is saved. Only ever sent from the UI to the core; the
/// core wraps the key in a [`crate::Secret`] at once. `Debug` never shows it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AiKeyChange {
    /// Keep the stored key (or none).
    Keep,
    /// Delete the stored key.
    Remove,
    /// Store this key in the keychain (an empty key is `remove`).
    Set { key: String },
}

impl std::fmt::Debug for AiKeyChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keep => f.write_str("Keep"),
            Self::Remove => f.write_str("Remove"),
            Self::Set { .. } => f.write_str("Set { key: [redacted] }"),
        }
    }
}

/// A profile to create (`id` null) or change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiProfileInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: AiProviderKind,
    pub base_url: String,
    pub model: String,
    pub structured_output: bool,
    pub max_output_tokens: Option<u32>,
    pub key: AiKeyChange,
}

/// Which profile a function uses; `null` is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiFunctionChoice {
    pub function: AiFunction,
    pub profile_id: Option<String>,
}

/// Whether cloud providers may see an account's mail. Every account starts without.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiAccountCloud {
    pub account_id: AccountId,
    pub cloud_allowed: bool,
}

/// A known endpoint for "new profile".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiPreset {
    pub name: String,
    pub kind: AiProviderKind,
    pub base_url: String,
    pub needs_key: bool,
    pub locality: AiLocality,
}

/// Settings of the open-reply hints (no AI involved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenReplySettings {
    pub enabled: bool,
    /// A sent question counts as unanswered after this many days (1..=30).
    pub days: u32,
}

impl Default for OpenReplySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            days: 3,
        }
    }
}

/// The settings page "AI in the app".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsInfo {
    /// Master switch; off until the user turns it on. Off: no request leaves the device.
    pub enabled: bool,
    pub profiles: Vec<AiProfileInfo>,
    /// One entry per [`AiFunction`], in that order.
    pub functions: Vec<AiFunctionChoice>,
    /// One entry per account.
    pub accounts: Vec<AiAccountCloud>,
    pub presets: Vec<AiPreset>,
    pub open_replies: OpenReplySettings,
}

/// Which functions are usable right now (for showing the AI panel and buttons): AI is on and
/// the function has a profile with a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiAvailability {
    pub enabled: bool,
    pub summary: bool,
    pub reply_suggestion: bool,
    pub writing_help: bool,
    pub search: bool,
}

/// What was shortened for the length budget. All zero: nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiTrimming {
    pub mails_left_out: u32,
    pub mails_shortened: u32,
    pub chars_removed: u32,
}

/// What one request sends and to whom, shown before and while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiTransfer {
    pub function: AiFunction,
    pub profile_id: String,
    pub profile_name: String,
    /// The recipient: host and port of the provider.
    pub host: String,
    pub locality: AiLocality,
    pub model: String,
    /// Messages sent (0 for a question's first step and for the writing help).
    pub mails: u32,
    /// Accounts the messages belong to.
    pub accounts: Vec<AccountId>,
    /// Bytes of text sent.
    pub bytes: u32,
    /// Attachment names sent (never contents).
    pub attachment_names: u32,
    /// Quoted passages left out because an earlier message has them.
    pub quotes_removed: u32,
    pub trimming: AiTrimming,
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiRequestOutcome {
    Ok,
    Cancelled,
    Failed,
}

/// One entry of the request log: metadata only, never content or keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestEntry {
    #[specta(type = Number)]
    pub id: i64,
    /// Unix ms.
    #[specta(type = Number)]
    pub at: i64,
    pub function: AiFunction,
    pub profile_name: String,
    pub host: String,
    pub locality: AiLocality,
    pub model: String,
    pub mails: u32,
    pub bytes: u32,
    pub outcome: AiRequestOutcome,
    /// For `failed`: the error code.
    pub error: Option<ErrorCode>,
    pub duration_ms: u32,
}

/// A page of the request log, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestPage {
    pub entries: Vec<AiRequestEntry>,
    /// Pass as `before` for the next page; `null` at the end.
    #[specta(type = Option<Number>)]
    pub next_before: Option<i64>,
}

/// A statement with the messages it is based on (each one exists and was sent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiClaim {
    pub text: String,
    pub sources: Vec<MessageId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiDeadline {
    pub text: String,
    /// As the message states it ("Friday", "30.11.").
    pub due: String,
    pub sources: Vec<MessageId>,
}

/// A thread summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSummary {
    pub key_points: Vec<AiClaim>,
    pub open_questions: Vec<AiClaim>,
    pub deadlines: Vec<AiDeadline>,
    /// Statements dropped because they named no message that was sent.
    pub dropped: u32,
    pub transfer: AiTransfer,
}

/// A reply suggestion; "open draft" (`aiOpenReplyDraft`) turns it into an editable draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiReplySuggestion {
    pub text: String,
    /// The message the reply answers.
    pub in_reply_to: MessageId,
    pub transfer: AiTransfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiTone {
    Formal,
    Friendly,
    Neutral,
    Direct,
}

/// Target languages of "translate".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiTargetLanguage {
    En,
    De,
    Fr,
    Es,
    It,
    Nl,
    Pt,
    Pl,
}

/// What the writing help does with the passage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AiWritingTask {
    Improve,
    Shorten,
    Rephrase,
    Tone { tone: AiTone },
    Translate { language: AiTargetLanguage },
}

/// The rewritten passage, for the preview. Nothing in the draft changes until the UI applies
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiWritingResult {
    pub text: String,
    pub transfer: AiTransfer,
}

/// The answer to a question in natural language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiAnswer {
    /// `false`: honestly nothing found (no candidates, or nothing in them answers it);
    /// `statements` is empty then.
    pub found: bool,
    pub statements: Vec<AiClaim>,
    /// The search the question became (stage 7 language; the UI can offer it).
    pub query: String,
    /// Messages the search found.
    pub candidates: u32,
    /// Candidates not sent because their account has no cloud permission.
    pub excluded: u32,
    /// Statements dropped for lack of a valid reference.
    pub dropped: u32,
    /// One per request sent.
    pub transfers: Vec<AiTransfer>,
}

/// A model of a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub display_name: Option<String>,
}

/// Result of "test connection" (no mail content is sent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiConnectionTest {
    /// Models the provider lists.
    pub models: u32,
    /// The profile's model is among them (`null`: no model chosen).
    pub model_listed: Option<bool>,
    /// A short test request with the profile's model answered (`null`: no model chosen).
    pub chat_ok: Option<bool>,
    /// Why the test request failed.
    pub chat_error: Option<ErrorCode>,
}

/// Progress of a running request, as `aiRequest` events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "phase", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AiRequestPhase {
    /// About to send this (one per provider request; a question has up to two).
    Sending { transfer: AiTransfer },
    /// Characters of the answer received so far.
    Receiving { chars: u32 },
    /// Done (the command returns the result or the error).
    Finished { outcome: AiRequestOutcome },
}

/// Why an open-reply hint is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OpenReplyKind {
    /// The user asked a question and no answer came for some days.
    AwaitingReply,
    /// Someone asked the user directly and the user has not answered.
    UnansweredQuestion,
}

/// A dismissable suggestion, never a claim: "maybe still open".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenReplyHint {
    pub message_id: MessageId,
    pub account_id: AccountId,
    pub kind: OpenReplyKind,
    pub subject: String,
    /// Awaiting: the first recipient; unanswered: the sender.
    pub counterpart: Option<Address>,
    /// Unix seconds.
    #[specta(type = Number)]
    pub date: i64,
    /// Whole days since `date`.
    pub days: u32,
}

/// An AI request failed; the code tells the UI what to say.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiFailure {
    /// AI is off, or the function has no profile (or the profile no model).
    #[error("AI is turned off for this function")]
    Off,
    /// A cloud provider needs the user's consent first (`grantAiConsent`).
    #[error("the user has not agreed to send data to this provider yet")]
    ConsentRequired { profile_id: String },
    /// An account involved has no cloud permission; nothing was sent.
    #[error("{accounts} account(s) may not send mail to a cloud provider")]
    NotPermitted { accounts: u32 },
    #[error("the AI request was cancelled")]
    Cancelled,
    /// No connection, or no answer in time.
    #[error("the AI provider could not be reached: {0}")]
    Unreachable(String),
    /// Key missing, wrong or without access.
    #[error("the AI provider refused the key: {0}")]
    Unauthorized(String),
    /// Rate limit or overload; try later.
    #[error("the AI provider is busy: {0}")]
    Busy(String),
    /// The model declined.
    #[error("the AI provider declined the request")]
    Refused,
    /// Anything else (wrong address or model, too large, unreadable answer, cut off).
    #[error("the AI request failed: {0}")]
    Failed(String),
}

impl AiFailure {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Off => ErrorCode::AiOff,
            Self::ConsentRequired { .. } => ErrorCode::AiConsentRequired,
            Self::NotPermitted { .. } => ErrorCode::AiNotPermitted,
            Self::Cancelled => ErrorCode::AiCancelled,
            Self::Unreachable(_) => ErrorCode::AiUnreachable,
            Self::Unauthorized(_) => ErrorCode::AiUnauthorized,
            Self::Busy(_) => ErrorCode::AiBusy,
            Self::Refused => ErrorCode::AiRefused,
            Self::Failed(_) => ErrorCode::AiFailed,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Off => "AI is turned off for this function.",
            Self::ConsentRequired { .. } => {
                "Agree to sending data to this AI provider before the first request."
            }
            Self::NotPermitted { .. } => {
                "This account's mail may not be sent to a cloud AI provider."
            }
            Self::Cancelled => "The AI request was cancelled.",
            Self::Unreachable(_) => "The AI provider could not be reached.",
            Self::Unauthorized(_) => "The AI provider did not accept the key.",
            Self::Busy(_) => "The AI provider is busy. Try again later.",
            Self::Refused => "The AI provider declined the request.",
            Self::Failed(_) => "The AI request failed.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Off | Self::Cancelled | Self::Refused => None,
            Self::ConsentRequired { profile_id } => Some(format!("profile {profile_id}")),
            Self::NotPermitted { accounts } => Some(format!("{accounts} account(s)")),
            Self::Unreachable(d) | Self::Unauthorized(d) | Self::Busy(d) | Self::Failed(d) => {
                (!d.is_empty()).then(|| d.clone())
            }
        }
    }
}
