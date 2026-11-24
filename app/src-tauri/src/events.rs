//! Forwards the core's event stream to every window as the typed tauri-specta event
//! `core-event`, except the new-mail popup's (stage 18, decision 0051): it hears only its own
//! event.

use mail_model::CoreEvent;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, EventTarget, Runtime};
use tauri_specta::Event;
use tokio::sync::broadcast::{self, error::RecvError};

/// Wire wrapper that makes [`CoreEvent`] a tauri-specta event. (The event trait cannot be
/// implemented on `CoreEvent` itself, because `mail-model` must not depend on Tauri.)
#[derive(Debug, Clone, Serialize, Deserialize, tauri_specta::Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "core-event")]
pub struct CoreEventMessage(pub CoreEvent);

/// Transparent on the TypeScript side as well: the event's type is exactly `CoreEvent`, without
/// an extra alias for the wrapper (a derive would emit `type CoreEventMessage = CoreEvent`).
impl specta::Type for CoreEventMessage {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <CoreEvent as specta::Type>::definition(types)
    }
}

/// Event name on the wire.
pub const CORE_EVENT_NAME: &str = <CoreEventMessage as Event>::NAME;

/// Whether a target gets the core's events: every window but the new-mail popup.
pub fn hears_core_events(target: &EventTarget) -> bool {
    match target {
        EventTarget::AnyLabel { label }
        | EventTarget::Window { label }
        | EventTarget::Webview { label }
        | EventTarget::WebviewWindow { label } => label != crate::popup::POPUP_WINDOW,
        _ => true,
    }
}

/// Spawns the task that emits every core event to all windows (but the popup's) until the core
/// shuts down.
pub(crate) fn forward<R: Runtime>(app: AppHandle<R>, mut events: broadcast::Receiver<CoreEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Err(err) = CoreEventMessage(event).emit_filter(&app, hears_core_events) {
                        tracing::error!(%err, "could not emit core event to the UI");
                    }
                }
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(
                        missed,
                        "UI event forwarder fell behind; events were dropped"
                    );
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}
