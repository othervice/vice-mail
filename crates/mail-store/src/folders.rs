//! Audit 24.11.2026: renaming and deleting folders of the user's own through the outbox.
//!
//! Only folders without a role (never the inbox, sent, drafts, ...) and only on accounts that may
//! write. A rename stays within the folder's parent (the user types the new name, not a path); a
//! delete needs a folder without subfolders. Nothing changes locally when they are recorded: the
//! folder tree shows the pending operation, and the change follows once the server confirmed
//! (`rename_rows`, then the folder list from the server). The favourite and colour of a renamed
//! folder (`mailbox_pref`, by decoded path) move with it.

use mail_model::{AccountId, ActionError, ActionKind, MailboxId, OperationPayload, TargetMailbox};
use rusqlite::{Connection, OptionalExtension, params};

use crate::outbox::{Recorder, repeated, require_writes};
use crate::settings::now_millis;
use crate::{OutboxChange, Store, StoreError, accounts};

/// Longest folder name accepted (one level of the path).
pub const MAX_FOLDER_NAME_CHARS: usize = 200;

/// The folder as these operations need it.
struct Folder {
    account_row: i64,
    name: String,
    path: String,
    delimiter: Option<String>,
    has_role: bool,
}

pub(crate) const FOLDER: &str = "
    SELECT account_id, name, path, delimiter, role IS NOT NULL FROM mailbox WHERE id = ?1";

pub(crate) const HAS_SUBFOLDER: &str = "SELECT 1 FROM mailbox WHERE parent_id = ?1 LIMIT 1";

pub(crate) const PATH_TAKEN: &str =
    "SELECT 1 FROM mailbox WHERE account_id = ?1 AND path = ?2 LIMIT 1";

/// A rename or delete of this folder still on its way (one at a time per folder).
pub(crate) const PENDING_CHANGE: &str = "
    SELECT 1 FROM operation
    WHERE mailbox_id = ?1 AND kind IN ('rename_mailbox', 'delete_mailbox')
      AND state IN ('queued', 'in_flight')
    LIMIT 1";

/// The folder `mailbox` of `account`, checked: it exists there, has no role and is not being
/// renamed or deleted already.
fn changeable(
    conn: &Connection,
    account: AccountId,
    mailbox: MailboxId,
) -> Result<(i64, Folder), StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    let folder = conn
        .prepare_cached(FOLDER)?
        .query_row([mailbox.0], |r| {
            Ok(Folder {
                account_row: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                delimiter: r.get(3)?,
                has_role: r.get(4)?,
            })
        })
        .optional()?
        .filter(|f| f.account_row == account_row)
        .ok_or_else(|| StoreError::NotFound(format!("mailbox {mailbox}")))?;
    if folder.has_role || folder.name.eq_ignore_ascii_case("INBOX") {
        return Err(StoreError::Action(ActionError::MailboxHasRole(mailbox)));
    }
    let pending = conn
        .prepare_cached(PENDING_CHANGE)?
        .query_row([mailbox.0], |_| Ok(()))
        .optional()?;
    if pending.is_some() {
        return Err(StoreError::InvalidArgument(format!(
            "a change of mailbox {mailbox} is still on its way"
        )));
    }
    Ok((account_row, folder))
}

/// The new decoded path of a folder renamed to `name` within its parent.
fn renamed_path(folder: &Folder, name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    let invalid = || StoreError::InvalidArgument(format!("folder name {name:?} is not valid"));
    if name.is_empty()
        || name.chars().count() > MAX_FOLDER_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    match folder.delimiter.as_deref().filter(|d| !d.is_empty()) {
        Some(delimiter) => {
            if name.contains(delimiter) {
                return Err(invalid());
            }
            Ok(match folder.path.rsplit_once(delimiter) {
                Some((parent, _)) => format!("{parent}{delimiter}{name}"),
                None => name.to_owned(),
            })
        }
        None => Ok(name.to_owned()),
    }
}

/// See [`Store::enqueue_rename_mailbox`].
pub(crate) fn enqueue_rename(
    conn: &Connection,
    account: AccountId,
    mailbox: MailboxId,
    name: &str,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    require_writes(conn, account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let (account_row, folder) = changeable(conn, account, mailbox)?;
    let new_path = renamed_path(&folder, name)?;
    if new_path == folder.path {
        return Err(StoreError::InvalidArgument(
            "the folder has this name already".into(),
        ));
    }
    let taken = conn
        .prepare_cached(PATH_TAKEN)?
        .query_row(params![account_row, new_path], |_| Ok(()))
        .optional()?;
    if taken.is_some() || new_path.eq_ignore_ascii_case("INBOX") {
        return Err(StoreError::Action(ActionError::MailboxExists(new_path)));
    }
    let mut recorder = Recorder::new(
        conn,
        account_row,
        account,
        ActionKind::RenameMailbox,
        request,
        now,
    );
    recorder.record(OperationPayload::RenameMailbox {
        mailbox: TargetMailbox {
            mailbox_id: mailbox,
            name: folder.name,
        },
        path: folder.path,
        new_path,
    })?;
    Ok(recorder.change)
}

/// See [`Store::enqueue_delete_mailbox`].
pub(crate) fn enqueue_delete(
    conn: &Connection,
    account: AccountId,
    mailbox: MailboxId,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    require_writes(conn, account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let (account_row, folder) = changeable(conn, account, mailbox)?;
    let subfolder = conn
        .prepare_cached(HAS_SUBFOLDER)?
        .query_row([mailbox.0], |_| Ok(()))
        .optional()?;
    if subfolder.is_some() {
        return Err(StoreError::Action(ActionError::MailboxHasSubfolders(
            mailbox,
        )));
    }
    let mut recorder = Recorder::new(
        conn,
        account_row,
        account,
        ActionKind::DeleteMailbox,
        request,
        now,
    );
    recorder.record(OperationPayload::DeleteMailbox {
        mailbox: TargetMailbox {
            mailbox_id: mailbox,
            name: folder.name,
        },
        path: folder.path,
    })?;
    Ok(recorder.change)
}

/// After the server renamed the folder: its row and those of its subfolders take the new wire
/// name and path (so the next folder list finds them, with their messages and UIDVALIDITY), and
/// favourites and colours move along. Does nothing when another row has the new name already
/// (the folder list then sorts it out).
pub(crate) fn rename_rows(
    conn: &Connection,
    mailbox: MailboxId,
    new_name: &str,
    new_path: &str,
) -> Result<bool, StoreError> {
    let Some(folder) = conn
        .prepare_cached(FOLDER)?
        .query_row([mailbox.0], |r| {
            Ok(Folder {
                account_row: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                delimiter: r.get(3)?,
                has_role: r.get(4)?,
            })
        })
        .optional()?
    else {
        return Ok(false);
    };
    let clash: Option<i64> = conn
        .prepare_cached("SELECT id FROM mailbox WHERE account_id = ?1 AND name = ?2 AND id <> ?3")?
        .query_row(params![folder.account_row, new_name, mailbox.0], |r| {
            r.get(0)
        })
        .optional()?;
    if clash.is_some() {
        return Ok(false);
    }
    let account_uuid: String = conn
        .prepare_cached("SELECT uuid FROM account WHERE id = ?1")?
        .query_row([folder.account_row], |r| r.get(0))?;
    conn.prepare_cached("UPDATE mailbox SET name = ?2, path = ?3 WHERE id = ?1")?
        .execute(params![mailbox.0, new_name, new_path])?;
    conn.prepare_cached(
        "UPDATE mailbox_pref SET path = ?3, updated_at = ?4 WHERE account_uuid = ?1 AND path = ?2",
    )?
    .execute(params![account_uuid, folder.path, new_path, now_millis()])?;
    if let Some(delimiter) = folder.delimiter.as_deref().filter(|d| !d.is_empty()) {
        let old_name = format!("{}{delimiter}", folder.name);
        let old_path = format!("{}{delimiter}", folder.path);
        conn.prepare_cached(
            "UPDATE mailbox
             SET name = ?2 || substr(name, length(?3) + 1), path = ?4 || substr(path, length(?5) + 1)
             WHERE account_id = ?1 AND substr(name, 1, length(?3)) = ?3
               AND substr(path, 1, length(?5)) = ?5",
        )?
        .execute(params![
            folder.account_row,
            format!("{new_name}{delimiter}"),
            old_name,
            format!("{new_path}{delimiter}"),
            old_path,
        ])?;
        conn.prepare_cached(
            "UPDATE mailbox_pref SET path = ?2 || substr(path, length(?3) + 1), updated_at = ?4
             WHERE account_uuid = ?1 AND substr(path, 1, length(?3)) = ?3",
        )?
        .execute(params![
            account_uuid,
            format!("{new_path}{delimiter}"),
            old_path,
            now_millis()
        ])?;
    }
    Ok(true)
}

impl Store {
    /// Records the rename of a folder of the user's own to `name` within its parent (see the
    /// module docs). Refused on accounts without writes (`read_only`), for folders with a role,
    /// for a name taken by another folder and while a change of the folder is on its way.
    pub async fn enqueue_rename_mailbox(
        &self,
        account: AccountId,
        mailbox: MailboxId,
        name: String,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| enqueue_rename(tx, account, mailbox, &name, &request, now_millis()))
            .await
    }

    /// Records the deletion of a folder of the user's own with its messages (see the module
    /// docs). Refused like a rename, and for folders with subfolders.
    pub async fn enqueue_delete_mailbox(
        &self,
        account: AccountId,
        mailbox: MailboxId,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| enqueue_delete(tx, account, mailbox, &request, now_millis()))
            .await
    }

    /// After the server confirmed a rename: see [`rename_rows`]. Whether the rows changed.
    pub async fn rename_mailbox_rows(
        &self,
        mailbox: MailboxId,
        new_name: String,
        new_path: String,
    ) -> Result<bool, StoreError> {
        self.write(move |tx| rename_rows(tx, mailbox, &new_name, &new_path))
            .await
    }
}
