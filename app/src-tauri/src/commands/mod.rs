//! Commands exposed to the UI. Each one only forwards to [`mail_engine::AppCore`] and turns a
//! [`CoreError`] into the typed [`IpcError`]. No logic lives here.

pub mod accounts;
pub mod actions;
pub mod ai;
pub mod app;
pub mod comfort;
pub mod compose;
pub mod composer;
pub mod contacts;
pub mod eml;
pub mod groups;
pub mod mail;
pub mod mcp;
pub mod parity;
pub mod preferences;
pub mod reading;
pub mod search;
pub mod settings_sync;
pub mod snippets;
pub mod threads;
pub mod ui;
pub mod updates;

use mail_model::{CoreError, IpcError};

/// Logs a failed command once, at the bridge, and converts the error for the UI. The log line
/// holds the code and a fixed summary, never the error text with its paths or addresses
/// ([`mail_engine::logging::command_failed`], finding V-07).
fn to_ipc(command: &'static str) -> impl FnOnce(CoreError) -> IpcError {
    move |err| {
        mail_engine::logging::command_failed(command, &err);
        IpcError::from(err)
    }
}
