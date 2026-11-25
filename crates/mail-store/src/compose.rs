//! Drafts, sends in the outbox and their follow-ups (stage 5, decision 0018). Synchronous
//! functions over a connection; `compose_api` runs them in one writer transaction each.
//!
//! - **Drafts** are local rows (`draft`), saved by the composer. Every save raises `revision`;
//!   a copy for the server's Drafts folder is an `append` operation that builds the newest
//!   version when it runs (one queued append per draft is enough).
//! - **Send** freezes the draft (`state = 'sending'`), cancels its queued appends and records a
//!   `send` operation with `not_before` (the undo delay). Cancelling before it ran deletes the
//!   operation and releases the draft. A failed send keeps the draft frozen, so "retry" sends
//!   exactly what failed; "edit" (discard or cancel) releases it.
//! - **Follow-ups** of an accepted send are recorded in the transaction that confirms it
//!   ([`send_followups`]): the copy into Sent, removing the draft's copy in Drafts, `\Answered`
//!   (reply) or `$Forwarded` (forward) on the original's locations; the draft row goes.

use mail_model::{
    AccountId, ActionError, ActionKind, Address, AppendSource, ComposeFormat, DraftError,
    DraftFile, DraftId, DraftKind, DraftState, ErrorCode, Flag, IdentityId, MailAction, MailboxId,
    MailboxRole, MessageId, MessageRef, OperationId, OperationNotice, OperationPayload,
    OperationState, QuotePosition, SendJob, SendPhase, ServerCopy, TargetMailbox, UidValidity,
    UnclearDecision,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::StoreError;
use crate::accounts::{self, account_id_at, conversion_error};
use crate::compose_api::{DraftContent, NewDraftRow, StoredDraft};
use crate::outbox::{
    self, DELETE_OPERATION, FINISH_OPERATION, LATER_OPERATION, Recorder, account_writes,
    json_error, operation, payload_text, repeated, require_operation, require_writes,
};
use crate::outbox_api::{OutboxChange, StoredOperation};

// ---------------------------------------------------------------------------------------------
// SQL (every statement has a plan test)

macro_rules! select_drafts {
    ($rest:literal) => {
        concat!(
            "SELECT d.id, d.uuid, (SELECT a.uuid FROM account a WHERE a.id = d.account_id),
                    d.account_id, d.identity_id, d.kind, d.message_id_hdr, d.original_message_id,
                    d.original_msgid_hdr, d.in_reply_to, d.references_hdr, d.to_addrs,
                    d.cc_addrs, d.bcc_addrs, d.subject, d.format, d.body_html, d.body_text,
                    d.quote_html, d.quote_text, d.include_quote, d.quote_position,
                    d.include_signature, d.attachments, d.state, d.revision, d.server_revision,
                    d.server_mailbox_id, d.server_mailbox, d.server_uidvalidity, d.server_uid,
                    d.created_at, d.updated_at,
                    (SELECT o.id FROM operation o
                      WHERE o.draft_id = d.id AND o.kind = 'send' AND o.state != 'confirmed'
                      ORDER BY o.id DESC LIMIT 1),
                    EXISTS (SELECT 1 FROM operation o
                             WHERE o.draft_id = d.id AND o.kind = 'append'
                               AND o.state IN ('queued', 'in_flight')),
                    d.signature_uuid, d.archive_original, d.request_receipt, d.kept_original,
                    d.follow_up_at
             FROM draft d ",
            $rest
        )
    };
}

pub(crate) const DRAFT_BY_UUID: &str = select_drafts!("WHERE d.uuid = ?1");

pub(crate) const DRAFT_BY_MESSAGE_ID: &str =
    select_drafts!("WHERE d.account_id = ?1 AND d.message_id_hdr = ?2");

/// The draft made from a foreign original that stays in the Drafts folder (`kept_original`).
pub(crate) const DRAFT_BY_KEPT_ORIGINAL: &str =
    select_drafts!("WHERE d.account_id = ?1 AND d.kept_original = ?2 ORDER BY d.id LIMIT 1");

/// The account's drafts, newest change first (`draft_account`, read backwards).
pub(crate) const DRAFTS_OF_ACCOUNT: &str =
    select_drafts!("WHERE d.account_id = ?1 ORDER BY d.updated_at DESC LIMIT ?2");

/// Drafts whose newest revision is not on the server yet. A scan: the table holds only the
/// drafts that are open on this device (a few), never mail.
pub(crate) const DRAFTS_TO_SAVE: &str = "
    SELECT uuid FROM draft
    WHERE state = 'editing' AND (server_revision IS NULL OR server_revision < revision)";

pub(crate) const INSERT_DRAFT: &str = "
    INSERT INTO draft (uuid, account_id, identity_id, kind, message_id_hdr, original_message_id,
                       original_msgid_hdr, in_reply_to, references_hdr, to_addrs, cc_addrs,
                       bcc_addrs, subject, format, body_html, body_text, quote_html, quote_text,
                       include_quote, quote_position, include_signature, attachments, state,
                       revision, server_revision, server_mailbox_id, server_mailbox,
                       server_uidvalidity, server_uid, created_at, updated_at, signature_uuid,
                       kept_original)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
            ?20, ?21, ?22, 'editing', 1, ?23, ?24, ?25, ?26, ?27, ?28, ?28, ?29, ?30)";

pub(crate) const UPDATE_DRAFT: &str = "
    UPDATE draft
    SET identity_id = ?2, to_addrs = ?3, cc_addrs = ?4, bcc_addrs = ?5, subject = ?6,
        format = ?7, body_html = ?8, body_text = ?9, include_quote = ?10, quote_position = ?11,
        include_signature = ?12, revision = revision + 1, updated_at = ?13,
        signature_uuid = ?14
    WHERE id = ?1 AND state = 'editing'";

pub(crate) const SET_DRAFT_ATTACHMENTS: &str = "
    UPDATE draft SET attachments = ?2, revision = revision + 1, updated_at = ?3
    WHERE id = ?1 AND state = 'editing'";

pub(crate) const SET_DRAFT_STATE: &str =
    "UPDATE draft SET state = ?2, updated_at = ?3 WHERE id = ?1";

pub(crate) const RELEASE_DRAFT: &str =
    "UPDATE draft SET state = 'editing', updated_at = ?2 WHERE uuid = ?1 AND state = 'sending'";

pub(crate) const SET_DRAFT_SERVER_COPY: &str = "
    UPDATE draft
    SET server_revision = ?2, server_mailbox_id = ?3, server_mailbox = ?4,
        server_uidvalidity = ?5, server_uid = ?6
    WHERE id = ?1";

pub(crate) const DELETE_DRAFT: &str = "DELETE FROM draft WHERE id = ?1";

/// Stage 16: the send options of a draft; a new revision (the Drafts copy carries the
/// read-receipt request).
pub(crate) const SET_DRAFT_SEND_OPTIONS: &str = "
    UPDATE draft
    SET archive_original = ?2, request_receipt = ?3, follow_up_at = ?5, revision = revision + 1,
        updated_at = ?4
    WHERE id = ?1 AND state = 'editing'";

/// Queued (not yet sent) appends of a draft (`operation_draft`).
pub(crate) const QUEUED_APPENDS_OF_DRAFT: &str =
    "SELECT id FROM operation WHERE draft_id = ?1 AND kind = 'append' AND state = 'queued'";

/// The selectable mailbox with a role (`mailbox_role`).
pub(crate) const MAILBOX_WITH_ROLE: &str =
    "SELECT id, name FROM mailbox WHERE account_id = ?1 AND role = ?2 AND selectable = 1";

/// Local messages with this Message-ID (`message_msgid`).
pub(crate) const MESSAGES_WITH_MSGID: &str =
    "SELECT id FROM message WHERE account_id = ?1 AND message_id_hdr = ?2";

/// Mailboxes a message lies in (`loc_message`).
pub(crate) const LOCATIONS_OF_MESSAGE: &str =
    "SELECT mailbox_id FROM message_location WHERE message_id = ?1";

// ---------------------------------------------------------------------------------------------
// Rows

fn addresses_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<Vec<Address>> {
    let text: String = r.get(idx)?;
    serde_json::from_str(&text).map_err(|err| conversion_error(idx, Type::Text, err.to_string()))
}

fn files_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<Vec<DraftFile>> {
    let text: String = r.get(idx)?;
    serde_json::from_str(&text).map_err(|err| conversion_error(idx, Type::Text, err.to_string()))
}

fn files_text(files: &[DraftFile]) -> Result<String, StoreError> {
    serde_json::to_string(files).map_err(json_error("encode draft attachments"))
}

fn parsed<T>(r: &Row<'_>, idx: usize, parse: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    let text: String = r.get(idx)?;
    parse(&text).ok_or_else(|| conversion_error(idx, Type::Text, format!("value {text:?}")))
}

fn draft_from_row(r: &Row<'_>) -> rusqlite::Result<StoredDraft> {
    let uuid: String = r.get(1)?;
    let references: Option<String> = r.get(10)?;
    let server_mailbox_id: Option<i64> = r.get(27)?;
    let server_mailbox: Option<String> = r.get(28)?;
    let server_uidvalidity: Option<u32> = r.get(29)?;
    let server_uid: Option<u32> = r.get(30)?;
    let message_id_header: String = r.get(6)?;
    let server_copy = match (
        server_mailbox_id,
        server_mailbox,
        server_uidvalidity,
        server_uid,
    ) {
        (Some(mailbox_id), Some(name), Some(uid_validity), Some(uid)) => Some(ServerCopy {
            mailbox: TargetMailbox {
                mailbox_id: MailboxId(mailbox_id),
                name,
            },
            uid_validity: UidValidity(uid_validity),
            uid,
            message_id_header: message_id_header.clone(),
        }),
        _ => None,
    };
    Ok(StoredDraft {
        row: r.get(0)?,
        id: DraftId::parse(&uuid)
            .ok_or_else(|| conversion_error(1, Type::Text, format!("draft id {uuid:?}")))?,
        account: account_id_at(r, 2)?,
        account_row: r.get(3)?,
        identity: r.get::<_, Option<i64>>(4)?.map(IdentityId),
        kind: parsed(r, 5, DraftKind::parse)?,
        message_id_header,
        original: r.get::<_, Option<i64>>(7)?.map(MessageId),
        original_message_id_header: r.get(8)?,
        in_reply_to: r.get(9)?,
        references: references
            .map(|refs| refs.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
        to: addresses_at(r, 11)?,
        cc: addresses_at(r, 12)?,
        bcc: addresses_at(r, 13)?,
        subject: r.get(14)?,
        format: parsed(r, 15, ComposeFormat::parse)?,
        body_html: r.get(16)?,
        body_text: r.get(17)?,
        quote_html: r.get(18)?,
        quote_text: r.get(19)?,
        include_quote: r.get(20)?,
        quote_position: parsed(r, 21, QuotePosition::parse)?,
        include_signature: r.get(22)?,
        attachments: files_at(r, 23)?,
        state: parsed(r, 24, DraftState::parse)?,
        revision: r.get(25)?,
        server_revision: r.get(26)?,
        server_copy,
        created_at: r.get(31)?,
        updated_at: r.get(32)?,
        outgoing: r.get::<_, Option<i64>>(33)?.map(OperationId),
        saving: r.get(34)?,
        signature: r
            .get::<_, Option<String>>(35)?
            .as_deref()
            .and_then(mail_model::SignatureId::parse),
        archive_original: r.get(36)?,
        request_receipt: r.get(37)?,
        kept_original: r.get(38)?,
        follow_up_at: r.get(39)?,
    })
}

pub(crate) fn draft(conn: &Connection, id: &DraftId) -> Result<Option<StoredDraft>, StoreError> {
    Ok(conn
        .prepare_cached(DRAFT_BY_UUID)?
        .query_row([id.as_str()], draft_from_row)
        .optional()?)
}

pub(crate) fn require_draft(conn: &Connection, id: &DraftId) -> Result<StoredDraft, StoreError> {
    draft(conn, id)?.ok_or_else(|| StoreError::Draft(DraftError::NotFound(id.clone())))
}

fn require_editing(conn: &Connection, id: &DraftId) -> Result<StoredDraft, StoreError> {
    let draft = require_draft(conn, id)?;
    if draft.state != DraftState::Editing {
        return Err(StoreError::Draft(DraftError::Sending(id.clone())));
    }
    Ok(draft)
}

pub(crate) fn draft_for_message(
    conn: &Connection,
    account: AccountId,
    message_id_header: &str,
) -> Result<Option<StoredDraft>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    Ok(conn
        .prepare_cached(DRAFT_BY_MESSAGE_ID)?
        .query_row(params![row, message_id_header], draft_from_row)
        .optional()?)
}

/// See [`crate::Store::draft_for_kept_original`].
pub(crate) fn draft_for_kept_original(
    conn: &Connection,
    account: AccountId,
    message_id_header: &str,
) -> Result<Option<StoredDraft>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    Ok(conn
        .prepare_cached(DRAFT_BY_KEPT_ORIGINAL)?
        .query_row(params![row, message_id_header], draft_from_row)
        .optional()?)
}

pub(crate) fn drafts(
    conn: &Connection,
    account: AccountId,
    limit: u32,
) -> Result<Vec<StoredDraft>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    Ok(conn
        .prepare_cached(DRAFTS_OF_ACCOUNT)?
        .query_map(params![row, limit], draft_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn drafts_to_save(conn: &Connection) -> Result<Vec<DraftId>, StoreError> {
    let ids = conn
        .prepare_cached(DRAFTS_TO_SAVE)?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids.iter().filter_map(|id| DraftId::parse(id)).collect())
}

fn references_text(references: &[String]) -> Option<String> {
    (!references.is_empty()).then(|| references.join(" "))
}

fn addresses(list: &[Address]) -> Result<String, StoreError> {
    serde_json::to_string(list).map_err(json_error("encode draft addresses"))
}

/// See [`crate::Store::create_draft`].
pub(crate) fn insert_draft(
    conn: &Connection,
    new: &NewDraftRow,
) -> Result<StoredDraft, StoreError> {
    let account_row = accounts::row_id(conn, new.account)?;
    if let Some(identity) = new.identity {
        let owner: Option<i64> = conn
            .prepare_cached("SELECT account_id FROM identity WHERE id = ?1")?
            .query_row([identity.0], |r| r.get(0))
            .optional()?;
        if owner != Some(account_row) {
            return Err(StoreError::Draft(DraftError::NoIdentity(format!(
                "identity {} does not belong to account {}",
                identity.0, new.account
            ))));
        }
    }
    let content = &new.content;
    let (copy_mailbox, copy_name, copy_validity, copy_uid) = match &new.server_copy {
        Some(copy) => (
            Some(copy.mailbox.mailbox_id.0),
            Some(copy.mailbox.name.clone()),
            Some(copy.uid_validity.0),
            Some(copy.uid),
        ),
        None => (None, None, None, None),
    };
    conn.prepare_cached(INSERT_DRAFT)?
        .execute(params![
            new.id.as_str(),
            account_row,
            content.identity.map(|i| i.0).or(new.identity.map(|i| i.0)),
            new.kind.as_str(),
            new.message_id_header,
            new.original.map(|m| m.0),
            new.original_message_id_header,
            new.in_reply_to,
            references_text(&new.references),
            addresses(&content.to)?,
            addresses(&content.cc)?,
            addresses(&content.bcc)?,
            content.subject,
            content.format.as_str(),
            content.body_html,
            content.body_text,
            new.quote_html,
            new.quote_text,
            content.include_quote,
            content.quote_position.as_str(),
            content.include_signature,
            files_text(&new.attachments)?,
            // A draft opened from the Drafts folder: the server copy is revision 1.
            new.server_copy.as_ref().map(|_| 1_i64),
            copy_mailbox,
            copy_name,
            copy_validity,
            copy_uid,
            new.now,
            content
                .signature
                .as_ref()
                .map(mail_model::SignatureId::as_str),
            new.kept_original,
        ])
        .map_err(StoreError::sqlite("insert draft"))?;
    require_draft(conn, &new.id)
}

/// See [`crate::Store::save_draft`].
pub(crate) fn save_draft(
    conn: &Connection,
    id: &DraftId,
    content: &DraftContent,
    now: i64,
) -> Result<StoredDraft, StoreError> {
    let draft = require_editing(conn, id)?;
    if let Some(identity) = content.identity {
        let owner: Option<i64> = conn
            .prepare_cached("SELECT account_id FROM identity WHERE id = ?1")?
            .query_row([identity.0], |r| r.get(0))
            .optional()?;
        if owner != Some(draft.account_row) {
            return Err(StoreError::Draft(DraftError::NoIdentity(format!(
                "identity {} does not belong to the draft's account",
                identity.0
            ))));
        }
    }
    conn.prepare_cached(UPDATE_DRAFT)?
        .execute(params![
            draft.row,
            content.identity.map(|i| i.0),
            addresses(&content.to)?,
            addresses(&content.cc)?,
            addresses(&content.bcc)?,
            content.subject,
            content.format.as_str(),
            content.body_html,
            content.body_text,
            content.include_quote,
            content.quote_position.as_str(),
            content.include_signature,
            now,
            content
                .signature
                .as_ref()
                .map(mail_model::SignatureId::as_str),
        ])
        .map_err(StoreError::sqlite("save draft"))?;
    require_draft(conn, id)
}

/// See [`crate::Store::set_draft_send_options`].
pub(crate) fn set_send_options(
    conn: &Connection,
    id: &DraftId,
    archive_original: bool,
    request_receipt: bool,
    follow_up_at: Option<i64>,
    now: i64,
) -> Result<StoredDraft, StoreError> {
    let draft = require_editing(conn, id)?;
    if draft.archive_original != archive_original
        || draft.request_receipt != request_receipt
        || draft.follow_up_at != follow_up_at
    {
        conn.prepare_cached(SET_DRAFT_SEND_OPTIONS)?
            .execute(params![
                draft.row,
                archive_original,
                request_receipt,
                now,
                follow_up_at
            ])
            .map_err(StoreError::sqlite("set draft send options"))?;
    }
    require_draft(conn, id)
}

/// See [`crate::Store::set_draft_attachments`].
pub(crate) fn set_attachments(
    conn: &Connection,
    id: &DraftId,
    attachments: &[DraftFile],
    now: i64,
) -> Result<StoredDraft, StoreError> {
    let draft = require_editing(conn, id)?;
    conn.prepare_cached(SET_DRAFT_ATTACHMENTS)?
        .execute(params![draft.row, files_text(attachments)?, now])
        .map_err(StoreError::sqlite("set draft attachments"))?;
    require_draft(conn, id)
}

fn queued_appends(conn: &Connection, draft_row: i64) -> Result<Vec<OperationId>, StoreError> {
    Ok(conn
        .prepare_cached(QUEUED_APPENDS_OF_DRAFT)?
        .query_map([draft_row], |r| Ok(OperationId(r.get(0)?)))?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Deletes the draft's queued appends (nothing went out for them yet).
fn cancel_appends(
    conn: &Connection,
    draft_row: i64,
    change: &mut OutboxChange,
) -> Result<(), StoreError> {
    for id in queued_appends(conn, draft_row)? {
        conn.prepare_cached(DELETE_OPERATION)?
            .execute([id.0])
            .map_err(StoreError::sqlite("cancel append"))?;
        change.removed.push(id);
    }
    Ok(())
}

pub(crate) fn mailbox_with_role(
    conn: &Connection,
    account_row: i64,
    role: MailboxRole,
) -> Result<Option<TargetMailbox>, StoreError> {
    let Some(role) = role.as_db() else {
        return Ok(None);
    };
    Ok(conn
        .prepare_cached(MAILBOX_WITH_ROLE)?
        .query_row(params![account_row, role], |r| {
            Ok(TargetMailbox {
                mailbox_id: MailboxId(r.get(0)?),
                name: r.get(1)?,
            })
        })
        .optional()?)
}

/// See [`crate::Store::delete_draft`]: the row goes; its queued appends are cancelled; a copy
/// on the server is removed through the outbox (only with writes on: otherwise it stays).
pub(crate) fn delete_draft(
    conn: &Connection,
    id: &DraftId,
    request: &str,
    now: i64,
) -> Result<(StoredDraft, OutboxChange), StoreError> {
    let draft = require_editing(conn, id)?;
    let (account, writes) = account_writes(conn, draft.account_row)?;
    let mut recorder = Recorder::new(
        conn,
        draft.account_row,
        account,
        ActionKind::DeleteDraft,
        request,
        now,
    );
    cancel_appends(conn, draft.row, &mut recorder.change)?;
    if writes && let Some(copy) = draft.server_copy.clone() {
        recorder.record(OperationPayload::RemoveCopy { copy })?;
    }
    conn.prepare_cached(DELETE_DRAFT)?
        .execute([draft.row])
        .map_err(StoreError::sqlite("delete draft"))?;
    recorder.change.drafts_removed.push(id.clone());
    Ok((draft, recorder.change))
}

/// See [`crate::Store::enqueue_draft_save`]. `None` when nothing is queued: the draft is
/// frozen, writes are off, the account has no Drafts folder, the server has this revision
/// already, or an append that builds the newest version is still queued.
pub(crate) fn enqueue_save(
    conn: &Connection,
    id: &DraftId,
    request: &str,
    now: i64,
) -> Result<Option<OutboxChange>, StoreError> {
    let Some(draft) = draft(conn, id)? else {
        return Ok(None);
    };
    if draft.state != DraftState::Editing {
        return Ok(None);
    }
    let (account, writes) = account_writes(conn, draft.account_row)?;
    if !writes {
        return Ok(None);
    }
    if draft.server_copy.is_some() && draft.server_revision >= Some(draft.revision) {
        return Ok(None);
    }
    if !queued_appends(conn, draft.row)?.is_empty() {
        return Ok(None);
    }
    let Some(target) = mailbox_with_role(conn, draft.account_row, MailboxRole::Drafts)? else {
        return Ok(None);
    };
    if repeated(conn, request)?.is_some() {
        return Ok(None);
    }
    let mut recorder = Recorder::new(
        conn,
        draft.account_row,
        account,
        ActionKind::SaveDraft,
        request,
        now,
    );
    recorder.record(OperationPayload::Append {
        target,
        source: AppendSource::Draft {
            draft: draft.id.clone(),
            revision: None,
        },
        message_id_header: draft.message_id_header.clone(),
        draft_flag: true,
        mark: None,
        replace: None,
    })?;
    recorder.change.draft_changed(id);
    Ok(Some(recorder.change))
}

/// See [`crate::Store::enqueue_send`].
pub(crate) fn enqueue_send(
    conn: &Connection,
    id: &DraftId,
    job: SendJob,
    not_before: Option<i64>,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let draft = require_draft(conn, id)?;
    let account = require_writes(conn, draft.account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    if draft.state != DraftState::Editing {
        return Err(StoreError::Draft(DraftError::Sending(id.clone())));
    }
    let mut recorder = Recorder::new(
        conn,
        draft.account_row,
        account,
        ActionKind::Send,
        request,
        now,
    );
    cancel_appends(conn, draft.row, &mut recorder.change)?;
    conn.prepare_cached(SET_DRAFT_STATE)?
        .execute(params![draft.row, DraftState::Sending.as_str(), now])
        .map_err(StoreError::sqlite("freeze draft"))?;
    recorder.not_before = not_before;
    recorder.record(OperationPayload::Send(job))?;
    recorder.change.draft_changed(id);
    Ok(recorder.change)
}

/// Releases the draft of a send that was cancelled or discarded (back to `editing`).
pub(crate) fn release_draft(
    conn: &Connection,
    op: &StoredOperation,
    now: i64,
    change: &mut OutboxChange,
) -> Result<(), StoreError> {
    if let OperationPayload::Send(job) = &op.payload {
        let released = conn
            .prepare_cached(RELEASE_DRAFT)?
            .execute(params![job.draft.as_str(), now])
            .map_err(StoreError::sqlite("release draft"))?;
        if released > 0 {
            change.draft_changed(&job.draft);
        }
    }
    Ok(())
}

/// A retried send needs its frozen draft (the bytes are frozen anyway; the draft is what the
/// user gets back with "edit").
pub(crate) fn require_frozen(conn: &Connection, id: &DraftId) -> Result<(), StoreError> {
    match draft(conn, id)? {
        Some(draft) if draft.state == DraftState::Sending => Ok(()),
        Some(_) => Err(StoreError::Draft(DraftError::NotFound(id.clone()))),
        None => Err(StoreError::Draft(DraftError::NotFound(id.clone()))),
    }
}

/// See [`crate::Store::cancel_send`].
pub(crate) fn cancel_send(
    conn: &Connection,
    id: &DraftId,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let draft = require_draft(conn, id)?;
    let mut change = OutboxChange::new(draft.account);
    let Some(op_id) = draft.outgoing else {
        // Frozen without a send (cannot normally happen): release it.
        if draft.state == DraftState::Sending {
            conn.prepare_cached(SET_DRAFT_STATE)?
                .execute(params![draft.row, DraftState::Editing.as_str(), now])
                .map_err(StoreError::sqlite("release draft"))?;
            change.draft_changed(id);
        }
        return Ok(change);
    };
    let op = require_operation(conn, op_id)?;
    match op.state {
        OperationState::InFlight => Err(StoreError::Draft(DraftError::TooLate(op_id))),
        OperationState::Confirmed => Err(StoreError::Draft(DraftError::TooLate(op_id))),
        OperationState::Queued | OperationState::Failed | OperationState::Conflict => {
            conn.prepare_cached(DELETE_OPERATION)?
                .execute([op_id.0])
                .map_err(StoreError::sqlite("cancel send"))?;
            change.removed.push(op_id);
            release_draft(conn, &op, now, &mut change)?;
            Ok(change)
        }
    }
}

/// See [`crate::Store::resolve_unclear_send`].
pub(crate) fn resolve_unclear(
    conn: &Connection,
    id: OperationId,
    decision: UnclearDecision,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let op = require_operation(conn, id)?;
    let unclear = op.state == OperationState::Failed
        && op
            .last_error
            .as_ref()
            .is_some_and(|e| e.code == ErrorCode::SendUnclear);
    let OperationPayload::Send(mut job) = op.payload.clone() else {
        return Err(StoreError::Action(ActionError::WrongState(id, op.state)));
    };
    if !unclear {
        return Err(StoreError::Action(ActionError::WrongState(id, op.state)));
    }
    let mut change = OutboxChange::new(op.account);
    match decision {
        UnclearDecision::TreatAsSent => {
            let notice = send_followups(conn, &op, false, now, &mut change)?;
            conn.prepare_cached(FINISH_OPERATION)?
                .execute(params![
                    id.0,
                    OperationState::Confirmed.as_str(),
                    None::<String>,
                    None::<String>,
                    notice.map(OperationNotice::as_str),
                    now,
                ])
                .map_err(StoreError::sqlite("confirm unclear send"))?;
        }
        UnclearDecision::SendAgain => {
            let account_row = accounts::row_id(conn, op.account)?;
            require_writes(conn, account_row)?;
            job.phase = SendPhase::Submit;
            job.checks = 0;
            conn.prepare_cached(LATER_OPERATION)?
                .execute(params![
                    id.0,
                    OperationState::Queued.as_str(),
                    None::<String>,
                    None::<i64>,
                    Some(payload_text(&OperationPayload::Send(job))?),
                    now,
                ])
                .map_err(StoreError::sqlite("send again"))?;
        }
    }
    let updated = operation(conn, id)?;
    change.operations.splice(0..0, updated);
    Ok(change)
}

/// See [`crate::OperationResult::Sent`]. Returns the notice of the send.
pub(crate) fn send_followups(
    conn: &Connection,
    op: &StoredOperation,
    copy_on_server: bool,
    now: i64,
    change: &mut OutboxChange,
) -> Result<Option<OperationNotice>, StoreError> {
    let OperationPayload::Send(job) = &op.payload else {
        return Ok(None);
    };
    let account_row = accounts::row_id(conn, op.account)?;
    if let Some(original) = &job.receipt_for {
        receipt_sent(conn, op, account_row, original, now, change)?;
        return Ok(None);
    }
    let request = format!("send-{}", op.id);
    let mut recorder = Recorder::new(
        conn,
        account_row,
        op.account,
        ActionKind::Send,
        &request,
        now,
    );
    recorder.change.batch = Some(op.batch);
    let mut notice = None;
    if !copy_on_server && !job.server_saves_sent {
        match mailbox_with_role(conn, account_row, MailboxRole::Sent)? {
            Some(target) => recorder.record(OperationPayload::Append {
                target,
                source: AppendSource::File {
                    file: job.sent_file.clone(),
                },
                message_id_header: job.message_id_header.clone(),
                draft_flag: false,
                mark: None,
                replace: None,
            })?,
            None => notice = Some(OperationNotice::NoSentCopy),
        }
    }
    if let Some(draft) = draft(conn, &job.draft)? {
        cancel_appends(conn, draft.row, &mut recorder.change)?;
        if let Some(copy) = draft.server_copy {
            recorder.record(OperationPayload::RemoveCopy { copy })?;
        }
        conn.prepare_cached(DELETE_DRAFT)?
            .execute([draft.row])
            .map_err(StoreError::sqlite("delete sent draft"))?;
        recorder.change.drafts_removed.push(job.draft.clone());
    }
    if let Some(original) = &job.original {
        let flag = if original.forwarded {
            Flag::Keyword("$Forwarded".into())
        } else {
            Flag::Answered
        };
        let messages = conn
            .prepare_cached(MESSAGES_WITH_MSGID)?
            .query_map(params![account_row, original.message_id_header], |r| {
                Ok(MessageId(r.get(0)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for message in messages {
            let mailboxes = conn
                .prepare_cached(LOCATIONS_OF_MESSAGE)?
                .query_map([message.0], |r| Ok(MailboxId(r.get(0)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            for mailbox in mailboxes {
                let at = MessageRef {
                    mailbox_id: mailbox,
                    message_id: message,
                };
                if let Some(planned) = outbox::location_for_plan(conn, at)? {
                    recorder.record_flag(planned.location.source, flag.clone(), true)?;
                }
            }
        }
    }
    let recorded = recorder.change;
    merge_change(change, recorded);
    // Stage 20 (decision 0059): "remind me if no reply" counts from the accepted message.
    if let Some(due) = job.follow_up_at {
        crate::follow_ups::record_sent(conn, account_row, &job.message_id_header, due, now)?;
    }
    if job.archive_original
        && let Some(original) = &job.original
    {
        archive_original(
            conn,
            op,
            account_row,
            &original.message_id_header,
            now,
            change,
        )?;
    }
    Ok(notice)
}

fn merge_change(change: &mut OutboxChange, recorded: OutboxChange) {
    change.operations.extend(recorded.operations);
    change.removed.extend(recorded.removed);
    for mailbox in recorded.mailboxes {
        if !change.mailboxes.contains(&mailbox) {
            change.mailboxes.push(mailbox);
        }
    }
    change.drafts_removed.extend(recorded.drafts_removed);
}

/// The locations of the account's messages with this Message-ID, as the planner sees them.
fn original_locations(
    conn: &Connection,
    account_row: i64,
    message_id_header: &str,
) -> Result<Vec<mail_model::PlanLocation>, StoreError> {
    let messages = conn
        .prepare_cached(MESSAGES_WITH_MSGID)?
        .query_map(params![account_row, message_id_header], |r| {
            Ok(MessageId(r.get(0)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for message in messages {
        let mailboxes = conn
            .prepare_cached(LOCATIONS_OF_MESSAGE)?
            .query_map([message.0], |r| Ok(MailboxId(r.get(0)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for mailbox in mailboxes {
            let at = MessageRef {
                mailbox_id: mailbox,
                message_id: message,
            };
            if let Some(planned) = outbox::location_for_plan(conn, at)? {
                out.push(planned.location);
            }
        }
    }
    Ok(out)
}

/// Stage 16, "send and archive" (decision 0033): the original's locations outside Archive
/// (and outside Trash, Junk, Sent and Drafts) move to the archive mailbox, recorded in the
/// transaction that confirms the send. Without an archive mailbox the send's payload notes it
/// (`archive_missing`); the send itself stays confirmed.
fn archive_original(
    conn: &Connection,
    op: &StoredOperation,
    account_row: i64,
    message_id_header: &str,
    now: i64,
    change: &mut OutboxChange,
) -> Result<(), StoreError> {
    let locations: Vec<_> = original_locations(conn, account_row, message_id_header)?
        .into_iter()
        .filter(|l| {
            !matches!(
                l.role,
                MailboxRole::Trash | MailboxRole::Junk | MailboxRole::Sent | MailboxRole::Drafts
            )
        })
        .collect();
    let mailboxes = outbox::plan_mailboxes(conn, account_row)?;
    let missing = !mailboxes
        .iter()
        .any(|m| m.role == MailboxRole::Archive && m.selectable);
    if missing {
        if let OperationPayload::Send(mut job) = op.payload.clone() {
            job.archive_missing = true;
            conn.prepare_cached(outbox::SET_PAYLOAD)?
                .execute(params![
                    op.id.0,
                    payload_text(&OperationPayload::Send(job))?
                ])
                .map_err(StoreError::sqlite("note missing archive"))?;
        }
        return Ok(());
    }
    if locations.is_empty() {
        return Ok(());
    }
    let payloads = match mail_model::plan_action(&MailAction::Archive, &locations, &mailboxes) {
        Ok(payloads) => payloads,
        Err(err) => {
            // Never a reason to fail the send that was accepted.
            tracing::warn!(error = %err, "archiving the original after the send was not planned");
            return Ok(());
        }
    };
    let (account, writes) = account_writes(conn, account_row)?;
    if !writes {
        return Ok(());
    }
    let request = format!("send-{}-archive", op.id);
    let mut recorder = Recorder::new(
        conn,
        account_row,
        account,
        ActionKind::Archive,
        &request,
        now,
    );
    recorder.change.batch = Some(op.batch);
    for payload in payloads {
        recorder.record(payload)?;
    }
    merge_change(change, recorder.change);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Read receipts (stage 16, decision 0033)

/// The keyword that tells every program the read-receipt request was answered (RFC 3503).
pub const MDN_SENT_KEYWORD: &str = "$MDNSent";

const UPSERT_RECEIPT_ANSWER: &str = "
    INSERT INTO receipt_answer (account_uuid, message_id_hdr, state, operation_id, updated_at)
    VALUES (?1, ?2, ?3, ?4, ?5)
    ON CONFLICT (account_uuid, message_id_hdr)
    DO UPDATE SET state = excluded.state, operation_id = excluded.operation_id,
                  updated_at = excluded.updated_at";

pub(crate) const RECEIPT_ANSWER: &str = "
    SELECT a.state, a.operation_id, (SELECT o.state FROM operation o WHERE o.id = a.operation_id)
    FROM receipt_answer a WHERE a.account_uuid = ?1 AND a.message_id_hdr = ?2";

const UPSERT_RECEIPT_RECEIVED: &str = "
    INSERT INTO receipt_received (account_uuid, original_msgid_hdr, recipient, disposition,
                                  received_at)
    VALUES (?1, ?2, ?3, ?4, ?5)
    ON CONFLICT (account_uuid, original_msgid_hdr, recipient)
    DO UPDATE SET disposition = excluded.disposition, received_at = excluded.received_at
    WHERE excluded.received_at >= receipt_received.received_at";

pub(crate) const RECEIPTS_RECEIVED: &str = "
    SELECT recipient, disposition, received_at FROM receipt_received
    WHERE account_uuid = ?1 AND original_msgid_hdr = ?2 LIMIT 100";

/// The user's answer to a read-receipt request as stored: the state, and for a queued one the
/// state of its send operation (`None` once purged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReceiptAnswer {
    /// `queued`, `sent` or `declined`.
    pub state: String,
    pub operation: Option<OperationId>,
    pub operation_state: Option<OperationState>,
}

fn set_answer(
    conn: &Connection,
    account: AccountId,
    message_id_header: &str,
    state: &str,
    operation: Option<OperationId>,
    now: i64,
) -> Result<(), StoreError> {
    conn.prepare_cached(UPSERT_RECEIPT_ANSWER)?
        .execute(params![
            account.to_string(),
            message_id_header,
            state,
            operation.map(|o| o.0),
            now
        ])
        .map_err(StoreError::sqlite("record read receipt answer"))?;
    Ok(())
}

/// `$MDNSent` on the original's locations, where the account may write.
fn mark_answered(
    conn: &Connection,
    account_row: i64,
    message_id_header: &str,
    request: &str,
    now: i64,
    change: &mut OutboxChange,
) -> Result<(), StoreError> {
    let (account, writes) = account_writes(conn, account_row)?;
    if !writes {
        return Ok(());
    }
    let locations = original_locations(conn, account_row, message_id_header)?;
    let mut recorder = Recorder::new(
        conn,
        account_row,
        account,
        ActionKind::AddKeyword,
        request,
        now,
    );
    for location in locations {
        recorder.record_flag(
            location.source,
            Flag::Keyword(MDN_SENT_KEYWORD.into()),
            true,
        )?;
    }
    merge_change(change, recorder.change);
    Ok(())
}

/// The follow-up of an accepted read receipt: the answer is `sent`, the original gets
/// `$MDNSent`. No copy in Sent, no draft.
fn receipt_sent(
    conn: &Connection,
    op: &StoredOperation,
    account_row: i64,
    original: &str,
    now: i64,
    change: &mut OutboxChange,
) -> Result<(), StoreError> {
    set_answer(conn, op.account, original, "sent", Some(op.id), now)?;
    mark_answered(
        conn,
        account_row,
        original,
        &format!("receipt-{}", op.id),
        now,
        change,
    )
}

/// See [`crate::Store::enqueue_receipt`].
pub(crate) fn enqueue_receipt(
    conn: &Connection,
    account: AccountId,
    job: SendJob,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    require_writes(conn, account_row)?;
    let original = job
        .receipt_for
        .clone()
        .ok_or_else(|| StoreError::InvalidArgument("not a read receipt".into()))?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    let mut recorder = Recorder::new(conn, account_row, account, ActionKind::Send, request, now);
    let id = recorder.insert(&OperationPayload::Send(job))?;
    set_answer(conn, account, &original, "queued", Some(id), now)?;
    Ok(recorder.change)
}

/// See [`crate::Store::decline_receipt`].
pub(crate) fn decline_receipt(
    conn: &Connection,
    account: AccountId,
    message_id_header: &str,
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    let account_row = accounts::row_id(conn, account)?;
    let mut change = OutboxChange::new(account);
    if repeated(conn, request)?.is_some() {
        change.repeated = true;
        return Ok(change);
    }
    set_answer(conn, account, message_id_header, "declined", None, now)?;
    mark_answered(
        conn,
        account_row,
        message_id_header,
        request,
        now,
        &mut change,
    )?;
    Ok(change)
}

pub(crate) fn receipt_answer(
    conn: &Connection,
    account: AccountId,
    message_id_header: &str,
) -> Result<Option<StoredReceiptAnswer>, StoreError> {
    Ok(conn
        .prepare_cached(RECEIPT_ANSWER)?
        .query_row(params![account.to_string(), message_id_header], |r| {
            let state: Option<String> = r.get(2)?;
            Ok(StoredReceiptAnswer {
                state: r.get(0)?,
                operation: r.get::<_, Option<i64>>(1)?.map(OperationId),
                operation_state: state.as_deref().and_then(OperationState::parse),
            })
        })
        .optional()?)
}

/// A read receipt whose body was just stored: recorded for the message it reports on.
pub(crate) fn record_received(
    conn: &Connection,
    receipt: MessageId,
    report: &crate::ReceivedReceipt,
) -> Result<(), StoreError> {
    let row: Option<(String, i64)> = conn
        .prepare_cached(
            "SELECT (SELECT a.uuid FROM account a WHERE a.id = m.account_id), m.date
             FROM message m WHERE m.id = ?1",
        )?
        .query_row([receipt.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((account, date)) = row else {
        return Ok(());
    };
    conn.prepare_cached(UPSERT_RECEIPT_RECEIVED)?
        .execute(params![
            account,
            report.original_message_id,
            report.recipient.as_deref().unwrap_or_default(),
            report.disposition.as_str(),
            date.saturating_mul(1000)
        ])
        .map_err(StoreError::sqlite("record received read receipt"))?;
    Ok(())
}

pub(crate) fn received_receipts(
    conn: &Connection,
    account: AccountId,
    message_id_header: &str,
) -> Result<Vec<mail_model::ReceivedReadReceipt>, StoreError> {
    // One row per recipient of one message: sorted here, not by a temporary B-tree.
    let mut out = conn
        .prepare_cached(RECEIPTS_RECEIVED)?
        .query_map(params![account.to_string(), message_id_header], |r| {
            let recipient: String = r.get(0)?;
            let disposition: String = r.get(1)?;
            Ok(mail_model::ReceivedReadReceipt {
                recipient: (!recipient.is_empty()).then_some(recipient),
                disposition: mail_model::ReceiptDisposition::parse(&disposition)
                    .unwrap_or(mail_model::ReceiptDisposition::Other),
                received_at: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    out.sort_by_key(|r| std::cmp::Reverse(r.received_at));
    Ok(out)
}

/// See [`crate::OperationResult::Appended`].
pub(crate) fn appended(
    conn: &Connection,
    op: &StoredOperation,
    copy: Option<&ServerCopy>,
    now: i64,
    change: &mut OutboxChange,
) -> Result<(), StoreError> {
    let OperationPayload::Append {
        source: AppendSource::Draft {
            draft: id,
            revision,
        },
        ..
    } = &op.payload
    else {
        return Ok(());
    };
    match draft(conn, id)? {
        Some(draft) => {
            let copy = copy.cloned();
            conn.prepare_cached(SET_DRAFT_SERVER_COPY)?
                .execute(params![
                    draft.row,
                    copy.as_ref().and(*revision),
                    copy.as_ref().map(|c| c.mailbox.mailbox_id.0),
                    copy.as_ref().map(|c| c.mailbox.name.clone()),
                    copy.as_ref().map(|c| c.uid_validity.0),
                    copy.as_ref().map(|c| c.uid),
                ])
                .map_err(StoreError::sqlite("set draft server copy"))?;
            change.draft_changed(id);
        }
        None => {
            // Discarded or sent while this version was on its way: remove it again.
            if let Some(copy) = copy {
                let account_row = accounts::row_id(conn, op.account)?;
                let request = format!("orphan-{}", op.id);
                let mut recorder = Recorder::new(
                    conn,
                    account_row,
                    op.account,
                    ActionKind::DeleteDraft,
                    &request,
                    now,
                );
                recorder.record(OperationPayload::RemoveCopy { copy: copy.clone() })?;
                change.operations.extend(recorder.change.operations);
            }
        }
    }
    Ok(())
}

/// The account's sends, newest first, below `before` (outbox view).
pub(crate) fn sends(
    conn: &Connection,
    account: AccountId,
    before: Option<OperationId>,
    limit: u32,
) -> Result<Vec<StoredOperation>, StoreError> {
    let row = accounts::row_id(conn, account)?;
    let before = before.map_or(i64::MAX, |id| id.0);
    Ok(conn
        .prepare_cached(outbox::SENDS_OF_ACCOUNT)?
        .query_map(params![row, before, limit], outbox::operation_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}
