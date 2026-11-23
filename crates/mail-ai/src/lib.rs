//! Optional AI in the app (stage 13, decisions 0010 and 0019): the provider layer and the
//! core logic around it, without interface and without access to store, outbox or mail
//! servers.
//!
//! - [`AiProvider`]: one request with a fixed system text and one user message, answered as a
//!   [`ChatStream`] (cancel with [`CancelToken`] or by dropping it), and the model list. Two
//!   implementations, built by [`build_provider`] from a [`ProviderConfig`]:
//!   [`OpenAiCompatible`] (Ollama, LM Studio, llama.cpp server, OpenRouter, OpenAI, any
//!   base URL) and [`AnthropicMessages`] (decision 0019).
//! - Transport ([`HttpLimits`], module `http`): TLS from the caller, no cookies, no referrer,
//!   no proxy, no redirects, no retries, neutral `User-Agent`, time and size limits. The
//!   "local" label ([`locality_of`]) is kept at connect time.
//! - Prompts (module `prompt`): fixed system texts, mail content only inside a data block with
//!   a random nonce, no tools, JSON answers.
//! - Data minimisation ([`MailInput`], [`Budget`], [`TransferInfo`]): only the thread or the
//!   candidates, known quotes removed, attachments as names, a visible length budget.
//! - [`prepare_summary`]/[`summarize`] and [`ask`] (with [`prepare_query`], [`parse_query`],
//!   [`prepare_answer`]): every statement keeps only references to messages that were sent;
//!   no candidates means "nothing found" without a model call.
//! - [`CloudPolicy`]: accounts without cloud permission never reach a provider that is not
//!   local.
//!
//! Model output is returned as checked data ([`Summary`], [`Answer`]) of plain text and
//! message ids. Nothing in this crate turns it into an action or a network request.

mod anthropic;
mod ask;
mod cancel;
mod claims;
mod endpoint;
mod error;
mod http;
mod openai;
mod policy;
mod prepare;
mod profile;
mod prompt;
mod provider;
mod sse;
mod summary;
mod text;

pub use anthropic::{
    ANTHROPIC_BASE_URL, ANTHROPIC_VERSION, AnthropicMessages, DEFAULT_MAX_TOKENS,
    DEFAULT_MODEL as ANTHROPIC_DEFAULT_MODEL,
};
pub use ask::{
    Answer, AnswerOutcome, AskRequest, CandidateSource, MAX_QUESTION_CHARS, PreparedAnswer,
    PreparedQuery, QueryPlan, ask, parse_query, prepare_answer, prepare_query,
};
pub use cancel::CancelToken;
pub use claims::Claim;
pub use endpoint::{Locality, ip_locality, locality, locality_of};
pub use error::{AiError, AiErrorCode, TimeoutPhase};
pub use http::{HttpLimits, NEUTRAL_USER_AGENT};
pub use openai::OpenAiCompatible;
pub use policy::CloudPolicy;
pub use prepare::{
    Budget, MAX_ATTACHMENT_NAMES, MAX_RECIPIENTS, MailInput, TransferInfo, Trimming, visible_text,
};
pub use profile::{PRESETS, Preset, ProviderConfig, build_provider};
pub use prompt::{ANSWER_TASK, AnswerLanguage, DATA_RULES, QUERY_SYSTEM, SUMMARY_TASK};
pub use provider::{
    AiProvider, BoxFuture, ChatRequest, ChatStream, Completed, Finish, ModelInfo, OutputSchema,
    ProviderInfo, ProviderKind, StopReason, StreamEvent, Usage, collect,
};
pub use summary::{Context, Deadline, PreparedSummary, Summary, prepare_summary, summarize};

/// A TLS configuration for unit tests, which only talk plain HTTP to local servers. Built
/// with the process default provider (whichever one rustls was compiled with), so the tests
/// do not name a provider; the product passes `mail_imap::tls::client_config`.
#[cfg(test)]
pub(crate) fn test_tls() -> rustls::ClientConfig {
    rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth()
}
