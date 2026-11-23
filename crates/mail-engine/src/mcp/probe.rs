//! "Test connection" (stage 12 "Einrichtungshilfe"): the app starts its helper program the way
//! an AI client would, with the endpoint and a token in the environment, and speaks MCP with it
//! over stdin/stdout: `initialize`, `tools/call list_accounts` (which makes the helper connect
//! and run the handshake with the app), `tools/list`.
//!
//! The token belongs to a temporary client that exists only for the test: it is never stored,
//! never written to the keychain and never logged (it is the app talking to itself; nothing
//! leaves the app). The answer of `list_accounts` is read only to see whether it is an error.

use std::process::Stdio;
use std::time::{Duration, Instant};

use mail_mcp::helper::{ENV_ENDPOINT, ENV_TOKEN, ENV_TOKEN_FILE};
use mail_mcp::{AccessKind, ClientToken, ToolName};
use mail_model::{CoreError, McpConnectionTest, McpHostState, McpRefusal, McpTestProblem, McpTool};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::broadcast;

use super::{lock, log};
use crate::core::{AppCore, Connector};

/// The whole test, from start to the last answer.
const TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Protocol version of the test client (the newest of the `initialize` era).
const PROTOCOL: &str = "2025-11-25";
/// Longest answer line read.
const MAX_LINE: usize = 4 * 1024 * 1024;

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let mut bytes = serde_json::to_vec(&line).map_err(|err| err.to_string())?;
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|err| format!("writing to the helper failed: {err}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|err| format!("writing to the helper failed: {err}"))?;
        loop {
            let mut buf = Vec::new();
            let read = (&mut self.stdout)
                .take(u64::try_from(MAX_LINE).unwrap_or(u64::MAX))
                .read_until(b'\n', &mut buf)
                .await
                .map_err(|err| format!("reading from the helper failed: {err}"))?;
            if read == 0 {
                return Err("the helper ended without an answer".into());
            }
            let value: Value = serde_json::from_slice(&buf)
                .map_err(|_| "the helper's answer is not JSON-RPC".to_owned())?;
            if value.get("id") != Some(&json!(id)) {
                // A notification, or something else; the helper sends none, but be tolerant.
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(format!(
                    "the helper answered {method} with an error: {error}"
                ));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| format!("the helper's answer to {method} has no result"));
        }
    }

    async fn notify(&mut self, method: &str) {
        let line = json!({"jsonrpc": "2.0", "method": method});
        if let Ok(mut bytes) = serde_json::to_vec(&line) {
            bytes.push(b'\n');
            let _ = self.stdin.write_all(&bytes).await;
        }
    }

    /// Closes stdin (the stdio transport's way to end) and waits a moment for the helper.
    async fn end(mut self) {
        drop(self.stdin);
        if tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

fn failed(problem: McpTestProblem, detail: Option<String>) -> McpConnectionTest {
    McpConnectionTest {
        ok: false,
        problem: Some(problem),
        refusal: None,
        helper_path: None,
        protocol_version: None,
        tools: Vec::new(),
        shared_accounts: 0,
        detail,
        duration_ms: 0,
    }
}

pub(super) async fn run<C: Connector>(core: &AppCore<C>) -> Result<McpConnectionTest, CoreError> {
    let started = Instant::now();
    let mut result = check(core).await?;
    result.duration_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    if result.ok {
        tracing::info!(ms = result.duration_ms, "MCP connection test passed");
    } else {
        tracing::warn!(problem = ?result.problem, refusal = ?result.refusal, "MCP connection test failed");
    }
    Ok(result)
}

async fn check<C: Connector>(core: &AppCore<C>) -> Result<McpConnectionTest, CoreError> {
    core.store_ref()?;
    let state = core.mcp();
    let helper = state.helper_info();
    let Some(path) = helper.path.clone().filter(|_| helper.found) else {
        return Ok(McpConnectionTest {
            helper_path: helper.path,
            ..failed(McpTestProblem::HelperMissing, None)
        });
    };
    let status = state.status();
    if !status.enabled {
        return Ok(McpConnectionTest {
            helper_path: Some(path),
            ..failed(McpTestProblem::Disabled, None)
        });
    }
    let endpoint = match (status.state, status.endpoint) {
        (McpHostState::Failed, _) | (_, None) => {
            return Ok(McpConnectionTest {
                helper_path: Some(path),
                ..failed(McpTestProblem::EndpointFailed, status.error)
            });
        }
        (_, Some(endpoint)) => endpoint,
    };
    let shared_accounts = {
        let accounts = core.list_accounts().await?;
        let settings = state.settings.borrow();
        let count = accounts
            .iter()
            .filter(|a| settings.access(&a.id) != mail_mcp::AccountAccess::Hidden)
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    };

    let token = ClientToken::generate().map_err(|err| CoreError::Internal(err.to_string()))?;
    lock(&state.probes).insert(token.id, token.key.clone());
    let reload = core.mcp_reload().await;
    let mut observed = state.sink.observers.subscribe();
    let outcome = match reload {
        Ok(_) => {
            tokio::time::timeout(TEST_TIMEOUT, talk(&path, &endpoint, &token.expose_secret())).await
        }
        Err(err) => Ok(Err((McpTestProblem::HelperFailed, err.to_string()))),
    };
    lock(&state.probes).remove(&token.id);
    if let Err(err) = core.mcp_reload().await {
        tracing::warn!(error = %err, "MCP settings could not be republished after the test");
    }

    let mut result = McpConnectionTest {
        ok: false,
        problem: None,
        refusal: None,
        helper_path: Some(path),
        protocol_version: None,
        tools: Vec::new(),
        shared_accounts,
        detail: None,
        duration_ms: 0,
    };
    match outcome {
        Err(_) => {
            result.problem = Some(McpTestProblem::Timeout);
        }
        Ok(Err((problem, detail))) => {
            result.problem = Some(problem);
            result.detail = Some(detail);
        }
        Ok(Ok(talked)) => {
            result.protocol_version = talked.protocol;
            result.tools = talked.tools;
            if talked.call_ok {
                result.ok = true;
            } else {
                let refusal = refusal_of(&mut observed, &token);
                result.problem = Some(if refusal.is_some() {
                    McpTestProblem::Refused
                } else {
                    McpTestProblem::Unreachable
                });
                result.refusal = refusal;
                result.detail = talked.error;
            }
        }
    }
    Ok(result)
}

/// The refusal the host reported for the probe's client, if any.
fn refusal_of(
    observed: &mut broadcast::Receiver<mail_mcp::AccessEvent>,
    token: &ClientToken,
) -> Option<McpRefusal> {
    let mut found = None;
    while let Ok(event) = observed.try_recv() {
        if event.client_id == Some(token.id)
            && let AccessKind::Refused { reason } = event.kind
        {
            found = Some(log::refusal(reason));
        }
    }
    found
}

struct Talked {
    protocol: Option<String>,
    call_ok: bool,
    error: Option<String>,
    tools: Vec<McpTool>,
}

async fn talk(
    helper: &str,
    endpoint: &str,
    token: &str,
) -> Result<Talked, (McpTestProblem, String)> {
    let mut child = Command::new(helper)
        .env(ENV_ENDPOINT, endpoint)
        .env(ENV_TOKEN, token)
        // A token file in the app's own environment would win over the probe's token.
        .env_remove(ENV_TOKEN_FILE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| {
            (
                McpTestProblem::HelperFailed,
                format!("the helper did not start: {err}"),
            )
        })?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err((
            McpTestProblem::HelperFailed,
            "no pipes to the helper".into(),
        ));
    };
    let mut session = Session {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    };
    let helper_failed = |detail: String| (McpTestProblem::HelperFailed, detail);
    let init = session
        .request(
            1,
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "connection-test", "version": "1"}
            }),
        )
        .await
        .map_err(helper_failed)?;
    let protocol = init
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(str::to_owned);
    session.notify("notifications/initialized").await;
    let call = session
        .request(
            2,
            "tools/call",
            json!({"name": ToolName::ListAccounts.as_str(), "arguments": {}}),
        )
        .await
        .map_err(helper_failed)?;
    let call_ok = call.get("isError").and_then(Value::as_bool) == Some(false);
    let error = (!call_ok).then(|| {
        call.pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("no text")
            .chars()
            .take(500)
            .collect::<String>()
    });
    let listed = session
        .request(3, "tools/list", json!({}))
        .await
        .map_err(helper_failed)?;
    let tools = listed
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                .filter_map(McpTool::parse)
                .collect()
        })
        .unwrap_or_default();
    session.end().await;
    Ok(Talked {
        protocol,
        call_ok,
        error,
        tools,
    })
}
