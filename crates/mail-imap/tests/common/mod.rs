//! Shared helpers of the server tests. They need a running, seeded test server:
//!
//! ```text
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000
//! cargo test -p mail-imap -- --include-ignored
//! scripts/testmail.sh stop
//! ```
//!
//! Tests that change data work in their own folder (`VmTest-...`), created and deleted with a
//! read-write connection, so they do not disturb each other or the seed.

#![allow(dead_code)] // each test file uses a different part

use std::collections::HashMap;

use mail_imap::test_support::TestEnv;
use mail_imap::test_support::imap_proto::{AttributeValue, Response};
use mail_imap::{ImapBackend, ImapConnector};
use mail_model::{Access, MailConnector, Security};

pub async fn connect(env: &TestEnv, access: Access) -> ImapBackend {
    let config = env.config(&env.alice, Security::Tls, access);
    ImapConnector::new()
        .connect(&config)
        .await
        .unwrap_or_else(|err| panic!("connect as alice: {err}"))
}

/// A unique folder name for one test run.
pub fn scratch_name(test: &str) -> String {
    format!("VmTest-{test}-{}", std::process::id())
}

/// A small message for `APPEND`.
pub fn message(folder: &str, n: usize) -> String {
    format!(
        "From: Test Writer <writer@vicemail.test>\r\nTo: alice@vicemail.test\r\n\
         Subject: {folder} message {n}\r\nDate: Mon, 02 Nov 2026 10:{n:02}:00 +0000\r\n\
         Message-ID: <{folder}-{n}@vicemail.test>\r\n\r\nBody {n}.\r\n"
    )
}

/// Creates `name` with `count` messages through a read-write connection (test setup only).
pub async fn create_folder(writer: &mut ImapBackend, name: &str, count: usize) {
    // Leftovers of an aborted earlier run.
    let _ = writer
        .raw_command(&format!("DELETE \"{name}\""), |_| false)
        .await;
    let reply = writer
        .raw_command(&format!("CREATE \"{name}\""), |_| false)
        .await
        .expect("CREATE");
    assert!(reply.ok, "CREATE {name}: {}", reply.text);
    for n in 1..=count {
        let body = message(name, n);
        // LITERAL+ (the test server has it): the literal goes out with the command.
        let line = format!("APPEND \"{name}\" (\\Seen) {{{}+}}\r\n{body}", body.len());
        let reply = writer.raw_command(&line, |_| false).await.expect("APPEND");
        assert!(reply.ok, "APPEND to {name}: {}", reply.text);
    }
}

pub async fn delete_folder(writer: &mut ImapBackend, name: &str) {
    writer.raw_command("UNSELECT", |_| false).await.ok();
    let reply = writer
        .raw_command(&format!("DELETE \"{name}\""), |_| false)
        .await
        .expect("DELETE");
    assert!(reply.ok, "DELETE {name}: {}", reply.text);
}

/// `X-Vicemail-Seed` of every message in the open mailbox, by UID (a read-only FETCH with
/// PEEK, so it also works on a read-only connection).
pub async fn seed_keys(backend: &mut ImapBackend) -> HashMap<u32, String> {
    let mut keys = HashMap::new();
    let reply = backend
        .raw_command(
            "UID FETCH 1:* (UID BODY.PEEK[HEADER.FIELDS (X-VICEMAIL-SEED)])",
            |resp| {
                let Response::Fetch(_, attrs) = resp else {
                    return false;
                };
                let mut uid = None;
                let mut key = None;
                for attr in attrs {
                    match attr {
                        AttributeValue::Uid(u) => uid = Some(*u),
                        AttributeValue::BodySection {
                            data: Some(data), ..
                        } => {
                            let text = String::from_utf8_lossy(data);
                            key = text
                                .split_once(':')
                                .map(|(_, value)| value.trim().to_owned())
                                .filter(|v| !v.is_empty());
                        }
                        _ => {}
                    }
                }
                if let (Some(uid), Some(key)) = (uid, key) {
                    keys.insert(uid, key);
                }
                true
            },
        )
        .await
        .expect("fetch seed keys");
    assert!(reply.ok, "{}", reply.text);
    keys
}
