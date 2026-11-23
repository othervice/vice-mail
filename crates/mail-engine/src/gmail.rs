//! Stage 14, block A (decision 0034): Gmail over IMAP in the core.
//!
//! The adapter reads `X-GM-MSGID` and `X-GM-LABELS` on Gmail (`X-GM-EXT-1`), the store keeps
//! one message across its labels and the labels with it, and the sync records at login whether
//! the server is Gmail. Label changes are ordinary actions (`MailAction::AddLabel`,
//! `RemoveLabel`) through the outbox; archive on Gmail removes the inbox label. This module
//! only lets the UI read a message's labels.

use mail_model::{ActionResult, CoreError, MailAction, MessageId};

use crate::AppCore;
use crate::core::Connector;

impl<C: Connector> AppCore<C> {
    /// Stage 14: the Gmail labels of a message, system labels as `\Inbox`, `\Important`,
    /// `\Starred`, ...; `None` for messages of other servers (and unknown ids). Changing them is
    /// `apply_action` with `addLabel`/`removeLabel` (writes on, Gmail only).
    pub async fn get_message_labels(
        &self,
        message: MessageId,
    ) -> Result<Option<Vec<String>>, CoreError> {
        Ok(self.store_ref()?.message_labels(message).await?)
    }

    /// Stage 14: adds (`add`) or removes a Gmail label of one message, for the reading pane.
    /// The core picks the location the change is sent from ("All Mail" if known) and applies
    /// `addLabel`/`removeLabel` like any action (outbox, undo by batch). `not_found` if the
    /// message has no location; `unsupported_server` off Gmail; `read_only` with writes off.
    pub async fn set_message_label(
        &self,
        message: MessageId,
        label: String,
        add: bool,
        request_id: String,
    ) -> Result<ActionResult, CoreError> {
        let at = self
            .store_ref()?
            .label_location(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {}", message.0)))?;
        let action = if add {
            MailAction::AddLabel { label }
        } else {
            MailAction::RemoveLabel { label }
        };
        self.apply_action(action, vec![at], request_id).await
    }
}
