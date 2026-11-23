//! Stage 9: inbox categories in the store (decisions 0016 and 0020).
//!
//! The store parses no mail. The engine gives it a [`Classifier`] (the rules of
//! `mail_content::classify`), which it calls on the writer thread when a new message is
//! stored, with the local facts only the database knows: the user's own addresses, how often
//! the user wrote to the sender (contacts), and the senders of the messages a new one answers.
//! The automatic result is kept with the message (`category_auto`, `category_reason`); the
//! user's sender rules ([`RuleSet`]) give the effective `category` and `important`. A changed
//! rule is applied again by a background job over all messages, newest first; messages stored
//! before stage 9 are classified the same way from their stored header data.

use std::collections::HashMap;
use std::sync::Arc;

use mail_model::{
    Address, CategoryReason, MessageCategory, MessageHeader, SenderKey, SenderKeyKind,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::StoreError;

/// Rows per step of the background jobs (classifying old messages, applying changed rules).
pub const CATEGORY_BATCH: u32 = 500;

/// Most `In-Reply-To`/`References` ids looked up for "answers a message of mine".
const MAX_REFERENCES_CHECKED: usize = 10;

/// The normalised keys the sender rules compare with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SenderKeys {
    /// `mail_content::normalize_address(..).key` of the first From address.
    pub address: Option<String>,
    /// Its domain, ASCII lower case.
    pub domain: Option<String>,
    /// The `List-Id` identifier, lower case.
    pub list_id: Option<String>,
}

/// What only the database knows about a new message's sender.
#[derive(Debug, Clone, Copy)]
pub struct LocalFacts<'a> {
    /// Addresses of all accounts and identities, as stored.
    pub own_addresses: &'a [String],
    /// Messages the user sent to the From address (contacts).
    pub sent_to_sender: u32,
    /// From addresses of stored messages the new one names in `In-Reply-To`/`References`.
    pub referenced_senders: &'a [String],
}

/// The automatic category of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub category: MessageCategory,
    pub reason: CategoryReason,
    /// `List-Unsubscribe-Post: List-Unsubscribe=One-Click` was present; `None` when the
    /// headers were not fetched (stored header data only).
    pub one_click: Option<bool>,
}

/// The category rules, supplied by the engine (`mail_content::classify`).
pub trait Classifier: Send + Sync {
    /// The keys of the sender rules for this message.
    fn sender_keys(&self, header: &MessageHeader) -> SenderKeys;
    /// The automatic category from the headers (and `header.category_headers` when fetched)
    /// and the local facts.
    fn classify(&self, header: &MessageHeader, local: &LocalFacts<'_>) -> Classified;
}

/// The user's sender rules, indexed by normalised key.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    addresses: HashMap<String, Rule>,
    domains: HashMap<String, Rule>,
    lists: HashMap<String, Rule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rule {
    category: Option<MessageCategory>,
    important: bool,
}

/// The effective category of a message and its important mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub category: MessageCategory,
    /// The sender rule's key, when a correction decided the category.
    pub corrected_by: Option<SenderKey>,
    pub important: Option<SenderKey>,
}

impl RuleSet {
    pub(crate) fn load(conn: &Connection) -> Result<Self, StoreError> {
        let mut set = Self::default();
        let mut stmt = conn
            .prepare_cached("SELECT key_kind, key_value, category, important FROM sender_rule")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        for row in rows {
            let (kind, value, category, important) = row?;
            let rule = Rule {
                category: category.and_then(MessageCategory::from_code),
                important,
            };
            match SenderKeyKind::parse(&kind) {
                Some(SenderKeyKind::Address) => set.addresses.insert(value, rule),
                Some(SenderKeyKind::Domain) => set.domains.insert(value, rule),
                Some(SenderKeyKind::ListId) => set.lists.insert(value, rule),
                None => None,
            };
        }
        Ok(set)
    }

    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty() && self.domains.is_empty() && self.lists.is_empty()
    }

    /// The first matching rule with `want`: the address, then the list, then the domain and
    /// its parents (most specific first), as `mail_content::SenderRules`.
    fn find(&self, keys: &SenderKeys, want: impl Fn(&Rule) -> bool) -> Option<(SenderKey, Rule)> {
        if let Some(address) = &keys.address
            && let Some(rule) = self.addresses.get(address).filter(|r| want(r))
        {
            return Some((SenderKey::address(address.clone()), *rule));
        }
        if let Some(list) = &keys.list_id
            && let Some(rule) = self.lists.get(list).filter(|r| want(r))
        {
            return Some((SenderKey::list_id(list.clone()), *rule));
        }
        let mut domain = keys.domain.as_deref()?;
        loop {
            if let Some(rule) = self.domains.get(domain).filter(|r| want(r)) {
                return Some((SenderKey::domain(domain.to_owned()), *rule));
            }
            domain = domain.split_once('.')?.1;
        }
    }

    /// The category after the user's correction, and the important mark.
    pub fn apply(&self, keys: &SenderKeys, automatic: MessageCategory) -> Effective {
        let correction = self.find(keys, |r| r.category.is_some());
        let important = self.find(keys, |r| r.important).map(|(key, _)| key);
        match correction {
            Some((key, rule)) => Effective {
                category: rule.category.unwrap_or(automatic),
                corrected_by: Some(key),
                important,
            },
            None => Effective {
                category: automatic,
                corrected_by: None,
                important,
            },
        }
    }
}

/// What `store_headers` needs for classifying, loaded once per window.
pub(crate) struct WindowClassifier {
    classifier: Arc<dyn Classifier>,
    own: Vec<String>,
    rules: RuleSet,
}

/// The columns of a classified new message.
pub(crate) struct CategoryColumns {
    pub category: i64,
    pub category_auto: i64,
    pub reason: String,
    pub important: bool,
    pub keys: SenderKeys,
    pub one_click: Option<bool>,
}

pub(crate) const OWN_ADDRESSES: &str = "SELECT email FROM account UNION SELECT email FROM identity";
pub(crate) const SENT_TO: &str = "SELECT sent FROM contact WHERE address_key = ?1";
pub(crate) const REFERENCED_FROM: &str =
    "SELECT from_addr FROM message WHERE account_id = ?1 AND message_id_hdr = ?2 LIMIT 3";

impl WindowClassifier {
    pub(crate) fn new(
        conn: &Connection,
        classifier: Arc<dyn Classifier>,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            classifier,
            own: own_addresses(conn)?,
            rules: RuleSet::load(conn)?,
        })
    }

    pub(crate) fn columns(
        &self,
        conn: &Connection,
        account_row: i64,
        header: &MessageHeader,
    ) -> Result<CategoryColumns, StoreError> {
        let keys = self.classifier.sender_keys(header);
        let sent_to_sender = match &keys.address {
            Some(address) => conn
                .prepare_cached(SENT_TO)?
                .query_row([address], |r| r.get::<_, i64>(0))
                .optional()?
                .map_or(0, |n| u32::try_from(n.max(0)).unwrap_or(u32::MAX)),
            None => 0,
        };
        let mut referenced = Vec::new();
        let ids = header
            .in_reply_to
            .iter()
            .chain(header.references.iter().rev())
            .take(MAX_REFERENCES_CHECKED);
        let mut stmt = conn.prepare_cached(REFERENCED_FROM)?;
        for id in ids {
            let rows =
                stmt.query_map(params![account_row, id], |r| r.get::<_, Option<String>>(0))?;
            for from in rows {
                if let Some(from) = from? {
                    referenced.push(from);
                }
            }
        }
        let classified = self.classifier.classify(
            header,
            &LocalFacts {
                own_addresses: &self.own,
                sent_to_sender,
                referenced_senders: &referenced,
            },
        );
        Ok(columns_of(&self.rules, keys, classified))
    }
}

fn columns_of(rules: &RuleSet, keys: SenderKeys, classified: Classified) -> CategoryColumns {
    let effective = rules.apply(&keys, classified.category);
    CategoryColumns {
        category: effective.category.code(),
        category_auto: classified.category.code(),
        reason: serde_json::to_string(&classified.reason).unwrap_or_else(|_| "{}".into()),
        important: effective.important.is_some(),
        keys,
        one_click: classified.one_click,
    }
}

pub(crate) fn own_addresses(conn: &Connection) -> Result<Vec<String>, StoreError> {
    Ok(conn
        .prepare_cached(OWN_ADDRESSES)?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------------------------------------
// Background jobs

/// Messages not classified yet, newest first (`message_category_todo`).
///
/// `INDEXED BY` here and in [`ANY_UNCLASSIFIED`]: `PRAGMA optimize` records the size of the
/// partial index while every message is still unclassified (a database from before stage 9, a
/// first sync). After the backfill the index is empty, but the statistics still count every
/// message, and the planner then scans the whole `message` table to find no row: at 100,000
/// messages about 100 MB read at every start (`docs/testing/performance.md`, stage 8/9).
pub(crate) const UNCLASSIFIED: &str = "
    SELECT id, account_id, subject, from_name, from_addr, to_addrs, cc_addrs, message_id_hdr,
           in_reply_to, references_hdr, list_id, list_unsubscribe, auto_submitted, precedence,
           date, size, has_attachments
    FROM message INDEXED BY message_category_todo
    WHERE category IS NULL ORDER BY id DESC LIMIT ?1";

/// Whether any message is still unclassified (`message_category_todo`, see [`UNCLASSIFIED`]).
pub(crate) const ANY_UNCLASSIFIED: &str = "
    SELECT EXISTS (SELECT 1 FROM message INDEXED BY message_category_todo WHERE category IS NULL)";

pub(crate) const SET_CATEGORY: &str = "
    UPDATE message SET category = ?2, category_auto = ?3, category_reason = ?4, important = ?5,
                       sender_key = ?6, sender_domain = ?7, list_key = ?8
    WHERE id = ?1";

/// Classifies up to `limit` messages stored without a category; returns how many.
pub(crate) fn backfill_step(
    txn: &Transaction<'_>,
    classifier: &Arc<dyn Classifier>,
    limit: u32,
) -> Result<usize, StoreError> {
    let window = WindowClassifier::new(txn, Arc::clone(classifier))?;
    let rows: Vec<(i64, i64, MessageHeader)> = txn
        .prepare_cached(UNCLASSIFIED)?
        .query_map([i64::from(limit)], |r| {
            let from_name: Option<String> = r.get(3)?;
            let from_addr: Option<String> = r.get(4)?;
            let references: Option<String> = r.get(9)?;
            let date: i64 = r.get(14)?;
            let header = MessageHeader {
                uid: 1,
                modseq: None,
                flags: mail_model::Flags::default(),
                size: u32::try_from(r.get::<_, i64>(15)?.max(0)).unwrap_or(u32::MAX),
                internal_date: date,
                date: Some(date),
                subject: r.get(2)?,
                from: from_addr
                    .map(|email| Address {
                        name: from_name,
                        email,
                    })
                    .into_iter()
                    .collect(),
                to: crate::messages::addresses_at(r, 5)?,
                cc: crate::messages::addresses_at(r, 6)?,
                message_id: r.get(7)?,
                in_reply_to: r.get(8)?,
                references: references
                    .map(|refs| refs.split_whitespace().map(str::to_owned).collect())
                    .unwrap_or_default(),
                list_id: r.get(10)?,
                list_unsubscribe: r.get(11)?,
                auto_submitted: r.get(12)?,
                precedence: r.get(13)?,
                has_attachments: r.get(16)?,
                category_headers: Vec::new(),
                gmail: None,
            };
            Ok((r.get(0)?, r.get(1)?, header))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut update = txn.prepare_cached(SET_CATEGORY)?;
    for (id, account_row, header) in &rows {
        let columns = window.columns(txn, *account_row, header)?;
        update.execute(params![
            id,
            columns.category,
            columns.category_auto,
            columns.reason,
            columns.important,
            columns.keys.address,
            columns.keys.domain,
            columns.keys.list_id,
        ])?;
    }
    Ok(rows.len())
}

pub(crate) const RULES_BELOW: &str = "SELECT rules_below FROM category_job WHERE id = 1";
pub(crate) const SET_RULES_BELOW: &str = "UPDATE category_job SET rules_below = ?1 WHERE id = 1";
pub(crate) const RULE_ROWS: &str = "
    SELECT id, sender_key, sender_domain, list_key, category_auto, category, important
    FROM message WHERE id < ?1 ORDER BY id DESC LIMIT ?2";
pub(crate) const SET_EFFECTIVE: &str =
    "UPDATE message SET category = ?2, important = ?3 WHERE id = ?1";

/// Asks the job to apply the sender rules to every message again.
pub(crate) fn schedule_rules(conn: &Connection) -> Result<(), StoreError> {
    conn.prepare_cached(SET_RULES_BELOW)?
        .execute([i64::MAX])
        .map_err(StoreError::sqlite("schedule category rules"))?;
    Ok(())
}

/// One step of applying the sender rules again; returns how many messages changed and
/// whether the pass is done. A message without an automatic category (not classified yet,
/// or set by a local rule only) keeps its category unless a correction applies.
pub(crate) fn rules_step(txn: &Transaction<'_>, limit: u32) -> Result<(usize, bool), StoreError> {
    let Some(below) = txn
        .prepare_cached(RULES_BELOW)?
        .query_row([], |r| r.get::<_, Option<i64>>(0))?
    else {
        return Ok((0, true));
    };
    let rules = RuleSet::load(txn)?;
    struct Row {
        id: i64,
        keys: SenderKeys,
        auto: Option<i64>,
        category: Option<i64>,
        important: bool,
    }
    let rows: Vec<Row> = txn
        .prepare_cached(RULE_ROWS)?
        .query_map(params![below, i64::from(limit)], |r| {
            Ok(Row {
                id: r.get(0)?,
                keys: SenderKeys {
                    address: r.get(1)?,
                    domain: r.get(2)?,
                    list_id: r.get(3)?,
                },
                auto: r.get(4)?,
                category: r.get(5)?,
                important: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = 0;
    let mut update = txn.prepare_cached(SET_EFFECTIVE)?;
    for row in &rows {
        // Not classified yet: the backfill job sets everything.
        let Some(base) = row.auto.or(row.category) else {
            continue;
        };
        let automatic = MessageCategory::from_code(base).unwrap_or(MessageCategory::Person);
        let effective = rules.apply(&row.keys, automatic);
        let category = effective.category.code();
        let important = effective.important.is_some();
        if Some(category) != row.category || important != row.important {
            update.execute(params![row.id, category, important])?;
            changed += 1;
        }
    }
    let done = rows.len() < usize::try_from(limit).unwrap_or(usize::MAX);
    let next = if done {
        None
    } else {
        rows.last().map(|r| r.id)
    };
    txn.prepare_cached(SET_RULES_BELOW)?.execute([next])?;
    Ok((changed, done))
}

/// Whether a job has work: unclassified messages or a pending rules pass.
pub(crate) fn work_pending(conn: &Connection) -> Result<bool, StoreError> {
    let unclassified: bool = conn
        .prepare_cached(ANY_UNCLASSIFIED)?
        .query_row([], |r| r.get(0))?;
    let rules: Option<i64> = conn
        .prepare_cached(RULES_BELOW)?
        .query_row([], |r| r.get(0))?;
    Ok(unclassified || rules.is_some())
}

// ---------------------------------------------------------------------------------------------
// Sender rules

pub(crate) const SENDER_RULES: &str = "
    SELECT uuid, key_kind, key_value, category, important, updated_at FROM sender_rule
    ORDER BY key_kind, key_value";
pub(crate) const FIND_SENDER_RULE: &str =
    "SELECT id, category, important FROM sender_rule WHERE key_kind = ?1 AND key_value = ?2";

pub(crate) fn sender_rules(conn: &Connection) -> Result<Vec<mail_model::SenderRule>, StoreError> {
    Ok(conn
        .prepare_cached(SENDER_RULES)?
        .query_map([], |r| {
            let kind: String = r.get(1)?;
            Ok(mail_model::SenderRule {
                id: r.get(0)?,
                key: SenderKey {
                    kind: SenderKeyKind::parse(&kind).unwrap_or(SenderKeyKind::Address),
                    value: r.get(2)?,
                },
                category: r
                    .get::<_, Option<i64>>(3)?
                    .and_then(MessageCategory::from_code),
                important: r.get(4)?,
                updated_at: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// What a change to a sender rule sets. `None` leaves that part as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenderRuleChange {
    pub category: Option<Option<MessageCategory>>,
    pub important: Option<bool>,
}

/// Creates, changes or (when nothing is left) deletes the rule of a normalised key and
/// schedules applying the rules again. Returns the rule, `None` when it was deleted.
pub(crate) fn put_sender_rule(
    conn: &Connection,
    key: &SenderKey,
    change: SenderRuleChange,
    new_uuid: &str,
    now: i64,
) -> Result<Option<mail_model::SenderRule>, StoreError> {
    if key.value.is_empty() {
        return Err(StoreError::InvalidArgument("an empty sender key".into()));
    }
    let existing: Option<(i64, Option<i64>, bool)> = conn
        .prepare_cached(FIND_SENDER_RULE)?
        .query_row(params![key.kind.as_str(), key.value], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let (old_category, old_important) = existing
        .map(|(_, c, i)| (c.and_then(MessageCategory::from_code), i))
        .unwrap_or((None, false));
    let category = change.category.unwrap_or(old_category);
    let important = change.important.unwrap_or(old_important);
    schedule_rules(conn)?;
    match (existing, category.is_some() || important) {
        (Some((id, ..)), false) => {
            conn.execute("DELETE FROM sender_rule WHERE id = ?1", [id])?;
            return Ok(None);
        }
        (None, false) => return Ok(None),
        (Some((id, ..)), true) => {
            conn.execute(
                "UPDATE sender_rule SET category = ?2, important = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, category.map(MessageCategory::code), important, now],
            )?;
        }
        (None, true) => {
            conn.execute(
                "INSERT INTO sender_rule (uuid, key_kind, key_value, category, important,
                                          created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    new_uuid,
                    key.kind.as_str(),
                    key.value,
                    category.map(MessageCategory::code),
                    important,
                    now
                ],
            )?;
        }
    }
    Ok(sender_rules(conn)?
        .into_iter()
        .find(|r| r.key.kind == key.kind && r.key.value == key.value))
}

/// Deletes a rule by id; whether it existed. Schedules applying the rules again.
pub(crate) fn delete_sender_rule(conn: &Connection, uuid: &str) -> Result<bool, StoreError> {
    let deleted = conn.execute("DELETE FROM sender_rule WHERE uuid = ?1", [uuid])? > 0;
    if deleted {
        schedule_rules(conn)?;
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------------------------
// One message

/// The stored category data of a message: automatic reason, keys, effective category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCategory {
    pub category: Option<MessageCategory>,
    pub automatic: Option<MessageCategory>,
    pub reason: Option<CategoryReason>,
    pub important: bool,
    pub keys: SenderKeys,
    pub unsubscribe_one_click: Option<bool>,
    /// The raw `List-Unsubscribe` value stored with the header data.
    pub list_unsubscribe: Option<String>,
}

pub(crate) const MESSAGE_CATEGORY: &str = "
    SELECT category, category_auto, category_reason, important, sender_key, sender_domain,
           list_key, unsubscribe_one_click, list_unsubscribe
    FROM message WHERE id = ?1";

pub(crate) fn message_category(
    conn: &Connection,
    message: i64,
) -> Result<Option<StoredCategory>, StoreError> {
    Ok(conn
        .prepare_cached(MESSAGE_CATEGORY)?
        .query_row([message], |r| {
            let reason: Option<String> = r.get(2)?;
            Ok(StoredCategory {
                category: r
                    .get::<_, Option<i64>>(0)?
                    .and_then(MessageCategory::from_code),
                automatic: r
                    .get::<_, Option<i64>>(1)?
                    .and_then(MessageCategory::from_code),
                reason: reason.and_then(|json| serde_json::from_str(&json).ok()),
                important: r.get(3)?,
                keys: SenderKeys {
                    address: r.get(4)?,
                    domain: r.get(5)?,
                    list_id: r.get(6)?,
                },
                unsubscribe_one_click: r.get(7)?,
                list_unsubscribe: r.get(8)?,
            })
        })
        .optional()?)
}

/// A local rule set the category of one message: it becomes the automatic result (with the
/// rule as the reason), the user's sender corrections still go first.
pub(crate) fn set_local_category(
    conn: &Connection,
    message: i64,
    category: MessageCategory,
    rule_name: &str,
) -> Result<bool, StoreError> {
    let rules = RuleSet::load(conn)?;
    let Some(stored) = message_category(conn, message)? else {
        return Ok(false);
    };
    let effective = rules.apply(&stored.keys, category);
    let reason = serde_json::to_string(&CategoryReason::LocalRule {
        rule_name: rule_name.to_owned(),
    })
    .unwrap_or_else(|_| "{}".into());
    conn.execute(
        "UPDATE message SET category_auto = ?2, category_reason = ?3, category = ?4, important = ?5
         WHERE id = ?1",
        params![
            message,
            category.code(),
            reason,
            effective.category.code(),
            effective.important.is_some()
        ],
    )?;
    Ok(true)
}
