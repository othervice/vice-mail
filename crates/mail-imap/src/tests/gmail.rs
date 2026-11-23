//! Stage 14, block A (decision 0034): Gmail over IMAP against recorded Gmail responses. There is
//! no Gmail test server; the lines below follow what imap.gmail.com sends (capabilities,
//! EXAMINE, FETCH with `X-GM-MSGID`/`X-GM-LABELS`, STORE of labels). What only a real account
//! can prove is listed in decision 0034.

use mail_model::{Flag, MailWriter, StoreMode};

use super::*;
use crate::ImapWriter;

const GMAIL_GREETING: Step = Send("* OK Gimap ready for requests from 192.0.2.1 x1mb\r\n");
const GMAIL_CAPS: Step = Send(
    "{tag} OK [CAPABILITY IMAP4rev1 UNSELECT IDLE NAMESPACE QUOTA ID XLIST CHILDREN X-GM-EXT-1 \
UIDPLUS COMPRESS=DEFLATE ENABLE MOVE CONDSTORE ESEARCH UTF8=ACCEPT LIST-EXTENDED LIST-STATUS \
LITERAL- SPECIAL-USE APPENDLIMIT=35651584] alice@gmail.com authenticated (Success)\r\n",
);
const GMAIL_ENABLED: Step = Send("* ENABLED CONDSTORE\r\n{tag} OK Success\r\n");
const GMAIL_OPENED: &str = "* FLAGS (\\Answered \\Flagged \\Draft \\Deleted \\Seen $NotPhishing $Phishing)\r\n\
* OK [PERMANENTFLAGS (\\Answered \\Flagged \\Draft \\Deleted \\Seen $NotPhishing $Phishing \\*)] Flags permitted.\r\n\
* OK [UIDVALIDITY 3] UIDs valid.\r\n* 2 EXISTS\r\n* 0 RECENT\r\n\
* OK [UIDNEXT 12] Predicted next UID.\r\n* OK [HIGHESTMODSEQ 4223]\r\n";

/// Two messages of the inbox. Gmail leaves the label of the folder they were fetched from out
/// of `X-GM-LABELS` on some servers (here: no `\Inbox`); the store adds it back.
const HEADERS: &str = "* 1 FETCH (X-GM-MSGID 1278455344230334865 X-GM-LABELS (\\Important \"Muy Importante\" Work/&AMQ-rger) \
UID 10 MODSEQ (4100) FLAGS (\\Seen) RFC822.SIZE 2048 INTERNALDATE \"01-Sep-2026 12:00:00 +0000\" \
ENVELOPE (\"Tue, 01 Sep 2026 13:59:00 +0200\" \"Hello\" ((NIL NIL \"bob\" \"example.org\")) NIL NIL \
((NIL NIL \"alice\" \"gmail.com\")) NIL NIL NIL \"<one@example.org>\") \
BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" 10 1 NIL NIL NIL NIL) \
BODY[HEADER.FIELDS (MESSAGE-ID)] {33}\r\nMessage-ID: <one@example.org>\r\n\r\n)\r\n\
* 2 FETCH (UID 11 X-GM-MSGID 1278455344230334866 X-GM-LABELS () MODSEQ (4200) FLAGS () RFC822.SIZE 10 \
INTERNALDATE \"02-Sep-2026 12:00:00 +0000\" ENVELOPE (NIL \"No id\" NIL NIL NIL NIL NIL NIL NIL NIL) \
BODYSTRUCTURE (\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 10 1 NIL NIL NIL NIL))\r\n\
{tag} OK Success\r\n";

async fn gmail(access: Access, rest: Vec<Step>) -> Fake {
    let open = if access == Access::ReadOnly {
        Expect("EXAMINE \"INBOX\" (CONDSTORE)")
    } else {
        Expect("SELECT \"INBOX\" (CONDSTORE)")
    };
    let opened: &'static str = if access == Access::ReadOnly {
        Box::leak(
            format!("{GMAIL_OPENED}{{tag}} OK [READ-ONLY] INBOX selected. (Success)\r\n")
                .into_boxed_str(),
        )
    } else {
        Box::leak(
            format!("{GMAIL_OPENED}{{tag}} OK [READ-WRITE] INBOX selected. (Success)\r\n")
                .into_boxed_str(),
        )
    };
    let mut script = vec![
        GMAIL_GREETING,
        LOGIN,
        GMAIL_CAPS,
        Expect("ENABLE CONDSTORE"),
        GMAIL_ENABLED,
        open,
        Send(opened),
    ];
    script.extend(rest);
    connect(access, Duration::from_secs(2), script).await
}

#[tokio::test]
async fn gmail_headers_and_flags_carry_message_ids_and_labels_read_only() {
    let (backend, handle) = gmail(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 10:11 (UID FLAGS RFC822.SIZE"),
            Send(HEADERS),
            Expect("UID FETCH 1:11 (UID FLAGS MODSEQ X-GM-LABELS) (CHANGEDSINCE 4000)"),
            Send(
                "* 1 FETCH (X-GM-LABELS (\\Inbox \\Starred) UID 10 MODSEQ (4300) FLAGS (\\Seen \\Flagged))\r\n\
{tag} OK Success\r\n",
            ),
        ],
    )
    .await;
    let mut backend = backend.expect("login");
    assert!(backend.capabilities().gmail);
    let inbox = backend.open_mailbox("INBOX").await.expect("examine");
    assert!(inbox.read_only);

    let mut headers = backend
        .fetch_headers(&inbox, &UidSet::range(10, 11))
        .await
        .expect("headers");
    headers.sort_by_key(|h| h.uid);
    let [one, two] = headers.as_slice() else {
        panic!("two headers: {headers:?}");
    };
    let info = one.gmail.as_ref().expect("gmail info");
    assert_eq!(info.msgid, 1_278_455_344_230_334_865);
    assert_eq!(
        info.labels,
        ["\\Important", "Muy Importante", "Work/Ärger"],
        "decoded from modified UTF-7, system labels first"
    );
    assert_eq!(one.message_id.as_deref(), Some("one@example.org"));
    let info = two.gmail.as_ref().expect("gmail info without Message-ID");
    assert_eq!(info.msgid, 1_278_455_344_230_334_866);
    assert!(info.labels.is_empty());
    assert!(two.message_id.is_none());

    let updates = backend
        .fetch_flags(&inbox, &UidSet::range(1, 11), Some(ModSeq(4000)))
        .await
        .expect("flags");
    let [update] = updates.as_slice() else {
        panic!("one update: {updates:?}");
    };
    assert_eq!(
        update.gmail_labels.as_deref(),
        Some(["\\Inbox".to_owned(), "\\Starred".to_owned()].as_slice())
    );
    assert!(update.flags.flagged);

    // Labels are data items: reading them is allowed on the read-only connection; changing
    // them is a STORE and never leaves the adapter.
    for line in [
        "UID STORE 10 +X-GM-LABELS (\\Inbox)",
        "UID STORE 10 -X-GM-LABELS (\"Work\")",
        "STORE 1 +X-GM-LABELS.SILENT (foo)",
    ] {
        let result = backend.conn_mut().run(line, |_| false).await;
        assert!(
            matches!(result, Err(BackendError::ReadOnly(_))),
            "{line}: {result:?}"
        );
    }
    let lines = finish(backend, handle).await;
    let header_fetch = lines
        .iter()
        .find(|l| l.contains("UID FETCH 10:11"))
        .expect("header fetch");
    assert!(
        header_fetch.ends_with(" MODSEQ X-GM-MSGID X-GM-LABELS)"),
        "{header_fetch}"
    );
    assert!(
        header_fetch.contains("BODY.PEEK[") && !header_fetch.contains("BODY["),
        "{header_fetch}"
    );
    assert!(lines.iter().all(|l| !l.contains(" STORE ")), "{lines:?}");
}

#[tokio::test]
async fn other_servers_are_not_asked_for_gmail_items() {
    let (mut backend, inbox, handle) = opened(
        Access::ReadOnly,
        vec![
            Expect("UID FETCH 1:* (UID FLAGS MODSEQ) (CHANGEDSINCE 900)"),
            Send("{tag} OK done\r\n"),
        ],
    )
    .await;
    assert!(!backend.capabilities().gmail);
    backend
        .fetch_flags(&inbox, &UidSet::all(), inbox.highest_modseq)
        .await
        .expect("flags");
    let lines = finish(backend, handle).await;
    assert!(lines.iter().all(|l| !l.contains("X-GM-")), "{lines:?}");
}

#[tokio::test]
async fn labels_are_stored_only_on_the_writer_and_encoded_like_mailbox_names() {
    let (backend, handle) = gmail(
        Access::ReadWrite,
        vec![
            Expect("UID STORE 10:11 -X-GM-LABELS (\\Inbox)"),
            Send(
                "* 1 FETCH (X-GM-LABELS (\\Important) UID 10)\r\n\
* 2 FETCH (X-GM-LABELS () UID 11)\r\n{tag} OK Success\r\n",
            ),
            Expect("UID STORE 10 +X-GM-LABELS (\"Work/&AMQ-rger\" \"Muy Importante\" \\Starred)"),
            Send("{tag} OK Success\r\n"),
            Expect("UID STORE 10 +X-GM-LABELS (\"Nope\")"),
            Send("{tag} NO [CANNOT] Invalid label\r\n"),
        ],
    )
    .await;
    let mut writer = ImapWriter::new(backend.expect("login")).expect("read-write");
    let inbox = writer.open_mailbox("INBOX").await.expect("select");

    // Archive on Gmail: the inbox label goes, not a MOVE to All Mail.
    let out = writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([10, 11]),
            StoreMode::Remove,
            &[Flag::label("\\inbox").expect("label")],
            None,
        )
        .await
        .expect("archive");
    assert!(out.updates.is_empty() && out.modified.is_empty());
    writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([10]),
            StoreMode::Add,
            &[
                Flag::label("Work/Ärger").expect("label"),
                Flag::label("Muy Importante").expect("label"),
                Flag::label("\\Starred").expect("label"),
            ],
            None,
        )
        .await
        .expect("add labels");
    // A refusal is typed and keeps the connection.
    let err = writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([10]),
            StoreMode::Add,
            &[Flag::Label("Nope".into())],
            None,
        )
        .await
        .expect_err("NO");
    assert!(matches!(err, BackendError::Rejected(_)), "{err}");

    // Nothing of these reaches the server.
    let refused: [(&[Flag], Option<ModSeq>); 4] = [
        (&[Flag::Label("a\r\nA9 DELETE \"INBOX\"".into())], None),
        (&[Flag::Label("[Gmail]/All Mail".into())], None),
        (&[Flag::Label("Work".into()), Flag::Seen], None),
        (&[Flag::Label("Work".into())], Some(ModSeq(4223))),
    ];
    for (flags, since) in refused {
        let err = writer
            .store_flags(
                &inbox,
                &UidSet::from_uids([10]),
                StoreMode::Add,
                flags,
                since,
            )
            .await
            .expect_err("refused before sending");
        assert!(
            matches!(err, BackendError::InvalidState(_)),
            "{flags:?}: {err}"
        );
    }
    let err = writer
        .append(
            "INBOX",
            &[Flag::Label("Work".into())],
            b"Subject: x\r\n\r\nx\r\n",
        )
        .await
        .expect_err("no labels in APPEND");
    assert!(matches!(err, BackendError::InvalidState(_)), "{err}");
    drop(writer);
    let lines = handle.await.expect("fake server");
    let stores: Vec<&String> = lines.iter().filter(|l| l.contains(" STORE ")).collect();
    assert_eq!(stores.len(), 3, "{lines:?}");
    assert!(
        lines
            .iter()
            .all(|l| !l.contains("MOVE") && !l.contains("APPEND"))
    );
}

#[tokio::test]
async fn labels_need_gmail() {
    let script = vec![
        GREETING,
        LOGIN,
        LOGIN_OK,
        ENABLE,
        ENABLE_OK,
        Expect("SELECT \"INBOX\" (CONDSTORE)"),
        Send(
            "* 3 EXISTS\r\n* OK [UIDVALIDITY 77] ok\r\n* OK [UIDNEXT 40] ok\r\n\
* OK [HIGHESTMODSEQ 900] ok\r\n{tag} OK [READ-WRITE] SELECT completed\r\n",
        ),
    ];
    let (backend, handle) = connect(Access::ReadWrite, Duration::from_secs(2), script).await;
    let mut writer = ImapWriter::new(backend.expect("login")).expect("read-write");
    let inbox = writer.open_mailbox("INBOX").await.expect("select");
    let err = writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([1]),
            StoreMode::Remove,
            &[Flag::Label("\\Inbox".into())],
            None,
        )
        .await
        .expect_err("not Gmail");
    assert!(matches!(err, BackendError::Unsupported(_)), "{err}");
    drop(writer);
    let lines = handle.await.expect("fake server");
    assert!(lines.iter().all(|l| !l.contains("X-GM-")), "{lines:?}");
}

#[test]
fn read_only_allowlist_and_settings_sync_scope_know_gmail_items() {
    use crate::command::{FolderScope, permit};
    for line in [
        "UID FETCH 1:5 (UID FLAGS MODSEQ X-GM-LABELS) (CHANGEDSINCE 7)",
        "UID FETCH 1:5 (UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)] X-GM-MSGID X-GM-LABELS)",
    ] {
        assert_eq!(permit(Access::ReadOnly, line), Ok(()), "{line}");
    }
    let scope = FolderScope::new("Vice Mail Sync").expect("scope");
    for (line, name) in [
        ("UID STORE 1 +X-GM-LABELS (\\Inbox)", "UID STORE"),
        ("UID STORE 1 -X-GM-LABELS (\"Vice Mail Sync\")", "UID STORE"),
        ("STORE 1 +X-GM-LABELS (foo)", "STORE"),
    ] {
        assert_eq!(
            permit(Access::ReadOnly, line),
            Err(BackendError::ReadOnly(name.to_owned())),
            "{line}"
        );
        assert!(scope.check_line(line).is_err(), "{line}");
        assert_eq!(permit(Access::ReadWrite, line), Ok(()), "{line}");
    }
}
