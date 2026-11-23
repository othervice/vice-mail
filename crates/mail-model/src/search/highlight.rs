//! Where the words of a query occur in a shown text (subject, sender, preview), for highlighting
//! search hits.
//!
//! The index is contentless (decision 0003), so FTS5's `highlight()` and `snippet()` have no text
//! to work on; the ranges are found here, in the text the list shows. Matching imitates the
//! tokenizer `unicode61 remove_diacritics 2`: words are runs of letters and digits, compared
//! without case and without the diacritics of Latin letters (`Müller` matches `muller`). A phrase
//! highlights each of its words; a prefix term (`rech*`, or the word being typed) also the start
//! of longer words. Close enough for marking: the hit itself comes from the index.

use super::parse::{is_token_char, last_word_chars};
use super::{Clause, CompileOptions, Filter, MIN_PREFIX_CHARS, ParsedQuery, Span, TextField};

/// The shown field a text belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightField {
    Subject,
    /// Display name or address of the sender.
    From,
    /// Start of the body text.
    Preview,
}

/// The words of one positive text term, folded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightTerm {
    pub field: TextField,
    pub words: Vec<String>,
    /// The last word also matches the start of longer words.
    pub prefix_last: bool,
}

impl HighlightTerm {
    fn applies_to(&self, field: HighlightField) -> bool {
        matches!(
            (self.field, field),
            (TextField::Any, _)
                | (TextField::Subject, HighlightField::Subject)
                | (TextField::From, HighlightField::From)
        )
    }
}

impl ParsedQuery {
    /// The positive text terms (also inside `OR` groups) as words to highlight. Excluded terms
    /// are never highlighted.
    pub fn highlight_terms(&self, options: CompileOptions) -> Vec<HighlightTerm> {
        let typing_at = (options.prefix_last_word && self.open_end).then_some(self.input_len);
        let mut out = Vec::new();
        for clause in &self.clauses {
            let terms = match clause {
                Clause::Term(term) => std::slice::from_ref(term),
                Clause::AnyOf(terms) => terms.as_slice(),
            };
            for term in terms {
                let Filter::Text(text) = &term.filter else {
                    continue;
                };
                if term.negated {
                    continue;
                }
                let words: Vec<String> = words(&text.text).map(|(_, w)| fold(w)).collect();
                if words.is_empty() {
                    continue;
                }
                let open = typing_at == Some(term.span.end)
                    && !text.quoted
                    && text.text.chars().next_back().is_some_and(is_token_char);
                let prefix_last =
                    (text.prefix || open) && last_word_chars(&text.text) >= MIN_PREFIX_CHARS;
                out.push(HighlightTerm {
                    field: text.field,
                    words,
                    prefix_last,
                });
            }
        }
        out
    }
}

/// Byte ranges of `text` to mark for `terms`, sorted, without overlaps.
pub fn highlight(text: &str, field: HighlightField, terms: &[HighlightTerm]) -> Vec<Span> {
    let terms: Vec<&HighlightTerm> = terms.iter().filter(|t| t.applies_to(field)).collect();
    if terms.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Span> = Vec::new();
    for (span, word) in words(text) {
        let folded = fold(word);
        let hit = terms.iter().any(|term| {
            term.words.iter().enumerate().any(|(i, w)| {
                if term.prefix_last && i + 1 == term.words.len() {
                    folded.starts_with(w.as_str())
                } else {
                    folded == *w
                }
            })
        });
        if hit {
            out.push(span);
        }
    }
    out
}

/// Words of `text` (runs of token characters) with their byte ranges.
fn words(text: &str) -> impl Iterator<Item = (Span, &str)> {
    let mut rest = text.char_indices().peekable();
    std::iter::from_fn(move || {
        while let Some(&(_, c)) = rest.peek() {
            if is_token_char(c) {
                break;
            }
            rest.next();
        }
        let (start, _) = *rest.peek()?;
        let mut end = start;
        while let Some(&(i, c)) = rest.peek() {
            if !is_token_char(c) {
                break;
            }
            end = i + c.len_utf8();
            rest.next();
        }
        Some((Span::new(start, end), &text[start..end]))
    })
}

/// Lower case without the diacritics of Latin letters (`Ä` → `a`, `ß` stays), like the
/// tokenizer's `remove_diacritics 2` for the letters mail in European languages uses.
pub fn fold(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .map(strip_diacritic)
        .collect()
}

fn strip_diacritic(c: char) -> char {
    const TABLE: &[(&str, char)] = &[
        ("àáâãäåāăąǎ", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęěȩ", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥħ", 'h'),
        ("ìíîïĩīĭįıǐ", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöøōŏőǒ", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşšș", 's'),
        ("ţťŧț", 't'),
        ("ùúûüũūŭůűųǔ", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    if c.is_ascii() {
        return c;
    }
    TABLE
        .iter()
        .find(|(letters, _)| letters.contains(c))
        .map_or(c, |(_, base)| *base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{FixedOffset, SearchContext, SlashDateOrder, parse};

    fn terms(input: &str, typing: bool) -> Vec<HighlightTerm> {
        let ctx = SearchContext {
            now: 1_795_305_600,
            time_zone: &FixedOffset(3600),
            slash_dates: SlashDateOrder::DayFirst,
        };
        parse(input, &ctx).highlight_terms(CompileOptions {
            prefix_last_word: typing,
        })
    }

    fn marked<'a>(text: &'a str, field: HighlightField, input: &str, typing: bool) -> Vec<&'a str> {
        highlight(text, field, &terms(input, typing))
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect()
    }

    #[test]
    fn words_fold_case_and_diacritics() {
        assert_eq!(fold("Müller"), "muller");
        assert_eq!(fold("GRÜSSE"), "grusse");
        assert_eq!(fold("Straße"), "straße");
        assert_eq!(
            marked(
                "Rechnung für Herrn Müller",
                HighlightField::Subject,
                "muller rechnung",
                false
            ),
            ["Rechnung", "Müller"]
        );
    }

    #[test]
    fn fields_prefixes_and_exclusions() {
        // `from:` marks the sender only; free text marks everything.
        assert_eq!(
            marked("Bob Baumann", HighlightField::From, "from:bob", false),
            ["Bob"]
        );
        assert!(marked("Bob", HighlightField::Subject, "from:bob", false).is_empty());
        // Typing: the last word is a prefix (at least three characters).
        assert_eq!(
            marked(
                "Rechnungen und Rechte",
                HighlightField::Preview,
                "rech",
                true
            ),
            ["Rechnungen", "Rechte"]
        );
        assert!(marked("Rechnungen", HighlightField::Preview, "rech", false).is_empty());
        assert!(marked("Rechnungen", HighlightField::Preview, "re", true).is_empty());
        // Excluded words and filters are never marked.
        assert!(
            marked(
                "spam here",
                HighlightField::Preview,
                "-spam is:unread",
                false
            )
            .is_empty()
        );
        // Phrases mark each word; OR groups both sides.
        assert_eq!(
            marked(
                "Vielen Dank, Anna",
                HighlightField::Preview,
                "\"vielen dank\" OR anna",
                false
            ),
            ["Vielen", "Dank", "Anna"]
        );
        // Byte ranges stay on character boundaries in non-ASCII text.
        let text = "Grüße 🎉 Ärger";
        for span in highlight(text, HighlightField::Subject, &terms("ärger grüße", false)) {
            assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
        }
    }
}
