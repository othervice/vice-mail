//! Mailboxes: the folder list, parents, roles and sync bookkeeping.

use std::collections::{HashMap, HashSet};

use mail_model::{
    AccountId, Mailbox, MailboxId, MailboxRole, ModSeq, RemoteMailbox, RoleInput, RoleSource,
    UidValidity, assign_roles, leaf_name,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::accounts;
use crate::mail_api::{MailboxBaseline, MailboxListReport, MailboxSyncState};

/// Mailboxes of an account for the UI, with local counts. Unread through the partial index
/// `loc_unseen`, total through `loc_uid`; both covering, proportional to the counted rows.
pub(crate) const LIST_MAILBOXES: &str = "
    SELECT b.id, b.path, b.delimiter, b.parent_id, b.role, b.role_source, b.selectable,
           (SELECT count(*) FROM message_location l
             WHERE l.mailbox_id = b.id AND l.seen = 0),
           (SELECT count(*) FROM message_location l INDEXED BY loc_uid
             WHERE l.mailbox_id = b.id)
    FROM mailbox b WHERE b.account_id = ?1 ORDER BY b.path";

/// Unread count of every mailbox of an account (`loc_unseen`).
pub(crate) const UNREAD_COUNTS: &str = "
    SELECT b.id,
           (SELECT count(*) FROM message_location l
             WHERE l.mailbox_id = b.id AND l.seen = 0)
    FROM mailbox b WHERE b.account_id = ?1 ORDER BY b.path";

pub(crate) const SYNC_STATES: &str = "
    SELECT id, name, path, role, selectable, uidvalidity, uidnext, highestmodseq, sync_low_uid
    FROM mailbox WHERE account_id = ?1 ORDER BY path";

/// A mailbox and its account, before setting a role (scalar subquery as in `MAILBOX_ACCOUNT`).
pub(crate) const MAILBOX_FOR_ROLE: &str = "
    SELECT b.account_id, (SELECT a.uuid FROM account a WHERE a.id = b.account_id),
           b.path, b.role, b.selectable
    FROM mailbox b WHERE b.id = ?1";

/// All mailboxes of an account, for the list refresh.
pub(crate) const LOAD: &str = "
    SELECT id, name, path, delimiter, parent_id, attributes, selectable, role, role_source
    FROM mailbox WHERE account_id = ?1";

/// A stored mailbox row, as far as the list refresh needs it.
struct Row {
    id: i64,
    name: String,
    path: String,
    delimiter: Option<String>,
    parent_id: Option<i64>,
    attributes: String,
    selectable: bool,
    role: Option<String>,
    role_source: String,
}

impl Row {
    fn delimiter_char(&self) -> Option<char> {
        self.delimiter.as_deref().and_then(|d| d.chars().next())
    }
}

fn load(conn: &Connection, account_row: i64) -> Result<Vec<Row>, StoreError> {
    let mut rows = conn
        .prepare_cached(LOAD)?
        .query_map([account_row], |r| {
            Ok(Row {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                delimiter: r.get(3)?,
                parent_id: r.get(4)?,
                attributes: r.get(5)?,
                selectable: r.get(6)?,
                role: r.get(7)?,
                role_source: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<Row>, _>>()?;
    // Sorted here, not in SQL: no temporary B-tree, and a stable order for `recompute`.
    rows.sort_by_key(|m| m.id);
    Ok(rows)
}

/// See [`crate::Store::sync_mailboxes`].
pub(crate) fn sync(
    conn: &Connection,
    account: AccountId,
    remote: &[RemoteMailbox],
) -> Result<MailboxListReport, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    let existing = load(conn, account_row)?;
    let by_name: HashMap<&str, &Row> = existing.iter().map(|m| (m.name.as_str(), m)).collect();

    let mut report = MailboxListReport::default();
    let mut listed: HashSet<&str> = HashSet::new();
    for mailbox in remote {
        if !listed.insert(mailbox.name.as_str()) {
            tracing::warn!(%account, "the server listed a mailbox twice; the first entry is used");
            continue;
        }
        let attributes = mailbox.attributes.join(" ");
        let delimiter = mailbox.delimiter.map(String::from);
        let selectable = mailbox.is_selectable();
        match by_name.get(mailbox.name.as_str()) {
            Some(old) => {
                if old.path != mailbox.path
                    || old.delimiter != delimiter
                    || old.attributes != attributes
                    || old.selectable != selectable
                {
                    conn.prepare_cached(
                        "UPDATE mailbox SET path = ?2, delimiter = ?3, attributes = ?4,
                                            selectable = ?5
                         WHERE id = ?1",
                    )?
                    .execute(params![
                        old.id,
                        mailbox.path,
                        delimiter,
                        attributes,
                        selectable
                    ])
                    .map_err(StoreError::sqlite("update mailbox"))?;
                    report.changed = true;
                }
            }
            None => {
                conn.prepare_cached(
                    "INSERT INTO mailbox (account_id, name, path, delimiter, attributes, selectable)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?
                .execute(params![
                    account_row,
                    mailbox.name,
                    mailbox.path,
                    delimiter,
                    attributes,
                    selectable
                ])
                .map_err(StoreError::sqlite("insert mailbox"))?;
                report.added.push(MailboxId(conn.last_insert_rowid()));
            }
        }
    }

    for old in &existing {
        if !listed.contains(old.name.as_str()) {
            // Locations go with the mailbox (ON DELETE CASCADE); messages stay as orphans.
            conn.prepare_cached("DELETE FROM mailbox WHERE id = ?1")?
                .execute([old.id])
                .map_err(StoreError::sqlite("delete mailbox"))?;
            report.removed.push(MailboxId(old.id));
        }
    }

    let recomputed = recompute(conn, account_row, None)?;
    report.changed |= recomputed || !report.added.is_empty() || !report.removed.is_empty();
    Ok(report)
}

/// See [`crate::Store::set_mailbox_role`].
pub(crate) fn set_role(
    conn: &Connection,
    mailbox: MailboxId,
    role: MailboxRole,
) -> Result<Vec<Mailbox>, StoreError> {
    let found = conn
        .prepare_cached(MAILBOX_FOR_ROLE)?
        .query_row([mailbox.0], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                accounts::account_id_at(r, 1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })
        .optional()?;
    let Some((account_row, account, path, current, selectable)) = found else {
        return Err(StoreError::NotFound(format!("mailbox {mailbox}")));
    };
    if role == MailboxRole::Inbox {
        return Err(StoreError::InvalidArgument(
            "the inbox role belongs to INBOX and cannot be assigned".into(),
        ));
    }
    if current.as_deref() == MailboxRole::Inbox.as_db() || path.eq_ignore_ascii_case("INBOX") {
        return Err(StoreError::InvalidArgument(
            "the inbox keeps its role".into(),
        ));
    }
    if !selectable {
        return Err(StoreError::InvalidArgument(format!(
            "mailbox {mailbox} cannot hold messages and has no role"
        )));
    }
    recompute(conn, account_row, Some((mailbox.0, role)))?;
    list(conn, account_row, account)
}

/// Recomputes `parent_id` from the paths and the roles with [`assign_roles`], and writes what
/// changed. `chosen` is a role the user sets now on one mailbox; another mailbox whose user
/// role equals it loses that choice. Returns whether anything changed.
fn recompute(
    conn: &Connection,
    account_row: i64,
    chosen: Option<(i64, MailboxRole)>,
) -> Result<bool, StoreError> {
    let rows = load(conn, account_row)?;

    // Rows are ordered by id, so of two mailboxes with the same path the older is the parent.
    let mut by_path: HashMap<&str, i64> = HashMap::new();
    for m in &rows {
        by_path.entry(m.path.as_str()).or_insert(m.id);
    }

    let attributes: Vec<Vec<String>> = rows
        .iter()
        .map(|m| m.attributes.split_whitespace().map(str::to_owned).collect())
        .collect();
    let inputs: Vec<RoleInput<'_>> = rows
        .iter()
        .zip(&attributes)
        .map(|(m, attributes)| RoleInput {
            path: &m.path,
            delimiter: m.delimiter_char(),
            attributes,
            user_role: user_role(m, chosen),
        })
        .collect();
    let assigned = assign_roles(&inputs);

    struct Change {
        id: i64,
        parent_id: Option<i64>,
        role: Option<&'static str>,
        source: &'static str,
        clears_role: bool,
    }
    let mut changes = Vec::new();
    for (m, assignment) in rows.iter().zip(assigned) {
        let parent_id = m
            .delimiter_char()
            .and_then(|d| m.path.rsplit_once(d))
            .and_then(|(parent, _)| by_path.get(parent).copied())
            .filter(|&parent| parent != m.id);
        let role = assignment.role.as_db();
        let source = assignment.source.as_db();
        if parent_id != m.parent_id || role != m.role.as_deref() || source != m.role_source {
            changes.push(Change {
                id: m.id,
                parent_id,
                role,
                source,
                clears_role: m.role.is_some() && role != m.role.as_deref(),
            });
        }
    }

    // Two passes keep the unique index `mailbox_role` satisfied after every statement: roles
    // that move are cleared first, then all new values are written.
    for change in changes.iter().filter(|c| c.clears_role) {
        conn.prepare_cached("UPDATE mailbox SET role = NULL WHERE id = ?1")?
            .execute([change.id])
            .map_err(StoreError::sqlite("clear mailbox role"))?;
    }
    for change in &changes {
        conn.prepare_cached(
            "UPDATE mailbox SET parent_id = ?2, role = ?3, role_source = ?4 WHERE id = ?1",
        )?
        .execute(params![
            change.id,
            change.parent_id,
            change.role,
            change.source
        ])
        .map_err(StoreError::sqlite("update mailbox role"))?;
    }
    Ok(!changes.is_empty())
}

/// The role the user chose for `m`, as input for [`assign_roles`].
fn user_role(m: &Row, chosen: Option<(i64, MailboxRole)>) -> Option<MailboxRole> {
    if let Some((id, role)) = chosen
        && id == m.id
    {
        return Some(role);
    }
    if m.role_source != RoleSource::User.as_db() {
        return None;
    }
    let stored = MailboxRole::from_db(m.role.as_deref());
    match chosen {
        // The role moves to the mailbox the user picks now.
        Some((_, role)) if role != MailboxRole::Custom && role == stored => None,
        _ => Some(stored),
    }
}

/// Mailboxes of the account for the UI, ordered by path.
pub(crate) fn list(
    conn: &Connection,
    account_row: i64,
    account: AccountId,
) -> Result<Vec<Mailbox>, StoreError> {
    let mailboxes = conn
        .prepare_cached(LIST_MAILBOXES)?
        .query_map([account_row], |r| {
            let path: String = r.get(1)?;
            let delimiter: Option<String> = r.get(2)?;
            let name =
                leaf_name(&path, delimiter.as_deref().and_then(|d| d.chars().next())).to_owned();
            let role: Option<String> = r.get(4)?;
            let role_source: String = r.get(5)?;
            Ok(Mailbox {
                id: MailboxId(r.get(0)?),
                account_id: account,
                path,
                name,
                delimiter,
                parent_id: r.get::<_, Option<i64>>(3)?.map(MailboxId),
                role: MailboxRole::from_db(role.as_deref()),
                role_source: RoleSource::from_db(&role_source),
                selectable: r.get(6)?,
                unread: count_at(r, 7)?,
                total: count_at(r, 8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(mailboxes)
}

pub(crate) fn list_for(conn: &Connection, account: AccountId) -> Result<Vec<Mailbox>, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    list(conn, account_row, account)
}

pub(crate) fn unread_counts(
    conn: &Connection,
    account: AccountId,
) -> Result<Vec<(MailboxId, u32)>, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    let counts = conn
        .prepare_cached(UNREAD_COUNTS)?
        .query_map([account_row], |r| {
            Ok((MailboxId(r.get(0)?), count_at(r, 1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(counts)
}

pub(crate) fn sync_states(
    conn: &Connection,
    account: AccountId,
) -> Result<Vec<MailboxSyncState>, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    let states = conn
        .prepare_cached(SYNC_STATES)?
        .query_map([account_row], |r| {
            let role: Option<String> = r.get(3)?;
            Ok(MailboxSyncState {
                mailbox_id: MailboxId(r.get(0)?),
                name: r.get(1)?,
                path: r.get(2)?,
                role: MailboxRole::from_db(role.as_deref()),
                selectable: r.get(4)?,
                uid_validity: r.get::<_, Option<u32>>(5)?.map(UidValidity),
                uid_next: r.get(6)?,
                highest_modseq: modseq_at(r, 7)?,
                sync_low_uid: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(states)
}

/// Checks that the mailbox exists and its stored UIDVALIDITY is `given`. Returns the account's
/// row id. Every sync write except [`reset`] goes through here first.
pub(crate) fn check_uid_validity(
    conn: &Connection,
    mailbox: MailboxId,
    given: UidValidity,
) -> Result<i64, StoreError> {
    let found: Option<(i64, Option<u32>)> = conn
        .prepare_cached("SELECT account_id, uidvalidity FROM mailbox WHERE id = ?1")?
        .query_row([mailbox.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((account_row, stored)) = found else {
        return Err(StoreError::NotFound(format!("mailbox {mailbox}")));
    };
    if stored != Some(given.0) {
        return Err(StoreError::UidValidityMismatch {
            mailbox: mailbox.0,
            stored,
            given: given.0,
        });
    }
    Ok(account_row)
}

/// Fails with [`StoreError::NotFound`] unless the mailbox exists.
pub(crate) fn ensure_exists(conn: &Connection, mailbox: MailboxId) -> Result<(), StoreError> {
    let found: Option<i64> = conn
        .prepare_cached("SELECT id FROM mailbox WHERE id = ?1")?
        .query_row([mailbox.0], |r| r.get(0))
        .optional()?;
    found
        .map(|_| ())
        .ok_or_else(|| StoreError::NotFound(format!("mailbox {mailbox}")))
}

/// See [`crate::Store::reset_mailbox`].
pub(crate) fn reset(
    conn: &Connection,
    mailbox: MailboxId,
    baseline: MailboxBaseline,
) -> Result<usize, StoreError> {
    ensure_exists(conn, mailbox)?;
    let removed = conn
        .prepare_cached("DELETE FROM message_location WHERE mailbox_id = ?1")?
        .execute([mailbox.0])
        .map_err(StoreError::sqlite("delete locations of mailbox"))?;
    conn.prepare_cached(
        "UPDATE mailbox SET uidvalidity = ?2, uidnext = ?3, highestmodseq = ?4,
                            sync_low_uid = ?3
         WHERE id = ?1",
    )?
    .execute(params![
        mailbox.0,
        baseline.uid_validity.0,
        baseline.uid_next,
        modseq_param(baseline.highest_modseq)?,
    ])
    .map_err(StoreError::sqlite("reset mailbox"))?;
    Ok(removed)
}

pub(crate) fn set_uid_next(
    conn: &Connection,
    mailbox: MailboxId,
    uid_validity: UidValidity,
    uid_next: u32,
) -> Result<(), StoreError> {
    check_uid_validity(conn, mailbox, uid_validity)?;
    conn.prepare_cached("UPDATE mailbox SET uidnext = ?2 WHERE id = ?1")?
        .execute(params![mailbox.0, uid_next])
        .map_err(StoreError::sqlite("set uidnext"))?;
    Ok(())
}

pub(crate) fn set_highest_modseq(
    conn: &Connection,
    mailbox: MailboxId,
    uid_validity: UidValidity,
    highest_modseq: Option<ModSeq>,
) -> Result<(), StoreError> {
    check_uid_validity(conn, mailbox, uid_validity)?;
    conn.prepare_cached("UPDATE mailbox SET highestmodseq = ?2 WHERE id = ?1")?
        .execute(params![mailbox.0, modseq_param(highest_modseq)?])
        .map_err(StoreError::sqlite("set highestmodseq"))?;
    Ok(())
}

pub(crate) fn set_sync_low_uid(
    conn: &Connection,
    mailbox: MailboxId,
    sync_low_uid: u32,
) -> Result<(), StoreError> {
    conn.prepare_cached("UPDATE mailbox SET sync_low_uid = ?2 WHERE id = ?1")?
        .execute(params![mailbox.0, sync_low_uid])
        .map_err(StoreError::sqlite("set sync progress"))?;
    Ok(())
}

/// MODSEQ as SQLite INTEGER. RFC 7162 limits it to 2^63 - 1, which fits.
pub(crate) fn modseq_param(modseq: Option<ModSeq>) -> Result<Option<i64>, StoreError> {
    modseq
        .map(|m| {
            i64::try_from(m.0)
                .map_err(|_| StoreError::InvalidArgument(format!("MODSEQ {m} is out of range")))
        })
        .transpose()
}

fn modseq_at(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<ModSeq>> {
    r.get::<_, Option<i64>>(idx)?
        .map(|v| {
            u64::try_from(v).map(ModSeq).map_err(|_| {
                accounts::conversion_error(
                    idx,
                    rusqlite::types::Type::Integer,
                    format!("MODSEQ {v}"),
                )
            })
        })
        .transpose()
}

fn count_at(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<u32> {
    let n: i64 = r.get(idx)?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}
