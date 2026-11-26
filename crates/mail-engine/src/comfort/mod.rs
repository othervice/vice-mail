//! Stage 9 in the core (decision 0020): the inbox with categories and bundles, snoozes, pins,
//! tasks, sender rules and local rules, notifications, the badge count, unsubscribing,
//! `mailto:` links and `.eml` export, as methods of [`AppCore`]; and the background work.
//!
//! **Background** (each task holds the core only weakly and ends with it):
//! - *categories*: classifies messages stored without a category (from the stored header
//!   data) and applies changed sender rules to all messages, newest first, in small steps;
//!   `inboxChanged` at most once a second while it changes something.
//! - *snoozes*: returns due snoozes. It looks at the wall clock at least every
//!   [`SNOOZE_CHECK`] (Tokio's timers stop while the computer sleeps, the wall clock does
//!   not), at start (the app was closed over the time) and on `resume`. A returned snooze
//!   stands above the inbox, is announced, and its server keyword is removed.
//! - *arrivals*: the sync reports every new-mail window of an inbox (never the first sync);
//!   the local rules run on its messages and the notification is decided.
//! - *events*: failed and unclear sends are announced once; stage 18: the new-mail popup's
//!   cards follow their messages (snippet once the body is stored, gone when the message or
//!   its account is, see [`popup`]).
//!
//! **Server keywords.** Where the account may write, a snooze sets `$Snoozed` and an open task
//! `$Task` on the message through the outbox (removed again when the snooze returns or the
//! task is done). Nothing is ever moved on the server for a snooze, pin or task. Read-only
//! accounts keep everything local.

pub(crate) mod calendar;
pub(crate) mod classifier;
pub(crate) mod follow_up;
pub mod notify;
pub(crate) mod popup;
pub(crate) mod rules;
pub mod time;
pub(crate) mod unsubscribe;

#[cfg(test)]
mod bench;
#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use jiff::tz::TimeZone;
use mail_content::{SenderIdentity, SenderRules, apply_user_rules, normalize_key};
use mail_model::{
    AccountId, BadgeMode, BundleItem, BundleOptions, BundleRow, BundleState, CategoryCorrection,
    CategoryDecision, ConversationIntent, ConversationSummary, CoreError, CoreEvent,
    DEFAULT_BUNDLE_CHIPS, DateSection, ErrorCode, ExtraRootCa, InboxCursor, InboxFilter, InboxPage,
    InboxRow, InboxScope, InboxSettings, InboxTop, ListCursor, MAX_ACTION_MESSAGES,
    MAX_TOP_FOLLOW_UPS, MAX_TOP_PINNED, MAX_TOP_RETURNED, MailAction, MailRule, MailboxId,
    MarkScope, MessageCategory, MessageCategoryInfo, MessageId, MessageListItem, MessagePage,
    MessageRef, NewMailStyle, NotificationSettings, NotificationTexts, OperationId,
    PRIORITY_PREVIEW, RuleId, RuleInput, RuleSkipReason, SendState, SenderKey, SenderRule,
    SnoozeChoice, SnoozeTimes, TaskFilter, TaskMark, ThreadListItem, ThreadPage, UidSet,
    UnifiedView, UnsubscribeMethod, UnsubscribeOptions, UnsubscribeOutcome, bundle_page,
};
use mail_store::{
    GLOBAL_SCOPE, InboxListKind, InboxMailboxes, MAX_MARK_VIEW, MarkView, MessageKey,
    SenderRuleChange, SettleScope, Store,
};
use tokio::sync::{Notify, broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::core::{AppCore, Connector, WeakCore};

pub use notify::{Notification, NotificationKind, NotificationSink, PopupUpdate, RecordingSink};

/// The keyword a snooze sets on the server where the account may write.
pub const SNOOZE_KEYWORD: &str = "$Snoozed";
/// The keyword an open task sets on the server where the account may write.
pub const TASK_KEYWORD: &str = "$Task";
/// The snooze timer looks at the wall clock at least this often.
pub const SNOOZE_CHECK: Duration = Duration::from_secs(15);

const INBOX_SETTINGS_KEY: &str = "inbox.settings";
const NOTIFY_SETTINGS_KEY: &str = "notifications.settings";
/// Notifications kept until the shell set a sink.
const MAX_PENDING: usize = 20;
/// `inboxChanged` while the category job runs, at most this often.
const CATEGORY_EVENT_GAP: Duration = Duration::from_secs(1);

/// A new-mail window the sync stored.
#[derive(Debug, Clone)]
pub(crate) struct Arrival {
    pub account: AccountId,
    pub mailbox: MailboxId,
    pub uids: UidSet,
}

/// Told by the sync after each new-mail window (outside the first sync). Must not block.
pub(crate) type OnArrival = Arc<dyn Fn(Arrival) + Send + Sync>;

/// [`OnArrival`] for the sync's reporter (which is `Debug`).
#[derive(Clone)]
pub(crate) struct ArrivalHook(pub OnArrival);

impl std::fmt::Debug for ArrivalHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArrivalHook")
    }
}

/// The state of stage 9 in the core.
pub(crate) struct ComfortState {
    arrivals: mpsc::UnboundedSender<Arrival>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<Arrival>>>,
    categories: Arc<Notify>,
    snoozes: Arc<Notify>,
    sink: Mutex<Option<Arc<dyn NotificationSink>>>,
    pending: Mutex<Vec<Notification>>,
    texts: Mutex<NotificationTexts>,
    zone: Mutex<TimeZone>,
    one_click: unsubscribe::OneClick,
    /// Sends already announced: operation and attempt.
    announced_sends: Mutex<HashSet<(OperationId, u32)>>,
    /// Stage 18: the cards of the new-mail popup.
    popup: Mutex<popup::PopupQueue>,
    /// Stage 18: one publication of the popup at a time, so the shell sees changes in order.
    popup_publish: tokio::sync::Mutex<()>,
    /// Stage 18: numbers the popup's delete requests (with the time, for the outbox).
    popup_requests: AtomicU64,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl ComfortState {
    /// Gives the store its classifier. `extra_root` and `allow_loopback` only in tests (the
    /// one-click client may then reach a local test server).
    pub(crate) fn new(
        store: Option<&Store>,
        extra_root: Option<&ExtraRootCa>,
        allow_loopback: bool,
    ) -> Self {
        if let Some(store) = store {
            store.set_classifier(Arc::new(classifier::ContentClassifier));
        }
        let (arrivals, receiver) = mpsc::unbounded_channel();
        Self {
            arrivals,
            receiver: Mutex::new(Some(receiver)),
            categories: Arc::new(Notify::new()),
            snoozes: Arc::new(Notify::new()),
            sink: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            texts: Mutex::new(NotificationTexts::default()),
            zone: Mutex::new(TimeZone::system()),
            one_click: unsubscribe::OneClick::new(extra_root, allow_loopback),
            announced_sends: Mutex::new(HashSet::new()),
            popup: Mutex::new(popup::PopupQueue::default()),
            popup_publish: tokio::sync::Mutex::new(()),
            popup_requests: AtomicU64::new(0),
            tasks: Mutex::new(Vec::new()),
        }
    }

    /// For the sync's environment.
    pub(crate) fn on_arrival(&self) -> OnArrival {
        let sender = self.arrivals.clone();
        Arc::new(move |arrival| {
            // Fails only while the core shuts down.
            let _ = sender.send(arrival);
        })
    }

    pub(crate) fn wake_snoozes(&self) {
        self.snoozes.notify_one();
    }

    pub(crate) fn kick_categories(&self) {
        self.categories.notify_one();
    }

    pub(crate) fn zone(&self) -> TimeZone {
        lock(&self.zone).clone()
    }

    pub(crate) fn stop(&self) {
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
    }
}

impl Drop for ComfortState {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts the background tasks of stage 9 (see the module docs). What is due at start is
/// looked up here, so the tasks only touch the store when there is work: a core dropped right
/// after its start closes the database at once (the tasks hold it only weakly, and never
/// while idle).
///
/// Returns whether the category jobs have work. They begin only when [`AppCore::start`] kicks
/// them at its end (decision 0029): a backfill step holds the writer, and the start's own
/// writes (missed sends, the outbox) would wait behind it.
pub(crate) async fn start<C: Connector>(core: &AppCore<C>) -> bool {
    let state = core.comfort();
    let Ok(store) = core.store_ref() else {
        return false;
    };
    let categories_pending = store.category_work_pending().await.unwrap_or_else(|err| {
        tracing::error!(error = %err, "categories: pending work unknown");
        false
    });
    let next_snooze = next_due(store).await;
    let mut tasks = lock(&state.tasks);
    tasks.push(tokio::spawn(category_worker(
        core.downgrade(),
        Arc::clone(&state.categories),
    )));
    tasks.push(tokio::spawn(snooze_worker(
        core.downgrade(),
        Arc::clone(&state.snoozes),
        next_snooze,
    )));
    if let Some(receiver) = lock(&state.receiver).take() {
        tasks.push(tokio::spawn(arrival_worker(core.downgrade(), receiver)));
    }
    tasks.push(tokio::spawn(event_worker(
        core.downgrade(),
        core.subscribe(),
    )));
    categories_pending
}

/// The next time the snooze timer has work: a snooze or (stage 20) a reminder due.
async fn next_due(store: &Store) -> Option<i64> {
    let snooze = store.next_snooze_due().await.unwrap_or_else(|err| {
        tracing::error!(error = %err, "snoozes: next one unknown");
        None
    });
    let follow_up = store.next_follow_up_due().await.unwrap_or_else(|err| {
        tracing::error!(error = %err, "reminders: next one unknown");
        None
    });
    match (snooze, follow_up) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Waits for [`ComfortState::kick_categories`] (a kick before the wait is kept by the
/// `Notify`), then runs the category jobs until nothing is left, and waits again.
async fn category_worker<C: Connector>(weak: WeakCore<C>, wake: Arc<Notify>) {
    wake.notified().await;
    loop {
        let mut unannounced = false;
        let mut last_event = Instant::now();
        loop {
            let Some(core) = weak.upgrade() else { return };
            let Ok(store) = core.store_ref() else { return };
            match store.category_step().await {
                Ok(step) => {
                    unannounced |= step.classified + step.changed > 0;
                    if unannounced && last_event.elapsed() >= CATEGORY_EVENT_GAP {
                        core.emit_event(CoreEvent::InboxChanged { account_id: None });
                        unannounced = false;
                        last_event = Instant::now();
                    }
                    if step.done && step.classified == 0 {
                        break;
                    }
                }
                Err(err) => {
                    tracing::error!(error = %err, "categories: background step failed");
                    break;
                }
            }
            drop(core);
            tokio::task::yield_now().await;
        }
        if unannounced && let Some(core) = weak.upgrade() {
            core.emit_event(CoreEvent::InboxChanged { account_id: None });
        }
        wake.notified().await;
    }
}

/// Waits for the next due snooze (`next`, Unix ms), at most [`SNOOZE_CHECK`] at a time, or
/// for a wake (a new snooze, `resume`); without any snooze only for a wake. Holds the core
/// only while it returns snoozes.
async fn snooze_worker<C: Connector>(weak: WeakCore<C>, wake: Arc<Notify>, mut next: Option<i64>) {
    loop {
        match next {
            None => wake.notified().await,
            Some(due) => {
                let Some(now) = weak.upgrade().map(|core| core.now_ms()) else {
                    return;
                };
                if due > now {
                    let ms = u64::try_from(due - now).unwrap_or(u64::MAX);
                    let wait = Duration::from_millis(ms).min(SNOOZE_CHECK);
                    tokio::select! {
                        () = tokio::time::sleep(wait) => {}
                        () = wake.notified() => {}
                    }
                }
            }
        }
        let Some(core) = weak.upgrade() else { return };
        if let Err(err) = core.return_due_snoozes().await {
            tracing::error!(code = ?err.code(), error = %err, "snoozes could not be returned");
        }
        // Stage 20 (decision 0059): due reminders come back unless an answer came.
        if let Err(err) = core.settle_follow_ups(SettleScope::Due).await {
            tracing::error!(code = ?err.code(), error = %err, "reminders could not be settled");
        }
        next = match core.store_ref() {
            Ok(store) => next_due(store).await,
            Err(_) => return,
        };
    }
}

async fn arrival_worker<C: Connector>(
    weak: WeakCore<C>,
    mut receiver: mpsc::UnboundedReceiver<Arrival>,
) {
    while let Some(arrival) = receiver.recv().await {
        let Some(core) = weak.upgrade() else { return };
        if let Err(err) = core.handle_arrival(arrival).await {
            tracing::error!(code = ?err.code(), error = %err, "new mail: rules or notification failed");
        }
    }
}

/// Stage 20: after new mail of an account, its reminders are looked at once this long after
/// the last change (a sync stores many windows in a row).
const FOLLOW_UP_SETTLE_DELAY: Duration = Duration::from_secs(1);

async fn event_worker<C: Connector>(weak: WeakCore<C>, mut events: broadcast::Receiver<CoreEvent>) {
    // Stage 20 (decision 0059): accounts whose mail changed, for the answer check.
    let mut pending: HashSet<AccountId> = HashSet::new();
    loop {
        let received = if pending.is_empty() {
            Some(events.recv().await)
        } else {
            tokio::time::timeout(FOLLOW_UP_SETTLE_DELAY, events.recv())
                .await
                .ok()
        };
        let event = match received {
            None => {
                let Some(core) = weak.upgrade() else { return };
                for account in pending.drain() {
                    core.settle_account_follow_ups(account).await;
                }
                continue;
            }
            Some(Ok(event)) => event,
            Some(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Some(Err(broadcast::error::RecvError::Closed)) => return,
        };
        let mails = match event {
            CoreEvent::OutgoingChanged { mails, .. } => mails,
            CoreEvent::SendNeedsDecision { mail, .. } => vec![mail],
            CoreEvent::MessagesChanged {
                account_id,
                mailbox_id,
                ..
            } => {
                // Stage 20 (decision 0059): the account's reminders are looked at once its
                // changes settle.
                pending.insert(account_id);
                // Stage 18: the popup's cards follow their messages and accounts.
                let Some(core) = weak.upgrade() else { return };
                core.refresh_popup(mailbox_id).await;
                continue;
            }
            CoreEvent::AccountsChanged => {
                let Some(core) = weak.upgrade() else { return };
                core.prune_popup_accounts().await;
                continue;
            }
            _ => continue,
        };
        let Some(core) = weak.upgrade() else { return };
        for mail in mails {
            if matches!(mail.state, SendState::Failed | SendState::Unclear) {
                core.announce_failed_send(
                    mail.operation_id,
                    mail.attempts,
                    mail.account_id,
                    &mail.subject,
                )
                .await;
            }
        }
    }
}

/// A list row with its date section, for the bundler. `unread`: the message, or in
/// conversation view (decision 0039) any message of its conversation, is unread.
struct Sectioned {
    item: MessageListItem,
    section: DateSection,
    unread: bool,
}

fn filter_kind(filter: InboxFilter) -> InboxListKind {
    match filter {
        InboxFilter::Category { category } => InboxListKind::Category(category),
        InboxFilter::Priority => InboxListKind::Important,
        // Stage 16 (decision 0033): "Nur ungelesene".
        InboxFilter::Unread => InboxListKind::Unread,
    }
}

/// Decision 0039: the conversation summaries of an inbox page, once per row message.
#[derive(Default)]
struct Summaries {
    list: Vec<ConversationSummary>,
    seen: HashSet<MessageId>,
}

impl Summaries {
    fn add(&mut self, item: &ThreadListItem) {
        if self.seen.insert(item.message.id) {
            self.list.push(ConversationSummary::of(item));
        }
    }

    /// The rows' messages with "unread" for the bundler, keeping the summaries.
    fn take(&mut self, items: Vec<ThreadListItem>) -> Vec<(MessageListItem, bool)> {
        items
            .into_iter()
            .map(|item| {
                self.add(&item);
                (item.message, item.unread_count > 0)
            })
            .collect()
    }

    fn messages(&mut self, items: Vec<ThreadListItem>) -> Vec<MessageListItem> {
        items
            .into_iter()
            .map(|item| {
                self.add(&item);
                item.message
            })
            .collect()
    }

    fn plain(items: Vec<MessageListItem>) -> Vec<(MessageListItem, bool)> {
        items
            .into_iter()
            .map(|item| {
                let unread = item.unread;
                (item, unread)
            })
            .collect()
    }
}

impl BundleItem for Sectioned {
    fn category(&self) -> MessageCategory {
        self.item.category.unwrap_or(MessageCategory::Person)
    }
    fn keep_single(&self) -> bool {
        self.item.important || self.item.pinned
    }
    fn sender(&self) -> Option<&mail_model::Address> {
        self.item.from.as_ref()
    }
    fn unread(&self) -> bool {
        self.unread
    }
    fn section(&self) -> i64 {
        self.section.start
    }
}

fn group_of(view: UnifiedView) -> Option<mail_model::AccountGroupId> {
    match view {
        UnifiedView::All => None,
        UnifiedView::Group { group_id } => Some(group_id),
    }
}

fn mailboxes_of(scope: InboxScope) -> InboxMailboxes {
    match scope {
        InboxScope::Mailbox { mailbox_id } => InboxMailboxes::Mailbox(mailbox_id),
        InboxScope::Unified { view } => InboxMailboxes::Unified(group_of(view)),
    }
}

fn check_messages(messages: &[MessageId]) -> Result<(), CoreError> {
    if messages.is_empty() || messages.len() > MAX_ACTION_MESSAGES {
        return Err(CoreError::InvalidArgument(format!(
            "1 to {MAX_ACTION_MESSAGES} messages"
        )));
    }
    Ok(())
}

fn normalized(key: &SenderKey) -> Result<SenderKey, CoreError> {
    let value = normalize_key(key).ok_or_else(|| {
        CoreError::InvalidArgument(format!(
            "{:?} is not a valid {}",
            key.value,
            key.kind.as_str()
        ))
    })?;
    Ok(SenderKey {
        kind: key.kind,
        value,
    })
}

fn decode<T: serde::de::DeserializeOwned + Default>(json: Option<&str>) -> T {
    json.and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default()
}

impl<C: Connector> AppCore<C> {
    // -----------------------------------------------------------------------------------------
    // Inbox
    // -----------------------------------------------------------------------------------------

    /// One page of the inbox (see `mail_model::InboxPage`): rows newest first with date
    /// sections, notifications and newsletters bundled when categories are on; on the first
    /// page also the pinned, returned and priority messages. `limit` 1..=500 counts messages,
    /// not rows.
    pub async fn list_inbox(
        &self,
        scope: InboxScope,
        cursor: Option<InboxCursor>,
        limit: u32,
    ) -> Result<InboxPage, CoreError> {
        self.sorted_inbox(scope, cursor, limit, false).await
    }

    /// Decision 0039: the inbox as conversations. Same page as [`AppCore::list_inbox`], but
    /// every message of the rows and of the top stands for its conversation (its newest
    /// message in the inbox) and `conversations` holds their summaries. Bundles hold
    /// conversations; pinned conversations and those with a returned snooze stand on top once;
    /// waiting snoozes and mail of blocked senders are left out per message. `limit` 1..=500
    /// counts conversations.
    pub async fn list_inbox_conversations(
        &self,
        scope: InboxScope,
        cursor: Option<InboxCursor>,
        limit: u32,
    ) -> Result<InboxPage, CoreError> {
        self.sorted_inbox(scope, cursor, limit, true).await
    }

    async fn sorted_inbox(
        &self,
        scope: InboxScope,
        cursor: Option<InboxCursor>,
        limit: u32,
        conversations: bool,
    ) -> Result<InboxPage, CoreError> {
        let store = self.store_ref()?;
        let settings = self.get_inbox_settings().await?;
        let mailboxes = mailboxes_of(scope);
        let follow_up_scope = mailboxes.clone();
        let before = cursor.as_ref().map(|c| c.before);
        let mut summaries = Summaries::default();
        let (page_items, page_next) = if conversations {
            let page = store
                .inbox_conversations(mailboxes.clone(), InboxListKind::Rows, before, limit)
                .await?;
            (summaries.take(page.items), page.next_cursor)
        } else {
            let page = store
                .inbox_list(mailboxes.clone(), InboxListKind::Rows, before, limit)
                .await?;
            (Summaries::plain(page.items), page.next_cursor)
        };
        let top = if cursor.is_none() {
            let (pinned, returned) = if conversations {
                let pinned = store
                    .inbox_top_conversations(mailboxes.clone(), false, MAX_TOP_PINNED)
                    .await?;
                let returned = store
                    .inbox_top_conversations(mailboxes.clone(), true, MAX_TOP_RETURNED)
                    .await?;
                (summaries.messages(pinned), summaries.messages(returned))
            } else {
                (
                    store
                        .inbox_top(mailboxes.clone(), false, MAX_TOP_PINNED)
                        .await?,
                    store
                        .inbox_top(mailboxes.clone(), true, MAX_TOP_RETURNED)
                        .await?,
                )
            };
            let (priority, priority_total, priority_unread) = if settings.categories {
                let priority = if conversations {
                    let page = store
                        .inbox_conversations(
                            mailboxes.clone(),
                            InboxListKind::Important,
                            None,
                            PRIORITY_PREVIEW,
                        )
                        .await?;
                    summaries.messages(page.items)
                } else {
                    store
                        .inbox_list(
                            mailboxes.clone(),
                            InboxListKind::Important,
                            None,
                            PRIORITY_PREVIEW,
                        )
                        .await?
                        .items
                };
                // Messages, also in conversation view: the count is one index range.
                let (total, unread) = store.important_counts(mailboxes).await?;
                (priority, total, unread)
            } else {
                (Vec::new(), 0, 0)
            };
            // Stage 20 (decision 0059): sent mail without an answer by the reminder's time.
            let follow_ups = store
                .inbox_follow_ups(follow_up_scope, MAX_TOP_FOLLOW_UPS)
                .await?;
            Some(InboxTop {
                pinned,
                returned,
                priority,
                priority_total,
                priority_unread,
                follow_ups,
            })
        } else {
            None
        };
        let options = if settings.categories {
            BundleOptions {
                notifications: settings.bundle_notifications,
                newsletters: settings.bundle_newsletters,
                max_chips: DEFAULT_BUNDLE_CHIPS,
            }
        } else {
            BundleOptions::off()
        };
        let mut state: BundleState = cursor
            .as_ref()
            .and_then(|c| c.bundles.as_deref())
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        let zone = self.comfort().zone();
        let now = self.now_ms();
        let items: Vec<Sectioned> = page_items
            .into_iter()
            .map(|(item, unread)| Sectioned {
                section: time::section(item.date, now, &zone),
                unread,
                item,
            })
            .collect();
        let last_section = items.last().map(|s| s.section.start);
        let rows = bundle_page(items, &mut state, &options)
            .into_iter()
            .map(|row| match row {
                BundleRow::Message(s) => InboxRow::Message {
                    item: s.item,
                    section: s.section,
                },
                BundleRow::Bundle(bundle) => InboxRow::Bundle {
                    section: bundle.members.first().map_or(
                        DateSection {
                            kind: mail_model::DateSectionKind::Today,
                            start: bundle.summary.key.section,
                        },
                        |m| m.section,
                    ),
                    summary: bundle.summary,
                    members: bundle.members.into_iter().map(|m| m.item).collect(),
                },
                BundleRow::More(bundle) => InboxRow::More {
                    summary: bundle.summary,
                    members: bundle.members.into_iter().map(|m| m.item).collect(),
                },
            })
            .collect();
        if let Some(last) = last_section {
            state.keep_section(last);
        }
        let next_cursor = page_next.map(|before| InboxCursor {
            before,
            bundles: (!state.is_empty())
                .then(|| serde_json::to_string(&state).ok())
                .flatten(),
        });
        Ok(InboxPage {
            rows,
            next_cursor,
            top,
            categories: settings.categories,
            conversations: summaries.list,
        })
    }

    /// Decision 0039: the inbox filtered (a category, the important senders, unread only) as
    /// conversations, newest first; waiting snoozes left out. `limit` 1..=500.
    pub async fn list_inbox_conversations_filtered(
        &self,
        scope: InboxScope,
        filter: InboxFilter,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<ThreadPage, CoreError> {
        Ok(self
            .store_ref()?
            .inbox_conversations(mailboxes_of(scope), filter_kind(filter), cursor, limit)
            .await?)
    }

    /// Decision 0039: the messages an action on conversation rows touches (`rows`: each row's
    /// message and the mailbox it is listed in), as list rows; the UI then runs the action on
    /// them as usual (see [`ConversationIntent::reach`]).
    pub async fn conversation_targets(
        &self,
        rows: Vec<mail_model::MessageRef>,
        intent: ConversationIntent,
    ) -> Result<Vec<MessageListItem>, CoreError> {
        Ok(self
            .store_ref()?
            .conversation_targets(rows, intent.reach(), intent.per_location())
            .await?)
    }

    /// The inbox filtered by category or to the important senders, newest first (snoozed
    /// messages left out). `limit` 1..=500.
    pub async fn list_inbox_filtered(
        &self,
        scope: InboxScope,
        filter: InboxFilter,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, CoreError> {
        Ok(self
            .store_ref()?
            .inbox_list(mailboxes_of(scope), filter_kind(filter), cursor, limit)
            .await?)
    }

    pub async fn get_inbox_settings(&self) -> Result<InboxSettings, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(decode(
            settings
                .iter()
                .find(|s| s.key == INBOX_SETTINGS_KEY)
                .map(|s| s.value.as_str()),
        ))
    }

    /// Stores the inbox settings; emits `inboxChanged`.
    pub async fn set_inbox_settings(
        &self,
        settings: InboxSettings,
    ) -> Result<InboxSettings, CoreError> {
        let json = serde_json::to_string(&settings)
            .map_err(|err| CoreError::Internal(format!("inbox settings: {err}")))?;
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(INBOX_SETTINGS_KEY.to_owned(), json)])
            .await?;
        self.emit_event(CoreEvent::InboxChanged { account_id: None });
        Ok(settings)
    }

    // -----------------------------------------------------------------------------------------
    // Categories and sender rules
    // -----------------------------------------------------------------------------------------

    /// "Why here?": the category of a message with its reason (the user's correction first),
    /// and the keys an "always as …" can use.
    pub async fn get_message_category(
        &self,
        message: MessageId,
    ) -> Result<MessageCategoryInfo, CoreError> {
        let store = self.store_ref()?;
        let stored = store
            .message_category(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let rules = store.sender_rules().await?;
        let corrections: Vec<CategoryCorrection> = rules
            .iter()
            .filter_map(|r| {
                r.category.map(|category| CategoryCorrection {
                    key: r.key.clone(),
                    category,
                })
            })
            .collect();
        let important: Vec<SenderKey> = rules
            .iter()
            .filter(|r| r.important)
            .map(|r| r.key.clone())
            .collect();
        let sender_rules = SenderRules::new(&corrections, &important);
        let identity = SenderIdentity {
            address: stored.keys.address.clone(),
            domain: stored.keys.domain.clone(),
            list_id: stored.keys.list_id.clone(),
        };
        let decision = match (stored.automatic.or(stored.category), stored.reason) {
            (Some(category), Some(reason)) => Some(apply_user_rules(
                CategoryDecision {
                    category,
                    reason,
                    important: None,
                },
                &identity,
                &sender_rules,
            )),
            _ => None,
        };
        Ok(MessageCategoryInfo {
            message_id: message,
            decision,
            address: stored.keys.address.map(SenderKey::address),
            domain: stored.keys.domain.map(SenderKey::domain),
            list: stored.keys.list_id.map(SenderKey::list_id),
        })
    }

    /// "Always as …" for a sender, domain or list; `None` removes the correction. Applies to
    /// every message (in the background, newest first; `inboxChanged` follows). Returns the
    /// rule, `None` when none is left for the key.
    pub async fn set_sender_category(
        &self,
        key: SenderKey,
        category: Option<MessageCategory>,
    ) -> Result<Option<SenderRule>, CoreError> {
        let key = normalized(&key)?;
        let rule = self
            .store_ref()?
            .put_sender_rule(
                key,
                SenderRuleChange {
                    category: Some(category),
                    important: None,
                },
                self.now_ms(),
            )
            .await?;
        self.comfort().kick_categories();
        Ok(rule)
    }

    /// Marks a sender, domain or list important (the "Priority" block), or not.
    pub async fn set_sender_important(
        &self,
        key: SenderKey,
        important: bool,
    ) -> Result<Option<SenderRule>, CoreError> {
        let key = normalized(&key)?;
        let rule = self
            .store_ref()?
            .put_sender_rule(
                key,
                SenderRuleChange {
                    category: None,
                    important: Some(important),
                },
                self.now_ms(),
            )
            .await?;
        self.comfort().kick_categories();
        Ok(rule)
    }

    pub async fn list_sender_rules(&self) -> Result<Vec<SenderRule>, CoreError> {
        Ok(self.store_ref()?.sender_rules().await?)
    }

    /// Deletes a sender rule (`not_found` if there is none).
    pub async fn delete_sender_rule(&self, id: String) -> Result<(), CoreError> {
        if !self.store_ref()?.delete_sender_rule(id.clone()).await? {
            return Err(CoreError::NotFound(format!("sender rule {id}")));
        }
        self.comfort().kick_categories();
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Snoozes, pins, tasks
    // -----------------------------------------------------------------------------------------

    /// The times the snooze menu offers now, in the user's zone.
    pub fn snooze_times(&self) -> SnoozeTimes {
        time::snooze_times(self.now_ms(), &self.comfort().zone())
    }

    /// Snoozes messages until the chosen time (they leave the inbox and come back then, on
    /// top, with a notification). Returns the time (Unix ms). `invalid_argument` for a message
    /// without Message-ID or a time not ahead.
    pub async fn snooze_messages(
        &self,
        messages: Vec<MessageId>,
        choice: SnoozeChoice,
    ) -> Result<i64, CoreError> {
        check_messages(&messages)?;
        let now = self.now_ms();
        let until = time::until(choice, now, &self.comfort().zone())?;
        let store = self.store_ref()?;
        let mut accounts = HashSet::new();
        for message in &messages {
            let (stored, _) = store.snooze_message(*message, until, now).await?;
            accounts.insert(stored.key.account);
            if !stored.keyword && self.mirror_keyword(&stored.key, SNOOZE_KEYWORD, true).await {
                store.set_snooze_keyword(stored.key.clone(), true).await?;
            }
        }
        tracing::info!(count = messages.len(), "messages snoozed");
        self.comfort().wake_snoozes();
        self.inbox_changed(&accounts);
        Ok(until)
    }

    /// Ends the snooze of messages (waiting ones come back now without a notification; returned
    /// ones lose their mark). Messages without a snooze are left alone.
    pub async fn unsnooze_messages(&self, messages: Vec<MessageId>) -> Result<(), CoreError> {
        check_messages(&messages)?;
        let store = self.store_ref()?;
        let mut accounts = HashSet::new();
        for message in &messages {
            if let Some(removed) = store.unsnooze_message(*message).await? {
                accounts.insert(removed.key.account);
                if removed.keyword {
                    self.mirror_keyword(&removed.key, SNOOZE_KEYWORD, false)
                        .await;
                }
            }
        }
        self.comfort().wake_snoozes();
        self.inbox_changed(&accounts);
        Ok(())
    }

    /// Pins or unpins messages.
    pub async fn set_pinned(
        &self,
        messages: Vec<MessageId>,
        pinned: bool,
    ) -> Result<(), CoreError> {
        check_messages(&messages)?;
        let store = self.store_ref()?;
        let now = self.now_ms();
        let mut accounts = HashSet::new();
        for message in &messages {
            let key = store.message_key(*message).await?;
            let changed = if pinned {
                store.pin_message(*message, now).await?
            } else {
                store.unpin_message(*message).await?
            };
            if changed {
                accounts.insert(key.account);
            }
        }
        self.inbox_changed(&accounts);
        Ok(())
    }

    /// Marks messages as tasks (`Some(TaskMark { done: false })`), as done (`done: true`), or
    /// removes the mark (`None`).
    pub async fn set_task(
        &self,
        messages: Vec<MessageId>,
        mark: Option<TaskMark>,
    ) -> Result<(), CoreError> {
        check_messages(&messages)?;
        let store = self.store_ref()?;
        let now = self.now_ms();
        let mut accounts = HashSet::new();
        for message in &messages {
            let (before, after) = store.set_task(*message, mark.map(|m| m.done), now).await?;
            let key = match (&after, &before) {
                (Some(task), _) | (None, Some(task)) => task.key.clone(),
                (None, None) => continue,
            };
            accounts.insert(key.account);
            let had_keyword = before.as_ref().is_some_and(|t| t.keyword);
            let open = after.as_ref().is_some_and(|t| t.done_at.is_none());
            if open && !had_keyword {
                if self.mirror_keyword(&key, TASK_KEYWORD, true).await {
                    store.set_task_keyword(key, true).await?;
                }
            } else if !open && had_keyword {
                self.mirror_keyword(&key, TASK_KEYWORD, false).await;
                if after.is_some() {
                    store.set_task_keyword(key, false).await?;
                }
            }
        }
        self.inbox_changed(&accounts);
        Ok(())
    }

    /// "Später": waiting snoozes of the scope, the next due first (at most 1,000).
    pub async fn list_snoozed(&self, scope: MarkScope) -> Result<Vec<MessageListItem>, CoreError> {
        self.mark_view(MarkView::Snoozed, scope).await
    }

    /// "Angepinnt": pinned messages of the scope, the most recently pinned first.
    pub async fn list_pinned(&self, scope: MarkScope) -> Result<Vec<MessageListItem>, CoreError> {
        self.mark_view(MarkView::Pinned, scope).await
    }

    /// "Markiert": messages flagged on the server (IMAP `\Flagged`, JMAP `$flagged`) in any
    /// folder of the scope's accounts but Trash and Junk, the newest first, each once (at most
    /// 1,000), shown in its inbox where it is flagged there.
    pub async fn list_flagged(&self, scope: MarkScope) -> Result<Vec<MessageListItem>, CoreError> {
        let (accounts, prefer) = self.mark_scope(scope).await?;
        Ok(self
            .store_ref()?
            .flagged_view(accounts, prefer, MAX_MARK_VIEW)
            .await?)
    }

    /// "Aufgaben": task messages of the scope (open, done or all), the newest task first.
    pub async fn list_tasks(
        &self,
        scope: MarkScope,
        filter: TaskFilter,
    ) -> Result<Vec<MessageListItem>, CoreError> {
        self.mark_view(MarkView::Tasks(filter), scope).await
    }

    pub(crate) async fn mark_view(
        &self,
        view: MarkView,
        scope: MarkScope,
    ) -> Result<Vec<MessageListItem>, CoreError> {
        let (accounts, prefer) = self.mark_scope(scope).await?;
        Ok(self
            .store_ref()?
            .mark_view(view, accounts, prefer, MAX_MARK_VIEW)
            .await?)
    }

    /// The accounts of a mark view and their inboxes (where a marked message is shown).
    pub(crate) async fn mark_scope(
        &self,
        scope: MarkScope,
    ) -> Result<(Vec<AccountId>, Vec<MailboxId>), CoreError> {
        let store = self.store_ref()?;
        let accounts = match scope {
            MarkScope::All => store.visible_accounts(None).await?,
            MarkScope::Group { group_id } => store.visible_accounts(Some(group_id)).await?,
            MarkScope::Account { account_id } => {
                if store.account(account_id).await?.is_none() {
                    return Err(CoreError::NotFound(format!("account {account_id}")));
                }
                vec![account_id]
            }
        };
        let mut prefer = Vec::new();
        for account in &accounts {
            if let Some(inbox) = store.inbox_of(*account).await? {
                prefer.push(inbox);
            }
        }
        Ok((accounts, prefer))
    }

    /// Returns the snoozes due now: they come back on top, their keyword goes, the user is
    /// notified. Called by the timer; `resume` triggers it as well.
    pub(crate) async fn return_due_snoozes(&self) -> Result<usize, CoreError> {
        let store = self.store_ref()?;
        let now = self.now_ms();
        let returned = store.return_due_snoozes(now).await?;
        if returned.is_empty() {
            return Ok(0);
        }
        let settings = self.get_notification_settings().await?;
        let texts = lock(&self.comfort().texts).clone();
        let minute = time::minute_of_day(now, &self.comfort().zone());
        // Stage 21: the snoozes come back either way; a pause only keeps them quiet.
        let paused = self.notifications_paused().await;
        let mut accounts = HashSet::new();
        for r in &returned {
            accounts.insert(r.snooze.key.account);
            if r.snooze.keyword {
                self.mirror_keyword(&r.snooze.key, SNOOZE_KEYWORD, false)
                    .await;
                store
                    .set_snooze_keyword(r.snooze.key.clone(), false)
                    .await?;
            }
            let Some(&message) = r.messages.first() else {
                continue;
            };
            let subject = store
                .message_summary(message)
                .await?
                .map(|s| s.subject)
                .unwrap_or_default();
            let inbox = store.inbox_of(r.snooze.key.account).await?;
            if let Some(n) = notify::snooze_due(
                &settings,
                &texts,
                minute,
                r.snooze.key.account,
                inbox,
                message,
                &subject,
            ) && !paused
            {
                self.notify(n);
            }
        }
        tracing::info!(count = returned.len(), "snoozed messages came back");
        self.inbox_changed(&accounts);
        Ok(returned.len())
    }

    /// Sets or removes `keyword` on every location of the messages with this identity, through
    /// the outbox, if the account may write. Whether something was queued. Failures are
    /// logged; the local mark stays the truth.
    async fn mirror_keyword(&self, key: &MessageKey, keyword: &str, add: bool) -> bool {
        let Ok(store) = self.store_ref() else {
            return false;
        };
        match store.account(key.account).await {
            Ok(Some(account)) if account.writes_enabled => {}
            _ => return false,
        }
        let Ok(messages) = store.messages_with_key(key.clone()).await else {
            return false;
        };
        let mut refs = Vec::new();
        for message in messages {
            if let Ok(locations) = store.message_locations(message).await {
                refs.extend(
                    locations
                        .into_iter()
                        .filter(|l| l.selectable)
                        .map(|l| MessageRef {
                            mailbox_id: l.mailbox_id,
                            message_id: message,
                        }),
                );
            }
        }
        refs.truncate(MAX_ACTION_MESSAGES);
        if refs.is_empty() {
            return false;
        }
        let action = if add {
            MailAction::AddKeyword {
                keyword: keyword.to_owned(),
            }
        } else {
            MailAction::RemoveKeyword {
                keyword: keyword.to_owned(),
            }
        };
        let request = uuid::Uuid::new_v4().to_string();
        match self.apply_action(action, refs, request).await {
            Ok(_) => true,
            Err(err) => {
                tracing::warn!(code = ?err.code(), error = %err, keyword, "server keyword not queued");
                false
            }
        }
    }

    /// Stage 20: the answer check of one account's reminders, if it has any.
    async fn settle_account_follow_ups(&self, account: AccountId) {
        let Ok(store) = self.store_ref() else { return };
        match store.has_follow_ups(account).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(err) => {
                tracing::error!(error = %err, "reminders: unknown whether there are any");
                return;
            }
        }
        if let Err(err) = self.settle_follow_ups(SettleScope::Account(account)).await {
            tracing::error!(code = ?err.code(), error = %err, "reminders could not be settled");
        }
    }

    fn inbox_changed(&self, accounts: &HashSet<AccountId>) {
        match accounts.len() {
            0 => {}
            1 => self.emit_event(CoreEvent::InboxChanged {
                account_id: accounts.iter().next().copied(),
            }),
            _ => self.emit_event(CoreEvent::InboxChanged { account_id: None }),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Local rules
    // -----------------------------------------------------------------------------------------

    pub async fn list_rules(&self) -> Result<Vec<MailRule>, CoreError> {
        Ok(self.store_ref()?.rules().await?)
    }

    pub async fn create_rule(&self, input: RuleInput) -> Result<MailRule, CoreError> {
        Ok(self.store_ref()?.create_rule(input, self.now_ms()).await?)
    }

    pub async fn update_rule(&self, id: RuleId, input: RuleInput) -> Result<MailRule, CoreError> {
        Ok(self
            .store_ref()?
            .update_rule(id, input, self.now_ms())
            .await?)
    }

    pub async fn delete_rule(&self, id: RuleId) -> Result<(), CoreError> {
        if !self.store_ref()?.delete_rule(id.clone()).await? {
            return Err(CoreError::NotFound(format!("rule {id}")));
        }
        Ok(())
    }

    /// Puts the rules in this order (the rest after them); returns all rules.
    pub async fn reorder_rules(&self, ids: Vec<RuleId>) -> Result<Vec<MailRule>, CoreError> {
        Ok(self.store_ref()?.reorder_rules(ids, self.now_ms()).await?)
    }

    /// New mail in an inbox: runs the local rules, then decides the notification.
    pub(crate) async fn handle_arrival(&self, arrival: Arrival) -> Result<(), CoreError> {
        let store = self.store_ref()?;
        if store.inbox_of(arrival.account).await? != Some(arrival.mailbox) {
            return Ok(());
        }
        let mut messages = store
            .arrived_messages(arrival.mailbox, arrival.uids)
            .await?;
        if messages.is_empty() {
            return Ok(());
        }
        let account = store
            .account(arrival.account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", arrival.account)))?;
        // Stage 16 (decision 0033): mail of blocked senders goes to junk or is hidden; no
        // rules, no notification.
        let blocked = self.handle_blocked_arrivals(&account, &messages).await?;
        messages.retain(|m| !blocked.contains(&m.id));
        // Stage 22 (decision 0077): what the learning spam filter takes for spam goes to junk
        // or is marked; no rules, no notification. A failure leaves the mail to the rules.
        match self.handle_spam_arrivals(&account, &messages).await {
            Ok(spam) => messages.retain(|m| !spam.contains(&m.id)),
            Err(err) => {
                tracing::error!(code = ?err.code(), error = %err, "spam filter: new mail not looked at");
            }
        }
        if messages.is_empty() {
            return Ok(());
        }
        let all_rules = store.rules().await?;
        // Per rule and reason the messages it skipped (a message counts once per rule).
        let mut skipped: HashMap<(RuleId, String, RuleSkipReason), HashSet<MessageId>> =
            HashMap::new();
        let mut overrides = Vec::with_capacity(messages.len());
        let mut marks_changed = false;
        for message in &mut messages {
            let plan = rules::plan(&all_rules, message);
            let mut notify = plan.notify;
            if let Some((rule, category)) = &plan.category
                && store
                    .set_local_category(message.id, *category, rule.name.clone())
                    .await?
            {
                message.category = Some(*category);
                marks_changed = true;
            }
            if let Some(rule) = &plan.pin {
                match store.pin_message(message.id, self.now_ms()).await {
                    Ok(_) => marks_changed = true,
                    Err(mail_store::StoreError::InvalidArgument(_)) => {
                        skipped
                            .entry((
                                rule.id.clone(),
                                rule.name.clone(),
                                RuleSkipReason::NoMessageId,
                            ))
                            .or_default()
                            .insert(message.id);
                    }
                    Err(err) => return Err(err.into()),
                }
            }
            let target = MessageRef {
                mailbox_id: message.mailbox_id,
                message_id: message.id,
            };
            if let Some(rule) = &plan.mark_read {
                let request = format!("rule-{}-{}-r", rule.id, message.id);
                if let Err(reason) = self
                    .rule_write(&account, MailAction::MarkRead, target, request)
                    .await
                {
                    skipped
                        .entry((rule.id.clone(), rule.name.clone(), reason))
                        .or_default()
                        .insert(message.id);
                } else {
                    message.unread = false;
                }
            }
            if let Some((rule, account_id, path)) = &plan.move_to {
                let result = match self.mailbox_by_path(*account_id, path).await? {
                    Some(mailbox) if *account_id == message.account_id => {
                        let request = format!("rule-{}-{}-m", rule.id, message.id);
                        self.rule_write(
                            &account,
                            MailAction::Move { target: mailbox },
                            target,
                            request,
                        )
                        .await
                    }
                    _ => Err(RuleSkipReason::MissingMailbox),
                };
                match result {
                    Ok(()) => {
                        // Moved away: announced only if a rule asks for it.
                        notify = Some(notify.unwrap_or(false));
                    }
                    Err(reason) => {
                        skipped
                            .entry((rule.id.clone(), rule.name.clone(), reason))
                            .or_default()
                            .insert(message.id);
                    }
                }
            }
            overrides.push((message.clone(), notify));
        }
        for ((rule_id, rule_name, reason), messages) in skipped {
            let count = u32::try_from(messages.len()).unwrap_or(u32::MAX);
            tracing::info!(rule = %rule_id, ?reason, count, "rule action skipped");
            self.emit_event(CoreEvent::RuleSkipped {
                account_id: arrival.account,
                rule_id,
                rule_name,
                reason,
                count,
            });
        }
        if marks_changed {
            self.emit_event(CoreEvent::InboxChanged {
                account_id: Some(arrival.account),
            });
        }
        // Stage 21 (decision 0069): paused notifications drop new mail like quiet hours do.
        if self.notifications_paused().await {
            tracing::debug!("notifications paused: new mail not announced");
            return Ok(());
        }
        let settings = self.get_notification_settings().await?;
        let texts = lock(&self.comfort().texts).clone();
        let minute = time::minute_of_day(self.now_ms(), &self.comfort().zone());
        match settings.new_mail_style {
            NewMailStyle::System => {
                if let Some(n) = notify::new_mail(&settings, &texts, minute, &overrides) {
                    self.notify(n);
                }
            }
            // Stage 18 (decision 0051): the same messages, as cards of the app's popup.
            NewMailStyle::Popup => {
                let announced = notify::announced(&settings, minute, &overrides);
                self.popup_new_mail(&settings, &announced).await;
            }
        }
        Ok(())
    }

    /// A rule's writing action through the outbox; why it did not run.
    async fn rule_write(
        &self,
        account: &mail_store::StoredAccount,
        action: MailAction,
        target: MessageRef,
        request: String,
    ) -> Result<(), RuleSkipReason> {
        if !account.writes_enabled {
            return Err(RuleSkipReason::ReadOnly);
        }
        match self.apply_action(action, vec![target], request).await {
            Ok(_) => Ok(()),
            Err(err) => Err(match err.code() {
                ErrorCode::ReadOnly => RuleSkipReason::ReadOnly,
                ErrorCode::MissingMailbox => RuleSkipReason::MissingMailbox,
                _ => {
                    tracing::warn!(code = ?err.code(), error = %err, "rule action refused");
                    RuleSkipReason::Failed
                }
            }),
        }
    }

    async fn mailbox_by_path(
        &self,
        account: AccountId,
        path: &str,
    ) -> Result<Option<MailboxId>, CoreError> {
        let store = self.store_ref()?;
        if store.account(account).await?.is_none() {
            return Ok(None);
        }
        Ok(store
            .mailboxes(account)
            .await?
            .into_iter()
            .find(|m| m.path == path && m.selectable)
            .map(|m| m.id))
    }

    // -----------------------------------------------------------------------------------------
    // Notifications and the badge
    // -----------------------------------------------------------------------------------------

    pub async fn get_notification_settings(&self) -> Result<NotificationSettings, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(decode(
            settings
                .iter()
                .find(|s| s.key == NOTIFY_SETTINGS_KEY)
                .map(|s| s.value.as_str()),
        ))
    }

    /// Stores the notification settings (`invalid_argument` for quiet hours outside the day,
    /// and, stage 16, for a sound name that is not in its platform's list).
    pub async fn set_notification_settings(
        &self,
        settings: NotificationSettings,
    ) -> Result<NotificationSettings, CoreError> {
        let day = 24 * 60;
        if settings.quiet_hours.start_minute >= day || settings.quiet_hours.end_minute >= day {
            return Err(CoreError::InvalidArgument(
                "quiet hours are minutes 0..1440 of the day".into(),
            ));
        }
        if let Some(name) = settings.sound.unknown_name() {
            return Err(CoreError::InvalidArgument(format!(
                "{name:?} is not a notification sound"
            )));
        }
        let json = serde_json::to_string(&settings)
            .map_err(|err| CoreError::Internal(format!("notification settings: {err}")))?;
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(NOTIFY_SETTINGS_KEY.to_owned(), json)])
            .await?;
        Ok(settings)
    }

    /// Stage 16: the notification sounds this platform offers, `Default` first.
    pub fn notification_sound_choices(&self) -> mail_model::NotificationSoundChoices {
        let platform = mail_model::SoundPlatform::current();
        mail_model::NotificationSoundChoices {
            platform,
            sounds: platform.sounds().iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// The texts of system notifications in the UI's language (from `app/src/i18n/`).
    pub fn set_notification_texts(&self, texts: NotificationTexts) {
        *lock(&self.comfort().texts) = texts;
    }

    /// Where notifications go (the shell's native notifications). Notifications from before
    /// (for example snoozes that came back while the app was closed) are shown now.
    pub fn set_notification_sink(&self, sink: Arc<dyn NotificationSink>) {
        let pending: Vec<Notification> = lock(&self.comfort().pending).drain(..).collect();
        for notification in pending {
            sink.show(notification);
        }
        *lock(&self.comfort().sink) = Some(sink);
    }

    /// The user clicked a notification: the UI shows the message (`openMessageRequested`).
    pub fn notification_clicked(
        &self,
        account: AccountId,
        mailbox: Option<MailboxId>,
        message: MessageId,
    ) {
        self.emit_event(CoreEvent::OpenMessageRequested {
            account_id: account,
            mailbox_id: mailbox,
            message_id: message,
        });
    }

    fn notify(&self, notification: Notification) {
        let sink = lock(&self.comfort().sink).clone();
        match sink {
            Some(sink) => sink.show(notification),
            None => {
                let mut pending = lock(&self.comfort().pending);
                if pending.len() < MAX_PENDING {
                    pending.push(notification);
                }
            }
        }
    }

    async fn announce_failed_send(
        &self,
        operation: OperationId,
        attempts: u32,
        account: AccountId,
        subject: &str,
    ) {
        if !lock(&self.comfort().announced_sends).insert((operation, attempts)) {
            return;
        }
        let Ok(settings) = self.get_notification_settings().await else {
            return;
        };
        if self.notifications_paused().await {
            return;
        }
        let texts = lock(&self.comfort().texts).clone();
        let minute = time::minute_of_day(self.now_ms(), &self.comfort().zone());
        if let Some(n) = notify::send_failed(&settings, &texts, minute, account, subject) {
            self.notify(n);
        }
    }

    /// The number for the Dock or taskbar icon, by the badge setting (0 when off).
    pub async fn badge_count(&self) -> Result<u32, CoreError> {
        let settings = self.get_notification_settings().await?;
        let store = self.store_ref()?;
        Ok(match settings.badge {
            BadgeMode::Off => 0,
            BadgeMode::InboxUnread => store.unread_in_inboxes(false).await?,
            BadgeMode::PeopleUnread => store.unread_in_inboxes(true).await?,
        })
    }

    // -----------------------------------------------------------------------------------------
    // Unsubscribing
    // -----------------------------------------------------------------------------------------

    /// What a message offers for unsubscribing (empty when nothing).
    pub async fn get_unsubscribe_options(
        &self,
        message: MessageId,
    ) -> Result<UnsubscribeOptions, CoreError> {
        let (uris, one_click) = self.unsubscribe_sources(message).await?;
        Ok(unsubscribe::options(&uris, one_click))
    }

    /// Unsubscribes after the user's click: one-click POST, the fields of a `mailto:` request
    /// for the composer, or the page in the browser. Nothing goes out before this call.
    pub async fn unsubscribe(
        &self,
        message: MessageId,
        method: UnsubscribeMethod,
    ) -> Result<UnsubscribeOutcome, CoreError> {
        let (uris, one_click) = self.unsubscribe_sources(message).await?;
        let target = unsubscribe::target(&uris, one_click, method)?;
        match method {
            UnsubscribeMethod::OneClick => {
                let status = self.comfort().one_click.post(&target).await?;
                Ok(UnsubscribeOutcome::Requested { status })
            }
            UnsubscribeMethod::Mailto => Ok(UnsubscribeOutcome::Compose {
                request: mail_content::parse_mailto(&target)?,
            }),
            UnsubscribeMethod::Web => {
                self.reading_ref()?.open_web_page(target).await?;
                Ok(UnsubscribeOutcome::Opened)
            }
        }
    }

    /// The `List-Unsubscribe` URIs and the one-click flag: from the header data of stage 9, or
    /// for older messages from the stored body.
    async fn unsubscribe_sources(
        &self,
        message: MessageId,
    ) -> Result<(Vec<String>, bool), CoreError> {
        let store = self.store_ref()?;
        let stored = store
            .message_category(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let uris = stored
            .list_unsubscribe
            .as_deref()
            .map(unsubscribe::uris_of)
            .unwrap_or_default();
        if let Some(one_click) = stored.unsubscribe_one_click {
            return Ok((uris, one_click));
        }
        if let Some(info) = store.body_info(message).await?
            && info.complete
            && let Some(raw) = store.load_message(message).await?
        {
            let parsed =
                tokio::task::spawn_blocking(move || mail_content::parse(&raw).headers.list)
                    .await
                    .map_err(|err| CoreError::Internal(format!("parse task: {err}")))?;
            let one_click = parsed.unsubscribe_post.as_deref().is_some_and(|post| {
                post.split_whitespace()
                    .collect::<String>()
                    .eq_ignore_ascii_case("List-Unsubscribe=One-Click")
            });
            let uris = if parsed.unsubscribe.is_empty() {
                uris
            } else {
                parsed.unsubscribe
            };
            return Ok((uris, one_click));
        }
        Ok((uris, false))
    }

    // -----------------------------------------------------------------------------------------
    // mailto: links, export
    // -----------------------------------------------------------------------------------------

    /// A `mailto:` link as composer fields (RFC 6068), without an event; other schemes are
    /// `invalid_argument`.
    pub fn parse_mailto_url(&self, url: &str) -> Result<mail_model::ComposeRequest, CoreError> {
        Ok(mail_content::parse_mailto(url)?)
    }

    /// The system opened a URL with the app (default mail app): a `mailto:` link becomes
    /// `composeRequested`. Returns the parsed request; other schemes are `invalid_argument`.
    pub fn open_url(&self, url: &str) -> Result<mail_model::ComposeRequest, CoreError> {
        let request = self.parse_mailto_url(url)?;
        tracing::info!(
            recipients = request.to.len() + request.cc.len() + request.bcc.len(),
            notices = request.notices.len(),
            "mailto link opened"
        );
        self.emit_event(CoreEvent::ComposeRequested {
            request: request.clone(),
        });
        Ok(request)
    }

    /// The file name the save dialog suggests for an `.eml` export.
    pub async fn export_file_name(&self, message: MessageId) -> Result<String, CoreError> {
        let summary = self
            .store_ref()?
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let subject = summary.subject.trim();
        let base = if subject.is_empty() {
            "message"
        } else {
            subject
        };
        let base: String = base.chars().take(120).collect();
        Ok(mail_content::sanitize_filename(&format!("{base}.eml")))
    }

    /// Writes the message as it is on the server to `path` (chosen in the system dialog) as
    /// `.eml`; fetched first if it is not stored whole.
    pub async fn export_message(
        &self,
        message: MessageId,
        path: PathBuf,
    ) -> Result<PathBuf, CoreError> {
        let reading = self.reading_ref()?;
        let raw = reading.raw_message(message).await?;
        let path = reading.save_bytes_to(path, raw).await?;
        tracing::info!(message = message.0, "message exported");
        Ok(path)
    }

    // -----------------------------------------------------------------------------------------
    // Tests

    /// Tests: the zone the core computes sections, snoozes and quiet hours in.
    #[cfg(test)]
    pub(crate) fn set_time_zone(&self, zone: TimeZone) {
        *lock(&self.comfort().zone) = zone;
    }
}
