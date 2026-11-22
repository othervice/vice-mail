//! Core of the mail client. The shell talks to [`AppCore`] only; it receives the typed event
//! stream through [`AppCore::subscribe`]. Everything here runs and is tested without a WebView.

mod core;
pub mod logging;

pub use crate::core::{AppCore, BuildInfo, CoreConfig, DATABASE_FILE};

#[cfg(test)]
mod tests;
