//! Accounts and identities. Synchronous functions over a connection; `mail_api` runs them on the
//! writer thread or a reader.

use mail_model::{AccountColor, AccountId, Identity, IdentityId, Security, ServerSettings};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::StoreError;
use crate::mail_api::{NewAccount, StoredAccount};
use crate::settings::now_millis;

pub(crate) const SELECT_ACCOUNTS: &str = "
    SELECT uuid, email, display_name, color,
           imap_host, imap_port, imap_security, imap_username,
           smtp_host, smtp_port, smtp_security, smtp_username,
           credential_ref, writes_enabled, created_at
    FROM account ORDER BY created_at, id";

pub(crate) const SELECT_ACCOUNT: &str = "
    SELECT uuid, email, display_name, color,
           imap_host, imap_port, imap_security, imap_username,
           smtp_host, smtp_port, smtp_security, smtp_username,
           credential_ref, writes_enabled, created_at
    FROM account WHERE uuid = ?1";

pub(crate) const ACCOUNT_ROW: &str = "SELECT id FROM account WHERE uuid = ?1";

/// Local row id of the account, or [`StoreError::NotFound`].
pub(crate) fn row_id(conn: &Connection, account: AccountId) -> Result<i64, StoreError> {
    conn.prepare_cached(ACCOUNT_ROW)?
        .query_row([account.to_string()], |r| r.get(0))
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("account {account}")))
}

pub(crate) fn insert(conn: &Connection, new: &NewAccount) -> Result<StoredAccount, StoreError> {
    let taken: Option<i64> = conn
        .prepare_cached("SELECT id FROM account WHERE email = ?1")?
        .query_row([&new.email], |r| r.get(0))
        .optional()?;
    if taken.is_some() {
        return Err(StoreError::AccountExists(new.email.clone()));
    }

    let used = conn
        .prepare_cached("SELECT color FROM account")?
        .query_map([], |r| color_at(r, 0))?
        .collect::<Result<Vec<_>, _>>()?;
    let color = AccountColor::next_free(&used);
    let created_at = now_millis();

    conn.prepare_cached(
        "INSERT INTO account (uuid, email, display_name, color,
                              imap_host, imap_port, imap_security, imap_username,
                              smtp_host, smtp_port, smtp_security, smtp_username,
                              credential_ref, writes_enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, ?14)",
    )?
    .execute(params![
        new.id.to_string(),
        new.email,
        new.display_name,
        color.index(),
        new.imap.host,
        new.imap.port,
        new.imap.security.as_str(),
        new.imap.username,
        new.smtp.host,
        new.smtp.port,
        new.smtp.security.as_str(),
        new.smtp.username,
        new.credential_ref,
        created_at,
    ])
    .map_err(StoreError::sqlite("insert account"))?;
    let row = conn.last_insert_rowid();

    conn.prepare_cached(
        "INSERT INTO identity (account_id, email, name, reply_to, is_default)
         VALUES (?1, ?2, ?3, NULL, 1)",
    )?
    .execute(params![row, new.email, new.display_name])
    .map_err(StoreError::sqlite("insert default identity"))?;

    Ok(StoredAccount {
        id: new.id,
        email: new.email.clone(),
        display_name: new.display_name.clone(),
        color,
        imap: new.imap.clone(),
        smtp: new.smtp.clone(),
        credential_ref: new.credential_ref.clone(),
        writes_enabled: false,
        created_at,
    })
}

pub(crate) fn all(conn: &Connection) -> Result<Vec<StoredAccount>, StoreError> {
    let accounts = conn
        .prepare_cached(SELECT_ACCOUNTS)?
        .query_map([], account_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(accounts)
}

pub(crate) fn one(conn: &Connection, id: AccountId) -> Result<Option<StoredAccount>, StoreError> {
    Ok(conn
        .prepare_cached(SELECT_ACCOUNT)?
        .query_row([id.to_string()], account_from_row)
        .optional()?)
}

/// Deletes the account; foreign keys cascade to identities, mailboxes, messages and locations.
pub(crate) fn delete(conn: &Connection, id: AccountId) -> Result<bool, StoreError> {
    let removed = conn
        .prepare_cached("DELETE FROM account WHERE uuid = ?1")?
        .execute([id.to_string()])
        .map_err(StoreError::sqlite("delete account"))?;
    Ok(removed > 0)
}

pub(crate) fn identities(
    conn: &Connection,
    account: AccountId,
) -> Result<Vec<Identity>, StoreError> {
    let row = row_id(conn, account)?;
    let identities = conn
        .prepare_cached(
            "SELECT id, email, name, reply_to, is_default FROM identity
             WHERE account_id = ?1 ORDER BY is_default DESC, id",
        )?
        .query_map([row], |r| {
            Ok(Identity {
                id: IdentityId(r.get(0)?),
                account_id: account,
                email: r.get(1)?,
                name: r.get(2)?,
                reply_to: r.get(3)?,
                is_default: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(identities)
}

/// Column `idx` as [`AccountId`].
pub(crate) fn account_id_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<AccountId> {
    let text: String = r.get(idx)?;
    text.parse()
        .map_err(|err: mail_model::CoreError| conversion_error(idx, Type::Text, err.to_string()))
}

pub(crate) fn conversion_error(idx: usize, ty: Type, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, ty, message.into())
}

fn color_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<AccountColor> {
    let index: u8 = r.get(idx)?;
    AccountColor::new(index)
        .ok_or_else(|| conversion_error(idx, Type::Integer, format!("account color {index}")))
}

fn server_at(r: &Row<'_>, first: usize) -> rusqlite::Result<ServerSettings> {
    let security: String = r.get(first + 2)?;
    let security = Security::parse(&security)
        .ok_or_else(|| conversion_error(first + 2, Type::Text, format!("security {security:?}")))?;
    Ok(ServerSettings {
        host: r.get(first)?,
        port: r.get(first + 1)?,
        security,
        username: r.get(first + 3)?,
    })
}

fn account_from_row(r: &Row<'_>) -> rusqlite::Result<StoredAccount> {
    Ok(StoredAccount {
        id: account_id_at(r, 0)?,
        email: r.get(1)?,
        display_name: r.get(2)?,
        color: color_at(r, 3)?,
        imap: server_at(r, 4)?,
        smtp: server_at(r, 8)?,
        credential_ref: r.get(12)?,
        writes_enabled: r.get(13)?,
        created_at: r.get(14)?,
    })
}
