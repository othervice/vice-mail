//! The contact directory for the UI (stage 10, decision 0023): ranked listing, names the user
//! edits, duplicate suggestions and merges the user confirmed.
//!
//! A contact is one address, or several the user merged (they share a group; the address of
//! the contact that was kept is the preferred one). Contacts come from sent and received mail
//! (decision 0016). Since stage 17 every address of From, To, Cc and Reply-To becomes one;
//! list and system addresses (newsletters, `noreply@`, `MAILER-DAEMON@`) are marked
//! ([`ContactKind`]) and can be left out. A contact can have a note and a picture the user
//! chose, and can be marked as spam (its addresses are blocked senders, stage 16).

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{AccountId, BatchId, ErrorCode};

/// Most contacts of one page of [`ContactPage`].
pub const MAX_CONTACT_PAGE: u32 = 200;

/// Most contacts the duplicate check reads (the highest ranked).
pub const MAX_DUPLICATE_SCAN: u32 = 20_000;

/// Stage 17: longest note of a contact, in characters.
pub const MAX_CONTACT_NOTE_CHARS: usize = 1_000;

/// Stage 17: largest image file read for a contact picture.
pub const MAX_CONTACT_PICTURE_FILE_BYTES: u32 = 20 * 1024 * 1024;

/// Stage 17: width and height of a stored contact picture in pixels (a square JPEG).
pub const CONTACT_PICTURE_SIZE: u32 = 256;

/// Stage 17: largest stored contact picture (JPEG bytes); also what one sync record carries.
pub const MAX_CONTACT_PICTURE_BYTES: usize = 40 * 1024;

/// Stage 17: longest side of the image the cropper shows.
pub const CONTACT_PICTURE_SOURCE_SIZE: u32 = 1_024;

/// Stage 17: most recent messages [`ContactMessages`] lists.
pub const MAX_CONTACT_MESSAGES: u32 = 50;

/// What kind of correspondent an address is (stage 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ContactKind {
    /// A person: the user wrote to it, or it was seen in mail from a person.
    Person,
    /// Seen only in newsletters, notifications or list mail, or a newsletter address.
    List,
    /// A program: nobody reads it (`noreply@`, `MAILER-DAEMON@`), or it sends notifications.
    System,
}

impl ContactKind {
    /// The stored number (`contact.kind`).
    pub const fn code(self) -> i64 {
        match self {
            Self::Person => 0,
            Self::List => 1,
            Self::System => 2,
        }
    }

    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Person),
            1 => Some(Self::List),
            2 => Some(Self::System),
            _ => None,
        }
    }
}

/// One address of a contact and how it was used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactAddressInfo {
    /// The address to write to (local part as last written, Unicode domain).
    pub email: String,
    /// Messages the user sent to it.
    pub sent: u32,
    /// Messages received from it (stage 17: also newsletters and notifications).
    pub received: u32,
    /// Stage 17: other messages it was in (copied, or as Reply-To).
    pub seen: u32,
    /// Last message to, from or with it, Unix ms.
    #[specta(type = Option<Number>)]
    pub last_used: Option<i64>,
    /// Stage 17: new mail from it is blocked (marked as spam, or blocked in stage 16).
    pub blocked: bool,
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
    /// Stage 17: person, list or system address (of the preferred address).
    pub kind: ContactKind,
    /// Stage 17: the note the user wrote (synchronised).
    pub note: Option<String>,
    /// Stage 17: the revision of the contact's picture (`getContactPicture`), null without one.
    #[specta(type = Option<Number>)]
    pub picture: Option<i64>,
    /// Stage 17: at least one address is blocked (marked as spam).
    pub blocked: bool,
}

/// Stage 17: which contacts a page lists.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ContactFilter {
    /// People only: list and system addresses are left out.
    #[default]
    People,
    /// Every contact.
    All,
}

/// Stage 17: an image the user chose for a contact, re-encoded in the helper and at most
/// [`CONTACT_PICTURE_SOURCE_SIZE`] pixels per side, for the cropper. `token` names it in
/// `setContactPicture` for a few minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactPictureSource {
    pub token: String,
    /// `data:image/jpeg;base64,…`, written by the helper.
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// Stage 17: the square the user cut out, in pixels of [`ContactPictureSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PictureCrop {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

/// Stage 17: a contact's picture as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactPicture {
    /// `data:image/jpeg;base64,…`, written by the helper.
    pub data_url: String,
    #[specta(type = Number)]
    pub revision: i64,
}

/// Stage 17: which addresses show a picture: every address of a contact that has one, lower
/// case (as written and with the ASCII domain), and the picture's revision. `key` is the
/// contact's (for `getContactPicture`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactPictureRef {
    pub key: String,
    pub emails: Vec<String>,
    #[specta(type = Number)]
    pub revision: i64,
}

/// Stage 17: the newest messages from or to a contact (any of its addresses), newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactMessages {
    pub items: Vec<crate::MessageListItem>,
}

/// Stage 17: existing mail of a contact marked as spam in one account: messages from its
/// addresses outside junk, trash, sent and drafts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamMailCount {
    pub account_id: AccountId,
    pub count: u32,
    /// The account may write (`writes_enabled`); otherwise the mail stays where it is.
    pub writes_enabled: bool,
}

/// Stage 17: what "Mark as spam" did: the contact's addresses are blocked now; `mail` is what
/// could be moved to junk (`moveContactMailToJunk`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamMark {
    pub contact: ContactEntry,
    /// The addresses blocked by this mark (not those blocked before), for undo.
    pub blocked: Vec<String>,
    pub mail: Vec<SpamMailCount>,
}

/// Stage 17: the moves to junk, one batch per account and chunk (each undoable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamMove {
    pub batches: Vec<BatchId>,
    /// Messages queued for the move.
    pub moved: u32,
    /// Accounts whose mail stayed: read-only, without a junk folder, or refused.
    pub kept: Vec<SpamMailCount>,
}

/// Stage 17: undoing "Mark as spam": the addresses the mark blocked are unblocked and the
/// moves undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamUndo {
    /// Addresses blocked by the mark (`SpamMark.blocked`).
    pub addresses: Vec<String>,
    /// Batches of `SpamMove.batches`.
    pub batches: Vec<BatchId>,
}

/// Stage 17: result of [`SpamUndo`]: the contact, and how many moves could not be undone
/// (finished on the server in a way that cannot be reversed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamUndone {
    pub contact: ContactEntry,
    pub not_undone: u32,
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
    /// Stage 17: the note is longer than [`MAX_CONTACT_NOTE_CHARS`] or holds control
    /// characters other than line breaks and tabs.
    #[error("not a usable note")]
    InvalidNote,
    /// Stage 17: the image file is larger than [`MAX_CONTACT_PICTURE_FILE_BYTES`], or larger
    /// than the decoder's limits, or the helper stopped over its time or memory limit.
    #[error("the picture is too large: {0}")]
    PictureTooLarge(String),
    /// Stage 17: not a PNG, JPEG, GIF or WebP image (by its bytes).
    #[error("not a picture the app reads: {0}")]
    PictureUnsupported(String),
    /// Stage 17: the image could not be decoded (damaged, or crafted).
    #[error("the picture could not be read: {0}")]
    PictureDamaged(String),
    /// Stage 17: the chosen image is no longer held (chosen too long ago); choose it again.
    #[error("the chosen picture expired")]
    PictureExpired,
}

impl ContactRequestError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::NotConfirmed => ErrorCode::ConfirmationRequired,
            Self::SameContact | Self::InvalidName | Self::NotMerged(_) | Self::InvalidNote => {
                ErrorCode::InvalidArgument
            }
            Self::PictureTooLarge(_) => ErrorCode::PictureTooLarge,
            Self::PictureUnsupported(_) => ErrorCode::PictureUnsupported,
            Self::PictureDamaged(_) => ErrorCode::PictureDamaged,
            Self::PictureExpired => ErrorCode::NotFound,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "The contact no longer exists.",
            Self::NotConfirmed => "Merging contacts needs your confirmation.",
            Self::SameContact => "These addresses already belong to one contact.",
            Self::InvalidName => "Enter a name (not an email address).",
            Self::NotMerged(_) => "This address is not merged with another one.",
            Self::InvalidNote => "The note is too long or contains characters it cannot have.",
            Self::PictureTooLarge(_) => "The picture is too large.",
            Self::PictureUnsupported(_) => "Choose a PNG, JPEG, GIF or WebP picture.",
            Self::PictureDamaged(_) => "The picture could not be read.",
            Self::PictureExpired => "Choose the picture again.",
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
        assert_eq!(
            serde_json::to_value(ContactKind::System).expect("kind"),
            json!("system")
        );
        for kind in [ContactKind::Person, ContactKind::List, ContactKind::System] {
            assert_eq!(ContactKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(ContactKind::from_code(3), None);
        assert_eq!(
            ContactRequestError::PictureDamaged("x".into()).code(),
            ErrorCode::PictureDamaged
        );
        assert_eq!(
            ContactRequestError::PictureExpired.code(),
            ErrorCode::NotFound
        );
    }
}
