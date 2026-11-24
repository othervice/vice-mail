//! Stage 18 (decision 0051): the app's popup for new mail. The core keeps the cards; the shell
//! shows them in a window of its own (the [`super::NotificationSink`] receives every change as
//! a [`PopupUpdate`]) and forwards the popup's buttons to the methods here.
//!
//! - **Which messages**: the same decision as for system notifications
//!   ([`super::notify::announced`]: first sync never, muted accounts, quiet hours, categories,
//!   local rules). Each announced message is one card; newest on top, at most
//!   [`MAX_POPUP_CARDS`] visible and [`MAX_POPUP_QUEUE`] kept.
//! - **Hidden content**: the card keeps neither sender nor subject nor snippet; it only knows
//!   which message and account it is about.
//! - **Snippet**: the list preview. A message that just arrived usually has none yet; once the
//!   prefetch stored its body (`messagesChanged` of the inbox) the card gets it.
//! - **Read**: the card goes and the main window shows the message (`openMessageRequested`, as
//!   for a click on a system notification).
//! - **Delete**: the list's action (`MailAction::Delete` through the outbox into the trash);
//!   the card stays as "Deleted" with Undo until it is dismissed. Read-only accounts are refused
//!   by the outbox as everywhere (`read_only`).
//! - **"N more"**: everything goes and the main window shows the inbox (`openInboxRequested`).
//!
//! Every change publishes the whole popup (cards, "N more", the app's appearance) with a growing
//! revision; an empty popup tells the shell to close its window.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use mail_model::{
    AccountId, Address, BatchId, CoreError, CoreEvent, MAX_POPUP_CARDS, MAX_POPUP_QUEUE,
    MailAction, MailboxId, MessageId, MessageRef, NewMailAccount, NewMailCard, NewMailContent,
    NewMailPopup, NotificationSettings, PopupAppearance,
};
use mail_store::ArrivedMessage;

use super::lock;
use super::notify::{self, PopupUpdate};
use crate::core::{AppCore, Connector};

/// One message waiting in the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Queued {
    pub account: AccountId,
    pub mailbox: MailboxId,
    pub message: MessageId,
    /// Sort date, Unix milliseconds.
    pub date: i64,
    /// The settings hid the content when the message arrived: no sender, subject or snippet
    /// are kept.
    pub hidden: bool,
    pub from: Option<Address>,
    pub subject: String,
    pub snippet: Option<String>,
    /// Deleted from the popup: the batch that Undo reverts.
    pub deleted: Option<BatchId>,
}

/// The cards, newest first, and the revision of their last change.
#[derive(Debug, Default)]
pub(crate) struct PopupQueue {
    cards: Vec<Queued>,
    revision: u64,
}

impl PopupQueue {
    /// New messages go on top, newest first among themselves; a message that is already there
    /// moves up. Beyond [`MAX_POPUP_QUEUE`] the oldest cards are dropped.
    pub(crate) fn add(&mut self, mut arrived: Vec<Queued>) {
        if arrived.is_empty() {
            return;
        }
        arrived.sort_by_key(|c| std::cmp::Reverse((c.date, c.message)));
        arrived.dedup_by_key(|c| c.message);
        self.cards
            .retain(|c| !arrived.iter().any(|a| a.message == c.message));
        arrived.append(&mut self.cards);
        arrived.truncate(MAX_POPUP_QUEUE);
        self.cards = arrived;
        self.revision += 1;
    }

    pub(crate) fn find(&self, message: MessageId) -> Option<&Queued> {
        self.cards.iter().find(|c| c.message == message)
    }

    /// Changes one card; `false` if it is not there.
    pub(crate) fn update(&mut self, message: MessageId, change: impl FnOnce(&mut Queued)) -> bool {
        let Some(card) = self.cards.iter_mut().find(|c| c.message == message) else {
            return false;
        };
        change(card);
        self.revision += 1;
        true
    }

    pub(crate) fn remove(&mut self, message: MessageId) -> Option<Queued> {
        let index = self.cards.iter().position(|c| c.message == message)?;
        self.revision += 1;
        Some(self.cards.remove(index))
    }

    /// Removes the cards `keep` refuses; `true` if any went.
    pub(crate) fn retain(&mut self, keep: impl FnMut(&Queued) -> bool) -> bool {
        let before = self.cards.len();
        self.cards.retain(keep);
        let changed = self.cards.len() != before;
        if changed {
            self.revision += 1;
        }
        changed
    }

    /// Removes every card and returns them, newest first.
    pub(crate) fn clear(&mut self) -> Vec<Queued> {
        if !self.cards.is_empty() {
            self.revision += 1;
        }
        std::mem::take(&mut self.cards)
    }

    pub(crate) fn cards(&self) -> &[Queued] {
        &self.cards
    }

    /// The cards shown (at most [`MAX_POPUP_CARDS`]), how many wait behind them, and the
    /// revision.
    pub(crate) fn visible(&self) -> (Vec<Queued>, u32, u64) {
        let shown: Vec<Queued> = self.cards.iter().take(MAX_POPUP_CARDS).cloned().collect();
        let more = u32::try_from(self.cards.len() - shown.len()).unwrap_or(u32::MAX);
        (shown, more, self.revision)
    }
}

fn not_in_popup(message: MessageId) -> CoreError {
    CoreError::NotFound(format!(
        "message {} is not in the new-mail popup",
        message.0
    ))
}

/// A text, trimmed, or `None` when nothing is left.
fn non_empty(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

impl<C: Connector> AppCore<C> {
    /// New messages of one sync window become cards (see the module docs). `announced` as
    /// [`notify::announced`] chose them; the sound is that of the newest one's account.
    pub(crate) async fn popup_new_mail(
        &self,
        settings: &NotificationSettings,
        announced: &[&ArrivedMessage],
    ) {
        let Some(newest) = announced.iter().max_by_key(|m| m.id) else {
            return;
        };
        let sound = notify::sound(settings, newest.account_id);
        let mut picked: Vec<&ArrivedMessage> = announced.to_vec();
        picked.sort_by_key(|m| std::cmp::Reverse(m.id));
        picked.truncate(MAX_POPUP_QUEUE);
        // Date and preview are not part of the arrival; the list rows have them.
        let items: HashMap<MessageId, (i64, Option<String>)> = match self.store_ref() {
            Ok(store) => store
                .message_items(picked.iter().map(|m| m.id).collect())
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(error = %err, "new-mail popup: list rows unavailable");
                    Vec::new()
                })
                .into_iter()
                .map(|item| (item.id, (item.date, item.preview)))
                .collect(),
            Err(_) => HashMap::new(),
        };
        let now = self.now_ms();
        let hidden = settings.hide_content;
        let cards = picked
            .into_iter()
            .map(|m| {
                let (date, preview) = items.get(&m.id).cloned().unwrap_or((now, None));
                Queued {
                    account: m.account_id,
                    mailbox: m.mailbox_id,
                    message: m.id,
                    date,
                    hidden,
                    from: if hidden { None } else { m.from.clone() },
                    subject: if hidden {
                        String::new()
                    } else {
                        m.subject.clone()
                    },
                    snippet: if hidden {
                        None
                    } else {
                        non_empty(preview.as_deref())
                    },
                    deleted: None,
                }
            })
            .collect();
        lock(&self.comfort().popup).add(cards);
        self.publish_popup(sound).await;
    }

    /// The popup as it is now (the popup window asks when it opens).
    pub async fn new_mail_popup(&self) -> NewMailPopup {
        self.popup_view().await
    }

    /// "Read": the card goes and the main window shows the message in its inbox
    /// (`openMessageRequested`). `not_found` if the card is gone; `invalid_argument` for a
    /// card deleted from the popup (its message is in the trash).
    pub async fn read_from_popup(&self, message: MessageId) -> Result<NewMailPopup, CoreError> {
        let card = {
            let mut popup = lock(&self.comfort().popup);
            match popup.find(message) {
                None => return Err(not_in_popup(message)),
                Some(card) if card.deleted.is_some() => {
                    return Err(CoreError::InvalidArgument(
                        "the message was deleted from the popup; undo first".into(),
                    ));
                }
                Some(_) => popup.remove(message).ok_or_else(|| not_in_popup(message))?,
            }
        };
        self.notification_clicked(card.account, Some(card.mailbox), card.message);
        Ok(self.publish_popup(None).await)
    }

    /// "Delete": the list's delete (through the outbox into the trash, `read_only` for a
    /// read-only account, `missing_mailbox` without a trash folder); the card shows "Deleted"
    /// with Undo. Deleting a card twice changes nothing.
    pub async fn delete_from_popup(&self, message: MessageId) -> Result<NewMailPopup, CoreError> {
        let card = lock(&self.comfort().popup)
            .find(message)
            .cloned()
            .ok_or_else(|| not_in_popup(message))?;
        if card.deleted.is_some() {
            return Ok(self.popup_view().await);
        }
        let n = self
            .comfort()
            .popup_requests
            .fetch_add(1, Ordering::Relaxed);
        // Unique across restarts too: the outbox ignores a request id it has seen.
        let request = format!("popup-{}-{}-{n}", message.0, self.now_ms());
        let result = self
            .apply_action(
                MailAction::Delete,
                vec![MessageRef {
                    mailbox_id: card.mailbox,
                    message_id: card.message,
                }],
                request,
            )
            .await?;
        let Some(batch) = result.batch else {
            // Nothing was recorded (the message was already on its way to the trash).
            return Ok(self.popup_view().await);
        };
        lock(&self.comfort().popup).update(message, |c| c.deleted = Some(batch));
        Ok(self.publish_popup(None).await)
    }

    /// "Undo" on a deleted card: the outbox reverts the batch, the card is a new message again.
    pub async fn undo_popup_delete(&self, message: MessageId) -> Result<NewMailPopup, CoreError> {
        let card = lock(&self.comfort().popup)
            .find(message)
            .cloned()
            .ok_or_else(|| not_in_popup(message))?;
        let Some(batch) = card.deleted else {
            return Ok(self.popup_view().await);
        };
        self.undo_action(batch).await?;
        lock(&self.comfort().popup).update(message, |c| c.deleted = None);
        Ok(self.publish_popup(None).await)
    }

    /// Closes one card (its close button, or the end of "Deleted") or, with `None`, all of
    /// them (the popup hid itself). A card that is already gone is no error.
    pub async fn dismiss_popup(&self, message: Option<MessageId>) -> NewMailPopup {
        let changed = {
            let mut popup = lock(&self.comfort().popup);
            match message {
                Some(message) => popup.remove(message).is_some(),
                None => !popup.clear().is_empty(),
            }
        };
        if changed {
            self.publish_popup(None).await
        } else {
            self.popup_view().await
        }
    }

    /// "N more": every card goes and the main window shows the inbox of their account, or the
    /// unified inbox when they came from several (`openInboxRequested`).
    pub async fn open_inbox_from_popup(&self) -> NewMailPopup {
        let cards = lock(&self.comfort().popup).clear();
        let first = cards.first().map(|c| c.account);
        let account = first.filter(|a| cards.iter().all(|c| c.account == *a));
        self.emit_event(CoreEvent::OpenInboxRequested {
            account_id: account,
        });
        self.publish_popup(None).await
    }

    /// `messagesChanged` of `mailbox`: cards of it get their snippet once the body is stored,
    /// and cards whose message is gone (deleted elsewhere) go.
    pub(crate) async fn refresh_popup(&self, mailbox: MailboxId) {
        let wanted: Vec<MessageId> = lock(&self.comfort().popup)
            .cards()
            .iter()
            .filter(|c| c.mailbox == mailbox && c.deleted.is_none())
            .map(|c| c.message)
            .collect();
        if wanted.is_empty() {
            return;
        }
        let Ok(store) = self.store_ref() else { return };
        let items = match store.message_items(wanted.clone()).await {
            Ok(items) => items,
            Err(err) => {
                tracing::debug!(error = %err, "new-mail popup: rows not refreshed");
                return;
            }
        };
        let changed = {
            let mut popup = lock(&self.comfort().popup);
            let mut changed = false;
            for id in wanted {
                match items.iter().find(|item| item.id == id) {
                    None => changed |= popup.remove(id).is_some(),
                    Some(item) => {
                        let snippet = non_empty(item.preview.as_deref());
                        let needs = popup
                            .find(id)
                            .is_some_and(|c| !c.hidden && c.snippet.is_none());
                        if needs && snippet.is_some() {
                            changed |= popup.update(id, |c| c.snippet = snippet);
                        }
                    }
                }
            }
            changed
        };
        if changed {
            self.publish_popup(None).await;
        }
    }

    /// `accountsChanged`: cards of removed accounts go.
    pub(crate) async fn prune_popup_accounts(&self) {
        if lock(&self.comfort().popup).cards().is_empty() {
            return;
        }
        let Ok(store) = self.store_ref() else { return };
        let Ok(accounts) = store.accounts().await else {
            return;
        };
        let changed =
            lock(&self.comfort().popup).retain(|c| accounts.iter().any(|a| a.id == c.account));
        if changed {
            self.publish_popup(None).await;
        }
    }

    /// Hands the popup as it is now to the sink (one at a time, so the shell sees the changes
    /// in order) and returns it. Without a sink nothing is shown; the cards stay.
    async fn publish_popup(&self, sound: Option<String>) -> NewMailPopup {
        let _turn = self.comfort().popup_publish.lock().await;
        let popup = self.popup_view().await;
        let sink = lock(&self.comfort().sink).clone();
        if let Some(sink) = sink {
            sink.popup(PopupUpdate {
                popup: popup.clone(),
                sound,
            });
        }
        popup
    }

    /// The visible cards with what the UI shows: the account where there are several, whether
    /// it may write, the sender's picture; and the app's theme, language and accent.
    async fn popup_view(&self) -> NewMailPopup {
        let (cards, more, revision) = lock(&self.comfort().popup).visible();
        let accounts = match self.store_ref() {
            Ok(store) if !cards.is_empty() => store.accounts().await.unwrap_or_else(|err| {
                tracing::warn!(error = %err, "new-mail popup: accounts unavailable");
                Vec::new()
            }),
            _ => Vec::new(),
        };
        let several = accounts.len() > 1;
        let mut shown = Vec::with_capacity(cards.len());
        for card in cards {
            let account = accounts.iter().find(|a| a.id == card.account);
            let content = if card.hidden {
                None
            } else {
                let email = card
                    .from
                    .as_ref()
                    .and_then(|a| non_empty(Some(a.email.as_str())));
                let picture = match &email {
                    Some(email) => self
                        .get_contact_picture(email.clone())
                        .await
                        .ok()
                        .flatten()
                        .map(|p| p.data_url),
                    None => None,
                };
                Some(NewMailContent {
                    sender_name: card
                        .from
                        .as_ref()
                        .and_then(|a| non_empty(a.name.as_deref())),
                    sender_email: email,
                    subject: card.subject.clone(),
                    snippet: card.snippet.clone(),
                    picture,
                })
            };
            shown.push(NewMailCard {
                account_id: card.account,
                mailbox_id: card.mailbox,
                message_id: card.message,
                date: card.date,
                content,
                account: account.filter(|_| several).map(|a| NewMailAccount {
                    name: non_empty(Some(a.display_name.as_str()))
                        .unwrap_or_else(|| a.email.clone()),
                    color: a.color,
                }),
                writes_enabled: account.is_some_and(|a| a.writes_enabled),
                deleted: card.deleted.is_some(),
            });
        }
        let preferences = self.get_preferences().await.unwrap_or_default();
        let accent = self
            .get_ui_preferences()
            .await
            .map(|p| p.accent_theme)
            .unwrap_or_default();
        NewMailPopup {
            cards: shown,
            more,
            appearance: PopupAppearance {
                theme: preferences.theme,
                language: preferences.language,
                accent,
            },
            revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(n: u8) -> AccountId {
        format!("00000000-0000-4000-8000-0000000000{n:02}")
            .parse()
            .expect("uuid")
    }

    fn queued(id: i64, date: i64) -> Queued {
        Queued {
            account: account(1),
            mailbox: MailboxId(1),
            message: MessageId(id),
            date,
            hidden: false,
            from: None,
            subject: format!("Subject {id}"),
            snippet: None,
            deleted: None,
        }
    }

    fn ids(queue: &PopupQueue) -> Vec<i64> {
        queue.cards().iter().map(|c| c.message.0).collect()
    }

    #[test]
    fn new_cards_go_on_top_newest_first() {
        let mut queue = PopupQueue::default();
        queue.add(vec![queued(1, 100), queued(3, 300), queued(2, 200)]);
        assert_eq!(ids(&queue), [3, 2, 1]);
        // A later sync window goes on top even with an older date (it arrived later).
        queue.add(vec![queued(4, 50)]);
        assert_eq!(ids(&queue), [4, 3, 2, 1]);
        let (shown, more, revision) = queue.visible();
        assert_eq!(
            shown.iter().map(|c| c.message.0).collect::<Vec<_>>(),
            [4, 3, 2]
        );
        assert_eq!((more, revision), (1, 2));
        // The same message again moves up instead of showing twice.
        queue.add(vec![queued(2, 200), queued(2, 200)]);
        assert_eq!(ids(&queue), [2, 4, 3, 1]);
        queue.add(Vec::new());
        assert_eq!(queue.visible().2, 3, "nothing added, no new revision");
    }

    #[test]
    fn the_queue_keeps_the_newest() {
        let mut queue = PopupQueue::default();
        let many: Vec<Queued> = (1..=60).map(|i| queued(i, i * 10)).collect();
        queue.add(many);
        assert_eq!(queue.cards().len(), MAX_POPUP_QUEUE);
        assert_eq!(queue.cards().first().map(|c| c.message.0), Some(60));
        assert_eq!(queue.cards().last().map(|c| c.message.0), Some(11));
        let (shown, more, _) = queue.visible();
        assert_eq!(shown.len(), MAX_POPUP_CARDS);
        assert_eq!(more as usize, MAX_POPUP_QUEUE - MAX_POPUP_CARDS);
    }

    #[test]
    fn remove_update_retain_and_clear_count_revisions() {
        let mut queue = PopupQueue::default();
        queue.add(vec![queued(1, 1), queued(2, 2)]);
        let start = queue.visible().2;
        assert!(queue.remove(MessageId(9)).is_none());
        assert_eq!(queue.visible().2, start, "nothing removed");
        assert!(!queue.update(MessageId(9), |c| c.deleted = Some(BatchId(1))));
        assert!(queue.update(MessageId(1), |c| c.deleted = Some(BatchId(5))));
        assert_eq!(
            queue.find(MessageId(1)).and_then(|c| c.deleted),
            Some(BatchId(5))
        );
        assert!(!queue.retain(|_| true));
        assert!(queue.retain(|c| c.message != MessageId(2)));
        assert_eq!(ids(&queue), [1]);
        assert_eq!(
            queue.remove(MessageId(1)).map(|c| c.message),
            Some(MessageId(1))
        );
        assert!(queue.clear().is_empty());
        assert_eq!(queue.visible(), (Vec::new(), 0, start + 3));
    }
}
