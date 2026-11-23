//! [`JmapWriter`]: `MailWriter` over JMAP, only for connections opened with
//! `Access::ReadWrite` (the connector refuses read-only configurations before connecting).
//!
//! - Flags are keyword patches of `Email/set`; `unchanged_since` is checked against the UID
//!   map after bringing it up to date: a message whose entry changed later is reported as
//!   modified and left alone (CONDSTORE `[MODIFIED]`).
//! - A move patches `mailboxIds` (atomic, like `UID MOVE`); the target's new UIDs are assigned
//!   at once and reported like COPYUID.
//! - Expunge destroys a message that lies only in this mailbox; one that lies in others too
//!   (labels on Fastmail) only leaves this one.
//! - Append uploads the message and imports it (`Email/import`), with APPENDUID.

use std::future::Future;
use std::time::Duration;

use mail_model::search::ServerCriterion;
use mail_model::{
    Access, AppendUid, BackendError, BodySection, Capabilities, CopyUid, ExpungeOutcome,
    FetchedSection, Flag, FlagUpdate, IdleOutcome, MailBackend, MailWriter, MailboxRole,
    MailboxStatus, MessageHeader, MessageStructure, ModSeq, MoveMethod, MoveOutcome, RawMessage,
    RemoteMailbox, SelectedMailbox, StoreMode, StoreOutcome, Uid, UidSet, UidValidity,
};
use serde_json::{Map, Value, json};

use crate::backend::JmapBackend;
use crate::email;
use crate::error;
use crate::mailbox;

/// One JMAP connection that may change the server.
pub struct JmapWriter {
    inner: JmapBackend,
}

impl JmapWriter {
    pub(crate) fn new(inner: JmapBackend) -> Result<Self, BackendError> {
        if inner.client.access() != Access::ReadWrite {
            return Err(BackendError::ReadOnly("writer".into()));
        }
        Ok(Self { inner })
    }

    /// `Email/set` updates in chunks; returns the ids the server updated. Messages that are
    /// gone (`notFound`) are skipped like IMAP skips vanished UIDs; other refusals fail.
    async fn update(&self, patches: Vec<(String, Value)>) -> Result<Vec<String>, BackendError> {
        let mut updated = Vec::new();
        for chunk in patches.chunks(self.inner.session.max_objects_in_set) {
            let mut update = Map::new();
            for (id, patch) in chunk {
                update.insert(id.clone(), patch.clone());
            }
            let responses = self
                .inner
                .call(vec![(
                    "Email/set",
                    json!({"accountId": self.inner.session.account_id, "update": update}),
                    "u",
                )])
                .await?;
            let args = responses.get("u", "Email/set")?;
            if let Some(map) = args.get("updated").and_then(Value::as_object) {
                updated.extend(map.keys().cloned());
            }
            if let Some(failed) = args.get("notUpdated").and_then(Value::as_object) {
                for (id, err) in failed {
                    if err.get("type").and_then(Value::as_str) == Some("notFound") {
                        tracing::debug!(email = %id, "message gone before it was changed");
                        continue;
                    }
                    return Err(error::from_set_error("Email/set", err));
                }
            }
        }
        Ok(updated)
    }

    async fn destroy(&self, ids: &[String]) -> Result<(), BackendError> {
        for chunk in ids.chunks(self.inner.session.max_objects_in_set) {
            let responses = self
                .inner
                .call(vec![(
                    "Email/set",
                    json!({"accountId": self.inner.session.account_id, "destroy": chunk}),
                    "d",
                )])
                .await?;
            let args = responses.get("d", "Email/set")?;
            if let Some(failed) = args.get("notDestroyed").and_then(Value::as_object) {
                for (_, err) in failed {
                    if err.get("type").and_then(Value::as_str) != Some("notFound") {
                        return Err(error::from_set_error("Email/set", err));
                    }
                }
            }
        }
        Ok(())
    }

    /// Numbers `mailbox` (JMAP id) and gives `ids` their UIDs there, after the change log has
    /// been read (so the change that put them there does not number them twice).
    async fn numbered(
        &mut self,
        mailbox: &str,
        ids: &[String],
    ) -> Result<(u32, Vec<Uid>), BackendError> {
        let mut map = self.inner.map.lock().await;
        map.prepare(&self.inner.session.account_id).await?;
        map.refresh(&self.inner.client, &self.inner.session).await?;
        map.fill(&self.inner.client, &self.inner.session, mailbox)
            .await?;
        let uids = map.assign(mailbox, ids);
        map.flush().await?;
        let validity = map
            .selected(mailbox)
            .map(|s| s.uid_validity)
            .ok_or_else(|| BackendError::InvalidState(format!("no map for mailbox {mailbox}")))?;
        Ok((validity, uids))
    }

    async fn create_one(
        &mut self,
        name: &str,
        parent: Option<&str>,
        role: Option<&str>,
    ) -> Result<(), BackendError> {
        let mut create = json!({"name": name, "parentId": parent});
        if let (Some(role), Some(object)) = (role, create.as_object_mut()) {
            object.insert("role".into(), Value::String(role.to_owned()));
        }
        let call = |create: Value| {
            vec![(
                "Mailbox/set",
                json!({"accountId": self.inner.session.account_id, "create": {"m": create}}),
                "c",
            )]
        };
        let responses = self.inner.call(call(create.clone())).await?;
        let args = responses.get("c", "Mailbox/set")?;
        if args.get("created").and_then(|c| c.get("m")).is_some() {
            return Ok(());
        }
        let err = args
            .get("notCreated")
            .and_then(|n| n.get("m"))
            .cloned()
            .unwrap_or(Value::Null);
        if role.is_some() && err.get("type").and_then(Value::as_str) == Some("invalidProperties") {
            // Another mailbox has the role: create it without (as IMAP's `[USEATTR]`).
            if let Some(object) = create.as_object_mut() {
                object.remove("role");
            }
            let responses = self.inner.call(call(create)).await?;
            let args = responses.get("c", "Mailbox/set")?;
            if args.get("created").and_then(|c| c.get("m")).is_some() {
                return Ok(());
            }
            let err = args
                .get("notCreated")
                .and_then(|n| n.get("m"))
                .cloned()
                .unwrap_or(Value::Null);
            return Err(error::from_set_error("Mailbox/set", &err));
        }
        Err(error::from_set_error("Mailbox/set", &err))
    }
}

impl MailBackend for JmapWriter {
    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }

    fn access(&self) -> Access {
        self.inner.access()
    }

    async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
        self.inner.list_mailboxes().await
    }

    async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        self.inner.open_mailbox(name).await
    }

    async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
        self.inner.search_uids(mailbox).await
    }

    async fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<MessageHeader>, BackendError> {
        self.inner.fetch_headers(mailbox, uids).await
    }

    async fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        changed_since: Option<ModSeq>,
    ) -> Result<Vec<FlagUpdate>, BackendError> {
        self.inner.fetch_flags(mailbox, uids, changed_since).await
    }

    async fn idle<S>(
        &mut self,
        mailbox: &SelectedMailbox,
        max: Duration,
        stop: S,
    ) -> Result<IdleOutcome, BackendError>
    where
        S: Future<Output = ()> + Send,
    {
        self.inner.idle(mailbox, max, stop).await
    }

    async fn fetch_structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<MessageStructure>, BackendError> {
        self.inner.fetch_structure(mailbox, uid).await
    }

    async fn fetch_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<RawMessage>, BackendError> {
        self.inner.fetch_messages(mailbox, uids).await
    }

    async fn fetch_sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
        self.inner.fetch_sections(mailbox, uid, sections).await
    }

    async fn search_text(
        &mut self,
        mailbox: &SelectedMailbox,
        criteria: &[ServerCriterion],
    ) -> Result<UidSet, BackendError> {
        self.inner.search_text(mailbox, criteria).await
    }

    async fn noop(&mut self) -> Result<(), BackendError> {
        self.inner.noop().await
    }

    async fn logout(self) -> Result<(), BackendError> {
        self.inner.logout().await
    }
}

impl MailWriter for JmapWriter {
    async fn mailbox_status(&mut self, name: &str) -> Result<MailboxStatus, BackendError> {
        let found = self.inner.resolve(name).await?;
        let state = self.inner.mailbox_state(&found.id).await?;
        Ok(MailboxStatus {
            messages: state.exists,
            unseen: Some(found.unread),
            uid_next: Some(state.uid_next),
            uid_validity: Some(UidValidity(state.uid_validity)),
            highest_modseq: Some(ModSeq(state.modseq)),
        })
    }

    async fn store_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<ModSeq>,
    ) -> Result<StoreOutcome, BackendError> {
        let id = self.inner.check(mailbox)?;
        if flags.iter().any(|f| matches!(f, Flag::Label(_))) {
            return Err(BackendError::Unsupported("Gmail labels".into()));
        }
        let keywords: Vec<String> = flags.iter().filter_map(email::keyword_of).collect();
        if keywords.is_empty() {
            return Ok(StoreOutcome::default());
        }
        if let Some(keyword) = keywords
            .iter()
            .find(|k| !mail_model::is_valid_keyword(k) && !k.starts_with('$'))
        {
            return Err(BackendError::InvalidState(format!(
                "not a keyword: {keyword:?}"
            )));
        }
        if unchanged_since.is_some() {
            // The latest changes first, so a change by someone else is seen.
            let mut map = self.inner.map.lock().await;
            map.prepare(&self.inner.session.account_id).await?;
            map.refresh(&self.inner.client, &self.inner.session).await?;
        }
        let entries = self.inner.lookup(&id, uids).await;
        let mut modified = Vec::new();
        let mut patches = Vec::new();
        let mut uid_of = std::collections::HashMap::new();
        for (uid, eid, modseq) in &entries {
            if unchanged_since.is_some_and(|since| *modseq > since.0) {
                modified.push(*uid);
                continue;
            }
            uid_of.insert(eid.clone(), (*uid, *modseq));
            patches.push((
                eid.clone(),
                email::keyword_patch(&keywords, mode == StoreMode::Add),
            ));
        }
        let updated = self.update(patches).await?;
        // Read the own change back into the map at once, so the modseq reported here is the
        // one the entry keeps: a later `unchanged_since` with it is no conflict.
        let after: std::collections::HashMap<Uid, u64> = {
            let mut map = self.inner.map.lock().await;
            map.prepare(&self.inner.session.account_id).await?;
            map.refresh(&self.inner.client, &self.inner.session).await?;
            map.lookup(&id, uids)
                .into_iter()
                .map(|(uid, _, modseq)| (uid, modseq))
                .collect()
        };
        let list = self
            .inner
            .get_emails(&updated, &["id", "keywords"], None)
            .await?;
        let updates = list
            .iter()
            .filter_map(|e| {
                let eid = e.get("id").and_then(Value::as_str)?;
                let (uid, modseq) = uid_of.get(eid)?;
                Some(FlagUpdate {
                    uid: *uid,
                    flags: email::flags_of(e.get("keywords")),
                    modseq: Some(ModSeq(after.get(uid).copied().unwrap_or(*modseq))),
                    gmail_labels: None,
                })
            })
            .collect();
        Ok(StoreOutcome {
            updates,
            modified: UidSet::from_uids(modified),
        })
    }

    async fn move_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        target: &str,
    ) -> Result<MoveOutcome, BackendError> {
        let source = self.inner.check(mailbox)?;
        let target = self.inner.resolve(target).await?;
        let entries = self.inner.lookup(&source, uids).await;
        let patches = entries
            .iter()
            .map(|(_, eid, _)| (eid.clone(), email::move_patch(&source, &target.id)))
            .collect();
        let moved = self.update(patches).await?;
        let moved_entries: Vec<(Uid, String)> = entries
            .iter()
            .filter(|(_, eid, _)| moved.contains(eid))
            .map(|(uid, eid, _)| (*uid, eid.clone()))
            .collect();
        let ids: Vec<String> = moved_entries.iter().map(|(_, e)| e.clone()).collect();
        {
            let mut map = self.inner.map.lock().await;
            for eid in &ids {
                map.remove(&source, eid);
            }
            map.flush().await?;
        }
        let (validity, new_uids) = self.numbered(&target.id, &ids).await?;
        Ok(MoveOutcome {
            method: MoveMethod::Move,
            copy_uid: Some(CopyUid {
                uid_validity: UidValidity(validity),
                pairs: moved_entries
                    .iter()
                    .map(|(uid, _)| *uid)
                    .zip(new_uids)
                    .collect(),
            }),
        })
    }

    async fn expunge_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<ExpungeOutcome, BackendError> {
        let source = self.inner.check(mailbox)?;
        let entries = self.inner.lookup(&source, uids).await;
        let ids: Vec<String> = entries.iter().map(|(_, e, _)| e.clone()).collect();
        let list = self
            .inner
            .get_emails(&ids, &["id", "mailboxIds"], None)
            .await?;
        let mut destroy = Vec::new();
        let mut leave = Vec::new();
        for e in &list {
            let Some(eid) = e.get("id").and_then(Value::as_str) else {
                continue;
            };
            let elsewhere = e
                .get("mailboxIds")
                .and_then(Value::as_object)
                .is_some_and(|m| m.keys().any(|k| k != &source));
            if elsewhere {
                leave.push((
                    eid.to_owned(),
                    json!({format!("mailboxIds/{}", email::escape_pointer(&source)): null}),
                ));
            } else {
                destroy.push(eid.to_owned());
            }
        }
        self.destroy(&destroy).await?;
        self.update(leave).await?;
        let mut map = self.inner.map.lock().await;
        for eid in &ids {
            map.remove(&source, eid);
        }
        map.flush().await?;
        Ok(ExpungeOutcome::Expunged)
    }

    async fn create_mailbox(
        &mut self,
        path: &str,
        role: Option<MailboxRole>,
    ) -> Result<String, BackendError> {
        self.inner.load_mailboxes().await?;
        let segments: Vec<&str> = path
            .split(mailbox::DELIMITER)
            .filter(|s| !s.is_empty())
            .collect();
        if segments.is_empty() {
            return Err(BackendError::InvalidState("empty mailbox path".into()));
        }
        let mut parent: Option<String> = None;
        let mut prefix = String::new();
        for (i, segment) in segments.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push(mailbox::DELIMITER);
            }
            prefix.push_str(segment);
            if let Some(found) = self.inner.mailboxes.iter().find(|m| m.path == prefix) {
                parent = Some(found.id.clone());
                continue;
            }
            let leaf = i + 1 == segments.len();
            let role = if leaf {
                role.and_then(mailbox::jmap_role)
            } else {
                None
            };
            self.create_one(segment, parent.as_deref(), role).await?;
            self.inner.load_mailboxes().await?;
            parent = self
                .inner
                .mailboxes
                .iter()
                .find(|m| m.path == prefix)
                .map(|m| m.id.clone());
        }
        Ok(segments.join(&mailbox::DELIMITER.to_string()))
    }

    async fn append(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        message: &[u8],
    ) -> Result<Option<AppendUid>, BackendError> {
        let target = self.inner.resolve(mailbox).await?;
        let upload = self.inner.session.upload()?;
        let blob = self
            .inner
            .client
            .upload(&upload, "message/rfc822", message)
            .await?;
        let responses = self
            .inner
            .call(vec![(
                "Email/import",
                json!({
                    "accountId": self.inner.session.account_id,
                    "emails": {"i": {
                        "blobId": blob,
                        "mailboxIds": {target.id.clone(): true},
                        "keywords": email::keywords_object(flags),
                    }},
                }),
                "i",
            )])
            .await?;
        let args = responses.get("i", "Email/import")?;
        let Some(created) = args
            .get("created")
            .and_then(|c| c.get("i"))
            .and_then(|c| c.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            let err = args
                .get("notCreated")
                .and_then(|n| n.get("i"))
                .cloned()
                .unwrap_or(Value::Null);
            return Err(error::from_set_error("Email/import", &err));
        };
        let (validity, uids) = self.numbered(&target.id, &[created]).await?;
        Ok(uids.first().map(|uid| AppendUid {
            uid_validity: UidValidity(validity),
            uid: *uid,
        }))
    }

    async fn find_message_id(
        &mut self,
        mailbox: &SelectedMailbox,
        message_id: &str,
    ) -> Result<UidSet, BackendError> {
        let source = self.inner.check(mailbox)?;
        let account = self.inner.session.account_id.clone();
        // The server's search, and the newest messages (an index may lag behind an import).
        let responses = self
            .inner
            .call(vec![
                (
                    "Email/query",
                    json!({
                        "accountId": account,
                        "filter": {"operator": "AND", "conditions": [
                            {"inMailbox": source},
                            {"header": ["Message-ID", message_id]},
                        ]},
                        "limit": 100,
                    }),
                    "h",
                ),
                (
                    "Email/query",
                    json!({
                        "accountId": account,
                        "filter": {"inMailbox": source},
                        "sort": [{"property": "receivedAt", "isAscending": false}],
                        "limit": 50,
                    }),
                    "n",
                ),
            ])
            .await?;
        let mut ids: Vec<String> = Vec::new();
        for call in ["h", "n"] {
            // A server without header search still answers the newest messages.
            if let Ok(args) = responses.get(call, "Email/query") {
                ids.extend(
                    args.get("ids")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned),
                );
            }
        }
        ids.sort();
        ids.dedup();
        let list = self
            .inner
            .get_emails(&ids, &["id", "messageId"], None)
            .await?;
        let matching: Vec<String> = list
            .iter()
            .filter(|e| {
                e.get("messageId")
                    .and_then(Value::as_array)
                    .is_some_and(|l| l.iter().any(|m| m.as_str() == Some(message_id)))
            })
            .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect();
        if matching.is_empty() {
            return Ok(UidSet::new());
        }
        let mut map = self.inner.map.lock().await;
        map.prepare(&account).await?;
        map.refresh(&self.inner.client, &self.inner.session).await?;
        let Some(numbers) = map.mailbox(&source) else {
            return Ok(UidSet::new());
        };
        Ok(UidSet::from_uids(
            matching.iter().filter_map(|eid| numbers.uid_of(eid)),
        ))
    }
}
