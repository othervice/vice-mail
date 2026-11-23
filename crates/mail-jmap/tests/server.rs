//! The JMAP adapter against the local test server (Stalwart over HTTPS, decision 0041),
//! cross-checked over IMAP on the same mailboxes: the first sync sees what EXAMINE sees, later
//! changes arrive by push, writes and sending reach the server, a read-only connection never
//! writes.
//!
//! Ignored by default. Take the shared lock, start the server (seeded):
//!
//! ```text
//! scripts/testmail.sh lock jmap && scripts/testmail.sh start
//! cargo test -p mail-jmap --test server -- --include-ignored --test-threads=1
//! scripts/testmail.sh unlock jmap
//! ```
//!
//! Reads alice's seed; writes only messages `<...@jmap.test>` in bob's account, removed before
//! and after.

use std::sync::Arc;
use std::time::Duration;

use mail_imap::ImapConnector;
use mail_imap::test_support::TestEnv;
use mail_jmap::{JmapConnector, JmapSender, MethodLog, TlsConfigFn};
use mail_model::{
    Access, AccountId, BackendError, Envelope, Flag, IdleOutcome, MailBackend, MailConnector,
    MailSender, MailWriter, Security, SendOutcome, StoreMode, UidSet, WriteConnector,
};

const DOMAIN: &str = "jmap.test";

fn tls() -> TlsConfigFn {
    Arc::new(mail_imap::tls::client_config)
}

fn account(n: u8) -> Option<AccountId> {
    Some(
        format!("00000000-0000-4000-8000-0000000000{n:02}")
            .parse()
            .expect("id"),
    )
}

fn run() -> String {
    format!("jm{}", std::process::id())
}

async fn imap(env: &TestEnv, who: &mail_imap::test_support::TestAccount) -> mail_imap::ImapBackend {
    ImapConnector::new()
        .connect(&env.config(who, Security::Tls, Access::ReadWrite))
        .await
        .expect("IMAP session")
}

async fn raw(admin: &mut mail_imap::ImapBackend, line: &str) {
    let reply = admin
        .raw_command(line, |_| false)
        .await
        .unwrap_or_else(|err| panic!("{line}: {err}"));
    assert!(reply.ok, "{line}: {}", reply.text);
}

/// Where a message with this Message-ID lies (over IMAP): (folder, flagged, seen).
async fn places(admin: &mut mail_imap::ImapBackend, id: &str) -> Vec<(String, bool, bool)> {
    let mut found = Vec::new();
    for remote in admin.list_mailboxes().await.expect("list") {
        if !remote.is_selectable() {
            continue;
        }
        let opened = admin.open_mailbox(&remote.name).await.expect("select");
        let uids = admin.search_uids(&opened).await.expect("search");
        if uids.is_empty() {
            continue;
        }
        for header in admin.fetch_headers(&opened, &uids).await.expect("fetch") {
            if header.message_id.as_deref() == Some(id) {
                found.push((remote.name.clone(), header.flags.flagged, header.flags.seen));
            }
        }
    }
    found
}

/// Removes this test's messages (`@jmap.test`) from every folder of the account.
async fn clean(admin: &mut mail_imap::ImapBackend) {
    for remote in admin.list_mailboxes().await.expect("list") {
        if !remote.is_selectable() {
            continue;
        }
        let opened = admin.open_mailbox(&remote.name).await.expect("select");
        let uids = admin.search_uids(&opened).await.expect("search");
        if uids.is_empty() {
            continue;
        }
        let mine: Vec<u32> = admin
            .fetch_headers(&opened, &uids)
            .await
            .expect("fetch")
            .into_iter()
            .filter(|h| {
                h.message_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with(&format!("@{DOMAIN}")))
            })
            .map(|h| h.uid)
            .collect();
        if !mine.is_empty() {
            let set = UidSet::from_uids(mine);
            raw(admin, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")).await;
            raw(admin, &format!("UID EXPUNGE {set}")).await;
        }
    }
}

fn message(run: &str, n: u32, to: &str) -> Vec<u8> {
    format!(
        "From: JMAP Test <alice@vicemail.test>\r\nTo: {to}\r\nSubject: {run} message {n}\r\n\
         Date: Mon, 23 Nov 2026 10:{:02}:00 +0000\r\nMessage-ID: <{run}-{n}@{DOMAIN}>\r\n\r\n\
         Body {n}.\r\n",
        n % 60
    )
    .into_bytes()
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn the_first_sync_sees_what_imap_sees() {
    let env = TestEnv::require();
    let log = MethodLog::new();
    let connector = JmapConnector::new(tls()).with_method_log(log.clone());
    let mut jmap = connector
        .connect_for(&env.jmap_config(&env.alice, Access::ReadOnly), account(1))
        .await
        .expect("JMAP session");
    let caps = jmap.capabilities().clone();
    assert!(
        caps.jmap && caps.idle && caps.condstore && caps.move_,
        "{caps:?}"
    );
    let mut admin = imap(&env, &env.alice).await;

    let mut jmap_names: Vec<String> = jmap
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .map(|m| m.name)
        .collect();
    jmap_names.sort();
    for needed in [
        "INBOX",
        "Archive",
        "Sent",
        "Drafts",
        "Junk",
        "Trash",
        "Projects/2026",
        "Projects/Überprüfung",
    ] {
        assert!(
            jmap_names.iter().any(|n| n == needed),
            "{needed} in {jmap_names:?}"
        );
    }

    for name in ["INBOX", "Archive", "Projects/2026", "Projects/Überprüfung"] {
        let over_jmap = jmap.open_mailbox(name).await.expect("open");
        let remote = admin
            .list_mailboxes()
            .await
            .expect("list")
            .into_iter()
            .find(|m| m.path == name)
            .expect("IMAP folder");
        let over_imap = admin.open_mailbox(&remote.name).await.expect("examine");
        assert_eq!(over_jmap.exists, over_imap.exists, "{name}");
        let jmap_uids = jmap.search_uids(&over_jmap).await.expect("uids");
        let imap_uids = admin.search_uids(&over_imap).await.expect("uids");
        let jmap_headers = jmap
            .fetch_headers(&over_jmap, &jmap_uids)
            .await
            .expect("headers");
        let imap_headers = admin
            .fetch_headers(&over_imap, &imap_uids)
            .await
            .expect("headers");
        assert_eq!(jmap_headers.len(), imap_headers.len(), "{name}");
        // Senders of the seed's deliberately broken messages differ by design: RFC 8621 reads
        // the last of two From fields, IMAP's ENVELOPE the first; Stalwart's IMAP invents
        // `unknown@localhost` for a missing From, its JMAP leaves it empty.
        let key = |h: &mail_model::MessageHeader| {
            let id = h.message_id.clone().unwrap_or_default();
            let broken = id.is_empty() || id.starts_with("broken.") || id.starts_with("no-angle");
            (
                id,
                h.subject.clone(),
                h.flags.seen,
                h.flags.flagged,
                h.from
                    .first()
                    .filter(|_| !broken)
                    .map(|a| a.email.to_lowercase()),
            )
        };
        let mut a: Vec<_> = jmap_headers.iter().map(key).collect();
        let mut b: Vec<_> = imap_headers.iter().map(key).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "{name}: the same messages, subjects and flags");
        // UIDs grow with arrival, like IMAP's.
        let mut by_uid = jmap_headers.clone();
        by_uid.sort_by_key(|h| h.uid);
        assert!(
            by_uid
                .windows(2)
                .all(|w| w[0].internal_date <= w[1].internal_date),
            "{name}: numbered oldest first"
        );
    }

    // A whole message is the same bytes over both protocols.
    let inbox = jmap.open_mailbox("INBOX").await.expect("open");
    let uid = jmap
        .search_uids(&inbox)
        .await
        .expect("uids")
        .max()
        .expect("one");
    let raw_jmap = jmap
        .fetch_messages(&inbox, &UidSet::from_uids([uid]))
        .await
        .expect("body")
        .pop()
        .expect("one");
    let header = jmap
        .fetch_headers(&inbox, &UidSet::from_uids([uid]))
        .await
        .expect("header")
        .pop()
        .expect("one");
    let over_imap = admin.open_mailbox("INBOX").await.expect("examine");
    let all = admin.search_uids(&over_imap).await.expect("uids");
    let imap_uid = admin
        .fetch_headers(&over_imap, &all)
        .await
        .expect("headers")
        .into_iter()
        .find(|h| h.message_id == header.message_id)
        .expect("same message")
        .uid;
    let raw_imap = admin
        .fetch_messages(&over_imap, &UidSet::from_uids([imap_uid]))
        .await
        .expect("body")
        .pop()
        .expect("one");
    assert_eq!(raw_jmap.data, raw_imap.data);

    // A read-only connection only read.
    let methods = log.entries();
    assert!(
        methods.iter().any(|m| m == "Email/query"),
        "positive control"
    );
    for method in &methods {
        assert!(
            [
                "Mailbox/get",
                "Email/get",
                "Email/query",
                "Email/changes",
                "Core/echo",
                "download"
            ]
            .contains(&method.as_str()),
            "{method} on a read-only connection"
        );
    }
    assert!(matches!(
        connector
            .connect_writer(&env.jmap_config(&env.alice, Access::ReadOnly))
            .await,
        Err(BackendError::ReadOnly(_))
    ));
    admin.logout().await.ok();
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn push_changes_writes_and_sending_reach_the_server() {
    let env = TestEnv::require();
    let run = run();
    let mut admin = imap(&env, &env.bob).await;
    clean(&mut admin).await;
    let connector = JmapConnector::new(tls());
    let read_log = MethodLog::new();
    let mut reader = connector
        .clone()
        .with_method_log(read_log.clone())
        .connect_for(&env.jmap_config(&env.bob, Access::ReadOnly), account(2))
        .await
        .expect("reader");
    let mut watcher = connector
        .clone()
        .with_method_log(read_log.clone())
        .connect_for(&env.jmap_config(&env.bob, Access::ReadOnly), account(2))
        .await
        .expect("watcher");
    let inbox = reader.open_mailbox("INBOX").await.expect("open");
    let watched = watcher.open_mailbox("INBOX").await.expect("open");

    // 1. New mail by push: alice sends over JMAP while bob's watcher idles.
    let idle = tokio::spawn(async move {
        let outcome = watcher
            .idle(&watched, Duration::from_secs(60), std::future::pending())
            .await;
        (watcher, outcome)
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let sender = JmapSender::new(tls());
    let alice = env.jmap_config(&env.alice, Access::ReadWrite);
    sender.verify(&alice).await.expect("alice may send");
    let envelope = Envelope {
        from: env.alice.address.clone(),
        recipients: vec![env.bob.address.clone()],
    };
    let sent = sender
        .send(&alice, &envelope, &message(&run, 1, &env.bob.address))
        .await
        .expect("send");
    assert!(matches!(sent, SendOutcome::Accepted(_)), "{sent:?}");
    let (_watcher, outcome) = tokio::time::timeout(Duration::from_secs(30), idle)
        .await
        .expect("push in time")
        .expect("idle task");
    assert_eq!(outcome.expect("idle"), IdleOutcome::Changed);

    let again = reader.open_mailbox("INBOX").await.expect("open");
    assert!(again.uid_next > inbox.uid_next);
    let new = UidSet::range(inbox.uid_next, again.uid_next - 1);
    let headers = reader.fetch_headers(&again, &new).await.expect("headers");
    let arrived = headers
        .iter()
        .find(|h| h.message_id.as_deref() == Some(&format!("{run}-1@{DOMAIN}")))
        .expect("the message arrived");
    assert_eq!(arrived.subject, format!("{run} message 1"));
    let uid = arrived.uid;

    // 2. A change by another client (IMAP) is a changed flag for the next pass.
    let before = again.highest_modseq.expect("modseq");
    let over_imap = admin.open_mailbox("INBOX").await.expect("select");
    let all = admin.search_uids(&over_imap).await.expect("uids");
    let imap_uid = admin
        .fetch_headers(&over_imap, &all)
        .await
        .expect("headers")
        .into_iter()
        .find(|h| h.message_id.as_deref() == Some(&format!("{run}-1@{DOMAIN}")))
        .expect("over IMAP")
        .uid;
    raw(
        &mut admin,
        &format!("UID STORE {imap_uid} +FLAGS.SILENT (\\Flagged)"),
    )
    .await;
    let after = reader.open_mailbox("INBOX").await.expect("open");
    assert!(after.highest_modseq > Some(before));
    let changed = reader
        .fetch_flags(&after, &UidSet::range(1, after.uid_next), Some(before))
        .await
        .expect("changed");
    assert!(
        changed.iter().any(|f| f.uid == uid && f.flags.flagged),
        "{changed:?}"
    );

    // 3. Writes on the writer: flags, a stale change as conflict, move, delete, append.
    let mut writer = connector
        .connect_writer_for(&env.jmap_config(&env.bob, Access::ReadWrite), account(2))
        .await
        .expect("writer");
    let open = writer.open_mailbox("INBOX").await.expect("open");
    let stored = writer
        .store_flags(
            &open,
            &UidSet::from_uids([uid]),
            StoreMode::Add,
            &[Flag::Seen],
            None,
        )
        .await
        .expect("store");
    assert!(stored.updates.iter().any(|u| u.uid == uid && u.flags.seen));
    let id = format!("{run}-1@{DOMAIN}");
    assert_eq!(
        places(&mut admin, &id).await,
        [("INBOX".to_owned(), true, true)]
    );
    let stale = writer
        .store_flags(
            &open,
            &UidSet::from_uids([uid]),
            StoreMode::Remove,
            &[Flag::Flagged],
            Some(before),
        )
        .await
        .expect("store");
    assert_eq!(
        stale.modified,
        UidSet::from_uids([uid]),
        "changed since: a conflict"
    );
    assert_eq!(
        places(&mut admin, &id).await,
        [("INBOX".to_owned(), true, true)]
    );

    let moved = writer
        .move_messages(&open, &UidSet::from_uids([uid]), "Archive")
        .await
        .expect("move");
    let archive_uid = moved
        .copy_uid
        .expect("COPYUID")
        .target_of(uid)
        .expect("pair");
    assert_eq!(
        places(&mut admin, &id).await,
        [("Archive".to_owned(), true, true)]
    );
    let archive = writer.open_mailbox("Archive").await.expect("archive");
    writer
        .expunge_messages(&archive, &UidSet::from_uids([archive_uid]))
        .await
        .expect("expunge");
    assert!(places(&mut admin, &id).await.is_empty(), "deleted");

    let draft = message(&run, 2, &env.alice.address);
    let appended = writer
        .append("Drafts", &[Flag::Seen, Flag::Draft], &draft)
        .await
        .expect("append")
        .expect("APPENDUID");
    let drafts = writer.open_mailbox("Drafts").await.expect("drafts");
    assert_eq!(drafts.uid_validity, appended.uid_validity);
    assert_eq!(
        writer
            .find_message_id(&drafts, &format!("{run}-2@{DOMAIN}"))
            .await
            .expect("find"),
        UidSet::from_uids([appended.uid])
    );
    assert_eq!(
        places(&mut admin, &format!("{run}-2@{DOMAIN}")).await,
        [("Drafts".to_owned(), false, true)]
    );

    // 4. A refused submission leaves nothing behind.
    let refused = sender
        .send(
            &alice,
            &Envelope {
                from: env.alice.address.clone(),
                recipients: vec!["nobody@elsewhere.example".into()],
            },
            &message(&run, 3, "nobody@elsewhere.example"),
        )
        .await
        .expect("answer");
    assert!(matches!(refused, SendOutcome::Rejected(_)), "{refused:?}");
    let mut alice_imap = imap(&env, &env.alice).await;
    assert!(
        places(&mut alice_imap, &format!("{run}-3@{DOMAIN}"))
            .await
            .is_empty()
    );
    clean(&mut alice_imap).await;
    alice_imap.logout().await.ok();

    // The read connections only read.
    for method in read_log.entries() {
        assert!(
            !method.ends_with("/set") && method != "Email/import" && method != "upload",
            "{method} on a read-only connection"
        );
    }
    clean(&mut admin).await;
    admin.logout().await.ok();
}
