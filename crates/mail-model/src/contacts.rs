//! The contact directory for the UI (stage 10, decision 0023): ranked listing, names the user
//! edits, duplicate suggestions and merges the user confirmed.
//!
//! A contact is one address, or several the user merged (they share a group; the address of
//! the contact that was kept is the preferred one). Contacts come from sent and received mail
//! (decision 0016); addresses nobody reads (`noreply@` …) are never listed.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::ErrorCode;

/// Most contacts of one page of [`ContactPage`].
pub const MAX_CONTACT_PAGE: u32 = 200;

/// Most contacts the duplicate check reads (the highest ranked).
pub const MAX_DUPLICATE_SCAN: u32 = 20_000;

/// One address of a contact and how it was used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactAddressInfo {
    /// The address to write to (local part as last written, Unicode domain).
    pub email: String,
    /// Messages the user sent to it.
    pub sent: u32,
    /// Messages received from it (only from people, not newsletters or notifications).
    pub received: u32,
    /// Last message to or from it, Unix ms.
    #[specta(type = Option<Number>)]
    pub last_used: Option<i64>,
}

/// A contact of the directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactEntry {
    /// Normalised address of the preferred address: the contact's id in every command.
    pub key: String,
    /// The name to show: the user's, else the most used one seen in mail.
    pub name: Option<String>,
    /// The name the user gave (synchronised between devices).
    pub user_name: Option<String>,
    /// The preferred address to write to.
    pub email: String,
    /// Every address, the preferred one first.
    pub addresses: Vec<ContactAddressInfo>,
    /// Over all addresses.
    pub sent: u32,
    pub received: u32,
    /// Unix ms.
    #[specta(type = Option<Number>)]
    pub last_used: Option<i64>,
}

/// One page of the directory, highest ranked first (frequency and recency in one score:
/// sent mail counts four times a received one, and every 30 days a message counts half).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactPage {
    pub items: Vec<ContactEntry>,
    /// Pass to `listContacts` for the next page; `null` at the end. Opaque.
    pub next: Option<String>,
}

/// Why two contacts may be the same person (or one contact has several names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateReason {
    /// `alice+shop@x` and `alice@x`.
    PlusAddress,
    /// The same mailbox at the provider (Gmail ignores dots; `googlemail.com`).
    SameMailbox,
    /// The same name at two addresses (`name`).
    SameName,
    /// One address was seen with different names (`names`); the user may pick one with
    /// `setContactName`. `contacts` has one entry.
    NamesDiffer,
}

/// A possible duplicate. Nothing is merged unless the user confirms (`mergeContacts` with
/// `confirmed: true`); `dismissDuplicate` hides it for good.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactDuplicate {
    pub reason: DuplicateReason,
    /// The shared name (`sameName`).
    pub name: Option<String>,
    /// The names seen (`namesDiffer`).
    pub names: Vec<String>,
    /// Two contacts, or one for `namesDiffer`.
    pub contacts: Vec<ContactEntry>,
}

/// Merging contacts the user confirmed as one person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactMergeRequest {
    /// Any address of the contact that stays; its preferred address stays preferred.
    pub keep: String,
    /// Any address of each contact merged into it.
    pub merge: Vec<String>,
    /// Must be `true`: the user confirmed the merge. Without it nothing changes
    /// (`confirmation_required`).
    pub confirmed: bool,
}

/// Why a contact request was refused; nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContactRequestError {
    /// No contact has this address.
    #[error("no contact with the address {0:?}")]
    NotFound(String),
    /// A merge without the user's confirmation.
    #[error("merging contacts needs the user's confirmation")]
    NotConfirmed,
    /// The addresses belong to one contact already (or nothing to merge).
    #[error("the addresses belong to the same contact")]
    SameContact,
    /// Not a usable name: empty, an address, control characters.
    #[error("not a usable name")]
    InvalidName,
    /// The address is not merged with another one.
    #[error("{0:?} is not merged with another address")]
    NotMerged(String),
}

impl ContactRequestError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::NotConfirmed => ErrorCode::ConfirmationRequired,
            Self::SameContact | Self::InvalidName | Self::NotMerged(_) => {
                ErrorCode::InvalidArgument
            }
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "The contact no longer exists.",
            Self::NotConfirmed => "Merging contacts needs your confirmation.",
            Self::SameContact => "These addresses already belong to one contact.",
            Self::InvalidName => "Enter a name (not an email address).",
            Self::NotMerged(_) => "This address is not merged with another one.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        Some(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format() {
        assert_eq!(
            serde_json::to_value(DuplicateReason::NamesDiffer).expect("reason"),
            json!("names_differ")
        );
        let request: ContactMergeRequest = serde_json::from_value(json!({
            "keep": "a@x", "merge": ["b@x"], "confirmed": false
        }))
        .expect("request");
        assert!(!request.confirmed);
        assert_eq!(
            ContactRequestError::NotConfirmed.code(),
            ErrorCode::ConfirmationRequired
        );
    }
}
