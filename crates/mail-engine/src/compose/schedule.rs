//! Stage 10 (decision 0023): sending later and the size limit of the SMTP server.
//!
//! - **Send later, locally.** A scheduled send is the stage 5 `send` operation with the chosen
//!   time as `not_before` and `SendJob::scheduled`; the message is built and frozen at once,
//!   dated with the chosen time. It goes out only if the app runs and is online then (an
//!   offline app retries as for any send once it is back). Moving it rebuilds the frozen
//!   message for the new date; cancelling is `cancelSend` (back to the draft).
//! - **Missed while the app was off.** At start, before the outbox runs, a scheduled send whose
//!   time passed more than [`MISSED_SEND_GRACE_MS`] ago is not sent: it becomes `failed` with
//!   `send_missed` (state `missed`), and `sendNeedsDecision` asks: send now, schedule again,
//!   or keep as draft. Up to an hour late it simply goes out.
//! - **SIZE.** The server's limit (RFC 1870) is read once per session and account over a
//!   connection that sends nothing; [`Compose::size_check`] compares the built message with it,
//!   and sending refuses a message above a known limit (`message_too_large`).

use std::time::{Duration, Instant};

use mail_model::{
    AccountId, CoreError, CoreEvent, DraftError, DraftId, DraftSizeCheck, DraftState,
    LARGE_ATTACHMENT_WARNING_BYTES, MAX_SCHEDULE_AHEAD_MS, MISSED_SEND_GRACE_MS,
    MissedSendDecision, OperationId, OperationPayload, OutgoingMail,
};
use mail_store::{Reschedule, StoredOperation};

use super::{Compose, PoisonError, blocking, build, files, outgoing};
use crate::core::Connector;

/// A SIZE limit read from the server, kept for the session.
#[derive(Debug, Clone, Copy)]
pub(super) struct KnownLimit {
    max: Option<u64>,
    read_at: Instant,
}

/// How long a limit read from the server is trusted.
const LIMIT_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// How long the size check waits for the server.
const LIMIT_WAIT: Duration = Duration::from_secs(15);

/// A send time the user may choose: after now, at most [`MAX_SCHEDULE_AHEAD_MS`] ahead.
pub(super) fn check_time(at: i64, now: i64) -> Result<(), CoreError> {
    if at <= now {
        return Err(CoreError::Draft(DraftError::InvalidSendTime(
            "the time has passed".into(),
        )));
    }
    if at.saturating_sub(now) > MAX_SCHEDULE_AHEAD_MS {
        return Err(CoreError::Draft(DraftError::InvalidSendTime(
            "at most a year ahead".into(),
        )));
    }
    Ok(())
}

/// Rebuilt frozen files get a name of their own (`message-<ms>.eml`): an attempt that already
/// read the old name keeps reading complete bytes.
fn frozen_names(at: i64) -> (String, String) {
    (format!("message-{at}.eml"), format!("sent-{at}.eml"))
}

impl<C: Connector> Compose<C> {
    pub(super) fn now(&self) -> i64 {
        self.inner.clock.now_ms()
    }

    /// Scheduled sends of every account (waiting, and missed ones to decide), the earliest
    /// first.
    pub(crate) async fn scheduled_sends(&self) -> Result<Vec<OutgoingMail>, CoreError> {
        let now = self.now();
        Ok(self
            .inner
            .store
            .open_sends()
            .await?
            .iter()
            .filter(|op| mail_store::is_waiting_schedule(op) || mail_store::is_missed(op))
            .filter_map(|op| outgoing::outgoing_mail(op, now))
            .collect())
    }

    /// At start (before the outbox runs): scheduled sends missed by more than an hour are not
    /// sent but asked about.
    pub(crate) async fn check_missed_sends(&self) {
        let now = self.now();
        match self
            .inner
            .store
            .mark_missed_sends(now, MISSED_SEND_GRACE_MS)
            .await
        {
            Ok(changes) => {
                for change in changes {
                    self.inner.outbox.changed(&change);
                    for op in &change.operations {
                        if let Some(mail) = outgoing::outgoing_mail(op, now) {
                            tracing::info!(account = %change.account, operation = %op.id, "scheduled send missed; asking");
                            self.emit(CoreEvent::SendNeedsDecision {
                                account_id: change.account,
                                mail,
                            });
                        }
                    }
                }
            }
            Err(err) => tracing::error!(error = %err, "missed scheduled sends unreadable"),
        }
    }

    /// Moves a scheduled send (not started) or a missed one to `at` (Unix ms), or with `None`
    /// sends it now (after the undo delay). The frozen message is rebuilt with the new date.
    pub(crate) async fn reschedule(
        &self,
        operation: OperationId,
        at: Option<i64>,
    ) -> Result<OutgoingMail, CoreError> {
        let store = &self.inner.store;
        let op = store
            .operation(operation)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("operation {operation}")))?;
        let OperationPayload::Send(job) = &op.payload else {
            return Err(CoreError::Draft(DraftError::NotScheduled(operation)));
        };
        if !mail_store::is_waiting_schedule(&op) && !mail_store::is_missed(&op) {
            return Err(CoreError::Draft(match op.state {
                mail_model::OperationState::InFlight | mail_model::OperationState::Confirmed => {
                    DraftError::TooLate(operation)
                }
                _ => DraftError::NotScheduled(operation),
            }));
        }
        let now = self.now();
        let not_before = match at {
            Some(at) => {
                check_time(at, now)?;
                Some(at)
            }
            None => {
                let delay = self.send_delay().await?;
                (delay > 0).then(|| now + i64::from(delay) * 1000)
            }
        };
        let draft = self.stored(&job.draft).await?;
        if draft.state != DraftState::Sending {
            return Err(CoreError::Draft(DraftError::NotScheduled(operation)));
        }
        let identities = store.compose_identities(draft.account).await?;
        let sender = build::sender_of(&draft, &identities).ok_or_else(|| {
            CoreError::Draft(DraftError::NoIdentity("choose a sender address".into()))
        })?;
        let sender = super::snippets::with_signature(store, &draft, sender).await?;
        let date = not_before.unwrap_or(now);
        let root = self.inner.root.clone();
        let for_build = draft.clone();
        let built = blocking(move || {
            let files = build::files_of(&root, &for_build)?;
            let message = build::outgoing(&for_build, &sender, files, date / 1000);
            mail_content::build_message(&message).map_err(build::compose_error)
        })
        .await??;
        let (message_name, sent_name) = frozen_names(now);
        let message_file = files::outgoing_path(&job.draft, &message_name);
        let sent_file = files::outgoing_path(&job.draft, &sent_name);
        let lock = self.inner.outbox.files_lock();
        let held = lock.lock().await;
        let root = self.inner.root.clone();
        let (m, s) = (message_file.clone(), sent_file.clone());
        blocking(move || {
            files::write_atomically(&files::resolve(&root, &m)?, &built.for_sending)?;
            files::write_atomically(&files::resolve(&root, &s)?, &built.for_sent_folder)
        })
        .await??;
        let change = store
            .reschedule_send(
                operation,
                Reschedule {
                    not_before,
                    scheduled: at.is_some(),
                    files: Some((message_file.clone(), sent_file.clone())),
                },
            )
            .await;
        let change = match change {
            Ok(change) => change,
            Err(err) => {
                files::remove_file(&self.inner.root, &message_file);
                files::remove_file(&self.inner.root, &sent_file);
                return Err(err.into());
            }
        };
        // The old frozen files stay until the send is finished (the sweep removes the draft's
        // whole outgoing directory then): an attempt that started at this very moment may
        // still read them.
        drop(held);
        self.inner.outbox.changed(&change);
        tracing::info!(account = %change.account, %operation, scheduled = at.is_some(), "send moved");
        let op = self.operation_after(operation, &change.operations).await?;
        outgoing::outgoing_mail(&op, self.now())
            .ok_or_else(|| CoreError::NotFound(format!("send {operation}")))
    }

    /// The user's answer to a missed scheduled send.
    pub(crate) async fn resolve_missed(
        &self,
        operation: OperationId,
        decision: MissedSendDecision,
    ) -> Result<Option<OutgoingMail>, CoreError> {
        let op = self
            .inner
            .store
            .operation(operation)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("operation {operation}")))?;
        if !mail_store::is_missed(&op) {
            return Err(CoreError::Draft(DraftError::NotScheduled(operation)));
        }
        match decision {
            MissedSendDecision::SendNow => self.reschedule(operation, None).await.map(Some),
            MissedSendDecision::Reschedule { send_at } => {
                self.reschedule(operation, Some(send_at)).await.map(Some)
            }
            MissedSendDecision::KeepAsDraft => {
                let OperationPayload::Send(job) = &op.payload else {
                    return Err(CoreError::Draft(DraftError::NotScheduled(operation)));
                };
                self.cancel_send(job.draft.clone()).await?;
                Ok(None)
            }
        }
    }

    async fn operation_after(
        &self,
        id: OperationId,
        changed: &[StoredOperation],
    ) -> Result<StoredOperation, CoreError> {
        match changed.iter().find(|op| op.id == id) {
            Some(op) => Ok(op.clone()),
            None => self
                .inner
                .store
                .operation(id)
                .await?
                .ok_or_else(|| CoreError::NotFound(format!("operation {id}"))),
        }
    }

    // -- size ------------------------------------------------------------------------------

    fn known_limit(&self, account: AccountId) -> Option<KnownLimit> {
        self.inner
            .limits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&account)
            .copied()
            .filter(|known| known.read_at.elapsed() < LIMIT_TTL)
    }

    /// A send above a SIZE limit the server announced (read earlier in this session) is
    /// refused before anything is queued; without a known limit the server decides.
    pub(super) fn refuse_too_large(
        &self,
        account: AccountId,
        size: usize,
    ) -> Result<(), CoreError> {
        let size = u64::try_from(size).unwrap_or(u64::MAX);
        match self.known_limit(account).and_then(|known| known.max) {
            Some(limit) if size > limit => Err(CoreError::Draft(DraftError::TooLargeForServer {
                size,
                limit,
            })),
            _ => Ok(()),
        }
    }

    /// The draft's size as it would go out, against the server's limit (read from the server
    /// once per session; offline: `limitKnown` false).
    pub(crate) async fn size_check(&self, id: DraftId) -> Result<DraftSizeCheck, CoreError> {
        let draft = self.stored(&id).await?;
        let store = &self.inner.store;
        let account = store
            .account(draft.account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", draft.account)))?;
        let identities = store.compose_identities(draft.account).await?;
        let sender = build::sender_of(&draft, &identities)
            .or_else(|| build::default_sender(&identities))
            .ok_or_else(|| {
                CoreError::Draft(DraftError::NoIdentity("the account has none".into()))
            })?;
        let sender = super::snippets::with_signature(store, &draft, sender).await?;
        let attachments: u64 = draft
            .attachments
            .iter()
            .filter(|a| a.content_id.is_none())
            .map(|a| a.size)
            .sum();
        let root = self.inner.root.clone();
        let for_build = draft.clone();
        let now = self.now();
        let size = blocking(move || {
            let files = build::files_of(&root, &for_build)?;
            let message = build::outgoing(&for_build, &sender, files, now / 1000);
            // Lenient like a draft: recipients may still be missing or unfinished.
            mail_content::build_draft_message(&message)
                .map(|bytes| bytes.len())
                .map_err(build::compose_error)
        })
        .await??;
        let size = u64::try_from(size).unwrap_or(u64::MAX);
        let known = match self.known_limit(draft.account) {
            Some(known) => Some(known),
            None => {
                match tokio::time::timeout(LIMIT_WAIT, self.inner.outbox.smtp_limits(&account))
                    .await
                {
                    Ok(Ok(limits)) => {
                        let known = KnownLimit {
                            max: limits.max_message_bytes,
                            read_at: Instant::now(),
                        };
                        self.inner
                            .limits
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .insert(draft.account, known);
                        Some(known)
                    }
                    Ok(Err(err)) => {
                        tracing::info!(account = %draft.account, code = ?err.code(), "SMTP size limit unknown");
                        None
                    }
                    Err(_) => {
                        tracing::info!(account = %draft.account, "SMTP size limit: no answer in time");
                        None
                    }
                }
            }
        };
        let server_limit = known.and_then(|k| k.max);
        Ok(DraftSizeCheck {
            size,
            server_limit,
            limit_known: known.is_some(),
            exceeds_server_limit: server_limit.is_some_and(|limit| size > limit),
            large: attachments > LARGE_ATTACHMENT_WARNING_BYTES,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_times_are_checked() {
        let now = 1_795_000_000_000;
        assert!(check_time(now + 1, now).is_ok());
        assert!(check_time(now + MAX_SCHEDULE_AHEAD_MS, now).is_ok());
        for bad in [now, now - 1, now + MAX_SCHEDULE_AHEAD_MS + 1] {
            let err = check_time(bad, now).expect_err("refused");
            assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument);
        }
    }
}
