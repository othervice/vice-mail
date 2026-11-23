//! Typed errors of the AI layer. Texts never contain an API key: every message that comes from
//! a server passes through [`server_message`], which removes the key and control characters and
//! cuts the text short.

use std::time::Duration;

use mail_model::{AccountId, Secret};

/// Longest server message kept in an error, in characters.
const MAX_MESSAGE_CHARS: usize = 300;

/// What went wrong. [`AiError::code`] gives a stable code for the interface.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    /// The caller cancelled ([`crate::CancelToken::cancel`]).
    #[error("cancelled")]
    Cancelled,

    /// The provider profile is not usable (base URL, scheme, model name).
    #[error("invalid AI provider settings: {0}")]
    InvalidConfig(String),

    /// Nothing to send (no messages, an empty question).
    #[error("nothing to send to the AI provider")]
    NothingToSend,

    /// Accounts whose mail must not go to a cloud provider ([`crate::CloudPolicy`]). Nothing
    /// was sent.
    #[error("{} account(s) are not allowed to send mail to a cloud provider", accounts.len())]
    NotPermitted { accounts: Vec<AccountId> },

    /// No connection (DNS, refused, TLS, the address check of a local endpoint).
    #[error("could not connect to the AI provider: {0}")]
    Connect(String),

    /// A time limit was reached.
    #[error("the AI provider did not answer in time ({0})")]
    Timeout(TimeoutPhase),

    /// 401 or 403: the key is missing, wrong or lacks access.
    #[error("the AI provider refused the credentials (status {status})")]
    Unauthorized {
        status: u16,
        message: Option<String>,
    },

    /// 404: wrong base URL or unknown model.
    #[error("not found at the AI provider (wrong address or model?)")]
    NotFound { message: Option<String> },

    /// 429: too many requests. `retry_after` from `retry-after-ms` or `retry-after`.
    #[error("rate limited by the AI provider")]
    RateLimited { retry_after: Option<Duration> },

    /// 503, 529 or an `overloaded_error` event: try again later.
    #[error("the AI provider is overloaded")]
    Overloaded { retry_after: Option<Duration> },

    /// Another 5xx, or a server error event in the stream.
    #[error("the AI provider failed (status {status})")]
    Server {
        status: u16,
        message: Option<String>,
    },

    /// Another 4xx (or a redirect, which is never followed).
    #[error("the AI provider rejected the request (status {status})")]
    Rejected {
        status: u16,
        message: Option<String>,
    },

    /// The provider declined to answer (Anthropic `stop_reason: "refusal"`, OpenAI
    /// `content_filter` or `refusal`). Partial text is discarded.
    #[error("the AI provider declined the request")]
    Refused {
        category: Option<String>,
        explanation: Option<String>,
    },

    /// The answer stopped at the output limit (`max_tokens`, `length`).
    #[error("the answer was cut off at the output limit")]
    Truncated,

    /// The request is larger than [`crate::HttpLimits::max_request_bytes`] or the provider
    /// said so (413).
    #[error("the request is too large for the AI provider")]
    RequestTooLarge,

    /// The answer is larger than the limits.
    #[error("the answer of the AI provider is too large")]
    ResponseTooLarge,

    /// The stream or body does not follow the protocol (broken SSE, invalid JSON, ended early).
    #[error("unexpected answer from the AI provider: {0}")]
    Protocol(String),

    /// The model's text is not the requested structure.
    #[error("the model's answer could not be read: {0}")]
    InvalidOutput(String),

    /// A bug or a failing system facility (random numbers).
    #[error("internal error: {0}")]
    Internal(String),
}

/// Which time limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeoutPhase {
    /// TCP connect and TLS handshake.
    Connect,
    /// Waiting for the response headers (a local model may still be loading).
    Response,
    /// No new bytes in the stream.
    Idle,
    /// The whole request.
    Total,
}

impl std::fmt::Display for TimeoutPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Connect => "connect",
            Self::Response => "response",
            Self::Idle => "idle",
            Self::Total => "total",
        })
    }
}

/// Stable error codes for the interface (snake_case on the wire later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiErrorCode {
    Cancelled,
    InvalidConfig,
    NothingToSend,
    NotPermitted,
    Connect,
    Timeout,
    Unauthorized,
    NotFound,
    RateLimited,
    Overloaded,
    Server,
    Rejected,
    Refused,
    Truncated,
    RequestTooLarge,
    ResponseTooLarge,
    Protocol,
    InvalidOutput,
    Internal,
}

impl AiError {
    pub fn code(&self) -> AiErrorCode {
        match self {
            Self::Cancelled => AiErrorCode::Cancelled,
            Self::InvalidConfig(_) => AiErrorCode::InvalidConfig,
            Self::NothingToSend => AiErrorCode::NothingToSend,
            Self::NotPermitted { .. } => AiErrorCode::NotPermitted,
            Self::Connect(_) => AiErrorCode::Connect,
            Self::Timeout(_) => AiErrorCode::Timeout,
            Self::Unauthorized { .. } => AiErrorCode::Unauthorized,
            Self::NotFound { .. } => AiErrorCode::NotFound,
            Self::RateLimited { .. } => AiErrorCode::RateLimited,
            Self::Overloaded { .. } => AiErrorCode::Overloaded,
            Self::Server { .. } => AiErrorCode::Server,
            Self::Rejected { .. } => AiErrorCode::Rejected,
            Self::Refused { .. } => AiErrorCode::Refused,
            Self::Truncated => AiErrorCode::Truncated,
            Self::RequestTooLarge => AiErrorCode::RequestTooLarge,
            Self::ResponseTooLarge => AiErrorCode::ResponseTooLarge,
            Self::Protocol(_) => AiErrorCode::Protocol,
            Self::InvalidOutput(_) => AiErrorCode::InvalidOutput,
            Self::Internal(_) => AiErrorCode::Internal,
        }
    }

    /// Worth trying again later (the interface offers "try again"; nothing retries by itself,
    /// so a request is never sent twice without the user).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Connect(_)
                | Self::Timeout(_)
                | Self::RateLimited { .. }
                | Self::Overloaded { .. }
                | Self::Server { .. }
        )
    }

    /// How long the provider asked to wait, if it said so.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } | Self::Overloaded { retry_after } => *retry_after,
            _ => None,
        }
    }
}

/// A message from a server, made safe for an error: the key (if the server echoed it) becomes
/// `[redacted]`, control and invisible characters are removed, and the text is cut to
/// [`MAX_MESSAGE_CHARS`]. `None` for an empty message.
pub(crate) fn server_message(raw: &str, key: Option<&Secret<String>>) -> Option<String> {
    let mut text = raw.to_owned();
    if let Some(key) = key {
        let key = key.expose_secret();
        // Short keys (and empty ones) would redact ordinary words; real keys are long.
        if key.len() >= 8 {
            // `Secret`'s own redaction text, so there is one spelling of it.
            text = text.replace(key.as_str(), &Secret::new(()).to_string());
        }
    }
    let cleaned = crate::text::clean_line(&text, MAX_MESSAGE_CHARS);
    (!cleaned.is_empty()).then_some(cleaned)
}
