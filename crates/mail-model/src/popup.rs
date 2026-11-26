//! Stage 18 (decision 0051): the app's own popup for new mail at the bottom right of the screen.
//! Types only: the engine keeps the cards (`mail_engine::comfort::popup`), the shell shows them
//! in a window of their own, the UI draws them.
//!
//! The core decides *whether* a message is announced exactly as for system notifications
//! (first sync, muted accounts, quiet hours, categories, local rules); the popup is only another
//! way to show it, with one card per message.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{
    AccentTheme, AccountColor, AccountId, LanguagePreference, MailboxId, MessageId, ThemePreference,
};

/// Cards the popup shows at once, newest on top; the rest are one line "N more".
pub const MAX_POPUP_CARDS: usize = 3;

/// Cards the core keeps behind the visible ones. Older ones are dropped: their messages are in
/// the inbox anyway, and the popup stays small.
pub const MAX_POPUP_QUEUE: usize = 50;

/// What a card says about its message. Absent when the notification settings hide the content
/// (the card then says "New message" and names the account).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMailContent {
    /// Display name of the first `From` address; null without one.
    pub sender_name: Option<String>,
    /// Its address; null for a message without a sender.
    pub sender_email: Option<String>,
    /// Empty without a subject.
    pub subject: String,
    /// The start of the text as plain text (the list preview, at most 256 characters); null
    /// until the body is stored (the core fills it in as soon as the prefetch has it).
    pub snippet: Option<String>,
}

/// The account of a card, named only when the app has more than one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMailAccount {
    /// The display name, else the address.
    pub name: String,
    pub color: AccountColor,
}

/// One card of the popup: one new message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMailCard {
    pub account_id: AccountId,
    /// The inbox the message arrived in ("Read" opens it there).
    pub mailbox_id: MailboxId,
    pub message_id: MessageId,
    /// Sort date of the message in Unix milliseconds.
    #[specta(type = Number)]
    pub date: i64,
    /// Null when the content is hidden: then only "Read" is offered.
    pub content: Option<NewMailContent>,
    /// Null with a single account.
    pub account: Option<NewMailAccount>,
    /// `false` for a read-only account: "Delete" is disabled and says why.
    pub writes_enabled: bool,
    /// Deleted from the popup (through the outbox into the trash): the card shows "Deleted"
    /// with "Undo" for a few seconds.
    pub deleted: bool,
}

/// How the popup looks: the same theme, language and accent as the app's windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PopupAppearance {
    pub theme: ThemePreference,
    pub language: LanguagePreference,
    pub accent: AccentTheme,
}

/// The popup as the UI draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMailPopup {
    /// At most [`MAX_POPUP_CARDS`], newest first. Empty: the popup closes.
    pub cards: Vec<NewMailCard>,
    /// Cards waiting behind the visible ones ("N more" opens the inbox).
    pub more: u32,
    pub appearance: PopupAppearance,
    /// Grows with every change of the cards. The popup hears of a change twice (the event and
    /// the answer of its own command) and keeps the newest, whatever arrives last.
    #[specta(type = Number)]
    pub revision: u64,
}

impl NewMailPopup {
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format() {
        let account: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let popup = NewMailPopup {
            cards: vec![NewMailCard {
                account_id: account,
                mailbox_id: MailboxId(3),
                message_id: MessageId(42),
                date: 1_764_000_000_000,
                content: Some(NewMailContent {
                    sender_name: Some("Bob".into()),
                    sender_email: Some("bob@example.org".into()),
                    subject: "Lunch".into(),
                    snippet: None,
                }),
                account: None,
                writes_enabled: false,
                deleted: false,
            }],
            more: 2,
            appearance: PopupAppearance {
                theme: ThemePreference::Dark,
                language: LanguagePreference::De,
                accent: AccentTheme::Orange,
            },
            revision: 7,
        };
        assert_eq!(
            serde_json::to_value(&popup).expect("serialize"),
            json!({
                "cards": [{
                    "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                    "mailboxId": 3, "messageId": 42, "date": 1_764_000_000_000_i64,
                    "content": { "senderName": "Bob", "senderEmail": "bob@example.org",
                                 "subject": "Lunch", "snippet": null },
                    "account": null, "writesEnabled": false, "deleted": false
                }],
                "more": 2,
                "appearance": { "theme": "dark", "language": "de", "accent": "orange" },
                "revision": 7
            })
        );
    }
}
