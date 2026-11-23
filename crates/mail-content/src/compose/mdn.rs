//! Read receipts (MDN, RFC 8098; stage 16, decision 0033), without IO.
//!
//! - [`receipt_request`]: does an incoming message ask for a read receipt
//!   (`Disposition-Notification-To`)? Only a single valid address is honoured; a read receipt
//!   itself never asks for one (RFC 8098 2.1), and the answer says whether the address differs
//!   from the `Return-Path` (then RFC 8098 asks to confirm with the user; the app always asks).
//! - [`parse_report`]: a received read receipt (`multipart/report;
//!   report-type=disposition-notification`): the Message-ID it reports on, the recipient and the
//!   disposition.
//! - Building: an [`crate::OutgoingMessage`] with [`DispositionReport`] becomes a
//!   `multipart/report` with the human-readable text ([`receipt_text`]) and the
//!   `message/disposition-notification` part (`Final-Recipient`, `Original-Recipient` when the
//!   message had one, `Original-Message-ID`, `Disposition: manual-action/MDN-sent-manually;
//!   displayed`). No `Reporting-UA`: the message does not name the program. The original's
//!   headers are not attached.

use std::borrow::Cow;
use std::panic::{self, AssertUnwindSafe};

use mail_builder::headers::content_type::ContentType;
use mail_builder::mime::{BodyPart, MimePart};
use mail_model::ReceiptDisposition;
use mail_parser::MessageParser;

use super::QuoteLanguage;
use super::address::{is_valid_message_id, normalize_email};
use super::build::{crlf, fnv1a};

/// The header block read for a request; anything longer is not a header block.
const MAX_HEADER_BYTES: usize = 256 * 1024;
/// Largest report part read.
const MAX_REPORT_BYTES: usize = 16 * 1024;

/// The machine-readable part of a read receipt the app sends (RFC 8098 3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispositionReport {
    /// The address that received the message (`Final-Recipient`): the identity's.
    pub final_recipient: String,
    /// The message's `Original-Recipient` address, if it had one.
    pub original_recipient: Option<String>,
    /// The message's Message-ID, without angle brackets.
    pub original_message_id: Option<String>,
}

/// An incoming message's request for a read receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRequest {
    /// Where the receipt goes (normalised, ASCII domain).
    pub address: String,
    /// The address is not the `Return-Path` (or there is none).
    pub return_path_differs: bool,
    /// `Original-Recipient` of the message (the address only), for the report.
    pub original_recipient: Option<String>,
}

/// A read receipt as received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedReport {
    /// The Message-ID it reports on, without angle brackets.
    pub original_message_id: String,
    /// `Final-Recipient`, the address only.
    pub final_recipient: Option<String>,
    pub disposition: ReceiptDisposition,
}

/// The top-level header block of `raw` (up to the first empty line).
fn header_block(raw: &[u8]) -> &[u8] {
    let limit = raw.len().min(MAX_HEADER_BYTES);
    let head = raw.get(..limit).unwrap_or(raw);
    for (i, w) in head.windows(2).enumerate() {
        if w == b"\n\n" {
            return head.get(..i + 1).unwrap_or(head);
        }
        if w == b"\n\r" && head.get(i + 2) == Some(&b'\n') {
            return head.get(..i + 1).unwrap_or(head);
        }
    }
    head
}

/// Header fields of a block, unfolded: lower-case name and trimmed value. Lines without a
/// colon are skipped.
fn fields(block: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(block);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = out.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    out
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The one address of a field value (`Name <a@b>` or `a@b`); `None` for none, several or an
/// invalid one.
fn single_address(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = if value.contains('<') {
        if value.matches('<').count() != 1 || value.matches('>').count() != 1 {
            return None;
        }
        let start = value.find('<')? + 1;
        let end = value.find('>')?;
        value.get(start..end)?
    } else {
        if value.contains(',') || value.contains(' ') {
            return None;
        }
        value
    };
    normalize_email(inner.trim()).ok()
}

/// The address after `rfc822;` (`Final-Recipient`, `Original-Recipient`).
fn typed_address(value: &str) -> Option<String> {
    let (kind, address) = value.split_once(';')?;
    if !kind.trim().eq_ignore_ascii_case("rfc822") {
        return None;
    }
    normalize_email(address.trim()).ok()
}

fn message_id_of(value: &str) -> Option<String> {
    let value = value.trim();
    let id = value
        .strip_prefix('<')
        .and_then(|v| v.split_once('>').map(|(id, _)| id))
        .unwrap_or(value)
        .trim();
    is_valid_message_id(id).then(|| id.to_owned())
}

/// `Content-Type: multipart/report; report-type=disposition-notification`.
fn is_disposition_report(fields: &[(String, String)]) -> bool {
    let Some(value) = field(fields, "content-type") else {
        return false;
    };
    let value = value.to_ascii_lowercase().replace([' ', '\t', '"'], "");
    value.starts_with("multipart/report")
        && value
            .split(';')
            .any(|p| p == "report-type=disposition-notification")
}

/// Whether `raw` asks for a read receipt the app may offer to send (see module docs).
pub fn receipt_request(raw: &[u8]) -> Option<ReceiptRequest> {
    let fields = fields(header_block(raw));
    if is_disposition_report(&fields) {
        return None;
    }
    let requests: Vec<&str> = fields
        .iter()
        .filter(|(n, _)| n == "disposition-notification-to")
        .map(|(_, v)| v.as_str())
        .collect();
    let [request] = requests.as_slice() else {
        return None;
    };
    let address = single_address(request)?;
    let return_path = field(&fields, "return-path").and_then(single_address);
    Some(ReceiptRequest {
        return_path_differs: return_path
            .as_deref()
            .is_none_or(|rp| !rp.eq_ignore_ascii_case(&address)),
        address,
        original_recipient: field(&fields, "original-recipient").and_then(typed_address),
    })
}

/// Whether `raw` is a read receipt (by its header only).
pub fn is_receipt(raw: &[u8]) -> bool {
    is_disposition_report(&fields(header_block(raw)))
}

/// A received read receipt (see module docs); `None` for anything else.
pub fn parse_report(raw: &[u8]) -> Option<ReceivedReport> {
    if !is_receipt(raw) {
        return None;
    }
    panic::catch_unwind(AssertUnwindSafe(|| report_of(raw)))
        .ok()
        .flatten()
}

fn report_of(raw: &[u8]) -> Option<ReceivedReport> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.parts().find(|part| {
        part.content_type().is_some_and(|ct| {
            ct.ctype().eq_ignore_ascii_case("message")
                && ct
                    .subtype()
                    .is_some_and(|s| s.eq_ignore_ascii_case("disposition-notification"))
        })
    })?;
    let body = part.raw_body();
    let body = body.get(..body.len().min(MAX_REPORT_BYTES))?;
    let fields = fields(body);
    let original_message_id = field(&fields, "original-message-id").and_then(message_id_of)?;
    let disposition = field(&fields, "disposition")
        .and_then(|value| value.split_once(';').map(|(_, kind)| kind))
        .map(|kind| {
            let kind = kind.trim().to_ascii_lowercase();
            let kind = kind.split(['/', ' ', '\t']).next().unwrap_or_default();
            ReceiptDisposition::parse(kind).unwrap_or(ReceiptDisposition::Other)
        })?;
    Some(ReceivedReport {
        original_message_id,
        final_recipient: field(&fields, "final-recipient").and_then(typed_address),
        disposition,
    })
}

/// The subject of a read receipt for a message with `subject`.
pub fn receipt_subject(subject: &str, language: QuoteLanguage) -> String {
    let prefix = match language {
        QuoteLanguage::German => "Gelesen",
        QuoteLanguage::English => "Read",
    };
    let subject = subject.trim();
    if subject.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}: {subject}")
    }
}

/// The human-readable part of a read receipt (RFC 8098 3.1: it must not claim more than that
/// the message was displayed).
pub fn receipt_text(subject: &str, recipient: &str, language: QuoteLanguage) -> String {
    let subject = subject.trim();
    match language {
        QuoteLanguage::German => format!(
            "Dies ist eine Lesebestätigung für die Nachricht „{subject}“ an {recipient}.\n\n\
             Die Nachricht wurde auf dem Bildschirm des Empfängers angezeigt. Das heißt nicht, \
             dass sie gelesen oder verstanden wurde.\n"
        ),
        QuoteLanguage::English => format!(
            "This is a read receipt for the message \"{subject}\" to {recipient}.\n\n\
             The message was displayed on the recipient's screen. This is no guarantee that it \
             has been read or understood.\n"
        ),
    }
}

/// Only printable ASCII without `;` breaks: a value that cannot start a field of its own.
fn report_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The `multipart/report` body of a read receipt (root part headers included).
pub(super) fn report_body(
    report: &DispositionReport,
    text: Option<&str>,
    message_id: &str,
) -> Vec<u8> {
    let mut fields = String::new();
    if let Some(original) = report.original_recipient.as_deref() {
        fields.push_str(&format!(
            "Original-Recipient: rfc822;{}\r\n",
            report_value(original)
        ));
    }
    fields.push_str(&format!(
        "Final-Recipient: rfc822;{}\r\n",
        report_value(&report.final_recipient)
    ));
    if let Some(id) = report
        .original_message_id
        .as_deref()
        .filter(|id| is_valid_message_id(id))
    {
        fields.push_str(&format!("Original-Message-ID: <{}>\r\n", report_value(id)));
    }
    fields.push_str("Disposition: manual-action/MDN-sent-manually; displayed\r\n");
    let text = crlf(text.unwrap_or_default());
    let seed = format!("{:016x}", fnv1a(message_id.as_bytes()));
    let mut boundary = format!("=_{seed}_r");
    for round in 0u32..16 {
        if !text.contains(&boundary) && !fields.contains(&boundary) {
            break;
        }
        boundary = format!("=_{seed}_{round}_r");
    }
    let root = MimePart::new(
        ContentType::new("multipart/report")
            .attribute("report-type", "disposition-notification")
            .attribute("boundary", boundary),
        BodyPart::Multipart(vec![
            MimePart::new(
                ContentType::new("text/plain").attribute("charset", "utf-8"),
                BodyPart::Text(Cow::Owned(text)),
            ),
            MimePart::new(
                ContentType::new("message/disposition-notification"),
                BodyPart::Text(Cow::Owned(fields)),
            )
            .transfer_encoding("7bit"),
        ]),
    );
    let mut out = Vec::new();
    root.write_part(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use mail_model::Address;

    use super::*;
    use crate::{OutgoingMessage, build_message};

    const REQUEST: &[u8] = b"Return-Path: <bob@example.net>\r\nFrom: Bob <bob@example.net>\r\n\
To: alice@example.org\r\nSubject: Offer\r\nMessage-ID: <offer-1@example.net>\r\n\
Disposition-Notification-To: Bob\r\n <bob@example.net>\r\n\r\nHello\r\n";

    #[test]
    fn requests_are_recognised_with_one_valid_address() {
        let request = receipt_request(REQUEST).expect("request");
        assert_eq!(request.address, "bob@example.net");
        assert!(!request.return_path_differs);

        let other = b"Return-Path: <bounce@lists.example>\r\nDisposition-Notification-To: \
bob@example.net\r\nOriginal-Recipient: rfc822;alice@example.org\r\n\r\nx";
        let request = receipt_request(other).expect("request");
        assert!(request.return_path_differs);
        assert_eq!(
            request.original_recipient.as_deref(),
            Some("alice@example.org")
        );

        for raw in [
            &b"From: a@b.example\r\nSubject: x\r\n\r\nDisposition-Notification-To: a@b.example\r\n"[..],
            b"Disposition-Notification-To: a@b.example, c@d.example\r\n\r\n",
            b"Disposition-Notification-To: <a@b.example> <c@d.example>\r\n\r\n",
            b"Disposition-Notification-To: not an address\r\n\r\n",
            b"Disposition-Notification-To: a@b.example\r\nDisposition-Notification-To: c@d.example\r\n\r\n",
            // A read receipt never asks for one.
            b"Content-Type: multipart/report; report-type=\"disposition-notification\";\r\n \
boundary=x\r\nDisposition-Notification-To: a@b.example\r\n\r\n",
        ] {
            assert_eq!(receipt_request(raw), None, "{}", String::from_utf8_lossy(raw));
        }
    }

    fn receipt() -> OutgoingMessage {
        OutgoingMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.org".into(),
            },
            to: vec![Address {
                name: None,
                email: "bob@example.net".into(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            subject: receipt_subject("Offer", QuoteLanguage::English),
            text: Some(receipt_text(
                "Offer",
                "alice@example.org",
                QuoteLanguage::English,
            )),
            html: Some("<p>ignored</p>".into()),
            attachments: Vec::new(),
            inline_images: Vec::new(),
            message_id: "mdn-1@example.org".into(),
            in_reply_to: Some("offer-1@example.net".into()),
            references: vec!["offer-1@example.net".into()],
            date: 1_795_439_100,
            user_agent: None,
            disposition_notification_to: None,
            report: Some(DispositionReport {
                final_recipient: "alice@example.org".into(),
                original_recipient: Some("alice@example.org".into()),
                original_message_id: Some("offer-1@example.net".into()),
            }),
        }
    }

    #[test]
    fn a_built_receipt_is_a_disposition_report_and_parses_back() {
        let built = build_message(&receipt()).expect("built");
        let text = String::from_utf8(built.for_sending.clone()).expect("ascii and utf-8");
        assert!(text.contains("Content-Type: multipart/report"), "{text}");
        assert!(
            text.contains("report-type=\"disposition-notification\"")
                || text.contains("report-type=disposition-notification"),
            "{text}"
        );
        assert!(
            text.contains("Content-Type: message/disposition-notification"),
            "{text}"
        );
        assert!(
            text.contains("Disposition: manual-action/MDN-sent-manually; displayed"),
            "{text}"
        );
        assert!(
            text.contains("Final-Recipient: rfc822;alice@example.org"),
            "{text}"
        );
        assert!(
            text.contains("Original-Message-ID: <offer-1@example.net>"),
            "{text}"
        );
        assert!(text.contains("Subject: Read: Offer"), "{text}");
        assert!(!text.contains("Reporting-UA"), "{text}");
        assert!(!text.contains("ignored"), "{text}");
        // A receipt never asks for a receipt.
        assert!(!text.contains("Disposition-Notification-To"), "{text}");
        assert_eq!(receipt_request(&built.for_sending), None);
        assert!(is_receipt(&built.for_sending));
        let report = parse_report(&built.for_sending).expect("report");
        assert_eq!(report.original_message_id, "offer-1@example.net");
        assert_eq!(report.final_recipient.as_deref(), Some("alice@example.org"));
        assert_eq!(report.disposition, ReceiptDisposition::Displayed);
        assert_eq!(built.envelope.recipients, ["bob@example.net"]);
    }

    #[test]
    fn reports_of_other_programs_parse_and_anything_else_is_none() {
        let thunderbird = b"From: bob@example.net\r\nTo: alice@example.org\r\n\
Subject: Return Receipt (displayed) - Offer\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/report; report-type=disposition-notification;\r\n\tboundary=\"b1\"\r\n\r\n\
--b1\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nThis is a Return Receipt.\r\n\
--b1\r\nContent-Type: message/disposition-notification; name=\"MDNPart2.txt\"\r\n\
Content-Disposition: inline\r\n\r\nReporting-UA: x; y\r\nFinal-Recipient: rfc822;bob@example.net\r\n\
Original-Message-ID: <sent-1@example.org>\r\nDisposition: manual-action/MDN-sent-manually; deleted\r\n\r\n\
--b1--\r\n";
        let report = parse_report(thunderbird).expect("report");
        assert_eq!(report.original_message_id, "sent-1@example.org");
        assert_eq!(report.final_recipient.as_deref(), Some("bob@example.net"));
        assert_eq!(report.disposition, ReceiptDisposition::Deleted);
        assert_eq!(parse_report(REQUEST), None);
        assert_eq!(parse_report(b"garbage"), None);
    }

    #[test]
    fn a_request_header_is_written_only_when_asked() {
        let mut message = receipt();
        message.report = None;
        message.html = None;
        message.disposition_notification_to = Some("alice@example.org".into());
        let built = build_message(&message).expect("built");
        let text = String::from_utf8(built.for_sending).expect("utf-8");
        assert!(
            text.contains("Disposition-Notification-To: <alice@example.org>")
                || text.contains("Disposition-Notification-To: alice@example.org"),
            "{text}"
        );
        let request = receipt_request(text.as_bytes()).expect("request");
        assert_eq!(request.address, "alice@example.org");
        message.disposition_notification_to = Some("not valid".into());
        assert!(build_message(&message).is_err());
    }
}
