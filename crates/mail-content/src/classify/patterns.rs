//! Sender addresses that belong to programs or newsletters, by their local part and the labels
//! of their domain. Matching folds case and German umlauts (`ä` → `ae`, `ß` → `ss`) and ignores
//! separators and digits, so `No-Reply`, `no_reply2`, `nicht.antworten` and `NichtAntworten`
//! are the same.

/// What an address looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SenderPattern {
    /// Nobody reads it (`noreply@`, `nicht-antworten@`, `MAILER-DAEMON@`).
    NoReply(&'static str),
    /// A program (`notifications@`, `alerts@`, `rechnung@`).
    Automated(&'static str),
    /// A newsletter sender (`newsletter@`, `news@`, `angebote@`).
    Newsletter(&'static str),
}

/// Contained anywhere in the folded local part (or a domain label).
const NO_REPLY_PARTS: &[&str] = &[
    "noreply",
    "donotreply",
    "dontreply",
    "noresponse",
    "nichtantworten",
    "keineantwort",
    "nepasrepondre",
    "mailerdaemon",
];
/// Equal to one word of the local part.
const NO_REPLY_WORDS: &[&str] = &["postmaster", "bounce", "bounces"];

const AUTOMATED_WORDS: &[&str] = &[
    "notification",
    "notifications",
    "notify",
    "notifier",
    "alert",
    "alerts",
    "automated",
    "automail",
    "automailer",
    "autoconfirm",
    "autoresponder",
    "robot",
    "bot",
    "daemon",
    "cron",
    "system",
    "mailer",
    "monitoring",
    "monitor",
    "reminder",
    "reminders",
    "receipt",
    "receipts",
    "invoice",
    "invoices",
    "billing",
    "confirmation",
    "confirmations",
    "confirm",
    "order",
    "orders",
    "bestellung",
    "bestellungen",
    "versand",
    "shipping",
    "shipment",
    "tracking",
    "sendungsverfolgung",
    "security",
    "verify",
    "verification",
    "calendar",
    "kalender",
];
/// German compounds (`versandbestaetigung`, `onlinerechnung`): contained anywhere.
const AUTOMATED_PARTS: &[&str] = &["bestaetigung", "benachrichtigung", "rechnung", "erinnerung"];

const NEWSLETTER_WORDS: &[&str] = &[
    "newsletter",
    "newsletters",
    "news",
    "mailing",
    "mailings",
    "marketing",
    "promo",
    "promotion",
    "promotions",
    "angebot",
    "angebote",
    "deals",
    "offers",
    "werbung",
    "aktionen",
];

/// Domain labels left of the last two (`news` in `news.verlag.example`).
const NEWSLETTER_LABELS: &[&str] = &[
    "newsletter",
    "newsletters",
    "news",
    "mailing",
    "mailings",
    "marketing",
    "promo",
];
const AUTOMATED_LABELS: &[&str] = &[
    "notification",
    "notifications",
    "notify",
    "alert",
    "alerts",
    "bounce",
    "bounces",
];

/// The pattern of `email`, if any. No-reply wins over automated over newsletter; the local
/// part wins over the domain.
pub(crate) fn sender_pattern(email: &str) -> Option<SenderPattern> {
    let (local, domain) = email.trim().rsplit_once('@')?;
    let words = fold_words(local);
    let joined: String = words.concat();
    if let Some(p) = NO_REPLY_PARTS.iter().find(|p| joined.contains(**p)) {
        return Some(SenderPattern::NoReply(p));
    }
    if let Some(p) = NO_REPLY_WORDS
        .iter()
        .find(|p| words.iter().any(|w| w == *p))
    {
        return Some(SenderPattern::NoReply(p));
    }
    let labels: Vec<String> = domain
        .split('.')
        .map(|label| fold_words(label).concat())
        .collect();
    let left = &labels[..labels.len().saturating_sub(2)];
    if left.iter().any(|l| l == "noreply") {
        return Some(SenderPattern::NoReply("noreply"));
    }
    if let Some(p) = AUTOMATED_WORDS
        .iter()
        .find(|p| words.iter().any(|w| w == *p))
    {
        return Some(SenderPattern::Automated(p));
    }
    if let Some(p) = AUTOMATED_PARTS.iter().find(|p| joined.contains(**p)) {
        return Some(SenderPattern::Automated(p));
    }
    if joined.contains("newsletter") {
        return Some(SenderPattern::Newsletter("newsletter"));
    }
    if let Some(p) = NEWSLETTER_WORDS
        .iter()
        .find(|p| words.iter().any(|w| w == *p))
    {
        return Some(SenderPattern::Newsletter(p));
    }
    if let Some(p) = AUTOMATED_LABELS
        .iter()
        .find(|p| left.iter().any(|l| l == *p))
    {
        return Some(SenderPattern::Automated(p));
    }
    if let Some(p) = NEWSLETTER_LABELS
        .iter()
        .find(|p| left.iter().any(|l| l == *p))
    {
        return Some(SenderPattern::Newsletter(p));
    }
    None
}

/// An address nobody reads replies to (`noreply@`, `nicht-antworten@`, `MAILER-DAEMON@`,
/// `bounce@`): not offered as a contact, never a correspondent.
pub fn is_no_reply_address(email: &str) -> bool {
    matches!(sender_pattern(email), Some(SenderPattern::NoReply(_)))
}

/// Lower case, umlauts spelled out, split into words at everything but letters.
fn fold_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => word.push_str("ae"),
            'ö' => word.push_str("oe"),
            'ü' => word.push_str("ue"),
            'ß' => word.push_str("ss"),
            c if c.is_alphabetic() => word.push(c),
            _ => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::SenderPattern::{Automated, Newsletter, NoReply};
    use super::*;

    #[test]
    fn patterns() {
        let cases: &[(&str, Option<SenderPattern>)] = &[
            ("noreply@dhl.example", Some(NoReply("noreply"))),
            ("No-Reply@example.com", Some(NoReply("noreply"))),
            ("no_reply2@example.com", Some(NoReply("noreply"))),
            ("do-not-reply@bank.example", Some(NoReply("donotreply"))),
            ("alerts-noreply@google.com", Some(NoReply("noreply"))),
            (
                "nicht-antworten@kasse.example",
                Some(NoReply("nichtantworten")),
            ),
            (
                "bitte.nicht.antworten@amt.example",
                Some(NoReply("nichtantworten")),
            ),
            ("KeineAntwort@stadt.example", Some(NoReply("keineantwort"))),
            ("MAILER-DAEMON@example.net", Some(NoReply("mailerdaemon"))),
            ("postmaster@example.net", Some(NoReply("postmaster"))),
            ("bounce-123@mail.example", Some(NoReply("bounce"))),
            ("info@noreply.shop.example", Some(NoReply("noreply"))),
            ("notifications@github.com", Some(Automated("notifications"))),
            (
                "calendar-notification@google.com",
                Some(Automated("notification")),
            ),
            (
                "benachrichtigung@bank.example",
                Some(Automated("benachrichtigung")),
            ),
            (
                "versandbestaetigung@shop.example",
                Some(Automated("bestaetigung")),
            ),
            ("versand@paket.example", Some(Automated("versand"))),
            (
                "buchungsbestaetigung@bahn.example",
                Some(Automated("bestaetigung")),
            ),
            (
                "Bestellbestätigung@shop.example",
                Some(Automated("bestaetigung")),
            ),
            ("rechnung@stadtwerke.example", Some(Automated("rechnung"))),
            (
                "onlinerechnung@telefon.example",
                Some(Automated("rechnung")),
            ),
            ("auto-confirm@shop.example", Some(Automated("confirm"))),
            ("orders@shop.example", Some(Automated("orders"))),
            ("newsletter@zeitung.example", Some(Newsletter("newsletter"))),
            (
                "shop-newsletter@zeitung.example",
                Some(Newsletter("newsletter")),
            ),
            ("angebote@shop.example", Some(Newsletter("angebote"))),
            ("news@news.example", Some(Newsletter("news"))),
            (
                "info@newsletter.verlag.example",
                Some(Newsletter("newsletter")),
            ),
            (
                "hello@notifications.app.example",
                Some(Automated("notifications")),
            ),
            // People and ambiguous role addresses.
            ("carol@example.org", None),
            ("info@verein.example", None),
            ("service@bank.example", None),
            ("support@firma.example", None),
            ("jürgen@firma.example", None),
            ("nora.eply@example.org", None),
            ("broken", None),
            ("@", None),
        ];
        for (email, expected) in cases {
            assert_eq!(sender_pattern(email), *expected, "{email}");
        }
        assert!(is_no_reply_address("noreply@x.example"));
        assert!(!is_no_reply_address("notifications@x.example"));
    }
}
