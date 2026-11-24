//! Decision 0039: the sorted inbox as conversations, the summary of a conversation row, and the
//! messages an action on conversation rows touches.
//!
//! **Rows.** The conversation variant of an inbox list (`inbox::InboxListKind`) is the message
//! list of that kind grouped by thread, one row per thread at its newest member: a location is
//! the thread's row when no newer message of its thread lies in the same mailbox *and would be a
//! row of the same list* (same category, unread, not snoozed, not hidden, …). Every message of
//! the message list therefore belongs to exactly one conversation row, and keyset paging by the
//! row's message id has neither duplicates nor gaps: the representative of a thread does not
//! depend on the cursor.
//!
//! - Pinned conversations and conversations with a returned snooze stand on top: the rows leave
//!   out every thread with a pinned or returned member (one probe per thread member of the
//!   unique indexes of `pin` and `snooze`), the top lists them once per thread.
//! - Waiting snoozes and mail of blocked senders hide messages, as in the message list: a reply
//!   to a snoozed conversation brings it back with the reply as its row.
//! - "Unread first" as conversations: the unread list grouped, then the read list grouped
//!   without the threads that have an unread member in the mailbox.
//!
//! Like every list: keyset by message id, `CROSS JOIN` for the join order, one `UNION ALL`
//! branch per mailbox, a plan test for each query (`conversation_tests`). Threads stay within
//! one account (each branch is one mailbox, `thread` rows belong to one account).

use std::collections::{HashMap, HashSet};

use mail_model::{
    AccountId, Address, ConversationReach, ListCursor, MAX_ACTION_MESSAGES,
    MAX_CONVERSATION_PARTICIPANTS, MailboxId, MessageId, MessageListItem, MessageRef,
    ThreadListItem, ThreadPage, UnreadFirstCursor, UnreadFirstThreadPage, UnreadPhase,
};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::StoreError;
use crate::inbox::InboxListKind;
use crate::lists::{entry, item_at, list_columns, page};

/// The location condition of `kind` besides mailbox and cursor, on location alias `l`.
fn location_condition(kind: InboxListKind, l: &str) -> String {
    match kind {
        InboxListKind::Rows => String::new(),
        InboxListKind::Category(_) => format!("AND {l}.category = ?3"),
        InboxListKind::Important => format!("AND {l}.important = 1"),
        InboxListKind::Unread => format!("AND {l}.seen = 0"),
        InboxListKind::Read => format!("AND {l}.seen = 1"),
    }
}

/// What hides a message `m` from a list of `kind` (account UUID `?{a}`); `x` keeps the aliases
/// of nested checks apart.
pub(crate) fn hidden(kind: InboxListKind, m: &str, a: usize, x: &str) -> String {
    let marks = match kind {
        InboxListKind::Rows => format!(
            "AND NOT EXISTS (SELECT 1 FROM snooze s{x}
                             WHERE s{x}.account_uuid = ?{a} AND s{x}.message_id_hdr = {m}.message_id_hdr)
             AND NOT EXISTS (SELECT 1 FROM pin p{x}
                             WHERE p{x}.account_uuid = ?{a} AND p{x}.message_id_hdr = {m}.message_id_hdr)"
        ),
        _ => format!(
            "AND NOT EXISTS (SELECT 1 FROM snooze s{x}
                             WHERE s{x}.account_uuid = ?{a} AND s{x}.message_id_hdr = {m}.message_id_hdr
                               AND s{x}.returned_at IS NULL)"
        ),
    };
    format!(
        "{marks}
             AND NOT EXISTS (SELECT 1 FROM blocked_hidden h{x}
                             WHERE h{x}.account_uuid = ?{a} AND h{x}.message_id_hdr = {m}.message_id_hdr)"
    )
}

/// One branch: mailbox `?{p}`, its account UUID `?{p + 1}`; cursor `?1`, limit `?2`, category
/// code `?3`. Columns: the list columns (0..=13), the mailbox id (14), the thread id (15).
fn branch(kind: InboxListKind, p: usize) -> String {
    let a = p + 1;
    // `INDEXED BY` as in `inbox`: without statistics the planner would walk the key and filter.
    let index = match kind {
        InboxListKind::Rows | InboxListKind::Read => "",
        InboxListKind::Category(_) => "INDEXED BY loc_category",
        InboxListKind::Important => "INDEXED BY loc_important",
        InboxListKind::Unread => "INDEXED BY loc_unseen",
    };
    let on_top = match kind {
        // Pinned conversations and those with a returned snooze stand on top, once.
        InboxListKind::Rows => format!(
            "AND NOT EXISTS (SELECT 1 FROM message t CROSS JOIN pin tp
                               ON tp.account_uuid = ?{a} AND tp.message_id_hdr = t.message_id_hdr
                             WHERE t.thread_id = m.thread_id)
             AND NOT EXISTS (SELECT 1 FROM message r CROSS JOIN snooze rs
                               ON rs.account_uuid = ?{a} AND rs.message_id_hdr = r.message_id_hdr
                             WHERE r.thread_id = m.thread_id AND rs.returned_at IS NOT NULL)"
        ),
        // "Unread first", second part: conversations the first part did not have.
        InboxListKind::Read => format!(
            "AND NOT EXISTS (SELECT 1 FROM message u CROSS JOIN message_location ul
                               ON ul.mailbox_id = l.mailbox_id AND ul.message_id = u.id
                             WHERE u.thread_id = m.thread_id AND ul.seen = 0
                               {hidden})",
            hidden = hidden(InboxListKind::Unread, "u", a, "u")
        ),
        _ => String::new(),
    };
    format!(
        "SELECT * FROM (SELECT {cols}, l.mailbox_id, m.thread_id
           FROM message_location l {index} CROSS JOIN message m ON m.id = l.message_id
           WHERE l.mailbox_id = ?{p} {here} AND l.message_id < ?1
             {hidden}
             AND NOT EXISTS (SELECT 1 FROM message n CROSS JOIN message_location nl
                               ON nl.mailbox_id = l.mailbox_id AND nl.message_id = n.id
                             WHERE n.thread_id = m.thread_id AND n.id > m.id {there}
                               {newer_hidden})
             {on_top}
           ORDER BY l.message_id DESC LIMIT ?2)",
        cols = list_columns(),
        here = location_condition(kind, "l"),
        there = location_condition(kind, "nl"),
        hidden = hidden(kind, "m", a, ""),
        newer_hidden = hidden(kind, "n", a, "n"),
    )
}

/// The conversation variant of `kind` over `n` mailboxes.
pub(crate) fn conversations_sql(kind: InboxListKind, n: usize) -> String {
    let branches: Vec<String> = (0..n).map(|i| branch(kind, 4 + 2 * i)).collect();
    if n == 1 {
        return branches.into_iter().next().unwrap_or_default();
    }
    format!("{} ORDER BY 1 DESC LIMIT ?2", branches.join(" UNION ALL "))
}

/// One page of the conversation variant of an inbox list over `mailboxes` (with their accounts).
/// `limit` counts conversations.
pub(crate) fn list(
    conn: &Connection,
    kind: InboxListKind,
    mailboxes: &[(MailboxId, AccountId)],
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<ThreadPage, StoreError> {
    if limit == 0 || limit > crate::MAX_PAGE_SIZE {
        return Err(StoreError::InvalidArgument(format!(
            "list limit {limit} is outside 1..={}",
            crate::MAX_PAGE_SIZE
        )));
    }
    if mailboxes.is_empty() {
        return Ok(ThreadPage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let accounts: HashMap<i64, AccountId> = mailboxes.iter().map(|(m, a)| (m.0, *a)).collect();
    let category = match kind {
        InboxListKind::Category(c) => c.code(),
        _ => -1,
    };
    let mut values: Vec<rusqlite::types::Value> = vec![
        cursor.map_or(i64::MAX, |c| c.before().0).into(),
        (i64::from(limit) + 1).into(),
        category.into(),
    ];
    for (mailbox, account) in mailboxes {
        values.push(mailbox.0.into());
        values.push(account.to_string().into());
    }
    let rows = conn
        .prepare_cached(&conversations_sql(kind, mailboxes.len()))?
        .query_map(params_from_iter(values), |r| {
            let mailbox = MailboxId(r.get(14)?);
            let account = accounts
                .get(&mailbox.0)
                .copied()
                .ok_or(rusqlite::Error::InvalidColumnIndex(14))?;
            Ok((item_at(r, account, mailbox)?, r.get::<_, Option<i64>>(15)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (rows, next_cursor) = page(rows, limit, |(item, _)| item.id);
    let mut items = Vec::with_capacity(rows.len());
    for (mut message, thread) in rows {
        crate::marks::decorate(conn, std::slice::from_mut(&mut message))?;
        items.push(summarize(conn, message, thread)?);
    }
    Ok(ThreadPage { items, next_cursor })
}

// ---------------------------------------------------------------------------------------------
// The summary of a row

/// Most messages of a thread whose senders, attachments and flags a row sums up: the newest
/// ones. Count and unread messages are exact; a list thread of thousands of messages would
/// otherwise cost a row lookup per message (18 ms for 7,620 messages, decision 0039).
pub(crate) const SUMMARY_MESSAGES: i64 = 200;

/// Size, unread messages (unread in at least one mailbox) and newest message of a thread: the
/// thread's ids from `message_thread` and one probe of `loc_message` each (as in decision 0017).
/// `INDEXED BY`: with no unread location at all, `PRAGMA optimize` leaves the planner thinking
/// that scanning an empty partial index is cheaper (decision 0029).
pub(crate) const THREAD_COUNTS: &str = "
    SELECT count(*),
           coalesce(sum(EXISTS (SELECT 1 FROM message_location u INDEXED BY loc_message
                                WHERE u.message_id = m.id AND u.seen = 0)), 0),
           max(m.id)
    FROM message m WHERE m.thread_id = ?1";

/// The newest [`SUMMARY_MESSAGES`] messages of a thread, newest first: sender, attachments,
/// flagged anywhere.
pub(crate) const THREAD_RECENT: &str = "
    SELECT m.from_name, m.from_addr, m.has_attachments,
           EXISTS (SELECT 1 FROM message_location f INDEXED BY loc_message
                   WHERE f.message_id = m.id AND f.flagged = 1)
    FROM message m WHERE m.thread_id = ?1 ORDER BY m.id DESC LIMIT ?2";

/// Stage 17 (decision 0049): the subject of a thread's oldest message, which its row shows
/// without reply and forward prefixes. One probe of `message_thread` (it holds the rowid, which
/// is the id).
pub(crate) const THREAD_ROOT_SUBJECT: &str = "
    SELECT subject FROM message INDEXED BY message_thread WHERE thread_id = ?1
    ORDER BY id LIMIT 1";

/// The subject a conversation row shows (stage 17): its thread's oldest message's, else the
/// message's own, without reply and forward prefixes.
pub(crate) fn conversation_subject(
    conn: &Connection,
    thread: Option<i64>,
    own: &str,
) -> Result<String, StoreError> {
    let root: Option<String> = match thread {
        Some(thread) => conn
            .prepare_cached(THREAD_ROOT_SUBJECT)?
            .query_row([thread], |r| r.get(0))
            .optional()?,
        None => None,
    };
    Ok(mail_model::threading::display_subject(root.as_deref().unwrap_or(own)).to_owned())
}

/// The same for a message without a thread (not threaded yet).
pub(crate) const MESSAGE_SUMMARY: &str = "
    SELECT m.from_name, m.from_addr, m.has_attachments,
           EXISTS (SELECT 1 FROM message_location f INDEXED BY loc_message
                   WHERE f.message_id = m.id AND f.flagged = 1),
           EXISTS (SELECT 1 FROM message_location u INDEXED BY loc_message
                   WHERE u.message_id = m.id AND u.seen = 0)
    FROM message m WHERE m.id = ?1";

/// `message` as the row of its conversation `thread`: count and unread messages (unread in at
/// least one mailbox) over every message of the thread in every mailbox of the account; senders
/// (oldest first), attachments and flags over its newest [`SUMMARY_MESSAGES`]. The cost grows
/// with the thread's size (decision 0017: counted when read, not kept).
pub(crate) fn summarize(
    conn: &Connection,
    message: MessageListItem,
    thread: Option<i64>,
) -> Result<ThreadListItem, StoreError> {
    struct Recent {
        from_name: Option<String>,
        from_addr: Option<String>,
        has_attachments: bool,
        flagged: bool,
    }
    let recent = |r: &rusqlite::Row<'_>| {
        Ok(Recent {
            from_name: r.get(0)?,
            from_addr: r.get(1)?,
            has_attachments: r.get(2)?,
            flagged: r.get(3)?,
        })
    };
    let (count, unread, last, mut recent) = match thread {
        Some(thread) => {
            let (count, unread, last): (i64, i64, Option<i64>) = conn
                .prepare_cached(THREAD_COUNTS)?
                .query_row([thread], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            let recent = conn
                .prepare_cached(THREAD_RECENT)?
                .query_map(params![thread, SUMMARY_MESSAGES], recent)?
                .collect::<Result<Vec<_>, _>>()?;
            (count, unread, last.unwrap_or(message.id.0), recent)
        }
        None => {
            let mut unread = 0;
            let recent = conn
                .prepare_cached(MESSAGE_SUMMARY)?
                .query_map([message.id.0], |r| {
                    unread = i64::from(r.get::<_, bool>(4)?);
                    recent(r)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            (1, unread, message.id.0, recent)
        }
    };
    // Oldest first.
    recent.reverse();
    let mut participants: Vec<Address> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut has_attachments = false;
    let mut flagged = false;
    for entry in recent {
        has_attachments |= entry.has_attachments;
        flagged |= entry.flagged;
        if let Some(email) = entry.from_addr
            && participants.len() < MAX_CONVERSATION_PARTICIPANTS
            && seen.insert(email.to_lowercase())
        {
            participants.push(Address {
                name: entry.from_name,
                email,
            });
        }
    }
    let count = u32::try_from(count.max(1)).unwrap_or(u32::MAX);
    let unread = u32::try_from(unread.max(0)).unwrap_or(u32::MAX);
    let last = last.max(message.id.0);
    let subject = conversation_subject(conn, thread, &message.subject)?;
    Ok(ThreadListItem {
        thread_id: thread,
        message_count: count.max(1),
        unread_count: unread,
        // The id is the sort date (or a few seconds above it), as in `lists::threads`.
        last_activity: last.saturating_mul(1000),
        participants,
        has_attachments: has_attachments || message.has_attachments,
        flagged: flagged || message.flagged,
        subject,
        message,
    })
}

// ---------------------------------------------------------------------------------------------
// Above the rows

/// The newest message of a thread in a mailbox.
pub(crate) const NEWEST_IN_MAILBOX: &str = "
    SELECT t.id FROM message t CROSS JOIN message_location l
      ON l.mailbox_id = ?2 AND l.message_id = t.id
    WHERE t.thread_id = ?1 ORDER BY t.id DESC LIMIT 1";

/// Whether a message lies in a mailbox.
pub(crate) const LOCATION_EXISTS: &str =
    "SELECT 1 FROM message_location WHERE mailbox_id = ?1 AND message_id = ?2";

/// Pinned conversations (`returned = false`) or those with a returned snooze, once per thread,
/// as the row of their newest message in `mailboxes`; the most recently pinned or returned
/// first. A conversation counts when any of its messages is pinned (returned) and one of its
/// messages lies in the scope: exactly the threads the rows leave out.
pub(crate) fn top(
    conn: &Connection,
    returned: bool,
    mailboxes: &[(MailboxId, AccountId)],
    limit: u32,
) -> Result<Vec<ThreadListItem>, StoreError> {
    let mut by_account: HashMap<AccountId, Vec<MailboxId>> = HashMap::new();
    for (mailbox, account) in mailboxes {
        by_account.entry(*account).or_default().push(*mailbox);
    }
    let sql = if returned {
        crate::inbox::RETURNED_IN
    } else {
        crate::inbox::PINNED_IN
    };
    let mut out: Vec<(i64, ThreadListItem)> = Vec::new();
    let mut accounts: Vec<&AccountId> = by_account.keys().collect();
    accounts.sort();
    for account in accounts {
        let Some(prefer) = by_account.get(account) else {
            continue;
        };
        let keys: Vec<(crate::marks::MessageKey, i64)> = conn
            .prepare_cached(sql)?
            .query_map(
                params![account.to_string(), i64::from(crate::MAX_PAGE_SIZE)],
                |r| {
                    Ok((
                        crate::marks::MessageKey {
                            account: crate::accounts::account_id_at(r, 0)?,
                            message_id_hdr: r.get(1)?,
                        },
                        r.get::<_, i64>(2)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut threads: HashSet<i64> = HashSet::new();
        let mut messages: HashSet<MessageId> = HashSet::new();
        for (key, rank) in keys {
            for message in crate::marks::messages_of(conn, &key)? {
                let thread = thread_of(conn, message)?;
                if let Some(t) = thread
                    && !threads.insert(t)
                {
                    break;
                }
                let Some(row) = newest_in(conn, message, thread, prefer)? else {
                    continue;
                };
                if !messages.insert(row) {
                    break;
                }
                let Some(entry) = entry(conn, row, prefer)? else {
                    continue;
                };
                out.push((rank, summarize(conn, entry.item, thread)?));
                break;
            }
        }
    }
    out.sort_by_key(|(rank, item)| (std::cmp::Reverse(*rank), std::cmp::Reverse(item.message.id)));
    out.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(out.into_iter().map(|(_, item)| item).collect())
}

fn thread_of(conn: &Connection, message: MessageId) -> Result<Option<i64>, StoreError> {
    Ok(conn
        .prepare_cached(crate::lists::THREAD_OF)?
        .query_row([message.0], |r| r.get::<_, Option<i64>>(0))
        .optional()?
        .flatten())
}

/// The newest message of `thread` (or `message` itself without one) lying in one of `mailboxes`.
fn newest_in(
    conn: &Connection,
    message: MessageId,
    thread: Option<i64>,
    mailboxes: &[MailboxId],
) -> Result<Option<MessageId>, StoreError> {
    let mut best: Option<i64> = None;
    for mailbox in mailboxes {
        let found: Option<i64> = match thread {
            Some(thread) => conn
                .prepare_cached(NEWEST_IN_MAILBOX)?
                .query_row(params![thread, mailbox.0], |r| r.get(0))
                .optional()?,
            None => conn
                .prepare_cached(LOCATION_EXISTS)?
                .query_row(params![mailbox.0, message.0], |r| r.get::<_, i64>(0))
                .optional()?
                .map(|_| message.0),
        };
        best = best.max(found);
    }
    Ok(best.map(MessageId))
}

// ---------------------------------------------------------------------------------------------
// Unread first

/// One page of "unread first" as conversations: those with an unread message in the scope,
/// then the others (decision 0039).
pub(crate) fn unread_first(
    conn: &Connection,
    mailboxes: &[(MailboxId, AccountId)],
    cursor: Option<UnreadFirstCursor>,
    limit: u32,
) -> Result<UnreadFirstThreadPage, StoreError> {
    let cursor = cursor.unwrap_or(UnreadFirstCursor {
        phase: UnreadPhase::Unread,
        before: None,
    });
    let mut items = Vec::new();
    if cursor.phase == UnreadPhase::Unread {
        let page = list(conn, InboxListKind::Unread, mailboxes, cursor.before, limit)?;
        items = page.items;
        if let Some(before) = page.next_cursor {
            return Ok(UnreadFirstThreadPage {
                items,
                next_cursor: Some(UnreadFirstCursor {
                    phase: UnreadPhase::Unread,
                    before: Some(before),
                }),
            });
        }
    }
    let rest = limit.saturating_sub(u32::try_from(items.len()).unwrap_or(u32::MAX));
    let before = match cursor.phase {
        UnreadPhase::Unread => None,
        UnreadPhase::Read => cursor.before,
    };
    if rest == 0 {
        return Ok(UnreadFirstThreadPage {
            items,
            next_cursor: Some(UnreadFirstCursor {
                phase: UnreadPhase::Read,
                before,
            }),
        });
    }
    let page = list(conn, InboxListKind::Read, mailboxes, before, rest)?;
    items.extend(page.items);
    Ok(UnreadFirstThreadPage {
        items,
        next_cursor: page.next_cursor.map(|before| UnreadFirstCursor {
            phase: UnreadPhase::Read,
            before: Some(before),
        }),
    })
}

// ---------------------------------------------------------------------------------------------
// What an action on conversation rows touches

/// The messages of a thread lying in a mailbox, newest first.
pub(crate) const THREAD_IN_MAILBOX: &str = "
    SELECT t.id FROM message t CROSS JOIN message_location l
      ON l.mailbox_id = ?2 AND l.message_id = t.id
    WHERE t.thread_id = ?1 ORDER BY t.id DESC";

/// The messages an action on the conversation rows `rows` (each the row's message and its
/// mailbox) touches, as list rows, newest first and each location once (decision 0039):
/// - [`ConversationReach::Newest`]: the row's message, in the row's mailbox;
/// - [`ConversationReach::Listed`]: every message of its thread in the row's mailbox;
/// - [`ConversationReach::Everywhere`]: every message of its thread; `per_location`: once per
///   mailbox it lies in (mail actions), else once (marks), in the row's mailbox where it lies
///   there.
///
/// A row whose message is gone is [`StoreError::NotFound`] (the list is outdated).
pub(crate) fn targets(
    conn: &Connection,
    rows: &[MessageRef],
    reach: ConversationReach,
    per_location: bool,
) -> Result<Vec<MessageListItem>, StoreError> {
    if rows.is_empty() || rows.len() > MAX_ACTION_MESSAGES {
        return Err(StoreError::InvalidArgument(format!(
            "1 to {MAX_ACTION_MESSAGES} conversations"
        )));
    }
    let mut out: Vec<MessageListItem> = Vec::new();
    let mut taken: HashSet<(MessageId, MailboxId)> = HashSet::new();
    let mut taken_messages: HashSet<MessageId> = HashSet::new();
    for row in rows {
        let thread = conn
            .prepare_cached(crate::lists::THREAD_OF)?
            .query_row([row.message_id.0], |r| r.get::<_, Option<i64>>(0))
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("message {}", row.message_id)))?;
        let prefer = [row.mailbox_id];
        let ids: Vec<MessageId> = match (reach, thread) {
            (ConversationReach::Newest, _) | (_, None) => vec![row.message_id],
            (ConversationReach::Listed, Some(thread)) => conn
                .prepare_cached(THREAD_IN_MAILBOX)?
                .query_map(params![thread, row.mailbox_id.0], |r| {
                    r.get::<_, i64>(0).map(MessageId)
                })?
                .collect::<Result<Vec<_>, _>>()?,
            (ConversationReach::Everywhere, Some(thread)) => conn
                .prepare_cached(crate::lists::THREAD_MESSAGES)?
                .query_map(params![thread, i64::MAX], |r| {
                    r.get::<_, i64>(0).map(MessageId)
                })?
                .collect::<Result<Vec<_>, _>>()?,
        };
        for id in ids {
            let Some(found) = entry(conn, id, &prefer)? else {
                continue;
            };
            // Listed and Newest act in the row's mailbox only.
            let here = found.mailbox_ids.contains(&row.mailbox_id);
            match reach {
                ConversationReach::Newest | ConversationReach::Listed => {
                    if !here && (per_location || reach == ConversationReach::Listed) {
                        continue;
                    }
                    if taken.insert((id, found.item.mailbox_id)) && taken_messages.insert(id) {
                        out.push(found.item);
                    }
                }
                ConversationReach::Everywhere if per_location => {
                    for mailbox in &found.mailbox_ids {
                        if !taken.insert((id, *mailbox)) {
                            continue;
                        }
                        if *mailbox == found.item.mailbox_id {
                            out.push(found.item.clone());
                        } else if let Some(other) = entry(conn, id, &[*mailbox])? {
                            out.push(other.item);
                        }
                    }
                }
                ConversationReach::Everywhere => {
                    if taken_messages.insert(id) {
                        out.push(found.item);
                    }
                }
            }
        }
    }
    out.sort_by_key(|item| (std::cmp::Reverse(item.id), item.mailbox_id));
    Ok(out)
}
