//! Which navigations the mail frame may make (decisions 0005 and 0011).
//!
//! Mail HTML is shown in a sandboxed `srcdoc` frame. The frame may load its own document
//! (`about:srcdoc`, `about:blank`) and nothing else. A click on a link navigates the frame to the
//! placeholder the sanitizer wrote into every `href` (`https://link.invalid/<token>/<index>`);
//! the navigation hook cancels it and hands token and index to the core, which alone knows the
//! real target. Every other navigation is cancelled before a request leaves the device.
//!
//! Platform neutral. The navigation hook of the main window (`on_navigation`) asks
//! [`decide_main_navigation`]: on macOS it sees the mail frame's navigations as well, without
//! telling which frame navigates, so the app's own URLs are allowed and everything else follows
//! the frame rule. On Windows it sees the top-level document only; the WebView2 frame guard
//! (crate `webview2-guard`) asks [`decide_frame_navigation`] for the mail frame (decision 0011).

use mail_model::{RenderToken, parse_link_placeholder};
use tauri::Url;

/// What the hook does with one navigation of the mail frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameNavigation {
    /// The frame loading its own document: let it proceed.
    Allow,
    /// A click on link `index` of the rendering `token`: cancel and report to the core.
    Link { token: RenderToken, index: u32 },
    /// Anything else: cancel.
    Block,
}

/// Classifies the URL a frame is about to navigate to.
pub fn frame_navigation(url: &str) -> FrameNavigation {
    if matches!(url, "about:srcdoc" | "about:blank") {
        return FrameNavigation::Allow;
    }
    match parse_link_placeholder(url) {
        Some((token, index)) => FrameNavigation::Link { token, index },
        None => FrameNavigation::Block,
    }
}

/// Whether `url` is the app itself: `tauri://localhost` (macOS), `http(s)://tauri.localhost`
/// (Windows), or the Vite server of `tauri dev` (only when `dev_url` is given).
pub fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    let app_host = |host: &str| url.host_str() == Some(host) && url.port().is_none();
    match url.scheme() {
        "tauri" => app_host("localhost"),
        "http" | "https" => {
            app_host("tauri.localhost") || dev_url.is_some_and(|dev| dev.origin() == url.origin())
        }
        _ => false,
    }
}

/// The main window's hook: the app's own URLs, then the frame rule.
pub fn decide_main_navigation(
    url: &Url,
    dev_url: Option<&Url>,
    on_link: &dyn Fn(RenderToken, u32),
) -> bool {
    is_app_url(url, dev_url) || decide_frame_navigation(url.as_str(), on_link)
}

/// The hook's answer for one frame navigation: `true` lets it proceed. A link click goes to
/// `on_link`. A blocked navigation is logged with scheme and host only, because the rest of a
/// URL from a mail can carry personal data.
pub fn decide_frame_navigation(url: &str, on_link: &dyn Fn(RenderToken, u32)) -> bool {
    match frame_navigation(url) {
        FrameNavigation::Allow => true,
        FrameNavigation::Link { token, index } => {
            on_link(token, index);
            false
        }
        FrameNavigation::Block => {
            let (scheme, rest) = url.split_once(':').unwrap_or(("", url));
            let host = rest
                .strip_prefix("//")
                .and_then(|r| r.split(['/', '?', '#']).next())
                .map(|authority| authority.rsplit('@').next().unwrap_or(authority));
            tracing::warn!(scheme, host, "cancelled a navigation of the mail frame");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use mail_model::link_placeholder;

    use super::*;

    fn token() -> RenderToken {
        RenderToken::from_random(0x0123_4567_89ab_cdef_0011_2233_4455_6677)
    }

    #[test]
    fn the_frame_may_only_load_its_own_document() {
        assert_eq!(frame_navigation("about:srcdoc"), FrameNavigation::Allow);
        assert_eq!(frame_navigation("about:blank"), FrameNavigation::Allow);
        for url in [
            "https://example.com/",
            "http://127.0.0.1:18999/v/link",
            "javascript:alert(1)",
            "data:text/html,<script>1</script>",
            "mailto:someone@example.com",
            // The app itself never loads in the mail frame (macOS and Windows app origins).
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html",
            "https://tauri.localhost/",
            "about:srcdoc#x",
            "about:blank?x",
            "",
        ] {
            assert_eq!(frame_navigation(url), FrameNavigation::Block, "{url:?}");
        }
    }

    #[test]
    fn the_main_window_may_load_the_app_and_nothing_else() {
        let url = |s: &str| Url::parse(s).expect("url");
        let dev = url("http://localhost:1420/");
        let never = |_: RenderToken, _: u32| panic!("not a link");
        for app in [
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html",
        ] {
            assert!(decide_main_navigation(&url(app), None, &never), "{app}");
        }
        assert!(decide_main_navigation(&url("about:srcdoc"), None, &never));
        assert!(decide_main_navigation(
            &url("http://localhost:1420/src/main.tsx"),
            Some(&dev),
            &never
        ));
        for other in [
            "http://localhost:1420/",
            "http://localhost:1421/",
            "tauri://evil.example/",
            "tauri://localhost:1/",
            "http://tauri.localhost:8080/",
            "http://tauri.localhost.evil.example/",
            "https://example.com/",
            "file:///etc/passwd",
            "data:text/html,x",
        ] {
            assert!(
                !decide_main_navigation(&url(other), None, &never),
                "{other}"
            );
        }
        assert!(!decide_main_navigation(
            &url("http://localhost:1421/"),
            Some(&dev),
            &never
        ));

        // A link click in the mail frame reaches the main hook on macOS: reported, cancelled.
        let seen = RefCell::new(Vec::new());
        let on_link = |token: RenderToken, index: u32| seen.borrow_mut().push((token, index));
        assert!(!decide_main_navigation(
            &url(&link_placeholder(&token(), 3)),
            None,
            &on_link
        ));
        assert_eq!(*seen.borrow(), vec![(token(), 3)]);
    }

    #[test]
    fn link_placeholders_are_reported_and_cancelled() {
        let url = link_placeholder(&token(), 42);
        assert_eq!(
            frame_navigation(&url),
            FrameNavigation::Link {
                token: token(),
                index: 42
            }
        );

        let seen = RefCell::new(Vec::new());
        let on_link = |token: RenderToken, index: u32| seen.borrow_mut().push((token, index));
        assert!(!decide_frame_navigation(&url, &on_link));
        assert_eq!(*seen.borrow(), vec![(token(), 42)]);

        // Near misses are blocked without a report: the core only ever gets exact placeholders.
        for near in [
            url.replacen("https://", "http://", 1),
            format!("{url}?x"),
            format!("{url}#x"),
            url.replacen("link.invalid", "link.invalid:443", 1),
            url.replacen("link.invalid", "user@link.invalid", 1),
            url.replacen("link.invalid", "link.invalid.example", 1),
        ] {
            assert!(!decide_frame_navigation(&near, &on_link), "{near:?}");
        }
        assert!(decide_frame_navigation("about:srcdoc", &on_link));
        assert!(!decide_frame_navigation("https://example.com/", &on_link));
        assert_eq!(
            seen.borrow().len(),
            1,
            "only the exact placeholder is reported"
        );
    }
}
