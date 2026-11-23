//! Inbox categories without AI (stage 9, decision 0016): the category of a message, the reason
//! the UI shows under "Why here?", the user's corrections per sender, and the senders marked
//! important (the "Priority" block). Types only; the rules live in `mail_content::classify`.
//!
//! Stored values use [`MessageCategory::as_str`] and [`SenderKeyKind::as_str`]; a
//! [`CategoryReason`] is stored as JSON (serde), so the "Why here?" text can be shown for old
//! messages without classifying them again.

use serde::{Deserialize, Serialize};

/// The category of a message in the inbox.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum MessageCategory {
    /// Written by a person (the default when nothing points to a program or a list).
    Person,
    /// Written by a program for the user: notifications, confirmations, automatic replies,
    /// delivery reports.
    Notification,
    /// Mass mail: newsletters, mailing lists, marketing.
    Newsletter,
}

impl MessageCategory {
    pub const ALL: [Self; 3] = [Self::Person, Self::Notification, Self::Newsletter];

    /// Stable name for the database and settings sync.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Notification => "notification",
            Self::Newsletter => "newsletter",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }

    /// Rows of this category are folded into bundle rows in the inbox (`docs/DESIGN.md`).
    pub const fn is_bundled(self) -> bool {
        !matches!(self, Self::Person)
    }
}

/// What a correction or an important mark applies to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum SenderKeyKind {
    /// One sender address.
    Address,
    /// A domain and all its subdomains.
    Domain,
    /// A mailing list, by the identifier of its `List-Id`.
    ListId,
}

impl SenderKeyKind {
    pub const ALL: [Self; 3] = [Self::Address, Self::Domain, Self::ListId];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::Domain => "domain",
            Self::ListId => "list_id",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A sender, domain or list a user rule applies to. `value` as the user chose it; the
/// classifier normalises it (lower case, ASCII domain) before comparing.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub struct SenderKey {
    pub kind: SenderKeyKind,
    pub value: String,
}

impl SenderKey {
    pub fn address(value: impl Into<String>) -> Self {
        Self {
            kind: SenderKeyKind::Address,
            value: value.into(),
        }
    }

    pub fn domain(value: impl Into<String>) -> Self {
        Self {
            kind: SenderKeyKind::Domain,
            value: value.into(),
        }
    }

    pub fn list_id(value: impl Into<String>) -> Self {
        Self {
            kind: SenderKeyKind::ListId,
            value: value.into(),
        }
    }
}

/// "Always as …": the user's correction for a sender, domain or list. Stored as a rule (sync).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCorrection {
    pub key: SenderKey,
    pub category: MessageCategory,
}

/// Why a message got its category: the first rule that matched (decision 0016 lists the order).
/// Values are copied from the message (header values cut to a few dozen characters).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CategoryReason {
    /// The user's correction ("always as …") for this sender, list or domain.
    UserRule { key: SenderKey },
    /// `Auto-Submitted` other than `no` (RFC 3834): written by a program.
    AutoSubmitted { value: String },
    /// An automatic reply (vacation, out of office): `Precedence: auto_reply`, `X-Autoreply`,
    /// `X-Autorespond` or a subject such as "Automatic reply:" / "Automatische Antwort:".
    AutoReply { evidence: String },
    /// A delivery, read or other report (`multipart/report`, `MAILER-DAEMON`, empty
    /// `Return-Path`).
    Report { evidence: String },
    /// A header only a known system writes (`X-GitHub-Reason`, `X-GitLab-Project`, ...).
    KnownSystem { system: String, header: String },
    /// Sent from one of the user's own addresses.
    OwnMessage,
    /// Answers a message the user sent.
    ReplyToOwn,
    /// A mailing list the members write to (`List-Post`).
    DiscussionList { list_id: Option<String> },
    /// A mailing list (`List-Id`).
    MailingList { list_id: String },
    /// `Precedence: bulk`, `list` or `junk`.
    Precedence { value: String },
    /// Newsletter software or a campaign header (`X-CSA-Complaints`, `X-Campaign`, the
    /// `X-Mailer` of a newsletter tool, a marketing service's bounce address).
    Campaign { evidence: String },
    /// The user has written to this sender before.
    Correspondent { sent: u32 },
    /// Offers unsubscribing (`List-Unsubscribe`) and nothing points to a person.
    Unsubscribe,
    /// Sent through a mailing service (`Feedback-ID`, a service's bounce address) and offers no
    /// unsubscribing: a transactional mail.
    MailingService { evidence: String },
    /// An address nobody reads (`noreply@`, `nicht-antworten@`).
    NoReplyAddress { address: String },
    /// An address of a program (`notifications@`, `alerts@`, `rechnung@`).
    AutomatedAddress { address: String, pattern: String },
    /// An address that sends newsletters (`newsletter@`, `news@`).
    NewsletterAddress { address: String, pattern: String },
    /// Nothing points to a program or a list.
    NoBulkSignals,
    /// Stage 9: a local rule set the category of this message (its name at the time).
    LocalRule { rule_name: String },
}

impl MessageCategory {
    /// Stage 9: the number stored in `message.category` (0 person, 1 notification,
    /// 2 newsletter).
    pub const fn code(self) -> i64 {
        match self {
            Self::Person => 0,
            Self::Notification => 1,
            Self::Newsletter => 2,
        }
    }

    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Person),
            1 => Some(Self::Notification),
            2 => Some(Self::Newsletter),
            _ => None,
        }
    }
}

/// The category of a message, why, and whether it belongs to the "Priority" block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDecision {
    pub category: MessageCategory,
    pub reason: CategoryReason,
    /// The important mark that matched (sender, domain or list); `None` outside "Priority".
    pub important: Option<SenderKey>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn names_round_trip() {
        for category in MessageCategory::ALL {
            assert_eq!(MessageCategory::parse(category.as_str()), Some(category));
        }
        for kind in SenderKeyKind::ALL {
            assert_eq!(SenderKeyKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(MessageCategory::parse("Person"), None);
        assert!(!MessageCategory::Person.is_bundled());
        assert!(MessageCategory::Newsletter.is_bundled());
    }

    #[test]
    fn wire_format() {
        let decision = CategoryDecision {
            category: MessageCategory::Notification,
            reason: CategoryReason::KnownSystem {
                system: "GitHub".into(),
                header: "X-GitHub-Reason".into(),
            },
            important: Some(SenderKey::list_id("repo.owner.github.com")),
        };
        let value = serde_json::to_value(&decision).expect("json");
        assert_eq!(
            value,
            json!({
                "category": "notification",
                "reason": { "kind": "knownSystem", "system": "GitHub", "header": "X-GitHub-Reason" },
                "important": { "kind": "list_id", "value": "repo.owner.github.com" }
            })
        );
        let back: CategoryDecision = serde_json::from_value(value).expect("back");
        assert_eq!(back, decision);
        let reason =
            serde_json::to_value(CategoryReason::DiscussionList { list_id: None }).expect("json");
        assert_eq!(reason, json!({ "kind": "discussionList", "listId": null }));
    }
}
