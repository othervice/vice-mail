//! Text cleaning in both directions.
//!
//! Into the model: mail text loses control characters and characters that show nothing to a
//! human but are read by a model (`mail_content::strip_hidden_chars`: Unicode tag characters,
//! bidi overrides, zero-width spaces, variation selectors outside emoji sequences, …), so an
//! instruction cannot hide from the user in plain text. Text hidden by CSS in HTML bodies never
//! gets here (`mail_content::ParsedMessage::visible_text`: the visible HTML text, else the text body).
//!
//! Out of the model: [`clean_output`] makes one plain line of every text the model returns: no
//! control or hidden characters, Markdown image embeds reduced to their alt text (so a later
//! Markdown renderer cannot load them), limited length. Output is only ever data for text
//! nodes; links in it are shown through `mail_content::render_text` and opened only after
//! `mail_content::check_link`, like links in mails.

/// A multi-line text (a body) cleaned for the model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Cleaned {
    pub text: String,
    /// Hidden characters removed.
    pub hidden_removed: usize,
}

/// Line ends `\n`, tabs kept, other control characters and hidden characters removed, more
/// than two blank lines in a row collapsed, trailing white space of lines removed.
pub(crate) fn clean_text(input: &str) -> Cleaned {
    let stripped = mail_content::strip_hidden_chars(input);
    let mut out = Cleaned {
        hidden_removed: stripped.removed,
        ..Cleaned::default()
    };
    let mut blank_run = 0_usize;
    let normalized = stripped.text.replace("\r\n", "\n").replace('\r', "\n");
    for line in normalized.split('\n') {
        let mut kept = String::with_capacity(line.len());
        for c in line.chars() {
            if c == '\t' || !c.is_control() {
                kept.push(c);
            }
        }
        let kept = kept.trim_end();
        if kept.is_empty() {
            blank_run += 1;
            if blank_run > 2 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.text.push_str(kept);
        out.text.push('\n');
    }
    let end = out.text.trim_end().len();
    out.text.truncate(end);
    let start = out.text.len() - out.text.trim_start_matches('\n').len();
    out.text.drain(..start);
    out
}

/// One line (subject, name, file name, server message): white space and control characters
/// become one space, hidden characters are removed, cut to `max_chars` (with `…`).
pub(crate) fn clean_line(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut space = false;
    for c in mail_content::strip_hidden_chars(input).text.chars() {
        if c.is_whitespace() || c.is_control() {
            space = !out.is_empty();
            continue;
        }
        if space {
            if count + 1 >= max_chars {
                out.push('…');
                return out;
            }
            out.push(' ');
            count += 1;
            space = false;
        }
        if count == max_chars {
            out.push('…');
            return out;
        }
        out.push(c);
        count += 1;
    }
    out
}

/// A text from the model for display: [`clean_line`] after reducing Markdown image embeds
/// (`![alt](target)`) to their alt text. HTML stays literal text (the interface never parses
/// it).
pub(crate) fn clean_output(input: &str, max_chars: usize) -> String {
    clean_line(&strip_markdown_images(input), max_chars)
}

/// `![alt](anything)` becomes `alt`; also the reference form `![alt][ref]`. An unterminated
/// embed loses its `!` so it is no image either.
fn strip_markdown_images(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find("![") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let Some(close) = after.find(']') else {
            out.push_str(after);
            return out;
        };
        let alt = &after[..close];
        let tail = &after[close + 1..];
        let target_end = if let Some(target) = tail.strip_prefix('(') {
            target.find(')').map(|end| end + 2)
        } else if let Some(target) = tail.strip_prefix('[') {
            target.find(']').map(|end| end + 2)
        } else {
            Some(0)
        };
        out.push_str(alt);
        rest = match target_end {
            Some(end) => &tail[end..],
            // `![alt](` without end: drop the rest of the target.
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// The JSON object in a model's text: the whole text, or inside a ```` ``` ```` fence, or from
/// the first `{` to its matching `}`. Small local models like to add prose around it.
pub(crate) fn extract_json_object(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    if let Ok(value @ serde_json::Value::Object(_)) = serde_json::from_str(trimmed) {
        return Some(value);
    }
    let mut candidates = Vec::new();
    // Fenced blocks first.
    let mut rest = trimmed;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let body_start = after.find('\n').map_or(0, |n| n + 1);
        let body = &after[body_start..];
        let Some(end) = body.find("```") else { break };
        candidates.push(&body[..end]);
        rest = &body[end + 3..];
    }
    // Then every balanced `{ … }` at top level.
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = matching_brace(&trimmed[i..]) {
                candidates.push(&trimmed[i..i + end]);
                i += end;
                continue;
            }
            break;
        }
        i += 1;
    }
    candidates
        .into_iter()
        .find_map(|candidate| match serde_json::from_str(candidate.trim()) {
            Ok(value @ serde_json::Value::Object(_)) => Some(value),
            _ => None,
        })
}

/// Length up to and including the `}` that closes the `{` at the start, respecting strings.
fn matching_brace(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_and_control_characters_are_removed_from_bodies() {
        let input =
            "Hello\u{200B} world\r\n\u{E0049}\u{E0047}\u{E004E}\u{202E}evil\u{0007}\n\n\n\n\nend  ";
        let cleaned = clean_text(input);
        assert_eq!(cleaned.text, "Hello world\nevil\n\n\nend");
        assert_eq!(cleaned.hidden_removed, 5);
    }

    #[test]
    fn lines_collapse_space_and_are_cut() {
        assert_eq!(clean_line("  a\t b\n\nc  ", 100), "a b c");
        assert_eq!(clean_line("abcdef", 3), "abc…");
        assert_eq!(clean_line("ab cd", 3), "ab…");
        assert_eq!(clean_line("a\u{200B}b", 10), "ab");
    }

    #[test]
    fn markdown_images_lose_their_target() {
        assert_eq!(
            clean_output("See ![chart](http://evil.example/p.png?d=1) now", 200),
            "See chart now"
        );
        assert_eq!(clean_output("![x][ref] and ![y](", 200), "x and y");
        assert_eq!(
            clean_output("no image [link](http://a) here", 200),
            "no image [link](http://a) here"
        );
        assert_eq!(
            clean_output("<img src=x> stays text", 200),
            "<img src=x> stays text"
        );
    }

    #[test]
    fn json_is_found_in_prose_and_fences() {
        let plain = extract_json_object(r#"{"a": 1}"#).expect("plain");
        assert_eq!(plain["a"], 1);
        let fenced =
            extract_json_object("Here you go:\n```json\n{\"a\": 2}\n```\nThanks").expect("fenced");
        assert_eq!(fenced["a"], 2);
        let prose =
            extract_json_object("Sure! {\"a\": \"}{\", \"b\": {\"c\": 3}} done").expect("prose");
        assert_eq!(prose["b"]["c"], 3);
        assert!(extract_json_object("no json here").is_none());
        assert!(extract_json_object("[1, 2]").is_none());
        assert!(extract_json_object("{broken").is_none());
    }
}
