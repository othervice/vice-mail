//! From a stored body to what the reading pane shows, without IO: the parsed message (also for
//! large messages of which only some sections are stored), the attachment list, the header,
//! the problems, and the text segments. The HTML itself is rendered by `mail-content`.

use std::collections::HashSet;

use mail_content::{
    ParseProblem, ParsedMessage, RenderedText, TextSpan, attachment_file_name, is_executable_type,
};
use mail_model::{
    Address, AttachmentInfo, BodyFormat, BodySection, Disposition, FetchedSection, MessageProblem,
    MessageStructure, MessageViewHeader, MimePart, PartPath, TextSegment,
};
use mail_store::StoredMessage;

/// A stored body, parsed.
pub(crate) struct Content {
    pub parsed: ParsedMessage,
    pub attachments: Vec<Attachment>,
    /// Only some sections of a large message are stored.
    pub partial: bool,
}

/// One entry of the attachment list and where its bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub info: AttachmentInfo,
    pub source: AttachmentSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttachmentSource {
    /// Part id (mail-parser numbering) in the complete raw message.
    Whole { part_id: u32 },
    /// IMAP part of a partial body; fetched with `n.MIME` and `n` when not stored.
    Section { path: PartPath },
}

impl Content {
    /// A complete raw message.
    pub fn complete(raw: &[u8]) -> Self {
        let parsed = mail_content::parse(raw);
        let attachments = complete_attachments(&parsed);
        Self {
            parsed,
            attachments,
            partial: false,
        }
    }

    /// The stored sections of a large message: the readable parts are assembled into a
    /// message with the original structure and parsed like a complete one; the attachment list
    /// comes from the structure, so attachments that are not stored are listed as well.
    pub fn partial(structure: &MessageStructure, sections: &[FetchedSection]) -> Self {
        let raw = assemble_partial(structure, sections);
        let parsed = mail_content::parse(&raw);
        let stored: HashSet<&BodySection> = sections.iter().map(|s| &s.section).collect();
        let attachments = partial_attachments(structure, &stored, parsed.html.as_deref());
        Self {
            parsed,
            attachments,
            partial: true,
        }
    }

    /// `html` and `text` when the message has HTML (the text view derives text from it),
    /// otherwise `text` only.
    pub fn formats(&self) -> Vec<BodyFormat> {
        if self.parsed.html.is_some() {
            vec![BodyFormat::Html, BodyFormat::Text]
        } else {
            vec![BodyFormat::Text]
        }
    }

    /// Whether `format` is shown as HTML.
    pub fn shows_html(&self, format: BodyFormat) -> bool {
        self.parsed.html.is_some() && format != BodyFormat::Text
    }

    pub fn problems(&self) -> Vec<MessageProblem> {
        let mut problems = Vec::new();
        let mut push = |problem| {
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        };
        for problem in &self.parsed.problems {
            match problem {
                ParseProblem::NoHeaders | ParseProblem::Internal => {
                    push(MessageProblem::RawFallback);
                }
                ParseProblem::DamagedStructure { .. } | ParseProblem::LimitReached { .. } => {
                    push(MessageProblem::BrokenStructure);
                }
                ParseProblem::TransferEncoding { .. } => push(MessageProblem::UndecodablePart),
                ParseProblem::UnknownCharset { .. } | ParseProblem::MalformedText { .. } => {
                    push(MessageProblem::UnknownCharset);
                }
                // A cut body and an unreadable date have no code of their own; the body is
                // still shown, the date comes from the server (sort date).
                ParseProblem::BodyTruncated { .. } | ParseProblem::InvalidDate => {}
            }
        }
        // No readable text: empty, or only attachments (also when a broken structure left the
        // whole body as one opaque part).
        let readable = |t: &Option<String>| t.as_deref().is_some_and(|t| !t.trim().is_empty());
        if !readable(&self.parsed.text) && !readable(&self.parsed.html) {
            push(MessageProblem::NoContent);
        }
        problems
    }

    /// The header of the reading pane: decoded addresses from the message, date and subject as
    /// in the list. Bcc only for the user's own mail.
    pub fn header(&self, summary: &StoredMessage, own_mail: bool) -> MessageViewHeader {
        let headers = &self.parsed.headers;
        let or_stored = |parsed: &[Address], stored: Vec<Address>| {
            if parsed.is_empty() {
                stored
            } else {
                parsed.to_vec()
            }
        };
        MessageViewHeader {
            from: or_stored(&headers.from, summary.from.clone().into_iter().collect()),
            to: or_stored(&headers.to, summary.to.clone()),
            cc: or_stored(&headers.cc, summary.cc.clone()),
            bcc: if own_mail {
                headers.bcc.clone()
            } else {
                Vec::new()
            },
            reply_to: headers.reply_to.clone(),
            date: summary.date.saturating_mul(1000),
            subject: summary.subject.clone(),
        }
    }
}

/// The header while no body is stored: what the list knows.
pub(crate) fn stored_header(summary: &StoredMessage) -> MessageViewHeader {
    MessageViewHeader {
        from: summary.from.clone().into_iter().collect(),
        to: summary.to.clone(),
        cc: summary.cc.clone(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        date: summary.date.saturating_mul(1000),
        subject: summary.subject.clone(),
    }
}

/// The text view as segments: quote markers as `> ` text, lines joined with `\n`, adjacent text
/// merged.
pub(crate) fn text_segments(rendered: &RenderedText) -> Vec<TextSegment> {
    let mut segments: Vec<TextSegment> = Vec::new();
    let push_text = |segments: &mut Vec<TextSegment>, text: &str| {
        if text.is_empty() {
            return;
        }
        if let Some(TextSegment::Text { text: last }) = segments.last_mut() {
            last.push_str(text);
        } else {
            segments.push(TextSegment::Text {
                text: text.to_owned(),
            });
        }
    };
    for (i, line) in rendered.lines.iter().enumerate() {
        if i > 0 {
            push_text(&mut segments, "\n");
        }
        if line.quote_depth > 0 {
            push_text(&mut segments, &"> ".repeat(usize::from(line.quote_depth)));
        }
        for span in &line.spans {
            match span {
                TextSpan::Text(text) => push_text(&mut segments, text),
                TextSpan::Link { index, text } => segments.push(TextSegment::Link {
                    text: text.clone(),
                    index: *index,
                }),
            }
        }
    }
    segments
}

/// Content-IDs the HTML refers to with `cid:` (ASCII case-insensitive, like most mailers).
fn cid_checker(html: Option<&str>) -> impl Fn(&str) -> bool {
    let lower = html.map(str::to_ascii_lowercase).unwrap_or_default();
    move |content_id: &str| {
        let id = content_id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        !id.is_empty() && lower.contains(&format!("cid:{}", id.to_ascii_lowercase()))
    }
}

/// Attachments of a complete message: mail-content's list without the images the HTML shows.
fn complete_attachments(parsed: &ParsedMessage) -> Vec<Attachment> {
    let referenced = cid_checker(parsed.html.as_deref());
    parsed
        .attachments
        .iter()
        .filter(|a| {
            let inline_id = parsed
                .inline
                .iter()
                .find(|i| i.part_id == a.part_id)
                .map(|i| i.content_id.as_str());
            let shown = a.content_id.as_deref().is_some_and(&referenced)
                || inline_id.is_some_and(&referenced);
            !shown
        })
        .enumerate()
        .map(|(index, a)| Attachment {
            info: AttachmentInfo {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                name: a.file_name.clone(),
                mime_type: a.mime.clone(),
                size: u32::try_from(a.size).unwrap_or(u32::MAX),
                executable: a.executable,
                local: true,
                preview: mail_content::preview_kind(&a.file_name, &a.mime),
            },
            source: AttachmentSource::Whole { part_id: a.part_id },
        })
        .collect()
}

/// Whether a part of the structure belongs to the readable body (fetched first, assembled):
/// a body text part or an inline image, outside attachments.
fn readable(part: &MimePart) -> bool {
    part.is_body_text() || part.is_inline_image()
}

/// Walks the structure in pre-order and reports, per part, whether it lies inside an attached
/// message or a part marked as attachment (whose children are not the message's own body).
fn walk(structure: &MessageStructure) -> Vec<(&MimePart, bool)> {
    let mut out = Vec::with_capacity(structure.parts.len());
    // Depths of the enclosing attachment-like parts.
    let mut enclosing: Vec<u16> = Vec::new();
    for part in &structure.parts {
        while enclosing.last().is_some_and(|&d| d >= part.depth) {
            enclosing.pop();
        }
        let inside = !enclosing.is_empty();
        out.push((part, inside));
        if part.disposition == Some(Disposition::Attachment) || part.mime_type == "message/rfc822" {
            enclosing.push(part.depth);
        }
    }
    out
}

/// Attachments of a partial body, from its structure: every leaf outside the readable body,
/// an attached message as one entry, inline images only if the HTML does not show them.
fn partial_attachments(
    structure: &MessageStructure,
    stored: &HashSet<&BodySection>,
    html: Option<&str>,
) -> Vec<Attachment> {
    let referenced = cid_checker(html);
    let mut out = Vec::new();
    for (part, inside) in walk(structure) {
        let Some(path) = &part.path else { continue };
        if inside || part.is_multipart() {
            continue;
        }
        if part.is_body_text() {
            continue;
        }
        if part.is_inline_image() && part.content_id.as_deref().is_some_and(&referenced) {
            continue;
        }
        let name = attachment_file_name(part.file_name.as_deref(), &part.mime_type);
        let executable = is_executable_type(&name, &part.mime_type);
        let preview = mail_content::preview_kind(&name, &part.mime_type);
        let size = match part.encoding.as_str() {
            // 76 characters and CRLF per 57 bytes.
            "base64" => u32::try_from(u64::from(part.size) * 57 / 78).unwrap_or(u32::MAX),
            _ => part.size,
        };
        out.push(Attachment {
            info: AttachmentInfo {
                index: u32::try_from(out.len()).unwrap_or(u32::MAX),
                name,
                mime_type: part.mime_type.clone(),
                size,
                executable,
                local: stored.contains(&BodySection::Part(path.clone())),
                preview,
            },
            source: AttachmentSource::Section { path: path.clone() },
        });
    }
    out
}

/// Header fields that describe the body of the original message and are replaced in the
/// assembled one.
fn is_body_header(line: &[u8]) -> bool {
    let name = line
        .iter()
        .position(|&b| b == b':')
        .map_or(line, |colon| &line[..colon]);
    let name = name.trim_ascii();
    [
        &b"content-type"[..],
        b"content-transfer-encoding",
        b"mime-version",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
}

/// The header block without its trailing empty line and without the fields of
/// [`is_body_header`] (folded continuation lines go with their field).
fn header_without_body_fields(header: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(header.len());
    let mut skipping = false;
    for line in header.split_inclusive(|&b| b == b'\n') {
        let content = line.trim_ascii_end();
        if content.is_empty() {
            break;
        }
        let continuation = matches!(line.first(), Some(b' ' | b'\t'));
        if !continuation {
            skipping = is_body_header(line);
        }
        if !skipping {
            out.extend_from_slice(content);
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

/// A message with the original header and structure that contains only the readable parts
/// that are stored. Multiparts without a stored readable part are left out; attached messages
/// and attachments are always left out (the attachment list comes from the structure).
pub(crate) fn assemble_partial(
    structure: &MessageStructure,
    sections: &[FetchedSection],
) -> Vec<u8> {
    let find = |section: &BodySection| {
        sections
            .iter()
            .find(|s| &s.section == section)
            .map(|s| s.data.as_slice())
    };
    let header = find(&BodySection::Header).unwrap_or_default();
    let walked = walk(structure);
    let Some(&(root, _)) = walked.first() else {
        return header.to_vec();
    };

    // A single-part message: the original header and the stored content.
    if !root.is_multipart() {
        let mut out = header.to_vec();
        if !out.ends_with(b"\r\n\r\n") && !out.ends_with(b"\n\n") {
            out.extend_from_slice(b"\r\n");
        }
        if let Some(path) = &root.path
            && let Some(content) = find(&BodySection::Part(path.clone()))
        {
            out.extend_from_slice(content);
        }
        return out;
    }

    // Which readable leaves are stored, and which multiparts contain one.
    let stored_leaf = |part: &MimePart, inside: bool| -> Option<(&[u8], &[u8])> {
        let path = part.path.as_ref()?;
        if inside || part.is_multipart() || !readable(part) {
            return None;
        }
        Some((
            find(&BodySection::Mime(path.clone()))?,
            find(&BodySection::Part(path.clone()))?,
        ))
    };
    let mut keep = vec![false; walked.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, &(part, inside)) in walked.iter().enumerate() {
        while open
            .last()
            .is_some_and(|&j| walked.get(j).is_some_and(|(p, _)| p.depth >= part.depth))
        {
            open.pop();
        }
        if part.is_multipart() && !inside {
            open.push(i);
        } else if stored_leaf(part, inside).is_some() {
            keep[i] = true;
            for &j in &open {
                keep[j] = true;
            }
        }
    }

    let boundary = |i: usize| format!("=_vm_partial_{i}");
    let mut out = header_without_body_fields(header);
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(
        format!(
            "Content-Type: {}; boundary=\"{}\"\r\n\r\n",
            root.mime_type,
            boundary(0)
        )
        .as_bytes(),
    );
    // Open multiparts: (depth, index).
    let mut stack: Vec<(u16, usize)> = vec![(root.depth, 0)];
    for (i, &(part, inside)) in walked.iter().enumerate().skip(1) {
        while stack.last().is_some_and(|&(d, _)| d >= part.depth) {
            if let Some((_, j)) = stack.pop() {
                out.extend_from_slice(format!("--{}--\r\n", boundary(j)).as_bytes());
            }
        }
        if !keep[i] {
            continue;
        }
        let Some(&(_, parent)) = stack.last() else {
            break;
        };
        out.extend_from_slice(format!("--{}\r\n", boundary(parent)).as_bytes());
        if part.is_multipart() {
            out.extend_from_slice(
                format!(
                    "Content-Type: {}; boundary=\"{}\"\r\n\r\n",
                    part.mime_type,
                    boundary(i)
                )
                .as_bytes(),
            );
            stack.push((part.depth, i));
        } else if let Some((mime, content)) = stored_leaf(part, inside) {
            out.extend_from_slice(mime);
            out.extend_from_slice(content);
            out.extend_from_slice(b"\r\n");
        }
    }
    while let Some((_, j)) = stack.pop() {
        out.extend_from_slice(format!("--{}--\r\n", boundary(j)).as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use mail_model::LARGE_MESSAGE_THRESHOLD;

    use super::*;

    fn path(s: &str) -> PartPath {
        s.parse().expect("path")
    }

    fn part(p: Option<&str>, depth: u16, mime: &str) -> MimePart {
        MimePart {
            path: p.map(path),
            depth,
            mime_type: mime.into(),
            charset: None,
            file_name: None,
            disposition: None,
            content_id: None,
            encoding: if mime.starts_with("multipart/") {
                String::new()
            } else {
                "7bit".into()
            },
            size: 10,
        }
    }

    fn section(section: BodySection, data: &str) -> FetchedSection {
        FetchedSection {
            section,
            data: data.as_bytes().to_vec(),
        }
    }

    /// mixed( alternative( text, related( html, image ) ), pdf, message/rfc822( text ) )
    fn structure() -> MessageStructure {
        let mut image = part(Some("1.2.2"), 3, "image/png");
        image.content_id = Some("logo@x".into());
        image.encoding = "base64".into();
        let mut pdf = part(Some("2"), 1, "application/pdf");
        pdf.file_name = Some("../Rechnung.pdf".into());
        pdf.disposition = Some(Disposition::Attachment);
        pdf.encoding = "base64".into();
        pdf.size = 7_800;
        let mut unused = part(Some("4"), 1, "image/gif");
        unused.content_id = Some("unused@x".into());
        MessageStructure {
            size: LARGE_MESSAGE_THRESHOLD + 1,
            parts: vec![
                part(None, 0, "multipart/mixed"),
                part(Some("1"), 1, "multipart/alternative"),
                part(Some("1.1"), 2, "text/plain"),
                part(Some("1.2"), 2, "multipart/related"),
                part(Some("1.2.1"), 3, "text/html"),
                image,
                pdf,
                part(Some("3"), 1, "message/rfc822"),
                part(Some("3.1"), 2, "text/plain"),
                unused,
            ],
        }
    }

    const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    fn stored() -> Vec<FetchedSection> {
        vec![
            section(
                BodySection::Header,
                "From: Jürgen <j@example.org>\r\nSubject: Big\r\nMIME-Version: 1.0\r\n\
                 Content-Type: multipart/mixed;\r\n boundary=\"orig\"\r\n\r\n",
            ),
            section(
                BodySection::Mime(path("1.1")),
                "Content-Type: text/plain; charset=utf-8\r\n\r\n",
            ),
            section(BodySection::Part(path("1.1")), "Plain text, grüß dich."),
            section(
                BodySection::Mime(path("1.2.1")),
                "Content-Type: text/html; charset=utf-8\r\n\r\n",
            ),
            section(
                BodySection::Part(path("1.2.1")),
                "<p>HTML <img src=\"cid:LOGO@x\"></p>",
            ),
            section(
                BodySection::Mime(path("1.2.2")),
                "Content-Type: image/png\r\nContent-ID: <logo@x>\r\n\
                 Content-Transfer-Encoding: base64\r\n\r\n",
            ),
            section(BodySection::Part(path("1.2.2")), PNG_B64),
        ]
    }

    #[test]
    fn a_partial_body_is_assembled_with_its_structure() {
        let content = Content::partial(&structure(), &stored());
        let parsed = &content.parsed;
        assert!(content.partial);
        assert_eq!(
            parsed.text.as_deref().map(str::trim),
            Some("Plain text, grüß dich.")
        );
        assert!(
            parsed
                .html
                .as_deref()
                .is_some_and(|h| h.contains("<p>HTML")),
            "{:?}",
            parsed.html
        );
        assert_eq!(parsed.headers.subject.as_deref(), Some("Big"));
        assert_eq!(
            parsed.headers.from.first().map(|a| a.email.as_str()),
            Some("j@example.org")
        );
        assert_eq!(parsed.inline.len(), 1, "the cid image is available");
        assert!(content.problems().is_empty(), "{:?}", parsed.problems);
        assert_eq!(content.formats(), [BodyFormat::Html, BodyFormat::Text]);

        // The PDF, the attached message and the unreferenced image; not the logo.
        let names: Vec<(&str, u32, bool, u32)> = content
            .attachments
            .iter()
            .map(|a| {
                (
                    a.info.name.as_str(),
                    a.info.index,
                    a.info.local,
                    a.info.size,
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("Rechnung.pdf", 0, false, 5_700),
                ("attachment.eml", 1, false, 10),
                ("attachment.gif", 2, false, 10),
            ]
        );
        assert_eq!(
            content.attachments[0].source,
            AttachmentSource::Section { path: path("2") }
        );
    }

    #[test]
    fn missing_sections_leave_their_parts_out() {
        // Only the header and the text part are stored.
        let sections: Vec<FetchedSection> = stored().into_iter().take(3).collect();
        let content = Content::partial(&structure(), &sections);
        assert!(content.parsed.html.is_none());
        assert_eq!(content.formats(), [BodyFormat::Text]);
        assert!(content.parsed.display_text().contains("Plain text"));
        // Without HTML the logo is an attachment as well.
        assert_eq!(content.attachments.len(), 4);

        // Nothing but the header: no content, never a panic.
        let content = Content::partial(&structure(), &sections[..1]);
        assert!(content.problems().contains(&MessageProblem::NoContent));
        let content = Content::partial(
            &MessageStructure {
                size: 1,
                parts: vec![],
            },
            &[],
        );
        assert!(content.problems().contains(&MessageProblem::NoContent));
    }

    #[test]
    fn a_single_part_message_keeps_its_header() {
        let structure = MessageStructure {
            size: LARGE_MESSAGE_THRESHOLD + 1,
            parts: vec![part(Some("1"), 0, "text/plain")],
        };
        let sections = vec![
            section(
                BodySection::Header,
                "Subject: One\r\nContent-Type: text/plain; charset=utf-8\r\n\
                 Content-Transfer-Encoding: quoted-printable\r\n\r\n",
            ),
            section(BodySection::Part(path("1")), "Gr=C3=BC=C3=9Fe"),
        ];
        let content = Content::partial(&structure, &sections);
        assert_eq!(content.parsed.text.as_deref().map(str::trim), Some("Grüße"));
        assert!(content.attachments.is_empty());
    }

    #[test]
    fn complete_messages_hide_the_images_the_html_shows() {
        let raw = format!(
            "From: a@example.org\r\nSubject: x\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
             --m\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n\
             --r\r\nContent-Type: text/html\r\n\r\n<p><img src=\"cid:pic@x\"></p>\r\n\
             --r\r\nContent-Type: image/png\r\nContent-ID: <pic@x>\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n{PNG_B64}\r\n--r--\r\n\
             --m\r\nContent-Type: application/x-msdownload\r\n\
             Content-Disposition: attachment; filename=\"setup.exe\"\r\n\
             Content-Transfer-Encoding: base64\r\n\r\nTVo=\r\n--m--\r\n"
        );
        let content = Content::complete(raw.as_bytes());
        let infos: Vec<&AttachmentInfo> = content.attachments.iter().map(|a| &a.info).collect();
        assert_eq!(infos.len(), 1, "{infos:?}");
        assert_eq!(infos[0].name, "setup.exe");
        assert!(infos[0].executable);
        assert!(infos[0].local);
        assert_eq!(infos[0].size, 2);
        assert!(matches!(
            content.attachments[0].source,
            AttachmentSource::Whole { .. }
        ));
    }

    #[test]
    fn text_segments_keep_lines_quotes_and_links() {
        let rendered = mail_content::render_text("Hi,\n> see https://example.org now\n\nBye");
        let segments = text_segments(&rendered);
        assert_eq!(
            segments,
            [
                TextSegment::Text {
                    text: "Hi,\n> see ".into()
                },
                TextSegment::Link {
                    text: "https://example.org".into(),
                    index: 0
                },
                TextSegment::Text {
                    text: " now\n\nBye".into()
                },
            ]
        );
    }

    #[test]
    fn problems_map_to_codes_once() {
        let content = Content::complete(b"no header block at all");
        assert!(content.problems().contains(&MessageProblem::RawFallback));
        let content = Content::complete(
            b"Subject: x\r\nContent-Type: text/plain; charset=x-made-up\r\n\r\nGr\xfc\xdfe\r\n",
        );
        let problems = content.problems();
        assert_eq!(
            problems
                .iter()
                .filter(|p| **p == MessageProblem::UnknownCharset)
                .count(),
            1,
            "{problems:?}"
        );
    }
}
