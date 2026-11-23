//! Folding for search: case, accents and compatibility forms do not matter (`Müller`, `MULLER`
//! and `Muller` are the same), and German spellings without umlauts find names with them
//! (`mue` finds `Müller`).

use icu_normalizer::DecomposingNormalizerBorrowed;
use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;

/// Text in lower case without accents: NFKD, combining marks removed, letters that do not
/// decompose spelled out (`ß` → `ss`, `æ` → `ae`, `ø` → `o`, `ł` → `l`, `đ` → `d`, `þ` → `th`,
/// `œ` → `oe`, `ı` → `i`). Punctuation stays.
pub fn fold(text: &str) -> String {
    let nfkd = DecomposingNormalizerBorrowed::new_nfkd();
    let category = CodePointMapData::<GeneralCategory>::new();
    let mut out = String::with_capacity(text.len());
    for c in nfkd.normalize(text).chars() {
        if matches!(
            category.get(c),
            GeneralCategory::NonspacingMark
                | GeneralCategory::SpacingMark
                | GeneralCategory::EnclosingMark
        ) {
            continue;
        }
        for lower in c.to_lowercase() {
            match lower {
                'ß' => out.push_str("ss"),
                'æ' => out.push_str("ae"),
                'œ' => out.push_str("oe"),
                'ø' => out.push('o'),
                'ł' => out.push('l'),
                'đ' | 'ð' => out.push('d'),
                'þ' => out.push_str("th"),
                'ı' => out.push('i'),
                other => out.push(other),
            }
        }
    }
    out
}

/// [`fold`] after spelling German umlauts out (`ä` → `ae`, `ö` → `oe`, `ü` → `ue`): `Müller` →
/// `mueller`.
pub fn fold_german(text: &str) -> String {
    let mut spelled = String::with_capacity(text.len() + 4);
    for c in text.chars() {
        match c {
            'ä' => spelled.push_str("ae"),
            'ö' => spelled.push_str("oe"),
            'ü' => spelled.push_str("ue"),
            'Ä' => spelled.push_str("Ae"),
            'Ö' => spelled.push_str("Oe"),
            'Ü' => spelled.push_str("Ue"),
            c => spelled.push(c),
        }
    }
    fold(&spelled)
}

/// Words of `text`, folded both ways, split at everything but letters and digits.
pub(crate) fn words(text: &str, out: &mut Vec<String>) {
    for folded in [fold(text), fold_german(text)] {
        for word in folded.split(|c: char| !c.is_alphanumeric()) {
            if !word.is_empty() && !out.iter().any(|w| w == word) {
                out.push(word.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding() {
        assert_eq!(fold("Müller"), "muller");
        assert_eq!(fold("MÜLLER"), "muller");
        assert_eq!(fold("Mu\u{308}ller"), "muller", "decomposed input");
        assert_eq!(fold("Groß"), "gross");
        assert_eq!(fold("Łukasz Wiśniewski"), "lukasz wisniewski");
        assert_eq!(fold("Søren Ærø"), "soren aero");
        assert_eq!(fold("Zoë Ångström"), "zoe angstrom");
        assert_eq!(fold("ﬁ Ｆｕｌｌ"), "fi full", "compatibility forms");
        assert_eq!(fold("Иванова"), "иванова");
        assert_eq!(
            fold("hans.mueller@firma.example"),
            "hans.mueller@firma.example"
        );
        assert_eq!(fold_german("Müller-Lüdenscheidt"), "mueller-luedenscheidt");
        assert_eq!(fold_german("Groß"), "gross");
        let mut out = Vec::new();
        words("Hans-Jürgen Müller", &mut out);
        assert_eq!(out, ["hans", "jurgen", "muller", "juergen", "mueller"]);
    }
}
