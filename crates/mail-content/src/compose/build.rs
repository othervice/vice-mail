//! The outgoing message as bytes (mail-builder underneath): headers, `multipart/alternative`
//! from text and mail HTML, `multipart/related` around it with the inline images the HTML
//! refers to (`cid:`, stage 10), `multipart/mixed` with attachments; a forwarded message
//! (`message/rfc822`) goes in unencoded when it is 7-bit clean. Two byte sequences come out:
//! the one for the SMTP server (no `Bcc` header) and the copy for the Sent folder (with it),
//! byte for byte the same otherwise.
//!
//! - `Date` is written in UTC (`+0000`), so the message does not reveal the time zone.
//! - No header names the program, unless [`OutgoingMessage::user_agent`] is set.
//! - Boundaries are derived from the Message-ID, so building the same draft twice gives the
//!   same bytes; a boundary that occurs in a text part is changed.
//! - Addresses are checked; domains are written in their ASCII form (IDNA), display names,
//!   subject and file names are encoded by mail-builder (RFC 2047, RFC 2231). Control
//!   characters in them become spaces, so no input can start a header of its own.

use std::borrow::Cow;
use std::fmt;

use mail_builder::headers::address::Address as MbAddress;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::date::Date;
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::raw::Raw;
use mail_builder::headers::text::Text;
use mail_builder::headers::{Header, HeaderType};
use mail_builder::mime::{BodyPart, MimePart};
use mail_model::{Address, Envelope};

use super::address::{clean_header_text, is_valid_message_id, normalize_email};
use super::html::{document, html_blocks, inline_image_ids, is_valid_content_id};
use crate::filename::sanitize_filename;
use crate::text::html_to_text;

/// Everything the composer hands over for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    /// The sending identity: its address and display name.
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Blind copies: in the envelope and the Sent copy, never in the message that is sent.
    pub bcc: Vec<Address>,
    /// The identity's reply address, if any.
    pub reply_to: Vec<Address>,
    pub subject: String,
    /// Plain text. Without [`OutgoingMessage::html`] the message is `text/plain` only; with it,
    /// this replaces the text alternative derived from the HTML.
    pub text: Option<String>,
    /// HTML from the editor. Turned into mail HTML (see [`crate::mail_html`]); the message
    /// becomes `multipart/alternative` with a text version.
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    /// Images the HTML shows (`<img src="cid:…">`), as parts of `multipart/related`. Only
    /// those the HTML refers to go out (an image the user deleted from the text is not sent);
    /// without HTML none do.
    pub inline_images: Vec<InlineImage>,
    /// Without angle brackets. Made once when the draft is created ([`crate::new_message_id`])
    /// and kept for every attempt.
    pub message_id: String,
    /// Message-ID of the message answered, without angle brackets.
    pub in_reply_to: Option<String>,
    /// Oldest first, without angle brackets ([`crate::reply_references`]).
    pub references: Vec<String>,
    /// Unix seconds.
    pub date: i64,
    /// A `User-Agent` header, only if the user turned it on. `None` by default.
    pub user_agent: Option<String>,
    /// Stage 16: ask for a read receipt to this address (`Disposition-Notification-To`,
    /// RFC 8098). Checked like the other addresses.
    pub disposition_notification_to: Option<String>,
    /// Stage 16: the message is a read receipt: the body is `multipart/report` with the human
    /// readable text ([`OutgoingMessage::text`]) and the machine readable report. Attachments,
    /// HTML and inline images are ignored.
    pub report: Option<super::mdn::DispositionReport>,
}

/// A file attached to an outgoing message.
#[derive(Clone, PartialEq, Eq)]
pub struct Attachment {
    pub file_name: String,
    /// MIME type such as `application/pdf`; anything unusable becomes
    /// `application/octet-stream`.
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachment")
            .field("file_name", &self.file_name)
            .field("mime", &self.mime)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// An image shown in the message's HTML, referred to as `cid:<content_id>`.
#[derive(Clone, PartialEq, Eq)]
pub struct InlineImage {
    /// Without angle brackets; see [`crate::is_valid_content_id`]. Invalid ids are left out.
    pub content_id: String,
    pub file_name: String,
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`; anything else is left out.
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for InlineImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineImage")
            .field("content_id", &self.content_id)
            .field("mime", &self.mime)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// Raster types an inline image may have (the reading side shows only these, never SVG).
const INLINE_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The message ready to send.
#[derive(Clone, PartialEq, Eq)]
pub struct BuiltMessage {
    /// For the SMTP server: without the `Bcc` header.
    pub for_sending: Vec<u8>,
    /// For the Sent folder: the same bytes with the `Bcc` header when there are blind copies.
    pub for_sent_folder: Vec<u8>,
    /// Sender and every recipient (To, Cc, Bcc), each once.
    pub envelope: Envelope,
}

impl fmt::Debug for BuiltMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltMessage")
            .field("for_sending", &self.for_sending.len())
            .field("for_sent_folder", &self.for_sent_folder.len())
            .field("recipients", &self.envelope.recipients.len())
            .finish()
    }
}

/// Why a message cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// No To, Cc or Bcc recipient.
    NoRecipients,
    /// An address (sender, recipient or reply address) is not valid; holds it.
    InvalidAddress(String),
    /// The message's own Message-ID is not valid; holds it.
    InvalidMessageId(String),
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRecipients => f.write_str("the message has no recipient"),
            Self::InvalidAddress(address) => write!(f, "not a valid email address: {address:?}"),
            Self::InvalidMessageId(id) => write!(f, "not a valid Message-ID: {id:?}"),
        }
    }
}

impl std::error::Error for ComposeError {}

/// Builds the message (see module docs).
pub fn build_message(message: &OutgoingMessage) -> Result<BuiltMessage, ComposeError> {
    build(message, false)
}

/// Builds a draft for the server's Drafts folder: like [`build_message`], but a draft may have
/// no recipient yet (then there is no `To` header) and recipients or reply addresses that are
/// not valid (yet) are left out instead of failing; they stay in the local draft. The bytes are
/// the copy with `Bcc` ([`BuiltMessage::for_sent_folder`]): a draft keeps its blind copies.
/// Only an invalid sender or Message-ID fails.
pub fn build_draft_message(message: &OutgoingMessage) -> Result<Vec<u8>, ComposeError> {
    build(message, true).map(|built| built.for_sent_folder)
}

fn build(message: &OutgoingMessage, draft: bool) -> Result<BuiltMessage, ComposeError> {
    let from = mailbox(&message.from)?;
    // Each recipient once: in To before Cc before Bcc.
    let mut seen = Vec::new();
    let valid = |list: &[Address]| -> Vec<Address> {
        list.iter()
            .filter(|a| !draft || mailbox(a).is_ok())
            .cloned()
            .collect()
    };
    let to = recipients(&valid(&message.to), &mut seen)?;
    let cc = recipients(&valid(&message.cc), &mut seen)?;
    let bcc = recipients(&valid(&message.bcc), &mut seen)?;
    let reply_to = mailboxes(&valid(&message.reply_to))?;
    let receipt_to = match message.disposition_notification_to.as_deref() {
        Some(address) => Some(normalize_email(address)?),
        None => None,
    };
    if !draft && to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(ComposeError::NoRecipients);
    }
    let message_id = message.message_id.trim();
    let message_id = message_id
        .strip_prefix('<')
        .and_then(|id| id.strip_suffix('>'))
        .unwrap_or(message_id);
    if !is_valid_message_id(message_id) {
        return Err(ComposeError::InvalidMessageId(message.message_id.clone()));
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

    // Headers; the Bcc field is kept apart so it can go into the Sent copy only.
    let mut head: Vec<u8> = Vec::new();
    write_header(&mut head, "Date", &Date::new(message.date).into());
    write_header(
        &mut head,
        "From",
        &address_header(std::slice::from_ref(&from)),
    );
    if to.is_empty() && cc.is_empty() && !(draft && bcc.is_empty()) {
        write_header(
            &mut head,
            "To",
            &Raw::new("undisclosed-recipients:;").into(),
        );
    } else if !to.is_empty() {
        write_header(&mut head, "To", &address_header(&to));
    }
    if !cc.is_empty() {
        write_header(&mut head, "Cc", &address_header(&cc));
    }
    let bcc_at = head.len();
    let mut bcc_field = Vec::new();
    if !bcc.is_empty() {
        write_header(&mut bcc_field, "Bcc", &address_header(&bcc));
    }
    if !reply_to.is_empty() {
        write_header(&mut head, "Reply-To", &address_header(&reply_to));
    }
    let subject = clean_header_text(&message.subject);
    if !subject.is_empty() {
        write_header(&mut head, "Subject", &Text::new(subject).into());
    }
    write_header(
        &mut head,
        "Message-ID",
        &MessageId::new(message_id.to_owned()).into(),
    );
    if let Some(parent) = message
        .in_reply_to
        .as_deref()
        .map(str::trim)
        .filter(|id| is_valid_message_id(id))
    {
        write_header(
            &mut head,
            "In-Reply-To",
            &MessageId::new(parent.to_owned()).into(),
        );
    }
    let references: Vec<String> = message
        .references
        .iter()
        .map(|id| id.trim().to_owned())
        .filter(|id| is_valid_message_id(id))
        .collect();
    if !references.is_empty() {
        write_header(&mut head, "References", &MessageId::from(references).into());
    }
    if let Some(address) = receipt_to {
        write_header(
            &mut head,
            "Disposition-Notification-To",
            &MbAddress::new_address(None::<String>, address).into(),
        );
    }
    if let Some(agent) = message.user_agent.as_deref().map(clean_header_text)
        && !agent.is_empty()
    {
        write_header(&mut head, "User-Agent", &Text::new(agent).into());
    }
    head.extend_from_slice(b"MIME-Version: 1.0\r\n");

    let body = match &message.report {
        Some(report) => super::mdn::report_body(report, message.text.as_deref(), message_id),
        None => body(message, message_id),
    };

    let mut for_sending = Vec::with_capacity(head.len() + body.len());
    for_sending.extend_from_slice(&head);
    for_sending.extend_from_slice(&body);
    let mut for_sent_folder = Vec::with_capacity(for_sending.len() + bcc_field.len());
    for_sent_folder.extend_from_slice(head.get(..bcc_at).unwrap_or_default());
    for_sent_folder.extend_from_slice(&bcc_field);
    for_sent_folder.extend_from_slice(head.get(bcc_at..).unwrap_or_default());
    for_sent_folder.extend_from_slice(&body);
    Ok(BuiltMessage {
        for_sending,
        for_sent_folder,
        envelope,
    })
}

/// A checked mailbox: display name (cleaned) and address (ASCII domain).
type Mailbox = (Option<String>, String);

pub(super) fn mailbox(address: &Address) -> Result<Mailbox, ComposeError> {
    let email = normalize_email(&address.email)?;
    let name = address
        .name
        .as_deref()
        .map(clean_header_text)
        .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(&email));
    Ok((name, email))
}

fn mailboxes(addresses: &[Address]) -> Result<Vec<Mailbox>, ComposeError> {
    addresses.iter().map(mailbox).collect()
}

/// Checked recipients, without those already in `seen` (compared without case).
pub(super) fn recipients(
    addresses: &[Address],
    seen: &mut Vec<String>,
) -> Result<Vec<Mailbox>, ComposeError> {
    let mut out = Vec::new();
    for address in addresses {
        let checked = mailbox(address)?;
        let key = checked.1.to_lowercase();
        if !seen.contains(&key) {
            seen.push(key);
            out.push(checked);
        }
    }
    Ok(out)
}

pub(super) fn address_header(list: &[Mailbox]) -> HeaderType<'static> {
    let mut items: Vec<MbAddress<'static>> = list
        .iter()
        .map(|(name, email)| MbAddress::new_address(name.clone(), email.clone()))
        .collect();
    if items.len() == 1
        && let Some(item) = items.pop()
    {
        return item.into();
    }
    MbAddress::new_list(items).into()
}

pub(super) fn write_header(out: &mut Vec<u8>, name: &str, value: &HeaderType<'_>) {
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(b": ");
    value.write_header(out, name.len() + 2);
}

/// Line ends as CRLF.
pub(super) fn crlf(text: &str) -> String {
    let unix = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = unix.replace('\n', "\r\n");
    if !out.ends_with("\r\n") {
        out.push_str("\r\n");
    }
    out
}

/// A usable MIME type, lower case; `application/octet-stream` otherwise. Multipart types are
/// never taken over (they would break the structure).
fn attachment_type(mime: &str) -> String {
    let mime = mime.trim().to_ascii_lowercase();
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };
    match mime.split_once('/') {
        Some((kind, sub)) if token(kind) && token(sub) && kind != "multipart" => mime.clone(),
        _ => "application/octet-stream".to_owned(),
    }
}

/// Line ends of raw bytes as CRLF (a stored message may have bare LF).
pub(super) fn crlf_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 32);
    let mut previous = 0u8;
    for &byte in bytes {
        if byte == b'\n' && previous != b'\r' {
            out.push(b'\r');
        }
        out.push(byte);
        previous = byte;
    }
    if !out.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// 7-bit data in lines of at most 998 bytes, without NUL or bare CR (RFC 5322, 2.1.1).
fn is_7bit_clean(bytes: &[u8]) -> bool {
    bytes.split(|b| *b == b'\n').all(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        line.len() <= 998 && line.iter().all(|b| *b != 0 && *b != b'\r' && b.is_ascii())
    })
}

/// FNV-1a, for boundaries that only need to be stable and distinct.
pub(super) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The MIME body (root part headers included).
fn body(message: &OutgoingMessage, message_id: &str) -> Vec<u8> {
    let html = message
        .html
        .as_deref()
        .map(|editor| document(&html_blocks(editor)));
    let text = match (&message.text, &html) {
        (Some(text), _) => text.clone(),
        (None, Some(html)) => html_to_text(html),
        (None, None) => String::new(),
    };
    let text = crlf(&text);
    // The images the HTML shows, each once, in the order the HTML shows them.
    let inline: Vec<&InlineImage> = match &message.html {
        Some(editor) => inline_image_ids(editor)
            .iter()
            .filter_map(|id| {
                message.inline_images.iter().find(|image| {
                    image.content_id == *id
                        && is_valid_content_id(&image.content_id)
                        && INLINE_IMAGE_TYPES
                            .contains(&image.mime.trim().to_ascii_lowercase().as_str())
                })
            })
            .collect(),
        None => Vec::new(),
    };
    let html = html.map(|h| crlf(&h));
    let attachments: Vec<(String, String, Cow<'_, [u8]>)> = message
        .attachments
        .iter()
        .map(|a| {
            let mime = attachment_type(&a.mime);
            let bytes = if mime == "message/rfc822" {
                Cow::Owned(crlf_bytes(&a.bytes))
            } else {
                Cow::Borrowed(a.bytes.as_slice())
            };
            (sanitize_filename(&a.file_name), mime, bytes)
        })
        .collect();

    // Boundaries must not occur in parts that may go out unencoded (text, 7-bit attachments).
    let seed = format!("{:016x}", fnv1a(message_id.as_bytes()));
    let mut suffix = String::new();
    let boundary = |n: u8, suffix: &str| format!("=_{seed}{suffix}_{n}");
    let collides = |b: &str| {
        let b = b.as_bytes();
        let has = |hay: &[u8]| hay.windows(b.len()).any(|w| w == b);
        has(text.as_bytes())
            || html.as_deref().is_some_and(|h| has(h.as_bytes()))
            || attachments.iter().any(|(_, mime, bytes)| {
                (mime.starts_with("text/") || mime == "message/rfc822") && has(bytes)
            })
    };
    for round in 0u32..16 {
        if !collides(&boundary(1, &suffix))
            && !collides(&boundary(2, &suffix))
            && !collides(&boundary(3, &suffix))
        {
            break;
        }
        suffix = format!("_{round}");
    }

    let text_part = MimePart::new(
        ContentType::new("text/plain").attribute("charset", "utf-8"),
        BodyPart::Text(Cow::Owned(text)),
    );
    let content = match html {
        Some(html) => {
            let html_part = MimePart::new(
                ContentType::new("text/html").attribute("charset", "utf-8"),
                BodyPart::Text(Cow::Owned(html)),
            );
            MimePart::new(
                ContentType::new("multipart/alternative")
                    .attribute("boundary", boundary(1, &suffix)),
                BodyPart::Multipart(vec![text_part, html_part]),
            )
        }
        None => text_part,
    };
    let content = if inline.is_empty() {
        content
    } else {
        let mut parts = vec![content];
        for image in inline {
            let name = sanitize_filename(&image.file_name);
            parts.push(
                MimePart::new(
                    ContentType::new(image.mime.trim().to_ascii_lowercase())
                        .attribute("name", name.clone()),
                    BodyPart::Binary(Cow::Borrowed(image.bytes.as_slice())),
                )
                .header(
                    "Content-Disposition",
                    ContentType::new("inline").attribute("filename", name),
                )
                .cid(image.content_id.clone()),
            );
        }
        MimePart::new(
            ContentType::new("multipart/related")
                .attribute("type", "multipart/alternative")
                .attribute("boundary", boundary(3, &suffix)),
            BodyPart::Multipart(parts),
        )
    };
    let root = if attachments.is_empty() {
        content
    } else {
        let mut parts = vec![content];
        for (name, mime, bytes) in attachments {
            let clean = mime == "message/rfc822" && is_7bit_clean(&bytes);
            let part = MimePart::new(
                ContentType::new(mime).attribute("name", name.clone()),
                BodyPart::Binary(bytes),
            );
            // A forwarded message is a message, not a file: unencoded (RFC 2046, 5.2.1) when
            // it can be; base64 only for 8-bit or long-lined originals (readers accept it).
            let part = if clean {
                part.transfer_encoding("7bit")
            } else {
                part
            };
            parts.push(part.attachment(name));
        }
        MimePart::new(
            ContentType::new("multipart/mixed").attribute("boundary", boundary(2, &suffix)),
            BodyPart::Multipart(parts),
        )
    };
    let mut out = Vec::new();
    root.write_part(&mut out);
    out
}
