//! The Anthropic Messages API (decision 0019): `POST {base}/v1/messages` with `stream: true`,
//! `GET {base}/v1/models`. Raw HTTP after the API documentation (there is no official Rust
//! SDK).
//!
//! Request: `x-api-key`, `anthropic-version: 2026-10-21`; body with `model`, `max_tokens`,
//! `system`, one user message, `stream`, and `output_config.format` (JSON schema) when
//! structured output is on. No `temperature`, `top_p` or `top_k` (rejected on this API
//! version), no `thinking` (current models choose adaptive thinking themselves; the stream's
//! thinking blocks are ignored), no `tools`. For models with server-side refusal fallback,
//! `fallbacks: "default"` with the beta header `server-side-fallback-2026-07-01`, only towards
//! `api.anthropic.com`.
//!
//! Stream: `message_start` (model), `content_block_start`/`_delta`/`_stop` (only `text_delta`
//! is answer text; a `fallback` block names the model that continues), `message_delta` with
//! `stop_reason` and `stop_details`, `message_stop`; `ping` is ignored, unknown events too
//! (the API may add some); an `error` event ends the stream with the matching error. The stop
//! reason is checked before the text is used ([`crate::Completed::into_text`]): `refusal`
//! (HTTP 200) becomes [`AiError::Refused`] with `stop_details.category` and `explanation`,
//! `max_tokens` and `model_context_window_exceeded` become [`AiError::Truncated`].

use mail_model::Secret;
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderMap, HeaderName, HeaderValue};
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

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
/// The API version sent with every request.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Suggested model for a new Anthropic profile (the user chooses from the model list).
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// `max_tokens` when the profile sets none: room for adaptive thinking and a summary.
pub const DEFAULT_MAX_TOKENS: u32 = 16_000;
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Model families with server-side refusal fallback (Anthropic documentation, 11/2026).
const FALLBACK_MODELS: &[&str] = &[
    "claude-fable-5",
    "claude-mythos-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];
/// Pages of the model list read at most (1000 models each).
const MAX_MODEL_PAGES: usize = 5;

#[derive(Debug)]
pub struct AnthropicMessages {
    endpoint: Endpoint,
    key: Option<Secret<String>>,
    http: HttpClient,
    structured_output: bool,
    max_tokens: u32,
    refusal_fallback: bool,
    info: ProviderInfo,
}

impl AnthropicMessages {
    pub fn new(config: ProviderConfig, tls: &rustls::ClientConfig) -> Result<Self, AiError> {
        let base = if config.base_url.trim().is_empty() {
            ANTHROPIC_BASE_URL
        } else {
            config.base_url.as_str()
        };
        let endpoint = Endpoint::parse(base)?;
        let http = HttpClient::new(tls, config.limits, &endpoint)?;
        let info = ProviderInfo {
            kind: ProviderKind::AnthropicMessages,
            host: endpoint.host().to_owned(),
            locality: endpoint.locality(),
        };
        Ok(Self {
            // Server-side fallback exists only on Anthropic's own API (not behind proxies).
            refusal_fallback: config.refusal_fallback && endpoint.host() == "api.anthropic.com",
            endpoint,
            key: config.api_key,
            http,
            structured_output: config.structured_output,
            max_tokens: config
                .max_output_tokens
                .unwrap_or(DEFAULT_MAX_TOKENS)
                .max(1),
            info,
        })
    }

    fn url(&self, path: &str) -> Result<reqwest::Url, AiError> {
        if self.endpoint.ends_with("/v1") {
            self.endpoint.url(path)
        } else {
            self.endpoint.url(&format!("/v1{path}"))
        }
    }

    fn uses_fallback(&self, model: &str) -> bool {
        self.refusal_fallback
            && FALLBACK_MODELS
                .iter()
                .any(|family| model.starts_with(family))
    }

    fn headers(&self, accept: &'static str, fallback: bool) -> Result<HeaderMap, AiError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if let Some(key) = &self.key {
            let (name, value) = header("x-api-key", key.expose_secret())?;
            headers.insert(name, value);
        }
        if fallback {
            headers.insert(
                HeaderName::from_static("anthropic-beta"),
                HeaderValue::from_static(FALLBACK_BETA),
            );
        }
        Ok(headers)
    }

    pub(crate) fn body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = json!({
            "model": request.model,
            "max_tokens": self.max_tokens,
            "system": request.system,
            "messages": [{"role": "user", "content": request.user}],
            "stream": true,
        });
        if let (true, Some(output)) = (self.structured_output, &request.output) {
            body["output_config"] = json!({
                "format": {"type": "json_schema", "schema": output.schema},
            });
        }
        if self.uses_fallback(&request.model) {
            body["fallbacks"] = json!("default");
        }
        body
    }
}

impl AiProvider for AnthropicMessages {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<ChatStream, AiError>> {
        Box::pin(async move {
            let url = self.url("/messages")?;
            let body = serde_json::to_vec(&self.body(request))
                .map_err(|err| AiError::Internal(format!("request: {err}")))?;
            let headers = self.headers("text/event-stream", self.uses_fallback(&request.model))?;
            let response = self
                .http
                .post_json(url, headers, body, self.key.as_ref(), cancel)
                .await?;
            let limits = self.http.limits();
            Ok(ChatStream::http(
                EventReader::new(response, limits.max_event_bytes),
                Box::new(AnthropicDecoder::new(self.key.clone())),
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
            let mut models = Vec::new();
            let mut after: Option<String> = None;
            for _ in 0..MAX_MODEL_PAGES {
                let mut url = self.url("/models")?;
                url.query_pairs_mut().append_pair("limit", "1000");
                if let Some(after) = &after {
                    url.query_pairs_mut().append_pair("after_id", after);
                }
                let page = self
                    .http
                    .get_json(
                        url,
                        self.headers("application/json", false)?,
                        self.key.as_ref(),
                        cancel,
                    )
                    .await?;
                let data = page
                    .get("data")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| AiError::Protocol("model list without data".into()))?;
                for model in data {
                    let Some(id) = model.get("id").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    models.push(ModelInfo {
                        id: clean_line(id, 200),
                        display_name: model
                            .get("display_name")
                            .and_then(serde_json::Value::as_str)
                            .map(|name| clean_line(name, 200)),
                    });
                }
                let more = page.get("has_more").and_then(serde_json::Value::as_bool) == Some(true);
                after = page
                    .get("last_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                if !more || after.is_none() {
                    break;
                }
            }
            Ok(models)
        })
    }
}

/// Reads the Messages stream.
struct AnthropicDecoder {
    key: Option<Secret<String>>,
    model: Option<String>,
    stop_reason: Option<String>,
    stop_details: Option<serde_json::Value>,
    usage: Usage,
    stopped: bool,
}

impl AnthropicDecoder {
    fn new(key: Option<Secret<String>>) -> Self {
        Self {
            key,
            model: None,
            stop_reason: None,
            stop_details: None,
            usage: Usage::default(),
            stopped: false,
        }
    }

    fn finish(&self) -> Result<Finish, AiError> {
        let stop = match self.stop_reason.as_deref() {
            Some("end_turn" | "stop_sequence") => StopReason::Complete,
            Some("max_tokens" | "model_context_window_exceeded") => StopReason::MaxTokens,
            Some("refusal") => {
                let detail = |name: &str| {
                    self.stop_details
                        .as_ref()
                        .and_then(|details| details.get(name))
                        .and_then(serde_json::Value::as_str)
                        .map(|text| clean_line(text, 300))
                        .filter(|text| !text.is_empty())
                };
                StopReason::Refused {
                    category: detail("category"),
                    explanation: detail("explanation"),
                }
            }
            Some(other) => StopReason::Other(clean_line(other, 50)),
            None => {
                return Err(AiError::Protocol(
                    "the stream ended without a stop reason".into(),
                ));
            }
        };
        Ok(Finish {
            stop,
            usage: self.usage,
            model: self.model.clone(),
            request_id: None,
        })
    }
}

impl Decoder for AnthropicDecoder {
    fn event(&mut self, event: SseEvent) -> Result<Vec<StreamEvent>, AiError> {
        if self.stopped {
            return Ok(Vec::new());
        }
        let data: serde_json::Value = serde_json::from_str(&event.data)
            .map_err(|_| AiError::Protocol("invalid JSON in the event stream".into()))?;
        let kind = data
            .get("type")
            .and_then(serde_json::Value::as_str)
            .or(event.name.as_deref())
            .unwrap_or_default();
        let text = |value: Option<&serde_json::Value>| {
            value.and_then(serde_json::Value::as_str).map(str::to_owned)
        };
        match kind {
            "message_start" => {
                let message = data.get("message");
                if let Some(model) = text(message.and_then(|m| m.get("model"))) {
                    self.model = Some(clean_line(&model, 200));
                }
                if let Some(input) = message
                    .and_then(|m| m.pointer("/usage/input_tokens"))
                    .and_then(serde_json::Value::as_u64)
                {
                    self.usage.input_tokens = Some(input);
                }
            }
            "content_block_start" => {
                let block = data.get("content_block");
                match text(block.and_then(|b| b.get("type"))).as_deref() {
                    Some("text") => {
                        if let Some(start) = text(block.and_then(|b| b.get("text")))
                            .filter(|start| !start.is_empty())
                        {
                            return Ok(vec![StreamEvent::Text(start)]);
                        }
                    }
                    // Server-side fallback: from here on another model writes.
                    Some("fallback") => {
                        if let Some(model) = text(block.and_then(|b| b.pointer("/to/model"))) {
                            self.model = Some(clean_line(&model, 200));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let delta = data.get("delta");
                if text(delta.and_then(|d| d.get("type"))).as_deref() == Some("text_delta")
                    && let Some(piece) = text(delta.and_then(|d| d.get("text")))
                    && !piece.is_empty()
                {
                    return Ok(vec![StreamEvent::Text(piece)]);
                }
            }
            "message_delta" => {
                if let Some(reason) = text(data.pointer("/delta/stop_reason")) {
                    self.stop_reason = Some(reason);
                }
                if let Some(details) = data
                    .pointer("/delta/stop_details")
                    .filter(|d| d.is_object())
                {
                    self.stop_details = Some(details.clone());
                }
                if let Some(output) = data
                    .pointer("/usage/output_tokens")
                    .and_then(serde_json::Value::as_u64)
                {
                    self.usage.output_tokens = Some(output);
                }
            }
            "message_stop" => {
                self.stopped = true;
                return Ok(vec![StreamEvent::Finished(self.finish()?)]);
            }
            "error" => return Err(stream_error(data.get("error"), self.key.as_ref())),
            // ping, content_block_stop and event types added later.
            _ => {}
        }
        Ok(Vec::new())
    }

    fn end(&mut self) -> Result<Finish, AiError> {
        Err(AiError::Protocol(
            "the stream ended before message_stop".into(),
        ))
    }
}

/// An `error` event: its `type` decides the error, as the HTTP status would.
fn stream_error(error: Option<&serde_json::Value>, key: Option<&Secret<String>>) -> AiError {
    let kind = error
        .and_then(|e| e.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let status = match kind {
        "invalid_request_error" => 400,
        "authentication_error" => 401,
        "billing_error" => 402,
        "permission_error" => 403,
        "not_found_error" => 404,
        "request_too_large" => 413,
        "rate_limit_error" => 429,
        "timeout_error" => 504,
        "overloaded_error" => 529,
        _ => 500,
    };
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
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

    fn feed(events: &[(&str, &str)]) -> (String, Result<Finish, AiError>) {
        let mut decoder = AnthropicDecoder::new(None);
        let mut text = String::new();
        for (name, data) in events {
            let event = SseEvent {
                name: Some((*name).to_owned()),
                data: (*data).to_owned(),
            };
            match decoder.event(event) {
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
        (text, decoder.end())
    }

    const START: (&str, &str) = (
        "message_start",
        r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-opus-5-5","stop_reason":null,"usage":{"input_tokens":25,"output_tokens":1}}}"#,
    );

    #[test]
    fn text_thinking_ping_and_stop() {
        let (text, finish) = feed(&[
            START,
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"secret plan"}}"#,
            ),
            (
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            ),
            ("ping", r#"{"type": "ping"}"#),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hello"}}"#,
            ),
            ("something_new", r#"{"type":"something_new"}"#),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"!"}}"#,
            ),
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        let finish = finish.expect("finish");
        assert_eq!(text, "Hello!");
        assert_eq!(finish.stop, StopReason::Complete);
        assert_eq!(finish.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(
            finish.usage,
            Usage {
                input_tokens: Some(25),
                output_tokens: Some(15)
            }
        );
    }

    #[test]
    fn refusal_with_details_and_fallback_model() {
        let (text, finish) = feed(&[
            START,
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello.."}}"#,
            ),
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"This request was declined because it could enable cyber harm."}},"usage":{"output_tokens":3}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        assert_eq!(text, "Hello..");
        assert_eq!(
            finish.expect("finish").stop,
            StopReason::Refused {
                category: Some("cyber".into()),
                explanation: Some(
                    "This request was declined because it could enable cyber harm.".into()
                )
            }
        );
        // Null category and explanation are normal.
        let (_, finish) = feed(&[
            START,
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":null,"explanation":null}}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        assert_eq!(
            finish.expect("finish").stop,
            StopReason::Refused {
                category: None,
                explanation: None
            }
        );
        let (_, finish) = feed(&[
            START,
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"},"trigger":{"type":"refusal","category":"cyber"}}}"#,
            ),
            (
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        assert_eq!(
            finish.expect("finish").model.as_deref(),
            Some("claude-opus-4-8")
        );
    }

    #[test]
    fn errors_and_cut_streams() {
        let (_, finish) = feed(&[
            START,
            (
                "error",
                r#"{"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}"#,
            ),
        ]);
        assert!(matches!(finish, Err(AiError::Overloaded { .. })));
        let (_, finish) = feed(&[
            START,
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}}"#,
            ),
        ]);
        assert!(matches!(finish, Err(AiError::Protocol(_))));
        let (_, finish) = feed(&[START, ("message_stop", r#"{"type":"message_stop"}"#)]);
        assert!(matches!(finish, Err(AiError::Protocol(_))));
        let (_, finish) = feed(&[
            START,
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        assert_eq!(finish.expect("finish").stop, StopReason::MaxTokens);
        let (_, finish) = feed(&[("message_start", "{oops")]);
        assert!(matches!(finish, Err(AiError::Protocol(_))));
    }

    #[test]
    fn requests_follow_the_documentation() {
        let tls = crate::test_tls();
        let mut config = ProviderConfig::new(
            ProviderKind::AnthropicMessages,
            "",
            Some(Secret::new("sk-ant-test".into())),
        );
        config.max_output_tokens = None;
        let provider = AnthropicMessages::new(config, &tls).expect("provider");
        assert_eq!(
            provider.url("/messages").expect("url").as_str(),
            "https://api.anthropic.com/v1/messages"
        );
        let request = ChatRequest::connection_test(DEFAULT_MODEL).expect("request");
        let body = provider.body(&request);
        assert_eq!(body["max_tokens"], 16_000);
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(1));
        for absent in [
            "temperature",
            "top_p",
            "top_k",
            "tools",
            "tool_choice",
            "thinking",
        ] {
            assert!(body.get(absent).is_none(), "{absent}");
        }
        let headers = provider
            .headers("text/event-stream", true)
            .expect("headers");
        assert_eq!(headers["anthropic-version"], ANTHROPIC_VERSION);
        assert_eq!(headers["anthropic-beta"], FALLBACK_BETA);
        assert!(headers.get("authorization").is_none());
        assert!(headers["x-api-key"].is_sensitive());
        // No fallback for models without it, or behind another host.
        assert!(!provider.uses_fallback("claude-haiku-4-5"));
        assert!(!provider.uses_fallback("claude-opus-4-8"));
        assert!(provider.uses_fallback("claude-sonnet-5-5"));
        let proxy = AnthropicMessages::new(
            ProviderConfig::new(
                ProviderKind::AnthropicMessages,
                "https://llm.example.org/v1",
                None,
            ),
            &tls,
        )
        .expect("proxy");
        assert!(!proxy.uses_fallback(DEFAULT_MODEL));
        assert_eq!(
            proxy.url("/models").expect("url").as_str(),
            "https://llm.example.org/v1/models"
        );
    }
}
