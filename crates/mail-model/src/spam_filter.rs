//! Stage 22 (decision 0077): the learning spam filter's types on the IPC bridge.
//!
//! The filter learns on this device from the user's "Spam" and "Not spam" and, when asked, from
//! the existing junk folder and read inbox mail; it looks at new mail in an inbox while the app
//! runs. Above [`SPAM_THRESHOLD`] a mail goes to the junk folder through the outbox where the
//! account may write and has one, otherwise it is only marked locally as suspected spam.

use serde::{Deserialize, Serialize};

use crate::{AccountId, IpcError, MailboxId, MessageId};

/// Score from which the filter acts (decision 0077: conservative, measured on a corpus).
pub const SPAM_THRESHOLD: f64 = 0.99;
/// The filter acts only after it learned at least this many spam mails …
pub const MIN_LEARNED_SPAM: u32 = 50;
/// … and this many other mails.
pub const MIN_LEARNED_HAM: u32 = 200;
/// Learning from existing mail takes at most this many spam mails (newest of the junk folders)
/// …
pub const MAX_LEARN_SPAM: u32 = 500;
/// … and at most this many other mails (read inbox mail, replied first).
pub const MAX_LEARN_HAM: u32 = 1_000;
/// Most accounts the settings may exclude.
pub const MAX_SPAM_EXCLUDED_ACCOUNTS: usize = 100;
/// Most messages one local "spam" / "not spam" covers.
pub const MAX_SPAM_FEEDBACK_MESSAGES: usize = 500;

/// The setting `spam.filter` (global, synchronised like other settings; the learned data is
/// not). Off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamFilterSettings {
    pub enabled: bool,
    /// Accounts the filter leaves alone (it neither looks at their new mail nor learns from
    /// them).
    #[serde(default)]
    pub excluded_accounts: Vec<AccountId>,
}

impl SpamFilterSettings {
    /// Whether the filter looks at this account's mail.
    pub fn applies_to(&self, account: AccountId) -> bool {
        self.enabled && !self.excluded_accounts.contains(&account)
    }
}

/// What the filter learned and does, for Settings (`getSpamFilter`, event
/// `spamFilterChanged`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamFilterStatus {
    pub settings: SpamFilterSettings,
    /// Mails learned as spam on this device.
    pub learned_spam: u32,
    /// Mails learned as not spam on this device.
    pub learned_ham: u32,
    /// Distinct tokens the filter knows.
    pub tokens: u32,
    /// It learned enough to act ([`MIN_LEARNED_SPAM`], [`MIN_LEARNED_HAM`]).
    pub ready: bool,
    pub min_spam: u32,
    pub min_ham: u32,
    /// Learning from existing mail: running, or how the last run ended (this session).
    pub learning: Option<SpamLearning>,
}

/// Learning from existing mail (`learnSpamFromMail`), as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamLearning {
    pub state: SpamLearningState,
    /// Mails looked at so far.
    pub done: u32,
    /// Mails chosen to learn from.
    pub total: u32,
    /// Learned as spam and as not spam in this run.
    pub spam: u32,
    pub ham: u32,
    /// Mails left out: their content could not be read, or (not spam) the filter already
    /// thought them spam.
    pub skipped: u32,
    /// Why it failed (`failed`).
    pub error: Option<IpcError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SpamLearningState {
    Running,
    Done,
    Cancelled,
    Failed,
}

/// The filter's mark on a list row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SpamFilterMark {
    /// Suspected spam, marked only on this device (the account may not write or has no junk
    /// folder).
    Suspected,
    /// Moved to the junk folder by the filter.
    Moved,
}

/// The filter's verdict on one message, for the reading pane's note (`getSpamVerdict`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpamVerdict {
    pub message_id: MessageId,
    /// Where the note's buttons act on it: its location in the junk folder for a moved mail,
    /// else in the inbox, else any; null when it lies nowhere (moving).
    pub mailbox_id: Option<MailboxId>,
    pub mark: SpamFilterMark,
    /// 0 to 1.
    pub score: f64,
    /// Unix ms.
    #[specta(type = specta_typescript::Number)]
    pub at: i64,
    /// "Move to spam" can run (writes on, a junk folder): for a suspected mail.
    pub can_move: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_off_and_apply_per_account() {
        let account = AccountId::from_uuid(uuid::Uuid::from_u128(1));
        let other = AccountId::from_uuid(uuid::Uuid::from_u128(2));
        let mut settings = SpamFilterSettings::default();
        assert!(!settings.applies_to(account));
        settings.enabled = true;
        assert!(settings.applies_to(account));
        settings.excluded_accounts.push(account);
        assert!(!settings.applies_to(account));
        assert!(settings.applies_to(other));
    }

    #[test]
    fn settings_read_without_the_excluded_list() {
        let settings: SpamFilterSettings =
            serde_json::from_str(r#"{"enabled":true}"#).expect("json");
        assert!(settings.enabled);
        assert!(settings.excluded_accounts.is_empty());
        let json = serde_json::to_value(&settings).expect("json");
        assert_eq!(json["excludedAccounts"], serde_json::json!([]));
    }
}
