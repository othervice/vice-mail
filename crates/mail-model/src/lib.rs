//! Types shared by every layer: errors, the [`Secret`] wrapper and the types that cross the
//! IPC bridge. Pure data, no IO and no dependency on other workspace crates.
//!
//! IPC types derive `serde` and `specta::Type`; the shell exports them to TypeScript with
//! tauri-specta (decision 0004). Structs use camelCase on the wire, error codes snake_case.

mod error;
mod ipc;
mod secret;

pub use error::{CoreError, ErrorCode, IpcError};
pub use ipc::{AppInfo, CoreEvent, LanguagePreference, Platform, Preferences, ThemePreference};
pub use secret::Secret;
