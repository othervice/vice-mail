//! Stage 19, block C (decision 0056): contact groups ("Kontaktgruppen", distribution lists) and
//! vCard import and export of contacts and groups.
//!
//! A group has a name and member addresses in the order added. Members need no contact of the
//! directory (the directory is a cache of addresses from mail; a group is the user's). Groups
//! are synchronised like names and notes of contacts (record kind `contactList`).

use serde::{Deserialize, Serialize};

use crate::Address;

/// Most groups.
pub const MAX_CONTACT_LISTS: u32 = 500;

/// Most members of one group.
pub const MAX_CONTACT_LIST_MEMBERS: u32 = 1_000;

/// Longest group name, in characters.
pub const MAX_CONTACT_LIST_NAME_CHARS: u32 = 100;

/// From this many recipients in To and Cc on, the composer suggests Bcc after a group was
/// expanded (the recipients would see each other's addresses).
pub const MANY_RECIPIENTS: u32 = 10;

/// Most contacts one vCard export writes.
pub const MAX_VCARD_EXPORT_CONTACTS: u32 = 10_000;

/// A group's id: a lowercase hyphenated UUID, the same on every device.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct ContactListId(String);

impl ContactListId {
    /// `None` unless `text` is a hyphenated UUID (lower-cased).
    pub fn parse(text: &str) -> Option<Self> {
        let lower = text.to_ascii_lowercase();
        let shaped = lower.len() == 36
            && lower.char_indices().all(|(i, c)| match i {
                8 | 13 | 18 | 23 => c == '-',
                _ => c.is_ascii_hexdigit(),
            });
        shaped.then_some(Self(lower))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A group as lists show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactListSummary {
    pub id: ContactListId,
    pub name: String,
    pub member_count: u32,
}

/// One member of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactListMember {
    /// The address as it was added.
    pub email: String,
    /// The name given with it, else the contact's name; null without either.
    pub name: Option<String>,
    /// The contact of the directory with this address (its key), if there is one.
    pub contact: Option<String>,
}

/// A group with its members, in the order added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactList {
    pub id: ContactListId,
    pub name: String,
    pub members: Vec<ContactListMember>,
}

impl ContactList {
    /// The members as recipients (the composer expands a group into these).
    pub fn recipients(&self) -> Vec<Address> {
        self.members
            .iter()
            .map(|m| Address {
                name: m.name.clone(),
                email: m.email.clone(),
            })
            .collect()
    }
}

/// The vCard version an export writes. 3.0 is read by every address book (macOS Contacts,
/// Outlook, Google); 4.0 (RFC 6350) is what Thunderbird writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum VcardVersionChoice {
    #[default]
    V3,
    V4,
}

/// What a vCard export writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContactExport {
    /// Every contact of the view (people, or every contact) and every group.
    All { people_only: bool },
    /// One contact, by any of its addresses.
    Contact { email: String },
    /// One group: a card for the group and one for each member.
    List { id: ContactListId },
}

/// What an export wrote; `cancelled` when the user closed the save dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum VcardExportOutcome {
    Cancelled,
    Saved { path: String, cards: u32 },
}

/// Why a card of an imported file was left out or changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum VcardImportProblemKind {
    /// The file is larger than the reader takes; the rest was not read.
    TooLarge,
    /// More cards than the reader takes; the rest was not read.
    TooManyCards,
    /// A line or card could not be read.
    Malformed,
    /// A picture was too large, not an image the app shows, or a link (never loaded).
    PictureSkipped,
    /// A card without any usable address.
    NoAddress,
    /// A group would get more members than allowed, or there are too many groups.
    TooManyMembers,
}

/// One problem of an import, by the card's position in the file (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VcardImportProblem {
    pub card: u32,
    pub kind: VcardImportProblemKind,
}

/// What a vCard import did. Contacts whose addresses were known already are merged into
/// (`updated`): their name and note are only filled where empty, the counts stay.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VcardImportReport {
    /// Cards read from the file.
    pub cards: u32,
    /// New contacts.
    pub created: u32,
    /// Known contacts that got a name, a note, a picture or more addresses.
    pub updated: u32,
    /// Addresses merged into one contact (several addresses on a card, or a card naming
    /// addresses of different contacts).
    pub merged: u32,
    /// Groups created or extended.
    pub lists: u32,
    /// Pictures set (re-encoded by the helper, only where the contact had none).
    pub pictures: u32,
    /// Problems, at most 32.
    pub problems: Vec<VcardImportProblem>,
}

/// The outcome of "Import vCard…": cancelled in the dialog, or the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum VcardImportOutcome {
    Cancelled,
    Imported { report: VcardImportReport },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_ids_are_lowercase_uuids() {
        let id = ContactListId::parse("0B5E8F43-6C1F-4A51-9A51-0C4F5B3F6A01").expect("uuid");
        assert_eq!(id.as_str(), "0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01");
        assert!(ContactListId::parse("../x").is_none());
        assert!(ContactListId::parse("0b5e8f43x6c1f-4a51-9a51-0c4f5b3f6a01").is_none());
        assert!(ContactListId::parse("").is_none());
    }

    #[test]
    fn exports_and_outcomes_have_a_kind_on_the_wire() {
        let export: ContactExport =
            serde_json::from_value(serde_json::json!({ "kind": "all", "peopleOnly": true }))
                .expect("all");
        assert_eq!(export, ContactExport::All { people_only: true });
        let saved = serde_json::to_value(VcardExportOutcome::Saved {
            path: "/x.vcf".into(),
            cards: 2,
        })
        .expect("json");
        assert_eq!(
            saved,
            serde_json::json!({ "kind": "saved", "path": "/x.vcf", "cards": 2 })
        );
        assert_eq!(
            serde_json::to_value(VcardVersionChoice::V4).expect("json"),
            "v4"
        );
    }
}
