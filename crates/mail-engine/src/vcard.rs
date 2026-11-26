//! Stage 19, block C (decision 0056): vCard import and export (`.vcf`, 3.0 and 4.0).
//!
//! **Import** reads a file the user chose in the system dialog (at most
//! [`mail_content::MAX_VCARD_FILE_BYTES`]; the reader is `mail_content::read_vcards`). Each
//! individual card becomes a contact or is merged into the known one:
//! - addresses the directory does not know get a row with the card's name as the user's name;
//! - a known contact keeps its counts; it gets the card's name and note only where it has none;
//! - several addresses on one card, or cards naming addresses of different contacts, are merged
//!   into one contact (the head is the contact of the first known address, else the first);
//! - a picture on the card is skipped: contacts have no pictures (stage 22, decision 0072).
//!
//! Group cards (`KIND:group`, Apple's `X-ADDRESSBOOKSERVER-KIND:group`) become contact groups:
//! members by address, or by the UID of another card of the file; a group with the same name is
//! extended instead of created twice.
//!
//! **Export** writes the chosen contacts, a group or everything to the path from the system's
//! save dialog: one card per contact (name, every address, note; no picture), one per group with
//! its members (by the UID of their card in the same file, else by address). UIDs are derived
//! from the contact's key, so exporting again gives the same cards.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use mail_content::{
    VcardCard, VcardKind, VcardMember, VcardProblemKind, VcardVersion, normalize_address,
};
use mail_model::{
    Address, ContactExport, ContactListId, CoreError, CoreEvent, MAX_VCARD_EXPORT_CONTACTS,
    VcardImportProblem, VcardImportProblemKind, VcardImportReport, VcardVersionChoice,
};
use mail_store::{MergeOutcome, Store, StoredContactGroup};
use sha2::{Digest, Sha256};

use crate::AppCore;
use crate::compose::contacts::{self, Extras, email_of, entry_of};
use crate::contact_lists;
use crate::core::Connector;

/// Problems one report lists at most.
const MAX_PROBLEMS: usize = 32;

/// A UUID-shaped, stable UID for the card of the contact with this key.
fn uid_of(key: &str) -> String {
    let hash = Sha256::digest(key.as_bytes());
    let hex: String = hash.iter().take(16).map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn problem_kind(kind: VcardProblemKind) -> VcardImportProblemKind {
    match kind {
        VcardProblemKind::TooLarge => VcardImportProblemKind::TooLarge,
        VcardProblemKind::TooManyCards => VcardImportProblemKind::TooManyCards,
        _ => VcardImportProblemKind::Malformed,
    }
}

struct Report(VcardImportReport);

impl Report {
    fn problem(&mut self, card: usize, kind: VcardImportProblemKind) {
        if self.0.problems.len() < MAX_PROBLEMS {
            self.0.problems.push(VcardImportProblem {
                card: u32::try_from(card).unwrap_or(u32::MAX),
                kind,
            });
        }
    }
}

/// Reads at most `MAX_VCARD_FILE_BYTES` (+1 to tell a larger file).
async fn read_file(path: PathBuf) -> Result<Vec<u8>, CoreError> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let limit = u64::try_from(mail_content::MAX_VCARD_FILE_BYTES).unwrap_or(u64::MAX);
        let file = std::fs::File::open(&path).map_err(|err| {
            CoreError::InvalidArgument(format!("the file could not be read: {err}"))
        })?;
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            return Err(CoreError::InvalidArgument("not a file".into()));
        }
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| {
                CoreError::InvalidArgument(format!("the file could not be read: {err}"))
            })?;
        Ok(bytes)
    })
    .await
    .map_err(|err| CoreError::Internal(format!("vcard task failed: {err}")))?
}

impl<C: Connector> AppCore<C> {
    /// Stage 19: imports the contacts and groups of a `.vcf` file (a path from the system
    /// dialog). Duplicates are merged; nothing a known contact has is overwritten. Emits
    /// `contactsChanged`.
    pub async fn import_vcard_file(&self, path: PathBuf) -> Result<VcardImportReport, CoreError> {
        let bytes = read_file(path).await?;
        let file = tokio::task::spawn_blocking(move || mail_content::read_vcards(&bytes))
            .await
            .map_err(|err| CoreError::Internal(format!("vcard task failed: {err}")))?;
        let store = self.store_ref()?;
        let mut report = Report(VcardImportReport {
            cards: u32::try_from(file.cards.len()).unwrap_or(u32::MAX),
            ..VcardImportReport::default()
        });
        for problem in &file.problems {
            report.problem(problem.card, problem_kind(problem.kind));
        }
        // Individuals first: groups name them by UID.
        let mut by_uid: HashMap<String, Address> = HashMap::new();
        for (index, card) in file.cards.iter().enumerate() {
            if card.kind == VcardKind::Group {
                continue;
            }
            let Some(first) = self.import_card(store, card, index, &mut report).await? else {
                continue;
            };
            if let Some(uid) = &card.uid {
                by_uid.insert(uid.clone(), first);
            }
        }
        for (index, card) in file.cards.iter().enumerate() {
            if card.kind == VcardKind::Group {
                self.import_group(card, index, &by_uid, &mut report).await?;
            }
        }
        tracing::info!(
            cards = report.0.cards,
            created = report.0.created,
            updated = report.0.updated,
            lists = report.0.lists,
            "vCard file imported"
        );
        self.emit_event(CoreEvent::ContactsChanged);
        Ok(report.0)
    }

    /// One individual card: rows for new addresses, name and note where empty, merged.
    /// Returns the address groups can name it by.
    async fn import_card(
        &self,
        store: &Store,
        card: &VcardCard,
        index: usize,
        report: &mut Report,
    ) -> Result<Option<Address>, CoreError> {
        let mut seen = HashSet::new();
        let addresses: Vec<_> = card
            .emails
            .iter()
            .filter_map(|email| normalize_address(email))
            .filter(|a| seen.insert(a.key.clone()))
            .collect();
        let Some(first) = addresses.first() else {
            report.problem(index, VcardImportProblemKind::NoAddress);
            return Ok(None);
        };
        let name = card
            .formatted_name
            .as_deref()
            .and_then(|n| mail_content::clean_display_name(n, &first.key));
        let keys: Vec<String> = addresses.iter().map(|a| a.key.clone()).collect();
        let known = store.contacts_by_keys(keys.clone()).await?;
        let new_rows: Vec<_> = addresses
            .iter()
            .filter(|a| !known.iter().any(|k| k.key == a.key))
            .map(|a| contacts::imported_row(a.clone(), name.clone()))
            .collect();
        let created = store.insert_new_contacts(new_rows).await?;
        let mut changed = false;
        if known.is_empty() && created > 0 {
            report.0.created += 1;
        } else if created > 0 {
            changed = true;
        }
        // The head: the contact of the first address the directory knew, else the first.
        let head_key = match known.first() {
            Some(row) => contacts::group_of(store, &row.key)
                .await?
                .head()
                .key
                .clone(),
            None => first.key.clone(),
        };
        let mut absorb = Vec::new();
        for key in &keys {
            let group = contacts::group_of(store, key).await?;
            let group_head = group.head().key.clone();
            if group_head != head_key && !absorb.contains(&group_head) {
                absorb.push(group_head);
            }
        }
        if !absorb.is_empty() {
            let count = u32::try_from(absorb.len()).unwrap_or(u32::MAX);
            if let MergeOutcome::Merged(_) = store.merge_contacts(head_key.clone(), absorb).await? {
                report.0.merged += count;
                changed = true;
            }
        }
        let group = contacts::group_of(store, &head_key).await?;
        let head = group.head().clone();
        if head.user_name.is_none()
            && let Some(name) = &name
        {
            contacts::set_name(store, &head.key, Some(name)).await?;
            changed = true;
        }
        if head.note.is_none()
            && let Some(note) = card
                .note
                .as_deref()
                .filter(|n| crate::contacts_view::is_usable_note(n))
        {
            store
                .set_contact_note(head.key.clone(), Some(note.to_owned()))
                .await?;
            changed = true;
        }
        if changed && !(known.is_empty() && created > 0) {
            report.0.updated += 1;
        }
        Ok(Some(Address {
            name,
            email: card
                .emails
                .first()
                .cloned()
                .unwrap_or_else(|| first.key.clone()),
        }))
    }

    /// One group card: a new group, or the members added to the group of the same name.
    async fn import_group(
        &self,
        card: &VcardCard,
        index: usize,
        by_uid: &HashMap<String, Address>,
        report: &mut Report,
    ) -> Result<(), CoreError> {
        let Some(name) = card
            .formatted_name
            .as_deref()
            .and_then(|n| contact_lists::clean_name(n).ok())
        else {
            report.problem(index, VcardImportProblemKind::Malformed);
            return Ok(());
        };
        let members: Vec<Address> = card
            .members
            .iter()
            .filter_map(|member| match member {
                VcardMember::Email(email) => Some(Address {
                    name: None,
                    email: email.clone(),
                }),
                VcardMember::Uid(uid) => by_uid.get(uid).cloned(),
            })
            .filter(|a| normalize_address(&a.email).is_some())
            .collect();
        let existing = self
            .list_contact_lists()
            .await?
            .into_iter()
            .find(|l| l.name.to_lowercase() == name.to_lowercase());
        let result = match existing {
            Some(list) => self
                .add_contact_list_members(list.id, members)
                .await
                .map(drop),
            None => self.create_contact_list(name, members).await.map(drop),
        };
        match result {
            Ok(()) => report.0.lists += 1,
            Err(CoreError::InvalidArgument(_)) => {
                report.problem(index, VcardImportProblemKind::TooManyMembers);
            }
            Err(err) => return Err(err),
        }
        Ok(())
    }

    /// Stage 19: the file name the save dialog suggests for an export.
    pub async fn vcard_export_name(&self, export: &ContactExport) -> Result<String, CoreError> {
        let base = match export {
            ContactExport::All { .. } => "contacts".to_owned(),
            ContactExport::Contact { email } => {
                let entry = contacts::get(self.store_ref()?, email).await?;
                entry.name.unwrap_or(entry.email)
            }
            ContactExport::List { id } => self.get_contact_list(id.clone()).await?.name,
        };
        let base: String = base.chars().take(120).collect();
        Ok(mail_content::sanitize_filename(&format!("{base}.vcf")))
    }

    /// Stage 19: writes the chosen contacts or group as vCards to `path` (from the system's
    /// save dialog). Returns how many cards were written.
    pub async fn export_vcards(
        &self,
        export: ContactExport,
        version: VcardVersionChoice,
        path: PathBuf,
    ) -> Result<u32, CoreError> {
        let cards = self.export_cards(&export).await?;
        let count = u32::try_from(cards.len()).unwrap_or(u32::MAX);
        let version = match version {
            VcardVersionChoice::V3 => VcardVersion::V3,
            VcardVersionChoice::V4 => VcardVersion::V4,
        };
        let text = tokio::task::spawn_blocking(move || mail_content::write_vcards(&cards, version))
            .await
            .map_err(|err| CoreError::Internal(format!("vcard task failed: {err}")))?;
        self.reading_ref()?
            .save_bytes_to(path, text.into_bytes())
            .await?;
        tracing::info!(cards = count, "vCard file exported");
        Ok(count)
    }

    async fn export_cards(&self, export: &ContactExport) -> Result<Vec<VcardCard>, CoreError> {
        let store = self.store_ref()?;
        let extras = Extras::load(store).await?;
        let mut cards = Vec::new();
        let mut uids: HashMap<String, String> = HashMap::new();
        match export {
            ContactExport::All { people_only } => {
                for group in store.contact_groups(MAX_VCARD_EXPORT_CONTACTS).await? {
                    let person = group.head().kind == mail_model::ContactKind::Person.code();
                    if *people_only && !person {
                        continue;
                    }
                    cards.push(contact_card(&group, &extras, &mut uids));
                }
                for list in store.contact_lists().await? {
                    if let Some(id) = ContactListId::parse(&list.uuid) {
                        let list = self.get_contact_list(id).await?;
                        cards.push(group_card(&list, &uids));
                    }
                }
            }
            ContactExport::Contact { email } => {
                let group = contacts::group_of(store, email).await?;
                cards.push(contact_card(&group, &extras, &mut uids));
            }
            ContactExport::List { id } => {
                let list = self.get_contact_list(id.clone()).await?;
                for member in &list.members {
                    match contacts::group_of(store, &member.email).await {
                        Ok(group) => {
                            if !uids.contains_key(&group.head().key) {
                                cards.push(contact_card(&group, &extras, &mut uids));
                            }
                            let key = group.head().key.clone();
                            if let Some(uid) = uids.get(&key).cloned()
                                && let Some(norm) = normalize_address(&member.email)
                            {
                                uids.insert(norm.key, uid);
                            }
                        }
                        Err(_) => {
                            let Some(norm) = normalize_address(&member.email) else {
                                continue;
                            };
                            let uid = uid_of(&norm.key);
                            uids.insert(norm.key, uid.clone());
                            cards.push(VcardCard {
                                uid: Some(uid),
                                formatted_name: member.name.clone(),
                                emails: vec![member.email.clone()],
                                ..VcardCard::default()
                            });
                        }
                    }
                }
                cards.push(group_card(&list, &uids));
            }
        }
        Ok(cards)
    }
}

/// The card of one contact; its UID is remembered for every address (group members).
fn contact_card(
    group: &StoredContactGroup,
    extras: &Extras,
    uids: &mut HashMap<String, String>,
) -> VcardCard {
    let entry = entry_of(group, extras);
    let uid = uid_of(&group.head().key);
    for member in &group.members {
        uids.insert(member.key.clone(), uid.clone());
    }
    VcardCard {
        uid: Some(uid),
        kind: VcardKind::Individual,
        formatted_name: entry.name.clone(),
        emails: group.members.iter().map(email_of).collect(),
        note: entry.note,
        members: Vec::new(),
    }
}

/// The card of a group: members by the UID of their card in the file, else by address.
fn group_card(list: &mail_model::ContactList, uids: &HashMap<String, String>) -> VcardCard {
    VcardCard {
        uid: Some(list.id.as_str().to_owned()),
        kind: VcardKind::Group,
        formatted_name: Some(list.name.clone()),
        emails: Vec::new(),
        note: None,
        members: list
            .members
            .iter()
            .map(|m| {
                normalize_address(&m.email)
                    .and_then(|n| uids.get(&n.key).cloned())
                    .map_or_else(|| VcardMember::Email(m.email.clone()), VcardMember::Uid)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests;
