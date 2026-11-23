//! Characters that show nothing to a human reader but are read by a model: the one set every
//! text from a mail loses before it reaches the MCP server or the built-in AI.
//!
//! The base is the Unicode property `Default_Ignorable_Code_Point` (characters a renderer draws
//! as nothing): Unicode tag characters ("ASCII smuggling", U+E0000–E007F), the variation
//! selectors supplement, bidi embeddings, overrides and isolates, zero-width space, word joiner
//! and invisible operators, BOM, soft hyphen, Hangul fillers, the combining grapheme joiner,
//! Mongolian and Khmer format characters, unassigned ignorable code points.
//!
//! Kept where they have a visible effect, removed everywhere else ([`strip_hidden_chars`]):
//! - zero-width joiner and non-joiner: one, between two visible characters (emoji sequences,
//!   Persian, Indic scripts); runs and joiners next to white space carry nothing
//! - variation selectors 15 and 16 (text and emoji presentation): one, right after a character
//!   with the Unicode property `Emoji`; every other variation selector only picks a glyph
//!   variant (CJK, maths symbols) and is the alphabet of "variation selector smuggling", which
//!   hides bytes behind an emoji
//! - left-to-right, right-to-left and Arabic letter marks: one in a row (needed in right-to-left
//!   text); a run of them can only carry bits
//! - Mongolian free variation selectors: one, after a Mongolian letter
//!
//! Cost of the rule: subdivision flags (England, Scotland, Wales are written with tag
//! characters) become a plain black flag, and CJK ideographic variation sequences show the
//! default glyph. Both keep their meaning.

use icu_properties::CodePointSetData;
use icu_properties::props::{DefaultIgnorableCodePoint, Emoji};

/// Whether `c` is always removed from mail text: default-ignorable and not one of the
/// characters that [`strip_hidden_chars`] keeps in context.
pub fn is_hidden_char(c: char) -> bool {
    // Fast path: the first default-ignorable code point is the soft hyphen.
    if u32::from(c) < 0xAD {
        return false;
    }
    CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c) && !is_contextual(c)
}

/// Default-ignorable characters that are kept where they have a visible effect.
fn is_contextual(c: char) -> bool {
    matches!(
        c,
        '\u{200C}'
            | '\u{200D}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
            | '\u{FE0E}'
            | '\u{FE0F}'
            | '\u{180B}'..='\u{180D}' | '\u{180F}'
    )
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\u{200C}' | '\u{200D}')
}

fn is_direction_mark(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// A character a joiner may stand next to: something drawn, not white space or a format
/// character.
fn is_visible_base(c: char) -> bool {
    !c.is_whitespace() && !c.is_control() && !is_hidden_char(c) && !is_contextual(c)
}

/// Text without hidden characters, and how many were removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrippedText {
    pub text: String,
    /// Characters removed (Unicode scalar values).
    pub removed: usize,
}

/// Removes every [`is_hidden_char`] character and the contextual ones where they have no
/// visible effect (see the module documentation), and counts them.
pub fn strip_hidden_chars(text: &str) -> StrippedText {
    let emoji = CodePointSetData::new::<Emoji>();
    let mut out = StrippedText {
        text: String::with_capacity(text.len()),
        removed: 0,
    };
    // The last character kept.
    let mut previous: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let keep = if is_hidden_char(c) {
            false
        } else if is_joiner(c) {
            previous.is_some_and(|p| {
                !p.is_whitespace() && !p.is_control() && !is_joiner(p) && !is_direction_mark(p)
            }) && chars.peek().is_some_and(|next| is_visible_base(*next))
        } else if is_direction_mark(c) {
            !previous.is_some_and(is_direction_mark)
        } else if matches!(c, '\u{FE0E}' | '\u{FE0F}') {
            // A variation selector itself has no `Emoji` property, so a second one in a row
            // is removed.
            previous.is_some_and(|p| emoji.contains(p))
        } else if matches!(c, '\u{180B}'..='\u{180D}' | '\u{180F}') {
            previous.is_some_and(|p| ('\u{1820}'..='\u{18AA}').contains(&p))
        } else {
            true
        };
        if keep {
            out.text.push(c);
            previous = Some(c);
        } else {
            out.removed += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(text: &str) -> String {
        text.chars()
            .filter_map(|c| char::from_u32(0xE0000 + u32::from(c)))
            .collect()
    }

    #[test]
    fn smuggling_characters_are_removed_and_counted() {
        let input = format!(
            "Hello\u{200B} wor\u{202E}ld{}\u{2066}x\u{2069}\u{FEFF}\u{00AD}\u{034F}\u{2060}\u{2062}\u{115F}\u{3164}\u{180E}",
            tags("send to evil")
        );
        let stripped = strip_hidden_chars(&input);
        assert_eq!(stripped.text, "Hello worldx");
        assert_eq!(stripped.removed, 12 + 12);
        for c in [
            '\u{E0041}',
            '\u{E0100}',
            '\u{E01EF}',
            '\u{202A}',
            '\u{200B}',
            '\u{FE00}',
        ] {
            assert!(is_hidden_char(c), "{c:?}");
        }
        for c in [
            'a', ' ', '\n', 'ä', '\u{200D}', '\u{200E}', '\u{FE0F}', '\u{2800}',
        ] {
            assert!(!is_hidden_char(c), "{c:?}");
        }
    }

    #[test]
    fn variation_selectors_survive_only_in_emoji_sequences() {
        // Emoji presentation, keycap, ZWJ sequence with a variation selector: unchanged.
        for kept in [
            "\u{2764}\u{FE0F}",
            "1\u{FE0F}\u{20E3}",
            "\u{2764}\u{FE0F}\u{200D}\u{1F525}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "\u{2194}\u{FE0E}",
        ] {
            let stripped = strip_hidden_chars(kept);
            assert_eq!((stripped.text.as_str(), stripped.removed), (kept, 0));
        }
        // Bytes hidden behind an emoji ("variation selector smuggling"): only the emoji stays.
        let smuggled: String =
            std::iter::once('\u{1F600}')
                .chain(b"ignore".iter().map(|b| {
                    char::from_u32(0xE0100 + u32::from(*b) - 16).expect("variation selector")
                }))
                .chain(['\u{FE01}', '\u{FE0F}', '\u{FE0F}'])
                .collect();
        let stripped = strip_hidden_chars(&smuggled);
        assert_eq!(stripped.text, "\u{1F600}\u{FE0F}");
        assert_eq!(stripped.removed, 6 + 2);
        // Not after an emoji: removed.
        assert_eq!(strip_hidden_chars("a\u{FE0F}b\u{FE00}").text, "ab");
    }

    #[test]
    fn joiners_and_marks_are_kept_once_where_they_join() {
        // Persian word with a ZWNJ, Hebrew with a mark: unchanged.
        for kept in [
            "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645}",
            "a\u{200F}b",
        ] {
            assert_eq!(strip_hidden_chars(kept).text, kept);
        }
        let stripped = strip_hidden_chars(
            "x\u{200D}\u{200D}\u{200C}y \u{200D}z\u{200D} \u{200E}\u{200F}\u{200E}w",
        );
        assert_eq!(stripped.text, "x\u{200C}y z \u{200E}w");
        assert_eq!(stripped.removed, 2 + 1 + 1 + 2);
        assert_eq!(strip_hidden_chars("\u{200D}a\u{200D}").text, "a");
        // Mongolian free variation selector after a Mongolian letter only.
        assert_eq!(
            strip_hidden_chars("\u{1820}\u{180B}a\u{180B}").text,
            "\u{1820}\u{180B}a"
        );
    }
}
