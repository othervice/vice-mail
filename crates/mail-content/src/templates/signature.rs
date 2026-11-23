//! Signatures: allowlisted HTML for the editor and its plain-text form.

use super::snippet::{SanitizeNotes, SnippetError, check_size, sanitize};
use crate::compose::{Block, Inline, html_blocks, text_blocks, trim_paragraphs};
use crate::text::html_to_text;

/// Largest signature as editor HTML; with its text it fits a sync record (64 KiB, `docs/SYNC.md`).
pub const MAX_SIGNATURE_BYTES: usize = 16 * 1024;

/// The signature separator line of plain text (RFC 3676, section 4.3): dash, dash, space.
pub const SIGNATURE_SEPARATOR: &str = "-- ";

/// A signature as stored: allowlisted editor HTML and the plain text, both without the
/// separator (it is added when the message is put together).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signature {
    pub html: String,
    pub text: String,
}

impl Signature {
    /// From the HTML of the signature editor (or pasted HTML): sanitised with the mail HTML
    /// allowlist; a leading separator line the user typed is removed.
    pub fn from_html(html: &str) -> Result<(Self, SanitizeNotes), SnippetError> {
        let (mut blocks, notes) = sanitize(html, MAX_SIGNATURE_BYTES)?;
        drop_separator(&mut blocks);
        let html = check_size(&blocks, MAX_SIGNATURE_BYTES)?;
        let text = clean_text(&html_to_text(&html));
        Ok((Self { html, text }, notes))
    }

    /// From plain text (lines and links, no formatting).
    pub fn from_text(text: &str) -> Result<Self, SnippetError> {
        let mut lines: Vec<&str> = text.lines().collect();
        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
        if lines.first().is_some_and(|l| is_separator(l)) {
            lines.remove(0);
        }
        let text = clean_text(&lines.join("\n"));
        let mut blocks = text_blocks(&text);
        trim_paragraphs(&mut blocks);
        let html = check_size(&blocks, MAX_SIGNATURE_BYTES)?;
        Ok(Self { html, text })
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.html.trim().is_empty()
    }

    /// The plain text with the separator line before it.
    pub fn text_with_separator(&self) -> String {
        format!("{SIGNATURE_SEPARATOR}\n{}", self.text)
    }

    pub(crate) fn blocks(&self) -> Vec<Block> {
        html_blocks(&self.html)
    }
}

fn is_separator(line: &str) -> bool {
    matches!(line.trim(), "--" | "—" | "–" | "---")
}

/// Removes a separator line at the start: a paragraph of its own, or the first line of the
/// first paragraph (`<p>-- <br>Hans</p>`).
fn drop_separator(blocks: &mut Vec<Block>) {
    let Some(Block::Paragraph(inlines)) = blocks.first_mut() else {
        return;
    };
    let starts_with_separator = match inlines.first() {
        Some(Inline::Text(text, _)) => is_separator(text),
        _ => false,
    };
    let first_line_is_separator =
        starts_with_separator && matches!(inlines.get(1), None | Some(Inline::Break));
    if !first_line_is_separator {
        return;
    }
    if inlines.len() <= 2 {
        blocks.remove(0);
    } else {
        inlines.drain(..2);
    }
    trim_paragraphs(blocks);
}

/// Line ends `\n`, trailing white space of lines and blank lines at both ends removed, no
/// control characters except tabs.
fn clean_text(text: &str) -> String {
    let lines: Vec<String> = text
        .replace("\r\n", "\n")
        .split('\n')
        .map(|line| {
            line.chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    let start = lines.iter().take_while(|l| l.is_empty()).count();
    let end = lines.len() - lines.iter().rev().take_while(|l| l.is_empty()).count();
    lines
        .get(start..end.max(start))
        .unwrap_or_default()
        .join("\n")
}
