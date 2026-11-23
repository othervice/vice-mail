//! `.eml` files through `AppCore` (decision 0027): parsing, limits and typed errors, the
//! reading pipeline (sanitised HTML, blocked external content, link check, attachments), the
//! register of opened files, and that nothing reaches the store.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use mail_model::{
    BodyFormat, CoreError, EmlFileId, ErrorCode, LinkWarning, MAX_EML_FILE_BYTES,
    MAX_OPEN_EML_FILES, MessageBody, OpenAttachmentResult, RenderToken, TextSegment,
};
use tempfile::TempDir;

use super::super::tests::{Harness, binary_attachment, counting_image_server};
use super::{display_name, is_message, read_file};
use crate::reading::RemoteConfig;
use crate::sync::fake::FakeServer;

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write file");
    path
}

fn html_mail(image_port: u16) -> String {
    format!(
        "From: Grace Hopper <grace@example.org>\r\nTo: Ada <ada@example.org>\r\n\
         Bcc: archive@example.org\r\nSubject: =?UTF-8?Q?Gr=C3=BC=C3=9Fe?= from a file\r\n\
         Date: Mon, 23 Nov 2026 08:00:00 +0100\r\nMessage-ID: <file-1@example.org>\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
         <p onclick=\"steal()\">Hello <a href=\"https://evil.example/login\">https://bank.example</a>\
         <img src=\"http://127.0.0.1:{image_port}/pixel.png\"><script>alert(1)</script></p>\r\n"
    )
}

fn token_of(body: &MessageBody) -> RenderToken {
    match body {
        MessageBody::Html { render_token, .. } | MessageBody::Text { render_token, .. } => {
            render_token.clone()
        }
        other => panic!("no rendering: {other:?}"),
    }
}

fn code(err: &CoreError) -> ErrorCode {
    err.code()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opened_files_go_through_the_reading_pipeline_and_never_into_the_store() {
    let (port, connections) = counting_image_server().await;
    let server = FakeServer::new();
    let h = Harness::new(
        &server,
        RemoteConfig {
            allow_loopback: true,
        },
    )
    .await;
    let dir = TempDir::new().expect("dir");
    let path = write(dir.path(), "Grüße.eml", html_mail(port).as_bytes());

    let file = h.core.open_eml_file(path).await.expect("open");
    assert_eq!(file.name, "Grüße.eml");
    let view = h
        .core
        .get_eml_message(file.id.clone(), BodyFormat::Auto)
        .await
        .expect("view");
    assert_eq!(view.file_id, file.id);
    assert_eq!(view.file_name, "Grüße.eml");
    assert_eq!(view.header.subject, "Grüße from a file");
    assert_eq!(view.header.from[0].email, "grace@example.org");
    assert_eq!(view.header.to[0].email, "ada@example.org");
    assert_eq!(view.header.bcc[0].email, "archive@example.org");
    assert_eq!(view.header.date, 1_795_417_200_000);
    assert_eq!(view.formats, [BodyFormat::Html, BodyFormat::Text]);
    let MessageBody::Html {
        srcdoc,
        render_token,
        blocked_remote,
        remote_loaded,
    } = &view.body
    else {
        panic!("html expected: {:?}", view.body);
    };
    // The sanitiser and the frame's CSP, as for stored mail.
    assert!(srcdoc.contains("default-src 'none'"), "{srcdoc}");
    assert!(!srcdoc.contains("<script"), "{srcdoc}");
    assert!(!srcdoc.contains("onclick"), "{srcdoc}");
    assert!(
        !srcdoc.contains("evil.example"),
        "the real target stays in the core"
    );
    assert_eq!(*blocked_remote, 1);
    assert!(!remote_loaded);
    assert_eq!(connections.load(Ordering::SeqCst), 0, "nothing loaded");

    // Links: the placeholder is checked and opened by the core only.
    assert!(h.core.link_activated(render_token.clone(), 0));
    let link = h.core.describe_link(render_token, 0).expect("describe");
    assert_eq!(link.target, "https://evil.example/login");
    assert!(link.warnings.contains(&LinkWarning::DomainMismatch {
        shown_domain: "bank.example".into(),
        target_domain: "evil.example".into()
    }));
    assert_eq!(
        h.core.open_link(render_token, 0).await.expect("open link"),
        mail_model::LinkOpened::Opened
    );
    assert_eq!(h.opener.opened(), ["https://evil.example/login"]);

    // External content only when asked, once, loaded by the core.
    let loaded = h
        .core
        .load_eml_remote_content(file.id.clone())
        .await
        .expect("load");
    assert!(matches!(
        loaded.body,
        MessageBody::Html {
            remote_loaded: true,
            ..
        }
    ));
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    let again = h
        .core
        .get_eml_message(file.id.clone(), BodyFormat::Auto)
        .await
        .expect("view");
    assert!(
        matches!(
            again.body,
            MessageBody::Html {
                remote_loaded: false,
                blocked_remote: 1,
                ..
            }
        ),
        "never remembered"
    );

    // The text view of the same file.
    let text = h
        .core
        .get_eml_message(file.id.clone(), BodyFormat::Text)
        .await
        .expect("text");
    let MessageBody::Text { segments, .. } = &text.body else {
        panic!("text expected");
    };
    assert!(
        segments
            .iter()
            .any(|s| matches!(s, TextSegment::Link { .. }))
    );

    // Nothing reached the store: no account, no message.
    assert!(h.core.list_accounts().await.expect("accounts").is_empty());
    assert!(
        h.core
            .get_messages_by_ids(vec![mail_model::MessageId(1)])
            .await
            .expect("by ids")
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_of_a_file_are_saved_exactly_and_opened_only_after_confirmation() {
    let (bytes, base64) = binary_attachment();
    let raw = format!(
        "From: a@example.org\r\nSubject: Files\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
         --m\r\nContent-Type: text/plain\r\n\r\nTwo files.\r\n\
         --m\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"../../etc/Daten\u{202e}fdp.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{base64}\r\n\
         --m\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"setup.exe\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{base64}\r\n--m--\r\n"
    );
    let server = FakeServer::new();
    let h = Harness::new(&server, RemoteConfig::default()).await;
    let dir = TempDir::new().expect("dir");
    let file = h
        .core
        .open_eml_file(write(dir.path(), "files.eml", raw.as_bytes()))
        .await
        .expect("open");
    let view = h
        .core
        .get_eml_message(file.id.clone(), BodyFormat::Auto)
        .await
        .expect("view");
    assert_eq!(view.attachments.len(), 2);
    let name = &view.attachments[0].name;
    assert!(!name.contains('/') && !name.contains('\u{202e}'), "{name}");
    assert!(view.attachments[1].executable);
    assert_eq!(
        h.core
            .eml_attachment_name(file.id.clone(), 0)
            .await
            .expect("name"),
        *name
    );

    let target = dir.path().join("saved.bin");
    let saved = h
        .core
        .save_eml_attachment_to(file.id.clone(), 0, target.clone())
        .await
        .expect("save");
    assert_eq!(saved, target);
    assert_eq!(std::fs::read(&target).expect("read"), bytes);

    assert_eq!(
        h.core
            .open_eml_attachment(file.id.clone(), 1, false)
            .await
            .expect("ask"),
        OpenAttachmentResult::NeedsConfirmation
    );
    assert!(
        h.opener.opened().is_empty(),
        "nothing opened before the confirmation"
    );
    assert_eq!(
        h.core
            .open_eml_attachment(file.id.clone(), 1, true)
            .await
            .expect("open"),
        OpenAttachmentResult::Opened
    );
    let opened = h.opener.opened();
    assert_eq!(opened.len(), 1);
    let copy = PathBuf::from(&opened[0]);
    assert!(copy.starts_with(&h.open_dir), "{copy:?}");
    assert!(
        std::fs::metadata(&copy)
            .expect("copy")
            .permissions()
            .readonly()
    );
    assert_eq!(std::fs::read(&copy).expect("copy"), bytes);

    let err = h
        .core
        .open_eml_attachment(file.id.clone(), 7, false)
        .await
        .expect_err("no such attachment");
    assert_eq!(code(&err), ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_that_cannot_be_shown_are_refused_with_typed_errors() {
    let server = FakeServer::new();
    let h = Harness::new(&server, RemoteConfig::default()).await;
    let dir = TempDir::new().expect("dir");
    let cases: Vec<(PathBuf, ErrorCode)> = vec![
        (write(dir.path(), "empty.eml", b""), ErrorCode::EmlInvalid),
        (
            write(
                dir.path(),
                "text.eml",
                b"just some text\nwithout any header\n",
            ),
            ErrorCode::EmlInvalid,
        ),
        (
            write(dir.path(), "binary.eml", &[0, 159, 146, 150, 255, 0, 1, 2]),
            ErrorCode::EmlInvalid,
        ),
        (dir.path().join("missing.eml"), ErrorCode::EmlUnreadable),
        (dir.path().to_path_buf(), ErrorCode::EmlUnreadable),
    ];
    for (path, expected) in cases {
        let err = h
            .core
            .open_eml_file(path.clone())
            .await
            .expect_err("refused");
        assert_eq!(code(&err), expected, "{path:?}: {err}");
        assert!(err.detail().is_some());
    }
    // Larger than the limit: refused from the size, without reading (a sparse file).
    let large = dir.path().join("large.eml");
    let file = std::fs::File::create(&large).expect("create");
    file.set_len(u64::from(MAX_EML_FILE_BYTES) + 1)
        .expect("size");
    drop(file);
    let err = h.core.open_eml_file(large).await.expect_err("too large");
    assert_eq!(code(&err), ErrorCode::EmlTooLarge);
    assert!(
        err.detail()
            .is_some_and(|d| d.contains(&MAX_EML_FILE_BYTES.to_string()))
    );
    // Exactly the limit is read.
    let exact = dir.path().join("exact.eml");
    let file = std::fs::File::create(&exact).expect("create");
    file.set_len(u64::from(MAX_EML_FILE_BYTES)).expect("size");
    drop(file);
    assert_eq!(
        read_file(&exact).expect("read").len(),
        MAX_EML_FILE_BYTES as usize
    );
    // Nothing of the refused files is remembered.
    assert_eq!(h.core.reading_ref().expect("reading").eml.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_from_the_system_keep_their_error_for_their_window() {
    let server = FakeServer::new();
    let h = Harness::new(&server, RemoteConfig::default()).await;
    let dir = TempDir::new().expect("dir");
    let bad = h
        .core
        .open_eml_from_system(write(dir.path(), "notes.eml", b"no mail here"))
        .await
        .expect("remembered");
    assert_eq!(bad.name, "notes.eml");
    let err = h
        .core
        .get_eml_message(bad.id.clone(), BodyFormat::Auto)
        .await
        .expect_err("the error");
    assert_eq!(code(&err), ErrorCode::EmlInvalid);
    let good = h
        .core
        .open_eml_from_system(write(
            dir.path(),
            "plain.eml",
            b"From: a@example.org\r\nSubject: Plain\r\n\r\nHello\r\n",
        ))
        .await
        .expect("opened");
    let view = h
        .core
        .get_eml_message(good.id, BodyFormat::Auto)
        .await
        .expect("view");
    assert_eq!(view.header.subject, "Plain");
    assert_eq!(view.header.date, 0, "no Date header");
    assert!(matches!(view.body, MessageBody::Text { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_register_is_bounded_and_closing_forgets_the_file_and_its_links() {
    let server = FakeServer::new();
    let h = Harness::new(&server, RemoteConfig::default()).await;
    let dir = TempDir::new().expect("dir");
    let path = write(
        dir.path(),
        "link.eml",
        b"From: a@example.org\r\nSubject: Link\r\n\r\nSee https://example.org/page\r\n",
    );
    let first = h.core.open_eml_file(path.clone()).await.expect("open");
    let view = h
        .core
        .get_eml_message(first.id.clone(), BodyFormat::Auto)
        .await
        .expect("view");
    let token = token_of(&view.body);
    assert!(h.core.describe_link(&token, 0).is_ok());

    assert!(h.core.close_eml_file(&first.id));
    assert!(!h.core.close_eml_file(&first.id), "only once");
    let err = h
        .core
        .get_eml_message(first.id.clone(), BodyFormat::Auto)
        .await
        .expect_err("closed");
    assert_eq!(code(&err), ErrorCode::NotFound);
    let err = h
        .core
        .describe_link(&token, 0)
        .expect_err("links forgotten");
    assert_eq!(code(&err), ErrorCode::NotFound);

    let mut ids = Vec::new();
    for _ in 0..=MAX_OPEN_EML_FILES {
        ids.push(h.core.open_eml_file(path.clone()).await.expect("open").id);
    }
    assert_eq!(
        h.core.reading_ref().expect("reading").eml.len(),
        MAX_OPEN_EML_FILES as usize
    );
    let oldest = h
        .core
        .get_eml_message(ids[0].clone(), BodyFormat::Auto)
        .await
        .expect_err("oldest forgotten");
    assert_eq!(code(&oldest), ErrorCode::NotFound);
    let newest = ids.last().expect("ids").clone();
    assert!(
        h.core
            .get_eml_message(newest, BodyFormat::Auto)
            .await
            .is_ok()
    );
    assert_ne!(ids[1], ids[2], "every opening gets its own id");

    let unknown = EmlFileId::from_random(42);
    let err = h
        .core
        .get_eml_message(unknown, BodyFormat::Auto)
        .await
        .expect_err("unknown");
    assert_eq!(code(&err), ErrorCode::NotFound);
}

#[test]
fn names_and_the_mail_check() {
    let name = display_name(Path::new("/tmp/Re: \u{202e}Hi?.eml"));
    assert!(
        name.ends_with(".eml") && !name.contains(['/', ':', '\u{202e}']),
        "{name}"
    );
    let check = |raw: &[u8]| is_message(&mail_content::parse(raw));
    assert!(check(b"Subject: only a subject\r\n\r\nbody"));
    assert!(check(b"Message-ID: <a@b>\r\n\r\n"));
    assert!(!check(b""));
    assert!(!check(b"X-Custom: something\r\n\r\nbody"));
    assert!(!check(b"%PDF-1.7 binary"));
}
