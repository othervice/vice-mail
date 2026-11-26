//! Stage 15: what the diagnostics export learns from the database. Numbers and fixed names only:
//! row counts per table, page statistics, outbox states and error codes, and per account the
//! number of folders, messages and stored bodies. No address, name, subject, folder name or
//! other text a person wrote ever leaves this module.

use mail_model::AccountId;
use rusqlite::Connection;

use crate::{Store, StoreError};

/// Counts of one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStats {
    pub account: AccountId,
    pub mailboxes: i64,
    /// Folders without a role (named by the user or the server).
    pub custom_mailboxes: i64,
    /// Folders whose first sync is complete.
    pub synced_mailboxes: i64,
    pub messages: i64,
    /// Messages with a stored body (complete or partial).
    pub bodies: i64,
    /// The last login saw Gmail's extension (stage 14).
    pub gmail: bool,
}

/// Everything the diagnostics export reads from the database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseStats {
    pub page_size: i64,
    pub page_count: i64,
    pub freelist_count: i64,
    /// Ordinary tables of the schema and their row counts, by name. Names are the schema's own
    /// (`[a-z0-9_]`); full-text tables and their shadow tables are left out.
    pub tables: Vec<(String, i64)>,
    /// Outbox operations per state (`queued`, `in_flight`, `confirmed`, `failed`, `conflict`).
    pub operations: Vec<(String, i64)>,
    /// Failed or conflicting operations per error code of their last error.
    pub operation_errors: Vec<(String, i64)>,
    /// In the user's order, like [`Store::accounts`] (stage 22, decision 0078).
    pub accounts: Vec<AccountStats>,
}

impl Store {
    /// Counts for the diagnostics export (stage 15). Reads only; numbers and schema names only.
    pub async fn diagnostics_stats(&self) -> Result<DatabaseStats, StoreError> {
        self.read(stats).await
    }
}

fn stats(conn: &Connection) -> Result<DatabaseStats, StoreError> {
    let pragma = |name: &str| -> Result<i64, StoreError> {
        Ok(conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))?)
    };
    Ok(DatabaseStats {
        page_size: pragma("page_size")?,
        page_count: pragma("page_count")?,
        freelist_count: pragma("freelist_count")?,
        tables: table_counts(conn)?,
        operations: grouped(
            conn,
            "SELECT state, count(*) FROM operation GROUP BY state ORDER BY state",
        )?,
        operation_errors: grouped(
            conn,
            "SELECT coalesce(json_extract(last_error, '$.code'), 'none'), count(*)
             FROM operation WHERE state IN ('failed', 'conflict')
             GROUP BY 1 ORDER BY 1",
        )?,
        accounts: account_stats(conn)?,
    })
}

/// Whether `name` is a plain schema name (what our migrations create).
fn is_schema_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn table_counts(conn: &Connection) -> Result<Vec<(String, i64)>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT name, coalesce(sql, '') FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
    )?;
    let all = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let virtual_tables: Vec<&str> = all
        .iter()
        .filter(|(_, sql)| {
            sql.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL TABLE")
        })
        .map(|(name, _)| name.as_str())
        .collect();
    let mut counts = Vec::new();
    for (name, _) in &all {
        let shadow = virtual_tables
            .iter()
            .any(|v| name == v || name.starts_with(&format!("{v}_")));
        if shadow || !is_schema_name(name) {
            continue;
        }
        let count: i64 = conn.query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |r| {
            r.get(0)
        })?;
        counts.push((name.clone(), count));
    }
    Ok(counts)
}

/// Rows of `(text, count)`; texts that are not plain codes become `other`.
fn grouped(conn: &Connection, sql: &str) -> Result<Vec<(String, i64)>, StoreError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| {
            let key: Option<String> = r.get(0)?;
            Ok((key.unwrap_or_default(), r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for (key, count) in rows {
        let key = if is_schema_name(&key) {
            key
        } else {
            "other".to_owned()
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += count,
            None => out.push((key, count)),
        }
    }
    Ok(out)
}

fn account_stats(conn: &Connection) -> Result<Vec<AccountStats>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT a.uuid,
           (SELECT count(*) FROM mailbox b WHERE b.account_id = a.id),
           (SELECT count(*) FROM mailbox b WHERE b.account_id = a.id AND b.role IS NULL),
           (SELECT count(*) FROM mailbox b WHERE b.account_id = a.id AND b.sync_low_uid = 1),
           (SELECT count(*) FROM message m WHERE m.account_id = a.id),
           (SELECT count(*) FROM message m JOIN message_body mb ON mb.message_id = m.id
              WHERE m.account_id = a.id),
           coalesce((SELECT s.gmail FROM account_server s WHERE s.account_id = a.id), 0)
         FROM account a ORDER BY a.position IS NULL, a.position, a.created_at, a.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(uuid, mailboxes, custom, synced, messages, bodies, gmail)| {
                let account = uuid.parse::<AccountId>().map_err(|_| {
                    StoreError::Config(format!("account row with an invalid id {uuid:?}"))
                })?;
                Ok(AccountStats {
                    account,
                    mailboxes,
                    custom_mailboxes: custom,
                    synced_mailboxes: synced,
                    messages,
                    bodies,
                    gmail: gmail != 0,
                })
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use mail_model::{RemoteMailbox, Security, ServerSettings};
    use tempfile::TempDir;

    use super::*;
    use crate::{NewAccount, StoreOptions};

    #[tokio::test]
    async fn counts_tables_accounts_and_outbox_without_any_text() {
        let dir = TempDir::new().expect("temp dir");
        let store =
            Store::open(&dir.path().join("mail.sqlite3"), StoreOptions::default()).expect("open");
        let empty = store.diagnostics_stats().await.expect("stats");
        assert!(empty.page_size > 0 && empty.page_count > 0);
        assert!(empty.accounts.is_empty() && empty.operations.is_empty());
        let names: Vec<&str> = empty.tables.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"account") && names.contains(&"message"));
        assert!(
            !names.iter().any(|n| n.starts_with("message_fts")),
            "full-text tables are left out: {names:?}"
        );

        let server = ServerSettings {
            host: "imap.example.org".into(),
            port: 993,
            security: Security::Tls,
            username: "alice".into(),
        };
        let id: AccountId = "00000000-0000-4000-8000-000000000001".parse().expect("id");
        store
            .insert_account(NewAccount {
                id,
                email: "alice@example.org".into(),
                display_name: "Alice".into(),
                imap: server.clone(),
                smtp: server,
                credential_ref: id.to_string(),
            })
            .await
            .expect("account");
        let remote = |name: &str, attrs: &[&str]| RemoteMailbox {
            name: name.into(),
            path: name.into(),
            delimiter: Some('/'),
            attributes: attrs.iter().map(|a| (*a).to_owned()).collect(),
            status: None,
        };
        store
            .sync_mailboxes(id, vec![remote("INBOX", &[]), remote("Steuer 2026", &[])])
            .await
            .expect("mailboxes");

        let stats = store.diagnostics_stats().await.expect("stats");
        assert_eq!(
            stats.accounts,
            vec![AccountStats {
                account: id,
                mailboxes: 2,
                custom_mailboxes: 1,
                synced_mailboxes: 0,
                messages: 0,
                bodies: 0,
                gmail: false,
            }]
        );
        let count = |table: &str| {
            stats
                .tables
                .iter()
                .find(|(n, _)| n == table)
                .map(|(_, c)| *c)
        };
        assert_eq!(count("account"), Some(1));
        assert_eq!(count("mailbox"), Some(2));
        assert_eq!(count("identity"), Some(1));
        let text = format!("{stats:?}");
        assert!(
            !text.contains("alice") && !text.contains("Steuer"),
            "{text}"
        );
    }

    #[test]
    fn only_plain_schema_names_pass() {
        assert!(is_schema_name("message_location"));
        assert!(is_schema_name("auth_failed"));
        for bad in ["", "Alice", "a b", "x;drop", "ä", &"a".repeat(65)] {
            assert!(!is_schema_name(bad), "{bad}");
        }
    }
}
