//! Message bodies (stage 3): BODYSTRUCTURE into [`MimePart`]s, and the collectors of the
//! `BODY.PEEK[...]` FETCH responses. Only PEEK forms are ever requested, so fetching a body
//! never sets `\Seen` (the read-only allowlist refuses the other forms anyway).
//!
//! Nothing here may panic on server data; deep nesting is walked without recursion.

use std::borrow::Cow;

use async_imap::imap_proto::{
    AttributeValue, BodyContentCommon, BodyContentSinglePart, BodyParams, BodyStructure,
    ContentEncoding, MessageSection, Response, SectionPath,
};
use mail_model::{
    BodySection, Disposition, FetchedSection, MessageStructure, MimePart, PartPath, RawMessage,
    Uid, UidSet,
};

use crate::convert::{decode_text, uid_of};

/// FETCH items of `fetch_structure`.
pub(crate) const STRUCTURE_ITEMS: &str = "UID RFC822.SIZE BODYSTRUCTURE";

/// FETCH items of `fetch_messages`.
pub(crate) const MESSAGE_ITEMS: &str = "UID BODY.PEEK[]";

/// FETCH items of `fetch_sections`: `UID BODY.PEEK[HEADER] BODY.PEEK[1.1.MIME] ...`.
pub(crate) fn section_items(sections: &[BodySection]) -> String {
    let mut items = String::from("UID");
    for section in sections {
        items.push_str(" BODY.PEEK[");
        items.push_str(&section.spec());
        items.push(']');
    }
    items
}

// ---------------------------------------------------------------------------------------------
// BODYSTRUCTURE

/// The MIME tree in pre-order with IMAP part numbers (RFC 3501 section 6.4.5):
/// - the root of a single-part message is part 1; a multipart root has no number and its
///   children are 1, 2, ...;
/// - a multipart inside a multipart has a number, its children extend it (1.1, 1.2);
/// - the body of a `message/rfc822` part n: a multipart has no number of its own and its
///   children are n.1, n.2; a single part is n.1.
pub(crate) fn structure_parts(root: &BodyStructure<'_>) -> Vec<MimePart> {
    let mut out = Vec::new();
    let mut stack = vec![message_body(root, Vec::new(), 0)];
    while let Some(Pending {
        node,
        path,
        base,
        depth,
    }) = stack.pop()
    {
        let part_path = path.clone().and_then(PartPath::new);
        let child_depth = depth.saturating_add(1);
        match node {
            BodyStructure::Multipart { common, bodies, .. } => {
                out.push(mime_part(part_path, depth, common, None));
                let prefix = path.unwrap_or(base);
                // Reverse, so the stack yields the children in order.
                for (i, child) in bodies.iter().enumerate().rev() {
                    let n = u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1);
                    let mut child_path = prefix.clone();
                    child_path.push(n);
                    stack.push(Pending {
                        node: child,
                        path: Some(child_path),
                        base: Vec::new(),
                        depth: child_depth,
                    });
                }
            }
            BodyStructure::Message {
                common,
                other,
                body,
                ..
            } => {
                out.push(mime_part(part_path, depth, common, Some(other)));
                stack.push(message_body(body, path.unwrap_or(base), child_depth));
            }
            BodyStructure::Text { common, other, .. }
            | BodyStructure::Basic { common, other, .. } => {
                out.push(mime_part(part_path, depth, common, Some(other)));
            }
        }
    }
    out
}

/// A node of [`structure_parts`] waiting to be visited.
struct Pending<'s, 'a> {
    node: &'s BodyStructure<'a>,
    /// The node's own number, `None` for the multipart body of a message.
    path: Option<Vec<u32>>,
    /// Number the children of a message body extend (the message's number, or nothing).
    base: Vec<u32>,
    depth: u16,
}

/// The body of a message whose number is `number` (empty for the root).
fn message_body<'s, 'a>(
    node: &'s BodyStructure<'a>,
    number: Vec<u32>,
    depth: u16,
) -> Pending<'s, 'a> {
    match node {
        BodyStructure::Multipart { .. } => Pending {
            node,
            path: None,
            base: number,
            depth,
        },
        _ => {
            let mut path = number;
            path.push(1);
            Pending {
                node,
                path: Some(path),
                base: Vec::new(),
                depth,
            }
        }
    }
}

fn mime_part(
    path: Option<PartPath>,
    depth: u16,
    common: &BodyContentCommon<'_>,
    single: Option<&BodyContentSinglePart<'_>>,
) -> MimePart {
    let mime_type =
        format!("{}/{}", common.ty.ty.trim(), common.ty.subtype.trim()).to_ascii_lowercase();
    let disposition = common.disposition.as_ref().map(|d| {
        if d.ty.trim().eq_ignore_ascii_case("inline") {
            Disposition::Inline
        } else {
            // RFC 2183: an unknown disposition is treated as attachment.
            Disposition::Attachment
        }
    });
    let file_name = common
        .disposition
        .as_ref()
        .and_then(|d| param(&d.params, "filename"))
        .or_else(|| param(&common.ty.params, "name"))
        .filter(|n| !n.is_empty());
    let charset = param(&common.ty.params, "charset").filter(|c| !c.is_empty());
    let (content_id, encoding, size) = match single {
        Some(other) => (
            other.id.as_deref().and_then(clean_content_id),
            encoding_name(&other.transfer_encoding),
            other.octets,
        ),
        None => (None, String::new(), 0),
    };
    MimePart {
        path,
        depth,
        mime_type,
        charset,
        file_name,
        disposition,
        content_id,
        encoding,
        size,
    }
}

fn encoding_name(encoding: &ContentEncoding<'_>) -> String {
    match encoding {
        ContentEncoding::SevenBit => "7bit".into(),
        ContentEncoding::EightBit => "8bit".into(),
        ContentEncoding::Binary => "binary".into(),
        ContentEncoding::Base64 => "base64".into(),
        ContentEncoding::QuotedPrintable => "quoted-printable".into(),
        ContentEncoding::Other(other) => other.trim().to_ascii_lowercase(),
    }
}

fn clean_content_id(id: &str) -> Option<String> {
    let id = id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// Value of parameter `name` (ASCII case-insensitive). RFC 2231 forms (`name*`, or the
/// continuations `name*0*`, `name*1`, ...) win over the plain one; encoded words (RFC 2047),
/// which many clients put into plain values, are decoded too.
pub(crate) fn param(params: &BodyParams<'_>, name: &str) -> Option<String> {
    let params = params.as_ref()?;
    let mut plain = None;
    let mut extended = None;
    let mut pieces: Vec<(u32, bool, &str)> = Vec::new();
    for (key, value) in params {
        let key = key.trim().to_ascii_lowercase();
        let value: &str = value.as_ref();
        if key == name {
            plain = Some(value);
            continue;
        }
        let Some(rest) = key.strip_prefix(name).and_then(|r| r.strip_prefix('*')) else {
            continue;
        };
        if rest.is_empty() {
            extended = Some(value);
            continue;
        }
        let (number, encoded) = match rest.strip_suffix('*') {
            Some(number) => (number, true),
            None => (rest, false),
        };
        if number.bytes().all(|b| b.is_ascii_digit())
            && let Ok(n) = number.parse::<u32>()
        {
            pieces.push((n, encoded, value));
        }
    }
    if let Some(value) = extended {
        let (charset, text) = split_charset(value);
        return Some(decode_charset(charset, &percent_decode(text)));
    }
    if !pieces.is_empty() {
        pieces.sort_by_key(|(n, _, _)| *n);
        let mut charset = None;
        let mut bytes = Vec::new();
        for (i, (_, encoded, value)) in pieces.iter().enumerate() {
            if *encoded {
                let text = if i == 0 {
                    let (cs, text) = split_charset(value);
                    charset = cs;
                    text
                } else {
                    value
                };
                bytes.extend(percent_decode(text));
            } else {
                bytes.extend_from_slice(value.as_bytes());
            }
        }
        return Some(decode_charset(charset, &bytes));
    }
    plain.map(|value| decode_text(value.as_bytes()))
}

/// `charset'language'value` → (charset, value). Without the two quotes the whole text is the
/// value.
fn split_charset(value: &str) -> (Option<&str>, &str) {
    let mut parts = value.splitn(3, '\'');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(charset), Some(_language), Some(text)) => {
            (Some(charset.trim()).filter(|c| !c.is_empty()), text)
        }
        _ => (None, value),
    }
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = |j: usize| bytes.get(j).and_then(|h| char::from(*h).to_digit(16));
        if b == b'%'
            && let (Some(hi), Some(lo)) = (hex(i + 1), hex(i + 2))
        {
            out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Bytes in `charset` as text. UTF-8 and ASCII directly; other charsets through mail-parser,
/// as one Q-encoded word (every byte escaped), so they get the same charset tables as headers.
fn decode_charset(charset: Option<&str>, bytes: &[u8]) -> String {
    let utf8 = charset
        .is_none_or(|c| c.eq_ignore_ascii_case("utf-8") || c.eq_ignore_ascii_case("us-ascii"));
    let safe_name = charset.is_some_and(|c| {
        c.len() <= 40
            && c.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
    });
    if utf8 || !safe_name || bytes.is_ascii() {
        let text = String::from_utf8_lossy(bytes);
        return text.replace(['\r', '\n', '\0'], " ").trim().to_owned();
    }
    let mut word = format!("=?{}?Q?", charset.unwrap_or("utf-8"));
    for b in bytes {
        word.push_str(&format!("={b:02X}"));
    }
    word.push_str("?=");
    decode_text(word.as_bytes())
}

/// Collects the answer of `fetch_structure` for one UID.
pub(crate) struct StructureCollector {
    uid: Uid,
    size: Option<u32>,
    parts: Option<Vec<MimePart>>,
    /// A FETCH for the UID without the requested data (a flag change on the side).
    pub(crate) unsolicited: bool,
}

impl StructureCollector {
    pub(crate) fn new(uid: Uid) -> Self {
        Self {
            uid,
            size: None,
            parts: None,
            unsolicited: false,
        }
    }

    pub(crate) fn take(&mut self, resp: &Response<'_>) -> bool {
        let Response::Fetch(_, attrs) = resp else {
            return false;
        };
        if uid_of(attrs) != Some(self.uid) {
            return false;
        }
        let mut found = false;
        for attr in attrs {
            match attr {
                AttributeValue::Rfc822Size(size) => {
                    self.size = Some(*size);
                    found = true;
                }
                AttributeValue::BodyStructure(body) => {
                    self.parts = Some(structure_parts(body));
                    found = true;
                }
                _ => {}
            }
        }
        if !found {
            self.unsolicited = true;
        }
        true
    }

    /// `None` if the server sent no structure (the UID does not exist).
    pub(crate) fn finish(self) -> Option<MessageStructure> {
        let parts = self.parts?;
        Some(MessageStructure {
            size: self.size.unwrap_or(0),
            parts,
        })
    }
}

/// Collects the answers of `fetch_messages`.
pub(crate) struct MessageCollector<'a> {
    wanted: &'a UidSet,
    pub(crate) messages: Vec<RawMessage>,
    pub(crate) unsolicited: bool,
}

impl<'a> MessageCollector<'a> {
    pub(crate) fn new(wanted: &'a UidSet) -> Self {
        Self {
            wanted,
            messages: Vec::new(),
            unsolicited: false,
        }
    }

    pub(crate) fn take(&mut self, resp: &Response<'_>) -> bool {
        let Response::Fetch(_, attrs) = resp else {
            return false;
        };
        let Some(uid) = uid_of(attrs).filter(|uid| self.wanted.contains(*uid)) else {
            return false;
        };
        let body = attrs.iter().find_map(|attr| match attr {
            AttributeValue::BodySection {
                section: None,
                data: Some(data),
                ..
            } => Some(data),
            _ => None,
        });
        match body {
            Some(data) => {
                if let Some(existing) = self.messages.iter_mut().find(|m| m.uid == uid) {
                    existing.data = data.to_vec();
                } else {
                    self.messages.push(RawMessage {
                        uid,
                        data: data.to_vec(),
                    });
                }
            }
            None => self.unsolicited = true,
        }
        true
    }
}

/// Collects the answer of `fetch_sections` for one UID.
pub(crate) struct SectionCollector<'a> {
    uid: Uid,
    wanted: &'a [BodySection],
    /// `None` until a FETCH for the UID arrived.
    pub(crate) sections: Option<Vec<FetchedSection>>,
    pub(crate) unsolicited: bool,
}

impl<'a> SectionCollector<'a> {
    pub(crate) fn new(uid: Uid, wanted: &'a [BodySection]) -> Self {
        Self {
            uid,
            wanted,
            sections: None,
            unsolicited: false,
        }
    }

    pub(crate) fn take(&mut self, resp: &Response<'_>) -> bool {
        let Response::Fetch(_, attrs) = resp else {
            return false;
        };
        if uid_of(attrs) != Some(self.uid) {
            return false;
        }
        let mut found = false;
        for attr in attrs {
            let AttributeValue::BodySection {
                section: Some(path),
                data,
                ..
            } = attr
            else {
                continue;
            };
            let Some(section) = body_section(path).filter(|s| self.wanted.contains(s)) else {
                continue;
            };
            found = true;
            let data = data.as_ref().map_or(Cow::Borrowed(&[][..]), Cow::clone);
            let sections = self.sections.get_or_insert_with(Vec::new);
            match sections.iter_mut().find(|s| s.section == section) {
                Some(existing) => existing.data = data.into_owned(),
                None => sections.push(FetchedSection {
                    section,
                    data: data.into_owned(),
                }),
            }
        }
        if !found {
            self.unsolicited = true;
        }
        true
    }
}

/// The model form of a section in a FETCH response; `None` for forms never requested.
fn body_section(path: &SectionPath) -> Option<BodySection> {
    match path {
        SectionPath::Full(MessageSection::Header) => Some(BodySection::Header),
        SectionPath::Part(numbers, None) => PartPath::new(numbers.clone()).map(BodySection::Part),
        SectionPath::Part(numbers, Some(MessageSection::Mime)) => {
            PartPath::new(numbers.clone()).map(BodySection::Mime)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use async_imap::imap_proto::Response;

    use super::*;

    fn parse(line: &[u8]) -> Response<'_> {
        match Response::parse(line) {
            Ok(([], resp)) => resp,
            other => panic!("test response does not parse: {other:?}"),
        }
    }

    fn structure(body: &[u8]) -> Vec<MimePart> {
        let mut line = b"* 1 FETCH (UID 9 RFC822.SIZE 4321 BODYSTRUCTURE ".to_vec();
        line.extend_from_slice(body);
        line.extend_from_slice(b")\r\n");
        let resp = parse(&line);
        let mut collector = StructureCollector::new(9);
        assert!(collector.take(&resp));
        let structure = collector.finish().expect("structure");
        assert_eq!(structure.size, 4321);
        structure.parts
    }

    fn summary(parts: &[MimePart]) -> Vec<String> {
        parts
            .iter()
            .map(|p| {
                let path = p.path.as_ref().map_or("-".to_owned(), ToString::to_string);
                format!("{}{path} {}", " ".repeat(usize::from(p.depth)), p.mime_type)
            })
            .collect()
    }

    #[test]
    fn single_part_message_is_part_one() {
        let parts =
            structure(b"(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"iso-8859-1\") NIL NIL \"QUOTED-PRINTABLE\" 120 4 NIL NIL NIL NIL)");
        assert_eq!(summary(&parts), ["1 text/plain"]);
        let p = &parts[0];
        assert_eq!(p.charset.as_deref(), Some("iso-8859-1"));
        assert_eq!((p.encoding.as_str(), p.size), ("quoted-printable", 120));
        assert!(p.is_body_text());
    }

    #[test]
    fn nested_multiparts_and_attached_messages_are_numbered_as_in_rfc_3501() {
        // mixed( related( alternative(plain, html), png cid ), pdf attachment,
        //        message/rfc822( mixed(plain, jpeg) ), message/rfc822( plain ) )
        let plain = r#"("TEXT" "PLAIN" ("CHARSET" "utf-8") NIL NIL "7BIT" 10 1 NIL NIL NIL NIL)"#;
        let html = r#"("TEXT" "HTML" ("CHARSET" "utf-8") NIL NIL "BASE64" 300 4 NIL NIL NIL NIL)"#;
        let alternative = format!(r#"({plain}{html} "ALTERNATIVE" ("BOUNDARY" "a") NIL NIL NIL)"#);
        let logo =
            r#"("IMAGE" "PNG" NIL "<logo@seed>" NIL "BASE64" 900 NIL ("INLINE" NIL) NIL NIL)"#;
        let related = format!(r#"({alternative}{logo} "RELATED" ("BOUNDARY" "r") NIL NIL NIL)"#);
        let pdf = r#"("APPLICATION" "PDF" ("NAME" "x.pdf") NIL NIL "BASE64" 5000 NIL ("ATTACHMENT" ("FILENAME" "Rechnung.pdf")) NIL NIL)"#;
        let envelope = r#"(NIL "inner" NIL NIL NIL NIL NIL NIL NIL NIL)"#;
        let inner = r#"(("TEXT" "PLAIN" NIL NIL NIL "7BIT" 5 1 NIL NIL NIL NIL)("IMAGE" "JPEG" NIL NIL NIL "BASE64" 400 NIL NIL NIL NIL) "MIXED" ("BOUNDARY" "m") NIL NIL NIL)"#;
        let forwarded = format!(
            r#"("MESSAGE" "RFC822" NIL NIL NIL "7BIT" 800 {envelope} {inner} 20 NIL ("ATTACHMENT" NIL) NIL NIL)"#
        );
        let simple = format!(
            r#"("MESSAGE" "RFC822" NIL NIL NIL "7BIT" 300 {envelope} ("TEXT" "PLAIN" NIL NIL NIL "7BIT" 5 1 NIL NIL NIL NIL) 8 NIL NIL NIL NIL)"#
        );
        let body =
            format!(r#"({related}{pdf}{forwarded}{simple} "MIXED" ("BOUNDARY" "x") NIL NIL NIL)"#);
        let parts = structure(body.as_bytes());
        assert_eq!(
            summary(&parts),
            [
                "- multipart/mixed",
                " 1 multipart/related",
                "  1.1 multipart/alternative",
                "   1.1.1 text/plain",
                "   1.1.2 text/html",
                "  1.2 image/png",
                " 2 application/pdf",
                " 3 message/rfc822",
                "  - multipart/mixed",
                "   3.1 text/plain",
                "   3.2 image/jpeg",
                " 4 message/rfc822",
                "  4.1 text/plain",
            ]
        );
        let logo = &parts[5];
        assert_eq!(logo.content_id.as_deref(), Some("logo@seed"));
        assert_eq!(logo.disposition, Some(Disposition::Inline));
        assert!(logo.is_inline_image());
        let pdf = &parts[6];
        assert_eq!(
            pdf.file_name.as_deref(),
            Some("Rechnung.pdf"),
            "filename wins over name"
        );
        assert_eq!(pdf.disposition, Some(Disposition::Attachment));
        assert_eq!(parts[0].encoding, "");
        assert_eq!(parts[0].size, 0);
    }

    #[test]
    fn deep_nesting_does_not_recurse() {
        let mut body = b"(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 3 1)".to_vec();
        for _ in 0..40 {
            let mut outer = b"(".to_vec();
            outer.extend_from_slice(&body);
            outer.extend_from_slice(b" \"MIXED\")");
            body = outer;
        }
        let parts = structure(&body);
        assert_eq!(parts.len(), 41);
        let leaf = parts.last().expect("leaf");
        assert_eq!(leaf.depth, 40);
        assert_eq!(
            leaf.path.as_ref().map(ToString::to_string).as_deref(),
            Some(vec!["1"; 40].join(".").as_str())
        );
    }

    fn params(pairs: &[(&'static str, &'static str)]) -> BodyParams<'static> {
        Some(
            pairs
                .iter()
                .map(|(k, v)| (Cow::Borrowed(*k), Cow::Borrowed(*v)))
                .collect(),
        )
    }

    #[test]
    fn file_names_in_rfc_2231_and_2047() {
        type Case<'a> = (&'a [(&'a str, &'a str)], Option<&'a str>);
        let cases: [Case<'_>; 9] = [
            (&[("FILENAME", "plain.txt")], Some("plain.txt")),
            (
                &[("filename*", "utf-8''Rechnung%20M%C3%A4rz%202026.pdf")],
                Some("Rechnung März 2026.pdf"),
            ),
            (
                &[("filename*", "iso-8859-1'de'%DCbersicht.csv")],
                Some("Übersicht.csv"),
            ),
            (
                &[
                    ("filename*1*", "%20lang.pdf"),
                    ("filename*0*", "utf-8''sehr"),
                    ("filename", "fallback.pdf"),
                ],
                Some("sehr lang.pdf"),
            ),
            (
                &[("filename*0", "part one "), ("filename*1", "and two.txt")],
                Some("part one and two.txt"),
            ),
            (
                &[("filename", "=?utf-8?B?6K2w5LqL6YyyLnR4dA==?=")],
                Some("議事録.txt"),
            ),
            (&[("FileName*", "''no-charset.txt")], Some("no-charset.txt")),
            (
                &[("filename*", "utf-8''bad%zzescape")],
                Some("bad%zzescape"),
            ),
            (&[("name", "other.txt")], None),
        ];
        for (pairs, want) in cases {
            assert_eq!(
                param(&params(pairs), "filename").as_deref(),
                want,
                "{pairs:?}"
            );
        }
        assert_eq!(param(&None, "filename"), None);
        // A charset name that cannot be put into an encoded word is read as UTF-8.
        assert_eq!(
            param(&params(&[("filename*", "x?y''a%C3%A4.txt")]), "filename").as_deref(),
            Some("aä.txt")
        );
    }

    #[test]
    fn whole_messages_are_collected_by_uid() {
        let wanted = UidSet::from_uids([5, 6, 7]);
        let mut collector = MessageCollector::new(&wanted);
        assert!(collector.take(&parse(
            b"* 1 FETCH (UID 5 BODY[] {12}\r\nSubject: a\r\n)\r\n"
        )));
        assert!(collector.take(&parse(
            b"* 2 FETCH (BODY[] {15}\r\nSubject: bb\r\n\r\n UID 6)\r\n"
        )));
        // Flags only: changed on the side.
        assert!(collector.take(&parse(b"* 3 FETCH (UID 7 FLAGS (\\Seen))\r\n")));
        // Not asked for.
        assert!(!collector.take(&parse(b"* 9 FETCH (UID 99 BODY[] {1}\r\nx)\r\n")));
        assert!(!collector.take(&parse(b"* 9 EXISTS\r\n")));
        assert!(collector.unsolicited);
        let got: Vec<(u32, &[u8])> = collector
            .messages
            .iter()
            .map(|m| (m.uid, m.data.as_slice()))
            .collect();
        assert_eq!(
            got,
            [
                (5, &b"Subject: a\r\n"[..]),
                (6, &b"Subject: bb\r\n\r\n"[..])
            ]
        );
    }

    #[test]
    fn sections_are_collected_and_mapped_back() {
        let wanted = [
            BodySection::Header,
            BodySection::Mime("1.2".parse().expect("path")),
            BodySection::Part("1.2".parse().expect("path")),
        ];
        assert_eq!(
            section_items(&wanted),
            "UID BODY.PEEK[HEADER] BODY.PEEK[1.2.MIME] BODY.PEEK[1.2]"
        );
        let mut collector = SectionCollector::new(4, &wanted);
        assert!(!collector.take(&parse(b"* 1 FETCH (UID 3 BODY[HEADER] {2}\r\n\r\n)\r\n")));
        assert!(collector.sections.is_none());
        assert!(collector.take(&parse(
            b"* 1 FETCH (UID 4 BODY[HEADER] {11}\r\nSubject: \r\n BODY[1.2.MIME] {27}\r\nContent-Type: text/html\r\n\r\n BODY[1.2] NIL BODY[3] {1}\r\nx)\r\n"
        )));
        let sections = collector.sections.expect("sections");
        let got: Vec<(String, usize)> = sections
            .iter()
            .map(|s| (s.section.spec(), s.data.len()))
            .collect();
        // BODY[3] was not asked for; NIL becomes empty data.
        assert_eq!(
            got,
            [
                ("HEADER".to_owned(), 11),
                ("1.2.MIME".to_owned(), 27),
                ("1.2".to_owned(), 0)
            ]
        );
        assert!(!collector.unsolicited);
    }
}
