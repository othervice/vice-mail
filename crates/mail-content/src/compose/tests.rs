//! Tests of the compose module: snapshots of the mail HTML (files in `snapshots/`, refreshed
//! with `UPDATE_SNAPSHOTS=1 cargo test -p mail-content compose`), round trips of built messages
//! through [`crate::parse`], and the reply and forward logic.

use std::fs;
use std::path::PathBuf;

use mail_model::{AccountId, Address, Identity, IdentityId};

use super::*;
use crate::dom::{self, Descend, NodeData, Visit};
use crate::{attachment_bytes, parse};

// ---------------------------------------------------------------------------------------------
// Helpers

fn snapshot(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/compose/snapshots")
        .join(format!("{name}.html"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "{}: {err}; create it with UPDATE_SNAPSHOTS=1",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "snapshot {name} changed; check the difference and accept it with UPDATE_SNAPSHOTS=1"
    );
}

/// Elements and attributes the mail HTML may contain, checked independently of the writer.
fn assert_allowlisted(html: &str) {
    const ELEMENTS: &[&str] = &[
        "html",
        "head",
        "meta",
        "body",
        "div",
        "p",
        "br",
        "strong",
        "em",
        "u",
        "s",
        "ul",
        "ol",
        "li",
        "blockquote",
        "a",
        "pre",
        "code",
    ];
    let lower = html.to_ascii_lowercase();
    for banned in [
        " class=", " id=", "<script", "<img", "<iframe", "<style", "<svg",
    ] {
        assert!(!lower.contains(banned), "{banned:?} in {html}");
    }
    let parsed = dom::parse_document(html);
    dom::walk(&parsed.document, |visit| {
        if let Visit::Enter(node) = visit
            && let NodeData::Element { name, attrs, .. } = &node.data
        {
            let tag = name.local.to_string();
            assert!(ELEMENTS.contains(&tag.as_str()), "element {tag} in {html}");
            for attr in attrs.borrow().iter() {
                let attr_name = attr.name.local.to_string();
                let ok = match (tag.as_str(), attr_name.as_str()) {
                    ("meta", "http-equiv" | "content") => true,
                    ("a", "href") => ["http://", "https://", "mailto:"]
                        .iter()
                        .any(|p| attr.value.starts_with(p)),
                    (_, "style") => tag != "meta",
                    _ => false,
                };
                assert!(ok, "attribute {attr_name}={:?} on {tag}", attr.value);
            }
        }
        Descend::Yes
    });
}

fn address(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.to_owned(),
    }
}

fn identity(id: i64, email: &str, default: bool) -> Identity {
    Identity {
        id: IdentityId(id),
        account_id: "6f1c2b9e-3d4a-4c5b-8e7f-0a1b2c3d4e5f"
            .parse::<AccountId>()
            .expect("account id"),
        email: email.to_owned(),
        name: "Alice".to_owned(),
        reply_to: None,
        is_default: default,
    }
}

fn outgoing() -> OutgoingMessage {
    OutgoingMessage {
        from: address(Some("Alice Example"), "alice@example.org"),
        to: vec![address(Some("Bob"), "bob@example.net")],
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        subject: "Hello".to_owned(),
        text: None,
        html: Some("<p>Hello <strong>Bob</strong></p>".to_owned()),
        attachments: Vec::new(),
        message_id: "0123456789abcdef0123456789abcdef@example.org".to_owned(),
        in_reply_to: None,
        references: Vec::new(),
        // 2026-11-23 13:05:00 UTC
        date: 1_795_439_100,
        user_agent: None,
    }
}

fn header_block(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.split("\r\n\r\n").next().unwrap_or_default().to_owned()
}

/// Top-level header names in order (unfolded).
fn header_names(raw: &[u8]) -> Vec<String> {
    header_block(raw)
        .split("\r\n")
        .filter(|line| !line.starts_with([' ', '\t']))
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.to_owned()))
        .collect()
}

fn options(language: QuoteLanguage) -> QuoteOptions {
    QuoteOptions {
        language,
        utc_offset_minutes: 60,
    }
}

// ---------------------------------------------------------------------------------------------
// Mail HTML snapshots

const EDITOR_BASIC: &str = "<p>Hello <strong>Alice</strong>,</p><p>this is <em>important</em> \
     and <u>underlined</u>, <s>gone</s>, <code>x = 1</code>, <strong><em>both</em></strong>.</p>\
     <p></p><p>Line one<br>line two</p><p></p>";

const EDITOR_STRUCTURE: &str = "<ul><li><p>one</p></li><li><p>two</p><ul><li><p>nested</p></li>\
     </ul></li></ul><ol><li><p>first</p></li><li><p>second</p></li></ol><blockquote><p>quoted</p>\
     <blockquote><p>deeper</p></blockquote></blockquote><pre><code>fn main() {\n    \
     println!(\"&lt;hi&gt; &amp; bye\");\n}</code></pre><p>after</p>";

const EDITOR_LINKS: &str = "<p><a href=\"https://example.com/a?b=1&amp;c=2\" target=\"_blank\" \
     rel=\"noopener noreferrer nofollow\">safe</a> <a href=\"javascript:alert(1)\">js</a> \
     <a href=\"mailto:bob@example.org\">mail</a> <a href=\"/relative\">rel</a> \
     <a href=\"data:text/html,x\">data</a> <a href=\"HTTP://Example.COM/X\">upper</a> \
     <a href=\"https://example.com/&quot;onmouseover=&quot;x\">quote</a> \
     <a href=\"https://a.example\"><strong>bold link</strong></a></p>";

const EDITOR_HOSTILE: &str = "<p class=\"x\" id=\"y\" style=\"color:red\" onclick=\"evil()\">\
     text</p><script>alert(1)</script><style>p{}</style><iframe src=\"https://evil\"></iframe>\
     <img src=\"https://tracker.example/x.png\" alt=\"pic\"><form><input value=\"x\">\
     <button>Go</button><textarea>secret</textarea></form><svg><text>svg</text></svg>\
     <div><h1>Title</h1>div text <span style=\"x\">span</span></div>\
     <table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>\
     <!-- comment --><object data=\"x\"></object><p>end</p>";

const EDITOR_UNICODE: &str =
    "<p>Grüße 👋 aus Köln — „Zitat“ &amp; &lt;tag&gt;&nbsp;Ende   und   Leerzeichen</p>";

#[test]
fn mail_html_snapshots() {
    for (name, input) in [
        ("editor_basic", EDITOR_BASIC),
        ("editor_structure", EDITOR_STRUCTURE),
        ("editor_links", EDITOR_LINKS),
        ("editor_hostile", EDITOR_HOSTILE),
        ("editor_unicode", EDITOR_UNICODE),
        ("editor_empty", ""),
        ("editor_plain_text_input", "just text\nwith a newline"),
    ] {
        let html = mail_html(input);
        assert_allowlisted(&html);
        snapshot(name, &html);
    }
}

#[test]
fn mail_html_is_stable_when_converted_again() {
    // The editor may load our own output again (a draft reopened): no growth, same content.
    for input in [EDITOR_BASIC, EDITOR_STRUCTURE, EDITOR_LINKS, EDITOR_UNICODE] {
        let once = mail_html(input);
        assert_eq!(mail_html(&once), once);
    }
}

#[test]
fn deep_or_huge_html_does_not_overflow() {
    let deep = "<blockquote>".repeat(5_000) + "inner" + &"</blockquote>".repeat(5_000);
    let html = mail_html(&deep);
    assert!(html.contains("inner"));
    let lists = "<ul><li>".repeat(400) + "x" + &"</li></ul>".repeat(400);
    let html = mail_html(&lists);
    assert!(html.contains('x'));
    assert!(html.matches("<ul").count() <= 48);
    let many = "<b>".repeat(200_000);
    let html = mail_html(&many);
    assert_allowlisted(&html);
}

// ---------------------------------------------------------------------------------------------
// Building and round trips

#[test]
fn round_trip_keeps_umlauts_emoji_names_and_file_names() {
    let pdf: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
    let text_file = "Zeile 1\r\nZeile 2 mit Ümlaut\r\n".as_bytes().to_vec();
    let mut message = outgoing();
    message.from = address(Some("Jörg Müller 🦀"), "joerg@example.org");
    message.to = vec![
        address(Some("Zoë Ångström"), "zoe@example.net"),
        address(None, "plain@example.net"),
    ];
    message.cc = vec![address(Some("Ünal, \"Quote\" Çelik"), "unal@example.net")];
    message.subject =
        "Grüße aus Köln 👋 – ein sehr langer Betreff, damit er gefaltet werden muss, \
                       mit Emoji 🎉 am Ende"
            .to_owned();
    message.html = Some("<p>Hallo Zoë,</p><p>anbei die Unterlagen 📎.</p>".to_owned());
    message.attachments = vec![
        Attachment {
            file_name: "Jahresabschluß 2026 – endgültig 📊.pdf".to_owned(),
            mime: "application/pdf".to_owned(),
            bytes: pdf.clone(),
        },
        Attachment {
            file_name: "notiz.txt".to_owned(),
            mime: "text/plain".to_owned(),
            bytes: text_file.clone(),
        },
    ];
    let built = build_message(&message).expect("build");
    assert!(built.for_sending.is_ascii(), "7-bit message");
    for line in String::from_utf8_lossy(&built.for_sending).split("\r\n") {
        assert!(line.len() <= 998, "line too long: {}", line.len());
    }

    let parsed = parse(&built.for_sending);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let h = &parsed.headers;
    assert_eq!(h.subject.as_deref(), Some(message.subject.as_str()));
    assert_eq!(
        h.from,
        vec![address(Some("Jörg Müller 🦀"), "joerg@example.org")]
    );
    assert_eq!(h.to, message.to);
    assert_eq!(h.cc, message.cc);
    assert_eq!(
        h.message_id.as_deref(),
        Some("0123456789abcdef0123456789abcdef@example.org")
    );
    assert_eq!(h.date, Some(message.date));
    let text = parsed.text.as_deref().expect("text alternative");
    assert!(text.contains("Hallo Zoë,") && text.contains("📎"), "{text}");
    let html = parsed.html.as_deref().expect("html");
    assert!(html.contains("anbei die Unterlagen 📎."), "{html}");

    assert_eq!(parsed.attachments.len(), 2);
    let names: Vec<&str> = parsed
        .attachments
        .iter()
        .map(|a| a.file_name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Jahresabschluß 2026 – endgültig 📊.pdf", "notiz.txt"]
    );
    assert!(parsed.attachments.iter().all(|a| !a.name_changed));
    let pdf_part = &parsed.attachments[0];
    assert_eq!(pdf_part.mime, "application/pdf");
    assert_eq!(
        attachment_bytes(&built.for_sending, pdf_part.part_id).as_deref(),
        Some(pdf.as_slice())
    );
    assert_eq!(
        attachment_bytes(&built.for_sending, parsed.attachments[1].part_id).as_deref(),
        Some(text_file.as_slice())
    );
}

#[test]
fn structure_follows_the_content() {
    let content_type = |raw: &[u8]| {
        header_block(raw)
            .split("\r\n")
            .find(|l| l.to_ascii_lowercase().starts_with("content-type:"))
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let mut message = outgoing();
    message.html = None;
    message.text = Some("Only text.\nSecond line.".to_owned());
    let built = build_message(&message).expect("build");
    assert!(content_type(&built.for_sending).starts_with("content-type: text/plain"));
    let parsed = parse(&built.for_sending);
    assert_eq!(parsed.text.as_deref(), Some("Only text.\nSecond line.\n"));
    assert!(parsed.html.is_none());

    let built = build_message(&outgoing()).expect("build");
    assert!(content_type(&built.for_sending).contains("multipart/alternative"));
    let parsed = parse(&built.for_sending);
    assert_eq!(parsed.text.as_deref().map(str::trim_end), Some("Hello Bob"));
    assert!(
        parsed
            .html
            .as_deref()
            .is_some_and(|h| h.contains("<strong>Bob</strong>"))
    );

    let mut message = outgoing();
    message.attachments = vec![Attachment {
        file_name: "a.bin".to_owned(),
        mime: "multipart/mixed".to_owned(),
        bytes: vec![1, 2, 3],
    }];
    let built = build_message(&message).expect("build");
    assert!(content_type(&built.for_sending).contains("multipart/mixed"));
    let raw = String::from_utf8_lossy(&built.for_sending).to_ascii_lowercase();
    assert!(
        raw.contains("multipart/alternative"),
        "alternative inside mixed"
    );
    assert!(
        raw.contains("content-type: application/octet-stream"),
        "a multipart type is not taken over for an attachment"
    );
    let parsed = parse(&built.for_sending);
    assert_eq!(parsed.attachments.len(), 1);
    assert!(parsed.text.is_some() && parsed.html.is_some());

    // The same draft gives the same bytes (boundaries come from the Message-ID).
    assert_eq!(build_message(&message).expect("build"), built);
}

#[test]
fn bcc_only_in_the_sent_copy_and_the_envelope() {
    let mut message = outgoing();
    message.to = vec![
        address(Some("Bob"), "bob@example.net"),
        address(None, "BOB@example.net"),
    ];
    message.cc = vec![address(Some("Carol"), "carol@example.net")];
    message.bcc = vec![address(Some("Secret Dave"), "dave@example.net")];
    let built = build_message(&message).expect("build");
    let sending = String::from_utf8_lossy(&built.for_sending).to_string();
    assert!(!sending.contains("dave"), "{sending}");
    assert!(!sending.contains("Secret"), "{sending}");
    assert!(!sending.to_ascii_lowercase().contains("bcc"), "{sending}");
    let sent = String::from_utf8_lossy(&built.for_sent_folder).to_string();
    let bcc_field = "Bcc: \"Secret Dave\" <dave@example.net>\r\n";
    assert!(sent.contains(bcc_field), "{sent}");
    // Byte for byte the same apart from the Bcc field.
    assert_eq!(sent.replace(bcc_field, ""), sending);
    // Each address once in the headers too.
    let to = parse(&built.for_sending).headers.to;
    assert_eq!(to, vec![address(Some("Bob"), "bob@example.net")]);
    assert_eq!(
        parse(&built.for_sent_folder).headers.bcc,
        vec![address(Some("Secret Dave"), "dave@example.net")]
    );
    assert_eq!(built.envelope.from, "alice@example.org");
    assert_eq!(
        built.envelope.recipients,
        ["bob@example.net", "carol@example.net", "dave@example.net"],
        "each recipient once"
    );

    // Only blind copies: the message says so instead of having no recipient header.
    let mut message = outgoing();
    message.to.clear();
    message.bcc = vec![address(None, "dave@example.net")];
    let built = build_message(&message).expect("build");
    let head = header_block(&built.for_sending);
    assert!(head.contains("To: undisclosed-recipients:;"), "{head}");
    assert!(!head.contains("dave"));
}

#[test]
fn no_header_reveals_the_program_unless_turned_on() {
    let built = build_message(&outgoing()).expect("build");
    assert_eq!(
        header_names(&built.for_sending),
        [
            "Date",
            "From",
            "To",
            "Subject",
            "Message-ID",
            "MIME-Version",
            "Content-Type"
        ]
    );
    let head = header_block(&built.for_sending);
    assert!(
        head.contains("Date: Mon, 23 Nov 2026 13:05:00 +0000"),
        "{head}"
    );
    let raw = String::from_utf8_lossy(&built.for_sending).to_ascii_lowercase();
    for word in [
        "vice",
        "mailer",
        "user-agent",
        "x-",
        "localhost",
        "lettre",
        "builder",
    ] {
        assert!(!raw.contains(word), "{word:?} in the message");
    }

    let mut message = outgoing();
    message.user_agent = Some("Example Mail 1.0".to_owned());
    let built = build_message(&message).expect("build");
    assert!(header_block(&built.for_sending).contains("User-Agent: Example Mail 1.0"));
}

#[test]
fn reply_headers_are_written() {
    let mut message = outgoing();
    message.in_reply_to = Some("parent@example.net".to_owned());
    message.references = vec![
        "root@example.net".to_owned(),
        "bad id".to_owned(),
        "parent@example.net".to_owned(),
    ];
    message.reply_to = vec![address(None, "replies@example.org")];
    let built = build_message(&message).expect("build");
    let parsed = parse(&built.for_sending);
    assert_eq!(parsed.headers.in_reply_to, ["parent@example.net"]);
    assert_eq!(
        parsed.headers.references,
        ["root@example.net", "parent@example.net"],
        "invalid ids are dropped"
    );
    assert_eq!(
        parsed.headers.reply_to,
        vec![address(None, "replies@example.org")]
    );
}

#[test]
fn hostile_input_cannot_add_headers() {
    let mut message = outgoing();
    message.subject = "Hi\r\nBcc: evil@example.com\r\n\r\nbody".to_owned();
    message.from.name = Some("Alice\r\nX-Evil: 1".to_owned());
    message.attachments = vec![Attachment {
        file_name: "a\r\nContent-Type: text/html\r\n\r\n.txt".to_owned(),
        mime: "text/plain\r\nX-Evil: 2".to_owned(),
        bytes: b"x".to_vec(),
    }];
    let built = build_message(&message).expect("build");
    let names = header_names(&built.for_sending);
    assert!(
        !names.iter().any(|n| n.eq_ignore_ascii_case("bcc")),
        "{names:?}"
    );
    let raw = String::from_utf8_lossy(&built.for_sending);
    assert!(!raw.contains("\r\nX-Evil"), "{raw}");
    let parsed = parse(&built.for_sending);
    assert!(parsed.headers.bcc.is_empty());
    assert_eq!(
        parsed.headers.subject.as_deref(),
        Some("Hi  Bcc: evil@example.com    body")
    );
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].mime, "application/octet-stream");
}

#[test]
fn addresses_and_ids_are_checked() {
    let mut message = outgoing();
    message.to = vec![address(None, "not an address")];
    assert_eq!(
        build_message(&message),
        Err(ComposeError::InvalidAddress("not an address".into()))
    );
    let mut message = outgoing();
    message.to.clear();
    assert_eq!(build_message(&message), Err(ComposeError::NoRecipients));
    let mut message = outgoing();
    message.message_id = "no at sign".into();
    assert!(matches!(
        build_message(&message),
        Err(ComposeError::InvalidMessageId(_))
    ));

    assert_eq!(
        normalize_email(" <Jörg@Müller.Example> ").as_deref(),
        Ok("Jörg@xn--mller-kva.example")
    );
    assert_eq!(
        normalize_email("a.b+tag@[192.0.2.1]").as_deref(),
        Ok("a.b+tag@[192.0.2.1]")
    );
    for bad in [
        "",
        "@example.org",
        "a@",
        "a b@example.org",
        "a..b@example.org",
        ".a@example.org",
        "a@exa mple.org",
        "a@-example.org",
        "a@192.0.2.1",
        "a@example.org.",
        "a\r\n@example.org",
        "\"quoted\"@example.org",
        "a<b@example.org",
        "a@exa%41mple.org",
        "a@example.org:25",
    ] {
        assert!(normalize_email(bad).is_err(), "{bad:?} accepted");
    }
}

#[test]
fn message_ids_are_random_and_name_only_the_sender_domain() {
    let id = new_message_id("Alice@Example.ORG", [0xab; 16]);
    assert_eq!(id, "abababababababababababababababab@example.org");
    let other = new_message_id("alice@example.org", [0x01; 16]);
    assert_ne!(id, other);
    assert_eq!(
        new_message_id("x@bücher.example", [0; 16]),
        "00000000000000000000000000000000@xn--bcher-kva.example"
    );
    assert!(new_message_id("broken", [0; 16]).ends_with("@localhost"));
    let mut message = outgoing();
    message.message_id = id.clone();
    assert!(build_message(&message).is_ok());
}

// ---------------------------------------------------------------------------------------------
// Replies and forwards

const HTML_ORIGINAL: &[u8] = b"From: Bob Example <bob@example.net>\r\n\
To: Alice <alice@example.org>, Carol <carol@example.net>\r\n\
Cc: Dave <dave@example.net>, alice.work@example.org\r\n\
Subject: AW: Re: Treffen am Montag\r\n\
Date: Mon, 23 Nov 2026 13:05:00 +0000\r\n\
Message-ID: <parent@example.net>\r\n\
References: <root@example.net> <middle@example.net>\r\n\
In-Reply-To: <middle@example.net>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: multipart/alternative; boundary=\"b2\"\r\n\
\r\n\
--b2\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hi Alice,\r\n\
\r\n\
see https://example.com/agenda\r\n\
> earlier text\r\n\
--b2\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><head><style>p{color:red}</style></head><body><p>Hi <b>Alice</b>,</p>\
<p>see <a href=\"https://example.com/agenda\" class=\"x\">the agenda</a>\
<img src=\"https://tracker.example/p.gif\"></p><blockquote><p>earlier text</p></blockquote>\
<p></p><script>evil()</script></body></html>\r\n\
--b2--\r\n\
--b1\r\n\
Content-Type: application/pdf; name=\"agenda.pdf\"\r\n\
Content-Disposition: attachment; filename=\"agenda.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQKJcfsj6IK\r\n\
--b1--\r\n";

const TEXT_ORIGINAL: &[u8] = b"From: carol@example.net\r\n\
To: alice+lists@example.org\r\n\
Reply-To: Team List <team@lists.example>\r\n\
Delivered-To: alice@example.org\r\n\
Subject: Plan\r\n\
Date: Sun, 01 Mar 2026 23:30:00 +0000\r\n\
Message-ID: <text-parent@example.net>\r\n\
In-Reply-To: <only-parent@example.net>\r\n\
\r\n\
First line\r\n\
second line with www.example.org\r\n\
\r\n\
> quoted\r\n\
>> deeper  spaced\r\n";

#[test]
fn reply_and_reply_all_recipients_identity_and_threading() {
    let identities = [
        identity(1, "alice@example.org", true),
        identity(2, "alice.work@example.org", false),
    ];
    let original = Original::parse(HTML_ORIGINAL);

    let reply = prepare_reply(
        &original,
        ReplyKind::Sender,
        &identities,
        options(QuoteLanguage::English),
    );
    assert_eq!(reply.identity, Some(IdentityId(1)), "first identity in To");
    assert_eq!(
        reply.to,
        vec![address(Some("Bob Example"), "bob@example.net")]
    );
    assert!(reply.cc.is_empty());
    assert_eq!(reply.subject, "Re: Treffen am Montag");
    assert_eq!(reply.in_reply_to.as_deref(), Some("parent@example.net"));
    assert_eq!(
        reply.references,
        [
            "root@example.net",
            "middle@example.net",
            "parent@example.net"
        ]
    );
    assert!(reply.attachments.is_empty());

    let all = prepare_reply(
        &original,
        ReplyKind::All,
        &identities,
        options(QuoteLanguage::English),
    );
    assert_eq!(
        all.to,
        vec![
            address(Some("Bob Example"), "bob@example.net"),
            address(Some("Carol"), "carol@example.net"),
        ],
        "own identities removed"
    );
    assert_eq!(all.cc, vec![address(Some("Dave"), "dave@example.net")]);

    // Reply-To wins; the identity comes from the plus address or Delivered-To.
    let original = Original::parse(TEXT_ORIGINAL);
    assert_eq!(original.delivered_to, ["alice@example.org"]);
    let reply = prepare_reply(
        &original,
        ReplyKind::All,
        &identities,
        options(QuoteLanguage::German),
    );
    assert_eq!(reply.identity, Some(IdentityId(1)));
    assert_eq!(
        reply.to,
        vec![address(Some("Team List"), "team@lists.example")]
    );
    assert!(reply.cc.is_empty(), "{:?}", reply.cc);
    assert!(
        !reply.to.iter().any(|a| a.email.starts_with("alice")),
        "alice+lists@ is the user's own address: {:?}",
        reply.to
    );
    assert_eq!(reply.subject, "Re: Plan");
    assert_eq!(
        reply.references,
        ["only-parent@example.net", "text-parent@example.net"],
        "a single In-Reply-To stands in for missing References"
    );
}

#[test]
fn reply_to_own_message_goes_to_its_recipients() {
    let identities = [identity(7, "alice@example.org", true)];
    let raw = b"From: Alice <alice@example.org>\r\nTo: Bob <bob@example.net>\r\n\
                Cc: Carol <carol@example.net>\r\nSubject: Mine\r\n\
                Message-ID: <mine@example.org>\r\n\r\nbody\r\n";
    let original = Original::parse(raw);
    let reply = prepare_reply(
        &original,
        ReplyKind::Sender,
        &identities,
        options(QuoteLanguage::English),
    );
    assert_eq!(reply.identity, Some(IdentityId(7)));
    assert_eq!(reply.to, vec![address(Some("Bob"), "bob@example.net")]);
    let all = prepare_reply(
        &original,
        ReplyKind::All,
        &identities,
        options(QuoteLanguage::English),
    );
    assert_eq!(all.cc, vec![address(Some("Carol"), "carol@example.net")]);

    // Sent to oneself: the reply goes to oneself.
    let raw = b"From: alice@example.org\r\nTo: alice@example.org\r\nSubject: Note\r\n\r\nx\r\n";
    let reply = prepare_reply(
        &Original::parse(raw),
        ReplyKind::All,
        &identities,
        options(QuoteLanguage::English),
    );
    assert_eq!(reply.to, vec![address(None, "alice@example.org")]);
}

#[test]
fn identity_choice_order() {
    let identities = [
        identity(1, "alice@example.org", false),
        identity(2, "sales@example.org", true),
    ];
    let headers = |to: &[&str], cc: &[&str]| crate::MessageHeaders {
        from: vec![address(None, "someone@example.net")],
        to: to.iter().map(|e| address(None, e)).collect(),
        cc: cc.iter().map(|e| address(None, e)).collect(),
        ..crate::MessageHeaders::default()
    };
    let id = |h: &crate::MessageHeaders, delivered: &[&str]| {
        let delivered: Vec<String> = delivered.iter().map(|s| (*s).to_owned()).collect();
        choose_identity(&identities, h, &delivered).map(|i| i.id.0)
    };
    assert_eq!(
        id(&headers(&["x@y.z", "SALES@example.org"], &[]), &[]),
        Some(2)
    );
    assert_eq!(
        id(&headers(&["x@y.z"], &["alice@example.org"]), &[]),
        Some(1)
    );
    assert_eq!(
        id(
            &headers(&["list@lists.example"], &[]),
            &["alice@example.org"]
        ),
        Some(1),
        "Bcc or list: the delivery header"
    );
    assert_eq!(id(&headers(&["alice+news@example.org"], &[]), &[]), Some(1));
    assert_eq!(id(&headers(&["x@y.z"], &[]), &[]), Some(2), "default");
    assert_eq!(choose_identity(&[], &headers(&[], &[]), &[]), None);
}

#[test]
fn subject_prefixes_are_recognised_and_not_doubled() {
    for (subject, reply, forward) in [
        ("Treffen", "Re: Treffen", "Fwd: Treffen"),
        ("Re: Treffen", "Re: Treffen", "Fwd: Re: Treffen"),
        (
            "RE: re: AW: Treffen",
            "Re: Treffen",
            "Fwd: RE: re: AW: Treffen",
        ),
        ("AW: Treffen", "Re: Treffen", "Fwd: AW: Treffen"),
        ("Antw: Treffen", "Re: Treffen", "Fwd: Antw: Treffen"),
        ("Re[2]: Treffen", "Re: Treffen", "Fwd: Re[2]: Treffen"),
        ("WG: Treffen", "Re: WG: Treffen", "Fwd: Treffen"),
        (
            "Fwd: Fw: WG: Treffen",
            "Re: Fwd: Fw: WG: Treffen",
            "Fwd: Treffen",
        ),
        ("TR : Réunion", "Re: TR : Réunion", "Fwd: Réunion"),
        ("Regeln", "Re: Regeln", "Fwd: Regeln"),
        ("Rewe: Angebot", "Re: Rewe: Angebot", "Fwd: Rewe: Angebot"),
        ("", "Re:", "Fwd:"),
    ] {
        assert_eq!(reply_subject(subject), reply, "reply to {subject:?}");
        assert_eq!(forward_subject(subject), forward, "forward of {subject:?}");
    }
    assert_eq!(strip_subject_prefixes("Re: Fwd: AW: WG: x"), "x");
}

#[test]
fn long_reference_chains_are_cut_in_the_middle() {
    let references: Vec<String> = (0..40).map(|n| format!("r{n}@example.net")).collect();
    let headers = crate::MessageHeaders {
        message_id: Some("parent@example.net".into()),
        references: references.clone(),
        ..crate::MessageHeaders::default()
    };
    let (parent, chain) = reply_references(&headers);
    assert_eq!(parent.as_deref(), Some("parent@example.net"));
    assert_eq!(chain.len(), MAX_REFERENCES);
    assert_eq!(chain[0], "r0@example.net", "thread root kept");
    assert_eq!(chain[MAX_REFERENCES - 1], "parent@example.net");
    assert_eq!(chain[MAX_REFERENCES - 2], "r39@example.net");

    // Duplicates and the parent itself are not repeated.
    let headers = crate::MessageHeaders {
        message_id: Some("p@x".into()),
        references: vec!["a@x".into(), "a@x".into(), "p@x".into(), "b@x".into()],
        ..crate::MessageHeaders::default()
    };
    assert_eq!(reply_references(&headers).1, ["a@x", "b@x", "p@x"]);
    // No Message-ID: no In-Reply-To.
    assert_eq!(
        reply_references(&crate::MessageHeaders::default()),
        (None, Vec::new())
    );
}

#[test]
fn reply_quotes_snapshots() {
    let identities = [identity(1, "alice@example.org", true)];
    for (name, raw, language) in [
        ("reply_html_en", HTML_ORIGINAL, QuoteLanguage::English),
        ("reply_html_de", HTML_ORIGINAL, QuoteLanguage::German),
        ("reply_text_de", TEXT_ORIGINAL, QuoteLanguage::German),
    ] {
        let reply = prepare_reply(
            &Original::parse(raw),
            ReplyKind::Sender,
            &identities,
            options(language),
        );
        snapshot(name, &reply.html);
        // What the recipient gets once the user wrote above the quote.
        let mail = mail_html(&format!("<p>My answer.</p>{}", reply.html));
        assert_allowlisted(&mail);
        snapshot(&format!("{name}_mail"), &mail);
    }
}

#[test]
fn reply_text_quote_and_attribution() {
    let identities = [identity(1, "alice@example.org", true)];
    let reply = prepare_reply(
        &Original::parse(TEXT_ORIGINAL),
        ReplyKind::Sender,
        &identities,
        options(QuoteLanguage::German),
    );
    // 23:30 UTC is 00:30 the next day at UTC+1.
    assert_eq!(
        reply.text,
        "Am 02.03.2026 um 00:30 schrieb carol@example.net:\n\
         > First line\n\
         > second line with www.example.org\n\
         >\n\
         >> quoted\n\
         >>> deeper  spaced"
    );
    let reply = prepare_reply(
        &Original::parse(HTML_ORIGINAL),
        ReplyKind::Sender,
        &identities,
        options(QuoteLanguage::English),
    );
    assert!(
        reply.text.starts_with(
            "On Mon, 23 Nov 2026 at 14:05, Bob Example <bob@example.net> wrote:\n> Hi Alice,"
        ),
        "{}",
        reply.text
    );
    // Without a date.
    let raw = b"From: Bob <bob@example.net>\r\nSubject: x\r\n\r\nbody\r\n";
    let reply = prepare_reply(
        &Original::parse(raw),
        ReplyKind::Sender,
        &identities,
        options(QuoteLanguage::German),
    );
    assert_eq!(reply.text, "Bob <bob@example.net> schrieb:\n> body");
}

#[test]
fn forward_takes_the_attachments_along() {
    let identities = [
        identity(1, "alice@example.org", true),
        identity(2, "alice.work@example.org", false),
    ];
    let original = Original::parse(HTML_ORIGINAL);
    let forward = prepare_forward(&original, &identities, options(QuoteLanguage::German));
    assert!(forward.to.is_empty() && forward.cc.is_empty());
    assert_eq!(forward.identity, Some(IdentityId(1)));
    assert_eq!(forward.subject, "Fwd: AW: Re: Treffen am Montag");
    assert_eq!(forward.in_reply_to, None);
    assert_eq!(
        forward.references.last().map(String::as_str),
        Some("parent@example.net")
    );
    assert_eq!(forward.attachments.len(), 1);
    let attachment = &forward.attachments[0];
    assert_eq!(attachment.file_name, "agenda.pdf");
    assert_eq!(attachment.mime, "application/pdf");
    assert_eq!(attachment.bytes, b"%PDF-1.4\n%\xc7\xec\x8f\xa2\n");
    assert!(
        forward.text.starts_with(
            "-------- Weitergeleitete Nachricht --------\n\
             Betreff: AW: Re: Treffen am Montag\n\
             Datum: 23.11.2026 14:05 +0100\n\
             Von: Bob Example <bob@example.net>\n\
             An: Alice <alice@example.org>, Carol <carol@example.net>\n\
             Kopie: Dave <dave@example.net>, alice.work@example.org\n\nHi Alice,"
        ),
        "{}",
        forward.text
    );
    snapshot("forward_html_de", &forward.html);

    // The forward can be built and the attachment arrives unchanged.
    let mut message = outgoing();
    message.subject = forward.subject.clone();
    message.html = Some(forward.html.clone());
    message.references = forward.references.clone();
    message.attachments = forward.attachments.clone();
    let built = build_message(&message).expect("build");
    let parsed = parse(&built.for_sending);
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(
        attachment_bytes(&built.for_sending, parsed.attachments[0].part_id),
        Some(attachment.bytes.clone())
    );
    let forward_en = prepare_forward(&original, &identities, options(QuoteLanguage::English));
    assert!(
        forward_en
            .text
            .starts_with("-------- Forwarded Message --------\nSubject: AW: Re: Treffen am Montag\nDate: Mon, 23 Nov 2026 14:05 +0100\n"),
        "{}",
        forward_en.text
    );
}
