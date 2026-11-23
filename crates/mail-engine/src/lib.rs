//! Core of the mail client. The shell talks to [`AppCore`] only; it receives the typed event
//! stream through [`AppCore::subscribe`]. Everything here runs and is tested without a WebView.

pub mod clock;
pub mod comfort;
pub mod compose;
mod core;
pub mod credentials;
pub mod discovery;
pub mod logging;
pub mod outbox;
pub mod reading;
pub mod search;
pub mod settings_sync;
pub mod sync;
pub mod sync_state;

pub use crate::clock::{Clock, SystemClock};
pub use crate::core::{AppCore, BuildInfo, Connector, CoreConfig, CoreServices, DATABASE_FILE};
pub use crate::outbox::{ComposeConfig, OutboxConfig, SenderHandle};
pub use crate::reading::{
    DesktopOpener, ReadingServices, RecordingOpener, RemoteConfig, SystemOpener,
};
pub use crate::sync::SyncConfig;

#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod tests;
