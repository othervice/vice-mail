//! Stage 19, block C (decision 0057): a folder exported as an mbox file, and an mbox file
//! imported into a folder of an account through the outbox (`append`), as background jobs with
//! progress and cancel.

use serde::{Deserialize, Serialize};

use crate::{AccountId, IpcError, MailboxId};

/// Most messages one job exports or imports.
pub const MAX_MBOX_JOB_MESSAGES: u32 = 200_000;

/// An import keeps at most this many of its appends waiting in the outbox at once (and their
/// frozen copies on disk); it reads further once the server took some.
pub const MBOX_IMPORT_WINDOW: u32 = 50;

/// A job's id: 32 lowercase hex digits from the operating system's random source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct MboxJobId(String);

impl MboxJobId {
    /// `None` unless `text` is 32 lowercase hex digits.
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == 32
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(text.to_owned()))
    }

    /// From 128 random bits (the engine draws them).
    pub fn from_random(bits: u128) -> Self {
        Self(format!("{bits:032x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MboxJobKind {
    Export,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MboxJobState {
    Running,
    Done,
    Cancelled,
    Failed,
}

/// A job as the UI shows it; every change comes as `mboxJobChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MboxJob {
    pub id: MboxJobId,
    pub kind: MboxJobKind,
    pub account_id: AccountId,
    pub mailbox_id: MailboxId,
    /// The folder's decoded path.
    pub mailbox_name: String,
    /// The file's name (never its folder).
    pub file_name: String,
    pub state: MboxJobState,
    /// Export: the messages of the folder; import: unknown until the file was read (null).
    pub total: Option<u32>,
    /// Export: messages written; import: messages the server took.
    pub done: u32,
    /// Import: appends waiting in the outbox.
    pub queued: u32,
    /// Messages left out: no longer on the server (export), marked deleted by Thunderbird, too
    /// large, or refused by the server (import).
    pub skipped: u32,
    /// Why it failed (`failed`).
    pub error: Option<IpcError>,
}

/// What starting a job answered: the user closed the dialog, or the job runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MboxStart {
    Cancelled,
    Started { job: MboxJob },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_are_32_hex_digits() {
        let id = MboxJobId::from_random(0xfeed);
        assert_eq!(id.as_str(), "0000000000000000000000000000feed");
        assert_eq!(MboxJobId::parse(id.as_str()), Some(id));
        assert!(MboxJobId::parse("../x").is_none());
        assert!(MboxJobId::parse(&"F".repeat(32)).is_none());
    }
}
