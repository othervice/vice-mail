//! Running a search (decision 0003, stage 7). The engine parses and compiles the query and
//! resolves `account:` and `in:` to ids; this module turns the result into SQL.
//!
//! **Drivers** (what the query walks, newest first, stopping after `limit + 1` rows):
//! 1. A text match: `message_fts MATCH ?` ordered by rowid descending, with the date filter and
//!    the cursor as a rowid range inside FTS5; every other filter is checked per row on
//!    `message` (CROSS JOIN, rowid lookup) and on `message_location` (`loc_message`).
//! 2. No text, but mailboxes (`in:` or a mailbox scope): one branch per mailbox of the first
//!    set, each a range of the location key (with `is:unread`/`is:flagged` on the location, so
//!    `loc_unseen` can drive), combined with `UNION ALL`. A message in two of these mailboxes
//!    comes twice and is merged here.
//! 3. Otherwise `message` by its primary key from the cursor down (filters per row; only
//!    exclusions such as `-spam` become `rowid NOT IN (SELECT rowid … MATCH ?)`).
//!
//! The SQL text depends on the filters (lists of ids are written as integer literals: they
//! come from the database, never from input); values the user typed are always parameters.

use mail_model::search::{RowidRange, TextMatch};
use mail_model::{AccountId, ListCursor, MailboxId, MessageId};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::StoreError;
use crate::accounts;
use crate::lists::{Entry, entry};

/// Largest page of [`crate::Store::search`].
pub const MAX_SEARCH_LIMIT: u32 = 500;

/// A compiled query with `account:` and `in:` resolved (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchFilter {
    pub text: TextMatch,
    pub dates: RowidRange,
    /// Only these accounts; `None` for all.
    pub accounts: Option<Vec<AccountId>>,
    /// The message lies in at least one mailbox of each set (`in:`, the mailbox scope). An
    /// empty set matches nothing. `is:unread`/`is:read`/`is:flagged` refer to the locations in
    /// the first set if there is one.
    pub mailbox_sets: Vec<Vec<MailboxId>>,
    /// The message lies in none of these (`-in:`).
    pub excluded_mailboxes: Vec<MailboxId>,
    /// `Some(true)`: unread in some location; `Some(false)`: read everywhere.
    pub unread: Option<bool>,
    pub flagged: Option<bool>,
    pub has_attachment: Option<bool>,
    /// Only these messages (server search: what the server found). Text conditions are then
    /// ignored: the server evaluated them.
    pub only: Option<Vec<MessageId>>,
    /// Only messages whose whole body is not stored (server search).
    pub without_complete_body: bool,
    /// Stage 9: `Some(true)` waiting snoozes only (`is:snoozed`), `Some(false)` none of them.
    pub snoozed: Option<bool>,
}

/// One result of [`crate::Store::search`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredHit {
    pub item: mail_model::MessageListItem,
    /// All locations, ascending.
    pub mailbox_ids: Vec<MailboxId>,
    /// The whole message is stored locally.
    pub body_complete: bool,
    /// Decision 0039: the message's thread (none while not threaded) and its message count.
    pub thread_id: Option<i64>,
    pub thread_message_count: u32,
    /// Stage 17: the subject the hit's conversation row shows (`ThreadListItem::subject`).
    pub conversation_subject: String,
}

/// Thread and message count of a hit (decision 0039).
pub(crate) const HIT_THREAD: &str = "
    SELECT m.thread_id, (SELECT t.message_count FROM thread t WHERE t.id = m.thread_id)
    FROM message m WHERE m.id = ?1";

/// A page of [`StoredHit`], newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredHitPage {
    pub hits: Vec<StoredHit>,
    pub next_cursor: Option<ListCursor>,
}

/// SQL and parameters of the id query.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Built {
    pub(crate) sql: String,
    pub(crate) params: Vec<Value>,
}

fn ids_list(ids: impl IntoIterator<Item = i64>) -> String {
    ids.into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Per-row conditions on `m` (and its locations), in SQL, appended to `out`.
fn conditions(
    f: &SearchFilter,
    account_rows: Option<&[i64]>,
    skip_first_set: bool,
    params: &mut Vec<Value>,
    out: &mut Vec<String>,
) {
    // `+` keeps these terms off the indexes: the driver decides the order (newest first), an
    // index on `account_id` would read the account's messages and sort them.
    if let Some(rows) = account_rows {
        out.push(format!(
            "+m.account_id IN ({})",
            ids_list(rows.iter().copied())
        ));
    }
    if let Some(has) = f.has_attachment {
        out.push(format!("+m.has_attachments = {}", i64::from(has)));
    }
    let scope = f
        .mailbox_sets
        .first()
        .map(|set| {
            format!(
                " AND fl.mailbox_id IN ({})",
                ids_list(set.iter().map(|m| m.0))
            )
        })
        .unwrap_or_default();
    let location = |cond: &str, exists: bool| {
        format!(
            "{}EXISTS (SELECT 1 FROM message_location fl WHERE fl.message_id = m.id AND {cond}{scope})",
            if exists { "" } else { "NOT " }
        )
    };
    match f.unread {
        Some(true) if !skip_first_set => out.push(location("fl.seen = 0", true)),
        Some(false) => out.push(location("fl.seen = 0", false)),
        _ => {}
    }
    match f.flagged {
        Some(true) if !skip_first_set => out.push(location("fl.flagged = 1", true)),
        Some(false) => out.push(location("fl.flagged = 1", false)),
        _ => {}
    }
    for (i, set) in f.mailbox_sets.iter().enumerate() {
        if i == 0 && skip_first_set {
            continue;
        }
        out.push(format!(
            "EXISTS (SELECT 1 FROM message_location il WHERE il.message_id = m.id AND il.mailbox_id IN ({}))",
            ids_list(set.iter().map(|m| m.0))
        ));
    }
    if !f.excluded_mailboxes.is_empty() {
        out.push(format!(
            "NOT EXISTS (SELECT 1 FROM message_location xl WHERE xl.message_id = m.id AND xl.mailbox_id IN ({}))",
            ids_list(f.excluded_mailboxes.iter().map(|m| m.0))
        ));
    }
    if let Some(snoozed) = f.snoozed {
        out.push(format!(
            "{}EXISTS (SELECT 1 FROM snooze sz
                        WHERE sz.message_id_hdr = m.message_id_hdr AND sz.returned_at IS NULL
                          AND sz.account_uuid = (SELECT a.uuid FROM account a WHERE a.id = m.account_id))",
            if snoozed { "" } else { "NOT " }
        ));
    }
    if f.without_complete_body {
        out.push(
            "NOT EXISTS (SELECT 1 FROM message_body cb WHERE cb.message_id = m.id AND cb.complete = 1)"
                .into(),
        );
    }
    if f.only.is_none()
        && let TextMatch::Exclude(expr) = &f.text
    {
        params.push(Value::Text(expr.clone()));
        out.push(format!(
            "m.id NOT IN (SELECT rowid FROM message_fts WHERE message_fts MATCH ?{})",
            params.len()
        ));
    }
}

fn and(conditions: &[String]) -> String {
    conditions
        .iter()
        .map(|c| format!(" AND {c}"))
        .collect::<String>()
}

/// Builds the id query: ids newest first, at most `fetch` rows, all below `upper` and at or above
/// `lower`.
pub(crate) fn build(
    f: &SearchFilter,
    account_rows: Option<&[i64]>,
    lower: Option<i64>,
    upper: i64,
    fetch: i64,
) -> Built {
    let mut params: Vec<Value> = Vec::new();
    let push = |params: &mut Vec<Value>, v: Value| {
        params.push(v);
        params.len()
    };
    if let Some(only) = &f.only {
        // The candidates as literals: they are ids from the database.
        let mut conds = Vec::new();
        conditions(f, account_rows, false, &mut params, &mut conds);
        let n_upper = push(&mut params, Value::Integer(upper));
        let n_lower = push(&mut params, Value::Integer(lower.unwrap_or(i64::MIN)));
        let n_fetch = push(&mut params, Value::Integer(fetch));
        let sql = format!(
            "SELECT m.id FROM message m WHERE m.id IN ({}) AND m.id < ?{n_upper} AND m.id >= ?{n_lower}{}
             ORDER BY m.id DESC LIMIT ?{n_fetch}",
            ids_list(only.iter().map(|m| m.0)),
            and(&conds)
        );
        return Built { sql, params };
    }
    match &f.text {
        TextMatch::Match(expr) => {
            let n_match = push(&mut params, Value::Text(expr.clone()));
            let n_upper = push(&mut params, Value::Integer(upper));
            let mut range = format!(" AND message_fts.rowid < ?{n_upper}");
            if let Some(lower) = lower {
                let n = push(&mut params, Value::Integer(lower));
                range.push_str(&format!(" AND message_fts.rowid >= ?{n}"));
            }
            let mut conds = Vec::new();
            conditions(f, account_rows, false, &mut params, &mut conds);
            let n_fetch = push(&mut params, Value::Integer(fetch));
            let sql = format!(
                "SELECT m.id FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid
                 WHERE message_fts MATCH ?{n_match}{range}{}
                 ORDER BY message_fts.rowid DESC LIMIT ?{n_fetch}",
                and(&conds)
            );
            Built { sql, params }
        }
        _ if !f.mailbox_sets.is_empty() => {
            let n_upper = push(&mut params, Value::Integer(upper));
            let n_lower = lower.map(|l| push(&mut params, Value::Integer(l)));
            let n_fetch = push(&mut params, Value::Integer(fetch));
            let mut conds = Vec::new();
            conditions(f, account_rows, true, &mut params, &mut conds);
            let mut on_location = String::new();
            if f.unread == Some(true) {
                on_location.push_str(" AND l.seen = 0");
            }
            if f.flagged == Some(true) {
                on_location.push_str(" AND l.flagged = 1");
            }
            if let Some(n) = n_lower {
                on_location.push_str(&format!(" AND l.message_id >= ?{n}"));
            }
            let branches: Vec<String> = f.mailbox_sets[0]
                .iter()
                .map(|mailbox| {
                    format!(
                        "SELECT * FROM (SELECT l.message_id AS id
                           FROM message_location l CROSS JOIN message m ON m.id = l.message_id
                           WHERE l.mailbox_id = {mailbox} AND l.message_id < ?{n_upper}{on_location}{conds}
                           ORDER BY l.message_id DESC LIMIT ?{n_fetch})",
                        mailbox = mailbox.0,
                        conds = and(&conds),
                    )
                })
                .collect();
            let sql = if branches.len() == 1 {
                branches.into_iter().next().unwrap_or_default()
            } else {
                format!(
                    "{} ORDER BY 1 DESC LIMIT ?{n_fetch}",
                    branches.join(" UNION ALL ")
                )
            };
            Built { sql, params }
        }
        _ => {
            let n_upper = push(&mut params, Value::Integer(upper));
            let mut range = format!("m.id < ?{n_upper}");
            if let Some(lower) = lower {
                let n = push(&mut params, Value::Integer(lower));
                range.push_str(&format!(" AND m.id >= ?{n}"));
            }
            let mut conds = Vec::new();
            conditions(f, account_rows, false, &mut params, &mut conds);
            let n_fetch = push(&mut params, Value::Integer(fetch));
            let sql = format!(
                "SELECT m.id FROM message m WHERE {range}{} ORDER BY m.id DESC LIMIT ?{n_fetch}",
                and(&conds)
            );
            Built { sql, params }
        }
    }
}

/// See [`crate::Store::search`].
pub(crate) fn search(
    conn: &Connection,
    f: &SearchFilter,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<StoredHitPage, StoreError> {
    if limit == 0 || limit > MAX_SEARCH_LIMIT {
        return Err(StoreError::InvalidArgument(format!(
            "search limit {limit} is outside 1..={MAX_SEARCH_LIMIT}"
        )));
    }
    if f.dates.is_empty()
        || f.mailbox_sets.iter().any(Vec::is_empty)
        || f.accounts.as_ref().is_some_and(Vec::is_empty)
        || f.only.as_ref().is_some_and(Vec::is_empty)
    {
        return Ok(StoredHitPage::default());
    }
    let account_rows = match &f.accounts {
        Some(accounts) => {
            let mut rows = Vec::with_capacity(accounts.len());
            for account in accounts {
                match accounts::row_id(conn, *account) {
                    Ok(row) => rows.push(row),
                    Err(StoreError::NotFound(_)) => {}
                    Err(err) => return Err(err),
                }
            }
            if rows.is_empty() {
                return Ok(StoredHitPage::default());
            }
            Some(rows)
        }
        None => None,
    };
    let upper = cursor
        .map_or(i64::MAX, |c| c.before().0)
        .min(f.dates.until.unwrap_or(i64::MAX));
    let built = build(
        f,
        account_rows.as_deref(),
        f.dates.from,
        upper,
        i64::from(limit) + 1,
    );
    let mut ids: Vec<i64> = conn
        .prepare_cached(&built.sql)?
        .query_map(params_from_iter(built.params.iter()), |r| {
            r.get::<_, i64>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // Several mailbox branches can hold the same message.
    let fetched = ids.len();
    ids.dedup();
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let more = fetched > limit;
    ids.truncate(limit);
    let prefer: Vec<MailboxId> = f.mailbox_sets.first().cloned().unwrap_or_default();
    let mut hits = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(Entry {
            item,
            mailbox_ids,
            body_complete,
        }) = entry(conn, MessageId(*id), &prefer)?
        {
            let (thread_id, count): (Option<i64>, Option<i64>) =
                conn.prepare_cached(HIT_THREAD)?
                    .query_row([*id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let conversation_subject =
                crate::conversations::conversation_subject(conn, thread_id, &item.subject)?;
            hits.push(StoredHit {
                item,
                mailbox_ids,
                body_complete,
                thread_id,
                thread_message_count: u32::try_from(count.unwrap_or(1).max(1)).unwrap_or(u32::MAX),
                conversation_subject,
            });
        }
    }
    let next_cursor = if more {
        ids.last().map(|id| ListCursor::after(MessageId(*id)))
    } else {
        None
    };
    Ok(StoredHitPage { hits, next_cursor })
}
