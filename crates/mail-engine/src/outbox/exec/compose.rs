//! Stage 5 operations (decision 0018): `send` (SMTP), `append` (a draft version or the copy of
//! a sent message) and `remove_copy` (an old draft version).
//!
//! **Send.** A queued send is submitted by [`submit`], outside the IMAP step: SMTP needs no IMAP
//! connection. Before the first byte the operation is `in_flight`, so a crash in the middle
//! leaves it in flight, and in-flight sends are never submitted again: they are checked
//! ([`Step::verify_send`]). The SMTP adapter tells "nothing transmitted" (an error: back to
//! `queued`, the worker backs off, a wake retries at once) from "unclear" (the connection ended
//! after the end of the data). Unclear: where the server files sent mail itself (Gmail), the
//! Sent folder is searched for the Message-ID a few times; found means sent. Otherwise, and
//! when it stays unfound, the send fails with `send_unclear` and the user decides; nothing is
//! sent again without that decision.
//!
//! **Append.** Before the command the target's UIDNEXT is recorded (`mark`), and for drafts the
//! version it replaces. After a crash, a copy with the Message-ID at or above the mark is this
//! operation's copy; the APPEND is repeated only if there is none. The replaced version is
//! removed after the new one is on the server, and only if its UID still holds the draft's
//! Message-ID (never someone else's mail).

use std::sync::Arc;
use std::time::Duration;

use mail_model::{
    Access, AppendSource, BackendError, CoreError, DraftState, Envelope, Flag, IpcError,
    MailWriter, MailboxRole, OperationPayload, OperationState, SendOutcome, SendPhase, ServerCopy,
    TargetMailbox, TargetMark, Uid, UidSet,
};
use mail_store::{OperationResult, OutboxChange, StoreError, StoredAccount, StoredOperation};

use super::{Step, StepError, lost, refusal};
use crate::clock::Clock;
use crate::compose::{build, files};
use crate::outbox::Shared;

/// Stage 16: the flags a message copied from another account keeps: the system flags, never
/// `\Deleted`. Keywords stay behind (the adapter appends without keywords).
fn transfer_flags(flags: &mail_model::Flags) -> Vec<Flag> {
    let mut out = Vec::new();
    if flags.seen {
        out.push(Flag::Seen);
    }
    if flags.answered {
        out.push(Flag::Answered);
    }
    if flags.flagged {
        out.push(Flag::Flagged);
    }
    if flags.draft {
        out.push(Flag::Draft);
    }
    out
}

fn unclear(detail: &str) -> IpcError {
    IpcError::from(CoreError::Mail(BackendError::SendUnclear(
        detail.to_owned(),
    )))
}

fn now_ms() -> i64 {
    crate::clock::SystemClock.now_ms()
}

fn later_ms(delay: Duration) -> i64 {
    now_ms().saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX))
}

/// A queued send in its submit phase: [`submit`] runs it, without the IMAP connection.
pub(crate) fn is_submit(op: &StoredOperation) -> bool {
    op.state == OperationState::Queued
        && matches!(&op.payload, OperationPayload::Send(job) if job.phase == SendPhase::Submit)
}

/// What [`submit`] did.
pub(crate) enum Submitted {
    /// Done for now (sent, refused, unclear, or cancelled meanwhile).
    Done(Option<OutboxChange>),
    /// Nothing was transmitted and it may work later (offline, a temporary refusal): the
    /// worker backs off; a wake (the network is back) retries at once.
    RetryLater(Option<OutboxChange>, CoreError),
}

/// Submits a queued send over SMTP and records the outcome (see module docs).
pub(crate) async fn submit<C: Send + Sync + 'static>(
    shared: Arc<Shared<C>>,
    account: StoredAccount,
    op: StoredOperation,
) -> Result<Submitted, StoreError> {
    let OperationPayload::Send(job) = op.payload.clone() else {
        return Ok(Submitted::Done(None));
    };
    let store = &shared.store;
    // Only if it is still due: a scheduled send moved to a later time meanwhile waits.
    let begun = store
        .begin_due_operations(vec![(op.id, op.payload.clone())], shared.clock.now_ms())
        .await?;
    if begun.is_empty() {
        // Cancelled, decided or moved just before.
        return Ok(Submitted::Done(None));
    }
    let attempts = op.attempts.saturating_add(1);
    // The UI shows "sending" now; the outcome follows.
    if let Some(current) = store.operation(op.id).await? {
        let mut change = OutboxChange::new(op.account);
        change.operations.push(current);
        shared.emitter.change(&change);
    }

    let finish = |result: OperationResult| async move { shared_finish(store, op.id, result).await };
    let root = files::compose_dir(store);
    let file = job.file.clone();
    let bytes = match tokio::task::spawn_blocking(move || files::read(&root, &file)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            let error = IpcError::from(CoreError::Internal(format!(
                "the frozen message is missing: {err}"
            )));
            return Ok(Submitted::Done(
                finish(OperationResult::Failed { error }).await?,
            ));
        }
        Err(join) => {
            let error = IpcError::from(CoreError::Internal(format!("reading failed: {join}")));
            return Ok(Submitted::Done(
                finish(OperationResult::Failed { error }).await?,
            ));
        }
    };
    // Stage 14: a password or an access token for the provider's SMTP server.
    let login = match shared
        .credentials
        .login(&account.credential_ref, &account.smtp, false)
        .await
    {
        Ok(login) => login,
        Err(err) => {
            let change = finish(OperationResult::Later {
                keep_in_flight: false,
                error: Some(IpcError::from(&err)),
                not_before: None,
                payload: None,
            })
            .await?;
            return Ok(Submitted::RetryLater(change, err));
        }
    };
    let bearer = login.auth == mail_model::LoginAuth::Bearer;
    let mut config = login.config(&account.smtp, Access::ReadWrite);
    crate::protocols::for_account(&mut config, &account);
    config.timeouts = shared.config.timeouts;
    config.extra_root_ca = shared.config.extra_root_ca.clone();
    let envelope = Envelope {
        from: job.from.clone(),
        recipients: job.recipients.clone(),
    };
    let sender = Arc::clone(&shared.config.sender.0);
    let mut outcome = sender.send(&config, &envelope, &bytes).await;
    // A rejected access token: nothing was sent yet; renew it and try once more.
    if bearer
        && matches!(outcome, Err(mail_model::BackendError::AuthFailed(_)))
        && shared.credentials.login_rejected(&account.credential_ref)
    {
        match shared
            .credentials
            .login(&account.credential_ref, &account.smtp, false)
            .await
        {
            Ok(login) => {
                config.password = login.secret;
                outcome = sender.send(&config, &envelope, &bytes).await;
            }
            Err(err) => {
                tracing::warn!(account = %op.account, code = ?err.code(), "access token not renewed for sending");
            }
        }
    }
    let checks = &shared.config.compose.sent_checks;
    let (result, retry) = match outcome {
        Ok(SendOutcome::Accepted(reply)) => {
            tracing::info!(account = %op.account, operation = %op.id, code = reply.code, "message sent");
            (
                OperationResult::Sent {
                    copy_on_server: false,
                },
                None,
            )
        }
        Ok(SendOutcome::Rejected(rejection)) => {
            let err = rejection.to_error();
            let error = IpcError::from(CoreError::Mail(err.clone()));
            if !rejection.is_permanent() && attempts < shared.config.max_attempts {
                (
                    OperationResult::Later {
                        keep_in_flight: false,
                        error: Some(error),
                        not_before: None,
                        payload: None,
                    },
                    Some(CoreError::Mail(err)),
                )
            } else {
                (OperationResult::Failed { error }, None)
            }
        }
        Ok(SendOutcome::Unclear(detail)) => {
            tracing::warn!(account = %op.account, operation = %op.id, "send outcome unclear; never sent again by itself");
            match checks.first().filter(|_| job.server_saves_sent) {
                Some(first) => {
                    let mut job = job.clone();
                    job.phase = SendPhase::Verify;
                    job.checks = 0;
                    (
                        OperationResult::Later {
                            keep_in_flight: true,
                            error: Some(unclear(&detail)),
                            not_before: Some(later_ms(*first)),
                            payload: Some(Box::new(OperationPayload::Send(job))),
                        },
                        None,
                    )
                }
                None => (
                    OperationResult::Failed {
                        error: unclear(&detail),
                    },
                    None,
                ),
            }
        }
        Err(err) if err.is_transient() => {
            tracing::info!(account = %op.account, operation = %op.id, code = ?err.code(), "nothing sent; trying again later");
            (
                OperationResult::Later {
                    keep_in_flight: false,
                    error: Some(IpcError::from(CoreError::Mail(err.clone()))),
                    not_before: None,
                    payload: None,
                },
                Some(CoreError::Mail(err)),
            )
        }
        Err(err) => (
            OperationResult::Failed {
                error: IpcError::from(CoreError::Mail(err)),
            },
            None,
        ),
    };
    let change = finish(result).await?;
    Ok(match retry {
        Some(err) => Submitted::RetryLater(change, err),
        None => Submitted::Done(change),
    })
}

async fn shared_finish(
    store: &mail_store::Store,
    id: mail_model::OperationId,
    result: OperationResult,
) -> Result<Option<OutboxChange>, StoreError> {
    store.finish_operations(vec![(id, result)]).await
}

impl<W: MailWriter> Step<'_, W> {
    fn appended(&mut self, op: &StoredOperation, copy: Option<ServerCopy>) {
        if let Some(target) = op.payload.target() {
            self.server_mailboxes.insert(target.mailbox_id);
        }
        self.results
            .push((op.id, OperationResult::Appended { copy }));
    }

    /// A copy with `message_id` that arrived in `target` at or above `mark` (this operation's).
    async fn find_new_copy(
        &mut self,
        target: &TargetMailbox,
        mark: TargetMark,
        message_id: &str,
    ) -> Result<Option<Uid>, StepError> {
        // A fresh SELECT: an earlier one of the same mailbox has an old UIDNEXT.
        self.conn.selected = None;
        let opened = match self.conn.open(&target.name).await {
            Ok(opened) => opened,
            Err(err) => {
                refusal(err)?;
                return Ok(None);
            }
        };
        if opened.uid_validity != mark.uid_validity || opened.uid_next <= mark.uid_next {
            return Ok(None);
        }
        let arrived = self
            .conn
            .writer
            .fetch_headers(&opened, &UidSet::range(mark.uid_next, Uid::MAX))
            .await
            .map_err(lost)?;
        Ok(arrived
            .iter()
            .filter(|h| h.message_id.as_deref() == Some(message_id))
            .map(|h| h.uid)
            .max())
    }

    /// Removes `copy` if its UID still holds its Message-ID. Returns whether it did.
    async fn remove_if_ours(&mut self, copy: &ServerCopy) -> Result<bool, StepError> {
        let opened = match self.conn.open(&copy.mailbox.name).await {
            Ok(opened) => opened,
            Err(err) => {
                refusal(err)?;
                return Ok(false);
            }
        };
        if opened.uid_validity != copy.uid_validity {
            return Ok(false);
        }
        let uids = UidSet::from_uids([copy.uid]);
        let headers = self
            .conn
            .writer
            .fetch_headers(&opened, &uids)
            .await
            .map_err(lost)?;
        let ours = headers
            .iter()
            .any(|h| h.uid == copy.uid && h.message_id.as_deref() == Some(&copy.message_id_header));
        if !ours {
            if !headers.is_empty() {
                tracing::warn!(account = %self.account, "a draft's old UID holds another message; left alone");
            }
            return Ok(false);
        }
        match self.conn.writer.expunge_messages(&opened, &uids).await {
            Ok(_) => {
                self.server_mailboxes.insert(copy.mailbox.mailbox_id);
                Ok(true)
            }
            Err(err) => {
                let error = refusal(err)?;
                tracing::warn!(account = %self.account, code = ?error.code, "removing an old draft version was refused");
                Ok(false)
            }
        }
    }

    async fn remove_replaced(
        &mut self,
        replaced: Option<&ServerCopy>,
        new: Option<&ServerCopy>,
    ) -> Result<(), StepError> {
        let Some(old) = replaced else {
            return Ok(());
        };
        if new.is_some_and(|n| n.uid == old.uid && n.mailbox.name == old.mailbox.name) {
            return Ok(());
        }
        self.remove_if_ours(old).await?;
        Ok(())
    }

    /// The bytes an append puts on the server, with the source and the replaced version as
    /// recorded. `None` after recording why there is nothing to append.
    async fn append_bytes(
        &mut self,
        op: &StoredOperation,
        source: &AppendSource,
    ) -> Result<Option<(Vec<u8>, AppendSource, Option<ServerCopy>)>, StepError> {
        let root = files::compose_dir(self.store);
        match source {
            AppendSource::File { file } | AppendSource::Transfer { file, .. } => {
                let file = file.clone();
                let read = tokio::task::spawn_blocking(move || files::read(&root, &file)).await;
                match read {
                    Ok(Ok(bytes)) => Ok(Some((bytes, source.clone(), None))),
                    Ok(Err(err)) => {
                        self.fail(op, IpcError::from(err));
                        Ok(None)
                    }
                    Err(join) => {
                        self.fail(
                            op,
                            IpcError::from(CoreError::Internal(format!("reading failed: {join}"))),
                        );
                        Ok(None)
                    }
                }
            }
            AppendSource::Draft { draft, .. } => {
                let Some(stored) = self.store.draft(draft.clone()).await? else {
                    // Discarded meanwhile: nothing to save.
                    self.appended(op, None);
                    return Ok(None);
                };
                if stored.state != DraftState::Editing {
                    // Frozen for sending: its copy is removed once it is sent.
                    self.appended(op, None);
                    return Ok(None);
                }
                let identities = self.store.compose_identities(stored.account).await?;
                let Some(sender) = build::sender_of(&stored, &identities)
                    .or_else(|| build::default_sender(&identities))
                else {
                    self.fail(
                        op,
                        IpcError::from(CoreError::Draft(mail_model::DraftError::NoIdentity(
                            "the account has no identity".into(),
                        ))),
                    );
                    return Ok(None);
                };
                let sender =
                    crate::compose::snippets::with_signature(self.store, &stored, sender).await?;
                let revision = stored.revision;
                let replace = stored.server_copy.clone();
                let built = tokio::task::spawn_blocking(move || {
                    build::draft_bytes(&root, &stored, &sender)
                })
                .await;
                match built {
                    Ok(Ok(bytes)) => Ok(Some((
                        bytes,
                        AppendSource::Draft {
                            draft: draft.clone(),
                            revision: Some(revision),
                        },
                        replace,
                    ))),
                    Ok(Err(err)) => {
                        self.fail(op, IpcError::from(err));
                        Ok(None)
                    }
                    Err(join) => {
                        self.fail(
                            op,
                            IpcError::from(CoreError::Internal(format!("building failed: {join}"))),
                        );
                        Ok(None)
                    }
                }
            }
        }
    }

    /// See module docs.
    pub(super) async fn append(&mut self, op: StoredOperation) -> Result<(), StepError> {
        let OperationPayload::Append {
            target,
            source,
            message_id_header,
            draft_flag,
            mark,
            replace,
        } = op.payload.clone()
        else {
            return Ok(());
        };
        if op.state == OperationState::InFlight
            && let Some(mark) = mark
            && let Some(uid) = self
                .find_new_copy(&target, mark, &message_id_header)
                .await?
        {
            tracing::info!(account = %self.account, operation = %op.id, "append had arrived; finishing it");
            let copy = ServerCopy {
                mailbox: target.clone(),
                uid_validity: mark.uid_validity,
                uid,
                message_id_header: message_id_header.clone(),
            };
            self.remove_replaced(replace.as_ref(), Some(&copy)).await?;
            self.appended(&op, Some(copy));
            return Ok(());
        }
        let Some(op) = self.within_attempts(vec![op]).pop() else {
            return Ok(());
        };
        // Stage 16: a message from another account keeps its flags.
        let flags: Vec<Flag> = match &source {
            AppendSource::Transfer { flags: kept, .. } => transfer_flags(kept),
            _ if draft_flag => vec![Flag::Seen, Flag::Draft],
            _ => vec![Flag::Seen],
        };
        let Some((bytes, source, replace)) = self.append_bytes(&op, &source).await? else {
            return Ok(());
        };
        let mark = match self.target_mark(&target).await {
            Ok(mark) => mark,
            Err(err) => {
                let error = refusal(err)?;
                self.fail(&op, error);
                return Ok(());
            }
        };
        let mut planned = op;
        planned.payload = OperationPayload::Append {
            target: target.clone(),
            source,
            message_id_header: message_id_header.clone(),
            draft_flag,
            mark: Some(mark),
            replace: replace.clone(),
        };
        let Some(op) = self.begin(vec![planned]).await?.pop() else {
            return Ok(());
        };
        match self.conn.writer.append(&target.name, &flags, &bytes).await {
            Ok(appended) => {
                let uid = match appended {
                    Some(at) if at.uid_validity == mark.uid_validity => Some(at.uid),
                    _ => {
                        self.find_new_copy(&target, mark, &message_id_header)
                            .await?
                    }
                };
                let copy = uid.map(|uid| ServerCopy {
                    mailbox: target.clone(),
                    uid_validity: mark.uid_validity,
                    uid,
                    message_id_header: message_id_header.clone(),
                });
                self.remove_replaced(replace.as_ref(), copy.as_ref())
                    .await?;
                self.appended(&op, copy);
            }
            Err(err) => {
                let error = refusal(err)?;
                self.fail(&op, error);
            }
        }
        Ok(())
    }

    /// See module docs.
    pub(super) async fn remove_copy(&mut self, op: StoredOperation) -> Result<(), StepError> {
        let OperationPayload::RemoveCopy { copy } = op.payload.clone() else {
            return Ok(());
        };
        let Some(op) = self.within_attempts(vec![op]).pop() else {
            return Ok(());
        };
        let Some(op) = self.begin(vec![op]).await?.pop() else {
            return Ok(());
        };
        self.remove_if_ours(&copy).await?;
        self.confirm(&op, None, None, None);
        Ok(())
    }

    /// An in-flight send (unclear, or the app stopped while it was being submitted): look for
    /// it in Sent where the server files sent mail itself, otherwise ask the user.
    pub(super) async fn verify_send(
        &mut self,
        op: StoredOperation,
        checks: &[Duration],
    ) -> Result<(), StepError> {
        let OperationPayload::Send(mut job) = op.payload.clone() else {
            return Ok(());
        };
        if op.state != OperationState::InFlight {
            tracing::warn!(account = %self.account, operation = %op.id, "a queued send reached the IMAP step");
            return Ok(());
        }
        job.phase = SendPhase::Verify;
        let detail = "the connection ended before the server confirmed the message";
        if !job.server_saves_sent {
            self.results.push((
                op.id,
                OperationResult::Failed {
                    error: unclear(detail),
                },
            ));
            return Ok(());
        }
        let Some(sent) = self
            .store
            .mailbox_with_role(self.account, MailboxRole::Sent)
            .await?
        else {
            self.results.push((
                op.id,
                OperationResult::Failed {
                    error: unclear("no Sent folder to look in"),
                },
            ));
            return Ok(());
        };
        self.conn.selected = None;
        let opened = match self.conn.open(&sent.name).await {
            Ok(opened) => opened,
            Err(err) => {
                let error = refusal(err)?;
                tracing::info!(account = %self.account, code = ?error.code, "Sent folder unusable for the check");
                self.results.push((
                    op.id,
                    OperationResult::Failed {
                        error: unclear("the Sent folder could not be opened"),
                    },
                ));
                return Ok(());
            }
        };
        let found = self
            .conn
            .writer
            .find_message_id(&opened, &job.message_id_header)
            .await
            .map_err(lost)?;
        if !found.is_empty() {
            tracing::info!(account = %self.account, operation = %op.id, "the unclear message is in Sent: it was sent");
            self.server_mailboxes.insert(sent.mailbox_id);
            self.results.push((
                op.id,
                OperationResult::Sent {
                    copy_on_server: true,
                },
            ));
            return Ok(());
        }
        let done = usize::try_from(job.checks).unwrap_or(usize::MAX);
        match checks.get(done.saturating_add(1)) {
            Some(next) => {
                job.checks = job.checks.saturating_add(1);
                self.results.push((
                    op.id,
                    OperationResult::Later {
                        keep_in_flight: true,
                        error: Some(unclear(detail)),
                        not_before: Some(later_ms(*next)),
                        payload: Some(Box::new(OperationPayload::Send(job))),
                    },
                ));
            }
            None => {
                tracing::info!(account = %self.account, operation = %op.id, "not in Sent; the user decides");
                self.results.push((
                    op.id,
                    OperationResult::Failed {
                        error: unclear("not found in the Sent folder"),
                    },
                ));
            }
        }
        Ok(())
    }
}
