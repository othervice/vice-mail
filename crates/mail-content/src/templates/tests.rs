//! Tests of signatures, templates and draft assembly. Snapshots live in `snapshots/` and are
//! refreshed with `UPDATE_SNAPSHOTS=1 cargo test -p mail-content templates`.

use std::fs;
use std::path::PathBuf;

use mail_model::Address;

use super::*;
use crate::compose::QuoteLanguage;
use crate::dom::{self, NodeData, Visit};
use crate::{
    Original, OutgoingMessage, QuoteOptions, ReplyKind, build_message, mail_html, parse,
    prepare_reply,
};

fn snapshot(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/templates/snapshots")
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

/// Only elements and attributes of the mail HTML allowlist (checked independently).
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
    // Markup only; escaped text may well say `onerror`.
    for banned in ["<script", "<img", "<style", "<svg"] {
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
                assert!(ok, "attribute {attr_name} on {tag} in {html}");
            }
        }
        dom::Descend::Yes
    });
}

/// A signature as Outlook or a web editor would paste it.
const PASTED_SIGNATURE: &str = r##"<div style="font-family:Calibri;color:#1F497D">
<p class="MsoNormal">-- <br></p>
<table cellpadding="0"><tr>
<td><img src="https://firma.example/logo.png" width="80" alt="Logo"></td>
<td><p><b><font color="#003366" size="3">Hans Müller</font></b><br>
<i>Leiter Vertrieb</i> | Firma GmbH<br>
Tel. <a href="tel:+49301234567">+49 30 1234567</a><br>
<a href="https://firma.example" onclick="track()">firma.example</a> ·
<a href="mailto:hans.mueller@firma.example">hans.mueller@firma.example</a><br>
<a href="javascript:alert(1)">Klick</a></p></td>
</tr></table>
<script>alert('x')</script>
<p style="font-size:8pt;color:gray">Firma GmbH, Musterstraße 1, 10115 Berlin<br>
Geschäftsführer: Erika Mustermann</p>
</div>"##;

#[test]
fn an_html_signature_is_cleaned_with_the_mail_allowlist() {
    let (signature, notes) = Signature::from_html(PASTED_SIGNATURE).expect("signature");
    snapshot("signature_html", &signature.html);
    assert_allowlisted(&signature.html);
    assert_allowlisted(&mail_html(&signature.html));
    assert_eq!(
        notes,
        SanitizeNotes {
            images_removed: 1,
            formatting_simplified: true
        }
    );
    assert!(
        !signature.html.contains("--"),
        "the typed separator is removed"
    );
    assert_eq!(
        signature.text,
        "Hans Müller\nLeiter Vertrieb | Firma GmbH\nTel. +49 30 1234567\n\
         firma.example <https://firma.example/> · hans.mueller@firma.example\nKlick\n\n\
         Firma GmbH, Musterstraße 1, 10115 Berlin\nGeschäftsführer: Erika Mustermann"
    );
    assert_eq!(
        signature.text_with_separator().lines().next(),
        Some("-- "),
        "the separator keeps its space"
    );
}

#[test]
fn separators_and_plain_text_signatures() {
    for html in [
        "<p>--</p><p>Hans</p>",
        "<p>-- <br>Hans</p>",
        "<p>Hans</p>",
        "\n<p>—</p>Hans",
    ] {
        let (signature, _) = Signature::from_html(html).expect("signature");
        assert_eq!(signature.html, "<p>Hans</p>\n", "{html}");
        assert_eq!(signature.text, "Hans", "{html}");
    }
    // A dash line later on stays.
    let (signature, notes) = Signature::from_html("<p>Hans<br>--<br>Firma</p>").expect("sig");
    assert_eq!(signature.text, "Hans\n--\nFirma");
    assert_eq!(notes, SanitizeNotes::default());

    let signature =
        Signature::from_text("\r\n-- \r\nErika Mustermann  \r\nhttps://firma.example\r\n\r\n")
            .expect("text");
    assert_eq!(signature.text, "Erika Mustermann\nhttps://firma.example");
    assert_eq!(
        signature.html,
        "<p>Erika Mustermann<br><a href=\"https://firma.example/\">https://firma.example</a></p>\n"
    );
    assert!(Signature::from_text("").expect("empty").is_empty());
    assert!(!signature.is_empty());
}

#[test]
fn sizes_are_limited() {
    let big = format!("<p>{}</p>", "x".repeat(MAX_SIGNATURE_BYTES + 1));
    assert!(matches!(
        Signature::from_html(&big),
        Err(SnippetError::TooLarge { max, .. }) if max == MAX_SIGNATURE_BYTES
    ));
    let huge = "x".repeat(MAX_SNIPPET_INPUT_BYTES + 1);
    assert!(matches!(
        Template::from_html("s", &huge),
        Err(SnippetError::TooLarge { max, .. }) if max == MAX_SNIPPET_INPUT_BYTES
    ));
    // Styles alone do not count: a big pasted document can make a small signature.
    let styled = format!(
        "<div style=\"{}\"><p>Hans</p></div>",
        "color:red;".repeat(MAX_SIGNATURE_BYTES / 4)
    );
    assert!(Signature::from_html(&styled).is_ok());
}

fn address(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.to_owned(),
    }
}

#[test]
fn templates_fill_placeholders_as_text() {
    let (template, _) = Template::from_html(
        "Angebot für {{ last_name }}\r\n({{date}})",
        "<p>Hallo {{first_name|zusammen}},</p>\
         <p>anbei das Angebot vom {{date}} für {{name}} &lt;{{email}}&gt;. {{unknown_thing}} {{ }} {{a b}}</p>\
         <ul><li><b>{{FIRST_NAME}}</b></li></ul><pre>{{my_email}}</pre>\
         <p><a href=\"https://firma.example/?for={{email}}\">Link</a></p>\
         <p>Viele Grüße<br>{{my_first_name}} ({{my_name}})</p>",
    )
    .expect("template");
    assert_eq!(template.subject, "Angebot für {{ last_name }} ({{date}})");
    let (known, unknown) = template.placeholders();
    assert_eq!(
        known,
        [
            Placeholder::LastName,
            Placeholder::Date,
            Placeholder::FirstName,
            Placeholder::Name,
            Placeholder::Email,
            Placeholder::MyEmail,
            Placeholder::MyFirstName,
            Placeholder::MyName,
        ]
    );
    assert_eq!(unknown, ["unknown_thing"]);

    let recipient = address(Some("Dr. Hans Müller"), "hans.mueller@firma.example");
    let sender = address(Some("Erika Mustermann"), "erika@firma.example");
    let values = TemplateValues {
        recipient: Some(&recipient),
        sender: Some(&sender),
        date: Some(LocalDate {
            year: 2026,
            month: 11,
            day: 3,
        }),
        language: QuoteLanguage::German,
    };
    let filled = fill_template(&template, &values);
    assert_eq!(filled.subject, "Angebot für Müller (03.11.2026)");
    assert_eq!(
        filled.text,
        "Hallo Hans,\n\nanbei das Angebot vom 03.11.2026 für Dr. Hans Müller \
         <hans.mueller@firma.example>. {{unknown_thing}} {{ }} {{a b}}\n\n• Hans\n\n\
         erika@firma.example\n\nLink <https://firma.example/?for={{email}}>\n\n\
         Viele Grüße\nErika (Erika Mustermann)"
    );
    assert!(
        filled.html.contains("<li><strong>Hans</strong></li>"),
        "{}",
        filled.html
    );
    assert!(
        filled
            .html
            .contains("href=\"https://firma.example/?for={{email}}\""),
        "link targets are not filled: {}",
        filled.html
    );
    assert_eq!(
        filled.problems,
        [TemplateProblem::Unknown("unknown_thing".into())]
    );

    // Without a recipient name: fallback, else empty and reported.
    let nameless = address(None, "info@firma.example");
    let filled = fill_template(
        &template,
        &TemplateValues {
            recipient: Some(&nameless),
            sender: None,
            date: None,
            language: QuoteLanguage::English,
        },
    );
    assert!(
        filled.text.starts_with("Hallo zusammen,"),
        "{}",
        filled.text
    );
    assert_eq!(filled.subject, "Angebot für ()");
    assert!(
        filled
            .problems
            .contains(&TemplateProblem::Missing(Placeholder::LastName))
    );
    assert!(
        filled
            .problems
            .contains(&TemplateProblem::Missing(Placeholder::Date))
    );
    assert!(
        filled
            .problems
            .contains(&TemplateProblem::Missing(Placeholder::MyName))
    );
}

#[test]
fn values_cannot_inject_html_or_placeholders() {
    let template =
        Template::from_text("Re: {{name}}", "Hallo {{name}}, {{first_name}}!").expect("t");
    let hostile = address(
        Some("<script>alert(1)</script>\"><img src=x onerror=alert(2)> {{my_email}}\r\nBcc: evil"),
        "evil@x.example",
    );
    let me = address(Some("Me"), "me@x.example");
    let filled = fill_template(
        &template,
        &TemplateValues {
            recipient: Some(&hostile),
            sender: Some(&me),
            date: None,
            language: QuoteLanguage::English,
        },
    );
    assert_allowlisted(&filled.html);
    assert_allowlisted(&mail_html(&filled.html));
    assert!(filled.html.contains("&lt;/script&gt;"), "{}", filled.html);
    assert!(filled.html.contains("&lt;img src=x"), "{}", filled.html);
    assert!(
        filled.text.contains("{{my_email}}"),
        "values are not expanded again"
    );
    assert!(!filled.text.contains("me@x.example"));
    assert!(!filled.subject.contains('\n') && !filled.subject.contains('\r'));
    assert!(filled.subject.contains("</script>"), "the subject is text");
}

#[test]
fn dates_in_both_languages() {
    let date = LocalDate {
        year: 2026,
        month: 1,
        day: 9,
    };
    assert_eq!(date.format(QuoteLanguage::German), "09.01.2026");
    assert_eq!(date.format(QuoteLanguage::English), "9 January 2026");
}

const ORIGINAL: &[u8] = b"From: Carol Clark <carol@example.org>\r\n\
To: Alice Archer <alice@vicemail.test>\r\n\
Subject: Angebot\r\n\
Date: Mon, 23 Nov 2026 14:05:00 +0000\r\n\
Message-ID: <orig@example.org>\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n\
<p>Hallo Alice,</p><p>wie ist der <b>Stand</b>?</p><p>-- <br>Carol</p>\r\n";

fn signature() -> Signature {
    Signature::from_html(
        "<p><strong>Alice Archer</strong><br>Vice Mail · \
         <a href=\"https://vicemail.example\">vicemail.example</a></p>",
    )
    .expect("signature")
    .0
}

#[test]
fn new_message_starts_with_an_empty_line_and_the_signature() {
    let signature = signature();
    let draft = assemble_draft(&DraftParts {
        body_html: "",
        signature: Some(&signature),
        quote: None,
        position: ReplyPosition::AboveQuote,
    });
    assert_eq!(
        draft.html,
        // A line break keeps the open marks (the writer of `mail_html`); it looks the same.
        "<p><br></p>\n<p><strong>Alice Archer<br></strong>Vice Mail · \
         <a href=\"https://vicemail.example/\">vicemail.example</a></p>\n"
    );
    assert_eq!(
        draft.text,
        "\n\n-- \nAlice Archer\nVice Mail · vicemail.example <https://vicemail.example/>\n"
    );
    // Without a signature: no separator.
    let plain = assemble_draft(&DraftParts {
        body_html: "<p>Hallo</p>",
        signature: None,
        quote: None,
        position: ReplyPosition::AboveQuote,
    });
    assert_eq!(plain.text, "Hallo\n");
    assert!(!plain.html.contains("--"));
    let empty_signature = Signature::default();
    let plain = assemble_draft(&DraftParts {
        signature: Some(&empty_signature),
        ..DraftParts {
            body_html: "<p>Hallo</p>",
            signature: None,
            quote: None,
            position: ReplyPosition::AboveQuote,
        }
    });
    assert_eq!(plain.text, "Hallo\n");
}

#[test]
fn reply_has_the_signature_above_the_quote() {
    let original = Original::parse(ORIGINAL);
    let prepared = prepare_reply(
        &original,
        ReplyKind::Sender,
        &[],
        QuoteOptions {
            language: QuoteLanguage::German,
            utc_offset_minutes: 60,
        },
    );
    let signature = signature();
    let quote = Quote {
        html: &prepared.html,
        text: &prepared.text,
    };
    let draft = assemble_draft(&DraftParts {
        body_html: "<p>Hallo Carol,</p><p>alles im Plan.</p>",
        signature: Some(&signature),
        quote: Some(quote),
        position: ReplyPosition::AboveQuote,
    });
    let mail = mail_html(&draft.html);
    snapshot("reply_with_signature_mail", &mail);
    assert_allowlisted(&mail);
    let signature_at = mail.find("Alice Archer").expect("signature");
    let quote_at = mail.find("<blockquote").expect("quote");
    assert!(mail.find("alles im Plan").expect("body") < signature_at);
    assert!(signature_at < quote_at, "signature above the quote");
    assert_eq!(
        draft.text,
        "Hallo Carol,\n\nalles im Plan.\n\n-- \nAlice Archer\n\
         Vice Mail · vicemail.example <https://vicemail.example/>\n\n\
         Am 23.11.2026 um 15:05 schrieb Carol Clark <carol@example.org>:\n\
         > Hallo Alice,\n>\n> wie ist der Stand?\n>\n> --\n> Carol\n"
    );

    // Answering below the quote: quote, answer, signature.
    let below = assemble_draft(&DraftParts {
        body_html: "<p>Alles im Plan.</p>",
        signature: Some(&signature),
        quote: Some(quote),
        position: ReplyPosition::BelowQuote,
    });
    assert!(below.text.starts_with("Am 23.11.2026"));
    assert!(below.text.ends_with(
        "-- \nAlice Archer\nVice Mail · vicemail.example <https://vicemail.example/>\n"
    ));
    let html = below.html;
    let quote_at = html.find("<blockquote").expect("quote");
    assert!(quote_at < html.find("Alles im Plan").expect("body"));
    assert!(html.find("Alles im Plan").expect("body") < html.find("Alice Archer").expect("sig"));
}

#[test]
fn the_sent_message_carries_the_signature_in_both_parts() {
    let original = Original::parse(ORIGINAL);
    let prepared = prepare_reply(
        &original,
        ReplyKind::Sender,
        &[],
        QuoteOptions {
            language: QuoteLanguage::English,
            utc_offset_minutes: 0,
        },
    );
    let signature = signature();
    let draft = assemble_draft(&DraftParts {
        body_html: "<p>On track.</p>",
        signature: Some(&signature),
        quote: Some(Quote {
            html: &prepared.html,
            text: &prepared.text,
        }),
        position: ReplyPosition::AboveQuote,
    });
    let built = build_message(&OutgoingMessage {
        from: address(Some("Alice Archer"), "alice@vicemail.test"),
        to: prepared.to.clone(),
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        subject: prepared.subject.clone(),
        text: Some(draft.text.clone()),
        html: Some(draft.html.clone()),
        attachments: Vec::new(),
        inline_images: Vec::new(),
        message_id: "sig-test@vicemail.test".into(),
        in_reply_to: prepared.in_reply_to.clone(),
        references: prepared.references.clone(),
        date: 1_795_000_000,
        user_agent: None,
        disposition_notification_to: None,
        report: None,
    })
    .expect("built");
    let message = parse(&built.for_sending);
    let text = message.text.expect("text part");
    assert!(text.contains("\n-- \nAlice Archer\n"), "{text:?}");
    assert!(
        text.find("-- \n").expect("separator") < text.find("> Hallo Alice").expect("quote"),
        "{text:?}"
    );
    let html = message.html.expect("html part");
    assert!(html.find("Alice Archer").expect("sig") < html.find("<blockquote").expect("quote"));
}
