//! Stage 14: [`JmapSyncFolder`], the settings sync folder (decisions 0014, 0022) on a JMAP
//! account: the same `SyncFolder` the IMAP adapter offers, over JMAP.
//!
//! - **One mailbox, one message per record version.** The folder is a mailbox with the same
//!   wire name as over IMAP (`Settings-Sync` at the top level, or below the inbox). An entry is
//!   uploaded as a blob and imported into it (`Email/import`, `$seen`); listing is
//!   `Email/query` in the mailbox and `Email/get` of the size and the header fields; a body is
//!   the downloaded blob without its header (what IMAP's `BODY.PEEK[TEXT]` gives); deleting
//!   destroys the message, or takes it out of this mailbox when it lies in others too.
//! - **Numbers per connection.** JMAP has string ids; the folder numbers the messages it sees in
//!   the order they arrived, for as long as the connection lives (the settings sync keeps one
//!   connection per pass, so the numbers never change underneath it). UIDVALIDITY comes from
//!   the mailbox id; "HIGHESTMODSEQ" is a hash of the folder's message ids, so the state that
//!   [`SyncFolder::open`] reports changes exactly when an entry was added or removed.
//! - **Writes only here.** The connection is read-only (the allowlist of `client`); the folder's
//!   own writes pass a scope check that looks at their arguments before anything is sent:
//!   `Mailbox/set` may only create this folder, `Email/import` only into it, `Email/set` only
//!   destroy messages listed in it or take them out of it. Any other write fails with
//!   `read_only`, also on accounts without `writes_enabled`.
//! - **Bounded reads** (finding F2 of the stage 15 review): the settings sync decides from the
//!   listed sizes what to download; a body is read only up to [`MAX_ENTRY_READ`] bytes,
//!   whatever the server delivers.

use std::collections::{BTreeMap, HashMap, HashSet};

use mail_model::{
    BackendError, ModSeq, SyncFolder, SyncFolderEntry, SyncFolderState, Uid, UidSet, UidValidity,
};
use serde_json::{Map, Value, json};

use crate::backend::USING;
use crate::client::Client;
use crate::error;
use crate::mailbox::{self, DELIMITER, JmapMailbox};
use crate::session::Session;

/// Largest body read for one entry: a record is at most 1 MiB of ciphertext (about 1.4 MiB in
/// ASCII armor), and the settings sync never asks for an entry listed above 2 MiB.
pub(crate) const MAX_ENTRY_READ: usize = 4 * 1024 * 1024;

/// Longest header field name the caller may ask for.
const MAX_FIELD_NAME: usize = 64;

/// One message of the folder as this connection knows it.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    blob: Option<String>,
    /// The message lies in other mailboxes too (deleting only takes it out of this one).
    elsewhere: bool,
}

/// The settings sync connection of one JMAP account. Built by
/// [`crate::JmapConnector::connect_sync_folder`].
pub struct JmapSyncFolder {
    client: Client,
    session: Session,
    /// Wire name of the folder (`Settings-Sync`, `INBOX/Settings-Sync`).
    folder: String,
    /// JMAP id of the folder once it was opened.
    mailbox: Option<String>,
    entries: BTreeMap<Uid, Entry>,
    uids: HashMap<String, Uid>,
    next_uid: Uid,
}

impl std::fmt::Debug for JmapSyncFolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JmapSyncFolder")
            .field("folder", &self.folder)
            .field("open", &self.mailbox.is_some())
            .finish_non_exhaustive()
    }
}

/// FNV-1a, 64 bit: a hash that is the same on every run and every computer.
fn stable_hash(parts: impl IntoIterator<Item = impl AsRef<[u8]>>) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.as_ref().iter().copied().chain([0xff]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// The ids of a JSON object's keys that are `true` (`mailboxIds`).
fn true_keys(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// The body of a message: everything after the empty line that ends its header.
fn body_of(raw: &[u8]) -> Vec<u8> {
    let crlf = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    let lf = raw.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    let start = match (crlf, lf) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => raw.len(),
    };
    raw[start..].to_vec()
}

impl JmapSyncFolder {
    pub(crate) fn new(
        client: Client,
        session: Session,
        folder: &str,
    ) -> Result<Self, BackendError> {
        if folder.is_empty()
            || folder.chars().any(char::is_control)
            || folder.starts_with(DELIMITER)
            || folder.ends_with(DELIMITER)
        {
            return Err(BackendError::InvalidState(format!(
                "{} is not a usable folder name",
                error::short(folder)
            )));
        }
        Ok(Self {
            client,
            session,
            folder: folder.to_owned(),
            mailbox: None,
            entries: BTreeMap::new(),
            uids: HashMap::new(),
            next_uid: 1,
        })
    }

    fn opened(&self) -> Result<&str, BackendError> {
        self.mailbox.as_deref().ok_or_else(|| {
            BackendError::InvalidState(format!("{} is not open", error::short(&self.folder)))
        })
    }

    /// The folder's parent path and its own name (`INBOX/Settings-Sync` → `INBOX`, `Settings-Sync`).
    fn split(&self) -> (Option<&str>, &str) {
        match self.folder.rsplit_once(DELIMITER) {
            Some((parent, leaf)) => (Some(parent), leaf),
            None => (None, self.folder.as_str()),
        }
    }

    /// What this connection may write (see the module docs). `mailbox` is the folder's id,
    /// `members` the messages listed in it, `parent` the id the folder is created under.
    fn permits(
        name: &str,
        args: &Value,
        mailbox: Option<&str>,
        members: &HashSet<&str>,
        create: Option<(&str, Option<&str>)>,
    ) -> bool {
        let empty = |key: &str| {
            args.get(key)
                .is_none_or(|v| v.is_null() || v.as_object().is_some_and(Map::is_empty))
        };
        match name {
            "Mailbox/set" => {
                let Some((leaf, parent)) = create else {
                    return false;
                };
                let Some(created) = args.get("create").and_then(Value::as_object) else {
                    return false;
                };
                empty("update")
                    && args
                        .get("destroy")
                        .is_none_or(|d| d.is_null() || d.as_array().is_some_and(Vec::is_empty))
                    && created.len() == 1
                    && created.values().all(|m| {
                        m.get("name").and_then(Value::as_str) == Some(leaf)
                            && m.get("parentId").and_then(Value::as_str) == parent
                            && m.get("role").is_none_or(Value::is_null)
                    })
            }
            "Email/import" => {
                let Some(mailbox) = mailbox else {
                    return false;
                };
                args.get("emails")
                    .and_then(Value::as_object)
                    .is_some_and(|emails| {
                        !emails.is_empty()
                            && emails.values().all(|e| {
                                true_keys(e.get("mailboxIds")) == [mailbox]
                                    && e.get("mailboxIds")
                                        .and_then(Value::as_object)
                                        .is_some_and(|m| m.len() == 1)
                            })
                    })
            }
            "Email/set" => {
                let Some(mailbox) = mailbox else {
                    return false;
                };
                let patch_key = format!("mailboxIds/{mailbox}");
                let destroys_members = args.get("destroy").is_none_or(|d| {
                    d.is_null()
                        || d.as_array().is_some_and(|ids| {
                            ids.iter()
                                .all(|id| id.as_str().is_some_and(|id| members.contains(id)))
                        })
                });
                let updates_only_leave = args.get("update").is_none_or(|u| {
                    u.is_null()
                        || u.as_object().is_some_and(|updates| {
                            updates.iter().all(|(id, patch)| {
                                members.contains(id.as_str())
                                    && patch.as_object().is_some_and(|p| {
                                        p.len() == 1
                                            && p.get(&patch_key).is_some_and(Value::is_null)
                                    })
                            })
                        })
                });
                empty("create") && destroys_members && updates_only_leave
            }
            _ => false,
        }
    }

    /// Calls methods under this folder's scope.
    async fn call(
        &self,
        calls: Vec<crate::client::Call>,
        create: Option<(&str, Option<&str>)>,
    ) -> Result<crate::client::Responses, BackendError> {
        let members: HashSet<&str> = self.entries.values().map(|e| e.id.as_str()).collect();
        let mailbox = self.mailbox.as_deref();
        self.client
            .call_scoped(&self.session.api_url, USING, calls, |name, args| {
                Self::permits(name, args, mailbox, &members, create)
            })
            .await
    }

    async fn mailboxes(&self) -> Result<Vec<JmapMailbox>, BackendError> {
        let responses = self
            .call(
                vec![(
                    "Mailbox/get",
                    json!({"accountId": self.session.account_id, "ids": null,
                           "properties": mailbox::PROPERTIES}),
                    "m",
                )],
                None,
            )
            .await?;
        let list = responses
            .get("m", "Mailbox/get")?
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(mailbox::from_list(&list))
    }

    /// Creates the folder (no role, so no client treats it as anything special). Another
    /// client creating it meanwhile is fine: the caller looks again.
    async fn create(&self, list: &[JmapMailbox]) -> Result<(), BackendError> {
        let (parent, leaf) = self.split();
        let parent_id = match parent {
            None => None,
            Some(path) => Some(
                list.iter()
                    .find(|m| m.path == path)
                    .map(|m| m.id.clone())
                    .ok_or_else(|| BackendError::MailboxNotFound(path.to_owned()))?,
            ),
        };
        let responses = self
            .call(
                vec![(
                    "Mailbox/set",
                    json!({"accountId": self.session.account_id,
                           "create": {"s": {"name": leaf, "parentId": parent_id}}}),
                    "c",
                )],
                Some((leaf, parent_id.as_deref())),
            )
            .await?;
        let args = responses.get("c", "Mailbox/set")?;
        if args.get("created").and_then(|c| c.get("s")).is_none() {
            let err = args
                .get("notCreated")
                .and_then(|n| n.get("s"))
                .cloned()
                .unwrap_or(Value::Null);
            tracing::debug!(error = ?err.get("type"), "creating the settings sync mailbox refused");
        }
        Ok(())
    }

    /// Lists the folder again: new messages get the next numbers (oldest first), vanished ones
    /// lose theirs.
    async fn refresh(&mut self) -> Result<(), BackendError> {
        let mailbox = self.opened()?.to_owned();
        let (ids, _) = crate::map::list_mailbox(&self.client, &self.session, &mailbox).await?;
        let present: HashSet<&str> = ids.iter().map(String::as_str).collect();
        self.entries.retain(|_, e| present.contains(e.id.as_str()));
        self.uids.retain(|id, _| present.contains(id.as_str()));
        for id in ids {
            if !self.uids.contains_key(&id) {
                self.add(id, None, false);
            }
        }
        Ok(())
    }

    fn add(&mut self, id: String, blob: Option<String>, elsewhere: bool) -> Uid {
        let uid = self.next_uid;
        self.next_uid = self.next_uid.saturating_add(1);
        self.uids.insert(id.clone(), uid);
        self.entries.insert(
            uid,
            Entry {
                id,
                blob,
                elsewhere,
            },
        );
        uid
    }

    fn state(&self, mailbox: &str) -> SyncFolderState {
        let validity = u32::try_from(stable_hash([mailbox.as_bytes()]) >> 33)
            .unwrap_or(1)
            .max(1);
        let mut ids: Vec<&str> = self.entries.values().map(|e| e.id.as_str()).collect();
        ids.sort_unstable();
        SyncFolderState {
            uid_validity: UidValidity(validity),
            uid_next: self.next_uid,
            exists: u32::try_from(self.entries.len()).unwrap_or(u32::MAX),
            highest_modseq: Some(ModSeq((stable_hash(ids) >> 1).max(1))),
        }
    }
}

impl SyncFolder for JmapSyncFolder {
    async fn open(&mut self, create: bool) -> Result<Option<SyncFolderState>, BackendError> {
        let mut list = self.mailboxes().await?;
        let mut found = list.iter().find(|m| m.path == self.folder).cloned();
        if found.is_none() && create {
            self.create(&list).await?;
            list = self.mailboxes().await?;
            found = list.iter().find(|m| m.path == self.folder).cloned();
            if found.is_none() {
                return Err(BackendError::Rejected(format!(
                    "the server did not create {}",
                    error::short(&self.folder)
                )));
            }
        }
        let Some(found) = found else {
            return Ok(None);
        };
        if self.mailbox.as_deref() != Some(found.id.as_str()) {
            // Another mailbox under this name: its messages are numbered anew.
            self.entries.clear();
            self.uids.clear();
        }
        self.mailbox = Some(found.id.clone());
        self.refresh().await?;
        Ok(Some(self.state(&found.id)))
    }

    async fn list(&mut self, fields: &[&str]) -> Result<Vec<SyncFolderEntry>, BackendError> {
        self.opened()?;
        if fields.iter().any(|f| {
            f.is_empty()
                || f.len() > MAX_FIELD_NAME
                || !f.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            return Err(BackendError::InvalidState(
                "header field names are letters, digits and '-'".into(),
            ));
        }
        self.refresh().await?;
        let mailbox = self.opened()?.to_owned();
        let ids: Vec<String> = self.entries.values().map(|e| e.id.clone()).collect();
        let mut found: HashMap<String, (u64, Option<String>, bool, Vec<u8>)> = HashMap::new();
        for chunk in ids.chunks(self.session.max_objects_in_get) {
            let responses = self
                .call(
                    vec![(
                        "Email/get",
                        json!({"accountId": self.session.account_id, "ids": chunk,
                               "properties": ["id", "blobId", "size", "mailboxIds", "headers"]}),
                        "g",
                    )],
                    None,
                )
                .await?;
            let list = responses
                .get("g", "Email/get")?
                .get("list")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for email in list {
                let Some(id) = email.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let mailboxes = true_keys(email.get("mailboxIds"));
                if !mailboxes.contains(&mailbox.as_str()) {
                    // Moved away meanwhile.
                    continue;
                }
                let mut header = Vec::new();
                for field in email
                    .get("headers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let (Some(name), Some(value)) = (
                        field.get("name").and_then(Value::as_str),
                        field.get("value").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    if fields.iter().any(|f| f.eq_ignore_ascii_case(name)) {
                        header.extend_from_slice(name.as_bytes());
                        header.push(b':');
                        header.extend_from_slice(value.as_bytes());
                        header.extend_from_slice(b"\r\n");
                    }
                }
                found.insert(
                    id.to_owned(),
                    (
                        email.get("size").and_then(Value::as_u64).unwrap_or(0),
                        email
                            .get("blobId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        mailboxes.len() > 1,
                        header,
                    ),
                );
            }
        }
        let mut out = Vec::with_capacity(found.len());
        for (uid, entry) in &mut self.entries {
            let Some((size, blob, elsewhere, header)) = found.remove(&entry.id) else {
                continue;
            };
            entry.blob = blob;
            entry.elsewhere = elsewhere;
            out.push(SyncFolderEntry {
                uid: *uid,
                size: u32::try_from(size).unwrap_or(u32::MAX),
                header,
            });
        }
        Ok(out)
    }

    async fn read(&mut self, uids: &UidSet) -> Result<Vec<(Uid, Vec<u8>)>, BackendError> {
        self.opened()?;
        let mut bodies = Vec::new();
        for uid in uids.iter() {
            let Some(entry) = self.entries.get(&uid) else {
                continue;
            };
            let Some(blob) = entry.blob.clone() else {
                continue;
            };
            let url = self
                .session
                .download(&blob, "entry.eml", "message/rfc822")?;
            if let Some(raw) = self.client.download_limited(&url, MAX_ENTRY_READ).await? {
                bodies.push((uid, body_of(&raw)));
            }
        }
        Ok(bodies)
    }

    async fn append(&mut self, message: &[u8]) -> Result<Option<Uid>, BackendError> {
        if message.is_empty() || message.contains(&0) {
            return Err(BackendError::InvalidState(
                "an empty message or one with NUL bytes".into(),
            ));
        }
        let mailbox = self.opened()?.to_owned();
        let upload = self.session.upload()?;
        let blob = self
            .client
            .upload_for_folder(&upload, "message/rfc822", message)
            .await?;
        let responses = self
            .call(
                vec![(
                    "Email/import",
                    json!({"accountId": self.session.account_id, "emails": {"e": {
                        "blobId": blob,
                        "mailboxIds": {mailbox.clone(): true},
                        "keywords": {"$seen": true},
                    }}}),
                    "i",
                )],
                None,
            )
            .await?;
        let args = responses.get("i", "Email/import")?;
        let Some(created) = args.get("created").and_then(|c| c.get("e")) else {
            let err = args
                .get("notCreated")
                .and_then(|n| n.get("e"))
                .cloned()
                .unwrap_or(Value::Null);
            if err.get("type").and_then(Value::as_str) == Some("invalidProperties")
                && err
                    .get("properties")
                    .and_then(Value::as_array)
                    .is_some_and(|p| p.iter().any(|p| p.as_str() == Some("mailboxIds")))
            {
                // The folder was removed meanwhile (IMAP's `[TRYCREATE]`).
                return Err(BackendError::MailboxNotFound(self.folder.clone()));
            }
            return Err(error::from_set_error("Email/import", &err));
        };
        let id = created
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| BackendError::Protocol("Email/import answer without id".into()))?
            .to_owned();
        let blob = created
            .get("blobId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(uid) = self.uids.get(&id) {
            return Ok(Some(*uid));
        }
        Ok(Some(self.add(id, blob, false)))
    }

    async fn delete(&mut self, uids: &UidSet) -> Result<(), BackendError> {
        let mailbox = self.opened()?.to_owned();
        let mut destroy = Vec::new();
        let mut leave = Map::new();
        for uid in uids.iter() {
            let Some(entry) = self.entries.get(&uid) else {
                continue;
            };
            if entry.elsewhere {
                leave.insert(
                    entry.id.clone(),
                    json!({ format!("mailboxIds/{mailbox}"): null }),
                );
            } else {
                destroy.push(entry.id.clone());
            }
        }
        let chunk = self.session.max_objects_in_set;
        for ids in destroy.chunks(chunk) {
            let responses = self
                .call(
                    vec![(
                        "Email/set",
                        json!({"accountId": self.session.account_id, "destroy": ids}),
                        "d",
                    )],
                    None,
                )
                .await?;
            let args = responses.get("d", "Email/set")?;
            for (_, err) in args
                .get("notDestroyed")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if err.get("type").and_then(Value::as_str) != Some("notFound") {
                    return Err(error::from_set_error("Email/set", err));
                }
            }
        }
        let leave: Vec<(String, Value)> = leave.into_iter().collect();
        for part in leave.chunks(chunk) {
            let update: Map<String, Value> = part.iter().cloned().collect();
            let responses = self
                .call(
                    vec![(
                        "Email/set",
                        json!({"accountId": self.session.account_id, "update": update}),
                        "u",
                    )],
                    None,
                )
                .await?;
            let args = responses.get("u", "Email/set")?;
            for (_, err) in args
                .get("notUpdated")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if err.get("type").and_then(Value::as_str) != Some("notFound") {
                    return Err(error::from_set_error("Email/set", err));
                }
            }
        }
        for uid in uids.iter() {
            if let Some(entry) = self.entries.remove(&uid) {
                self.uids.remove(&entry.id);
            }
        }
        Ok(())
    }

    async fn logout(self) -> Result<(), BackendError> {
        // HTTP: nothing stays open.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_allows_only_writes_to_this_folder() {
        let members: HashSet<&str> = ["e1", "e2"].into_iter().collect();
        let mb = Some("ms");
        let ok = |name: &str, args: Value| {
            JmapSyncFolder::permits(name, &args, mb, &members, Some(("Settings-Sync", None)))
        };
        assert!(ok(
            "Mailbox/set",
            json!({"create": {"s": {"name": "Settings-Sync", "parentId": null}}})
        ));
        for refused in [
            json!({"create": {"s": {"name": "Other", "parentId": null}}}),
            json!({"create": {"s": {"name": "Settings-Sync", "parentId": "mi"}}}),
            json!({"create": {"s": {"name": "Settings-Sync", "parentId": null, "role": "inbox"}}}),
            json!({"create": {"s": {"name": "Settings-Sync"}, "t": {"name": "Settings-Sync"}}}),
            json!({"create": {"s": {"name": "Settings-Sync"}}, "destroy": ["mi"]}),
            json!({"update": {"mi": {"name": "x"}}}),
        ] {
            assert!(!ok("Mailbox/set", refused.clone()), "{refused}");
        }
        assert!(!JmapSyncFolder::permits(
            "Mailbox/set",
            &json!({"create": {"s": {"name": "Settings-Sync", "parentId": null}}}),
            mb,
            &members,
            None
        ));

        assert!(ok(
            "Email/import",
            json!({"emails": {"e": {"blobId": "b", "mailboxIds": {"ms": true}}}})
        ));
        for refused in [
            json!({"emails": {"e": {"blobId": "b", "mailboxIds": {"mi": true}}}}),
            json!({"emails": {"e": {"blobId": "b", "mailboxIds": {"ms": true, "mi": true}}}}),
            json!({"emails": {}}),
        ] {
            assert!(!ok("Email/import", refused.clone()), "{refused}");
        }

        assert!(ok("Email/set", json!({"destroy": ["e1", "e2"]})));
        assert!(ok(
            "Email/set",
            json!({"update": {"e1": {"mailboxIds/ms": null}}})
        ));
        for refused in [
            json!({"destroy": ["e1", "e3"]}),
            json!({"update": {"e1": {"mailboxIds/mi": null}}}),
            json!({"update": {"e1": {"mailboxIds/ms": null, "keywords/$seen": true}}}),
            json!({"update": {"e3": {"mailboxIds/ms": null}}}),
            json!({"update": {"e1": {"keywords/$flagged": true}}}),
            json!({"create": {"x": {"mailboxIds": {"ms": true}}}}),
        ] {
            assert!(!ok("Email/set", refused.clone()), "{refused}");
        }
        for other in [
            "EmailSubmission/set",
            "Identity/set",
            "Email/copy",
            "Blob/upload",
        ] {
            assert!(!ok(other, json!({})), "{other}");
        }
        // Not open yet: no writes to messages at all.
        assert!(!JmapSyncFolder::permits(
            "Email/import",
            &json!({"emails": {"e": {"blobId": "b", "mailboxIds": {"ms": true}}}}),
            None,
            &members,
            None
        ));
    }

    #[test]
    fn bodies_and_hashes() {
        assert_eq!(body_of(b"A: 1\r\nB: 2\r\n\r\nbody\r\n"), b"body\r\n");
        assert_eq!(body_of(b"A: 1\n\nbody"), b"body");
        assert_eq!(body_of(b"no header end"), b"");
        assert_eq!(stable_hash(["a", "b"]), stable_hash(["a", "b"]));
        assert_ne!(stable_hash(["ab"]), stable_hash(["a", "b"]));
        assert_ne!(stable_hash(["a", "b"]), stable_hash(["b", "a"]));
    }
}
