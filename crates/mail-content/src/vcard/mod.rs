//! vCard files (stage 19): a reader for vCard 2.1, 3.0 (RFC 2426) and 4.0 (RFC 6350) and a
//! writer for 3.0 and 4.0, for importing and exporting contacts and contact groups. Pure: no IO,
//! no network (a photo given as a URL is never fetched), no randomness.
//!
//! **What is read.** Per card: `UID`, `FN` (else given and family name from `N`), every `EMAIL`
//! (preferred first: `PREF=1` in 4.0, `TYPE=pref` or a bare `PREF` in 3.0 and 2.1), `NOTE`, an
//! inline `PHOTO` (4.0 `data:` URI, 3.0/2.1 `ENCODING=b`/`BASE64`; only PNG, JPEG, GIF and WebP
//! by their bytes: the engine encodes it again in its helper process, decision 0046), and for
//! groups (`KIND:group` in 4.0, Apple's `X-ADDRESSBOOKSERVER-KIND:group` in 3.0) their members
//! (`MEMBER` / `X-ADDRESSBOOKSERVER-MEMBER` as `mailto:` or `urn:uuid:`). Everything else is
//! left out. Cards without a name, an address and members are dropped.
//!
//! **Encodings.** Lines end in CRLF or LF; folded lines (a line break followed by a space or tab)
//! are joined on bytes, and so are vCard 2.1's quoted-printable soft line breaks. Values are
//! converted from their `CHARSET`; without one, bytes that are not UTF-8 are read as
//! Windows-1252. Property groups (`item1.EMAIL`) and vCard 2.1's bare parameters
//! (`EMAIL;INTERNET;PREF:`) are understood. A byte order mark is skipped.
//!
//! **Never panics and never fails**: what cannot be read is left out and reported as a
//! [`VcardProblem`] (at most 32); a `catch_unwind` guard turns a bug into
//! [`VcardProblemKind::Internal`]. Hard limits keep hostile files cheap:
//! [`MAX_VCARD_FILE_BYTES`] of input (cut at the last line break before it),
//! [`MAX_VCARD_CARDS`] cards, [`MAX_VCARD_LINE_BYTES`] per unfolded line,
//! [`MAX_VCARD_EMAILS`] addresses and [`MAX_VCARD_MEMBERS`] members per card,
//! [`MAX_VCARD_PHOTO_BYTES`] per photo. Nested cards (2.1's `AGENT`) are skipped. Work is
//! linear in the input.
//!
//! **Writing** ([`write_vcards`]): CRLF, lines folded at 75 octets on character boundaries, TEXT
//! escaped; 4.0 with `KIND:group` and `MEMBER`, 3.0 with Apple's `X-ADDRESSBOOKSERVER-*`
//! properties, which macOS Contacts and most other programs read.

mod lines;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};

use lines::{Property, Unfolder, components, escape, parse_property, push_folded, unescape};

/// Largest input read; a larger file is cut at the last line break before this.
pub const MAX_VCARD_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Cards read from one file; more are ignored ([`VcardProblemKind::TooManyCards`]).
pub const MAX_VCARD_CARDS: usize = 10_000;
/// Addresses kept per card (preferred ones first).
pub const MAX_VCARD_EMAILS: usize = 20;
/// Members kept per group card.
pub const MAX_VCARD_MEMBERS: usize = 1_000;
/// Largest decoded inline photo; a larger one is dropped ([`VcardProblemKind::PhotoTooLarge`]).
pub const MAX_VCARD_PHOTO_BYTES: usize = 4 * 1024 * 1024;
/// Largest unfolded content line (a base64 photo of [`MAX_VCARD_PHOTO_BYTES`] fits); a longer
/// one is skipped ([`VcardProblemKind::Malformed`]).
pub const MAX_VCARD_LINE_BYTES: usize = 6 * 1024 * 1024;

/// Problems reported per file.
const MAX_PROBLEMS: usize = 32;
/// Address candidates looked at per card before sorting (bounds hostile cards).
const MAX_EMAIL_CANDIDATES: usize = 1_000;
/// Longest UID kept (in characters).
const MAX_UID_CHARS: usize = 256;

/// The vCard version [`write_vcards`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcardVersion {
    /// vCard 3.0 (RFC 2426), with Apple's `X-ADDRESSBOOKSERVER-*` group properties.
    V3,
    /// vCard 4.0 (RFC 6350).
    V4,
}

/// What a card describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VcardKind {
    /// A person or organisation (`KIND:individual`, or no `KIND`).
    #[default]
    Individual,
    /// A contact group (`KIND:group`, `X-ADDRESSBOOKSERVER-KIND:group`).
    Group,
}

/// A member of a group card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcardMember {
    /// `mailto:` an address.
    Email(String),
    /// `urn:uuid:` (without the prefix): the card in the same file with this `UID`.
    Uid(String),
}

/// One card as read, or to be written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VcardCard {
    /// `UID` without a `urn:uuid:` prefix.
    pub uid: Option<String>,
    pub kind: VcardKind,
    /// `FN`, else given and family name from `N`: on one line, trimmed, without bidirectional
    /// controls, at most [`crate::MAX_NAME_CHARS`] characters; `None` when empty.
    pub formatted_name: Option<String>,
    /// Addresses (with `@`, without `mailto:`), each once ignoring case, preferred first, at most
    /// [`MAX_VCARD_EMAILS`].
    pub emails: Vec<String>,
    /// `NOTE`, without bidirectional controls and control characters other than line breaks and
    /// tabs, at most [`mail_model::MAX_CONTACT_NOTE_CHARS`] characters.
    pub note: Option<String>,
    /// Inline photo bytes (PNG, JPEG, GIF or WebP by their bytes), at most
    /// [`MAX_VCARD_PHOTO_BYTES`].
    pub photo: Option<Vec<u8>>,
    /// Members of a group card, each once, at most [`MAX_VCARD_MEMBERS`].
    pub members: Vec<VcardMember>,
}

/// Why something in a file was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcardProblemKind {
    /// The file is larger than [`MAX_VCARD_FILE_BYTES`]; the rest was not read.
    TooLarge,
    /// The file has more than [`MAX_VCARD_CARDS`] cards; the rest was not read.
    TooManyCards,
    /// A line over [`MAX_VCARD_LINE_BYTES`], or a card without `END:VCARD`.
    Malformed,
    /// A photo over [`MAX_VCARD_PHOTO_BYTES`].
    PhotoTooLarge,
    /// A photo given as a URL, not decodable, or not a PNG, JPEG, GIF or WebP image.
    PhotoUnsupported,
    /// A bug in the reader (caught); nothing was read.
    Internal,
}

/// A problem and the card it was found in (0-based index of the card in the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcardProblem {
    pub card: usize,
    pub kind: VcardProblemKind,
}

/// What [`read_vcards`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VcardFile {
    pub cards: Vec<VcardCard>,
    /// At most 32.
    pub problems: Vec<VcardProblem>,
}

/// Reads the cards of a vCard file. Never panics and never fails; see the module documentation
/// for what is read and the limits.
pub fn read_vcards(bytes: &[u8]) -> VcardFile {
    panic::catch_unwind(AssertUnwindSafe(|| read(bytes))).unwrap_or_else(|_| VcardFile {
        cards: Vec::new(),
        problems: vec![VcardProblem {
            card: 0,
            kind: VcardProblemKind::Internal,
        }],
    })
}

#[derive(Default)]
struct Reader {
    cards: Vec<VcardCard>,
    problems: Vec<VcardProblem>,
    /// Top-level cards begun so far.
    begun: usize,
}

impl Reader {
    fn problem(&mut self, card: usize, kind: VcardProblemKind) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(VcardProblem { card, kind });
        }
    }
}

fn read(bytes: &[u8]) -> VcardFile {
    let mut reader = Reader::default();
    let mut data = bytes;
    let mut too_large = false;
    if data.len() > MAX_VCARD_FILE_BYTES {
        let head = data.get(..MAX_VCARD_FILE_BYTES).unwrap_or_default();
        let end = head.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        data = head.get(..end).unwrap_or_default();
        too_large = true;
    }
    data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);

    // Depth 0: between cards; 1: in a card; more: in a nested card (skipped).
    let mut depth = 0usize;
    let mut card: Option<CardBuilder> = None;
    for line in Unfolder::new(data, MAX_VCARD_LINE_BYTES) {
        let index = reader.begun.saturating_sub(1);
        if line.cut {
            if depth > 0 {
                reader.problem(index, VcardProblemKind::Malformed);
            }
            continue;
        }
        let Some(property) = parse_property(&line.bytes) else {
            continue;
        };
        match property.name.as_str() {
            "BEGIN" if is_vcard(&property) => {
                if depth > 0 {
                    depth += 1;
                    continue;
                }
                if reader.begun == MAX_VCARD_CARDS {
                    reader.problem(MAX_VCARD_CARDS, VcardProblemKind::TooManyCards);
                    break;
                }
                reader.begun += 1;
                depth = 1;
                card = Some(CardBuilder::default());
            }
            "END" if is_vcard(&property) => match depth {
                0 => {}
                1 => {
                    depth = 0;
                    if let Some(builder) = card.take() {
                        builder.finish(&mut reader);
                    }
                }
                _ => depth -= 1,
            },
            _ if depth == 1 => {
                if let Some(builder) = card.as_mut() {
                    builder.property(&property, index, &mut reader);
                }
            }
            _ => {}
        }
    }
    if let Some(builder) = card.take() {
        let index = reader.begun.saturating_sub(1);
        reader.problem(index, VcardProblemKind::Malformed);
        builder.finish(&mut reader);
    }
    if too_large {
        let index = reader.begun;
        reader.problem(index, VcardProblemKind::TooLarge);
    }
    VcardFile {
        cards: reader.cards,
        problems: reader.problems,
    }
}

fn is_vcard(property: &Property<'_>) -> bool {
    property.text().trim().eq_ignore_ascii_case("VCARD")
}

/// The properties of one card while it is read.
#[derive(Default)]
struct CardBuilder {
    uid: Option<String>,
    kind: VcardKind,
    formatted_name: Option<String>,
    /// Given and family name from `N`, for a card without `FN`.
    structured_name: Option<String>,
    /// (preference rank, address) in file order.
    emails: Vec<(u32, String)>,
    note: Option<String>,
    photo: Option<Vec<u8>>,
    members: Vec<VcardMember>,
    member_keys: HashSet<(bool, String)>,
}

impl CardBuilder {
    fn property(&mut self, property: &Property<'_>, index: usize, reader: &mut Reader) {
        match property.name.as_str() {
            "FN" if self.formatted_name.is_none() => {
                self.formatted_name = clean_name(&unescape(&property.text()));
            }
            "N" if self.structured_name.is_none() => {
                let parts = components(&property.text());
                let family = parts.first().map(String::as_str).unwrap_or_default();
                let given = parts.get(1).map(String::as_str).unwrap_or_default();
                self.structured_name = clean_name(&format!("{given} {family}"));
            }
            "EMAIL" if self.emails.len() < MAX_EMAIL_CANDIDATES => {
                if let Some(email) = clean_email(&unescape(&property.text())) {
                    self.emails.push((preference(property), email));
                }
            }
            "NOTE" if self.note.is_none() => self.note = clean_note(&unescape(&property.text())),
            "UID" if self.uid.is_none() => self.uid = clean_uid(&unescape(&property.text())),
            "KIND" | "X-ADDRESSBOOKSERVER-KIND" => {
                if unescape(&property.text())
                    .trim()
                    .eq_ignore_ascii_case("group")
                {
                    self.kind = VcardKind::Group;
                }
            }
            "MEMBER" | "X-ADDRESSBOOKSERVER-MEMBER" if self.members.len() < MAX_VCARD_MEMBERS => {
                if let Some(member) = member(&unescape(&property.text())) {
                    let key = match &member {
                        VcardMember::Email(email) => (true, email.to_lowercase()),
                        VcardMember::Uid(uid) => (false, uid.clone()),
                    };
                    if self.member_keys.insert(key) {
                        self.members.push(member);
                    }
                }
            }
            "PHOTO" if self.photo.is_none() => match photo(property) {
                Ok(bytes) => self.photo = Some(bytes),
                Err(kind) => reader.problem(index, kind),
            },
            _ => {}
        }
    }

    fn finish(self, reader: &mut Reader) {
        let mut ranked = self.emails;
        // Stable: equally preferred addresses keep their order in the file.
        ranked.sort_by_key(|(rank, _)| *rank);
        let mut seen = HashSet::new();
        let mut emails = Vec::new();
        for (_, email) in ranked {
            if emails.len() == MAX_VCARD_EMAILS {
                break;
            }
            if seen.insert(email.to_lowercase()) {
                emails.push(email);
            }
        }
        let card = VcardCard {
            uid: self.uid,
            kind: self.kind,
            formatted_name: self.formatted_name.or(self.structured_name),
            emails,
            note: self.note,
            photo: self.photo,
            members: self.members,
        };
        if card.formatted_name.is_some() || !card.emails.is_empty() || !card.members.is_empty() {
            reader.cards.push(card);
        }
    }
}

/// Preference of an address: 1 for `TYPE=pref`/bare `PREF` (3.0, 2.1), the value of `PREF=n`
/// (4.0, 1 is most preferred), `u32::MAX` without.
fn preference(property: &Property<'_>) -> u32 {
    if let Some(value) = property.param("PREF")
        && let Ok(rank) = value.parse::<u32>()
    {
        return rank.max(1);
    }
    if property.has_param_value("TYPE", "PREF") || property.param("PREF").is_some() {
        return 1;
    }
    u32::MAX
}

/// A name on one line: bidirectional controls dropped, control characters as spaces, white space
/// collapsed, at most [`crate::MAX_NAME_CHARS`] characters.
fn clean_name(raw: &str) -> Option<String> {
    let mut name = String::new();
    for word in raw
        .chars()
        .filter(|c| !mail_model::is_bidi_control(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
    {
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
    }
    let name: String = name.chars().take(crate::MAX_NAME_CHARS).collect();
    let name = name.trim_end();
    (!name.is_empty()).then(|| name.to_owned())
}

fn clean_email(raw: &str) -> Option<String> {
    let email = raw.trim();
    let email = strip_prefix_ignore_case(email, "mailto:")
        .unwrap_or(email)
        .trim();
    let usable = email.contains('@')
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || mail_model::is_bidi_control(c));
    usable.then(|| email.to_owned())
}

fn clean_note(raw: &str) -> Option<String> {
    let note: String = raw
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| !mail_model::is_bidi_control(*c))
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(mail_model::MAX_CONTACT_NOTE_CHARS)
        .collect();
    let note = note.trim();
    (!note.is_empty()).then(|| note.to_owned())
}

fn clean_uid(raw: &str) -> Option<String> {
    let uid = raw.trim();
    let uid = strip_prefix_ignore_case(uid, "urn:uuid:")
        .unwrap_or(uid)
        .trim();
    let uid: String = uid
        .chars()
        .filter(|c| !c.is_control() && !mail_model::is_bidi_control(*c))
        .take(MAX_UID_CHARS)
        .collect();
    (!uid.is_empty()).then_some(uid)
}

fn member(raw: &str) -> Option<VcardMember> {
    let value = raw.trim();
    if let Some(email) = strip_prefix_ignore_case(value, "mailto:") {
        return clean_email(email).map(VcardMember::Email);
    }
    if strip_prefix_ignore_case(value, "urn:uuid:").is_none() && value.contains('@') {
        return clean_email(value).map(VcardMember::Email);
    }
    clean_uid(value).map(VcardMember::Uid)
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .and_then(|_| text.get(prefix.len()..))
}

/// The bytes of an inline photo.
fn photo(property: &Property<'_>) -> Result<Vec<u8>, VcardProblemKind> {
    // Base64 needs 4 characters for 3 bytes; anything far larger is refused before decoding.
    let too_long =
        |payload: &str| payload.len() > MAX_VCARD_PHOTO_BYTES.saturating_mul(4) / 3 + 1024;
    let text = property.text();
    let bytes = if property.is_base64() {
        let payload: String = text.split_whitespace().collect();
        if too_long(&payload) {
            return Err(VcardProblemKind::PhotoTooLarge);
        }
        crate::image::base64_decode(&payload).ok_or(VcardProblemKind::PhotoUnsupported)?
    } else {
        let uri = text.trim();
        if strip_prefix_ignore_case(uri, "data:").is_none() {
            // A URL (or anything else): never fetched.
            return Err(VcardProblemKind::PhotoUnsupported);
        }
        let payload = uri.split_once(',').map_or("", |(_, payload)| payload);
        if too_long(payload) {
            return Err(VcardProblemKind::PhotoTooLarge);
        }
        crate::image::decode_data_url(uri, usize::MAX).ok_or(VcardProblemKind::PhotoUnsupported)?
    };
    if bytes.len() > MAX_VCARD_PHOTO_BYTES {
        return Err(VcardProblemKind::PhotoTooLarge);
    }
    if crate::sniff_image(&bytes).is_none() {
        return Err(VcardProblemKind::PhotoUnsupported);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------------------------
// Writing

/// Writes the cards as one vCard file (CRLF, folded at 75 octets, TEXT escaped). Every card gets
/// an `FN`: its name, else its first address, else its UID, else `Unnamed`.
pub fn write_vcards(cards: &[VcardCard], version: VcardVersion) -> String {
    let mut out = String::new();
    for card in cards {
        write_card(&mut out, card, version);
    }
    out
}

fn write_card(out: &mut String, card: &VcardCard, version: VcardVersion) {
    let v4 = version == VcardVersion::V4;
    let group = card.kind == VcardKind::Group;
    push_folded(out, "BEGIN:VCARD");
    push_folded(out, if v4 { "VERSION:4.0" } else { "VERSION:3.0" });
    if group {
        push_folded(
            out,
            if v4 {
                "KIND:group"
            } else {
                "X-ADDRESSBOOKSERVER-KIND:group"
            },
        );
    }
    if let Some(uid) = card.uid.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        if v4 && looks_like_uuid(uid) {
            push_folded(out, &format!("UID:urn:uuid:{uid}"));
        } else {
            push_folded(out, &format!("UID:{}", escape(uid)));
        }
    }
    let name = card
        .formatted_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let shown = name
        .or_else(|| card.emails.first().map(String::as_str))
        .or(card.uid.as_deref())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("Unnamed");
    push_folded(out, &format!("FN:{}", escape(shown)));
    if group {
        if !v4 {
            push_folded(out, &format!("N:{};;;;", escape(shown)));
        }
    } else {
        let person = name.map(crate::split_name).unwrap_or_default();
        push_folded(
            out,
            &format!(
                "N:{};{};;;",
                escape(person.last.as_deref().unwrap_or_default()),
                escape(person.first.as_deref().unwrap_or_default())
            ),
        );
    }
    for (i, email) in card.emails.iter().enumerate() {
        let params = match (v4, i) {
            (true, 0) => ";PREF=1",
            (true, _) => "",
            (false, 0) => ";TYPE=INTERNET,pref",
            (false, _) => ";TYPE=INTERNET",
        };
        push_folded(out, &format!("EMAIL{params}:{}", escape(email)));
    }
    if let Some(note) = card.note.as_deref().filter(|n| !n.trim().is_empty()) {
        push_folded(out, &format!("NOTE:{}", escape(note)));
    }
    if let Some(photo) = card.photo.as_deref().filter(|p| !p.is_empty()) {
        let kind = crate::sniff_image(photo).unwrap_or(crate::ImageType::Jpeg);
        let mut line = String::new();
        if v4 {
            line.push_str("PHOTO:data:");
            line.push_str(kind.mime());
            line.push_str(";base64,");
        } else {
            let subtype = kind
                .mime()
                .trim_start_matches("image/")
                .to_ascii_uppercase();
            line.push_str("PHOTO;ENCODING=b;TYPE=");
            line.push_str(&subtype);
            line.push(':');
        }
        crate::image::base64_encode_into(photo, &mut line);
        push_folded(out, &line);
    }
    let member_property = if v4 {
        "MEMBER"
    } else {
        "X-ADDRESSBOOKSERVER-MEMBER"
    };
    for member in &card.members {
        let value = match member {
            VcardMember::Email(email) => format!("mailto:{email}"),
            VcardMember::Uid(uid) => format!("urn:uuid:{uid}"),
        };
        push_folded(out, &format!("{member_property}:{value}"));
    }
    push_folded(out, "END:VCARD");
}

/// `8-4-4-4-12` hexadecimal digits.
fn looks_like_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}
