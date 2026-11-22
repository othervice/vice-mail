//! Handing a link or a file to the operating system (default browser, mail app or viewer).
//! Behind a trait, so tests check what would be opened without launching anything.

use std::path::Path;

use mail_model::CoreError;

/// Opens URLs and files with the system's default application. Only ever called after the core
/// checked the target (links: `http`, `https`, `mailto`; files: a copy in the app's own
/// directory, executables only after confirmation).
pub trait SystemOpener: Send + Sync {
    fn open_url(&self, url: &str) -> Result<(), CoreError>;
    fn open_path(&self, path: &Path) -> Result<(), CoreError>;
}

/// The product's opener: the `open` crate (macOS `/usr/bin/open`, Windows `ShellExecuteW`),
/// detached, so the app never waits for the other program.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopOpener;

impl SystemOpener for DesktopOpener {
    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        open::that_detached(url).map_err(|err| {
            CoreError::Internal(format!("the system could not open the link: {err}"))
        })
    }

    fn open_path(&self, path: &Path) -> Result<(), CoreError> {
        open::that_detached(path).map_err(|err| {
            CoreError::Internal(format!("the system could not open the file: {err}"))
        })
    }
}

/// Records instead of opening; for tests.
#[derive(Debug, Default)]
pub struct RecordingOpener {
    opened: std::sync::Mutex<Vec<String>>,
}

impl RecordingOpener {
    /// Everything "opened" so far: URLs as given, files as their path.
    pub fn opened(&self) -> Vec<String> {
        self.opened
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn record(&self, what: String) {
        self.opened
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(what);
    }
}

impl SystemOpener for RecordingOpener {
    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        self.record(url.to_owned());
        Ok(())
    }

    fn open_path(&self, path: &Path) -> Result<(), CoreError> {
        self.record(path.display().to_string());
        Ok(())
    }
}
