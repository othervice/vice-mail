//! Stage 19 (decision 0055): redirecting a message ("Umleiten").
//!
//! The original goes unchanged to new recipients with a block of `Resent-*` fields in front
//! (`mail_content::build_redirect`, RFC 5322 section 3.6.6), from one of the account's
//! identities. Like every send it is a `send` operation of the outbox, recorded with the undo
//! delay of the compose settings (`not_before`) and shown in the send notices and the outbox
//! view; "undo" within the delay is [`AppCore::cancel_redirect`]. Only for accounts with writes
//! enabled. No draft, no copy in Sent (the copy would look like the original sender's mail);
//! the original gets `$Forwarded` once the server accepted the redirect.

use mail_content::Redirect;
use mail_model::{
    Address, CoreError, DraftError, DraftId, MAX_REDIRECT_RECIPIENTS, OperationId,
    OperationPayload, OriginalRef, OutgoingMail, RedirectRequest, SendJob, SendPhase,
};

use super::{
    Compose, blocking, build, clean_addresses, files, random_bytes, random_u128, valid_request_id,
};
use crate::AppCore;
use crate::core::Connector;

impl<C: Connector> Compose<C> {
    /// See [`AppCore::redirect_message`].
    pub(crate) async fn redirect(
        &self,
        request: RedirectRequest,
        request_id: String,
    ) -> Result<OutgoingMail, CoreError> {
        valid_request_id(&request_id)?;
        let store = &self.inner.store;
        let to = clean_addresses(&request.to);
        let cc = clean_addresses(&request.cc);
        let bcc = clean_addresses(&request.bcc);
        let count = to.len() + cc.len() + bcc.len();
        if count == 0 {
            return Err(CoreError::Draft(DraftError::NoRecipients));
        }
        if count > usize::try_from(MAX_REDIRECT_RECIPIENTS).unwrap_or(usize::MAX) {
            return Err(CoreError::InvalidArgument(format!(
                "a redirect has at most {MAX_REDIRECT_RECIPIENTS} recipients"
            )));
        }
        let summary = store
            .message_summary(request.message_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {}", request.message_id)))?;
        let account = store
            .account(summary.account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", summary.account_id)))?;
        if !account.writes_enabled {
            return Err(CoreError::Action(mail_model::ActionError::ReadOnly(
                account.id,
            )));
        }
        let identities: Vec<mail_model::Identity> = store
            .compose_identities(account.id)
            .await?
            .into_iter()
            .map(|(identity, _)| identity)
            .collect();
        let raw = self.inner.reading.raw_message(request.message_id).await?;
        let identity = match request.identity_id {
            Some(id) => identities
                .iter()
                .find(|i| i.id == id)
                .cloned()
                .ok_or_else(|| {
                    CoreError::Draft(DraftError::NoIdentity(format!(
                        "identity {} is not one of the account's",
                        id.0
                    )))
                })?,
            None => {
                let original = mail_content::Original::parse(&raw);
                mail_content::choose_identity(
                    &identities,
                    &original.message.headers,
                    &original.delivered_to,
                )
                .or_else(|| identities.iter().find(|i| i.is_default))
                .or_else(|| identities.first())
                .cloned()
                .ok_or_else(|| {
                    CoreError::Draft(DraftError::NoIdentity("the account has none".into()))
                })?
            }
        };
        let now = self.now();
        let name = identity.name.trim();
        let resent_id = mail_content::new_message_id(&identity.email, random_bytes());
        let redirect = Redirect {
            from: Address {
                name: (!name.is_empty()).then(|| name.to_owned()),
                email: identity.email.clone(),
            },
            to: to.clone(),
            cc: cc.clone(),
            bcc,
            date: now / 1000,
            message_id: resent_id.clone(),
        };
        let built = blocking(move || {
            mail_content::build_redirect(&raw, &redirect).map_err(build::compose_error)
        })
        .await??;
        let shown: Vec<Address> = to.iter().chain(&cc).take(5).cloned().collect();
        let id = DraftId::from_random(random_u128());
        let file = files::outgoing_path(&id, files::MESSAGE_FILE);
        let job = SendJob {
            draft: id.clone(),
            message_id_header: resent_id,
            from: built.envelope.from.clone(),
            recipients: built.envelope.recipients.clone(),
            file: file.clone(),
            sent_file: file.clone(),
            subject: summary.subject.clone(),
            to: shown,
            recipient_count: u32::try_from(built.envelope.recipients.len()).unwrap_or(u32::MAX),
            server_saves_sent: false,
            original: summary
                .message_id_header
                .clone()
                .map(|message_id_header| OriginalRef {
                    message_id_header,
                    forwarded: true,
                }),
            phase: SendPhase::Submit,
            checks: 0,
            scheduled: false,
            archive_original: false,
            archive_missing: false,
            receipt_for: None,
            redirect: true,
        };
        let delay = self.compose_settings().await?.send_delay_seconds;
        let not_before = (delay > 0).then(|| now + i64::from(delay) * 1000);
        let lock = self.inner.outbox.files_lock();
        let held = lock.lock().await;
        let root = self.inner.root.clone();
        let bytes = built.bytes;
        blocking(move || files::write_atomically(&files::resolve(&root, &file)?, &bytes)).await??;
        let change = match store
            .enqueue_redirect(account.id, job, not_before, request_id)
            .await
        {
            Ok(change) => change,
            Err(err) => {
                files::remove_outgoing_dir(&self.inner.root, &id);
                return Err(err.into());
            }
        };
        if change.repeated {
            // The same request again: the redirect recorded the first time stands.
            files::remove_outgoing_dir(&self.inner.root, &id);
        }
        drop(held);
        self.inner.outbox.changed(&change);
        let op = match change
            .operations
            .iter()
            .find(|op| matches!(&op.payload, OperationPayload::Send(job) if job.redirect))
        {
            Some(op) => op.clone(),
            None => {
                // The same request again, and that redirect left the outbox already.
                return Err(CoreError::Action(
                    mail_model::ActionError::OperationNotFound(OperationId(0)),
                ));
            }
        };
        tracing::info!(
            account = %account.id,
            recipients = count,
            delayed = not_before.is_some(),
            "redirect queued"
        );
        super::outgoing::outgoing_mail(&op, self.now())
            .ok_or_else(|| CoreError::Internal("the redirect was not recorded".into()))
    }

    /// See [`AppCore::cancel_redirect`].
    pub(crate) async fn cancel_redirect(&self, operation: OperationId) -> Result<(), CoreError> {
        let store = &self.inner.store;
        let op = store
            .operation(operation)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("operation {}", operation.0)))?;
        let change = store.cancel_redirect(operation).await?;
        self.inner.outbox.changed(&change);
        if let OperationPayload::Send(job) = &op.payload {
            let root = self.inner.root.clone();
            let draft = job.draft.clone();
            blocking(move || files::remove_outgoing_dir(&root, &draft)).await?;
        }
        tracing::info!(account = %change.account, "redirect cancelled before it was sent");
        Ok(())
    }
}

impl<C: Connector> AppCore<C> {
    /// Stage 19: redirects a message unchanged to new recipients with `Resent-*` fields
    /// (decision 0055). Queued in the outbox after the undo delay, like a send; `request_id`
    /// names the press of "Redirect" (the same id twice queues once). Refused before anything
    /// is queued: `read_only` without writes, `no_recipients`, `invalid_address`,
    /// `no_identity` for an identity of another account, `invalid_argument` over
    /// [`MAX_REDIRECT_RECIPIENTS`], `not_found`.
    pub async fn redirect_message(
        &self,
        request: RedirectRequest,
        request_id: String,
    ) -> Result<OutgoingMail, CoreError> {
        self.compose_ref()?.redirect(request, request_id).await
    }

    /// Stage 19: "undo" of a redirect within the delay, or dropping a failed one: it leaves the
    /// outbox and nothing is sent. `send_in_progress` once the submission started or it was
    /// sent.
    pub async fn cancel_redirect(&self, operation: OperationId) -> Result<(), CoreError> {
        self.compose_ref()?.cancel_redirect(operation).await
    }
}
