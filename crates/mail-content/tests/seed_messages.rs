//! Acceptance (stage 3, parser level): broken mails from the seed lead to a readable display or
//! a clear problem report, never to a crash; charsets, encoded words, structures and file names
//! of the seed are decoded correctly.
//!
//! The fixtures in `testmail/corpus/seed/` are the `broken/`, `charset/`, `mime/`, `html/`,
//! `list/` and `auto/` messages of `testmail/seed.py`, written with
//! `python3 testmail/seed.py 145 --dump DIR` and copied from `DIR/alice/`. The ignored test
//! `seed_fixtures_are_current` compares them with a fresh dump.

mod common;

use std::fs;
use std::path::Path;

use common::{Mode, eml_files, file_name, violations};
use mail_content::{
    MAIL_CSP, ParseProblem, ParsedMessage, RenderOptions, attachment_bytes, check_link, parse,
    render_html, render_text,
};

fn load(name: &str) -> (Vec<u8>, ParsedMessage) {
    let raw = fs::read(common::corpus_dir("seed").join(name)).expect("read fixture");
    let message = parse(&raw);
    (raw, message)
}

#[test]
fn every_seed_message_is_readable_or_explained() {
    let files = eml_files("seed");
    assert!(files.len() >= 70, "seed fixtures missing: {}", files.len());
    let broken = files
        .iter()
        .filter(|p| file_name(p).starts_with("broken_"))
        .count();
    assert_eq!(broken, 26, "the seed has 26 broken messages");
    for path in &files {
        let name = file_name(path);
        let raw = fs::read(path).expect("read fixture");
        let message = parse(&raw);
        assert!(
            !message.problems.contains(&ParseProblem::Internal),
            "{name}: parser failure"
        );
        let readable =
            message.has_body() || !message.problems.is_empty() || message.headers.subject.is_some();
        assert!(readable, "{name}: neither content nor a problem report");
        assert!(
            message.preview.chars().count() <= mail_content::PREVIEW_CHARS,
            "{name}"
        );
        assert!(
            !message.preview.contains(['\n', '\r', '\u{0}']),
            "{name}: preview not one line"
        );
        // The text view and the HTML view work for every message.
        let text = message.display_text();
        let rendered = render_text(&text);
        assert!(!rendered.lines.is_empty(), "{name}");
        if let Some(html) = &message.html {
            let rendered = render_html(
                html,
                &RenderOptions {
                    inline: message.inline_images(),
                    ..RenderOptions::new(common::token())
                },
            );
            assert_eq!(
                rendered.fallback, None,
                "{name}: self-check fell back to text"
            );
            let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
            assert!(found.is_empty(), "{name}: {found:#?}");
        }
        for attachment in &message.attachments {
            let problems = common::harmless_file_name_problems(&attachment.file_name);
            assert!(
                problems.is_empty(),
                "{name}: {:?} {problems:?}",
                attachment.file_name
            );
            let bytes = attachment_bytes(&raw, attachment.part_id);
            assert!(
                bytes.is_some(),
                "{name}: attachment {} not readable",
                attachment.part_id
            );
        }
    }
}

#[test]
fn broken_messages_report_what_is_wrong() {
    let (_, m) = load("broken_mislabeled-charset.eml");
    assert!(
        m.problems
            .iter()
            .any(|p| matches!(p, ParseProblem::MalformedText { .. })),
        "{:?}",
        m.problems
    );
    assert!(m.text.as_deref().is_some_and(|t| t.contains("aus K")));

    let (_, m) = load("broken_unknown-charset.eml");
    assert!(
        m.problems.contains(&ParseProblem::UnknownCharset {
            part_id: 0,
            charset: "x-made-up-charset".into()
        }),
        "{:?}",
        m.problems
    );

    let (_, m) = load("broken_invalid-base64.eml");
    assert!(
        m.problems
            .iter()
            .any(|p| matches!(p, ParseProblem::TransferEncoding { .. }))
    );
    assert!(m.text.as_deref().is_some_and(|t| t.contains("Hello")));

    let (_, m) = load("broken_unknown-cte.eml");
    assert!(
        m.problems
            .iter()
            .any(|p| matches!(p, ParseProblem::TransferEncoding { .. }))
    );

    let (_, m) = load("broken_bad-date.eml");
    assert_eq!(m.headers.date, None);
    assert!(m.problems.contains(&ParseProblem::InvalidDate));

    let (_, m) = load("broken_missing-date.eml");
    assert_eq!(m.headers.date, None);
    assert!(!m.problems.contains(&ParseProblem::InvalidDate));

    // mail-parser does not read the obsolete form "1 Sep 26 12:00 EST"; it is reported.
    let (_, m) = load("broken_two-digit-year.eml");
    assert!(m.headers.date.is_some() || m.problems.contains(&ParseProblem::InvalidDate));

    let (_, m) = load("broken_missing-headers.eml");
    assert!(m.headers.from.is_empty() && m.headers.subject.is_none());
    assert!(m.text.as_deref().is_some_and(|t| t.contains("No From")));

    let (_, m) = load("broken_headers-only.eml");
    assert_eq!(m.headers.subject.as_deref(), Some("Headers only, no body"));
    assert!(!m.has_body());

    let (raw, m) = load("broken_truncated-base64.eml");
    let cut = m
        .attachments
        .iter()
        .find(|a| a.file_name == "cut.pdf")
        .expect("cut.pdf");
    assert!(attachment_bytes(&raw, cut.part_id).is_some_and(|b| b.starts_with(b"%PDF")));

    let (_, m) = load("broken_deep-nesting.eml");
    assert!(
        m.text
            .as_deref()
            .is_some_and(|t| t.contains("Deepest part."))
    );

    let (_, m) = load("broken_missing-final-boundary.eml");
    assert!(
        m.text
            .as_deref()
            .is_some_and(|t| t.contains("Second part, never closed."))
    );
    assert!(
        m.problems
            .iter()
            .any(|p| matches!(p, ParseProblem::DamagedStructure { .. }))
    );

    let (_, m) = load("broken_wrong-boundary.eml");
    assert!(m.has_body());

    let (_, m) = load("broken_8bit-headers.eml");
    let subject = m.headers.subject.clone().unwrap_or_default();
    assert!(
        subject.starts_with("Unencoded Latin-1 in header"),
        "{subject}"
    );

    let (_, m) = load("broken_bad-encoded-words.eml");
    assert!(m.headers.subject.is_some());

    let (_, m) = load("broken_bad-addresses.eml");
    assert_eq!(m.headers.subject.as_deref(), Some("Broken address lists"));

    let (_, m) = load("broken_long-lines.eml");
    assert!(m.text.as_deref().is_some_and(|t| t.len() > 15_000));
    assert!(m.preview.chars().count() <= 200);

    let (_, m) = load("broken_charset-in-filename.eml");
    assert_eq!(m.attachments[0].file_name, "Größe ✓.bin");

    let (_, m) = load("broken_reference-loop.eml");
    assert!(
        m.headers
            .references
            .contains(&"missing-parent@nowhere.example".to_owned())
    );

    let (_, m) = load("broken_binary-garbage.eml");
    assert!(m.has_body() || !m.problems.is_empty());
}

#[test]
fn charsets_and_encoded_words_are_decoded() {
    let cases = [
        ("charset_big5.eml", "愛麗絲你好"),
        ("charset_euc-jp.eml", "来週の出張"),
        ("charset_euc-kr.eml", "앨리스 님"),
        ("charset_gb2312.eml", "爱丽丝你好"),
        ("charset_iso-2022-jp.eml", "アリス様"),
        ("charset_iso-8859-1.eml", "Frau Müller"),
        ("charset_iso-8859-15.eml", "99 €"),
        ("charset_iso-8859-2.eml", "Zażółć gęślą jaźń"),
        ("charset_koi8-r.eml", "Здравствуйте, Алиса"),
        ("charset_shift_jis.eml", "見積書を添付"),
        (
            "charset_utf-8.eml",
            "family 👨\u{200d}👩\u{200d}👧\u{200d}👦",
        ),
        ("charset_utf-16.eml", "UTF-16 text with BOM: Grüße"),
        ("charset_windows-1251.eml", "Добрый день"),
        ("charset_windows-1252.eml", "„Angebot“"),
    ];
    for (file, expected) in cases {
        let (_, m) = load(file);
        let text = m.text.clone().unwrap_or_default();
        assert!(text.contains(expected), "{file}: {text:?}");
        assert!(!text.contains('\u{FFFD}'), "{file}: replacement characters");
        assert!(
            !m.problems.iter().any(|p| matches!(
                p,
                ParseProblem::UnknownCharset { .. } | ParseProblem::MalformedText { .. }
            )),
            "{file}: {:?}",
            m.problems
        );
        let subject = m.headers.subject.clone().unwrap_or_default();
        assert!(
            !subject.is_empty() && !subject.contains('\u{FFFD}'),
            "{file}: {subject:?}"
        );
    }
    let (_, m) = load("charset_koi8-r.eml");
    assert_eq!(m.headers.subject.as_deref(), Some("Встреча в четверг"));
    let (_, m) = load("charset_rfc2047-mix.eml");
    assert_eq!(
        m.headers.subject.as_deref(),
        Some("Café und Müsli plain text \u{1F47B}€")
    );
    let (_, m) = load("charset_utf8-headers.eml");
    assert_eq!(
        m.headers.subject.as_deref(),
        Some("Raw UTF-8 header (RFC 6532): Größenänderung ✓")
    );
}

#[test]
fn mime_structures_are_understood() {
    let (_, m) = load("mime_alternative.eml");
    assert!(m.text.is_some() && m.html.is_some());

    let (_, m) = load("mime_html-only.eml");
    assert!(m.text.is_none() && m.html.is_some());
    assert!(!m.preview.is_empty());
    // The seed's HTML shows a literal "<br>" (escaped in the source); no markup is left.
    let text = m.display_text();
    assert!(
        text.starts_with("Hi Alice,") && !text.contains("<p") && !text.contains("</"),
        "{text}"
    );

    let (_, m) = load("mime_flowed.eml");
    let text = m.text.unwrap_or_default();
    assert!(
        text.starts_with(
            "This paragraph uses format=flowed soft line breaks that a client should join.\n"
        ),
        "{text:?}"
    );

    let (_, m) = load("mime_related-cid.eml");
    assert_eq!(m.inline.len(), 1);
    assert_eq!(m.inline[0].content_id, "logo@seed");
    let rendered = render_html(
        m.html.as_deref().unwrap_or_default(),
        &RenderOptions {
            inline: m.inline_images(),
            ..RenderOptions::new(common::token())
        },
    );
    assert!(
        rendered
            .srcdoc
            .contains("<img src=\"data:image/png;base64,"),
        "cid image resolved"
    );
    assert!(m.attachments.iter().any(|a| a.file_name == "beilage.pdf"));

    let (_, m) = load("mime_inline-image.eml");
    assert!(
        m.attachments
            .iter()
            .any(|a| a.file_name == "photo.png" && a.inline)
    );

    let (raw, m) = load("mime_mixed-attachments.eml");
    let names: Vec<&str> = m.attachments.iter().map(|a| a.file_name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Rechnung März 2026.pdf",
            "chart.png",
            "Übersicht.csv",
            "kickoff.ics",
            "archive.zip"
        ]
    );
    let pdf = &m.attachments[0];
    assert_eq!(pdf.mime, "application/pdf");
    let bytes = attachment_bytes(&raw, pdf.part_id).expect("pdf bytes");
    assert_eq!(bytes.len() as u64, pdf.size);
    assert!(bytes.starts_with(b"%PDF"));

    let (_, m) = load("mime_filenames.eml");
    let names: Vec<&str> = m.attachments.iter().map(|a| a.file_name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Sehr langer Dateiname mit Umlauten äöü und Sonderzeichen – Protokoll der Mitgliederversammlung vom 12. März 2026 (endgültige Fassung).pdf",
            "📎 Notizen.txt",
            "議事録.txt",
            "plain-ascii.txt"
        ]
    );
    assert!(m.attachments.iter().all(|a| !a.name_changed));

    let (_, m) = load("mime_hostile-filenames.eml");
    let names: Vec<&str> = m.attachments.iter().map(|a| a.file_name.as_str()).collect();
    let long = format!("{}.txt", "x".repeat(196));
    assert_eq!(
        names,
        [
            "evil.sh",
            // Written unescaped in a quoted string: `\W` is a quoted pair (RFC 5322), so the
            // backslashes are gone before sanitising; harmless either way.
            "C_WindowsSystem32evil.bat",
            "_CON",
            "_aux.txt",
            "invoicefdp.exe",
            "tab andbell.txt",
            "trailing dots and spaces",
            long.as_str(),
            "attachment"
        ]
    );
    assert!(m.attachments.iter().all(|a| a.name_changed));
    let executable: Vec<&str> = m
        .attachments
        .iter()
        .filter(|a| a.executable)
        .map(|a| a.file_name.as_str())
        .collect();
    assert_eq!(
        executable,
        ["evil.sh", "C_WindowsSystem32evil.bat", "invoicefdp.exe"]
    );

    let (_, m) = load("mime_forward-rfc822.eml");
    let forwarded = m
        .attachments
        .iter()
        .find(|a| a.mime == "message/rfc822")
        .expect("forwarded");
    assert_eq!(forwarded.file_name, "forwarded.eml");

    let (_, m) = load("mime_reply-to.eml");
    assert_eq!(m.headers.reply_to[0].email, "team@example.org");
    assert_eq!(m.headers.sender[0].email, "assistant@example.org");

    let (_, m) = load("mime_many-recipients.eml");
    assert_eq!(m.headers.to.len(), 40);

    let (_, m) = load("mime_bcc-sent.eml");
    assert_eq!(m.headers.bcc.len(), 1);
}

#[test]
fn lists_and_auto_replies_expose_their_headers() {
    let (_, m) = load("list_rust-users.eml");
    assert_eq!(m.headers.list.post, ["mailto:rust-users@lists.example"]);
    assert_eq!(
        m.headers.list.archive,
        ["https://lists.example/archive/rust-users"]
    );
    assert_eq!(
        m.headers.list.unsubscribe,
        ["mailto:rust-users-leave@lists.example"]
    );
    assert!(m.headers.list.id.is_some());
    assert_eq!(m.headers.precedence.as_deref(), Some("list"));

    let (_, m) = load("html_newsletter-0.eml");
    assert_eq!(m.headers.list.unsubscribe.len(), 2);
    assert_eq!(
        m.headers.list.unsubscribe_post.as_deref(),
        Some("List-Unsubscribe=One-Click")
    );

    let (_, m) = load("auto_out-of-office.eml");
    assert_eq!(m.headers.auto_submitted.as_deref(), Some("auto-replied"));
}

#[test]
fn the_seed_phishing_mail_warns() {
    let (_, m) = load("html_hostile.eml");
    let rendered = render_html(
        m.html.as_deref().unwrap_or_default(),
        &RenderOptions::new(common::token()),
    );
    let warned: Vec<(&str, usize)> = rendered
        .links
        .iter()
        .map(|l| (l.href.as_str(), check_link(&l.text, &l.href).warnings.len()))
        .collect();
    // Text "https://bank.example/login" -> evil.example; javascript:; data:; bänk vs bönk;
    // user info; the onclick link with href="#".
    assert!(warned.iter().all(|(_, w)| *w > 0), "{warned:?}");
    assert_eq!(warned.len(), 6, "{warned:?}");
}

/// Regenerates the seed and compares with the fixtures (needs python3; run with `--ignored`).
#[test]
#[ignore = "needs python3; run when testmail/seed.py changes"]
fn seed_fixtures_are_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("mail-content-seed-{}", std::process::id()));
    let status = std::process::Command::new("python3")
        .arg(root.join("testmail/seed.py"))
        .args(["145", "--dump"])
        .arg(&out)
        .status()
        .expect("run python3");
    assert!(status.success());
    for path in eml_files("seed") {
        let name = file_name(&path);
        let fresh = fs::read(out.join("alice").join(&name)).unwrap_or_default();
        assert_eq!(
            fs::read(&path).expect("fixture"),
            fresh,
            "{name} differs from the seed"
        );
    }
    let _ = fs::remove_dir_all(out);
}
