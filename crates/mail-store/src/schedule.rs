//! Stage 10, "send later" (decision 0023): scheduled sends are `send` operations whose
//! `not_before` is the time the user chose (`SendJob::scheduled`). Listing them across
//! accounts, moving them, and at start the sends the app missed while it was not running:
//! missed by more than the grace period, they are not sent but marked `failed` with
//! `send_missed`, and the user decides.

use mail_model::{
    CoreError, DraftError, ErrorCode, IpcError, OperationId, OperationPayload, OperationState,
    SendPhase,
};
use rusqlite::{Connection, params};

use crate::outbox::{
    LATER_OPERATION, error_text, operation, operation_from_row, payload_text, require_operation,
    require_writes,
};
use crate::outbox_api::{OutboxChange, StoredOperation};
use crate::settings::now_millis;
use crate::{Store, StoreError, accounts};

/// Sends that wait for a time or a decision, of every account, the earliest first. Reads the
/// partial index `operation_send` (sends only; confirmed ones are purged after a day).
pub(crate) const OPEN_SENDS: &str = "
    SELECT o.id, (SELECT a.uuid FROM account a WHERE a.id = o.account_id), o.batch,
           o.action, o.payload, o.state, o.attempts, o.last_error, o.conflict,
           o.notice, o.mailbox_id, o.message_id, o.target_mailbox_id,
           (SELECT l.modseq FROM message_location l
             WHERE l.mailbox_id = o.mailbox_id AND l.message_id = o.message_id),
           o.created_at, o.updated_at, o.not_before
    FROM operation o INDEXED BY operation_send
    WHERE o.kind = 'send' AND o.state IN ('queued', 'failed')
    ORDER BY coalesce(o.not_before, o.created_at), o.id";

/// Whether `op` is a send the user scheduled that has not started.
pub fn is_waiting_schedule(op: &StoredOperation) -> bool {
    matches!(&op.payload, OperationPayload::Send(job)
        if job.scheduled && job.phase == SendPhase::Submit)
        && op.state == OperationState::Queued
        && op.attempts == 0
}

/// Whether `op` is a scheduled send that was missed while the app was off.
pub fn is_missed(op: &StoredOperation) -> bool {
    op.state == OperationState::Failed
        && op
            .last_error
            .as_ref()
            .is_some_and(|e| e.code == ErrorCode::SendMissed)
}

fn open_sends(conn: &Connection) -> Result<Vec<StoredOperation>, StoreError> {
    Ok(conn
        .prepare_cached(OPEN_SENDS)?
        .query_map([], operation_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

/// See [`Store::mark_missed_sends`].
pub(crate) fn mark_missed(
    conn: &Connection,
    now: i64,
    grace_ms: i64,
) -> Result<Vec<OutboxChange>, StoreError> {
    let mut changes: Vec<OutboxChange> = Vec::new();
    for op in open_sends(conn)? {
        let overdue = op
            .not_before
            .is_some_and(|at| now.saturating_sub(at) > grace_ms);
        if !is_waiting_schedule(&op) || !overdue {
            continue;
        }
        let error = IpcError::from(CoreError::Draft(DraftError::Missed(op.id)));
        conn.prepare_cached(LATER_OPERATION)?
            .execute(params![
                op.id.0,
                OperationState::Failed.as_str(),
                Some(error_text(&error)?),
                op.not_before,
                None::<String>,
                now,
            ])
            .map_err(StoreError::sqlite("mark missed send"))?;
        let updated = require_operation(conn, op.id)?;
        match changes.iter_mut().find(|c| c.account == op.account) {
            Some(change) => change.operations.push(updated),
            None => {
                let mut change = OutboxChange::new(op.account);
                change.operations.push(updated);
                changes.push(change);
            }
        }
    }
    Ok(changes)
}

/// See [`Store::reschedule_send`].
pub(crate) fn reschedule(
    conn: &Connection,
    id: OperationId,
    request: Reschedule,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let op = require_operation(conn, id)?;
    let OperationPayload::Send(mut job) = op.payload.clone() else {
        return Err(StoreError::Draft(DraftError::NotScheduled(id)));
    };
    let waiting = is_waiting_schedule(&op);
    let missed = is_missed(&op);
    if !waiting && !missed {
        return Err(match op.state {
            OperationState::InFlight | OperationState::Confirmed => {
                StoreError::Draft(DraftError::TooLate(id))
            }
            _ => StoreError::Draft(DraftError::NotScheduled(id)),
        });
    }
    let account_row = accounts::row_id(conn, op.account)?;
    require_writes(conn, account_row)?;
    job.scheduled = request.scheduled;
    if let Some((file, sent_file)) = request.files {
        job.file = file;
        job.sent_file = sent_file;
    }
    conn.prepare_cached(LATER_OPERATION)?
        .execute(params![
            id.0,
            OperationState::Queued.as_str(),
            None::<String>,
            request.not_before,
            Some(payload_text(&OperationPayload::Send(job))?),
            now,
        ])
        .map_err(StoreError::sqlite("reschedule send"))?;
    let mut change = OutboxChange::new(op.account);
    change.operations.extend(operation(conn, id)?);
    Ok(change)
}

/// A new time for a scheduled (or missed) send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reschedule {
    /// Unix ms; `None` sends at once.
    pub not_before: Option<i64>,
    /// The user chose the time (`false`: "send now", only the undo delay).
    pub scheduled: bool,
    /// The message rebuilt for the new date: new frozen files (relative paths) replace the
    /// old ones in the job; the old files are swept once nothing names them.
    pub files: Option<(String, String)>,
}

impl Store {
    /// Sends that wait for a time or a decision (queued or failed), of every account, the
    /// earliest first. The engine keeps those the user scheduled and the missed ones.
    pub async fn open_sends(&self) -> Result<Vec<StoredOperation>, StoreError> {
        self.read(open_sends).await
    }

    /// At start, before the outbox runs: scheduled sends whose time passed more than
    /// `grace_ms` before `now` (the app was not running) become `failed` with `send_missed`;
    /// nothing is sent, the draft stays frozen until the user decides. Sends tried already
    /// (the app ran but was offline) are left to the outbox. One change per account.
    pub async fn mark_missed_sends(
        &self,
        now: i64,
        grace_ms: i64,
    ) -> Result<Vec<OutboxChange>, StoreError> {
        self.write(move |tx| mark_missed(tx, now, grace_ms)).await
    }

    /// Moves a scheduled send that has not started, or a missed one, to a new time (back to
    /// `queued`). `TooLate` once the submission started, `NotScheduled` for any other
    /// operation, `ReadOnly` without writes.
    pub async fn reschedule_send(
        &self,
        id: OperationId,
        request: Reschedule,
    ) -> Result<OutboxChange, StoreError> {
        let now = now_millis();
        self.write(move |tx| reschedule(tx, id, request, now)).await
    }

    /// Starts queued or in-flight operations like [`Store::begin_operations`], but only those
    /// due at `now` (their `not_before` passed): a send moved to a later time meanwhile is not
    /// started with the payload the caller read before.
    pub async fn begin_due_operations(
        &self,
        operations: Vec<(OperationId, OperationPayload)>,
        now: i64,
    ) -> Result<Vec<OperationId>, StoreError> {
        let updated_at = now_millis();
        self.write(move |tx| {
            let mut begun = Vec::with_capacity(operations.len());
            for (id, payload) in &operations {
                let updated = tx
                    .prepare_cached(BEGIN_DUE_OPERATION)?
                    .execute(params![id.0, payload_text(payload)?, updated_at, now])
                    .map_err(StoreError::sqlite("begin operation"))?;
                if updated > 0 {
                    begun.push(*id);
                }
            }
            Ok(begun)
        })
        .await
    }
}

const BEGIN_DUE_OPERATION: &str = "
    UPDATE operation SET state = 'in_flight', attempts = attempts + 1, payload = ?2,
                         updated_at = ?3
    WHERE id = ?1 AND state IN ('queued', 'in_flight') AND (not_before IS NULL OR not_before <= ?4)";

#[cfg(test)]
mod tests {
    //! Send later in the store with a set clock (`now` passed in): due, missed below and above
    //! the grace period, tried while running (offline), moved, started.

    use mail_model::{
        Address, ComposeFormat, DraftId, DraftKind, DraftState, ErrorCode, IpcError,
        OperationPayload, OperationState, QuotePosition, SendJob, SendPhase,
    };

    use super::*;
    use crate::test_support::{Fixture, fixture};
    use crate::{DraftContent, NewDraftRow, OperationResult};

    const T: i64 = 1_795_000_000_000;
    const MIN: i64 = 60 * 1000;
    const GRACE: i64 = mail_model::MISSED_SEND_GRACE_MS;

    async fn writable() -> Fixture {
        let f = fixture().await;
        f.store
            .set_writes_enabled(f.account, true)
            .await
            .expect("writes on");
        f
    }

    /// A draft frozen in a `send` operation due at `at` (`scheduled`: chosen by the user).
    async fn queued(f: &Fixture, n: u128, at: i64, scheduled: bool) -> (DraftId, StoredOperation) {
        let identity = f.store.identities(f.account).await.expect("identities")[0].id;
        let id = DraftId::from_random(n);
        f.store
            .create_draft(NewDraftRow {
                id: id.clone(),
                account: f.account,
                identity: Some(identity),
                kind: DraftKind::New,
                message_id_header: format!("later-{n}@example.org"),
                original: None,
                original_message_id_header: None,
                in_reply_to: None,
                references: Vec::new(),
                content: DraftContent {
                    identity: Some(identity),
                    to: vec![Address {
                        name: None,
                        email: "bob@example.org".into(),
                    }],
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: "Later".into(),
                    format: ComposeFormat::Html,
                    body_html: "<p>Hi</p>".into(),
                    body_text: String::new(),
                    include_quote: true,
                    quote_position: QuotePosition::Above,
                    include_signature: true,
                    signature: None,
                },
                quote_html: None,
                quote_text: None,
                attachments: Vec::new(),
                server_copy: None,
                kept_original: None,
                now: T - 60 * MIN,
            })
            .await
            .expect("draft");
        let job = SendJob {
            draft: id.clone(),
            message_id_header: format!("later-{n}@example.org"),
            from: "alice@example.org".into(),
            recipients: vec!["bob@example.org".into()],
            file: format!("outgoing/{id}/message.eml"),
            sent_file: format!("outgoing/{id}/sent.eml"),
            subject: "Later".into(),
            to: Vec::new(),
            recipient_count: 1,
            server_saves_sent: false,
            original: None,
            phase: SendPhase::Submit,
            checks: 0,
            scheduled,
            archive_original: false,
            archive_missing: false,
            receipt_for: None,
            follow_up_at: None,
            redirect: false,
        };
        let change = f
            .store
            .enqueue_send(id.clone(), job, Some(at), format!("req-{n}"))
            .await
            .expect("send");
        let op = change
            .operations
            .into_iter()
            .find(|op| matches!(op.payload, OperationPayload::Send(_)))
            .expect("send operation");
        (id, op)
    }

    async fn reload(f: &Fixture, op: &StoredOperation) -> StoredOperation {
        f.store
            .operation(op.id)
            .await
            .expect("read")
            .expect("operation")
    }

    #[tokio::test]
    async fn the_app_off_below_an_hour_sends_and_above_an_hour_asks() {
        let f = writable().await;
        let (_, early) = queued(&f, 1, T, true).await;
        let (draft, late) = queued(&f, 2, T - 2 * GRACE, true).await;
        let (_, future) = queued(&f, 3, T + 2 * GRACE, true).await;
        let (_, undo) = queued(&f, 4, T - 2 * GRACE, false).await;

        // Start 59 minutes after the first one was due.
        let changes = f
            .store
            .mark_missed_sends(T + GRACE - MIN, GRACE)
            .await
            .expect("check");
        assert_eq!(changes.len(), 1);
        let marked: Vec<_> = changes[0].operations.iter().map(|op| op.id).collect();
        assert_eq!(
            marked,
            vec![late.id],
            "only the one missed by more than an hour"
        );

        let early = reload(&f, &early).await;
        assert_eq!(
            early.state,
            OperationState::Queued,
            "under an hour: it goes out"
        );
        assert!(early.is_due(T + GRACE - MIN));
        assert!(is_waiting_schedule(&early));
        let future = reload(&f, &future).await;
        assert_eq!(future.state, OperationState::Queued);
        assert!(!future.is_due(T + GRACE - MIN));
        // An undo delay that ran out while the app was off is not a schedule: it goes out.
        assert_eq!(reload(&f, &undo).await.state, OperationState::Queued);

        let late = reload(&f, &late).await;
        assert_eq!(late.state, OperationState::Failed);
        assert!(is_missed(&late));
        assert_eq!(
            late.last_error.as_ref().map(|e| e.code),
            Some(ErrorCode::SendMissed)
        );
        assert_eq!(late.attempts, 0, "nothing was tried");
        let frozen = f.store.draft(draft).await.expect("read").expect("draft");
        assert_eq!(
            frozen.state,
            DraftState::Sending,
            "kept until the user decides"
        );

        // Checking again changes nothing (one question per missed send).
        let again = f
            .store
            .mark_missed_sends(T + 3 * GRACE, GRACE)
            .await
            .expect("check");
        let ids: Vec<_> = again
            .iter()
            .flat_map(|c| c.operations.iter().map(|op| op.id))
            .collect();
        assert!(!ids.contains(&late.id), "{ids:?}");

        let open: Vec<_> = f
            .store
            .open_sends()
            .await
            .expect("open")
            .into_iter()
            .map(|op| op.id)
            .collect();
        assert!(open.contains(&late.id) && open.contains(&future.id));
    }

    #[tokio::test]
    async fn a_send_tried_while_the_app_ran_offline_is_never_a_missed_one() {
        let f = writable().await;
        let (_, op) = queued(&f, 1, T, true).await;
        // Due at T, tried, nothing transmitted (offline): back to queued with the error.
        f.store
            .begin_due_operations(vec![(op.id, op.payload.clone())], T)
            .await
            .expect("begin");
        f.store
            .finish_operations(vec![(
                op.id,
                OperationResult::Later {
                    keep_in_flight: false,
                    error: Some(IpcError::from(mail_model::CoreError::Mail(
                        mail_model::BackendError::ConnectionRefused,
                    ))),
                    not_before: None,
                    payload: None,
                },
            )])
            .await
            .expect("later");
        // The app is quit offline and started three hours later: it goes out, no question.
        let changes = f
            .store
            .mark_missed_sends(T + 3 * GRACE, GRACE)
            .await
            .expect("check");
        assert!(changes.is_empty());
        let op = reload(&f, &op).await;
        assert_eq!(op.state, OperationState::Queued);
        assert_eq!(op.attempts, 1);
        assert!(op.is_due(T + 3 * GRACE));
    }

    #[tokio::test]
    async fn scheduled_sends_start_only_when_due_and_can_be_moved_until_then() {
        let f = writable().await;
        let (draft, op) = queued(&f, 1, T + 10 * MIN, true).await;
        // Not due yet: not started, also not with a payload read before.
        let begun = f
            .store
            .begin_due_operations(vec![(op.id, op.payload.clone())], T)
            .await
            .expect("begin");
        assert!(begun.is_empty());

        // Moved to later, with the rebuilt files.
        let new_file = format!("outgoing/{draft}/message-2.eml");
        let new_sent = format!("outgoing/{draft}/sent-2.eml");
        let change = f
            .store
            .reschedule_send(
                op.id,
                Reschedule {
                    not_before: Some(T + 60 * MIN),
                    scheduled: true,
                    files: Some((new_file.clone(), new_sent.clone())),
                },
            )
            .await
            .expect("move");
        let moved = change.operations.first().expect("changed").clone();
        assert_eq!(moved.not_before, Some(T + 60 * MIN));
        let OperationPayload::Send(job) = &moved.payload else {
            panic!("not a send");
        };
        assert_eq!(
            (job.file.as_str(), job.sent_file.as_str()),
            (new_file.as_str(), new_sent.as_str())
        );
        // The old time passes: nothing starts.
        let begun = f
            .store
            .begin_due_operations(vec![(op.id, op.payload.clone())], T + 11 * MIN)
            .await
            .expect("begin");
        assert!(begun.is_empty());
        // On time it starts; then it can no longer be moved.
        let begun = f
            .store
            .begin_due_operations(vec![(moved.id, moved.payload.clone())], T + 60 * MIN)
            .await
            .expect("begin");
        assert_eq!(begun, vec![op.id]);
        let err = f
            .store
            .reschedule_send(
                op.id,
                Reschedule {
                    not_before: Some(T + 120 * MIN),
                    scheduled: true,
                    files: None,
                },
            )
            .await
            .expect_err("started");
        assert!(
            matches!(err, StoreError::Draft(DraftError::TooLate(_))),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_missed_send_can_be_sent_now_or_moved_and_others_cannot_be_moved() {
        let f = writable().await;
        let (_, missed) = queued(&f, 1, T - 2 * GRACE, true).await;
        f.store.mark_missed_sends(T, GRACE).await.expect("check");
        // "Send now": back to queued without a schedule, the error gone.
        let change = f
            .store
            .reschedule_send(
                missed.id,
                Reschedule {
                    not_before: None,
                    scheduled: false,
                    files: None,
                },
            )
            .await
            .expect("send now");
        let now = change.operations.first().expect("changed");
        assert_eq!(now.state, OperationState::Queued);
        assert!(now.last_error.is_none());
        assert!(now.is_due(T));
        assert!(!is_waiting_schedule(now) && !is_missed(now));

        // An undo-delay send is not scheduled: refused.
        let (_, undo) = queued(&f, 2, T + MIN, false).await;
        let err = f
            .store
            .reschedule_send(
                undo.id,
                Reschedule {
                    not_before: Some(T + 60 * MIN),
                    scheduled: true,
                    files: None,
                },
            )
            .await
            .expect_err("not scheduled");
        assert!(
            matches!(err, StoreError::Draft(DraftError::NotScheduled(_))),
            "{err:?}"
        );

        // Writes turned off meanwhile: refused, nothing changes.
        let (_, waiting) = queued(&f, 3, T + MIN, true).await;
        f.store
            .set_writes_enabled(f.account, false)
            .await
            .expect("writes off");
        let err = f
            .store
            .reschedule_send(
                waiting.id,
                Reschedule {
                    not_before: Some(T + 60 * MIN),
                    scheduled: true,
                    files: None,
                },
            )
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn open_sends_read_the_send_index() {
        let f = fixture().await;
        let conn = rusqlite::Connection::open(crate::test_support::db_path(&f._dir)).expect("open");
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {OPEN_SENDS}"))
            .expect("prepare");
        let lines: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .expect("plan")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");
        // Only the sends (partial index); confirmed ones are purged after a day, so the sort
        // is over a handful of rows.
        assert!(
            lines
                .iter()
                .any(|l| l == "SCAN o USING INDEX operation_send"),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|l| l == "SCAN o"), "{lines:?}");
    }
}
