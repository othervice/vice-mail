//! Stage 19 (decision 0055): a message's source ("view source", in a window of its own), its
//! header fields ("all headers" in the reading pane), redirecting a message with `Resent-*`
//! fields, and editing a message as a new one.

use serde::{Deserialize, Serialize};

use crate::{AccountId, Address, IdentityId, MessageId};

/// Most bytes of a message the source window shows; a larger message is cut at a line break
/// and offers "Save as .eml" for the whole.
pub const MAX_SOURCE_BYTES: u32 = 1024 * 1024;

/// Most header fields "all headers" lists.
pub const MAX_HEADER_FIELDS: u32 = 500;

/// Header values are cut to this many characters.
pub const MAX_HEADER_VALUE_CHARS: u32 = 8 * 1024;

/// Most recipients of one redirect (To, Cc and Bcc together).
pub const MAX_REDIRECT_RECIPIENTS: u32 = 100;

/// The source window's content: the message as plain text, never markup (control and direction
/// characters shown as visible symbols, bytes that are not UTF-8 as U+FFFD).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageSource {
    pub account_id: AccountId,
    pub message_id: MessageId,
    /// For the window's heading; empty without a subject.
    pub subject: String,
    pub text: String,
    /// Size of the whole message in bytes.
    pub total_bytes: u32,
    /// Bytes `text` shows: the whole message unless `truncated`.
    pub shown_bytes: u32,
    pub truncated: bool,
}

/// One header field as the reading pane's "all headers" lists it: the name as written, the
/// value unfolded with encoded words decoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

/// "Redirect": the message unchanged to new recipients, with `Resent-*` fields, from one of the
/// account's identities (its default, or the identity the message went to, without
/// `identityId`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RedirectRequest {
    pub message_id: MessageId,
    pub identity_id: Option<IdentityId>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_requests_use_camel_case_on_the_wire() {
        let request: RedirectRequest = serde_json::from_value(serde_json::json!({
            "messageId": 7,
            "identityId": null,
            "to": [{ "name": null, "email": "carol@example.net" }],
            "cc": [],
            "bcc": [],
        }))
        .expect("request");
        assert_eq!(request.message_id, MessageId(7));
        assert_eq!(request.to[0].email, "carol@example.net");
        let source = MessageSource {
            account_id: "6f9619ff-8b86-4011-b42d-00c04fc964ff".parse().expect("id"),
            message_id: MessageId(7),
            subject: String::new(),
            text: "x".into(),
            total_bytes: 1,
            shown_bytes: 1,
            truncated: false,
        };
        let json = serde_json::to_value(&source).expect("json");
        assert_eq!(json["totalBytes"], 1);
        assert_eq!(json["shownBytes"], 1);
        assert_eq!(json["accountId"], "6f9619ff-8b86-4011-b42d-00c04fc964ff");
    }
}
