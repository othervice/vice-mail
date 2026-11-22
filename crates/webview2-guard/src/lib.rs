//! WebView2 frame guard (Windows only, decision 0011, option B).
//!
//! wry subscribes WebView2's `NavigationStarting`, which only fires for the top-level document.
//! Navigations inside the mail frame (a click on a link) raise `FrameNavigationStarting`
//! instead, and Tauri's navigation hook never sees them. [`guard_frame_navigations`] subscribes
//! that event on a web view's `ICoreWebView2` and asks a policy whether each child-frame
//! navigation may proceed; everything the policy refuses, and every navigation whose URL cannot
//! be read, is cancelled before any request is made.
//!
//! **The one crate with `unsafe`.** The WebView2 COM methods are `unsafe fn` in the `windows`
//! bindings. Every other crate of the workspace keeps `unsafe_code = "forbid"`; this one has
//! `deny`, and each of the four COM calls below carries its own `#[allow(unsafe_code)]` and a
//! `SAFETY` comment. `scripts/check.sh` checks that `unsafe` appears nowhere else. The crate is
//! empty on other platforms. It knows nothing of Tauri: the shell gets the controller through
//! Tauri's safe `with_webview` and hands it in.

#![cfg(windows)]

use std::sync::Arc;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
pub use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use webview2_com::{NavigationStartingEventHandler, take_pwstr};

/// Decides one child-frame navigation from its URL: `true` lets it proceed.
pub type FramePolicy = dyn Fn(&str) -> bool + Send + Sync + 'static;

/// Error of the COM calls (an `HRESULT` with its message).
pub type Error = webview2_com::Error;

/// Subscribes the frame guard on the web view of `controller`. Must run on the thread that owns
/// the web view (Tauri's `with_webview` callback does). The registration lives as long as the
/// web view; it is never removed.
pub fn guard_frame_navigations(
    controller: &ICoreWebView2Controller,
    policy: Arc<FramePolicy>,
) -> Result<(), Error> {
    // SAFETY: `controller` is a live COM interface pointer: the caller holds a counted reference
    // (wry's controller, cloned with AddRef by Tauri's `PlatformWebview::controller`), and we
    // are on the web view's own thread, as WebView2 requires. `CoreWebView2` only writes the
    // returned interface, which the bindings wrap in an owned, reference-counted value.
    #[allow(unsafe_code)]
    let core: ICoreWebView2 = unsafe { controller.CoreWebView2() }?;
    let handler = NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        // Reads the target URL and cancels the navigation unless the policy allows it. A URL
        // that cannot be read is cancelled as well.
        let mut uri = Default::default();
        // SAFETY: `args` is valid for the duration of this event callback (WebView2 passes a
        // counted reference); `Uri` writes a string allocated with CoTaskMemAlloc into `uri`,
        // which `take_pwstr` copies and frees exactly once.
        #[allow(unsafe_code)]
        let read = unsafe { args.Uri(&mut uri) };
        let allow = match read {
            Ok(()) => policy(&take_pwstr(uri)),
            Err(err) => {
                tracing::warn!(%err, "frame navigation without readable URL, cancelled");
                false
            }
        };
        // SAFETY: as above, `args` is valid during the callback; `SetCancel` takes a plain flag
        // by value and touches no memory of ours.
        #[allow(unsafe_code)]
        let cancelled = unsafe { args.SetCancel(!allow) };
        cancelled?;
        Ok(())
    }));
    let mut token = 0_i64;
    // SAFETY: `core` is valid (see above); `handler` is a COM object that WebView2 AddRefs and
    // keeps for as long as the subscription lives; `token` is a valid, exclusively borrowed
    // out-pointer for the duration of the call.
    #[allow(unsafe_code)]
    let subscribed = unsafe { core.add_FrameNavigationStarting(&handler, &mut token) };
    subscribed?;
    Ok(())
}
