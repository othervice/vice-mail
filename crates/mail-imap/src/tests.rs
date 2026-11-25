//! Protocol tests of the backend against a scripted fake server over an in-memory pipe. The fake
//! server records every line the adapter wrote, so these tests also prove what did not leave it.

use std::time::Duration;

use async_imap::Client;
use mail_model::{
    Access, BackendError, BodyFetchPlan, BodySection, ConnectConfig, IdleOutcome,
    LARGE_MESSAGE_THRESHOLD, MailBackend, ModSeq, Secret, Security, SelectedMailbox,
    ServerSettings, UidSet, UidValidity,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::task::JoinHandle;

use crate::backend::ImapBackend;
use crate::command::looks_like_tag;
use crate::connector::read_greeting;
use crate::timeout_io::{IoTimeout, TimeoutHandle};

enum Step {
    /// Write this text; `{tag}` becomes the tag of the last tagged command received.
    Send(&'static str),
    /// Read one line and check that it (without tag) starts with this text.
    Expect(&'static str),
    Pause(Duration),
    /// Close the connection.
    Close,
}

use Step::{Close, Expect, Pause, Send};

const GREETING: Step = Send("* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] fake ready\r\n");
const LOGIN: Step = Expect("LOGIN \"alice\" \"pw\"");
const LOGIN_OK: Step = Send(
    "{tag} OK [CAPABILITY IMAP4rev1 IDLE CONDSTORE ENABLE SPECIAL-USE LIST-STATUS LIST-EXTENDED] logged in\r\n",
);
const ENABLE: Step = Expect("ENABLE CONDSTORE");
const ENABLE_OK: Step = Send("* ENABLED CONDSTORE\r\n{tag} OK enabled\r\n");
const EXAMINE: Step = Expect("EXAMINE \"INBOX\" (CONDSTORE)");
const EXAMINE_OK: Step = Send(
    "* 3 EXISTS\r\n* 0 RECENT\r\n* FLAGS (\\Seen \\Flagged)\r\n* OK [PERMANENTFLAGS ()] none\r\n\
* OK [UIDVALIDITY 77] ok\r\n* OK [UIDNEXT 40] ok\r\n* OK [HIGHESTMODSEQ 900] ok\r\n\
{tag} OK [READ-ONLY] EXAMINE completed\r\n",
);

async fn next_line<R: AsyncBufRead + Unpin>(lines: &mut R) -> Option<String> {
    let mut line = String::new();
    match lines.read_line(&mut line).await {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end().to_owned()),
    }
}

/// Runs `script` as the server; returns every line the client wrote, also after the script
/// ended, until the client closes.
async fn fake_server(stream: DuplexStream, script: Vec<Step>) -> Vec<String> {
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read);
    let mut received = Vec::new();
    let mut tag = String::new();
    for step in script {
        match step {
            Send(text) => {
                if write
                    .write_all(text.replace("{tag}", &tag).as_bytes())
                    .await
                    .is_err()
                {
                    return received;
                }
            }
            Expect(want) => {
                let Some(line) = next_line(&mut lines).await else {
                    panic!("client closed, expected {want:?}; received {received:?}");
                };
                let rest = match line.split_once(' ') {
                    Some((first, rest)) if looks_like_tag(first) => {
                        tag = first.to_owned();
                        rest.to_owned()
                    }
                    _ => line.clone(),
                };
                assert!(rest.starts_with(want), "expected {want:?}, got {line:?}");
                received.push(line);
            }
            Pause(d) => tokio::time::sleep(d).await,
            Close => return received,
        }
    }
    while let Some(line) = next_line(&mut lines).await {
        received.push(line);
    }
    received
}

fn config(access: Access) -> ConnectConfig {
    let server = ServerSettings {
        host: "fake.test".into(),
        port: 993,
        security: Security::Tls,
        username: "alice".into(),
    };
    ConnectConfig::new(&server, Secret::new("pw".into()), access)
}

type Fake = (
    Result<ImapBackend<DuplexStream>, BackendError>,
    JoinHandle<Vec<String>>,
);

async fn connect(access: Access, read_limit: Duration, script: Vec<Step>) -> Fake {
    let (client, server) = tokio::io::duplex(1 << 16);
    let handle = tokio::spawn(fake_server(server, script));
    let timeouts = TimeoutHandle::new(read_limit, Duration::from_secs(2));
    let mut client = Client::new(IoTimeout::new(client, timeouts.clone()));
    let backend = match read_greeting(&mut client).await {
        Ok(caps) => {
            ImapBackend::establish(client, timeouts, &config(access), caps.as_deref()).await
        }
        Err(err) => Err(err),
    };
    (backend, handle)
}

/// Login, ENABLE and EXAMINE INBOX, then `rest`.
async fn opened(
    access: Access,
    rest: Vec<Step>,
) -> (
    ImapBackend<DuplexStream>,
    SelectedMailbox,
    JoinHandle<Vec<String>>,
) {
    let mut script = vec![
        GREETING, LOGIN, LOGIN_OK, ENABLE, ENABLE_OK, EXAMINE, EXAMINE_OK,
    ];
    script.extend(rest);
    let (backend, handle) = connect(access, Duration::from_secs(2), script).await;
    let mut backend = backend.expect("login");
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    (backend, inbox, handle)
}

async fn finish(
    backend: ImapBackend<DuplexStream>,
    handle: JoinHandle<Vec<String>>,
) -> Vec<String> {
    drop(backend);
    handle.await.expect("fake server")
}

#[tokio::test]
async fn login_capabilities_enable_and_examine() {
    let (backend, inbox, handle) = opened(Access::ReadOnly, vec![]).await;
    let caps = backend.capabilities();
    assert!(caps.imap4rev1 && caps.idle && caps.condstore && caps.list_status);
    assert!(!caps.qresync && !caps.move_);
    assert_eq!(backend.enabled(), ["CONDSTORE"]);
    assert_eq!(backend.access(), Access::ReadOnly);
    assert_eq!(
        inbox,
        SelectedMailbox {
            name: "INBOX".into(),
            uid_validity: UidValidity(77),
            uid_next: 40,
            exists: 3,
            highest_modseq: Some(ModSeq(900)),
            read_only: true,
            keywords_allowed: false,
        }
    );
    let lines = finish(backend, handle).await;
    assert_eq!(lines.len(), 3, "{lines:?}");
}

#[tokio::test]
async fn read_only_refuses_writes_before_sending_anything() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![Expect("NOOP"), Send("{tag} OK noop\r\n")],
    )
    .await;
    let writes = [
        "SELECT \"INBOX\"",
        "UID STORE 1 +FLAGS (\\Seen)",
        "STORE 1 +FLAGS (\\Deleted)",
        "APPEND \"INBOX\" {3+}\r\nabc",
        "UID COPY 1 \"Trash\"",
        "UID MOVE 1 \"Trash\"",
        "EXPUNGE",
        "UID EXPUNGE 1",
        "CREATE \"x\"",
        "DELETE \"x\"",
        "RENAME \"a\" \"b\"",
        "SUBSCRIBE \"x\"",
        "CLOSE",
        "UID FETCH 1 BODY[]",
    ];
    for line in writes {
        let result = backend.conn_mut().run(line, |_| false).await;
        assert!(
            matches!(result, Err(BackendError::ReadOnly(_))),
            "{line}: {result:?}"
        );
    }
    // The connection is still fine, and the positive control shows the recorder records.
    backend.noop().await.expect("noop after refusals");
    assert_eq!(backend.access(), Access::ReadOnly);
    drop(inbox);
    let lines = finish(backend, handle).await;
    let verbs: Vec<&str> = lines
        .iter()
        .map(|l| l.split(' ').nth(1).unwrap_or_default())
        .collect();
    assert_eq!(
        verbs[..4],
        ["LOGIN", "ENABLE", "EXAMINE", "NOOP"],
        "{lines:?}"
    );
    assert_eq!(lines.len(), 4, "nothing else reached the server: {lines:?}");
    assert!(lines.iter().all(|l| !l.contains("BODY[")));
}

#[tokio::test]
async fn read_write_opens_with_select() {
    let script = vec![
        GREETING,
        LOGIN,
        LOGIN_OK,
        ENABLE,
        ENABLE_OK,
        Expect("SELECT \"Sent\" (CONDSTORE)"),
        Send(
            "* 0 EXISTS\r\n* OK [UIDVALIDITY 5] x\r\n* OK [PERMANENTFLAGS (\\Seen \\*)] x\r\n* OK [NOMODSEQ] no modseq here\r\n{tag} OK [READ-WRITE] done\r\n",
        ),
    ];
    let (backend, handle) = connect(Access::ReadWrite, Duration::from_secs(2), script).await;
    let mut backend = backend.expect("login");
    let sent = backend.open_mailbox("Sent").await.expect("select");
    // UIDNEXT missing and the mailbox is empty: 1. NOMODSEQ: no HIGHESTMODSEQ.
    assert_eq!(
        (sent.uid_next, sent.exists, sent.highest_modseq),
        (1, 0, None)
    );
    assert!(!sent.read_only && sent.keywords_allowed);
    finish(backend, handle).await;
}

#[tokio::test]
async fn missing_uidnext_is_derived_from_the_highest_uid() {
    let script = vec![
        GREETING,
        LOGIN,
        LOGIN_OK,
        ENABLE,
        ENABLE_OK,
        Expect("EXAMINE \"Archive\""),
        Send("* 2 EXISTS\r\n* OK [UIDVALIDITY 9] x\r\n{tag} OK done\r\n"),
        Expect("FETCH * (UID)"),
        Send("* 2 FETCH (UID 41)\r\n{tag} OK done\r\n"),
    ];
    let (backend, handle) = connect(Access::ReadOnly, Duration::from_secs(2), script).await;
    let mut backend = backend.expect("login");
    let archive = backend.open_mailbox("Archive").await.expect("examine");
    assert_eq!(archive.uid_next, 42);
    assert!(archive.read_only);
    finish(backend, handle).await;
}

#[tokio::test]
async fn refusals_are_typed_and_keep_the_connection() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 1:3 (UID FLAGS MODSEQ)"),
            Send("{tag} NO [SERVERBUG] fetch failed\r\n"),
            Expect("EXAMINE \"Gone\""),
            Send("{tag} NO [NONEXISTENT] no such mailbox\r\n"),
            Expect("EXAMINE \"Locked\""),
            Send("{tag} NO [NOPERM] not for you\r\n"),
            Expect("NOOP"),
            Send("{tag} OK fine\r\n"),
        ],
    )
    .await;
    // async-imap's own stream would report an empty Ok here (issue #151).
    let err = backend
        .fetch_flags(&inbox, &UidSet::range(1, 3), None)
        .await
        .expect_err("NO");
    assert!(
        matches!(&err, BackendError::Rejected(text) if text.contains("SERVERBUG")),
        "{err}"
    );
    assert_eq!(
        backend.open_mailbox("Gone").await,
        Err(BackendError::MailboxNotFound("Gone".into()))
    );
    assert!(matches!(
        backend.open_mailbox("Locked").await,
        Err(BackendError::Rejected(_))
    ));
    // No mailbox is open any more: mailbox-scoped calls refuse without sending.
    assert!(matches!(
        backend.search_uids(&inbox).await,
        Err(BackendError::InvalidState(_))
    ));
    backend.noop().await.expect("still usable");
    finish(backend, handle).await;
}

#[tokio::test]
async fn mailbox_scoped_calls_check_name_and_uidvalidity() {
    let (mut backend, inbox, handle) = opened(Access::ReadOnly, vec![]).await;
    let stale = SelectedMailbox {
        uid_validity: UidValidity(76),
        ..inbox.clone()
    };
    let other = SelectedMailbox {
        name: "Sent".into(),
        ..inbox.clone()
    };
    for mailbox in [&stale, &other] {
        assert!(matches!(
            backend.fetch_headers(mailbox, &UidSet::range(1, 2)).await,
            Err(BackendError::InvalidState(_))
        ));
    }
    // Empty sets are not sent at all.
    assert_eq!(
        backend.fetch_headers(&inbox, &UidSet::new()).await,
        Ok(vec![])
    );
    assert_eq!(
        backend.fetch_flags(&inbox, &UidSet::new(), None).await,
        Ok(vec![])
    );
    let lines = finish(backend, handle).await;
    assert_eq!(lines.len(), 3, "nothing after EXAMINE: {lines:?}");
}

#[tokio::test]
async fn fetch_flags_with_changedsince_and_unsolicited_changes() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 1:* (UID FLAGS MODSEQ) (CHANGEDSINCE 900)"),
            Send(
                "* 2 FETCH (UID 12 FLAGS (\\Seen) MODSEQ (901))\r\n* 4 EXISTS\r\n{tag} OK done\r\n",
            ),
        ],
    )
    .await;
    let updates = backend
        .fetch_flags(&inbox, &UidSet::all(), inbox.highest_modseq)
        .await
        .expect("flags");
    assert_eq!(updates.len(), 1);
    assert_eq!((updates[0].uid, updates[0].modseq), (12, Some(ModSeq(901))));
    // The EXISTS arrived on the side: the next IDLE returns at once, without a command.
    assert_eq!(
        backend
            .idle(&inbox, Duration::from_secs(60), std::future::pending())
            .await,
        Ok(IdleOutcome::Changed)
    );
    let lines = finish(backend, handle).await;
    assert!(lines.iter().all(|l| !l.contains("IDLE")), "{lines:?}");
}

#[tokio::test]
async fn idle_reports_changes_timeouts_and_stop() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            // 1: new mail while idling.
            Expect("IDLE"),
            Send("+ idling\r\n"),
            Pause(Duration::from_millis(100)),
            Send("* STATUS \"INBOX\" (MESSAGES 4)\r\n"),
            Send("* 4 EXISTS\r\n"),
            Expect("DONE"),
            Send("{tag} OK IDLE done\r\n"),
            // 2: time limit.
            Expect("IDLE"),
            Send("+ idling\r\n"),
            Expect("DONE"),
            Send("{tag} OK IDLE done\r\n"),
            // 3: stop signal; an EXPUNGE arrives while ending IDLE and stays pending.
            Expect("IDLE"),
            Send("+ idling\r\n"),
            Expect("DONE"),
            Send("* 1 EXPUNGE\r\n{tag} OK IDLE done\r\n"),
        ],
    )
    .await;
    let start = std::time::Instant::now();
    let outcome = backend
        .idle(&inbox, Duration::from_secs(30), std::future::pending())
        .await;
    assert_eq!(outcome, Ok(IdleOutcome::Changed));
    assert!(start.elapsed() < Duration::from_secs(5));

    let start = std::time::Instant::now();
    let outcome = backend
        .idle(&inbox, Duration::from_millis(300), std::future::pending())
        .await;
    assert_eq!(outcome, Ok(IdleOutcome::TimedOut));
    assert!(start.elapsed() >= Duration::from_millis(290));

    let outcome = backend
        .idle(
            &inbox,
            Duration::from_secs(30),
            tokio::time::sleep(Duration::from_millis(100)),
        )
        .await;
    assert_eq!(outcome, Ok(IdleOutcome::Stopped));
    // The EXPUNGE is reported by the next call without IDLE.
    assert_eq!(
        backend
            .idle(&inbox, Duration::from_secs(30), std::future::pending())
            .await,
        Ok(IdleOutcome::Changed)
    );
    let lines = finish(backend, handle).await;
    assert_eq!(
        lines.iter().filter(|l| l.ends_with("IDLE")).count(),
        3,
        "{lines:?}"
    );
}

#[tokio::test]
async fn connection_closed_mid_fetch_is_lost_and_poisons() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 1:3"),
            Send("* 1 FETCH (UID 1 FLAGS (\\Seen) RFC822.SIZE 10"),
            Close,
        ],
    )
    .await;
    let err = backend
        .fetch_headers(&inbox, &UidSet::range(1, 3))
        .await
        .expect_err("closed");
    assert!(matches!(err, BackendError::ConnectionLost(_)), "{err}");
    let again = backend.noop().await.expect_err("poisoned");
    assert!(
        matches!(&again, BackendError::ConnectionLost(text) if text.contains("earlier failure"))
    );
    finish(backend, handle).await;
}

#[tokio::test]
async fn bye_then_close_names_the_reason() {
    let (mut backend, _inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("NOOP"),
            Send("* BYE server shutting down\r\n"),
            Close,
        ],
    )
    .await;
    let err = backend.noop().await.expect_err("bye");
    assert!(
        matches!(&err, BackendError::ConnectionLost(text) if text.contains("shutting down")),
        "{err}"
    );
    finish(backend, handle).await;
}

#[tokio::test]
async fn silent_server_times_out_and_dropped_futures_poison() {
    let mut script = vec![
        GREETING, LOGIN, LOGIN_OK, ENABLE, ENABLE_OK, EXAMINE, EXAMINE_OK,
    ];
    script.extend([Expect("NOOP"), Pause(Duration::from_secs(5))]);
    let (backend, handle) = connect(Access::ReadOnly, Duration::from_millis(300), script).await;
    let mut backend = backend.expect("login");
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    let start = std::time::Instant::now();
    let err = backend.noop().await.expect_err("silent");
    assert!(matches!(err, BackendError::Timeout(_)), "{err}");
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        backend.search_uids(&inbox).await,
        Err(BackendError::ConnectionLost(_))
    ));
    drop(backend);
    handle.abort();

    // A future dropped half-way (here by a caller's timeout) leaves the connection unusable.
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![Expect("UID SEARCH ALL"), Pause(Duration::from_secs(5))],
    )
    .await;
    let cut = tokio::time::timeout(Duration::from_millis(100), backend.search_uids(&inbox)).await;
    assert!(cut.is_err());
    assert!(matches!(
        backend.noop().await,
        Err(BackendError::ConnectionLost(_))
    ));
    drop(backend);
    handle.abort();
}

#[tokio::test]
async fn list_with_list_status_and_modified_utf7() {
    let (backend, handle) = connect(
        Access::ReadOnly,
        Duration::from_secs(2),
        vec![
            GREETING,
            LOGIN,
            LOGIN_OK,
            ENABLE,
            ENABLE_OK,
            Expect(
                "LIST \"\" \"*\" RETURN (SPECIAL-USE STATUS (MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ))",
            ),
            Send(
                "* LIST (\\HasNoChildren) \"/\" \"inbox\"\r\n\
* STATUS \"INBOX\" (MESSAGES 3 UNSEEN 1 UIDNEXT 40 UIDVALIDITY 77 HIGHESTMODSEQ 900)\r\n\
* LIST (\\HasNoChildren \\Sent) \"/\" \"Gesendet\"\r\n\
* STATUS \"Gesendet\" (MESSAGES 0 UNSEEN 0 UIDNEXT 1 UIDVALIDITY 78 HIGHESTMODSEQ 1)\r\n\
* LIST (\\Noselect \\HasChildren) \"/\" \"Projects\"\r\n\
* LIST (\\HasNoChildren) \"/\" \"Projects/&ANw-berpr&APw-fung\"\r\n\
* STATUS \"Projects/&ANw-berpr&APw-fung\" (MESSAGES 1 UNSEEN 0 UIDNEXT 2 UIDVALIDITY 79)\r\n\
{tag} OK LIST done\r\n",
            ),
        ],
    )
    .await;
    let mut backend = backend.expect("login");
    let list = backend.list_mailboxes().await.expect("list");
    let paths: Vec<&str> = list.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        ["INBOX", "Gesendet", "Projects", "Projects/Überprüfung"]
    );
    assert_eq!(list[0].name, "INBOX");
    assert_eq!(list[3].name, "Projects/&ANw-berpr&APw-fung");
    assert_eq!(list[3].delimiter, Some('/'));
    assert!(list[1].has_attribute("\\Sent"));
    assert!(!list[2].is_selectable() && list[2].status.is_none());
    let inbox = list[0].status.expect("status");
    assert_eq!(
        (inbox.messages, inbox.unseen, inbox.uid_next),
        (3, Some(1), Some(40))
    );
    assert_eq!(inbox.highest_modseq, Some(ModSeq(900)));
    assert_eq!(
        list[3].status.map(|s| s.uid_validity),
        Some(Some(UidValidity(79)))
    );
    finish(backend, handle).await;
}

#[tokio::test]
async fn list_without_list_status_asks_status_per_mailbox() {
    let (backend, handle) = connect(
        Access::ReadOnly,
        Duration::from_secs(2),
        vec![
            Send("* OK hello\r\n"),
            LOGIN,
            Send("{tag} OK logged in\r\n"),
            Expect("CAPABILITY"),
            Send("* CAPABILITY IMAP4rev1 IDLE\r\n{tag} OK done\r\n"),
            Expect("LIST \"\" \"*\""),
            Send("* LIST () \".\" INBOX\r\n* LIST () \".\" \"Papierkorb\"\r\n{tag} OK done\r\n"),
            Expect("STATUS \"INBOX\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)"),
            Send("* STATUS INBOX (MESSAGES 2 UIDNEXT 3 UIDVALIDITY 1)\r\n{tag} OK done\r\n"),
            Expect("STATUS \"Papierkorb\""),
            Send("{tag} NO cannot\r\n"),
        ],
    )
    .await;
    let mut backend = backend.expect("login");
    assert!(!backend.capabilities().condstore && backend.enabled().is_empty());
    let list = backend.list_mailboxes().await.expect("list");
    assert_eq!(list[0].status.map(|s| s.messages), Some(2));
    assert_eq!(list[1].status, None);
    assert_eq!(list[1].delimiter, Some('.'));
    finish(backend, handle).await;
}

#[tokio::test]
async fn wrong_password_is_auth_failed_and_old_servers_are_refused() {
    let (backend, handle) = connect(
        Access::ReadOnly,
        Duration::from_secs(2),
        vec![
            GREETING,
            LOGIN,
            Send("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n"),
        ],
    )
    .await;
    let err = backend.expect_err("wrong password");
    assert_eq!(
        err,
        BackendError::AuthFailed("[AUTHENTICATIONFAILED] Authentication failed.".into())
    );
    assert!(!err.to_string().contains("\"pw\""));
    handle.await.expect("fake server");

    let (backend, handle) = connect(
        Access::ReadOnly,
        Duration::from_secs(2),
        vec![
            GREETING,
            LOGIN,
            Send("{tag} NO [UNAVAILABLE] try later\r\n"),
        ],
    )
    .await;
    assert!(matches!(backend, Err(BackendError::Rejected(_))));
    handle.await.expect("fake server");

    let (backend, handle) = connect(
        Access::ReadOnly,
        Duration::from_secs(2),
        vec![
            Send("* OK hello\r\n"),
            LOGIN,
            // imap-proto refuses a capability list without IMAP4rev1 as a response code ...
            Send("{tag} OK [CAPABILITY IMAP2] ok\r\n"),
            Expect("CAPABILITY"),
            // ... and as a response.
            Send("* CAPABILITY IMAP2 IDLE\r\n{tag} OK done\r\n"),
        ],
    )
    .await;
    assert!(
        matches!(&backend, Err(BackendError::Unsupported(text)) if text.starts_with("IMAP4rev1")),
        "{:?}",
        backend.err()
    );
    handle.await.expect("fake server");

    let (backend, handle) = connect(
        Access::ReadOnly,
        Duration::from_secs(2),
        vec![Send("* BYE too many connections\r\n")],
    )
    .await;
    assert!(matches!(backend, Err(BackendError::Rejected(text)) if text.contains("too many")));
    handle.await.expect("fake server");
}

#[tokio::test]
async fn changedsince_without_modseq_is_unsupported() {
    let script = vec![
        Send("* OK hi\r\n"),
        LOGIN,
        Send("{tag} OK [CAPABILITY IMAP4rev1] ok\r\n"),
        Expect("EXAMINE \"INBOX\""),
        Send("* 1 EXISTS\r\n* OK [UIDVALIDITY 1] x\r\n* OK [UIDNEXT 2] x\r\n{tag} OK done\r\n"),
    ];
    let (backend, handle) = connect(Access::ReadOnly, Duration::from_secs(2), script).await;
    let mut backend = backend.expect("login");
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    assert_eq!(inbox.highest_modseq, None);
    assert!(matches!(
        backend
            .fetch_flags(&inbox, &UidSet::all(), Some(ModSeq(1)))
            .await,
        Err(BackendError::Unsupported(_))
    ));
    assert!(matches!(
        backend
            .idle(&inbox, Duration::from_secs(1), std::future::pending())
            .await,
        Err(BackendError::Unsupported(_))
    ));
    let lines = finish(backend, handle).await;
    assert_eq!(lines.len(), 2, "{lines:?}");
}

fn umlaut_config() -> ConnectConfig {
    let mut config = config(Access::ReadOnly);
    config.password = Secret::new("pässwört".into());
    config
}

async fn connect_with(config: ConnectConfig, script: Vec<Step>) -> Fake {
    let (client, server) = tokio::io::duplex(1 << 16);
    let handle = tokio::spawn(fake_server(server, script));
    let timeouts = TimeoutHandle::new(Duration::from_secs(2), Duration::from_secs(2));
    let mut client = Client::new(IoTimeout::new(client, timeouts.clone()));
    let backend = match read_greeting(&mut client).await {
        Ok(caps) => ImapBackend::establish(client, timeouts, &config, caps.as_deref()).await,
        Err(err) => Err(err),
    };
    (backend, handle)
}

/// `\0alice\0pässwört`, base64.
const PLAIN_UMLAUT: &str = "AGFsaWNlAHDDpHNzd8O2cnQ=";

#[tokio::test]
async fn non_ascii_password_uses_authenticate_plain() {
    // AUTH=PLAIN without SASL-IR: the credentials follow the server's continuation.
    let (backend, handle) = connect_with(
        umlaut_config(),
        vec![
            GREETING,
            Expect("AUTHENTICATE PLAIN"),
            Send("+ \r\n"),
            Expect(PLAIN_UMLAUT),
            Send("{tag} OK authenticated\r\n"),
            Expect("CAPABILITY"),
            Send("* CAPABILITY IMAP4rev1 IDLE\r\n{tag} OK done\r\n"),
        ],
    )
    .await;
    let backend = backend.expect("authenticated");
    assert!(backend.capabilities().idle);
    let lines = finish(backend, handle).await;
    assert_eq!(
        lines[0].split_once(' ').map(|(_, l)| l),
        Some("AUTHENTICATE PLAIN")
    );
    assert!(lines.iter().all(|l| !l.contains("pässwört")), "{lines:?}");

    // With SASL-IR the initial response goes on the command line: one round trip.
    let (backend, handle) = connect_with(
        umlaut_config(),
        vec![
            Send("* OK [CAPABILITY IMAP4rev1 SASL-IR AUTH=PLAIN] ready\r\n"),
            Expect("AUTHENTICATE PLAIN AGFsaWNlAHDDpHNzd8O2cnQ="),
            Send("{tag} OK [CAPABILITY IMAP4rev1] authenticated\r\n"),
            Expect("CAPABILITY"),
            Send("* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n"),
        ],
    )
    .await;
    let backend = backend.expect("authenticated with SASL-IR");
    drop(finish(backend, handle).await);

    // A wrong password is AuthFailed, as with LOGIN.
    let (backend, handle) = connect_with(
        umlaut_config(),
        vec![
            GREETING,
            Expect("AUTHENTICATE PLAIN"),
            Send("+ \r\n"),
            Expect(PLAIN_UMLAUT),
            Send("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n"),
        ],
    )
    .await;
    let err = backend.expect_err("wrong password");
    assert!(matches!(err, BackendError::AuthFailed(_)), "{err:?}");
    assert!(!format!("{err} {err:?}").contains("pässwört"));
    handle.await.expect("fake server");

    // A server known to lack AUTH=PLAIN gets LOGIN with a quoted UTF-8 string.
    let (backend, handle) = connect_with(
        umlaut_config(),
        vec![
            Send("* OK [CAPABILITY IMAP4rev1 AUTH=CRAM-MD5] ready\r\n"),
            Expect("LOGIN \"alice\" \"pässwört\""),
            Send("{tag} OK [CAPABILITY IMAP4rev1] logged in\r\n"),
        ],
    )
    .await;
    drop(finish(backend.expect("login"), handle).await);

    // ASCII credentials keep using LOGIN even when AUTH=PLAIN is offered.
    let (backend, handle) = connect_with(
        config(Access::ReadOnly),
        vec![
            Send("* OK [CAPABILITY IMAP4rev1 SASL-IR AUTH=PLAIN] ready\r\n"),
            LOGIN,
            Send("{tag} OK [CAPABILITY IMAP4rev1] logged in\r\n"),
        ],
    )
    .await;
    drop(finish(backend.expect("login"), handle).await);
}

/// A raw message of `len` bytes (header plus filler lines), as a static string for the script.
fn raw_message(len: usize) -> &'static str {
    let mut text = String::from("Subject: big\r\nMessage-ID: <big@fake.test>\r\n\r\n");
    while text.len() < len {
        text.push_str("0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstu\r\n");
    }
    text.truncate(len);
    Box::leak(text.into_boxed_str())
}

fn literal_response(seq: u32, uid: u32, item: &str, data: &str) -> &'static str {
    Box::leak(
        format!(
            "* {seq} FETCH (UID {uid} {item} {{{}}}\r\n{data})\r\n",
            data.len()
        )
        .into_boxed_str(),
    )
}

#[tokio::test]
async fn bodies_are_fetched_with_peek_only_and_parsed() {
    let small = "From: a@x\r\nSubject: hi\r\nContent-Type: text/plain\r\n\r\nHello.\r\n";
    let big = raw_message(3 * 1024 * 1024);
    let structure = "* 2 FETCH (UID 12 RFC822.SIZE 3145728 BODYSTRUCTURE ((\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" 7 1 NIL NIL NIL NIL)\
(\"APPLICATION\" \"PDF\" NIL NIL NIL \"BASE64\" 3145000 NIL (\"ATTACHMENT\" (\"FILENAME*\" \"utf-8''M%C3%A4rz.pdf\")) NIL NIL) \"MIXED\" (\"BOUNDARY\" \"b\") NIL NIL NIL))\r\n{tag} OK done\r\n";
    let sections = "* 2 FETCH (UID 12 BODY[HEADER] {12}\r\nSubject: x\r\n BODY[1.MIME] {28}\r\n\
Content-Type: text/plain\r\n\r\n BODY[1] {7}\r\nHello.\n)\r\n{tag} OK done\r\n";
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 12 (UID RFC822.SIZE BODYSTRUCTURE)"),
            Send(structure),
            Expect("UID FETCH 12 (UID BODY.PEEK[HEADER] BODY.PEEK[1.MIME] BODY.PEEK[1])"),
            Send(sections),
            Expect("UID FETCH 11:13 (UID BODY.PEEK[])"),
            Send(literal_response(1, 11, "BODY[]", small)),
            Send(literal_response(2, 12, "BODY[]", big)),
            // UID 13 is gone; a flag change of another message arrives on the side.
            Send("* 3 FETCH (UID 20 FLAGS (\\Seen))\r\n{tag} OK done\r\n"),
            Expect("UID FETCH 99 (UID RFC822.SIZE BODYSTRUCTURE)"),
            Send("{tag} OK nothing\r\n"),
            Expect("UID FETCH 99 (UID BODY.PEEK[HEADER])"),
            Send("{tag} OK nothing\r\n"),
        ],
    )
    .await;

    let found = backend
        .fetch_structure(&inbox, 12)
        .await
        .expect("structure")
        .expect("uid 12 exists");
    assert_eq!(found.size, 3_145_728);
    let paths: Vec<String> = found
        .parts
        .iter()
        .map(|p| p.path.as_ref().map(ToString::to_string).unwrap_or_default())
        .collect();
    assert_eq!(paths, ["", "1", "2"]);
    assert_eq!(found.parts[2].file_name.as_deref(), Some("März.pdf"));
    let BodyFetchPlan::Sections(plan) = found.fetch_plan(LARGE_MESSAGE_THRESHOLD) else {
        panic!("a 3 MB message is fetched in sections");
    };
    let fetched = backend
        .fetch_sections(&inbox, 12, &plan)
        .await
        .expect("sections")
        .expect("uid 12 exists");
    let got: Vec<(String, &[u8])> = fetched
        .iter()
        .map(|s| (s.section.spec(), s.data.as_slice()))
        .collect();
    assert_eq!(
        got,
        [
            ("HEADER".to_owned(), &b"Subject: x\r\n"[..]),
            (
                "1.MIME".to_owned(),
                &b"Content-Type: text/plain\r\n\r\n"[..]
            ),
            ("1".to_owned(), &b"Hello.\n"[..]),
        ]
    );

    let mut messages = backend
        .fetch_messages(&inbox, &UidSet::range(11, 13))
        .await
        .expect("messages");
    messages.sort_by_key(|m| m.uid);
    assert_eq!(messages.len(), 2, "UID 13 is missing, not an error");
    assert_eq!(messages[0].data, small.as_bytes());
    assert_eq!(messages[1].data.len(), big.len(), "3 MB literal read whole");
    assert_eq!(messages[1].data, big.as_bytes());

    assert_eq!(backend.fetch_structure(&inbox, 99).await, Ok(None));
    assert_eq!(
        backend
            .fetch_sections(&inbox, 99, &[BodySection::Header])
            .await,
        Ok(None)
    );
    // Nothing to send: empty sets and UID 0 never reach the server.
    assert_eq!(
        backend.fetch_messages(&inbox, &UidSet::new()).await,
        Ok(vec![])
    );
    assert_eq!(
        backend.fetch_sections(&inbox, 12, &[]).await,
        Ok(Some(vec![]))
    );
    assert_eq!(backend.fetch_structure(&inbox, 0).await, Ok(None));
    // The flag change seen during the body fetch is reported by the next IDLE at once.
    assert_eq!(
        backend
            .idle(&inbox, Duration::from_secs(30), std::future::pending())
            .await,
        Ok(IdleOutcome::Changed)
    );

    let lines = finish(backend, handle).await;
    let fetches: Vec<&String> = lines.iter().filter(|l| l.contains("FETCH")).collect();
    assert_eq!(fetches.len(), 5, "{lines:?}");
    for line in &lines {
        let upper = line.to_ascii_uppercase();
        assert!(
            !upper.replace("BODY.PEEK[", "").contains("BODY["),
            "a body section without PEEK went out: {line}"
        );
        let verbs: Vec<&str> = upper.split(' ').skip(1).take(2).collect();
        assert!(
            !verbs.contains(&"STORE") && !upper.contains("\\SEEN"),
            "{line}"
        );
        assert!(
            !upper.contains("RFC822)") && !upper.contains("RFC822.TEXT"),
            "{line}"
        );
    }
}

#[tokio::test]
async fn body_fetch_refusals_and_wrong_mailbox() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 5 (UID BODY.PEEK[])"),
            Send("{tag} NO [UNAVAILABLE] try later\r\n"),
            Expect("NOOP"),
            Send("{tag} OK fine\r\n"),
        ],
    )
    .await;
    let err = backend
        .fetch_messages(&inbox, &UidSet::range(5, 5))
        .await
        .expect_err("NO");
    assert!(
        matches!(&err, BackendError::Rejected(text) if text.contains("UNAVAILABLE")),
        "{err}"
    );
    let stale = SelectedMailbox {
        uid_validity: UidValidity(1),
        ..inbox.clone()
    };
    assert!(matches!(
        backend.fetch_structure(&stale, 5).await,
        Err(BackendError::InvalidState(_))
    ));
    assert!(matches!(
        backend
            .fetch_sections(&stale, 5, &[BodySection::Header])
            .await,
        Err(BackendError::InvalidState(_))
    ));
    assert!(matches!(
        backend.fetch_messages(&stale, &UidSet::range(5, 5)).await,
        Err(BackendError::InvalidState(_))
    ));
    backend
        .noop()
        .await
        .expect("still usable after the refusal");
    let lines = finish(backend, handle).await;
    assert_eq!(lines.len(), 5, "{lines:?}");
}

// ---------------------------------------------------------------------------------------------
// Writer (stage 4)

mod writer {
    use mail_model::{
        ExpungeOutcome, Flag, Flags, MailWriter, MoveMethod, StoreMode, WriteConnector,
    };

    use super::*;
    use crate::{ImapConnector, ImapWriter};

    const SELECT: Step = Expect("SELECT \"INBOX\" (CONDSTORE)");
    const SELECT_OK: Step = Send(
        "* 3 EXISTS\r\n* OK [PERMANENTFLAGS (\\Seen \\Deleted \\*)] ok\r\n\
* OK [UIDVALIDITY 77] ok\r\n* OK [UIDNEXT 40] ok\r\n* OK [HIGHESTMODSEQ 900] ok\r\n\
{tag} OK [READ-WRITE] SELECT completed\r\n",
    );

    /// Login with `caps`, ENABLE, SELECT INBOX, then `rest`.
    async fn writer(
        caps: &'static str,
        rest: Vec<Step>,
    ) -> (
        ImapWriter<DuplexStream>,
        SelectedMailbox,
        JoinHandle<Vec<String>>,
    ) {
        let mut script = vec![
            GREETING,
            LOGIN,
            Send(caps),
            ENABLE,
            ENABLE_OK,
            SELECT,
            SELECT_OK,
        ];
        script.extend(rest);
        let (backend, handle) = connect(Access::ReadWrite, Duration::from_secs(2), script).await;
        let mut writer = ImapWriter::new(backend.expect("login")).expect("read-write");
        let inbox = writer.open_mailbox("INBOX").await.expect("select");
        assert!(!inbox.read_only && inbox.keywords_allowed);
        (writer, inbox, handle)
    }

    async fn done(
        writer: ImapWriter<DuplexStream>,
        handle: JoinHandle<Vec<String>>,
    ) -> Vec<String> {
        drop(writer);
        handle.await.expect("fake server")
    }

    const NO_MOVE: &str = "{tag} OK [CAPABILITY IMAP4rev1 CONDSTORE ENABLE] logged in\r\n";
    const MOVE_UIDPLUS: &str =
        "{tag} OK [CAPABILITY IMAP4rev1 CONDSTORE ENABLE MOVE UIDPLUS] logged in\r\n";

    #[tokio::test]
    async fn store_reports_new_flags_and_modified_uids() {
        let (mut w, inbox, handle) = writer(
            NO_MOVE,
            vec![
                Expect("UID STORE 5,7 (UNCHANGEDSINCE 900) +FLAGS (\\Seen $Junk)"),
                Send(
                    "* 1 FETCH (UID 5 FLAGS (\\Seen $Junk) MODSEQ (901))\r\n\
* 2 FETCH (UID 6 FLAGS (\\Flagged) MODSEQ (902))\r\n\
{tag} OK [MODIFIED 7] Conditional STORE failed\r\n",
                ),
                Expect("UID STORE 5 -FLAGS (\\Flagged)"),
                Send("{tag} NO [CANNOT] not today\r\n"),
            ],
        )
        .await;
        let flags = [Flag::Seen, Flag::keyword("$Junk").expect("kw")];
        let out = w
            .store_flags(
                &inbox,
                &UidSet::from_uids([5, 7]),
                StoreMode::Add,
                &flags,
                Some(ModSeq(900)),
            )
            .await
            .expect("store");
        assert_eq!(out.modified, UidSet::from_uids([7]));
        assert_eq!(out.updates.len(), 1, "UID 6 was not asked for: {out:?}");
        assert_eq!(out.updates[0].uid, 5);
        assert_eq!(out.updates[0].modseq, Some(ModSeq(901)));
        assert_eq!(out.updates[0].flags, Flags::from_imap(["\\Seen", "$Junk"]));

        // A refusal is a typed error, not an empty success (async-imap's uid_store).
        let err = w
            .store_flags(
                &inbox,
                &UidSet::from_uids([5]),
                StoreMode::Remove,
                &[Flag::Flagged],
                None,
            )
            .await
            .expect_err("NO");
        assert!(matches!(err, BackendError::Rejected(_)), "{err}");

        // Invalid keywords never leave the adapter.
        let err = w
            .store_flags(
                &inbox,
                &UidSet::from_uids([5]),
                StoreMode::Add,
                &[Flag::Keyword("two words".into())],
                None,
            )
            .await
            .expect_err("invalid keyword");
        assert!(matches!(err, BackendError::InvalidState(_)), "{err}");
        let lines = done(w, handle).await;
        assert!(lines.iter().all(|l| !l.contains("two words")), "{lines:?}");
    }

    #[tokio::test]
    async fn move_without_move_or_uidplus_never_expunges_foreign_deleted_mail() {
        let (mut w, inbox, handle) = writer(
            NO_MOVE,
            vec![
                Expect("UID COPY 5 \"Archive\""),
                Send("{tag} OK copied\r\n"),
                Expect("UID STORE 5 +FLAGS.SILENT (\\Deleted)"),
                Send("* 1 FETCH (UID 5 MODSEQ (950))\r\n{tag} OK stored\r\n"),
                Expect("UID SEARCH DELETED"),
                // UID 3 was marked deleted by someone else.
                Send("* SEARCH 3 5\r\n{tag} OK searched\r\n"),
                // Second move: only our message is marked, so EXPUNGE is safe.
                Expect("UID COPY 6 \"Archive\""),
                Send("{tag} OK copied\r\n"),
                Expect("UID STORE 6 +FLAGS.SILENT (\\Deleted)"),
                Send("{tag} OK stored\r\n"),
                Expect("UID SEARCH DELETED"),
                Send("* SEARCH 6\r\n{tag} OK searched\r\n"),
                Expect("EXPUNGE"),
                Send("* 2 EXPUNGE\r\n{tag} OK expunged\r\n"),
            ],
        )
        .await;
        let first = w
            .move_messages(&inbox, &UidSet::from_uids([5]), "Archive")
            .await
            .expect("move");
        assert_eq!(first.method, MoveMethod::CopyLeftDeleted);
        assert!(first.copy_uid.is_none());
        let second = w
            .move_messages(&inbox, &UidSet::from_uids([6]), "Archive")
            .await
            .expect("move");
        assert_eq!(second.method, MoveMethod::CopyExpunge);
        let lines = done(w, handle).await;
        let expunges = lines.iter().filter(|l| l.ends_with(" EXPUNGE")).count();
        assert_eq!(expunges, 1, "{lines:?}");
    }

    #[tokio::test]
    async fn move_and_expunge_with_move_and_uidplus() {
        let (mut w, inbox, handle) = writer(
            MOVE_UIDPLUS,
            vec![
                Expect("UID MOVE 5:6 \"Trash\""),
                Send(
                    "* OK [COPYUID 9 5:6 100:101] Moved\r\n* 1 EXPUNGE\r\n* 1 EXPUNGE\r\n\
{tag} OK done\r\n",
                ),
                Expect("UID STORE 7 +FLAGS.SILENT (\\Deleted)"),
                Send("{tag} OK stored\r\n"),
                Expect("UID EXPUNGE 7"),
                Send("* 1 EXPUNGE\r\n{tag} OK expunged\r\n"),
                Expect("STATUS \"Trash\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ)"),
                Send("* STATUS \"Trash\" (MESSAGES 2 UIDNEXT 102 UIDVALIDITY 9 UNSEEN 0 HIGHESTMODSEQ 5)\r\n{tag} OK\r\n"),
                Expect("STATUS \"Gone\""),
                Send("{tag} NO [NONEXISTENT] no such mailbox\r\n"),
            ],
        )
        .await;
        let moved = w
            .move_messages(&inbox, &UidSet::range(5, 6), "Trash")
            .await
            .expect("move");
        assert_eq!(moved.method, MoveMethod::Move);
        let copy = moved.copy_uid.expect("COPYUID");
        assert_eq!(
            (copy.target_of(5), copy.target_of(6)),
            (Some(100), Some(101))
        );
        assert_eq!(
            w.expunge_messages(&inbox, &UidSet::from_uids([7]))
                .await
                .expect("expunge"),
            ExpungeOutcome::Expunged
        );
        let status = w.mailbox_status("Trash").await.expect("status");
        assert_eq!(status.uid_next, Some(102));
        assert_eq!(status.uid_validity, Some(UidValidity(9)));
        assert_eq!(
            w.mailbox_status("Gone").await,
            Err(BackendError::MailboxNotFound("Gone".into()))
        );
        done(w, handle).await;
    }

    #[tokio::test]
    async fn create_is_idempotent() {
        let (mut w, _inbox, handle) = writer(
            NO_MOVE,
            vec![
                Expect("CREATE \"Archiv &APw-ber\""),
                Send("{tag} NO [ALREADYEXISTS] exists\r\n"),
                Expect("LIST \"\" \"Archiv &APw-ber\""),
                Send("* LIST () \"/\" \"Archiv &APw-ber\"\r\n{tag} OK listed\r\n"),
                Expect("SUBSCRIBE \"Archiv &APw-ber\""),
                Send("{tag} OK subscribed\r\n"),
                Expect("CREATE \"Bad\""),
                Send("{tag} NO [NOPERM] not allowed\r\n"),
                Expect("LIST \"\" \"Bad\""),
                Send("{tag} OK listed\r\n"),
            ],
        )
        .await;
        let name = w
            .create_mailbox("Archiv über", Some(mail_model::MailboxRole::Archive))
            .await
            .expect("exists already = success");
        assert_eq!(name, "Archiv &APw-ber");
        let err = w.create_mailbox("Bad", None).await.expect_err("refused");
        assert!(matches!(err, BackendError::Rejected(t) if t.contains("NOPERM")));
        done(w, handle).await;
    }

    #[tokio::test]
    async fn create_without_the_special_use_the_server_refuses() {
        let (mut w, _inbox, handle) = writer(
            "{tag} OK [CAPABILITY IMAP4rev1 CONDSTORE ENABLE CREATE-SPECIAL-USE] logged in\r\n",
            vec![
                Expect("CREATE \"Ablage\" (USE (\\Archive))"),
                Send("{tag} NO [USEATTR] a mailbox with this role exists\r\n"),
                Expect("CREATE \"Ablage\""),
                Send("{tag} OK created\r\n"),
                Expect("SUBSCRIBE \"Ablage\""),
                Send("{tag} OK subscribed\r\n"),
            ],
        )
        .await;
        let name = w
            .create_mailbox("Ablage", Some(mail_model::MailboxRole::Archive))
            .await
            .expect("created without the attribute");
        assert_eq!(name, "Ablage");
        done(w, handle).await;
    }

    #[tokio::test]
    async fn writers_need_read_write_access() {
        // Refused before any network access: the host does not even resolve.
        let err = ImapConnector::new()
            .connect_writer(&config(Access::ReadOnly))
            .await
            .expect_err("read-only config");
        assert!(matches!(err, BackendError::ReadOnly(_)), "{err}");

        let (backend, handle) = connect(
            Access::ReadOnly,
            Duration::from_secs(2),
            vec![GREETING, LOGIN, LOGIN_OK, ENABLE, ENABLE_OK],
        )
        .await;
        let err = ImapWriter::new(backend.expect("login")).expect_err("read-only backend");
        assert!(matches!(err, BackendError::ReadOnly(_)));
        let lines = handle.await.expect("fake server");
        assert_eq!(lines.len(), 2, "only LOGIN and ENABLE: {lines:?}");
    }
}

// Stage 14, block A: Gmail over IMAP (recorded responses).
mod gmail;

// Stage 14 (decision 0042): accounts that sign in with Microsoft or Google.

fn bearer_config() -> ConnectConfig {
    let mut config = config(Access::ReadOnly);
    config.password = Secret::new("tok-ya29".into());
    config.auth = mail_model::LoginAuth::Bearer;
    config
}

/// `user=alice^Aauth=Bearer tok-ya29^A^A`, base64.
const XOAUTH2: &str = "dXNlcj1hbGljZQFhdXRoPUJlYXJlciB0b2steWEyOQEB";

#[tokio::test]
async fn bearer_tokens_log_in_with_xoauth2_and_never_with_login() {
    // SASL-IR announced (Gmail, Microsoft): one line.
    let (backend, handle) = connect_with(
        bearer_config(),
        vec![
            Send("* OK [CAPABILITY IMAP4rev1 SASL-IR AUTH=PLAIN AUTH=XOAUTH2 AUTH=OAUTHBEARER] ready\r\n"),
            Expect("AUTHENTICATE XOAUTH2 dXNlcj1hbGljZQFhdXRoPUJlYXJlciB0b2steWEyOQEB"),
            Send("{tag} OK [CAPABILITY IMAP4rev1 IDLE] authenticated\r\n"),
            Expect("CAPABILITY"),
            Send("* CAPABILITY IMAP4rev1 IDLE\r\n{tag} OK done\r\n"),
        ],
    )
    .await;
    let backend = backend.expect("authenticated");
    let lines = finish(backend, handle).await;
    assert!(lines.iter().all(|l| !l.contains("LOGIN")), "{lines:?}");

    // Capabilities unknown (STARTTLS): XOAUTH2 after the continuation, never LOGIN.
    let (backend, handle) = connect_with(
        bearer_config(),
        vec![
            Send("* OK ready\r\n"),
            Expect("AUTHENTICATE XOAUTH2"),
            Send("+ \r\n"),
            Expect(XOAUTH2),
            Send("{tag} OK authenticated\r\n"),
            Expect("CAPABILITY"),
            Send("* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n"),
        ],
    )
    .await;
    drop(finish(backend.expect("authenticated"), handle).await);

    // A rejected token: the error challenge gets an empty answer, the NO is AuthFailed (the
    // engine then refreshes the token once), and the token is in no error text.
    let (backend, handle) = connect_with(
        bearer_config(),
        vec![
            Send("* OK [CAPABILITY IMAP4rev1 SASL-IR AUTH=XOAUTH2] ready\r\n"),
            Expect("AUTHENTICATE XOAUTH2 "),
            Send("+ eyJzdGF0dXMiOiJpbnZhbGlkX3Rva2VuIn0=\r\n"),
            Expect(""),
            Send("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n"),
        ],
    )
    .await;
    let err = backend.expect_err("token rejected");
    assert!(matches!(err, BackendError::AuthFailed(_)), "{err:?}");
    assert!(!format!("{err:?} {err}").contains("tok-ya29"));
    let lines = handle.await.expect("server");
    assert_eq!(lines.get(1).map(String::as_str), Some(""), "{lines:?}");
}

#[tokio::test]
async fn bearer_tokens_use_oauthbearer_where_only_that_is_offered() {
    let (backend, handle) = connect_with(
        bearer_config(),
        vec![
            Send("* OK [CAPABILITY IMAP4rev1 SASL-IR AUTH=OAUTHBEARER] ready\r\n"),
            Expect("AUTHENTICATE OAUTHBEARER bixhPWFsaWNlLAFob3N0PWZha2UudGVzdAFwb3J0PTk5MwFhdXRoPUJlYXJlciB0b2steWEyOQEB"),
            Send("+ eyJzdGF0dXMiOiJpbnZhbGlkX3Rva2VuIn0=\r\n"),
            Expect("AQ=="),
            Send("{tag} NO [AUTHENTICATIONFAILED] Invalid token\r\n"),
        ],
    )
    .await;
    let err = backend.expect_err("token rejected");
    assert!(matches!(err, BackendError::AuthFailed(_)), "{err:?}");
    drop(handle.await.expect("server"));
}

const LOGIN_OK_QUOTA: Step = Send(
    "{tag} OK [CAPABILITY IMAP4rev1 IDLE CONDSTORE ENABLE QUOTA QUOTA=RES-STORAGE] logged in\r\n",
);

/// Stage 20 (decision 0060): `GETQUOTAROOT INBOX` on a read-only connection (RFC 9208), the
/// `STORAGE` resource of the inbox's root in KiB; nothing without the capability or the
/// resource; a refusal reports nothing and keeps the connection.
#[tokio::test]
async fn the_quota_of_the_inbox_root_is_read_only() {
    let script = vec![
        GREETING,
        LOGIN,
        LOGIN_OK_QUOTA,
        ENABLE,
        ENABLE_OK,
        Expect("GETQUOTAROOT INBOX"),
        Send(
            "* QUOTAROOT INBOX \"\" \"user/alice\"\r\n* QUOTA \"\" (MESSAGE 12 1000)\r\n\
             * QUOTA \"user/alice\" (STORAGE 950 1000 MESSAGE 12 1000)\r\n{tag} OK done\r\n",
        ),
        Expect("GETQUOTAROOT INBOX"),
        Send("{tag} NO [NOPERM] not for you\r\n"),
        Expect("NOOP"),
        Send("{tag} OK fine\r\n"),
    ];
    let (backend, handle) = connect(Access::ReadOnly, Duration::from_secs(2), script).await;
    let mut backend = backend.expect("login");
    let quota = backend.quota().await.expect("quota").expect("reported");
    assert_eq!(
        (quota.used_bytes, quota.limit_bytes),
        (950 * 1024, 1000 * 1024)
    );
    assert_eq!(quota.percent(), 95);
    assert_eq!(backend.quota().await.expect("refused"), None);
    backend.noop().await.expect("still usable");
    let lines = finish(backend, handle).await;
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("GETQUOTAROOT INBOX"))
            .count(),
        2
    );

    // Without the capability nothing is asked.
    let (backend, inbox, handle) = opened(Access::ReadOnly, vec![]).await;
    let _ = inbox;
    let mut backend = backend;
    assert_eq!(backend.quota().await.expect("none"), None);
    let lines = finish(backend, handle).await;
    assert!(!lines.iter().any(|l| l.contains("QUOTA")), "{lines:?}");
}

#[test]
fn storage_of_the_named_root_wins_and_a_zero_limit_reports_nothing() {
    use crate::backend::storage_quota;
    let roots = vec!["b".to_owned(), "a".to_owned()];
    let storage = vec![("a".to_owned(), 1, 10), ("b".to_owned(), 5, 20)];
    let quota = storage_quota(&roots, &storage).expect("b");
    assert_eq!((quota.used_bytes, quota.limit_bytes), (5 * 1024, 20 * 1024));
    assert_eq!(storage_quota(&[], &[("x".to_owned(), 1, 0)]), None);
    assert_eq!(storage_quota(&roots, &[]), None);
}

/// Stage 20: a full mailbox is typed, so the app can say what to do.
#[tokio::test]
async fn overquota_is_a_full_mailbox() {
    let tagged = crate::command::Tagged::for_test(
        crate::command::TaggedStatus::No,
        "[OVERQUOTA] Quota exceeded",
    );
    assert!(matches!(
        tagged.check("APPEND"),
        Err(BackendError::OverQuota(text)) if text.contains("APPEND")
    ));
    let other =
        crate::command::Tagged::for_test(crate::command::TaggedStatus::No, "[TRYCREATE] no");
    assert!(matches!(
        other.check("APPEND"),
        Err(BackendError::Rejected(_))
    ));
}
