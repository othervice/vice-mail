//! Mailboxes (IMAP folders): the UI form, the adapter's LIST result and roles.

use serde::{Deserialize, Serialize};

use crate::{AccountId, MailboxId, ModSeq, Uid, UidValidity};

/// What a mailbox is for. At most one mailbox per account has each role except `custom`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum MailboxRole {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Trash,
    Junk,
    /// Virtual folder with every message (Gmail "All Mail", `\All`).
    All,
    /// Virtual folder with flagged messages (Gmail "Starred", `\Flagged`).
    Flagged,
    /// An ordinary folder.
    Custom,
}

impl MailboxRole {
    /// Text form in the database; `None` for [`MailboxRole::Custom`] (stored as NULL).
    pub const fn as_db(self) -> Option<&'static str> {
        match self {
            Self::Inbox => Some("inbox"),
            Self::Sent => Some("sent"),
            Self::Drafts => Some("drafts"),
            Self::Archive => Some("archive"),
            Self::Trash => Some("trash"),
            Self::Junk => Some("junk"),
            Self::All => Some("all"),
            Self::Flagged => Some("flagged"),
            Self::Custom => None,
        }
    }

    /// Inverse of [`MailboxRole::as_db`]; NULL and unknown text are `Custom`.
    pub fn from_db(s: Option<&str>) -> Self {
        match s {
            Some("inbox") => Self::Inbox,
            Some("sent") => Self::Sent,
            Some("drafts") => Self::Drafts,
            Some("archive") => Self::Archive,
            Some("trash") => Self::Trash,
            Some("junk") => Self::Junk,
            Some("all") => Self::All,
            Some("flagged") => Self::Flagged,
            _ => Self::Custom,
        }
    }
}

/// Why a mailbox has its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum RoleSource {
    /// The server said so: a SPECIAL-USE attribute (RFC 6154), or the name `INBOX`.
    Server,
    /// Guessed from the folder name (English and German names, Gmail folders).
    Heuristic,
    /// Set by the user; survives every refresh of the folder list.
    User,
    /// No role (`custom`) and no reason to think otherwise.
    None,
}

impl RoleSource {
    pub const fn as_db(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Heuristic => "heuristic",
            Self::User => "user",
            Self::None => "none",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "server" => Self::Server,
            "heuristic" => Self::Heuristic,
            "user" => Self::User,
            _ => Self::None,
        }
    }
}

/// A mailbox as the UI sees it (sidebar, folder tree).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub id: MailboxId,
    pub account_id: AccountId,
    /// Full decoded path, for example `Projekte/2026`. The inbox is `INBOX`.
    pub path: String,
    /// Last segment of `path`, for the tree. The UI shows a translated name for roles.
    pub name: String,
    /// Hierarchy delimiter, null if the server has a flat namespace.
    pub delimiter: Option<String>,
    pub parent_id: Option<MailboxId>,
    pub role: MailboxRole,
    pub role_source: RoleSource,
    /// `false` for placeholders that cannot hold messages (`\Noselect`, `\NonExistent`).
    pub selectable: bool,
    /// Unread messages stored locally.
    pub unread: u32,
    /// Messages stored locally (grows during the first sync).
    pub total: u32,
}

/// One entry of the server's folder list, as the adapter returns it. Internal, not exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMailbox {
    /// Name exactly as the server sent it (modified UTF-7). Pass it to `open_mailbox`. The
    /// adapter normalises any spelling of the inbox to `INBOX`.
    pub name: String,
    /// Decoded display path (Unicode). Equal to `name` if decoding failed.
    pub path: String,
    pub delimiter: Option<char>,
    /// LIST attributes as sent, for example `\HasNoChildren`, `\Sent`, `\Noselect`.
    pub attributes: Vec<String>,
    /// Counters from LIST-STATUS or STATUS; `None` for non-selectable mailboxes or if the
    /// server refused STATUS for this one.
    pub status: Option<MailboxStatus>,
}

impl RemoteMailbox {
    /// Whether the mailbox has the attribute (case-insensitive, with or without backslash).
    pub fn has_attribute(&self, attribute: &str) -> bool {
        has_attribute(&self.attributes, attribute)
    }

    /// `false` for `\Noselect` and `\NonExistent` entries.
    pub fn is_selectable(&self) -> bool {
        is_selectable(&self.attributes)
    }

    /// Last segment of the path.
    pub fn leaf_name(&self) -> &str {
        leaf_name(&self.path, self.delimiter)
    }

    /// Decoded path of the parent mailbox, if the path has one. The parent need not exist.
    pub fn parent_path(&self) -> Option<&str> {
        let d = self.delimiter?;
        self.path.rsplit_once(d).map(|(parent, _)| parent)
    }
}

/// STATUS counters of a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxStatus {
    pub messages: u32,
    pub unseen: Option<u32>,
    pub uid_next: Option<Uid>,
    pub uid_validity: Option<UidValidity>,
    pub highest_modseq: Option<ModSeq>,
}

pub(crate) fn has_attribute(attributes: &[String], attribute: &str) -> bool {
    let want = attribute.trim_start_matches('\\');
    attributes
        .iter()
        .any(|a| a.trim_start_matches('\\').eq_ignore_ascii_case(want))
}

/// `false` for `\Noselect` and `\NonExistent` (RFC 3501, RFC 5258).
pub fn is_selectable(attributes: &[String]) -> bool {
    !has_attribute(attributes, "Noselect") && !has_attribute(attributes, "NonExistent")
}

/// Last segment of a decoded path.
pub fn leaf_name(path: &str, delimiter: Option<char>) -> &str {
    match delimiter {
        Some(d) => path.rsplit(d).next().unwrap_or(path),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(path: &str, attributes: &[&str]) -> RemoteMailbox {
        RemoteMailbox {
            name: path.into(),
            path: path.into(),
            delimiter: Some('/'),
            attributes: attributes.iter().map(|s| (*s).to_owned()).collect(),
            status: None,
        }
    }

    #[test]
    fn attributes_and_paths() {
        let m = remote("Projekte/2026/Q1", &["\\HasNoChildren", "\\noselect"]);
        assert!(m.has_attribute("\\NoSelect") && m.has_attribute("hasnochildren"));
        assert!(!m.is_selectable());
        assert!(!remote("x", &["\\NonExistent"]).is_selectable());
        assert!(remote("x", &[]).is_selectable());
        assert_eq!(m.leaf_name(), "Q1");
        assert_eq!(m.parent_path(), Some("Projekte/2026"));
        assert_eq!(remote("INBOX", &[]).parent_path(), None);
        assert_eq!(leaf_name("a.b", None), "a.b");
    }

    #[test]
    fn db_text_round_trips() {
        for role in [
            MailboxRole::Inbox,
            MailboxRole::Sent,
            MailboxRole::Drafts,
            MailboxRole::Archive,
            MailboxRole::Trash,
            MailboxRole::Junk,
            MailboxRole::All,
            MailboxRole::Flagged,
            MailboxRole::Custom,
        ] {
            assert_eq!(MailboxRole::from_db(role.as_db()), role);
            // The database text equals the wire text (except custom, stored as NULL).
            if let Some(db) = role.as_db() {
                assert_eq!(serde_json::to_value(role).expect("serialize"), db);
            }
        }
        for source in [
            RoleSource::Server,
            RoleSource::Heuristic,
            RoleSource::User,
            RoleSource::None,
        ] {
            assert_eq!(RoleSource::from_db(source.as_db()), source);
        }
    }
}
