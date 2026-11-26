//! Stage 17, block B (decision 0050): what the contacts view does beyond the directory of
//! stage 10 (`compose::contacts`).
//!
//! - **Notes** the user writes on a contact (synchronised with the `contact` record).
//! - **No pictures** since stage 22 (decision 0072): the pictures of stage 17 were removed.
//! - **A contact's mail**: the newest messages from it (index on the sender) and to it (the
//!   search, `to:`), for the detail pane.
//! - **Mark as spam**: every address of the contact becomes a blocked sender (stage 16: new
//!   mail from it goes to junk, or is hidden on read-only accounts); existing mail from it can
//!   be moved to junk through the outbox, per account and only where the account may write.
//!   Undo unblocks what the mark blocked and undoes the moves.

use mail_content::normalize_address;
use mail_model::{
    ContactEntry, ContactMessages, ContactRequestError, CoreError, CoreEvent, MAX_ACTION_MESSAGES,
    MAX_CONTACT_MESSAGES, MAX_CONTACT_NOTE_CHARS, MailAction, MessageRef, SearchRequest,
    SearchScope, SpamMailCount, SpamMark, SpamMove, SpamUndo, SpamUndone,
};
use mail_store::{Store, StoredContactGroup};

use crate::compose::contacts::{self, email_of};
use crate::core::Connector;

/// Whether a note can be stored: at most [`MAX_CONTACT_NOTE_CHARS`] characters, no control
/// characters but line breaks and tabs.
pub(crate) fn is_usable_note(note: &str) -> bool {
    note.chars().count() <= MAX_CONTACT_NOTE_CHARS
        && !note
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        && !note.trim().is_empty()
}

/// A note as stored: line breaks `\n`, white space at the ends trimmed, empty is none.
fn clean_note(note: Option<&str>) -> Result<Option<String>, CoreError> {
    let Some(note) = note else {
        return Ok(None);
    };
    let note = note.replace("\r\n", "\n").replace('\r', "\n");
    let note = note.trim();
    if note.is_empty() {
        return Ok(None);
    }
    if !is_usable_note(note) {
        return Err(ContactRequestError::InvalidNote.into());
    }
    Ok(Some(note.to_owned()))
}

/// The addresses of a contact as keys, the preferred one first.
fn keys_of(group: &StoredContactGroup) -> Vec<String> {
    group.members.iter().map(|m| m.key.clone()).collect()
}

impl<C: Connector> crate::AppCore<C> {
    fn contacts_store(&self) -> Result<&Store, CoreError> {
        self.compose_ref()?;
        self.store_ref()
    }

    /// Sets (or with `None` or empty text clears) the note of the contact with this address;
    /// synchronised.
    pub async fn set_contact_note(
        &self,
        email: String,
        note: Option<String>,
    ) -> Result<ContactEntry, CoreError> {
        let store = self.contacts_store()?;
        let note = clean_note(note.as_deref())?;
        let group = contacts::group_of(store, &email).await?;
        let head = group.head().key.clone();
        if !store.set_contact_note(head.clone(), note).await? {
            return Err(ContactRequestError::NotFound(email).into());
        }
        self.emit_event(CoreEvent::ContactsChanged);
        contacts::get(store, &head).await
    }

    /// The newest messages from and to the contact with this address, newest first.
    pub async fn contact_messages(
        &self,
        email: String,
        limit: u32,
    ) -> Result<ContactMessages, CoreError> {
        let store = self.contacts_store()?;
        let limit = limit.clamp(1, MAX_CONTACT_MESSAGES);
        let group = contacts::group_of(store, &email).await?;
        let mut ids = Vec::new();
        for key in keys_of(&group) {
            ids.extend(store.sender_messages(key, limit).await?);
        }
        let mut items = store.message_items(ids).await?;
        // Mail to the contact: the search over the recipients, checked address by address.
        let keys = keys_of(&group);
        for member in &group.members {
            let page = self
                .search(SearchRequest {
                    query: format!("to:\"{}\"", email_of(member)),
                    scope: SearchScope::All,
                    cursor: None,
                    limit,
                    typing: false,
                    month_first: false,
                })
                .await?;
            for hit in page.hits {
                let to_contact =
                    hit.message.to.iter().any(|a| {
                        normalize_address(&a.email).is_some_and(|n| keys.contains(&n.key))
                    });
                if to_contact && !items.iter().any(|i| i.id == hit.message.id) {
                    items.push(hit.message);
                }
            }
        }
        items.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.0.cmp(&a.id.0)));
        items.truncate(limit as usize);
        Ok(ContactMessages { items })
    }

    /// Mail from the contact's addresses that "mark as spam" can move, per account.
    async fn spam_mail(
        &self,
        group: &StoredContactGroup,
    ) -> Result<Vec<(SpamMailCount, Vec<MessageRef>)>, CoreError> {
        let store = self.contacts_store()?;
        let accounts = store.accounts().await?;
        let mut by_account: Vec<(SpamMailCount, Vec<MessageRef>)> = Vec::new();
        for key in keys_of(group) {
            for location in store.sender_locations(key).await? {
                let target = MessageRef {
                    mailbox_id: location.mailbox,
                    message_id: location.message,
                };
                let index = match by_account
                    .iter()
                    .position(|(count, _)| count.account_id == location.account)
                {
                    Some(index) => index,
                    None => {
                        let writes_enabled = accounts
                            .iter()
                            .find(|a| a.id == location.account)
                            .is_some_and(|a| a.writes_enabled);
                        by_account.push((
                            SpamMailCount {
                                account_id: location.account,
                                count: 0,
                                writes_enabled,
                            },
                            Vec::new(),
                        ));
                        by_account.len() - 1
                    }
                };
                let (count, refs) = &mut by_account[index];
                if !refs.contains(&target) {
                    refs.push(target);
                    count.count = count.count.saturating_add(1);
                }
            }
        }
        Ok(by_account)
    }

    /// Marks the contact with this address as spam: every address of it becomes a blocked
    /// sender (stage 16), so new mail from it no longer lands in the inbox. Returns what was
    /// blocked (for undo) and the existing mail that `move_contact_mail_to_junk` can move.
    pub async fn mark_contact_spam(&self, email: String) -> Result<SpamMark, CoreError> {
        let store = self.contacts_store()?;
        let group = contacts::group_of(store, &email).await?;
        let mut blocked = Vec::new();
        for key in keys_of(&group) {
            if store.block_sender(key.clone(), self.now_ms()).await? {
                blocked.push(key);
            }
        }
        if !blocked.is_empty() {
            self.emit_event(CoreEvent::InboxChanged { account_id: None });
        }
        self.emit_event(CoreEvent::ContactsChanged);
        tracing::info!(addresses = blocked.len(), "contact marked as spam");
        let mail = self
            .spam_mail(&group)
            .await?
            .into_iter()
            .map(|(count, _)| count)
            .collect();
        Ok(SpamMark {
            contact: contacts::get(store, &group.head().key).await?,
            blocked,
            mail,
        })
    }

    /// Moves existing mail from the contact's addresses to junk through the outbox, one batch
    /// per account (and per [`MAX_ACTION_MESSAGES`]); accounts that may not write, or have no
    /// junk folder, keep their mail and are listed in `kept`.
    pub async fn move_contact_mail_to_junk(&self, email: String) -> Result<SpamMove, CoreError> {
        let store = self.contacts_store()?;
        let group = contacts::group_of(store, &email).await?;
        let mut result = SpamMove {
            batches: Vec::new(),
            moved: 0,
            kept: Vec::new(),
        };
        let stamp = self.now_ms();
        for (index, (count, refs)) in self.spam_mail(&group).await?.into_iter().enumerate() {
            if !count.writes_enabled {
                result.kept.push(count);
                continue;
            }
            let mut queued = 0u32;
            let mut refused = false;
            for (chunk_index, chunk) in refs.chunks(MAX_ACTION_MESSAGES).enumerate() {
                let request = format!("contact-spam-{stamp}-{index}-{chunk_index}");
                match self
                    .apply_action_quietly(MailAction::Spam, chunk.to_vec(), request)
                    .await
                {
                    Ok(done) => {
                        queued = queued.saturating_add(done.queued);
                        result.batches.extend(done.batch);
                    }
                    Err(err) => {
                        tracing::info!(code = ?err.code(), "contact's mail not moved to junk");
                        refused = true;
                        break;
                    }
                }
            }
            result.moved = result.moved.saturating_add(queued);
            if refused {
                result.kept.push(count);
            }
        }
        Ok(result)
    }

    /// Undoes "mark as spam": unblocks the addresses the mark blocked and undoes the moves to
    /// junk. A move that can no longer be undone is counted, the rest goes on.
    pub async fn undo_contact_spam(
        &self,
        email: String,
        undo: SpamUndo,
    ) -> Result<SpamUndone, CoreError> {
        let store = self.contacts_store()?;
        let group = contacts::group_of(store, &email).await?;
        let keys = keys_of(&group);
        for address in &undo.addresses {
            let key = crate::parity::sender_address(address)?;
            if keys.contains(&key) {
                store.unblock_sender(key).await?;
            }
        }
        let mut not_undone = 0u32;
        for batch in undo.batches {
            if let Err(err) = self.undo_action(batch).await {
                tracing::info!(code = ?err.code(), "move to junk not undone");
                not_undone = not_undone.saturating_add(1);
            }
        }
        self.emit_event(CoreEvent::InboxChanged { account_id: None });
        self.emit_event(CoreEvent::ContactsChanged);
        Ok(SpamUndone {
            contact: contacts::get(store, &group.head().key).await?,
            not_undone,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_cleaned_and_bounded() {
        assert_eq!(clean_note(None).expect("none"), None);
        assert_eq!(clean_note(Some("  \r\n ")).expect("empty"), None);
        assert_eq!(
            clean_note(Some(" Met at the fair\r\nLikes tea\t ")).expect("note"),
            Some("Met at the fair\nLikes tea".to_owned())
        );
        let long = "x".repeat(MAX_CONTACT_NOTE_CHARS + 1);
        for bad in [long.as_str(), "bell\u{7}"] {
            let err = clean_note(Some(bad)).expect_err("refused");
            assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument);
        }
    }
}
