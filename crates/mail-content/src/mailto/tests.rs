//! `mailto:` links after RFC 6068: every example of its section 6, encoded subjects and bodies
//! (percent-encoded UTF-8 and RFC 2047), and links that try more than filling a draft.

use std::time::{Duration, Instant};

use mail_model::{ErrorCode, MAX_RECIPIENTS, MAX_SUBJECT_CHARS};

use super::*;

fn parse(uri: &str) -> ComposeRequest {
    parse_mailto(uri).unwrap_or_else(|e| panic!("{uri}: {e}"))
}

fn emails(addresses: &[Address]) -> Vec<&str> {
    addresses.iter().map(|a| a.email.as_str()).collect()
}

fn to(uri: &str) -> Vec<String> {
    parse(uri).to.into_iter().map(|a| a.email).collect()
}

fn invalid(value: &str) -> MailtoNotice {
    MailtoNotice::AddressInvalid {
        value: value.to_owned(),
    }
}

fn ignored(name: &str) -> MailtoNotice {
    MailtoNotice::HeaderIgnored {
        name: name.to_owned(),
    }
}

#[test]
fn rfc6068_basic_examples() {
    // Section 6.1, in the order of the RFC.
    let request = parse("mailto:chris@example.com");
    assert_eq!(emails(&request.to), ["chris@example.com"]);
    assert_eq!(request.to[0].name, None);
    assert!(request.notices.is_empty());
    assert_eq!(
        request,
        ComposeRequest {
            to: request.to.clone(),
            ..ComposeRequest::default()
        }
    );

    let request = parse("mailto:infobot@example.com?subject=current-issue");
    assert_eq!(emails(&request.to), ["infobot@example.com"]);
    assert_eq!(request.subject.as_deref(), Some("current-issue"));
    assert_eq!(request.body, None);

    let request = parse("mailto:infobot@example.com?body=send%20current-issue");
    assert_eq!(request.body.as_deref(), Some("send current-issue"));
    assert_eq!(request.subject, None);

    let request = parse("mailto:infobot@example.com?body=send%20current-issue%0D%0Asend%20index");
    assert_eq!(
        request.body.as_deref(),
        Some("send current-issue\nsend index")
    );
    assert!(request.notices.is_empty(), "CRLF is a normal line break");

    let request = parse("mailto:list@example.org?In-Reply-To=%3C3469A91.D10AF4C@example.com%3E");
    assert_eq!(emails(&request.to), ["list@example.org"]);
    assert_eq!(
        request.in_reply_to.as_deref(),
        Some("3469A91.D10AF4C@example.com")
    );

    let request = parse("mailto:majordomo@example.com?body=subscribe%20bamboo-l");
    assert_eq!(request.body.as_deref(), Some("subscribe bamboo-l"));

    let request = parse("mailto:joe@example.com?cc=bob@example.com&body=hello");
    assert_eq!(emails(&request.to), ["joe@example.com"]);
    assert_eq!(emails(&request.cc), ["bob@example.com"]);
    assert!(request.bcc.is_empty());
    assert_eq!(request.body.as_deref(), Some("hello"));
    assert!(request.notices.is_empty());
    // "Another way of expressing the same thing".
    assert_eq!(
        parse("mailto:?to=joe@example.com&cc=bob@example.com&body=hello"),
        request
    );

    // "by using "?" twice, is incorrect": the second `?` belongs to the cc value.
    let wrong = parse("mailto:joe@example.com?cc=bob@example.com?body=hello");
    assert_eq!(emails(&wrong.to), ["joe@example.com"]);
    assert!(wrong.cc.is_empty());
    assert_eq!(wrong.body, None);
    assert_eq!(wrong.notices, [invalid("bob@example.com?body=hello")]);

    // Reserved characters in addresses, percent-encoded.
    assert_eq!(
        to("mailto:gorby%25kremvax@example.com"),
        ["gorby%kremvax@example.com"]
    );
    let request = parse("mailto:unlikely%3Faddress@example.com?blat=foop");
    assert_eq!(emails(&request.to), ["unlikely?address@example.com"]);
    assert_eq!(
        request.notices,
        [ignored("blat")],
        "the blat field is ignored"
    );
    assert_eq!(
        to("mailto:Mike%26family@example.org"),
        ["Mike&family@example.org"]
    );
}

#[test]
fn rfc6068_complicated_addresses() {
    // Section 6.2: quoted local parts are decoded exactly, but the composer only takes
    // dot-atoms, so they are reported with the decoded address and left out.
    for (uri, address) in [
        (
            "mailto:%22not%40me%22@example.org",
            r#""not@me"@example.org"#,
        ),
        (
            "mailto:%22oh%5C%5Cno%22@example.org",
            r#""oh\\no"@example.org"#,
        ),
        (
            "mailto:%22%5C%5C%5C%22it's%5C%20ugly%5C%5C%5C%22%22@example.org",
            r#""\\\"it's\ ugly\\\""@example.org"#,
        ),
    ] {
        let request = parse(uri);
        assert!(request.to.is_empty(), "{uri}");
        assert_eq!(request.notices, [invalid(address)], "{uri}");
    }
    // A quoted comma does not split the list.
    let request = parse("mailto:%22a,b%22@example.org,c@example.org");
    assert_eq!(emails(&request.to), ["c@example.org"]);
    assert_eq!(request.notices, [invalid(r#""a,b"@example.org"#)]);
}

#[test]
fn rfc6068_non_ascii_examples() {
    // Section 6.3: the same subject as UTF-8 and as encoded words in UTF-8 and ISO-8859-1.
    for uri in [
        "mailto:user@example.org?subject=caf%C3%A9",
        "mailto:user@example.org?subject=%3D%3Futf-8%3FQ%3Fcaf%3DC3%3DA9%3F%3D",
        "mailto:user@example.org?subject=%3D%3Fiso-8859-1%3FQ%3Fcaf%3DE9%3F%3D",
    ] {
        let request = parse(uri);
        assert_eq!(emails(&request.to), ["user@example.org"], "{uri}");
        assert_eq!(request.subject.as_deref(), Some("café"), "{uri}");
        assert!(request.notices.is_empty(), "{uri}");
    }
    let request = parse("mailto:user@example.org?subject=caf%C3%A9&body=caf%C3%A9");
    assert_eq!(request.subject.as_deref(), Some("café"));
    assert_eq!(request.body.as_deref(), Some("café"));

    // A domain in Unicode ("natto") goes into the message as punycode, as the RFC shows.
    let request = parse("mailto:user@%E7%B4%8D%E8%B1%86.example.org?subject=Test&body=NATTO");
    assert_eq!(emails(&request.to), ["user@xn--99zt52a.example.org"]);
    assert_eq!(request.subject.as_deref(), Some("Test"));
    assert_eq!(request.body.as_deref(), Some("NATTO"));
    assert_eq!(
        parse("mailto:user@xn--99zt52a.example.org?subject=Test&body=NATTO"),
        request
    );
}

#[test]
fn non_ascii_addresses() {
    assert_eq!(to("mailto:%E2%99%A1@example.org"), ["♡@example.org"]);
    assert_eq!(
        to("mailto:%e2%99%a1@example.org"),
        ["♡@example.org"],
        "lower-case hex"
    );
    // An IRI as some systems hand it over, not percent-encoded.
    assert_eq!(
        to("mailto:jürgen@Müller.example"),
        ["jürgen@xn--mller-kva.example"]
    );
}

#[test]
fn encoded_subject_and_body() {
    let request = parse(
        "mailto:hans@example.de?subject=Gr%C3%BC%C3%9Fe%20aus%20K%C3%B6ln%20%F0%9F%98%80\
         &body=Hallo%20J%C3%BCrgen%2C%0D%0A%0D%0Aa%26b%3Dc%3Fd%23e%0AZeile%093%0DEnde%0D%0A",
    );
    assert_eq!(request.subject.as_deref(), Some("Grüße aus Köln 😀"));
    assert_eq!(
        request.body.as_deref(),
        Some("Hallo Jürgen,\n\na&b=c?d#e\nZeile\t3\nEnde\n"),
        "CRLF, lone LF and lone CR become \\n; tabs stay; &, =, ? and # are text"
    );
    assert!(request.notices.is_empty());
    // `%20` is the space; `+` stays a plus sign (RFC 6068 section 5).
    let request = parse("mailto:bill+ietf@example.org?subject=1+1%202&body=a+b%2Bc");
    assert_eq!(emails(&request.to), ["bill+ietf@example.org"]);
    assert_eq!(request.subject.as_deref(), Some("1+1 2"));
    assert_eq!(request.body.as_deref(), Some("a+b+c"));
    // The body is not a header: encoded words stay as written.
    assert_eq!(
        parse("mailto:?body=%3D%3FUTF-8%3FQ%3Fcaf%3DC3%3DA9%3F%3D")
            .body
            .as_deref(),
        Some("=?UTF-8?Q?caf=C3=A9?=")
    );
}

#[test]
fn rfc2047_subjects_and_names() {
    let subject = |encoded: &str| parse(&format!("mailto:?subject={encoded}")).subject;
    assert_eq!(
        subject("%3D%3FUTF-8%3FB%3Fw4RyZ2VyIMO8YmVyIMOWbA%3D%3D%3F%3D").as_deref(),
        Some("Ärger über Öl")
    );
    assert_eq!(
        subject("%3D%3FISO-8859-1%3FQ%3FM%3DFCller_gr%3DFC%3DDFt%3F%3D").as_deref(),
        Some("Müller grüßt")
    );
    assert_eq!(
        subject("Re%3A%20%3D%3FUTF-8%3FQ%3Fcaf%3DC3%3DA9%3F%3D%20ok").as_deref(),
        Some("Re: café ok")
    );
    // Not an encoded word: stays as written.
    assert_eq!(subject("x%3D%3Fy").as_deref(), Some("x=?y"));
    // A line break inside an encoded word does not survive either.
    let request =
        parse("mailto:?subject=%3D%3FUTF-8%3FQ%3Fa%3D0D%3D0ABcc%3A_x%40example.net%3F%3D");
    assert_eq!(request.subject.as_deref(), Some("a Bcc: x@example.net"));
    assert!(request.bcc.is_empty());
    assert_eq!(
        request.notices,
        [MailtoNotice::ControlCharactersRemoved {
            field: MailtoField::Subject
        }]
    );

    // Display names: encoded words, quotes, plain words.
    let request = parse(
        "mailto:?to=%3D%3FUTF-8%3FQ%3FJ%3DC3%3DB6rg%3F%3D%20%3Cjoerg@example.org%3E\
         &cc=%22Doe,%20John%22%20%3Cjohn@example.com%3E,%20Jane%20Roe%20%3Cjane@example.com%3E,\
         %3Cbare@example.com%3E,%20same%20%3Csame@example.com%3E",
    );
    assert_eq!(
        request.to,
        [Address {
            name: Some("Jörg".into()),
            email: "joerg@example.org".into()
        }]
    );
    let names: Vec<_> = request.cc.iter().map(|a| a.name.as_deref()).collect();
    assert_eq!(
        emails(&request.cc),
        [
            "john@example.com",
            "jane@example.com",
            "bare@example.com",
            "same@example.com"
        ]
    );
    assert_eq!(
        names,
        [Some("Doe, John"), Some("Jane Roe"), None, None],
        "a name that is the local part is dropped"
    );
}

#[test]
fn blind_copies_are_flagged_first() {
    let request = parse(
        "mailto:a@example.org?x-priority=1&subject=Hi&bcc=boss@example.org,%20audit@example.org",
    );
    assert_eq!(
        emails(&request.bcc),
        ["boss@example.org", "audit@example.org"]
    );
    assert_eq!(
        request.notices,
        [MailtoNotice::BccPresent, ignored("x-priority")]
    );
    // A blind copy that is already a recipient adds nothing, so there is nothing to flag.
    let request = parse("mailto:a@example.org?bcc=A@example.org");
    assert!(request.bcc.is_empty());
    assert!(request.notices.is_empty());
}

#[test]
fn dangerous_headers_are_never_applied() {
    let request = parse(
        "mailto:a@example.org?from=evil@example.net&Reply-To=evil@example.net\
         &sender=evil@example.net&return-path=%3Cevil@example.net%3E&x-mailer=Evil\
         &content-type=text/html&Content-Transfer-Encoding=base64&MIME-Version=1.0\
         &received=from%20evil&FROM=other@example.net&references=%3Cx@y%3E&body=hi",
    );
    assert_eq!(emails(&request.to), ["a@example.org"]);
    assert!(request.cc.is_empty() && request.bcc.is_empty());
    assert_eq!(request.subject, None);
    assert_eq!(request.body.as_deref(), Some("hi"));
    assert_eq!(
        request.notices,
        [
            ignored("from"),
            ignored("reply-to"),
            ignored("sender"),
            ignored("return-path"),
            ignored("x-mailer"),
            ignored("content-type"),
            ignored("content-transfer-encoding"),
            ignored("mime-version"),
            ignored("received"),
            ignored("references"),
        ],
        "in the order of the link, each name once, in lower case"
    );
    // Field names are percent-decoded and compared ignoring ASCII case only.
    let request = parse("mailto:?%73ubject=Hi&SUBJECT=Again&\u{17f}ubject=long-s");
    assert_eq!(request.subject.as_deref(), Some("Hi"));
    assert_eq!(
        request.notices,
        [
            ignored("\u{17f}ubject"),
            MailtoNotice::DuplicateIgnored {
                field: MailtoField::Subject
            },
        ]
    );
}

#[test]
fn header_injection_stays_text() {
    let request = parse("mailto:a@example.org?subject=Hi%0D%0ABcc:%20evil@example.net");
    assert_eq!(request.subject.as_deref(), Some("Hi Bcc: evil@example.net"));
    assert!(request.bcc.is_empty());
    assert_eq!(
        request.notices,
        [MailtoNotice::ControlCharactersRemoved {
            field: MailtoField::Subject
        }]
    );
    for uri in [
        "mailto:?subject=Hi%0ABcc:%20evil@example.net",
        "mailto:?subject=Hi%0DBcc:%20evil@example.net",
    ] {
        assert_eq!(
            parse(uri).subject.as_deref(),
            Some("Hi Bcc: evil@example.net"),
            "{uri}"
        );
    }
    assert_eq!(
        parse("mailto:?subject=a%00b%07c%09d").subject.as_deref(),
        Some("a b c d")
    );

    // In the path or a recipient field, a line break makes the address invalid.
    let request = parse("mailto:a@example.org%0D%0ABcc:%20evil@example.net");
    assert!(request.to.is_empty() && request.bcc.is_empty());
    assert_eq!(
        request.notices,
        [invalid("a@example.org Bcc: evil@example.net")]
    );
    let request = parse("mailto:?to=a@example.org%0ABcc:evil@example.net");
    assert!(request.to.is_empty() && request.bcc.is_empty());

    // In the body, control characters other than line breaks and tabs are dropped.
    let request = parse("mailto:?body=a%00b%1B[31mc%7F%0D%0A.%0D%0Aend");
    assert_eq!(request.body.as_deref(), Some("ab[31mc\n.\nend"));
    assert_eq!(
        request.notices,
        [MailtoNotice::ControlCharactersRemoved {
            field: MailtoField::Body
        }]
    );
    // A name cannot smuggle a line break into a header either.
    let request = parse("mailto:?to=Eve%0D%0AX-Evil:%201%20%3Ceve@example.org%3E");
    assert_eq!(
        request.to,
        [Address {
            name: Some("Eve X-Evil: 1".into()),
            email: "eve@example.org".into()
        }]
    );
}

#[test]
fn invalid_percent_escapes_and_utf8() {
    // `%` without two hex digits stays as written.
    assert_eq!(
        parse("mailto:?subject=100%G1%25%").subject.as_deref(),
        Some("100%G1%%")
    );
    assert_eq!(parse("mailto:?subject=a%2").subject.as_deref(), Some("a%2"));
    // Bytes that are not UTF-8 become U+FFFD and are reported.
    let request = parse("mailto:?subject=caf%E9&body=%FF%FEab%C3");
    assert_eq!(request.subject.as_deref(), Some("caf\u{FFFD}"));
    assert_eq!(request.body.as_deref(), Some("\u{FFFD}\u{FFFD}ab\u{FFFD}"));
    assert_eq!(
        request.notices,
        [
            MailtoNotice::InvalidEncoding {
                field: MailtoField::Subject
            },
            MailtoNotice::InvalidEncoding {
                field: MailtoField::Body
            },
        ]
    );
    // An address with such bytes is never used.
    let request = parse("mailto:a%FF@example.org,b@example.org");
    assert_eq!(emails(&request.to), ["b@example.org"]);
    assert_eq!(request.notices, [invalid("a\u{FFFD}@example.org")]);
    // A lone `%` in a local part is a valid character (as written).
    assert_eq!(to("mailto:a%G1@example.org"), ["a%G1@example.org"]);
}

#[test]
fn invalid_addresses_are_left_out() {
    let request = parse(
        "mailto:alice,bob@,@example.org,a%20b@example.org,,good@example.org,a@-x.example,\
         a@b@c,a..b@example.org,%3Cbroken@example.org",
    );
    assert_eq!(emails(&request.to), ["good@example.org"]);
    assert_eq!(
        request.notices,
        [
            invalid("alice"),
            invalid("bob@"),
            invalid("@example.org"),
            invalid("a b@example.org"),
            invalid("a@-x.example"),
            invalid("a@b@c"),
            invalid("a..b@example.org"),
            invalid("<broken@example.org"),
        ],
        "empty items are skipped silently"
    );
}

#[test]
fn recipients_accumulate_and_are_unique() {
    let request = parse(
        "mailto:a@example.org?to=b@example.org&to=c@example.org,d@example.org;e@example.org\
         &cc=f@example.org&cc=g@example.org",
    );
    assert_eq!(
        emails(&request.to),
        [
            "a@example.org",
            "b@example.org",
            "c@example.org",
            "d@example.org",
            "e@example.org"
        ]
    );
    assert_eq!(emails(&request.cc), ["f@example.org", "g@example.org"]);
    // `%2C` separates like `,` (as in `?to=joe@example.com%2C%20bob@example.com`).
    assert_eq!(
        to("mailto:?to=joe@example.com%2C%20bob@example.com"),
        ["joe@example.com", "bob@example.com"]
    );

    // To wins over Cc over Bcc, whatever the order in the link; case and IDNA do not matter.
    let request = parse(
        "mailto:Alice@Example.org?bcc=BOB@example.org,carol@example.org\
         &cc=alice@example.ORG,bob@example.org,user@xn--mller-kva.example\
         &to=bob@example.org,alice@example.org&to=user@m%C3%BCller.example",
    );
    assert_eq!(
        emails(&request.to),
        [
            "Alice@example.org",
            "bob@example.org",
            "user@xn--mller-kva.example"
        ]
    );
    assert!(request.cc.is_empty());
    assert_eq!(emails(&request.bcc), ["carol@example.org"]);
    assert_eq!(request.notices, [MailtoNotice::BccPresent]);
}

#[test]
fn single_fields_first_wins() {
    let request = parse(
        "mailto:?subject=First&subject=Second&body=&body=B&in-reply-to=%3Ca@example.org%3E\
         &In-Reply-To=%3Cc@example.org%3E&subject=&subject=%20",
    );
    assert_eq!(request.subject.as_deref(), Some("First"));
    assert_eq!(
        request.body.as_deref(),
        Some("B"),
        "an empty body is not one"
    );
    assert_eq!(request.in_reply_to.as_deref(), Some("a@example.org"));
    assert_eq!(
        request.notices,
        [
            MailtoNotice::DuplicateIgnored {
                field: MailtoField::Subject
            },
            MailtoNotice::DuplicateIgnored {
                field: MailtoField::InReplyTo
            },
        ]
    );
    // A subject that is only white space is none; the next one is used.
    assert_eq!(
        parse("mailto:?subject=%20%0D%0A&subject=Real")
            .subject
            .as_deref(),
        Some("Real")
    );
}

#[test]
fn in_reply_to_forms() {
    let id = |uri: &str| parse(uri).in_reply_to;
    assert_eq!(
        id("mailto:?in-reply-to=abc@example.com").as_deref(),
        Some("abc@example.com")
    );
    assert_eq!(
        id("mailto:?IN-REPLY-TO=%20%3Cabc@example.com%3E%20").as_deref(),
        Some("abc@example.com")
    );
    for (uri, value) in [
        ("mailto:?in-reply-to=not-an-id", "not-an-id"),
        ("mailto:?in-reply-to=%3Ca@b%3E%20%3Cc@d%3E", "<a@b> <c@d>"),
        ("mailto:?in-reply-to=%3C@example.com%3E", "<@example.com>"),
        (
            "mailto:?in-reply-to=a%0D%0Ab@example.com",
            "a b@example.com",
        ),
    ] {
        let request = parse(uri);
        assert_eq!(request.in_reply_to, None, "{uri}");
        assert_eq!(
            request.notices,
            [MailtoNotice::MessageIdInvalid {
                value: value.to_owned()
            }],
            "{uri}"
        );
    }
}

#[test]
fn scheme_whitespace_and_fragment() {
    let request = parse("MAILTO:Joe@Example.COM?SUBJECT=Hi&Cc=bob@example.com");
    assert_eq!(
        emails(&request.to),
        ["Joe@example.com"],
        "local part as written"
    );
    assert_eq!(emails(&request.cc), ["bob@example.com"]);
    assert_eq!(request.subject.as_deref(), Some("Hi"));
    assert_eq!(to(" \tMailTo:a@example.org\r\n"), ["a@example.org"]);
    let request = parse("mailto:a@example.org?subject=Hi#&bcc=evil@example.net");
    assert_eq!(request.subject.as_deref(), Some("Hi"));
    assert!(request.bcc.is_empty() && request.notices.is_empty());
    assert_eq!(
        parse("mailto:?subject=Issue%20%2342").subject.as_deref(),
        Some("Issue #42")
    );
    // An HTML link copied with its `&amp;`: the field name is not a known one.
    let request = parse("mailto:joe@example.com?cc=bob@example.com&amp;body=hello");
    assert_eq!(request.body, None);
    assert_eq!(request.notices, [ignored("amp;body")]);
}

#[test]
fn empty_links() {
    for uri in [
        "mailto:",
        "mailto:?",
        "mailto:?&&",
        "mailto:#top",
        "mailto:?to=&cc=&bcc=&subject=&body=&in-reply-to=",
        "mailto:?subject&body",
        "mailto:?=x",
        "mailto:,%20,",
    ] {
        assert_eq!(parse(uri), ComposeRequest::default(), "{uri}");
    }
}

#[test]
fn not_mailto() {
    for uri in [
        "",
        "mailto",
        "mail:a@example.org",
        "https://example.org/?mailto:a@example.org",
        "xmailto:a@example.org",
        "javascript:alert(1)//mailto:",
        "ma\u{e9}lto:a@example.org",
    ] {
        assert_eq!(parse_mailto(uri), Err(MailtoError::NotMailto), "{uri}");
    }
    let error = CoreError::from(MailtoError::NotMailto);
    assert_eq!(error.code(), ErrorCode::InvalidArgument);
    assert_eq!(error.detail().as_deref(), Some("not a mailto: link"));
}

#[test]
fn length_limits() {
    let fill = |prefix: &str, len: usize| {
        let mut uri = prefix.to_owned();
        while uri.len() < len {
            uri.push('a');
        }
        uri
    };
    let longest = fill("mailto:?body=", MAX_MAILTO_URI_BYTES);
    assert!(parse_mailto(&longest).is_ok());
    let too_long = fill("mailto:?body=", MAX_MAILTO_URI_BYTES + 1);
    assert_eq!(
        parse_mailto(&too_long),
        Err(MailtoError::TooLong {
            bytes: MAX_MAILTO_URI_BYTES + 1,
            max: MAX_MAILTO_URI_BYTES
        })
    );
    assert!(
        CoreError::from(MailtoError::TooLong { bytes: 1, max: 0 })
            .detail()
            .is_some_and(|d| !d.contains("mailto:?")),
        "the link itself is not in the error"
    );

    // The subject is cut at MAX_SUBJECT_CHARS characters.
    let subject = "%C3%BC".repeat(MAX_SUBJECT_CHARS + 10);
    let request = parse(&format!("mailto:?subject={subject}"));
    let cut = request.subject.expect("subject");
    assert_eq!(cut.chars().count(), MAX_SUBJECT_CHARS);
    assert!(cut.chars().all(|c| c == 'ü'));
    assert_eq!(
        request.notices,
        [MailtoNotice::Truncated {
            field: MailtoField::Subject
        }]
    );

    // The body is cut at MAX_MAILTO_BODY_BYTES on a character boundary ("ü" is two bytes;
    // written as is, since percent-encoded it would not fit into the longest link).
    let body = format!("x{}", "ü".repeat(MAX_MAILTO_BODY_BYTES / 2 + 10));
    let request = parse(&format!("mailto:?body={body}"));
    let cut = request.body.expect("body");
    assert_eq!(cut.len(), MAX_MAILTO_BODY_BYTES - 1);
    assert!(cut.starts_with('x') && cut[1..].chars().all(|c| c == 'ü'));
    assert_eq!(
        request.notices,
        [MailtoNotice::Truncated {
            field: MailtoField::Body
        }]
    );
}

#[test]
fn recipient_limit() {
    let list = |prefix: &str, count: usize| {
        (0..count)
            .map(|i| format!("{prefix}{i}@example.org"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let request = parse(&format!(
        "mailto:{}?cc={}&bcc={}",
        list("t", 300),
        list("c", 300),
        list("b", 10)
    ));
    assert_eq!(request.to.len(), 300);
    assert_eq!(request.cc.len(), MAX_RECIPIENTS - 300);
    assert!(request.bcc.is_empty());
    assert_eq!(
        request.cc.last().map(|a| a.email.as_str()),
        Some("c199@example.org")
    );
    assert_eq!(
        request.notices,
        [
            MailtoNotice::Truncated {
                field: MailtoField::Cc
            },
            MailtoNotice::Truncated {
                field: MailtoField::Bcc
            },
        ],
        "no BccPresent: no blind copy is left"
    );
    let request = parse(&format!("mailto:{}", list("t", MAX_RECIPIENTS)));
    assert_eq!(request.to.len(), MAX_RECIPIENTS);
    assert!(request.notices.is_empty());
}

#[test]
fn very_long_links_stay_fast_and_bounded() {
    let fill = |prefix: &str, item: &str| {
        let mut uri = prefix.to_owned();
        while uri.len() + item.len() <= MAX_MAILTO_URI_BYTES {
            uri.push_str(item);
        }
        uri
    };
    let started = Instant::now();

    // Thousands of copies of one address: one recipient.
    let request = parse(&fill("mailto:", "A@example.org,"));
    assert_eq!(emails(&request.to), ["A@example.org"]);
    assert!(request.notices.is_empty());

    // Thousands of invalid addresses and unknown fields: the hints stay few.
    let mut uri = String::from("mailto:");
    let mut i = 0;
    while uri.len() < MAX_MAILTO_URI_BYTES - 20 {
        uri.push_str(&format!("bad{i},"));
        i += 1;
    }
    let request = parse(&uri);
    assert!(request.to.is_empty());
    assert_eq!(request.notices.len(), MAX_MAILTO_VALUE_NOTICES + 1);
    assert_eq!(request.notices[0], invalid("bad0"));
    assert_eq!(
        request.notices.last(),
        Some(&MailtoNotice::NoticesOmitted {
            count: u32::try_from(i - MAX_MAILTO_VALUE_NOTICES).expect("count")
        })
    );
    let request = parse(&fill("mailto:?", "x-a=1&x-b=2&"));
    assert_eq!(
        request.notices,
        [ignored("x-a"), ignored("x-b")],
        "each name once"
    );

    // A long value in a hint is cut.
    let request = parse(&format!("mailto:{}", "x".repeat(4000)));
    let [MailtoNotice::AddressInvalid { value }] = request.notices.as_slice() else {
        panic!("{:?}", request.notices);
    };
    assert_eq!(value.chars().count(), MAX_MAILTO_NOTICE_VALUE_CHARS);

    // Thousands of encoded words and line breaks.
    let request = parse(&fill("mailto:?subject=", "%3D%3FUTF-8%3FQ%3Fa%3F%3D%0D%0A"));
    assert!(request.subject.is_some());
    let request = parse(&fill("mailto:?body=", "%0D%0A%0D"));
    assert_eq!(request.body, None, "only line breaks");

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}
