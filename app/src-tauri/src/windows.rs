//! The app's windows (decisions 0005, 0011, 0025): the main window and, since stage 8, one
//! window per opened message and one per draft (the composer in a window of its own).
//!
//! Every window is built through [`guard`] and [`finish`], so each gets the same protection:
//! the navigation hook (the app may load itself, the mail frame its own document, a link
//! placeholder `https://link.invalid/<token>/<index>` is cancelled and reported to the core,
//! everything else is cancelled), no new windows from the page, and on Windows the WebView2
//! frame guard. The CSP comes from `tauri.conf.json` and applies to every page the app serves;
//! the capability `default` names the main window, the message windows (`message-*`) and the
//! composer windows (`composer-*`), so each has exactly the main window's permissions and no
//! window outside these labels has any.

use std::path::PathBuf;

use mail_model::{AccountId, CoreError, DraftId, EmlFile, EmlFileId, MessageId};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{AppHandle, Manager, Runtime, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{MAIN_WINDOW, drops, navigation};

/// Labels of message windows start with this (and match `message-*` in the capability).
pub(crate) const MESSAGE_WINDOW_PREFIX: &str = "message-";

/// Labels of composer windows start with this (and match `composer-*` in the capability).
pub(crate) const COMPOSER_WINDOW_PREFIX: &str = "composer-";

/// Stage 9 (decision 0027): windows of opened `.eml` files are message windows
/// (`message-file-<id>`): the same capability, guards and layout, and no label of a stored
/// message can look like one (those continue with a UUID without hyphens and a number).
pub(crate) const EML_WINDOW_PREFIX: &str = "message-file-";

/// Stage 19 (decision 0055): labels of source windows start with this. They match neither
/// `message-*` nor `composer-*`: the capability `source-window` gives them only
/// `core:window:allow-close`, no events and nothing else (the page reads the source once).
pub(crate) const SOURCE_WINDOW_PREFIX: &str = "source-";

/// Initial and smallest size of a message or composer window (logical pixels).
const MESSAGE_WINDOW_SIZE: (f64, f64) = (900.0, 760.0);
const COMPOSER_WINDOW_SIZE: (f64, f64) = (820.0, 720.0);
const EXTRA_WINDOW_MIN_SIZE: (f64, f64) = (560.0, 420.0);

/// The label of the window of one message: `message-<account uuid, simple>-<message id>`.
pub fn message_window_label(account: AccountId, message: MessageId) -> String {
    format!(
        "{MESSAGE_WINDOW_PREFIX}{}-{}",
        account.as_uuid().simple(),
        message.0
    )
}

/// The page of a message window: the app's own entry with a query the UI reads
/// (`window=message`, `accountId`, `messageId`).
pub fn message_window_path(account: AccountId, message: MessageId) -> String {
    format!(
        "index.html?window=message&accountId={}&messageId={}",
        account.as_uuid().hyphenated(),
        message.0
    )
}

/// The label of the window of an opened `.eml` file: `message-file-<file id>`.
pub fn eml_window_label(file: &EmlFileId) -> String {
    format!("{EML_WINDOW_PREFIX}{}", file.as_str())
}

/// The page of an `.eml` window: the app's own entry with `window=eml&fileId=<id>`.
pub fn eml_window_path(file: &EmlFileId) -> String {
    format!("index.html?window=eml&fileId={}", file.as_str())
}

/// Opens the window of an opened `.eml` file, titled with the file's name. The core forgets
/// the file (bytes and renderings) when the window is gone.
pub fn open_eml_window<R: Runtime>(
    app: &AppHandle<R>,
    file: &EmlFile,
    theme: Option<Theme>,
) -> tauri::Result<WebviewWindow<R>> {
    let window = open_window(
        app,
        &eml_window_label(&file.id),
        &eml_window_path(&file.id),
        &file.name,
        theme,
        MESSAGE_WINDOW_SIZE,
    )?;
    let closing = app.clone();
    let id = file.id.clone();
    window.on_window_event(move |event| {
        forget_eml_file(&closing, &id, event);
    });
    Ok(window)
}

/// The window of an opened file is gone: the core forgets the file. `true` if it did.
pub(crate) fn forget_eml_file<R: Runtime>(
    app: &AppHandle<R>,
    file: &EmlFileId,
    event: &tauri::WindowEvent,
) -> bool {
    matches!(event, tauri::WindowEvent::Destroyed)
        && app
            .try_state::<mail_engine::AppCore>()
            .is_some_and(|core| core.close_eml_file(file))
}

/// The label of the source window of one message: `source-<account uuid, simple>-<message id>`.
pub fn source_window_label(account: AccountId, message: MessageId) -> String {
    format!(
        "{SOURCE_WINDOW_PREFIX}{}-{}",
        account.as_uuid().simple(),
        message.0
    )
}

/// The page of a source window: `window=source`, `accountId`, `messageId`.
pub fn source_window_path(account: AccountId, message: MessageId) -> String {
    format!(
        "index.html?window=source&accountId={}&messageId={}",
        account.as_uuid().hyphenated(),
        message.0
    )
}

/// Opens the source window of one message (or focuses it), titled with the subject. Built like
/// every window ([`guard`], [`finish`]); its capability allows closing itself and nothing else.
pub fn open_source_window<R: Runtime>(
    app: &AppHandle<R>,
    account: AccountId,
    message: MessageId,
    subject: &str,
    theme: Option<Theme>,
) -> tauri::Result<WebviewWindow<R>> {
    open_window(
        app,
        &source_window_label(account, message),
        &source_window_path(account, message),
        subject,
        theme,
        MESSAGE_WINDOW_SIZE,
    )
}

/// The label of the composer window of one draft: `composer-<draft uuid>`.
pub fn composer_window_label(draft: &DraftId) -> String {
    format!("{COMPOSER_WINDOW_PREFIX}{}", draft.as_str())
}

/// The page of a composer window: the app's own entry with `window=composer&draftId=<uuid>`.
pub fn composer_window_path(draft: &DraftId) -> String {
    format!("index.html?window=composer&draftId={}", draft.as_str())
}

/// Which stored layout a window uses: `main` for the main window, `message` for every message
/// window, `composer` for every composer window (so the settings do not grow with each opened
/// window). Other labels have none.
pub fn layout_slot(label: &str) -> Result<&'static str, CoreError> {
    if label == MAIN_WINDOW {
        Ok("main")
    } else if label.starts_with(MESSAGE_WINDOW_PREFIX) {
        Ok("message")
    } else if label.starts_with(COMPOSER_WINDOW_PREFIX) {
        Ok("composer")
    } else {
        Err(CoreError::InvalidArgument(format!(
            "window {label:?} has no layout"
        )))
    }
}

/// The navigation decision of every app window (see the module docs).
pub(crate) fn navigation_policy<R: Runtime>(
    app: &AppHandle<R>,
) -> impl Fn(&tauri::Url) -> bool + Send + 'static {
    // The Vite server of `tauri dev` is the app as well; never in a built app.
    let dev_url = if cfg!(dev) {
        app.config().build.dev_url.clone()
    } else {
        None
    };
    let links = app.clone();
    move |url| {
        navigation::decide_main_navigation(url, dev_url.as_ref(), &|token, index| {
            crate::report_link(&links, token, index);
        })
    }
}

/// The hooks every window is built with.
pub(crate) fn guard<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &AppHandle<R>,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
        .on_navigation(navigation_policy(app))
        // Nothing in the page opens windows; `window.open` and `target=_blank`, also from the
        // mail frame, are refused (WebView2 marks the request handled, decision 0011).
        .on_new_window(|url, _features| {
            tracing::warn!(
                scheme = url.scheme(),
                host = url.host_str(),
                "refused to open a new window"
            );
            NewWindowResponse::Deny
        })
        // Downloads the web view would start itself (context menu "Download Linked File" or
        // "Save Image As", WebView2's download of a link) are refused: the app saves files only
        // in the core, after the system's save dialog (stage 15 review).
        .on_download(|_webview, event| refuse_download(&event))
}

/// The download handler of every window: never lets the web view download anything.
fn refuse_download(event: &DownloadEvent<'_>) -> bool {
    if let DownloadEvent::Requested { url, .. } = event {
        tracing::warn!(
            scheme = url.scheme(),
            host = url.host_str(),
            "refused a download in the web view"
        );
    }
    false
}

/// What every window gets after it is built: dropped files are remembered (stage 5; the
/// composer may attach exactly these), and on Windows the WebView2 frame guard and the window
/// icon of its display scale (stage 21).
pub(crate) fn finish<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
) -> tauri::Result<()> {
    let drop_app = app.clone();
    #[cfg(windows)]
    let icon_window = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
            && let Some(registry) = drop_app.try_state::<drops::DropRegistry>()
        {
            registry.record(paths);
        }
        #[cfg(windows)]
        if let tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } = event {
            set_window_icon(&icon_window, *scale_factor);
        }
    });
    #[cfg(all(windows, feature = "webview2-frame-guard"))]
    crate::install_frame_guard(window)?;
    #[cfg(windows)]
    set_window_icon(window, window.scale_factor().unwrap_or(1.0));
    Ok(())
}

/// Stage 21 (decision 0068), Windows: Tauri sets one window icon, which Windows draws in the
/// title bar and, without a separate large icon, on the taskbar button. Taken from the app
/// icon's own entry for the taskbar's size at this scale (24 logical pixels: 24/30/36/48 px),
/// so the taskbar shows that drawing unscaled instead of a 32 px one shrunk to 24.
#[cfg(windows)]
fn set_window_icon<R: Runtime>(window: &WebviewWindow<R>, scale: f64) {
    let size = crate::badge::window_icon_size(scale);
    match tauri::image::Image::from_app_icon_resource(size) {
        Ok(icon) => {
            if let Err(err) = window.set_icon(icon) {
                tracing::debug!(%err, size, "window icon not set");
            }
        }
        Err(err) => tracing::debug!(%err, size, "no app icon of this size"),
    }
}

/// Brings the window with this label to the front if it is open; `false` if there is none.
pub fn focus_window<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    match app.get_webview_window(label) {
        Some(window) => {
            focus(&window);
            true
        }
        None => false,
    }
}

fn focus<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.unminimize();
    let _ = window.show();
    if let Err(err) = window.set_focus() {
        tracing::debug!(%err, "could not focus the window");
    }
}

/// Opens the window of one message (or focuses it, if a parallel call was faster). The title
/// is the subject, or the main window's title for a message without one.
pub fn open_message_window<R: Runtime>(
    app: &AppHandle<R>,
    account: AccountId,
    message: MessageId,
    subject: &str,
    theme: Option<Theme>,
) -> tauri::Result<WebviewWindow<R>> {
    open_window(
        app,
        &message_window_label(account, message),
        &message_window_path(account, message),
        subject,
        theme,
        MESSAGE_WINDOW_SIZE,
    )
}

/// Opens the composer window of one draft (or focuses it), titled with the draft's subject.
pub fn open_composer_window<R: Runtime>(
    app: &AppHandle<R>,
    draft: &DraftId,
    subject: &str,
    theme: Option<Theme>,
) -> tauri::Result<WebviewWindow<R>> {
    open_window(
        app,
        &composer_window_label(draft),
        &composer_window_path(draft),
        subject,
        theme,
        COMPOSER_WINDOW_SIZE,
    )
}

fn open_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    path: &str,
    subject: &str,
    theme: Option<Theme>,
    size: (f64, f64),
) -> tauri::Result<WebviewWindow<R>> {
    let title = message_window_title(subject, || main_title(app));
    let url = WebviewUrl::App(PathBuf::from(path));
    let builder = WebviewWindowBuilder::new(app, label, url)
        .title(title)
        .inner_size(size.0, size.1)
        .min_inner_size(EXTRA_WINDOW_MIN_SIZE.0, EXTRA_WINDOW_MIN_SIZE.1)
        .theme(theme)
        .focused(true);
    let window = match guard(builder, app).build() {
        Ok(window) => window,
        Err(
            tauri::Error::WindowLabelAlreadyExists(_) | tauri::Error::WebviewLabelAlreadyExists(_),
        ) => {
            let window = app
                .get_webview_window(label)
                .ok_or(tauri::Error::WindowNotFound)?;
            focus(&window);
            return Ok(window);
        }
        Err(err) => return Err(err),
    };
    finish(app, &window)?;
    if let Some(core) = app.try_state::<mail_engine::AppCore>() {
        crate::window_theme::follow(window.clone(), core.subscribe());
    }
    tracing::info!(
        kind = label.split('-').next().unwrap_or_default(),
        "window opened"
    );
    Ok(window)
}

/// The subject, single-line and at most 200 characters; the fallback without one.
fn message_window_title(subject: &str, fallback: impl FnOnce() -> String) -> String {
    let line: String = subject
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    if line.is_empty() { fallback() } else { line }
}

/// The main window's title from `tauri.conf.json` (the product name lives only there).
fn main_title<R: Runtime>(app: &AppHandle<R>) -> String {
    app.config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .map(|w| w.title.clone())
        .unwrap_or_else(|| app.package_info().name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (AccountId, MessageId) {
        let account = "6f9619ff-8b86-4011-b42d-00c04fc964ff"
            .parse()
            .expect("account id");
        (account, MessageId(42))
    }

    #[test]
    fn eml_windows_are_message_windows_with_their_own_query() {
        let file = EmlFileId::from_random(0xfeed);
        let label = eml_window_label(&file);
        assert_eq!(label, "message-file-0000000000000000000000000000feed");
        assert!(
            label.starts_with(MESSAGE_WINDOW_PREFIX),
            "the message windows' capability"
        );
        assert_eq!(layout_slot(&label).expect("slot"), "message");
        let (account, message) = ids();
        assert!(!message_window_label(account, message).starts_with(EML_WINDOW_PREFIX));
        assert_eq!(
            eml_window_path(&file),
            "index.html?window=eml&fileId=0000000000000000000000000000feed"
        );
    }

    #[test]
    fn source_windows_have_their_own_label_page_and_no_layout() {
        let (account, message) = ids();
        let label = source_window_label(account, message);
        assert_eq!(label, "source-6f9619ff8b864011b42d00c04fc964ff-42");
        assert!(!label.starts_with(MESSAGE_WINDOW_PREFIX));
        assert!(!label.starts_with(COMPOSER_WINDOW_PREFIX));
        assert!(layout_slot(&label).is_err(), "no stored layout");
        let base: tauri::Url = "tauri://localhost".parse().expect("base");
        let url = base
            .join(&source_window_path(account, message))
            .expect("join");
        assert_eq!(url.path(), "/index.html");
        assert_eq!(
            url.query(),
            Some("window=source&accountId=6f9619ff-8b86-4011-b42d-00c04fc964ff&messageId=42")
        );
    }

    #[test]
    fn message_windows_have_one_label_per_message() {
        let (account, message) = ids();
        let label = message_window_label(account, message);
        assert_eq!(label, "message-6f9619ff8b864011b42d00c04fc964ff-42");
        assert_ne!(label, message_window_label(account, MessageId(43)));
        // Tauri allows letters, digits, `-`, `/`, `:` and `_` in labels.
        assert!(
            label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | ':' | '_'))
        );
    }

    #[test]
    fn message_window_page_is_the_app_entry_with_a_query() {
        let (account, message) = ids();
        let path = message_window_path(account, message);
        assert_eq!(
            path,
            "index.html?window=message&accountId=6f9619ff-8b86-4011-b42d-00c04fc964ff&messageId=42"
        );
        // Resolved like Tauri does (`get_app_url().join(path)`): same origin as the main window.
        for base in [
            "tauri://localhost",
            "http://tauri.localhost",
            "http://localhost:1420",
        ] {
            let base: tauri::Url = base.parse().expect("base");
            let url = base.join(&path).expect("join");
            assert_eq!(
                (url.scheme(), url.host_str(), url.port()),
                (base.scheme(), base.host_str(), base.port())
            );
            assert_eq!(url.path(), "/index.html");
            let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
            assert_eq!(
                query,
                vec![
                    ("window".into(), "message".into()),
                    ("accountId".into(), account.to_string()),
                    ("messageId".into(), "42".into()),
                ]
            );
        }
    }

    #[test]
    fn composer_windows_have_one_label_and_page_per_draft() {
        let draft = DraftId::parse("0B5E8F43-6C1F-4A51-9A51-0C4F5B3F6A01").expect("draft");
        assert_eq!(
            composer_window_label(&draft),
            "composer-0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01"
        );
        let base: tauri::Url = "tauri://localhost".parse().expect("base");
        let url = base.join(&composer_window_path(&draft)).expect("join");
        assert_eq!(url.path(), "/index.html");
        assert_eq!(
            url.query(),
            Some("window=composer&draftId=0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01")
        );
    }

    #[test]
    fn message_window_title_is_the_subject_on_one_line() {
        let fallback = || "App".to_owned();
        assert_eq!(
            message_window_title("Lunch\r\n  today", fallback),
            "Lunch today"
        );
        assert_eq!(message_window_title("  ", fallback), "App");
        assert_eq!(
            message_window_title(&"x".repeat(300), fallback)
                .chars()
                .count(),
            200
        );
    }

    #[test]
    fn layouts_are_kept_per_kind_of_window() {
        let (account, message) = ids();
        assert_eq!(layout_slot(MAIN_WINDOW).expect("main"), "main");
        assert_eq!(
            layout_slot(&message_window_label(account, message)).expect("message"),
            "message"
        );
        let draft = DraftId::parse("0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01").expect("draft");
        assert_eq!(
            layout_slot(&composer_window_label(&draft)).expect("composer"),
            "composer"
        );
        assert!(layout_slot("other").is_err());
        assert!(layout_slot("").is_err());
    }

    #[test]
    fn the_web_view_never_downloads() {
        let mut destination = PathBuf::from("/tmp/x");
        let requested = DownloadEvent::Requested {
            url: "https://link.invalid/0123456789abcdeffedcba9876543210/0"
                .parse()
                .expect("url"),
            destination: &mut destination,
        };
        assert!(!refuse_download(&requested));
        let image = DownloadEvent::Requested {
            url: "data:image/png;base64,iVBORw0KGgo=".parse().expect("url"),
            destination: &mut destination,
        };
        assert!(!refuse_download(&image));
    }
}
