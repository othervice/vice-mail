//! Text from mail headers that is shown as one line (subjects, display names): what it may not
//! contain beyond control characters (finding C-8 of the stage 15 review).

/// Bidirectional controls: embeddings and overrides (U+202A–U+202E), isolates (U+2066–U+2069)
/// and the direction marks (U+200E, U+200F, U+061C). In a subject or a sender name they can
/// reorder what is shown (`Invoice\u{202E}fdp.exe` shows as `Invoiceexe.pdf`), so header text loses
/// them. Right-to-left names and subjects still show correctly: the UI lays out each one on
/// its own, and the marks only matter where scripts mix inside one string.
pub fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}

/// `text` without [`is_bidi_control`] characters.
pub fn strip_bidi_controls(text: &str) -> String {
    text.chars().filter(|c| !is_bidi_control(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bidi_control_goes_and_nothing_else() {
        let controls: Vec<char> = ('\u{202A}'..='\u{202E}')
            .chain('\u{2066}'..='\u{2069}')
            .chain(['\u{200E}', '\u{200F}', '\u{061C}'])
            .collect();
        assert_eq!(controls.len(), 12);
        assert!(controls.iter().all(|c| is_bidi_control(*c)));
        for kept in [
            'a', ' ', 'ü', 'ש', 'م', '\u{200C}', '\u{200D}', '\u{2065}', '\u{202F}',
        ] {
            assert!(!is_bidi_control(kept), "{kept:?}");
        }
        assert_eq!(
            strip_bidi_controls("Invoice\u{202E}fdp.exe"),
            "Invoicefdp.exe"
        );
        assert_eq!(
            strip_bidi_controls("\u{2067}שלום\u{2069} \u{200F}abc"),
            "שלום abc"
        );
    }
}
