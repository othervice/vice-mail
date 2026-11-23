//! Stage 11 (decision 0014): the settings sync connection writes to its folder and nowhere
//! else, also for an account without write access.
//!
//! A wire tap inside TLS records what the adapter wrote. Every write outside the folder must
//! fail with `read_only` and add nothing to the tap; the folder's own writes are the positive
//! control. Works in bob's account with a scratch folder name (the engine tests use alice's
//! real `Settings-Sync`). Needs the test server (see `tests/common/mod.rs`).

mod common;

use std::time::Duration;

use mail_imap::test_support::TestEnv;
use mail_imap::{ImapConnector, WireTap};
use mail_model::{
    Access, BackendError, MailConnector, Security, SyncFolder, SyncFolderConnector, UidSet,
};

fn record(n: u32) -> String {
    format!(
        "Date: Mon, 23 Nov 2026 10:00:00 +0000\r\nSubject: Settings sync data\r\n\
         X-Settings-Sync-Id: {n:032x}\r\nX-Settings-Sync-Version: v{n}\r\n\
         Content-Type: text/plain; charset=us-ascii\r\n\r\nbody {n}\r\n"
    )
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn sync_folder_connection_writes_only_to_its_folder() {
    let env = TestEnv::require();
    let folder = common::scratch_name("SyncFolder");
    let mut admin = ImapConnector::new()
        .connect(&env.config(&env.bob, Security::Tls, Access::ReadWrite))
        .await
        .expect("admin");
    admin
        .raw_command(&format!("DELETE \"{folder}\""), |_| false)
        .await
        .ok();

    let tap = WireTap::new();
    // The account's own configuration: read-only, as for `writes_enabled = false`.
    let config = env.config(&env.bob, Security::Tls, Access::ReadOnly);
    let mut sync = ImapConnector::new()
        .with_wire_tap(tap.clone())
        .connect_sync_folder(&config, &folder)
        .await
        .expect("connect");

    assert_eq!(
        sync.open(false).await.expect("open"),
        None,
        "not created yet"
    );
    let state = sync
        .open(true)
        .await
        .expect("create and select")
        .expect("state");
    assert_eq!(state.exists, 0);

    // Created, but not subscribed.
    let mut subscribed = Vec::new();
    admin
        .raw_command("LIST (SUBSCRIBED) \"\" \"*\"", |resp| {
            subscribed.push(format!("{resp:?}"));
            true
        })
        .await
        .expect("list subscribed");
    assert!(
        subscribed.iter().all(|line| !line.contains(&folder)),
        "the folder is subscribed: {subscribed:?}"
    );

    // Appending, listing with the two header fields, reading, deleting.
    let first = sync
        .append(record(1).as_bytes())
        .await
        .expect("append")
        .expect("APPENDUID");
    let second = sync
        .append(record(2).as_bytes())
        .await
        .expect("append")
        .expect("APPENDUID");
    sync.open(false).await.expect("select again");
    let fields = ["X-Settings-Sync-Id", "X-Settings-Sync-Version"];
    let listed = sync.list(&fields).await.expect("list");
    assert_eq!(
        listed.iter().map(|e| e.uid).collect::<Vec<_>>(),
        [first, second]
    );
    let header = String::from_utf8_lossy(&listed[1].header).into_owned();
    assert!(header.contains("X-Settings-Sync-Version: v2"), "{header}");
    assert!(
        !header.contains("Subject"),
        "only the asked fields: {header}"
    );
    let bodies = sync.read(&UidSet::from_uids([second])).await.expect("read");
    assert_eq!(bodies.len(), 1);
    assert!(String::from_utf8_lossy(&bodies[0].1).starts_with("body 2"));
    sync.delete(&UidSet::from_uids([first]))
        .await
        .expect("delete");
    let left = sync.list(&fields).await.expect("list");
    assert_eq!(left.iter().map(|e| e.uid).collect::<Vec<_>>(), [second]);

    // Writes anywhere else fail before they are sent, even in this session.
    let writes = [
        "APPEND \"INBOX\" (\\Seen) {12+}\r\nSubject: x\r\n",
        "SELECT \"INBOX\"",
        "CREATE \"VmTest-must-not-exist\"",
        "DELETE \"INBOX\"",
        "SUBSCRIBE \"INBOX\"",
        "UID COPY 1 \"INBOX\"",
        "UID MOVE 1 \"INBOX\"",
        "UID STORE 1 +FLAGS (\\Flagged)",
        "UID FETCH 1 BODY[]",
    ];
    let control = tap.len();
    for line in writes {
        let before = tap.len();
        let result = sync.backend_mut().raw_command(line, |_| false).await;
        assert!(
            matches!(result, Err(BackendError::ReadOnly(_))),
            "{line:?}: {result:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(tap.len(), before, "{line:?} reached the wire");
    }
    // Once another mailbox is open (read-only), deleting is refused too: the UIDs would be
    // that mailbox's.
    sync.backend_mut()
        .raw_command("EXAMINE \"INBOX\"", |_| false)
        .await
        .expect("examine");
    for line in [
        "UID STORE 1 +FLAGS.SILENT (\\Deleted)",
        "UID EXPUNGE 1",
        "EXPUNGE",
    ] {
        let before = tap.len();
        let result = sync.backend_mut().raw_command(line, |_| false).await;
        assert!(
            matches!(result, Err(BackendError::ReadOnly(_))),
            "{line:?}: {result:?}"
        );
        assert_eq!(tap.len(), before, "{line:?} reached the wire");
    }
    assert!(tap.len() > control, "the tap records (EXAMINE)");
    // Commands only (tagged lines; the appended messages' lines are literal data).
    let tagged = |line: &&String| {
        line.split(' ').next().is_some_and(|tag| {
            tag.len() > 1 && tag.starts_with('A') && tag[1..].bytes().all(|b| b.is_ascii_digit())
        })
    };
    let lines: Vec<String> = tap.lines().into_iter().filter(|l| tagged(&l)).collect();
    let verbs: Vec<String> = lines
        .iter()
        .map(|l| {
            let mut words = l.split(' ').skip(1);
            let first = words.next().unwrap_or_default().to_ascii_uppercase();
            if first == "UID" {
                format!(
                    "UID {}",
                    words.next().unwrap_or_default().to_ascii_uppercase()
                )
            } else {
                first
            }
        })
        .collect();
    let used = [
        "LOGIN",
        "ENABLE",
        "SELECT",
        "CREATE",
        "UNSUBSCRIBE",
        "APPEND",
        "UID SEARCH",
        "UID FETCH",
        "UID STORE",
        "UID EXPUNGE",
        "EXAMINE",
    ];
    for verb in &verbs {
        assert!(used.contains(&verb.as_str()), "{verb} in {verbs:?}");
    }
    for line in &lines {
        if line.contains("APPEND") || line.contains("SELECT") || line.contains("CREATE") {
            assert!(line.contains(&folder), "write outside the folder: {line}");
        }
    }
    sync.logout().await.ok();
    admin
        .raw_command(&format!("DELETE \"{folder}\""), |_| false)
        .await
        .expect("cleanup");
}
