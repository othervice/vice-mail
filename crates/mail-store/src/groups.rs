//! Account groups (class sync, by account UUID), hidden accounts (class Gerät) and identities.
//! Synchronous functions over a connection; `search_api` runs them on the writer thread or a
//! reader.

use std::collections::{HashMap, HashSet};

use mail_model::{
    AccountGroup, AccountGroupId, AccountId, Identity, IdentityId, IdentityInput,
    MAX_GROUP_NAME_CHARS, MailboxId,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::accounts::{self, account_id_at};
use crate::settings::now_millis;

/// A random UUID v4 from SQLite's random source (the store has no randomness of its own).
pub(crate) const NEW_UUID: &str = "
    SELECT lower(substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-4' || substr(h, 14, 3) || '-' ||
                 substr('89ab', 1 + (abs(random()) % 4), 1) || substr(h, 18, 3) || '-' ||
                 substr(h, 21, 12))
    FROM (SELECT hex(randomblob(16)) AS h)";

pub(crate) const GROUPS: &str =
    "SELECT id, uuid, name, position FROM account_group ORDER BY position, id";
pub(crate) const GROUP_BY_UUID: &str = "SELECT id FROM account_group WHERE uuid = ?1";
pub(crate) const GROUP_MEMBERS: &str =
    "SELECT account_uuid FROM account_group_member WHERE group_id = ?1";
pub(crate) const HIDDEN: &str = "SELECT account_uuid FROM account_hidden";
/// Names and roles of an account's mailboxes, without counters (`in:` of a search).
pub(crate) const MAILBOX_NAMES: &str =
    "SELECT id, path, delimiter, role FROM mailbox WHERE account_id = ?1";

/// A mailbox as `in:` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxName {
    pub id: MailboxId,
    /// Full decoded path.
    pub path: String,
    /// Last part of the path.
    pub name: String,
    pub role: mail_model::MailboxRole,
}

pub(crate) fn mailbox_names(
    conn: &Connection,
    account: AccountId,
) -> Result<Vec<MailboxName>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    let names = conn
        .prepare_cached(MAILBOX_NAMES)?
        .query_map([row], |r| {
            let path: String = r.get(1)?;
            let delimiter: Option<String> = r.get(2)?;
            let role: Option<String> = r.get(3)?;
            let name =
                mail_model::leaf_name(&path, delimiter.and_then(|d| d.chars().next())).to_owned();
            Ok(MailboxName {
                id: MailboxId(r.get(0)?),
                path,
                name,
                role: mail_model::MailboxRole::from_db(role.as_deref()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

pub(crate) const INBOX_OF: &str =
    "SELECT id FROM mailbox WHERE account_id = ?1 AND role = 'inbox' AND selectable = 1";

pub(crate) fn new_uuid(conn: &Connection) -> Result<String, StoreError> {
    Ok(conn.prepare_cached(NEW_UUID)?.query_row([], |r| r.get(0))?)
}

fn clean_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_GROUP_NAME_CHARS {
        return Err(StoreError::InvalidArgument(format!(
            "a group name has 1 to {MAX_GROUP_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(StoreError::InvalidArgument(
            "a group name has no control characters".into(),
        ));
    }
    Ok(name.to_owned())
}

/// All groups, in sidebar order; members in account order (oldest first).
pub(crate) fn groups(conn: &Connection) -> Result<Vec<AccountGroup>, StoreError> {
    let order: HashMap<AccountId, usize> = accounts::all(conn)?
        .into_iter()
        .enumerate()
        .map(|(i, a)| (a.id, i))
        .collect();
    let rows = conn
        .prepare_cached(GROUPS)?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (row, uuid, name, position) in rows {
        let mut account_ids = conn
            .prepare_cached(GROUP_MEMBERS)?
            .query_map([row], |r| account_id_at(r, 0))?
            .collect::<Result<Vec<_>, _>>()?;
        account_ids.sort_by_key(|a| order.get(a).copied().unwrap_or(usize::MAX));
        out.push(AccountGroup {
            id: AccountGroupId(uuid),
            name,
            account_ids,
            position,
        });
    }
    Ok(out)
}

fn group_row(conn: &Connection, id: &AccountGroupId) -> Result<i64, StoreError> {
    conn.prepare_cached(GROUP_BY_UUID)?
        .query_row([&id.0], |r| r.get(0))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("account group {id}")))
}

fn set_members(conn: &Connection, group: i64, members: &[AccountId]) -> Result<(), StoreError> {
    for account in members {
        accounts::row_id(conn, *account)?;
    }
    conn.execute(
        "DELETE FROM account_group_member WHERE group_id = ?1",
        [group],
    )
    .map_err(StoreError::sqlite("clear group members"))?;
    let mut insert = conn.prepare_cached(
        "INSERT OR IGNORE INTO account_group_member (group_id, account_uuid) VALUES (?1, ?2)",
    )?;
    for account in members {
        insert
            .execute(params![group, account.to_string()])
            .map_err(StoreError::sqlite("add group member"))?;
    }
    Ok(())
}

pub(crate) fn create_group(
    conn: &Connection,
    name: &str,
    members: &[AccountId],
) -> Result<AccountGroupId, StoreError> {
    let name = clean_name(name)?;
    let uuid = new_uuid(conn)?;
    let position: i64 = conn.query_row(
        "SELECT coalesce(max(position), -1) + 1 FROM account_group",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO account_group (uuid, name, position, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![uuid, name, position, now_millis()],
    )
    .map_err(StoreError::sqlite("create account group"))?;
    set_members(conn, conn.last_insert_rowid(), members)?;
    Ok(AccountGroupId(uuid))
}

pub(crate) fn update_group(
    conn: &Connection,
    id: &AccountGroupId,
    name: &str,
    members: &[AccountId],
) -> Result<(), StoreError> {
    let name = clean_name(name)?;
    let row = group_row(conn, id)?;
    conn.execute(
        "UPDATE account_group SET name = ?2 WHERE id = ?1",
        params![row, name],
    )
    .map_err(StoreError::sqlite("rename account group"))?;
    set_members(conn, row, members)
}

pub(crate) fn delete_group(conn: &Connection, id: &AccountGroupId) -> Result<bool, StoreError> {
    Ok(conn
        .execute("DELETE FROM account_group WHERE uuid = ?1", [&id.0])
        .map_err(StoreError::sqlite("delete account group"))?
        > 0)
}

pub(crate) fn hidden(conn: &Connection) -> Result<HashSet<AccountId>, StoreError> {
    let rows = conn
        .prepare_cached(HIDDEN)?
        .query_map([], |r| account_id_at(r, 0))?
        .collect::<Result<HashSet<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn set_hidden(
    conn: &Connection,
    account: AccountId,
    hidden: bool,
) -> Result<bool, StoreError> {
    accounts::row_id(conn, account)?;
    let changed = if hidden {
        conn.execute(
            "INSERT OR IGNORE INTO account_hidden (account_uuid, hidden_at) VALUES (?1, ?2)",
            params![account.to_string(), now_millis()],
        )
    } else {
        conn.execute(
            "DELETE FROM account_hidden WHERE account_uuid = ?1",
            [account.to_string()],
        )
    }
    .map_err(StoreError::sqlite("hide account"))?;
    Ok(changed > 0)
}

/// The visible accounts (oldest first), or the visible members of `group`.
pub(crate) fn visible_accounts(
    conn: &Connection,
    group: Option<&AccountGroupId>,
) -> Result<Vec<(i64, AccountId)>, StoreError> {
    let hidden = hidden(conn)?;
    let members: Option<HashSet<AccountId>> = match group {
        Some(group) => {
            let row = group_row(conn, group)?;
            Some(
                conn.prepare_cached(GROUP_MEMBERS)?
                    .query_map([row], |r| account_id_at(r, 0))?
                    .collect::<Result<HashSet<_>, _>>()?,
            )
        }
        None => None,
    };
    let mut out = Vec::new();
    for account in accounts::all(conn)? {
        if hidden.contains(&account.id)
            || members.as_ref().is_some_and(|m| !m.contains(&account.id))
        {
            continue;
        }
        out.push((accounts::row_id(conn, account.id)?, account.id));
    }
    Ok(out)
}

/// The inboxes of the visible accounts (or of the group's), in account order.
pub(crate) fn unified_inboxes(
    conn: &Connection,
    group: Option<&AccountGroupId>,
) -> Result<Vec<(MailboxId, AccountId)>, StoreError> {
    let mut out = Vec::new();
    for (row, account) in visible_accounts(conn, group)? {
        if let Some(inbox) = conn
            .prepare_cached(INBOX_OF)?
            .query_row([row], |r| r.get::<_, i64>(0))
            .optional()?
        {
            out.push((MailboxId(inbox), account));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Identities

fn clean_identity(input: &IdentityInput) -> Result<IdentityInput, StoreError> {
    let email = input.email.trim();
    let valid = |a: &str| {
        let mut parts = a.split('@');
        matches!((parts.next(), parts.next(), parts.next()),
            (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty())
            && !a
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | '"'))
            && a.len() <= 254
    };
    if !valid(email) {
        return Err(StoreError::InvalidArgument(format!(
            "{email:?} is not an email address"
        )));
    }
    let name = input.name.trim();
    if name.chars().any(char::is_control) || name.chars().count() > 200 {
        return Err(StoreError::InvalidArgument(
            "the name has at most 200 characters and no control characters".into(),
        ));
    }
    let reply_to = input
        .reply_to
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());
    if let Some(reply_to) = reply_to
        && !valid(reply_to)
    {
        return Err(StoreError::InvalidArgument(format!(
            "{reply_to:?} is not an email address"
        )));
    }
    Ok(IdentityInput {
        email: email.to_owned(),
        name: name.to_owned(),
        reply_to: reply_to.map(str::to_owned),
    })
}

fn identity_account(conn: &Connection, id: IdentityId) -> Result<(i64, bool), StoreError> {
    conn.prepare_cached("SELECT account_id, is_default FROM identity WHERE id = ?1")?
        .query_row([id.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("identity {}", id.0)))
}

fn email_taken(
    conn: &Connection,
    account_row: i64,
    email: &str,
    except: Option<IdentityId>,
) -> Result<bool, StoreError> {
    let found: Option<i64> = conn
        .prepare_cached(
            "SELECT id FROM identity WHERE account_id = ?1 AND email = ?2 COLLATE NOCASE",
        )?
        .query_row(params![account_row, email], |r| r.get(0))
        .optional()?;
    Ok(found.is_some_and(|f| Some(IdentityId(f)) != except))
}

pub(crate) fn add_identity(
    conn: &Connection,
    account: AccountId,
    input: &IdentityInput,
) -> Result<IdentityId, StoreError> {
    let input = clean_identity(input)?;
    let row = accounts::row_id(conn, account)?;
    if email_taken(conn, row, &input.email, None)? {
        return Err(StoreError::InvalidArgument(format!(
            "the account has an identity for {} already",
            input.email
        )));
    }
    conn.execute(
        "INSERT INTO identity (account_id, email, name, reply_to, is_default)
         VALUES (?1, ?2, ?3, ?4, 0)",
        params![row, input.email, input.name, input.reply_to],
    )
    .map_err(StoreError::sqlite("add identity"))?;
    Ok(IdentityId(conn.last_insert_rowid()))
}

pub(crate) fn update_identity(
    conn: &Connection,
    id: IdentityId,
    input: &IdentityInput,
) -> Result<AccountId, StoreError> {
    let input = clean_identity(input)?;
    let (row, _) = identity_account(conn, id)?;
    if email_taken(conn, row, &input.email, Some(id))? {
        return Err(StoreError::InvalidArgument(format!(
            "the account has an identity for {} already",
            input.email
        )));
    }
    conn.execute(
        "UPDATE identity SET email = ?2, name = ?3, reply_to = ?4 WHERE id = ?1",
        params![id.0, input.email, input.name, input.reply_to],
    )
    .map_err(StoreError::sqlite("update identity"))?;
    account_of_row(conn, row)
}

/// Makes `id` the default of its account (the old default stops being one).
pub(crate) fn set_default_identity(
    conn: &Connection,
    id: IdentityId,
) -> Result<AccountId, StoreError> {
    let (row, _) = identity_account(conn, id)?;
    conn.execute(
        "UPDATE identity SET is_default = 0 WHERE account_id = ?1 AND is_default = 1",
        [row],
    )
    .map_err(StoreError::sqlite("clear default identity"))?;
    conn.execute("UPDATE identity SET is_default = 1 WHERE id = ?1", [id.0])
        .map_err(StoreError::sqlite("set default identity"))?;
    account_of_row(conn, row)
}

/// Deletes an identity; the default one and the last one cannot be deleted.
pub(crate) fn delete_identity(conn: &Connection, id: IdentityId) -> Result<AccountId, StoreError> {
    let (row, is_default) = identity_account(conn, id)?;
    if is_default {
        return Err(StoreError::InvalidArgument(
            "the default identity cannot be deleted; make another one the default first".into(),
        ));
    }
    conn.execute("DELETE FROM identity WHERE id = ?1", [id.0])
        .map_err(StoreError::sqlite("delete identity"))?;
    account_of_row(conn, row)
}

fn account_of_row(conn: &Connection, row: i64) -> Result<AccountId, StoreError> {
    conn.prepare_cached("SELECT uuid FROM account WHERE id = ?1")?
        .query_row([row], |r| account_id_at(r, 0))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("account row {row}")))
}

/// The identity with this id.
pub(crate) fn identity(conn: &Connection, id: IdentityId) -> Result<Identity, StoreError> {
    let (row, _) = identity_account(conn, id)?;
    let account = account_of_row(conn, row)?;
    accounts::identities(conn, account)?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| StoreError::NotFound(format!("identity {}", id.0)))
}
