//! Preview text for the message list: the start of what the sender wrote, without quoted
//! replies and signatures, on one line.

use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;

use crate::text::html_text;

/// Length of a preview in characters.
pub const PREVIEW_CHARS: usize = 200;

/// Preview of a plain-text body. Stops at a signature separator (`-- `) or an Outlook reply
/// header; skips quoted lines (`>`) and the "On … wrote:" line before them; collapses all
/// whitespace; removes control and invisible format characters (newsletters pad their hidden
/// preheader with them).
pub fn preview_from_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut collected = String::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if *line == "-- " || trimmed == "--" || is_reply_header(trimmed) {
            break;
        }
        if trimmed.starts_with('>') {
            continue;
        }
        if trimmed.ends_with(':') && next_content_is_quote(&lines[i + 1..]) {
            continue;
        }
        collected.push_str(trimmed);
        collected.push(' ');
        if collected.chars().count() > PREVIEW_CHARS * 2 {
            break;
        }
    }
    normalize(&collected)
}

/// Preview of an HTML body: its text (without link targets), then [`preview_from_text`].
/// Quoted replies in `<blockquote>` are skipped like `>` lines.
pub fn preview_from_html(html: &str) -> String {
    preview_from_text(&html_text(html, false))
}

fn is_reply_header(line: &str) -> bool {
    line.starts_with("-----Original Message-----")
        || line.starts_with("-----Ursprüngliche Nachricht-----")
        || (line.len() >= 10 && line.chars().all(|c| c == '_'))
}

fn next_content_is_quote(rest: &[&str]) -> bool {
    rest.iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .is_some_and(|line| line.starts_with('>'))
}

/// One line, single spaces, no invisible padding, at most [`PREVIEW_CHARS`] characters.
fn normalize(text: &str) -> String {
    let category = CodePointMapData::<GeneralCategory>::new();
    let invisible = |c: char| {
        matches!(
            c,
            '\u{200C}' | '\u{200D}' | '\u{034F}' | '\u{FE00}'..='\u{FE0F}'
        ) || category.get(c) == GeneralCategory::Format
    };
    let mut out = String::new();
    let mut count = 0;
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()) {
        // Words made only of invisible characters are padding.
        if word.chars().all(invisible) {
            continue;
        }
        let word: String = word
            .chars()
            .filter(|c| {
                *c == '\u{200C}' || *c == '\u{200D}' || category.get(*c) != GeneralCategory::Format
            })
            .collect();
        let word = word.trim_matches(['\u{200C}', '\u{200D}', '\u{034F}']);
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= PREVIEW_CHARS {
                return out.trim_end().to_owned();
            }
            out.push(c);
            count += 1;
        }
        if count >= PREVIEW_CHARS {
            break;
        }
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_preview_skips_quotes_and_signature() {
        let text = "Hi Alice,\n\nthe   meeting is\tmoved.\n\nOn Mon, Bob wrote:\n> old text\n> more\n\
                    Thanks!\n-- \nBob\nCEO";
        assert_eq!(
            preview_from_text(text),
            "Hi Alice, the meeting is moved. Thanks!"
        );
        let outlook = "Sure.\n\n-----Original Message-----\nFrom: x";
        assert_eq!(preview_from_text(outlook), "Sure.");
        let underscores = "Ok\n________________________________\nVon: x";
        assert_eq!(preview_from_text(underscores), "Ok");
    }

    #[test]
    fn preview_is_bounded_and_clean() {
        let long = "word ".repeat(200);
        let preview = preview_from_text(&long);
        assert!(preview.chars().count() <= PREVIEW_CHARS);
        assert!(!preview.ends_with(' '));
        let padded =
            "Sale\u{200C}\u{00A0}\u{034F} \u{200C} \u{034F}\u{200C} \u{FEFF}now\u{202E}!\u{0}x";
        assert_eq!(preview_from_text(padded), "Sale now! x");
        assert_eq!(preview_from_text(""), "");
        // Emoji sequences keep their joiner.
        assert_eq!(preview_from_text("👩\u{200D}💻 ok"), "👩\u{200D}💻 ok");
    }

    #[test]
    fn html_preview_uses_visible_text() {
        let html = "<html><head><style>.x{}</style><title>Newsletter</title></head><body>\
                    <div style='display:none'>Preheader text&zwnj;&nbsp;&zwnj;&nbsp;</div>\
                    <p>Hello <a href='https://tracker.example/x'>there</a></p>\
                    <blockquote>quoted reply</blockquote><script>no()</script></body></html>";
        assert_eq!(preview_from_html(html), "Preheader text Hello there");
    }
}
