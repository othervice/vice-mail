//! The helper program `mail-mcp` that AI clients start (decision 0015): MCP on stdin/stdout,
//! the app over the local IPC channel. It holds no mail data and no permissions; the app decides
//! everything. It connects on the first request that needs the app and reconnects when the app
//! was restarted, so the client never has to be restarted.
//!
//! Configuration, both from the environment the client passes:
//! - `MAIL_MCP_ENDPOINT`: socket path (macOS) or pipe name (Windows), from the app's settings
//! - `MAIL_MCP_TOKEN`: the client token (`mcp1_…`)
//!
//! Without them the helper still answers MCP and explains in every tool result what is missing.

use std::io;
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::io::{BufReader, ReadHalf, WriteHalf};
use tokio::time::timeout;

use crate::endpoint::{Endpoint, Stream, connect};
use crate::ipc::{
    CallError, Handshake, Line, PROTOCOL, Refusal, Reply, Request, Side, proof, read_line, verify,
    write_line,
};
use crate::limits;
use crate::mcp::{BackendError, McpServer, ToolBackend, ToolReply};
use crate::permissions::{ClientToken, hex, parse_hex, random_bytes};
use crate::tools::ToolName;

pub const ENV_ENDPOINT: &str = "MAIL_MCP_ENDPOINT";
pub const ENV_TOKEN: &str = "MAIL_MCP_TOKEN";

/// Where to reach the app and as whom.
#[derive(Debug, Clone)]
pub struct HelperConfig {
    pub endpoint: Endpoint,
    pub token: ClientToken,
}

impl HelperConfig {
    /// From `MAIL_MCP_ENDPOINT` and `MAIL_MCP_TOKEN`. The error text names what is wrong,
    /// never the token.
    pub fn from_env() -> Result<Self, String> {
        let endpoint = std::env::var(ENV_ENDPOINT)
            .map_err(|_| format!("{ENV_ENDPOINT} is not set"))
            .and_then(|value| {
                Endpoint::new(value).map_err(|_| format!("{ENV_ENDPOINT} is not a valid endpoint"))
            })?;
        let token = std::env::var(ENV_TOKEN)
            .map_err(|_| format!("{ENV_TOKEN} is not set"))
            .and_then(|value| {
                ClientToken::parse(&value).map_err(|err| format!("{ENV_TOKEN}: {err}"))
            })?;
        Ok(Self { endpoint, token })
    }
}

/// Runs the helper on its stdin and stdout until the client closes stdin.
pub async fn run_stdio(config: Result<HelperConfig, String>) -> io::Result<()> {
    McpServer::new(AppBackend::new(config))
        .serve(tokio::io::stdin(), tokio::io::stdout())
        .await
}

struct Connection {
    reader: BufReader<ReadHalf<Box<dyn Stream>>>,
    writer: WriteHalf<Box<dyn Stream>>,
}

/// [`ToolBackend`] that forwards to the app.
pub struct AppBackend {
    config: Result<HelperConfig, String>,
    connection: Option<Connection>,
    next_id: u64,
}

impl AppBackend {
    pub fn new(config: Result<HelperConfig, String>) -> Self {
        Self {
            config,
            connection: None,
            next_id: 0,
        }
    }

    /// Sends one request and returns the app's reply to it. A broken connection is reopened
    /// once (the app may have restarted since the last request).
    async fn request(&mut self, make: impl Fn(u64) -> Request) -> Result<Reply, BackendError> {
        let config = self
            .config
            .as_ref()
            .map_err(|detail| BackendError::NotConfigured(detail.clone()))?;
        let mut reopened = false;
        loop {
            let connection = match &mut self.connection {
                Some(connection) => connection,
                None => {
                    reopened = true;
                    self.connection.insert(open(config).await?)
                }
            };
            self.next_id += 1;
            let id = self.next_id;
            match exchange(connection, &make(id), id).await {
                Ok(Reply::Closed { reason }) => {
                    self.connection = None;
                    return Err(BackendError::Refused(reason));
                }
                Ok(reply) => return Ok(reply),
                Err(Exchange::Broken) if !reopened => {
                    self.connection = None;
                }
                Err(Exchange::Broken) => {
                    self.connection = None;
                    return Err(BackendError::Unreachable);
                }
                Err(Exchange::Failed(err)) => {
                    self.connection = None;
                    return Err(err);
                }
            }
        }
    }
}

impl ToolBackend for AppBackend {
    async fn list_tools(&mut self) -> Result<Vec<ToolName>, BackendError> {
        match self.request(|id| Request::ListTools { id }).await? {
            Reply::Tools { tools, .. } => Ok(tools
                .iter()
                .filter_map(|name| ToolName::parse(name))
                .collect()),
            other => Err(unexpected(&other)),
        }
    }

    async fn call_tool(
        &mut self,
        tool: ToolName,
        arguments: Map<String, Value>,
    ) -> Result<ToolReply, BackendError> {
        let reply = self
            .request(|id| Request::CallTool {
                id,
                name: tool.as_str().to_owned(),
                arguments: arguments.clone(),
            })
            .await?;
        match reply {
            Reply::Result { text, is_error, .. } => Ok(ToolReply::Result { text, is_error }),
            Reply::Error { code, message, .. } => match code {
                CallError::UnknownTool => Ok(ToolReply::Unavailable),
                CallError::RateLimited => Err(BackendError::RateLimited),
                CallError::Timeout => Err(BackendError::Timeout),
                CallError::Malformed => Err(BackendError::Failed(message)),
            },
            other => Err(unexpected(&other)),
        }
    }
}

fn unexpected(reply: &Reply) -> BackendError {
    BackendError::Failed(format!("unexpected answer {reply:?}"))
}

enum Exchange {
    /// The connection is gone (the app quit or restarted).
    Broken,
    Failed(BackendError),
}

async fn exchange(
    connection: &mut Connection,
    request: &Request,
    id: u64,
) -> Result<Reply, Exchange> {
    if write_line(&mut connection.writer, request).await.is_err() {
        return Err(Exchange::Broken);
    }
    let wait = Duration::from_secs(limits::CALL_TIMEOUT_SECS + 5);
    loop {
        let line = timeout(
            wait,
            read_line(&mut connection.reader, limits::RESPONSE_LINE_BYTES),
        )
        .await
        .map_err(|_| Exchange::Failed(BackendError::Timeout))?;
        let bytes = match line {
            Ok(Line::Data(bytes)) => bytes,
            Ok(Line::TooLong) => {
                return Err(Exchange::Failed(BackendError::Failed(
                    "answer too long".into(),
                )));
            }
            Ok(Line::Eof) | Err(_) => return Err(Exchange::Broken),
        };
        let reply: Reply = serde_json::from_slice(&bytes)
            .map_err(|_| Exchange::Failed(BackendError::Failed("unreadable answer".into())))?;
        let reply_id = match &reply {
            Reply::Tools { id, .. } | Reply::Result { id, .. } | Reply::Error { id, .. } => *id,
            Reply::Closed { .. } => return Ok(reply),
        };
        match reply_id {
            reply_id if reply_id == id => return Ok(reply),
            // An error about an unreadable line (id 0) or an answer to an abandoned request.
            _ => continue,
        }
    }
}

async fn open(config: &HelperConfig) -> Result<Connection, BackendError> {
    let stream = connect(&config.endpoint).await.map_err(|err| {
        tracing::info!(error = %err, "mail app not reachable");
        BackendError::Unreachable
    })?;
    let (read, writer) = tokio::io::split(stream);
    let mut connection = Connection {
        reader: BufReader::new(read),
        writer,
    };
    timeout(
        Duration::from_secs(limits::HANDSHAKE_TIMEOUT_SECS),
        handshake(&mut connection, &config.token),
    )
    .await
    .map_err(|_| BackendError::Failed("the handshake timed out".into()))??;
    Ok(connection)
}

async fn read_handshake(connection: &mut Connection) -> Result<Handshake, BackendError> {
    match read_line(&mut connection.reader, limits::HANDSHAKE_LINE_BYTES).await {
        Ok(Line::Data(bytes)) => serde_json::from_slice(&bytes)
            .map_err(|_| BackendError::Failed("unreadable handshake".into())),
        // The app closed the connection without a word (for example another user's process).
        Ok(Line::Eof) | Err(_) => Err(BackendError::Unreachable),
        Ok(Line::TooLong) => Err(BackendError::Failed("handshake line too long".into())),
    }
}

async fn handshake(connection: &mut Connection, token: &ClientToken) -> Result<(), BackendError> {
    let helper_nonce = random_bytes::<32>().map_err(|err| BackendError::Failed(err.to_string()))?;
    let broken = |_| BackendError::Unreachable;
    write_line(
        &mut connection.writer,
        &Handshake::Hello {
            protocol: PROTOCOL,
            client_id: token.id,
            nonce: hex(&helper_nonce),
        },
    )
    .await
    .map_err(broken)?;
    let (app_nonce, app_proof) = match read_handshake(connection).await? {
        Handshake::Challenge {
            protocol,
            nonce,
            proof,
        } if protocol == PROTOCOL => (nonce, proof),
        Handshake::Refused { reason, .. } => return Err(BackendError::Refused(reason)),
        _ => return Err(BackendError::Failed("unexpected handshake message".into())),
    };
    let verified = parse_hex::<32>(&app_nonce).filter(|app_nonce| {
        verify(
            &token.key,
            Side::App,
            &token.id,
            &helper_nonce,
            app_nonce,
            &app_proof,
        )
    });
    let Some(app_nonce) = verified else {
        // Tell the app why (it records it), then stop: nothing else is sent.
        let _ = write_line(
            &mut connection.writer,
            &Handshake::Refused {
                reason: Refusal::BadProof,
                supported: Vec::new(),
            },
        )
        .await;
        return Err(BackendError::NotVerified);
    };
    write_line(
        &mut connection.writer,
        &Handshake::Auth {
            proof: proof(
                &token.key,
                Side::Helper,
                &token.id,
                &helper_nonce,
                &app_nonce,
            ),
        },
    )
    .await
    .map_err(broken)?;
    match read_handshake(connection).await? {
        Handshake::Ready => Ok(()),
        Handshake::Refused { reason, .. } => Err(BackendError::Refused(reason)),
        _ => Err(BackendError::Failed("unexpected handshake message".into())),
    }
}
