//! Message bodies as the adapter fetches them (stage 3): the MIME structure from BODYSTRUCTURE,
//! IMAP body sections and the rule for what to fetch first of a large message.
//!
//! Internal types (adapter results, storage), not exported to the UI. All fetching is read-only:
//! the adapter only uses `BODY.PEEK[...]`, which never sets `\Seen` (decision 0009).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Uid;

/// Messages above this RFC822.SIZE are opened text first: the header and the text parts (plus
/// inline images while the total stays below the threshold), attachments only on request.
/// Below it the whole message comes with one `BODY.PEEK[]` (decision 0009): 2 MiB is far above
/// text and HTML mails (a large newsletter is 50 to 200 KB) and costs about 0.3 s at 50 Mbit/s,
/// so only attachment-heavy mails take the two-step path.
pub const LARGE_MESSAGE_THRESHOLD: u32 = 2 * 1024 * 1024;

/// IMAP part number (RFC 3501 section 6.4.5): `1`, `1.2`, `2.1.3`. Never empty, every number at
/// least 1. Text form with dots, also on the wire and in the database.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartPath(Vec<u32>);

impl PartPath {
    /// `None` if `numbers` is empty or contains a 0.
    pub fn new(numbers: Vec<u32>) -> Option<Self> {
        (!numbers.is_empty() && numbers.iter().all(|&n| n > 0)).then_some(Self(numbers))
    }

    pub fn numbers(&self) -> &[u32] {
        &self.0
    }

    /// The path of the `n`-th child (1-based) of this part.
    pub fn child(&self, n: u32) -> Option<Self> {
        let mut numbers = self.0.clone();
        numbers.push(n);
        Self::new(numbers)
    }
}

impl fmt::Display for PartPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, n) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{n}")?;
        }
        Ok(())
    }
}

impl FromStr for PartPath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let numbers = s
            .split('.')
            .map(|n| {
                // `u32::from_str` accepts a leading `+`; IMAP part numbers are plain digits.
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!("part number {s:?}"));
                }
                n.parse::<u32>().map_err(|_| format!("part number {s:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(numbers).ok_or_else(|| format!("part number {s:?}"))
    }
}

impl Serialize for PartPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PartPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// One fetchable piece of a message. The adapter requests it as `BODY.PEEK[<spec>]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BodySection {
    /// `HEADER`: the header of the message (up to and including the empty line).
    Header,
    /// `1.2`: the content of a leaf part, still transfer-encoded (base64, quoted-printable).
    Part(PartPath),
    /// `1.2.MIME`: the MIME header of a part. With [`BodySection::Part`] it forms a complete
    /// single-part MIME entity that a parser can decode on its own.
    Mime(PartPath),
}

impl BodySection {
    /// The section text inside `BODY[...]`.
    pub fn spec(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for BodySection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header => f.write_str("HEADER"),
            Self::Part(path) => write!(f, "{path}"),
            Self::Mime(path) => write!(f, "{path}.MIME"),
        }
    }
}

impl FromStr for BodySection {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("HEADER") {
            return Ok(Self::Header);
        }
        let upper = s.to_ascii_uppercase();
        match upper.strip_suffix(".MIME") {
            Some(path) => path.parse().map(Self::Mime),
            None => s.parse().map(Self::Part),
        }
        .map_err(|_| format!("body section {s:?}"))
    }
}

impl Serialize for BodySection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for BodySection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// `Content-Disposition` of a part. Unknown values count as `attachment` (RFC 2183).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    Inline,
    Attachment,
}

/// One node of the MIME tree from BODYSTRUCTURE. A [`MessageStructure`] lists them in pre-order
/// with their depth, which is enough to rebuild the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MimePart {
    /// IMAP part number. `None` for the multipart body of a message (the root, or the body of an
    /// attached `message/rfc822`), which has no number of its own (RFC 3501 section 6.4.5).
    pub path: Option<PartPath>,
    /// Nesting depth; the root is 0.
    pub depth: u16,
    /// Lower-case `type/subtype`, for example `text/html`, `multipart/alternative`.
    pub mime_type: String,
    /// `charset` parameter, as sent.
    pub charset: Option<String>,
    /// File name: `filename` of the disposition, else `name` of the content type. Decoded
    /// (RFC 2231 and RFC 2047) but **not sanitized**; never use it as a path.
    pub file_name: Option<String>,
    pub disposition: Option<Disposition>,
    /// `Content-ID` without angle brackets (target of `cid:` URLs).
    pub content_id: Option<String>,
    /// Lower-case `Content-Transfer-Encoding` (`base64`, `quoted-printable`, `7bit`, ...);
    /// empty for multipart nodes.
    pub encoding: String,
    /// Size of the still encoded content in bytes, as the server reports it; 0 for multipart
    /// nodes.
    pub size: u32,
}

impl MimePart {
    pub fn is_multipart(&self) -> bool {
        self.mime_type.starts_with("multipart/")
    }

    /// Part of the readable body rather than a file: `text/plain` or `text/html`, not marked as
    /// attachment, and either without a file name or explicitly inline.
    pub fn is_body_text(&self) -> bool {
        matches!(self.mime_type.as_str(), "text/plain" | "text/html")
            && self.disposition != Some(Disposition::Attachment)
            && (self.file_name.is_none() || self.disposition == Some(Disposition::Inline))
    }

    /// An image the HTML can show inline (`cid:`): `image/*` with a Content-ID, not marked as
    /// attachment.
    pub fn is_inline_image(&self) -> bool {
        self.mime_type.starts_with("image/")
            && self.content_id.is_some()
            && self.disposition != Some(Disposition::Attachment)
    }
}

/// RFC822.SIZE and the MIME tree of one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageStructure {
    pub size: u32,
    /// Pre-order; the first entry is the root.
    pub parts: Vec<MimePart>,
}

/// What [`MessageStructure::fetch_plan`] decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyFetchPlan {
    /// One `BODY.PEEK[]`.
    Whole,
    /// These sections, in this order, in one FETCH.
    Sections(Vec<BodySection>),
}

impl MessageStructure {
    /// What to fetch when the message is opened (or prefetched with a size limit):
    /// - up to `limit` bytes (RFC822.SIZE): the whole message;
    /// - above: the header and every body text part (with its MIME header) whatever its size,
    ///   then inline images in order while the encoded total stays within `limit`. Parts inside
    ///   an attachment (an attached `message/rfc822`) count as attachment. Everything else
    ///   comes on request.
    pub fn fetch_plan(&self, limit: u32) -> BodyFetchPlan {
        if self.size <= limit {
            return BodyFetchPlan::Whole;
        }
        let mut sections = vec![BodySection::Header];
        let mut images = Vec::new();
        let mut total: u64 = 0;
        // Depths of the attachment ancestors of the current node.
        let mut attachment_depths: Vec<u16> = Vec::new();
        for part in &self.parts {
            while attachment_depths.last().is_some_and(|&d| d >= part.depth) {
                attachment_depths.pop();
            }
            let in_attachment = !attachment_depths.is_empty();
            if part.disposition == Some(Disposition::Attachment) {
                attachment_depths.push(part.depth);
            }
            let Some(path) = part.path.as_ref().filter(|_| !part.is_multipart()) else {
                continue;
            };
            if in_attachment {
                continue;
            }
            if part.is_body_text() {
                total += u64::from(part.size);
                sections.push(BodySection::Mime(path.clone()));
                sections.push(BodySection::Part(path.clone()));
            } else if part.is_inline_image() {
                images.push((path.clone(), part.size));
            }
        }
        for (path, size) in images {
            total += u64::from(size);
            if total > u64::from(limit) {
                break;
            }
            sections.push(BodySection::Mime(path.clone()));
            sections.push(BodySection::Part(path));
        }
        BodyFetchPlan::Sections(sections)
    }

    /// The part with this number, if any.
    pub fn part(&self, path: &PartPath) -> Option<&MimePart> {
        self.parts.iter().find(|p| p.path.as_ref() == Some(path))
    }
}

/// The whole raw message of one UID, from `BODY.PEEK[]`.
#[derive(Clone, PartialEq, Eq)]
pub struct RawMessage {
    pub uid: Uid,
    pub data: Vec<u8>,
}

impl fmt::Debug for RawMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawMessage(uid {}, {} bytes)", self.uid, self.data.len())
    }
}

/// One fetched section, raw as the server sent it.
#[derive(Clone, PartialEq, Eq)]
pub struct FetchedSection {
    pub section: BodySection,
    pub data: Vec<u8>,
}

impl fmt::Debug for FetchedSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FetchedSection({}, {} bytes)",
            self.section,
            self.data.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> PartPath {
        s.parse().expect("part path")
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
                "base64".into()
            },
            size: 0,
        }
    }

    #[test]
    fn part_paths_and_sections_round_trip_as_text() {
        assert_eq!(path("1.2.10").numbers(), [1, 2, 10]);
        assert_eq!(path("1.2.10").to_string(), "1.2.10");
        assert_eq!(path("3").child(1), Some(path("3.1")));
        for bad in [
            "",
            "0",
            "1..2",
            "1.0",
            "a",
            "1.",
            "+1",
            "1.-2",
            "99999999999",
        ] {
            assert!(bad.parse::<PartPath>().is_err(), "{bad:?}");
        }
        assert_eq!(PartPath::new(vec![]), None);

        let cases = [
            (BodySection::Header, "HEADER"),
            (BodySection::Part(path("1.2")), "1.2"),
            (BodySection::Mime(path("1.2")), "1.2.MIME"),
        ];
        for (section, text) in cases {
            assert_eq!(section.spec(), text);
            assert_eq!(text.parse::<BodySection>(), Ok(section.clone()));
            let json = serde_json::to_string(&section).expect("serialize");
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<BodySection>(&json).expect("deserialize"),
                section
            );
        }
        assert_eq!("header".parse::<BodySection>(), Ok(BodySection::Header));
        assert_eq!(
            "2.mime".parse::<BodySection>(),
            Ok(BodySection::Mime(path("2")))
        );
        for bad in ["TEXT", "MIME", "1.HEADER", "", "x.MIME"] {
            assert!(bad.parse::<BodySection>().is_err(), "{bad:?}");
        }
    }

    /// multipart/mixed
    ///   multipart/related (1)
    ///     multipart/alternative (1.1)
    ///       text/plain (1.1.1)
    ///       text/html (1.1.2)
    ///     image/png cid (1.2)
    ///   application/pdf attachment (2)
    ///   message/rfc822 attachment (3)
    ///     multipart/mixed (no number)
    ///       text/plain (3.1)
    ///       image/png cid (3.2)
    ///   image/jpeg cid inline, too large for the budget (4)
    fn newsletter_with_attachments(size: u32) -> MessageStructure {
        let mut text = part(Some("1.1.1"), 3, "text/plain");
        text.size = 4_000;
        let mut html = part(Some("1.1.2"), 3, "text/html");
        html.size = 2_500_000;
        let mut logo = part(Some("1.2"), 2, "image/png");
        logo.content_id = Some("logo@x".into());
        logo.size = 30_000;
        let mut pdf = part(Some("2"), 1, "application/pdf");
        pdf.disposition = Some(Disposition::Attachment);
        pdf.file_name = Some("a.pdf".into());
        pdf.size = 3_000_000;
        let mut forwarded = part(Some("3"), 1, "message/rfc822");
        forwarded.disposition = Some(Disposition::Attachment);
        let inner_text = part(Some("3.1"), 3, "text/plain");
        let mut inner_image = part(Some("3.2"), 3, "image/png");
        inner_image.content_id = Some("inner@x".into());
        let mut photo = part(Some("4"), 1, "image/jpeg");
        photo.content_id = Some("photo@x".into());
        photo.disposition = Some(Disposition::Inline);
        photo.size = 900_000;
        MessageStructure {
            size,
            parts: vec![
                part(None, 0, "multipart/mixed"),
                part(Some("1"), 1, "multipart/related"),
                part(Some("1.1"), 2, "multipart/alternative"),
                text,
                html,
                logo,
                pdf,
                forwarded,
                part(None, 2, "multipart/mixed"),
                inner_text,
                inner_image,
                photo,
            ],
        }
    }

    #[test]
    fn small_messages_are_fetched_whole() {
        let structure = newsletter_with_attachments(LARGE_MESSAGE_THRESHOLD);
        assert_eq!(
            structure.fetch_plan(LARGE_MESSAGE_THRESHOLD),
            BodyFetchPlan::Whole
        );
    }

    #[test]
    fn large_messages_get_header_text_and_inline_images_within_budget() {
        let structure = newsletter_with_attachments(7_000_000);
        let BodyFetchPlan::Sections(sections) = structure.fetch_plan(LARGE_MESSAGE_THRESHOLD)
        else {
            panic!("sections");
        };
        let specs: Vec<String> = sections.iter().map(BodySection::spec).collect();
        // Text parts always (the HTML alone is 2.5 MB), the logo did not fit after them; the
        // PDF and everything inside the attached message stay on the server.
        assert_eq!(
            specs,
            ["HEADER", "1.1.1.MIME", "1.1.1", "1.1.2.MIME", "1.1.2"],
            "{specs:?}"
        );

        // With a larger budget the inline images come too, in order, while they fit.
        let BodyFetchPlan::Sections(sections) = structure.fetch_plan(3_000_000) else {
            panic!("sections");
        };
        let specs: Vec<String> = sections.iter().map(BodySection::spec).collect();
        assert_eq!(
            specs,
            [
                "HEADER",
                "1.1.1.MIME",
                "1.1.1",
                "1.1.2.MIME",
                "1.1.2",
                "1.2.MIME",
                "1.2"
            ],
            "{specs:?}"
        );
        assert_eq!(
            structure
                .part(&path("2"))
                .and_then(|p| p.file_name.as_deref()),
            Some("a.pdf")
        );
    }

    #[test]
    fn body_text_and_inline_image_rules() {
        let mut p = part(Some("1"), 0, "text/plain");
        assert!(p.is_body_text());
        p.file_name = Some("notes.txt".into());
        assert!(
            !p.is_body_text(),
            "a named text part without disposition is a file"
        );
        p.disposition = Some(Disposition::Inline);
        assert!(p.is_body_text());
        p.disposition = Some(Disposition::Attachment);
        assert!(!p.is_body_text());
        assert!(!part(Some("1"), 0, "text/calendar").is_body_text());

        let mut img = part(Some("2"), 1, "image/png");
        assert!(!img.is_inline_image(), "no Content-ID");
        img.content_id = Some("a@b".into());
        assert!(img.is_inline_image());
        img.disposition = Some(Disposition::Attachment);
        assert!(!img.is_inline_image());
    }

    #[test]
    fn structure_serializes_for_storage() {
        let structure = newsletter_with_attachments(7_000_000);
        let json = serde_json::to_string(&structure).expect("serialize");
        assert!(json.contains("\"path\":\"1.1.2\""), "{json}");
        assert!(json.contains("\"disposition\":\"attachment\""), "{json}");
        let back: MessageStructure = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, structure);
    }

    #[test]
    fn raw_data_is_not_printed() {
        let raw = RawMessage {
            uid: 7,
            data: b"Subject: secret\r\n\r\nhello".to_vec(),
        };
        assert_eq!(format!("{raw:?}"), "RawMessage(uid 7, 24 bytes)");
        let section = FetchedSection {
            section: BodySection::Mime(path("1")),
            data: b"x".to_vec(),
        };
        assert_eq!(format!("{section:?}"), "FetchedSection(1.MIME, 1 bytes)");
    }
}
