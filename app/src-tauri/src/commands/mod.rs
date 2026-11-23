//! Commands exposed to the UI. Each one only forwards to [`mail_engine::AppCore`] and turns a
//! [`CoreError`] into the typed [`IpcError`]. No logic lives here.

pub mod accounts;
pub mod actions;
pub mod app;
pub mod groups;
pub mod mail;
pub mod preferences;
pub mod reading;
pub mod search;
pub mod threads;

use mail_model::{CoreError, IpcError};

/// Logs a failed command once, at the bridge, and converts the error for the UI.
fn to_ipc(command: &'static str) -> impl FnOnce(CoreError) -> IpcError {
    move |err| {
        tracing::warn!(command, code = ?err.code(), error = %err, "command failed");
        IpcError::from(err)
    }
}
