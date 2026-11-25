//! Stage 19 (decision 0055): a message's source as display text ("view source") and its header
//! fields one by one ("all headers").
//!
//! Both are plain text for the UI, never markup: the UI shows them as text nodes. Characters
//! that would change how the text looks without being visible are replaced, so the source shows
//! what is there:
//! - control characters other than tab and line feed by the symbols of the Unicode block
//!   "Control Pictures" (U+2400 to U+2421), C1 controls (U+0080 to U+009F) by U+FFFD;
//! - direction controls (`mail_model::is_bidi_control`, security review C-8) by U+FFFD;
//! - bytes that are not UTF-8 by U+FFFD (lossy decoding; "Save as .eml" keeps the bytes).

use mail_parser::{HeaderForm, MessageParser};

/// A message's source for the window: the text and whether it was cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceText {
    pub text: String,
    /// Bytes of the message the text shows (the whole message unless `truncated`).
    pub shown_bytes: usize,
    pub truncated: bool,
}

/// One header field: the name as written, the value unfolded and with RFC 2047 encoded words
/// decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

/// The raw message as text: at most `max_bytes` (cut after the last line break within the
/// limit, or at the limit for a single longer line), CRLF and lone CR as LF, every character
/// visible (see the module docs).
pub fn source_text(raw: &[u8], max_bytes: usize) -> SourceText {
    let (shown, truncated) = if raw.len() > max_bytes {
        let head = raw.get(..max_bytes).unwrap_or(raw);
        let cut = head
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(head.len(), |i| i + 1);
        (raw.get(..cut).unwrap_or(head), true)
    } else {
        (raw, false)
    };
    let decoded = String::from_utf8_lossy(shown);
    let mut text = String::with_capacity(decoded.len());
    let mut chars = decoded.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    text.push('\n');
                }
            }
            '\n' | '\t' => text.push(c),
            _ => text.push(visible(c)),
        }
    }
    SourceText {
        text,
        shown_bytes: shown.len(),
        truncated,
    }
}

/// The header fields of the message's top-level header, in order: at most `max_fields`, values
/// cut to `max_value_chars` characters. Whitespace runs (folding) become one space.
pub fn header_fields(raw: &[u8], max_fields: usize, max_value_chars: usize) -> Vec<HeaderField> {
    let Some(message) = MessageParser::new().parse_headers(raw) else {
        return Vec::new();
    };
    message
        .headers()
        .iter()
        .take(max_fields)
        .map(|header| {
            let name: String = header.raw_name().chars().map(visible).collect();
            let decoded = header
                .parse_as(HeaderForm::Text)
                .value()
                .as_text()
                .map(str::to_owned);
            let value =
                decoded.unwrap_or_else(|| String::from_utf8_lossy(header.raw_value()).into_owned());
            HeaderField {
                name,
                value: one_line(&value, max_value_chars),
            }
        })
        .collect()
}

/// The value on one line: runs of whitespace as one space, trimmed, cut, every character
/// visible.
fn one_line(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for word in value.split(|c: char| c.is_whitespace()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            if count >= max_chars {
                break;
            }
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= max_chars {
                break;
            }
            out.push(visible(c));
            count += 1;
        }
    }
    out
}

/// The character as it can be shown (see the module docs).
fn visible(c: char) -> char {
    match c {
        '\u{0}'..='\u{1f}' => char::from_u32(0x2400 + u32::from(c)).unwrap_or('\u{fffd}'),
        '\u{7f}' => '\u{2421}',
        '\u{80}'..='\u{9f}' => '\u{fffd}',
        c if mail_model::is_bidi_control(c) => '\u{fffd}',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_text_with_every_character_visible() {
        let raw = b"Subject: x\r\nX-Odd: a\x1bb\x7f\r\n\r\nInvoice\xe2\x80\xaefdp.exe \xff ok\rnext\ttab\r\n";
        let source = source_text(raw, 1024);
        assert!(!source.truncated);
        assert_eq!(source.shown_bytes, raw.len());
        assert_eq!(
            source.text,
            "Subject: x\nX-Odd: a\u{241b}b\u{2421}\n\nInvoice\u{fffd}fdp.exe \u{fffd} ok\nnext\ttab\n"
        );
    }

    #[test]
    fn large_messages_are_cut_at_a_line_break() {
        let raw = b"line one\r\nline two\r\nline three\r\n";
        let source = source_text(raw, 15);
        assert!(source.truncated);
        assert_eq!(source.shown_bytes, 10);
        assert_eq!(source.text, "line one\n");
        // One long line: cut at the limit.
        let long = source_text(&[b'a'; 40], 16);
        assert_eq!((long.shown_bytes, long.truncated), (16, true));
        // A cut inside a multi-byte character does not panic.
        let umlaut = source_text("ääää".as_bytes(), 3);
        assert!(umlaut.truncated);
        assert_eq!(umlaut.text, "ä\u{fffd}");
    }

    #[test]
    fn header_fields_are_listed_in_order_decoded_and_unfolded() {
        let raw = b"Received: from a\r\n\tby b; Mon, 23 Nov 2026 12:00:00 +0000\r\n\
From: =?utf-8?q?J=C3=B6rg?= <joerg@example.com>\r\n\
Subject: =?utf-8?b?R3LDvMOfZQ==?= and more\r\n\
X-Custom:   spaced    value  \r\n\
Subject: second\r\n\r\nbody";
        let fields = header_fields(raw, 100, 1000);
        let pairs: Vec<(&str, &str)> = fields
            .iter()
            .map(|f| (f.name.as_str(), f.value.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("Received", "from a by b; Mon, 23 Nov 2026 12:00:00 +0000"),
                ("From", "Jörg <joerg@example.com>"),
                ("Subject", "Grüße and more"),
                ("X-Custom", "spaced value"),
                ("Subject", "second"),
            ]
        );
    }

    #[test]
    fn header_fields_are_bounded_and_never_hide_characters() {
        let mut raw = Vec::new();
        for n in 0..20 {
            raw.extend_from_slice(format!("X-N{n}: {}\r\n", "v".repeat(50)).as_bytes());
        }
        raw.extend_from_slice(b"\r\n");
        let fields = header_fields(&raw, 5, 10);
        assert_eq!(fields.len(), 5);
        assert!(fields.iter().all(|f| f.value.chars().count() == 10));
        let tricky = header_fields("Subject: a\u{202e}b\r\n\r\n".as_bytes(), 5, 100);
        assert_eq!(tricky[0].value, "a\u{fffd}b");
        assert!(header_fields(b"", 5, 100).is_empty());
    }
}
