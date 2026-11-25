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
//! - Quiet hours drop notifications (they are not delivered later), and so does a pause (stage 21,
//!   decision 0069: [`crate::AppCore::pause_notifications`]), checked by the callers.
//! - **Sound** (stage 16): each notification carries the sound the settings choose for its
//!   account on this platform (`NotificationSound::for_account`), or none (sounds off, a
//!   silent account). No notification, no sound: quiet hours and muted accounts drop both.
//! - **Popup** (stage 18, decision 0051): with `NewMailStyle::Popup` (the default) new mail is
//!   not a system notification but cards in the app's popup ([`super::popup`]). Which messages
//!   are announced is decided by [`announced`] for both; the sink then receives a
//!   [`PopupUpdate`] (with the sound of the sync window) instead of a [`Notification`]. Failed
//!   sends and returned snoozes stay system notifications.

use std::sync::{Mutex, PoisonError};

use mail_model::{
    AccountId, MailboxId, MessageCategory, MessageId, NewMailPopup, NotificationSettings,
    NotificationTexts, SoundPlatform,
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
    /// Stage 16: the sound to play, a name of this platform's list (`MACOS_SOUNDS`,
    /// `WINDOWS_SOUNDS`; `Default` is the system's notification sound); `None` = silent.
    pub sound: Option<String>,
}

/// Stage 18: the new-mail popup changed. The shell opens its window for cards, updates it, or
/// closes it when there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupUpdate {
    pub popup: NewMailPopup,
    /// New messages arrived: the sound of their sync window (as a system notification would
    /// carry it, see [`Notification::sound`]); `None` for every other change and for silence.
    pub sound: Option<String>,
}

/// The sound of a notification about `account` on this platform.
pub(crate) fn sound(settings: &NotificationSettings, account: AccountId) -> Option<String> {
    settings
        .sound
        .for_account(account, SoundPlatform::current())
}

/// Shows notifications. The shell's implementation posts native ones and owns the popup's
/// window; it must not block.
pub trait NotificationSink: Send + Sync + 'static {
    fn show(&self, notification: Notification);
    /// Stage 18: the new-mail popup changed (see [`PopupUpdate`]).
    fn popup(&self, update: PopupUpdate);
}

/// Records notifications and popup updates instead of showing them (tests).
#[derive(Debug, Default)]
pub struct RecordingSink {
    shown: Mutex<Vec<Notification>>,
    popups: Mutex<Vec<PopupUpdate>>,
}

impl RecordingSink {
    pub fn shown(&self) -> Vec<Notification> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every popup update so far, oldest first.
    pub fn popups(&self) -> Vec<PopupUpdate> {
        self.popups
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

    fn popup(&self, update: PopupUpdate) {
        self.popups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(update);
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

/// The new messages of one sync window that are announced (as a notification or popup cards),
/// in the order given; empty when none is. `messages` with the override of a local rule
/// (`Some(true)`/`Some(false)`).
pub(crate) fn announced<'a>(
    settings: &NotificationSettings,
    minute_of_day: u16,
    messages: &'a [(ArrivedMessage, Option<bool>)],
) -> Vec<&'a ArrivedMessage> {
    if !allowed(settings, minute_of_day) || !settings.new_mail {
        return Vec::new();
    }
    messages
        .iter()
        .filter(|(m, _)| !settings.muted_accounts.contains(&m.account_id))
        // A rule's decision stands (also for a message the rule marked as read); otherwise
        // unread messages by category.
        .filter(|(m, rule)| rule.unwrap_or_else(|| m.unread && by_category(settings, m)))
        .map(|(m, _)| m)
        .collect()
}

/// The system notification for new messages of one sync window, `None` if nothing is
/// announced. `messages` with the override of a local rule (`Some(true)`/`Some(false)`).
pub(crate) fn new_mail(
    settings: &NotificationSettings,
    texts: &NotificationTexts,
    minute_of_day: u16,
    messages: &[(ArrivedMessage, Option<bool>)],
) -> Option<Notification> {
    let announced = announced(settings, minute_of_day, messages);
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
        sound: sound(settings, newest.account_id),
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
        sound: sound(settings, account),
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
        sound: sound(settings, account),
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

    /// Stage 18: the popup announces exactly the messages a system notification would cover.
    #[test]
    fn the_popup_announces_what_a_notification_would() {
        let mut settings = NotificationSettings::default();
        let mut read = arrived(3, Some(MessageCategory::Person), false);
        read.unread = false;
        let batch = vec![
            (arrived(1, Some(MessageCategory::Person), false), None),
            (arrived(2, Some(MessageCategory::Newsletter), false), None),
            (read, None),
            (
                arrived(4, Some(MessageCategory::Newsletter), false),
                Some(true),
            ),
            (
                arrived(5, Some(MessageCategory::Person), false),
                Some(false),
            ),
            (arrived(6, Some(MessageCategory::Notification), true), None),
        ];
        let ids = |s: &NotificationSettings, minute| {
            announced(s, minute, &batch)
                .iter()
                .map(|m| m.id.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&settings, NOON), [1, 4, 6]);
        let texts = NotificationTexts::default();
        let note = new_mail(&settings, &texts, NOON, &batch).expect("notification");
        assert_eq!(note.title, "3 new messages");
        settings.muted_accounts = vec![account(1)];
        assert!(ids(&settings, NOON).is_empty());
        settings.muted_accounts.clear();
        settings.new_mail = false;
        assert!(ids(&settings, NOON).is_empty());
        settings.new_mail = true;
        settings.quiet_hours = QuietHours {
            enabled: true,
            start_minute: 11 * 60,
            end_minute: 13 * 60,
        };
        assert!(ids(&settings, NOON).is_empty());
        settings.quiet_hours.enabled = false;
        settings.enabled = false;
        assert!(ids(&settings, NOON).is_empty());
    }

    /// Stage 18: settings stored before the popup existed show it; the style round-trips.
    #[test]
    fn the_popup_is_the_default_style() {
        let old: NotificationSettings =
            serde_json::from_str(r#"{"enabled":true,"newMail":true,"hideContent":true}"#)
                .expect("old settings");
        assert_eq!(old.new_mail_style, mail_model::NewMailStyle::Popup);
        assert!(old.hide_content);
        let system = NotificationSettings {
            new_mail_style: mail_model::NewMailStyle::System,
            ..NotificationSettings::default()
        };
        let json = serde_json::to_string(&system).expect("json");
        assert!(json.contains(r#""newMailStyle":"system""#), "{json}");
        let back: NotificationSettings = serde_json::from_str(&json).expect("back");
        assert_eq!(back, system);
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

    /// Stage 16: every notification carries its sound; off, a silent account or quiet hours
    /// mean none (quiet hours: no notification at all).
    #[test]
    fn notifications_carry_the_sound_of_their_account() {
        let mut settings = NotificationSettings::default();
        settings.sound.macos = "Glass".into();
        settings.sound.windows = "Mail".into();
        let texts = NotificationTexts::default();
        let expected = match SoundPlatform::current() {
            SoundPlatform::Macos => "Glass",
            SoundPlatform::Windows => "Mail",
            SoundPlatform::Other => "Default",
        };
        let batch = [(arrived(1, Some(MessageCategory::Person), false), None)];
        let mail = new_mail(&settings, &texts, NOON, &batch).expect("mail");
        assert_eq!(mail.sound.as_deref(), Some(expected));
        let failed = send_failed(&settings, &texts, NOON, account(1), "x").expect("failed");
        assert_eq!(failed.sound.as_deref(), Some(expected));
        let due =
            snooze_due(&settings, &texts, NOON, account(1), None, MessageId(1), "x").expect("due");
        assert_eq!(due.sound.as_deref(), Some(expected));

        // A silent account still notifies, without sound; another account keeps its sound.
        settings.sound.silent_accounts = vec![account(1)];
        let mail = new_mail(&settings, &texts, NOON, &batch).expect("silent");
        assert_eq!(mail.sound, None);
        assert_eq!(
            send_failed(&settings, &texts, NOON, account(2), "x")
                .expect("other")
                .sound
                .as_deref(),
            Some(expected)
        );
        settings.sound.silent_accounts.clear();

        // An unknown name (from a newer version) plays the default sound.
        settings.sound.macos = "Fanfare".into();
        settings.sound.windows = "Fanfare".into();
        assert_eq!(
            new_mail(&settings, &texts, NOON, &batch)
                .expect("mail")
                .sound
                .as_deref(),
            Some("Default")
        );

        // Sounds off: silent notifications.
        settings.sound.enabled = false;
        assert_eq!(
            new_mail(&settings, &texts, NOON, &batch)
                .expect("mail")
                .sound,
            None
        );

        // Quiet hours and muted accounts: no notification, so no sound.
        settings.sound.enabled = true;
        settings.quiet_hours = QuietHours {
            enabled: true,
            start_minute: 22 * 60,
            end_minute: 7 * 60,
        };
        assert!(new_mail(&settings, &texts, 23 * 60, &batch).is_none());
        settings.quiet_hours.enabled = false;
        settings.muted_accounts = vec![account(1)];
        assert!(new_mail(&settings, &texts, NOON, &batch).is_none());
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
