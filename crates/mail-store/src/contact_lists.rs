//! Stage 19, block C (decision 0056): contact groups. One row per group (`contact_list`), one
//! per member address (`contact_list_member`, by normalised address key, in the order added).
//! Synchronised as record kind `contactList` (UUID, name, members with address and name).
//!
//! The engine checks names and addresses; the store keeps the limits that need the stored
//! state (most groups, most members per group).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use mail_model::{MAX_CONTACT_LIST_MEMBERS, MAX_CONTACT_LISTS};

use crate::groups::NEW_UUID;
use crate::settings::now_millis;
use crate::{Store, StoreError};

/// A group as stored, with its members in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContactList {
    pub uuid: String,
    pub name: String,
    pub members: Vec<StoredListMember>,
}

/// A member, with what the directory knows about its address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredListMember {
    pub address_key: String,
    pub email: String,
    /// The name given with the member.
    pub name: Option<String>,
    /// The key of the contact (the head of a merged contact) with this address, if any.
    pub contact_key: Option<String>,
    /// The contact's name: the user's, else the name seen most often.
    pub contact_name: Option<String>,
}

/// A member to add: normalised key, the address as written, the name given with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewListMember {
    pub address_key: String,
    pub email: String,
    pub name: Option<String>,
}

/// A group in a listing: UUID, name, number of members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSummary {
    pub uuid: String,
    pub name: String,
    pub members: u32,
}

const SUMMARIES: &str = "
    SELECT l.uuid, l.name, (SELECT count(*) FROM contact_list_member m WHERE m.list_id = l.id)
    FROM contact_list l ORDER BY l.name COLLATE NOCASE, l.id";

const LIST_ROW: &str = "SELECT id, name FROM contact_list WHERE uuid = ?1";

const MEMBERS: &str = "
    SELECT m.address_key, m.email, m.name, c.address_key, c.user_name, c.names,
           h.address_key, h.user_name, h.names
    FROM contact_list_member m
    LEFT JOIN contact c ON c.address_key = m.address_key
    LEFT JOIN contact h ON h.id = c.contact_group
    WHERE m.list_id = ?1 ORDER BY m.position";

/// The name seen most often in a contact's `names` JSON.
fn most_seen(names: Option<String>) -> Option<String> {
    let names: Vec<Value> = serde_json::from_str(names.as_deref()?).ok()?;
    names
        .iter()
        .filter_map(|n| Some((n.get("name")?.as_str()?, n.get("count")?.as_u64()?)))
        .max_by_key(|(_, count)| *count)
        .map(|(name, _)| name.to_owned())
}

pub(crate) fn summaries(conn: &Connection) -> Result<Vec<ListSummary>, StoreError> {
    Ok(conn
        .prepare_cached(SUMMARIES)?
        .query_map([], |r| {
            Ok(ListSummary {
                uuid: r.get(0)?,
                name: r.get(1)?,
                members: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn row_of(conn: &Connection, uuid: &str) -> Result<Option<(i64, String)>, StoreError> {
    Ok(conn
        .prepare_cached(LIST_ROW)?
        .query_row([uuid], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?)
}

pub(crate) fn list(conn: &Connection, uuid: &str) -> Result<Option<StoredContactList>, StoreError> {
    let Some((row, name)) = row_of(conn, uuid)? else {
        return Ok(None);
    };
    let members = conn
        .prepare_cached(MEMBERS)?
        .query_map([row], |r| {
            let own_key: Option<String> = r.get(3)?;
            let head_key: Option<String> = r.get(6)?;
            let contact_name = r
                .get::<_, Option<String>>(7)?
                .or(r.get::<_, Option<String>>(4)?)
                .or_else(|| most_seen(r.get(8).ok().flatten()))
                .or_else(|| most_seen(r.get(5).ok().flatten()));
            Ok(StoredListMember {
                address_key: r.get(0)?,
                email: r.get(1)?,
                name: r.get(2)?,
                contact_key: head_key.or(own_key),
                contact_name,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(StoredContactList {
        uuid: uuid.to_owned(),
        name,
        members,
    }))
}

fn count_lists(conn: &Connection) -> Result<u32, StoreError> {
    Ok(conn
        .prepare_cached("SELECT count(*) FROM contact_list")?
        .query_row([], |r| r.get(0))?)
}

/// Inserts the members not in the group yet, after the last one. `InvalidArgument` when the
/// group would get more than [`MAX_CONTACT_LIST_MEMBERS`]; nothing is added then.
fn insert_members(
    conn: &Connection,
    row: i64,
    members: &[NewListMember],
) -> Result<usize, StoreError> {
    let (count, last): (u32, i64) = conn
        .prepare_cached(
            "SELECT count(*), coalesce(max(position), -1) FROM contact_list_member WHERE list_id = ?1",
        )?
        .query_row([row], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut exists = conn.prepare_cached(
        "SELECT 1 FROM contact_list_member WHERE list_id = ?1 AND address_key = ?2",
    )?;
    let mut fresh: Vec<&NewListMember> = Vec::new();
    for member in members {
        let known = exists
            .query_row(params![row, member.address_key], |_| Ok(()))
            .optional()?
            .is_some();
        if !known && !fresh.iter().any(|m| m.address_key == member.address_key) {
            fresh.push(member);
        }
    }
    let total = u64::from(count) + fresh.len() as u64;
    if total > u64::from(MAX_CONTACT_LIST_MEMBERS) {
        return Err(StoreError::InvalidArgument(format!(
            "a group has at most {MAX_CONTACT_LIST_MEMBERS} members"
        )));
    }
    let mut insert = conn.prepare_cached(
        "INSERT INTO contact_list_member (list_id, address_key, email, name, position)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (n, member) in fresh.iter().enumerate() {
        let position = last
            .saturating_add(1)
            .saturating_add(i64::try_from(n).unwrap_or(i64::MAX));
        insert.execute(params![
            row,
            member.address_key,
            member.email,
            member.name,
            position
        ])?;
    }
    Ok(fresh.len())
}

fn touch(conn: &Connection, row: i64, now: i64) -> Result<(), StoreError> {
    conn.prepare_cached("UPDATE contact_list SET updated = ?2 WHERE id = ?1")?
        .execute(params![row, now])?;
    Ok(())
}

/// See [`Store::create_contact_list`].
pub(crate) fn create(
    conn: &Connection,
    name: &str,
    search: &str,
    members: &[NewListMember],
    now: i64,
) -> Result<String, StoreError> {
    if count_lists(conn)? >= MAX_CONTACT_LISTS {
        return Err(StoreError::InvalidArgument(format!(
            "at most {MAX_CONTACT_LISTS} groups"
        )));
    }
    let uuid: String = conn.prepare_cached(NEW_UUID)?.query_row([], |r| r.get(0))?;
    conn.prepare_cached(
        "INSERT INTO contact_list (uuid, name, search, created, updated) VALUES (?1, ?2, ?3, ?4, ?4)",
    )?
    .execute(params![uuid, name, search, now])?;
    let row = conn.last_insert_rowid();
    insert_members(conn, row, members)?;
    Ok(uuid)
}

/// See [`Store::rename_contact_list`].
pub(crate) fn rename(
    conn: &Connection,
    uuid: &str,
    name: &str,
    search: &str,
    now: i64,
) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached(
            "UPDATE contact_list SET name = ?2, search = ?3, updated = ?4 WHERE uuid = ?1",
        )?
        .execute(params![uuid, name, search, now])?
        > 0)
}

/// See [`Store::delete_contact_list`].
pub(crate) fn delete(conn: &Connection, uuid: &str) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached("DELETE FROM contact_list WHERE uuid = ?1")?
        .execute([uuid])?
        > 0)
}

/// See [`Store::add_contact_list_members`]. `None` for an unknown group.
pub(crate) fn add_members(
    conn: &Connection,
    uuid: &str,
    members: &[NewListMember],
    now: i64,
) -> Result<Option<usize>, StoreError> {
    let Some((row, _)) = row_of(conn, uuid)? else {
        return Ok(None);
    };
    let added = insert_members(conn, row, members)?;
    if added > 0 {
        touch(conn, row, now)?;
    }
    Ok(Some(added))
}

/// See [`Store::remove_contact_list_member`]. `None` for an unknown group.
pub(crate) fn remove_member(
    conn: &Connection,
    uuid: &str,
    address_key: &str,
    now: i64,
) -> Result<Option<bool>, StoreError> {
    let Some((row, _)) = row_of(conn, uuid)? else {
        return Ok(None);
    };
    let removed = conn
        .prepare_cached("DELETE FROM contact_list_member WHERE list_id = ?1 AND address_key = ?2")?
        .execute(params![row, address_key])?
        > 0;
    if removed {
        touch(conn, row, now)?;
    }
    Ok(Some(removed))
}

/// Groups whose name has a word starting with every one of `words` (folded by the caller), by
/// name; at most `limit`.
pub(crate) fn search(
    conn: &Connection,
    words: &[String],
    limit: u32,
) -> Result<Vec<ListSummary>, StoreError> {
    let mut sql = String::from(
        "SELECT l.uuid, l.name, (SELECT count(*) FROM contact_list_member m WHERE m.list_id = l.id)
         FROM contact_list l WHERE 1",
    );
    for n in 0..words.len() {
        sql.push_str(&format!(
            " AND l.search LIKE '% ' || ?{} || '%' ESCAPE '\\'",
            n + 1
        ));
    }
    sql.push_str(&format!(
        " ORDER BY l.name COLLATE NOCASE, l.id LIMIT ?{}",
        words.len() + 1
    ));
    let escaped: Vec<String> = words
        .iter()
        .map(|w| {
            w.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        })
        .collect();
    let mut values: Vec<&dyn rusqlite::ToSql> =
        escaped.iter().map(|w| w as &dyn rusqlite::ToSql).collect();
    values.push(&limit);
    Ok(conn
        .prepare(&sql)?
        .query_map(values.as_slice(), |r| {
            Ok(ListSummary {
                uuid: r.get(0)?,
                name: r.get(1)?,
                members: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// The synchronised content of a group: name, its search words, and the members (address as
/// added, given name, normalised key). The receiving engine checks the derived parts
/// (`settings_sync::record`) before they are applied.
pub(crate) fn synced_content(conn: &Connection, uuid: &str) -> Result<Option<Value>, StoreError> {
    let Some((row, name)) = row_of(conn, uuid)? else {
        return Ok(None);
    };
    let search: String = conn
        .prepare_cached("SELECT search FROM contact_list WHERE id = ?1")?
        .query_row([row], |r| r.get(0))?;
    let members: Vec<Value> = conn
        .prepare_cached(
            "SELECT email, name, address_key FROM contact_list_member WHERE list_id = ?1
             ORDER BY position",
        )?
        .query_map([row], |r| {
            Ok(json!({
                "email": r.get::<_, String>(0)?,
                "name": r.get::<_, Option<String>>(1)?,
                "key": r.get::<_, String>(2)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(
        json!({ "name": name, "search": search, "members": members }),
    ))
}

/// The members of a synchronised group record (already checked by the engine).
pub(crate) fn synced_members(value: &Value) -> Vec<NewListMember> {
    value
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(NewListMember {
                address_key: m.get("key")?.as_str()?.to_owned(),
                email: m.get("email")?.as_str()?.to_owned(),
                name: m.get("name").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect()
}

/// A group from another device: replaces name and members (keys and search words come with the
/// record, checked by the engine), or removes it (`None`).
pub(crate) fn apply_synced(
    conn: &Connection,
    uuid: &str,
    content: Option<(&str, &str, &[NewListMember])>,
    now: i64,
) -> Result<usize, StoreError> {
    let Some((name, search, members)) = content else {
        return delete(conn, uuid).map(usize::from);
    };
    conn.prepare_cached(
        "INSERT INTO contact_list (uuid, name, search, created, updated) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (uuid) DO UPDATE SET name = excluded.name, search = excluded.search,
           updated = excluded.updated",
    )?
    .execute(params![uuid, name, search, now])?;
    let (row, _) = row_of(conn, uuid)?.ok_or_else(|| StoreError::NotFound(uuid.to_owned()))?;
    conn.prepare_cached("DELETE FROM contact_list_member WHERE list_id = ?1")?
        .execute([row])?;
    // A record of a newer version may name more members than this one takes: the first ones.
    let take = usize::try_from(MAX_CONTACT_LIST_MEMBERS).unwrap_or(usize::MAX);
    let kept: Vec<NewListMember> = members.iter().take(take).cloned().collect();
    insert_members(conn, row, &kept)?;
    Ok(1)
}

/// Stage 19 (vCard import): new contact rows for addresses the directory does not know yet;
/// rows of known addresses are left as they are (their counts belong to the directory job).
const INSERT_NEW_CONTACT: &str = "
    INSERT INTO contact (address_key, local_part, domain, contact_group, sent, received,
                         last_sent, last_received, rank, names, user_name, search, updated_at,
                         seen, last_seen, personal, kind)
    VALUES (?1, ?2, ?3, NULL, 0, 0, NULL, NULL, ?4, ?5, ?6, ?7, ?8, 0, NULL, 1, ?9)
    ON CONFLICT (address_key) DO NOTHING";

/// See [`Store::insert_new_contacts`].
pub(crate) fn insert_new_contacts(
    conn: &Connection,
    contacts: &[crate::StoredContact],
    now: i64,
) -> Result<usize, StoreError> {
    let mut insert = conn.prepare_cached(INSERT_NEW_CONTACT)?;
    let mut added = 0;
    for c in contacts {
        let names = serde_json::to_string(&c.names)
            .map_err(crate::outbox::json_error("encode contact names"))?;
        let rank = if c.rank.is_finite() { c.rank } else { -1e308 };
        added += insert.execute(params![
            c.key,
            c.local_part,
            c.domain,
            rank,
            names,
            c.user_name,
            c.search,
            now,
            c.kind,
        ])?;
    }
    Ok(added)
}

impl Store {
    /// Stage 19: every group, by name.
    pub async fn contact_lists(&self) -> Result<Vec<ListSummary>, StoreError> {
        self.read(summaries).await
    }

    /// Stage 19: one group with its members; `None` if there is none with this UUID.
    pub async fn contact_list(
        &self,
        uuid: String,
    ) -> Result<Option<StoredContactList>, StoreError> {
        self.read(move |c| list(c, &uuid)).await
    }

    /// Stage 19: a new group with these members; returns its UUID. `InvalidArgument` beyond
    /// [`MAX_CONTACT_LISTS`] groups or [`MAX_CONTACT_LIST_MEMBERS`] members.
    pub async fn create_contact_list(
        &self,
        name: String,
        search: String,
        members: Vec<NewListMember>,
    ) -> Result<String, StoreError> {
        let now = now_millis();
        self.write(move |tx| create(tx, &name, &search, &members, now))
            .await
    }

    /// Stage 19: a new name; `false` for an unknown group.
    pub async fn rename_contact_list(
        &self,
        uuid: String,
        name: String,
        search: String,
    ) -> Result<bool, StoreError> {
        let now = now_millis();
        self.write(move |tx| rename(tx, &uuid, &name, &search, now))
            .await
    }

    /// Stage 19: removes the group (not its members' contacts); `false` for an unknown one.
    pub async fn delete_contact_list(&self, uuid: String) -> Result<bool, StoreError> {
        self.write(move |tx| delete(tx, &uuid)).await
    }

    /// Stage 19: adds the members not in the group yet (at the end); how many were added.
    /// `None` for an unknown group; `InvalidArgument` beyond [`MAX_CONTACT_LIST_MEMBERS`].
    pub async fn add_contact_list_members(
        &self,
        uuid: String,
        members: Vec<NewListMember>,
    ) -> Result<Option<usize>, StoreError> {
        let now = now_millis();
        self.write(move |tx| add_members(tx, &uuid, &members, now))
            .await
    }

    /// Stage 19: removes one member by its address key; `None` for an unknown group.
    pub async fn remove_contact_list_member(
        &self,
        uuid: String,
        address_key: String,
    ) -> Result<Option<bool>, StoreError> {
        let now = now_millis();
        self.write(move |tx| remove_member(tx, &uuid, &address_key, now))
            .await
    }

    /// Stage 19 (vCard import): rows for addresses the directory does not know yet, with the
    /// name from the card; known addresses are left alone. Returns how many were new.
    pub async fn insert_new_contacts(
        &self,
        contacts: Vec<crate::StoredContact>,
    ) -> Result<usize, StoreError> {
        let now = now_millis();
        self.write(move |tx| insert_new_contacts(tx, &contacts, now))
            .await
    }

    /// Stage 19: groups for the composer's suggestions (see [`search`]).
    pub async fn search_contact_lists(
        &self,
        words: Vec<String>,
        limit: u32,
    ) -> Result<Vec<ListSummary>, StoreError> {
        self.read(move |c| search(c, &words, limit)).await
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::StoreOptions;
    use crate::test_support::db_path;

    fn open() -> (TempDir, Store) {
        let dir = TempDir::new().expect("temp dir");
        let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
        (dir, store)
    }

    fn member(email: &str, name: Option<&str>) -> NewListMember {
        NewListMember {
            address_key: email.to_lowercase(),
            email: email.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn groups_keep_their_members_in_order_once_each() {
        let (_dir, store) = open();
        let uuid = store
            .create_contact_list(
                "Team Nord".into(),
                " team nord".into(),
                vec![
                    member("ada@example.org", Some("Ada")),
                    member("grace@example.org", None),
                    member("ADA@example.org", None),
                ],
            )
            .await
            .expect("create");
        let list = store
            .contact_list(uuid.clone())
            .await
            .expect("read")
            .expect("list");
        assert_eq!(list.name, "Team Nord");
        let emails: Vec<&str> = list.members.iter().map(|m| m.email.as_str()).collect();
        assert_eq!(emails, ["ada@example.org", "grace@example.org"]);
        assert_eq!(list.members[0].name.as_deref(), Some("Ada"));
        assert_eq!(list.members[0].contact_key, None, "no contact needed");

        let added = store
            .add_contact_list_members(
                uuid.clone(),
                vec![
                    member("linus@example.org", None),
                    member("grace@example.org", None),
                ],
            )
            .await
            .expect("add");
        assert_eq!(added, Some(1));
        assert_eq!(
            store
                .remove_contact_list_member(uuid.clone(), "ada@example.org".into())
                .await
                .expect("remove"),
            Some(true)
        );
        let list = store
            .contact_list(uuid.clone())
            .await
            .expect("read")
            .expect("list");
        let emails: Vec<&str> = list.members.iter().map(|m| m.email.as_str()).collect();
        assert_eq!(emails, ["grace@example.org", "linus@example.org"]);

        assert!(
            store
                .rename_contact_list(uuid.clone(), "Team Süd".into(), " team sud".into())
                .await
                .expect("rename")
        );
        let found = store
            .search_contact_lists(vec!["su".into()], 10)
            .await
            .expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members, 2);
        assert!(
            store
                .search_contact_lists(vec!["nord".into()], 10)
                .await
                .expect("search")
                .is_empty()
        );
        // LIKE wildcards in the query match nothing special.
        assert!(
            store
                .search_contact_lists(vec!["%".into()], 10)
                .await
                .expect("search")
                .is_empty()
        );
        assert!(
            store
                .delete_contact_list(uuid.clone())
                .await
                .expect("delete")
        );
        assert!(store.contact_list(uuid).await.expect("read").is_none());
        assert_eq!(
            store
                .add_contact_list_members("0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01".into(), Vec::new())
                .await
                .expect("unknown"),
            None
        );
    }

    #[tokio::test]
    async fn a_synced_group_arrives_as_it_was_and_a_tombstone_removes_it() {
        let (_a, here) = open();
        let (_b, there) = open();
        let uuid = here
            .create_contact_list(
                "Chor".into(),
                " chor".into(),
                vec![
                    member("ada@example.org", Some("Ada L.")),
                    member("b@example.org", None),
                ],
            )
            .await
            .expect("create");
        let content = here
            .read({
                let uuid = uuid.clone();
                move |c| synced_content(c, &uuid)
            })
            .await
            .expect("content")
            .expect("record");
        for _ in 0..2 {
            let (uuid, content) = (uuid.clone(), content.clone());
            there
                .write(move |tx| {
                    let members = synced_members(&content);
                    apply_synced(tx, &uuid, Some(("Chor", " chor", &members)), 1)
                })
                .await
                .expect("apply twice: idempotent");
        }
        let arrived = there
            .contact_list(uuid.clone())
            .await
            .expect("read")
            .expect("list");
        assert_eq!(
            arrived,
            here.contact_list(uuid.clone())
                .await
                .expect("read")
                .expect("list")
        );
        there
            .write({
                let uuid = uuid.clone();
                move |tx| apply_synced(tx, &uuid, None, 2)
            })
            .await
            .expect("tombstone");
        assert!(there.contact_list(uuid).await.expect("read").is_none());
    }

    #[tokio::test]
    async fn the_member_limit_holds_and_changes_are_captured_for_the_sync() {
        let (_dir, store) = open();
        let many: Vec<NewListMember> = (0..=MAX_CONTACT_LIST_MEMBERS)
            .map(|n| member(&format!("m{n}@example.org"), None))
            .collect();
        let err = store
            .create_contact_list("Too many".into(), " too many".into(), many)
            .await
            .expect_err("too many");
        assert!(matches!(err, StoreError::InvalidArgument(_)), "{err:?}");
        assert!(
            store.contact_lists().await.expect("lists").is_empty(),
            "nothing kept"
        );

        let uuid = store
            .create_contact_list(
                "Synced".into(),
                " synced".into(),
                vec![member("a@example.org", None)],
            )
            .await
            .expect("create");
        let dirty: Vec<(String, String)> = store
            .read(|c| {
                Ok(
                    c.prepare("SELECT kind, id FROM sync_dirty WHERE kind = 'contactList'")?
                        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                        .collect::<Result<Vec<_>, _>>()?,
                )
            })
            .await
            .expect("dirty");
        assert_eq!(dirty, vec![("contactList".to_owned(), uuid.clone())]);
        let content = store
            .read({
                let uuid = uuid.clone();
                move |c| synced_content(c, &uuid)
            })
            .await
            .expect("content")
            .expect("record");
        assert_eq!(
            content,
            json!({
                "name": "Synced",
                "search": " synced",
                "members": [{ "email": "a@example.org", "name": null, "key": "a@example.org" }]
            })
        );
    }
}
