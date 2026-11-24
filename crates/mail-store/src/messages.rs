//! Message headers and their locations: storing sync windows, flags, removals and the list.

use std::collections::BTreeMap;

use mail_model::{
    AccountId, Address, FlagUpdate, Flags, ListCursor, MailboxId, MessageActivity, MessageHeader,
    MessageId, MessageListItem, MessagePage, ModSeq, Uid, UidSet, UidValidity,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::accounts::{self, conversion_error};
use crate::mail_api::{MAX_PAGE_SIZE, StoreReport};
use crate::mailboxes::{self, modseq_param};

/// One page of a mailbox, newest first. `CROSS JOIN` fixes the join order (decision 0003): the
/// page is a contiguous range of the location key, each message a rowid lookup.
///
/// The last column is the outbox state of the row (stage 4): the operations on this location
/// that are not confirmed, `failed` (3) before `conflict` (2) before pending (1), NULL if none.
/// One probe of `operation_location` per row; `+o.message_id` keeps the planner on that index
/// (the message id only guards against a UID reused after a UIDVALIDITY change).
pub(crate) const LIST_PAGE: &str = "
    SELECT l.message_id, l.seen, l.flagged, l.answered, l.draft, l.keywords,
           m.subject, m.from_name, m.from_addr, m.to_addrs, m.date, m.has_attachments, m.preview,
           (SELECT max(CASE o.state WHEN 'failed' THEN 3 WHEN 'conflict' THEN 2 ELSE 1 END)
            FROM operation o
            WHERE o.mailbox_id = l.mailbox_id AND o.uid = l.uid
              AND +o.message_id = l.message_id AND o.state <> 'confirmed'),
           m.size
    FROM message_location l CROSS JOIN message m ON m.id = l.message_id
    WHERE l.mailbox_id = ?1 AND l.message_id < ?2
    ORDER BY l.message_id DESC LIMIT ?3";

/// Account of a mailbox, for the list items. A scalar subquery, not a join: with statistics of
/// a single account the planner scanned `account` for the join.
pub(crate) const MAILBOX_ACCOUNT: &str = "
    SELECT (SELECT a.uuid FROM account a WHERE a.id = b.account_id) FROM mailbox b WHERE b.id = ?1";

pub(crate) const LOCATION_BY_UID: &str = "
    SELECT seen, flagged, answered, draft, deleted, keywords, modseq
    FROM message_location WHERE mailbox_id = ?1 AND uid = ?2";

pub(crate) const UPDATE_LOCATION: &str = "
    UPDATE message_location
    SET seen = ?3, flagged = ?4, answered = ?5, draft = ?6, deleted = ?7, keywords = ?8,
        modseq = ?9
    WHERE mailbox_id = ?1 AND uid = ?2";

/// A message of the account with this Message-ID that does not lie in the mailbox yet
/// (merge rule of decision 0003). The oldest such row, so duplicates pair up stably.
pub(crate) const MERGE_CANDIDATE: &str = "
    SELECT m.id FROM message m
    WHERE m.account_id = ?1 AND m.message_id_hdr = ?2
      AND NOT EXISTS (SELECT 1 FROM message_location l
                      WHERE l.mailbox_id = ?3 AND l.message_id = m.id)
    ORDER BY m.id LIMIT 1";

/// Taken ids from a candidate upwards, to find the first gap.
pub(crate) const IDS_FROM: &str = "SELECT id FROM message WHERE id >= ?1 ORDER BY id";

pub(crate) const INSERT_MESSAGE: &str = "
    INSERT INTO message (id, account_id, message_id_hdr, date, subject, from_name, from_addr,
                         to_addrs, cc_addrs, in_reply_to, references_hdr, list_id,
                         list_unsubscribe, auto_submitted, precedence, size, has_attachments,
                         thread_id, subject_key, category, category_auto, category_reason,
                         important, sender_key, sender_domain, list_key, unsubscribe_one_click,
                         reply_to_addrs)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
            ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)";

/// A new message's location gets the category and important mark just classified (`?11`,
/// `?12`), so the trigger `loc_category_new` (migration 0007) only runs where they are not
/// known here: a merged location, an unclassified message. Its UPDATE of the new row cost
/// about 1.8 ms per window of 500 (`docs/testing/performance.md`, stage 8/9).
///
/// Stage 19 (decision 0061): the sort keys come from the message row (`sort_keys`), so the
/// message must exist; nothing is inserted otherwise.
pub(crate) const INSERT_LOCATION: &str = "
    INSERT INTO message_location (mailbox_id, message_id, uid, seen, flagged, answered, draft,
                                  deleted, keywords, modseq, category, important,
                                  sort_sender, sort_recipient, sort_subject, size)
    SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
           vm_sort_name(m.from_name, m.from_addr), vm_sort_recipient(m.to_addrs),
           vm_sort_subject(m.subject), max(m.size, 0)
    FROM message m WHERE m.id = ?2";

pub(crate) const REMOVE_UID_RANGE: &str =
    "DELETE FROM message_location WHERE mailbox_id = ?1 AND uid BETWEEN ?2 AND ?3";

pub(crate) const KNOWN_UIDS: &str =
    "SELECT uid FROM message_location WHERE mailbox_id = ?1 ORDER BY uid";

/// Messages without a location, except those an outbox operation still refers to (stage 4):
/// a pending move has removed the source location, and a confirmed one may wait for the
/// target's sync to merge the message back.
pub(crate) const DELETE_ORPHANS: &str = "
    DELETE FROM message
    WHERE account_id = ?1
      AND NOT EXISTS (SELECT 1 FROM message_location l WHERE l.message_id = message.id)
      AND NOT EXISTS (SELECT 1 FROM operation o WHERE o.message_id = message.id
                      AND o.state IN ('queued', 'in_flight', 'confirmed'))";

/// See [`crate::Store::store_headers`].
pub(crate) fn store_headers(
    conn: &Connection,
    mailbox: MailboxId,
    uid_validity: UidValidity,
    mut headers: Vec<MessageHeader>,
    sync_low_uid: Option<Uid>,
    classifier: Option<&std::sync::Arc<dyn crate::Classifier>>,
) -> Result<StoreReport, StoreError> {
    let account_row = mailboxes::check_uid_validity(conn, mailbox, uid_validity)?;
    // Stage 9: new messages are classified in this transaction (own addresses and the sender
    // rules loaded once per window, and only when a new message needs them).
    let mut window: Option<crate::categories::WindowClassifier> = None;

    // Ascending sort date, then UID: new rows get ascending ids (B-tree appends, and the FTS
    // rows of stage 7 are cheapest in rowid order), and of two mails with the same date the
    // later arrival gets the larger id, so it is listed first.
    headers.sort_by_key(|h| (h.sort_date(), h.uid));

    let mut report = StoreReport::default();
    let mut ids = IdAllocator::default();
    // Stage 7: each new message is threaded and indexed (headers) in this transaction.
    let mut threader = crate::threads::Threader::default();
    let mut fts_rows = Vec::new();
    // Stage 14 (decision 0034): the label this mailbox stands for on Gmail.
    let gmail_label = if headers.iter().any(|h| h.gmail.is_some()) {
        crate::gmail::mailbox_label(conn, mailbox)?
    } else {
        None
    };
    for header in &headers {
        let flags = clean_flags(mailbox, header.uid, &header.flags);
        if apply_flags(conn, mailbox, header.uid, &flags, header.modseq)?.is_some() {
            if let Some(info) = &header.gmail {
                crate::gmail::refresh_labels(
                    conn,
                    mailbox,
                    header.uid,
                    &info.labels,
                    gmail_label.as_deref(),
                )?;
            }
            report.updated += 1;
            continue;
        }

        let message_id = header.message_id.as_deref().filter(|m| !m.is_empty());
        // Gmail: one message across labels by X-GM-MSGID; elsewhere by Message-ID.
        let existing: Option<i64> = match (&header.gmail, message_id) {
            (Some(info), _) => {
                crate::gmail::merge_candidate(conn, account_row, mailbox, info, message_id)?
            }
            (None, Some(message_id)) => conn
                .prepare_cached(MERGE_CANDIDATE)?
                .query_row(params![account_row, message_id, mailbox.0], |r| r.get(0))
                .optional()?,
            (None, None) => None,
        };
        {
            if let Some(existing) = existing {
                // The trigger copies the message's category onto the new location.
                insert_location(
                    conn,
                    mailbox,
                    existing,
                    header.uid,
                    &flags,
                    header.modseq,
                    None,
                )?;
                if let Some(info) = &header.gmail {
                    crate::gmail::tag_message(conn, existing, info, gmail_label.as_deref())?;
                }
                report.merged += 1;
                continue;
            }
        }

        let id = ids.allocate(conn, header.sort_date())?;
        let thread = threader.assign_new(conn, account_row, id, header)?;
        let category = match classifier {
            Some(classifier) => {
                if window.is_none() {
                    window = Some(crate::categories::WindowClassifier::new(
                        conn,
                        std::sync::Arc::clone(classifier),
                    )?);
                }
                match &window {
                    Some(w) => Some(w.columns(conn, account_row, header)?),
                    None => None,
                }
            }
            None => None,
        };
        insert_message(
            conn,
            id,
            account_row,
            message_id,
            header,
            thread,
            category.as_ref(),
        )?;
        insert_location(
            conn,
            mailbox,
            id,
            header.uid,
            &flags,
            header.modseq,
            category.as_ref(),
        )?;
        if let Some(info) = &header.gmail {
            crate::gmail::tag_message(conn, id, info, gmail_label.as_deref())?;
        }
        fts_rows.push((id, crate::fts::Row::from_header(header)));
        report.inserted += 1;
        if !flags.seen && !flags.deleted {
            report.new_unread += 1;
        }
    }
    if !fts_rows.is_empty() {
        crate::fts::write(conn, &crate::fts::targets(conn)?, &mut fts_rows)?;
    }

    if let Some(low) = sync_low_uid {
        mailboxes::set_sync_low_uid(conn, mailbox, low)?;
    }
    tracing::debug!(
        mailbox = mailbox.0,
        inserted = report.inserted,
        merged = report.merged,
        updated = report.updated,
        "stored headers"
    );
    Ok(report)
}

/// See [`crate::Store::update_flags`].
pub(crate) fn update_flags(
    conn: &Connection,
    mailbox: MailboxId,
    uid_validity: UidValidity,
    updates: &[FlagUpdate],
) -> Result<usize, StoreError> {
    mailboxes::check_uid_validity(conn, mailbox, uid_validity)?;
    let gmail_label = if updates.iter().any(|u| u.gmail_labels.is_some()) {
        crate::gmail::mailbox_label(conn, mailbox)?
    } else {
        None
    };
    let mut changed = 0;
    for update in updates {
        let flags = clean_flags(mailbox, update.uid, &update.flags);
        let mut moved =
            apply_flags(conn, mailbox, update.uid, &flags, update.modseq)? == Some(true);
        // Stage 14: Gmail labels read with the flags.
        if let Some(labels) = &update.gmail_labels {
            moved |= crate::gmail::refresh_labels(
                conn,
                mailbox,
                update.uid,
                labels,
                gmail_label.as_deref(),
            )?;
        }
        if moved {
            changed += 1;
        }
    }
    Ok(changed)
}

/// See [`crate::Store::remove_uids`].
pub(crate) fn remove_uids(
    conn: &Connection,
    mailbox: MailboxId,
    uid_validity: UidValidity,
    uids: &UidSet,
) -> Result<usize, StoreError> {
    mailboxes::check_uid_validity(conn, mailbox, uid_validity)?;
    let mut stmt = conn.prepare_cached(REMOVE_UID_RANGE)?;
    let mut removed = 0;
    for &(first, last) in uids.ranges() {
        removed += stmt
            .execute(params![mailbox.0, first, last])
            .map_err(StoreError::sqlite("remove locations"))?;
    }
    Ok(removed)
}

pub(crate) fn known_uids(conn: &Connection, mailbox: MailboxId) -> Result<UidSet, StoreError> {
    mailboxes::ensure_exists(conn, mailbox)?;
    let uids = conn
        .prepare_cached(KNOWN_UIDS)?
        .query_map([mailbox.0], |r| r.get::<_, Uid>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(UidSet::from_uids(uids))
}

/// Deletes the orphans; returns their count and the body files that became unreferenced (to
/// remove after the commit).
pub(crate) fn delete_orphans(
    conn: &Connection,
    account: AccountId,
) -> Result<(usize, Vec<String>), StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    let files = crate::bodies::orphan_files(conn, account_row)?;
    let deleted = conn
        .prepare_cached(DELETE_ORPHANS)?
        .execute([account_row])
        .map_err(StoreError::sqlite("delete orphan messages"))?;
    // A file packed with bodies of messages that still exist stays.
    Ok((deleted, crate::bodies::unreferenced(conn, files)?))
}

/// See [`crate::Store::list_messages`].
pub(crate) fn list(
    conn: &Connection,
    mailbox: MailboxId,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, StoreError> {
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(StoreError::InvalidArgument(format!(
            "list limit {limit} is outside 1..={MAX_PAGE_SIZE}"
        )));
    }
    let account: AccountId = conn
        .prepare_cached(MAILBOX_ACCOUNT)?
        .query_row([mailbox.0], |r| accounts::account_id_at(r, 0))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("mailbox {mailbox}")))?;

    let before = cursor.map_or(i64::MAX, |c| c.before().0);
    // One row more than asked tells whether another page follows.
    let mut items: Vec<MessageListItem> = conn
        .prepare_cached(LIST_PAGE)?
        .query_map(params![mailbox.0, before, i64::from(limit) + 1], |r| {
            let from_name: Option<String> = r.get(7)?;
            let from_addr: Option<String> = r.get(8)?;
            let from = from_addr.map(|email| Address {
                name: from_name,
                email,
            });
            let date: i64 = r.get(10)?;
            Ok(MessageListItem {
                id: MessageId(r.get(0)?),
                account_id: account,
                mailbox_id: mailbox,
                from,
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
                activity: match r.get::<_, Option<i64>>(13)? {
                    None => None,
                    Some(3) => Some(MessageActivity::Failed),
                    Some(2) => Some(MessageActivity::Conflict),
                    Some(_) => Some(MessageActivity::Pending),
                },
                category: None,
                important: false,
                pinned: false,
                snooze: None,
                task: None,
                reading_list: false,
                size: crate::lists::size_at(r, 14)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|item| ListCursor::after(item.id))
    } else {
        None
    };
    // Stage 9: category, important mark, pin, snooze and task of each row.
    crate::marks::decorate(conn, &mut items)?;
    Ok(MessagePage { items, next_cursor })
}

/// Allocates `message.id` values: the sort date, or the next free number above it.
///
/// Remembers the runs of taken ids it has seen during one window, so a window of many mails
/// with the same date costs one scan of the run, not one per mail. Only valid inside the
/// transaction that inserts the allocated ids.
#[derive(Default)]
struct IdAllocator {
    /// Start → end (inclusive) of runs of ids known to be taken.
    taken: BTreeMap<i64, i64>,
}

impl IdAllocator {
    fn allocate(&mut self, conn: &Connection, candidate: i64) -> Result<i64, StoreError> {
        let (run_start, probe) = match self.taken.range(..=candidate).next_back() {
            Some((&start, &end)) if end >= candidate => (start, end + 1),
            _ => (candidate, candidate),
        };

        let mut free = probe;
        let mut stmt = conn.prepare_cached(IDS_FROM)?;
        let mut rows = stmt.query([probe])?;
        while let Some(row) = rows.next()? {
            if row.get::<_, i64>(0)? != free {
                break;
            }
            free += 1;
        }

        // [run_start, free] is taken now. Absorb known runs that start inside or right after.
        let mut end = free;
        let absorbed: Vec<i64> = self
            .taken
            .range(run_start..=free.saturating_add(1))
            .map(|(&start, _)| start)
            .collect();
        for start in absorbed {
            if let Some(e) = self.taken.remove(&start) {
                end = end.max(e);
            }
        }
        self.taken.insert(run_start, end);
        Ok(free)
    }
}

/// Flags as stored: keywords normalised, and anything that is not an IMAP atom dropped (it
/// would break the space-separated column; a server cannot send one).
fn clean_flags(mailbox: MailboxId, uid: Uid, flags: &Flags) -> Flags {
    let mut flags = flags.clone();
    let before = flags.keywords.len();
    flags
        .keywords
        .retain(|k| !k.is_empty() && !k.chars().any(|c| c.is_whitespace() || c.is_control()));
    if flags.keywords.len() != before {
        tracing::warn!(
            mailbox = mailbox.0,
            uid,
            dropped = before - flags.keywords.len(),
            "dropped keywords that are not IMAP atoms"
        );
    }
    flags.normalize_keywords();
    flags
}

struct StoredFlags {
    seen: bool,
    flagged: bool,
    answered: bool,
    draft: bool,
    deleted: bool,
    keywords: Option<String>,
    modseq: Option<i64>,
}

/// Writes `flags` and `modseq` to the location with this UID. `None` if there is none,
/// otherwise whether flags or keywords changed. Keywords that only differ in ASCII case keep
/// their stored spelling.
fn apply_flags(
    conn: &Connection,
    mailbox: MailboxId,
    uid: Uid,
    flags: &Flags,
    modseq: Option<ModSeq>,
) -> Result<Option<bool>, StoreError> {
    let stored = conn
        .prepare_cached(LOCATION_BY_UID)?
        .query_row(params![mailbox.0, uid], |r| {
            Ok(StoredFlags {
                seen: r.get(0)?,
                flagged: r.get(1)?,
                answered: r.get(2)?,
                draft: r.get(3)?,
                deleted: r.get(4)?,
                keywords: r.get(5)?,
                modseq: r.get(6)?,
            })
        })
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let modseq = modseq_param(modseq)?;
    let same_keywords = keywords_equal(stored.keywords.as_deref(), &flags.keywords);
    let same_flags = stored.seen == flags.seen
        && stored.flagged == flags.flagged
        && stored.answered == flags.answered
        && stored.draft == flags.draft
        && stored.deleted == flags.deleted
        && same_keywords;
    if same_flags && stored.modseq == modseq {
        return Ok(Some(false));
    }
    let keywords = if same_keywords {
        stored.keywords
    } else {
        keywords_text(&flags.keywords)
    };
    conn.prepare_cached(UPDATE_LOCATION)?
        .execute(params![
            mailbox.0,
            uid,
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft,
            flags.deleted,
            keywords,
            modseq,
        ])
        .map_err(StoreError::sqlite("update flags"))?;
    Ok(Some(!same_flags))
}

/// `category`: the columns just written for a new message; `None` leaves the copy to the
/// trigger (merged locations, unclassified messages).
fn insert_location(
    conn: &Connection,
    mailbox: MailboxId,
    message: i64,
    uid: Uid,
    flags: &Flags,
    modseq: Option<ModSeq>,
    category: Option<&crate::categories::CategoryColumns>,
) -> Result<(), StoreError> {
    let inserted = conn
        .prepare_cached(INSERT_LOCATION)?
        .execute(params![
            mailbox.0,
            message,
            uid,
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft,
            flags.deleted,
            keywords_text(&flags.keywords),
            modseq_param(modseq)?,
            category.map(|c| c.category),
            category.is_some_and(|c| c.important),
        ])
        .map_err(StoreError::sqlite("insert location"))?;
    if inserted == 0 {
        return Err(StoreError::NotFound(format!("message {message}")));
    }
    Ok(())
}

fn insert_message(
    conn: &Connection,
    id: i64,
    account_row: i64,
    message_id: Option<&str>,
    h: &MessageHeader,
    (thread, subject_key): (i64, Option<String>),
    category: Option<&crate::categories::CategoryColumns>,
) -> Result<(), StoreError> {
    let from = h.from.first();
    let references = (!h.references.is_empty()).then(|| h.references.join(" "));
    let (keys, one_click) = match category {
        Some(c) => (c.keys.clone(), c.one_click),
        None => (crate::categories::SenderKeys::default(), None),
    };
    // Stage 17: Reply-To for the contacts, where the header block was fetched.
    let reply_to = match category {
        Some(c) if !c.reply_to.is_empty() => addresses_json(&c.reply_to)?,
        _ => None,
    };
    conn.prepare_cached(INSERT_MESSAGE)?
        .execute(params![
            id,
            account_row,
            message_id,
            h.sort_date(),
            h.subject,
            from.and_then(|a| a.name.as_deref()),
            from.map(|a| a.email.as_str()),
            addresses_json(&h.to)?,
            addresses_json(&h.cc)?,
            h.in_reply_to,
            references,
            h.list_id,
            h.list_unsubscribe,
            h.auto_submitted,
            h.precedence,
            h.size,
            h.has_attachments,
            thread,
            subject_key,
            category.map(|c| c.category),
            category.map(|c| c.category_auto),
            category.map(|c| c.reason.as_str()),
            category.is_some_and(|c| c.important),
            keys.address,
            keys.domain,
            keys.list_id,
            one_click,
            reply_to,
        ])
        .map_err(StoreError::sqlite("insert message"))?;
    Ok(())
}

fn keywords_text(keywords: &[String]) -> Option<String> {
    (!keywords.is_empty()).then(|| keywords.join(" "))
}

pub(crate) fn keywords_from(text: Option<&str>) -> Vec<String> {
    text.map(|t| t.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Both lists are normalised (sorted ignoring ASCII case), so they compare element by element.
fn keywords_equal(stored: Option<&str>, new: &[String]) -> bool {
    let mut stored = stored.unwrap_or_default().split_whitespace();
    let mut new = new.iter();
    loop {
        match (stored.next(), new.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => {}
            _ => return false,
        }
    }
}

fn addresses_json(addresses: &[Address]) -> Result<Option<String>, StoreError> {
    if addresses.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(addresses)
        .map(Some)
        .map_err(|err| StoreError::InvalidArgument(format!("addresses: {err}")))
}

pub(crate) fn addresses_at(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Vec<Address>> {
    let Some(json) = r.get::<_, Option<String>>(idx)? else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&json).map_err(|err| conversion_error(idx, Type::Text, err.to_string()))
}
