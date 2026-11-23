//! "Export diagnostics" (stage 15): the result the UI sees and the one typed failure.
//!
//! The core writes a plain-text file the user picked in the system's save dialog: app and
//! system facts, counts from the database, the accounts' kinds and states (no addresses, user
//! names or server names), settings that are not personal and the tail of the recent log files,
//! redacted once more. What it contains and what it never contains is listed in
//! `docs/PRIVACY.md` ("Diagnostics").

use serde::{Deserialize, Serialize};

use crate::ErrorCode;

/// Result of `exportDiagnostics`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DiagnosticsExportResult {
    /// Written to the path the user chose in the system dialog.
    Saved { path: String },
    /// The user closed the dialog; nothing was written.
    Cancelled,
}

/// Why the diagnostics file could not be written. Collecting never fails: a part that cannot
/// be read (for example the database) is described in the file instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiagnosticsError {
    /// The chosen file could not be written (no permission, disk full, folder gone). Nothing
    /// was left behind.
    #[error("the diagnostics file could not be written: {0}")]
    WriteFailed(String),
}

impl DiagnosticsError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::WriteFailed(_) => ErrorCode::DiagnosticsWriteFailed,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::WriteFailed(_) => "The diagnostics file could not be saved.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::WriteFailed(d) => (!d.is_empty()).then(|| d.clone()),
        }
    }
}
