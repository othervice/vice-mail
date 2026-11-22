//! Integration tests against the local test server (Stalwart, `scripts/testmail.sh`). Ignored by
//! default; run them with the server started and seeded (see `tests/common/mod.rs`):
//!
//! ```text
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000
//! cargo test -p mail-imap -- --include-ignored
//! scripts/testmail.sh stop
//! ```

mod common;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use common::{connect, create_folder, delete_folder, scratch_name, seed_keys};
use mail_imap::test_support::{Fault, Proxy, TestEnv, send_mail};
use mail_imap::{ImapConnector, WireTap};
use mail_model::{
    Access, BackendError, IdleOutcome, MailBackend, MailConnector, MailboxRole, MessageHeader,
    RoleInput, RoleSource, Security, UidSet, assign_roles,
};

#[tokio::test]
#[ignore = "needs the test server"]
async fn tls_and_starttls_with_the_test_ca() {
    let env = TestEnv::require();
    for security in [Security::Tls, Security::Starttls] {
        let config = env.config(&env.alice, security, Access::ReadOnly);
        let backend = ImapConnector::new()
            .connect(&config)
            .await
            .unwrap_or_else(|err| panic!("{security:?}: {err}"));
        let caps = backend.capabilities();
        assert!(caps.imap4rev1, "{security:?}");
        backend.logout().await.expect("logout");
    }
    // The certificate also names `localhost` (resolves to ::1 first; the server only listens on
    // 127.0.0.1, so this also exercises trying the next address).
    let config = env.config_at(
        &env.alice,
        "localhost",
        env.imaps_port,
        Security::Tls,
        Access::ReadOnly,
    );
    let backend = ImapConnector::new()
        .connect(&config)
        .await
        .expect("localhost");
    backend.logout().await.expect("logout");
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn certificate_and_login_errors_are_typed() {
    let env = TestEnv::require();
    // Without the test CA the system store does not trust the server: TLS and STARTTLS.
    for security in [Security::Tls, Security::Starttls] {
        let mut config = env.config(&env.alice, security, Access::ReadOnly);
        config.extra_root_ca = None;
        let err = ImapConnector::new()
            .connect(&config)
            .await
            .expect_err("untrusted");
        assert!(
            matches!(err, BackendError::CertUntrusted(_)),
            "{security:?}: {err:?}"
        );
        assert_eq!(err.code(), mail_model::ErrorCode::CertUntrusted);
    }
    // Trusted CA, wrong name.
    let config = env.config(&env.alice, Security::Tls, Access::ReadOnly);
    let err = ImapConnector::new()
        .with_tls_server_name("wrong.vicemail.test")
        .connect(&config)
        .await
        .expect_err("name mismatch");
    assert_eq!(err, BackendError::CertNameMismatch);

    // Wrong password, both ports. The text is the server's, never the password.
    for security in [Security::Tls, Security::Starttls] {
        let mut config = env.config(&env.alice, security, Access::ReadOnly);
        config.password = mail_model::Secret::new("definitely-wrong-pw".into());
        let err = ImapConnector::new()
            .connect(&config)
            .await
            .expect_err("wrong password");
        assert!(
            matches!(err, BackendError::AuthFailed(_)),
            "{security:?}: {err:?}"
        );
        assert!(!format!("{err} {err:?}").contains("definitely-wrong-pw"));
        assert!(!err.is_transient());
    }

    // Nothing listens: refused. Unknown host: DNS.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = closed.local_addr().expect("addr").port();
    drop(closed);
    let config = env.config_at(
        &env.alice,
        "127.0.0.1",
        port,
        Security::Tls,
        Access::ReadOnly,
    );
    let err = ImapConnector::new()
        .connect(&config)
        .await
        .expect_err("closed port");
    assert_eq!(err, BackendError::ConnectionRefused);
    let config = env.config_at(
        &env.alice,
        "no-such-host.invalid",
        993,
        Security::Tls,
        Access::ReadOnly,
    );
    let err = ImapConnector::new()
        .connect(&config)
        .await
        .expect_err("dns");
    assert!(matches!(err, BackendError::Dns(_)), "{err:?}");

    // STARTTLS requested on the implicit-TLS port: the greeting never comes in plain text,
    // the TLS port waits for a ClientHello. A short read limit makes this quick.
    let mut config = env.config_at(
        &env.alice,
        &env.host,
        env.imaps_port,
        Security::Starttls,
        Access::ReadOnly,
    );
    config.timeouts.read = Duration::from_millis(500);
    let err = ImapConnector::new()
        .connect(&config)
        .await
        .expect_err("no plaintext greeting");
    assert!(
        matches!(
            err,
            BackendError::Timeout(_) | BackendError::ConnectionLost(_) | BackendError::Protocol(_)
        ),
        "{err:?}"
    );
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn capabilities_and_enable() {
    let env = TestEnv::require();
    let backend = connect(&env, Access::ReadOnly).await;
    let caps = backend.capabilities().clone();
    assert!(caps.imap4rev1 && caps.idle && caps.condstore && caps.qresync && caps.enable);
    assert!(caps.move_ && caps.uidplus && caps.special_use && caps.list_status);
    assert!(caps.literal_plus && caps.esearch);
    assert!(caps.has("LIST-EXTENDED") && caps.has("namespace"));
    assert!(caps.names.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(backend.access(), Access::ReadOnly);
    let enabled = backend.enabled().to_vec();
    assert!(
        enabled.contains(&"CONDSTORE".to_owned()) && enabled.contains(&"QRESYNC".to_owned()),
        "{enabled:?}"
    );
    backend.logout().await.expect("logout");
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn folder_list_with_roles_counters_and_decoded_names() {
    let env = TestEnv::require();
    let mut backend = connect(&env, Access::ReadOnly).await;
    let list: Vec<_> = backend
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .filter(|m| !m.name.starts_with("VmTest"))
        .collect();
    let by_path: HashMap<&str, &mail_model::RemoteMailbox> =
        list.iter().map(|m| (m.path.as_str(), m)).collect();
    for path in [
        "INBOX",
        "Sent",
        "Drafts",
        "Archive",
        "Junk",
        "Trash",
        "Projects",
        "Projects/2026",
        "Projects/Überprüfung",
        "Projects/Empty",
    ] {
        assert!(
            by_path.contains_key(path),
            "{path} missing in {:?}",
            by_path.keys()
        );
    }
    let review = by_path["Projects/Überprüfung"];
    assert_eq!(review.name, "Projects/&ANw-berpr&APw-fung");
    assert_eq!(review.delimiter, Some('/'));
    assert_eq!(review.leaf_name(), "Überprüfung");
    assert_eq!(review.parent_path(), Some("Projects"));
    for (path, attr) in [
        ("Sent", "\\Sent"),
        ("Drafts", "\\Drafts"),
        ("Trash", "\\Trash"),
        ("Junk", "\\Junk"),
        ("Archive", "\\Archive"),
    ] {
        assert!(
            by_path[path].has_attribute(attr),
            "{path}: {:?}",
            by_path[path].attributes
        );
    }
    // Counters came with LIST-STATUS, for every selectable folder.
    let seed_counts = [
        ("Archive", 97),
        ("Drafts", 21),
        ("Junk", 17),
        ("Projects/2026", 59),
        ("Projects/Empty", 0),
        ("Projects/Überprüfung", 7),
        ("Sent", 127),
        ("Trash", 15),
    ];
    for (path, count) in seed_counts {
        let status = by_path[path]
            .status
            .unwrap_or_else(|| panic!("{path}: no STATUS"));
        assert_eq!(status.messages, count, "{path}");
        assert!(
            status.uid_validity.is_some() && status.uid_next.is_some(),
            "{path}"
        );
        assert!(status.highest_modseq.is_some(), "{path}");
    }
    let inbox = by_path["INBOX"].status.expect("INBOX status");
    assert!(inbox.messages >= 657 && inbox.unseen.is_some());

    // Role detection on the adapter's raw data.
    let inputs: Vec<RoleInput<'_>> = list
        .iter()
        .map(|m| RoleInput {
            path: &m.path,
            delimiter: m.delimiter,
            attributes: &m.attributes,
            user_role: None,
        })
        .collect();
    let roles = assign_roles(&inputs);
    let role_of = |path: &str| {
        list.iter()
            .position(|m| m.path == path)
            .and_then(|i| roles.get(i))
            .copied()
            .unwrap_or_else(|| panic!("{path}"))
    };
    for (path, role) in [
        ("INBOX", MailboxRole::Inbox),
        ("Sent", MailboxRole::Sent),
        ("Drafts", MailboxRole::Drafts),
        ("Trash", MailboxRole::Trash),
        ("Junk", MailboxRole::Junk),
        ("Archive", MailboxRole::Archive),
    ] {
        let got = role_of(path);
        assert_eq!((got.role, got.source), (role, RoleSource::Server), "{path}");
    }
    assert_eq!(role_of("Projects/Überprüfung").role, MailboxRole::Custom);
    backend.logout().await.expect("logout");
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn examine_reports_uidvalidity_uidnext_and_highestmodseq() {
    let env = TestEnv::require();
    let tap = WireTap::new();
    let config = env.config(&env.alice, Security::Tls, Access::ReadOnly);
    let mut backend = ImapConnector::new()
        .with_wire_tap(tap.clone())
        .connect(&config)
        .await
        .expect("connect");
    let status = backend
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .find(|m| m.path == "Archive")
        .and_then(|m| m.status)
        .expect("Archive status");
    let archive = backend.open_mailbox("Archive").await.expect("examine");
    assert_eq!(Some(archive.uid_validity), status.uid_validity);
    assert_eq!(Some(archive.uid_next), status.uid_next);
    assert_eq!(archive.exists, 97);
    // Stalwart counts HIGHESTMODSEQ per account: changes in other folders (other tests) raise it.
    assert!(archive.highest_modseq >= status.highest_modseq);
    assert!(archive.highest_modseq.is_some() && archive.read_only);
    // Modified UTF-7 name goes out as sent by the server.
    let review = backend
        .open_mailbox("Projects/&ANw-berpr&APw-fung")
        .await
        .expect("examine non-ASCII");
    assert_eq!(review.exists, 7);
    assert_eq!(
        backend.open_mailbox("Does/Not/Exist").await,
        Err(BackendError::MailboxNotFound("Does/Not/Exist".into()))
    );
    backend.noop().await.expect("usable after NO");
    let verbs = tap.verbs();
    assert!(
        verbs.contains(&"EXAMINE".to_owned()) && !verbs.contains(&"SELECT".to_owned()),
        "{verbs:?}"
    );
    backend.logout().await.expect("logout");
}

fn expected_message_id(key: &str) -> String {
    format!("{}@seed.vicemail.test", key.replace('/', "."))
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn headers_in_windows_newest_first_including_broken_mails() {
    let env = TestEnv::require();
    let mut backend = connect(&env, Access::ReadOnly).await;
    let folders: Vec<_> = backend
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .filter(|m| m.is_selectable() && !m.name.starts_with("VmTest"))
        .collect();
    let mut all: Vec<(String, String, MessageHeader)> = Vec::new(); // (folder, seed key, header)
    let started = Instant::now();
    for folder in &folders {
        let mailbox = backend.open_mailbox(&folder.name).await.expect("examine");
        let keys = seed_keys(&mut backend).await;
        let uids = backend.search_uids(&mailbox).await.expect("search");
        if folder.path != "INBOX" {
            assert_eq!(uids.len(), u64::from(mailbox.exists), "{}", folder.path);
        }
        // First window 100 so the list fills at once, then 500; newest first.
        let mut remaining = uids.clone();
        let mut size = 100;
        let mut previous_low = u32::MAX;
        while !remaining.is_empty() {
            let (window, rest) = remaining.split_highest(size);
            let headers = backend
                .fetch_headers(&mailbox, &window)
                .await
                .expect("fetch");
            let got = UidSet::from_uids(headers.iter().map(|h| h.uid));
            assert_eq!(
                got, window,
                "{}: every UID of the window came back",
                folder.path
            );
            let high = window.max().unwrap_or_default();
            assert!(high < previous_low, "windows go from new to old");
            previous_low = window.min().unwrap_or_default();
            for header in headers {
                let key = keys.get(&header.uid).cloned().unwrap_or_default();
                all.push((folder.path.clone(), key, header));
            }
            remaining = rest;
            size = 500;
        }
    }
    let elapsed = started.elapsed();
    assert!(all.len() >= 1000, "{} headers", all.len());
    println!(
        "{} headers from {} folders in {elapsed:?}",
        all.len(),
        folders.len()
    );

    let by_key: HashMap<&str, &MessageHeader> =
        all.iter().map(|(_, key, h)| (key.as_str(), h)).collect();
    // Every message is complete enough to show.
    for (folder, key, h) in &all {
        assert!(h.uid > 0 && h.size > 0, "{folder} {key}");
        assert!(
            h.internal_date > 1_600_000_000,
            "{folder} {key}: {}",
            h.internal_date
        );
        assert!(h.modseq.is_some(), "{folder} {key}");
        // Only the deliberately broken encoded words may stay as they are.
        assert!(
            !h.subject.contains("=?") || key == "broken/bad-encoded-words",
            "{folder} {key}: undecoded {:?}",
            h.subject
        );
        if !key.starts_with("broken/") && !key.is_empty() {
            assert_eq!(
                h.message_id.as_deref(),
                Some(expected_message_id(key).as_str()),
                "{key}"
            );
        }
        if folder == "Drafts" {
            assert!(h.flags.draft, "{key}");
        }
    }
    // All 26 broken messages arrived and were parsed.
    let broken: Vec<&str> = by_key
        .keys()
        .copied()
        .filter(|k| k.starts_with("broken/"))
        .collect();
    assert_eq!(broken.len(), 26, "{broken:?}");
    let get = |key: &str| *by_key.get(key).unwrap_or_else(|| panic!("{key} missing"));
    assert_eq!(get("broken/bad-date").date, None);
    assert_eq!(get("broken/missing-date").date, None);
    assert!(get("broken/missing-headers").message_id.is_none());
    assert_eq!(
        get("broken/bad-addresses").message_id.as_deref(),
        Some("no-angle-brackets@seed")
    );
    let loop_refs = &get("broken/reference-loop").references;
    assert!(
        loop_refs.contains(&expected_message_id("broken/reference-loop")),
        "{loop_refs:?}"
    );
    // Valid words around the broken ones are decoded; the split emoji is joined.
    let words = &get("broken/bad-encoded-words").subject;
    assert!(
        words.contains("foo") && words.contains('\u{1f47b}'),
        "{words:?}"
    );
    assert!(
        get("broken/8bit-headers")
            .subject
            .contains("rger mit der Stra")
    );
    assert!(!get("broken/deep-nesting").has_attachments);
    assert!(get("broken/truncated-base64").has_attachments);
    assert!(get("broken/charset-in-filename").has_attachments);
    assert_eq!(
        get("broken/duplicate-message-id").message_id.as_deref(),
        Some(expected_message_id("thread/offsite-03").as_str())
    );

    // List and auto-reply headers.
    let rust = get("list/rust-users");
    assert_eq!(
        rust.list_id.as_deref(),
        Some("Rust users discussion <rust-users.lists.example>")
    );
    assert_eq!(
        rust.list_unsubscribe.as_deref(),
        Some("<mailto:rust-users-leave@lists.example>")
    );
    assert_eq!(rust.precedence.as_deref(), Some("list"));
    let ooo = get("auto/out-of-office");
    assert_eq!(ooo.auto_submitted.as_deref(), Some("auto-replied"));
    assert_eq!(ooo.precedence.as_deref(), Some("auto_reply"));
    assert_eq!(ooo.subject, "Abwesenheitsnotiz: Projekt Phönix");
    assert_eq!(
        ooo.from.first().and_then(|a| a.name.as_deref()),
        Some("Jürgen Groß")
    );
    assert_eq!(
        get("auto/ci-failed").auto_submitted.as_deref(),
        Some("auto-generated")
    );
    assert!(get("list/junk-precedence").list_id.is_none());

    // Thread headers (offsite-12 has no In-Reply-To, offsite-22 no References).
    let first = get("thread/offsite-00");
    assert!(first.in_reply_to.is_none() && first.references.is_empty());
    let second = get("thread/offsite-01");
    assert_eq!(
        second.in_reply_to.as_deref(),
        Some(expected_message_id("thread/offsite-00").as_str())
    );
    assert_eq!(
        second.references,
        [expected_message_id("thread/offsite-00")]
    );
    assert!(
        get("thread/offsite-12").in_reply_to.is_none()
            && !get("thread/offsite-12").references.is_empty()
    );
    assert!(
        get("thread/offsite-22").in_reply_to.is_some()
            && get("thread/offsite-22").references.is_empty()
    );

    // Attachments and charsets.
    assert!(get("mime/mixed-attachments").has_attachments);
    assert!(get("mime/many-attachments").has_attachments);
    assert!(!get("mime/alternative").has_attachments);
    let charset_subjects: Vec<&str> = by_key
        .iter()
        .filter(|(k, _)| k.starts_with("charset/"))
        .map(|(_, h)| h.subject.as_str())
        .collect();
    assert!(charset_subjects.len() >= 10, "{charset_subjects:?}");
    assert!(
        charset_subjects
            .iter()
            .any(|s| s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)
                || ('\u{4e00}'..='\u{9fff}').contains(&c))),
        "{charset_subjects:?}"
    );
    assert!(
        charset_subjects
            .iter()
            .any(|s| s.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c))),
        "{charset_subjects:?}"
    );
    backend.logout().await.expect("logout");
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn changedsince_returns_only_messages_changed_by_another_session() {
    let env = TestEnv::require();
    let folder = scratch_name("flags");
    let mut writer = connect(&env, Access::ReadWrite).await;
    create_folder(&mut writer, &folder, 3).await;

    let mut reader = connect(&env, Access::ReadOnly).await;
    let mailbox = reader.open_mailbox(&folder).await.expect("examine");
    let since = mailbox.highest_modseq.expect("CONDSTORE");
    let all = UidSet::range(1, mailbox.uid_next - 1);
    let before = reader
        .fetch_flags(&mailbox, &all, None)
        .await
        .expect("all flags");
    assert_eq!(before.len(), 3);
    assert!(
        before
            .iter()
            .all(|u| u.flags.seen && !u.flags.flagged && u.modseq.is_some())
    );
    assert!(
        reader
            .fetch_flags(&mailbox, &all, Some(since))
            .await
            .expect("none")
            .is_empty()
    );

    // The second session flags the middle message.
    let middle = before[1].uid;
    writer.open_mailbox(&folder).await.expect("select");
    let reply = writer
        .raw_command(
            &format!("UID STORE {middle} +FLAGS (\\Flagged $Vmtest)"),
            |_| false,
        )
        .await
        .expect("store");
    assert!(reply.ok, "{}", reply.text);

    let changed = reader
        .fetch_flags(&mailbox, &all, Some(since))
        .await
        .expect("changed");
    assert_eq!(changed.len(), 1, "{changed:?}");
    let update = &changed[0];
    assert_eq!(update.uid, middle);
    assert!(update.flags.flagged && update.flags.seen && update.flags.has_keyword("$VMTEST"));
    assert!(update.modseq.is_some_and(|m| m > since));

    reader.logout().await.expect("logout");
    delete_folder(&mut writer, &folder).await;
    writer.logout().await.expect("logout");
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn uid_search_after_deletion_from_outside() {
    let env = TestEnv::require();
    let folder = scratch_name("delete");
    let mut writer = connect(&env, Access::ReadWrite).await;
    create_folder(&mut writer, &folder, 3).await;

    let mut reader = connect(&env, Access::ReadOnly).await;
    let mailbox = reader.open_mailbox(&folder).await.expect("examine");
    let uids = reader.search_uids(&mailbox).await.expect("search");
    assert_eq!(uids.len(), 3);
    let gone = uids.iter().nth(1).expect("three UIDs");

    writer.open_mailbox(&folder).await.expect("select");
    for line in [
        format!("UID STORE {gone} +FLAGS.SILENT (\\Deleted)"),
        format!("UID EXPUNGE {gone}"),
    ] {
        let reply = writer.raw_command(&line, |_| false).await.expect("delete");
        assert!(reply.ok, "{line}: {}", reply.text);
    }

    // The search index of the server may lag a little behind (decision 0001).
    let deadline = Instant::now() + Duration::from_secs(5);
    let now = loop {
        let now = reader.search_uids(&mailbox).await.expect("search");
        if now.len() == 2 || Instant::now() > deadline {
            break now;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(!now.contains(gone), "{now}");
    assert_eq!(uids.difference(&now), UidSet::from_uids([gone]));
    // Fetching the deleted UID returns nothing instead of failing.
    let headers = reader
        .fetch_headers(&mailbox, &UidSet::from_uids([gone]))
        .await
        .expect("fetch");
    assert!(headers.is_empty());

    reader.logout().await.expect("logout");
    delete_folder(&mut writer, &folder).await;
    writer.logout().await.expect("logout");
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn idle_reports_new_mail_ends_at_the_limit_and_on_stop() {
    let env = TestEnv::require();
    let mut backend = connect(&env, Access::ReadOnly).await;
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");

    // New mail from bob through SMTP submission (1587, STARTTLS, AUTH) while idling.
    let subject = format!("idle test {}", std::process::id());
    let sender_env = env.clone();
    let sender_subject = subject.clone();
    let sender = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let sent = Instant::now();
        let to = sender_env.alice.address.clone();
        send_mail(
            &sender_env,
            &sender_env.bob,
            &to,
            &sender_subject,
            "Hello from the IDLE test.",
        )
        .await
        .map(|()| sent)
    });
    let start = Instant::now();
    let outcome = backend
        .idle(&inbox, Duration::from_secs(30), std::future::pending())
        .await
        .expect("idle");
    let woke = Instant::now();
    let sent = sender.await.expect("join").expect("smtp");
    assert_eq!(outcome, IdleOutcome::Changed);
    assert!(woke.duration_since(start) < Duration::from_secs(10));
    println!(
        "new mail reported {:?} after the SMTP send started",
        woke.saturating_duration_since(sent)
    );

    // The new message is there and readable on the same connection.
    let reopened = backend.open_mailbox("INBOX").await.expect("examine");
    assert!(reopened.exists > inbox.exists);
    let new = UidSet::range(inbox.uid_next, reopened.uid_next - 1);
    let headers = backend
        .fetch_headers(&reopened, &new)
        .await
        .expect("fetch new");
    assert!(headers.iter().any(|h| h.subject == subject), "{headers:?}");

    // Time limit.
    let start = Instant::now();
    let outcome = backend
        .idle(&reopened, Duration::from_secs(1), std::future::pending())
        .await
        .expect("idle");
    assert_eq!(outcome, IdleOutcome::TimedOut);
    let waited = start.elapsed();
    assert!(
        waited >= Duration::from_millis(950) && waited < Duration::from_secs(4),
        "{waited:?}"
    );

    // Stop signal from outside.
    let start = Instant::now();
    let outcome = backend
        .idle(
            &reopened,
            Duration::from_secs(30),
            tokio::time::sleep(Duration::from_millis(200)),
        )
        .await
        .expect("idle");
    assert_eq!(outcome, IdleOutcome::Stopped);
    assert!(start.elapsed() < Duration::from_secs(3));
    backend.noop().await.expect("usable after IDLE");
    backend.logout().await.expect("logout");
}

async fn proxy(env: &TestEnv) -> std::sync::Arc<Proxy> {
    let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
        .parse()
        .expect("addr");
    Proxy::start(upstream).await.expect("proxy")
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn cut_or_silent_connection_mid_fetch_is_a_typed_error_without_hanging() {
    let env = TestEnv::require();
    let proxy = proxy(&env).await;
    let config = env.config_at(
        &env.alice,
        "127.0.0.1",
        proxy.port(),
        Security::Tls,
        Access::ReadOnly,
    );

    // Measure how many bytes connecting and EXAMINE take, then cut 50 KB into the FETCH.
    let mut probe = ImapConnector::new().connect(&config).await.expect("probe");
    probe.open_mailbox("INBOX").await.expect("examine");
    let setup = proxy.bytes_to_client();
    probe.logout().await.expect("logout");

    proxy.arm(Fault::CloseAfter(setup + 50_000)).await;
    let mut backend = ImapConnector::new()
        .connect(&config)
        .await
        .expect("connect");
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        backend.fetch_headers(&inbox, &UidSet::range(1, inbox.uid_next - 1)),
    )
    .await
    .expect("no hang");
    let err = result.expect_err("cut");
    assert!(matches!(err, BackendError::ConnectionLost(_)), "{err:?}");
    assert!(err.is_transient());
    println!("cut noticed after {:?}: {err}", start.elapsed());
    assert!(proxy.fired_at().await.is_some());
    // The connection is not used again.
    assert!(matches!(
        backend.noop().await,
        Err(BackendError::ConnectionLost(_))
    ));

    // Silent link: the read limit turns it into a timeout.
    proxy.arm(Fault::StallAfter(setup + 50_000)).await;
    let mut config = config.clone();
    config.timeouts.read = Duration::from_secs(2);
    let mut backend = ImapConnector::new()
        .connect(&config)
        .await
        .expect("connect");
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        backend.fetch_headers(&inbox, &UidSet::range(1, inbox.uid_next - 1)),
    )
    .await
    .expect("no hang");
    let err = result.expect_err("stalled");
    let waited = start.elapsed();
    assert!(matches!(err, BackendError::Timeout(_)), "{err:?}");
    assert!(
        waited >= Duration::from_secs(2) && waited < Duration::from_secs(6),
        "{waited:?}"
    );
    proxy.release_stalled();

    // A refused connection attempt is typed as well.
    proxy.refuse_next(1);
    let err = ImapConnector::new()
        .connect(&config)
        .await
        .expect_err("refused");
    assert!(matches!(err, BackendError::ConnectionLost(_)), "{err:?}");
    assert!(err.is_transient());
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn recreated_folder_has_a_new_uidvalidity_and_stale_handles_are_refused() {
    let env = TestEnv::require();
    let folder = scratch_name("uidvalidity");
    let mut writer = connect(&env, Access::ReadWrite).await;
    create_folder(&mut writer, &folder, 2).await;

    let mut reader = connect(&env, Access::ReadOnly).await;
    let old = reader.open_mailbox(&folder).await.expect("examine");
    assert_eq!(old.exists, 2);

    // Delete and create again under the same name (how UIDVALIDITY changes on this server).
    delete_folder(&mut writer, &folder).await;
    create_folder(&mut writer, &folder, 1).await;

    let new = reader.open_mailbox(&folder).await.expect("examine again");
    assert_ne!(new.uid_validity, old.uid_validity);
    assert_eq!(new.exists, 1);
    let status = reader
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .find(|m| m.name == folder)
        .and_then(|m| m.status)
        .expect("status");
    assert_eq!(status.uid_validity, Some(new.uid_validity));
    // UIDs of the old UIDVALIDITY are never applied to the new mailbox.
    let err = reader
        .fetch_headers(&old, &UidSet::range(1, 2))
        .await
        .expect_err("stale handle");
    assert!(matches!(err, BackendError::InvalidState(_)), "{err:?}");
    assert_eq!(
        reader
            .fetch_headers(&new, &UidSet::all())
            .await
            .expect("fetch")
            .len(),
        1
    );

    reader.logout().await.expect("logout");
    delete_folder(&mut writer, &folder).await;
    writer.logout().await.expect("logout");
}
