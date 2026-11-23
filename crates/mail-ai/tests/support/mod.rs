//! A local HTTP/1.1 server that plays OpenAI-compatible and Anthropic answers, including
//! streams, for the contract tests. It records every connection and request, and notices when
//! the client goes away in the middle of a stream (the "server sees the abort" checks).
//! Every reply closes its connection, so connections and requests can be counted 1:1.

#![allow(dead_code)] // each test file uses a different part

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mail_ai::{AiProvider, ProviderConfig, ProviderKind, build_provider};
use mail_model::Secret;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

pub const KEY: &str = "sk-test-0123456789abcdefSECRET";

/// One request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("request body is JSON")
    }

    pub fn raw(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// How a scripted stream ends.
#[derive(Debug, Clone)]
pub enum StreamEnd {
    /// Close the connection.
    Close,
    /// Keep it open without sending anything until the client goes away.
    Hang,
    /// Keep sending this chunk every 20 ms until the client goes away.
    Forever(String),
}

/// The answer to the next request.
#[derive(Debug, Clone)]
pub enum Reply {
    /// A complete response with `Content-Length`.
    Full {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// `200 text/event-stream`, the chunks written one by one.
    Stream {
        headers: Vec<(String, String)>,
        chunks: Vec<Vec<u8>>,
        delay: Duration,
        end: StreamEnd,
    },
    /// Read the request, never answer.
    Silent,
}

impl Reply {
    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self::Full {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string().into_bytes(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        match &mut self {
            Self::Full { headers, .. } | Self::Stream { headers, .. } => {
                headers.push((name.into(), value.into()));
            }
            Self::Silent => {}
        }
        self
    }

    /// An SSE stream of `events`, each already formatted (`data: …\n\n`).
    pub fn sse(events: Vec<String>) -> Self {
        Self::Stream {
            headers: Vec::new(),
            chunks: events.into_iter().map(String::into_bytes).collect(),
            delay: Duration::from_millis(2),
            end: StreamEnd::Close,
        }
    }

    pub fn ending(mut self, new_end: StreamEnd) -> Self {
        if let Self::Stream { end, .. } = &mut self {
            *end = new_end;
        }
        self
    }
}

#[derive(Default)]
struct State {
    connections: usize,
    requests: Vec<Recorded>,
    replies: VecDeque<Reply>,
    aborted: usize,
}

pub struct MockServer {
    pub addr: SocketAddr,
    state: Arc<Mutex<State>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockServer {
    pub async fn start(replies: Vec<Reply>) -> Self {
        Self::start_with(replies, None).await
    }

    /// The same server behind TLS (`acceptor` holds its certificate).
    pub async fn start_tls(replies: Vec<Reply>, acceptor: tokio_rustls::TlsAcceptor) -> Self {
        Self::start_with(replies, Some(acceptor)).await
    }

    async fn start_with(replies: Vec<Reply>, tls: Option<tokio_rustls::TlsAcceptor>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(Mutex::new(State {
            replies: replies.into(),
            ..State::default()
        }));
        let task = tokio::spawn({
            let state = Arc::clone(&state);
            async move {
                loop {
                    let Ok((socket, _)) = listener.accept().await else {
                        return;
                    };
                    lock(&state).connections += 1;
                    let state = Arc::clone(&state);
                    match tls.clone() {
                        None => {
                            tokio::spawn(serve(socket, state));
                        }
                        Some(acceptor) => {
                            tokio::spawn(async move {
                                // A failed handshake (untrusted certificate) is just a closed
                                // connection.
                                if let Ok(stream) = acceptor.accept(socket).await {
                                    serve(stream, state).await;
                                }
                            });
                        }
                    }
                }
            }
        });
        Self { addr, state, task }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.addr.port())
    }

    pub fn connections(&self) -> usize {
        lock(&self.state).connections
    }

    pub fn requests(&self) -> Vec<Recorded> {
        lock(&self.state).requests.clone()
    }

    pub fn aborted(&self) -> usize {
        lock(&self.state).aborted
    }

    /// Waits until the server noticed a client going away mid-stream.
    pub async fn wait_for_abort(&self) -> bool {
        for _ in 0..300 {
            if self.aborted() > 0 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn serve<S>(mut socket: S, state: Arc<Mutex<State>>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let reply = {
        let mut state = lock(&state);
        state.requests.push(request);
        state.replies.pop_front()
    };
    let reply = reply.unwrap_or_else(|| {
        Reply::json(
            500,
            serde_json::json!({"error": {"message": "no reply scripted"}}),
        )
    });
    match reply {
        Reply::Full {
            status,
            headers,
            body,
        } => {
            let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ));
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
            let _ = socket.shutdown().await;
        }
        Reply::Stream {
            headers,
            chunks,
            delay,
            end,
        } => {
            let mut head = String::from(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n",
            );
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str("Connection: close\r\n\r\n");
            if socket.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            for chunk in chunks {
                if socket.write_all(&chunk).await.is_err() || socket.flush().await.is_err() {
                    lock(&state).aborted += 1;
                    return;
                }
                tokio::time::sleep(delay).await;
            }
            match end {
                StreamEnd::Close => {
                    let _ = socket.shutdown().await;
                }
                StreamEnd::Hang => {
                    let mut buf = [0_u8; 64];
                    // Returns when the client closes (0 bytes) or resets.
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                    }
                    lock(&state).aborted += 1;
                }
                StreamEnd::Forever(chunk) => {
                    let (mut reader, mut writer) = tokio::io::split(socket);
                    let mut buf = [0_u8; 64];
                    loop {
                        tokio::select! {
                            read = reader.read(&mut buf) => {
                                if matches!(read, Ok(0) | Err(_)) {
                                    break;
                                }
                            }
                            () = tokio::time::sleep(Duration::from_millis(20)) => {
                                if writer.write_all(chunk.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    lock(&state).aborted += 1;
                }
            }
        }
        Reply::Silent => {
            let mut buf = [0_u8; 64];
            while let Ok(n) = socket.read(&mut buf).await {
                if n == 0 {
                    break;
                }
            }
            lock(&state).aborted += 1;
        }
    }
}

async fn read_request<S: AsyncRead + Unpin>(socket: &mut S) -> Option<Recorded> {
    let mut data = Vec::new();
    let mut buf = [0_u8; 4096];
    let head_end = loop {
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = data[head_end + 4..].to_vec();
    while body.len() < length {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    Some(Recorded {
        method,
        path,
        headers,
        body,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        529 => "Overloaded",
        _ => "Status",
    }
}

/// `data: {json}\n\n`
pub fn data(json: &serde_json::Value) -> String {
    format!("data: {json}\n\n")
}

/// `event: name\ndata: {json}\n\n`
pub fn event(name: &str, json: &serde_json::Value) -> String {
    format!("event: {name}\ndata: {json}\n\n")
}

/// An OpenAI-compatible stream that delivers `text` in pieces of `piece` characters.
pub fn openai_stream(text: &str, piece: usize) -> Reply {
    let mut events = vec![data(&serde_json::json!({
        "id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "test-model",
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]
    }))];
    let chars: Vec<char> = text.chars().collect();
    for part in chars.chunks(piece.max(1)) {
        let content: String = part.iter().collect();
        events.push(data(&serde_json::json!({
            "id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "test-model",
            "choices": [{"index": 0, "delta": {"content": content}, "finish_reason": null}]
        })));
    }
    events.push(data(&serde_json::json!({
        "id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "test-model",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    })));
    events.push("data: [DONE]\n\n".into());
    Reply::sse(events)
}

/// An Anthropic Messages stream for `text`, ending with `stop_reason` (and `stop_details`).
pub fn anthropic_stream(text: &str, stop_reason: &str, stop_details: serde_json::Value) -> Reply {
    let mut events = vec![
        event(
            "message_start",
            &serde_json::json!({"type": "message_start", "message": {
                "id": "msg_1", "type": "message", "role": "assistant", "content": [],
                "model": "claude-opus-5-5", "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": 25, "output_tokens": 1}}}),
        ),
        event(
            "content_block_start",
            &serde_json::json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}}),
        ),
        event("ping", &serde_json::json!({"type": "ping"})),
    ];
    let chars: Vec<char> = text.chars().collect();
    for part in chars.chunks(7) {
        let piece: String = part.iter().collect();
        events.push(event(
            "content_block_delta",
            &serde_json::json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": piece}}),
        ));
    }
    events.push(event(
        "content_block_stop",
        &serde_json::json!({"type": "content_block_stop", "index": 0}),
    ));
    events.push(event(
        "message_delta",
        &serde_json::json!({"type": "message_delta",
            "delta": {"stop_reason": stop_reason, "stop_sequence": null, "stop_details": stop_details},
            "usage": {"output_tokens": 15}}),
    ));
    events.push(event(
        "message_stop",
        &serde_json::json!({"type": "message_stop"}),
    ));
    Reply::sse(events)
}

/// TLS for the tests: the process default provider; the mock speaks plain HTTP anyway.
pub fn tls() -> rustls::ClientConfig {
    rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth()
}

/// Short limits so time-limit tests finish quickly.
pub fn quick(mut config: ProviderConfig) -> ProviderConfig {
    config.limits.connect_timeout = Duration::from_secs(2);
    config.limits.response_timeout = Duration::from_secs(5);
    config.limits.idle_timeout = Duration::from_secs(5);
    config.limits.total_timeout = Duration::from_secs(10);
    config
}

pub fn openai(server: &MockServer) -> Box<dyn AiProvider> {
    openai_with(server, |config| config)
}

pub fn openai_with(
    server: &MockServer,
    adjust: impl FnOnce(ProviderConfig) -> ProviderConfig,
) -> Box<dyn AiProvider> {
    let config = ProviderConfig::new(
        ProviderKind::OpenAiCompatible,
        server.url("/v1"),
        Some(Secret::new(KEY.to_owned())),
    );
    build_provider(adjust(quick(config)), &tls()).expect("provider")
}

pub fn anthropic(server: &MockServer) -> Box<dyn AiProvider> {
    let config = ProviderConfig::new(
        ProviderKind::AnthropicMessages,
        server.url(""),
        Some(Secret::new(KEY.to_owned())),
    );
    build_provider(quick(config), &tls()).expect("provider")
}
