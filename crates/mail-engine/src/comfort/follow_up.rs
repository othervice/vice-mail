//! Stage 20, task B (decision 0059): "Remind me if no reply by …", as methods of [`AppCore`].
//!
//! - Set in the composer (`DraftSendOptions::follow_up_at`, recorded when the server accepted
//!   the message) or on any stored message with a Message-ID (`setFollowUp`, also to change
//!   the time). Local like a snooze, synced as kind `followUp`; nothing goes to the server.
//! - **Answered:** a message of the conversation (stage 7 threading, any folder of the account)
//!   from someone else than the user, newer than the sending. Looked for when new mail of the
//!   account is stored (`messagesChanged`, at most once a second per account) and when the
//!   reminder is due; an answer removes the reminder (waiting or returned).
//! - **Due without an answer:** it comes back above the inbox ("No reply since …",
//!   `InboxTop::follow_ups`) and stays until the user clears it, sets a new time or an answer
//!   comes. The snooze timer brings it back (the same wall-clock checks).

use std::collections::HashSet;

use mail_model::{
    AccountId, CoreError, FollowUpChoice, FollowUpResult, FollowUpTimes, MarkScope, MessageId,
    MessageListItem,
};
use mail_store::{MAX_FOLLOW_UP_VIEW, SettleScope};

use super::{check_messages, time};
use crate::core::{AppCore, Connector};

impl<C: Connector> AppCore<C> {
    /// The times the reminder menu offers now, in the user's zone.
    pub fn follow_up_times(&self) -> FollowUpTimes {
        time::follow_up_times(self.now_ms(), &self.comfort().zone())
    }

    /// Sets (or moves) "remind me if no reply" on messages. Returns the time and how many of
    /// them already have an answer (those keep no reminder). `invalid_argument` for a message
    /// without Message-ID or a time not ahead.
    pub async fn set_follow_up(
        &self,
        messages: Vec<MessageId>,
        choice: FollowUpChoice,
    ) -> Result<FollowUpResult, CoreError> {
        check_messages(&messages)?;
        let now = self.now_ms();
        let due = time::follow_up_due(choice, now, &self.comfort().zone())?;
        let store = self.store_ref()?;
        let mut accounts = HashSet::new();
        for message in &messages {
            let (stored, _) = store.set_follow_up(*message, due, now).await?;
            accounts.insert(stored.key.account);
        }
        let mut answered = 0u32;
        for account in &accounts {
            let settled = store
                .settle_follow_ups(now, SettleScope::Account(*account))
                .await?;
            answered = answered.saturating_add(u32::try_from(settled.answered.len()).unwrap_or(0));
        }
        tracing::info!(count = messages.len(), answered, "reminders set");
        self.comfort().wake_snoozes();
        self.inbox_changed(&accounts);
        Ok(FollowUpResult { due, answered })
    }

    /// Removes the reminders of messages (waiting or returned). Messages without one are left
    /// alone.
    pub async fn clear_follow_up(&self, messages: Vec<MessageId>) -> Result<(), CoreError> {
        check_messages(&messages)?;
        let store = self.store_ref()?;
        let mut accounts = HashSet::new();
        for message in &messages {
            if let Some(removed) = store.remove_follow_up(*message).await? {
                accounts.insert(removed.key.account);
            }
        }
        self.comfort().wake_snoozes();
        self.inbox_changed(&accounts);
        Ok(())
    }

    /// "Waiting for reply": the reminders of the scope, the next due first, then the ones that
    /// came back (at most 1,000), each shown where it lies (usually Sent).
    pub async fn list_follow_ups(
        &self,
        scope: MarkScope,
    ) -> Result<Vec<MessageListItem>, CoreError> {
        let (accounts, _) = self.mark_scope(scope).await?;
        Ok(self
            .store_ref()?
            .follow_up_rows(accounts, false, MAX_FOLLOW_UP_VIEW)
            .await?)
    }

    /// Settles the reminders of `scope`: answered ones go, due ones come back. Called by the
    /// snooze timer (`Due`) and after new mail of an account (`Account`). Returns how many
    /// changed.
    pub(crate) async fn settle_follow_ups(&self, scope: SettleScope) -> Result<usize, CoreError> {
        let store = self.store_ref()?;
        let settled = store.settle_follow_ups(self.now_ms(), scope).await?;
        let changed = settled.returned.len() + settled.answered.len();
        if changed == 0 {
            return Ok(0);
        }
        let accounts: HashSet<AccountId> = settled
            .returned
            .iter()
            .map(|(r, _)| r.key.account)
            .chain(settled.answered.iter().map(|a| a.key.account))
            .collect();
        tracing::info!(
            returned = settled.returned.len(),
            answered = settled.answered.len(),
            "reminders settled"
        );
        self.inbox_changed(&accounts);
        Ok(changed)
    }
}
