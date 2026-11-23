//! Contract tests of both providers against the local mock server (stage 13 acceptance:
//! streaming, cancelling, errors, rate limiting, refusal). No real AI service is contacted.

mod support;

use std::time::Duration;

use mail_ai::{
    AiError, CancelToken, ChatRequest, ProviderConfig, ProviderKind, StopReason, StreamEvent,
    TimeoutPhase, build_provider, collect,
};
use serde_json::json;
use support::{
    KEY, MockServer, Reply, StreamEnd, anthropic, anthropic_stream, data, event, openai,
    openai_stream, openai_with,
};

fn request() -> ChatRequest {
    ChatRequest::connection_test("test-model").expect("request")
}

/// Text and finish of a whole stream.
async fn run(provider: &dyn mail_ai::AiProvider) -> Result<(String, mail_ai::Finish), AiError> {
    let stream = provider.chat(&request(), &CancelToken::new()).await?;
    let completed = collect(stream, |_| {}).await?;
    let finish = completed.finish.clone();
    Ok((completed.into_text()?, finish))
}

fn assert_no_key(err: &AiError) {
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains("0123456789abcdef"), "key in error: {text}");
    }
}

// --- OpenAI-compatible ---------------------------------------------------------------------

#[tokio::test]
async fn openai_streams_and_sends_only_what_the_request_holds() {
    let server = MockServer::start(vec![openai_stream("OK, connection works.", 4)]).await;
    let provider = openai(&server);
    let (text, finish) = run(provider.as_ref()).await.expect("answer");
    assert_eq!(text, "OK, connection works.");
    assert_eq!(finish.stop, StopReason::Complete);
    assert_eq!(finish.model.as_deref(), Some("test-model"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let sent = &requests[0];
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.path, "/v1/chat/completions");
    assert_eq!(
        sent.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert_eq!(sent.header("user-agent"), Some("Mozilla/5.0"));
    assert_eq!(sent.header("accept"), Some("text/event-stream"));
    assert_eq!(sent.header("content-type"), Some("application/json"));
    for absent in [
        "cookie",
        "referer",
        "origin",
        "x-api-key",
        "accept-encoding",
        "proxy-authorization",
    ] {
        assert_eq!(sent.header(absent), None, "{absent}");
    }
    let body = sent.json();
    let mut keys: Vec<&str> = body
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["messages", "model", "stream"]);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn openai_schema_and_token_limit_only_when_configured() {
    let server = MockServer::start(vec![openai_stream("{}", 10), openai_stream("{}", 10)]).await;
    let zone = mail_model::search::FixedOffset(0);
    let context = mail_ai::Context {
        language: mail_ai::AnswerLanguage::English,
        time_zone: &zone,
        now: 1_795_392_000,
        slash_dates: mail_model::search::SlashDateOrder::DayFirst,
    };
    let query =
        mail_ai::prepare_query("test-model", "invoices from Anna", &context).expect("query");
    let with = openai_with(&server, |mut config| {
        config.max_output_tokens = Some(500);
        config
    });
    let stream = with
        .chat(query.request(), &CancelToken::new())
        .await
        .expect("stream");
    collect(stream, |_| {}).await.expect("answer");
    let without = openai_with(&server, |mut config| {
        config.structured_output = false;
        config
    });
    let stream = without
        .chat(query.request(), &CancelToken::new())
        .await
        .expect("stream");
    collect(stream, |_| {}).await.expect("answer");

    let requests = server.requests();
    let body = requests[0].json();
    assert_eq!(body["max_tokens"], 500);
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(
        body["response_format"]["json_schema"]["name"],
        "search_query"
    );
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    let body = requests[1].json();
    assert!(body.get("response_format").is_none() && body.get("max_tokens").is_none());
    for request in &requests {
        let body = request.json();
        for absent in [
            "tools",
            "functions",
            "tool_choice",
            "temperature",
            "top_p",
            "user",
            "store",
        ] {
            assert!(body.get(absent).is_none(), "{absent}");
        }
    }
}

#[tokio::test]
async fn cancelling_closes_the_connection_and_the_server_sees_it() {
    // A stream that never ends: only the client can stop it.
    let chunk = data(&json!({"choices": [{"delta": {"content": "more "}}]}));
    let server = MockServer::start(vec![
        Reply::sse(vec![chunk.clone()]).ending(StreamEnd::Forever(chunk)),
    ])
    .await;
    let provider = openai(&server);
    let cancel = CancelToken::new();
    let request = request();
    let mut stream = provider.chat(&request, &cancel).await.expect("stream");
    let first = stream.next().await.expect("item").expect("text");
    assert_eq!(first, StreamEvent::Text("more ".into()));
    cancel.cancel();
    let started = std::time::Instant::now();
    assert_eq!(stream.next().await, Some(Err(AiError::Cancelled)));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(stream.next().await, None);
    assert!(
        server.wait_for_abort().await,
        "the server did not see the abort"
    );
}

#[tokio::test]
async fn dropping_the_stream_closes_the_connection() {
    let chunk = data(&json!({"choices": [{"delta": {"content": "x"}}]}));
    let server = MockServer::start(vec![
        Reply::sse(vec![chunk.clone()]).ending(StreamEnd::Forever(chunk)),
    ])
    .await;
    let provider = openai(&server);
    let request = request();
    let mut stream = provider
        .chat(&request, &CancelToken::new())
        .await
        .expect("stream");
    stream.next().await.expect("item").expect("text");
    drop(stream);
    assert!(
        server.wait_for_abort().await,
        "the server did not see the abort"
    );
}

#[tokio::test]
async fn cancelling_before_the_answer_arrives() {
    let server = MockServer::start(vec![Reply::Silent]).await;
    let provider = openai(&server);
    let cancel = CancelToken::new();
    let request = request();
    let canceller = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        canceller.cancel();
    });
    let result = provider.chat(&request, &cancel).await;
    assert!(matches!(result, Err(AiError::Cancelled)), "{result:?}");
    assert!(server.wait_for_abort().await);
}

#[tokio::test]
async fn unauthorized_errors_never_show_the_key() {
    let echo = json!({"error": {"message": format!("Incorrect API key provided: {KEY}."), "type": "invalid_request_error", "code": "invalid_api_key"}});
    let server = MockServer::start(vec![Reply::json(401, echo)]).await;
    let err = run(openai(&server).as_ref()).await.expect_err("401");
    let AiError::Unauthorized { status, message } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(*status, 401);
    assert!(
        message.as_deref().is_some_and(|m| m.contains("[redacted]")),
        "{message:?}"
    );
    assert_no_key(&err);
    assert!(!err.is_transient());
}

#[tokio::test]
async fn rate_limits_carry_retry_after() {
    let server = MockServer::start(vec![
        Reply::json(429, json!({"error": {"message": "Rate limit reached"}}))
            .with_header("Retry-After", "7"),
        Reply::json(429, json!({"error": {"message": "slow"}}))
            .with_header("retry-after-ms", "250"),
        Reply::json(429, json!({"error": "no header"})),
    ])
    .await;
    let provider = openai(&server);
    let err = run(provider.as_ref()).await.expect_err("429");
    assert_eq!(
        err,
        AiError::RateLimited {
            retry_after: Some(Duration::from_secs(7))
        }
    );
    assert!(err.is_transient());
    let err = run(provider.as_ref()).await.expect_err("429");
    assert_eq!(err.retry_after(), Some(Duration::from_millis(250)));
    let err = run(provider.as_ref()).await.expect_err("429");
    assert_eq!(err, AiError::RateLimited { retry_after: None });
    // Nothing is retried by itself: three calls, three requests.
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn server_errors_and_overload() {
    let server = MockServer::start(vec![
        Reply::json(500, json!({"error": {"message": "internal"}})),
        Reply::json(503, json!({"error": {"message": "busy"}})).with_header("Retry-After", "30"),
        Reply::Full {
            status: 502,
            headers: vec![],
            body: b"<html>Bad Gateway</html>".to_vec(),
        },
        Reply::json(404, json!({"error": "model 'nope' not found"})),
    ])
    .await;
    let provider = openai(&server);
    assert!(matches!(
        run(provider.as_ref()).await,
        Err(AiError::Server { status: 500, .. })
    ));
    assert_eq!(
        run(provider.as_ref()).await.expect_err("503"),
        AiError::Overloaded {
            retry_after: Some(Duration::from_secs(30))
        }
    );
    assert!(matches!(
        run(provider.as_ref()).await,
        Err(AiError::Server { status: 502, .. })
    ));
    assert_eq!(
        run(provider.as_ref()).await.expect_err("404"),
        AiError::NotFound {
            message: Some("model 'nope' not found".into())
        }
    );
}

#[tokio::test]
async fn broken_event_streams_are_protocol_errors() {
    let cut = Reply::sse(vec![data(
        &json!({"choices": [{"delta": {"content": "half"}}]}),
    )]);
    let invalid = Reply::sse(vec!["data: {\"choices\": [\n\n".into()]);
    let not_utf8 = Reply::Stream {
        headers: vec![],
        chunks: vec![b"data: \xff\xfe\xfd\n\n".to_vec()],
        delay: Duration::ZERO,
        end: StreamEnd::Close,
    };
    let huge = Reply::sse(vec![format!("data: {}\n\n", "x".repeat(4096))]);
    let in_stream_error = Reply::sse(vec![
        data(&json!({"choices": [{"delta": {"content": "par"}}]})),
        data(&json!({"error": {"message": "upstream died", "code": 502}})),
    ]);
    let server = MockServer::start(vec![cut, invalid, not_utf8, huge, in_stream_error]).await;
    let provider = openai_with(&server, |mut config| {
        config.limits.max_event_bytes = 1024;
        config
    });
    for _ in 0..4 {
        let result = run(provider.as_ref()).await;
        assert!(matches!(result, Err(AiError::Protocol(_))), "{result:?}");
    }
    assert!(matches!(
        run(provider.as_ref()).await,
        Err(AiError::Server { status: 502, .. })
    ));
}

#[tokio::test]
async fn length_and_content_filter_are_checked_before_the_text() {
    let length = Reply::sse(vec![
        data(&json!({"choices": [{"delta": {"content": "{\"key_points\": ["}}]})),
        data(&json!({"choices": [{"delta": {}, "finish_reason": "length"}]})),
        "data: [DONE]\n\n".into(),
    ]);
    let filtered = Reply::sse(vec![
        data(&json!({"choices": [{"delta": {"content": "Partial"}}]})),
        data(&json!({"choices": [{"delta": {}, "finish_reason": "content_filter"}]})),
        "data: [DONE]\n\n".into(),
    ]);
    let server = MockServer::start(vec![length, filtered]).await;
    let provider = openai(&server);
    assert_eq!(
        run(provider.as_ref()).await.expect_err("length"),
        AiError::Truncated
    );
    assert!(matches!(
        run(provider.as_ref()).await,
        Err(AiError::Refused { .. })
    ));
}

#[tokio::test]
async fn redirects_are_never_followed() {
    let elsewhere = MockServer::start(vec![openai_stream("hijacked", 3)]).await;
    let server = MockServer::start(vec![Reply::Full {
        status: 307,
        headers: vec![("Location".into(), elsewhere.url("/v1/chat/completions"))],
        body: Vec::new(),
    }])
    .await;
    let err = run(openai(&server).as_ref()).await.expect_err("redirect");
    assert!(
        matches!(err, AiError::Rejected { status: 307, .. }),
        "{err:?}"
    );
    assert_eq!(server.connections(), 1);
    assert_eq!(elsewhere.connections(), 0);
}

#[tokio::test]
async fn time_limits() {
    let first = data(&json!({"choices": [{"delta": {"content": "slow"}}]}));
    let server = MockServer::start(vec![
        Reply::Silent,
        Reply::sse(vec![first]).ending(StreamEnd::Hang),
    ])
    .await;
    let provider = openai_with(&server, |mut config| {
        config.limits.response_timeout = Duration::from_millis(300);
        config.limits.idle_timeout = Duration::from_millis(300);
        config
    });
    let err = run(provider.as_ref()).await.expect_err("no headers");
    assert_eq!(err, AiError::Timeout(TimeoutPhase::Response));
    let err = run(provider.as_ref()).await.expect_err("stalled stream");
    assert_eq!(err, AiError::Timeout(TimeoutPhase::Idle));
    assert!(server.wait_for_abort().await);
}

#[tokio::test]
async fn too_large_requests_and_answers() {
    let server = MockServer::start(vec![openai_stream(&"y".repeat(500), 100)]).await;
    let tiny = openai_with(&server, |mut config| {
        config.limits.max_request_bytes = 100;
        config
    });
    assert_eq!(
        run(tiny.as_ref()).await.expect_err("request"),
        AiError::RequestTooLarge
    );
    assert_eq!(server.connections(), 0, "nothing may be sent");
    let small = openai_with(&server, |mut config| {
        config.limits.max_output_chars = 250;
        config
    });
    assert_eq!(
        run(small.as_ref()).await.expect_err("answer"),
        AiError::ResponseTooLarge
    );
}

#[tokio::test]
async fn models_are_listed() {
    let server = MockServer::start(vec![Reply::json(
        200,
        json!({"object": "list", "data": [
            {"id": "llama3.2:3b", "object": "model"},
            {"id": "qwen3:8b", "name": "Qwen 3 8B"},
            {"object": "model"}
        ]}),
    )])
    .await;
    let models = openai(&server)
        .list_models(&CancelToken::new())
        .await
        .expect("models");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "llama3.2:3b");
    assert_eq!(models[1].display_name.as_deref(), Some("Qwen 3 8B"));
    let sent = &server.requests()[0];
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str()),
        ("GET", "/v1/models")
    );
}

#[tokio::test]
async fn local_names_are_resolved_to_local_addresses_only() {
    let server = MockServer::start(vec![openai_stream("hi", 2)]).await;
    let config = support::quick(ProviderConfig::new(
        ProviderKind::OpenAiCompatible,
        format!("http://localhost:{}/v1", server.addr.port()),
        None,
    ));
    let provider = build_provider(config, &support::tls()).expect("provider");
    assert_eq!(provider.info().locality, mail_ai::Locality::ThisDevice);
    let (text, _) = run(provider.as_ref()).await.expect("answer via localhost");
    assert_eq!(text, "hi");
    assert_eq!(server.requests()[0].header("authorization"), None);
}

// --- Anthropic Messages --------------------------------------------------------------------

#[tokio::test]
async fn anthropic_streams_per_the_documentation() {
    let server = MockServer::start(vec![
        anthropic_stream("Hello! The connection works.", "end_turn", json!(null))
            .with_header("request-id", "req_018EeWyXxfu5pfWkrYcMdjWG"),
    ])
    .await;
    let provider = anthropic(&server);
    let (text, finish) = run(provider.as_ref()).await.expect("answer");
    assert_eq!(text, "Hello! The connection works.");
    assert_eq!(finish.stop, StopReason::Complete);
    assert_eq!(
        finish.request_id.as_deref(),
        Some("req_018EeWyXxfu5pfWkrYcMdjWG")
    );
    assert_eq!(finish.usage.output_tokens, Some(15));

    let sent = &server.requests()[0];
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str()),
        ("POST", "/v1/messages")
    );
    assert_eq!(sent.header("x-api-key"), Some(KEY));
    assert_eq!(sent.header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(sent.header("authorization"), None);
    // Not api.anthropic.com: no server-side fallback beta.
    assert_eq!(sent.header("anthropic-beta"), None);
    let body = sent.json();
    let mut keys: Vec<&str> = body
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["max_tokens", "messages", "model", "stream", "system"]
    );
    assert_eq!(body["max_tokens"], 16_000);
    assert_eq!(body["messages"][0]["role"], "user");
}

#[tokio::test]
async fn anthropic_refusal_is_checked_before_reading_the_answer() {
    let server = MockServer::start(vec![anthropic_stream(
        "Hello..",
        "refusal",
        json!({"type": "refusal", "category": "cyber", "explanation": "This request was declined because it could enable cyber harm."}),
    )])
    .await;
    let err = run(anthropic(&server).as_ref()).await.expect_err("refusal");
    assert_eq!(
        err,
        AiError::Refused {
            category: Some("cyber".into()),
            explanation: Some(
                "This request was declined because it could enable cyber harm.".into()
            )
        }
    );
    assert!(!err.is_transient());
}

#[tokio::test]
async fn anthropic_errors_status_and_in_stream() {
    let overloaded_event = Reply::sse(vec![
        event(
            "message_start",
            &json!({"type": "message_start", "message": {"model": "claude-opus-5-5"}}),
        ),
        event(
            "error",
            &json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
        ),
    ]);
    let server = MockServer::start(vec![
        Reply::json(529, json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}, "request_id": "req_1"})),
        Reply::json(429, json!({"type": "error", "error": {"type": "rate_limit_error", "message": "Rate limited"}})).with_header("retry-after", "12"),
        overloaded_event,
        Reply::json(401, json!({"type": "error", "error": {"type": "authentication_error", "message": format!("invalid x-api-key {KEY}")}})),
        anthropic_stream("{\"key_points\": [", "max_tokens", json!(null)),
    ])
    .await;
    let provider = anthropic(&server);
    assert_eq!(
        run(provider.as_ref()).await.expect_err("529"),
        AiError::Overloaded { retry_after: None }
    );
    assert_eq!(
        run(provider.as_ref()).await.expect_err("429"),
        AiError::RateLimited {
            retry_after: Some(Duration::from_secs(12))
        }
    );
    assert_eq!(
        run(provider.as_ref()).await.expect_err("event"),
        AiError::Overloaded { retry_after: None }
    );
    let err = run(provider.as_ref()).await.expect_err("401");
    assert!(matches!(err, AiError::Unauthorized { status: 401, .. }));
    assert_no_key(&err);
    assert_eq!(
        run(provider.as_ref()).await.expect_err("max_tokens"),
        AiError::Truncated
    );
}

#[tokio::test]
async fn anthropic_model_list_is_paged() {
    let server = MockServer::start(vec![
        Reply::json(
            200,
            json!({"data": [
            {"type": "model", "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5"},
            {"type": "model", "id": "claude-sonnet-5-5", "display_name": "Claude Sonnet 5.5"}
        ], "has_more": true, "first_id": "claude-opus-5-5", "last_id": "claude-sonnet-5-5"}),
        ),
        Reply::json(
            200,
            json!({"data": [
            {"type": "model", "id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5"}
        ], "has_more": false, "first_id": "claude-haiku-5-5", "last_id": "claude-haiku-5-5"}),
        ),
    ])
    .await;
    let models = anthropic(&server)
        .list_models(&CancelToken::new())
        .await
        .expect("models");
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]
    );
    assert_eq!(models[0].display_name.as_deref(), Some("Claude Opus 5.5"));
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/models?limit=1000");
    assert_eq!(
        requests[1].path,
        "/v1/models?limit=1000&after_id=claude-sonnet-5-5"
    );
    assert_eq!(requests[1].header("anthropic-version"), Some("2026-10-21"));
}

#[tokio::test]
async fn cookies_are_never_stored_or_sent() {
    let server = MockServer::start(vec![
        Reply::json(429, json!({"error": {"message": "later"}}))
            .with_header("Set-Cookie", "session=tracking-id; Path=/"),
        openai_stream("ok", 2),
    ])
    .await;
    let provider = openai(&server);
    assert!(run(provider.as_ref()).await.is_err());
    run(provider.as_ref()).await.expect("answer");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].header("cookie"), None);
}
