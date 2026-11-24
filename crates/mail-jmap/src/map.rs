//! The adapter's UID map (decision 0041): per JMAP mailbox a UIDVALIDITY, UIDs in the order the
//! messages arrived and the change counter of each, per account the Email state the map
//! reflects. Shared by every connection of an account (sync, IDLE, reading, outbox), kept in
//! memory and written through to the engine's store ([`UidMapStore`]).
//!
//! - **First look** at an account: the current Email state is recorded; each mailbox gets its
//!   numbers from a full `Email/query` (oldest first) when it is first opened.
//! - **Changes:** `Email/changes` since the recorded state, then `Email/get` of `mailboxIds`
//!   for every created or updated message: a message new in a complete mailbox gets the next
//!   UID, one that left it loses its entry, one still there gets the new counter value (its
//!   keywords may have changed). Destroyed messages lose every entry. Applying a change twice
//!   changes nothing, so a listing that already saw a message and a later change report agree.
//! - **`cannotCalculateChanges`:** every mailbox is listed again on its next open (UIDs of
//!   messages still there are kept) and every entry counts as changed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use mail_model::{
    AccountId, BackendError, UidMapChanges, UidMapEntry, UidMapMailbox, UidMapStore, UidSet,
};
use serde_json::{Value, json};

use crate::capability;
use crate::client::Client;
use crate::session::Session;

/// Changes asked for per `Email/changes` call.
const MAX_CHANGES: u64 = 1_000;

/// Ids per `Email/query` page (Stalwart caps a page at 5000).
const QUERY_PAGE: usize = 5_000;

const USING: &[&str] = &[capability::CORE, capability::MAIL];

/// The UID maps of all accounts, and the store they are written to once the engine attached it.
#[derive(Default)]
pub(crate) struct Maps {
    store: Mutex<Option<Arc<dyn UidMapStore>>>,
    accounts: Mutex<HashMap<AccountId, Arc<tokio::sync::Mutex<AccountMap>>>>,
}

impl Maps {
    pub(crate) fn attach(&self, store: Arc<dyn UidMapStore>) {
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = Some(store);
    }

    /// The account's shared map; a connection without account gets a map of its own that is
    /// never stored (account setup).
    pub(crate) fn for_account(
        &self,
        account: Option<AccountId>,
    ) -> Arc<tokio::sync::Mutex<AccountMap>> {
        let store = self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(account) = account else {
            return Arc::new(tokio::sync::Mutex::new(AccountMap::new(None, None)));
        };
        let mut accounts = self.accounts.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(accounts.entry(account).or_insert_with(|| {
            Arc::new(tokio::sync::Mutex::new(AccountMap::new(
                Some(account),
                store,
            )))
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: String,
    modseq: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MailboxMap {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub complete: bool,
    by_uid: BTreeMap<u32, Entry>,
    by_id: HashMap<String, u32>,
}

impl MailboxMap {
    pub(crate) fn len(&self) -> u32 {
        u32::try_from(self.by_uid.len()).unwrap_or(u32::MAX)
    }

    pub(crate) fn uid_of(&self, id: &str) -> Option<u32> {
        self.by_id.get(id).copied()
    }
}

/// What [`AccountMap::selected`] reports of a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MailboxState {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub exists: u32,
    pub modseq: u64,
}

/// One account's map.
pub(crate) struct AccountMap {
    account: Option<AccountId>,
    store: Option<Arc<dyn UidMapStore>>,
    loaded: bool,
    remote: String,
    state: Option<String>,
    modseq: u64,
    max_validity: u32,
    mailboxes: HashMap<String, MailboxMap>,
    pending: UidMapChanges,
    dirty_mailboxes: BTreeSet<String>,
    state_dirty: bool,
}

fn now_secs() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

impl AccountMap {
    fn new(account: Option<AccountId>, store: Option<Arc<dyn UidMapStore>>) -> Self {
        Self {
            account,
            store,
            loaded: false,
            remote: String::new(),
            state: None,
            modseq: 1,
            max_validity: 0,
            mailboxes: HashMap::new(),
            pending: UidMapChanges::default(),
            dirty_mailboxes: BTreeSet::new(),
            state_dirty: false,
        }
    }

    pub(crate) fn state(&self) -> Option<&str> {
        self.state.as_deref()
    }

    pub(crate) fn mailbox(&self, id: &str) -> Option<&MailboxMap> {
        self.mailboxes.get(id)
    }

    fn forget_memory(&mut self) {
        self.loaded = false;
        self.state = None;
        self.mailboxes.clear();
        self.pending = UidMapChanges::default();
        self.dirty_mailboxes.clear();
        self.state_dirty = false;
    }

    fn reset(&mut self, remote: &str) {
        self.remote = remote.to_owned();
        self.state = None;
        self.modseq = self.modseq.max(1);
        self.mailboxes.clear();
        self.pending = UidMapChanges {
            reset: true,
            ..UidMapChanges::default()
        };
        self.dirty_mailboxes.clear();
        self.state_dirty = true;
    }

    /// Loads the stored map once; a map of another server account is discarded.
    pub(crate) async fn prepare(&mut self, remote: &str) -> Result<(), BackendError> {
        if self.loaded && self.remote == remote {
            return Ok(());
        }
        if self.loaded {
            tracing::info!(account = ?self.account, "JMAP account id changed; numbering anew");
            self.reset(remote);
            return self.flush().await;
        }
        let snapshot = match (&self.store, self.account) {
            (Some(store), Some(account)) => store.load_uid_map(account).await?,
            _ => None,
        };
        self.loaded = true;
        match snapshot {
            Some(snapshot) if snapshot.remote_account == remote => {
                self.remote = snapshot.remote_account;
                self.state = snapshot.state;
                self.modseq = snapshot.modseq.max(1);
                for (mailbox, entries) in snapshot.mailboxes {
                    self.max_validity = self.max_validity.max(mailbox.uid_validity);
                    let mut map = MailboxMap {
                        uid_validity: mailbox.uid_validity,
                        uid_next: mailbox.uid_next,
                        complete: mailbox.complete,
                        ..MailboxMap::default()
                    };
                    for entry in entries {
                        map.by_id.insert(entry.remote_id.clone(), entry.uid);
                        map.by_uid.insert(
                            entry.uid,
                            Entry {
                                id: entry.remote_id,
                                modseq: entry.modseq,
                            },
                        );
                    }
                    self.mailboxes.insert(mailbox.mailbox, map);
                }
                Ok(())
            }
            Some(_) | None => {
                self.reset(remote);
                Ok(())
            }
        }
    }

    /// Writes what changed since the last flush. On failure the memory is dropped and loaded
    /// again on the next use, so memory and store never disagree for long.
    pub(crate) async fn flush(&mut self) -> Result<(), BackendError> {
        if self.pending.is_empty() && self.dirty_mailboxes.is_empty() && !self.state_dirty {
            return Ok(());
        }
        let mut changes = std::mem::take(&mut self.pending);
        changes.remote_account = self.remote.clone();
        changes.state = self.state.clone();
        changes.modseq = self.modseq;
        for id in std::mem::take(&mut self.dirty_mailboxes) {
            if let Some(map) = self.mailboxes.get(&id) {
                changes.mailboxes.push(UidMapMailbox {
                    mailbox: id,
                    uid_validity: map.uid_validity,
                    uid_next: map.uid_next,
                    complete: map.complete,
                });
            }
        }
        self.state_dirty = false;
        let (Some(store), Some(account)) = (&self.store, self.account) else {
            return Ok(());
        };
        if let Err(err) = store.save_uid_map(account, changes).await {
            tracing::warn!(account = %account, error = %err, "JMAP map not stored; reloading it");
            self.forget_memory();
            return Err(err);
        }
        Ok(())
    }

    fn next_validity(&mut self) -> u32 {
        let validity = now_secs().max(self.max_validity.saturating_add(1)).max(1);
        self.max_validity = validity;
        validity
    }

    /// The mailbox's map, created empty (incomplete) if it has none.
    pub(crate) fn ensure_mailbox(&mut self, id: &str) {
        if !self.mailboxes.contains_key(id) {
            let uid_validity = self.next_validity();
            self.mailboxes.insert(
                id.to_owned(),
                MailboxMap {
                    uid_validity,
                    uid_next: 1,
                    ..MailboxMap::default()
                },
            );
            self.dirty_mailboxes.insert(id.to_owned());
        }
    }

    /// Drops the maps of mailboxes the server no longer has.
    pub(crate) fn retain_mailboxes(&mut self, ids: &HashSet<&str>) {
        let gone: Vec<String> = self
            .mailboxes
            .keys()
            .filter(|id| !ids.contains(id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            self.mailboxes.remove(&id);
            self.dirty_mailboxes.remove(&id);
            self.pending.removed_mailboxes.push(id);
        }
    }

    pub(crate) fn selected(&self, id: &str) -> Option<MailboxState> {
        self.mailboxes.get(id).map(|map| MailboxState {
            uid_validity: map.uid_validity,
            uid_next: map.uid_next,
            exists: map.len(),
            modseq: self.modseq,
        })
    }

    pub(crate) fn uids(&self, id: &str) -> UidSet {
        self.mailboxes
            .get(id)
            .map(|map| UidSet::from_uids(map.by_uid.keys().copied()))
            .unwrap_or_default()
    }

    /// (UID, Email id, modseq) of the requested UIDs the mailbox has, in UID order.
    pub(crate) fn lookup(&self, id: &str, uids: &UidSet) -> Vec<(u32, String, u64)> {
        let Some(map) = self.mailboxes.get(id) else {
            return Vec::new();
        };
        // Walk the smaller side.
        if uids.len() < map.by_uid.len() as u64 {
            uids.iter()
                .filter_map(|uid| map.by_uid.get(&uid).map(|e| (uid, e.id.clone(), e.modseq)))
                .collect()
        } else {
            map.by_uid
                .iter()
                .filter(|(uid, _)| uids.contains(**uid))
                .map(|(uid, e)| (*uid, e.id.clone(), e.modseq))
                .collect()
        }
    }

    /// Gives `ids` (in this order) the next UIDs of the mailbox; ids it has keep theirs.
    pub(crate) fn assign(&mut self, mailbox: &str, ids: &[String]) -> Vec<u32> {
        self.ensure_mailbox(mailbox);
        let modseq = self.modseq;
        let Some(map) = self.mailboxes.get_mut(mailbox) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(uid) = map.by_id.get(id) {
                out.push(*uid);
                continue;
            }
            let uid = map.uid_next;
            map.uid_next = map.uid_next.saturating_add(1);
            map.by_id.insert(id.clone(), uid);
            map.by_uid.insert(
                uid,
                Entry {
                    id: id.clone(),
                    modseq,
                },
            );
            self.pending.upserts.push((
                mailbox.to_owned(),
                UidMapEntry {
                    uid,
                    remote_id: id.clone(),
                    modseq,
                },
            ));
            out.push(uid);
        }
        self.dirty_mailboxes.insert(mailbox.to_owned());
        out
    }

    /// Removes the message from the mailbox's numbering.
    pub(crate) fn remove(&mut self, mailbox: &str, id: &str) -> Option<u32> {
        let map = self.mailboxes.get_mut(mailbox)?;
        let uid = map.by_id.remove(id)?;
        map.by_uid.remove(&uid);
        self.pending.removed.push((mailbox.to_owned(), uid));
        Some(uid)
    }

    fn touch(&mut self, mailbox: &str, id: &str, modseq: u64) {
        let Some(map) = self.mailboxes.get_mut(mailbox) else {
            return;
        };
        let Some(uid) = map.by_id.get(id).copied() else {
            return;
        };
        if let Some(entry) = map.by_uid.get_mut(&uid)
            && entry.modseq != modseq
        {
            entry.modseq = modseq;
            self.pending.upserts.push((
                mailbox.to_owned(),
                UidMapEntry {
                    uid,
                    remote_id: id.to_owned(),
                    modseq,
                },
            ));
        }
    }

    /// The server's current Email state (`Email/get` with no ids).
    pub(crate) async fn current_state(
        client: &Client,
        session: &Session,
    ) -> Result<String, BackendError> {
        let responses = client
            .call(
                &session.api_url,
                USING,
                vec![(
                    "Email/get",
                    json!({"accountId": session.account_id, "ids": []}),
                    "s",
                )],
            )
            .await?;
        responses
            .get("s", "Email/get")?
            .get("state")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| BackendError::Protocol("Email/get without state".into()))
    }

    /// Brings the map to the server's current state (see the module docs). Returns whether
    /// anything changed.
    pub(crate) async fn refresh(
        &mut self,
        client: &Client,
        session: &Session,
    ) -> Result<bool, BackendError> {
        let Some(since) = self.state.clone() else {
            self.state = Some(Self::current_state(client, session).await?);
            self.state_dirty = true;
            self.flush().await?;
            return Ok(true);
        };
        let mut created: Vec<String> = Vec::new();
        let mut updated: Vec<String> = Vec::new();
        let mut destroyed: HashSet<String> = HashSet::new();
        let mut state = since.clone();
        loop {
            let responses = client
                .call(
                    &session.api_url,
                    USING,
                    vec![(
                        "Email/changes",
                        json!({
                            "accountId": session.account_id,
                            "sinceState": state,
                            "maxChanges": MAX_CHANGES,
                        }),
                        "c",
                    )],
                )
                .await?;
            let args = match responses.raw("c")? {
                Ok(args) => args,
                Err(err) if err.kind == "cannotCalculateChanges" => {
                    return self.rebuild(client, session).await.map(|()| true);
                }
                Err(err) => return Err(err.into_backend("Email/changes")),
            };
            let list = |key: &str| -> Vec<String> {
                args.get(key)
                    .and_then(Value::as_array)
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            created.extend(list("created"));
            updated.extend(list("updated"));
            destroyed.extend(list("destroyed"));
            let new_state = args
                .get("newState")
                .and_then(Value::as_str)
                .ok_or_else(|| BackendError::Protocol("Email/changes without newState".into()))?
                .to_owned();
            let more = args
                .get("hasMoreChanges")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let stuck = new_state == state;
            state = new_state;
            if !more || stuck {
                break;
            }
        }
        if state == since && created.is_empty() && updated.is_empty() && destroyed.is_empty() {
            return Ok(false);
        }
        let mut seen = HashSet::new();
        let changed: Vec<String> = created
            .into_iter()
            .chain(updated)
            .filter(|id| !destroyed.contains(id) && seen.insert(id.clone()))
            .collect();
        let found = Self::mailboxes_of(client, session, &changed).await?;
        for id in &changed {
            if !found.contains_key(id) {
                destroyed.insert(id.clone());
            }
        }
        self.apply(&found, &destroyed);
        self.state = Some(state);
        self.state_dirty = true;
        self.flush().await?;
        Ok(true)
    }

    /// Applies current mailbox memberships (`found`: id → (mailboxes, receivedAt)) and
    /// destroyed messages. Idempotent.
    fn apply(
        &mut self,
        found: &HashMap<String, (HashSet<String>, i64)>,
        destroyed: &HashSet<String>,
    ) {
        self.modseq += 1;
        let modseq = self.modseq;
        let mailbox_ids: Vec<String> = self.mailboxes.keys().cloned().collect();
        for mailbox in mailbox_ids {
            for id in destroyed {
                self.remove(&mailbox, id);
            }
            let complete = self.mailboxes.get(&mailbox).is_some_and(|m| m.complete);
            let mut arrived: Vec<(i64, &String)> = Vec::new();
            for (id, (boxes, received)) in found {
                let mapped = self
                    .mailboxes
                    .get(&mailbox)
                    .and_then(|m| m.uid_of(id))
                    .is_some();
                match (mapped, boxes.contains(&mailbox)) {
                    (true, true) => self.touch(&mailbox, id, modseq),
                    (true, false) => {
                        self.remove(&mailbox, id);
                    }
                    (false, true) if complete => arrived.push((*received, id)),
                    _ => {}
                }
            }
            arrived.sort();
            let ids: Vec<String> = arrived.into_iter().map(|(_, id)| id.clone()).collect();
            if !ids.is_empty() {
                self.assign(&mailbox, &ids);
            }
        }
    }

    /// `Email/get` of `mailboxIds` and `receivedAt`; missing ids are gone.
    async fn mailboxes_of(
        client: &Client,
        session: &Session,
        ids: &[String],
    ) -> Result<HashMap<String, (HashSet<String>, i64)>, BackendError> {
        let mut found = HashMap::new();
        for chunk in ids.chunks(session.max_objects_in_get) {
            let responses = client
                .call(
                    &session.api_url,
                    USING,
                    vec![(
                        "Email/get",
                        json!({
                            "accountId": session.account_id,
                            "ids": chunk,
                            "properties": ["id", "mailboxIds", "receivedAt"],
                        }),
                        "g",
                    )],
                )
                .await?;
            let args = responses.get("g", "Email/get")?;
            for email in args
                .get("list")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = email.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let boxes = email
                    .get("mailboxIds")
                    .and_then(Value::as_object)
                    .map(|m| {
                        m.iter()
                            .filter(|(_, on)| on.as_bool() == Some(true))
                            .map(|(k, _)| k.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let received = email
                    .get("receivedAt")
                    .and_then(Value::as_str)
                    .and_then(crate::email::parse_date)
                    .unwrap_or(0);
                found.insert(id.to_owned(), (boxes, received));
            }
        }
        Ok(found)
    }

    /// `cannotCalculateChanges`: every mailbox is listed again, every entry counts as changed.
    async fn rebuild(&mut self, client: &Client, session: &Session) -> Result<(), BackendError> {
        tracing::info!(account = ?self.account, "JMAP server cannot list changes; listing mailboxes again");
        let state = Self::current_state(client, session).await?;
        self.modseq += 1;
        let modseq = self.modseq;
        let all: Vec<(String, Vec<String>)> = self
            .mailboxes
            .iter()
            .map(|(m, map)| (m.clone(), map.by_id.keys().cloned().collect()))
            .collect();
        for (mailbox, ids) in all {
            for id in ids {
                self.touch(&mailbox, &id, modseq);
            }
            if let Some(map) = self.mailboxes.get_mut(&mailbox) {
                map.complete = false;
            }
            self.dirty_mailboxes.insert(mailbox);
        }
        self.state = Some(state);
        self.state_dirty = true;
        self.flush().await
    }

    /// Numbers a mailbox that is not complete yet: every message from a full listing, oldest
    /// first; numbers of messages it has keep theirs, entries no longer there go.
    pub(crate) async fn fill(
        &mut self,
        client: &Client,
        session: &Session,
        mailbox: &str,
    ) -> Result<(), BackendError> {
        self.ensure_mailbox(mailbox);
        if self.mailboxes.get(mailbox).is_some_and(|m| m.complete) {
            return Ok(());
        }
        let mut ids = Vec::new();
        for _attempt in 0..2 {
            let (listed, total) = list_mailbox(client, session, mailbox).await?;
            ids = listed;
            if total.is_none_or(|t| t == ids.len()) {
                break;
            }
            tracing::debug!(
                mailbox,
                "mailbox changed while it was listed; listing again"
            );
        }
        let present: HashSet<&String> = ids.iter().collect();
        let stale: Vec<String> = self
            .mailboxes
            .get(mailbox)
            .map(|m| {
                m.by_id
                    .keys()
                    .filter(|id| !present.contains(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for id in stale {
            self.remove(mailbox, &id);
        }
        self.assign(mailbox, &ids);
        if let Some(map) = self.mailboxes.get_mut(mailbox) {
            map.complete = true;
        }
        self.dirty_mailboxes.insert(mailbox.to_owned());
        self.flush().await
    }
}

/// Every Email id of a mailbox, oldest first, and the total the server reported.
pub(crate) async fn list_mailbox(
    client: &Client,
    session: &Session,
    mailbox: &str,
) -> Result<(Vec<String>, Option<usize>), BackendError> {
    let mut ids: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    let mut total;
    let mut position = 0usize;
    loop {
        let responses = client
            .call(
                &session.api_url,
                USING,
                vec![(
                    "Email/query",
                    json!({
                        "accountId": session.account_id,
                        "filter": {"inMailbox": mailbox},
                        "sort": [{"property": "receivedAt", "isAscending": true}],
                        "position": position,
                        "limit": QUERY_PAGE,
                        "calculateTotal": true,
                    }),
                    "q",
                )],
            )
            .await?;
        let args = responses.get("q", "Email/query")?;
        let page: Vec<String> = args
            .get("ids")
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        total = args
            .get("total")
            .and_then(Value::as_u64)
            .and_then(|t| usize::try_from(t).ok());
        let got = page.len();
        position += got;
        for id in page {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
        }
        let done = got == 0 || total.is_some_and(|t| position >= t);
        if done {
            break;
        }
    }
    Ok((ids, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> AccountMap {
        let mut map = AccountMap::new(None, None);
        map.loaded = true;
        map.remote = "b".into();
        map.state = Some("s1".into());
        map
    }

    fn found(entries: &[(&str, &[&str], i64)]) -> HashMap<String, (HashSet<String>, i64)> {
        entries
            .iter()
            .map(|(id, boxes, at)| {
                (
                    (*id).to_owned(),
                    (boxes.iter().map(|b| (*b).to_owned()).collect(), *at),
                )
            })
            .collect()
    }

    #[test]
    fn changes_assign_move_touch_and_remove_idempotently() {
        let mut map = map();
        map.ensure_mailbox("inbox");
        map.ensure_mailbox("archive");
        for m in ["inbox", "archive"] {
            map.mailboxes.get_mut(m).expect("map").complete = true;
        }
        assert_eq!(map.assign("inbox", &["e1".into(), "e2".into()]), [1, 2]);
        let start = map.modseq;

        // e3 and e4 arrive (assigned by arrival time), e1 moves to the archive, e2 changes.
        let changes = found(&[
            ("e4", &["inbox"], 20),
            ("e3", &["inbox"], 10),
            ("e1", &["archive"], 1),
            ("e2", &["inbox"], 2),
        ]);
        map.apply(&changes, &HashSet::new());
        let after = map.modseq;
        assert!(after > start);
        assert_eq!(map.uids("inbox"), UidSet::from_uids([2, 3, 4]));
        assert_eq!(map.mailbox("inbox").expect("inbox").uid_of("e3"), Some(3));
        assert_eq!(map.mailbox("inbox").expect("inbox").uid_of("e4"), Some(4));
        assert_eq!(map.uids("archive"), UidSet::from_uids([1]));
        let changed: Vec<u32> = map
            .lookup("inbox", &UidSet::range(1, 10))
            .into_iter()
            .filter(|(_, _, m)| *m > start)
            .map(|(uid, _, _)| uid)
            .collect();
        assert_eq!(changed, [2, 3, 4]);

        // The same report again changes no numbers.
        map.apply(&changes, &HashSet::new());
        assert_eq!(map.uids("inbox"), UidSet::from_uids([2, 3, 4]));
        assert_eq!(map.mailbox("inbox").expect("inbox").uid_next, 5);

        // Destroyed everywhere.
        map.apply(
            &HashMap::new(),
            &HashSet::from(["e1".to_owned(), "e3".to_owned()]),
        );
        assert_eq!(map.uids("inbox"), UidSet::from_uids([2, 4]));
        assert!(map.uids("archive").is_empty());
        assert_eq!(
            map.mailbox("archive").expect("archive").uid_next,
            2,
            "UIDs never reused"
        );
    }

    #[test]
    fn incomplete_mailboxes_only_lose_entries() {
        let mut map = map();
        map.ensure_mailbox("sent");
        map.apply(&found(&[("e9", &["sent"], 5)]), &HashSet::new());
        assert!(map.uids("sent").is_empty(), "the first listing numbers it");
        map.assign("sent", &["e9".into()]);
        map.apply(&found(&[("e9", &["trash"], 5)]), &HashSet::new());
        assert!(map.uids("sent").is_empty());
    }

    #[test]
    fn mailboxes_get_increasing_validities_and_vanish() {
        let mut map = map();
        map.ensure_mailbox("a");
        map.ensure_mailbox("b");
        let a = map.selected("a").expect("a").uid_validity;
        let b = map.selected("b").expect("b").uid_validity;
        assert!(b > a);
        map.retain_mailboxes(&HashSet::from(["b"]));
        assert!(map.selected("a").is_none());
        map.ensure_mailbox("a");
        assert!(
            map.selected("a").expect("a").uid_validity > b,
            "a new mailbox, a new validity"
        );
        assert_eq!(map.pending.removed_mailboxes, ["a"]);
    }
}
