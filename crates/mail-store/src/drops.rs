//! Stage 22 (decision 0081): messages dropped on a folder, from any list. The rows of one drop
//! may come from several folders and accounts; this sorts them into what each needs: an
//! ordinary move within the target's account, a transfer from another account (decision 0033),
//! and those already in the target folder, which stay as they are.

use mail_model::{AccountId, MailboxId, MessageRef, TransferMode};
use rusqlite::Connection;

use crate::outbox::{location_for_plan, require_writes};
use crate::parity::target_of;
use crate::{Store, StoreError};

/// What a drop of messages on `target` does with each of them ([`Store::plan_drop`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPlan {
    pub target_account: AccountId,
    /// Messages of the target's account in other folders: an ordinary move (or, copying, a
    /// copy through the outbox like a transfer).
    pub same_account: Vec<MessageRef>,
    /// Messages of other accounts: a transfer (copy first, the source removed once the copy
    /// arrived).
    pub other_accounts: Vec<MessageRef>,
    /// Messages already in the target folder: left alone.
    pub skipped: u32,
}

/// Sorts the dropped messages (see the module docs); a message named twice counts once.
/// Refused before anything is recorded: an unknown or unselectable target, a message not
/// found, `read_only` when the target's account may not write, or (moving) an account a
/// message comes from may not; the error names that account.
pub(crate) fn plan_drop(
    conn: &Connection,
    refs: &[MessageRef],
    target: MailboxId,
    mode: TransferMode,
) -> Result<DropPlan, StoreError> {
    let (_, target_row) = target_of(conn, target)?;
    let target_account = require_writes(conn, target_row)?;
    let mut plan = DropPlan {
        target_account,
        same_account: Vec::new(),
        other_accounts: Vec::new(),
        skipped: 0,
    };
    let mut seen: Vec<MessageRef> = Vec::with_capacity(refs.len());
    let mut checked: Vec<i64> = vec![target_row];
    for at in refs {
        if seen.contains(at) {
            continue;
        }
        seen.push(*at);
        let planned = location_for_plan(conn, *at)?.ok_or(StoreError::Action(
            mail_model::ActionError::MessageNotFound(*at),
        ))?;
        if at.mailbox_id == target {
            plan.skipped += 1;
            continue;
        }
        if mode == TransferMode::Move && !checked.contains(&planned.account_row) {
            // Moving changes the account it comes from: that account must write too.
            require_writes(conn, planned.account_row)?;
            checked.push(planned.account_row);
        }
        if planned.account_row == target_row {
            plan.same_account.push(*at);
        } else {
            plan.other_accounts.push(*at);
        }
    }
    Ok(plan)
}

impl Store {
    /// See [`plan_drop`].
    pub async fn plan_drop(
        &self,
        refs: Vec<MessageRef>,
        target: MailboxId,
        mode: TransferMode,
    ) -> Result<DropPlan, StoreError> {
        self.read(move |c| plan_drop(c, &refs, target, mode)).await
    }
}
