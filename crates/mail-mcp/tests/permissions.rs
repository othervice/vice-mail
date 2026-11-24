//! Permissions, authentication and the endpoint (stage 12 acceptance): MCP off, hidden accounts,
//! the draft tool, tokens, revocation, the access log, rate limits and the socket's file modes.

#![allow(clippy::unwrap_used)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use mail_mcp::helper::HelperConfig;
use mail_mcp::{
    AccessKind, AccountAccess, CallOutcome, ClientKey, ClientToken, HostStatus, MailAccess,
    Refusal, SearchQuery, ToolName,
};
use serde_json::json;
use support::{A, B, C, Call, FakeAccess, Fixture, RawClient, SECRET, account_id, data_block};
use tokio::sync::Notify;

const READ_TOOLS: [&str; 6] = [
    "list_accounts",
    "list_mailboxes",
    "search_messages",
    "list_messages",
    "read_message",
    "read_thread",
];

fn with_draft() -> Vec<&'static str> {
    let mut tools = READ_TOOLS.to_vec();
    tools.push("create_draft");
    tools
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_off_means_no_endpoint_and_no_answers() {
    let fixture = Fixture::start_with(FakeAccess::standard(), |token| {
        let mut settings = support::standard_settings(token);
        settings.enabled = false;
        settings
    })
    .await;
    fixture.wait_status(HostStatus::Stopped).await;
    #[cfg(unix)]
    assert!(
        !std::path::Path::new(fixture.endpoint.as_str()).exists(),
        "no socket while MCP is off"
    );
    let mut client = RawClient::new(Ok(fixture.config()));
    // The helper still lists the read tools, so the model can tell the user what is wrong ...
    assert_eq!(client.tool_names().await, READ_TOOLS);
    // ... but every call is refused.
    for tool in READ_TOOLS {
        let (text, is_error) = client.call(tool, json!({})).await;
        assert!(is_error, "{tool}");
        assert!(text.contains("not reachable"), "{text}");
    }
    assert!(fixture.access.calls().is_empty(), "the app was never asked");

    // Switched on: the same helper reaches the app without a restart.
    fixture.update(|s| s.enabled = true);
    fixture.wait_status(HostStatus::Listening).await;
    let (text, is_error) = client.call("list_accounts", json!({})).await;
    assert!(!is_error, "{text}");

    // Emergency stop: the open connection is closed and the endpoint removed.
    fixture.update(|s| s.emergency_stop());
    fixture.wait_status(HostStatus::Stopped).await;
    let (text, is_error) = client.call("list_accounts", json!({})).await;
    assert!(is_error);
    assert!(
        text.contains("turned off") || text.contains("not reachable"),
        "{text}"
    );
    #[cfg(unix)]
    assert!(!std::path::Path::new(fixture.endpoint.as_str()).exists());
    let (text, _) = client
        .call("read_message", json!({ "message_id": 101 }))
        .await;
    assert!(text.contains("not reachable"), "{text}");
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hidden_account_is_invisible_to_every_tool() {
    // The fake returns data of every account it holds; only the host's checks hide B.
    let fixture = Fixture::start_with(FakeAccess::leaky(), support::standard_settings).await;
    // Positive control: asked for A only, the fake still hands out B's mail.
    let leaked = fixture
        .access
        .search(SearchQuery {
            query: SECRET.into(),
            accounts: vec![account_id(A)],
            mailbox: None,
            limit: 10,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(leaked.hits.len(), 2);
    assert!(
        leaked
            .hits
            .iter()
            .all(|hit| hit.account_id == account_id(B))
    );
    fixture.access.calls.lock().unwrap().clear();

    let mut client = RawClient::new(Ok(fixture.config()));
    let b = account_id(B).to_string();
    let missing = account_id(0xDEAD).to_string();
    let mut outputs = Vec::new();

    let (accounts, _) = client.call("list_accounts", json!({})).await;
    assert_eq!(
        data_block(&accounts)["accounts"].as_array().unwrap().len(),
        2
    );
    outputs.push(accounts);

    // The same answer for the hidden account as for one that does not exist.
    for tool in [
        "list_mailboxes",
        "search_messages",
        "list_messages",
        "create_draft",
    ] {
        let arguments = |account: &str| match tool {
            "search_messages" => json!({ "query": "report", "account_id": account }),
            "list_messages" => json!({ "filter": "unread", "account_id": account }),
            "create_draft" => json!({ "account_id": account, "body": "x" }),
            _ => json!({ "account_id": account }),
        };
        let hidden = client.call(tool, arguments(&b)).await;
        let absent = client.call(tool, arguments(&missing)).await;
        assert!(hidden.1, "{tool}");
        assert_eq!(hidden, absent, "{tool}");
        outputs.push(hidden.0);
    }
    for tool in ["read_message", "read_thread"] {
        let hidden = client.call(tool, json!({ "message_id": 201 })).await;
        let absent = client.call(tool, json!({ "message_id": 999 })).await;
        assert!(hidden.1, "{tool}");
        assert_eq!(hidden, absent, "{tool}");
        outputs.push(hidden.0);
    }
    // B's inbox by id, like a mailbox that does not exist.
    let hidden = client
        .call(
            "search_messages",
            json!({ "query": "report", "mailbox_id": 20 }),
        )
        .await;
    let absent = client
        .call(
            "search_messages",
            json!({ "query": "report", "mailbox_id": 999 }),
        )
        .await;
    assert!(hidden.1);
    assert_eq!(hidden, absent);

    // Searching for B's content finds nothing, also over all accounts and in every list.
    for (tool, arguments) in [
        ("search_messages", json!({ "query": SECRET })),
        ("search_messages", json!({ "query": "report" })),
        ("search_messages", json!({ "query": "safe code" })),
        ("list_messages", json!({ "filter": "unread" })),
        ("list_messages", json!({ "filter": "flagged" })),
        ("read_thread", json!({ "message_id": 101 })),
    ] {
        let (text, is_error) = client.call(tool, arguments).await;
        assert!(!is_error, "{tool}: {text}");
        outputs.push(text);
    }
    // A reply to B's message from C is refused.
    let (text, is_error) = client
        .call(
            "create_draft",
            json!({ "account_id": account_id(C).to_string(), "in_reply_to": 201, "body": "x" }),
        )
        .await;
    assert!(is_error);
    assert!(text.contains("in_reply_to is not a message of this account"));
    outputs.push(text);

    for text in &outputs {
        for secret in [SECRET, b.as_str(), "alice@private.example", "Private"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }
    assert!(
        !fixture.access.accounts_asked().contains(&account_id(B)),
        "the host never named the hidden account"
    );
    assert!(fixture.access.drafts().is_empty());
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn create_draft_exists_only_with_permission() {
    let fixture = Fixture::start().await;
    let mut client = RawClient::new(Ok(fixture.config()));
    assert_eq!(client.tool_names().await, with_draft());

    // Drafts off for C: the tool disappears and a call is an unknown tool.
    fixture.update(|s| {
        s.accounts.insert(account_id(C), AccountAccess::Read);
    });
    assert_eq!(client.tool_names().await, READ_TOOLS);
    let answer = client
        .request(
            "tools/call",
            json!({ "name": "create_draft", "arguments": {
                "account_id": account_id(C).to_string(), "body": "hello" } }),
        )
        .await;
    assert_eq!(answer["error"]["code"], -32602, "{answer}");
    assert!(fixture.access.drafts().is_empty());
    let refused = fixture.log.events().into_iter().any(|e| {
        e.kind
            == AccessKind::ToolCall {
                tool: ToolName::CreateDraft,
                messages: 0,
                outcome: CallOutcome::Refused,
            }
    });
    assert!(refused, "the refused draft is in the access log");

    // Back on: listed and working again, without reconnecting.
    fixture.update(|s| {
        s.accounts
            .insert(account_id(C), AccountAccess::ReadAndDrafts);
    });
    assert_eq!(client.tool_names().await, with_draft());
    let (text, is_error) = client
        .call(
            "create_draft",
            json!({
                "account_id": account_id(C).to_string(),
                "to": ["bob@example.com"],
                "cc": ["carol@example.com"],
                "subject": "Re: lunch\r\nBcc: everyone@example.com",
                "body": "Sounds good.\r\n\u{E0041}Bye",
                "in_reply_to": 301,
            }),
        )
        .await;
    assert!(!is_error, "{text}");
    let drafts = fixture.access.drafts();
    assert_eq!(drafts.len(), 1);
    let draft = &drafts[0];
    assert_eq!(draft.to, ["bob@example.com"]);
    assert_eq!(draft.cc, ["carol@example.com"]);
    assert_eq!(
        draft.subject, "Re: lunch Bcc: everyone@example.com",
        "no header injection"
    );
    assert_eq!(draft.body, "Sounds good.\nBye");
    assert_eq!(draft.in_reply_to, Some(mail_model::MessageId(301)));
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_or_missing_tokens_are_refused() {
    let fixture = Fixture::start().await;
    let stranger = ClientToken::generate().unwrap();
    let wrong_key = ClientToken {
        id: fixture.token.id,
        key: ClientKey::from_bytes([7; 32]),
    };
    for (token, expected) in [
        (stranger, "revoked or its token is wrong"),
        (wrong_key, "token in the configuration does not match"),
    ] {
        let mut client = RawClient::new(Ok(HelperConfig {
            endpoint: fixture.endpoint.clone(),
            token,
        }));
        let (text, is_error) = client.call("list_accounts", json!({})).await;
        assert!(is_error);
        assert!(text.contains(expected), "{text}");
    }
    let mut unconfigured = RawClient::new(Err("MAIL_MCP_TOKEN is not set".into()));
    let (text, is_error) = unconfigured.call("list_accounts", json!({})).await;
    assert!(is_error);
    assert!(
        text.contains("not set up (MAIL_MCP_TOKEN is not set)"),
        "{text}"
    );

    let refusals: Vec<(Option<mail_mcp::ClientId>, AccessKind)> = fixture
        .log
        .events()
        .into_iter()
        .map(|e| (e.client_id, e.kind))
        .collect();
    // Recorded before the helper heard of (or reported) the refusal, so they are here now.
    assert!(
        refusals.contains(&(
            None,
            AccessKind::Refused {
                reason: Refusal::UnknownClient
            }
        )),
        "{refusals:?}"
    );
    assert!(
        refusals.contains(&(
            Some(fixture.token.id),
            AccessKind::Refused {
                reason: Refusal::BadProof
            }
        )),
        "{refusals:?}"
    );
    assert!(
        fixture.access.calls().is_empty(),
        "nothing reached the mail data"
    );
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_a_client_takes_effect_at_once() {
    let fixture = Fixture::start().await;
    let mut client = RawClient::new(Ok(fixture.config()));
    let (_, is_error) = client.call("list_accounts", json!({})).await;
    assert!(!is_error);

    // Revoked during a running call: the call ends at once with the revocation.
    let gate = Arc::new(Notify::new());
    *fixture.access.gate.lock().unwrap() = Some(gate.clone());
    let call = tokio::spawn(async move {
        let answer = client
            .call("read_message", json!({ "message_id": 101 }))
            .await;
        (client, answer)
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture
            .access
            .calls()
            .iter()
            .any(|c| matches!(c, Call::Message(..)))
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let id = fixture.token.id;
    fixture.update(|s| {
        assert!(s.revoke(&id));
    });
    let (mut client, (text, is_error)) = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("the call ends without waiting for the mail data")
        .unwrap();
    assert!(is_error);
    assert!(text.contains("revoked"), "{text}");
    gate.notify_waiters();

    // And every later call, on a new connection, too.
    let (text, is_error) = client.call("list_accounts", json!({})).await;
    assert!(is_error);
    assert!(text.contains("revoked"), "{text}");
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_tool_call_is_in_the_access_log() {
    let fixture = Fixture::start().await;
    let mut client = RawClient::new(Ok(fixture.config()));
    client.call("list_accounts", json!({})).await;
    client
        .call("search_messages", json!({ "query": "report" }))
        .await;
    client
        .call("read_message", json!({ "message_id": 101 }))
        .await;
    client
        .call("read_thread", json!({ "message_id": 102 }))
        .await;
    client
        .call("read_message", json!({ "message_id": 999 }))
        .await;
    let calls: Vec<(ToolName, u32, CallOutcome)> = fixture
        .log
        .events()
        .into_iter()
        .map(|event| {
            assert_eq!(event.client_id, Some(fixture.token.id));
            assert_eq!(event.client_name.as_deref(), Some("Test client"));
            assert!(event.at > 1_700_000_000_000, "a timestamp in milliseconds");
            match event.kind {
                AccessKind::ToolCall {
                    tool,
                    messages,
                    outcome,
                } => (tool, messages, outcome),
                other => panic!("unexpected {other:?}"),
            }
        })
        .collect();
    assert_eq!(
        calls,
        [
            (ToolName::ListAccounts, 0, CallOutcome::Ok),
            (ToolName::SearchMessages, 3, CallOutcome::Ok),
            (ToolName::ReadMessage, 1, CallOutcome::Ok),
            (ToolName::ReadThread, 2, CallOutcome::Ok),
            (ToolName::ReadMessage, 0, CallOutcome::Error),
        ]
    );
    // Only metadata: the event serialises without arguments or content.
    let json = serde_json::to_string(&fixture.log.events()).unwrap();
    assert!(
        !json.contains("report") && !json.contains("Quarterly"),
        "{json}"
    );
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_calls_are_rate_limited() {
    let fixture = Fixture::start().await;
    let mut client = RawClient::new(Ok(fixture.config()));
    for _ in 0..mail_mcp::limits::CALLS_PER_MINUTE {
        let (_, is_error) = client.call("list_accounts", json!({})).await;
        assert!(!is_error);
    }
    let (text, is_error) = client.call("list_accounts", json!({})).await;
    assert!(is_error);
    assert!(text.contains("Too many tool calls"), "{text}");
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn crafted_arguments_never_write() {
    let fixture = Fixture::start().await;
    let mut client = RawClient::new(Ok(fixture.config()));
    let a = account_id(A).to_string();
    for (tool, arguments) in [
        ("search_messages", json!({ "query": "x", "action": "send" })),
        (
            "search_messages",
            json!({ "query": "\" OR 1=1; DELETE FROM message; --" }),
        ),
        ("search_messages", json!({ "query": "x", "mailbox_id": -1 })),
        (
            "list_messages",
            json!({ "filter": "unread", "mark_read": true }),
        ),
        ("read_message", json!({ "message_id": 101, "mark": "seen" })),
        ("read_message", json!({ "message_id": 101.5 })),
        ("read_thread", json!({ "message_id": [101] })),
        (
            "list_mailboxes",
            json!({ "account_id": a, "create": "Junk" }),
        ),
        (
            "create_draft",
            json!({ "account_id": a, "body": "x", "send": true }),
        ),
        ("create_draft", json!({ "account_id": a, "body": "x" })),
        ("list_accounts", json!({ "writes_enabled": true })),
    ] {
        let (_, _) = client.call(tool, arguments).await;
    }
    for name in [
        "send_message",
        "delete_message",
        "move_message",
        "mark_read",
        "set_settings",
    ] {
        let answer = client
            .request("tools/call", json!({ "name": name, "arguments": {} }))
            .await;
        assert_eq!(answer["error"]["code"], -32602, "{name}");
    }
    assert!(
        fixture.access.drafts().is_empty(),
        "no draft: A allows reading only, and unknown arguments are refused"
    );
    assert!(
        fixture
            .access
            .calls()
            .iter()
            .all(|c| !matches!(c, Call::CreateDraft(_))),
        "the only writing method of MailAccess was never called"
    );
    fixture.stop().await;
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_socket_is_private_and_owned() {
        let fixture = Fixture::start().await;
        let socket_path = Path::new(fixture.endpoint.as_str()).to_owned();
        let socket = socket_path.as_path();
        assert_eq!(mode(socket), 0o600);
        assert_eq!(mode(socket.parent().unwrap()), 0o700);

        // A second host on the same endpoint does not take it over.
        let (_tx, rx) = tokio::sync::watch::channel(support::standard_settings(&fixture.token));
        let second = mail_mcp::McpHost::new(
            Arc::new(FakeAccess::standard()),
            rx,
            Arc::new(support::RecordingLog::default()),
        );
        let mut status = second.status();
        let endpoint = fixture.endpoint.clone();
        let run = tokio::spawn({
            let second = second.clone();
            async move { second.run(endpoint, std::future::pending()).await }
        });
        let failed = tokio::time::timeout(
            Duration::from_secs(5),
            status.wait_for(|s| matches!(s, HostStatus::Failed(_))),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        assert!(
            matches!(&failed, HostStatus::Failed(text) if text.contains("already listening")),
            "{failed:?}"
        );
        run.abort();
        assert!(socket.exists(), "the first host's socket is untouched");
        fixture.stop().await;
        assert!(!socket.exists(), "the socket is removed on shutdown");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stale_sockets_are_replaced_and_other_files_left_alone() {
        let dir = tempfile::Builder::new().prefix("mcp").tempdir().unwrap();
        let endpoint = support::endpoint_in(dir.path());
        let path = Path::new(endpoint.as_str()).to_owned();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A stale socket: bound, then the listener dropped without removing the file.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        let token = ClientToken::generate().unwrap();
        let (tx, rx) = tokio::sync::watch::channel(support::standard_settings(&token));
        let host = mail_mcp::McpHost::new(
            Arc::new(FakeAccess::standard()),
            rx,
            Arc::new(support::RecordingLog::default()),
        );
        let mut status = host.status();
        let run = tokio::spawn({
            let host = host.clone();
            let endpoint = endpoint.clone();
            async move { host.run(endpoint, std::future::pending()).await }
        });
        tokio::time::timeout(
            Duration::from_secs(5),
            status.wait_for(|s| *s == HostStatus::Listening),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(mode(&path), 0o600);
        // Switch off: socket gone. Put a regular file there; switching on leaves it alone.
        tx.send_modify(|s| s.enabled = false);
        tokio::time::timeout(
            Duration::from_secs(5),
            status.wait_for(|s| *s == HostStatus::Stopped),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!path.exists());
        std::fs::write(&path, b"not a socket").unwrap();
        tx.send_modify(|s| s.enabled = true);
        tokio::time::timeout(
            Duration::from_secs(5),
            status.wait_for(|s| matches!(s, HostStatus::Failed(_))),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"not a socket");
        run.abort();
    }

    async fn raw_handshake(fixture: &Fixture, first: &str) -> serde_json::Value {
        let stream = UnixStream::connect(fixture.endpoint.as_str())
            .await
            .unwrap();
        let (read, mut write) = stream.into_split();
        write.write_all(first.as_bytes()).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        let mut line = String::new();
        tokio::time::timeout(
            Duration::from_secs(10),
            BufReader::new(read).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_handshake_rejects_what_it_does_not_know() {
        let fixture = Fixture::start().await;
        let id = fixture.token.id;
        assert_eq!(
            raw_handshake(&fixture, "hello").await,
            json!({ "type": "refused", "reason": "malformed" })
        );
        assert_eq!(
            raw_handshake(
                &fixture,
                &format!(r#"{{"type":"hello","protocol":2,"client_id":"{id}","nonce":"00"}}"#)
            )
            .await,
            json!({ "type": "refused", "reason": "unsupported_protocol", "supported": [1] })
        );
        // Skipping the challenge: `auth` first.
        assert_eq!(
            raw_handshake(&fixture, r#"{"type":"auth","proof":"00"}"#).await,
            json!({ "type": "refused", "reason": "malformed" })
        );
        // A long line before authentication.
        assert_eq!(
            raw_handshake(&fixture, &"x".repeat(10_000)).await,
            json!({ "type": "refused", "reason": "malformed" })
        );
        // A valid hello gets a challenge; a wrong proof is refused.
        let stream = UnixStream::connect(fixture.endpoint.as_str())
            .await
            .unwrap();
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        let nonce = "11".repeat(32);
        write
            .write_all(
                format!("{{\"type\":\"hello\",\"protocol\":1,\"client_id\":\"{id}\",\"nonce\":\"{nonce}\"}}\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        let challenge: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(challenge["type"], "challenge");
        assert_eq!(challenge["nonce"].as_str().unwrap().len(), 64);
        write
            .write_all(
                format!("{{\"type\":\"auth\",\"proof\":\"{}\"}}\n", "ab".repeat(32)).as_bytes(),
            )
            .await
            .unwrap();
        line.clear();
        read.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap(),
            json!({ "type": "refused", "reason": "bad_proof" })
        );
        // Silence: refused after the handshake timeout (5 s).
        let started = std::time::Instant::now();
        let stream = UnixStream::connect(fixture.endpoint.as_str())
            .await
            .unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap(),
            json!({ "type": "refused", "reason": "timeout" })
        );
        assert!(started.elapsed() >= Duration::from_secs(4));
        assert!(fixture.access.calls().is_empty());
        fixture.stop().await;
    }
}
