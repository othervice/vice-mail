//! In-app updates (stage 15, decision 0037) as the UI sees them: the device setting
//! "check automatically", the one-time question, the state of the current check, download or
//! install, and the typed errors of each step.
//!
//! The check is the only connection to a server the user did not set up (the project's release
//! page), so it is opt-in: nothing is requested while the setting is off, except after a click on
//! "Check now". Nothing is downloaded or installed without a click.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{ErrorCode, IpcError};

/// The shortest time between two automatic checks: one day.
pub const UPDATE_CHECK_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// The stored device setting (key `updates`, never synced to other devices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// "Check for updates automatically". Off until the user turns it on.
    pub auto_check: bool,
    /// The one-time question was answered (either way) or the switch was changed; it is not
    /// asked again.
    pub consent_answered: bool,
    /// Unix ms of the last check that reached for the network (automatic or manual, whatever
    /// its outcome); the next automatic one waits [`UPDATE_CHECK_INTERVAL_MS`] after it.
    pub last_check_ms: Option<i64>,
}

impl UpdateSettings {
    /// Whether an automatic check may run now: only with the switch on, and at most once per
    /// [`UPDATE_CHECK_INTERVAL_MS`]. A last check in the future (the clock was set back) counts
    /// as due, so a wrong clock cannot stop checks for good.
    pub fn automatic_check_due(&self, now_ms: i64) -> bool {
        if !self.auto_check {
            return false;
        }
        match self.last_check_ms {
            None => true,
            Some(last) => now_ms < last || now_ms.saturating_sub(last) >= UPDATE_CHECK_INTERVAL_MS,
        }
    }
}

/// `getUpdateInfo` and the update commands: the setting, whether to ask, and the state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// The running version.
    pub current_version: String,
    /// This build can check for and install updates (false in development builds and builds
    /// without the updater, where the About page only shows the version).
    pub supported: bool,
    pub auto_check: bool,
    /// Show the one-time question now: not answered yet, and at least one account is set up.
    pub ask_consent: bool,
    /// Unix ms of the last check, if any.
    #[specta(type = Option<Number>)]
    pub last_check_ms: Option<i64>,
    pub status: UpdateStatus,
}

/// Where the update is. Only ever changes on the core's side; the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateStatus {
    /// No check since the app started.
    Idle,
    /// A check is running.
    Checking,
    /// The last check found no newer version.
    UpToDate {
        #[specta(type = Number)]
        checked_at_ms: i64,
    },
    /// A newer version is published; nothing was downloaded yet.
    Available { update: AvailableUpdate },
    /// The download runs; `total` is unknown if the server does not say.
    Downloading {
        version: String,
        #[specta(type = Number)]
        downloaded: u64,
        #[specta(type = Option<Number>)]
        total: Option<u64>,
    },
    /// Downloaded and its signature checked; installs with the user's "Restart to install".
    Ready { version: String },
    /// Installing; the app restarts (macOS) or the installer takes over (Windows).
    Installing { version: String },
    /// The last step failed. `version` is the update it concerned, if one was found.
    Failed {
        version: Option<String>,
        error: IpcError,
    },
}

/// A newer version announced by the update manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    /// Release notes as plain text (the UI never renders them as HTML or Markdown).
    pub notes: Option<String>,
    /// Publication date, RFC 3339, if the manifest has one.
    pub pub_date: Option<String>,
}

/// Longest release notes the core passes on; the rest is cut at a character boundary.
pub const MAX_UPDATE_NOTES_CHARS: usize = 4_000;

/// An update step failed. Texts never contain secrets; details are technical and in English.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UpdateError {
    /// The update manifest could not be fetched or read (offline, server error, no entry for
    /// this system, a download address outside the allowed hosts).
    #[error("the update check failed: {0}")]
    CheckFailed(String),
    /// The update file could not be downloaded completely.
    #[error("the update download failed: {0}")]
    DownloadFailed(String),
    /// The downloaded file's signature does not match the update key, or it was signed for
    /// another version than announced: it was discarded and nothing was installed.
    #[error("the update's signature is not valid: {0}")]
    SignatureInvalid(String),
    /// Installing failed; the installed version is unchanged.
    #[error("the update could not be installed: {0}")]
    InstallFailed(String),
    /// Download or install was asked for, but no update is available or ready (check first),
    /// or this build cannot update itself.
    #[error("no update is ready for this step: {0}")]
    NotReady(String),
}

impl UpdateError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::CheckFailed(_) => ErrorCode::UpdateCheckFailed,
            Self::DownloadFailed(_) => ErrorCode::UpdateDownloadFailed,
            Self::SignatureInvalid(_) => ErrorCode::UpdateSignatureInvalid,
            Self::InstallFailed(_) => ErrorCode::UpdateInstallFailed,
            Self::NotReady(_) => ErrorCode::UpdateNotReady,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::CheckFailed(_) => "The app could not check for updates.",
            Self::DownloadFailed(_) => "The update could not be downloaded.",
            Self::SignatureInvalid(_) => {
                "The update could not be verified and was discarded; nothing was installed."
            }
            Self::InstallFailed(_) => "The update could not be installed.",
            Self::NotReady(_) => "There is no update ready for this step.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::CheckFailed(d)
            | Self::DownloadFailed(d)
            | Self::SignatureInvalid(d)
            | Self::InstallFailed(d)
            | Self::NotReady(d) => (!d.is_empty()).then(|| d.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = UPDATE_CHECK_INTERVAL_MS;

    #[test]
    fn automatic_checks_need_the_switch_and_wait_a_day() {
        let now = 1_800_000_000_000;
        let off = UpdateSettings::default();
        assert!(!off.automatic_check_due(now), "off by default");
        let never = UpdateSettings {
            last_check_ms: None,
            ..UpdateSettings::default()
        };
        assert!(!never.automatic_check_due(now));

        let on = UpdateSettings {
            auto_check: true,
            ..UpdateSettings::default()
        };
        assert!(on.automatic_check_due(now), "never checked");
        let recent = UpdateSettings {
            last_check_ms: Some(now - DAY + 1),
            ..on
        };
        assert!(!recent.automatic_check_due(now), "less than a day ago");
        let old = UpdateSettings {
            last_check_ms: Some(now - DAY),
            ..on
        };
        assert!(old.automatic_check_due(now), "a day ago");
        let future = UpdateSettings {
            last_check_ms: Some(now + 1),
            ..on
        };
        assert!(future.automatic_check_due(now), "the clock went back");
    }

    #[test]
    fn settings_read_missing_fields_as_defaults() {
        let parsed: UpdateSettings = serde_json::from_str("{}").expect("parse");
        assert_eq!(parsed, UpdateSettings::default());
        let parsed: UpdateSettings =
            serde_json::from_str(r#"{"autoCheck":true,"lastCheckMs":5}"#).expect("parse");
        assert!(parsed.auto_check && !parsed.consent_answered);
        assert_eq!(parsed.last_check_ms, Some(5));
    }

    #[test]
    fn errors_map_to_codes() {
        let cases = [
            (
                UpdateError::CheckFailed("x".into()),
                ErrorCode::UpdateCheckFailed,
            ),
            (
                UpdateError::DownloadFailed("x".into()),
                ErrorCode::UpdateDownloadFailed,
            ),
            (
                UpdateError::SignatureInvalid("x".into()),
                ErrorCode::UpdateSignatureInvalid,
            ),
            (
                UpdateError::InstallFailed("x".into()),
                ErrorCode::UpdateInstallFailed,
            ),
            (UpdateError::NotReady("x".into()), ErrorCode::UpdateNotReady),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), code);
            let core = crate::CoreError::from(err.clone());
            assert_eq!(core.code(), code);
            assert_eq!(core.message(), err.message());
            assert_eq!(core.detail().as_deref(), Some("x"));
        }
        let status = UpdateStatus::Failed {
            version: Some("1.2.3".into()),
            error: IpcError::from(crate::CoreError::from(UpdateError::SignatureInvalid(
                "bad".into(),
            ))),
        };
        let json = serde_json::to_value(&status).expect("serialize");
        assert_eq!(json["state"], "failed");
        assert_eq!(json["error"]["code"], "update_signature_invalid");
        let json =
            serde_json::to_value(UpdateStatus::UpToDate { checked_at_ms: 7 }).expect("serialize");
        assert_eq!(json["state"], "upToDate");
        assert_eq!(json["checkedAtMs"], 7);
    }
}
