//! JMAP mailboxes (RFC 8621 section 2) as the engine's folder list: wire names are decoded
//! paths with `/` as delimiter, the inbox is `INBOX`, roles become SPECIAL-USE attributes.

use std::collections::HashMap;

use mail_model::{MailboxRole, MailboxStatus, RemoteMailbox};
use serde_json::Value;

/// The delimiter of the paths the adapter builds.
pub(crate) const DELIMITER: char = '/';

/// Properties fetched with `Mailbox/get`.
pub(crate) const PROPERTIES: &[&str] = &[
    "id",
    "name",
    "parentId",
    "role",
    "totalEmails",
    "unreadEmails",
];

/// One JMAP mailbox with its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JmapMailbox {
    pub id: String,
    /// Wire name and display path (`INBOX` for the inbox, `Projects/2026` below it).
    pub path: String,
    pub role: Option<String>,
    pub total: u32,
    pub unread: u32,
}

impl JmapMailbox {
    /// The engine's role of a JMAP role (RFC 8621 refers to the IANA IMAP mailbox names).
    pub(crate) fn model_role(&self) -> MailboxRole {
        match self.role.as_deref() {
            Some("inbox") => MailboxRole::Inbox,
            Some("sent") => MailboxRole::Sent,
            Some("drafts") => MailboxRole::Drafts,
            Some("archive") => MailboxRole::Archive,
            Some("trash") => MailboxRole::Trash,
            Some("junk") => MailboxRole::Junk,
            Some("all") => MailboxRole::All,
            Some("flagged") => MailboxRole::Flagged,
            _ => MailboxRole::Custom,
        }
    }

    pub(crate) fn remote(&self, status: Option<MailboxStatus>) -> RemoteMailbox {
        let attribute = match self.model_role() {
            MailboxRole::Sent => Some("\\Sent"),
            MailboxRole::Drafts => Some("\\Drafts"),
            MailboxRole::Archive => Some("\\Archive"),
            MailboxRole::Trash => Some("\\Trash"),
            MailboxRole::Junk => Some("\\Junk"),
            MailboxRole::All => Some("\\All"),
            MailboxRole::Flagged => Some("\\Flagged"),
            MailboxRole::Inbox | MailboxRole::Custom => None,
        };
        RemoteMailbox {
            name: self.path.clone(),
            path: self.path.clone(),
            delimiter: Some(DELIMITER),
            attributes: attribute.map(str::to_owned).into_iter().collect(),
            status,
        }
    }
}

/// The JMAP role for a role the engine asks to create (`Mailbox/set`).
pub(crate) fn jmap_role(role: MailboxRole) -> Option<&'static str> {
    match role {
        MailboxRole::Inbox => Some("inbox"),
        MailboxRole::Sent => Some("sent"),
        MailboxRole::Drafts => Some("drafts"),
        MailboxRole::Archive => Some("archive"),
        MailboxRole::Trash => Some("trash"),
        MailboxRole::Junk => Some("junk"),
        MailboxRole::All => Some("all"),
        MailboxRole::Flagged => Some("flagged"),
        MailboxRole::Custom => None,
    }
}

/// Builds the paths of a `Mailbox/get` list. A name containing the delimiter keeps it (it then
/// reads like a deeper path; JMAP allows it, IMAP servers rarely). Mailboxes whose parent chain
/// is broken or cyclic hang at the top.
pub(crate) fn from_list(list: &[Value]) -> Vec<JmapMailbox> {
    struct Raw<'a> {
        name: &'a str,
        parent: Option<&'a str>,
        role: Option<&'a str>,
        total: u32,
        unread: u32,
    }
    let count = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
    };
    let mut raw: HashMap<&str, Raw<'_>> = HashMap::new();
    let mut order = Vec::new();
    for item in list {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        order.push(id);
        raw.insert(
            id,
            Raw {
                name: item.get("name").and_then(Value::as_str).unwrap_or(id),
                parent: item.get("parentId").and_then(Value::as_str),
                role: item.get("role").and_then(Value::as_str),
                total: count(item, "totalEmails"),
                unread: count(item, "unreadEmails"),
            },
        );
    }
    let segment = |r: &Raw<'_>| {
        if r.role == Some("inbox") {
            "INBOX".to_owned()
        } else {
            r.name.to_owned()
        }
    };
    let mut out = Vec::with_capacity(order.len());
    for id in order {
        let Some(item) = raw.get(id) else { continue };
        let mut segments = vec![segment(item)];
        let mut parent = item.parent;
        let mut depth = 0;
        while let Some(pid) = parent {
            depth += 1;
            let Some(p) = raw.get(pid).filter(|_| depth <= 64 && pid != id) else {
                break;
            };
            segments.push(segment(p));
            parent = p.parent;
        }
        segments.reverse();
        out.push(JmapMailbox {
            id: id.to_owned(),
            path: segments.join(&DELIMITER.to_string()),
            role: item.role.map(str::to_owned),
            total: item.total,
            unread: item.unread,
        });
    }
    // Two mailboxes with the same path (possible when a name contains the delimiter): the
    // later ones get the id appended, so every wire name is unique.
    let mut seen = HashMap::new();
    for mailbox in &mut out {
        let n = seen.entry(mailbox.path.to_lowercase()).or_insert(0u32);
        *n += 1;
        if *n > 1 {
            mailbox.path = format!("{} ({})", mailbox.path, mailbox.id);
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn stalwart_s_mailboxes_become_paths_with_roles() {
        // Mailbox/get of the seeded alice (Stalwart 1.1.1, abridged).
        let list = json!([
            {"name": "Trash", "role": "trash", "parentId": null, "totalEmails": 0, "unreadEmails": 0, "id": "b"},
            {"name": "Projects", "role": null, "parentId": null, "totalEmails": 0, "id": "h"},
            {"name": "Junk", "role": "junk", "parentId": null, "totalEmails": 3, "id": "c"},
            {"name": "2026", "role": null, "parentId": "h", "totalEmails": 4, "id": "i"},
            {"name": "Überprüfung", "role": null, "parentId": "h", "totalEmails": 1, "id": "j"},
            {"name": "Sent", "role": "sent", "parentId": null, "totalEmails": 32, "id": "e"},
            {"name": "Inbox", "role": "inbox", "parentId": null, "totalEmails": 141, "unreadEmails": 70, "id": "a"},
            {"name": "Lost", "parentId": "gone", "id": "x"}
        ]);
        let mailboxes = from_list(list.as_array().expect("array"));
        let paths: Vec<&str> = mailboxes.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "INBOX",
                "Junk",
                "Lost",
                "Projects",
                "Projects/2026",
                "Projects/Überprüfung",
                "Sent",
                "Trash"
            ]
        );
        let inbox = mailboxes.iter().find(|m| m.id == "a").expect("inbox");
        assert_eq!(inbox.model_role(), MailboxRole::Inbox);
        assert_eq!((inbox.total, inbox.unread), (141, 70));
        let sent = mailboxes.iter().find(|m| m.id == "e").expect("sent");
        let remote = sent.remote(None);
        assert!(remote.has_attribute("\\Sent"));
        assert_eq!(remote.delimiter, Some('/'));
        let nested = mailboxes.iter().find(|m| m.id == "i").expect("nested");
        assert_eq!(nested.remote(None).leaf_name(), "2026");
        assert_eq!(jmap_role(MailboxRole::Archive), Some("archive"));
        assert_eq!(jmap_role(MailboxRole::Custom), None);
    }

    #[test]
    fn cycles_and_duplicate_paths_stay_unique() {
        let list = json!([
            {"id": "p", "name": "A", "parentId": "q"},
            {"id": "q", "name": "B", "parentId": "p"},
            {"id": "r", "name": "x/y"},
            {"id": "s", "name": "y", "parentId": "t"},
            {"id": "t", "name": "x"}
        ]);
        let mailboxes = from_list(list.as_array().expect("array"));
        assert_eq!(mailboxes.len(), 5);
        let mut paths: Vec<String> = mailboxes.iter().map(|m| m.path.clone()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 5, "{paths:?}");
    }
}
