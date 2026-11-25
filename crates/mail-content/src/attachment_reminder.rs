//! The attachment reminder (stage 20, decision 0058): does the user's own text speak of an
//! attachment? English and German, without the quote, the signature and quoted lines.
//!
//! The caller passes what the user wrote (the composer keeps quote and signature apart; for
//! HTML its text with [`crate::html_to_text`], which writes quotes as `> ` lines). Here the text
//! is cut at the signature separator (`-- `) and at the header of a quoted or forwarded
//! message that was pasted or typed into the text (`On … wrote:`, `Am … schrieb …:`,
//! `-----Original Message-----`, …); lines starting with `>` are skipped. Words in addresses
//! and links (`…/attachment.pdf`, `anhang@example.org`) do not count, nor a word right after a
//! negation (`no attachment`, `ohne Anhang`).

/// Words that speak of an attachment, compared in lower case.
const WORDS: &[&str] = &[
    // English
    "attach",
    "attached",
    "attaching",
    "attachment",
    "attachments",
    "enclosed",
    "enclosure",
    "enclosures",
    // German
    "anhang",
    "anhänge",
    "anhängen",
    "angehängt",
    "angehaengt",
    "anhaenge",
    "anbei",
    "anlage",
    "anlagen",
    "beigefügt",
    "beigefuegt",
    "beiliegend",
    "beigelegt",
];

/// Endings of German compounds that speak of an attachment (`Dateianhang`, `PDF-Anhänge` is
/// split at the hyphen anyway).
const COMPOUND_ENDINGS: &[&str] = &["anhang", "anhänge", "anhängen"];

/// Words that turn the next one or two into "no attachment".
const NEGATIONS: &[&str] = &[
    "no", "not", "without", "kein", "keine", "keinen", "keinem", "ohne", "nicht",
];

/// The first word of `own_text` that speaks of an attachment, as written; `None` if there is
/// none. See the module documentation for what is left out.
pub fn attachment_mention(own_text: &str) -> Option<String> {
    for line in own_text.lines() {
        let trimmed = line.trim();
        if is_signature_separator(line) || is_quote_header(trimmed) {
            return None;
        }
        if trimmed.starts_with('>') {
            continue;
        }
        if let Some(word) = mention_in_line(trimmed) {
            return Some(word);
        }
    }
    None
}

fn mention_in_line(line: &str) -> Option<String> {
    // The last two words seen, lower case, for negations.
    let mut previous: [Option<String>; 2] = [None, None];
    for chunk in line.split_whitespace() {
        if chunk.contains("://") || chunk.contains('@') || chunk.to_lowercase().starts_with("www.")
        {
            previous = [None, None];
            continue;
        }
        for token in chunk
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
        {
            let lower = token.to_lowercase();
            let negated = previous
                .iter()
                .flatten()
                .any(|word| NEGATIONS.contains(&word.as_str()));
            if !negated && speaks_of_attachment(&lower) {
                return Some(token.to_owned());
            }
            previous = [previous[1].take(), Some(lower)];
        }
    }
    None
}

fn speaks_of_attachment(lower: &str) -> bool {
    WORDS.contains(&lower)
        || COMPOUND_ENDINGS
            .iter()
            .any(|ending| lower.len() > ending.len() && lower.ends_with(ending))
}

/// `-- ` (RFC 3676), also without the trailing space as editors often drop it.
fn is_signature_separator(line: &str) -> bool {
    let line = line.trim_end_matches(['\r', '\n']);
    line == "-- " || line == "--"
}

/// The first line of a quoted or forwarded message inside the text.
fn is_quote_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    let dashes =
        |inner: &str| lower.starts_with("---") && lower.ends_with("---") && lower.contains(inner);
    dashes("original message")
        || dashes("ursprüngliche nachricht")
        || dashes("forwarded message")
        || dashes("weitergeleitete nachricht")
        || (lower.starts_with("on ") && lower.ends_with("wrote:"))
        || (lower.starts_with("am ") && lower.contains("schrieb") && lower.ends_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_and_german_words() {
        for (text, word) in [
            ("Hi Anna,\n\nI attached the invoice.", "attached"),
            ("Please see the attachment.", "attachment"),
            ("Enclosed you find the contract.", "Enclosed"),
            ("Hallo,\nim Anhang findest du die Rechnung.", "Anhang"),
            ("Ich habe dir das PDF angehängt.", "angehängt"),
            ("Anbei die Unterlagen.", "Anbei"),
            ("Die Datei liegt als Anlage bei.", "Anlage"),
            ("Siehe Dateianhang", "Dateianhang"),
            ("Die E-Mail-Anhänge sind zu groß", "Anhänge"),
            ("beigefügt: das Protokoll", "beigefügt"),
        ] {
            assert_eq!(attachment_mention(text).as_deref(), Some(word), "{text}");
        }
    }

    #[test]
    fn ordinary_text_and_look_alikes_do_not_count() {
        for text in [
            "Hi, see you tomorrow.",
            "Im Zusammenhang mit dem Vorhang",
            "We are attaché to the embassy",
            "",
        ] {
            assert_eq!(attachment_mention(text), None, "{text}");
        }
    }

    #[test]
    fn quotes_signatures_and_quoted_messages_are_left_out() {
        assert_eq!(
            attachment_mention("Thanks!\n> I attached the file.\n>> Anhang"),
            None
        );
        assert_eq!(
            attachment_mention("Thanks!\n\n-- \nAttachments are scanned for viruses."),
            None
        );
        assert_eq!(
            attachment_mention("Thanks!\n--\nAnhänge werden geprüft."),
            None
        );
        assert_eq!(
            attachment_mention(
                "Danke!\n\nAm 24.11.2026 um 10:00 schrieb Anna <anna@example.org>:\nIm Anhang …"
            ),
            None
        );
        assert_eq!(
            attachment_mention("Thanks!\nOn Mon, Nov 23, 2026 Bob wrote:\nsee attached"),
            None
        );
        assert_eq!(
            attachment_mention("FYI\n-------- Original Message --------\nAttached is"),
            None
        );
        // Before the quote it still counts.
        assert_eq!(
            attachment_mention("See attached.\n\nOn Mon, Bob wrote:\n> hi").as_deref(),
            Some("attached")
        );
    }

    #[test]
    fn links_addresses_and_negations_do_not_count() {
        assert_eq!(
            attachment_mention("Download it at https://example.org/attachment.pdf"),
            None
        );
        assert_eq!(attachment_mention("Write to anhang@example.org"), None);
        assert_eq!(
            attachment_mention("There is no attachment this time."),
            None
        );
        assert_eq!(attachment_mention("Diesmal ohne Anhang."), None);
        assert_eq!(
            attachment_mention("Ich habe keinen Anhang mitgeschickt."),
            None
        );
        assert_eq!(
            attachment_mention("No problem. The attachment follows.").as_deref(),
            Some("attachment")
        );
    }
}
