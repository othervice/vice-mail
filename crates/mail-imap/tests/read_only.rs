//! Acceptance test of stage 2: in read-only mode no writing command leaves the adapter.
//!
//! Two independent recorders watch the connection: a wire tap inside TLS (what the adapter
//! wrote, in plain text) and a TCP proxy that counts every byte from client to server. Each
//! write attempt must fail with `ReadOnly`, add nothing to the tap and move no byte through the
//! proxy; a NOOP afterwards is the positive control that both recorders record. Needs the test
//! server (see `tests/common/mod.rs`).

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use mail_imap::test_support::{Proxy, TestEnv};
use mail_imap::{ImapConnector, WireTap};
use mail_model::{Access, BackendError, MailBackend, MailConnector, Security, UidSet};

/// Writing commands of every kind, as the adapter would send them in stage 4 and later.
const WRITES: &[(&str, &str)] = &[
    ("UID STORE 1 +FLAGS (\\Flagged)", "UID STORE"),
    ("STORE 1 -FLAGS (\\Seen)", "STORE"),
    ("UID STORE 1 +FLAGS.SILENT (\\Deleted)", "UID STORE"),
    (
        "APPEND \"INBOX\" (\\Seen) {12+}\r\nSubject: x\r\n",
        "APPEND",
    ),
    ("UID COPY 1 \"Trash\"", "UID COPY"),
    ("COPY 1 \"Trash\"", "COPY"),
    ("UID MOVE 1 \"Trash\"", "UID MOVE"),
    ("MOVE 1 \"Trash\"", "MOVE"),
    ("EXPUNGE", "EXPUNGE"),
    ("UID EXPUNGE 1", "UID EXPUNGE"),
    ("SELECT \"INBOX\"", "SELECT"),
    ("CLOSE", "CLOSE"),
    ("UNSELECT", "UNSELECT"),
    ("CREATE \"VmTest-must-not-exist\"", "CREATE"),
    ("DELETE \"Trash\"", "DELETE"),
    ("RENAME \"Trash\" \"Bin\"", "RENAME"),
    ("SUBSCRIBE \"Trash\"", "SUBSCRIBE"),
    ("UNSUBSCRIBE \"INBOX\"", "UNSUBSCRIBE"),
    (
        "SETMETADATA \"INBOX\" (/private/comment \"x\")",
        "SETMETADATA",
    ),
    ("UID FETCH 1 BODY[]", "UID FETCH BODY["),
    ("FETCH 1 (RFC822)", "FETCH RFC822"),
];

#[tokio::test]
#[ignore = "needs the test server"]
async fn no_writing_command_leaves_the_adapter_in_read_only_mode() {
    let env = TestEnv::require();
    let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
        .parse()
        .expect("addr");
    let proxy = Proxy::start(upstream).await.expect("proxy");
    let tap = WireTap::new();
    let config = env.config_at(
        &env.alice,
        "127.0.0.1",
        proxy.port(),
        Security::Tls,
        Access::ReadOnly,
    );
    let mut backend = ImapConnector::new()
        .with_wire_tap(tap.clone())
        .connect(&config)
        .await
        .expect("connect read-only");
    assert_eq!(backend.access(), Access::ReadOnly);

    // Normal reading works and opens with EXAMINE.
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    assert!(inbox.read_only);
    let newest = UidSet::range(inbox.uid_next.saturating_sub(5), inbox.uid_next - 1);
    let before = backend
        .fetch_flags(&inbox, &newest, None)
        .await
        .expect("flags");
    backend
        .fetch_headers(&inbox, &newest)
        .await
        .expect("headers, PEEK only");

    for (line, name) in WRITES {
        let (tapped, sent) = (tap.len(), proxy.bytes_to_server());
        let result = backend.raw_command(line, |_| false).await;
        assert_eq!(
            result,
            Err(BackendError::ReadOnly((*name).to_owned())),
            "{line:?}"
        );
        // Give a stray write the chance to reach the proxy before counting.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(tap.len(), tapped, "{line:?} reached the wire tap");
        assert_eq!(
            proxy.bytes_to_server(),
            sent,
            "{line:?} moved bytes to the server"
        );
    }

    // Positive control: a permitted command shows up in both recorders.
    let (tapped, sent) = (tap.len(), proxy.bytes_to_server());
    backend.noop().await.expect("noop");
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(tap.len() > tapped, "the wire tap records");
    assert!(proxy.bytes_to_server() > sent, "the proxy counts");

    // Over the whole session: only reading commands, EXAMINE instead of SELECT.
    let verbs = tap.verbs();
    let reading = ["LOGIN", "ENABLE", "EXAMINE", "UID FETCH", "NOOP"];
    for verb in &verbs {
        assert!(
            reading.contains(&verb.as_str()),
            "unexpected command on the wire: {verb} in {verbs:?}"
        );
    }
    assert!(
        tap.lines()
            .iter()
            .all(|l| !l.to_ascii_uppercase().contains("BODY["))
    );
    println!("commands on the wire: {:?}", tap.lines());

    // The server state did not change: the same flags as before.
    let after = backend
        .fetch_flags(&inbox, &newest, None)
        .await
        .expect("flags");
    assert_eq!(before, after);
    backend.logout().await.expect("logout");
}
