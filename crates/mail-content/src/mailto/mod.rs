//! `mailto:` links (RFC 6068) as a [`ComposeRequest`] for the composer (stage 9, "default mail
//! app"). Pure: the link in, the draft fields and the hints for the user out.
//!
//! Rules, including the choices RFC 6068 leaves open:
//! - The scheme is matched in any case (`MAILTO:`); surrounding white space and control
//!   characters are trimmed; a fragment (`#…`) is ignored. Links over [`MAX_MAILTO_URI_BYTES`]
//!   are refused (the systems hand over far shorter ones; Windows command lines end at 32 767
//!   characters). Anything else is accepted: `mailto:` alone is an empty request.
//! - **Recipients** come from the path (`mailto:a@x,b@y`) and from every `to`, `cc` and `bcc`
//!   field; they accumulate. Each is a list of addr-specs or `Name <addr-spec>` (quoted names,
//!   RFC 2047 encoded words), separated by `,` or `;` (as Outlook writes them) outside quotes.
//!   Every address is checked with [`crate::normalize_address`] (dot-atom local part kept as
//!   written, domain in ASCII via IDNA); invalid ones are left out with
//!   [`MailtoNotice::AddressInvalid`]. Quoted local parts (RFC 6068 section 6.2) are decoded but
//!   the composer cannot send to them, so they are reported the same way. Duplicates (local part
//!   ignoring case, same domain) are dropped: the first one wins, To before Cc before Bcc. At most
//!   [`MAX_RECIPIENTS`] together, To first; the rest is cut with [`MailtoNotice::Truncated`].
//!   Blind copies always add [`MailtoNotice::BccPresent`] as the first notice.
//! - `subject`, `body` and `in-reply-to`: the first field with a usable value wins, later ones
//!   are reported ([`MailtoNotice::DuplicateIgnored`]). Empty fields are ignored; one that is
//!   only white space (or, for `in-reply-to`, not a valid Message-ID) gives way to the next.
//! - Every other field is ignored and reported ([`MailtoNotice::HeaderIgnored`]): a link never
//!   sets `From`, `Sender`, `Reply-To`, `Return-Path`, `Content-*`, `MIME-Version`, `X-*` or
//!   anything else. Field names are compared ignoring ASCII case, after percent-decoding.
//! - **Percent-decoding** gives bytes, read as UTF-8. Bytes that are not valid UTF-8 become
//!   U+FFFD ([`MailtoNotice::InvalidEncoding`]; an address containing one is invalid). A `%` that
//!   is not followed by two hex digits stays as written. `+` is a plus sign, not a space
//!   (RFC 6068 section 5).
//! - **Subject**: one line. Line breaks (CRLF, CR or LF each) become one space, other control
//!   characters a space ([`MailtoNotice::ControlCharactersRemoved`]; nothing in the link can
//!   start a header of its own). RFC 2047 encoded words (`=?UTF-8?B?…?=`, section 6.3 of RFC 6068)
//!   are decoded by mail-parser. Trimmed, cut at [`MAX_SUBJECT_CHARS`].
//! - **Body**: plain text, never decoded as RFC 2047. Line breaks (CRLF, lone CR, lone LF)
//!   become `\n`; other control characters except tabs are removed; cut at
//!   [`MAX_MAILTO_BODY_BYTES`] on a character boundary.
//! - `in-reply-to`: one Message-ID, angle brackets optional; returned without them.
//! - Hints that carry a value (ignored fields, invalid addresses and Message-IDs) are listed at
//!   most [`MAX_MAILTO_VALUE_NOTICES`] times, then counted ([`MailtoNotice::NoticesOmitted`]); each
//!   value is cut at [`MAX_MAILTO_NOTICE_VALUE_CHARS`] characters. Equal hints are listed once.

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fmt;

use mail_model::{
    Address, ComposeRequest, CoreError, MAX_RECIPIENTS, MAX_SUBJECT_CHARS, MailtoField,
    MailtoNotice,
};
use mail_parser::HeaderForm;

use crate::{clean_display_name, normalize_address};

/// Longest `mailto:` link accepted, in bytes.
pub const MAX_MAILTO_URI_BYTES: usize = 64 * 1024;
/// Longest body taken from a link, in bytes (UTF-8); longer bodies are cut.
pub const MAX_MAILTO_BODY_BYTES: usize = 32 * 1024;
/// Most hints that carry a value (see module docs).
pub const MAX_MAILTO_VALUE_NOTICES: usize = 16;
/// Longest value in a hint, in characters.
pub const MAX_MAILTO_NOTICE_VALUE_CHARS: usize = 256;
/// Longest Message-ID taken from `in-reply-to`, in bytes.
const MAX_MESSAGE_ID_BYTES: usize = 250;

/// Why a link is not a `mailto:` request at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailtoError {
    /// The link does not start with `mailto:`.
    NotMailto,
    /// The link is longer than [`MAX_MAILTO_URI_BYTES`].
    TooLong { bytes: usize, max: usize },
}

impl fmt::Display for MailtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMailto => f.write_str("not a mailto: link"),
            Self::TooLong { bytes, max } => {
                write!(f, "mailto: link too long: {bytes} bytes, at most {max}")
            }
        }
    }
}

impl std::error::Error for MailtoError {}

/// The link itself never goes into the error (it may carry personal text).
impl From<MailtoError> for CoreError {
    fn from(error: MailtoError) -> Self {
        CoreError::InvalidArgument(error.to_string())
    }
}

/// Parses a `mailto:` link (see module docs). Fails only for links that are not `mailto:` or are
/// too long; everything inside a link that cannot be used is left out and reported in
/// [`ComposeRequest::notices`].
pub fn parse_mailto(uri: &str) -> Result<ComposeRequest, MailtoError> {
    if uri.len() > MAX_MAILTO_URI_BYTES {
        return Err(MailtoError::TooLong {
            bytes: uri.len(),
            max: MAX_MAILTO_URI_BYTES,
        });
    }
    let uri = uri.trim_matches(|c: char| c.is_whitespace() || c.is_control());
    let rest = match uri.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("mailto:") => &uri[7..],
        _ => return Err(MailtoError::NotMailto),
    };
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));

    let mut fields = Fields::default();
    fields.to.push(path);
    let mut notices = Notices::default();
    for hfield in query.split('&') {
        let (name, value) = hfield.split_once('=').unwrap_or((hfield, ""));
        let (name, _) = percent_decode(name);
        if name.is_empty() {
            continue;
        }
        let known = |known: &str| name.eq_ignore_ascii_case(known);
        if known("to") {
            fields.to.push(value);
        } else if known("cc") {
            fields.cc.push(value);
        } else if known("bcc") {
            fields.bcc.push(value);
        } else if known("subject") {
            fields.subject.push(value);
        } else if known("body") {
            fields.body.push(value);
        } else if known("in-reply-to") {
            fields.in_reply_to.push(value);
        } else {
            let name = notice_value(&name.to_lowercase());
            notices.push_value(MailtoNotice::HeaderIgnored { name });
        }
    }

    let mut recipients = Recipients::default();
    let to = recipients.collect(&fields.to, &mut notices);
    let cc = recipients.collect(&fields.cc, &mut notices);
    let bcc = recipients.collect(&fields.bcc, &mut notices);
    let [to, cc, bcc] = limit_recipients(
        [
            (MailtoField::To, to),
            (MailtoField::Cc, cc),
            (MailtoField::Bcc, bcc),
        ],
        &mut notices,
    );

    let subject = first(&fields.subject, MailtoField::Subject, &mut notices, subject);
    let body = first(&fields.body, MailtoField::Body, &mut notices, body);
    let in_reply_to = first(
        &fields.in_reply_to,
        MailtoField::InReplyTo,
        &mut notices,
        message_id,
    );

    Ok(ComposeRequest {
        notices: notices.finish(!bcc.is_empty()),
        to,
        cc,
        bcc,
        subject,
        body,
        in_reply_to,
    })
}

/// The raw (still percent-encoded) values of the fields in the order of the link; empty values
/// are kept here and skipped later.
#[derive(Default)]
struct Fields<'a> {
    to: Vec<&'a str>,
    cc: Vec<&'a str>,
    bcc: Vec<&'a str>,
    subject: Vec<&'a str>,
    body: Vec<&'a str>,
    in_reply_to: Vec<&'a str>,
}

/// The hints collected while parsing (see module docs for the limits).
#[derive(Default)]
struct Notices {
    list: Vec<MailtoNotice>,
    seen: HashSet<MailtoNotice>,
    values: usize,
    omitted: u32,
}

impl Notices {
    /// A hint without a value: always listed (there are only a few of them).
    fn push(&mut self, notice: MailtoNotice) {
        if self.seen.insert(notice.clone()) {
            self.list.push(notice);
        }
    }

    /// A hint with a value from the link: listed up to [`MAX_MAILTO_VALUE_NOTICES`] times.
    fn push_value(&mut self, notice: MailtoNotice) {
        if self.values >= MAX_MAILTO_VALUE_NOTICES {
            if self.seen.insert(notice) {
                self.omitted = self.omitted.saturating_add(1);
            }
        } else if self.seen.insert(notice.clone()) {
            self.values += 1;
            self.list.push(notice);
        }
    }

    fn finish(self, bcc_present: bool) -> Vec<MailtoNotice> {
        let mut out = Vec::with_capacity(self.list.len() + 2);
        if bcc_present {
            out.push(MailtoNotice::BccPresent);
        }
        out.extend(self.list);
        if self.omitted > 0 {
            out.push(MailtoNotice::NoticesOmitted {
                count: self.omitted,
            });
        }
        out
    }
}

/// The address keys taken so far, across To, Cc and Bcc.
#[derive(Default)]
struct Recipients {
    keys: HashSet<String>,
}

impl Recipients {
    /// The valid, new addresses of all `values` (raw address lists of one field), in order.
    fn collect(&mut self, values: &[&str], notices: &mut Notices) -> Vec<Address> {
        let mut out = Vec::new();
        for value in values {
            let (list, _) = percent_decode(value);
            for item in split_address_list(&list) {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                match parse_address(item) {
                    Some((key, address)) => {
                        if self.keys.insert(key) {
                            out.push(address);
                        }
                    }
                    None => notices.push_value(MailtoNotice::AddressInvalid {
                        value: notice_value(item),
                    }),
                }
            }
        }
        out
    }
}

/// Keeps at most [`MAX_RECIPIENTS`] recipients, the fields in the given order.
fn limit_recipients(
    fields: [(MailtoField, Vec<Address>); 3],
    notices: &mut Notices,
) -> [Vec<Address>; 3] {
    let mut room = MAX_RECIPIENTS;
    fields.map(|(field, mut addresses)| {
        if addresses.len() > room {
            addresses.truncate(room);
            notices.push(MailtoNotice::Truncated { field });
        }
        room -= addresses.len();
        addresses
    })
}

/// The first value of a single field that gives a result; later non-empty values are reported.
fn first<T>(
    values: &[&str],
    field: MailtoField,
    notices: &mut Notices,
    parse: fn(&str, &mut Notices) -> Option<T>,
) -> Option<T> {
    let mut result = None;
    for value in values.iter().filter(|v| !v.is_empty()) {
        if result.is_some() {
            notices.push(MailtoNotice::DuplicateIgnored { field });
        } else {
            result = parse(value, notices);
        }
    }
    result
}

/// The subject: decoded, one line, RFC 2047 words decoded, trimmed and cut.
fn subject(raw: &str, notices: &mut Notices) -> Option<String> {
    let (decoded, lossy) = percent_decode(raw);
    if lossy {
        notices.push(MailtoNotice::InvalidEncoding {
            field: MailtoField::Subject,
        });
    }
    let (line, replaced) = single_line(&decoded);
    let (line, replaced_in_words) = single_line(&decode_encoded_words(&line));
    if replaced || replaced_in_words {
        notices.push(MailtoNotice::ControlCharactersRemoved {
            field: MailtoField::Subject,
        });
    }
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    if line.chars().count() > MAX_SUBJECT_CHARS {
        notices.push(MailtoNotice::Truncated {
            field: MailtoField::Subject,
        });
        return Some(
            line.chars()
                .take(MAX_SUBJECT_CHARS)
                .collect::<String>()
                .trim_end()
                .to_owned(),
        );
    }
    Some(line.to_owned())
}

/// The body: decoded, line breaks as `\n`, control characters other than tabs removed, cut.
fn body(raw: &str, notices: &mut Notices) -> Option<String> {
    let (decoded, lossy) = percent_decode(raw);
    if lossy {
        notices.push(MailtoNotice::InvalidEncoding {
            field: MailtoField::Body,
        });
    }
    let mut text = String::with_capacity(decoded.len());
    let mut removed = false;
    let mut chars = decoded.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                text.push('\n');
            }
            '\n' | '\t' => text.push(c),
            c if c.is_control() => removed = true,
            c => text.push(c),
        }
    }
    if removed {
        notices.push(MailtoNotice::ControlCharactersRemoved {
            field: MailtoField::Body,
        });
    }
    if text.trim().is_empty() {
        return None;
    }
    if text.len() > MAX_MAILTO_BODY_BYTES {
        notices.push(MailtoNotice::Truncated {
            field: MailtoField::Body,
        });
        let mut end = MAX_MAILTO_BODY_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Some(text)
}

/// One Message-ID, angle brackets optional, returned without them.
fn message_id(raw: &str, notices: &mut Notices) -> Option<String> {
    let (decoded, _) = percent_decode(raw);
    let trimmed = decoded.trim();
    let id = trimmed
        .strip_prefix('<')
        .and_then(|id| id.strip_suffix('>'))
        .unwrap_or(trimmed);
    let valid = id.len() <= MAX_MESSAGE_ID_BYTES
        && id
            .rsplit_once('@')
            .is_some_and(|(left, right)| !left.is_empty() && !right.is_empty())
        && id
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'<' && b != b'>');
    if valid {
        return Some(id.to_owned());
    }
    if !trimmed.is_empty() {
        notices.push_value(MailtoNotice::MessageIdInvalid {
            value: notice_value(trimmed),
        });
    }
    None
}

/// One address of a list: `addr-spec` or `Name <addr-spec>`. Returns its key (for duplicates)
/// and the address for the draft, or `None` if it is not a usable address.
fn parse_address(item: &str) -> Option<(String, Address)> {
    if item.contains('\u{FFFD}') {
        return None;
    }
    let (name, email) = match item.strip_suffix('>').and_then(|i| i.rsplit_once('<')) {
        Some((name, email)) => (Some(name.trim()), email.trim()),
        None => (None, item),
    };
    let normalized = normalize_address(email)?;
    let email = normalized.email();
    let name = name
        .filter(|n| !n.is_empty())
        .and_then(|n| clean_display_name(&decode_encoded_words(&unquote(n)), &email));
    Some((normalized.key, Address { name, email }))
}

/// A display name without its surrounding quotes and with `\x` escapes resolved. Line breaks
/// and control characters become spaces (so the RFC 2047 decoder sees one line).
fn unquote(name: &str) -> String {
    let (name, _) = single_line(name);
    let Some(inner) = name.strip_prefix('"').and_then(|n| n.strip_suffix('"')) else {
        return name;
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

/// Splits an address list at `,` and `;` outside quoted strings and angle brackets.
fn split_address_list(list: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    let mut angle = false;
    for (i, c) in list.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                items.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&list[start..]);
    items
}

/// RFC 2047 encoded words of a single line decoded (mail-parser, all its charsets); text
/// without `=?` is returned as it is.
fn decode_encoded_words(text: &str) -> String {
    if !text.contains("=?") {
        return text.to_owned();
    }
    let parsed = HeaderForm::Text.parse(text.as_bytes());
    parsed
        .value()
        .as_text()
        .map_or_else(|| text.to_owned(), str::to_owned)
}

/// The text on one line: each line break (CRLF, CR, LF) and every other control character
/// becomes one space. Also tells whether anything other than a tab was replaced.
fn single_line(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut replaced = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            chars.next_if_eq(&'\n');
        }
        if c.is_control() {
            replaced |= c != '\t';
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    (out, replaced)
}

/// A value from the link for a hint: on one line, cut at [`MAX_MAILTO_NOTICE_VALUE_CHARS`].
fn notice_value(text: &str) -> String {
    let (line, _) = single_line(text);
    line.trim()
        .chars()
        .take(MAX_MAILTO_NOTICE_VALUE_CHARS)
        .collect()
}

/// Percent-decoding (RFC 3986) as UTF-8. Returns the text and whether invalid UTF-8 was
/// replaced with U+FFFD. A `%` without two hex digits after it stays as written.
fn percent_decode(text: &str) -> (String, bool) {
    if !text.contains('%') {
        return (text.to_owned(), false);
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).and_then(|b| hex_value(*b)),
                bytes.get(i + 2).and_then(|b| hex_value(*b)),
            )
        {
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(text) => (text, false),
        Err(error) => (String::from_utf8_lossy(error.as_bytes()).into_owned(), true),
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}
