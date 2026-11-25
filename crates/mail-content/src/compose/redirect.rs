//! Stage 19 (decision 0055): redirecting a message ("Umleiten", RFC 5322 section 3.6.6).
//!
//! The original goes to new recipients as it is: a block of `Resent-*` fields is put in front
//! of its header, and the envelope names the new recipients. Nothing of the original is
//! re-encoded or rebuilt. Three kinds of field are taken out of the original's header first,
//! because sending them again would do harm:
//! - `Bcc`: a copy from the user's own Sent folder may list blind copies; redirecting it must
//!   not reveal them.
//! - `Delivered-To` and `Return-Path`: trace fields of the earlier delivery. The receiving
//!   server adds its own; an old `Delivered-To` that names the new recipient makes servers such
//!   as Postfix refuse the message as a mail loop.
//!
//! `Resent-Bcc` is never written (like `Bcc` in [`super::build_message`]): blind recipients are
//! only in the envelope. Line ends become CRLF (SMTP needs them; a stored message from IMAP has
//! them already).

use mail_builder::headers::date::Date;
use mail_builder::headers::message_id::MessageId;
use mail_model::{Address, Envelope};

use super::address::is_valid_message_id;
use super::build::{ComposeError, address_header, crlf_bytes, mailbox, recipients, write_header};

/// Header fields of the original that a redirected copy leaves out (compared without case).
const DROPPED_FIELDS: &[&str] = &["bcc", "delivered-to", "return-path"];

/// What a redirect adds: the user's identity and the new recipients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// The identity that redirects (`Resent-From`, envelope sender).
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Only in the envelope; no `Resent-Bcc` field is written.
    pub bcc: Vec<Address>,
    /// `Resent-Date`, Unix seconds.
    pub date: i64,
    /// `Resent-Message-ID`, without angle brackets.
    pub message_id: String,
}

/// The bytes to submit and the envelope of a redirect.
#[derive(Clone, PartialEq, Eq)]
pub struct BuiltRedirect {
    pub bytes: Vec<u8>,
    pub envelope: Envelope,
}

impl std::fmt::Debug for BuiltRedirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltRedirect")
            .field("bytes", &self.bytes.len())
            .field("recipients", &self.envelope.recipients.len())
            .finish()
    }
}

/// The original with a `Resent-*` block in front, and the envelope to the new recipients.
/// `NoRecipients` without any, `InvalidAddress` for a bad address, `InvalidMessageId` for a
/// bad Resent-Message-ID.
pub fn build_redirect(original: &[u8], redirect: &Redirect) -> Result<BuiltRedirect, ComposeError> {
    let from = mailbox(&redirect.from)?;
    let mut seen = Vec::new();
    let to = recipients(&redirect.to, &mut seen)?;
    let cc = recipients(&redirect.cc, &mut seen)?;
    let bcc = recipients(&redirect.bcc, &mut seen)?;
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(ComposeError::NoRecipients);
    }
    let message_id = redirect.message_id.trim();
    let message_id = message_id
        .strip_prefix('<')
        .and_then(|id| id.strip_suffix('>'))
        .unwrap_or(message_id);
    if !is_valid_message_id(message_id) {
        return Err(ComposeError::InvalidMessageId(redirect.message_id.clone()));
    }
    let envelope = Envelope {
        from: from.1.clone(),
        recipients: to
            .iter()
            .chain(&cc)
            .chain(&bcc)
            .map(|(_, email)| email.clone())
            .collect(),
    };

    // RFC 5322, 3.6.6: Resent-Date, Resent-From, Resent-To, Resent-Cc, Resent-Message-ID.
    let mut bytes = Vec::with_capacity(original.len() + 512);
    write_header(&mut bytes, "Resent-Date", &Date::new(redirect.date).into());
    write_header(
        &mut bytes,
        "Resent-From",
        &address_header(std::slice::from_ref(&from)),
    );
    if !to.is_empty() {
        write_header(&mut bytes, "Resent-To", &address_header(&to));
    }
    if !cc.is_empty() {
        write_header(&mut bytes, "Resent-Cc", &address_header(&cc));
    }
    write_header(
        &mut bytes,
        "Resent-Message-ID",
        &MessageId::new(message_id.to_owned()).into(),
    );
    bytes.extend_from_slice(&without_fields(&crlf_bytes(original), DROPPED_FIELDS));
    Ok(BuiltRedirect { bytes, envelope })
}

/// The message without the header fields named in `names` (with their continuation lines);
/// the body and every other byte stay as they are. `message` has CRLF line ends.
fn without_fields(message: &[u8], names: &[&str]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len());
    let mut rest = message;
    let mut dropping = false;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .map_or(rest.len(), |i| i + 1);
        let (line, tail) = rest.split_at(end);
        rest = tail;
        let content = line.strip_suffix(b"\r\n").unwrap_or(line);
        if content.is_empty() {
            // End of the header block: the rest is the body, unchanged.
            out.extend_from_slice(line);
            out.extend_from_slice(rest);
            break;
        }
        let continuation = matches!(content.first(), Some(b' ' | b'\t'));
        if !continuation {
            dropping = content
                .iter()
                .position(|b| *b == b':')
                .and_then(|colon| content.get(..colon))
                .map(|name| {
                    let name = String::from_utf8_lossy(name);
                    let name = name.trim();
                    names.iter().any(|n| name.eq_ignore_ascii_case(n))
                })
                .unwrap_or(false);
        }
        if !dropping {
            out.extend_from_slice(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(name: Option<&str>, email: &str) -> Address {
        Address {
            name: name.map(str::to_owned),
            email: email.to_owned(),
        }
    }

    fn redirect() -> Redirect {
        Redirect {
            from: address(Some("Alice Example"), "alice@example.org"),
            to: vec![address(Some("Carol"), "carol@example.net")],
            cc: vec![address(None, "dave@example.net")],
            bcc: vec![address(None, "erin@example.net")],
            // Mon, 23 Nov 2026 13:05:00 +0000
            date: 1_795_439_100,
            message_id: "redirect.1@example.org".into(),
        }
    }

    const ORIGINAL: &[u8] = b"Return-Path: <bob@example.com>\r\n\
Delivered-To: alice@example.org\r\n\
Received: from mx.example.com by mx.example.org;\r\n\
\tMon, 23 Nov 2026 12:00:00 +0000\r\n\
From: Bob <bob@example.com>\r\n\
To: Alice <alice@example.org>\r\n\
Bcc: secret@example.com,\r\n\
\x20other@example.com\r\n\
Subject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n\
Message-ID: <orig.1@example.com>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: 8bit\r\n\
\r\n\
Hallo, sch\xc3\xb6ne Gr\xc3\xbc\xc3\x9fe.\r\n\
Bcc: this line is body text and stays\r\n";

    #[test]
    fn resent_fields_come_first_and_the_original_follows_unchanged() {
        let built = build_redirect(ORIGINAL, &redirect()).expect("built");
        let text = String::from_utf8_lossy(&built.bytes);
        let head_end = text.find("Received:").expect("original header");
        let resent = &text[..head_end];
        assert!(resent.starts_with("Resent-Date: Mon, 23 Nov 2026 13:05:00 +0000\r\n"));
        assert!(resent.contains("Resent-From: \"Alice Example\" <alice@example.org>\r\n"));
        assert!(resent.contains("Resent-To: \"Carol\" <carol@example.net>\r\n"));
        assert!(resent.contains("Resent-Cc: <dave@example.net>\r\n"));
        assert!(resent.ends_with("Resent-Message-ID: <redirect.1@example.org>\r\n"));
        assert!(!text.contains("Resent-Bcc"), "{text}");
        assert!(
            !text.contains("erin@"),
            "blind recipients only in the envelope"
        );
        // The original without Bcc, Delivered-To and Return-Path, byte for byte otherwise.
        let expected = b"Received: from mx.example.com by mx.example.org;\r\n\
\tMon, 23 Nov 2026 12:00:00 +0000\r\n\
From: Bob <bob@example.com>\r\n\
To: Alice <alice@example.org>\r\n\
Subject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n\
Message-ID: <orig.1@example.com>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: 8bit\r\n\
\r\n\
Hallo, sch\xc3\xb6ne Gr\xc3\xbc\xc3\x9fe.\r\n\
Bcc: this line is body text and stays\r\n";
        assert!(
            built.bytes.ends_with(expected),
            "{text:?}\n{:?}",
            String::from_utf8_lossy(expected)
        );
        assert_eq!(built.bytes.len(), head_end + expected.len());
        assert_eq!(built.envelope.from, "alice@example.org");
        assert_eq!(
            built.envelope.recipients,
            vec![
                "carol@example.net".to_owned(),
                "dave@example.net".to_owned(),
                "erin@example.net".to_owned()
            ]
        );
    }

    #[test]
    fn bare_line_feeds_become_crlf_and_duplicates_go_once() {
        let mut request = redirect();
        request.cc = vec![address(None, "CAROL@example.net")];
        request.bcc.clear();
        let built =
            build_redirect(b"From: x@example.com\nSubject: s\n\nbody\n", &request).expect("built");
        assert!(
            built
                .bytes
                .ends_with(b"From: x@example.com\r\nSubject: s\r\n\r\nbody\r\n")
        );
        assert!(!String::from_utf8_lossy(&built.bytes).contains("Resent-Cc"));
        assert_eq!(built.envelope.recipients, vec!["carol@example.net"]);
    }

    #[test]
    fn redirects_need_recipients_valid_addresses_and_a_message_id() {
        let mut none = redirect();
        none.to.clear();
        none.cc.clear();
        none.bcc.clear();
        assert_eq!(
            build_redirect(ORIGINAL, &none),
            Err(ComposeError::NoRecipients)
        );
        let mut bad = redirect();
        bad.to = vec![address(None, "not an address")];
        assert!(matches!(
            build_redirect(ORIGINAL, &bad),
            Err(ComposeError::InvalidAddress(_))
        ));
        let mut id = redirect();
        id.message_id = "no at sign".into();
        assert!(matches!(
            build_redirect(ORIGINAL, &id),
            Err(ComposeError::InvalidMessageId(_))
        ));
        // Only Bcc: no Resent-To, still sent.
        let mut blind = redirect();
        blind.to.clear();
        blind.cc.clear();
        let built = build_redirect(ORIGINAL, &blind).expect("bcc only");
        assert!(!String::from_utf8_lossy(&built.bytes).contains("Resent-To"));
        assert_eq!(built.envelope.recipients, vec!["erin@example.net"]);
    }

    #[test]
    fn a_message_without_body_or_header_end_keeps_what_it_has() {
        let built = build_redirect(b"Subject: only a header\r\nBcc: x@example.com", &redirect())
            .expect("built");
        assert!(built.bytes.ends_with(
            b"Resent-Message-ID: <redirect.1@example.org>\r\nSubject: only a header\r\n"
        ));
    }
}
