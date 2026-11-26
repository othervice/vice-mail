//! Stage 16 (decision 0033): the reading list, unread lists, mailbox favourites and colours,
//! blocked senders, and moving or copying messages between accounts.
//!
//! - **Reading list** ("Zum Lesen"): a local mark like tasks (account UUID and Message-ID),
//!   synced (`readingList`); nothing changes on the server, also where the account may write.
//! - **Unread**: "only unread" is the inbox filter [`InboxFilter::Unread`]; "unread first"
//!   ([`AppCore::list_unread_first`]) pages the unread messages, then the read ones. Which views
//!   group unread on top is a device setting (`ui.unread_first`, a list of view keys the UI
//!   chooses).
//! - **Favourites and colours** per mailbox, synced (`mailboxPref`) by account UUID and decoded
//!   path; listed with the mailboxes.
//! - **Blocked senders** (normalised addresses, synced as `blockedSender`): new mail from them
//!   is moved to the junk folder through the outbox where the account may write and has one;
//!   otherwise it is hidden from the inbox lists. Neither runs the local rules nor notifies.
//! - **Pinned attachments** (decision 0036): a local mark by account, Message-ID and attachment
//!   index, synced (`attachmentPin`); the pin keeps name, type and size for the list.
//! - **Hidden mail of blocked senders** is counted per inbox scope for the list's note
//!   ([`AppCore::count_blocked_hidden`]).
//! - **Between accounts** ([`AppCore::transfer_messages`]): the message's bytes are frozen,
//!   appended to the target through the target account's outbox (crash check by Message-ID
//!   above the target's UIDNEXT, as every append), and for a move the source is removed through
//!   the source account's outbox only once the copy was confirmed.

use std::collections::HashSet;

use mail_content::normalize_address;
use mail_model::{
    AccountColor, AccountId, ActionResult, BlockedSender, CoreError, CoreEvent, DraftId,
    InboxScope, MAX_ACTION_MESSAGES, MAX_PINNED_ATTACHMENTS, MAX_UNREAD_FIRST_VIEWS,
    MAX_VIEW_KEY_CHARS, MailAction, Mailbox, MailboxId, MarkScope, MessageId, MessageListItem,
    MessageRef, PinnedAttachment, TransferMode, UnreadFirstCursor, UnreadFirstPage,
};
use mail_store::{ArrivedMessage, AttachmentPinInput, GLOBAL_SCOPE, InboxMailboxes, MarkView};

use crate::AppCore;
use crate::core::Connector;

/// Device setting: the list views that group unread messages on top.
pub(crate) const UNREAD_FIRST_KEY: &str = "ui.unread_first";

/// Most messages one copy or move between accounts covers (each is fetched whole).
pub const MAX_TRANSFER_MESSAGES: usize = 500;

fn group_of(view: mail_model::UnifiedView) -> Option<mail_model::AccountGroupId> {
    match view {
        mail_model::UnifiedView::All => None,
        mail_model::UnifiedView::Group { group_id } => Some(group_id),
    }
}

fn mailboxes_of(scope: InboxScope) -> InboxMailboxes {
    match scope {
        InboxScope::Mailbox { mailbox_id } => InboxMailboxes::Mailbox(mailbox_id),
        InboxScope::Unified { view } => InboxMailboxes::Unified(group_of(view)),
    }
}

fn check_count(messages: usize, max: usize) -> Result<(), CoreError> {
    if messages == 0 || messages > max {
        return Err(CoreError::InvalidArgument(format!("1 to {max} messages")));
    }
    Ok(())
}

/// The normalised form of a sender address the user typed or picked.
pub(crate) fn sender_address(address: &str) -> Result<String, CoreError> {
    normalize_address(address.trim())
        .map(|a| a.key)
        .ok_or_else(|| CoreError::InvalidArgument(format!("{address:?} is not an email address")))
}

impl<C: Connector> AppCore<C> {
    // -----------------------------------------------------------------------------------------
    // Reading list

    /// Puts messages on the reading list (`on`) or takes them off. Local only. A message
    /// without Message-ID is `invalid_argument`. Emits `inboxChanged`.
    pub async fn set_reading_list(
        &self,
        messages: Vec<MessageId>,
        on: bool,
    ) -> Result<(), CoreError> {
        check_count(messages.len(), MAX_ACTION_MESSAGES)?;
        let changed = self
            .store_ref()?
            .set_reading_list(messages, on, self.now_ms())
            .await?;
        let accounts: HashSet<AccountId> = changed.iter().map(|k| k.account).collect();
        self.marks_changed(&accounts);
        Ok(())
    }

    /// "Zum Lesen": the messages on the reading list, the most recently added first (at most
    /// 1,000), each in its inbox where it lies there.
    pub async fn list_reading_list(
        &self,
        scope: MarkScope,
    ) -> Result<Vec<MessageListItem>, CoreError> {
        self.mark_view(MarkView::ReadingList, scope).await
    }

    // -----------------------------------------------------------------------------------------
    // Pinned attachments (decision 0036)

    /// Pins attachment `index` of a message or takes the pin off. Pinning needs the attachment
    /// list of the stored body (the reading pane shows it); a message without Message-ID is
    /// `invalid_argument`. Local only; emits `inboxChanged`.
    pub async fn set_attachment_pinned(
        &self,
        message: MessageId,
        index: u32,
        pinned: bool,
    ) -> Result<(), CoreError> {
        let input = if pinned {
            let info = self.reading_ref()?.attachment_info(message, index).await?;
            Some(AttachmentPinInput {
                index,
                name: info.name,
                mime_type: info.mime_type,
                size: info.size,
            })
        } else {
            None
        };
        let (key, changed) = self
            .store_ref()?
            .set_attachment_pin(message, index, input, self.now_ms())
            .await?;
        if changed {
            tracing::info!(message = message.0, index, pinned, "attachment pin changed");
            self.marks_changed(&HashSet::from([key.account]));
        }
        Ok(())
    }

    /// The pinned attachment indexes of a message, for the reading pane.
    pub async fn get_attachment_pins(&self, message: MessageId) -> Result<Vec<u32>, CoreError> {
        Ok(self.store_ref()?.attachment_pins_of(message).await?)
    }

    /// "Angeheftete Anhänge": the pinned attachments of the scope, the newest pin first (at
    /// most 1,000), each with its message as a list row. Pins of messages not stored on this
    /// device are left out.
    pub async fn list_pinned_attachments(
        &self,
        scope: MarkScope,
    ) -> Result<Vec<PinnedAttachment>, CoreError> {
        let (accounts, prefer) = self.mark_scope(scope).await?;
        let rows = self
            .store_ref()?
            .pinned_attachments(accounts, prefer, MAX_PINNED_ATTACHMENTS)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| PinnedAttachment {
                preview: mail_content::preview_kind(&row.name, &row.mime_type),
                message: row.message,
                index: row.index,
                name: row.name,
                mime_type: row.mime_type,
                size: row.size,
                pinned_at: row.pinned_at,
            })
            .collect())
    }

    // -----------------------------------------------------------------------------------------
    // Hidden mail of blocked senders

    /// How many messages of blocked senders the inbox lists of `scope` leave out (read-only
    /// accounts, or no junk folder), for the list's quiet note.
    pub async fn count_blocked_hidden(&self, scope: InboxScope) -> Result<u32, CoreError> {
        Ok(self
            .store_ref()?
            .count_blocked_hidden(mailboxes_of(scope))
            .await?)
    }

    fn marks_changed(&self, accounts: &HashSet<AccountId>) {
        match accounts.len() {
            0 => {}
            1 => self.emit_event(CoreEvent::InboxChanged {
                account_id: accounts.iter().next().copied(),
            }),
            _ => self.emit_event(CoreEvent::InboxChanged { account_id: None }),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Unread

    /// "Ungelesene oben": a page of the scope's unread messages, then its read ones (waiting
    /// snoozes and hidden mail of blocked senders left out). `limit` 1..=500.
    pub async fn list_unread_first(
        &self,
        scope: InboxScope,
        cursor: Option<UnreadFirstCursor>,
        limit: u32,
    ) -> Result<UnreadFirstPage, CoreError> {
        Ok(self
            .store_ref()?
            .unread_first_list(mailboxes_of(scope), cursor, limit)
            .await?)
    }

    /// Decision 0039: "unread first" as conversations: those with an unread message in the
    /// scope (at their newest unread message), then the others. `limit` counts conversations.
    pub async fn list_unread_first_conversations(
        &self,
        scope: InboxScope,
        cursor: Option<UnreadFirstCursor>,
        limit: u32,
    ) -> Result<mail_model::UnreadFirstThreadPage, CoreError> {
        Ok(self
            .store_ref()?
            .unread_first_conversations(mailboxes_of(scope), cursor, limit)
            .await?)
    }

    /// The list views that group unread messages on top (keys the UI chooses, for example
    /// `mailbox:<id>` or `unified:all`). Device setting.
    pub async fn get_unread_first_views(&self) -> Result<Vec<String>, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(settings
            .iter()
            .find(|s| s.key == UNREAD_FIRST_KEY)
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s.value).ok())
            .unwrap_or_default())
    }

    /// Groups unread messages on top in `view` or not; returns all such views.
    pub async fn set_unread_first_view(
        &self,
        view: String,
        enabled: bool,
    ) -> Result<Vec<String>, CoreError> {
        let view = view.trim().to_owned();
        if view.is_empty()
            || view.chars().count() > MAX_VIEW_KEY_CHARS
            || view.chars().any(char::is_control)
        {
            return Err(CoreError::InvalidArgument(format!(
                "a view key is 1 to {MAX_VIEW_KEY_CHARS} characters without controls"
            )));
        }
        let mut views = self.get_unread_first_views().await?;
        views.retain(|v| *v != view);
        if enabled {
            if views.len() >= MAX_UNREAD_FIRST_VIEWS {
                return Err(CoreError::InvalidArgument(format!(
                    "at most {MAX_UNREAD_FIRST_VIEWS} views group unread messages on top"
                )));
            }
            views.push(view);
        }
        let json = serde_json::to_string(&views)
            .map_err(|err| CoreError::Internal(format!("unread first views: {err}")))?;
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(UNREAD_FIRST_KEY.to_owned(), json)])
            .await?;
        Ok(views)
    }

    // -----------------------------------------------------------------------------------------
    // Favourites and colours

    /// Makes a folder a favourite (at the end of the favourites) or not. Emits
    /// `mailboxesChanged` for its account.
    pub async fn set_mailbox_favorite(
        &self,
        mailbox: MailboxId,
        favorite: bool,
    ) -> Result<(), CoreError> {
        let (account, changed) = self
            .store_ref()?
            .set_mailbox_favorite(mailbox, favorite, self.now_ms())
            .await?;
        if changed {
            self.emit_event(CoreEvent::MailboxesChanged {
                account_id: account,
            });
        }
        Ok(())
    }

    /// Gives a folder a colour of the account palette (0–7) or none.
    pub async fn set_mailbox_color(
        &self,
        mailbox: MailboxId,
        color: Option<AccountColor>,
    ) -> Result<(), CoreError> {
        if let Some(color) = color
            && AccountColor::new(color.index()).is_none()
        {
            return Err(CoreError::InvalidArgument(format!(
                "colour {} is not in the palette",
                color.index()
            )));
        }
        let (account, changed) = self
            .store_ref()?
            .set_mailbox_color(mailbox, color, self.now_ms())
            .await?;
        if changed {
            self.emit_event(CoreEvent::MailboxesChanged {
                account_id: account,
            });
        }
        Ok(())
    }

    /// Puts the favourites in this order (the others after them); returns them.
    pub async fn reorder_favorite_mailboxes(
        &self,
        order: Vec<MailboxId>,
    ) -> Result<Vec<Mailbox>, CoreError> {
        if order.len() > mail_model::MAX_FAVORITE_MAILBOXES {
            return Err(CoreError::InvalidArgument("too many folders".into()));
        }
        let store = self.store_ref()?;
        let changed = store
            .reorder_favorite_mailboxes(order, self.now_ms())
            .await?;
        for account in changed {
            self.emit_event(CoreEvent::MailboxesChanged {
                account_id: account,
            });
        }
        Ok(store.favorite_mailboxes().await?)
    }

    /// The favourite folders of every account, in their order (sidebar top, palette).
    pub async fn list_favorite_mailboxes(&self) -> Result<Vec<Mailbox>, CoreError> {
        Ok(self.store_ref()?.favorite_mailboxes().await?)
    }

    /// Favourites or colours came from another device.
    pub(crate) async fn mailbox_prefs_changed(&self) {
        let Ok(store) = self.store_ref() else {
            return;
        };
        if let Ok(accounts) = store.accounts().await {
            for account in accounts {
                self.emit_event(CoreEvent::MailboxesChanged {
                    account_id: account.id,
                });
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Blocked senders

    /// Blocks a sender: new mail from the address goes to the junk folder (or is hidden on a
    /// read-only account). Returns all blocked senders. `invalid_argument` for something that
    /// is not an address.
    pub async fn block_sender(&self, address: String) -> Result<Vec<BlockedSender>, CoreError> {
        let address = sender_address(&address)?;
        let store = self.store_ref()?;
        if store.block_sender(address, self.now_ms()).await? {
            self.emit_event(CoreEvent::InboxChanged { account_id: None });
        }
        Ok(store.blocked_senders().await?)
    }

    /// Unblocks a sender; mail hidden for it shows again. Returns all blocked senders.
    pub async fn unblock_sender(&self, address: String) -> Result<Vec<BlockedSender>, CoreError> {
        let address = sender_address(&address)?;
        let store = self.store_ref()?;
        if store.unblock_sender(address).await? {
            self.emit_event(CoreEvent::InboxChanged { account_id: None });
        }
        Ok(store.blocked_senders().await?)
    }

    pub async fn list_blocked_senders(&self) -> Result<Vec<BlockedSender>, CoreError> {
        Ok(self.store_ref()?.blocked_senders().await?)
    }

    /// New mail in an inbox: messages of blocked senders go to junk where the account may
    /// write (and has a junk folder), else they are hidden. Returns the ids handled here; the
    /// caller runs neither rules nor notifications for them.
    pub(crate) async fn handle_blocked_arrivals(
        &self,
        account: &mail_store::StoredAccount,
        messages: &[ArrivedMessage],
    ) -> Result<HashSet<MessageId>, CoreError> {
        let store = self.store_ref()?;
        let mut handled = HashSet::new();
        for message in messages {
            let Some(sender) = message
                .from
                .as_ref()
                .and_then(|from| normalize_address(&from.email))
                .map(|a| a.key)
            else {
                continue;
            };
            if !store.is_sender_blocked(sender.clone()).await? {
                continue;
            }
            handled.insert(message.id);
            let mut moved = false;
            if account.writes_enabled {
                let target = MessageRef {
                    mailbox_id: message.mailbox_id,
                    message_id: message.id,
                };
                let request = format!("blocked-{}-{}", message.mailbox_id, message.id);
                match self
                    .apply_action_quietly(MailAction::Spam, vec![target], request)
                    .await
                {
                    Ok(_) => moved = true,
                    Err(err) => {
                        tracing::info!(code = ?err.code(), "blocked sender's mail not moved; hidden");
                    }
                }
            }
            if !moved {
                match store.hide_blocked_message(message.id, sender).await {
                    Ok(_) | Err(mail_store::StoreError::InvalidArgument(_)) => {}
                    Err(err) => return Err(err.into()),
                }
            }
        }
        if !handled.is_empty() {
            tracing::info!(count = handled.len(), "mail of blocked senders handled");
            self.emit_event(CoreEvent::InboxChanged {
                account_id: Some(account.id),
            });
        }
        Ok(handled)
    }

    // -----------------------------------------------------------------------------------------
    // Between accounts

    /// Copies or moves messages to a folder of another account (decision 0033), or copies them
    /// to another folder of their own account (stage 21, decision 0064). The bytes of each
    /// message are frozen first (the stored body if complete, else fetched whole), then appended
    /// to the target through its outbox; a move removes the source only after the copy was
    /// confirmed in the target by its Message-ID. Refused before anything is recorded:
    /// `read_only` (the target account must write, and for a move the source account too),
    /// `invalid_argument` (a move within the target's own account, a copy into the message's own
    /// folder, no Message-ID, too many), `not_found`. `request_id` as in `apply_action`.
    pub async fn transfer_messages(
        &self,
        messages: Vec<MessageRef>,
        target: MailboxId,
        mode: TransferMode,
        request_id: String,
    ) -> Result<ActionResult, CoreError> {
        check_count(messages.len(), MAX_TRANSFER_MESSAGES)?;
        if request_id.is_empty()
            || request_id.len() > 64
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(CoreError::InvalidArgument(
                "the request id must be 1 to 64 letters, digits, '-' or '_'".into(),
            ));
        }
        let store = self.store_ref()?;
        // Refuse early, before anything is fetched.
        let plan = store.check_transfer(messages.clone(), target, mode).await?;
        let reading = self.reading_ref()?;
        let outbox = self.outbox_ref()?;
        let root = crate::compose::files::compose_dir(store);
        let lock = outbox.files_lock();
        let _held = lock.lock().await;
        let mut files = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let raw = reading.raw_message(item.at.message_id).await?;
            let dir = DraftId::from_random(rand_bits());
            let relative = crate::compose::files::outgoing_path(&dir, "transfer.eml");
            let path = crate::compose::files::resolve(&root, &relative)?;
            tokio::task::spawn_blocking(move || {
                crate::compose::files::write_atomically(&path, &raw)
            })
            .await
            .map_err(|err| CoreError::Internal(format!("writing failed: {err}")))??;
            files.push((item.at, relative));
        }
        let change = store
            .enqueue_transfer(target, mode, files, request_id, self.now_ms())
            .await?;
        tracing::info!(account = %change.account, ?mode, queued = change.operations.len(), "transfer recorded");
        Ok(outbox.recorded_result(&change))
    }
}

fn rand_bits() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}
