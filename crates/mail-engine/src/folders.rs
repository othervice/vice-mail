//! Audit 24.11.2026: renaming and deleting folders of the user's own (see
//! `mail_store::folders`). Both go through the outbox like every write, are refused on accounts
//! without writes (`read_only`) and for folders with a role, and show as pending operations
//! until the server confirmed. JMAP accounts cannot do either yet: the JMAP adapter has no
//! `Mailbox/set` rename or destroy behind the writer trait, so the core refuses them with
//! [`ActionError::FolderChangesUnsupported`] before anything is recorded.
//!
//! Stage 21 (decision 0063): marking folders as read, also on JMAP accounts.

use mail_model::{AccountId, ActionError, ActionResult, CoreError, MailboxId, Protocol};

use crate::AppCore;
use crate::core::Connector;

impl<C: Connector> AppCore<C> {
    /// Renames a folder of the user's own to `name` (one level, within its parent; subfolders
    /// move along). Refused before anything changes: `read_only`, `invalid_argument` (a role,
    /// an invalid or taken name, a change of the folder on its way), `unsupported_server`
    /// (JMAP), `not_found`.
    pub async fn rename_mailbox(
        &self,
        account: AccountId,
        mailbox: MailboxId,
        name: String,
    ) -> Result<ActionResult, CoreError> {
        self.folder_changes_supported(account).await?;
        self.outbox_ref()?
            .rename_mailbox(account, mailbox, name)
            .await
    }

    /// Deletes a folder of the user's own, without subfolders, with its messages (the UI asks
    /// first). Refused like [`AppCore::rename_mailbox`], and for a folder with subfolders.
    pub async fn delete_mailbox(
        &self,
        account: AccountId,
        mailbox: MailboxId,
    ) -> Result<ActionResult, CoreError> {
        self.folder_changes_supported(account).await?;
        self.outbox_ref()?.delete_mailbox(account, mailbox).await
    }

    /// Stage 21 (decision 0063): marks every unread message of these folders of `account` as
    /// read ("Mark folder as read", and "Mark all as read" on the account: its inbox and own
    /// folders). Local at once, then one operation per folder with the UIDs that were unread, in
    /// one batch for [`AppCore::undo_action`]; IMAP and JMAP alike. `request_id` as in
    /// [`AppCore::apply_action`]. Refused before anything changes: `read_only`, `not_found` (a
    /// folder of another account), `invalid_argument`.
    pub async fn mark_mailboxes_read(
        &self,
        account: AccountId,
        mailboxes: Vec<MailboxId>,
        request_id: String,
    ) -> Result<ActionResult, CoreError> {
        crate::core::check_request_id(&request_id)?;
        self.outbox_ref()?
            .mark_read(account, mailboxes, request_id)
            .await
    }

    async fn folder_changes_supported(&self, account: AccountId) -> Result<(), CoreError> {
        let stored = self
            .store_ref()?
            .account(account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {account}")))?;
        if stored.protocol == Protocol::Jmap {
            return Err(CoreError::Action(ActionError::FolderChangesUnsupported));
        }
        Ok(())
    }
}
