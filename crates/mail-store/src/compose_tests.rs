//! Stage 5 in the store (decision 0018): the migration of the outbox table, drafts, sends and
//! their follow-ups, cancelling and the decision on unclear sends, draft copies, contacts,
//! durability without a clean close, and the query plans of the new statements.

use mail_model::{
    Address, AppendSource, ComposeFormat, ContactNameRecord, DraftError, DraftFile, DraftId,
    DraftKind, DraftState, ErrorCode, Flag, IpcError, MailboxRole, MessageId, OperationNotice,
    OperationPayload, OperationState, OriginalRef, QuotePosition, SendJob, SendPhase, ServerCopy,
    TargetMailbox, UidValidity, UnclearDecision,
};
use rusqlite::Connection;

use crate::migrations::MIGRATIONS;
use crate::store::Schema;
use crate::test_support::{Fixture, UV, count, db_path, fixture, header};
use crate::{
    DraftContent, NewDraftRow, OperationResult, Store, StoreError, StoreOptions, StoredContact,
    StoredOperation, compose, contacts,
};

const D: i64 = 1_764_000_000;

fn content(subject: &str) -> DraftContent {
    DraftContent {
        identity: None,
        to: vec![Address {
            name: Some("Bob".into()),
            email: "bob@example.org".into(),
        }],
        cc: Vec::new(),
        bcc: vec![Address {
            name: None,
            email: "hidden@example.org".into(),
        }],
        subject: subject.to_owned(),
        format: ComposeFormat::Html,
        body_html: "<p>Hallo</p>".into(),
        body_text: String::new(),
        include_quote: true,
        quote_position: QuotePosition::Above,
        include_signature: true,
        signature: None,
    }
}

async fn identity(f: &Fixture) -> mail_model::IdentityId {
    f.store.identities(f.account).await.expect("identities")[0].id
}

async fn new_draft(f: &Fixture, n: u128) -> crate::StoredDraft {
    let identity = identity(f).await;
    let mut c = content("Entwurf");
    c.identity = Some(identity);
    f.store
        .create_draft(NewDraftRow {
            id: DraftId::from_random(n),
            account: f.account,
            identity: Some(identity),
            kind: DraftKind::Reply,
            message_id_header: format!("draft-{n}@example.org"),
            original: None,
            original_message_id_header: Some("1@compose".into()),
            in_reply_to: Some("1@compose".into()),
            references: vec!["0@compose".into(), "1@compose".into()],
            content: c,
            quote_html: Some("<blockquote><p>Alt</p></blockquote>".into()),
            quote_text: Some("> Alt".into()),
            attachments: vec![DraftFile {
                id: "1".into(),
                file_name: "Prüfung.pdf".into(),
                mime: "application/pdf".into(),
                size: 3,
                sha256: "00".into(),
                file: format!("drafts/{}/1", DraftId::from_random(n)),
                content_id: None,
            }],
            server_copy: None,
            kept_original: None,
            now: D * 1000,
        })
        .await
        .expect("create draft")
}

fn job(draft: &DraftId, server_saves_sent: bool) -> SendJob {
    SendJob {
        draft: draft.clone(),
        message_id_header: "draft-1@example.org".into(),
        from: "alice@example.org".into(),
        recipients: vec!["bob@example.org".into(), "hidden@example.org".into()],
        file: format!("outgoing/{draft}/message.eml"),
        sent_file: format!("outgoing/{draft}/sent.eml"),
        subject: "Entwurf".into(),
        to: Vec::new(),
        recipient_count: 2,
        server_saves_sent,
        original: Some(OriginalRef {
            message_id_header: "1@compose".into(),
            forwarded: false,
        }),
        phase: SendPhase::Submit,
        checks: 0,
        scheduled: false,
        archive_original: false,
        archive_missing: false,
        receipt_for: None,
    }
}

async fn writable() -> Fixture {
    let f = fixture().await;
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes on");
    // The answered original lies in the inbox.
    f.store
        .store_headers(f.inbox, UV, vec![header(1, D, Some("1@compose"))], None)
        .await
        .expect("original");
    f.store
        .sync_mailboxes(
            f.account,
            vec![
                crate::test_support::remote("INBOX", &[]),
                crate::test_support::remote("Archive", &["\\Archive"]),
                crate::test_support::remote("Sent", &["\\Sent"]),
                crate::test_support::remote("Drafts", &["\\Drafts"]),
            ],
        )
        .await
        .expect("drafts folder");
    f
}

async fn drafts_box(f: &Fixture) -> TargetMailbox {
    f.store
        .mailbox_with_role(f.account, MailboxRole::Drafts)
        .await
        .expect("read")
        .expect("drafts")
}

async fn send_op(f: &Fixture) -> StoredOperation {
    f.store
        .sends(f.account, None, 10)
        .await
        .expect("sends")
        .into_iter()
        .next()
        .expect("a send")
}

#[tokio::test]
async fn the_rebuilt_outbox_keeps_rows_ids_and_its_counter() {
    let dir = tempfile::TempDir::new().expect("dir");
    let path = db_path(&dir);
    let stage5 = Schema::from_vec(MIGRATIONS[..5].to_vec());
    let store = Store::open_with(&path, StoreOptions::default(), &stage5).expect("stage 5");
    let account = store
        .insert_account(crate::test_support::new_account(1, "a@example.org"))
        .await
        .expect("account")
        .id;
    // Rows as stage 4 code wrote them (the current code writes the new columns).
    store
        .write(|tx| {
            for n in 1..=3 {
                tx.execute(
                    "INSERT INTO operation (account_id, batch, action, kind, payload,
                                            idempotency_key, state, created_at, updated_at)
                     VALUES (1, ?1, 'create_mailbox', 'create_mailbox',
                             json_object('kind', 'create_mailbox', 'path', 'F' || ?1, 'role', NULL),
                             'r' || ?1, 'failed', 0, 0)",
                    [n],
                )?;
            }
            Ok(())
        })
        .await
        .expect("stage 4 rows");
    // The newest is purged: its id must never come back.
    store
        .write(|tx| Ok(tx.execute("DELETE FROM operation WHERE id = 3", [])?))
        .await
        .expect("purge");
    drop(store);

    let store = Store::open(&path, StoreOptions::default()).expect("migrated");
    store
        .set_writes_enabled(account, true)
        .await
        .expect("writes on");
    let ops = store.operations(account, None, 10).await.expect("ops");
    assert_eq!(ops.iter().map(|o| o.id.0).collect::<Vec<_>>(), [2, 1]);
    assert!(ops.iter().all(|o| o.not_before.is_none()));
    let change = store
        .enqueue_create_mailbox(account, "F4".into(), None, "r4".into())
        .await
        .expect("record after");
    assert_eq!(change.operations[0].id.0, 4, "AUTOINCREMENT kept");
    // The CHECKs accept the new kinds and notices, and still refuse unknown ones.
    store
        .write(|tx| {
            assert!(
                tx.execute("UPDATE operation SET kind = 'frobnicate' WHERE id = 4", [])
                    .is_err()
            );
            tx.execute(
                "UPDATE operation SET notice = 'no_sent_copy' WHERE id = 4",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("checks");
}

#[tokio::test]
async fn drafts_are_saved_with_revisions_and_frozen_while_sending() {
    let f = writable().await;
    let draft = new_draft(&f, 1).await;
    assert_eq!(draft.revision, 1);
    assert_eq!(draft.state, DraftState::Editing);
    assert_eq!(draft.references, ["0@compose", "1@compose"]);
    assert_eq!(draft.attachments[0].file_name, "Prüfung.pdf");
    assert_eq!(draft.bcc[0].email, "hidden@example.org");

    let saved = f
        .store
        .save_draft(draft.id.clone(), content("Zweite Fassung"))
        .await
        .expect("save");
    assert_eq!(saved.revision, 2);
    assert_eq!(saved.subject, "Zweite Fassung");
    assert_eq!(
        f.store
            .drafts(f.account, 10)
            .await
            .expect("list")
            .iter()
            .map(|d| d.id.clone())
            .collect::<Vec<_>>(),
        std::slice::from_ref(&draft.id)
    );
    assert_eq!(
        f.store.drafts_to_save().await.expect("to save"),
        std::slice::from_ref(&draft.id)
    );
    let found = f
        .store
        .draft_for_message(f.account, draft.message_id_header.clone())
        .await
        .expect("by message id")
        .expect("found");
    assert_eq!(found.id, draft.id);

    // Send: frozen, a delayed operation, queued appends cancelled.
    let save = f
        .store
        .enqueue_draft_save(draft.id.clone(), "save-1".into())
        .await
        .expect("save to server")
        .expect("queued");
    let append = save.operations[0].id;
    let change = f
        .store
        .enqueue_send(
            draft.id.clone(),
            job(&draft.id, false),
            Some(D * 1000 + 10_000),
            "send-1".into(),
        )
        .await
        .expect("send");
    assert_eq!(change.removed, [append], "the queued append is cancelled");
    let op = &change.operations[0];
    assert_eq!(op.not_before, Some(D * 1000 + 10_000));
    assert!(!op.is_due(D * 1000) && op.is_due(D * 1000 + 10_000));
    let frozen = f
        .store
        .draft(draft.id.clone())
        .await
        .expect("read")
        .expect("draft");
    assert_eq!(frozen.state, DraftState::Sending);
    assert_eq!(frozen.outgoing, Some(op.id));
    let err = f
        .store
        .save_draft(draft.id.clone(), content("x"))
        .await
        .expect_err("frozen");
    assert!(
        matches!(err, StoreError::Draft(DraftError::Sending(_))),
        "{err}"
    );
    assert!(
        f.store
            .enqueue_draft_save(draft.id.clone(), "save-2".into())
            .await
            .expect("no save")
            .is_none()
    );
    // The same request twice records nothing new.
    let again = f
        .store
        .enqueue_send(
            draft.id.clone(),
            job(&draft.id, false),
            None,
            "send-1".into(),
        )
        .await
        .expect("repeated");
    assert!(again.repeated);

    // Cancel within the delay: the operation is gone, the draft editable again.
    let cancelled = f.store.cancel_send(draft.id.clone()).await.expect("cancel");
    assert_eq!(cancelled.removed, [op.id]);
    assert_eq!(cancelled.drafts, std::slice::from_ref(&draft.id));
    let back = f
        .store
        .draft(draft.id.clone())
        .await
        .expect("read")
        .expect("draft");
    assert_eq!(back.state, DraftState::Editing);
    assert!(back.outgoing.is_none());
    assert!(
        f.store
            .pending_operations(f.account)
            .await
            .expect("pending")
            .is_empty()
    );
}

#[tokio::test]
async fn an_in_flight_send_cannot_be_cancelled_and_a_sent_one_has_follow_ups() {
    let f = writable().await;
    let draft = new_draft(&f, 2).await;
    // The draft has a copy in the Drafts folder.
    let drafts = drafts_box(&f).await;
    let save = f
        .store
        .enqueue_draft_save(draft.id.clone(), "save".into())
        .await
        .expect("save")
        .expect("queued");
    let copy = ServerCopy {
        mailbox: drafts.clone(),
        uid_validity: UidValidity(9),
        uid: 41,
        message_id_header: draft.message_id_header.clone(),
    };
    let mut appended = save.operations[0].clone();
    if let OperationPayload::Append { source, .. } = &mut appended.payload {
        *source = AppendSource::Draft {
            draft: draft.id.clone(),
            revision: Some(1),
        };
    }
    f.store
        .begin_operations(vec![(appended.id, appended.payload.clone())])
        .await
        .expect("begin");
    f.store
        .finish_operations(vec![(
            appended.id,
            OperationResult::Appended {
                copy: Some(copy.clone()),
            },
        )])
        .await
        .expect("appended");
    let stored = f
        .store
        .draft(draft.id.clone())
        .await
        .expect("read")
        .expect("draft");
    assert_eq!(stored.server_copy, Some(copy.clone()));
    assert_eq!(stored.server_revision, Some(1));
    assert!(f.store.drafts_to_save().await.expect("to save").is_empty());

    f.store
        .enqueue_send(draft.id.clone(), job(&draft.id, false), None, "send".into())
        .await
        .expect("send");
    let op = send_op(&f).await;
    f.store
        .begin_operations(vec![(op.id, op.payload.clone())])
        .await
        .expect("begin");
    let err = f
        .store
        .cancel_send(draft.id.clone())
        .await
        .expect_err("too late");
    assert!(
        matches!(err, StoreError::Draft(DraftError::TooLate(_))),
        "{err}"
    );

    let change = f
        .store
        .finish_operations(vec![(
            op.id,
            OperationResult::Sent {
                copy_on_server: false,
            },
        )])
        .await
        .expect("sent")
        .expect("change");
    assert_eq!(change.drafts_removed, std::slice::from_ref(&draft.id));
    assert!(
        f.store
            .draft(draft.id.clone())
            .await
            .expect("read")
            .is_none()
    );
    let pending = f
        .store
        .pending_operations(f.account)
        .await
        .expect("pending");
    let kinds: Vec<_> = pending.iter().map(|o| o.payload.kind().as_str()).collect();
    assert_eq!(kinds, ["append", "remove_copy", "set_flag"], "{pending:#?}");
    assert!(pending.iter().all(|o| o.batch == op.batch), "one batch");
    match &pending[0].payload {
        OperationPayload::Append {
            target,
            source,
            draft_flag,
            ..
        } => {
            assert_eq!(target.name, "Sent");
            assert_eq!(
                source,
                &AppendSource::File {
                    file: job(&draft.id, false).sent_file
                }
            );
            assert!(!draft_flag);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(pending[1].payload, OperationPayload::RemoveCopy { copy });
    match &pending[2].payload {
        OperationPayload::SetFlag { flag, value, .. } => {
            assert_eq!(flag, &Flag::Answered);
            assert!(value);
        }
        other => panic!("{other:?}"),
    }
    let sent = f.store.operation(op.id).await.expect("read").expect("op");
    assert_eq!(sent.state, OperationState::Confirmed);
}

#[tokio::test]
async fn a_server_copy_or_a_missing_sent_folder_means_no_append() {
    let f = writable().await;
    let draft = new_draft(&f, 3).await;
    f.store
        .enqueue_send(draft.id.clone(), job(&draft.id, false), None, "send".into())
        .await
        .expect("send");
    let op = send_op(&f).await;
    f.store
        .begin_operations(vec![(op.id, op.payload.clone())])
        .await
        .expect("begin");
    f.store
        .finish_operations(vec![(
            op.id,
            OperationResult::Sent {
                copy_on_server: true,
            },
        )])
        .await
        .expect("sent");
    let kinds: Vec<_> = f
        .store
        .pending_operations(f.account)
        .await
        .expect("pending")
        .iter()
        .map(|o| o.payload.kind().as_str())
        .collect();
    assert_eq!(kinds, ["set_flag"], "the copy is on the server already");

    // Without a Sent folder: sent, with the notice.
    f.store
        .write(|tx| Ok(tx.execute("UPDATE mailbox SET role = NULL WHERE role = 'sent'", [])?))
        .await
        .expect("no sent");
    let draft = new_draft(&f, 4).await;
    f.store
        .enqueue_send(
            draft.id.clone(),
            job(&draft.id, false),
            None,
            "send-2".into(),
        )
        .await
        .expect("send");
    let op = send_op(&f).await;
    f.store
        .begin_operations(vec![(op.id, op.payload.clone())])
        .await
        .expect("begin");
    let change = f
        .store
        .finish_operations(vec![(
            op.id,
            OperationResult::Sent {
                copy_on_server: false,
            },
        )])
        .await
        .expect("sent")
        .expect("change");
    let sent = change
        .operations
        .iter()
        .find(|o| o.id == op.id)
        .expect("send");
    assert_eq!(sent.notice, Some(OperationNotice::NoSentCopy));
}

fn unclear() -> IpcError {
    IpcError::from(mail_model::CoreError::Mail(
        mail_model::BackendError::SendUnclear("eof".into()),
    ))
}

#[tokio::test]
async fn an_unclear_send_waits_for_the_user() {
    let f = writable().await;
    let draft = new_draft(&f, 5).await;
    f.store
        .enqueue_send(draft.id.clone(), job(&draft.id, false), None, "send".into())
        .await
        .expect("send");
    let op = send_op(&f).await;
    f.store
        .begin_operations(vec![(op.id, op.payload.clone())])
        .await
        .expect("begin");
    // Looking in Sent first: stays in flight, delayed.
    let mut verify = job(&draft.id, true);
    verify.phase = SendPhase::Verify;
    f.store
        .finish_operations(vec![(
            op.id,
            OperationResult::Later {
                keep_in_flight: true,
                error: Some(unclear()),
                not_before: Some(D * 1000),
                payload: Some(Box::new(OperationPayload::Send(verify))),
            },
        )])
        .await
        .expect("later");
    let waiting = f.store.operation(op.id).await.expect("read").expect("op");
    assert_eq!(waiting.state, OperationState::InFlight);
    assert_eq!(waiting.not_before, Some(D * 1000));
    // Then failed with send_unclear: the draft stays frozen; retry needs a decision.
    f.store
        .finish_operations(vec![(op.id, OperationResult::Failed { error: unclear() })])
        .await
        .expect("failed");
    let frozen = f
        .store
        .draft(draft.id.clone())
        .await
        .expect("read")
        .expect("draft");
    assert_eq!(
        frozen.state,
        DraftState::Sending,
        "a failed send keeps its draft"
    );
    let err = f
        .store
        .retry_operation(op.id, "retry".into())
        .await
        .expect_err("needs a decision");
    assert!(
        matches!(err, StoreError::Draft(DraftError::NeedsDecision(_))),
        "{err}"
    );
    assert_eq!(
        mail_model::CoreError::from(err).code(),
        ErrorCode::ConfirmationRequired
    );

    // "Send again": the same operation queued, in the submit phase.
    let change = f
        .store
        .resolve_unclear_send(op.id, UnclearDecision::SendAgain)
        .await
        .expect("again");
    let again = &change.operations[0];
    assert_eq!(again.state, OperationState::Queued);
    assert!(again.last_error.is_none());
    match &again.payload {
        OperationPayload::Send(job) => assert_eq!(job.phase, SendPhase::Submit),
        other => panic!("{other:?}"),
    }
    // Unclear again, then "it arrived".
    f.store
        .begin_operations(vec![(op.id, again.payload.clone())])
        .await
        .expect("begin");
    f.store
        .finish_operations(vec![(op.id, OperationResult::Failed { error: unclear() })])
        .await
        .expect("failed");
    let change = f
        .store
        .resolve_unclear_send(op.id, UnclearDecision::TreatAsSent)
        .await
        .expect("as sent");
    let done = change
        .operations
        .iter()
        .find(|o| o.id == op.id)
        .expect("op");
    assert_eq!(done.state, OperationState::Confirmed);
    assert_eq!(change.drafts_removed, std::slice::from_ref(&draft.id));
    assert!(
        !change
            .operations
            .iter()
            .any(|o| matches!(o.payload, OperationPayload::Append { .. })),
        "this server files sent mail itself: no copy from the app"
    );
    assert!(
        change
            .operations
            .iter()
            .any(|o| matches!(o.payload, OperationPayload::SetFlag { .. })),
        "the original is answered"
    );
    // Only for unclear sends.
    let err = f
        .store
        .resolve_unclear_send(op.id, UnclearDecision::SendAgain)
        .await
        .expect_err("not unclear");
    assert!(matches!(err, StoreError::Action(_)), "{err}");
}

#[tokio::test]
async fn discarded_drafts_remove_their_copy_and_orphans_are_cleaned() {
    let f = writable().await;
    let draft = new_draft(&f, 6).await;
    let drafts = drafts_box(&f).await;
    let copy = ServerCopy {
        mailbox: drafts,
        uid_validity: UidValidity(9),
        uid: 7,
        message_id_header: draft.message_id_header.clone(),
    };
    let save = f
        .store
        .enqueue_draft_save(draft.id.clone(), "save".into())
        .await
        .expect("save")
        .expect("queued");
    // Only one append per draft waits: it builds the newest version when it runs.
    f.store
        .save_draft(draft.id.clone(), content("neuer"))
        .await
        .expect("save");
    assert!(
        f.store
            .enqueue_draft_save(draft.id.clone(), "save-again".into())
            .await
            .expect("coalesced")
            .is_none()
    );
    let append = save.operations[0].clone();
    f.store
        .begin_operations(vec![(append.id, append.payload.clone())])
        .await
        .expect("begin");
    // Deleted while the append is on its way.
    let (_, change) = f
        .store
        .delete_draft(draft.id.clone(), "delete".into())
        .await
        .expect("delete");
    assert_eq!(change.drafts_removed, std::slice::from_ref(&draft.id));
    assert!(change.operations.is_empty(), "no copy known yet");
    let change = f
        .store
        .finish_operations(vec![(
            append.id,
            OperationResult::Appended {
                copy: Some(copy.clone()),
            },
        )])
        .await
        .expect("appended")
        .expect("change");
    assert!(
        change
            .operations
            .iter()
            .any(|o| o.payload == OperationPayload::RemoveCopy { copy: copy.clone() }),
        "the orphan version is removed: {change:#?}"
    );

    // A draft with a known copy: deleting queues the removal; read-only keeps it.
    let other = new_draft(&f, 7).await;
    f.store
        .write({
            let row = other.row;
            move |tx| {
                Ok(tx.execute(
                    "UPDATE draft SET server_mailbox_id = (SELECT id FROM mailbox WHERE role = 'drafts'),
                       server_mailbox = 'Drafts', server_uidvalidity = 9, server_uid = 8 WHERE id = ?1",
                    [row],
                )?)
            }
        })
        .await
        .expect("copy");
    f.store
        .set_writes_enabled(f.account, false)
        .await
        .expect("writes off");
    let (_, change) = f
        .store
        .delete_draft(other.id.clone(), "delete-2".into())
        .await
        .expect("delete");
    assert!(
        change.operations.is_empty(),
        "nothing written for a read-only account"
    );
}

#[tokio::test]
async fn a_draft_survives_a_store_that_was_never_closed() {
    let dir = tempfile::TempDir::new().expect("dir");
    let path = db_path(&dir);
    let store = Store::open(&path, StoreOptions::default()).expect("open");
    let account = store
        .insert_account(crate::test_support::new_account(1, "a@example.org"))
        .await
        .expect("account")
        .id;
    let id = DraftId::from_random(99);
    store
        .create_draft(NewDraftRow {
            id: id.clone(),
            account,
            identity: None,
            kind: DraftKind::New,
            message_id_header: "x@example.org".into(),
            original: None,
            original_message_id_header: None,
            in_reply_to: None,
            references: Vec::new(),
            content: content("Ungespeichert"),
            quote_html: None,
            quote_text: None,
            attachments: Vec::new(),
            server_copy: None,
            kept_original: None,
            now: D * 1000,
        })
        .await
        .expect("create");
    store
        .save_draft(id.clone(), content("Gerettet"))
        .await
        .expect("save");
    // No close, no checkpoint: as after a crash, the threads and connections just stay.
    std::mem::forget(store);
    let reopened = Store::open(&path, StoreOptions::default()).expect("reopen");
    let draft = reopened.draft(id).await.expect("read").expect("draft");
    assert_eq!(draft.subject, "Gerettet");
    assert_eq!(draft.revision, 2);
}

fn contact(key: &str, rank: f64, words: &str) -> StoredContact {
    let (local, domain) = key.split_once('@').expect("address");
    StoredContact {
        key: key.into(),
        local_part: local.into(),
        domain: domain.into(),
        group: None,
        sent: 1,
        received: 0,
        last_sent: Some(D),
        last_received: None,
        rank,
        names: vec![ContactNameRecord {
            name: "Hans Müller".into(),
            count: 1,
            last_seen: D,
        }],
        user_name: None,
        search: format!(" {words}"),
    }
}

#[tokio::test]
async fn contacts_are_found_by_word_prefixes_in_rank_order() {
    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, Some("c1@x")), header(2, D + 1, Some("c2@x"))],
            None,
        )
        .await
        .expect("headers");
    let todo = f.store.contacts_todo(10).await.expect("todo");
    assert_eq!(todo.len(), 2);
    assert_eq!(
        todo[0].from.as_ref().map(|a| a.email.as_str()),
        Some("sender@example.org")
    );
    f.store
        .record_contacts(
            vec![
                contact("hans@x.example", 1.0, "hans muller mueller hans@x.example"),
                contact("hanna@y.example", 5.0, "hanna hanna@y.example"),
                contact("otto@z.example", f64::NEG_INFINITY, "otto otto@z.example"),
            ],
            todo.iter().map(|t| t.message).collect(),
        )
        .await
        .expect("record");
    assert!(
        f.store.contacts_todo(10).await.expect("todo").is_empty(),
        "marked"
    );
    let found = f
        .store
        .search_contacts(vec!["han".into()], 10)
        .await
        .expect("search");
    let keys: Vec<_> = found.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["hanna@y.example", "hans@x.example"], "by rank");
    let found = f
        .store
        .search_contacts(vec!["han".into(), "mue".into()], 10)
        .await
        .expect("search");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].names[0].name, "Hans Müller");
    assert!(
        f.store
            .search_contacts(vec!["ller".into()], 10)
            .await
            .expect("search")
            .is_empty(),
        "no infix"
    );
    let otto = f
        .store
        .contacts_by_keys(vec!["otto@z.example".into()])
        .await
        .expect("by key");
    assert_eq!(otto[0].rank, f64::NEG_INFINITY);
    let _ = MessageId(0);
}

#[tokio::test]
async fn signatures_and_the_sent_copy_mode_are_stored() {
    let f = fixture().await;
    let id = identity(&f).await;
    assert_eq!(
        f.store
            .set_identity_signature(id, Some("Alice\r\nTel. 1".into()))
            .await
            .expect("set"),
        f.account
    );
    let identities = f.store.compose_identities(f.account).await.expect("read");
    assert_eq!(identities[0].1.as_deref(), Some("Alice\nTel. 1"));
    f.store
        .set_identity_signature(id, Some("   ".into()))
        .await
        .expect("clear");
    assert_eq!(
        f.store.compose_identities(f.account).await.expect("read")[0].1,
        None
    );
    assert!(
        f.store
            .set_identity_signature(id, Some("x".repeat(20_000)))
            .await
            .is_err()
    );
    assert_eq!(
        f.store.sent_copy_mode(f.account).await.expect("mode"),
        mail_model::SentCopyMode::Auto
    );
    f.store
        .set_sent_copy_mode(f.account, mail_model::SentCopyMode::Server)
        .await
        .expect("set mode");
    assert_eq!(
        f.store.sent_copy_mode(f.account).await.expect("mode"),
        mail_model::SentCopyMode::Server
    );
}

// ---------------------------------------------------------------------------------------------
// Query plans

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap_or_else(|err| panic!("prepare {sql}: {err}"));
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

/// Name, SQL, required plan lines, and the one table a scan is allowed on (tables that hold a
/// device's open drafts or its unfinished outbox, never mail).
fn expected() -> Vec<(
    &'static str,
    String,
    Vec<&'static str>,
    Option<&'static str>,
)> {
    vec![
        (
            "draft by id",
            compose::DRAFT_BY_UUID.to_owned(),
            vec![
                "SEARCH d USING INDEX sqlite_autoindex_draft_1 (uuid=?)",
                "SEARCH o USING INDEX operation_draft (draft_id=?)",
            ],
            None,
        ),
        (
            "draft by message id",
            compose::DRAFT_BY_MESSAGE_ID.to_owned(),
            vec!["SEARCH d USING INDEX draft_msgid (account_id=? AND message_id_hdr=?)"],
            None,
        ),
        (
            "draft by its kept original",
            compose::DRAFT_BY_KEPT_ORIGINAL.to_owned(),
            vec!["SEARCH d USING INDEX draft_kept_original (account_id=? AND kept_original=?)"],
            None,
        ),
        (
            "drafts of an account",
            compose::DRAFTS_OF_ACCOUNT.to_owned(),
            vec!["SEARCH d USING INDEX draft_account (account_id=?)"],
            None,
        ),
        (
            "drafts to save",
            compose::DRAFTS_TO_SAVE.to_owned(),
            vec!["SCAN draft"],
            Some("draft"),
        ),
        (
            "update a draft",
            compose::UPDATE_DRAFT.to_owned(),
            vec!["SEARCH draft USING INTEGER PRIMARY KEY (rowid=?)"],
            None,
        ),
        (
            "release a draft",
            compose::RELEASE_DRAFT.to_owned(),
            vec!["SEARCH draft USING INDEX sqlite_autoindex_draft_1 (uuid=?)"],
            None,
        ),
        (
            "queued appends of a draft",
            compose::QUEUED_APPENDS_OF_DRAFT.to_owned(),
            vec!["SEARCH operation USING INDEX operation_draft (draft_id=?)"],
            None,
        ),
        (
            "mailbox with a role",
            compose::MAILBOX_WITH_ROLE.to_owned(),
            vec!["SEARCH mailbox USING INDEX mailbox_role (account_id=? AND role=?)"],
            None,
        ),
        (
            "messages with a Message-ID",
            compose::MESSAGES_WITH_MSGID.to_owned(),
            vec![
                "SEARCH message USING COVERING INDEX message_msgid (account_id=? AND message_id_hdr=?)",
            ],
            None,
        ),
        (
            "locations of a message",
            compose::LOCATIONS_OF_MESSAGE.to_owned(),
            vec!["SEARCH message_location USING COVERING INDEX loc_message (message_id=?)"],
            None,
        ),
        // Stage 16 (decision 0033).
        (
            "draft send options",
            compose::SET_DRAFT_SEND_OPTIONS.to_owned(),
            vec!["SEARCH draft USING INTEGER PRIMARY KEY (rowid=?)"],
            None,
        ),
        (
            "read receipt answer",
            compose::RECEIPT_ANSWER.to_owned(),
            vec![
                "SEARCH a USING PRIMARY KEY (account_uuid=? AND message_id_hdr=?)",
                "SEARCH o USING INTEGER PRIMARY KEY (rowid=?)",
            ],
            None,
        ),
        (
            "read receipts received",
            compose::RECEIPTS_RECEIVED.to_owned(),
            vec![
                "SEARCH receipt_received USING PRIMARY KEY (account_uuid=? AND original_msgid_hdr=?)",
            ],
            None,
        ),
        (
            "sends of an account",
            crate::outbox::SENDS_OF_ACCOUNT.to_owned(),
            vec!["SEARCH o USING INDEX operation_send (account_id=? AND id<?)"],
            None,
        ),
        (
            "postpone an operation",
            crate::outbox::LATER_OPERATION.to_owned(),
            vec!["SEARCH operation USING INTEGER PRIMARY KEY (rowid=?)"],
            None,
        ),
        (
            "contact by key",
            contacts::CONTACT_BY_KEY.to_owned(),
            vec!["SEARCH contact USING INDEX sqlite_autoindex_contact_1 (address_key=?)"],
            None,
        ),
        (
            "contact search",
            contacts::search_sql(2),
            vec!["SCAN contact USING INDEX contact_rank"],
            Some("contact USING INDEX contact_rank"),
        ),
        (
            "messages for the contacts",
            contacts::CONTACTS_TODO.to_owned(),
            vec!["SCAN m USING INDEX message_contacts_todo"],
            Some("m USING INDEX message_contacts_todo"),
        ),
        (
            "mark contacts seen",
            contacts::MARK_CONTACTS_SEEN.to_owned(),
            vec!["SEARCH message USING INTEGER PRIMARY KEY (rowid=?)"],
            None,
        ),
        (
            "identities with signatures",
            contacts::COMPOSE_IDENTITIES.to_owned(),
            vec!["SEARCH identity USING INDEX identity_account (account_id=?)"],
            None,
        ),
        (
            "unfinished compose operations",
            contacts::UNFINISHED_COMPOSE.to_owned(),
            vec!["SCAN operation"],
            Some("operation"),
        ),
    ]
}

fn check_plans(conn: &Connection, stage: &str) {
    let mut problems = Vec::new();
    for (name, sql, required, allowed_scan) in expected() {
        let lines = plan(conn, &sql);
        let bad = lines.iter().any(|l| {
            (l.starts_with("SCAN") && !allowed_scan.is_some_and(|ok| l == &format!("SCAN {ok}")))
                || (l.contains("TEMP B-TREE") && !name.starts_with("identities"))
        });
        let missing: Vec<_> = required
            .iter()
            .filter(|want| !lines.iter().any(|l| l == *want))
            .collect();
        if bad || !missing.is_empty() {
            problems.push(format!("{name}: plan {lines:#?}, missing {missing:#?}"));
        }
    }
    assert!(problems.is_empty(), "{stage}:\n{}", problems.join("\n"));
}

#[tokio::test]
async fn every_stage_5_query_uses_its_index() {
    let f = writable().await;
    let check = |store: &Store, stage: &'static str| {
        let store = store.clone();
        async move {
            store
                .read(move |conn| {
                    check_plans(conn, stage);
                    Ok(())
                })
                .await
                .expect("reader");
            store
                .write(move |tx| {
                    check_plans(tx, stage);
                    Ok(())
                })
                .await
                .expect("writer");
        }
    };
    check(&f.store, "fresh database").await;
    for n in 10..40 {
        let draft = new_draft(&f, n).await;
        if n % 2 == 0 {
            f.store
                .enqueue_send(
                    draft.id.clone(),
                    job(&draft.id, false),
                    None,
                    format!("s{n}"),
                )
                .await
                .expect("send");
        }
    }
    let headers: Vec<_> = (2..=2_000u32)
        .map(|uid| header(uid, D + i64::from(uid), Some(&format!("{uid}@p"))))
        .collect();
    for window in headers.chunks(500) {
        f.store
            .store_headers(f.inbox, UV, window.to_vec(), None)
            .await
            .expect("headers");
    }
    f.store
        .write(|tx| Ok(tx.execute_batch("ANALYZE")?))
        .await
        .expect("analyze");
    check(&f.store, "after ANALYZE").await;
    assert!(count(&f.store, "SELECT count(*) FROM draft").await >= 30);
}
