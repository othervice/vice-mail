//! Stage 19, task A (decisions 0061 and 0062): a list view in another order than newest first,
//! or with the quick filter: the messages or conversations of one mailbox or of a unified view,
//! sorted by date, sender, recipient, subject or size, filtered by the search's conditions and
//! the quick filter's own (tag, sender among the contacts).
//!
//! **Driver.** Like every list (decision 0003): per mailbox a range of one index of
//! `message_location`, in index order, at most `limit + 1` rows, `CROSS JOIN message` for the
//! join order (`NOT INDEXED`: the message is a rowid lookup, never a scan of an index on it);
//! several mailboxes are `UNION ALL` of one branch each, merged by SQLite. Date order walks the
//! location key, `loc_unseen` or `loc_flagged`, or with a text filter the matches of the
//! full-text index in id order (one probe of `loc_message` each), as the planner sees fit; the
//! other orders walk `loc_sort_<key>` (`INDEXED BY`, migration 0026). The cursor is a row-value
//! range of that index, `(key, message_id)`, so equal keys sort by date in the same direction
//! and a page never depends on how many rows came before it.
//!
//! **Filters** are checked per row: flags and tag on the view's location `l`, the search's
//! conditions on `m` (`search::conditions_on`), the text as `l.message_id IN (… MATCH ?)`
//! (SQLite fills the matches once per statement). A rare filter in a large mailbox therefore
//! walks much of the index; the counts in `docs/testing/performance.md` (stage 19) say how much.
//!
//! **Inbox rules.** Inboxes and unified views leave out waiting snoozes and mail of blocked
//! senders, like their filtered lists (`conversations::hidden`); pinned and returned messages
//! stay in their place (no block on top in another order).
//!
//! **Conversations** (decision 0039): a location is its thread's row when no newer message of
//! the thread lies in the same mailbox *and matches the same filter*; the row takes its place in
//! the order by its own key. Every matching message thus belongs to exactly one row.

use std::collections::HashMap;

use mail_model::search::TextMatch;
use mail_model::{
    AccountId, ListOrder, MailboxId, MessageListItem, SortKey, ViewCursor, ViewMessagePage,
    ViewThreadPage,
};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::accounts;
use crate::comfort_api::{InboxMailboxes, resolve};
use crate::inbox::InboxListKind;
use crate::lists::{LIST_COLUMNS, item_at, list_columns};
use crate::search::{SearchFilter, conditions_on};
use crate::{Store, StoreError};

/// What a view's page is asked for, resolved (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewFilter {
    /// The compiled search conditions. `unread` and `flagged` refer to the view's location;
    /// `mailbox_sets` are the `in:` sets the user typed (the view's own mailboxes are not in it).
    pub search: SearchFilter,
    /// The sender is a contact (decision 0062).
    pub contacts: bool,
    /// A keyword of the view's location, compared without ASCII case.
    pub keyword: Option<String>,
}

/// The column, index and value type of an order.
struct OrderSql {
    /// The key column on `l`; `None` for date order (the location key).
    column: Option<&'static str>,
    index: Option<&'static str>,
    numeric: bool,
    direction: &'static str,
    compare: &'static str,
}

fn order_sql(order: ListOrder) -> OrderSql {
    let (column, index, numeric) = match order.key {
        SortKey::Date => (None, None, false),
        SortKey::Sender => (Some("l.sort_sender"), Some("loc_sort_sender"), false),
        SortKey::Recipient => (Some("l.sort_recipient"), Some("loc_sort_recipient"), false),
        SortKey::Subject => (Some("l.sort_subject"), Some("loc_sort_subject"), false),
        SortKey::Size => (Some("l.size"), Some("loc_sort_size"), true),
    };
    OrderSql {
        column,
        index,
        numeric,
        direction: if order.ascending { "ASC" } else { "DESC" },
        compare: if order.ascending { ">" } else { "<" },
    }
}

/// SQL parameters by number.
#[derive(Default)]
struct Params(Vec<Value>);

impl Params {
    fn push(&mut self, value: impl Into<Value>) -> usize {
        self.0.push(value.into());
        self.0.len()
    }
}

/// Parameters every branch shares.
struct Shared {
    /// `+` before the location's message id in the filter's terms, so they cannot drive the
    /// walk: the key orders must walk their own index (`INDEXED BY` alone still lets SQLite
    /// skip-scan it by the text's matches and sort them).
    pin: &'static str,
    /// The cursor: key (if any) and message id.
    cursor: Option<(Option<usize>, usize)>,
    fetch: usize,
    /// The text match, once for all branches.
    text: Option<usize>,
    keyword: Option<usize>,
}

/// The view's conditions on location `l` and message `m` (in SQL, each starting with ` AND `).
fn row_conditions(
    filter: &ViewFilter,
    l: &str,
    m: &str,
    account_rows: Option<&[i64]>,
    shared: &Shared,
    params: &mut Params,
) -> String {
    let f = &filter.search;
    let mut out: Vec<String> = Vec::new();
    match f.unread {
        Some(true) => out.push(format!("{l}.seen = 0")),
        Some(false) => out.push(format!("{l}.seen = 1")),
        None => {}
    }
    match f.flagged {
        Some(true) => out.push(format!("{l}.flagged = 1")),
        Some(false) => out.push(format!("{l}.flagged = 0")),
        None => {}
    }
    if let Some(from) = f.dates.from {
        let n = params.push(from);
        out.push(format!("{pin}{l}.message_id >= ?{n}", pin = shared.pin));
    }
    if let Some(until) = f.dates.until {
        let n = params.push(until);
        out.push(format!("{pin}{l}.message_id < ?{n}", pin = shared.pin));
    }
    if let Some(n) = shared.keyword {
        out.push(format!(
            "instr(' ' || lower(coalesce({l}.keywords, '')) || ' ', ' ' || ?{n} || ' ') > 0"
        ));
    }
    if let Some(n) = shared.text {
        out.push(format!(
            "{pin}{l}.message_id IN (SELECT rowid FROM message_fts WHERE message_fts MATCH ?{n})",
            pin = shared.pin
        ));
    }
    // The search's own conditions on the message; flags are on the location above.
    let rest = SearchFilter {
        unread: None,
        flagged: None,
        ..f.clone()
    };
    conditions_on(&rest, m, account_rows, false, &mut params.0, &mut out);
    if filter.contacts {
        out.push(format!(
            "EXISTS (SELECT 1 FROM contact c WHERE c.address_key = {m}.sender_key
                       AND (c.sent > 0 OR c.user_name IS NOT NULL OR c.note IS NOT NULL
                            OR c.contact_group IS NOT NULL))"
        ));
    }
    out.iter().map(|c| format!(" AND {c}")).collect()
}

/// One branch: mailbox `?{mailbox}` (account UUID `?{account}` for the inbox rules). Columns: the
/// list columns (0..=14), the mailbox id (15), the sort value (16) and, for conversations, the
/// thread id (17).
#[allow(clippy::too_many_arguments)]
fn branch(
    order: &OrderSql,
    filter: &ViewFilter,
    inbox_rules: bool,
    conversations: bool,
    mailbox: usize,
    account: usize,
    account_rows: Option<&[i64]>,
    shared: &Shared,
    params: &mut Params,
) -> String {
    let key = order.column.unwrap_or("l.message_id");
    let value = order.column.unwrap_or("NULL");
    let index = order
        .index
        .map(|i| format!("INDEXED BY {i}"))
        .unwrap_or_default();
    let cursor = match shared.cursor {
        None => String::new(),
        Some((Some(k), id)) => format!("AND ({key}, l.message_id) {} (?{k}, ?{id})", order.compare),
        Some((None, id)) => format!("AND l.message_id {} ?{id}", order.compare),
    };
    let here = row_conditions(filter, "l", "m", account_rows, shared, params);
    let rules = |m: &str, x: &str| {
        if inbox_rules {
            crate::conversations::hidden(InboxListKind::Unread, m, account, x)
        } else {
            String::new()
        }
    };
    let (thread, newest) = if conversations {
        let there = row_conditions(filter, "nl", "n", account_rows, shared, params);
        (
            ", m.thread_id",
            format!(
                "AND NOT EXISTS (SELECT 1 FROM message n CROSS JOIN message_location nl
                                   ON nl.mailbox_id = l.mailbox_id AND nl.message_id = n.id
                                 WHERE n.thread_id = m.thread_id AND n.id > m.id{there}
                                   {rules})",
                rules = rules("n", "n")
            ),
        )
    } else {
        ("", String::new())
    };
    let by = if order.column.is_some() {
        format!("{key} {dir}, l.message_id {dir}", dir = order.direction)
    } else {
        format!("l.message_id {}", order.direction)
    };
    format!(
        "SELECT * FROM (SELECT {cols}, l.mailbox_id, {value}{thread}
           FROM message_location l {index} CROSS JOIN message m NOT INDEXED ON m.id = l.message_id
           WHERE l.mailbox_id = ?{mailbox} {cursor}{here}
             {rules}
             {newest}
           ORDER BY {by} LIMIT ?{fetch})",
        cols = list_columns(),
        rules = rules("m", ""),
        fetch = shared.fetch,
    )
}

/// The whole statement over `mailboxes` and its parameters.
pub(crate) fn statement(
    view: &ResolvedView<'_>,
    conversations: bool,
    cursor: Option<&ViewCursor>,
    limit: u32,
) -> Result<(String, Vec<Value>), StoreError> {
    let order = order_sql(view.order);
    let mut params = Params::default();
    let cursor = match cursor {
        None => None,
        Some(c) => {
            let key = match (order.column, order.numeric, &c.text, c.number) {
                (None, _, _, _) => None,
                (Some(_), false, Some(text), _) => Some(params.push(text.clone())),
                (Some(_), true, _, Some(number)) => Some(params.push(number)),
                _ => {
                    return Err(StoreError::InvalidArgument(
                        "the cursor does not belong to this order".into(),
                    ));
                }
            };
            Some((key, params.push(c.id.0)))
        }
    };
    let fetch = params.push(i64::from(limit) + 1);
    let text = match &view.filter.search.text {
        TextMatch::Match(expr) if view.filter.search.only.is_none() => {
            Some(params.push(expr.clone()))
        }
        _ => None,
    };
    let keyword = view
        .filter
        .keyword
        .as_ref()
        .map(|k| params.push(k.to_ascii_lowercase()));
    let shared = Shared {
        pin: if order.column.is_some() { "+" } else { "" },
        cursor,
        fetch,
        text,
        keyword,
    };
    let mut branches = Vec::with_capacity(view.mailboxes.len());
    for (mailbox, account) in view.mailboxes {
        let m = params.push(mailbox.0);
        // Bound only where the inbox rules name it: SQLite counts the parameters a statement uses.
        let a = if view.inbox_rules {
            params.push(account.to_string())
        } else {
            0
        };
        branches.push(branch(
            &order,
            view.filter,
            view.inbox_rules,
            conversations,
            m,
            a,
            view.account_rows.as_deref(),
            &shared,
            &mut params,
        ));
    }
    let sql = if branches.len() == 1 {
        branches.remove(0)
    } else {
        let by = if order.column.is_some() {
            // 1-based: the sort value follows the list columns and the mailbox.
            format!("{} {dir}, 1 {dir}", LIST_COLUMNS + 2, dir = order.direction)
        } else {
            format!("1 {}", order.direction)
        };
        format!(
            "{} ORDER BY {by} LIMIT ?{fetch}",
            branches.join(" UNION ALL ")
        )
    };
    Ok((sql, params.0))
}

/// A view with its mailboxes and account rows resolved.
pub(crate) struct ResolvedView<'a> {
    pub(crate) mailboxes: &'a [(MailboxId, AccountId)],
    pub(crate) inbox_rules: bool,
    pub(crate) order: ListOrder,
    pub(crate) filter: &'a ViewFilter,
    /// Rows of `filter.search.accounts`; `None` for every account.
    pub(crate) account_rows: Option<Vec<i64>>,
}

/// Account rows of `filter.search.accounts`; `Some(empty)` if none of them exists.
pub(crate) fn account_rows(
    conn: &Connection,
    filter: &ViewFilter,
) -> Result<Option<Vec<i64>>, StoreError> {
    let Some(accounts) = &filter.search.accounts else {
        return Ok(None);
    };
    let mut rows = Vec::with_capacity(accounts.len());
    for account in accounts {
        match accounts::row_id(conn, *account) {
            Ok(row) => rows.push(row),
            Err(StoreError::NotFound(_)) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(Some(rows))
}

/// Nothing can match: an empty set of accounts, mailboxes or messages, an empty date range.
fn matches_nothing(view: &ResolvedView<'_>) -> bool {
    let f = &view.filter.search;
    view.mailboxes.is_empty()
        || f.dates.is_empty()
        || f.mailbox_sets.iter().any(Vec::is_empty)
        || f.only.is_some()
        || view.account_rows.as_ref().is_some_and(Vec::is_empty)
}

fn check_limit(limit: u32) -> Result<(), StoreError> {
    if limit == 0 || limit > crate::MAX_PAGE_SIZE {
        return Err(StoreError::InvalidArgument(format!(
            "list limit {limit} is outside 1..={}",
            crate::MAX_PAGE_SIZE
        )));
    }
    Ok(())
}

/// A row as read: the item, its sort value, its thread.
struct Read {
    item: MessageListItem,
    text: Option<String>,
    number: Option<i64>,
    thread: Option<i64>,
}

fn read_rows(
    conn: &Connection,
    view: &ResolvedView<'_>,
    conversations: bool,
    cursor: Option<&ViewCursor>,
    limit: u32,
) -> Result<(Vec<Read>, Option<ViewCursor>), StoreError> {
    check_limit(limit)?;
    if matches_nothing(view) {
        return Ok((Vec::new(), None));
    }
    let accounts: HashMap<i64, AccountId> = view.mailboxes.iter().map(|(m, a)| (m.0, *a)).collect();
    let numeric = order_sql(view.order).numeric;
    let keyed = order_sql(view.order).column.is_some();
    let (sql, params) = statement(view, conversations, cursor, limit)?;
    let mut rows = conn
        .prepare_cached(&sql)?
        .query_map(params_from_iter(params.iter()), |r| {
            let mailbox = MailboxId(r.get(LIST_COLUMNS)?);
            let account = accounts
                .get(&mailbox.0)
                .copied()
                .ok_or(rusqlite::Error::InvalidColumnIndex(LIST_COLUMNS))?;
            let (text, number) = match (keyed, numeric) {
                (false, _) => (None, None),
                (true, true) => (None, Some(r.get::<_, i64>(LIST_COLUMNS + 1)?)),
                (true, false) => (Some(r.get::<_, String>(LIST_COLUMNS + 1)?), None),
            };
            Ok(Read {
                item: item_at(r, account, mailbox)?,
                text,
                number,
                thread: if conversations {
                    r.get(LIST_COLUMNS + 2)?
                } else {
                    None
                },
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|r| ViewCursor {
            text: r.text.clone(),
            number: r.number,
            id: r.item.id,
        })
    } else {
        None
    };
    Ok((rows, next))
}

/// One page of the view, one row per message.
pub(crate) fn messages(
    conn: &Connection,
    view: &ResolvedView<'_>,
    cursor: Option<&ViewCursor>,
    limit: u32,
) -> Result<ViewMessagePage, StoreError> {
    let (rows, next_cursor) = read_rows(conn, view, false, cursor, limit)?;
    let mut items: Vec<MessageListItem> = rows.into_iter().map(|r| r.item).collect();
    crate::marks::decorate(conn, &mut items)?;
    Ok(ViewMessagePage { items, next_cursor })
}

/// One page of the view, one row per conversation; `limit` counts conversations.
pub(crate) fn conversations(
    conn: &Connection,
    view: &ResolvedView<'_>,
    cursor: Option<&ViewCursor>,
    limit: u32,
) -> Result<ViewThreadPage, StoreError> {
    let (rows, next_cursor) = read_rows(conn, view, true, cursor, limit)?;
    let mut items = Vec::with_capacity(rows.len());
    for Read {
        mut item, thread, ..
    } in rows
    {
        crate::marks::decorate(conn, std::slice::from_mut(&mut item))?;
        items.push(crate::conversations::summarize(conn, item, thread)?);
    }
    Ok(ViewThreadPage { items, next_cursor })
}

/// The statement that counts the matching messages of one mailbox (`?1`, account UUID `?2`).
fn count_sql(view: &ResolvedView<'_>, params: &mut Params) -> String {
    let text = match &view.filter.search.text {
        TextMatch::Match(expr) if view.filter.search.only.is_none() => {
            Some(params.push(expr.clone()))
        }
        _ => None,
    };
    let keyword = view
        .filter
        .keyword
        .as_ref()
        .map(|k| params.push(k.to_ascii_lowercase()));
    let shared = Shared {
        pin: "",
        cursor: None,
        fetch: 0,
        text,
        keyword,
    };
    let here = row_conditions(
        view.filter,
        "l",
        "m",
        view.account_rows.as_deref(),
        &shared,
        params,
    );
    let rules = if view.inbox_rules {
        crate::conversations::hidden(InboxListKind::Unread, "m", 2, "")
    } else {
        String::new()
    };
    format!(
        "SELECT count(*) FROM message_location l CROSS JOIN message m NOT INDEXED
           ON m.id = l.message_id
         WHERE l.mailbox_id = ?1{here} {rules}"
    )
}

/// How many messages of the view match (every message once per mailbox it lies in).
pub(crate) fn count(conn: &Connection, view: &ResolvedView<'_>) -> Result<u64, StoreError> {
    if matches_nothing(view) {
        return Ok(0);
    }
    let mut params = Params::default();
    // `?1` and `?2` are the mailbox and its account, set per mailbox; `?2` only with the inbox
    // rules, which name it (`count_sql` reads the account as `?2` then).
    params.push(0_i64);
    if view.inbox_rules {
        params.push(String::new());
    }
    let sql = count_sql(view, &mut params);
    let mut statement = conn.prepare_cached(&sql)?;
    let mut total = 0u64;
    for (mailbox, account) in view.mailboxes {
        params.0[0] = Value::Integer(mailbox.0);
        if view.inbox_rules {
            params.0[1] = Value::Text(account.to_string());
        }
        let n: i64 = statement.query_row(params_from_iter(params.0.iter()), |r| r.get(0))?;
        total += u64::try_from(n).unwrap_or(0);
    }
    Ok(total)
}

/// The tags (keywords not starting with `$`) of the locations in `mailboxes`, each once
/// (compared without ASCII case, spelled as first found), sorted, at most `max`.
pub(crate) fn tags(
    conn: &Connection,
    mailboxes: &[(MailboxId, AccountId)],
    max: usize,
) -> Result<Vec<String>, StoreError> {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut statement = conn.prepare_cached(
        "SELECT DISTINCT keywords FROM message_location
         WHERE mailbox_id = ?1 AND keywords IS NOT NULL",
    )?;
    for (mailbox, _) in mailboxes {
        let rows = statement.query_map([mailbox.0], |r| r.get::<_, String>(0))?;
        for keywords in rows {
            for keyword in keywords?.split_whitespace() {
                if keyword.starts_with('$') || seen.len() >= max {
                    continue;
                }
                seen.entry(keyword.to_ascii_lowercase())
                    .or_insert_with(|| keyword.to_owned());
            }
        }
    }
    let mut tags: Vec<String> = seen.into_values().collect();
    tags.sort_by_key(|t| t.to_ascii_lowercase());
    Ok(tags)
}

/// Whether the inbox rules apply to a mailbox: it is an inbox.
pub(crate) fn is_inbox(conn: &Connection, mailbox: MailboxId) -> Result<bool, StoreError> {
    let role: Option<String> = conn
        .prepare_cached("SELECT role FROM mailbox WHERE id = ?1")?
        .query_row([mailbox.0], |r| r.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                StoreError::NotFound(format!("mailbox {mailbox}"))
            }
            other => other.into(),
        })?;
    Ok(role.as_deref() == Some("inbox"))
}

/// A list view as the engine asks for it: the scope, the order and the resolved filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewQuery {
    pub scope: InboxMailboxes,
    pub order: ListOrder,
    pub filter: ViewFilter,
}

/// Runs `f` on the resolved view of `query`.
fn with_view<T>(
    conn: &Connection,
    query: &ViewQuery,
    f: impl FnOnce(&ResolvedView<'_>) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let mailboxes = resolve(conn, &query.scope)?;
    let inbox_rules = match &query.scope {
        InboxMailboxes::Unified(_) => true,
        InboxMailboxes::Mailbox(mailbox) => is_inbox(conn, *mailbox)?,
    };
    let view = ResolvedView {
        mailboxes: &mailboxes,
        inbox_rules,
        order: query.order,
        filter: &query.filter,
        account_rows: account_rows(conn, &query.filter)?,
    };
    f(&view)
}

impl Store {
    /// Stage 19: one page of a sorted or filtered list view, one row per message (see
    /// `views`). `limit` 1..=[`crate::MAX_PAGE_SIZE`].
    pub async fn view_messages(
        &self,
        query: ViewQuery,
        cursor: Option<ViewCursor>,
        limit: u32,
    ) -> Result<ViewMessagePage, StoreError> {
        self.read(move |c| with_view(c, &query, |v| messages(c, v, cursor.as_ref(), limit)))
            .await
    }

    /// Stage 19: one page of a sorted or filtered list view, one row per conversation.
    pub async fn view_conversations(
        &self,
        query: ViewQuery,
        cursor: Option<ViewCursor>,
        limit: u32,
    ) -> Result<ViewThreadPage, StoreError> {
        self.read(move |c| with_view(c, &query, |v| conversations(c, v, cursor.as_ref(), limit)))
            .await
    }

    /// Stage 19: how many messages of the view match its filter (the order does not matter).
    pub async fn view_count(&self, query: ViewQuery) -> Result<u64, StoreError> {
        self.read(move |c| with_view(c, &query, |v| count(c, v)))
            .await
    }

    /// Stage 19: the tags in the view's mailboxes, at most `max` (see [`tags`]).
    pub async fn view_tags(
        &self,
        scope: InboxMailboxes,
        max: usize,
    ) -> Result<Vec<String>, StoreError> {
        self.read(move |c| {
            let mailboxes = resolve(c, &scope)?;
            tags(c, &mailboxes, max)
        })
        .await
    }
}
