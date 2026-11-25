//! Stage 19, block C (decision 0056): contact groups on the contacts page and in the composer.
//!
//! A group has a name (at most [`MAX_CONTACT_LIST_NAME_CHARS`] characters, one line) and member
//! addresses in the order added, each once (by normalised address). Members need no contact of
//! the directory. The composer's autocomplete suggests groups by the words of their names
//! ([`AppCore::suggest_contact_lists`]) and expands one into its members
//! ([`AppCore::get_contact_list`]). Every change emits `contactsChanged` (the contacts page and
//! open composers reload), and is synchronised as record kind `contactList`.

use mail_content::{clean_display_name, fold, normalize_address};
use mail_model::{
    Address, ContactList, ContactListId, ContactListMember, ContactListSummary, CoreError,
    CoreEvent, MAX_CONTACT_LIST_MEMBERS, MAX_CONTACT_LIST_NAME_CHARS,
};
use mail_store::{ListSummary, NewListMember, Store, StoredContactList};

use crate::AppCore;
use crate::core::Connector;
use crate::settings_sync::record::ContactListContent;

/// Most groups one suggestion lists.
const MAX_SUGGESTIONS: u32 = 8;

/// The name as stored: one line, white space collapsed, 1 to
/// [`MAX_CONTACT_LIST_NAME_CHARS`] characters, no control or direction characters.
pub(crate) fn clean_name(name: &str) -> Result<String, CoreError> {
    let cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars = cleaned.chars().count();
    let usable = chars > 0
        && u32::try_from(chars).is_ok_and(|n| n <= MAX_CONTACT_LIST_NAME_CHARS)
        && !cleaned
            .chars()
            .any(|c| c.is_control() || mail_model::is_bidi_control(c));
    if usable {
        Ok(cleaned)
    } else {
        Err(CoreError::InvalidArgument(format!(
            "a group name has 1 to {MAX_CONTACT_LIST_NAME_CHARS} characters on one line"
        )))
    }
}

/// The folded words of a name, as stored for the suggestions: `" word word"`.
pub(crate) fn search_of(name: &str) -> String {
    let mut search = String::new();
    for word in name.split_whitespace() {
        let folded = fold(word);
        if !folded.is_empty() {
            search.push(' ');
            search.push_str(&folded);
        }
    }
    search
}

/// A member to store: the address checked and normalised, the name cleaned.
pub(crate) fn member_of(address: &Address) -> Result<NewListMember, CoreError> {
    let email = address.email.trim();
    let normalized = normalize_address(email).ok_or_else(|| {
        CoreError::Draft(mail_model::DraftError::InvalidAddress(email.to_owned()))
    })?;
    let name = address
        .name
        .as_deref()
        .and_then(|name| clean_display_name(name, email));
    Ok(NewListMember {
        address_key: normalized.key,
        email: email.to_owned(),
        name,
    })
}

/// Whether a `contactList` record from another device is what this device would store.
pub(crate) fn check_synced(list: &ContactListContent) -> Result<(), String> {
    let name = clean_name(&list.name).map_err(|err| err.to_string())?;
    if name != list.name {
        return Err("not a usable group name".into());
    }
    if list.search != search_of(&name) {
        return Err("the search words do not match the name".into());
    }
    if list.members.len() > usize::try_from(MAX_CONTACT_LIST_MEMBERS).unwrap_or(usize::MAX) {
        return Err(format!(
            "a group has at most {MAX_CONTACT_LIST_MEMBERS} members"
        ));
    }
    for member in &list.members {
        let stored = member_of(&Address {
            name: member.name.clone(),
            email: member.email.clone(),
        })
        .map_err(|_| format!("not a usable address: {:?}", member.email))?;
        if stored.address_key != member.key || stored.email != member.email {
            return Err("a member's key does not match its address".into());
        }
        if stored.name != member.name {
            return Err("not a usable member name".into());
        }
    }
    Ok(())
}

fn summary(list: ListSummary) -> Option<ContactListSummary> {
    Some(ContactListSummary {
        id: ContactListId::parse(&list.uuid)?,
        name: list.name,
        member_count: list.members,
    })
}

fn to_model(list: StoredContactList) -> Result<ContactList, CoreError> {
    let id = ContactListId::parse(&list.uuid)
        .ok_or_else(|| CoreError::Internal("a stored group has no usable id".into()))?;
    Ok(ContactList {
        id,
        name: list.name,
        members: list
            .members
            .into_iter()
            .map(|m| ContactListMember {
                email: m.email,
                name: m.name.or(m.contact_name),
                contact: m.contact_key,
            })
            .collect(),
    })
}

fn not_found(id: &ContactListId) -> CoreError {
    CoreError::NotFound(format!("contact group {}", id.as_str()))
}

fn members_of(addresses: &[Address]) -> Result<Vec<NewListMember>, CoreError> {
    if addresses.len() > usize::try_from(MAX_CONTACT_LIST_MEMBERS).unwrap_or(usize::MAX) {
        return Err(CoreError::InvalidArgument(format!(
            "a group has at most {MAX_CONTACT_LIST_MEMBERS} members"
        )));
    }
    addresses.iter().map(member_of).collect()
}

pub(crate) async fn load(store: &Store, id: &ContactListId) -> Result<ContactList, CoreError> {
    let list = store
        .contact_list(id.as_str().to_owned())
        .await?
        .ok_or_else(|| not_found(id))?;
    to_model(list)
}

impl<C: Connector> AppCore<C> {
    fn lists_store(&self) -> Result<&Store, CoreError> {
        self.store_ref()
    }

    /// Stage 19: every contact group, by name, with the number of members.
    pub async fn list_contact_lists(&self) -> Result<Vec<ContactListSummary>, CoreError> {
        Ok(self
            .lists_store()?
            .contact_lists()
            .await?
            .into_iter()
            .filter_map(summary)
            .collect())
    }

    /// Stage 19: one group with its members (the composer expands a group into these).
    /// `not_found` for an unknown group.
    pub async fn get_contact_list(&self, id: ContactListId) -> Result<ContactList, CoreError> {
        load(self.lists_store()?, &id).await
    }

    /// Stage 19: a new group, optionally with members. `invalid_argument` for an unusable name
    /// or beyond the limits, `invalid_address` for an address that cannot be one.
    pub async fn create_contact_list(
        &self,
        name: String,
        members: Vec<Address>,
    ) -> Result<ContactList, CoreError> {
        let store = self.lists_store()?;
        let name = clean_name(&name)?;
        let members = members_of(&members)?;
        let uuid = store
            .create_contact_list(name.clone(), search_of(&name), members)
            .await?;
        tracing::info!("contact group created");
        self.emit_event(CoreEvent::ContactsChanged);
        let id = ContactListId::parse(&uuid)
            .ok_or_else(|| CoreError::Internal("the new group has no usable id".into()))?;
        load(store, &id).await
    }

    /// Stage 19: a new name for the group.
    pub async fn rename_contact_list(
        &self,
        id: ContactListId,
        name: String,
    ) -> Result<ContactList, CoreError> {
        let store = self.lists_store()?;
        let name = clean_name(&name)?;
        if !store
            .rename_contact_list(id.as_str().to_owned(), name.clone(), search_of(&name))
            .await?
        {
            return Err(not_found(&id));
        }
        self.emit_event(CoreEvent::ContactsChanged);
        load(store, &id).await
    }

    /// Stage 19: removes the group; its members' contacts stay.
    pub async fn delete_contact_list(&self, id: ContactListId) -> Result<(), CoreError> {
        if !self
            .lists_store()?
            .delete_contact_list(id.as_str().to_owned())
            .await?
        {
            return Err(not_found(&id));
        }
        tracing::info!("contact group deleted");
        self.emit_event(CoreEvent::ContactsChanged);
        Ok(())
    }

    /// Stage 19: adds members at the end; addresses in the group already are left as they are.
    pub async fn add_contact_list_members(
        &self,
        id: ContactListId,
        members: Vec<Address>,
    ) -> Result<ContactList, CoreError> {
        let store = self.lists_store()?;
        let members = members_of(&members)?;
        let added = store
            .add_contact_list_members(id.as_str().to_owned(), members)
            .await?
            .ok_or_else(|| not_found(&id))?;
        if added > 0 {
            self.emit_event(CoreEvent::ContactsChanged);
        }
        load(store, &id).await
    }

    /// Stage 19: removes the member with this address (any spelling of it).
    pub async fn remove_contact_list_member(
        &self,
        id: ContactListId,
        email: String,
    ) -> Result<ContactList, CoreError> {
        let store = self.lists_store()?;
        let key = normalize_address(email.trim())
            .ok_or_else(|| CoreError::Draft(mail_model::DraftError::InvalidAddress(email.clone())))?
            .key;
        let removed = store
            .remove_contact_list_member(id.as_str().to_owned(), key)
            .await?
            .ok_or_else(|| not_found(&id))?;
        if removed {
            self.emit_event(CoreEvent::ContactsChanged);
        }
        load(store, &id).await
    }

    /// Stage 19: groups for the composer's autocomplete: every word of `query` starts a word of
    /// the name; by name, at most `limit` (1 to 8).
    pub async fn suggest_contact_lists(
        &self,
        query: String,
        limit: u32,
    ) -> Result<Vec<ContactListSummary>, CoreError> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(fold)
            .filter(|w| !w.is_empty())
            .take(8)
            .collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .lists_store()?
            .search_contact_lists(words, limit.clamp(1, MAX_SUGGESTIONS))
            .await?
            .into_iter()
            .filter_map(summary)
            .collect())
    }
}

#[cfg(test)]
mod tests;
