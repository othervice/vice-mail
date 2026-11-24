//! The app side: [`McpHost`] listens on the [`Endpoint`] while MCP is switched on, runs the
//! handshake with each helper, answers its requests through [`MailAccess`] and records every
//! tool call in the [`AccessLog`]. `AppCore` creates one host and runs it for the app's lifetime;
//! the host follows the settings by itself:
//!
//! - MCP off: no listening socket, every connection closed, every request refused.
//! - Client revoked: its connections close at once, also during a running call.
//! - Account hidden: it disappears from every tool at the next request.

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout};

use crate::access::MailAccess;
use crate::endpoint::{Endpoint, Listener};
use crate::exec::execute;
use crate::ipc::{
    CallError, Handshake, Line, PROTOCOL, Refusal, Reply, Request, Side, proof, read_line, verify,
    write_line,
};
use crate::limits;
use crate::permissions::{ClientId, Denied, McpSettings, parse_hex, random_bytes};
use crate::tools::{ToolName, tools_for};

/// Receives one entry per tool call and per refused connection (the "Zugriffe" view). Only
/// metadata: client, tool, time, number of messages; never arguments or content.
pub trait AccessLog: Send + Sync + 'static {
    fn record(&self, event: AccessEvent);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccessEvent {
    /// Unix milliseconds.
    pub at: i64,
    /// `None` for a connection refused before the client was known.
    pub client_id: Option<ClientId>,
    /// The client's name at that moment.
    pub client_name: Option<String>,
    pub kind: AccessKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccessKind {
    ToolCall {
        tool: ToolName,
        /// Messages whose data the answer contained.
        messages: u32,
        outcome: CallOutcome,
    },
    /// A helper could not connect (wrong token, revoked client, MCP off, ...).
    Refused { reason: Refusal },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    Ok,
    /// The tool ran and reported an error (bad argument, unknown id, app error).
    Error,
    /// Not run: not allowed for this client (for example `create_draft` without permission),
    /// rate limit, or timeout.
    Refused,
}

/// What the host is doing, for the settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStatus {
    /// MCP is off (or the host has not started or has ended).
    Stopped,
    Listening,
    /// MCP is on, but the endpoint could not be opened; retried on the next settings change.
    Failed(String),
}

/// The MCP host. Cheap to clone.
pub struct McpHost<A> {
    inner: Arc<Inner<A>>,
}

impl<A> Clone for McpHost<A> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<A> {
    access: Arc<A>,
    settings: watch::Receiver<McpSettings>,
    log: Arc<dyn AccessLog>,
    status: watch::Sender<HostStatus>,
    connections: Arc<Semaphore>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl<A: MailAccess> McpHost<A> {
    pub fn new(
        access: Arc<A>,
        settings: watch::Receiver<McpSettings>,
        log: Arc<dyn AccessLog>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                access,
                settings,
                log,
                status: watch::Sender::new(HostStatus::Stopped),
                connections: Arc::new(Semaphore::new(limits::MAX_CONNECTIONS)),
            }),
        }
    }

    pub fn status(&self) -> watch::Receiver<HostStatus> {
        self.inner.status.subscribe()
    }

    /// Listens on `endpoint` whenever MCP is on, until `shutdown` completes. Returns after
    /// every connection is closed and the endpoint removed.
    pub async fn run(&self, endpoint: Endpoint, shutdown: impl Future<Output = ()> + Send) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let mut settings = self.inner.settings.clone();
        let mut tasks = JoinSet::new();
        tokio::pin!(shutdown);
        'outer: loop {
            let enabled = settings.borrow_and_update().enabled;
            if !enabled {
                self.inner.status.send_replace(HostStatus::Stopped);
                tokio::select! {
                    changed = settings.changed() => {
                        if changed.is_err() { break 'outer; }
                        continue 'outer;
                    }
                    () = &mut shutdown => break 'outer,
                }
            }
            let mut listener = match Listener::bind(&endpoint).await {
                Ok(listener) => listener,
                Err(err) => {
                    tracing::error!(error = %err, "MCP endpoint could not be opened");
                    self.inner
                        .status
                        .send_replace(HostStatus::Failed(err.to_string()));
                    tokio::select! {
                        changed = settings.changed() => {
                            if changed.is_err() { break 'outer; }
                            continue 'outer;
                        }
                        () = &mut shutdown => break 'outer,
                    }
                }
            };
            self.inner.status.send_replace(HostStatus::Listening);
            tracing::info!("MCP host listening");
            loop {
                tokio::select! {
                    accepted = listener.accept() => match accepted {
                        Ok(stream) => {
                            let host = self.clone();
                            let stop = stop_rx.clone();
                            match Arc::clone(&self.inner.connections).try_acquire_owned() {
                                Ok(permit) => {
                                    tasks.spawn(async move {
                                        host.serve(stream, stop).await;
                                        drop(permit);
                                    });
                                }
                                Err(_) => {
                                    tasks.spawn(async move {
                                        host.refuse(stream, Refusal::TooManyConnections).await;
                                    });
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "MCP accept failed");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    },
                    changed = settings.changed() => {
                        if changed.is_err() { break 'outer; }
                        if !settings.borrow().enabled {
                            // Close the endpoint; open connections see the change themselves.
                            continue 'outer;
                        }
                    }
                    Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
                    () = &mut shutdown => break 'outer,
                }
            }
        }
        let _ = stop_tx.send(true);
        let drained = timeout(Duration::from_secs(2), async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
        self.inner.status.send_replace(HostStatus::Stopped);
    }

    /// Serves one connection over any byte stream (the listener's, or a test's).
    pub async fn serve_connection<S>(&self, stream: S)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (_keep, stop) = watch::channel(false);
        self.serve(stream, stop).await;
    }

    fn record(&self, client: Option<(&ClientId, &str)>, kind: AccessKind) {
        self.inner.log.record(AccessEvent {
            at: now_ms(),
            client_id: client.map(|(id, _)| *id),
            client_name: client.map(|(_, name)| name.to_owned()),
            kind,
        });
    }

    /// Recorded before the helper is told, so the log has the refusal by the time the helper
    /// reports it.
    async fn refuse<S: AsyncRead + AsyncWrite + Unpin + Send>(&self, stream: S, reason: Refusal) {
        self.record(None, AccessKind::Refused { reason });
        let (_, mut writer) = tokio::io::split(stream);
        let _ = send(
            &mut writer,
            &Handshake::Refused {
                reason,
                supported: Vec::new(),
            },
        )
        .await;
    }

    async fn serve<S>(&self, stream: S, mut stop: watch::Receiver<bool>)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (read, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(read);
        let handshake = timeout(
            Duration::from_secs(limits::HANDSHAKE_TIMEOUT_SECS),
            self.handshake(&mut reader, &mut writer),
        )
        .await
        .unwrap_or(Err((None, Refusal::Timeout)));
        let client = match handshake {
            Ok(client) => client,
            Err((client, reason)) => {
                // Recorded first: once the helper hears of the refusal (or closes after its
                // own), the log has it.
                let name = client.and_then(|id| {
                    self.inner
                        .settings
                        .borrow()
                        .client(&id)
                        .map(|c| (id, c.name.clone()))
                });
                self.record(
                    name.as_ref().map(|(id, name)| (id, name.as_str())),
                    AccessKind::Refused { reason },
                );
                let _ = send(
                    &mut writer,
                    &Handshake::Refused {
                        reason,
                        supported: if reason == Refusal::UnsupportedProtocol {
                            vec![PROTOCOL]
                        } else {
                            Vec::new()
                        },
                    },
                )
                .await;
                return;
            }
        };
        if send(&mut writer, &Handshake::Ready).await.is_err() {
            return;
        }

        // Lines are read in their own task, so waiting for one can be cancelled safely.
        let (lines_tx, mut lines) = mpsc::channel(2);
        let reader_task = tokio::spawn(async move {
            loop {
                let line = read_line(&mut reader, limits::REQUEST_LINE_BYTES).await;
                let last = !matches!(line, Ok(Line::Data(_) | Line::TooLong));
                if lines_tx.send(line).await.is_err() || last {
                    break;
                }
            }
        });
        let mut settings = self.inner.settings.clone();
        let mut calls: VecDeque<Instant> = VecDeque::new();
        loop {
            let current = settings.borrow_and_update().grant(&client);
            let grant = match current {
                Ok(grant) => grant,
                Err(denied) => {
                    let _ = send(
                        &mut writer,
                        &Reply::Closed {
                            reason: refusal(denied),
                        },
                    )
                    .await;
                    break;
                }
            };
            let line = tokio::select! {
                changed = settings.changed() => {
                    if changed.is_err() { break; }
                    continue;
                }
                _ = stop.changed() => break,
                line = lines.recv() => line,
            };
            let bytes = match line {
                Some(Ok(Line::Data(bytes))) => bytes,
                Some(Ok(Line::TooLong)) => {
                    let _ = send(
                        &mut writer,
                        &Reply::Error {
                            id: 0,
                            code: CallError::Malformed,
                            message: "request line too long".into(),
                        },
                    )
                    .await;
                    continue;
                }
                _ => break,
            };
            let request: Request = match serde_json::from_slice(&bytes) {
                Ok(request) => request,
                Err(_) => {
                    let _ = send(
                        &mut writer,
                        &Reply::Error {
                            id: 0,
                            code: CallError::Malformed,
                            message: "not a request".into(),
                        },
                    )
                    .await;
                    continue;
                }
            };
            let who = (&grant.client, grant.client_name.as_str());
            let reply = match request {
                Request::ListTools { id } => Reply::Tools {
                    id,
                    tools: tools_for(&grant)
                        .into_iter()
                        .map(|tool| tool.as_str().to_owned())
                        .collect(),
                },
                Request::CallTool {
                    id,
                    name,
                    arguments,
                } => {
                    let Some(tool) = ToolName::parse(&name) else {
                        let _ = send(
                            &mut writer,
                            &Reply::Error {
                                id,
                                code: CallError::UnknownTool,
                                message: "unknown tool".into(),
                            },
                        )
                        .await;
                        continue;
                    };
                    if !tools_for(&grant).contains(&tool) {
                        self.record(
                            Some(who),
                            AccessKind::ToolCall {
                                tool,
                                messages: 0,
                                outcome: CallOutcome::Refused,
                            },
                        );
                        Reply::Error {
                            id,
                            code: CallError::UnknownTool,
                            message: format!("{tool} is not available to this client"),
                        }
                    } else if !rate_ok(&mut calls) {
                        self.record(
                            Some(who),
                            AccessKind::ToolCall {
                                tool,
                                messages: 0,
                                outcome: CallOutcome::Refused,
                            },
                        );
                        Reply::Error {
                            id,
                            code: CallError::RateLimited,
                            message: format!(
                                "more than {} tool calls per minute",
                                limits::CALLS_PER_MINUTE
                            ),
                        }
                    } else {
                        let call = timeout(
                            Duration::from_secs(limits::CALL_TIMEOUT_SECS),
                            execute(self.inner.access.as_ref(), &grant, tool, arguments),
                        );
                        let outcome = tokio::select! {
                            outcome = call => outcome,
                            denied = lost_grant(&mut settings, &client) => {
                                let _ = send(&mut writer, &Reply::Closed {
                                    reason: refusal(denied),
                                })
                                .await;
                                break;
                            }
                            _ = stop.changed() => break,
                        };
                        match outcome {
                            Ok(outcome) => {
                                self.record(
                                    Some(who),
                                    AccessKind::ToolCall {
                                        tool,
                                        messages: outcome.messages,
                                        outcome: if outcome.is_error {
                                            CallOutcome::Error
                                        } else {
                                            CallOutcome::Ok
                                        },
                                    },
                                );
                                Reply::Result {
                                    id,
                                    text: outcome.text,
                                    is_error: outcome.is_error,
                                }
                            }
                            Err(_) => {
                                self.record(
                                    Some(who),
                                    AccessKind::ToolCall {
                                        tool,
                                        messages: 0,
                                        outcome: CallOutcome::Refused,
                                    },
                                );
                                Reply::Error {
                                    id,
                                    code: CallError::Timeout,
                                    message: "the mail app took too long".into(),
                                }
                            }
                        }
                    }
                }
            };
            if send(&mut writer, &reply).await.is_err() {
                break;
            }
        }
        reader_task.abort();
    }

    /// The app side of the handshake. On failure: the client id if one was named, and why.
    async fn handshake<R, W>(
        &self,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<ClientId, (Option<ClientId>, Refusal)>
    where
        R: tokio::io::AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let hello = read_handshake(reader)
            .await
            .ok_or((None, Refusal::Malformed))?;
        let Handshake::Hello {
            protocol,
            client_id,
            nonce,
        } = hello
        else {
            return Err((None, Refusal::Malformed));
        };
        if protocol != PROTOCOL {
            return Err((Some(client_id), Refusal::UnsupportedProtocol));
        }
        let helper_nonce = parse_hex::<32>(&nonce).ok_or((Some(client_id), Refusal::Malformed))?;
        let key = {
            let settings = self.inner.settings.borrow();
            settings
                .grant(&client_id)
                .map_err(|denied| (Some(client_id), refusal(denied)))?;
            settings
                .client(&client_id)
                .map(|client| client.key.clone())
                .ok_or((Some(client_id), Refusal::UnknownClient))?
        };
        let app_nonce = random_bytes::<32>().map_err(|err| {
            tracing::error!(error = %err, "no randomness for the MCP handshake");
            (Some(client_id), Refusal::Malformed)
        })?;
        let challenge = Handshake::Challenge {
            protocol: PROTOCOL,
            nonce: crate::permissions::hex(&app_nonce),
            proof: proof(&key, Side::App, &client_id, &helper_nonce, &app_nonce),
        };
        send(writer, &challenge)
            .await
            .map_err(|_| (Some(client_id), Refusal::Malformed))?;
        let given = match read_handshake(reader).await {
            Some(Handshake::Auth { proof }) => proof,
            // The helper could not verify our proof: its key is not this client's key.
            Some(Handshake::Refused { .. }) => return Err((Some(client_id), Refusal::BadProof)),
            _ => return Err((Some(client_id), Refusal::Malformed)),
        };
        if !verify(
            &key,
            Side::Helper,
            &client_id,
            &helper_nonce,
            &app_nonce,
            &given,
        ) {
            return Err((Some(client_id), Refusal::BadProof));
        }
        // Revoked or switched off while the handshake ran?
        self.inner
            .settings
            .borrow()
            .grant(&client_id)
            .map_err(|denied| (Some(client_id), refusal(denied)))?;
        Ok(client_id)
    }
}

/// Writes one line. A helper that does not take it within [`limits::WRITE_TIMEOUT_SECS`] is
/// dropped, so a client that stops reading cannot hold a connection (or the shutdown).
async fn send<W: AsyncWrite + Unpin, T: Serialize>(writer: &mut W, message: &T) -> io::Result<()> {
    timeout(
        Duration::from_secs(limits::WRITE_TIMEOUT_SECS),
        write_line(writer, message),
    )
    .await
    .unwrap_or_else(|_| Err(io::Error::from(io::ErrorKind::TimedOut)))
}

async fn read_handshake<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Option<Handshake> {
    match read_line(reader, limits::HANDSHAKE_LINE_BYTES).await {
        Ok(Line::Data(bytes)) => serde_json::from_slice(&bytes).ok(),
        _ => None,
    }
}

fn refusal(denied: Denied) -> Refusal {
    match denied {
        Denied::Disabled => Refusal::Disabled,
        Denied::UnknownClient => Refusal::UnknownClient,
    }
}

/// Completes when `client` loses its grant (MCP off or revoked). Pending forever otherwise.
async fn lost_grant(settings: &mut watch::Receiver<McpSettings>, client: &ClientId) -> Denied {
    loop {
        if settings.changed().await.is_err() {
            return std::future::pending().await;
        }
        if let Err(denied) = settings.borrow_and_update().grant(client) {
            return denied;
        }
    }
}

/// Sliding window of [`limits::CALLS_PER_MINUTE`] calls per minute.
fn rate_ok(calls: &mut VecDeque<Instant>) -> bool {
    let now = Instant::now();
    while calls
        .front()
        .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
    {
        calls.pop_front();
    }
    if calls.len() >= limits::CALLS_PER_MINUTE {
        return false;
    }
    calls.push_back(now);
    true
}
