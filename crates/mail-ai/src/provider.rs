//! The provider interface: one chat request (fixed system text plus one user message, never
//! tools), answered as a stream; cancelling; the list of models.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;

use crate::endpoint::Locality;
use crate::{AiError, CancelToken};

/// A boxed `Send` future, so [`AiProvider`] can be used as `dyn AiProvider`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Which wire protocol a profile speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    /// `POST {base}/chat/completions`, `GET {base}/models`: Ollama, LM Studio, llama.cpp
    /// server, OpenRouter, OpenAI and others. The base URL includes `/v1`.
    OpenAiCompatible,
    /// `POST {base}/v1/messages`, `GET {base}/v1/models` (Anthropic, decision 0019).
    AnthropicMessages,
}

/// What the interface shows about a profile ("an wen").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub kind: ProviderKind,
    /// Host and port of the base URL.
    pub host: String,
    pub locality: Locality,
}

/// A model the provider offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: Option<String>,
}

/// An AI provider. Implementations send exactly what a [`ChatRequest`] holds: no tools, no
/// function definitions, no sampling parameters, no conversation state.
pub trait AiProvider: Send + Sync {
    fn info(&self) -> &ProviderInfo;

    /// Starts a request; the stream delivers the answer. Dropping the stream or cancelling
    /// `cancel` closes the connection.
    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<ChatStream, AiError>>;

    /// The models of this provider, for the profile settings.
    fn list_models<'a>(
        &'a self,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Vec<ModelInfo>, AiError>>;
}

/// The JSON structure the answer must have. Providers that support it are asked for it
/// (structured outputs); the answer is parsed and checked in any case.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputSchema {
    /// `[a-z_]+`, for OpenAI's `json_schema.name`.
    pub name: &'static str,
    pub schema: serde_json::Value,
}

/// One request. Only this crate builds requests (fields are private): the system text is
/// always one of the fixed texts of this crate (module `prompt`), and mail content only ever sits inside
/// the delimited data block of the user message.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub(crate) model: String,
    pub(crate) system: String,
    pub(crate) user: String,
    pub(crate) output: Option<OutputSchema>,
}

impl ChatRequest {
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn system(&self) -> &str {
        &self.system
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn output_schema(&self) -> Option<&OutputSchema> {
        self.output.as_ref()
    }

    /// Bytes of text that leave the device (system text and user message).
    pub fn text_bytes(&self) -> usize {
        self.system.len() + self.user.len()
    }

    /// A request without mail content, for a "test connection" button: the model answers
    /// with a short word. Uses no output schema.
    pub fn connection_test(model: &str) -> Result<Self, AiError> {
        Ok(Self {
            model: crate::prompt::check_model(model)?,
            system: crate::prompt::CONNECTION_TEST_SYSTEM.to_owned(),
            user: crate::prompt::CONNECTION_TEST_USER.to_owned(),
            output: None,
        })
    }
}

/// Why the model stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Finished normally (`end_turn`, `stop`, `stop_sequence`).
    Complete,
    /// Output limit reached (`max_tokens`, `length`, `model_context_window_exceeded`).
    MaxTokens,
    /// The provider declined (Anthropic `refusal` with `stop_details`, OpenAI `refusal` text or
    /// `content_filter`).
    Refused {
        category: Option<String>,
        explanation: Option<String>,
    },
    /// Anything else the provider reported (kept for the log).
    Other(String),
}

/// Token counts as reported (`None` when the provider did not report them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// The end of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finish {
    pub stop: StopReason,
    pub usage: Usage,
    /// The model that produced the answer, as reported. With Anthropic's server-side
    /// fallback this can differ from the requested model.
    pub model: Option<String>,
    /// The provider's request id (`request-id`, `x-request-id`), for the request log.
    pub request_id: Option<String>,
}

/// One item of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// More answer text. Shown live at most; on a refusal it is void.
    Text(String),
    /// The last event.
    Finished(Finish),
}

/// Turns provider events into [`StreamEvent`]s (one per provider kind).
pub(crate) trait Decoder: Send {
    /// One server-sent event; may yield nothing (ping) or several items.
    fn event(&mut self, event: crate::sse::SseEvent) -> Result<Vec<StreamEvent>, AiError>;
    /// The stream ended without the provider's end marker: a [`Finish`] if what arrived is
    /// complete, else an error.
    fn end(&mut self) -> Result<Finish, AiError>;
}

/// A live HTTP stream (boxed: much larger than a scripted one).
struct HttpSource {
    reader: crate::http::EventReader,
    decoder: Box<dyn Decoder>,
    request_id: Option<String>,
}

enum Source {
    Http(Box<HttpSource>),
    Scripted(VecDeque<Result<StreamEvent, AiError>>),
}

/// The answer as it arrives. Ends with exactly one [`StreamEvent::Finished`] or an error;
/// after that `next` returns `None`.
pub struct ChatStream {
    source: Source,
    cancel: CancelToken,
    pending: VecDeque<StreamEvent>,
    done: bool,
    chars: usize,
    max_chars: usize,
}

impl std::fmt::Debug for ChatStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatStream")
            .field("done", &self.done)
            .field("chars", &self.chars)
            .finish_non_exhaustive()
    }
}

impl ChatStream {
    pub(crate) fn http(
        reader: crate::http::EventReader,
        decoder: Box<dyn Decoder>,
        cancel: CancelToken,
        max_chars: usize,
    ) -> Self {
        let request_id = ["request-id", "x-request-id"].iter().find_map(|name| {
            reader
                .headers()
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .map(|value| crate::text::clean_line(value, 100))
        });
        Self {
            source: Source::Http(Box::new(HttpSource {
                reader,
                decoder,
                request_id,
            })),
            cancel,
            pending: VecDeque::new(),
            done: false,
            chars: 0,
            max_chars,
        }
    }

    /// A stream that plays back `events`, for tests of code that uses a provider (the
    /// integration's tests use it too). The same checks apply as for a real stream.
    pub fn scripted(events: Vec<Result<StreamEvent, AiError>>, cancel: CancelToken) -> Self {
        Self {
            source: Source::Scripted(events.into()),
            cancel,
            pending: VecDeque::new(),
            done: false,
            chars: 0,
            max_chars: crate::HttpLimits::default().max_output_chars,
        }
    }

    /// The next item; `None` after the end or after an error.
    pub async fn next(&mut self) -> Option<Result<StreamEvent, AiError>> {
        if self.done {
            return None;
        }
        let item = self.next_inner().await;
        match &item {
            Ok(StreamEvent::Text(text)) => {
                self.chars += text.chars().count();
                if self.chars > self.max_chars {
                    self.finish();
                    return Some(Err(AiError::ResponseTooLarge));
                }
            }
            Ok(StreamEvent::Finished(_)) | Err(_) => self.finish(),
        }
        Some(item)
    }

    fn finish(&mut self) {
        self.done = true;
        // Drops the connection now rather than when the stream is dropped.
        self.source = Source::Scripted(VecDeque::new());
    }

    async fn next_inner(&mut self) -> Result<StreamEvent, AiError> {
        if self.cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }
        if let Some(event) = self.pending.pop_front() {
            return Ok(event);
        }
        match &mut self.source {
            Source::Scripted(events) => events
                .pop_front()
                .unwrap_or_else(|| Err(AiError::Protocol("stream ended early".into()))),
            Source::Http(http) => loop {
                let HttpSource {
                    reader,
                    decoder,
                    request_id,
                } = &mut **http;
                let Some(event) = reader.next(&self.cancel).await? else {
                    let mut finish = decoder.end()?;
                    finish.request_id = finish.request_id.or_else(|| request_id.clone());
                    return Ok(StreamEvent::Finished(finish));
                };
                let mut items: VecDeque<StreamEvent> = decoder.event(event)?.into();
                for item in &mut items {
                    if let StreamEvent::Finished(finish) = item {
                        finish.request_id = finish.request_id.take().or_else(|| request_id.clone());
                    }
                }
                if let Some(first) = items.pop_front() {
                    self.pending = items;
                    return Ok(first);
                }
            },
        }
    }
}

/// A finished answer. The text is only reachable through [`Completed::into_text`], which
/// checks the stop reason first: a refused or cut-off answer is never read as if complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completed {
    text: String,
    pub finish: Finish,
}

impl Completed {
    /// The text, if the model finished normally; [`AiError::Refused`] or
    /// [`AiError::Truncated`] otherwise.
    pub fn into_text(self) -> Result<String, AiError> {
        match self.finish.stop {
            StopReason::Complete => Ok(self.text),
            StopReason::MaxTokens => Err(AiError::Truncated),
            StopReason::Refused {
                category,
                explanation,
            } => Err(AiError::Refused {
                category,
                explanation,
            }),
            StopReason::Other(reason) => Err(AiError::Protocol(format!(
                "the model stopped for an unexpected reason ({reason})"
            ))),
        }
    }

    /// Characters received (also for refused answers, for the log).
    pub fn chars(&self) -> usize {
        self.text.chars().count()
    }
}

/// Reads a stream to its end. `progress` gets the number of characters so far after every
/// piece of text (for a progress display; the text itself is not handed out before the stop
/// reason is known).
pub async fn collect(
    mut stream: ChatStream,
    mut progress: impl FnMut(usize) + Send,
) -> Result<Completed, AiError> {
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        match item? {
            StreamEvent::Text(piece) => {
                text.push_str(&piece);
                progress(stream.chars);
            }
            StreamEvent::Finished(finish) => return Ok(Completed { text, finish }),
        }
    }
    Err(AiError::Protocol("stream ended early".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish(stop: StopReason) -> StreamEvent {
        StreamEvent::Finished(Finish {
            stop,
            usage: Usage::default(),
            model: None,
            request_id: None,
        })
    }

    #[tokio::test]
    async fn stop_reason_is_checked_before_the_text() {
        let refused = ChatStream::scripted(
            vec![
                Ok(StreamEvent::Text("Hello..".into())),
                Ok(finish(StopReason::Refused {
                    category: Some("cyber".into()),
                    explanation: None,
                })),
            ],
            CancelToken::new(),
        );
        let completed = collect(refused, |_| {}).await.expect("stream");
        assert_eq!(completed.chars(), 7);
        assert_eq!(
            completed.into_text(),
            Err(AiError::Refused {
                category: Some("cyber".into()),
                explanation: None
            })
        );

        let cut = ChatStream::scripted(
            vec![
                Ok(StreamEvent::Text("{\"a\":".into())),
                Ok(finish(StopReason::MaxTokens)),
            ],
            CancelToken::new(),
        );
        assert_eq!(
            collect(cut, |_| {}).await.expect("stream").into_text(),
            Err(AiError::Truncated)
        );

        let mut seen = Vec::new();
        let fine = ChatStream::scripted(
            vec![
                Ok(StreamEvent::Text("ab".into())),
                Ok(StreamEvent::Text("cd".into())),
                Ok(finish(StopReason::Complete)),
            ],
            CancelToken::new(),
        );
        let text = collect(fine, |n| seen.push(n))
            .await
            .expect("stream")
            .into_text();
        assert_eq!(text.as_deref(), Ok("abcd"));
        assert_eq!(seen, vec![2, 4]);
    }

    #[tokio::test]
    async fn streams_end_once_and_honour_cancel_and_size() {
        let empty =
            ChatStream::scripted(vec![Ok(StreamEvent::Text("x".into()))], CancelToken::new());
        assert!(matches!(
            collect(empty, |_| {}).await,
            Err(AiError::Protocol(_))
        ));

        let cancel = CancelToken::new();
        let mut stream =
            ChatStream::scripted(vec![Ok(StreamEvent::Text("x".into()))], cancel.clone());
        cancel.cancel();
        assert_eq!(stream.next().await, Some(Err(AiError::Cancelled)));
        assert_eq!(stream.next().await, None);

        let mut big = ChatStream::scripted(
            vec![
                Ok(StreamEvent::Text("abc".into())),
                Ok(finish(StopReason::Complete)),
            ],
            CancelToken::new(),
        );
        big.max_chars = 2;
        assert_eq!(big.next().await, Some(Err(AiError::ResponseTooLarge)));
        assert_eq!(big.next().await, None);
    }
}
