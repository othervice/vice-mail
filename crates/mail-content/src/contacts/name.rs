//! Display names: cleaning what mail headers carry, and splitting a person's name into first
//! and last name (for template placeholders).

use icu_normalizer::ComposingNormalizerBorrowed;

use super::fold::fold;

/// Longest display name kept.
pub const MAX_NAME_CHARS: usize = 128;

/// A display name worth keeping for `email`: in NFC, quotes and surrounding space removed, white
/// space collapsed, control characters dropped. `None` for empty names, names without a letter, and
/// names that are an address (contain `@`, or equal the address's local part).
pub fn clean_display_name(name: &str, email: &str) -> Option<String> {
    let mut cleaned = String::with_capacity(name.len());
    for word in ComposingNormalizerBorrowed::new_nfc()
        .normalize(name)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
    {
        if !cleaned.is_empty() {
            cleaned.push(' ');
        }
        cleaned.push_str(word);
    }
    let quotes: &[char] = &['"', '\'', '“', '”', '„', '‘', '’', '«', '»', '<', '>'];
    let cleaned = cleaned.trim_matches(|c: char| quotes.contains(&c) || c.is_whitespace());
    if cleaned.is_empty() || cleaned.contains('@') || !cleaned.chars().any(char::is_alphabetic) {
        return None;
    }
    let local = email.trim().rsplit_once('@').map_or("", |(l, _)| l);
    if !local.is_empty() && fold(cleaned) == fold(local) {
        return None;
    }
    let cut: String = cleaned.chars().take(MAX_NAME_CHARS).collect();
    Some(cut.trim_end().to_owned())
}

/// First and last name of a person.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonName {
    pub first: Option<String>,
    pub last: Option<String>,
}

/// Titles and salutations dropped before splitting (folded, without the dot).
const TITLES: &[&str] = &[
    "dr", "prof", "herr", "frau", "mr", "mrs", "ms", "mx", "miss", "dipl", "ing", "mag", "med",
    "rer", "nat", "phil", "jur", "sir", "dame",
];
/// Lower-case particles that belong to the last name (`von der Leyen`).
const PARTICLES: &[&str] = &[
    "von", "van", "der", "den", "de", "del", "della", "di", "da", "du", "zu", "zur", "le", "la",
    "ten", "ter", "bin", "al",
];

/// Splits a display name: `Hans Müller` and `Müller, Hans` give first `Hans`, last `Müller`;
/// titles (`Dr.`, `Herr`), additions in parentheses and after `|` or ` - ` (a company) are
/// dropped; lower-case particles stay with the last name (`Ursula von der Leyen`); a single word
/// is a first name. Middle names are left out.
pub fn split_name(display_name: &str) -> PersonName {
    let mut name = display_name.to_owned();
    // Additions: "(Firma GmbH)", "| Firma", " - Firma".
    while let (Some(open), Some(close)) = (name.find('('), name.find(')')) {
        if open >= close {
            break;
        }
        name.replace_range(open..=close, " ");
    }
    for separator in ["|", " - ", " – ", " / "] {
        if let Some(at) = name.find(separator) {
            name.truncate(at);
        }
    }
    let (first_part, last_part) = match name.split_once(',') {
        Some((last, first)) if !first.contains(',') && !first.trim().is_empty() => {
            (first.to_owned(), Some(last.to_owned()))
        }
        _ => (name.clone(), None),
    };
    let is_title = |word: &str| {
        let bare = fold(word).replace('.', "");
        let bare = bare.trim_matches('-');
        !bare.is_empty() && bare.split('-').all(|part| TITLES.contains(&part))
    };
    let words = |text: &str| -> Vec<String> {
        text.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
            .filter(|w| !w.is_empty() && !is_title(w) && w.chars().any(char::is_alphabetic))
            .map(str::to_owned)
            .collect()
    };
    let given = words(&first_part);
    if let Some(last) = last_part {
        let family = words(&last);
        return PersonName {
            first: given.first().cloned(),
            last: (!family.is_empty()).then(|| family.join(" ")),
        };
    }
    match given.len() {
        0 => PersonName::default(),
        1 => PersonName {
            first: given.first().cloned(),
            last: None,
        },
        _ => {
            let mut start = given.len() - 1;
            while start > 1 && PARTICLES.contains(&given[start - 1].as_str()) {
                start -= 1;
            }
            PersonName {
                first: given.first().cloned(),
                last: Some(given[start..].join(" ")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_are_cleaned() {
        let clean = |n: &str| clean_display_name(n, "hans.mueller@firma.example");
        assert_eq!(
            clean("  \"Hans   Müller\" ").as_deref(),
            Some("Hans Müller")
        );
        assert_eq!(clean("'Hans\tMüller'").as_deref(), Some("Hans Müller"));
        assert_eq!(clean("„Müller, Hans“").as_deref(), Some("Müller, Hans"));
        assert_eq!(clean("hans.mueller@firma.example"), None, "an address");
        assert_eq!(clean("'hans.mueller@firma.example'"), None);
        assert_eq!(clean("Hans.Mueller"), None, "the local part");
        assert_eq!(clean(""), None);
        assert_eq!(clean("   "), None);
        assert_eq!(clean("123 456"), None, "no letter");
        assert_eq!(clean("Hans\u{0}Müller").as_deref(), Some("Hans Müller"));
        assert_eq!(
            clean("Hans Mu\u{308}ller").as_deref(),
            Some("Hans Müller"),
            "NFC"
        );
        let long = "A".repeat(500);
        assert_eq!(
            clean(&long).map(|n| n.chars().count()),
            Some(MAX_NAME_CHARS)
        );
    }

    #[test]
    fn names_are_split() {
        let cases: &[(&str, Option<&str>, Option<&str>)] = &[
            ("Hans Müller", Some("Hans"), Some("Müller")),
            ("Müller, Hans", Some("Hans"), Some("Müller")),
            ("Dr. Hans Müller", Some("Hans"), Some("Müller")),
            (
                "Prof. Dr.-Ing. Erika Mustermann",
                Some("Erika"),
                Some("Mustermann"),
            ),
            ("Herr Hans Müller", Some("Hans"), Some("Müller")),
            ("Hans-Peter Müller", Some("Hans-Peter"), Some("Müller")),
            ("Hans Peter Müller", Some("Hans"), Some("Müller")),
            (
                "Ursula von der Leyen",
                Some("Ursula"),
                Some("von der Leyen"),
            ),
            ("Hans Müller (Firma GmbH)", Some("Hans"), Some("Müller")),
            ("Hans Müller | Firma GmbH", Some("Hans"), Some("Müller")),
            (
                "Erika Mustermann - Vertrieb",
                Some("Erika"),
                Some("Mustermann"),
            ),
            ("Carol", Some("Carol"), None),
            ("Zoë Ångström", Some("Zoë"), Some("Ångström")),
            ("王芳", Some("王芳"), None),
            ("", None, None),
            ("Dr.", None, None),
        ];
        for (name, first, last) in cases {
            let split = split_name(name);
            assert_eq!(split.first.as_deref(), *first, "{name}");
            assert_eq!(split.last.as_deref(), *last, "{name}");
        }
    }
}
