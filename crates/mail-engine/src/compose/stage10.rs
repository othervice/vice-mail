//! Stage 10 commands of [`AppCore`] (decision 0023): sending later, missed scheduled sends,
//! the server's size limit, inline images, pasted HTML.

use std::path::PathBuf;

use mail_model::{
    CleanedPaste, CoreError, DraftId, DraftSizeCheck, InlineImagesAdded, MissedSendDecision,
    OperationId, OutgoingMail,
};

use crate::AppCore;
use crate::core::Connector;

impl<C: Connector> AppCore<C> {
    /// "Send later": freezes the draft and queues its send for `send_at` (Unix ms, in the
    /// future, at most a year ahead). It goes out only if the app runs and is online then.
    pub async fn schedule_send(
        &self,
        id: DraftId,
        request_id: String,
        send_at: i64,
    ) -> Result<OutgoingMail, CoreError> {
        self.compose_ref()?
            .send_at(id, request_id, Some(send_at))
            .await
    }

    /// Scheduled sends of every account that wait (`scheduled`) or were missed (`missed`), the
    /// earliest first.
    pub async fn list_scheduled_sends(&self) -> Result<Vec<OutgoingMail>, CoreError> {
        self.compose_ref()?.scheduled_sends().await
    }

    /// Moves a scheduled send that has not started to `send_at`, or with `None` sends it now
    /// (after the undo delay). `send_in_progress` once it started or for other operations.
    pub async fn reschedule_send(
        &self,
        operation: OperationId,
        send_at: Option<i64>,
    ) -> Result<OutgoingMail, CoreError> {
        self.compose_ref()?.reschedule(operation, send_at).await
    }

    /// The user's answer to a scheduled send missed while the app was off. `None` after
    /// `keepAsDraft` (the draft is editable again).
    pub async fn resolve_missed_send(
        &self,
        operation: OperationId,
        decision: MissedSendDecision,
    ) -> Result<Option<OutgoingMail>, CoreError> {
        self.compose_ref()?
            .resolve_missed(operation, decision)
            .await
    }

    /// The draft's size against the SMTP server's SIZE limit (asked once per session).
    pub async fn check_draft_size(&self, id: DraftId) -> Result<DraftSizeCheck, CoreError> {
        self.compose_ref()?.size_check(id).await
    }

    /// Copies image files (paths from the system dialog or a drop on the window) into the
    /// draft as inline images.
    pub async fn add_inline_images(
        &self,
        id: DraftId,
        paths: Vec<PathBuf>,
    ) -> Result<InlineImagesAdded, CoreError> {
        self.compose_ref()?.add_inline_images(id, paths).await
    }

    /// An inline image of a draft as a `data:` URL for the editor.
    pub async fn inline_image_data(
        &self,
        id: DraftId,
        attachment_id: String,
    ) -> Result<String, CoreError> {
        self.compose_ref()?
            .inline_image_data(id, attachment_id)
            .await
    }

    /// Cleans pasted HTML (Word, web pages) for the editor; nothing is stored.
    pub async fn clean_pasted_html(&self, html: String) -> Result<CleanedPaste, CoreError> {
        let cleaned = tokio::task::spawn_blocking(move || mail_content::clean_pasted_html(&html))
            .await
            .map_err(|err| CoreError::Internal(format!("paste cleaning failed: {err}")))?;
        match cleaned {
            Ok(pasted) => Ok(CleanedPaste {
                html: pasted.html,
                images_removed: u32::try_from(pasted.notes.images_removed).unwrap_or(u32::MAX),
                formatting_simplified: pasted.notes.formatting_simplified,
            }),
            Err(mail_content::SnippetError::TooLarge { bytes, max }) => {
                Err(CoreError::InvalidArgument(format!(
                    "pasted content of {bytes} bytes; at most {max}"
                )))
            }
        }
    }
}
