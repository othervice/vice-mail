//! Identifiers and IMAP numbers.
//!
//! Bridge rules (decision 0004): local row ids, UIDs and timestamps stay far below 2^53 and cross
//! the bridge as TypeScript `number`; the account id is a UUID string. [`ModSeq`] can reach
//! 2^63 - 1 and is not exported in stage 2; if it ever is, it must go as a decimal string.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::CoreError;

/// Stable account id (UUID v4, lowercase hyphenated on the wire). It is the same on every
/// device, so synced records refer to accounts by it, never by a local row id.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct AccountId(#[specta(type = String)] uuid::Uuid);

impl AccountId {
    /// Wraps a UUID. Callers generate it (the engine uses `uuid::Uuid::new_v4`); this crate has
    /// no randomness because it does no IO.
    pub const fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    pub const fn as_uuid(&self) -> &uuid::Uuid {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for AccountId {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s)
            .map(Self)
            .map_err(|err| CoreError::InvalidArgument(format!("account id {s:?}: {err}")))
    }
}

/// Local row id of a mailbox (`mailbox.id`). Not stable across devices.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct MailboxId(#[specta(type = Number)] pub i64);

/// Local id of a message (`message.id`): its sort date in Unix seconds, or the next free number
/// above it (decision 0003). Larger is newer. Below 2^33, so exact as a JavaScript number.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct MessageId(#[specta(type = Number)] pub i64);

/// Local row id of an identity (`identity.id`). Not exported in stage 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IdentityId(pub i64);

/// Keyset cursor of a message list: pass `nextCursor` of one page back to get the next page.
/// Opaque to the UI; today it is the [`MessageId`] of the last item of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct ListCursor(#[specta(type = Number)] pub i64);

impl ListCursor {
    /// Cursor that continues after (that is, below) `last`.
    pub const fn after(last: MessageId) -> Self {
        Self(last.0)
    }

    /// The list continues with messages whose id is smaller than this.
    pub const fn before(self) -> MessageId {
        MessageId(self.0)
    }
}

/// IMAP UID: 1 to 2^32 - 1, valid together with the mailbox's [`UidValidity`].
pub type Uid = u32;

/// IMAP UIDVALIDITY of a mailbox. When it changes, all stored UIDs of the mailbox are void.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UidValidity(pub u32);

/// CONDSTORE mod-sequence (RFC 7162): 1 to 2^63 - 1. Stored as SQLite INTEGER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModSeq(pub u64);

impl fmt::Display for UidValidity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for ModSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for MailboxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn account_id_is_a_lowercase_uuid_string() {
        let id: AccountId = "6F9619FF-8B86-4D11-B42D-00C04FC964FF"
            .parse()
            .expect("parse");
        assert_eq!(id.to_string(), "6f9619ff-8b86-4d11-b42d-00c04fc964ff");
        assert_eq!(
            serde_json::to_value(id).expect("serialize"),
            json!("6f9619ff-8b86-4d11-b42d-00c04fc964ff")
        );
        let back: AccountId =
            serde_json::from_value(json!("6f9619ff-8b86-4d11-b42d-00c04fc964ff")).expect("de");
        assert_eq!(back, id);

        let err = "not-a-uuid".parse::<AccountId>().expect_err("invalid");
        assert_eq!(err.code(), crate::ErrorCode::InvalidArgument);
    }

    #[test]
    fn numeric_ids_are_plain_numbers() {
        assert_eq!(
            serde_json::to_value(MessageId(1_764_000_000)).expect("serialize"),
            json!(1_764_000_000)
        );
        assert_eq!(
            serde_json::to_value(MailboxId(7)).expect("serialize"),
            json!(7)
        );
        let cursor = ListCursor::after(MessageId(42));
        assert_eq!(serde_json::to_value(cursor).expect("serialize"), json!(42));
        assert_eq!(cursor.before(), MessageId(42));
    }
}
