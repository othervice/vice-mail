//! System notifications (stage 9): what the core announces, and to whom. The shell supplies
//! the [`NotificationSink`] (native notifications); tests record. The decisions are pure.
//!
//! - **New mail**: only unread messages that arrived in an inbox during an incremental sync
//!   (never the first sync), only when the account is not muted, outside quiet hours, and by
//!   category: people and important senders on, notifications and newsletters off (default).
//!   A local rule's "notify on/off" overrides the category (and the unread check). Several messages of one sync window
//!   are one notification.
//! - **Failed sends** (`failed`, `unclear`) and **returned snoozes**, each once.
//! - **Hidden content**: title "New message" (the UI's text) and no body. The lock-screen
//!   preview itself is the system's choice (macOS: "Show previews"; Windows: notifications on
//!   the lock screen); the app cannot tell the system which notification is private.
//! - Quiet hours drop notifications (they are not delivered later).

use std::sync::{Mutex, PoisonError};

use mail_model::{
    AccountId, MailboxId, MessageCategory, MessageId, NotificationSettings, NotificationTexts,
};
use mail_store::ArrivedMessage;

/// What a notification is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationKind {
    NewMail,
    SendFailed,
    SnoozeDue,
}

/// One system notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub kind: NotificationKind,
    pub title: String,
    /// Empty when the content is hidden.
    pub body: String,
    pub account_id: AccountId,
    /// What a click opens ([`crate::AppCore::notification_clicked`]).
    pub mailbox_id: Option<MailboxId>,
    pub message_id: Option<MessageId>,
}

/// Shows notifications. The shell's implementation posts native ones; it must not block.
pub trait NotificationSink: Send + Sync + 'static {
    fn show(&self, notification: Notification);
}

/// Records notifications instead of showing them (tests).
#[derive(Debug, Default)]
pub struct RecordingSink {
    shown: Mutex<Vec<Notification>>,
}

impl RecordingSink {
    pub fn shown(&self) -> Vec<Notification> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl NotificationSink for RecordingSink {
    fn show(&self, notification: Notification) {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(notification);
    }
}

/// Whether notifications may show at all now.
pub(crate) fn allowed(settings: &NotificationSettings, minute_of_day: u16) -> bool {
    settings.enabled && !settings.quiet_hours.contains(minute_of_day)
}

/// Whether a new message is announced by its category (without a rule's override).
fn by_category(settings: &NotificationSettings, message: &ArrivedMessage) -> bool {
    let c = &settings.categories;
    if message.important && c.priority {
        return true;
    }
    match message.category {
        None | Some(MessageCategory::Person) => c.people,
        Some(MessageCategory::Notification) => c.notifications,
        Some(MessageCategory::Newsletter) => c.newsletters,
    }
}

fn sender(message: &ArrivedMessage) -> Option<String> {
    let from = message.from.as_ref()?;
    from.name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .or_else(|| Some(from.email.clone()).filter(|e| !e.is_empty()))
}

/// The notification for new messages of one sync window, `None` if nothing is announced.
/// `messages` with the override of a local rule (`Some(true)`/`Some(false)`).
pub(crate) fn new_mail(
    settings: &NotificationSettings,
    texts: &NotificationTexts,
    minute_of_day: u16,
    messages: &[(ArrivedMessage, Option<bool>)],
) -> Option<Notification> {
    if !allowed(settings, minute_of_day) || !settings.new_mail {
        return None;
    }
    let announced: Vec<&ArrivedMessage> = messages
        .iter()
        .filter(|(m, _)| !settings.muted_accounts.contains(&m.account_id))
        // A rule's decision stands (also for a message the rule marked as read); otherwise
        // unread messages by category.
        .filter(|(m, rule)| rule.unwrap_or_else(|| m.unread && by_category(settings, m)))
        .map(|(m, _)| m)
        .collect();
    let newest = announced.iter().max_by_key(|m| m.id)?;
    let (title, body) = match announced.as_slice() {
        [one] if !settings.hide_content => (
            sender(one).unwrap_or_else(|| texts.new_message.clone()),
            non_empty(&one.subject).unwrap_or_else(|| texts.no_subject.clone()),
        ),
        [_] => (texts.new_message.clone(), String::new()),
        many => {
            let title = texts
                .new_messages
                .replace("{count}", &many.len().to_string());
            let body = if settings.hide_content {
                String::new()
            } else {
                let mut names: Vec<String> = Vec::new();
                for m in many.iter().rev() {
                    if let Some(name) = sender(m)
                        && !names.contains(&name)
                    {
                        names.push(name);
                    }
                }
                names.truncate(3);
                names.join(", ")
            };
            (title, body)
        }
    };
    Some(Notification {
        kind: NotificationKind::NewMail,
        title,
        body,
        account_id: newest.account_id,
        mailbox_id: Some(newest.mailbox_id),
        message_id: Some(newest.id),
    })
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A failed or unclear send.
pub(crate) fn send_failed(
    settings: &NotificationSettings,
    texts: &NotificationTexts,
    minute_of_day: u16,
    account: AccountId,
    subject: &str,
) -> Option<Notification> {
    if !allowed(settings, minute_of_day) || !settings.failed_sends {
        return None;
    }
    Some(Notification {
        kind: NotificationKind::SendFailed,
        title: texts.send_failed.clone(),
        body: if settings.hide_content {
            String::new()
        } else {
            non_empty(subject).unwrap_or_else(|| texts.no_subject.clone())
        },
        account_id: account,
        mailbox_id: None,
        message_id: None,
    })
}

/// A snooze came back.
pub(crate) fn snooze_due(
    settings: &NotificationSettings,
    texts: &NotificationTexts,
    minute_of_day: u16,
    account: AccountId,
    mailbox: Option<MailboxId>,
    message: MessageId,
    subject: &str,
) -> Option<Notification> {
    if !allowed(settings, minute_of_day) || !settings.snoozes {
        return None;
    }
    Some(Notification {
        kind: NotificationKind::SnoozeDue,
        title: texts.snooze_due.clone(),
        body: if settings.hide_content {
            String::new()
        } else {
            non_empty(subject).unwrap_or_else(|| texts.no_subject.clone())
        },
        account_id: account,
        mailbox_id: mailbox,
        message_id: Some(message),
    })
}

#[cfg(test)]
mod tests {
    use mail_model::{Address, QuietHours};

    use super::*;

    fn account(n: u8) -> AccountId {
        format!("00000000-0000-4000-8000-0000000000{n:02}")
            .parse()
            .expect("uuid")
    }

    fn arrived(id: i64, category: Option<MessageCategory>, important: bool) -> ArrivedMessage {
        ArrivedMessage {
            id: MessageId(id),
            mailbox_id: MailboxId(1),
            account_id: account(1),
            unread: true,
            subject: format!("Subject {id}"),
            from: Some(Address {
                name: Some(format!("Sender {id}")),
                email: format!("s{id}@example.org"),
            }),
            to: Vec::new(),
            cc: Vec::new(),
            list_id: None,
            category,
            important,
            message_id_hdr: None,
        }
    }

    const NOON: u16 = 12 * 60;

    #[test]
    fn people_and_priority_by_default() {
        let settings = NotificationSettings::default();
        let texts = NotificationTexts::default();
        let one = |m: ArrivedMessage| new_mail(&settings, &texts, NOON, &[(m, None)]);
        let person = one(arrived(1, Some(MessageCategory::Person), false)).expect("person");
        assert_eq!(
            (person.title.as_str(), person.body.as_str()),
            ("Sender 1", "Subject 1")
        );
        assert_eq!(person.message_id, Some(MessageId(1)));
        assert!(
            one(arrived(2, None, false)).is_some(),
            "unclassified counts as a person"
        );
        assert!(one(arrived(3, Some(MessageCategory::Newsletter), false)).is_none());
        assert!(one(arrived(4, Some(MessageCategory::Notification), false)).is_none());
        assert!(
            one(arrived(5, Some(MessageCategory::Newsletter), true)).is_some(),
            "priority"
        );
        let mut read = arrived(6, Some(MessageCategory::Person), false);
        read.unread = false;
        assert!(one(read).is_none(), "read mail is not announced");
    }

    #[test]
    fn rules_override_categories() {
        let settings = NotificationSettings::default();
        let texts = NotificationTexts::default();
        let news = arrived(1, Some(MessageCategory::Newsletter), false);
        assert!(new_mail(&settings, &texts, NOON, &[(news.clone(), Some(true))]).is_some());
        let person = arrived(2, Some(MessageCategory::Person), false);
        assert!(new_mail(&settings, &texts, NOON, &[(person, Some(false))]).is_none());
    }

    #[test]
    fn several_hidden_muted_quiet() {
        let mut settings = NotificationSettings::default();
        let texts = NotificationTexts::default();
        let batch: Vec<_> = (1..=4)
            .map(|i| (arrived(i, Some(MessageCategory::Person), false), None))
            .collect();
        let many = new_mail(&settings, &texts, NOON, &batch).expect("many");
        assert_eq!(many.title, "4 new messages");
        assert_eq!(many.body, "Sender 4, Sender 3, Sender 2");
        assert_eq!(many.message_id, Some(MessageId(4)), "the newest opens");
        settings.hide_content = true;
        let hidden = new_mail(&settings, &texts, NOON, &batch[..1]).expect("one");
        assert_eq!(
            (hidden.title.as_str(), hidden.body.as_str()),
            ("New message", "")
        );
        assert_eq!(
            new_mail(&settings, &texts, NOON, &batch)
                .expect("many")
                .body,
            ""
        );
        settings.hide_content = false;
        settings.muted_accounts = vec![account(1)];
        assert!(new_mail(&settings, &texts, NOON, &batch).is_none());
        settings.muted_accounts.clear();
        settings.quiet_hours = QuietHours {
            enabled: true,
            start_minute: 22 * 60,
            end_minute: 7 * 60,
        };
        assert!(new_mail(&settings, &texts, 23 * 60, &batch).is_none());
        assert!(new_mail(&settings, &texts, 6 * 60, &batch).is_none());
        assert!(new_mail(&settings, &texts, 7 * 60, &batch).is_some());
        assert!(send_failed(&settings, &texts, 23 * 60, account(1), "x").is_none());
        assert!(
            snooze_due(
                &settings,
                &texts,
                23 * 60,
                account(1),
                None,
                MessageId(1),
                "x"
            )
            .is_none()
        );
        settings.enabled = false;
        assert!(new_mail(&settings, &texts, NOON, &batch).is_none());
        assert!(send_failed(&settings, &texts, NOON, account(1), "x").is_none());
    }

    #[test]
    fn sends_and_snoozes() {
        let mut settings = NotificationSettings::default();
        let texts = NotificationTexts {
            send_failed: "Nicht gesendet".into(),
            no_subject: "(kein Betreff)".into(),
            ..NotificationTexts::default()
        };
        let failed = send_failed(&settings, &texts, NOON, account(1), " ").expect("failed");
        assert_eq!(
            (failed.title.as_str(), failed.body.as_str()),
            ("Nicht gesendet", "(kein Betreff)")
        );
        let due = snooze_due(
            &settings,
            &texts,
            NOON,
            account(1),
            Some(MailboxId(2)),
            MessageId(9),
            "Hi",
        )
        .expect("due");
        assert_eq!(due.kind, NotificationKind::SnoozeDue);
        assert_eq!(
            (due.mailbox_id, due.message_id),
            (Some(MailboxId(2)), Some(MessageId(9)))
        );
        settings.failed_sends = false;
        settings.snoozes = false;
        assert!(send_failed(&settings, &texts, NOON, account(1), "x").is_none());
        assert!(snooze_due(&settings, &texts, NOON, account(1), None, MessageId(1), "x").is_none());
    }
}
