//! Saved and recent searches (stage 7, decision 0026): the types that cross the bridge and the
//! checks of names and queries, shared by the engine and the settings sync.
//!
//! A saved search is a query of the search language ([`crate::search`]) with a name; it appears
//! as a view in the sidebar, searches all accounts (the query narrows with `account:` and `in:`)
//! and is synchronised (record kind `savedView`). Recent searches are this device's own.

use std::fmt;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::ErrorCode;
use crate::search::MAX_QUERY_BYTES;

/// Most saved searches.
pub const MAX_SAVED_SEARCHES: usize = 100;
/// Longest name of a saved search, in characters.
pub const MAX_SAVED_SEARCH_NAME_CHARS: usize = 100;
/// Recent searches kept on the device; the oldest beyond this are dropped.
pub const MAX_RECENT_SEARCHES: usize = 10;

crate::snippets::uuid_id!(
    /// Id of a saved search: a UUID, the same on every device (the sync record id).
    SavedSearchId
);

/// A saved search, as the sidebar shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    pub id: SavedSearchId,
    pub name: String,
    /// The search as the user typed it.
    pub query: String,
    /// Order in the sidebar; new ones come last.
    #[specta(type = Number)]
    pub position: i64,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// A search of this device's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecentSearch {
    pub query: String,
    /// Unix milliseconds of the last use.
    #[specta(type = Number)]
    pub used_at: i64,
}

/// Why a saved search was not stored; nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SavedSearchError {
    /// Another saved search has this name (compared without case).
    #[error("a saved search named {0:?} exists already")]
    Exists(String),
    /// [`MAX_SAVED_SEARCHES`] are stored already.
    #[error("at most {MAX_SAVED_SEARCHES} saved searches")]
    Limit,
}

impl SavedSearchError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Exists(_) => ErrorCode::SavedSearchExists,
            Self::Limit => ErrorCode::SavedSearchLimit,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Exists(_) => "A saved search with this name exists already.",
            Self::Limit => "No more searches can be saved; delete one first.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        Some(self.to_string())
    }
}

/// The name as stored: trimmed, inner whitespace runs as one space. `None` when it is empty,
/// longer than [`MAX_SAVED_SEARCH_NAME_CHARS`] or holds control characters.
pub fn clean_saved_search_name(name: &str) -> Option<String> {
    if name.chars().any(|c| c.is_control() && !c.is_whitespace()) {
        return None;
    }
    let cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars = cleaned.chars().count();
    (chars > 0 && chars <= MAX_SAVED_SEARCH_NAME_CHARS).then_some(cleaned)
}

/// The query as stored (saved or recent): trimmed. `None` when it is empty, longer than the
/// parser reads ([`MAX_QUERY_BYTES`]) or holds control characters other than whitespace.
pub fn clean_search_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    let usable = !trimmed.is_empty()
        && trimmed.len() <= MAX_QUERY_BYTES
        && !trimmed
            .chars()
            .any(|c| c.is_control() && !c.is_whitespace());
    usable.then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_and_bounded() {
        assert_eq!(
            clean_saved_search_name("  Ungelesen \t von  Bob "),
            Some("Ungelesen von Bob".into())
        );
        assert_eq!(
            clean_saved_search_name("Rechnungen 🧾"),
            Some("Rechnungen 🧾".into())
        );
        assert_eq!(clean_saved_search_name("   "), None);
        assert_eq!(clean_saved_search_name("a\u{0}b"), None);
        assert_eq!(clean_saved_search_name("a\u{1b}[31m"), None);
        let longest = "ä".repeat(MAX_SAVED_SEARCH_NAME_CHARS);
        assert_eq!(clean_saved_search_name(&longest), Some(longest.clone()));
        assert_eq!(clean_saved_search_name(&format!("{longest}x")), None);
    }

    #[test]
    fn queries_are_trimmed_and_bounded() {
        assert_eq!(
            clean_search_query("  from:bob is:unread \n"),
            Some("from:bob is:unread".into())
        );
        assert_eq!(clean_search_query(" \t "), None);
        assert_eq!(clean_search_query("a\u{7}"), None);
        let longest = "a".repeat(MAX_QUERY_BYTES);
        assert_eq!(clean_search_query(&longest), Some(longest.clone()));
        assert_eq!(clean_search_query(&format!("{longest}b")), None);
    }

    #[test]
    fn errors_have_their_own_codes() {
        assert_eq!(
            SavedSearchError::Exists("x".into()).code(),
            ErrorCode::SavedSearchExists
        );
        assert_eq!(SavedSearchError::Limit.code(), ErrorCode::SavedSearchLimit);
        assert!(SavedSearchError::Limit.detail().is_some());
    }

    #[test]
    fn ids_are_uuids_on_the_wire() {
        let id = SavedSearchId::parse("6F9619FF-8B86-4D11-B42D-00C04FC964FF").expect("uuid");
        assert_eq!(id.as_str(), "6f9619ff-8b86-4d11-b42d-00c04fc964ff");
        assert!(SavedSearchId::parse("not a uuid").is_none());
        let search = SavedSearch {
            id,
            name: "Bob".into(),
            query: "from:bob".into(),
            position: 0,
            updated_at: 1,
        };
        let json = serde_json::to_value(&search).expect("json");
        assert_eq!(json["id"], "6f9619ff-8b86-4d11-b42d-00c04fc964ff");
        assert_eq!(json["updatedAt"], 1);
    }
}
