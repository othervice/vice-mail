//! [`JmapBackend`]: `MailBackend` over JMAP. One backend is one authenticated HTTP client with
//! the session resource; mailboxes are "opened" by bringing the account's UID map up to date
//! (see `map`), so the engine sees what an EXAMINE would show.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use mail_model::search::ServerCriterion;
use mail_model::{
    Access, BackendError, BodySection, Capabilities, FetchedSection, FlagUpdate, IdleOutcome,
    MailBackend, MessageHeader, MessageStructure, ModSeq, RawMessage, RemoteMailbox,
    SelectedMailbox, Uid, UidSet, UidValidity,
};
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::capability;
use crate::client::Client;
use crate::email::{self, LeafPart};
use crate::mailbox::{self, JmapMailbox};
use crate::map::{AccountMap, MailboxState};
use crate::push;
use crate::session::Session;

pub(crate) const USING: &[&str] = &[capability::CORE, capability::MAIL];

/// How often the event source is asked to ping (servers may ignore it).
const PING_SECS: u64 = 30;

/// The mailbox the engine opened last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selected {
    pub name: String,
    pub mailbox: String,
    pub uid_validity: UidValidity,
}

/// One JMAP connection with read access. See the module docs.
pub struct JmapBackend {
    pub(crate) client: Client,
    pub(crate) session: Arc<Session>,
    caps: Capabilities,
    pub(crate) map: Arc<tokio::sync::Mutex<AccountMap>>,
    pub(crate) mailboxes: Vec<JmapMailbox>,
    pub(crate) selected: Option<Selected>,
    /// The Email state this connection last looked at (push compares against it).
    seen_state: Option<String>,
    /// The parts of the message whose structure was fetched last: (mailbox, UID, parts).
    parts: Option<(String, Uid, Vec<LeafPart>)>,
}

impl JmapBackend {
    pub(crate) fn new(
        client: Client,
        session: Session,
        map: Arc<tokio::sync::Mutex<AccountMap>>,
    ) -> Self {
        let caps = session.engine_capabilities();
        Self {
            client,
            session: Arc::new(session),
            caps,
            map,
            mailboxes: Vec::new(),
            selected: None,
            seen_state: None,
            parts: None,
        }
    }

    pub(crate) async fn call(
        &self,
        calls: Vec<crate::client::Call>,
    ) -> Result<crate::client::Responses, BackendError> {
        self.client.call(&self.session.api_url, USING, calls).await
    }

    /// `Mailbox/get` of every mailbox; the paths are the wire names.
    pub(crate) async fn load_mailboxes(&mut self) -> Result<(), BackendError> {
        let responses = self
            .call(vec![(
                "Mailbox/get",
                json!({
                    "accountId": self.session.account_id,
                    "ids": null,
                    "properties": mailbox::PROPERTIES,
                }),
                "m",
            )])
            .await?;
        let list = responses
            .get("m", "Mailbox/get")?
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.mailboxes = mailbox::from_list(&list);
        Ok(())
    }

    /// The mailbox with this wire name (`INBOX` in any case for the inbox), listing the
    /// mailboxes again once if it is unknown.
    pub(crate) async fn resolve(&mut self, name: &str) -> Result<JmapMailbox, BackendError> {
        let find = |list: &[JmapMailbox]| {
            list.iter()
                .find(|m| {
                    m.path == name
                        || (name.eq_ignore_ascii_case("INBOX")
                            && m.role.as_deref() == Some("inbox"))
                })
                .cloned()
        };
        if let Some(found) = find(&self.mailboxes) {
            return Ok(found);
        }
        self.load_mailboxes().await?;
        find(&self.mailboxes).ok_or_else(|| BackendError::MailboxNotFound(name.to_owned()))
    }

    /// Brings the map up to date and numbers the mailbox if needed; its state afterwards.
    pub(crate) async fn mailbox_state(
        &mut self,
        mailbox: &str,
    ) -> Result<MailboxState, BackendError> {
        let mut map = self.map.lock().await;
        map.prepare(&self.session.account_id).await?;
        map.refresh(&self.client, &self.session).await?;
        map.fill(&self.client, &self.session, mailbox).await?;
        self.seen_state = map.state().map(str::to_owned);
        map.selected(mailbox)
            .ok_or_else(|| BackendError::InvalidState(format!("no map for mailbox {mailbox}")))
    }

    /// The mailbox of `selected` if it is the open one.
    pub(crate) fn check(&self, selected: &SelectedMailbox) -> Result<String, BackendError> {
        match &self.selected {
            Some(open)
                if open.name == selected.name && open.uid_validity == selected.uid_validity =>
            {
                Ok(open.mailbox.clone())
            }
            _ => Err(BackendError::InvalidState(format!(
                "mailbox {} is not the open one",
                selected.name
            ))),
        }
    }

    /// (UID, Email id, modseq) of the requested UIDs.
    pub(crate) async fn lookup(&self, mailbox: &str, uids: &UidSet) -> Vec<(Uid, String, u64)> {
        self.map.lock().await.lookup(mailbox, uids)
    }

    /// `Email/get` of `ids` with these properties, in chunks; the entries by id.
    pub(crate) async fn get_emails(
        &self,
        ids: &[String],
        properties: &[&str],
        extra: Option<Value>,
    ) -> Result<Vec<Value>, BackendError> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(self.session.max_objects_in_get) {
            let mut args = json!({
                "accountId": self.session.account_id,
                "ids": chunk,
                "properties": properties,
            });
            if let (Some(Value::Object(extra)), Some(map)) = (&extra, args.as_object_mut()) {
                for (k, v) in extra {
                    map.insert(k.clone(), v.clone());
                }
            }
            let responses = self.call(vec![("Email/get", args, "g")]).await?;
            let list = responses
                .get("g", "Email/get")?
                .get("list")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            out.extend(list);
        }
        Ok(out)
    }

    async fn blob(
        &self,
        blob_id: &str,
        name: &str,
        kind: &str,
    ) -> Result<Option<Vec<u8>>, BackendError> {
        let url = self.session.download(blob_id, name, kind)?;
        self.client.download(&url).await
    }

    /// The parts of one message, from the cache or the server.
    async fn leaves(
        &mut self,
        mailbox: &str,
        uid: Uid,
    ) -> Result<Option<Vec<LeafPart>>, BackendError> {
        if let Some((m, u, parts)) = &self.parts
            && m == mailbox
            && *u == uid
        {
            return Ok(Some(parts.clone()));
        }
        Ok(self
            .structure_of(mailbox, uid)
            .await?
            .map(|(_, leaves)| leaves))
    }

    async fn structure_of(
        &mut self,
        mailbox: &str,
        uid: Uid,
    ) -> Result<Option<(MessageStructure, Vec<LeafPart>)>, BackendError> {
        let Some((_, id, _)) = self
            .lookup(mailbox, &UidSet::from_uids([uid]))
            .await
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let list = self
            .get_emails(
                &[id],
                &["id", "size", "bodyStructure"],
                Some(json!({"bodyProperties": email::PART_PROPERTIES})),
            )
            .await?;
        let Some(found) = list.first() else {
            return Ok(None);
        };
        let size = found
            .get("size")
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
        let body = found.get("bodyStructure").cloned().unwrap_or(Value::Null);
        let (structure, leaves) = email::structure(size, &body);
        self.parts = Some((mailbox.to_owned(), uid, leaves.clone()));
        Ok(Some((structure, leaves)))
    }

    /// The server's current Email state.
    async fn current_state(&self) -> Result<String, BackendError> {
        AccountMap::current_state(&self.client, &self.session).await
    }
}

/// A search criterion as a JMAP `FilterCondition` (RFC 8621 section 4.4.1).
pub(crate) fn filter(criterion: &ServerCriterion) -> Value {
    match criterion {
        ServerCriterion::Text(s) => json!({"text": s}),
        ServerCriterion::From(s) => json!({"from": s}),
        ServerCriterion::To(s) => json!({"to": s}),
        ServerCriterion::Cc(s) => json!({"cc": s}),
        ServerCriterion::Subject(s) => json!({"subject": s}),
        ServerCriterion::Since(d) => json!({"after": email::utc_midnight(d.year, d.month, d.day)}),
        ServerCriterion::Before(d) => {
            json!({"before": email::utc_midnight(d.year, d.month, d.day)})
        }
        ServerCriterion::Seen(true) => json!({"hasKeyword": "$seen"}),
        ServerCriterion::Seen(false) => json!({"notKeyword": "$seen"}),
        ServerCriterion::Flagged(true) => json!({"hasKeyword": "$flagged"}),
        ServerCriterion::Flagged(false) => json!({"notKeyword": "$flagged"}),
        ServerCriterion::Not(inner) => json!({"operator": "NOT", "conditions": [filter(inner)]}),
        ServerCriterion::Or(a, b) => {
            json!({"operator": "OR", "conditions": [filter(a), filter(b)]})
        }
    }
}

impl MailBackend for JmapBackend {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn access(&self) -> Access {
        self.client.access()
    }

    async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
        self.load_mailboxes().await?;
        let mut map = self.map.lock().await;
        map.prepare(&self.session.account_id).await?;
        let ids: HashSet<&str> = self.mailboxes.iter().map(|m| m.id.as_str()).collect();
        map.retain_mailboxes(&ids);
        map.flush().await?;
        Ok(self
            .mailboxes
            .iter()
            .map(|m| {
                let known = map.selected(&m.id);
                m.remote(Some(mail_model::MailboxStatus {
                    messages: m.total,
                    unseen: Some(m.unread),
                    uid_next: known.map(|k| k.uid_next),
                    uid_validity: known.map(|k| UidValidity(k.uid_validity)),
                    highest_modseq: known.map(|k| ModSeq(k.modseq)),
                }))
            })
            .collect())
    }

    async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        self.selected = None;
        let found = self.resolve(name).await?;
        let state = self.mailbox_state(&found.id).await?;
        let wire = if found.role.as_deref() == Some("inbox") {
            "INBOX".to_owned()
        } else {
            found.path.clone()
        };
        let selected = SelectedMailbox {
            name: if name.eq_ignore_ascii_case("INBOX") {
                name.to_owned()
            } else {
                wire
            },
            uid_validity: UidValidity(state.uid_validity),
            uid_next: state.uid_next,
            exists: state.exists,
            highest_modseq: Some(ModSeq(state.modseq)),
            read_only: self.client.access() == Access::ReadOnly,
            keywords_allowed: true,
        };
        self.selected = Some(Selected {
            name: selected.name.clone(),
            mailbox: found.id,
            uid_validity: selected.uid_validity,
        });
        Ok(selected)
    }

    async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
        let id = self.check(mailbox)?;
        Ok(self.map.lock().await.uids(&id))
    }

    async fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<MessageHeader>, BackendError> {
        let id = self.check(mailbox)?;
        let entries = self.lookup(&id, uids).await;
        let ids: Vec<String> = entries.iter().map(|(_, e, _)| e.clone()).collect();
        let list = self
            .get_emails(&ids, email::HEADER_PROPERTIES, None)
            .await?;
        let by_id: std::collections::HashMap<&str, &Value> = list
            .iter()
            .filter_map(|e| Some((e.get("id").and_then(Value::as_str)?, e)))
            .collect();
        Ok(entries
            .iter()
            .filter_map(|(uid, eid, modseq)| {
                by_id
                    .get(eid.as_str())
                    .map(|e| email::header(*uid, e, *modseq))
            })
            .collect())
    }

    async fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        changed_since: Option<ModSeq>,
    ) -> Result<Vec<FlagUpdate>, BackendError> {
        let id = self.check(mailbox)?;
        let entries: Vec<(Uid, String, u64)> = self
            .lookup(&id, uids)
            .await
            .into_iter()
            .filter(|(_, _, m)| changed_since.is_none_or(|since| *m > since.0))
            .collect();
        let ids: Vec<String> = entries.iter().map(|(_, e, _)| e.clone()).collect();
        let list = self.get_emails(&ids, &["id", "keywords"], None).await?;
        let by_id: std::collections::HashMap<&str, &Value> = list
            .iter()
            .filter_map(|e| Some((e.get("id").and_then(Value::as_str)?, e)))
            .collect();
        Ok(entries
            .iter()
            .filter_map(|(uid, eid, modseq)| {
                by_id.get(eid.as_str()).map(|e| FlagUpdate {
                    uid: *uid,
                    flags: email::flags_of(e.get("keywords")),
                    modseq: Some(ModSeq(*modseq)),
                    gmail_labels: None,
                })
            })
            .collect())
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
        self.check(mailbox)?;
        let url = self.session.events(PING_SECS).ok_or_else(|| {
            BackendError::Unsupported("push (the server names no event source)".into())
        })??;
        // Changes since this connection last looked do not wake the event source.
        let current = self.current_state().await?;
        if self.seen_state.as_deref() != Some(current.as_str()) {
            self.seen_state = Some(current);
            return Ok(IdleOutcome::Changed);
        }
        let mut response = self.client.event_stream(&url).await?;
        let mut parser = push::Parser::default();
        let deadline = Instant::now() + max;
        let stop = std::pin::pin!(stop);
        let mut stop = stop;
        loop {
            tokio::select! {
                biased;
                () = &mut stop => return Ok(IdleOutcome::Stopped),
                () = tokio::time::sleep_until(deadline) => return Ok(IdleOutcome::TimedOut),
                chunk = response.chunk() => {
                    let chunk = chunk.map_err(|err| crate::error::from_http(&err))?;
                    let Some(bytes) = chunk else {
                        return Err(BackendError::ConnectionLost("the event source closed".into()));
                    };
                    for event in parser.push(&bytes) {
                        if let Some(state) = push::email_state(&event, &self.session.account_id)
                            && self.seen_state.as_deref() != Some(state.as_str())
                        {
                            self.seen_state = Some(state);
                            return Ok(IdleOutcome::Changed);
                        }
                    }
                }
            }
        }
    }

    async fn fetch_structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<MessageStructure>, BackendError> {
        let id = self.check(mailbox)?;
        Ok(self.structure_of(&id, uid).await?.map(|(s, _)| s))
    }

    async fn fetch_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<RawMessage>, BackendError> {
        let id = self.check(mailbox)?;
        let entries = self.lookup(&id, uids).await;
        let ids: Vec<String> = entries.iter().map(|(_, e, _)| e.clone()).collect();
        let list = self.get_emails(&ids, &["id", "blobId"], None).await?;
        let mut out = Vec::with_capacity(list.len());
        for (uid, eid, _) in &entries {
            let Some(blob) = list
                .iter()
                .find(|e| e.get("id").and_then(Value::as_str) == Some(eid.as_str()))
                .and_then(|e| e.get("blobId"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if let Some(data) = self.blob(blob, "message.eml", "message/rfc822").await? {
                out.push(RawMessage { uid: *uid, data });
            }
        }
        Ok(out)
    }

    async fn fetch_sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
        let id = self.check(mailbox)?;
        let Some(leaves) = self.leaves(&id, uid).await? else {
            return Ok(None);
        };
        let mut out = Vec::with_capacity(sections.len());
        for section in sections {
            let data = match section {
                BodySection::Header => {
                    let Some((_, eid, _)) = self
                        .lookup(&id, &UidSet::from_uids([uid]))
                        .await
                        .into_iter()
                        .next()
                    else {
                        return Ok(None);
                    };
                    let list = self.get_emails(&[eid], &["id", "headers"], None).await?;
                    let Some(found) = list.first() else {
                        return Ok(None);
                    };
                    email::header_section(found)
                }
                BodySection::Mime(path) => {
                    let Some(leaf) = leaves.iter().find(|l| &l.path == path) else {
                        continue;
                    };
                    leaf.mime_header.clone()
                }
                BodySection::Part(path) => {
                    let Some(leaf) = leaves.iter().find(|l| &l.path == path) else {
                        continue;
                    };
                    match self.blob(&leaf.blob_id, "part", &leaf.mime_type).await? {
                        Some(data) => data,
                        None => continue,
                    }
                }
            };
            out.push(FetchedSection {
                section: section.clone(),
                data,
            });
        }
        Ok(Some(out))
    }

    async fn search_text(
        &mut self,
        mailbox: &SelectedMailbox,
        criteria: &[ServerCriterion],
    ) -> Result<UidSet, BackendError> {
        let id = self.check(mailbox)?;
        let mut conditions = vec![json!({"inMailbox": id})];
        conditions.extend(criteria.iter().map(filter));
        let mut found = Vec::new();
        let mut position = 0usize;
        loop {
            let responses = self
                .call(vec![(
                    "Email/query",
                    json!({
                        "accountId": self.session.account_id,
                        "filter": {"operator": "AND", "conditions": conditions},
                        "position": position,
                        "limit": 5000,
                    }),
                    "q",
                )])
                .await?;
            let page: Vec<String> = responses
                .get("q", "Email/query")?
                .get("ids")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if page.is_empty() || found.len() > 100_000 {
                break;
            }
            position += page.len();
            found.extend(page);
        }
        let map = self.map.lock().await;
        let Some(mailbox) = map.mailbox(&id) else {
            return Ok(UidSet::new());
        };
        Ok(UidSet::from_uids(
            found.iter().filter_map(|eid| mailbox.uid_of(eid)),
        ))
    }

    /// Stage 20 (decision 0060): `Quota/get` (RFC 9425) when the session offers quotas; the
    /// `octets` quota for mail, the account's own before a wider one.
    async fn quota(&mut self) -> Result<Option<mail_model::StorageQuota>, BackendError> {
        if !self
            .session
            .capabilities
            .iter()
            .any(|c| c == crate::capability::QUOTA)
        {
            return Ok(None);
        }
        let responses = self
            .client
            .call(
                &self.session.api_url,
                &[crate::capability::CORE, crate::capability::QUOTA],
                vec![(
                    "Quota/get",
                    json!({ "accountId": self.session.account_id, "ids": null }),
                    "q",
                )],
            )
            .await?;
        let list = responses
            .get("q", "Quota/get")?
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(storage_quota(&list))
    }

    async fn noop(&mut self) -> Result<(), BackendError> {
        self.call(vec![("Core/echo", json!({}), "e")])
            .await?
            .get("e", "Core/echo")
            .map(drop)
    }

    async fn logout(self) -> Result<(), BackendError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mail_model::search::CivilDate;

    use super::*;

    #[test]
    fn search_criteria_become_filters() {
        let date = CivilDate::new(2026, 11, 2).expect("date");
        let criterion = ServerCriterion::Or(
            Box::new(ServerCriterion::From("bob".into())),
            Box::new(ServerCriterion::Not(Box::new(ServerCriterion::Seen(true)))),
        );
        assert_eq!(
            filter(&criterion),
            json!({"operator": "OR", "conditions": [
                {"from": "bob"},
                {"operator": "NOT", "conditions": [{"hasKeyword": "$seen"}]}
            ]})
        );
        assert_eq!(
            filter(&ServerCriterion::Since(date)),
            json!({"after": "2026-11-02T00:00:00Z"})
        );
        assert_eq!(
            filter(&ServerCriterion::Flagged(false)),
            json!({"notKeyword": "$flagged"})
        );
    }
}

/// Stage 20: the storage of a `Quota/get` list: `resourceType` `octets` for mail (`types`
/// naming `Mail`, or none), the scope `account` before `domain` and `global`; `None` without
/// one or without a limit.
pub(crate) fn storage_quota(list: &[Value]) -> Option<mail_model::StorageQuota> {
    let rank = |scope: Option<&str>| match scope {
        Some("account") => 0,
        Some("domain") => 1,
        _ => 2,
    };
    list.iter()
        .filter(|q| q.get("resourceType").and_then(Value::as_str) == Some("octets"))
        .filter(|q| {
            q.get("types")
                .and_then(Value::as_array)
                .is_none_or(|types| {
                    types.is_empty() || types.iter().any(|t| t.as_str() == Some("Mail"))
                })
        })
        .filter_map(|q| {
            let used = q.get("used").and_then(Value::as_u64)?;
            let limit = q
                .get("hardLimit")
                .and_then(Value::as_u64)
                .filter(|l| *l > 0)?;
            Some((rank(q.get("scope").and_then(Value::as_str)), used, limit))
        })
        .min_by_key(|(rank, ..)| *rank)
        .map(|(_, used_bytes, limit_bytes)| mail_model::StorageQuota {
            used_bytes,
            limit_bytes,
        })
}

#[cfg(test)]
mod quota_tests {
    use serde_json::json;

    use super::storage_quota;

    /// Stage 20 (decision 0060): RFC 9425 quotas: the account's octets quota for mail wins
    /// over a domain one; counts, other types and quotas without a limit say nothing.
    #[test]
    fn the_accounts_octets_quota_for_mail() {
        let list = vec![
            json!({ "id": "d", "resourceType": "octets", "used": 900, "hardLimit": 1000,
                    "scope": "domain", "types": ["Mail"], "name": "domain" }),
            json!({ "id": "c", "resourceType": "count", "used": 5, "hardLimit": 10,
                    "scope": "account", "types": ["Mail"], "name": "count" }),
            json!({ "id": "a", "resourceType": "octets", "used": 300, "hardLimit": 400,
                    "scope": "account", "types": ["Mail", "Calendar"], "name": "account" }),
        ];
        let quota = storage_quota(&list).expect("account quota");
        assert_eq!((quota.used_bytes, quota.limit_bytes), (300, 400));
        assert_eq!(
            storage_quota(&[json!({ "resourceType": "octets", "used": 1, "scope": "account" })]),
            None,
            "no limit"
        );
        assert_eq!(
            storage_quota(
                &[json!({ "resourceType": "octets", "used": 1, "hardLimit": 9,
                                    "scope": "account", "types": ["Contact"] })]
            ),
            None,
            "not for mail"
        );
        assert_eq!(storage_quota(&[]), None);
    }
}
