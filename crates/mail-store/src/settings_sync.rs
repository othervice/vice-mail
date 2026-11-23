//! Stage 11: the local side of the settings sync (`docs/SYNC.md`, decisions 0014 and 0022).
//!
//! **Capturing changes, in one place.** Triggers on every synchronised table mark the changed
//! records in `sync_dirty` (migration 0009). At the end of every write transaction, before it
//! commits, [`flush`] reads each marked record's content from its table and the stored
//! `sync_record` row and hands both to the engine's [`SyncCapture`] hook, which assigns versions
//! with the device's clock (`LocalState::put`, `put_fields`, `delete` of the settings sync
//! logic) and returns the new rows. So no write path needs to know about the sync, and a change
//! and its new version are committed together or not at all.
//!
//! **Applying a pass.** [`Store::apply_sync`] writes records from the sync folder into their
//! tables and `sync_record`, one savepoint per record, and only if the record did not change
//! locally since the pass read it (it is then merged by the next pass). Applied records leave
//! no mark, so applying never creates a new version; changes that follow from it (another
//! identity losing "default", a group losing a member) are captured as usual.
//!
//! The content shapes per kind are the ones `docs/SYNC.md` lists; the engine validates them.

use std::collections::BTreeSet;

use mail_model::{AccountId, MessageCategory};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::settings::now_millis;
use crate::{Store, StoreError};

/// Assigns versions to captured changes (the engine's settings sync). Runs on the writer thread
/// inside the transaction, so it must be quick and must not block.
pub trait SyncCapture: Send + Sync + 'static {
    /// Every record changed by one transaction. Returns the rows to store; a change it leaves
    /// out (unchanged content, a device setting, invalid content) changes nothing.
    fn capture(&self, changes: Vec<CapturedChange>) -> CaptureOutcome;
}

/// One `sync_record` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRecordRow {
    pub kind: String,
    pub id: String,
    /// Hybrid logical clock text (fixed width, compares as text).
    pub version: String,
    pub synced_version: Option<String>,
    /// The record's content; `None` for a tombstone.
    pub content: Option<Value>,
    /// The version a tombstone deletes; `None` for content.
    pub deleted_version: Option<String>,
}

/// A changed record: what its table holds now (`None`: the row is gone) and what the sync
/// knew before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedChange {
    pub kind: String,
    pub id: String,
    pub current: Option<Value>,
    pub stored: Option<SyncRecordRow>,
}

/// What [`SyncCapture::capture`] decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureOutcome {
    pub rows: Vec<SyncRecordRow>,
    /// The clock's state after the capture, persisted with the rows.
    pub hlc_last: Option<String>,
}

/// What to do with the local table when a record is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableWrite {
    /// Only the sync state changes (the record is in sync now).
    Keep,
    /// Write this content (plain values; accounts without field versions).
    Put(Value),
    /// The record was deleted.
    Delete,
}

/// One record a pass applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncApply {
    pub kind: String,
    pub id: String,
    /// The `sync_record` version the pass planned against (`None`: no row). If the row has
    /// another version now, the record changed locally meanwhile and is skipped.
    pub expected: Option<String>,
    /// The new row; `None` removes it (the record is forgotten here).
    pub row: Option<SyncRecordRow>,
    pub table: TableWrite,
    /// Accounts and identities only: the local record with this id is the same one (same
    /// address) and takes the id of the record from the folder first (decision 0022).
    pub rekey_from: Option<String>,
}

/// Why a record was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplySkip {
    /// It changed locally after the pass read it; the next pass merges both.
    ChangedLocally,
    /// It belongs to an account (or identity) that does not exist here (yet).
    Missing(String),
    /// The table refused it (for example an address another account uses already).
    Refused(String),
}

/// What [`Store::apply_sync`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// Records written, by kind and id.
    pub applied: Vec<(String, String)>,
    pub skipped: Vec<(String, String, ApplySkip)>,
    /// Accounts created from the folder (they have no password here yet).
    pub accounts_added: Vec<AccountId>,
    /// Existing accounts whose settings were written.
    pub accounts_updated: Vec<AccountId>,
    /// Local accounts that took the id of the folder's record: (old, new).
    pub accounts_rekeyed: Vec<(AccountId, AccountId)>,
    /// Kinds whose table changed (for the UI's events).
    pub kinds: BTreeSet<String>,
}

/// `sync_state` and the sync's own settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStateRow {
    pub device_id: String,
    pub hlc_last: Option<String>,
    pub enabled: bool,
    pub account: Option<AccountId>,
    pub folder: Option<String>,
    pub last_complete_sync_ms: Option<i64>,
    pub last_full_check_ms: Option<i64>,
    pub folder_state: Option<String>,
}

/// Fields of `sync_state` a pass updates; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncProgress {
    pub last_complete_sync_ms: Option<i64>,
    pub last_full_check_ms: Option<i64>,
    pub folder_state: Option<String>,
    pub hlc_last: Option<String>,
}

/// A device of the sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSyncDevice {
    pub device_id: String,
    pub name: String,
    pub system: String,
    pub last_seen_ms: i64,
}

// ---------------------------------------------------------------------------------------------
// Capture

/// Turns the marks of the running transaction into `sync_record` rows (see the module docs).
pub(crate) fn flush(conn: &Connection, capture: &dyn SyncCapture) -> Result<(), StoreError> {
    let marks = conn
        .prepare_cached("SELECT kind, id FROM sync_dirty")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    if marks.is_empty() {
        return Ok(());
    }
    let mut changes = Vec::with_capacity(marks.len());
    for (kind, id) in marks {
        let current = content_of(conn, &kind, &id)?;
        let stored = record(conn, &kind, &id)?;
        changes.push(CapturedChange {
            kind,
            id,
            current,
            stored,
        });
    }
    let outcome = capture.capture(changes);
    let now = now_millis();
    for row in &outcome.rows {
        put_record(conn, row, now)?;
    }
    conn.prepare_cached("DELETE FROM sync_dirty")?.execute([])?;
    if let Some(last) = &outcome.hlc_last {
        set_hlc_last(conn, last)?;
    }
    Ok(())
}

fn set_hlc_last(conn: &Connection, last: &str) -> Result<(), StoreError> {
    conn.prepare_cached(
        "UPDATE sync_state SET hlc_last = ?1 WHERE id = 1 AND (hlc_last IS NULL OR hlc_last < ?1)",
    )?
    .execute([last])?;
    Ok(())
}

const RECORD: &str = "SELECT version, synced_version, content, deleted_version FROM sync_record
                      WHERE kind = ?1 AND id = ?2";

fn record(conn: &Connection, kind: &str, id: &str) -> Result<Option<SyncRecordRow>, StoreError> {
    Ok(conn
        .prepare_cached(RECORD)?
        .query_row([kind, id], |r| {
            Ok(SyncRecordRow {
                kind: kind.to_owned(),
                id: id.to_owned(),
                version: r.get(0)?,
                synced_version: r.get(1)?,
                content: json_column(r, 2)?,
                deleted_version: r.get(3)?,
            })
        })
        .optional()?)
}

fn json_column(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<Value>> {
    let text: Option<String> = r.get(idx)?;
    text.map(|t| {
        serde_json::from_str(&t).map_err(|err| {
            crate::accounts::conversion_error(idx, rusqlite::types::Type::Text, err.to_string())
        })
    })
    .transpose()
}

fn put_record(conn: &Connection, row: &SyncRecordRow, now: i64) -> Result<(), StoreError> {
    let content = row.content.as_ref().map(Value::to_string);
    conn.prepare_cached(
        "INSERT INTO sync_record (kind, id, version, synced_version, content, deleted_version,
                                  updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (kind, id) DO UPDATE SET
           version = excluded.version, synced_version = excluded.synced_version,
           content = excluded.content, deleted_version = excluded.deleted_version,
           updated_at = excluded.updated_at",
    )?
    .execute(params![
        row.kind,
        row.id,
        row.version,
        row.synced_version,
        content,
        row.deleted_version,
        now
    ])
    .map_err(StoreError::sqlite("write sync record"))?;
    Ok(())
}

/// `<account uuid>/<rest>`: the account part has no `/`.
fn split_scoped(id: &str) -> Option<(&str, &str)> {
    id.split_once('/')
}

fn category_name(code: Option<i64>) -> Value {
    code.and_then(MessageCategory::from_code)
        .map_or(Value::Null, |c| Value::String(c.as_str().to_owned()))
}

fn server_json(host: String, port: i64, security: String, username: String) -> Value {
    json!({ "host": host, "port": port, "security": security, "username": username })
}

/// The synchronised content of a record as its table holds it now, `None` if it is gone.
pub(crate) fn content_of(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<Option<Value>, StoreError> {
    let value = match kind {
        "setting" => {
            let Some((scope, key)) = split_scoped(id) else {
                return Ok(None);
            };
            let text: Option<String> = conn
                .prepare_cached("SELECT value FROM setting WHERE scope = ?1 AND key = ?2")?
                .query_row([scope, key], |r| r.get(0))
                .optional()?;
            text.and_then(|t| serde_json::from_str(&t).ok())
        }
        "account" => conn
            .prepare_cached(
                "SELECT email, display_name, color, imap_host, imap_port, imap_security,
                        imap_username, smtp_host, smtp_port, smtp_security, smtp_username
                 FROM account WHERE uuid = ?1",
            )?
            .query_row([id], |r| {
                Ok(json!({
                    "email": r.get::<_, String>(0)?,
                    "displayName": r.get::<_, String>(1)?,
                    "color": r.get::<_, i64>(2)?,
                    "imap": server_json(r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?),
                    "smtp": server_json(r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?),
                }))
            })
            .optional()?,
        "identity" => conn
            .prepare_cached(
                "SELECT a.uuid, i.email, i.name, i.reply_to, i.is_default, i.signature_new,
                        i.signature_reply
                 FROM identity i JOIN account a ON a.id = i.account_id WHERE i.uuid = ?1",
            )?
            .query_row([id], |r| {
                let mut value = json!({
                    "account": r.get::<_, String>(0)?,
                    "email": r.get::<_, String>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "replyTo": r.get::<_, Option<String>>(3)?,
                    "isDefault": r.get::<_, bool>(4)?,
                });
                // Stage 10: the default signatures, only when set (an identity without them
                // keeps the content, and so the version, it had before).
                for (field, idx) in [("signatureNew", 5), ("signatureReply", 6)] {
                    if let (Some(uuid), Some(map)) =
                        (r.get::<_, Option<String>>(idx)?, value.as_object_mut())
                    {
                        map.insert(field.to_owned(), Value::String(uuid));
                    }
                }
                Ok(value)
            })
            .optional()?,
        // Stage 10: a signature of the pool (decision 0023); `html` and `images` only when set.
        "signature" => conn
            .prepare_cached("SELECT name, html, text, images FROM signature WHERE uuid = ?1")?
            .query_row([id], |r| {
                let mut value = json!({
                    "name": r.get::<_, String>(0)?,
                    "text": r.get::<_, String>(2)?,
                });
                if let Some(map) = value.as_object_mut() {
                    if let Some(html) = r.get::<_, Option<String>>(1)? {
                        map.insert("html".to_owned(), Value::String(html));
                    }
                    let images = crate::signatures::images_from_text(&r.get::<_, String>(3)?);
                    if !images.is_empty() {
                        map.insert(
                            "images".to_owned(),
                            Value::Array(images.iter().map(|i| i.to_json()).collect()),
                        );
                    }
                }
                Ok(value)
            })
            .optional()?,
        "template" => conn
            .prepare_cached("SELECT name, subject, html FROM template WHERE uuid = ?1")?
            .query_row([id], |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "subject": r.get::<_, String>(1)?,
                    "html": r.get::<_, String>(2)?,
                }))
            })
            .optional()?,
        "senderRule" => {
            let Some((key_kind, key_value)) = id.split_once(':') else {
                return Ok(None);
            };
            conn.prepare_cached(
                "SELECT category, important FROM sender_rule
                 WHERE key_kind = ?1 AND key_value = ?2",
            )?
            .query_row([key_kind, key_value], |r| {
                Ok(json!({
                    "category": category_name(r.get(0)?),
                    "important": r.get::<_, bool>(1)?,
                }))
            })
            .optional()?
        }
        "rule" => conn
            .prepare_cached("SELECT name, position, enabled, definition FROM rule WHERE uuid = ?1")?
            .query_row([id], |r| {
                let definition: String = r.get(3)?;
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "position": r.get::<_, i64>(1)?,
                    "enabled": r.get::<_, bool>(2)?,
                    "definition": serde_json::from_str::<Value>(&definition)
                        .unwrap_or(Value::Null),
                }))
            })
            .optional()?,
        "snooze" | "pin" | "task" | "remoteContent" => {
            let Some((account, rest)) = split_scoped(id) else {
                return Ok(None);
            };
            let sql = match kind {
                "snooze" => {
                    "SELECT json_object('untilMs', until) FROM snooze
                     WHERE account_uuid = ?1 AND message_id_hdr = ?2"
                }
                "pin" => {
                    "SELECT json_object('pinnedAtMs', pinned_at) FROM pin
                     WHERE account_uuid = ?1 AND message_id_hdr = ?2"
                }
                "task" => {
                    "SELECT json_object('doneAtMs', done_at) FROM task
                     WHERE account_uuid = ?1 AND message_id_hdr = ?2"
                }
                _ => {
                    "SELECT '{}' FROM remote_content_sender
                     WHERE account_uuid = ?1 AND sender = ?2"
                }
            };
            let text: Option<String> = conn
                .prepare_cached(sql)?
                .query_row([account, rest], |r| r.get(0))
                .optional()?;
            text.and_then(|t| serde_json::from_str(&t).ok())
        }
        "accountGroup" => {
            let group: Option<(i64, String, i64)> = conn
                .prepare_cached("SELECT id, name, position FROM account_group WHERE uuid = ?1")?
                .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            match group {
                None => None,
                Some((row, name, position)) => {
                    let accounts = conn
                        .prepare_cached(
                            "SELECT account_uuid FROM account_group_member WHERE group_id = ?1
                             ORDER BY account_uuid",
                        )?
                        .query_map([row], |r| r.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    Some(json!({ "name": name, "position": position, "accounts": accounts }))
                }
            }
        }
        // Stage 10: the name the user gave and the merge (decision 0023).
        "contact" => crate::contacts_dir::synced_content(conn, id)?,
        "device" => conn
            .prepare_cached(
                "SELECT name, system, last_seen_ms FROM sync_device WHERE device_id = ?1",
            )?
            .query_row([id], |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "system": r.get::<_, String>(1)?,
                    "lastSeenMs": r.get::<_, i64>(2)?,
                }))
            })
            .optional()?,
        _ => None,
    };
    Ok(value)
}

// ---------------------------------------------------------------------------------------------
// Apply

fn account_row(conn: &Connection, uuid: &str) -> Result<Option<i64>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT id FROM account WHERE uuid = ?1")?
        .query_row([uuid], |r| r.get(0))
        .optional()?)
}

fn missing(what: impl Into<String>) -> ApplyError {
    ApplyError::Skip(ApplySkip::Missing(what.into()))
}

fn refused(what: impl Into<String>) -> ApplyError {
    ApplyError::Skip(ApplySkip::Refused(what.into()))
}

/// A failed table write: skip this record (the transaction goes on) or fail the whole apply.
enum ApplyError {
    Skip(ApplySkip),
    Store(StoreError),
}

impl From<StoreError> for ApplyError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<rusqlite::Error> for ApplyError {
    fn from(err: rusqlite::Error) -> Self {
        // Constraint violations concern this record only (an address taken, a missing
        // reference); anything else is a real database failure.
        match &err {
            rusqlite::Error::SqliteFailure(e, _)
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::Skip(ApplySkip::Refused(err.to_string()))
            }
            _ => Self::Store(StoreError::from(err)),
        }
    }
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, ApplyError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| refused(format!("field {field} missing")))
}

fn opt_text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn int(value: &Value, field: &str) -> Result<i64, ApplyError> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| refused(format!("field {field} missing")))
}

fn boolean(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn server<'a>(
    value: &'a Value,
    field: &str,
) -> Result<(&'a str, i64, &'a str, &'a str), ApplyError> {
    let s = value
        .get(field)
        .ok_or_else(|| refused(format!("field {field} missing")))?;
    Ok((
        text(s, "host")?,
        int(s, "port")?,
        text(s, "security")?,
        text(s, "username")?,
    ))
}

fn put_table(
    conn: &Connection,
    item: &SyncApply,
    value: &Value,
    outcome: &mut ApplyOutcome,
    now: i64,
) -> Result<(), ApplyError> {
    let id = item.id.as_str();
    match item.kind.as_str() {
        "setting" => {
            let (scope, key) = split_scoped(id).ok_or_else(|| refused("setting id"))?;
            conn.prepare_cached(
                "INSERT INTO setting (scope, key, value, version, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT (scope, key) DO UPDATE
                   SET value = excluded.value, version = setting.version + 1,
                       updated_at = excluded.updated_at
                   WHERE setting.value IS NOT excluded.value",
            )?
            .execute(params![scope, key, value.to_string(), now])?;
        }
        "account" => {
            let (ih, ip, is, iu) = server(value, "imap")?;
            let (sh, sp, ss, su) = server(value, "smtp")?;
            let email = text(value, "email")?;
            let name = text(value, "displayName")?;
            let color = int(value, "color")?;
            let account: AccountId = id.parse().map_err(|_| refused("account id"))?;
            if account_row(conn, id)?.is_some() {
                conn.prepare_cached(
                    "UPDATE account SET email = ?2, display_name = ?3, color = ?4,
                       imap_host = ?5, imap_port = ?6, imap_security = ?7, imap_username = ?8,
                       smtp_host = ?9, smtp_port = ?10, smtp_security = ?11, smtp_username = ?12
                     WHERE uuid = ?1",
                )?
                .execute(params![
                    id, email, name, color, ih, ip, is, iu, sh, sp, ss, su
                ])?;
                outcome.accounts_updated.push(account);
            } else {
                // Taken over from another device: read-only, no password yet, and no default
                // identity of its own (the identities come from the folder too).
                conn.prepare_cached(
                    "INSERT INTO account (uuid, email, display_name, color,
                                          imap_host, imap_port, imap_security, imap_username,
                                          smtp_host, smtp_port, smtp_security, smtp_username,
                                          credential_ref, writes_enabled, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?1, 0, ?13)",
                )?
                .execute(params![
                    id, email, name, color, ih, ip, is, iu, sh, sp, ss, su, now
                ])?;
                conn.prepare_cached(
                    "INSERT OR REPLACE INTO sync_adopted (account_uuid, adopted_at) VALUES (?1, ?2)",
                )?
                .execute(params![id, now])?;
                outcome.accounts_added.push(account);
            }
        }
        "identity" => {
            let account = text(value, "account")?;
            let row =
                account_row(conn, account)?.ok_or_else(|| missing(format!("account {account}")))?;
            let email = text(value, "email")?;
            let name = text(value, "name")?;
            let reply_to = opt_text(value, "replyTo");
            let is_default = boolean(value, "isDefault");
            if is_default {
                conn.prepare_cached(
                    "UPDATE identity SET is_default = 0
                     WHERE account_id = ?1 AND is_default = 1 AND uuid IS NOT ?2",
                )?
                .execute(params![row, id])?;
            }
            let signature_new = opt_text(value, "signatureNew");
            let signature_reply = opt_text(value, "signatureReply");
            let updated = conn
                .prepare_cached(
                    "UPDATE identity SET account_id = ?2, email = ?3, name = ?4, reply_to = ?5,
                       is_default = ?6, signature_new = ?7, signature_reply = ?8 WHERE uuid = ?1",
                )?
                .execute(params![
                    id,
                    row,
                    email,
                    name,
                    reply_to,
                    is_default,
                    signature_new,
                    signature_reply
                ])?;
            if updated == 0 {
                conn.prepare_cached(
                    "INSERT INTO identity (account_id, email, name, reply_to, is_default, uuid,
                                           signature_new, signature_reply)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )?
                .execute(params![
                    row,
                    email,
                    name,
                    reply_to,
                    is_default,
                    id,
                    signature_new,
                    signature_reply
                ])?;
            }
        }
        "signature" => {
            let images: Vec<crate::signatures::StoredSignatureImage> = value
                .get("images")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(crate::signatures::StoredSignatureImage::from_json)
                .collect();
            let text = opt_text(value, "text").unwrap_or_default();
            conn.prepare_cached(
                "INSERT INTO signature (uuid, name, html, text, images, position, created_at,
                                        updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, (SELECT coalesce(max(position), -1) + 1 FROM signature),
                         ?6, ?6)
                 ON CONFLICT (uuid) DO UPDATE SET name = excluded.name, html = excluded.html,
                   text = excluded.text, images = excluded.images,
                   updated_at = excluded.updated_at",
            )?
            .execute(params![
                id,
                opt_text(value, "name").unwrap_or_default(),
                opt_text(value, "html"),
                text,
                crate::signatures::images_text(&images),
                now
            ])?;
        }
        "template" => {
            conn.prepare_cached(
                "INSERT INTO template (uuid, name, subject, html, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, (SELECT coalesce(max(position), -1) + 1 FROM template),
                         ?5, ?5)
                 ON CONFLICT (uuid) DO UPDATE SET name = excluded.name,
                   subject = excluded.subject, html = excluded.html,
                   updated_at = excluded.updated_at",
            )?
            .execute(params![
                id,
                text(value, "name")?,
                text(value, "subject")?,
                text(value, "html")?,
                now
            ])?;
        }
        "senderRule" => {
            let (key_kind, key_value) = id
                .split_once(':')
                .ok_or_else(|| refused("sender rule id"))?;
            let category = opt_text(value, "category")
                .and_then(MessageCategory::parse)
                .map(MessageCategory::code);
            let important = boolean(value, "important");
            let uuid = crate::groups::new_uuid(conn)?;
            conn.prepare_cached(
                "INSERT INTO sender_rule (uuid, key_kind, key_value, category, important,
                                          created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT (key_kind, key_value) DO UPDATE
                   SET category = excluded.category, important = excluded.important,
                       updated_at = excluded.updated_at",
            )?
            .execute(params![uuid, key_kind, key_value, category, important, now])?;
            crate::categories::schedule_rules(conn)?;
        }
        "rule" => {
            let definition = value
                .get("definition")
                .filter(|d| d.is_object())
                .ok_or_else(|| refused("rule definition"))?;
            conn.prepare_cached(
                "INSERT INTO rule (uuid, name, position, enabled, definition, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT (uuid) DO UPDATE
                   SET name = excluded.name, position = excluded.position,
                       enabled = excluded.enabled, definition = excluded.definition,
                       updated_at = excluded.updated_at",
            )?
            .execute(params![
                id,
                text(value, "name")?,
                int(value, "position")?,
                boolean(value, "enabled"),
                definition.to_string(),
                now
            ])?;
        }
        "snooze" | "pin" | "task" | "remoteContent" => {
            let (account, rest) = split_scoped(id).ok_or_else(|| refused("id"))?;
            if account_row(conn, account)?.is_none() {
                return Err(missing(format!("account {account}")));
            }
            match item.kind.as_str() {
                "snooze" => {
                    conn.prepare_cached(
                        "INSERT INTO snooze (account_uuid, message_id_hdr, until, returned_at,
                                             created_at, updated_at)
                         VALUES (?1, ?2, ?3, NULL, ?4, ?4)
                         ON CONFLICT (account_uuid, message_id_hdr) DO UPDATE
                           SET returned_at = CASE WHEN snooze.until = excluded.until
                                                  THEN snooze.returned_at END,
                               until = excluded.until, updated_at = excluded.updated_at",
                    )?
                    .execute(params![
                        account,
                        rest,
                        int(value, "untilMs")?,
                        now
                    ])?;
                }
                "pin" => {
                    conn.prepare_cached(
                        "INSERT INTO pin (account_uuid, message_id_hdr, pinned_at)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT (account_uuid, message_id_hdr) DO UPDATE
                           SET pinned_at = excluded.pinned_at",
                    )?
                    .execute(params![
                        account,
                        rest,
                        int(value, "pinnedAtMs")?
                    ])?;
                }
                "task" => {
                    let done_at = value.get("doneAtMs").and_then(Value::as_i64);
                    conn.prepare_cached(
                        "INSERT INTO task (account_uuid, message_id_hdr, done_at, created_at,
                                           updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?4)
                         ON CONFLICT (account_uuid, message_id_hdr) DO UPDATE
                           SET done_at = excluded.done_at, updated_at = excluded.updated_at",
                    )?
                    .execute(params![account, rest, done_at, now])?;
                }
                _ => {
                    conn.prepare_cached(
                        "INSERT OR IGNORE INTO remote_content_sender (account_uuid, sender, created_at)
                         VALUES (?1, ?2, ?3)",
                    )?
                    .execute(params![account, rest, now])?;
                }
            }
        }
        "accountGroup" => {
            let name = text(value, "name")?;
            let position = int(value, "position")?;
            conn.prepare_cached(
                "INSERT INTO account_group (uuid, name, position, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (uuid) DO UPDATE SET name = excluded.name, position = excluded.position",
            )?
            .execute(params![id, name, position, now])?;
            let group: i64 = conn
                .prepare_cached("SELECT id FROM account_group WHERE uuid = ?1")?
                .query_row([id], |r| r.get(0))?;
            conn.prepare_cached("DELETE FROM account_group_member WHERE group_id = ?1")?
                .execute([group])?;
            for account in value
                .get("accounts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                // Members this device does not know (yet) are left out here.
                conn.prepare_cached(
                    "INSERT OR IGNORE INTO account_group_member (group_id, account_uuid)
                     SELECT ?1, uuid FROM account WHERE uuid = ?2",
                )?
                .execute(params![group, account])?;
            }
        }
        "contact" => {
            // The name the user gave and the merge; a contact this device never saw has nothing
            // to name: the record waits in `sync_record` until the address appears.
            crate::contacts_dir::apply_synced(
                conn,
                id,
                opt_text(value, "userName"),
                opt_text(value, "mergedInto"),
            )?;
        }
        "device" => {
            conn.prepare_cached(
                "INSERT INTO sync_device (device_id, name, system, last_seen_ms) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (device_id) DO UPDATE SET name = excluded.name,
                   system = excluded.system, last_seen_ms = excluded.last_seen_ms",
            )?
            .execute(params![
                id,
                text(value, "name")?,
                text(value, "system")?,
                int(value, "lastSeenMs")?
            ])?;
        }
        // Kinds without a local table yet (views, AI settings): kept in the sync state only,
        // so they are not lost for a later version.
        _ => return Ok(()),
    }
    outcome.kinds.insert(item.kind.clone());
    Ok(())
}

fn delete_table(
    conn: &Connection,
    item: &SyncApply,
    outcome: &mut ApplyOutcome,
) -> Result<(), ApplyError> {
    let id = item.id.as_str();
    let changed = match item.kind.as_str() {
        "setting" => {
            let (scope, key) = split_scoped(id).ok_or_else(|| refused("setting id"))?;
            conn.prepare_cached("DELETE FROM setting WHERE scope = ?1 AND key = ?2")?
                .execute([scope, key])?
        }
        // Accounts are only removed by the user (`AppCore::resolve_sync_question`).
        "account" => 0,
        "identity" => conn
            .prepare_cached("DELETE FROM identity WHERE uuid = ?1")?
            .execute([id])?,
        // The defaults and draft choices that name it stay: a name that points nowhere means
        // "none", and the deleting device sends the identities it changed itself.
        "signature" => conn
            .prepare_cached("DELETE FROM signature WHERE uuid = ?1")?
            .execute([id])?,
        "template" => conn
            .prepare_cached("DELETE FROM template WHERE uuid = ?1")?
            .execute([id])?,
        "senderRule" => {
            let (key_kind, key_value) = id
                .split_once(':')
                .ok_or_else(|| refused("sender rule id"))?;
            let n = conn
                .prepare_cached("DELETE FROM sender_rule WHERE key_kind = ?1 AND key_value = ?2")?
                .execute([key_kind, key_value])?;
            if n > 0 {
                crate::categories::schedule_rules(conn)?;
            }
            n
        }
        "rule" => conn
            .prepare_cached("DELETE FROM rule WHERE uuid = ?1")?
            .execute([id])?,
        "snooze" | "pin" | "task" | "remoteContent" => {
            let (account, rest) = split_scoped(id).ok_or_else(|| refused("id"))?;
            let sql = match item.kind.as_str() {
                "snooze" => "DELETE FROM snooze WHERE account_uuid = ?1 AND message_id_hdr = ?2",
                "pin" => "DELETE FROM pin WHERE account_uuid = ?1 AND message_id_hdr = ?2",
                "task" => "DELETE FROM task WHERE account_uuid = ?1 AND message_id_hdr = ?2",
                _ => "DELETE FROM remote_content_sender WHERE account_uuid = ?1 AND sender = ?2",
            };
            conn.prepare_cached(sql)?.execute([account, rest])?
        }
        "accountGroup" => conn
            .prepare_cached("DELETE FROM account_group WHERE uuid = ?1")?
            .execute([id])?,
        "contact" => crate::contacts_dir::apply_synced(conn, id, None, None)?,
        "device" => conn
            .prepare_cached("DELETE FROM sync_device WHERE device_id = ?1")?
            .execute([id])?,
        _ => 0,
    };
    if changed > 0 {
        outcome.kinds.insert(item.kind.clone());
    }
    Ok(())
}

/// The local account `from` is the folder's account `to` (same address, never synced): it
/// takes the folder's id everywhere it is referred to by id (decision 0022). Its local sync
/// records go; the records that follow from the new id are captured again.
fn rekey_account(conn: &Connection, from: &str, to: &str) -> Result<(), ApplyError> {
    let row = account_row(conn, from)?.ok_or_else(|| missing(format!("account {from}")))?;
    if account_row(conn, to)?.is_some() {
        return Err(refused(format!("account {to} exists already")));
    }
    // Children refer to the UUID without ON UPDATE CASCADE; checked at commit instead.
    conn.execute_batch("PRAGMA defer_foreign_keys = ON")?;
    conn.prepare_cached("UPDATE account SET uuid = ?2 WHERE uuid = ?1")?
        .execute([from, to])?;
    for table in [
        "snooze",
        "pin",
        "task",
        "remote_content_sender",
        "account_group_member",
        "account_hidden",
        "sync_adopted",
        "sync_state",
    ] {
        conn.execute(
            &format!("UPDATE {table} SET account_uuid = ?2 WHERE account_uuid = ?1"),
            [from, to],
        )?;
    }
    // References inside JSON (rule conditions and targets, muted accounts) and scoped settings.
    conn.execute(
        "UPDATE rule SET definition = replace(definition, ?1, ?2) WHERE instr(definition, ?1) > 0",
        [from, to],
    )?;
    conn.execute(
        "UPDATE setting SET value = replace(value, ?1, ?2) WHERE instr(value, ?1) > 0",
        [from, to],
    )?;
    conn.execute("UPDATE setting SET scope = ?2 WHERE scope = ?1", [from, to])?;
    conn.execute(
        "DELETE FROM sync_record WHERE (kind = 'account' AND id = ?1)
           OR (kind IN ('snooze', 'pin', 'task', 'remoteContent', 'setting')
               AND substr(id, 1, length(?1) + 1) = ?1 || '/')",
        [from],
    )?;
    // The identities' content names the account by id.
    conn.execute(
        "INSERT OR IGNORE INTO sync_dirty (kind, id)
         SELECT 'identity', uuid FROM identity WHERE account_id = ?1 AND uuid IS NOT NULL",
        [row],
    )?;
    conn.execute(
        "DELETE FROM sync_dirty WHERE kind = 'account' AND id IN (?1, ?2)",
        [from, to],
    )?;
    Ok(())
}

/// The local identity `from` is the folder's identity `to` (same account and address).
fn rekey_identity(conn: &Connection, from: &str, to: &str) -> Result<(), ApplyError> {
    let exists: Option<i64> = conn
        .prepare_cached("SELECT id FROM identity WHERE uuid = ?1")?
        .query_row([to], |r| r.get(0))
        .optional()?;
    if exists.is_some() {
        return Err(refused(format!("identity {to} exists already")));
    }
    let n = conn
        .prepare_cached("UPDATE identity SET uuid = ?2 WHERE uuid = ?1")?
        .execute([from, to])?;
    if n == 0 {
        return Err(missing(format!("identity {from}")));
    }
    // Since stage 10 a signature is a record of its own (also the one that has the identity's
    // former UUID): it keeps its id.
    conn.execute(
        "DELETE FROM sync_record WHERE kind = 'identity' AND id = ?1",
        [from],
    )?;
    conn.execute(
        "DELETE FROM sync_dirty WHERE kind = 'identity' AND id = ?1",
        [from],
    )?;
    Ok(())
}

fn apply_one(
    conn: &Connection,
    item: &SyncApply,
    outcome: &mut ApplyOutcome,
    now: i64,
) -> Result<(), ApplyError> {
    if let Some(from) = &item.rekey_from {
        match item.kind.as_str() {
            "account" => {
                rekey_account(conn, from, &item.id)?;
                if let (Ok(old), Ok(new)) = (from.parse(), item.id.parse()) {
                    outcome.accounts_rekeyed.push((old, new));
                }
            }
            "identity" => rekey_identity(conn, from, &item.id)?,
            _ => return Err(refused("only accounts and identities take another id")),
        }
    }
    let current = record(conn, &item.kind, &item.id)?.map(|r| r.version);
    if current != item.expected {
        return Err(ApplyError::Skip(ApplySkip::ChangedLocally));
    }
    match &item.table {
        TableWrite::Keep => {}
        TableWrite::Put(value) => put_table(conn, item, value, outcome, now)?,
        TableWrite::Delete => delete_table(conn, item, outcome)?,
    }
    match &item.row {
        Some(row) => put_record(conn, row, now)?,
        None => {
            conn.prepare_cached("DELETE FROM sync_record WHERE kind = ?1 AND id = ?2")?
                .execute([&item.kind, &item.id])?;
        }
    }
    conn.prepare_cached("DELETE FROM sync_dirty WHERE kind = ?1 AND id = ?2")?
        .execute([&item.kind, &item.id])?;
    Ok(())
}

/// Accounts first (identities, snoozes and the rest refer to them), identities before their
/// signatures, then the rest in the order given.
fn apply_order(kind: &str) -> u8 {
    match kind {
        "account" => 0,
        "identity" => 1,
        _ => 2,
    }
}

fn apply_all(
    conn: &Connection,
    mut items: Vec<SyncApply>,
    progress: Option<&SyncProgress>,
) -> Result<ApplyOutcome, StoreError> {
    items.sort_by_key(|item| apply_order(&item.kind));
    let now = now_millis();
    let mut outcome = ApplyOutcome::default();
    for item in items {
        conn.execute_batch("SAVEPOINT sync_apply")?;
        match apply_one(conn, &item, &mut outcome, now) {
            Ok(()) => {
                conn.execute_batch("RELEASE sync_apply")?;
                outcome.applied.push((item.kind, item.id));
            }
            Err(ApplyError::Skip(reason)) => {
                conn.execute_batch("ROLLBACK TO sync_apply; RELEASE sync_apply")?;
                outcome.skipped.push((item.kind, item.id, reason));
            }
            Err(ApplyError::Store(err)) => {
                conn.execute_batch("ROLLBACK TO sync_apply; RELEASE sync_apply")?;
                return Err(err);
            }
        }
    }
    if let Some(progress) = progress {
        put_progress(conn, progress)?;
    }
    Ok(outcome)
}

fn put_progress(conn: &Connection, progress: &SyncProgress) -> Result<(), StoreError> {
    conn.prepare_cached(
        "UPDATE sync_state SET
           last_complete_sync_ms = coalesce(?1, last_complete_sync_ms),
           last_full_check_ms = coalesce(?2, last_full_check_ms),
           folder_state = coalesce(?3, folder_state),
           updated_at = ?4
         WHERE id = 1",
    )?
    .execute(params![
        progress.last_complete_sync_ms,
        progress.last_full_check_ms,
        progress.folder_state,
        now_millis()
    ])?;
    if let Some(last) = &progress.hlc_last {
        set_hlc_last(conn, last)?;
    }
    Ok(())
}

fn state_row(conn: &Connection) -> Result<SyncStateRow, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT device_id, hlc_last, enabled, account_uuid, folder, last_complete_sync_ms,
                    last_full_check_ms, folder_state FROM sync_state WHERE id = 1",
        )?
        .query_row([], |r| {
            let account: Option<String> = r.get(3)?;
            Ok(SyncStateRow {
                device_id: r.get(0)?,
                hlc_last: r.get(1)?,
                enabled: r.get(2)?,
                account: account.and_then(|a| a.parse().ok()),
                folder: r.get(4)?,
                last_complete_sync_ms: r.get(5)?,
                last_full_check_ms: r.get(6)?,
                folder_state: r.get(7)?,
            })
        })?)
}

impl Store {
    /// Captures marks left from before the hook was set (data stored before migration 0009).
    pub async fn flush_sync_capture(&self) -> Result<(), StoreError> {
        self.write(|_| Ok(())).await
    }

    /// Every `sync_record` row.
    pub async fn sync_records(&self) -> Result<Vec<SyncRecordRow>, StoreError> {
        self.read(|conn| {
            let rows = conn
                .prepare_cached(
                    "SELECT kind, id, version, synced_version, content, deleted_version
                     FROM sync_record ORDER BY kind, id",
                )?
                .query_map([], |r| {
                    Ok(SyncRecordRow {
                        kind: r.get(0)?,
                        id: r.get(1)?,
                        version: r.get(2)?,
                        synced_version: r.get(3)?,
                        content: json_column(r, 4)?,
                        deleted_version: r.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// One `sync_record` row.
    pub async fn sync_record(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<Option<SyncRecordRow>, StoreError> {
        let (kind, id) = (kind.to_owned(), id.to_owned());
        self.read(move |conn| record(conn, &kind, &id)).await
    }

    /// The synchronised content of a record as its table holds it now.
    pub async fn sync_table_content(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<Option<Value>, StoreError> {
        let (kind, id) = (kind.to_owned(), id.to_owned());
        self.read(move |conn| content_of(conn, &kind, &id)).await
    }

    /// Local changes not yet in the folder.
    pub async fn sync_pending(&self) -> Result<u32, StoreError> {
        self.read(|conn| {
            let n: i64 = conn
                .prepare_cached(
                    "SELECT count(*) FROM sync_record WHERE synced_version IS NOT version",
                )?
                .query_row([], |r| r.get(0))?;
            Ok(u32::try_from(n).unwrap_or(u32::MAX))
        })
        .await
    }

    /// Applies the records of a pass and the pass's progress in one transaction (see the
    /// module docs).
    pub async fn apply_sync(
        &self,
        items: Vec<SyncApply>,
        progress: Option<SyncProgress>,
    ) -> Result<ApplyOutcome, StoreError> {
        self.write(move |tx| apply_all(tx, items, progress.as_ref()))
            .await
    }

    /// Writes rows into `sync_record` without touching the tables (a record kept against a
    /// removal elsewhere, renewed under a new version). Rows whose current version is not the
    /// expected one are skipped; returns how many were written.
    pub async fn put_sync_records(
        &self,
        rows: Vec<(Option<String>, SyncRecordRow)>,
    ) -> Result<usize, StoreError> {
        self.write(move |tx| {
            let now = now_millis();
            let mut written = 0;
            for (expected, row) in rows {
                let current = record(tx, &row.kind, &row.id)?.map(|r| r.version);
                if current == expected {
                    put_record(tx, &row, now)?;
                    written += 1;
                }
            }
            Ok(written)
        })
        .await
    }

    pub async fn sync_state(&self) -> Result<SyncStateRow, StoreError> {
        self.read(state_row).await
    }

    /// Turns the sync on (with its account and folder) or off.
    pub async fn set_sync_enabled(
        &self,
        enabled: bool,
        account: Option<AccountId>,
        folder: Option<String>,
    ) -> Result<SyncStateRow, StoreError> {
        self.write(move |tx| {
            tx.prepare_cached(
                "UPDATE sync_state SET enabled = ?1, account_uuid = ?2, folder = ?3,
                   folder_state = NULL, updated_at = ?4 WHERE id = 1",
            )?
            .execute(params![
                enabled,
                account.map(|a| a.to_string()),
                folder,
                now_millis()
            ])?;
            state_row(tx)
        })
        .await
    }

    pub async fn set_sync_progress(&self, progress: SyncProgress) -> Result<(), StoreError> {
        self.write(move |tx| put_progress(tx, &progress)).await
    }

    /// Every record counts as never synced (after a reset, or joining a folder reset
    /// elsewhere), and the device has never completed a pass.
    pub async fn forget_sync(&self) -> Result<(), StoreError> {
        self.write(|tx| {
            tx.execute("UPDATE sync_record SET synced_version = NULL", [])?;
            tx.execute(
                "UPDATE sync_state SET last_complete_sync_ms = NULL, last_full_check_ms = NULL,
                   folder_state = NULL WHERE id = 1",
                [],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn sync_devices(&self) -> Result<Vec<StoredSyncDevice>, StoreError> {
        self.read(|conn| {
            let rows = conn
                .prepare_cached(
                    "SELECT device_id, name, system, last_seen_ms FROM sync_device
                     ORDER BY last_seen_ms DESC, device_id",
                )?
                .query_map([], |r| {
                    Ok(StoredSyncDevice {
                        device_id: r.get(0)?,
                        name: r.get(1)?,
                        system: r.get(2)?,
                        last_seen_ms: r.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// This device's entry in the device list (captured like any synchronised change).
    pub async fn put_sync_device(&self, device: StoredSyncDevice) -> Result<(), StoreError> {
        self.write(move |tx| {
            tx.prepare_cached(
                "INSERT INTO sync_device (device_id, name, system, last_seen_ms) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (device_id) DO UPDATE SET name = excluded.name,
                   system = excluded.system, last_seen_ms = excluded.last_seen_ms",
            )?
            .execute(params![
                device.device_id,
                device.name,
                device.system,
                device.last_seen_ms
            ])?;
            Ok(())
        })
        .await
    }

    /// Accounts taken over from another device that have no password here yet.
    pub async fn sync_adopted_accounts(&self) -> Result<Vec<AccountId>, StoreError> {
        self.read(|conn| {
            let rows = conn
                .prepare_cached("SELECT account_uuid FROM sync_adopted ORDER BY adopted_at")?
                .query_map([], |r| crate::accounts::account_id_at(r, 0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    /// The account got its password here.
    pub async fn clear_sync_adopted(&self, account: AccountId) -> Result<(), StoreError> {
        self.write(move |tx| {
            tx.prepare_cached("DELETE FROM sync_adopted WHERE account_uuid = ?1")?
                .execute([account.to_string()])?;
            Ok(())
        })
        .await
    }
}
