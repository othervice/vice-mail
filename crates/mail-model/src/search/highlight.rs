//! Where the words of a query occur in a shown text (subject, sender, preview), for highlighting
//! search hits.
//!
//! The index is contentless (decision 0003), so FTS5's `highlight()` and `snippet()` have no text
//! to work on; the ranges are found here, in the text the list shows. Matching imitates the
//! tokenizer `unicode61 remove_diacritics 2`: words are runs of letters and digits, compared
//! without case and without the diacritics of Latin letters (`Müller` matches `muller`). A phrase
//! highlights each of its words; a prefix term (`rech*`, or the word being typed) also the start
//! of longer words. Stage 22 (decision 0074): a term that also matches inside words (unquoted, at
//! least [`MIN_SUBSTRING_CHARS`] characters) marks where it occurs in the text, folded the same
//! way (`uster` marks "uster" in "Muster"). Close enough for marking: the hit itself comes from
//! the index.

use super::compile::is_combining_mark;
use super::parse::{is_token_char, last_word_chars};
use super::{
    Clause, CompileOptions, Filter, MIN_PREFIX_CHARS, MIN_SUBSTRING_CHARS, ParsedQuery, Span,
    Substrings, TextField,
};

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
    /// Decision 0074: the whole term, folded, also marks where it occurs inside the text.
    pub substring: Option<String>,
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
                let folded = fold(&text.text);
                let substring = (options.substrings != Substrings::Off
                    && !text.quoted
                    && folded.chars().filter(|c| !is_combining_mark(*c)).count()
                        >= MIN_SUBSTRING_CHARS)
                    .then_some(folded);
                out.push(HighlightTerm {
                    field: text.field,
                    words,
                    prefix_last,
                    substring,
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
    if terms.iter().any(|t| t.substring.is_some()) {
        let folded = FoldedText::new(text);
        for needle in terms.iter().filter_map(|t| t.substring.as_deref()) {
            out.extend(folded.find(needle));
        }
        out.sort();
        out = merged(out);
    }
    out
}

/// Overlapping or touching ranges as one.
fn merged(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => out.push(span),
        }
    }
    out
}

/// A text folded like [`fold`], with the original byte range of every folded character.
struct FoldedText {
    folded: String,
    /// Per byte offset of a character in `folded` (ascending): the original character's range.
    origin: Vec<(usize, Span)>,
}

impl FoldedText {
    fn new(text: &str) -> Self {
        let mut folded = String::with_capacity(text.len());
        let mut origin = Vec::with_capacity(text.len());
        for (start, c) in text.char_indices() {
            let span = Span::new(start, start + c.len_utf8());
            if is_combining_mark(c) {
                continue;
            }
            for lower in c.to_lowercase() {
                origin.push((folded.len(), span));
                folded.push(strip_diacritic(lower));
            }
        }
        Self { folded, origin }
    }

    /// The original ranges where `needle` (folded) occurs; overlapping occurrences too.
    fn find<'a>(&'a self, needle: &'a str) -> impl Iterator<Item = Span> + 'a {
        let mut from = 0;
        std::iter::from_fn(move || {
            if needle.is_empty() {
                return None;
            }
            let at = from + self.folded.get(from..)?.find(needle)?;
            let end = at + needle.len();
            // The next search starts one character later.
            from = at + self.folded[at..].chars().next().map_or(1, char::len_utf8);
            let first = self.origin.partition_point(|(offset, _)| *offset < at);
            let last = self.origin.partition_point(|(offset, _)| *offset < end);
            let start = self.origin.get(first)?.1.start;
            let stop = self.origin.get(last.checked_sub(1)?)?.1.end;
            Some(Span::new(start, stop))
        })
    }
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
/// tokenizer's `remove_diacritics 2` for the letters mail in European languages uses. Combining
/// marks (`u` + U+0308) are dropped, as the tokenizers drop them.
pub fn fold(word: &str) -> String {
    word.chars()
        .filter(|c| !is_combining_mark(*c))
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
            substrings: Substrings::Off,
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

    fn marked_inside<'a>(text: &'a str, field: HighlightField, input: &str) -> Vec<&'a str> {
        let ctx = SearchContext {
            now: 1_795_305_600,
            time_zone: &FixedOffset(3600),
            slash_dates: SlashDateOrder::DayFirst,
        };
        let terms = parse(input, &ctx).highlight_terms(CompileOptions {
            prefix_last_word: false,
            substrings: Substrings::Complete,
        });
        highlight(text, field, &terms)
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect()
    }

    /// Decision 0074: substring hits are marked where they occur, folded like the index.
    #[test]
    fn substrings_are_marked_inside_words() {
        assert_eq!(
            marked_inside("Muster und MUSTERMANN", HighlightField::Subject, "uster"),
            ["uster", "USTER"]
        );
        // Diacritics and case fold; the marked range is the original text.
        assert_eq!(
            marked_inside("Grüße von Jürgen", HighlightField::Preview, "urge"),
            ["ürge"]
        );
        assert_eq!(
            marked_inside("Re: Bestellung 123456", HighlightField::Subject, "2345"),
            ["2345"]
        );
        // A word hit and a substring hit of the same term: one mark.
        assert_eq!(
            marked_inside("Rechnung", HighlightField::Subject, "rechnung"),
            ["Rechnung"]
        );
        // Overlapping occurrences join; punctuation inside the term counts.
        assert_eq!(
            marked_inside("aaaa", HighlightField::Subject, "aaa"),
            ["aaaa"]
        );
        assert_eq!(
            marked_inside("bob@example.org", HighlightField::From, "b@exa"),
            ["b@exa"]
        );
        // Quoted terms and short terms mark whole words only.
        assert!(marked_inside("Muster", HighlightField::Subject, "\"uster\"").is_empty());
        assert!(marked_inside("Muster", HighlightField::Subject, "us").is_empty());
        // Excluded terms are never marked; fields still apply.
        assert!(marked_inside("Muster", HighlightField::Subject, "-uster").is_empty());
        assert!(marked_inside("Muster", HighlightField::Subject, "from:uster").is_empty());
        assert_eq!(
            marked_inside("Max Muster", HighlightField::From, "from:uster"),
            ["uster"]
        );
        // Characters that fold to several (`İ`) and decomposed text keep valid ranges.
        for text in ["İstanbul İst", "Mu\u{308}ster", "🎉uster🎉"] {
            for span in highlight(
                text,
                HighlightField::Subject,
                &parse(
                    "ust",
                    &SearchContext {
                        now: 0,
                        time_zone: &FixedOffset(0),
                        slash_dates: SlashDateOrder::DayFirst,
                    },
                )
                .highlight_terms(CompileOptions {
                    prefix_last_word: false,
                    substrings: Substrings::Complete,
                }),
            ) {
                assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
                assert!(span.start < span.end);
            }
        }
        assert_eq!(
            marked_inside("Mu\u{308}ster", HighlightField::Subject, "muster"),
            ["Mu\u{308}ster"]
        );
    }
}
