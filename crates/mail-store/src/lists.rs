//! Lists of stage 7: the unified inbox, thread lists of a mailbox and of the unified inbox, the
//! messages of a thread, and list rows of single messages (search hits, thread entries).
//! Synchronous functions over a connection; `search_api` runs them on a reader.
//!
//! Every list is keyset-paged by message id (newest first) and fixes its join order with
//! `CROSS JOIN` (decision 0003). Views over several mailboxes are `UNION ALL` of one branch per
//! mailbox, each a range of the location key, never `mailbox_id IN (…)`; SQLite then merges at
//! most n × (limit + 1) rows. A test checks every plan (`stage7_plan_tests`).

use std::collections::HashMap;

use mail_model::{
    AccountId, Address, ListCursor, MailboxId, MessageActivity, MessageId, MessageListItem,
    MessagePage, ThreadEntry, ThreadPage, ThreadView,
};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::StoreError;
use crate::accounts::account_id_at;
use crate::messages::{addresses_at, keywords_from};

/// Outbox state of the location `l` (as in `messages::LIST_PAGE`).
const ACTIVITY: &str =
    "(SELECT max(CASE o.state WHEN 'failed' THEN 3 WHEN 'conflict' THEN 2 ELSE 1 END)
            FROM operation o
            WHERE o.mailbox_id = l.mailbox_id AND o.uid = l.uid
              AND +o.message_id = l.message_id AND o.state <> 'confirmed')";

/// Columns 0..=13 of every list row, as in `messages::LIST_PAGE`.
pub(crate) fn list_columns() -> String {
    format!(
        "l.message_id, l.seen, l.flagged, l.answered, l.draft, l.keywords,
           m.subject, m.from_name, m.from_addr, m.to_addrs, m.date, m.has_attachments, m.preview,
           {ACTIVITY}"
    )
}

/// A list row from columns 0..=13.
pub(crate) fn item_at(
    r: &Row<'_>,
    account: AccountId,
    mailbox: MailboxId,
) -> rusqlite::Result<MessageListItem> {
    let from_name: Option<String> = r.get(7)?;
    let from_addr: Option<String> = r.get(8)?;
    let date: i64 = r.get(10)?;
    Ok(MessageListItem {
        id: MessageId(r.get(0)?),
        account_id: account,
        mailbox_id: mailbox,
        from: from_addr.map(|email| Address {
            name: from_name,
            email,
        }),
        to: addresses_at(r, 9)?,
        subject: r.get(6)?,
        date: date.saturating_mul(1000),
        unread: !r.get::<_, bool>(1)?,
        flagged: r.get(2)?,
        answered: r.get(3)?,
        draft: r.get(4)?,
        has_attachments: r.get(11)?,
        keywords: keywords_from(r.get::<_, Option<String>>(5)?.as_deref()),
        preview: r.get(12)?,
        activity: activity(r.get(13)?),
        category: None,
        important: false,
        pinned: false,
        snooze: None,
        task: None,
        reading_list: false,
    })
}

fn activity(state: Option<i64>) -> Option<MessageActivity> {
    match state {
        None => None,
        Some(3) => Some(MessageActivity::Failed),
        Some(2) => Some(MessageActivity::Conflict),
        Some(_) => Some(MessageActivity::Pending),
    }
}

fn check_limit(limit: u32, max: u32) -> Result<(), StoreError> {
    if limit == 0 || limit > max {
        return Err(StoreError::InvalidArgument(format!(
            "list limit {limit} is outside 1..={max}"
        )));
    }
    Ok(())
}

/// Account of a mailbox; [`StoreError::NotFound`] if there is none.
pub(crate) fn mailbox_account(
    conn: &Connection,
    mailbox: MailboxId,
) -> Result<AccountId, StoreError> {
    conn.prepare_cached(crate::messages::MAILBOX_ACCOUNT)?
        .query_row([mailbox.0], |r| account_id_at(r, 0))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("mailbox {mailbox}")))
}

/// Cuts `rows` (newest first, one more than `limit` asked for) to a page.
pub(crate) fn page<T>(
    mut rows: Vec<T>,
    limit: u32,
    id: impl Fn(&T) -> MessageId,
) -> (Vec<T>, Option<ListCursor>) {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|r| ListCursor::after(id(r)))
    } else {
        None
    };
    (rows, next)
}

// ---------------------------------------------------------------------------------------------
// Unified inbox

/// One branch of the unified list: the newest locations of mailbox `?{param}` below the cursor
/// `?1`, at most `?2`, with the list columns and the mailbox id last.
fn unified_branch(param: usize) -> String {
    format!(
        "SELECT * FROM (SELECT {cols}, l.mailbox_id
           FROM message_location l CROSS JOIN message m ON m.id = l.message_id
           WHERE l.mailbox_id = ?{param} AND l.message_id < ?1
           ORDER BY l.message_id DESC LIMIT ?2)",
        cols = list_columns()
    )
}

/// `UNION ALL` of `branch` for `n` mailboxes (parameters `?3..`), newest first, at most `?2`.
/// One mailbox is its branch alone.
fn union_all(n: usize, branch: fn(usize) -> String) -> String {
    if n == 1 {
        return branch(3);
    }
    let branches: Vec<String> = (0..n).map(|i| branch(i + 3)).collect();
    format!("{} ORDER BY 1 DESC LIMIT ?2", branches.join(" UNION ALL "))
}

pub(crate) fn unified_sql(n: usize) -> String {
    union_all(n, unified_branch)
}

/// One page of several mailboxes mixed by time (the unified inbox). `mailboxes` with their
/// account; empty gives an empty page.
pub(crate) fn unified(
    conn: &Connection,
    mailboxes: &[(MailboxId, AccountId)],
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, StoreError> {
    check_limit(limit, crate::MAX_PAGE_SIZE)?;
    if mailboxes.is_empty() {
        return Ok(MessagePage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let accounts: HashMap<i64, AccountId> = mailboxes.iter().map(|(m, a)| (m.0, *a)).collect();
    let before = cursor.map_or(i64::MAX, |c| c.before().0);
    let mut values: Vec<i64> = vec![before, i64::from(limit) + 1];
    values.extend(mailboxes.iter().map(|(m, _)| m.0));
    let sql = unified_sql(mailboxes.len());
    let rows = conn
        .prepare_cached(&sql)?
        .query_map(params_from_iter(values), |r| {
            let mailbox = MailboxId(r.get(14)?);
            let account = accounts
                .get(&mailbox.0)
                .copied()
                .ok_or_else(|| rusqlite::Error::InvalidColumnIndex(14))?;
            item_at(r, account, mailbox)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (mut items, next_cursor) = page(rows, limit, |i| i.id);
    crate::marks::decorate(conn, &mut items)?;
    Ok(MessagePage { items, next_cursor })
}

// ---------------------------------------------------------------------------------------------
// Threads

/// One branch of a thread list: the newest message of each thread in mailbox `?{param}`,
/// below the cursor `?1`, at most `?2`. A location is the thread's row when no newer message
/// of its thread lies in the same mailbox (`message_thread` gives the thread's ids in order).
/// Unthreaded messages (thread NULL) are rows of their own. Columns: the list columns, then
/// thread id and mailbox id. Counts, senders and flags of the conversation come from
/// `conversations::summarize` (decision 0039).
fn thread_branch(param: usize) -> String {
    format!(
        "SELECT * FROM (SELECT {cols}, m.thread_id, l.mailbox_id
           FROM message_location l CROSS JOIN message m ON m.id = l.message_id
           WHERE l.mailbox_id = ?{param} AND l.message_id < ?1
             AND NOT EXISTS (SELECT 1 FROM message n CROSS JOIN message_location nl
                               ON nl.mailbox_id = l.mailbox_id AND nl.message_id = n.id
                             WHERE n.thread_id = m.thread_id AND n.id > m.id)
           ORDER BY l.message_id DESC LIMIT ?2)",
        cols = list_columns()
    )
}

pub(crate) fn threads_sql(n: usize) -> String {
    union_all(n, thread_branch)
}

/// One page of thread rows over `mailboxes` (one mailbox, or the unified inbox).
pub(crate) fn threads(
    conn: &Connection,
    mailboxes: &[(MailboxId, AccountId)],
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<ThreadPage, StoreError> {
    check_limit(limit, crate::MAX_PAGE_SIZE)?;
    if mailboxes.is_empty() {
        return Ok(ThreadPage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let accounts: HashMap<i64, AccountId> = mailboxes.iter().map(|(m, a)| (m.0, *a)).collect();
    let before = cursor.map_or(i64::MAX, |c| c.before().0);
    let mut values: Vec<i64> = vec![before, i64::from(limit) + 1];
    values.extend(mailboxes.iter().map(|(m, _)| m.0));
    let sql = threads_sql(mailboxes.len());
    let rows = conn
        .prepare_cached(&sql)?
        .query_map(params_from_iter(values), |r| {
            let mailbox = MailboxId(r.get(15)?);
            let account = accounts
                .get(&mailbox.0)
                .copied()
                .ok_or_else(|| rusqlite::Error::InvalidColumnIndex(15))?;
            Ok((item_at(r, account, mailbox)?, r.get::<_, Option<i64>>(14)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (rows, next_cursor) = page(rows, limit, |(item, _)| item.id);
    let mut items = Vec::with_capacity(rows.len());
    for (mut message, thread) in rows {
        crate::marks::decorate(conn, std::slice::from_mut(&mut message))?;
        items.push(crate::conversations::summarize(conn, message, thread)?);
    }
    Ok(ThreadPage { items, next_cursor })
}

/// The message at a UID (server search results).
pub(crate) const MESSAGE_BY_UID: &str =
    "SELECT message_id FROM message_location WHERE mailbox_id = ?1 AND uid = ?2";

pub(crate) const THREAD_OF: &str = "SELECT thread_id FROM message WHERE id = ?1";
pub(crate) const THREAD_MESSAGES: &str =
    "SELECT id FROM message WHERE thread_id = ?1 ORDER BY id DESC LIMIT ?2";

/// The messages of the thread of `message`, oldest first, at most the newest `max`.
/// `prefer` picks the location each entry shows (the mailbox the thread was opened from).
pub(crate) fn thread_view(
    conn: &Connection,
    message: MessageId,
    prefer: Option<MailboxId>,
    max: u32,
) -> Result<ThreadView, StoreError> {
    let thread: Option<i64> = conn
        .prepare_cached(THREAD_OF)?
        .query_row([message.0], |r| r.get(0))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("message {message}")))?;
    let (ids, total) = match thread {
        Some(thread) => {
            let mut ids = conn
                .prepare_cached(THREAD_MESSAGES)?
                .query_map(params![thread, i64::from(max)], |r| r.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.reverse();
            let total: i64 = conn
                .prepare_cached(crate::threads::THREAD_COUNTS)?
                .query_row([thread], |r| r.get(0))
                .optional()?
                .unwrap_or(i64::try_from(ids.len()).unwrap_or(i64::MAX));
            (ids, total)
        }
        None => (vec![message.0], 1),
    };
    let prefer: Vec<MailboxId> = prefer.into_iter().collect();
    let mut messages = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(entry) = entry(conn, MessageId(id), &prefer)? {
            messages.push(ThreadEntry {
                message: entry.item,
                mailbox_ids: entry.mailbox_ids,
            });
        }
    }
    Ok(ThreadView {
        thread_id: thread,
        messages,
        total: u32::try_from(total.max(0)).unwrap_or(u32::MAX),
    })
}

// ---------------------------------------------------------------------------------------------
// Single rows

pub(crate) const MESSAGE_ROW: &str = "
    SELECT (SELECT a.uuid FROM account a WHERE a.id = m.account_id), m.subject, m.from_name,
           m.from_addr, m.to_addrs, m.date, m.has_attachments, m.preview,
           (SELECT b.complete FROM message_body b WHERE b.message_id = m.id)
    FROM message m WHERE m.id = ?1";

pub(crate) fn locations_sql() -> String {
    format!(
        "SELECT l.mailbox_id, l.seen, l.flagged, l.answered, l.draft, l.keywords, {ACTIVITY}
         FROM message_location l WHERE l.message_id = ?1"
    )
}

/// A message as a list row of one of its locations.
pub(crate) struct Entry {
    pub(crate) item: MessageListItem,
    /// Every location, ascending.
    pub(crate) mailbox_ids: Vec<MailboxId>,
    /// The whole message is stored locally.
    pub(crate) body_complete: bool,
}

/// `message` as a list row: the location is the first one in `prefer` order, else the first.
/// `None` if the message does not exist or lies in no mailbox.
pub(crate) fn entry(
    conn: &Connection,
    message: MessageId,
    prefer: &[MailboxId],
) -> Result<Option<Entry>, StoreError> {
    struct Head {
        account: AccountId,
        subject: String,
        from: Option<Address>,
        to: Vec<Address>,
        date: i64,
        has_attachments: bool,
        preview: Option<String>,
        complete: Option<bool>,
    }
    let head = conn
        .prepare_cached(MESSAGE_ROW)?
        .query_row([message.0], |r| {
            let from_name: Option<String> = r.get(2)?;
            let from_addr: Option<String> = r.get(3)?;
            Ok(Head {
                account: account_id_at(r, 0)?,
                subject: r.get(1)?,
                from: from_addr.map(|email| Address {
                    name: from_name,
                    email,
                }),
                to: addresses_at(r, 4)?,
                date: r.get(5)?,
                has_attachments: r.get(6)?,
                preview: r.get(7)?,
                complete: r.get(8)?,
            })
        })
        .optional()?;
    let Some(head) = head else {
        return Ok(None);
    };
    struct Loc {
        mailbox: MailboxId,
        seen: bool,
        flagged: bool,
        answered: bool,
        draft: bool,
        keywords: Option<String>,
        activity: Option<i64>,
    }
    let locations = conn
        .prepare_cached(&locations_sql())?
        .query_map([message.0], |r| {
            Ok(Loc {
                mailbox: MailboxId(r.get(0)?),
                seen: r.get(1)?,
                flagged: r.get(2)?,
                answered: r.get(3)?,
                draft: r.get(4)?,
                keywords: r.get(5)?,
                activity: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let chosen = prefer
        .iter()
        .find_map(|p| locations.iter().find(|l| l.mailbox == *p))
        .or_else(|| locations.first());
    let Some(loc) = chosen else {
        return Ok(None);
    };
    let mut mailbox_ids: Vec<MailboxId> = locations.iter().map(|l| l.mailbox).collect();
    mailbox_ids.sort();
    let mut entry = Entry {
        item: MessageListItem {
            id: message,
            account_id: head.account,
            mailbox_id: loc.mailbox,
            from: head.from,
            to: head.to,
            subject: head.subject,
            date: head.date.saturating_mul(1000),
            unread: !loc.seen,
            flagged: loc.flagged,
            answered: loc.answered,
            draft: loc.draft,
            has_attachments: head.has_attachments,
            keywords: keywords_from(loc.keywords.as_deref()),
            preview: head.preview,
            activity: activity(loc.activity),
            category: None,
            important: false,
            pinned: false,
            snooze: None,
            task: None,
            reading_list: false,
        },
        mailbox_ids,
        body_complete: head.complete == Some(true),
    };
    crate::marks::decorate(conn, std::slice::from_mut(&mut entry.item))?;
    Ok(Some(entry))
}
