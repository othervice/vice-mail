//! The outbox view of a `send` operation ([`OutgoingMail`]).

use mail_model::{
    ArchiveAfterSend, ErrorCode, OperationPayload, OperationState, OutgoingMail, SendPhase,
    SendState,
};
use mail_store::StoredOperation;

/// The state the UI shows for a send operation at `now` (Unix ms).
pub(crate) fn send_state(op: &StoredOperation, phase: SendPhase, now: i64) -> SendState {
    match op.state {
        OperationState::Queued if !op.is_due(now) && op.attempts == 0 => SendState::Scheduled,
        OperationState::Queued => SendState::Queued,
        OperationState::InFlight => match phase {
            SendPhase::Submit => SendState::Sending,
            SendPhase::Verify => SendState::Verifying,
        },
        OperationState::Confirmed => SendState::Sent,
        OperationState::Failed
            if op
                .last_error
                .as_ref()
                .is_some_and(|e| e.code == ErrorCode::SendUnclear) =>
        {
            SendState::Unclear
        }
        OperationState::Failed
            if op
                .last_error
                .as_ref()
                .is_some_and(|e| e.code == ErrorCode::SendMissed) =>
        {
            SendState::Missed
        }
        OperationState::Failed | OperationState::Conflict => SendState::Failed,
    }
}

/// `None` for operations that are not sends.
pub(crate) fn outgoing_mail(op: &StoredOperation, now: i64) -> Option<OutgoingMail> {
    let OperationPayload::Send(job) = &op.payload else {
        return None;
    };
    let state = send_state(op, job.phase, now);
    Some(OutgoingMail {
        operation_id: op.id,
        account_id: op.account,
        // A read receipt (stage 16) has no draft.
        draft_id: (state != SendState::Sent && job.receipt_for.is_none())
            .then(|| job.draft.clone()),
        subject: job.subject.clone(),
        to: job.to.clone(),
        recipient_count: job.recipient_count,
        state,
        send_at: op.not_before.unwrap_or(op.created_at),
        scheduled: job.scheduled,
        error: op.last_error.clone(),
        attempts: op.attempts,
        notice: op.notice,
        archive: job.archive_original.then_some(match state {
            SendState::Sent if job.archive_missing => ArchiveAfterSend::NoArchiveMailbox,
            SendState::Sent => ArchiveAfterSend::Queued,
            _ => ArchiveAfterSend::Waiting,
        }),
        read_receipt: job.receipt_for.is_some(),
        created_at: op.created_at,
        updated_at: op.updated_at,
    })
}
