//! Content lines of vCard files (RFC 2425, RFC 6350 section 3.2, vCard 2.1): unfolding,
//! `[group.]NAME;PARAM=value:VALUE`, value decoding (quoted-printable, charsets) and the TEXT
//! escapes.
//!
//! Unfolding works on bytes, before any decoding: producers fold inside multi-byte characters
//! and inside quoted-printable escapes, which decode correctly once the pieces are joined. Every
//! input byte is looked at a bounded number of times, so the work is linear in the input.

use mail_parser::Charset;

/// One logical line as bytes.
pub(super) struct RawLine {
    pub bytes: Vec<u8>,
    /// The logical line was longer than the limit; its bytes are incomplete.
    pub cut: bool,
}

/// Splits input into logical lines: `\n` or `\r\n` ends a physical line; a physical line that
/// starts with a space or tab continues the previous one (without that first character). A
/// line with `ENCODING=QUOTED-PRINTABLE` (vCard 2.1) that ends in `=` continues on the next
/// physical line as it is (a soft line break).
pub(super) struct Unfolder<'a> {
    data: &'a [u8],
    pos: usize,
    max: usize,
}

impl<'a> Unfolder<'a> {
    pub fn new(data: &'a [u8], max: usize) -> Self {
        Self { data, pos: 0, max }
    }

    fn physical(&mut self) -> &'a [u8] {
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let line = rest.get(..end).unwrap_or_default();
        self.pos = self
            .pos
            .saturating_add(end)
            .saturating_add(1)
            .min(self.data.len());
        line.strip_suffix(b"\r").unwrap_or(line)
    }
}

impl Iterator for Unfolder<'_> {
    type Item = RawLine;

    fn next(&mut self) -> Option<RawLine> {
        if self.pos >= self.data.len() {
            return None;
        }
        let mut bytes = Vec::new();
        let mut cut = false;
        // Known after the first physical line: whether the value is quoted-printable.
        let mut quoted_printable = None;
        let mut skip_first = false;
        loop {
            let mut physical = self.physical();
            if skip_first {
                physical = physical.get(1..).unwrap_or_default();
            }
            let room = self.max.saturating_sub(bytes.len());
            if physical.len() > room {
                bytes.extend_from_slice(physical.get(..room).unwrap_or_default());
                cut = true;
            } else {
                bytes.extend_from_slice(physical);
            }
            let qp = *quoted_printable.get_or_insert_with(|| is_quoted_printable(&bytes));
            let next = self.data.get(self.pos..).unwrap_or_default();
            let ends_card = next
                .get(..9)
                .is_some_and(|start| start.eq_ignore_ascii_case(b"END:VCARD"));
            if !next.is_empty() && !ends_card && qp && physical.last() == Some(&b'=') {
                // A soft line break: the next physical line continues the value as it is.
                if !cut && bytes.last() == Some(&b'=') {
                    bytes.pop();
                }
                skip_first = false;
                continue;
            }
            match self.data.get(self.pos) {
                Some(b' ' | b'\t') => skip_first = true,
                _ => break,
            }
        }
        Some(RawLine { bytes, cut })
    }
}

/// Whether the name and parameters of a line (the part before the first colon outside quotes)
/// ask for quoted-printable.
fn is_quoted_printable(line: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"QUOTED-PRINTABLE";
    let head = line.get(..head_end(line)).unwrap_or_default();
    head.windows(NEEDLE.len())
        .any(|window| window.eq_ignore_ascii_case(NEEDLE))
}

/// Index of the colon that ends name and parameters (outside double quotes), or the length.
fn head_end(line: &[u8]) -> usize {
    let mut quoted = false;
    for (i, &b) in line.iter().enumerate() {
        match b {
            b'"' => quoted = !quoted,
            b':' if !quoted => return i,
            _ => {}
        }
    }
    line.len()
}

/// A parsed content line. Names in upper case, without the group prefix (`item1.`).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Property<'a> {
    pub name: String,
    /// Parameters in order: name in upper case (`TYPE` for vCard 2.1's bare values, `ENCODING`
    /// for a bare `QUOTED-PRINTABLE` or `BASE64`), value with quotes removed.
    pub params: Vec<(String, String)>,
    pub value: &'a [u8],
}

impl Property<'_> {
    /// Values of a parameter, split at commas, in upper case.
    pub fn param_values<'s>(&'s self, name: &'s str) -> impl Iterator<Item = String> + 's {
        self.params
            .iter()
            .filter(move |(n, _)| n == name)
            .flat_map(|(_, v)| v.split(','))
            .map(|v| v.trim().to_ascii_uppercase())
            .filter(|v| !v.is_empty())
    }

    pub fn has_param_value(&self, name: &str, value: &str) -> bool {
        self.param_values(name).any(|v| v == value)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.trim())
    }

    /// `ENCODING=b` / `BASE64` (vCard 3.0, 2.1).
    pub fn is_base64(&self) -> bool {
        self.param_values("ENCODING")
            .any(|v| v == "B" || v == "BASE64")
    }

    /// The value as text: quoted-printable undone, converted from `CHARSET` (UTF-8 when it is
    /// valid UTF-8 and no charset is named, else Windows-1252). Escapes are left in place.
    pub fn text(&self) -> String {
        let decoded;
        let bytes = if self.has_param_value("ENCODING", "QUOTED-PRINTABLE") {
            decoded = quoted_printable(self.value);
            decoded.as_slice()
        } else {
            self.value
        };
        let mut out = String::with_capacity(bytes.len());
        match self.param("CHARSET").map(str::as_bytes) {
            Some(label) if !label.eq_ignore_ascii_case(b"utf-8") => {
                match Charset::from_label(label) {
                    Some(charset) => charset.decode_append(bytes, &mut out),
                    None => out.push_str(&String::from_utf8_lossy(bytes)),
                }
            }
            _ => match std::str::from_utf8(bytes) {
                Ok(text) => out.push_str(text),
                Err(_) => Charset::Windows1252.decode_append(bytes, &mut out),
            },
        }
        out
    }
}

/// Parses one logical line; `None` when it has no colon or no usable name.
pub(super) fn parse_property(line: &[u8]) -> Option<Property<'_>> {
    let end = head_end(line);
    let head = line.get(..end)?;
    let value = line.get(end.checked_add(1)?..)?;
    let mut parts = split_params(head).into_iter();
    let name = parts.next()?;
    let name = name.rsplit(|&b| b == b'.').next().unwrap_or(name);
    if name.is_empty()
        || !name
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
    {
        return None;
    }
    let name = String::from_utf8_lossy(name).to_ascii_uppercase();
    let params = parts
        .filter(|p| !p.is_empty())
        .map(|param| {
            let text = String::from_utf8_lossy(param);
            match text.split_once('=') {
                Some((name, value)) => (
                    name.trim().to_ascii_uppercase(),
                    value.trim().replace('"', ""),
                ),
                None => {
                    let value = text.trim().replace('"', "");
                    let upper = value.to_ascii_uppercase();
                    let name = match upper.as_str() {
                        "QUOTED-PRINTABLE" | "BASE64" | "B" | "8BIT" | "7BIT" => "ENCODING",
                        _ => "TYPE",
                    };
                    (name.to_owned(), value)
                }
            }
        })
        .collect();
    Some(Property {
        name,
        params,
        value,
    })
}

/// Splits name and parameters at semicolons outside double quotes.
fn split_params(head: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, &b) in head.iter().enumerate() {
        match b {
            b'"' => quoted = !quoted,
            b';' if !quoted => {
                parts.push(head.get(start..i).unwrap_or_default());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(head.get(start..).unwrap_or_default());
    parts
}

/// Undoes quoted-printable (`=XX`); a stray `=` stays as it is.
pub(super) fn quoted_printable(bytes: &[u8]) -> Vec<u8> {
    let hex = |b: u8| char::from(b).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'='
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'?'));
            i += 3;
            continue;
        }
        out.push(b);
        i += 1;
    }
    out
}

/// TEXT unescaping: `\\`, `\,`, `\;`, `\n`/`\N` (line break); any other escaped character
/// stands for itself.
pub(super) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Splits a structured value (`N`) at unescaped semicolons, unescaping each component.
pub(super) fn components(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n' | 'N') => current.push('\n'),
                Some(other) => current.push(other),
                None => current.push('\\'),
            },
            ';' => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

/// TEXT escaping for writing: backslash, comma, semicolon, line breaks as `\n`.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// Appends one content line, folded at 75 octets on character boundaries (continuation lines
/// start with one space), ended with CRLF.
pub(super) fn push_folded(out: &mut String, line: &str) {
    const MAX_OCTETS: usize = 75;
    let mut width = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if width + len > MAX_OCTETS {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
}
