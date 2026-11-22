//! Whole messages: mail-parser (Stalwart, fuzzed against malformed mail) turns the raw bytes into
//! the structure the reading pane needs. Charsets, encoded words, RFC 2231 names and broken
//! structure are mail-parser's job; this module picks the bodies, decodes them, collects inline
//! images and attachments, and reports what was damaged instead of failing.

use std::borrow::Cow;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, LazyLock};

use mail_model::Address;
use mail_parser::{
    AddressList, DecodeProblems, HeaderName, MessageParser, MessagePart, MessageRef, PartFlags,
    PartKind,
};

use crate::filename::{attachment_file_name, is_executable_type};
use crate::html::InlineImages;
use crate::image::{ImageBytes, sniff_image};
use crate::preview::{preview_from_html, preview_from_text};
use crate::text::html_to_text;

/// Longest text body kept; longer text is cut and reported.
pub const MAX_TEXT_BODY_BYTES: usize = 4 << 20;
/// Longest HTML body kept; longer HTML is cut and reported.
pub const MAX_HTML_BODY_BYTES: usize = 8 << 20;
/// Largest inline image kept for `cid:` references.
pub const MAX_INLINE_IMAGE_BYTES: usize = 8 << 20;
/// Largest total of inline images kept.
const MAX_INLINE_TOTAL_BYTES: usize = 32 << 20;
/// At most this many addresses per header are kept.
const MAX_ADDRESSES: usize = 2000;

static PARSER: LazyLock<MessageParser> = LazyLock::new(MessageParser::new);

/// A message prepared for reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedMessage {
    pub headers: MessageHeaders,
    /// The text/plain body (several text parts joined, `format=flowed` undone, line ends `\n`).
    /// `None` if the message has no text part; then [`ParsedMessage::display_text`] derives one.
    pub text: Option<String>,
    /// The HTML body, unsanitised: always pass it through [`crate::render_html`]. When the body
    /// list mixes HTML, text and images (Apple Mail), text parts are added as escaped blocks and
    /// images as `cid:` references.
    pub html: Option<String>,
    /// Parts offered as attachments, in message order.
    pub attachments: Vec<AttachmentPart>,
    /// Images that the HTML can show through `cid:` (see [`ParsedMessage::inline_images`]).
    pub inline: Vec<InlinePart>,
    /// Preview for the message list (see [`crate::preview_from_text`]).
    pub preview: String,
    /// What was damaged or could not be decoded. Empty for a well-formed message.
    pub problems: Vec<ParseProblem>,
}

/// Header fields, decoded (RFC 2047, charsets). Message ids without angle brackets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageHeaders {
    pub from: Vec<Address>,
    pub sender: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    /// Control characters replaced by spaces, trimmed.
    pub subject: Option<String>,
    /// `Date` as Unix seconds; `None` when missing or unreadable.
    pub date: Option<i64>,
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    pub list: ListHeaders,
    pub auto_submitted: Option<String>,
    pub precedence: Option<String>,
}

/// Mailing-list header fields (RFC 2369, 2919, 8058). URI lists hold the URIs inside `<…>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListHeaders {
    /// The list identifier inside `<…>` of `List-Id`.
    pub id: Option<String>,
    pub unsubscribe: Vec<String>,
    /// `List-Unsubscribe-Post` (one-click unsubscribe), as written.
    pub unsubscribe_post: Option<String>,
    pub post: Vec<String>,
    pub archive: Vec<String>,
    pub help: Vec<String>,
    pub subscribe: Vec<String>,
    pub owner: Vec<String>,
}

/// One attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPart {
    /// Part number for [`attachment_bytes`].
    pub part_id: u32,
    /// The sanitised name ([`crate::sanitize_filename`]); show and save this one only.
    pub file_name: String,
    /// The declared name was changed by sanitising (or was missing).
    pub name_changed: bool,
    /// MIME type as declared, lower case (`application/octet-stream` if none).
    pub mime: String,
    /// Decoded size in bytes.
    pub size: u64,
    pub content_id: Option<String>,
    /// Marked to be shown inside the message (`Content-Disposition: inline`).
    pub inline: bool,
    /// Opening it can run code ([`crate::is_executable_type`]): warn first.
    pub executable: bool,
}

/// An image the HTML may show through `cid:`.
#[derive(Clone, PartialEq, Eq)]
pub struct InlinePart {
    /// Content-ID without angle brackets (synthesised for body images without one).
    pub content_id: String,
    pub part_id: u32,
    /// Type recognised from the bytes.
    pub mime: &'static str,
    pub bytes: ImageBytes,
}

impl fmt::Debug for InlinePart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlinePart")
            .field("content_id", &self.content_id)
            .field("part_id", &self.part_id)
            .field("mime", &self.mime)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// Something about the message that is damaged; the message is still shown as far as possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseProblem {
    /// No header block: the whole input is shown as text.
    NoHeaders,
    /// MIME structure damaged: missing or misplaced boundary, unterminated multipart, header
    /// block without blank line.
    DamagedStructure { part_id: u32 },
    /// Nesting depth or part count limit reached; the rest stays in the last part.
    LimitReached { part_id: u32 },
    /// Malformed base64/quoted-printable or an unknown Content-Transfer-Encoding.
    TransferEncoding { part_id: u32 },
    /// Unknown or unsupported charset; decoded as UTF-8.
    UnknownCharset { part_id: u32, charset: String },
    /// Bytes not valid in the charset were replaced by U+FFFD.
    MalformedText { part_id: u32 },
    /// A `Date` header exists but cannot be read.
    InvalidDate,
    /// A body was longer than the limit and was cut.
    BodyTruncated { part_id: u32 },
    /// The parser failed unexpectedly (a bug); the raw message is shown as text.
    Internal,
}

impl ParsedMessage {
    /// Images for [`crate::RenderOptions::inline`].
    pub fn inline_images(&self) -> InlineImages {
        let mut images = InlineImages::new();
        for part in &self.inline {
            images.insert(&part.content_id, part.bytes.clone());
        }
        images
    }

    /// Text for the text view: the text body, else the text of the HTML body.
    pub fn display_text(&self) -> Cow<'_, str> {
        match (&self.text, &self.html) {
            (Some(text), _) => Cow::Borrowed(text),
            (None, Some(html)) => Cow::Owned(html_to_text(html)),
            (None, None) => Cow::Borrowed(""),
        }
    }

    /// Whether there is anything to show besides the headers.
    pub fn has_body(&self) -> bool {
        self.text.as_deref().is_some_and(|t| !t.trim().is_empty())
            || self.html.as_deref().is_some_and(|h| !h.trim().is_empty())
            || !self.attachments.is_empty()
    }
}

/// Parses a message. Never panics: an unexpected failure gives the raw text with
/// [`ParseProblem::Internal`].
pub fn parse(raw: &[u8]) -> ParsedMessage {
    panic::catch_unwind(AssertUnwindSafe(|| parse_message(raw)))
        .unwrap_or_else(|_| raw_fallback(raw, ParseProblem::Internal))
}

/// The decoded content of part `part_id` (as in [`AttachmentPart::part_id`]), for saving.
/// `None` if there is no such part.
pub fn attachment_bytes(raw: &[u8], part_id: u32) -> Option<Vec<u8>> {
    panic::catch_unwind(AssertUnwindSafe(|| {
        let message = PARSER.parse(raw)?;
        let part = message.part(part_id)?;
        Some(part.decoded().into_owned())
    }))
    .ok()
    .flatten()
}

fn raw_fallback(raw: &[u8], problem: ParseProblem) -> ParsedMessage {
    let end = raw.len().min(MAX_TEXT_BODY_BYTES);
    let text = String::from_utf8_lossy(&raw[..end]).replace("\r\n", "\n");
    let text: String = text
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    ParsedMessage {
        preview: preview_from_text(&text),
        text: Some(text),
        problems: vec![problem],
        ..ParsedMessage::default()
    }
}

fn parse_message(raw: &[u8]) -> ParsedMessage {
    let Some(message) = PARSER.parse(raw) else {
        return raw_fallback(raw, ParseProblem::NoHeaders);
    };
    let root = message.root();
    let mut problems = Vec::new();
    let headers = headers(&root, &mut problems);

    for part in message.parts() {
        let flags = part.flags();
        if flags.intersects(PartFlags::LIMIT_REACHED | PartFlags::NESTING_LIMIT) {
            problems.push(ParseProblem::LimitReached { part_id: part.id() });
        }
        if flags.intersects(
            PartFlags::MISSING_DELIMITER
                | PartFlags::FALLBACK_DELIMITER
                | PartFlags::NO_BLANK_LINE
                | PartFlags::UNTERMINATED,
        ) {
            problems.push(ParseProblem::DamagedStructure { part_id: part.id() });
        }
    }

    let text = text_body(&root, &mut problems);
    let (html, synthesized) = html_body(&root, &mut problems);
    let inline = inline_parts(&root, &synthesized);
    let attachments = attachments(&root);
    let preview = match (&text, &html) {
        (Some(text), _) if !text.trim().is_empty() => preview_from_text(text),
        (_, Some(html)) => preview_from_html(html),
        _ => String::new(),
    };
    problems.dedup();
    ParsedMessage {
        headers,
        text,
        html,
        attachments,
        inline,
        preview,
        problems,
    }
}

fn headers(root: &MessageRef<'_>, problems: &mut Vec<ParseProblem>) -> MessageHeaders {
    let header_values = root.headers();
    let date = root
        .date()
        .filter(|d| d.is_valid())
        .map(|d| d.to_timestamp());
    if date.is_none() && header_values.get(HeaderName::Date).is_some() {
        problems.push(ParseProblem::InvalidDate);
    }
    let raw = |name: &str| {
        header_values
            .get(name)
            .map(|h| String::from_utf8_lossy(h.raw_value()).into_owned())
    };
    let uris = |name: &str| raw(name).map(|v| angle_items(&v)).unwrap_or_default();
    let list = ListHeaders {
        id: raw("List-Id").map(|v| {
            angle_items(&v)
                .into_iter()
                .next()
                .unwrap_or_else(|| clean_text(&v))
        }),
        unsubscribe: uris("List-Unsubscribe"),
        unsubscribe_post: raw("List-Unsubscribe-Post").map(|v| clean_text(&v)),
        post: uris("List-Post"),
        archive: uris("List-Archive"),
        help: uris("List-Help"),
        subscribe: uris("List-Subscribe"),
        owner: uris("List-Owner"),
    };
    let ids = |list: Option<mail_parser::TextList<'_>>| {
        list.map(|l| {
            l.iter()
                .map(clean_text)
                .filter(|id| !id.is_empty())
                .take(MAX_ADDRESSES)
                .collect()
        })
        .unwrap_or_default()
    };
    MessageHeaders {
        from: addresses(root.from()),
        sender: addresses(root.sender()),
        reply_to: addresses(root.reply_to()),
        to: addresses(root.to()),
        cc: addresses(root.cc()),
        bcc: addresses(root.bcc()),
        subject: root.subject().map(clean_text),
        date,
        message_id: root
            .message_id()
            .map(clean_text)
            .filter(|id| !id.is_empty()),
        in_reply_to: ids(root.in_reply_to()),
        references: ids(root.references()),
        list,
        auto_submitted: raw("Auto-Submitted")
            .map(|v| clean_text(&v))
            .filter(|v| !v.is_empty()),
        precedence: raw("Precedence")
            .map(|v| clean_text(&v))
            .filter(|v| !v.is_empty()),
    }
}

fn addresses(list: Option<AddressList<'_>>) -> Vec<Address> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.mailboxes()
        .filter_map(|mailbox| {
            let name = mailbox.name().map(clean_text).filter(|n| !n.is_empty());
            let email = mailbox.address().map(clean_text).unwrap_or_default();
            (name.is_some() || !email.is_empty()).then_some(Address { name, email })
        })
        .take(MAX_ADDRESSES)
        .collect()
}

/// Control characters (line breaks of folded headers included) become spaces; trimmed.
fn clean_text(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    mapped.trim().to_owned()
}

/// The items inside `<…>` of a header value such as `List-Unsubscribe`.
fn angle_items(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest[start + 1..].find('>') else {
            break;
        };
        let item: String = rest[start + 1..start + 1 + len]
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .collect();
        if !item.is_empty() && items.len() < 32 {
            items.push(item);
        }
        rest = &rest[start + 1 + len + 1..];
    }
    items
}

fn report_decode(part: &MessagePart<'_>, found: DecodeProblems, problems: &mut Vec<ParseProblem>) {
    let part_id = part.id();
    if found.intersects(
        DecodeProblems::MALFORMED_TRANSFER_ENCODING | DecodeProblems::UNKNOWN_TRANSFER_ENCODING,
    ) {
        problems.push(ParseProblem::TransferEncoding { part_id });
    }
    if found.contains(DecodeProblems::UNKNOWN_CHARSET) {
        let charset = part
            .content_type()
            .and_then(|ct| ct.attribute("charset"))
            .map(clean_text)
            .unwrap_or_default();
        problems.push(ParseProblem::UnknownCharset { part_id, charset });
    }
    if found.contains(DecodeProblems::MALFORMED_CHARSET) {
        problems.push(ParseProblem::MalformedText { part_id });
    }
}

fn text_body(root: &MessageRef<'_>, problems: &mut Vec<ParseProblem>) -> Option<String> {
    let mut out: Option<String> = None;
    for part in root.text_body() {
        if !matches!(part.kind(), PartKind::Text) {
            continue;
        }
        let Some((text, found)) = part.text_checked() else {
            continue;
        };
        report_decode(&part, found, problems);
        let text = text.replace("\r\n", "\n");
        let text = match flowed(&part) {
            Some(delsp) => unflow(&text, delsp),
            None => text,
        };
        let body = out.get_or_insert_with(String::new);
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&text);
        if body.len() > MAX_TEXT_BODY_BYTES {
            body.truncate(body.floor_char_boundary(MAX_TEXT_BODY_BYTES));
            problems.push(ParseProblem::BodyTruncated { part_id: part.id() });
            break;
        }
    }
    out
}

/// `Some(delsp)` for `format=flowed` text (RFC 3676).
fn flowed(part: &MessagePart<'_>) -> Option<bool> {
    let content_type = part.content_type()?;
    let format = content_type.attribute("format")?;
    format.eq_ignore_ascii_case("flowed").then(|| {
        content_type
            .attribute("delsp")
            .is_some_and(|d| d.eq_ignore_ascii_case("yes"))
    })
}

/// Joins soft line breaks of `format=flowed` text (RFC 3676 section 4): a line ending in a space
/// continues on the next line of the same quote depth. Space-stuffing is removed; the signature
/// separator `-- ` stays a hard break. Quoted lines keep `> ` markers per depth.
pub(crate) fn unflow(text: &str, delsp: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut paragraph = String::new();
    let mut paragraph_depth: Option<usize> = None;
    let flush = |out: &mut String, paragraph: &mut String, depth: usize| {
        for _ in 0..depth {
            out.push('>');
        }
        if depth > 0 && !paragraph.is_empty() {
            out.push(' ');
        }
        out.push_str(paragraph);
        out.push('\n');
        paragraph.clear();
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let count = if text.ends_with('\n') {
        lines.len() - 1
    } else {
        lines.len()
    };
    for line in &lines[..count] {
        let depth = line.chars().take_while(|c| *c == '>').count();
        let content = &line[depth..];
        let content = content.strip_prefix(' ').unwrap_or(content);
        if let Some(open) = paragraph_depth
            && open != depth
        {
            flush(&mut out, &mut paragraph, open);
        }
        let soft = content.ends_with(' ') && content != "-- ";
        if soft && delsp {
            paragraph.push_str(&content[..content.len() - 1]);
        } else {
            paragraph.push_str(content);
        }
        if soft {
            paragraph_depth = Some(depth);
        } else {
            flush(&mut out, &mut paragraph, depth);
            paragraph_depth = None;
        }
    }
    if let Some(open) = paragraph_depth {
        flush(&mut out, &mut paragraph, open);
    }
    out
}

/// Synthesised Content-IDs for body images without one, by part id.
type Synthesized = Vec<(u32, String)>;

fn html_body(
    root: &MessageRef<'_>,
    problems: &mut Vec<ParseProblem>,
) -> (Option<String>, Synthesized) {
    let parts: Vec<MessagePart<'_>> = root.html_body().collect();
    if !parts.iter().any(|p| matches!(p.kind(), PartKind::Html)) {
        return (None, Vec::new());
    }
    let mut out = String::new();
    let mut media = Vec::new();
    for part in &parts {
        match part.kind() {
            PartKind::Html => {
                if let Some((html, found)) = part.text_checked() {
                    report_decode(part, found, problems);
                    out.push_str(&html);
                }
            }
            PartKind::Text => {
                if let Some((text, found)) = part.text_checked() {
                    report_decode(part, found, problems);
                    out.push_str("<div style=\"white-space:pre-wrap\">");
                    escape_html(&text.replace("\r\n", "\n"), &mut out);
                    out.push_str("</div>");
                }
            }
            PartKind::Binary | PartKind::InlineBinary if is_image_part(part) => {
                media.push((out.len(), *part));
            }
            _ => {}
        }
        if out.len() > MAX_HTML_BODY_BYTES {
            out.truncate(out.floor_char_boundary(MAX_HTML_BODY_BYTES));
            problems.push(ParseProblem::BodyTruncated { part_id: part.id() });
            break;
        }
    }
    // Images in the body list (Apple Mail: text, image, text) are placed where they occur,
    // unless the HTML already refers to them.
    let mut synthesized = Vec::new();
    for (position, part) in media.into_iter().rev() {
        let referenced = part
            .content_id()
            .is_some_and(|id| out.contains(&format!("cid:{}", id.trim())));
        if referenced || position > out.len() {
            continue;
        }
        let cid = match part.content_id() {
            Some(id) if !id.trim().is_empty() => id.trim().to_owned(),
            _ => format!("vm-part-{}@vice-mail.invalid", part.id()),
        };
        let mut img = String::from("<img src=\"cid:");
        escape_html(&cid, &mut img);
        img.push_str("\" alt=\"\">");
        out.insert_str(position, &img);
        synthesized.push((part.id(), cid));
    }
    (Some(out), synthesized)
}

fn is_image_part(part: &MessagePart<'_>) -> bool {
    part.content_type()
        .is_some_and(|ct| ct.ctype().eq_ignore_ascii_case("image"))
}

fn escape_html(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn inline_parts(root: &MessageRef<'_>, synthesized: &Synthesized) -> Vec<InlinePart> {
    let mut out = Vec::new();
    let mut total = 0usize;
    for part in root.parts() {
        if !is_image_part(&part) || part.decoded_len() > MAX_INLINE_IMAGE_BYTES {
            continue;
        }
        let content_id = part
            .content_id()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
            .or_else(|| {
                synthesized
                    .iter()
                    .find(|(id, _)| *id == part.id())
                    .map(|(_, cid)| cid.clone())
            });
        let Some(content_id) = content_id else {
            continue;
        };
        let bytes = part.decoded();
        let Some(kind) = sniff_image(&bytes) else {
            continue;
        };
        total += bytes.len();
        if total > MAX_INLINE_TOTAL_BYTES {
            break;
        }
        out.push(InlinePart {
            content_id,
            part_id: part.id(),
            mime: kind.mime(),
            bytes: Arc::from(bytes.as_ref()),
        });
    }
    out
}

/// The attachment list, plus images and other media shown in the body lists (Apple Mail's
/// "text, image, text"), which mail-parser does not count as attachments: the text view cannot
/// show them, and the reading pane hides those the HTML shows (`RenderedHtml::inline_used`).
fn attachments(root: &MessageRef<'_>) -> Vec<AttachmentPart> {
    let mut parts: Vec<MessagePart<'_>> = root.attachments().collect();
    for part in root.text_body().chain(root.html_body()) {
        if matches!(part.kind(), PartKind::Binary | PartKind::InlineBinary)
            && !parts.iter().any(|p| p.id() == part.id())
        {
            parts.push(part);
        }
    }
    parts.sort_by_key(MessagePart::id);
    parts
        .into_iter()
        .map(|part| {
            let nested = match part.kind() {
                PartKind::Message(nested) => Some(nested),
                _ => None,
            };
            let mime = part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{}", ct.ctype(), sub),
                    None => ct.ctype().to_owned(),
                })
                .map(|m| clean_text(&m).to_ascii_lowercase())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| {
                    if nested.is_some() {
                        "message/rfc822".to_owned()
                    } else {
                        "application/octet-stream".to_owned()
                    }
                });
            let declared = continued_name(&part)
                .or_else(|| part.attachment_name().map(str::to_owned))
                .or_else(|| {
                    nested
                        .and_then(|m| m.subject())
                        .map(|subject| format!("{subject}.eml"))
                });
            let file_name = attachment_file_name(declared.as_deref(), &mime);
            let name_changed = declared.as_deref() != Some(file_name.as_str());
            AttachmentPart {
                part_id: part.id(),
                executable: is_executable_type(&file_name, &mime),
                file_name,
                name_changed,
                mime,
                size: part.decoded_len() as u64,
                content_id: part
                    .content_id()
                    .map(|id| id.trim().to_owned())
                    .filter(|id| !id.is_empty()),
                inline: part.is_inline(),
            }
        })
        .collect()
}

/// A file name in RFC 2231 continuations (`filename*0*=UTF-8''…; filename*1*=…`), with the
/// pieces joined before percent-decoding. Some mailers cut a piece inside an escape
/// (`%C3%A` / `4`); decoding piece by piece then garbles the name. `None` when the part has no
/// such parameter or the charset is not UTF-8, US-ASCII or ISO-8859-1.
fn continued_name(part: &MessagePart<'_>) -> Option<String> {
    let headers = part.headers();
    [
        (HeaderName::ContentDisposition, "filename"),
        (HeaderName::ContentType, "name"),
    ]
    .into_iter()
    .find_map(|(header, param)| {
        let raw = String::from_utf8_lossy(headers.get(header)?.raw_value()).into_owned();
        join_continuations(&raw, param)
    })
}

pub(crate) fn join_continuations(raw: &str, param: &str) -> Option<String> {
    let mut pieces: Vec<(u32, bool, String)> = Vec::new();
    for item in split_params(raw) {
        let Some((name, value)) = item.split_once('=') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let Some(rest) = name.strip_prefix(param).and_then(|r| r.strip_prefix('*')) else {
            continue;
        };
        let (number, encoded) = match rest.strip_suffix('*') {
            Some(number) => (number, true),
            None => (rest, false),
        };
        let Ok(number) = number.parse::<u32>() else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        pieces.push((
            number,
            encoded,
            value.replace("\\\"", "\"").replace("\\\\", "\\"),
        ));
    }
    if pieces.len() < 2 || !pieces.iter().any(|(_, encoded, _)| *encoded) {
        return None;
    }
    pieces.sort_by_key(|(number, _, _)| *number);
    let mut joined = String::new();
    let mut charset = String::from("us-ascii");
    for (index, (number, encoded, value)) in pieces.iter().enumerate() {
        if *number as usize != index {
            return None;
        }
        let mut value = value.as_str();
        if index == 0 && *encoded {
            let (cs, rest) = value.split_once('\'')?;
            let (_, rest) = rest.split_once('\'')?;
            charset = cs.to_ascii_lowercase();
            value = rest;
        }
        if *encoded {
            joined.push_str(value);
        } else {
            joined.push_str(&value.replace('%', "%25"));
        }
    }
    let mut bytes = Vec::with_capacity(joined.len());
    let mut i = 0;
    let raw_bytes = joined.as_bytes();
    while let Some(&byte) = raw_bytes.get(i) {
        if byte == b'%'
            && let Some(value) = joined
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            bytes.push(value);
            i += 3;
            continue;
        }
        bytes.push(byte);
        i += 1;
    }
    match charset.as_str() {
        "utf-8" | "utf8" | "us-ascii" | "" => String::from_utf8(bytes).ok(),
        "iso-8859-1" | "latin1" => Some(bytes.iter().map(|b| char::from(*b)).collect()),
        _ => None,
    }
}

/// Parameters of a MIME header value, split at `;` outside quotes (the first item is the value).
fn split_params(raw: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in raw.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                items.push(&raw[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&raw[start..]);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_message() {
        let raw = b"From: =?utf-8?q?J=C3=BCrgen?= <j@example.com>\r\nTo: a@example.com, B <b@example.com>\r\n\
                    Subject: Hello\r\n\tthere\r\nDate: Sat, 20 Nov 2021 14:22:01 -0800\r\nMessage-ID: <id@x>\r\n\
                    References: <a@x> <b@x>\r\nList-Unsubscribe: <mailto:u@x?subject=u>,\r\n <https://x/u>\r\n\
                    List-Id: Rust users <rust-users.lists.example>\r\n\r\nHi there\r\n-- \r\nSig\r\n";
        let m = parse(raw);
        assert_eq!(m.problems, []);
        assert_eq!(
            m.headers.from,
            [Address {
                name: Some("Jürgen".into()),
                email: "j@example.com".into()
            }]
        );
        assert_eq!(m.headers.to.len(), 2);
        assert_eq!(m.headers.subject.as_deref(), Some("Hello there"));
        assert_eq!(m.headers.date, Some(1_637_446_921));
        assert_eq!(m.headers.message_id.as_deref(), Some("id@x"));
        assert_eq!(m.headers.references, ["a@x", "b@x"]);
        assert_eq!(
            m.headers.list.unsubscribe,
            ["mailto:u@x?subject=u", "https://x/u"]
        );
        assert_eq!(
            m.headers.list.id.as_deref(),
            Some("rust-users.lists.example")
        );
        assert_eq!(m.text.as_deref(), Some("Hi there\n-- \nSig\n"));
        assert_eq!(m.preview, "Hi there");
        assert!(m.html.is_none());
    }

    #[test]
    fn garbage_is_shown_as_text() {
        let m = parse(b"");
        assert_eq!(m.problems, [ParseProblem::NoHeaders]);
        let m = parse(b"\x00\x01binary without newline \xff");
        assert!(m.text.is_some());
        assert!(!m.preview.contains('\u{0}'));
    }

    #[test]
    fn flowed_text_is_joined() {
        assert_eq!(
            unflow(
                "This is \nsoft.\n> quoted \n> more\n>> deep\n-- \nsig\n",
                false
            ),
            "This is soft.\n> quoted more\n>> deep\n-- \nsig\n"
        );
        assert_eq!(unflow("del \nsp\n", true), "delsp\n");
        assert_eq!(unflow(" stuffed\n", false), "stuffed\n");
    }

    #[test]
    fn rfc2231_continuations_are_joined_before_decoding() {
        let raw = " attachment;\r\n filename*0*=UTF-8''M%C3%A;\r\n filename*1*=4rz%20x;\r\n filename*2=\" (1).pdf\"";
        assert_eq!(
            join_continuations(raw, "filename").as_deref(),
            Some("März x (1).pdf")
        );
        assert_eq!(
            join_continuations(" a; filename*0*=iso-8859-1''%E4; filename*1*=b", "filename")
                .as_deref(),
            Some("äb")
        );
        assert_eq!(
            join_continuations(" a; filename=\"x.pdf\"", "filename"),
            None
        );
        assert_eq!(
            join_continuations(" a; filename*0*=UTF-8''a; filename*2*=b", "filename"),
            None
        );
    }

    #[test]
    fn angle_items_are_extracted() {
        assert_eq!(angle_items("<a>, <b c>"), ["a", "bc"]);
        assert_eq!(angle_items("<unterminated"), Vec::<String>::new());
        assert_eq!(angle_items("plain"), Vec::<String>::new());
    }
}
