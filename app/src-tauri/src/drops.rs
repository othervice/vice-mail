//! Files dropped on the window (stage 5, decision 0018).
//!
//! The composer attaches dropped files by their paths, which the page learns from Tauri's
//! drag-and-drop event. The core must not read whatever path the page names: a page that runs
//! foreign code (a bug, a compromised dependency) could attach `~/.ssh/id_ed25519` and send it
//! away. So the shell records the paths of every drop it sees itself (the window's
//! `DragDrop::Drop` event, produced by the operating system, not by the page) and
//! `addDroppedAttachments` accepts a path only if it was dropped on the window shortly before;
//! each drop can be used once. The file dialog (`pickAttachments`) needs no such check: the
//! shell opens it and gets the paths itself.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a dropped path stays usable.
pub const DROP_LIFETIME: Duration = Duration::from_secs(120);
/// Most dropped paths remembered.
const MAX_REMEMBERED: usize = 1_000;

/// Paths of recent drops on the main window.
#[derive(Debug, Default)]
pub struct DropRegistry {
    dropped: Mutex<VecDeque<(PathBuf, Instant)>>,
}

impl DropRegistry {
    /// Records the paths of a drop (from the window's own event).
    pub fn record(&self, paths: &[PathBuf]) {
        self.record_at(paths, Instant::now());
    }

    fn record_at(&self, paths: &[PathBuf], at: Instant) {
        let mut dropped = self
            .dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for path in paths {
            dropped.push_back((path.clone(), at));
        }
        while dropped.len() > MAX_REMEMBERED {
            dropped.pop_front();
        }
    }

    /// Takes `paths` out of the registry if every one was dropped within [`DROP_LIFETIME`];
    /// otherwise nothing is taken and the first unknown path is returned as the error.
    pub fn take(&self, paths: &[String]) -> Result<Vec<PathBuf>, String> {
        self.take_at(paths, Instant::now())
    }

    fn take_at(&self, paths: &[String], now: Instant) -> Result<Vec<PathBuf>, String> {
        let mut dropped = self
            .dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        dropped.retain(|(_, at)| now.saturating_duration_since(*at) <= DROP_LIFETIME);
        let mut picked = Vec::with_capacity(paths.len());
        let mut used = Vec::new();
        for path in paths {
            let wanted = Path::new(path);
            let found = dropped
                .iter()
                .enumerate()
                .find(|(i, (p, _))| p == wanted && !used.contains(i))
                .map(|(i, _)| i);
            match found {
                Some(i) => {
                    used.push(i);
                    picked.push(wanted.to_path_buf());
                }
                None => return Err(path.clone()),
            }
        }
        used.sort_unstable();
        for i in used.into_iter().rev() {
            dropped.remove(i);
        }
        Ok(picked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recently_dropped_paths_are_accepted_once() {
        let registry = DropRegistry::default();
        let start = Instant::now();
        registry.record_at(
            &[PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.png")],
            start,
        );
        let a = "/tmp/a.pdf".to_owned();
        let b = "/tmp/b.png".to_owned();
        let secret = "/Users/x/.ssh/id_ed25519".to_owned();
        assert_eq!(
            registry.take_at(&[a.clone(), secret.clone()], start),
            Err(secret),
            "a path never dropped is refused"
        );
        assert_eq!(
            registry.take_at(std::slice::from_ref(&a), start),
            Ok(vec![PathBuf::from(&a)]),
            "nothing was taken by the refused request"
        );
        assert!(
            registry.take_at(std::slice::from_ref(&a), start).is_err(),
            "only once"
        );
        assert!(
            registry
                .take_at(
                    std::slice::from_ref(&b),
                    start + DROP_LIFETIME + Duration::from_secs(1)
                )
                .is_err(),
            "too old"
        );
    }
}
