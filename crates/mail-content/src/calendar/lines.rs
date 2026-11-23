//! Content lines (RFC 5545 section 3.1): unfolding, `NAME;PARAM=value:VALUE`, TEXT escapes.
//!
//! Unfolding works on bytes, before UTF-8 decoding: some producers fold inside a multi-byte
//! character, which decodes correctly once the pieces are joined.

/// One logical line as bytes.
pub(super) struct RawLine {
    pub bytes: Vec<u8>,
    /// The logical line was longer than the limit and was cut.
    pub cut: bool,
}

/// Splits input into logical lines: `\n` or `\r\n` ends a physical line; a physical line that
/// starts with a space or tab continues the previous one (without that first character).
pub(super) struct Unfolder<'a> {
    data: &'a [u8],
    pos: usize,
    max: usize,
}

impl<'a> Unfolder<'a> {
    pub fn new(data: &'a [u8], max: usize) -> Self {
        Self { data, pos: 0, max }
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
        let mut first = true;
        loop {
            let rest = self.data.get(self.pos..).unwrap_or_default();
            let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
            let mut physical = rest.get(..end).unwrap_or_default();
            physical = physical.strip_suffix(b"\r").unwrap_or(physical);
            if !first {
                physical = physical.get(1..).unwrap_or_default();
            }
            let room = self.max.saturating_sub(bytes.len());
            if physical.len() > room {
                bytes.extend_from_slice(physical.get(..room).unwrap_or_default());
                cut = true;
            } else {
                bytes.extend_from_slice(physical);
            }
            self.pos = self
                .pos
                .saturating_add(end)
                .saturating_add(1)
                .min(self.data.len());
            match self.data.get(self.pos) {
                Some(b' ' | b'\t') => first = false,
                _ => break,
            }
        }
        if cut {
            // Do not leave half a character at the cut.
            if let Err(error) = std::str::from_utf8(&bytes)
                && error.error_len().is_none()
            {
                bytes.truncate(error.valid_up_to());
            }
        }
        Some(RawLine { bytes, cut })
    }
}

/// A parsed content line. Names in upper case.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Line<'a> {
    pub name: String,
    /// Parameters in order: name in upper case, value with quotes removed and RFC 6868 caret
    /// escapes decoded; several values joined with `,`.
    pub params: Vec<(String, String)>,
    pub value: &'a str,
}

impl Line<'_> {
    /// The first parameter of that name (upper case).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Parameters kept per line; more are ignored.
const MAX_PARAMS: usize = 32;

/// Parses `name *(";" param) ":" value`. `None` for a line that does not have that form.
pub(super) fn parse_line(text: &str) -> Option<Line<'_>> {
    let (name, mut rest) = take_name(text);
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    loop {
        let mut chars = rest.chars();
        match chars.next()? {
            ':' => {
                return Some(Line {
                    name: name.to_ascii_uppercase(),
                    params,
                    value: chars.as_str(),
                });
            }
            ';' => {
                let (param, after) = take_name(chars.as_str());
                if param.is_empty() {
                    return None;
                }
                rest = after;
                let mut values: Vec<String> = Vec::new();
                if let Some(after_eq) = rest.strip_prefix('=') {
                    rest = after_eq;
                    loop {
                        let (value, after) = take_param_value(rest)?;
                        values.push(decode_caret(value));
                        rest = after;
                        match rest.strip_prefix(',') {
                            Some(next) => rest = next,
                            None => break,
                        }
                    }
                }
                if params.len() < MAX_PARAMS {
                    params.push((param.to_ascii_uppercase(), values.join(",")));
                }
            }
            _ => return None,
        }
    }
}

/// An iana-token or x-name: letters, digits and `-`.
fn take_name(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(text.len());
    (
        text.get(..end).unwrap_or_default(),
        text.get(end..).unwrap_or_default(),
    )
}

/// A quoted or unquoted parameter value and the rest after it.
fn take_param_value(text: &str) -> Option<(&str, &str)> {
    if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"')?;
        return Some((quoted.get(..end)?, quoted.get(end + 1..)?));
    }
    let end = text.find([',', ';', ':', '"']).unwrap_or(text.len());
    Some((text.get(..end)?, text.get(end..)?))
}

/// RFC 6868: `^n` is a line break, `^^` a caret, `^'` a double quote.
fn decode_caret(value: &str) -> String {
    if !value.contains('^') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' {
            match chars.peek() {
                Some('n' | 'N') => {
                    out.push('\n');
                    chars.next();
                    continue;
                }
                Some('^') => {
                    out.push('^');
                    chars.next();
                    continue;
                }
                Some('\'') => {
                    out.push('"');
                    chars.next();
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

/// Undoes TEXT escapes: `\n`/`\N` line break, `\\`, `\,`, `\;`, and `\:` (Outlook). An unknown
/// escape keeps both characters.
pub(super) fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(e @ ('\\' | ',' | ';' | ':' | '"')) => out.push(e),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// One line of display text: control characters (line breaks too) become spaces, bidirectional
/// controls go (C-8), trimmed, cut to `max` bytes. `None` when empty.
pub(super) fn clean_line(text: &str, max: usize) -> Option<String> {
    let mapped: String = text
        .chars()
        .filter(|c| !mail_model::is_bidi_control(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = mapped.trim();
    let cut = trimmed
        .get(..trimmed.floor_char_boundary(max))
        .unwrap_or_default();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_owned())
}

/// A block of display text: `\r` removed, other control characters except `\n` and `\t`
/// removed, trimmed, cut to `max` bytes (then ending in `…`). `None` when empty.
pub(super) fn clean_block(text: &str, max: usize) -> Option<String> {
    let mapped: String = text
        .chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
        .collect();
    let trimmed = mapped.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= max {
        return Some(trimmed.to_owned());
    }
    let cut = trimmed
        .get(..trimmed.floor_char_boundary(max))
        .unwrap_or_default()
        .trim_end();
    Some(format!("{cut}…"))
}

/// A short, cleaned copy for problem details and raw values.
pub(super) fn short(text: &str, max: usize) -> String {
    clean_line(text, max).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(data: &[u8]) -> Vec<String> {
        Unfolder::new(data, 1000)
            .map(|l| String::from_utf8_lossy(&l.bytes).into_owned())
            .collect()
    }

    #[test]
    fn unfolds_crlf_lf_space_and_tab() {
        assert_eq!(
            lines(b"A:1\r\n 2\r\n\t3\r\nB:x\nC:y\n z"),
            ["A:123", "B:x", "C:yz"]
        );
        // A fold inside a UTF-8 character joins the bytes before decoding.
        assert_eq!(lines(b"S:\xC3\r\n \xA4\r\n"), ["S:ä"]);
        assert_eq!(lines(b""), Vec::<String>::new());
        assert_eq!(lines(b"\n\n"), ["", ""]);
    }

    #[test]
    fn long_lines_are_cut_on_a_character_boundary() {
        let mut data = b"S:".to_vec();
        data.extend("ä".repeat(10).as_bytes());
        let line = Unfolder::new(&data, 7).next().expect("line");
        assert!(line.cut);
        assert_eq!(line.bytes, "S:ää".as_bytes());
    }

    #[test]
    fn parameters_with_quotes() {
        let line = parse_line(
            "ATTENDEE;CN=\"Doe, John: Jr.\";ROLE=REQ-PARTICIPANT;DELEGATED-FROM=\"mailto:a@x\",\"mailto:b@x\";X-EMPTY:mailto:j@x",
        )
        .expect("line");
        assert_eq!(line.name, "ATTENDEE");
        assert_eq!(line.param("CN"), Some("Doe, John: Jr."));
        assert_eq!(line.param("ROLE"), Some("REQ-PARTICIPANT"));
        assert_eq!(line.param("DELEGATED-FROM"), Some("mailto:a@x,mailto:b@x"));
        assert_eq!(line.param("X-EMPTY"), Some(""));
        assert_eq!(line.value, "mailto:j@x");
        let line = parse_line("dtstart;tzid=Europe/Berlin:20260101T100000").expect("line");
        assert_eq!(line.name, "DTSTART");
        assert_eq!(line.param("TZID"), Some("Europe/Berlin"));
        assert_eq!(
            parse_line("X;CN=a^'b^'^nc^^:v").expect("line").param("CN"),
            Some("a\"b\"\nc^")
        );
        assert_eq!(parse_line("no colon"), None);
        assert_eq!(parse_line(":value"), None);
        assert_eq!(parse_line("A;CN=\"unclosed:x"), None);
        assert_eq!(parse_line("A;=x:y"), None);
        assert_eq!(parse_line("A:").expect("line").value, "");
    }

    #[test]
    fn text_escapes() {
        assert_eq!(
            unescape_text(r"a\, b\; c\\d\nnew\Nline\:x\q\"),
            "a, b; c\\d\nnew\nline:x\\q\\"
        );
        assert_eq!(clean_line(" a\tb\nc ", 100).as_deref(), Some("a b c"));
        assert_eq!(clean_line("  ", 100), None);
        // C-8: titles and names of invitations lose bidirectional controls.
        assert_eq!(
            clean_line("Call\u{202E}exe.pdf\u{2066}", 100).as_deref(),
            Some("Callexe.pdf")
        );
        assert_eq!(clean_line("\u{200E}", 100), None);
        assert_eq!(clean_line("äöü", 3).as_deref(), Some("ä"));
        assert_eq!(clean_block("a\r\n\u{7}b", 100).as_deref(), Some("a\nb"));
        assert_eq!(clean_block("abcdef", 3).as_deref(), Some("abc…"));
    }
}
