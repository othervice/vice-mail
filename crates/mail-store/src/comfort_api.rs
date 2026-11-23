//! Public API of the store for stage 9 (decision 0020): categories and sender rules, the inbox
//! lists, snoozes, pins, tasks, local rules, unread counts for the badge, and the messages a
//! sync window brought. Every method runs on the writer thread or a reader, like the rest.

use mail_model::{
    AccountGroupId, AccountId, ConversationReach, ListCursor, MailRule, MailboxId, MessageCategory,
    MessageId, MessageListItem, MessagePage, MessageRef, RuleId, RuleInput, SenderKey,
    ThreadListItem, ThreadPage, UidSet, UnreadFirstCursor, UnreadFirstThreadPage,
};

use crate::categories::{self, CATEGORY_BATCH, SenderRuleChange, StoredCategory};
use crate::inbox::{self, ArrivedMessage, InboxListKind};
use crate::marks::{self, MarkView, MessageKey, ReturnedSnooze, StoredSnooze, StoredTask};
use crate::{Store, StoreError, conversations, groups, lists, rules};

/// Which mailboxes an inbox list covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxMailboxes {
    /// One mailbox.
    Mailbox(MailboxId),
    /// The inboxes of the visible accounts, or of a group's visible members.
    Unified(Option<AccountGroupId>),
}

/// The result of one background step of the category jobs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CategoryStep {
    /// Messages classified for the first time.
    pub classified: usize,
    /// Messages whose category or important mark changed with the rules.
    pub changed: usize,
    /// Nothing is left to do.
    pub done: bool,
}

pub(crate) fn resolve(
    conn: &rusqlite::Connection,
    scope: &InboxMailboxes,
) -> Result<Vec<(MailboxId, AccountId)>, StoreError> {
    match scope {
        InboxMailboxes::Mailbox(mailbox) => {
            Ok(vec![(*mailbox, lists::mailbox_account(conn, *mailbox)?)])
        }
        InboxMailboxes::Unified(group) => groups::unified_inboxes(conn, group.as_ref()),
    }
}

impl Store {
    // ---------------------------------------------------------------------------------------
    // Categories

    /// One step of the category jobs: first the messages stored without a category (needs the
    /// classifier, [`Store::set_classifier`]), then a pending pass of the sender rules. At most
    /// [`CATEGORY_BATCH`] messages per step, newest first.
    pub async fn category_step(&self) -> Result<CategoryStep, StoreError> {
        let classifier = self.classifier();
        self.write(move |tx| {
            if let Some(classifier) = &classifier {
                let classified = categories::backfill_step(tx, classifier, CATEGORY_BATCH)?;
                if classified > 0 {
                    return Ok(CategoryStep {
                        classified,
                        changed: 0,
                        done: false,
                    });
                }
            }
            let (changed, done) = categories::rules_step(tx, CATEGORY_BATCH)?;
            Ok(CategoryStep {
                classified: 0,
                changed,
                done,
            })
        })
        .await
    }

    /// Whether the category jobs have work (unclassified messages, a rules pass).
    pub async fn category_work_pending(&self) -> Result<bool, StoreError> {
        self.read(categories::work_pending).await
    }

    /// The user's sender rules, by key.
    pub async fn sender_rules(&self) -> Result<Vec<mail_model::SenderRule>, StoreError> {
        self.read(categories::sender_rules).await
    }

    /// Changes the rule of a normalised key (creates it, or deletes it when it neither corrects
    /// nor marks important) and schedules applying the rules to all messages again. Returns
    /// the rule; `None` when there is none now.
    pub async fn put_sender_rule(
        &self,
        key: SenderKey,
        change: SenderRuleChange,
        now: i64,
    ) -> Result<Option<mail_model::SenderRule>, StoreError> {
        self.write(move |tx| {
            let uuid = groups::new_uuid(tx)?;
            categories::put_sender_rule(tx, &key, change, &uuid, now)
        })
        .await
    }

    /// Deletes a sender rule by id; whether there was one.
    pub async fn delete_sender_rule(&self, id: String) -> Result<bool, StoreError> {
        self.write(move |tx| categories::delete_sender_rule(tx, &id))
            .await
    }

    /// The stored category data of a message (`None` if there is no such message).
    pub async fn message_category(
        &self,
        message: MessageId,
    ) -> Result<Option<StoredCategory>, StoreError> {
        self.read(move |c| categories::message_category(c, message.0))
            .await
    }

    /// A local rule's "set category" for one message; whether the message exists.
    pub async fn set_local_category(
        &self,
        message: MessageId,
        category: MessageCategory,
        rule_name: String,
    ) -> Result<bool, StoreError> {
        self.write(move |tx| categories::set_local_category(tx, message.0, category, &rule_name))
            .await
    }

    /// The addresses of all accounts and identities.
    pub async fn own_addresses(&self) -> Result<Vec<String>, StoreError> {
        self.read(categories::own_addresses).await
    }

    // ---------------------------------------------------------------------------------------
    // Inbox lists

    /// One page of an inbox list (see `inbox`): the rows without snoozes and pins, one
    /// category, or the important senders.
    pub async fn inbox_list(
        &self,
        scope: InboxMailboxes,
        kind: InboxListKind,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, StoreError> {
        self.read(move |c| {
            let mailboxes = resolve(c, &scope)?;
            inbox::list(c, kind, &mailboxes, cursor, limit)
        })
        .await
    }

    /// Decision 0039: one page of the conversation variant of an inbox list (see
    /// `conversations`); `limit` counts conversations.
    pub async fn inbox_conversations(
        &self,
        scope: InboxMailboxes,
        kind: InboxListKind,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, StoreError> {
        self.read(move |c| {
            let mailboxes = resolve(c, &scope)?;
            conversations::list(c, kind, &mailboxes, cursor, limit)
        })
        .await
    }

    /// Decision 0039: pinned conversations (`returned = false`) or those with a returned
    /// snooze, once per thread, as the row of their newest message in the scope.
    pub async fn inbox_top_conversations(
        &self,
        scope: InboxMailboxes,
        returned: bool,
        limit: u32,
    ) -> Result<Vec<ThreadListItem>, StoreError> {
        self.read(move |c| {
            let mailboxes = resolve(c, &scope)?;
            conversations::top(c, returned, &mailboxes, limit)
        })
        .await
    }

    /// Decision 0039: "unread first" as conversations.
    pub async fn unread_first_conversations(
        &self,
        scope: InboxMailboxes,
        cursor: Option<UnreadFirstCursor>,
        limit: u32,
    ) -> Result<UnreadFirstThreadPage, StoreError> {
        self.read(move |c| {
            let mailboxes = resolve(c, &scope)?;
            conversations::unread_first(c, &mailboxes, cursor, limit)
        })
        .await
    }

    /// Decision 0039: the messages an action on conversation rows touches (see
    /// `conversations::targets`).
    pub async fn conversation_targets(
        &self,
        rows: Vec<MessageRef>,
        reach: ConversationReach,
        per_location: bool,
    ) -> Result<Vec<MessageListItem>, StoreError> {
        self.read(move |c| conversations::targets(c, &rows, reach, per_location))
            .await
    }

    /// Pinned (`returned = false`) or returned messages lying in the scope.
    pub async fn inbox_top(
        &self,
        scope: InboxMailboxes,
        returned: bool,
        limit: u32,
    ) -> Result<Vec<MessageListItem>, StoreError> {
        self.read(move |c| {
            let mailboxes = resolve(c, &scope)?;
            inbox::top(c, returned, &mailboxes, limit)
        })
        .await
    }

    /// Messages of important senders in the scope: all and unread.
    pub async fn important_counts(&self, scope: InboxMailboxes) -> Result<(u32, u32), StoreError> {
        self.read(move |c| {
            let mailboxes: Vec<MailboxId> = resolve(c, &scope)?.into_iter().map(|m| m.0).collect();
            inbox::important_counts(c, &mailboxes)
        })
        .await
    }

    /// Unread messages in the inboxes of the visible accounts (`people`: people's, important
    /// and unclassified ones only), for the badge.
    pub async fn unread_in_inboxes(&self, people: bool) -> Result<u32, StoreError> {
        self.read(move |c| {
            let mailboxes: Vec<MailboxId> = groups::unified_inboxes(c, None)?
                .into_iter()
                .map(|m| m.0)
                .collect();
            inbox::unread(c, &mailboxes, people)
        })
        .await
    }

    /// The stored messages at `uids` of `mailbox` (what a new-mail window brought).
    pub async fn arrived_messages(
        &self,
        mailbox: MailboxId,
        uids: UidSet,
    ) -> Result<Vec<ArrivedMessage>, StoreError> {
        self.read(move |c| inbox::arrived(c, mailbox, &uids)).await
    }

    /// The inbox of an account, if it has one.
    pub async fn inbox_of(&self, account: AccountId) -> Result<Option<MailboxId>, StoreError> {
        self.read(move |c| {
            let row = crate::accounts::row_id(c, account)?;
            use rusqlite::OptionalExtension;
            Ok(c.prepare_cached(groups::INBOX_OF)?
                .query_row([row], |r| r.get::<_, i64>(0))
                .optional()?
                .map(MailboxId))
        })
        .await
    }

    // ---------------------------------------------------------------------------------------
    // Snoozes, pins, tasks

    /// The sync identity (account UUID and Message-ID) of a message; `invalid_argument` without
    /// a Message-ID.
    pub async fn message_key(&self, message: MessageId) -> Result<MessageKey, StoreError> {
        self.read(move |c| marks::key_of(c, message)).await
    }

    /// Local messages with this sync identity, newest first.
    pub async fn messages_with_key(&self, key: MessageKey) -> Result<Vec<MessageId>, StoreError> {
        self.read(move |c| marks::messages_of(c, &key)).await
    }

    /// Snoozes a message until `until` (Unix ms). Returns the new and the previous snooze.
    pub async fn snooze_message(
        &self,
        message: MessageId,
        until: i64,
        now: i64,
    ) -> Result<(StoredSnooze, Option<StoredSnooze>), StoreError> {
        self.write(move |tx| marks::snooze(tx, message, until, now))
            .await
    }

    /// Removes a message's snooze (waiting or returned); the removed one.
    pub async fn unsnooze_message(
        &self,
        message: MessageId,
    ) -> Result<Option<StoredSnooze>, StoreError> {
        self.write(move |tx| marks::unsnooze(tx, message)).await
    }

    /// Marks the snoozes due at `now` as returned and returns them.
    pub async fn return_due_snoozes(&self, now: i64) -> Result<Vec<ReturnedSnooze>, StoreError> {
        self.write(move |tx| marks::return_due(tx, now)).await
    }

    /// When the next waiting snooze is due (Unix ms).
    pub async fn next_snooze_due(&self) -> Result<Option<i64>, StoreError> {
        self.read(marks::next_due).await
    }

    pub async fn set_snooze_keyword(
        &self,
        key: MessageKey,
        keyword: bool,
    ) -> Result<(), StoreError> {
        self.write(move |tx| marks::set_snooze_keyword(tx, &key, keyword))
            .await
    }

    /// Pins a message; whether it was not pinned before.
    pub async fn pin_message(&self, message: MessageId, now: i64) -> Result<bool, StoreError> {
        self.write(move |tx| marks::pin(tx, message, now)).await
    }

    /// Unpins a message; whether it was pinned.
    pub async fn unpin_message(&self, message: MessageId) -> Result<bool, StoreError> {
        self.write(move |tx| marks::unpin(tx, message)).await
    }

    /// Marks a message as an open task, as done, or removes the mark (`None`). Returns the task
    /// before and after.
    pub async fn set_task(
        &self,
        message: MessageId,
        done: Option<bool>,
        now: i64,
    ) -> Result<(Option<StoredTask>, Option<StoredTask>), StoreError> {
        self.write(move |tx| marks::set_task(tx, message, done, now))
            .await
    }

    pub async fn set_task_keyword(&self, key: MessageKey, keyword: bool) -> Result<(), StoreError> {
        self.write(move |tx| marks::set_task_keyword(tx, &key, keyword))
            .await
    }

    /// Pinned, snoozed or task messages of `accounts`, each shown in one of `prefer` where it
    /// lies there.
    pub async fn mark_view(
        &self,
        view: MarkView,
        accounts: Vec<AccountId>,
        prefer: Vec<MailboxId>,
        limit: u32,
    ) -> Result<Vec<MessageListItem>, StoreError> {
        self.read(move |c| marks::view(c, view, &accounts, &prefer, limit))
            .await
    }

    // ---------------------------------------------------------------------------------------
    // Local rules

    pub async fn rules(&self) -> Result<Vec<MailRule>, StoreError> {
        self.read(rules::rules).await
    }

    pub async fn create_rule(&self, input: RuleInput, now: i64) -> Result<MailRule, StoreError> {
        self.write(move |tx| {
            let uuid = groups::new_uuid(tx)?;
            rules::create(tx, &uuid, &input, now)
        })
        .await
    }

    pub async fn update_rule(
        &self,
        id: RuleId,
        input: RuleInput,
        now: i64,
    ) -> Result<MailRule, StoreError> {
        self.write(move |tx| rules::update(tx, &id, &input, now))
            .await
    }

    pub async fn delete_rule(&self, id: RuleId) -> Result<bool, StoreError> {
        self.write(move |tx| rules::delete(tx, &id)).await
    }

    pub async fn reorder_rules(
        &self,
        ids: Vec<RuleId>,
        now: i64,
    ) -> Result<Vec<MailRule>, StoreError> {
        self.write(move |tx| rules::reorder(tx, &ids, now)).await
    }
}
