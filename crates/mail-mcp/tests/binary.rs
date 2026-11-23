//! The shipped helper binary as a client starts it: a child process with MCP on stdin/stdout
//! and the configuration in its environment.

#![allow(clippy::unwrap_used)]

mod support;

use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use support::{Fixture, data_block};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn spawn(env: &[(&str, &str)]) -> Helper {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mail-mcp"));
    command
        .env_remove("MAIL_MCP_ENDPOINT")
        .env_remove("MAIL_MCP_TOKEN")
        .env_remove("MAIL_MCP_TOKEN_FILE")
        .env("MAIL_MCP_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    Helper {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        child,
    }
}

impl Helper {
    async fn send(&mut self, message: Value) -> Value {
        let mut line = serde_json::to_vec(&message).unwrap();
        line.push(b'\n');
        self.stdin.write_all(&line).await.unwrap();
        self.stdin.flush().await.unwrap();
        let mut answer = String::new();
        tokio::time::timeout(Duration::from_secs(20), self.stdout.read_line(&mut answer))
            .await
            .expect("the helper answers")
            .unwrap();
        serde_json::from_str(&answer).unwrap()
    }

    async fn initialize(&mut self) -> Value {
        let answer = self
            .send(
                json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": { "name": "binary-test", "version": "1" } } }),
            )
            .await;
        let mut line = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_vec();
        line.push(b'\n');
        self.stdin.write_all(&line).await.unwrap();
        answer
    }

    async fn call(&mut self, id: u64, tool: &str) -> (String, bool) {
        let answer = self
            .send(json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": tool, "arguments": {} } }))
            .await;
        (
            answer["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{answer}"))
                .to_owned(),
            answer["result"]["isError"].as_bool().unwrap(),
        )
    }

    /// Closes stdin (how clients stop a stdio server) and waits for a clean exit.
    async fn close(mut self) {
        drop(self.stdin);
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .expect("the helper exits when stdin closes")
            .unwrap();
        assert!(status.success(), "{status}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_configuration_it_explains_what_is_missing() {
    let mut helper = spawn(&[]);
    let init = helper.initialize().await;
    assert_eq!(init["result"]["serverInfo"]["name"], "mail-mcp");
    let (text, is_error) = helper.call(1, "list_accounts").await;
    assert!(is_error);
    assert!(text.contains("MAIL_MCP_ENDPOINT is not set"), "{text}");
    helper.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_app_it_says_so() {
    let dir = tempfile::Builder::new().prefix("mcp").tempdir().unwrap();
    let endpoint = support::endpoint_in(dir.path());
    let token = mail_mcp::ClientToken::generate().unwrap().expose_secret();
    let mut helper = spawn(&[
        ("MAIL_MCP_ENDPOINT", endpoint.as_str()),
        ("MAIL_MCP_TOKEN", &token),
    ]);
    helper.initialize().await;
    let list = helper
        .send(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await;
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 6);
    let (text, is_error) = helper.call(2, "search_messages").await;
    assert!(is_error);
    assert!(text.contains("not running"), "{text}");
    helper.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_app_it_serves_tools() {
    let fixture = Fixture::start().await;
    let token = fixture.token.expose_secret();
    let mut helper = spawn(&[
        ("MAIL_MCP_ENDPOINT", fixture.endpoint.as_str()),
        ("MAIL_MCP_TOKEN", &token),
    ]);
    // Modern client: no initialize, metadata on every request.
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let discover = helper
        .send(
            json!({ "jsonrpc": "2.0", "id": "d", "method": "server/discover",
            "params": { "_meta": meta } }),
        )
        .await;
    assert_eq!(
        discover["result"]["supportedVersions"],
        json!(["2026-07-28"])
    );
    let call = helper
        .send(json!({ "jsonrpc": "2.0", "id": "c", "method": "tools/call",
            "params": { "name": "list_accounts", "arguments": {}, "_meta": meta } }))
        .await;
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(data_block(text)["accounts"].as_array().unwrap().len(), 2);

    // The app restarts: the helper reconnects by itself.
    let endpoint = fixture.endpoint.clone();
    let settings = fixture.settings.borrow().clone();
    fixture.stop().await;
    let call = helper
        .send(json!({ "jsonrpc": "2.0", "id": "x", "method": "tools/call",
            "params": { "name": "list_accounts", "arguments": {}, "_meta": meta } }))
        .await;
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("not reachable"), "{text}");
    let (_tx, rx) = tokio::sync::watch::channel(settings);
    let host = mail_mcp::McpHost::new(
        std::sync::Arc::new(support::FakeAccess::standard()),
        rx,
        std::sync::Arc::new(support::RecordingLog::default()),
    );
    let mut status = host.status();
    let run = tokio::spawn({
        let host = host.clone();
        async move { host.run(endpoint, std::future::pending()).await }
    });
    status
        .wait_for(|s| *s == mail_mcp::HostStatus::Listening)
        .await
        .unwrap();
    let call = helper
        .send(json!({ "jsonrpc": "2.0", "id": "y", "method": "tools/call",
            "params": { "name": "list_accounts", "arguments": {}, "_meta": meta } }))
        .await;
    assert_eq!(call["result"]["isError"], false, "{call}");
    run.abort();
    helper.close().await;
}

#[test]
fn version_and_help() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mail-mcp"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("mail-mcp "));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mail-mcp"))
        .arg("--bogus")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

/// Finding F5: the token from `MAIL_MCP_TOKEN_FILE`, so no command line has to carry it.
#[tokio::test(flavor = "multi_thread")]
async fn the_token_can_come_from_a_private_file() {
    let fixture = Fixture::start().await;
    let file = fixture.dir.path().join("token");
    std::fs::write(&file, format!("{}\n", fixture.token.expose_secret())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut helper = spawn(&[
        ("MAIL_MCP_ENDPOINT", fixture.endpoint.as_str()),
        ("MAIL_MCP_TOKEN_FILE", file.to_str().unwrap()),
        // A wrong inline token: the file wins.
        ("MAIL_MCP_TOKEN", "mcp1_wrong"),
    ]);
    helper.initialize().await;
    let (text, is_error) = helper.call(1, "list_accounts").await;
    assert!(!is_error, "{text}");
    assert_eq!(data_block(&text)["accounts"].as_array().unwrap().len(), 2);
    helper.close().await;
    fixture.stop().await;
}

/// A token file that others may read is refused, with the reason on stderr and in every tool
/// result.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_token_file_others_may_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::start().await;
    let file = fixture.dir.path().join("token");
    let token = fixture.token.expose_secret();
    std::fs::write(&file, &token).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let env = [
        ("MAIL_MCP_ENDPOINT", fixture.endpoint.as_str()),
        ("MAIL_MCP_TOKEN_FILE", file.to_str().unwrap()),
    ];

    // stdin closed at once: the helper reports the problem on stderr and ends.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mail-mcp"))
        .env_remove("MAIL_MCP_TOKEN")
        .env_remove("MAIL_MCP_LOG")
        .envs(env)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("MAIL_MCP_TOKEN_FILE: other users may access")
            && stderr.contains("(mode 644)")
            && stderr.contains("chmod 600"),
        "{stderr}"
    );
    assert!(!stderr.contains(&token), "never the token");

    let mut helper = spawn(&env);
    helper.initialize().await;
    let (text, is_error) = helper.call(1, "list_accounts").await;
    assert!(is_error);
    assert!(text.contains("chmod 600"), "{text}");
    helper.close().await;
    assert!(
        fixture.log.events().is_empty(),
        "the helper never connected: {:?}",
        fixture.log.events()
    );
    fixture.stop().await;
}
