//! The contact directory of stage 10 (decision 0023): ranked pages with merged contacts once,
//! names the user gives, merges and dismissed duplicate suggestions. Rows stay one per address
//! (`contacts`); merged addresses share `contact_group`, the id of the kept contact's row (its
//! "head"). The rules (names, ranks, duplicate suggestions) are `mail_content`'s and run in the
//! engine; here are the rows.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::compose_api::StoredContact;
use crate::contacts::{CONTACT_COLUMNS, MAX_SEARCH_WORDS, contact_at};
use crate::settings::now_millis;
use crate::{Store, StoreError};

/// Most rows one page reads (merged rows that are not their group's best are skipped).
pub const MAX_CONTACT_ROWS: u32 = 500;

/// A contact as stored: its rows, the head (preferred address) first.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredContactGroup {
    pub members: Vec<StoredContact>,
}

impl StoredContactGroup {
    pub fn head(&self) -> &StoredContact {
        &self.members[0]
    }
}

/// Where a page of the directory continues: rank and row id of the last row read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactCursor {
    pub rank: f64,
    pub row: i64,
}

impl ContactCursor {
    /// Opaque text for the UI.
    pub fn encode(self) -> String {
        format!("{}/{}", self.rank, self.row)
    }

    pub fn decode(text: &str) -> Option<Self> {
        let (rank, row) = text.split_once('/')?;
        let rank: f64 = rank.parse().ok()?;
        (!rank.is_nan()).then_some(())?;
        Some(Self {
            rank,
            row: row.parse().ok()?,
        })
    }
}

/// A page of contacts.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredContactPage {
    pub groups: Vec<StoredContactGroup>,
    pub next: Option<ContactCursor>,
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The contacts are one now; holds the head's address key.
    Merged(String),
    /// No contact has this address key; nothing changed.
    NotFound(String),
    /// Every address belongs to the kept contact already; nothing changed.
    SameContact,
}

/// What separating an address from its contact did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeparateOutcome {
    Separated,
    NotFound,
    /// The address is not merged with another one.
    NotMerged,
}

/// Stored for "no messages" in `contact.rank` (see `contacts`).
const NO_RANK: f64 = -1.0e308;

fn stored_rank(rank: f64) -> f64 {
    if rank.is_finite() { rank } else { NO_RANK }
}

/// Word conditions on the `search` column of alias `alias`, parameters from `?first` on.
fn word_conditions(alias: &str, words: usize, first: usize) -> String {
    (0..words)
        .map(|i| format!(" AND instr({alias}.search, ' ' || ?{}) > 0", first + i))
        .collect()
}

/// The page query: rows by rank (index `contact_rank`, keyset on rank and row id), only the
/// best matching row of each group. Stage 17: `people_only` leaves out list and system
/// addresses (`kind` 0 only).
pub(crate) fn page_sql(words: usize, people_only: bool) -> String {
    let mut c_words = word_conditions("c", words, 4);
    let mut o_words = word_conditions("o", words, 4);
    if people_only {
        c_words.push_str(" AND c.kind = 0");
        o_words.push_str(" AND o.kind = 0");
    }
    format!(
        "SELECT c.id, {cols} FROM contact c INDEXED BY contact_rank
         WHERE c.rank <= ?1 AND (c.rank < ?1 OR c.id > ?2){c_words}
           AND (c.contact_group IS NULL OR NOT EXISTS (
                 SELECT 1 FROM contact o INDEXED BY contact_group
                 WHERE o.contact_group = c.contact_group
                   AND (o.rank > c.rank OR (o.rank = c.rank AND o.id < c.id)){o_words}))
         ORDER BY c.rank DESC, c.id LIMIT ?3",
        cols = prefixed("c")
    )
}

fn prefixed(alias: &str) -> String {
    CONTACT_COLUMNS
        .split(',')
        .map(|c| format!("{alias}.{}", c.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) const GROUP_MEMBERS: &str = "
    SELECT id, address_key, local_part, domain, contact_group, sent, received, last_sent,
           last_received, rank, names, user_name, search, seen, last_seen, personal, kind, note
    FROM contact INDEXED BY contact_group WHERE contact_group = ?1
    ORDER BY (id = contact_group) DESC, rank DESC, id";

const ROW_BY_KEY: &str = "SELECT id, contact_group FROM contact WHERE address_key = ?1";

/// The members of group `group`, head first.
fn members(conn: &Connection, group: i64) -> Result<Vec<StoredContact>, StoreError> {
    Ok(conn
        .prepare_cached(GROUP_MEMBERS)?
        .query_map([group], |r| contact_at(r, 1))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn group_of_row(
    conn: &Connection,
    contact: StoredContact,
) -> Result<StoredContactGroup, StoreError> {
    match contact.group {
        None => Ok(StoredContactGroup {
            members: vec![contact],
        }),
        Some(group) => {
            let mut members = members(conn, group)?;
            if members.is_empty() {
                members.push(contact);
            }
            Ok(StoredContactGroup { members })
        }
    }
}

pub(crate) fn page(
    conn: &Connection,
    words: &[String],
    after: Option<ContactCursor>,
    limit: u32,
    people_only: bool,
) -> Result<StoredContactPage, StoreError> {
    let words: Vec<&String> = words
        .iter()
        .filter(|w| !w.is_empty())
        .take(MAX_SEARCH_WORDS)
        .collect();
    let (rank, row) = after.map_or((f64::MAX, i64::MIN), |c| (stored_rank(c.rank), c.row));
    let limit = limit.clamp(1, MAX_CONTACT_ROWS);
    let sql = page_sql(words.len(), people_only);
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut values: Vec<&dyn rusqlite::ToSql> = vec![&rank, &row, &limit];
    for word in &words {
        values.push(*word);
    }
    let rows = stmt
        .query_map(values.as_slice(), |r| {
            Ok((r.get::<_, i64>(0)?, contact_at(r, 1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let next = (rows.len() == limit as usize)
        .then(|| rows.last())
        .flatten()
        .map(|(row, contact)| ContactCursor {
            rank: contact.rank,
            row: *row,
        });
    let mut groups = Vec::with_capacity(rows.len());
    for (_, contact) in rows {
        groups.push(group_of_row(conn, contact)?);
    }
    Ok(StoredContactPage { groups, next })
}

/// The contact (all its rows) that has the address `key`.
pub(crate) fn group_by_key(
    conn: &Connection,
    key: &str,
) -> Result<Option<StoredContactGroup>, StoreError> {
    let found = conn
        .prepare_cached(&format!(
            "SELECT id, {CONTACT_COLUMNS} FROM contact WHERE address_key = ?1"
        ))?
        .query_row([key], |r| contact_at(r, 1))
        .optional()?;
    match found {
        None => Ok(None),
        Some(contact) => group_of_row(conn, contact).map(Some),
    }
}

/// The highest ranked `limit` rows as contacts (merged rows together, head first).
pub(crate) fn all_groups(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<StoredContactGroup>, StoreError> {
    let rows = conn
        .prepare_cached(&format!(
            "SELECT id, {CONTACT_COLUMNS} FROM contact INDEXED BY contact_rank
             ORDER BY rank DESC, id LIMIT ?1"
        ))?
        .query_map([limit], |r| Ok((r.get::<_, i64>(0)?, contact_at(r, 1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut groups: Vec<StoredContactGroup> = Vec::new();
    let mut by_group: HashMap<i64, usize> = HashMap::new();
    for (row, contact) in rows {
        match contact.group {
            None => groups.push(StoredContactGroup {
                members: vec![contact],
            }),
            Some(group) => match by_group.get(&group) {
                Some(&index) => {
                    let members = &mut groups[index].members;
                    if row == group {
                        members.insert(0, contact);
                    } else {
                        members.push(contact);
                    }
                }
                None => {
                    by_group.insert(group, groups.len());
                    groups.push(StoredContactGroup {
                        members: vec![contact],
                    });
                }
            },
        }
    }
    Ok(groups)
}

/// Sets the name the user gave the contact with the address `key` (on its head row) and the
/// head's new search words. `false` if there is no such contact.
pub(crate) fn set_name(
    conn: &Connection,
    key: &str,
    name: Option<&str>,
    search: &str,
    now: i64,
) -> Result<bool, StoreError> {
    let Some((row, group)) = conn
        .prepare_cached(ROW_BY_KEY)?
        .query_row([key], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .optional()?
    else {
        return Ok(false);
    };
    let head = group.unwrap_or(row);
    let n = conn
        .prepare_cached(
            "UPDATE contact SET user_name = ?2, search = ?3, updated_at = ?4 WHERE id = ?1",
        )?
        .execute(params![head, name, search, now])
        .map_err(StoreError::sqlite("set contact name"))?;
    Ok(n > 0)
}

/// Merges the contacts with the addresses `absorb` into the one with `keep` (see
/// [`MergeOutcome`]). The kept contact keeps its preferred address and its name; without a
/// name of its own it takes the first merged contact's.
pub(crate) fn merge(
    conn: &Connection,
    keep: &str,
    absorb: &[String],
    now: i64,
) -> Result<MergeOutcome, StoreError> {
    let row_of = |key: &str| -> Result<Option<(i64, Option<i64>)>, StoreError> {
        Ok(conn
            .prepare_cached(ROW_BY_KEY)?
            .query_row([key], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
            })
            .optional()?)
    };
    let Some((keep_row, keep_group)) = row_of(keep)? else {
        return Ok(MergeOutcome::NotFound(keep.to_owned()));
    };
    let target = keep_group.unwrap_or(keep_row);
    let mut groups = Vec::new();
    for key in absorb {
        let Some((row, group)) = row_of(key)? else {
            return Ok(MergeOutcome::NotFound(key.clone()));
        };
        let group = group.unwrap_or(row);
        if group != target && !groups.contains(&group) {
            groups.push(group);
        }
    }
    if groups.is_empty() {
        return Ok(MergeOutcome::SameContact);
    }
    let name_of = |row: i64| -> Result<Option<String>, StoreError> {
        Ok(conn
            .prepare_cached("SELECT user_name FROM contact WHERE id = ?1")?
            .query_row([row], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten())
    };
    let kept_name = name_of(target)?;
    let mut taken_name = None;
    for group in &groups {
        if taken_name.is_none() {
            taken_name = name_of(*group)?;
        }
        conn.prepare_cached(
            "UPDATE contact SET contact_group = ?1, updated_at = ?3
             WHERE id = ?2 OR contact_group = ?2",
        )?
        .execute(params![target, group, now])
        .map_err(StoreError::sqlite("merge contacts"))?;
    }
    conn.prepare_cached(
        "UPDATE contact SET contact_group = ?1, updated_at = ?2
         WHERE id = ?1 AND contact_group IS NULL",
    )?
    .execute(params![target, now])
    .map_err(StoreError::sqlite("merge contacts"))?;
    if kept_name.is_none()
        && let Some(name) = taken_name
    {
        conn.prepare_cached("UPDATE contact SET user_name = ?2 WHERE id = ?1")?
            .execute(params![target, name])
            .map_err(StoreError::sqlite("merge contacts"))?;
    }
    let head: String = conn
        .prepare_cached("SELECT address_key FROM contact WHERE id = ?1")?
        .query_row([target], |r| r.get(0))?;
    Ok(MergeOutcome::Merged(head))
}

/// Takes the address `key` out of its contact. The head takes the whole group apart; a group
/// left with one address is no group any more.
pub(crate) fn separate(
    conn: &Connection,
    key: &str,
    now: i64,
) -> Result<SeparateOutcome, StoreError> {
    let Some((row, group)) = conn
        .prepare_cached(ROW_BY_KEY)?
        .query_row([key], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .optional()?
    else {
        return Ok(SeparateOutcome::NotFound);
    };
    let Some(group) = group else {
        return Ok(SeparateOutcome::NotMerged);
    };
    let others: i64 = conn
        .prepare_cached("SELECT count(*) FROM contact WHERE contact_group = ?1 AND id != ?2")?
        .query_row(params![group, row], |r| r.get(0))?;
    if others == 0 {
        // Alone in its group: nothing to separate, but clear the leftover.
        conn.prepare_cached("UPDATE contact SET contact_group = NULL WHERE id = ?1")?
            .execute([row])?;
        return Ok(SeparateOutcome::NotMerged);
    }
    if row == group {
        conn.prepare_cached(
            "UPDATE contact SET contact_group = NULL, updated_at = ?2 WHERE contact_group = ?1",
        )?
        .execute(params![group, now])
        .map_err(StoreError::sqlite("separate contact"))?;
        return Ok(SeparateOutcome::Separated);
    }
    conn.prepare_cached("UPDATE contact SET contact_group = NULL, updated_at = ?2 WHERE id = ?1")?
        .execute(params![row, now])
        .map_err(StoreError::sqlite("separate contact"))?;
    if others == 1 {
        conn.prepare_cached(
            "UPDATE contact SET contact_group = NULL, updated_at = ?2 WHERE contact_group = ?1",
        )?
        .execute(params![group, now])
        .map_err(StoreError::sqlite("separate contact"))?;
    }
    Ok(SeparateOutcome::Separated)
}

fn ordered<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a <= b { (a, b) } else { (b, a) }
}

pub(crate) fn dismiss(conn: &Connection, a: &str, b: &str, now: i64) -> Result<(), StoreError> {
    let (a, b) = ordered(a, b);
    conn.prepare_cached(
        "INSERT INTO contact_dismissed (a_key, b_key, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (a_key, b_key) DO NOTHING",
    )?
    .execute(params![a, b, now])
    .map_err(StoreError::sqlite("dismiss duplicate"))?;
    Ok(())
}

pub(crate) fn dismissed(conn: &Connection) -> Result<Vec<(String, String)>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT a_key, b_key FROM contact_dismissed")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?)
}

// -- settings sync ----------------------------------------------------------------------------

/// The synchronised content of the contact with address `key`: the name the user gave, the
/// head it is merged into and (stage 17) the note (`{userName?, mergedInto?, note?}`), `None`
/// without any.
pub(crate) fn synced_content(conn: &Connection, key: &str) -> Result<Option<Value>, StoreError> {
    let found = conn
        .prepare_cached(
            "SELECT c.user_name,
                    (SELECT h.address_key FROM contact h
                     WHERE h.id = c.contact_group AND h.id != c.id),
                    c.note
             FROM contact c WHERE c.address_key = ?1",
        )?
        .query_row([key], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .optional()?;
    Ok(match found {
        None | Some((None, None, None)) => None,
        Some((name, merged, note)) => {
            let mut object = serde_json::Map::new();
            if let Some(name) = name {
                object.insert("userName".into(), Value::String(name));
            }
            if let Some(merged) = merged {
                object.insert("mergedInto".into(), Value::String(merged));
            }
            if let Some(note) = note {
                object.insert("note".into(), Value::String(note));
            }
            Some(Value::Object(object))
        }
    })
}

/// A synchronised contact record: what the user set on one address.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SyncedContact<'a> {
    pub user_name: Option<&'a str>,
    pub merged_into: Option<&'a str>,
    pub note: Option<&'a str>,
}

impl<'a> SyncedContact<'a> {
    pub(crate) fn of(value: &'a Value) -> Self {
        let text = |field: &str| value.get(field).and_then(Value::as_str);
        Self {
            user_name: text("userName"),
            merged_into: text("mergedInto"),
            note: text("note"),
        }
    }
}

/// Applies a synchronised contact record to the row with address `key` (if this device has
/// it): the name, the note, and the merge into the head with address `merged_into` (if this
/// device has that one too), or no merge.
pub(crate) fn apply_synced(
    conn: &Connection,
    key: &str,
    record: SyncedContact<'_>,
) -> Result<usize, StoreError> {
    let SyncedContact {
        user_name,
        merged_into,
        note,
    } = record;
    let Some((row, group)) = conn
        .prepare_cached(ROW_BY_KEY)?
        .query_row([key], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .optional()?
    else {
        return Ok(0);
    };
    let mut changed = conn
        .prepare_cached("UPDATE contact SET user_name = ?2, note = ?3 WHERE id = ?1")?
        .execute(params![row, user_name, note])?;
    match merged_into {
        Some(head_key) if head_key != key => {
            let head = conn
                .prepare_cached(ROW_BY_KEY)?
                .query_row([head_key], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
                })
                .optional()?;
            if let Some((head_row, head_group)) = head {
                let target = head_group.unwrap_or(head_row);
                if group != Some(target) {
                    changed += conn
                        .prepare_cached("UPDATE contact SET contact_group = ?2 WHERE id = ?1")?
                        .execute(params![row, target])?;
                    conn.prepare_cached(
                        "UPDATE contact SET contact_group = ?1 WHERE id = ?1 AND contact_group IS NULL",
                    )?
                    .execute([target])?;
                }
            }
        }
        // Not merged on the other device: a member leaves its group (a head stays).
        _ => {
            if let Some(group) = group
                && group != row
            {
                changed += conn
                    .prepare_cached("UPDATE contact SET contact_group = NULL WHERE id = ?1")?
                    .execute([row])?;
            }
        }
    }
    Ok(changed)
}

/// A record synchronised before this device saw the address applies when the row appears.
pub(crate) fn apply_waiting_record(conn: &Connection, key: &str) -> Result<(), StoreError> {
    let content: Option<String> = conn
        .prepare_cached(
            "SELECT content FROM sync_record
             WHERE kind = 'contact' AND id = ?1 AND content IS NOT NULL",
        )?
        .query_row([key], |r| r.get(0))
        .optional()?;
    let Some(content) = content else {
        return Ok(());
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };
    apply_synced(conn, key, SyncedContact::of(&value))?;
    Ok(())
}

// -- the store's API ---------------------------------------------------------------------------

impl Store {
    /// One page of the directory, highest ranked first; each contact once (merged rows by their
    /// best ranked, or best matching, address). `words` folded by the caller (prefix match, at
    /// most 8); `limit` rows are read (1..=[`MAX_CONTACT_ROWS`]).
    pub async fn contact_page(
        &self,
        words: Vec<String>,
        after: Option<ContactCursor>,
        limit: u32,
        people_only: bool,
    ) -> Result<StoredContactPage, StoreError> {
        self.read(move |conn| page(conn, &words, after, limit, people_only))
            .await
    }

    /// The contact that has the address key `key`, all its addresses.
    pub async fn contact_group(
        &self,
        key: String,
    ) -> Result<Option<StoredContactGroup>, StoreError> {
        self.read(move |conn| group_by_key(conn, &key)).await
    }

    /// The highest ranked `limit` addresses as contacts (for the duplicate check).
    pub async fn contact_groups(&self, limit: u32) -> Result<Vec<StoredContactGroup>, StoreError> {
        self.read(move |conn| all_groups(conn, limit)).await
    }

    /// Sets or clears the name the user gave a contact, with its head row's new search words.
    pub async fn set_contact_name(
        &self,
        key: String,
        name: Option<String>,
        search: String,
    ) -> Result<bool, StoreError> {
        let now = now_millis();
        self.write(move |tx| set_name(tx, &key, name.as_deref(), &search, now))
            .await
    }

    /// Merges contacts the user confirmed as one (the engine checks the confirmation).
    pub async fn merge_contacts(
        &self,
        keep: String,
        absorb: Vec<String>,
    ) -> Result<MergeOutcome, StoreError> {
        let now = now_millis();
        self.write(move |tx| merge(tx, &keep, &absorb, now)).await
    }

    /// Takes an address out of its merged contact.
    pub async fn separate_contact(&self, key: String) -> Result<SeparateOutcome, StoreError> {
        let now = now_millis();
        self.write(move |tx| separate(tx, &key, now)).await
    }

    /// Hides a duplicate suggestion for good (`a == b`: "different names" of one contact).
    pub async fn dismiss_contact_pair(&self, a: String, b: String) -> Result<(), StoreError> {
        let now = now_millis();
        self.write(move |tx| dismiss(tx, &a, &b, now)).await
    }

    /// Duplicate suggestions the user dismissed, as pairs of contact keys.
    pub async fn dismissed_contact_pairs(&self) -> Result<Vec<(String, String)>, StoreError> {
        self.read(dismissed).await
    }
}
