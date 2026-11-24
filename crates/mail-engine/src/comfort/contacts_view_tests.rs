//! Stage 17, block B (decision 0050) through `AppCore` with the fake server: every address of
//! received mail becomes a contact with its kind, notes, the picture pipeline (encoded, cut,
//! stored, refused when crafted or too large, never decoded here when a helper is configured),
//! a contact's mail, "mark as spam" with the move to junk, read-only accounts and undo, and
//! the settings sync records of what the user set.

use std::path::PathBuf;

use image::codecs::png::PngEncoder;
use image::{Rgba, RgbaImage};
use mail_model::{
    ContactFilter, ContactKind, ErrorCode, IpcError, MAX_CONTACT_PICTURE_FILE_BYTES, MailboxRole,
    PictureCrop, SpamUndo,
};

use super::*;

const TEAM: &[u8] =
    b"From: Carol Clark <carol@work.example>\r\nTo: alice@fake.test, Dave <dave@work.example>\r\n\
Cc: Erin <erin@work.example>\r\nReply-To: Team <team@work.example>\r\nSubject: Team lunch\r\n\
Message-ID: <team-1@work.example>\r\nContent-Type: text/plain\r\n\r\nFriday?\r\n";

const BOUNCE: &[u8] =
    b"From: Mail Delivery System <MAILER-DAEMON@fake.test>\r\nTo: alice@fake.test\r\n\
Subject: Undelivered Mail Returned to Sender\r\nAuto-Submitted: auto-replied\r\n\
Message-ID: <bounce-1@fake.test>\r\nContent-Type: text/plain\r\n\r\nSorry\r\n";

const SPAM_1: &[u8] = b"From: Pushy <offers@spam.example>\r\nTo: alice@fake.test\r\n\
Subject: Win a prize\r\nMessage-ID: <win-1@spam.example>\r\nContent-Type: text/plain\r\n\r\nClick\r\n";

const SPAM_2: &[u8] = b"From: Pushy <offers@spam.example>\r\nTo: alice@fake.test\r\n\
Subject: Win again\r\nMessage-ID: <win-2@spam.example>\r\nContent-Type: text/plain\r\n\r\nClick\r\n";

const SPAM_3: &[u8] = b"From: Pushy <offers@spam.example>\r\nTo: alice@fake.test\r\n\
Subject: Last chance\r\nMessage-ID: <win-3@spam.example>\r\nContent-Type: text/plain\r\n\r\nClick\r\n";

fn code(err: mail_model::CoreError) -> ErrorCode {
    IpcError::from(err).code
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = RgbaImage::from_fn(width, height, |x, _| {
        Rgba([u8::try_from(x % 256).unwrap_or(0), 120, 60, 255])
    });
    let mut out = Vec::new();
    picture
        .write_with_encoder(PngEncoder::new(&mut out))
        .expect("png");
    out
}

fn add_junk(server: &FakeServer) {
    server.with(|s| {
        s.mailboxes.insert(
            "Junk".to_owned(),
            crate::sync::fake::FakeMailbox {
                uid_validity: 901,
                uid_next: 1,
                modseq: 1,
                attributes: vec!["\\Junk".to_owned()],
                messages: std::collections::BTreeMap::new(),
                changes: 0,
            },
        );
    });
}

async fn contact_of(h: &Harness, email: &str) -> mail_model::ContactEntry {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(found) = h.core.get_contact(email.to_owned()).await {
            return found;
        }
        assert!(Instant::now() < deadline, "{email} never became a contact");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_address_of_received_mail_becomes_a_contact_with_its_kind() {
    let mut h = Harness::new(FakeServer::new()).await;
    h.deliver(TEAM).await;
    h.deliver(BOUNCE).await;
    // The sender of a mail from a person, and its Reply-To (from the header block the adapter
    // fetches). The fake server's envelope names only the account's own address as To; To
    // and Cc are covered by `compose::contacts::tests` (planning and backfill).
    let carol = contact_of(&h, "carol@work.example").await;
    assert_eq!((carol.received, carol.kind), (1, ContactKind::Person));
    let team = contact_of(&h, "team@work.example").await;
    assert_eq!(team.kind, ContactKind::Person);
    assert_eq!(team.addresses[0].seen, 1);
    assert!(team.last_used.is_some());
    // A system address is a contact, marked, and left out of the default list and autocomplete.
    let daemon = contact_of(&h, "mailer-daemon@fake.test").await;
    assert_eq!(daemon.kind, ContactKind::System);
    let people = h
        .core
        .list_contacts(None, None, 50, ContactFilter::People)
        .await
        .expect("people");
    assert!(people.items.iter().all(|c| c.kind == ContactKind::Person));
    assert!(!people.items.iter().any(|c| c.key == daemon.key));
    let all = h
        .core
        .list_contacts(None, None, 50, ContactFilter::All)
        .await
        .expect("all");
    assert!(all.items.iter().any(|c| c.key == daemon.key));
    assert!(
        h.core
            .search_contacts("mailer".into(), 10)
            .await
            .expect("autocomplete")
            .is_empty()
    );
    // Searching by domain.
    let by_domain = h
        .core
        .list_contacts(Some("work.example".into()), None, 50, ContactFilter::People)
        .await
        .expect("domain");
    assert_eq!(by_domain.items.len(), 2);
    // The own address is never a contact.
    assert!(h.core.get_contact("alice@fake.test".into()).await.is_err());
    // Mail with the contact, newest first.
    let mail = h
        .core
        .contact_messages("carol@work.example".into(), 10)
        .await
        .expect("mail");
    assert_eq!(mail.items.len(), 1);
    assert_eq!(mail.items[0].subject, "Team lunch");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notes_and_pictures_are_set_shown_removed_and_synchronised() {
    let mut h = Harness::new(FakeServer::new()).await;
    h.deliver(TEAM).await;
    let carol = contact_of(&h, "carol@work.example").await;
    assert_eq!((carol.note.as_deref(), carol.picture), (None, None));

    let noted = h
        .core
        .set_contact_note(
            "Carol@Work.example".into(),
            Some("Met at the fair\r\n".into()),
        )
        .await
        .expect("note");
    assert_eq!(noted.note.as_deref(), Some("Met at the fair"));
    let refused = h
        .core
        .set_contact_note("carol@work.example".into(), Some("x".repeat(1_001)))
        .await
        .expect_err("too long");
    assert_eq!(code(refused), ErrorCode::InvalidArgument);

    // A file the user chose: read by the core, encoded for the cropper, cut to a square.
    let dir = h.dir.path().join("pictures");
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("carol.png");
    std::fs::write(&file, png(1600, 900)).expect("write");
    let source = h.core.load_contact_picture(file).await.expect("source");
    assert_eq!((source.width, source.height), (1024, 576));
    assert!(source.data_url.starts_with("data:image/jpeg;base64,"));
    let with_picture = h
        .core
        .set_contact_picture(
            "carol@work.example".into(),
            source.token.clone(),
            PictureCrop {
                x: 200,
                y: 0,
                size: 576,
            },
        )
        .await
        .expect("picture");
    let revision = with_picture.picture.expect("revision");
    let shown = h
        .core
        .get_contact_picture("carol@work.example".into())
        .await
        .expect("read")
        .expect("picture");
    assert_eq!(shown.revision, revision);
    assert!(shown.data_url.starts_with("data:image/jpeg;base64,/9j/"));
    // The token is used up.
    let expired = h
        .core
        .set_contact_picture(
            "carol@work.example".into(),
            source.token,
            PictureCrop {
                x: 0,
                y: 0,
                size: 10,
            },
        )
        .await
        .expect_err("used");
    assert_eq!(code(expired), ErrorCode::NotFound);
    let refs = h.core.list_contact_pictures().await.expect("refs");
    assert_eq!(refs.len(), 1);
    assert!(refs[0].emails.contains(&"carol@work.example".to_owned()));

    // After a restart the picture and the note are still there.
    let h = h.restart(0).await;
    let again = h
        .core
        .get_contact_picture("carol@work.example".into())
        .await
        .expect("read")
        .expect("kept");
    assert_eq!(again, shown);
    assert_eq!(
        h.core
            .get_contact("carol@work.example".into())
            .await
            .expect("contact")
            .note
            .as_deref(),
        Some("Met at the fair")
    );

    // What the user set travels through the settings sync; what the directory counts does not.
    let store = h.core.store_for_tests().clone();
    let records = store.sync_records().await.expect("records");
    let contact_records: Vec<_> = records
        .iter()
        .filter(|r| r.kind == "contact" || r.kind == "contactPicture")
        .map(|r| (r.kind.as_str(), r.id.as_str(), r.content.clone()))
        .collect();
    assert_eq!(contact_records.len(), 2, "{contact_records:?}");
    assert_eq!(
        contact_records[0],
        (
            "contact",
            "carol@work.example",
            Some(serde_json::json!({ "note": "Met at the fair" }))
        )
    );
    let picture = contact_records[1].2.as_ref().expect("content");
    assert!(
        crate::contacts_view::is_picture_record(picture["jpeg"].as_str().expect("jpeg")),
        "{picture}"
    );
    assert!(
        !records
            .iter()
            .any(|r| r.kind == "contact" && r.id == "dave@work.example"),
        "contacts nobody edited are not synchronised"
    );

    let removed = h
        .core
        .remove_contact_picture("carol@work.example".into())
        .await
        .expect("remove");
    assert_eq!(removed.picture, None);
    assert!(
        h.core
            .get_contact_picture("carol@work.example".into())
            .await
            .expect("read")
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn crafted_large_and_foreign_files_are_refused_with_typed_errors() {
    let h = Harness::new(FakeServer::new()).await;
    let dir = h.dir.path().join("pictures");
    std::fs::create_dir_all(&dir).expect("dir");
    // A PNG header that claims 60,000 × 60,000 pixels.
    let mut crafted = png(1, 1);
    crafted[16..20].copy_from_slice(&60_000u32.to_be_bytes());
    crafted[20..24].copy_from_slice(&60_000u32.to_be_bytes());
    let mut cut = png(40, 40);
    cut.truncate(60);
    let cases: [(&str, Vec<u8>, &[ErrorCode]); 3] = [
        (
            "crafted.png",
            crafted,
            &[ErrorCode::PictureTooLarge, ErrorCode::PictureDamaged],
        ),
        ("cut.png", cut, &[ErrorCode::PictureDamaged]),
        (
            "vector.svg",
            b"<svg/>".to_vec(),
            &[ErrorCode::PictureUnsupported],
        ),
    ];
    for (name, bytes, codes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        let err = h
            .core
            .load_contact_picture(path)
            .await
            .expect_err("refused");
        assert!(codes.contains(&code(err.clone())), "{name}: {err:?}");
    }
    // Over the size limit: refused before it is read.
    let big = dir.join("big.jpg");
    std::fs::File::create(&big)
        .and_then(|f| f.set_len(u64::from(MAX_CONTACT_PICTURE_FILE_BYTES) + 1))
        .expect("sparse file");
    let err = h.core.load_contact_picture(big).await.expect_err("large");
    assert_eq!(code(err), ErrorCode::PictureTooLarge);
    let err = h
        .core
        .load_contact_picture(dir.join("missing.png"))
        .await
        .expect_err("missing");
    assert_eq!(code(err), ErrorCode::PictureUnsupported);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_a_helper_configured_nothing_is_decoded_in_the_app() {
    let dir = TempDir::new().expect("dir");
    let mut services = CoreServices::with_connector(
        Credentials::new(Arc::new(MemorySecretStore::new())),
        FakeConnector(FakeServer::new()),
    );
    services.reading.preview_helper = Some(PathBuf::from("/nonexistent/mail-mcp"));
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    let file = dir.path().join("fine.png");
    std::fs::write(&file, png(20, 20)).expect("write");
    let err = core
        .load_contact_picture(file)
        .await
        .expect_err("no helper");
    assert_eq!(code(err), ErrorCode::PictureUnsupported);
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mark_as_spam_blocks_moves_to_junk_and_undo_restores() {
    let server = FakeServer::new();
    add_junk(&server);
    let mut h = Harness::new(server).await;
    h.writes(true).await;
    h.deliver(SPAM_1).await;
    h.deliver(SPAM_2).await;
    contact_of(&h, "offers@spam.example").await;

    let mark = h
        .core
        .mark_contact_spam("Offers@Spam.example".into())
        .await
        .expect("mark");
    assert_eq!(mark.blocked, ["offers@spam.example"]);
    assert!(mark.contact.blocked);
    assert_eq!(mark.mail.len(), 1);
    assert_eq!((mark.mail[0].count, mark.mail[0].writes_enabled), (2, true));
    let blocked = h.core.list_blocked_senders().await.expect("blocked");
    assert_eq!(blocked[0].address, "offers@spam.example");

    let moved = h
        .core
        .move_contact_mail_to_junk("offers@spam.example".into())
        .await
        .expect("move");
    assert_eq!(moved.moved, 2);
    assert!(moved.kept.is_empty());
    assert_eq!(moved.batches.len(), 1);
    eventually("both in junk on the server", || async {
        h.server.read(|s| {
            s.mailboxes["INBOX"].messages.is_empty() && s.mailboxes["Junk"].messages.len() == 2
        })
    })
    .await;
    // New mail from the address no longer lands in the inbox (stage 16's block).
    h.server.deliver_raw("INBOX", SPAM_3);
    eventually("new mail went to junk", || async {
        h.server.read(|s| {
            s.mailboxes["INBOX"].messages.is_empty() && s.mailboxes["Junk"].messages.len() == 3
        })
    })
    .await;

    // Undo: unblocked, and the two moved messages come back.
    let undone = h
        .core
        .undo_contact_spam(
            "offers@spam.example".into(),
            SpamUndo {
                addresses: mark.blocked.clone(),
                batches: moved.batches.clone(),
            },
        )
        .await
        .expect("undo");
    assert_eq!(undone.not_undone, 0);
    assert!(!undone.contact.blocked);
    assert!(
        h.core
            .list_blocked_senders()
            .await
            .expect("list")
            .is_empty()
    );
    eventually("moved back on the server", || async {
        h.server.read(|s| s.mailboxes["INBOX"].messages.len() == 2)
    })
    .await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    eventually("listed in the inbox again", || async {
        h.core.list_messages(inbox, None, 50).await.is_ok_and(|p| {
            p.items
                .iter()
                .filter(|i| i.subject.starts_with("Win"))
                .count()
                == 2
        })
    })
    .await;
    // The spam mark travelled as blocked-sender records and was taken back.
    let store = h.core.store_for_tests().clone();
    let record = store
        .sync_records()
        .await
        .expect("records")
        .into_iter()
        .find(|r| r.kind == "blockedSender")
        .expect("record");
    assert_eq!(record.id, "offers@spam.example");
    assert_eq!(record.content, None, "a tombstone after undo");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_only_accounts_keep_their_mail_and_say_so() {
    let server = FakeServer::new();
    add_junk(&server);
    let mut h = Harness::new(server).await;
    h.deliver(SPAM_1).await;
    contact_of(&h, "offers@spam.example").await;
    let mark = h
        .core
        .mark_contact_spam("offers@spam.example".into())
        .await
        .expect("mark");
    assert_eq!(mark.mail.len(), 1);
    assert!(!mark.mail[0].writes_enabled);
    let moved = h
        .core
        .move_contact_mail_to_junk("offers@spam.example".into())
        .await
        .expect("move");
    assert_eq!(moved.moved, 0);
    assert!(moved.batches.is_empty());
    assert_eq!(moved.kept.len(), 1);
    assert_eq!(moved.kept[0].count, 1);
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);
    assert_eq!(h.server.read(|s| s.mailboxes["INBOX"].messages.len()), 1);
}
