//! A `mailto:` link (RFC 6068) as a request to open the composer (stage 9, "default mail app").
//!
//! `mail_content::parse_mailto` turns the link into a [`ComposeRequest`]; the engine sends it to
//! the UI, which opens a new draft with these fields. The request only fills a draft: nothing is
//! sent without the user, and the [`MailtoNotice`]s tell the user what the link tried to do that
//! was not applied or needs a look before sending (blind copies above all).

use serde::{Deserialize, Serialize};

use crate::Address;

/// The fields a `mailto:` link fills in a new draft. Recipients are checked and unique across
/// To, Cc and Bcc (the address as it goes into the message: local part as written, domain in
/// ASCII); `subject` is one line; `body` is plain text with `\n` line breaks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposeRequest {
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: Option<String>,
    pub body: Option<String>,
    /// Message-ID of the message this answers, without angle brackets.
    pub in_reply_to: Option<String>,
    /// What the user should know before sending; shown as hints in the composer.
    pub notices: Vec<MailtoNotice>,
}

/// What `openLink` did (stage 10): opened the link in the system (`http`, `https`), or, for a
/// `mailto:` link in a mail, nothing yet: the UI opens its own composer with the request
/// (whether or not this app is the system's default mail app).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LinkOpened {
    Opened,
    Compose { request: ComposeRequest },
}

/// A field of a `mailto:` link that a [`MailtoNotice`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MailtoField {
    To,
    Cc,
    Bcc,
    Subject,
    Body,
    InReplyTo,
}

/// Something about a `mailto:` link the composer shows as a hint. Values are copied from the
/// link (decoded, on one line, cut to a few hundred characters).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MailtoNotice {
    /// The link adds blind copies: the user must see them before sending. Always the first
    /// notice when present.
    BccPresent,
    /// A header field outside the allowlist (`to`, `cc`, `bcc`, `subject`, `body`,
    /// `in-reply-to`), for example `from`, `reply-to` or `x-mailer`. Never applied; `name` in
    /// lower case.
    HeaderIgnored { name: String },
    /// `subject`, `body` or `in-reply-to` came more than once; only the first usable one is
    /// used.
    DuplicateIgnored { field: MailtoField },
    /// Not a usable address (left out). Quoted local parts (`"not@me"@example.org`) are
    /// decoded but not supported by the composer, so they end up here too.
    AddressInvalid { value: String },
    /// `in-reply-to` is not one valid Message-ID (left out).
    MessageIdInvalid { value: String },
    /// The field was over its limit and was cut (subject, body) or recipients were left out
    /// (To, Cc, Bcc: more than `MAX_RECIPIENTS` together).
    Truncated { field: MailtoField },
    /// Percent-encoded bytes were not valid UTF-8; they were replaced with U+FFFD.
    InvalidEncoding { field: MailtoField },
    /// Control characters were removed: line breaks in the subject became spaces, characters
    /// other than line breaks and tabs were dropped from the body.
    ControlCharactersRemoved { field: MailtoField },
    /// More hints of the kinds above that carry a value than are listed.
    NoticesOmitted { count: u32 },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format() {
        let request = ComposeRequest {
            to: vec![Address {
                name: Some("Joe".into()),
                email: "joe@example.com".into(),
            }],
            cc: Vec::new(),
            bcc: vec![Address {
                name: None,
                email: "boss@example.com".into(),
            }],
            subject: Some("café".into()),
            body: None,
            in_reply_to: Some("3469A91.D10AF4C@example.com".into()),
            notices: vec![
                MailtoNotice::BccPresent,
                MailtoNotice::HeaderIgnored {
                    name: "reply-to".into(),
                },
                MailtoNotice::Truncated {
                    field: MailtoField::InReplyTo,
                },
                MailtoNotice::NoticesOmitted { count: 3 },
            ],
        };
        let value = serde_json::to_value(&request).expect("json");
        assert_eq!(
            value,
            json!({
                "to": [{ "name": "Joe", "email": "joe@example.com" }],
                "cc": [],
                "bcc": [{ "name": null, "email": "boss@example.com" }],
                "subject": "café",
                "body": null,
                "inReplyTo": "3469A91.D10AF4C@example.com",
                "notices": [
                    { "kind": "bcc_present" },
                    { "kind": "header_ignored", "name": "reply-to" },
                    { "kind": "truncated", "field": "in_reply_to" },
                    { "kind": "notices_omitted", "count": 3 }
                ]
            })
        );
        let back: ComposeRequest = serde_json::from_value(value).expect("back");
        assert_eq!(back, request);
        assert_eq!(
            serde_json::to_value(MailtoNotice::ControlCharactersRemoved {
                field: MailtoField::Subject
            })
            .expect("json"),
            json!({ "kind": "control_characters_removed", "field": "subject" })
        );
    }
}
