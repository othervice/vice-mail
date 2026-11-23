//! Stage 15 (decision 0041): the guided first start (welcome, account, appearance, optional
//! extras, done). The core only remembers whether it still has to be shown; the steps
//! themselves use the commands they always use (accounts, preferences, updates, settings sync).

use serde::{Deserialize, Serialize};

/// How the guided first start ended. Stored per device (setting `first_start`, never synced:
/// accounts and the update question belong to this computer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum FirstStartOutcome {
    /// Walked through to the last step.
    Completed,
    /// "Skip setup" on one of the steps.
    Skipped,
    /// The installation already had an account when the core was first asked (it predates the
    /// first start), so it was never shown.
    NotNeeded,
}

/// What the UI may end the first start with (`finishFirstStart`). `NotNeeded` is the core's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum FirstStartEnd {
    Completed,
    Skipped,
}

impl From<FirstStartEnd> for FirstStartOutcome {
    fn from(end: FirstStartEnd) -> Self {
        match end {
            FirstStartEnd::Completed => Self::Completed,
            FirstStartEnd::Skipped => Self::Skipped,
        }
    }
}

/// `getFirstStart` and `finishFirstStart`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FirstStart {
    /// Show the guided first start now: it never ended and there is no account yet.
    pub show: bool,
    /// How it ended; `None` while it is still to be shown.
    pub outcome: Option<FirstStartOutcome>,
}

impl FirstStart {
    /// Still to be shown.
    pub const PENDING: Self = Self {
        show: true,
        outcome: None,
    };

    /// Ended (or never needed) with `outcome`.
    pub fn ended(outcome: FirstStartOutcome) -> Self {
        Self {
            show: false,
            outcome: Some(outcome),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_is_camel_case() {
        let json =
            serde_json::to_string(&FirstStart::ended(FirstStartOutcome::NotNeeded)).expect("json");
        assert_eq!(json, r#"{"show":false,"outcome":"notNeeded"}"#);
        let json = serde_json::to_string(&FirstStart::PENDING).expect("json");
        assert_eq!(json, r#"{"show":true,"outcome":null}"#);
        let end: FirstStartEnd = serde_json::from_str(r#""skipped""#).expect("end");
        assert_eq!(FirstStartOutcome::from(end), FirstStartOutcome::Skipped);
        assert!(serde_json::from_str::<FirstStartEnd>(r#""notNeeded""#).is_err());
    }
}
