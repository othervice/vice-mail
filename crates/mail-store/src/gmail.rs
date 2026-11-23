//! Stage 14, block A (decision 0034): Gmail over IMAP in the store.
//!
//! - **One message across labels:** a header with `X-GM-MSGID` merges with the account's
//!   message of that number (a message stored before this stage, without the number, is found
//!   by its Message-ID and gets it). A message in the inbox, two labels and "All Mail" is one
//!   row with four locations; counts and lists per mailbox stay as they are, the unified inbox
//!   lists inboxes only, and new mail is counted once (`StoreReport::new_unread`).
//! - **Labels:** `message.gm_labels` keeps `X-GM-LABELS` as last read, completed with the
//!   label of the folder it was read in. Shown labels ([`message_labels`]) are those plus the
//!   labels the message's locations imply, so messages stored before this stage show theirs too.
//! - **Changes** go through the outbox as `SetFlag` with [`Flag::Label`]; their local effect is
//!   on the message ([`set_local_label`]). Removing `\Inbox` from the inbox location (archive on
//!   Gmail) also takes the message out of the inbox locally, like a move; reverting puts it back.
//! - **Gmail seen at login** (`account_server.gmail`): the sync records it; the planner and the
//!   sent copy setting read it.

use std::collections::BTreeSet;

use mail_model::{
    AccountId, ActionError, Flag, GmailInfo, INBOX_LABEL, MailAction, MailboxId, MailboxRole,
    MessageId, MessageRef, OperationPayload, PlanLocation, PlanMailbox, SourceLocation, Uid,
    label_of_mailbox, normalize_labels,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Store, StoreError, accounts};

/// The account's message with this `X-GM-MSGID` that does not lie in the mailbox yet.
const BY_GMAIL_ID: &str = "
    SELECT m.id FROM message m INDEXED BY message_gm_msgid
    WHERE m.account_id = ?1 AND m.gm_msgid = ?2
      AND NOT EXISTS (SELECT 1 FROM message_location l
                      WHERE l.mailbox_id = ?3 AND l.message_id = m.id)
    ORDER BY m.id LIMIT 1";

/// A message stored before its `X-GM-MSGID` was known, by Message-ID (the old merge rule).
const BY_MESSAGE_ID_UNNUMBERED: &str = "
    SELECT m.id FROM message m
    WHERE m.account_id = ?1 AND m.message_id_hdr = ?2 AND m.gm_msgid IS NULL
      AND NOT EXISTS (SELECT 1 FROM message_location l
                      WHERE l.mailbox_id = ?3 AND l.message_id = m.id)
    ORDER BY m.id LIMIT 1";

const SET_GMAIL: &str = "UPDATE message SET gm_msgid = ?2, gm_labels = ?3 WHERE id = ?1";
const SET_LABELS: &str = "UPDATE message SET gm_labels = ?2 WHERE id = ?1";
const LABELS: &str = "SELECT gm_labels FROM message WHERE id = ?1";
/// Labels of a message and whether its account is Gmail.
const LABELS_AND_GMAIL: &str = "
    SELECT m.gm_labels,
           EXISTS (SELECT 1 FROM account_server s WHERE s.account_id = m.account_id AND s.gmail = 1)
    FROM message m WHERE m.id = ?1";
const MESSAGE_AT_UID: &str =
    "SELECT message_id FROM message_location WHERE mailbox_id = ?1 AND uid = ?2";
const MAILBOX_LABEL: &str = "SELECT role, path FROM mailbox WHERE id = ?1";
const LOCATION_LABELS: &str = "
    SELECT b.role, b.path FROM message_location l CROSS JOIN mailbox b ON b.id = l.mailbox_id
    WHERE l.message_id = ?1";
const DELETE_LOCATION: &str =
    "DELETE FROM message_location WHERE mailbox_id = ?1 AND message_id = ?2 AND uid = ?3";
const ACCOUNT_GMAIL: &str = "SELECT gmail FROM account_server WHERE account_id = ?1";
const SET_ACCOUNT_GMAIL: &str = "
    INSERT INTO account_server (account_id, gmail, updated_at) VALUES (?1, ?2, ?3)
    ON CONFLICT (account_id) DO UPDATE SET gmail = excluded.gmail, updated_at = excluded.updated_at
    WHERE gmail <> excluded.gmail";

/// `X-GM-MSGID` as stored: the signed pattern of Gmail's unsigned 64-bit number.
fn stored_msgid(msgid: u64) -> i64 {
    msgid.cast_signed()
}

fn labels_text(labels: &[String]) -> Result<String, StoreError> {
    serde_json::to_string(labels)
        .map_err(|err| StoreError::InvalidArgument(format!("labels: {err}")))
}

fn labels_from(text: Option<&str>) -> Vec<String> {
    text.and_then(|t| serde_json::from_str::<Vec<String>>(t).ok())
        .unwrap_or_default()
}

/// The label a message has because it lies in `mailbox` (see `mail_model::label_of_mailbox`).
pub(crate) fn mailbox_label(
    conn: &Connection,
    mailbox: MailboxId,
) -> Result<Option<String>, StoreError> {
    let row: Option<(Option<String>, String)> = conn
        .prepare_cached(MAILBOX_LABEL)?
        .query_row([mailbox.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(row.and_then(|(role, path)| label_of_mailbox(MailboxRole::from_db(role.as_deref()), &path)))
}

/// `X-GM-LABELS` completed with the label of the folder they were read in.
fn completed(labels: &[String], implied: Option<&str>) -> Vec<String> {
    normalize_labels(labels.iter().map(String::as_str).chain(implied))
}

/// The message a Gmail header belongs to: by `X-GM-MSGID`, else by Message-ID among messages
/// without a number (stored before stage 14). Distinct numbers are distinct messages, also with
/// the same Message-ID.
pub(crate) fn merge_candidate(
    conn: &Connection,
    account_row: i64,
    mailbox: MailboxId,
    info: &GmailInfo,
    message_id: Option<&str>,
) -> Result<Option<i64>, StoreError> {
    let found: Option<i64> = conn
        .prepare_cached(BY_GMAIL_ID)?
        .query_row(
            params![account_row, stored_msgid(info.msgid), mailbox.0],
            |r| r.get(0),
        )
        .optional()?;
    if found.is_some() {
        return Ok(found);
    }
    let Some(message_id) = message_id else {
        return Ok(None);
    };
    Ok(conn
        .prepare_cached(BY_MESSAGE_ID_UNNUMBERED)?
        .query_row(params![account_row, message_id, mailbox.0], |r| r.get(0))
        .optional()?)
}

/// Writes `X-GM-MSGID` and the labels of a header to its message.
pub(crate) fn tag_message(
    conn: &Connection,
    message: i64,
    info: &GmailInfo,
    implied: Option<&str>,
) -> Result<(), StoreError> {
    let labels = completed(&info.labels, implied);
    conn.prepare_cached(SET_GMAIL)?
        .execute(params![
            message,
            stored_msgid(info.msgid),
            labels_text(&labels)?
        ])
        .map_err(StoreError::sqlite("store Gmail message id"))?;
    Ok(())
}

/// Labels read with the flags of the message at `uid`. Returns whether they changed.
pub(crate) fn refresh_labels(
    conn: &Connection,
    mailbox: MailboxId,
    uid: Uid,
    labels: &[String],
    implied: Option<&str>,
) -> Result<bool, StoreError> {
    let message: Option<i64> = conn
        .prepare_cached(MESSAGE_AT_UID)?
        .query_row(params![mailbox.0, uid], |r| r.get(0))
        .optional()?;
    let Some(message) = message else {
        return Ok(false);
    };
    let labels = completed(labels, implied);
    let stored: Option<String> = conn
        .prepare_cached(LABELS)?
        .query_row([message], |r| r.get(0))
        .optional()?
        .flatten();
    let text = labels_text(&labels)?;
    if stored.as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    conn.prepare_cached(SET_LABELS)?
        .execute(params![message, text])
        .map_err(StoreError::sqlite("store Gmail labels"))?;
    Ok(true)
}

/// Labels of a message as shown: `X-GM-LABELS` as stored plus the labels its locations imply.
/// `None` if the message does not exist or is not a Gmail message (no labels stored and the
/// account's last login did not see `X-GM-EXT-1`).
pub(crate) fn message_labels(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<Vec<String>>, StoreError> {
    let stored: Option<(Option<String>, bool)> = conn
        .prepare_cached(LABELS_AND_GMAIL)?
        .query_row([message.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((stored, gmail)) = stored else {
        return Ok(None);
    };
    if !gmail && stored.is_none() {
        return Ok(None);
    }
    let mut labels = labels_from(stored.as_deref());
    let implied = conn
        .prepare_cached(LOCATION_LABELS)?
        .query_map([message.0], |r| {
            let role: Option<String> = r.get(0)?;
            let path: String = r.get(1)?;
            Ok(label_of_mailbox(
                MailboxRole::from_db(role.as_deref()),
                &path,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    labels.extend(implied.into_iter().flatten());
    if stored.is_none() && labels.is_empty() {
        return Ok(None);
    }
    Ok(Some(normalize_labels(labels)))
}

/// Whether the message carries `label` as far as the app knows (see [`message_labels`]), or
/// lies in the folder of that label.
pub(crate) fn label_state(
    conn: &Connection,
    source: &SourceLocation,
    label: &str,
) -> Result<bool, StoreError> {
    if mailbox_label(conn, source.mailbox_id)?.as_deref() == Some(label) {
        return Ok(true);
    }
    Ok(message_labels(conn, source.message_id)?
        .is_some_and(|labels| labels.iter().any(|l| l == label)))
}

fn is_inbox(conn: &Connection, mailbox: MailboxId) -> Result<bool, StoreError> {
    Ok(mailbox_label(conn, mailbox)?.as_deref() == Some(INBOX_LABEL))
}

/// Local effect of a label operation: the label on the message, and for `\Inbox` on the inbox
/// location the location itself (archive takes the message out of the inbox at once; revert
/// and cancel put it back with its flags). Returns whether something changed.
pub(crate) fn set_local_label(
    conn: &Connection,
    source: &SourceLocation,
    label: &str,
    value: bool,
) -> Result<bool, StoreError> {
    let mut changed = false;
    let stored: Option<Option<String>> = conn
        .prepare_cached(LABELS)?
        .query_row([source.message_id.0], |r| r.get(0))
        .optional()?;
    if let Some(stored) = stored {
        let mut labels = labels_from(stored.as_deref());
        let has = labels.iter().any(|l| l == label);
        if has != value {
            if value {
                labels.push(label.to_owned());
            } else {
                labels.retain(|l| l != label);
            }
            let labels = normalize_labels(labels);
            conn.prepare_cached(SET_LABELS)?
                .execute(params![source.message_id.0, labels_text(&labels)?])
                .map_err(StoreError::sqlite("set label"))?;
            changed = true;
        }
    }
    if label == INBOX_LABEL && is_inbox(conn, source.mailbox_id)? {
        if value {
            changed |= crate::outbox::reinsert_location(conn, source)?;
        } else if crate::outbox::generation(conn, source.mailbox_id)? == Some(source.uid_validity) {
            // Only the location the operation was planned for (same generation and UID).
            changed |= conn
                .prepare_cached(DELETE_LOCATION)?
                .execute(params![
                    source.mailbox_id.0,
                    source.message_id.0,
                    source.uid
                ])
                .map_err(StoreError::sqlite("remove inbox location"))?
                > 0;
        }
    }
    Ok(changed)
}

pub(crate) fn account_gmail(conn: &Connection, account_row: i64) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached(ACCOUNT_GMAIL)?
        .query_row([account_row], |r| r.get(0))
        .optional()?
        .unwrap_or(false))
}

/// Gmail-specific planning, before the generic planner: labels need Gmail, and archive on
/// Gmail removes the inbox label. `None` leaves the action to `mail_model::plan_action`.
pub(crate) fn plan(
    conn: &Connection,
    account_row: i64,
    action: &MailAction,
    locations: &[PlanLocation],
    mailboxes: &[PlanMailbox],
) -> Result<Option<Vec<OperationPayload>>, StoreError> {
    match action {
        MailAction::AddLabel { .. } | MailAction::RemoveLabel { .. } => {
            if account_gmail(conn, account_row)? {
                Ok(None)
            } else {
                Err(StoreError::Action(ActionError::LabelsUnsupported))
            }
        }
        MailAction::Archive if account_gmail(conn, account_row)? => {
            archive(conn, locations, mailboxes).map(Some)
        }
        MailAction::Move { target } if account_gmail(conn, account_row)? => {
            out_of_all_mail(conn, locations, mailboxes, *target)
        }
        _ => Ok(None),
    }
}

/// Moving out of "All Mail" on Gmail adds the target's label and leaves the message where it
/// is: an EXPUNGE in "All Mail" (the second half of MOVE without the extension, and possibly
/// of MOVE itself) can send the message to the trash. Only when every message lies in "All
/// Mail" and the target is the inbox or a user label; otherwise the generic plan.
fn out_of_all_mail(
    conn: &Connection,
    locations: &[PlanLocation],
    mailboxes: &[PlanMailbox],
    target: MailboxId,
) -> Result<Option<Vec<OperationPayload>>, StoreError> {
    if locations.is_empty() || locations.iter().any(|l| l.role != MailboxRole::All) {
        return Ok(None);
    }
    let Some(target) = mailboxes.iter().find(|m| m.id == target && m.selectable) else {
        return Ok(None);
    };
    if !matches!(target.role, MailboxRole::Inbox | MailboxRole::Custom) {
        return Ok(None);
    }
    let Some(label) = mailbox_label(conn, target.id)?.filter(|l| mail_model::is_valid_label(l))
    else {
        return Ok(None);
    };
    Ok(Some(
        locations
            .iter()
            .map(|loc| OperationPayload::SetFlag {
                source: loc.source.clone(),
                flag: Flag::Label(label.clone()),
                value: true,
                before: false,
            })
            .collect(),
    ))
}

/// Archive on Gmail: `\Inbox` removed, from the message's inbox location (wherever the user
/// archived it from). A message not in the inbox locally is only changed if the app knows it
/// carries the label; otherwise there is nothing to archive.
fn archive(
    conn: &Connection,
    locations: &[PlanLocation],
    mailboxes: &[PlanMailbox],
) -> Result<Vec<OperationPayload>, StoreError> {
    let inbox = mailboxes
        .iter()
        .find(|m| m.role == MailboxRole::Inbox && m.selectable);
    let mut done = BTreeSet::new();
    let mut out = Vec::new();
    for loc in locations {
        let message = loc.source.message_id;
        if !done.insert(message) {
            continue;
        }
        let in_inbox = match inbox {
            Some(_) if loc.role == MailboxRole::Inbox => Some(loc.source.clone()),
            Some(inbox) => crate::outbox::location_for_plan(
                conn,
                MessageRef {
                    mailbox_id: inbox.id,
                    message_id: message,
                },
            )?
            .map(|planned| planned.location.source),
            None => None,
        };
        let source = match in_inbox {
            Some(source) => source,
            None if label_state(conn, &loc.source, INBOX_LABEL)? => loc.source.clone(),
            None => continue,
        };
        out.push(OperationPayload::SetFlag {
            source,
            flag: Flag::Label(INBOX_LABEL.to_owned()),
            value: false,
            before: true,
        });
    }
    Ok(out)
}

/// The source of the inverse of a confirmed label operation. Undoing an archive puts `\Inbox`
/// back through "All Mail": the message is no longer in the inbox on the server. `Ok(None)`
/// keeps the operation's own source.
pub(crate) fn inverse_source(
    conn: &Connection,
    source: &SourceLocation,
    flag: &Flag,
    value: bool,
) -> Result<Option<SourceLocation>, StoreError> {
    let Flag::Label(label) = flag else {
        return Ok(None);
    };
    if value || label != INBOX_LABEL || !is_inbox(conn, source.mailbox_id)? {
        return Ok(None);
    }
    let all: Option<i64> = conn
        .prepare_cached(
            "SELECT b.id FROM mailbox b WHERE b.account_id = (SELECT account_id FROM mailbox \
             WHERE id = ?1) AND b.role = 'all' AND b.selectable = 1",
        )?
        .query_row([source.mailbox_id.0], |r| r.get(0))
        .optional()?;
    let planned = match all {
        Some(all) => crate::outbox::location_for_plan(
            conn,
            MessageRef {
                mailbox_id: MailboxId(all),
                message_id: source.message_id,
            },
        )?,
        None => None,
    };
    match planned {
        Some(planned) => Ok(Some(planned.location.source)),
        None => Err(StoreError::Action(ActionError::NotUndoable(
            "the archived message is not in “All Mail” yet; try again after the next sync".into(),
        ))),
    }
}

impl Store {
    /// Stage 14: records whether the last login saw `X-GM-EXT-1`. Returns whether it changed.
    pub async fn set_account_gmail(
        &self,
        account: AccountId,
        gmail: bool,
    ) -> Result<bool, StoreError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        self.write(move |tx| {
            let row = accounts::row_id(tx, account)?;
            let changed = tx
                .prepare_cached(SET_ACCOUNT_GMAIL)?
                .execute(params![row, gmail, now])
                .map_err(StoreError::sqlite("record Gmail"))?;
            Ok(changed > 0)
        })
        .await
    }

    /// Stage 14: whether the account's server is Gmail, as the last login said.
    pub async fn account_is_gmail(&self, account: AccountId) -> Result<bool, StoreError> {
        self.read(move |conn| {
            let row = accounts::row_id(conn, account)?;
            account_gmail(conn, row)
        })
        .await
    }

    /// Stage 14: where a label change of this message is sent: its location in "All Mail" if
    /// there is one (Gmail keeps every message there), else any other. `None` if it has none.
    pub async fn label_location(
        &self,
        message: MessageId,
    ) -> Result<Option<MessageRef>, StoreError> {
        self.read(move |conn| {
            let mailbox: Option<i64> = conn
                .prepare_cached(
                    "SELECT l.mailbox_id FROM message_location l CROSS JOIN mailbox b \
                     ON b.id = l.mailbox_id WHERE l.message_id = ?1 \
                     ORDER BY b.role IS 'all' DESC, b.role IS 'inbox' DESC, l.mailbox_id LIMIT 1",
                )?
                .query_row([message.0], |r| r.get(0))
                .optional()?;
            Ok(mailbox.map(|mailbox| MessageRef {
                mailbox_id: MailboxId(mailbox),
                message_id: message,
            }))
        })
        .await
    }

    /// Stage 14: the Gmail labels of a message (see [`message_labels`]); `None` for messages of
    /// other servers.
    pub async fn message_labels(
        &self,
        message: MessageId,
    ) -> Result<Option<Vec<String>>, StoreError> {
        self.read(move |conn| message_labels(conn, message)).await
    }
}
