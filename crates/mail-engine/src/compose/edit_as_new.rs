//! Stage 19 (decision 0055): "edit as new" ("Als neu bearbeiten"): a new local draft with the
//! recipients, subject, text and attachments of a stored message, also one from Sent.
//!
//! Unlike opening a message of the Drafts folder ([`Compose::open_from_message`]), the draft is
//! a new message: its own Message-ID, no In-Reply-To and no References, no server copy to
//! replace, and the message stays where it is. The body and attachments are taken over like a
//! draft of another program ([`Compose::copy_content`]: the HTML for the editor with the inline
//! images it shows, every other part as an attachment). The identity is the one that sent the
//! message (mail from Sent) or the one it went to, else the account's default. No signature is
//! added: the text carries the one it had.

use mail_model::{CoreError, DraftError, DraftId, DraftKind, MessageId, QuotePosition};
use mail_store::{DraftContent, NewDraftRow, StoredDraft};

use super::{Compose, blocking, now_ms, random_bytes, random_u128, synthetic_original};
use crate::core::Connector;

impl<C: Connector> Compose<C> {
    pub(super) async fn create_edit_as_new(
        &self,
        message: MessageId,
    ) -> Result<StoredDraft, CoreError> {
        let store = &self.inner.store;
        let summary = store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let account = summary.account_id;
        let identities = store.identities(account).await?;
        let (raw, text) = self.original_raw(message).await?;
        let whole = raw.is_some();
        let raw =
            raw.unwrap_or_else(|| synthetic_original(&summary, text.as_deref().unwrap_or("")));
        let parsed = {
            let raw = raw.clone();
            blocking(move || mail_content::parse(&raw)).await?
        };
        let headers = &parsed.headers;
        let identity = mail_content::choose_identity(&identities, headers, &[])
            .cloned()
            .ok_or_else(|| {
                CoreError::Draft(DraftError::NoIdentity("the account has none".into()))
            })?;
        let id = DraftId::from_random(random_u128());
        let lock = self.inner.outbox.files_lock();
        let _held = lock.lock().await;
        let copied = self
            .copy_content(&id, &raw, &parsed, &identity.email)
            .await?;
        let row = NewDraftRow {
            id,
            account,
            identity: Some(identity.id),
            kind: DraftKind::New,
            message_id_header: mail_content::new_message_id(&identity.email, random_bytes()),
            original: None,
            original_message_id_header: None,
            in_reply_to: None,
            references: Vec::new(),
            content: DraftContent {
                identity: Some(identity.id),
                to: headers.to.clone(),
                cc: headers.cc.clone(),
                bcc: headers.bcc.clone(),
                subject: headers.subject.clone().unwrap_or_default(),
                format: copied.format,
                body_html: copied.body_html,
                body_text: copied.body_text,
                include_quote: false,
                quote_position: QuotePosition::Above,
                include_signature: false,
                signature: None,
            },
            quote_html: None,
            quote_text: None,
            attachments: copied.attachments,
            server_copy: None,
            kept_original: None,
            now: now_ms(),
        };
        let draft = store.create_draft(row).await?;
        tracing::info!(
            %account,
            whole,
            attachments = draft.attachments.len(),
            lossy = copied.lossy,
            "draft created as a copy of a message"
        );
        Ok(draft)
    }
}
