//! Audit 24.11.2026: renaming and deleting folders of the user's own (see
//! `mail_store::folders`). Both go through the outbox like every write, are refused on accounts
//! without writes (`read_only`) and for folders with a role, and show as pending operations
//! until the server confirmed. JMAP accounts cannot do either yet: the JMAP adapter has no
//! `Mailbox/set` rename or destroy behind the writer trait, so the core refuses them with
//! [`ActionError::FolderChangesUnsupported`] before anything is recorded.

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
