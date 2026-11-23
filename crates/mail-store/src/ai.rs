//! Stage 13: AI in the app (migration 0011, decision 0024). The request log (metadata only,
//! with its retention), the dismissed open-reply hints and the rows the open-reply rules look
//! at. The AI settings are a device setting in `setting` (`ai.settings`); keys are in the
//! keychain.

use mail_model::{
    AI_REQUEST_MAX_ENTRIES, AI_REQUEST_RETENTION_DAYS, AccountId, Address, AiFunction, AiLocality,
    AiRequestEntry, AiRequestId, AiRequestOutcome, AiRequestPage, ErrorCode, MAX_AI_REQUEST_PAGE,
    MessageId,
};
use rusqlite::{Connection, Row, params};

use crate::accounts::account_id_at;
use crate::{Store, StoreError};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// One entry for [`Store::record_ai_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAiRequest {
    pub at: i64,
    pub function: AiFunction,
    pub profile_name: String,
    pub host: String,
    pub locality: AiLocality,
    pub model: String,
    pub mails: u32,
    pub bytes: u32,
    pub outcome: AiRequestOutcome,
    pub error: Option<ErrorCode>,
    pub duration_ms: u32,
}

/// A message as the open-reply rules see it (headers, preview and where it lies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenReplyRow {
    pub id: MessageId,
    pub account: AccountId,
    pub thread_id: Option<i64>,
    pub message_id_hdr: Option<String>,
    pub in_reply_to: Option<String>,
    /// Message ids without `<>`.
    pub references: Vec<String>,
    /// Unix seconds.
    pub date: i64,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// `List-Id` or `List-Unsubscribe` is present.
    pub list: bool,
    pub auto_submitted: Option<String>,
    pub precedence: Option<String>,
    /// 0 person, 1 notification, 2 newsletter, `None` not classified yet.
    pub category: Option<i64>,
    pub preview: Option<String>,
    /// Lies in a mailbox with the role `sent`.
    pub in_sent: bool,
    /// Lies in the inbox.
    pub in_inbox: bool,
    /// Lies only in Trash or Junk (or nowhere).
    pub only_trash: bool,
    /// `\Answered` in any of its locations.
    pub answered: bool,
    pub dismissed: bool,
}

const fn locality_text(locality: AiLocality) -> &'static str {
    match locality {
        AiLocality::ThisDevice => "this_device",
        AiLocality::LocalNetwork => "local_network",
        AiLocality::Internet => "internet",
    }
}

fn locality_of(text: &str) -> AiLocality {
    match text {
        "this_device" => AiLocality::ThisDevice,
        "local_network" => AiLocality::LocalNetwork,
        _ => AiLocality::Internet,
    }
}

const fn outcome_text(outcome: AiRequestOutcome) -> &'static str {
    match outcome {
        AiRequestOutcome::Ok => "ok",
        AiRequestOutcome::Cancelled => "cancelled",
        AiRequestOutcome::Failed => "failed",
    }
}

fn outcome_of(text: &str) -> AiRequestOutcome {
    match text {
        "ok" => AiRequestOutcome::Ok,
        "cancelled" => AiRequestOutcome::Cancelled,
        _ => AiRequestOutcome::Failed,
    }
}

fn error_text(code: ErrorCode) -> Option<String> {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn error_of(text: &str) -> Option<ErrorCode> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

const ENTRY_COLUMNS: &str = "id, at, function, profile_name, host, locality, model, mails, bytes, outcome, error, duration_ms";

fn entry_at(r: &Row<'_>) -> rusqlite::Result<AiRequestEntry> {
    let function: String = r.get(2)?;
    let locality: String = r.get(5)?;
    let mails: i64 = r.get(7)?;
    let bytes: i64 = r.get(8)?;
    let outcome: String = r.get(9)?;
    let error: Option<String> = r.get(10)?;
    let duration: i64 = r.get(11)?;
    Ok(AiRequestEntry {
        id: AiRequestId(r.get(0)?),
        at: r.get(1)?,
        // The CHECK allows only known names.
        function: AiFunction::parse(&function).unwrap_or(AiFunction::Summary),
        profile_name: r.get(3)?,
        host: r.get(4)?,
        locality: locality_of(&locality),
        model: r.get(6)?,
        mails: u32::try_from(mails).unwrap_or(u32::MAX),
        bytes: u32::try_from(bytes).unwrap_or(u32::MAX),
        outcome: outcome_of(&outcome),
        error: error.as_deref().and_then(error_of),
        duration_ms: u32::try_from(duration).unwrap_or(u32::MAX),
    })
}

fn record(conn: &Connection, entry: &NewAiRequest, now: i64) -> Result<AiRequestEntry, StoreError> {
    conn.prepare_cached(
        "INSERT INTO ai_request (at, function, profile_name, host, locality, model, mails, bytes,
                                 outcome, error, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        entry.at,
        entry.function.as_str(),
        entry.profile_name,
        entry.host,
        locality_text(entry.locality),
        entry.model,
        i64::from(entry.mails),
        i64::from(entry.bytes),
        outcome_text(entry.outcome),
        entry.error.and_then(error_text),
        i64::from(entry.duration_ms),
    ])
    .map_err(StoreError::sqlite("record AI request"))?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM ai_request WHERE at < ?1",
        [now - i64::from(AI_REQUEST_RETENTION_DAYS) * DAY_MS],
    )
    .map_err(StoreError::sqlite("prune AI request log"))?;
    conn.execute(
        "DELETE FROM ai_request WHERE id <= (SELECT id FROM ai_request ORDER BY id DESC
                                             LIMIT 1 OFFSET ?1)",
        [i64::from(AI_REQUEST_MAX_ENTRIES)],
    )
    .map_err(StoreError::sqlite("prune AI request log"))?;
    Ok(AiRequestEntry {
        id: AiRequestId(id),
        at: entry.at,
        function: entry.function,
        profile_name: entry.profile_name.clone(),
        host: entry.host.clone(),
        locality: entry.locality,
        model: entry.model.clone(),
        mails: entry.mails,
        bytes: entry.bytes,
        outcome: entry.outcome,
        error: entry.error,
        duration_ms: entry.duration_ms,
    })
}

fn addresses_json(text: Option<String>) -> Vec<Address> {
    text.and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

const OPEN_REPLY_ROWS: &str = "
SELECT m.id, a.uuid, m.thread_id, m.message_id_hdr, m.in_reply_to, m.references_hdr, m.date,
       m.subject, m.from_name, m.from_addr, m.to_addrs, m.cc_addrs,
       (m.list_id IS NOT NULL OR m.list_unsubscribe IS NOT NULL), m.auto_submitted,
       m.precedence, m.category, m.preview,
       EXISTS (SELECT 1 FROM message_location l JOIN mailbox b ON b.id = l.mailbox_id
               WHERE l.message_id = m.id AND b.role = 'sent'),
       EXISTS (SELECT 1 FROM message_location l JOIN mailbox b ON b.id = l.mailbox_id
               WHERE l.message_id = m.id AND b.role = 'inbox'),
       NOT EXISTS (SELECT 1 FROM message_location l JOIN mailbox b ON b.id = l.mailbox_id
                   WHERE l.message_id = m.id AND coalesce(b.role, '') NOT IN ('trash', 'junk')),
       EXISTS (SELECT 1 FROM message_location l WHERE l.message_id = m.id AND l.answered = 1),
       EXISTS (SELECT 1 FROM open_reply_dismissal d WHERE d.message_id = m.id)
FROM message m JOIN account a ON a.id = m.account_id
WHERE m.date >= ?1
ORDER BY m.date DESC, m.id DESC
LIMIT ?2";

fn open_reply_row(r: &Row<'_>) -> rusqlite::Result<OpenReplyRow> {
    let from_name: Option<String> = r.get(8)?;
    let from_addr: Option<String> = r.get(9)?;
    let references: Option<String> = r.get(5)?;
    Ok(OpenReplyRow {
        id: MessageId(r.get(0)?),
        account: account_id_at(r, 1)?,
        thread_id: r.get(2)?,
        message_id_hdr: r.get(3)?,
        in_reply_to: r.get(4)?,
        references: references
            .map(|text| text.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
        date: r.get(6)?,
        subject: r.get(7)?,
        from: from_addr.map(|email| Address {
            name: from_name,
            email,
        }),
        to: addresses_json(r.get(10)?),
        cc: addresses_json(r.get(11)?),
        list: r.get(12)?,
        auto_submitted: r.get(13)?,
        precedence: r.get(14)?,
        category: r.get(15)?,
        preview: r.get(16)?,
        in_sent: r.get(17)?,
        in_inbox: r.get(18)?,
        only_trash: r.get(19)?,
        answered: r.get(20)?,
        dismissed: r.get(21)?,
    })
}

impl Store {
    /// Writes one entry of the request log and prunes it (30 days, 10,000 entries).
    pub async fn record_ai_request(
        &self,
        entry: NewAiRequest,
        now: i64,
    ) -> Result<AiRequestEntry, StoreError> {
        self.write(move |tx| record(tx, &entry, now)).await
    }

    /// The request log, newest first, entries with an id below `before`. `limit` 1..=500.
    pub async fn ai_requests(
        &self,
        before: Option<AiRequestId>,
        limit: u32,
    ) -> Result<AiRequestPage, StoreError> {
        if limit == 0 || limit > MAX_AI_REQUEST_PAGE {
            return Err(StoreError::InvalidArgument(format!(
                "the limit {limit} is outside 1..={MAX_AI_REQUEST_PAGE}"
            )));
        }
        self.read(move |conn| {
            let mut entries = conn
                .prepare_cached(&format!(
                    "SELECT {ENTRY_COLUMNS} FROM ai_request WHERE id < ?1
                     ORDER BY id DESC LIMIT ?2"
                ))?
                .query_map(
                    params![before.map_or(i64::MAX, |id| id.0), i64::from(limit) + 1],
                    entry_at,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let more = entries.len() > usize::try_from(limit).unwrap_or(usize::MAX);
            entries.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            let next_before = more.then(|| entries.last().map(|e| e.id)).flatten();
            Ok(AiRequestPage {
                entries,
                next_before,
            })
        })
        .await
    }

    /// Messages dated at or after `since` (Unix seconds), newest first, at most `limit`, as the
    /// open-reply rules need them.
    pub async fn open_reply_rows(
        &self,
        since: i64,
        limit: u32,
    ) -> Result<Vec<OpenReplyRow>, StoreError> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(OPEN_REPLY_ROWS)?
                .query_map(params![since, i64::from(limit)], open_reply_row)?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// The addresses of every account: its own address and those of its identities, lower
    /// case.
    pub async fn own_addresses_by_account(
        &self,
    ) -> Result<Vec<(AccountId, Vec<String>)>, StoreError> {
        self.read(|conn| {
            let rows = conn
                .prepare_cached(
                    "SELECT a.uuid, lower(a.email) FROM account a
                     UNION
                     SELECT a.uuid, lower(i.email) FROM identity i JOIN account a ON a.id = i.account_id
                     ORDER BY 1, 2",
                )?
                .query_map([], |r| Ok((account_id_at(r, 0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut out: Vec<(AccountId, Vec<String>)> = Vec::new();
            for (account, email) in rows {
                match out.last_mut() {
                    Some((last, list)) if *last == account => list.push(email),
                    _ => out.push((account, vec![email])),
                }
            }
            Ok(out)
        })
        .await
    }

    /// Dismisses the open-reply hint of `message`. `false`: the message does not exist.
    pub async fn dismiss_open_reply(
        &self,
        message: MessageId,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.write(move |tx| {
            let exists: bool = tx
                .prepare_cached("SELECT EXISTS (SELECT 1 FROM message WHERE id = ?1)")?
                .query_row([message.0], |r| r.get(0))?;
            if !exists {
                return Ok(false);
            }
            tx.execute(
                "INSERT INTO open_reply_dismissal (message_id, dismissed_at) VALUES (?1, ?2)
                 ON CONFLICT (message_id) DO NOTHING",
                params![message.0, now],
            )
            .map_err(StoreError::sqlite("dismiss open-reply hint"))?;
            Ok(true)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::StoreOptions;
    use crate::test_support::db_path;

    fn entry(at: i64, outcome: AiRequestOutcome) -> NewAiRequest {
        NewAiRequest {
            at,
            function: AiFunction::Summary,
            profile_name: "Ollama".into(),
            host: "localhost:11434".into(),
            locality: AiLocality::ThisDevice,
            model: "llama3.2".into(),
            mails: 3,
            bytes: 1234,
            outcome,
            error: (outcome == AiRequestOutcome::Failed).then_some(ErrorCode::AiUnreachable),
            duration_ms: 42,
        }
    }

    #[tokio::test]
    async fn open_reply_rows_carry_what_the_rules_need() {
        use crate::test_support::{UV, fixture, header};
        let f = fixture().await;
        const D: i64 = 1_795_000_000;
        let mut asked = header(1, D, Some("q1@x"));
        asked.subject = "Lunch on Monday?".into();
        asked.from = vec![Address {
            name: Some("Bob".into()),
            email: "bob@example.org".into(),
        }];
        asked.to = vec![Address {
            name: None,
            email: "alice@example.org".into(),
        }];
        asked.flags.answered = true;
        let mut list = header(2, D + 60, Some("l1@x"));
        list.list_id = Some("news.example".into());
        list.auto_submitted = Some("auto-generated".into());
        list.precedence = Some("bulk".into());
        f.store
            .store_headers(f.inbox, UV, vec![asked, list], None)
            .await
            .expect("inbox");
        let mut reply = header(1, D + 120, Some("r1@x"));
        reply.in_reply_to = Some("q1@x".into());
        reply.references = vec!["q1@x".into()];
        reply.from = vec![Address {
            name: None,
            email: "alice@example.org".into(),
        }];
        f.store
            .store_headers(f.sent, UV, vec![reply], None)
            .await
            .expect("sent");
        // Older than `since`: left out.
        let rows = f.store.open_reply_rows(D, 100).await.expect("rows");
        assert_eq!(rows.len(), 3);
        let by_hdr = |id: &str| {
            rows.iter()
                .find(|r| r.message_id_hdr.as_deref() == Some(id))
                .expect("row")
        };
        let asked = by_hdr("q1@x");
        assert!(asked.in_inbox && !asked.in_sent && asked.answered && !asked.only_trash);
        assert_eq!(asked.to[0].email, "alice@example.org");
        assert_eq!(
            asked.from.as_ref().map(|a| a.email.as_str()),
            Some("bob@example.org")
        );
        assert!(!asked.list && !asked.dismissed);
        let list = by_hdr("l1@x");
        assert!(list.list);
        assert_eq!(list.auto_submitted.as_deref(), Some("auto-generated"));
        assert_eq!(list.precedence.as_deref(), Some("bulk"));
        let reply = by_hdr("r1@x");
        assert!(reply.in_sent && !reply.in_inbox);
        assert_eq!(reply.in_reply_to.as_deref(), Some("q1@x"));
        assert_eq!(reply.references, vec!["q1@x".to_owned()]);
        // Newest first.
        assert_eq!(rows[0].message_id_hdr.as_deref(), Some("r1@x"));
        assert_eq!(
            f.store
                .open_reply_rows(D + 61, 100)
                .await
                .expect("rows")
                .len(),
            1
        );

        assert!(
            f.store
                .dismiss_open_reply(asked.id, 5)
                .await
                .expect("dismiss")
        );
        assert!(
            f.store
                .dismiss_open_reply(asked.id, 6)
                .await
                .expect("again")
        );
        assert!(
            !f.store
                .dismiss_open_reply(MessageId(999_999), 7)
                .await
                .expect("missing")
        );
        let rows = f.store.open_reply_rows(D, 100).await.expect("rows");
        assert!(
            rows.iter()
                .find(|r| r.id == asked.id)
                .expect("row")
                .dismissed
        );

        let own = f.store.own_addresses_by_account().await.expect("own");
        assert_eq!(own, vec![(f.account, vec!["alice@example.org".to_owned()])]);
    }

    #[tokio::test]
    async fn the_request_log_pages_and_prunes() {
        let dir = TempDir::new().expect("temp dir");
        let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("store");
        let now = 100 * DAY_MS;
        store
            .record_ai_request(
                entry(now - 31 * DAY_MS, AiRequestOutcome::Ok),
                now - 31 * DAY_MS,
            )
            .await
            .expect("old");
        for i in 0..3 {
            store
                .record_ai_request(entry(now + i, AiRequestOutcome::Failed), now)
                .await
                .expect("record");
        }
        let first = store.ai_requests(None, 2).await.expect("page");
        assert_eq!(first.entries.len(), 2);
        assert_eq!(first.entries[0].error, Some(ErrorCode::AiUnreachable));
        assert_eq!(first.entries[0].outcome, AiRequestOutcome::Failed);
        assert_eq!(first.entries[0].host, "localhost:11434");
        let second = store.ai_requests(first.next_before, 2).await.expect("page");
        // The 31 days old entry is gone.
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.next_before, None);
        assert!(store.ai_requests(None, 0).await.is_err());
    }
}
