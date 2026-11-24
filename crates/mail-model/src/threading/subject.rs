//! Subject normalization for threads: the key under which replies and forwards of a subject
//! meet, and whether a subject is specific enough to thread by it at all.

/// Reply prefixes (lower case): English, German, Lotus Notes, Nordic, Dutch, Polish, Turkish,
/// Portuguese, Italian, Chinese.
const REPLY_PREFIXES: &[&str] = &[
    "re", "aw", "antw", "antwort", "sv", "vs", "odp", "ynt", "res", "rif", "回复", "回覆", "答复",
];
/// Forward prefixes (lower case): English, German, French, Spanish, Portuguese, Polish, Nordic,
/// Dutch, Chinese.
const FORWARD_PREFIXES: &[&str] = &[
    "fwd", "fw", "wg", "tr", "rv", "enc", "pd", "vb", "vl", "doorst", "转发", "轉寄",
];
/// At most this many prefixes and tags are removed (`Re: Re: AW: [x] …`).
const MAX_PREFIXES: usize = 32;
/// Longest `[tag]` removed.
const MAX_TAG_CHARS: usize = 40;

/// Subjects too common to say that two messages belong together (after normalization).
const GENERIC_SUBJECTS: &[&str] = &[
    "hallo",
    "hello",
    "hi",
    "hey",
    "moin",
    "servus",
    "test",
    "testmail",
    "test mail",
    "frage",
    "eine frage",
    "kurze frage",
    "question",
    "quick question",
    "info",
    "infos",
    "information",
    "informationen",
    "anfrage",
    "termin",
    "meeting",
    "besprechung",
    "call",
    "update",
    "status",
    "rechnung",
    "invoice",
    "angebot",
    "offer",
    "quote",
    "bestellung",
    "order",
    "auftrag",
    "danke",
    "vielen dank",
    "thanks",
    "thank you",
    "fyi",
    "zur info",
    "wichtig",
    "important",
    "dringend",
    "urgent",
    "kein betreff",
    "(kein betreff)",
    "no subject",
    "(no subject)",
    "hilfe",
    "help",
    "news",
    "newsletter",
    "nachricht",
    "message",
    "kontakt",
    "contact",
    "bewerbung",
    "unterlagen",
    "dokument",
    "dokumente",
    "document",
    "documents",
    "foto",
    "fotos",
    "photos",
    "pictures",
    "bilder",
    "scan",
    "gruß",
    "grüße",
    "greetings",
    "erinnerung",
    "reminder",
    "einladung",
    "invitation",
    "feedback",
    "notiz",
    "note",
    "todo",
    "neu",
    "new",
];
/// A key with a single word needs at least this many characters to be specific enough.
const MIN_SINGLE_WORD_CHARS: usize = 12;

/// A subject reduced to what replies and forwards share.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizedSubject {
    /// Lower case, reply/forward prefixes and `[list]` tags removed, whitespace collapsed.
    /// Empty if nothing is left.
    pub key: String,
    /// A reply prefix (`Re:`, `AW:`, …) was removed.
    pub reply: bool,
    /// A forward prefix (`Fwd:`, `WG:`, …) was removed.
    pub forward: bool,
}

/// Normalizes a subject: collapses whitespace and control characters, removes leading reply and
/// forward prefixes (also `Re[2]:`, `Re(2):`, `Re^2:`, `RE :` and a full-width colon) and
/// leading `[tags]` without digits (`[rust-users]`, `[EXTERN]`; `[Ticket #123]` stays, because
/// the number tells tickets apart), any number of times in any order, and lower-cases the rest.
/// Diacritics stay: `Grüße` and `Grusse` are different keys.
pub fn normalize_subject(subject: &str) -> NormalizedSubject {
    let collapsed = subject
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut rest = collapsed.as_str();
    let mut out = NormalizedSubject::default();
    for _ in 0..MAX_PREFIXES {
        rest = rest.trim_start();
        if let Some(after) = strip_tag(rest) {
            rest = after;
        } else if let Some((after, reply)) = strip_prefix(rest) {
            rest = after;
            if reply {
                out.reply = true;
            } else {
                out.forward = true;
            }
        } else {
            break;
        }
    }
    out.key = rest.trim().to_lowercase();
    out
}

/// The thread key of a subject; `None` if nothing is left after normalization.
pub fn subject_key(subject: &str) -> Option<String> {
    let key = normalize_subject(subject).key;
    (!key.is_empty()).then_some(key)
}

/// The subject a conversation row shows (stage 17, decision 0049, like Thunderbird): `subject`
/// without its leading reply and forward prefixes (`Re:`, `AW:`, `Fwd:`, `WG:`, `Re[2]:`, …, any
/// number of them, also `Re: [list] AW:` where a tag sits between them) and without surrounding
/// whitespace. Case, tags, inner whitespace and everything after the prefixes stay as they are;
/// what remains is the end of `subject` (after trimming), so ranges into `subject` still fit.
/// Empty if nothing but prefixes is left.
pub fn display_subject(subject: &str) -> &str {
    let mut rest = subject.trim();
    let mut shown = rest;
    for _ in 0..MAX_PREFIXES {
        rest = rest.trim_start();
        if let Some((after, _)) = strip_prefix(rest) {
            rest = after;
            shown = rest.trim_start();
        } else if let Some(after) = strip_tag(rest) {
            // A tag between prefixes stays only when no prefix follows it.
            rest = after;
        } else {
            break;
        }
    }
    shown
}

/// Whether two messages with this key may be threaded by subject alone: not empty, not in the
/// list of generic subjects (`hallo`, `rechnung`, `quick question`, …), and either two words or
/// one word of at least 12 characters.
pub fn is_specific_subject(key: &str) -> bool {
    if key.is_empty() || GENERIC_SUBJECTS.contains(&key) {
        return false;
    }
    let words = key
        .split(' ')
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count();
    words >= 2 || (words == 1 && key.chars().count() >= MIN_SINGLE_WORD_CHARS)
}

/// `[tag] rest` → `rest`, for a non-empty tag without digits.
fn strip_tag(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('[')?;
    let close = inner.find(']')?;
    let tag = &inner[..close];
    let ok = !tag.trim().is_empty()
        && tag.chars().count() <= MAX_TAG_CHARS
        && !tag.chars().any(|c| c.is_numeric() || c == '[');
    ok.then(|| &inner[close + 1..])
}

/// `Re: rest` → (`rest`, reply?). The word may carry a counter (`[2]`, `(2)`, `^2`) and spaces
/// before the colon.
fn strip_prefix(s: &str) -> Option<(&str, bool)> {
    let word_end = s
        .char_indices()
        .find(|&(_, c)| !c.is_alphabetic())
        .map_or(s.len(), |(i, _)| i);
    let word = s[..word_end].to_lowercase();
    let reply = if REPLY_PREFIXES.contains(&word.as_str()) {
        true
    } else if FORWARD_PREFIXES.contains(&word.as_str()) {
        false
    } else {
        return None;
    };
    let mut rest = &s[word_end..];
    rest = strip_counter(rest);
    rest = rest.trim_start_matches(' ');
    let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))?;
    Some((rest, reply))
}

fn strip_counter(s: &str) -> &str {
    for (open, close) in [("[", "]"), ("(", ")")] {
        if let Some(inner) = s.strip_prefix(open)
            && let Some(end) = inner.find(close)
            && end > 0
            && inner[..end].chars().all(|c| c.is_ascii_digit())
        {
            return &inner[end + 1..];
        }
    }
    if let Some(inner) = s.strip_prefix('^') {
        let digits = inner.len() - inner.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            return &inner[digits..];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_tags_whitespace_and_case() {
        // (subject, key, reply, forward)
        let cases: &[(&str, &str, bool, bool)] = &[
            (
                "Planning the 2026 offsite",
                "planning the 2026 offsite",
                false,
                false,
            ),
            (
                "Re: Planning the 2026 offsite",
                "planning the 2026 offsite",
                true,
                false,
            ),
            (
                "RE: planning THE 2026 Offsite",
                "planning the 2026 offsite",
                true,
                false,
            ),
            ("AW: Angebot", "angebot", true, false),
            ("Aw: AW: aw:Angebot", "angebot", true, false),
            ("Antw: Angebot", "angebot", true, false),
            ("Antwort: Angebot", "angebot", true, false),
            ("SV: Tilbud", "tilbud", true, false),
            ("VS: Tarjous", "tarjous", true, false),
            ("Odp: Oferta", "oferta", true, false),
            ("RES: Proposta", "proposta", true, false),
            ("Fwd: Angebot", "angebot", false, true),
            ("FW: Angebot", "angebot", false, true),
            ("WG: Angebot", "angebot", false, true),
            ("TR: Offre", "offre", false, true),
            ("RV: Oferta", "oferta", false, true),
            ("Fwd: Re: AW: WG: Angebot", "angebot", true, true),
            ("Re[2]: Angebot", "angebot", true, false),
            ("Re(3): Angebot", "angebot", true, false),
            ("Re^2: Angebot", "angebot", true, false),
            ("RE : Offre", "offre", true, false),
            ("回复：会议", "会议", true, false),
            ("转发: 会议", "会议", false, true),
            ("[rust-users] Async traits", "async traits", false, false),
            ("Re: [rust-users] Async traits", "async traits", true, false),
            ("[rust-users] Re: Async traits", "async traits", true, false),
            ("[EXTERN] AW: [Verein] Einladung", "einladung", true, false),
            (
                "[Ticket #2026112210000123] Drucker defekt",
                "[ticket #2026112210000123] drucker defekt",
                false,
                false,
            ),
            (
                "Re: [Ticket #123] Drucker",
                "[ticket #123] drucker",
                true,
                false,
            ),
            ("[] leer", "[] leer", false, false),
            ("[unclosed tag", "[unclosed tag", false, false),
            ("  Re:\tGrüße\n aus   Köln  ", "grüße aus köln", true, false),
            ("Grüße", "grüße", false, false),
            ("Re: Re: Re:", "", true, false),
            ("Re:", "", true, false),
            ("", "", false, false),
            ("Reply needed", "reply needed", false, false),
            ("Rechnung: November", "rechnung: november", false, false),
            ("Re-Design", "re-design", false, false),
            ("Re2: Angebot", "re2: angebot", false, false),
            ("Re[x]: Angebot", "re[x]: angebot", false, false),
            ("Weiterleitung", "weiterleitung", false, false),
            (
                "Abwesenheitsnotiz: Projekt Phönix",
                "abwesenheitsnotiz: projekt phönix",
                false,
                false,
            ),
        ];
        for (subject, key, reply, forward) in cases {
            let n = normalize_subject(subject);
            assert_eq!(
                (n.key.as_str(), n.reply, n.forward),
                (*key, *reply, *forward),
                "{subject:?}"
            );
        }
        assert_eq!(subject_key("Re: "), None);
        assert_eq!(subject_key("AW: Angebot").as_deref(), Some("angebot"));
        // Many prefixes stop after the limit without looping.
        let many = "Re: ".repeat(1000) + "x";
        assert!(normalize_subject(&many).key.starts_with("re:"));
    }

    #[test]
    fn display_subjects_lose_their_prefixes_only() {
        let cases: &[(&str, &str)] = &[
            ("Website relaunch", "Website relaunch"),
            ("Re: Website relaunch", "Website relaunch"),
            ("RE: Re: AW: Website  relaunch ", "Website  relaunch"),
            (
                "Fwd: Re: Planning the 2026 offsite",
                "Planning the 2026 offsite",
            ),
            ("WG: AW: Angebot", "Angebot"),
            ("Re[2]: Angebot", "Angebot"),
            ("RE : Offre", "Offre"),
            ("回复：会议", "会议"),
            ("  Re:\tGrüße aus Köln", "Grüße aus Köln"),
            // Tags stay; a prefix behind a tag goes with the tag only when another prefix follows.
            ("[rust-users] Async traits", "[rust-users] Async traits"),
            ("Re: [rust-users] Async traits", "[rust-users] Async traits"),
            ("Re: [rust-users] Re: Async traits", "Async traits"),
            ("[rust-users] Re: Async traits", "Async traits"),
            ("Re: [Ticket #123] Drucker", "[Ticket #123] Drucker"),
            (
                "Re: Planning the 2026 offsite (was: venue shortlist)",
                "Planning the 2026 offsite (was: venue shortlist)",
            ),
            ("Reply needed", "Reply needed"),
            ("Re-Design", "Re-Design"),
            ("Rechnung: November", "Rechnung: November"),
            ("Re:", ""),
            ("Re: Re: ", ""),
            ("", ""),
        ];
        for (subject, shown) in cases {
            let got = display_subject(subject);
            assert_eq!(got, *shown, "{subject:?}");
            // Always the end of the trimmed subject: ranges into the subject still fit.
            assert!(subject.trim_end().ends_with(got), "{subject:?}");
        }
        let many = "Re: ".repeat(1000) + "x";
        assert!(display_subject(&many).starts_with("Re:"));
    }

    #[test]
    fn specific_subjects() {
        for key in [
            "planning the 2026 offsite",
            "rechnung november",
            "angebot projekt phönix",
            "projektbesprechung",
            "die steuererklärung",
        ] {
            assert!(is_specific_subject(key), "{key}");
        }
        for key in [
            "",
            "hallo",
            "rechnung",
            "quick question",
            "kurze frage",
            "(kein betreff)",
            "angebot",
            "termin",
            "status",
            "projekt",
            "q3",
            "!!! !!!",
            "12345",
        ] {
            assert!(!is_specific_subject(key), "{key}");
        }
    }
}
