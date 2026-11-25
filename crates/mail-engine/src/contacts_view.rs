//! Stage 17, block B (decision 0050): what the contacts view does beyond the directory of
//! stage 10 (`compose::contacts`).
//!
//! - **Notes** the user writes on a contact (synchronised with the `contact` record).
//! - **Pictures**: the UI never touches a file. The shell hands the core a path the user chose
//!   in the system dialog or dropped on the window; the core reads it (at most
//!   [`MAX_CONTACT_PICTURE_FILE_BYTES`]) and has the helper process decode it and encode the
//!   whole image again for the cropper ([`PictureStep::Source`], decision 0046). The source is
//!   kept in memory under a token for a few minutes; the square the user cut out is encoded by
//!   the helper again ([`PictureStep::Crop`]): a JPEG of 256 × 256 px and at most 40 KiB, stored
//!   in the database and synchronised (`contactPicture`). A picture from another device is
//!   encoded once more by this device's helper before it is shown. No picture ever comes from
//!   the network.
//! - **A contact's mail**: the newest messages from it (index on the sender) and to it (the
//!   search, `to:`), for the detail pane.
//! - **Mark as spam**: every address of the contact becomes a blocked sender (stage 16: new
//!   mail from it goes to junk, or is hidden on read-only accounts); existing mail from it can
//!   be moved to junk through the outbox, per account and only where the account may write.
//!   Undo unblocks what the mark blocked and undoes the moves.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use mail_content::{PictureJpeg, normalize_address};
use mail_mcp::preview::PictureStep;
use mail_model::{
    ContactEntry, ContactMessages, ContactPicture, ContactPictureRef, ContactPictureSource,
    ContactRequestError, CoreError, CoreEvent, MAX_ACTION_MESSAGES, MAX_CONTACT_MESSAGES,
    MAX_CONTACT_NOTE_CHARS, MAX_CONTACT_PICTURE_BYTES, MAX_CONTACT_PICTURE_FILE_BYTES, MailAction,
    MessageRef, PictureCrop, PreviewError, SearchRequest, SearchScope, SpamMailCount, SpamMark,
    SpamMove, SpamUndo, SpamUndone,
};
use mail_store::{Store, StoredContactGroup};

use crate::compose::contacts::{self, email_of};
use crate::core::Connector;

/// How long a chosen image waits for its square.
const SOURCE_LIFETIME: Duration = Duration::from_secs(15 * 60);

/// Most chosen images kept at once.
const MAX_SOURCES: usize = 4;

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

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(BASE64[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

/// Strict standard base64 (padding required).
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| BASE64.iter().position(|a| *a == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let count = bytes.len() / 4;
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != count) {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk[..4 - padding].iter().enumerate() {
            n |= value(*c)? << (18 - 6 * i);
        }
        for i in 0..3 - padding {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// Whether a `contactPicture` record's `jpeg` is base64 of a JPEG within the stored size.
pub(crate) fn is_picture_record(text: &str) -> bool {
    text.len() <= MAX_CONTACT_PICTURE_BYTES.div_ceil(3) * 4
        && unbase64(text).is_some_and(|jpeg| {
            jpeg.len() <= MAX_CONTACT_PICTURE_BYTES && mail_content::is_jpeg(&jpeg)
        })
}

fn jpeg_data_url(jpeg: &[u8]) -> String {
    let mut out = String::from("data:image/jpeg;base64,");
    out.push_str(&base64(jpeg));
    out
}

/// A refusal of the helper as the contact error the UI shows.
fn picture_error(err: PreviewError) -> CoreError {
    match err {
        PreviewError::TooLarge(detail) => ContactRequestError::PictureTooLarge(detail),
        PreviewError::Unsupported(detail) => ContactRequestError::PictureUnsupported(detail),
        PreviewError::Damaged(detail) => ContactRequestError::PictureDamaged(detail),
    }
    .into()
}

/// Images the user chose, waiting for their square (in memory only).
#[derive(Debug, Default)]
pub(crate) struct PictureSources {
    sources: Mutex<VecDeque<(String, Vec<u8>, Instant)>>,
}

impl PictureSources {
    fn insert(&self, jpeg: Vec<u8>) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut sources = self.sources.lock().unwrap_or_else(PoisonError::into_inner);
        sources.retain(|(_, _, at)| at.elapsed() < SOURCE_LIFETIME);
        sources.push_back((token.clone(), jpeg, Instant::now()));
        while sources.len() > MAX_SOURCES {
            sources.pop_front();
        }
        token
    }

    fn get(&self, token: &str) -> Option<Vec<u8>> {
        let mut sources = self.sources.lock().unwrap_or_else(PoisonError::into_inner);
        sources.retain(|(_, _, at)| at.elapsed() < SOURCE_LIFETIME);
        sources
            .iter()
            .find(|(t, _, _)| t == token)
            .map(|(_, jpeg, _)| jpeg.clone())
    }

    fn remove(&self, token: &str) {
        self.sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(t, _, _)| t != token);
    }
}

/// The addresses of a contact as keys, the preferred one first.
fn keys_of(group: &StoredContactGroup) -> Vec<String> {
    group.members.iter().map(|m| m.key.clone()).collect()
}

/// Reads at most [`MAX_CONTACT_PICTURE_FILE_BYTES`] of the file the user chose.
async fn read_picture_file(path: PathBuf) -> Result<Vec<u8>, CoreError> {
    let limit = u64::from(MAX_CONTACT_PICTURE_FILE_BYTES);
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(&path).map_err(|err| {
            ContactRequestError::PictureUnsupported(format!("the file could not be read: {err}"))
        })?;
        let meta = file.metadata().map_err(|err| {
            ContactRequestError::PictureUnsupported(format!("the file could not be read: {err}"))
        })?;
        if !meta.is_file() {
            return Err(ContactRequestError::PictureUnsupported("not a file".into()).into());
        }
        if meta.len() > limit {
            return Err(ContactRequestError::PictureTooLarge(format!(
                "{} bytes, at most {limit}",
                meta.len()
            ))
            .into());
        }
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| {
                ContactRequestError::PictureUnsupported(format!(
                    "the file could not be read: {err}"
                ))
            })?;
        if bytes.len() as u64 > limit {
            return Err(
                ContactRequestError::PictureTooLarge(format!("more than {limit} bytes")).into(),
            );
        }
        Ok::<_, CoreError>(bytes)
    })
    .await
    .map_err(|err| CoreError::Internal(format!("picture task failed: {err}")))?
}

impl<C: Connector> crate::AppCore<C> {
    fn contacts_store(&self) -> Result<&Store, CoreError> {
        self.compose_ref()?;
        self.store_ref()
    }

    /// One step of the picture pipeline: in the helper when the app has one (always in the
    /// release build), else on the blocking pool (tests and tools), like previews (0043, 0046).
    pub(crate) async fn picture_step(
        &self,
        bytes: Vec<u8>,
        step: PictureStep,
    ) -> Result<PictureJpeg, CoreError> {
        let reading = self.reading_ref()?;
        let result = match reading.preview_helper() {
            Some(helper) => {
                mail_mcp::preview::contact_picture_isolated(
                    helper,
                    bytes,
                    step,
                    mail_mcp::preview::PreviewLimits::default(),
                )
                .await
            }
            None => tokio::task::spawn_blocking(move || step.run(&bytes))
                .await
                .map_err(|err| CoreError::Internal(format!("picture task failed: {err}")))?,
        };
        result.map_err(picture_error)
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

    /// Reads the image file the user chose (a path from the system dialog or a drop the shell
    /// saw) and has the helper encode it for the cropper.
    pub async fn load_contact_picture(
        &self,
        path: PathBuf,
    ) -> Result<ContactPictureSource, CoreError> {
        let bytes = read_picture_file(path).await?;
        let source = self.picture_step(bytes, PictureStep::Source).await?;
        let data_url = jpeg_data_url(&source.jpeg);
        let token = self.reading_ref()?.picture_sources.insert(source.jpeg);
        Ok(ContactPictureSource {
            token,
            data_url,
            width: source.width,
            height: source.height,
        })
    }

    /// Cuts the square `crop` out of the chosen image `token` and stores it as the picture of
    /// the contact with this address (its other addresses lose theirs); synchronised.
    pub async fn set_contact_picture(
        &self,
        email: String,
        token: String,
        crop: PictureCrop,
    ) -> Result<ContactEntry, CoreError> {
        let store = self.contacts_store()?;
        let group = contacts::group_of(store, &email).await?;
        let sources = &self.reading_ref()?.picture_sources;
        let source = sources
            .get(&token)
            .ok_or(ContactRequestError::PictureExpired)?;
        let picture = self.picture_step(source, PictureStep::Crop(crop)).await?;
        let head = group.head().key.clone();
        store
            .set_contact_picture(head.clone(), keys_of(&group), picture.jpeg)
            .await?;
        sources.remove(&token);
        tracing::info!("contact picture set");
        self.emit_event(CoreEvent::ContactsChanged);
        contacts::get(store, &head).await
    }

    /// Removes the picture of the contact with this address; synchronised.
    pub async fn remove_contact_picture(&self, email: String) -> Result<ContactEntry, CoreError> {
        let store = self.contacts_store()?;
        let group = contacts::group_of(store, &email).await?;
        store.remove_contact_pictures(keys_of(&group)).await?;
        self.emit_event(CoreEvent::ContactsChanged);
        contacts::get(store, &group.head().key).await
    }

    /// The picture of the contact with this address (any of its addresses, or an address with
    /// a picture from the sync that this device has no contact for yet); `None` without one.
    pub async fn get_contact_picture(
        &self,
        email: String,
    ) -> Result<Option<ContactPicture>, CoreError> {
        let store = self.contacts_store()?;
        let keys = match contacts::group_of(store, &email).await {
            Ok(group) => keys_of(&group),
            Err(CoreError::Contact(ContactRequestError::NotFound(_))) => {
                match normalize_address(&email) {
                    Some(address) => vec![address.key],
                    None => return Ok(None),
                }
            }
            Err(err) => return Err(err),
        };
        let Some(stored) = store.contact_picture_of(keys).await? else {
            return Ok(None);
        };
        let display = match stored.display {
            Some(display) => display,
            None => {
                // From the settings sync: shown only as this device's helper encoded it
                // (decision 0046). A refusal is kept as an empty picture, never shown.
                let whole = PictureCrop {
                    x: 0,
                    y: 0,
                    size: u32::MAX,
                };
                let display = match self
                    .picture_step(stored.image.clone(), PictureStep::Crop(whole))
                    .await
                {
                    Ok(encoded) => encoded.jpeg,
                    Err(err) => {
                        tracing::info!(code = ?err.code(), "synchronised contact picture refused");
                        Vec::new()
                    }
                };
                store
                    .set_contact_picture_display(
                        stored.key.clone(),
                        stored.revision,
                        display.clone(),
                    )
                    .await?;
                display
            }
        };
        if display.is_empty() {
            return Ok(None);
        }
        Ok(Some(ContactPicture {
            data_url: jpeg_data_url(&display),
            revision: stored.revision,
        }))
    }

    /// Which addresses show a picture (every address of a contact that has one).
    pub async fn list_contact_pictures(&self) -> Result<Vec<ContactPictureRef>, CoreError> {
        let store = self.contacts_store()?;
        let pictures: HashMap<String, i64> = store.contact_pictures().await?.into_iter().collect();
        let mut refs: Vec<ContactPictureRef> = Vec::new();
        let mut done: Vec<String> = Vec::new();
        let mut keys: Vec<&String> = pictures.keys().collect();
        keys.sort();
        for key in keys {
            if done.contains(key) {
                continue;
            }
            let Some(group) = store.contact_group(key.clone()).await? else {
                // A picture from the sync for an address this device has not seen yet.
                done.push(key.clone());
                refs.push(ContactPictureRef {
                    key: key.clone(),
                    emails: vec![key.clone()],
                    revision: pictures[key],
                });
                continue;
            };
            let Some(revision) = group.members.iter().find_map(|m| pictures.get(&m.key)) else {
                continue;
            };
            let mut emails = Vec::new();
            for member in &group.members {
                done.push(member.key.clone());
                for email in [email_of(member).to_lowercase(), member.key.clone()] {
                    if !emails.contains(&email) {
                        emails.push(email);
                    }
                }
            }
            refs.push(ContactPictureRef {
                key: group.head().key.clone(),
                emails,
                revision: *revision,
            });
        }
        Ok(refs)
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
                    .apply_action(MailAction::Spam, chunk.to_vec(), request)
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

    #[test]
    fn picture_records_are_base64_jpegs_within_the_limit() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
        assert!(is_picture_record(&base64(&jpeg)));
        assert_eq!(unbase64(&base64(&jpeg)).as_deref(), Some(&jpeg[..]));
        assert!(!is_picture_record(&base64(b"\x89PNG\r\n\x1a\n")));
        assert!(!is_picture_record("not base64"));
        let mut big = vec![0xFF, 0xD8, 0xFF];
        big.resize(MAX_CONTACT_PICTURE_BYTES + 1, 0);
        assert!(!is_picture_record(&base64(&big)));
    }

    #[test]
    fn chosen_images_expire_and_are_bounded() {
        let sources = PictureSources::default();
        let tokens: Vec<String> = (0..=MAX_SOURCES)
            .map(|i| sources.insert(vec![i as u8]))
            .collect();
        assert_eq!(sources.get(&tokens[0]), None, "the oldest went");
        assert_eq!(
            sources.get(&tokens[MAX_SOURCES]),
            Some(vec![MAX_SOURCES as u8])
        );
        sources.remove(&tokens[MAX_SOURCES]);
        assert_eq!(sources.get(&tokens[MAX_SOURCES]), None);
    }
}
