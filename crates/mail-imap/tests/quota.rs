//! Stage 20, task E (decision 0060): the quota against the test server (Stalwart announces
//! `QUOTA` and `QUOTA=RES-STORAGE`), read on a read-only connection; with a storage limit set
//! through the server's management API (`testmail/quota.py`) it is reported in bytes, and an
//! APPEND over the limit is a full mailbox (`NO [OVERQUOTA]`). Needs the test server (see
//! `tests/common/mod.rs`); the quota is cleared again at the end.

mod common;

use std::process::Command;

use mail_imap::ImapConnector;
use mail_imap::test_support::TestEnv;
use mail_model::{Access, BackendError, Flag, MailBackend, MailWriter, Security, WriteConnector};

/// What `testmail/quota.py` printed: the limit (bytes) if one is set, and the bytes in use.
struct Quota {
    limit: Option<u64>,
    used: u64,
}

/// The number after `"key": ` in the script's JSON line (no JSON parser in this crate's tests).
fn number_after(text: &str, key: &str) -> Option<u64> {
    let rest = text.split(&format!("\"{key}\": ")).nth(1)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse().ok())
}

/// Runs `testmail/quota.py`.
fn quota(env: &TestEnv, args: &[&str]) -> Quota {
    let out = Command::new("python3")
        .arg("-B")
        .arg(env.repo.join("testmail/quota.py"))
        .args(args)
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "quota.py {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    Quota {
        limit: number_after(&text, "maxDiskQuota"),
        used: number_after(&text, "used").unwrap_or(0),
    }
}

/// Clears alice's limit when the test ends, also when it fails: a limit left behind makes the
/// server refuse mail for her in every later test.
struct ClearQuota<'a>(&'a TestEnv);

impl Drop for ClearQuota<'_> {
    fn drop(&mut self) {
        quota(self.0, &["clear", "alice"]);
    }
}

/// A limit below this is not a meaningful "97 % full" (the server counts in KiB).
const SMALL_USE: u64 = 64 * 1024;

#[tokio::test]
#[ignore = "needs the test server"]
async fn the_test_servers_quota_is_read_and_a_full_mailbox_refuses_an_append() {
    let env = TestEnv::load().expect("test env");
    let used = quota(&env, &["clear", "alice"]).used;
    let _clear = ClearQuota(&env);

    // Without a limit the server names a root but no STORAGE: nothing is reported.
    let mut reader = common::connect(&env, Access::ReadOnly).await;
    assert!(reader.capabilities().has("QUOTA"));
    assert_eq!(reader.quota().await.expect("quota"), None);

    // A limit just above the use (on a fresh server with almost nothing stored: 64 KiB),
    // reported in bytes; with mail stored, at least 90 % full.
    let limit = if used >= SMALL_USE {
        quota(&env, &["fill", "alice", "97"])
    } else {
        quota(&env, &["set", "alice", &SMALL_USE.to_string()])
    }
    .limit
    .expect("limit");
    let reported = reader.quota().await.expect("quota").expect("reported");
    assert!(
        reported.limit_bytes.abs_diff(limit) < 2048,
        "{reported:?} vs {limit}"
    );
    assert!(
        reported.used_bytes.abs_diff(used) < 2048,
        "{reported:?} vs {used}"
    );
    if used >= SMALL_USE {
        assert!(reported.percent() >= 90, "{reported:?}");
    }
    reader.logout().await.ok();

    // A message larger than what is left: the server refuses it as over quota.
    let config = env.config(&env.alice, Security::Tls, Access::ReadWrite);
    let mut writer = ImapConnector::new()
        .connect_writer(&config)
        .await
        .expect("writer");
    let mut message = format!(
        "From: alice@vicemail.test\r\nTo: alice@vicemail.test\r\nSubject: too big\r\n\
         Message-ID: <quota-{}@vicemail.test>\r\n\r\n",
        std::process::id()
    )
    .into_bytes();
    // About 2.3 MB of text: more than what is left.
    for _ in 0..30_000 {
        message.extend_from_slice(&[b'x'; 76]);
        message.extend_from_slice(b"\r\n");
    }
    let err = writer
        .append("Drafts", &[Flag::Draft], &message)
        .await
        .expect_err("over quota");
    assert!(matches!(err, BackendError::OverQuota(_)), "{err:?}");
    assert_eq!(err.code(), mail_model::ErrorCode::OverQuota);
    writer.logout().await.ok();
}
