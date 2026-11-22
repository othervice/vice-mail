//! WebView2 frame guard (Windows only; decision 0011, draft).
//!
//! wry subscribes WebView2's `NavigationStarting`, which only fires for the top-level document.
//! Navigations inside the mail frame (a click on a link) raise `FrameNavigationStarting` instead,
//! and Tauri's navigation hook never sees them. This module subscribes that event on the
//! window's `ICoreWebView2` and asks a policy (`navigation::decide_frame_navigation`) whether
//! each child-frame navigation may proceed; everything else is cancelled before any request is
//! made. New windows are refused separately, through Tauri's safe `on_new_window` hook.
//!
//! The WebView2 COM methods are `unsafe fn` in the `windows` bindings, so this module cannot
//! compile under the workspace's `unsafe_code = "forbid"`. It is built only with the Cargo
//! feature `webview2-frame-guard`, which stays off until decision 0011 settles the lint.
//! Every `unsafe` block below calls one COM method on an interface obtained from wry or from
//! WebView2 itself, on the thread that owns the web view.

#![allow(unsafe_code)]

use std::sync::Arc;

use tauri::{Runtime, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2Controller};
use webview2_com::{NavigationStartingEventHandler, take_pwstr};

/// Decides one child-frame navigation from its URL: `true` lets it proceed.
pub type FramePolicy = dyn Fn(&str) -> bool + Send + Sync + 'static;

/// Subscribes the frame guard on the window's web view. Runs on the main thread (WebView2 calls
/// must come from the thread that created the web view); from Tauri's `setup` that happens
/// before this returns. A failure to subscribe is logged as an error.
pub fn install<R: Runtime>(
    window: &WebviewWindow<R>,
    policy: Arc<FramePolicy>,
) -> tauri::Result<()> {
    window.with_webview(move |webview| match subscribe(&webview.controller(), policy) {
        Ok(()) => tracing::info!("WebView2 frame guard active"),
        Err(err) => tracing::error!(
            %err,
            "WebView2 frame guard could not be installed: navigations in the mail frame are not checked"
        ),
    })
}

fn subscribe(
    controller: &ICoreWebView2Controller,
    policy: Arc<FramePolicy>,
) -> webview2_com::Result<()> {
    // SAFETY: `controller` is wry's live controller of this window, cloned (AddRef) by
    // `PlatformWebview::controller`; `with_webview` runs this on the web view's own thread.
    let core: ICoreWebView2 = unsafe { controller.CoreWebView2() }?;
    let handler = NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        // Reads the target URL and cancels the navigation unless the policy allows it. A URL
        // that cannot be read is cancelled as well.
        let mut uri = Default::default();
        // SAFETY: `args` is valid for the duration of this event callback; `Uri` writes a string
        // allocated with CoTaskMemAlloc into `uri`, which `take_pwstr` copies and frees.
        let allow = match unsafe { args.Uri(&mut uri) } {
            Ok(()) => policy(&take_pwstr(uri)),
            Err(err) => {
                tracing::warn!(%err, "frame navigation without readable URL, cancelled");
                false
            }
        };
        // SAFETY: as above; `SetCancel` takes a plain flag.
        unsafe { args.SetCancel(!allow) }
    }));
    // The registration lives as long as the web view; it is never removed.
    let mut token = 0_i64;
    // SAFETY: `core` is valid (see above); `handler` is a COM object that WebView2 AddRefs and
    // keeps; `token` is a valid out-pointer for the duration of the call.
    unsafe { core.add_FrameNavigationStarting(&handler, &mut token) }?;
    Ok(())
}
