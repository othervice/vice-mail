//! Test builds only (`--features e2e`): helpers the WebdriverIO run needs beyond W3C WebDriver.
//!
//! The embedded WebDriver (`tauri-plugin-wdio-webdriver`) clicks with `element.click()` and
//! synthetic DOM events from the main frame. That cannot reach into an opaque (cross-origin)
//! sandboxed frame and produces untrusted events without user activation. `e2e_native_mouse`
//! instead feeds real `NSEvent`s into the WKWebView, exactly like a user's mouse would, so the
//! tests also cover trusted clicks and hovers inside frames of any origin.

use tauri::WebviewWindow;

/// `kind`: `"click"` (left down + up) or `"move"`. `x`/`y`: CSS pixels in the main frame's
/// viewport (as returned by `getBoundingClientRect()`).
#[tauri::command]
pub async fn e2e_native_mouse(
    window: WebviewWindow,
    kind: String,
    x: f64,
    y: f64,
    viewport_height: Option<f64>,
    via: Option<String>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
        let via = via.unwrap_or_else(|| "view".into());
        window
            .with_webview(move |webview| {
                let result = native::send(
                    webview.inner(),
                    webview.ns_window(),
                    &kind,
                    x,
                    y,
                    viewport_height,
                    &via,
                );
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("native mouse event timed out: {e}"))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, kind, x, y, viewport_height, via);
        Err("native mouse events are only implemented for macOS in this spike".into())
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::c_void;

    use std::sync::atomic::{AtomicIsize, Ordering};

    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSView, NSWindow};
    use objc2_foundation::{NSPoint, NSProcessInfo};

    static EVENT_NUMBER: AtomicIsize = AtomicIsize::new(1000);

    pub fn send(
        webview: *mut c_void,
        ns_window: *mut c_void,
        kind: &str,
        x: f64,
        y: f64,
        viewport_height: Option<f64>,
        via: &str,
    ) -> Result<String, String> {
        if webview.is_null() || ns_window.is_null() {
            return Err("webview or window handle is null".into());
        }
        // SAFETY: Tauri hands out the live WKWebView (an NSView) and its NSWindow; this closure
        // runs on the main thread inside `with_webview`.
        let (view, window) = unsafe {
            (
                &*(webview as *const NSView),
                &*(ns_window as *const NSWindow),
            )
        };

        // The WKWebView spans the whole window, the web viewport starts below the title bar.
        let bounds = view.bounds();
        let top_inset = viewport_height.map_or(0.0, |h| (bounds.size.height - h).max(0.0));
        let y = y + top_inset;
        let local_y = if view.isFlipped() {
            y
        } else {
            bounds.size.height - y
        };
        if via == "activate" || kind == "move" {
            // Mouse-moved events only reach WebKit while the window is key.
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                #[allow(deprecated)]
                objc2_app_kit::NSApplication::sharedApplication(mtm)
                    .activateIgnoringOtherApps(true);
            }
            window.makeKeyAndOrderFront(None);
        }
        let location = view.convertPoint_toView(NSPoint::new(x, local_y), None);
        let make = |t: NSEventType, clicks: isize| {
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                t,
                location,
                NSEventModifierFlags::empty(),
                NSProcessInfo::processInfo().systemUptime(),
                window.windowNumber(),
                None,
                EVENT_NUMBER.fetch_add(1, Ordering::Relaxed),
                clicks,
                if clicks > 0 { 1.0 } else { 0.0 },
            )
            .ok_or_else(|| format!("could not create {t:?}"))
        };

        let deliver = |ev: &NSEvent, direct: fn(&NSView, &NSEvent)| {
            if via == "window" {
                window.sendEvent(ev);
            } else {
                direct(view, ev);
            }
        };
        match kind {
            "move" => {
                let ev = make(NSEventType::MouseMoved, 0)?;
                deliver(&ev, |v, e| v.mouseMoved(e));
            }
            "click" => {
                let down = make(NSEventType::LeftMouseDown, 1)?;
                let up = make(NSEventType::LeftMouseUp, 1)?;
                deliver(&down, |v, e| v.mouseDown(e));
                deliver(&up, |v, e| v.mouseUp(e));
            }
            other => return Err(format!("unknown mouse event kind `{other}`")),
        }
        Ok(format!(
            "via={via} key={} visible={} flipped={} bounds={:.0}x{:.0} location=({:.1},{:.1}) window={}",
            window.isKeyWindow(),
            window.isVisible(),
            view.isFlipped(),
            bounds.size.width,
            bounds.size.height,
            location.x,
            location.y,
            window.windowNumber()
        ))
    }
}
