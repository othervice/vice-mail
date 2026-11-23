//! Stage 9: the inbox lists (decision 0020). Like the lists of stage 7: keyset by message id,
//! newest first, one `UNION ALL` branch per mailbox, `CROSS JOIN` for the join order, a plan
//! test for each query (`stage9_plan_tests`).
//!
//! - **Rows** of the inbox leave out snoozed messages (waiting, or returned: those stand on top)
//!   and pinned ones (on top as well).
//! - **Filtered** lists (a category, the important senders) leave out waiting snoozes only. A
//!   category list is a range of `loc_category`, the important list of `loc_important`.
//! - The snooze and pin checks are one probe per row of the unique index on account UUID and
//!   Message-ID; the account of a branch is a parameter of its own.

use std::collections::HashMap;

use mail_model::{
    AccountId, Address, ListCursor, MailboxId, MessageCategory, MessageId, MessageListItem,
    MessagePage, UidSet,
};
use rusqlite::{Connection, params, params_from_iter};

use crate::StoreError;
use crate::lists::{item_at, list_columns, page};

/// Which rows an inbox list returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxListKind {
    /// The rows of the inbox: no snoozes, no pins.
    Rows,
    /// One category (after the user's rules), without waiting snoozes.
    Category(MessageCategory),
    /// Important senders, without waiting snoozes.
    Important,
}

/// One branch: mailbox `?{p}`, its account UUID `?{p + 1}`; cursor `?1`, limit `?2`, category
/// code `?3`.
fn branch(kind: InboxListKind, p: usize) -> String {
    let a = p + 1;
    // `INDEXED BY`: without statistics the planner would walk the primary key and filter.
    let (index, range, exclude) = match kind {
        InboxListKind::Rows => (
            "",
            format!("l.mailbox_id = ?{p} AND l.message_id < ?1"),
            format!(
                "AND NOT EXISTS (SELECT 1 FROM snooze s
                                 WHERE s.account_uuid = ?{a} AND s.message_id_hdr = m.message_id_hdr)
                 AND NOT EXISTS (SELECT 1 FROM pin p
                                 WHERE p.account_uuid = ?{a} AND p.message_id_hdr = m.message_id_hdr)"
            ),
        ),
        InboxListKind::Category(_) => (
            "INDEXED BY loc_category",
            format!("l.mailbox_id = ?{p} AND l.category = ?3 AND l.message_id < ?1"),
            waiting_snooze(a),
        ),
        InboxListKind::Important => (
            "INDEXED BY loc_important",
            format!("l.mailbox_id = ?{p} AND l.important = 1 AND l.message_id < ?1"),
            waiting_snooze(a),
        ),
    };
    format!(
        "SELECT * FROM (SELECT {cols}, l.mailbox_id
           FROM message_location l {index} CROSS JOIN message m ON m.id = l.message_id
           WHERE {range} {exclude}
           ORDER BY l.message_id DESC LIMIT ?2)",
        cols = list_columns()
    )
}

fn waiting_snooze(a: usize) -> String {
    format!(
        "AND NOT EXISTS (SELECT 1 FROM snooze s
                         WHERE s.account_uuid = ?{a} AND s.message_id_hdr = m.message_id_hdr
                           AND s.returned_at IS NULL)"
    )
}

/// The query over `n` mailboxes.
pub(crate) fn inbox_sql(kind: InboxListKind, n: usize) -> String {
    let branches: Vec<String> = (0..n).map(|i| branch(kind, 4 + 2 * i)).collect();
    if n == 1 {
        return branches.into_iter().next().unwrap_or_default();
    }
    format!("{} ORDER BY 1 DESC LIMIT ?2", branches.join(" UNION ALL "))
}

/// One page of an inbox list over `mailboxes` (with their accounts).
pub(crate) fn list(
    conn: &Connection,
    kind: InboxListKind,
    mailboxes: &[(MailboxId, AccountId)],
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, StoreError> {
    if limit == 0 || limit > crate::MAX_PAGE_SIZE {
        return Err(StoreError::InvalidArgument(format!(
            "list limit {limit} is outside 1..={}",
            crate::MAX_PAGE_SIZE
        )));
    }
    if mailboxes.is_empty() {
        return Ok(MessagePage {
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
        .prepare_cached(&inbox_sql(kind, mailboxes.len()))?
        .query_map(params_from_iter(values), |r| {
            let mailbox = MailboxId(r.get(14)?);
            let account = accounts
                .get(&mailbox.0)
                .copied()
                .ok_or(rusqlite::Error::InvalidColumnIndex(14))?;
            item_at(r, account, mailbox)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (mut items, next_cursor) = page(rows, limit, |i| i.id);
    crate::marks::decorate(conn, &mut items)?;
    Ok(MessagePage { items, next_cursor })
}

// ---------------------------------------------------------------------------------------------
// Above the rows: pins, returned snoozes, priority

pub(crate) const PINNED_IN: &str = "
    SELECT p.account_uuid, p.message_id_hdr, p.pinned_at FROM pin p WHERE p.account_uuid = ?1
    ORDER BY p.pinned_at DESC LIMIT ?2";
pub(crate) const RETURNED_IN: &str = "
    SELECT s.account_uuid, s.message_id_hdr, s.returned_at FROM snooze s
    WHERE s.account_uuid = ?1 AND s.returned_at IS NOT NULL
    ORDER BY s.returned_at DESC LIMIT ?2";

/// Pinned (`returned = false`) or returned messages of `mailboxes`, as rows of the mailbox they
/// lie in; the most recently pinned or returned first.
pub(crate) fn top(
    conn: &Connection,
    returned: bool,
    mailboxes: &[(MailboxId, AccountId)],
    limit: u32,
) -> Result<Vec<MessageListItem>, StoreError> {
    let mut by_account: HashMap<AccountId, Vec<MailboxId>> = HashMap::new();
    for (mailbox, account) in mailboxes {
        by_account.entry(*account).or_default().push(*mailbox);
    }
    let mut out: Vec<(i64, MessageListItem)> = Vec::new();
    let sql = if returned { RETURNED_IN } else { PINNED_IN };
    let mut accounts: Vec<&AccountId> = by_account.keys().collect();
    accounts.sort();
    for account in accounts {
        let Some(prefer) = by_account.get(account) else {
            continue;
        };
        let keys: Vec<(crate::marks::MessageKey, i64)> = conn
            .prepare_cached(sql)?
            .query_map(params![account.to_string(), i64::from(limit)], |r| {
                Ok((
                    crate::marks::MessageKey {
                        account: crate::accounts::account_id_at(r, 0)?,
                        message_id_hdr: r.get(1)?,
                    },
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (key, rank) in keys {
            for message in crate::marks::messages_of(conn, &key)? {
                let Some(entry) = crate::lists::entry(conn, message, prefer)? else {
                    continue;
                };
                if prefer.contains(&entry.item.mailbox_id) {
                    out.push((rank, entry.item));
                    break;
                }
            }
        }
    }
    out.sort_by_key(|(rank, item)| (std::cmp::Reverse(*rank), std::cmp::Reverse(item.id)));
    out.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(out.into_iter().map(|(_, item)| item).collect())
}

pub(crate) const IMPORTANT_COUNTS: &str = "
    SELECT count(*), coalesce(sum(seen = 0), 0) FROM message_location INDEXED BY loc_important
    WHERE mailbox_id = ?1 AND important = 1";

/// Messages of important senders in `mailboxes`: all and unread.
pub(crate) fn important_counts(
    conn: &Connection,
    mailboxes: &[MailboxId],
) -> Result<(u32, u32), StoreError> {
    let mut total = 0i64;
    let mut unread = 0i64;
    let mut stmt = conn.prepare_cached(IMPORTANT_COUNTS)?;
    for mailbox in mailboxes {
        let (t, u): (i64, i64) = stmt.query_row([mailbox.0], |r| Ok((r.get(0)?, r.get(1)?)))?;
        total += t;
        unread += u;
    }
    let cut = |n: i64| u32::try_from(n.max(0)).unwrap_or(u32::MAX);
    Ok((cut(total), cut(unread)))
}

pub(crate) const UNREAD_ALL: &str =
    "SELECT count(*) FROM message_location WHERE mailbox_id = ?1 AND seen = 0";
pub(crate) const UNREAD_PEOPLE: &str = "
    SELECT count(*) FROM message_location INDEXED BY loc_unseen
    WHERE mailbox_id = ?1 AND seen = 0 AND (category IS NULL OR category = 0 OR important = 1)";

/// Unread messages in `mailboxes`; `people`: only people's, important and unclassified ones.
pub(crate) fn unread(
    conn: &Connection,
    mailboxes: &[MailboxId],
    people: bool,
) -> Result<u32, StoreError> {
    let mut stmt = conn.prepare_cached(if people { UNREAD_PEOPLE } else { UNREAD_ALL })?;
    let mut total = 0i64;
    for mailbox in mailboxes {
        total += stmt.query_row([mailbox.0], |r| r.get::<_, i64>(0))?;
    }
    Ok(u32::try_from(total.max(0)).unwrap_or(u32::MAX))
}

// ---------------------------------------------------------------------------------------------
// New mail

/// A message that just arrived in a mailbox, for local rules and notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivedMessage {
    pub id: MessageId,
    pub mailbox_id: MailboxId,
    pub account_id: AccountId,
    pub unread: bool,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// The normalised List-Id, if any.
    pub list_id: Option<String>,
    pub category: Option<MessageCategory>,
    pub important: bool,
    pub message_id_hdr: Option<String>,
}

pub(crate) const ARRIVED: &str = "
    SELECT l.message_id, l.seen, m.subject, m.from_name, m.from_addr, m.to_addrs, m.cc_addrs,
           m.list_key, m.category, m.important, m.message_id_hdr,
           (SELECT a.uuid FROM account a WHERE a.id = m.account_id)
    FROM message_location l CROSS JOIN message m ON m.id = l.message_id
    WHERE l.mailbox_id = ?1 AND l.uid BETWEEN ?2 AND ?3
    ORDER BY l.uid";

/// The stored messages at `uids` of `mailbox`, oldest UID first.
pub(crate) fn arrived(
    conn: &Connection,
    mailbox: MailboxId,
    uids: &UidSet,
) -> Result<Vec<ArrivedMessage>, StoreError> {
    let mut out = Vec::new();
    let mut stmt = conn.prepare_cached(ARRIVED)?;
    for &(first, last) in uids.ranges() {
        let rows = stmt.query_map(params![mailbox.0, first, last], |r| {
            let from_name: Option<String> = r.get(3)?;
            let from_addr: Option<String> = r.get(4)?;
            Ok(ArrivedMessage {
                id: MessageId(r.get(0)?),
                mailbox_id: mailbox,
                account_id: crate::accounts::account_id_at(r, 11)?,
                unread: !r.get::<_, bool>(1)?,
                subject: r.get(2)?,
                from: from_addr.map(|email| Address {
                    name: from_name,
                    email,
                }),
                to: crate::messages::addresses_at(r, 5)?,
                cc: crate::messages::addresses_at(r, 6)?,
                list_id: r.get(7)?,
                category: r
                    .get::<_, Option<i64>>(8)?
                    .and_then(MessageCategory::from_code),
                important: r.get(9)?,
                message_id_hdr: r.get(10)?,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}
