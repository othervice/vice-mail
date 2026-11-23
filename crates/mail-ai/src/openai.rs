//! The OpenAI-compatible chat interface (`POST {base}/chat/completions` with `stream: true`,
//! `GET {base}/models`), as Ollama, LM Studio, the llama.cpp server, OpenRouter and OpenAI
//! speak it.
//!
//! The request holds the model, a system and a user message, `stream: true`, and only when
//! configured `max_tokens` and `response_format` (`json_schema`, strict). Nothing else: no
//! `tools`, `functions` or `tool_choice`, no `temperature`/`top_p` (some models reject any
//! value other than the default), no `user`, `store` or `metadata`.
//!
//! The stream: `data: {chunk}` events with `choices[0].delta.content`, a `finish_reason`
//! (`stop`, `length`, `content_filter`), `data: [DONE]` at the end; `delta.refusal` (OpenAI)
//! and `content_filter` count as a refusal; an `error` object in the stream (OpenRouter,
//! Ollama) ends it with the matching error. Reasoning fields (`delta.reasoning`, …) are ignored.

use mail_model::Secret;
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::json;

use crate::endpoint::Endpoint;
use crate::http::{EventReader, HttpClient, header, status_error};
use crate::profile::ProviderConfig;
use crate::provider::{
    AiProvider, BoxFuture, ChatRequest, ChatStream, Decoder, Finish, ModelInfo, ProviderInfo,
    ProviderKind, StopReason, StreamEvent, Usage,
};
use crate::sse::SseEvent;
use crate::text::clean_line;
use crate::{AiError, CancelToken};

/// Most models listed.
const MAX_MODELS: usize = 2000;

#[derive(Debug)]
pub struct OpenAiCompatible {
    endpoint: Endpoint,
    key: Option<Secret<String>>,
    http: HttpClient,
    structured_output: bool,
    max_output_tokens: Option<u32>,
    info: ProviderInfo,
}

impl OpenAiCompatible {
    pub fn new(config: ProviderConfig, tls: &rustls::ClientConfig) -> Result<Self, AiError> {
        let endpoint = Endpoint::parse(&config.base_url)?;
        let http = HttpClient::new(tls, config.limits, &endpoint)?;
        let info = ProviderInfo {
            kind: ProviderKind::OpenAiCompatible,
            host: endpoint.host().to_owned(),
            locality: endpoint.locality(),
        };
        Ok(Self {
            endpoint,
            key: config.api_key,
            http,
            structured_output: config.structured_output,
            max_output_tokens: config.max_output_tokens,
            info,
        })
    }

    fn headers(&self, accept: &'static str) -> Result<HeaderMap, AiError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        if let Some(key) = &self.key {
            let (_, value) = header("authorization", &format!("Bearer {}", key.expose_secret()))?;
            headers.insert(AUTHORIZATION, value);
        }
        Ok(headers)
    }

    /// The request body (public within the crate for the contract tests).
    pub(crate) fn body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = json!({
            "model": request.model,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.user},
            ],
            "stream": true,
        });
        if let Some(max) = self.max_output_tokens {
            body["max_tokens"] = json!(max);
        }
        if let (true, Some(output)) = (self.structured_output, &request.output) {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {"name": output.name, "strict": true, "schema": output.schema},
            });
        }
        body
    }
}

impl AiProvider for OpenAiCompatible {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<ChatStream, AiError>> {
        Box::pin(async move {
            let url = self.endpoint.url("/chat/completions")?;
            let body = serde_json::to_vec(&self.body(request))
                .map_err(|err| AiError::Internal(format!("request: {err}")))?;
            let response = self
                .http
                .post_json(
                    url,
                    self.headers("text/event-stream")?,
                    body,
                    self.key.as_ref(),
                    cancel,
                )
                .await?;
            let limits = self.http.limits();
            Ok(ChatStream::http(
                EventReader::new(response, limits.max_event_bytes),
                Box::new(OpenAiDecoder::new(self.key.clone())),
                cancel.clone(),
                limits.max_output_chars,
            ))
        })
    }

    fn list_models<'a>(
        &'a self,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Vec<ModelInfo>, AiError>> {
        Box::pin(async move {
            let url = self.endpoint.url("/models")?;
            let json = self
                .http
                .get_json(
                    url,
                    self.headers("application/json")?,
                    self.key.as_ref(),
                    cancel,
                )
                .await?;
            let data = json
                .get("data")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| AiError::Protocol("model list without data".into()))?;
            Ok(data
                .iter()
                .filter_map(|model| {
                    let id = clean_line(model.get("id")?.as_str()?, 200);
                    let display_name = model
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(|name| clean_line(name, 200))
                        .filter(|name| !name.is_empty() && *name != id);
                    (!id.is_empty()).then_some(ModelInfo { id, display_name })
                })
                .take(MAX_MODELS)
                .collect())
        })
    }
}

/// Reads `chat.completion.chunk` events.
struct OpenAiDecoder {
    key: Option<Secret<String>>,
    finish_reason: Option<String>,
    refusal: String,
    usage: Usage,
    model: Option<String>,
}

impl OpenAiDecoder {
    fn new(key: Option<Secret<String>>) -> Self {
        Self {
            key,
            finish_reason: None,
            refusal: String::new(),
            usage: Usage::default(),
            model: None,
        }
    }

    fn finish(&mut self) -> Finish {
        let stop = if !self.refusal.trim().is_empty() {
            StopReason::Refused {
                category: None,
                explanation: Some(clean_line(&self.refusal, 300)),
            }
        } else {
            match self.finish_reason.as_deref() {
                None | Some("stop" | "stop_sequence" | "end_turn" | "eos") => StopReason::Complete,
                Some("length" | "max_tokens") => StopReason::MaxTokens,
                Some("content_filter") => StopReason::Refused {
                    category: Some("content_filter".into()),
                    explanation: None,
                },
                Some(other) => StopReason::Other(clean_line(other, 50)),
            }
        };
        Finish {
            stop,
            usage: self.usage,
            model: self.model.clone(),
            request_id: None,
        }
    }
}

impl Decoder for OpenAiDecoder {
    fn event(&mut self, event: SseEvent) -> Result<Vec<StreamEvent>, AiError> {
        let data = event.data.trim();
        if data == "[DONE]" {
            return Ok(vec![StreamEvent::Finished(self.finish())]);
        }
        let chunk: serde_json::Value = serde_json::from_str(data)
            .map_err(|_| AiError::Protocol("invalid JSON in the event stream".into()))?;
        if let Some(error) = chunk.get("error") {
            return Err(stream_error(error, self.key.as_ref()));
        }
        if let Some(model) = chunk.get("model").and_then(serde_json::Value::as_str) {
            self.model = Some(clean_line(model, 200));
        }
        if let Some(usage) = chunk.get("usage").filter(|usage| usage.is_object()) {
            self.usage = Usage {
                input_tokens: usage
                    .get("prompt_tokens")
                    .and_then(serde_json::Value::as_u64),
                output_tokens: usage
                    .get("completion_tokens")
                    .and_then(serde_json::Value::as_u64),
            };
        }
        let mut events = Vec::new();
        let choice = chunk
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first());
        if let Some(choice) = choice {
            let delta = choice.get("delta");
            let field = |name: &str| {
                delta
                    .and_then(|delta| delta.get(name))
                    .and_then(serde_json::Value::as_str)
            };
            if let Some(text) = field("content").filter(|text| !text.is_empty()) {
                events.push(StreamEvent::Text(text.to_owned()));
            }
            if let Some(refusal) = field("refusal") {
                self.refusal.push_str(refusal);
            }
            if let Some(reason) = choice
                .get("finish_reason")
                .and_then(serde_json::Value::as_str)
            {
                self.finish_reason = Some(reason.to_owned());
            }
        }
        Ok(events)
    }

    fn end(&mut self) -> Result<Finish, AiError> {
        // Some servers end without `[DONE]`; a finish reason means the answer is complete.
        if self.finish_reason.is_some() {
            Ok(self.finish())
        } else {
            Err(AiError::Protocol(
                "the stream ended before the answer was complete".into(),
            ))
        }
    }
}

/// An `error` object inside the stream: its numeric `code` (OpenRouter) as status if there is
/// one, else a server error.
fn stream_error(error: &serde_json::Value, key: Option<&Secret<String>>) -> AiError {
    let status = error
        .get("code")
        .and_then(serde_json::Value::as_u64)
        .and_then(|code| u16::try_from(code).ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .filter(|status| status.is_client_error() || status.is_server_error())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = json!({ "error": error }).to_string();
    status_error(
        status,
        &HeaderMap::new(),
        body.as_bytes(),
        key,
        std::time::SystemTime::now(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(data: &str) -> SseEvent {
        SseEvent {
            name: None,
            data: data.to_owned(),
        }
    }

    fn run(events: &[&str]) -> (String, Result<Finish, AiError>) {
        let mut decoder = OpenAiDecoder::new(None);
        let mut text = String::new();
        for data in events {
            match decoder.event(event(data)) {
                Ok(items) => {
                    for item in items {
                        match item {
                            StreamEvent::Text(piece) => text.push_str(&piece),
                            StreamEvent::Finished(finish) => return (text, Ok(finish)),
                        }
                    }
                }
                Err(err) => return (text, Err(err)),
            }
        }
        let end = decoder.end();
        (text, end)
    }

    #[test]
    fn chunks_finish_reasons_and_refusals() {
        let (text, finish) = run(&[
            r#"{"model":"llama3.2","choices":[{"index":0,"delta":{"role":"assistant","content":""}}]}"#,
            r#"{"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"{"choices":[{"delta":{"reasoning":"hmm","content":"lo"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#,
            "[DONE]",
        ]);
        let finish = finish.expect("finish");
        assert_eq!(text, "Hello");
        assert_eq!(finish.stop, StopReason::Complete);
        assert_eq!(finish.model.as_deref(), Some("llama3.2"));
        assert_eq!(finish.usage.input_tokens, Some(12));

        let (_, finish) =
            run(&[r#"{"choices":[{"delta":{"content":"x"},"finish_reason":"length"}]}"#]);
        assert_eq!(finish.expect("finish").stop, StopReason::MaxTokens);
        let (_, finish) = run(&[
            r#"{"choices":[{"delta":{"refusal":"I can't help with that."}}]}"#,
            "[DONE]",
        ]);
        assert!(matches!(
            finish.expect("finish").stop,
            StopReason::Refused { .. }
        ));
        let (_, finish) = run(&[
            r#"{"choices":[{"delta":{},"finish_reason":"content_filter"}]}"#,
            "[DONE]",
        ]);
        assert_eq!(
            finish.expect("finish").stop,
            StopReason::Refused {
                category: Some("content_filter".into()),
                explanation: None
            }
        );
    }

    #[test]
    fn broken_streams_are_errors() {
        let (_, finish) = run(&[r#"{"choices":[{"delta":{"content":"x"}}]}"#]);
        assert!(matches!(finish, Err(AiError::Protocol(_))));
        let (_, finish) = run(&["{not json"]);
        assert!(matches!(finish, Err(AiError::Protocol(_))));
        let (_, finish) = run(&[r#"{"error":{"message":"Provider returned error","code":502}}"#]);
        assert!(matches!(finish, Err(AiError::Server { status: 502, .. })));
        let (_, finish) = run(&[r#"{"error":{"message":"slow down","code":429}}"#]);
        assert!(matches!(finish, Err(AiError::RateLimited { .. })));
        let (_, finish) = run(&[r#"{"error":{"message":"model crashed","type":"api_error"}}"#]);
        assert!(matches!(finish, Err(AiError::Server { status: 500, .. })));
    }
}
