//! Stage 12: AI access over MCP (migration 0008, decision 0021). The master switch, the access
//! level of shared accounts, the registered clients (the keys are in the keychain; here only
//! the reference) and the access log with its retention. Every table is class Gerät.

use mail_model::{
    AccountId, MAX_MCP_ACCESS_PAGE, MAX_MCP_CLIENTS, MCP_ACCESS_MAX_ENTRIES,
    MCP_ACCESS_RETENTION_DAYS, McpAccessEntry, McpAccessId, McpAccessOutcome, McpAccessPage,
    McpAccountAccess, McpClientInfo, McpRefusal, McpTool,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::accounts::{self, account_id_at};
use crate::{Store, StoreError};

/// A registered (or revoked) AI client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMcpClient {
    /// 32 lowercase hex digits.
    pub client_id: String,
    pub name: String,
    /// The keychain entry of the client's key.
    pub credential_ref: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

impl StoredMcpClient {
    pub fn info(&self) -> McpClientInfo {
        McpClientInfo {
            client_id: self.client_id.clone(),
            name: self.name.clone(),
            created_at: self.created_at,
            revoked_at: self.revoked_at,
            last_used_at: self.last_used_at,
        }
    }

    pub fn active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// Everything stored about AI access except the log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredMcpState {
    pub enabled: bool,
    /// The Windows pipe name, once made.
    pub endpoint: Option<String>,
    /// Shared accounts (never `hidden`), in account order.
    pub accounts: Vec<(AccountId, McpAccountAccess)>,
    /// Every client, newest first.
    pub clients: Vec<StoredMcpClient>,
}

/// One entry for [`Store::record_mcp_access`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMcpAccess {
    pub at: i64,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub tool: Option<McpTool>,
    pub messages: u32,
    pub outcome: McpAccessOutcome,
    pub refusal: Option<McpRefusal>,
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const CLIENT_COLUMNS: &str =
    "client_id, name, credential_ref, created_at, revoked_at, last_used_at";

fn client_at(r: &Row<'_>) -> rusqlite::Result<StoredMcpClient> {
    Ok(StoredMcpClient {
        client_id: r.get(0)?,
        name: r.get(1)?,
        credential_ref: r.get(2)?,
        created_at: r.get(3)?,
        revoked_at: r.get(4)?,
        last_used_at: r.get(5)?,
    })
}

const fn access_text(access: McpAccountAccess) -> Option<&'static str> {
    match access {
        McpAccountAccess::Hidden => None,
        McpAccountAccess::Read => Some("read"),
        McpAccountAccess::ReadAndDrafts => Some("read_and_drafts"),
    }
}

fn access_of(text: &str) -> McpAccountAccess {
    match text {
        "read" => McpAccountAccess::Read,
        "read_and_drafts" => McpAccountAccess::ReadAndDrafts,
        // The CHECK allows nothing else; anything unknown is the safe default.
        _ => McpAccountAccess::Hidden,
    }
}

const fn outcome_text(outcome: McpAccessOutcome) -> &'static str {
    match outcome {
        McpAccessOutcome::Ok => "ok",
        McpAccessOutcome::Error => "error",
        McpAccessOutcome::Refused => "refused",
    }
}

fn outcome_of(text: &str) -> McpAccessOutcome {
    match text {
        "ok" => McpAccessOutcome::Ok,
        "error" => McpAccessOutcome::Error,
        _ => McpAccessOutcome::Refused,
    }
}

fn refusal_text(refusal: McpRefusal) -> Option<String> {
    serde_json::to_value(refusal)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn refusal_of(text: &str) -> Option<McpRefusal> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

fn entry_at(r: &Row<'_>) -> rusqlite::Result<McpAccessEntry> {
    let tool: Option<String> = r.get(4)?;
    let messages: i64 = r.get(5)?;
    let outcome: String = r.get(6)?;
    let refusal: Option<String> = r.get(7)?;
    Ok(McpAccessEntry {
        id: McpAccessId(r.get(0)?),
        at: r.get(1)?,
        client_id: r.get(2)?,
        client_name: r.get(3)?,
        tool: tool.as_deref().and_then(McpTool::parse),
        messages: u32::try_from(messages).unwrap_or(u32::MAX),
        outcome: outcome_of(&outcome),
        refusal: refusal.as_deref().and_then(refusal_of),
    })
}

const ENTRY_COLUMNS: &str = "id, at, client_id, client_name, tool, messages, outcome, refusal";

pub(crate) fn state(conn: &Connection) -> Result<StoredMcpState, StoreError> {
    let (enabled, endpoint) = conn
        .prepare_cached("SELECT enabled, endpoint FROM mcp_state WHERE id = 1")?
        .query_row([], |r| {
            Ok((r.get::<_, bool>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .optional()?
        .unwrap_or((false, None));
    let accounts = conn
        .prepare_cached(
            "SELECT a.uuid, m.access FROM mcp_account m JOIN account a ON a.id = m.account_id
             ORDER BY a.id",
        )?
        .query_map([], |r| {
            let access: String = r.get(1)?;
            Ok((account_id_at(r, 0)?, access_of(&access)))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let clients = clients(conn)?;
    Ok(StoredMcpState {
        enabled,
        endpoint,
        accounts: accounts
            .into_iter()
            .filter(|(_, access)| *access != McpAccountAccess::Hidden)
            .collect(),
        clients,
    })
}

pub(crate) fn clients(conn: &Connection) -> Result<Vec<StoredMcpClient>, StoreError> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {CLIENT_COLUMNS} FROM mcp_client ORDER BY created_at DESC, id DESC"
        ))?
        .query_map([], client_at)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn client(conn: &Connection, client_id: &str) -> Result<Option<StoredMcpClient>, StoreError> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {CLIENT_COLUMNS} FROM mcp_client WHERE client_id = ?1"
        ))?
        .query_row([client_id], client_at)
        .optional()?)
}

pub(crate) fn set_enabled(conn: &Connection, enabled: bool, now: i64) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO mcp_state (id, enabled, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at",
        params![enabled, now],
    )
    .map_err(StoreError::sqlite("set MCP switch"))?;
    Ok(())
}

pub(crate) fn set_endpoint(conn: &Connection, endpoint: &str, now: i64) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO mcp_state (id, enabled, endpoint, updated_at) VALUES (1, 0, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET endpoint = excluded.endpoint, updated_at = excluded.updated_at",
        params![endpoint, now],
    )
    .map_err(StoreError::sqlite("store MCP endpoint"))?;
    Ok(())
}

pub(crate) fn set_access(
    conn: &Connection,
    account: AccountId,
    access: McpAccountAccess,
    now: i64,
) -> Result<(), StoreError> {
    let row = accounts::row_id(conn, account)?;
    match access_text(access) {
        Some(text) => conn.execute(
            "INSERT INTO mcp_account (account_id, access, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (account_id) DO UPDATE SET access = excluded.access,
                                                    updated_at = excluded.updated_at",
            params![row, text, now],
        ),
        None => conn.execute("DELETE FROM mcp_account WHERE account_id = ?1", [row]),
    }
    .map_err(StoreError::sqlite("set MCP account access"))?;
    Ok(())
}

pub(crate) fn insert_client(
    conn: &Connection,
    client_id: &str,
    name: &str,
    credential_ref: &str,
    now: i64,
) -> Result<StoredMcpClient, StoreError> {
    if client_id.len() != 32 || !client_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(StoreError::InvalidArgument(
            "an MCP client id is 32 hex digits".into(),
        ));
    }
    let active: i64 = conn
        .prepare_cached("SELECT count(*) FROM mcp_client WHERE revoked_at IS NULL")?
        .query_row([], |r| r.get(0))?;
    if active >= i64::from(MAX_MCP_CLIENTS) {
        return Err(StoreError::InvalidArgument(format!(
            "at most {MAX_MCP_CLIENTS} AI clients can be registered; revoke one first"
        )));
    }
    conn.execute(
        "INSERT INTO mcp_client (client_id, name, credential_ref, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![client_id, name, credential_ref, now],
    )
    .map_err(StoreError::sqlite("insert MCP client"))?;
    client(conn, client_id)?.ok_or_else(|| StoreError::NotFound(format!("MCP client {client_id}")))
}

/// The client as it was before (`None`: unknown) and whether this call revoked it.
pub(crate) fn revoke_client(
    conn: &Connection,
    client_id: &str,
    now: i64,
) -> Result<Option<(StoredMcpClient, bool)>, StoreError> {
    let Some(before) = client(conn, client_id)? else {
        return Ok(None);
    };
    let changed = conn
        .execute(
            "UPDATE mcp_client SET revoked_at = ?2 WHERE client_id = ?1 AND revoked_at IS NULL",
            params![client_id, now],
        )
        .map_err(StoreError::sqlite("revoke MCP client"))?;
    Ok(Some((before, changed > 0)))
}

pub(crate) fn delete_client(conn: &Connection, client_id: &str) -> Result<bool, StoreError> {
    Ok(conn
        .execute("DELETE FROM mcp_client WHERE client_id = ?1", [client_id])
        .map_err(StoreError::sqlite("delete MCP client"))?
        > 0)
}

pub(crate) fn record(
    conn: &Connection,
    entries: &[NewMcpAccess],
    now: i64,
) -> Result<Vec<McpAccessEntry>, StoreError> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        conn.prepare_cached(
            "INSERT INTO mcp_access (at, client_id, client_name, tool, messages, outcome, refusal)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            entry.at,
            entry.client_id,
            entry.client_name,
            entry.tool.map(McpTool::as_str),
            i64::from(entry.messages),
            outcome_text(entry.outcome),
            entry.refusal.and_then(refusal_text),
        ])
        .map_err(StoreError::sqlite("record MCP access"))?;
        let id = conn.last_insert_rowid();
        if let Some(client_id) = &entry.client_id {
            conn.prepare_cached(
                "UPDATE mcp_client SET last_used_at = max(coalesce(last_used_at, 0), ?2)
                 WHERE client_id = ?1",
            )?
            .execute(params![client_id, entry.at])?;
        }
        out.push(McpAccessEntry {
            id: McpAccessId(id),
            at: entry.at,
            client_id: entry.client_id.clone(),
            client_name: entry.client_name.clone(),
            tool: entry.tool,
            messages: entry.messages,
            outcome: entry.outcome,
            refusal: entry.refusal,
        });
    }
    prune(conn, now)?;
    Ok(out)
}

/// Retention: older than [`MCP_ACCESS_RETENTION_DAYS`], or beyond the newest
/// [`MCP_ACCESS_MAX_ENTRIES`] (ids grow by one per entry, so the cut is an id range).
pub(crate) fn prune(conn: &Connection, now: i64) -> Result<usize, StoreError> {
    let cutoff = now.saturating_sub(i64::from(MCP_ACCESS_RETENTION_DAYS) * DAY_MS);
    let mut removed = conn
        .prepare_cached("DELETE FROM mcp_access WHERE at < ?1")?
        .execute([cutoff])?;
    let newest: Option<i64> = conn
        .prepare_cached("SELECT max(id) FROM mcp_access")?
        .query_row([], |r| r.get(0))?;
    if let Some(newest) = newest {
        removed += conn
            .prepare_cached("DELETE FROM mcp_access WHERE id <= ?1")?
            .execute([newest - i64::from(MCP_ACCESS_MAX_ENTRIES)])?;
    }
    Ok(removed)
}

pub(crate) fn page(
    conn: &Connection,
    before: Option<McpAccessId>,
    limit: u32,
) -> Result<McpAccessPage, StoreError> {
    if limit == 0 || limit > MAX_MCP_ACCESS_PAGE {
        return Err(StoreError::InvalidArgument(format!(
            "the limit {limit} is outside 1..={MAX_MCP_ACCESS_PAGE}"
        )));
    }
    let mut entries = conn
        .prepare_cached(&format!(
            "SELECT {ENTRY_COLUMNS} FROM mcp_access WHERE id < ?1 ORDER BY id DESC LIMIT ?2"
        ))?
        .query_map(
            params![before.map_or(i64::MAX, |id| id.0), i64::from(limit) + 1],
            entry_at,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let more = entries.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    entries.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = if more {
        entries.last().map(|e| e.id)
    } else {
        None
    };
    Ok(McpAccessPage {
        entries,
        next_cursor,
    })
}

impl Store {
    /// The master switch, the stored endpoint, the shared accounts and every client.
    pub async fn mcp_state(&self) -> Result<StoredMcpState, StoreError> {
        self.read(state).await
    }

    pub async fn mcp_clients(&self) -> Result<Vec<StoredMcpClient>, StoreError> {
        self.read(clients).await
    }

    pub async fn set_mcp_enabled(&self, enabled: bool, now: i64) -> Result<(), StoreError> {
        self.write(move |tx| set_enabled(tx, enabled, now)).await
    }

    /// Stores the Windows pipe name (made once, used on every start).
    pub async fn set_mcp_endpoint(&self, endpoint: String, now: i64) -> Result<(), StoreError> {
        self.write(move |tx| set_endpoint(tx, &endpoint, now)).await
    }

    /// `hidden` removes the share. [`StoreError::NotFound`] for an unknown account.
    pub async fn set_mcp_account_access(
        &self,
        account: AccountId,
        access: McpAccountAccess,
        now: i64,
    ) -> Result<(), StoreError> {
        self.write(move |tx| set_access(tx, account, access, now))
            .await
    }

    /// Registers a client (its key must already be in the keychain under `credential_ref`).
    /// `invalid_argument` beyond [`MAX_MCP_CLIENTS`] active clients.
    pub async fn insert_mcp_client(
        &self,
        client_id: String,
        name: String,
        credential_ref: String,
        now: i64,
    ) -> Result<StoredMcpClient, StoreError> {
        self.write(move |tx| insert_client(tx, &client_id, &name, &credential_ref, now))
            .await
    }

    /// Marks the client revoked. Returns it as it was and whether this call revoked it;
    /// `None` for an unknown client.
    pub async fn revoke_mcp_client(
        &self,
        client_id: String,
        now: i64,
    ) -> Result<Option<(StoredMcpClient, bool)>, StoreError> {
        self.write(move |tx| revoke_client(tx, &client_id, now))
            .await
    }

    /// Removes a client row (only to undo a registration whose keychain step failed).
    pub async fn delete_mcp_client(&self, client_id: String) -> Result<bool, StoreError> {
        self.write(move |tx| delete_client(tx, &client_id)).await
    }

    /// Appends access log entries in one transaction, notes the clients' last use and prunes
    /// the log (30 days, 10,000 entries). Returns the entries with their ids.
    pub async fn record_mcp_access(
        &self,
        entries: Vec<NewMcpAccess>,
        now: i64,
    ) -> Result<Vec<McpAccessEntry>, StoreError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        self.write(move |tx| record(tx, &entries, now)).await
    }

    /// The access log, newest first, below `before` (an entry id). `limit` 1..=500.
    pub async fn mcp_access(
        &self,
        before: Option<McpAccessId>,
        limit: u32,
    ) -> Result<McpAccessPage, StoreError> {
        self.read(move |c| page(c, before, limit)).await
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::StoreOptions;
    use crate::test_support::{account_id, db_path, new_account};

    async fn store() -> (TempDir, Store) {
        let dir = TempDir::new().expect("temp dir");
        let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
        (dir, store)
    }

    fn access(at: i64, client: Option<&str>, tool: Option<McpTool>) -> NewMcpAccess {
        NewMcpAccess {
            at,
            client_id: client.map(str::to_owned),
            client_name: client.map(|_| "Claude".to_owned()),
            tool,
            messages: 3,
            outcome: if tool.is_some() {
                McpAccessOutcome::Ok
            } else {
                McpAccessOutcome::Refused
            },
            refusal: tool.is_none().then_some(McpRefusal::BadProof),
        }
    }

    #[tokio::test]
    async fn switch_shares_and_clients_round_trip() {
        let (_dir, store) = store().await;
        let initial = store.mcp_state().await.expect("state");
        assert_eq!(
            initial,
            StoredMcpState::default(),
            "off and empty after the migration"
        );

        store
            .insert_account(new_account(1, "a@example.org"))
            .await
            .expect("account 1");
        store
            .insert_account(new_account(2, "b@example.org"))
            .await
            .expect("account 2");
        store.set_mcp_enabled(true, 5).await.expect("on");
        store
            .set_mcp_endpoint(r"\\.\pipe\mail-mcp-00".into(), 6)
            .await
            .expect("endpoint");
        for (n, access) in [
            (1, McpAccountAccess::ReadAndDrafts),
            (2, McpAccountAccess::Read),
            (2, McpAccountAccess::Hidden),
            (2, McpAccountAccess::Read),
        ] {
            store
                .set_mcp_account_access(account_id(n), access, 7)
                .await
                .expect("share");
        }
        let unknown = store
            .set_mcp_account_access(account_id(9), McpAccountAccess::Read, 7)
            .await;
        assert!(
            matches!(unknown, Err(StoreError::NotFound(_))),
            "{unknown:?}"
        );

        let a = "a".repeat(32);
        let b = "b".repeat(32);
        store
            .insert_mcp_client(a.clone(), "Claude".into(), format!("mcp-client:{a}"), 10)
            .await
            .expect("client a");
        store
            .insert_mcp_client(b.clone(), "Cursor".into(), format!("mcp-client:{b}"), 20)
            .await
            .expect("client b");
        let bad = store
            .insert_mcp_client("xyz".into(), "Bad".into(), "r".into(), 1)
            .await;
        assert!(matches!(bad, Err(StoreError::InvalidArgument(_))));

        let state = store.mcp_state().await.expect("state");
        assert!(state.enabled);
        assert_eq!(state.endpoint.as_deref(), Some(r"\\.\pipe\mail-mcp-00"));
        assert_eq!(
            state.accounts,
            [
                (account_id(1), McpAccountAccess::ReadAndDrafts),
                (account_id(2), McpAccountAccess::Read)
            ]
        );
        let names: Vec<&str> = state.clients.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Cursor", "Claude"], "newest first");

        let (before, revoked) = store
            .revoke_mcp_client(a.clone(), 30)
            .await
            .expect("revoke")
            .expect("known");
        assert!(revoked && before.active());
        let (_, again) = store
            .revoke_mcp_client(a.clone(), 40)
            .await
            .expect("revoke")
            .expect("known");
        assert!(!again, "revoking twice changes nothing");
        assert!(
            store
                .revoke_mcp_client("c".repeat(32), 40)
                .await
                .expect("revoke")
                .is_none()
        );
        let clients = store.mcp_clients().await.expect("clients");
        let revoked_a = clients.iter().find(|c| c.client_id == a).expect("a");
        assert_eq!(revoked_a.revoked_at, Some(30));

        // Removing an account removes its share.
        store.delete_account(account_id(1)).await.expect("delete");
        let state = store.mcp_state().await.expect("state");
        assert_eq!(state.accounts, [(account_id(2), McpAccountAccess::Read)]);
        store.set_mcp_enabled(false, 50).await.expect("off");
        assert!(!store.mcp_state().await.expect("state").enabled);
    }

    #[tokio::test]
    async fn the_access_log_pages_counts_last_use_and_prunes() {
        let (_dir, store) = store().await;
        let a = "a".repeat(32);
        store
            .insert_mcp_client(a.clone(), "Claude".into(), "ref".into(), 1)
            .await
            .expect("client");
        let now = 100 * DAY_MS;
        let old = access(now - 31 * DAY_MS, Some(&a), Some(McpTool::ReadThread));
        let written = store
            .record_mcp_access(
                vec![
                    access(now - 2, Some(&a), Some(McpTool::SearchMessages)),
                    access(now - 1, None, None),
                    old,
                ],
                now,
            )
            .await
            .expect("record");
        assert_eq!(written.len(), 3);
        assert!(written[0].id < written[1].id);
        assert_eq!(written[1].refusal, Some(McpRefusal::BadProof));
        let page = store.mcp_access(None, 500).await.expect("page");
        assert_eq!(page.entries.len(), 2, "the 31 days old entry is pruned");
        assert_eq!(page.entries[0].tool, None);
        assert_eq!(page.entries[1].tool, Some(McpTool::SearchMessages));
        assert_eq!(page.entries[1].messages, 3);
        assert_eq!(page.entries[1].client_name.as_deref(), Some("Claude"));
        let clients = store.mcp_clients().await.expect("clients");
        assert_eq!(clients[0].last_used_at, Some(now - 2));

        // Paging.
        let first = store.mcp_access(None, 1).await.expect("page 1");
        assert_eq!(first.entries.len(), 1);
        let second = store
            .mcp_access(first.next_cursor, 1)
            .await
            .expect("page 2");
        assert_eq!(second.entries.len(), 1);
        assert!(second.entries[0].id < first.entries[0].id);
        assert_eq!(second.next_cursor, None);
        assert!(store.mcp_access(None, 0).await.is_err());
        assert!(store.mcp_access(None, 501).await.is_err());

        // At most 10,000 entries.
        let many: Vec<NewMcpAccess> = (0..10_050)
            .map(|n| access(now + n, Some(&a), Some(McpTool::ListAccounts)))
            .collect();
        store
            .record_mcp_access(many, now + 10_050)
            .await
            .expect("record many");
        let count = store
            .read(|c| {
                Ok(c.query_row("SELECT count(*) FROM mcp_access", [], |r| {
                    r.get::<_, i64>(0)
                })?)
            })
            .await
            .expect("count");
        assert_eq!(count, i64::from(MCP_ACCESS_MAX_ENTRIES));
        let newest = store.mcp_access(None, 1).await.expect("newest");
        assert_eq!(newest.entries[0].at, now + 10_049, "the newest stay");
    }
}
