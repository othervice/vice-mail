//! Stage 20, task B (decision 0059): reminders when no reply comes ("Remind me if no reply by
//! …"). Local like snoozes (by account UUID and Message-ID), carried by the settings sync like
//! snoozes, without AI: a message of the conversation from someone else after the sending is
//! the answer and ends the reminder.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

/// A reminder on a list row (a message the user sent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpMark {
    /// When the reminder comes (or came) without an answer, Unix milliseconds.
    #[specta(type = Number)]
    pub due: i64,
    /// When the message was sent ("No reply since …"), Unix milliseconds.
    #[specta(type = Number)]
    pub sent_at: i64,
    /// Due without an answer: it stands above the inbox until the user clears it or an answer
    /// comes.
    pub returned: bool,
}

/// When to remind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FollowUpChoice {
    /// Tomorrow at 08:00.
    Tomorrow,
    /// In three days at 08:00.
    InThreeDays,
    /// Next Monday at 08:00.
    NextWeek,
    /// A time the user picked, Unix milliseconds; at least a minute ahead, at most 5 years.
    At {
        #[specta(type = Number)]
        at: i64,
    },
}

/// The times the reminder menu offers right now, Unix milliseconds, in the user's zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpTimes {
    #[specta(type = Number)]
    pub tomorrow: i64,
    #[specta(type = Number)]
    pub in_three_days: i64,
    #[specta(type = Number)]
    pub next_week: i64,
}

/// What `setFollowUp` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpResult {
    /// When the reminder comes if no reply arrives, Unix milliseconds.
    #[specta(type = Number)]
    pub due: i64,
    /// Messages whose conversation already has a reply: they keep no reminder.
    pub answered: u32,
}

/// Reminders shown above the inbox at most.
pub const MAX_TOP_FOLLOW_UPS: u32 = 100;
