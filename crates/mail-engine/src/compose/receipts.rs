//! Stage 16 (decision 0033): the send options of a draft ("send and archive", asking for a
//! read receipt) and read receipts of incoming mail (RFC 8098).
//!
//! **Never by itself.** Nothing here runs on arrival: a read receipt is built and queued only by
//! [`AppCore::send_read_receipt`], the user's explicit answer in the reading pane. With the
//! setting `never` that command is refused and the pane offers nothing. The receipt goes through
//! the outbox like any send (`send` operation, the account's SMTP server), without a draft and
//! without a copy in Sent; once accepted the original gets `$MDNSent` (RFC 3503) where the
//! account may write, so other programs do not ask again. Declining records the answer (and
//! sets `$MDNSent` as well).
//!
//! **Received receipts** are recorded when their body is stored (`index_text`), for the
//! Message-ID they report on; [`AppCore::get_read_receipt`] of the sent message lists them.

use mail_content::{DispositionReport, Original, OutgoingMessage};
use mail_model::{
    Address, CoreError, Draft, DraftError, DraftId, DraftSendOptions, MessageId, OperationPayload,
    OperationState, OutgoingMail, QuoteLanguage, ReadReceiptAnswer, ReadReceiptInfo,
    ReadReceiptPolicy, ReadReceiptRequest, SendJob, SendPhase, SentCopyMode,
};
use mail_store::{MDN_SENT_KEYWORD, StoredMessage};

use super::{
    Compose, blocking, build, files, files_sent_itself, outgoing, random_bytes, random_u128,
    valid_request_id,
};
use crate::AppCore;
use crate::core::Connector;

fn content_language(language: QuoteLanguage) -> mail_content::QuoteLanguage {
    match language {
        QuoteLanguage::En => mail_content::QuoteLanguage::English,
        QuoteLanguage::De => mail_content::QuoteLanguage::German,
    }
}

impl<C: Connector> Compose<C> {
    /// See [`AppCore::set_draft_send_options`].
    pub(crate) async fn set_send_options(
        &self,
        id: DraftId,
        options: DraftSendOptions,
    ) -> Result<Draft, CoreError> {
        let draft = self
            .inner
            .store
            .set_draft_send_options(
                id.clone(),
                options.archive_original,
                options.request_read_receipt,
            )
            .await?;
        // The Drafts copy carries the read-receipt request: it is saved again after a pause.
        self.schedule(id.clone(), self.inner.config.draft_save_delay);
        self.drafts_changed(draft.account, vec![id], Vec::new());
        self.info(draft).await
    }

    async fn summary(&self, message: MessageId) -> Result<StoredMessage, CoreError> {
        self.inner
            .store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))
    }

    /// The message's sender is one of the user's own addresses.
    async fn sent_by_user(&self, summary: &StoredMessage) -> Result<bool, CoreError> {
        let Some(from) = &summary.from else {
            return Ok(false);
        };
        let Ok(from) = mail_content::normalize_email(&from.email) else {
            return Ok(false);
        };
        let identities = self
            .inner
            .store
            .compose_identities(summary.account_id)
            .await?;
        Ok(identities.iter().any(|(identity, _)| {
            mail_content::normalize_email(&identity.email)
                .is_ok_and(|own| own.eq_ignore_ascii_case(&from))
        }))
    }

    /// The answer to the request of `summary`'s message as the UI shows it.
    async fn answer(
        &self,
        summary: &StoredMessage,
        message_id_header: &str,
        policy: ReadReceiptPolicy,
    ) -> Result<ReadReceiptAnswer, CoreError> {
        let store = &self.inner.store;
        if let Some(answer) = store
            .receipt_answer(summary.account_id, message_id_header.to_owned())
            .await?
        {
            match answer.state.as_str() {
                "sent" => return Ok(ReadReceiptAnswer::Sent),
                "declined" => return Ok(ReadReceiptAnswer::Declined),
                // Queued: while its send is pending (a failed, discarded or cancelled one may
                // be answered again).
                _ => {
                    if answer
                        .operation_state
                        .is_some_and(|s| s.is_pending() || s == OperationState::Confirmed)
                    {
                        return Ok(ReadReceiptAnswer::Queued);
                    }
                }
            }
        }
        if policy == ReadReceiptPolicy::Never {
            return Ok(ReadReceiptAnswer::Off);
        }
        let answered_elsewhere = store
            .message_items(vec![summary.id])
            .await?
            .iter()
            .any(|item| {
                item.keywords
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(MDN_SENT_KEYWORD))
            });
        Ok(if answered_elsewhere {
            ReadReceiptAnswer::AnsweredElsewhere
        } else {
            ReadReceiptAnswer::Ask
        })
    }

    /// See [`AppCore::get_read_receipt`].
    pub(crate) async fn read_receipt_info(
        &self,
        message: MessageId,
    ) -> Result<ReadReceiptInfo, CoreError> {
        let store = &self.inner.store;
        let summary = self.summary(message).await?;
        let mut info = ReadReceiptInfo::default();
        if let Some(id) = &summary.message_id_header {
            info.received = store
                .received_receipts(summary.account_id, id.clone())
                .await?;
        }
        let Some(raw) = store.load_message(message).await? else {
            return Ok(info);
        };
        if let Some(report) = blocking({
            let raw = raw.clone();
            move || Ok::<_, CoreError>(mail_content::parse_report(&raw))
        })
        .await??
        {
            info.receipt_for = Some(report.original_message_id);
            return Ok(info);
        }
        let request = mail_content::receipt_request(&raw);
        if self.sent_by_user(&summary).await? {
            info.requested = request.is_some();
            return Ok(info);
        }
        if let (Some(request), Some(id)) = (request, &summary.message_id_header) {
            let policy = self.compose_settings().await?.read_receipts;
            info.request = Some(ReadReceiptRequest {
                to: request.address,
                return_path_differs: request.return_path_differs,
                answer: self.answer(&summary, id, policy).await?,
            });
        }
        Ok(info)
    }

    /// The outbox view of the receipt queued for this answer, if it is still there.
    async fn queued_receipt(
        &self,
        summary: &StoredMessage,
        message_id_header: &str,
    ) -> Result<Option<OutgoingMail>, CoreError> {
        let store = &self.inner.store;
        let Some(answer) = store
            .receipt_answer(summary.account_id, message_id_header.to_owned())
            .await?
        else {
            return Ok(None);
        };
        let Some(op) = answer.operation else {
            return Ok(None);
        };
        let Some(op) = store.operation(op).await? else {
            return Ok(None);
        };
        if !(op.state.is_pending() || op.state == OperationState::Confirmed) {
            return Ok(None);
        }
        Ok(outgoing::outgoing_mail(&op, self.now()))
    }

    /// See [`AppCore::send_read_receipt`].
    pub(crate) async fn send_receipt(
        &self,
        message: MessageId,
        language: QuoteLanguage,
        request: String,
    ) -> Result<OutgoingMail, CoreError> {
        valid_request_id(&request)?;
        let store = &self.inner.store;
        if self.compose_settings().await?.read_receipts == ReadReceiptPolicy::Never {
            return Err(CoreError::InvalidArgument(
                "read receipts are set to never: none is sent".into(),
            ));
        }
        let summary = self.summary(message).await?;
        let account = store
            .account(summary.account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", summary.account_id)))?;
        if !account.writes_enabled {
            return Err(CoreError::Action(mail_model::ActionError::ReadOnly(
                account.id,
            )));
        }
        let original_id = summary.message_id_header.clone().ok_or_else(|| {
            CoreError::InvalidArgument("the message has no Message-ID to report on".into())
        })?;
        if let Some(mail) = self.queued_receipt(&summary, &original_id).await? {
            // Answered already (a second press): the receipt that is on its way.
            return Ok(mail);
        }
        let raw = store.load_message(message).await?.ok_or_else(|| {
            CoreError::InvalidArgument("the message is not stored on this device yet".into())
        })?;
        if self.sent_by_user(&summary).await? || mail_content::is_receipt(&raw) {
            return Err(CoreError::InvalidArgument(
                "the message asks for no read receipt".into(),
            ));
        }
        let receipt_request = mail_content::receipt_request(&raw).ok_or_else(|| {
            CoreError::InvalidArgument("the message asks for no read receipt".into())
        })?;
        let identities: Vec<mail_model::Identity> = store
            .compose_identities(summary.account_id)
            .await?
            .into_iter()
            .map(|(identity, _)| identity)
            .collect();
        let identity = {
            let original = Original::parse(&raw);
            mail_content::choose_identity(
                &identities,
                &original.message.headers,
                &original.delivered_to,
            )
            .cloned()
        }
        .ok_or_else(|| CoreError::Draft(DraftError::NoIdentity("the account has none".into())))?;
        let language = content_language(language);
        let now = self.now();
        let name = identity.name.trim();
        let message_id = mail_content::new_message_id(&identity.email, random_bytes());
        let outgoing_message = OutgoingMessage {
            from: Address {
                name: (!name.is_empty()).then(|| name.to_owned()),
                email: identity.email.clone(),
            },
            to: vec![Address {
                name: None,
                email: receipt_request.address.clone(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            subject: mail_content::receipt_subject(&summary.subject, language),
            text: Some(mail_content::receipt_text(
                &summary.subject,
                &identity.email,
                language,
            )),
            html: None,
            attachments: Vec::new(),
            inline_images: Vec::new(),
            message_id: message_id.clone(),
            in_reply_to: Some(original_id.clone()),
            references: vec![original_id.clone()],
            date: now / 1000,
            user_agent: None,
            disposition_notification_to: None,
            report: Some(DispositionReport {
                final_recipient: identity.email.clone(),
                original_recipient: receipt_request.original_recipient.clone(),
                original_message_id: Some(original_id.clone()),
            }),
        };
        let built = blocking(move || {
            mail_content::build_message(&outgoing_message).map_err(build::compose_error)
        })
        .await??;
        let server_saves_sent = match store.sent_copy_mode(summary.account_id).await? {
            SentCopyMode::Server => true,
            SentCopyMode::App => false,
            SentCopyMode::Auto => files_sent_itself(&account.imap.host),
        };
        let id = DraftId::from_random(random_u128());
        let file = files::outgoing_path(&id, files::MESSAGE_FILE);
        let job = SendJob {
            draft: id.clone(),
            message_id_header: message_id,
            from: built.envelope.from.clone(),
            recipients: built.envelope.recipients.clone(),
            file: file.clone(),
            sent_file: file.clone(),
            subject: mail_content::receipt_subject(&summary.subject, language),
            to: vec![Address {
                name: None,
                email: receipt_request.address,
            }],
            recipient_count: u32::try_from(built.envelope.recipients.len()).unwrap_or(u32::MAX),
            server_saves_sent,
            original: None,
            phase: SendPhase::Submit,
            checks: 0,
            scheduled: false,
            archive_original: false,
            archive_missing: false,
            receipt_for: Some(original_id),
        };
        let lock = self.inner.outbox.files_lock();
        let held = lock.lock().await;
        let root = self.inner.root.clone();
        let bytes = built.for_sending;
        blocking(move || files::write_atomically(&files::resolve(&root, &file)?, &bytes)).await??;
        let change = match store
            .enqueue_receipt(summary.account_id, job, request)
            .await
        {
            Ok(change) => change,
            Err(err) => {
                files::remove_outgoing_dir(&self.inner.root, &id);
                return Err(err.into());
            }
        };
        drop(held);
        self.inner.outbox.changed(&change);
        let op = change
            .operations
            .iter()
            .find(|op| matches!(op.payload, OperationPayload::Send(_)))
            .cloned();
        tracing::info!(account = %summary.account_id, "read receipt queued on the user's answer");
        match op {
            Some(op) => outgoing::outgoing_mail(&op, self.now())
                .ok_or_else(|| CoreError::Internal("the receipt was not recorded".into())),
            // The same request again: the receipt recorded the first time.
            None => self
                .queued_receipt(&summary, change_original(&summary)?)
                .await?
                .ok_or_else(|| CoreError::Internal("the receipt was not recorded".into())),
        }
    }

    /// See [`AppCore::decline_read_receipt`].
    pub(crate) async fn decline_receipt(
        &self,
        message: MessageId,
        request: String,
    ) -> Result<ReadReceiptInfo, CoreError> {
        valid_request_id(&request)?;
        let summary = self.summary(message).await?;
        let original_id = change_original(&summary)?.to_owned();
        if self.queued_receipt(&summary, &original_id).await?.is_some() {
            // A receipt is on its way already: that answer stands.
            return self.read_receipt_info(message).await;
        }
        let change = self
            .inner
            .store
            .decline_receipt(summary.account_id, original_id, request)
            .await?;
        self.inner.outbox.changed(&change);
        self.read_receipt_info(message).await
    }
}

fn change_original(summary: &StoredMessage) -> Result<&str, CoreError> {
    summary
        .message_id_header
        .as_deref()
        .ok_or_else(|| CoreError::InvalidArgument("the message has no Message-ID".into()))
}

impl<C: Connector> AppCore<C> {
    /// Stage 16: the draft's send options ("send and archive", ask for a read receipt).
    /// `send_in_progress` while the draft is frozen.
    pub async fn set_draft_send_options(
        &self,
        id: DraftId,
        options: DraftSendOptions,
    ) -> Result<Draft, CoreError> {
        self.compose_ref()?.set_send_options(id, options).await
    }

    /// Stage 16: read receipts of a message: an incoming request with the user's answer so far,
    /// whether a sent message asked for one, and the receipts that came back for it.
    pub async fn get_read_receipt(&self, message: MessageId) -> Result<ReadReceiptInfo, CoreError> {
        self.compose_ref()?.read_receipt_info(message).await
    }

    /// Stage 16: the user's explicit answer "send" to a read-receipt request. The only way a
    /// read receipt is ever sent. Refused (`invalid_argument`) with the setting `never`, for a
    /// message that asks for none, for the user's own mail and for receipts; `read_only`
    /// without writes. Pressed twice: the receipt already on its way.
    pub async fn send_read_receipt(
        &self,
        message: MessageId,
        language: QuoteLanguage,
        request_id: String,
    ) -> Result<OutgoingMail, CoreError> {
        self.compose_ref()?
            .send_receipt(message, language, request_id)
            .await
    }

    /// Stage 16: the user's answer "do not send" to a read-receipt request. Nothing is sent;
    /// `$MDNSent` goes on the message where the account may write. A receipt already on its
    /// way stays (the answer is then `queued`).
    pub async fn decline_read_receipt(
        &self,
        message: MessageId,
        request_id: String,
    ) -> Result<ReadReceiptInfo, CoreError> {
        self.compose_ref()?
            .decline_receipt(message, request_id)
            .await
    }
}
