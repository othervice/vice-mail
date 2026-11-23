//! Statements from a model's answer, each tied to the messages it is based on. A statement
//! whose references do not name a message that was actually sent is dropped: the model cannot
//! make the interface point at a message it never saw, or at one that does not exist.

use mail_model::MessageId;

use crate::prepare::SentMail;
use crate::text::clean_output;

/// Longest statement text, in characters.
pub(crate) const MAX_CLAIM_CHARS: usize = 600;

/// One statement and its sources (in the order the model gave them, without duplicates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub text: String,
    pub sources: Vec<MessageId>,
}

/// The message ids behind `refs`. Accepts `m1`, `M1`, `[m1]`, `#m1` and the number `1`;
/// anything that names no sent message is ignored.
pub(crate) fn resolve_refs(refs: Option<&serde_json::Value>, sent: &[SentMail]) -> Vec<MessageId> {
    let Some(serde_json::Value::Array(refs)) = refs else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for value in refs {
        let reference = match value {
            serde_json::Value::String(text) => normalize(text),
            serde_json::Value::Number(number) => number.as_u64().map(|n| format!("m{n}")),
            _ => None,
        };
        let Some(reference) = reference else { continue };
        if let Some(mail) = sent.iter().find(|mail| mail.reference == reference)
            && !ids.contains(&mail.id)
        {
            ids.push(mail.id);
        }
    }
    ids
}

fn normalize(text: &str) -> Option<String> {
    let trimmed = text
        .trim()
        .trim_matches(|c| matches!(c, '[' | ']' | '(' | ')' | '#'))
        .trim()
        .to_ascii_lowercase();
    let digits = trimmed.strip_prefix('m').unwrap_or(&trimmed);
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("m{}", digits.trim_start_matches('0')))
}

/// A statement from `item` (`{"text": …, "refs": […]}`), or `None` if it has no text or no
/// valid reference.
pub(crate) fn claim(item: &serde_json::Value, sent: &[SentMail]) -> Option<Claim> {
    let text = clean_output(item.get("text")?.as_str()?, MAX_CLAIM_CHARS);
    if text.is_empty() {
        return None;
    }
    let sources = resolve_refs(item.get("refs"), sent);
    (!sources.is_empty()).then_some(Claim { text, sources })
}

/// The items of the array `key` (missing or not an array: none).
pub(crate) fn items<'a>(json: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    json.get(key)
        .and_then(serde_json::Value::as_array)
        .map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_model::AccountId;
    use serde_json::json;

    fn sent(n: usize) -> Vec<SentMail> {
        let account: AccountId = "00000000-0000-4000-8000-000000000001"
            .parse()
            .expect("account");
        (1..=n)
            .map(|i| SentMail {
                reference: format!("m{i}"),
                id: MessageId(100 + i as i64),
                account,
            })
            .collect()
    }

    #[test]
    fn references_are_checked_against_what_was_sent() {
        let sent = sent(3);
        let refs = json!([
            "m2", "M1", "[m3]", "m2", "m9", "m0", "x", 3, -1, null, "m01"
        ]);
        assert_eq!(
            resolve_refs(Some(&refs), &sent),
            vec![MessageId(102), MessageId(101), MessageId(103)]
        );
        assert!(resolve_refs(Some(&json!("m1")), &sent).is_empty());
        assert!(resolve_refs(None, &sent).is_empty());
    }

    #[test]
    fn claims_without_valid_reference_or_text_are_dropped() {
        let sent = sent(2);
        assert_eq!(
            claim(
                &json!({"text": " Budget due  Friday ", "refs": ["m2"]}),
                &sent
            ),
            Some(Claim {
                text: "Budget due Friday".into(),
                sources: vec![MessageId(102)]
            })
        );
        assert_eq!(
            claim(&json!({"text": "Invented", "refs": ["m7"]}), &sent),
            None
        );
        assert_eq!(claim(&json!({"text": "No refs"}), &sent), None);
        assert_eq!(claim(&json!({"text": "", "refs": ["m1"]}), &sent), None);
        assert_eq!(claim(&json!({"refs": ["m1"]}), &sent), None);
        assert_eq!(claim(&json!("just a string"), &sent), None);
    }
}
