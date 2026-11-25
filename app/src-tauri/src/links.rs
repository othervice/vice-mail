//! Stage 9: `mailto:` links the system hands to the app (default mail app). macOS delivers
//! them as `RunEvent::Opened`, Windows as an argument of the start, or of a second start
//! (passed on by the single-instance plugin).
//!
//! Links that arrive before the UI is ready wait here: the UI takes them once at start
//! (`takeLaunchLinks`); after that every link becomes `composeRequested` at once.

use std::sync::{Mutex, PoisonError};

use mail_engine::AppCore;
use tauri::{AppHandle, Manager, Runtime};

/// Links of the launch, and whether the UI took them.
#[derive(Default)]
pub struct LinkQueue(Mutex<(bool, Vec<String>)>);

impl LinkQueue {
    /// The waiting links; from now on links go straight to the UI.
    pub fn take(&self) -> Vec<String> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.0 = true;
        std::mem::take(&mut state.1)
    }

    /// Stage 21: a new main window was built; its page takes the links again.
    pub fn reset(&self) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).0 = false;
    }
}

/// A link the system handed over: queued while the UI is not ready, else `composeRequested`.
pub(crate) fn opened<R: Runtime>(app: &AppHandle<R>, url: &str) {
    if !url
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
    {
        tracing::info!("ignored a link that is not mailto:");
        return;
    }
    let Some(queue) = app.try_state::<LinkQueue>() else {
        return;
    };
    {
        let mut state = queue.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.0 {
            state.1.push(url.to_owned());
            drop(state);
            crate::lifecycle::show_main_window(app);
            return;
        }
    }
    if let Some(core) = app.try_state::<AppCore>()
        && let Err(err) = core.open_url(url)
    {
        tracing::warn!(code = ?err.code(), "mailto link refused");
    }
    // Stage 21: also opens the main window again if it was closed into the background.
    crate::lifecycle::show_main_window(app);
}
