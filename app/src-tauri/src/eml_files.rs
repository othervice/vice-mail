//! Stage 9 (decision 0027): `.eml` files the system hands to the app. The bundle declares the
//! file type (`bundle.fileAssociations` in `tauri.conf.json`: on macOS `CFBundleDocumentTypes`
//! with the rank "Alternate", so Mail stays the default; on Windows the installer's
//! "Open with" entry). macOS delivers a double-clicked file as a `file://` URL in
//! `RunEvent::Opened`, Windows as an argument of the start, or of a second start (passed on by
//! the single-instance plugin, with that start's working directory).
//!
//! Every file opens read-only in a window of its own. The core reads and checks it; a file that
//! is no mail still gets its window, which says why (at launch there is no other place to say
//! it). Nothing is imported into an account.

use std::path::{Path, PathBuf};

use mail_engine::AppCore;
use tauri::{AppHandle, Manager, Runtime};

/// Whether `path` names an `.eml` file (by its extension, ASCII case-insensitive).
fn is_eml(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("eml"))
}

/// The `.eml` files among the arguments of a start (without the program itself): options and
/// `mailto:` links are skipped, a relative path is taken from `cwd`.
pub(crate) fn eml_arguments(
    args: impl IntoIterator<Item = String>,
    cwd: Option<&Path>,
) -> Vec<PathBuf> {
    args.into_iter()
        .filter(|arg| !arg.starts_with('-') && !arg.contains("://") && !is_link(arg))
        .map(PathBuf::from)
        .filter(|path| is_eml(path))
        .map(|path| match cwd {
            Some(cwd) if path.is_relative() => cwd.join(path),
            _ => path,
        })
        .collect()
}

fn is_link(arg: &str) -> bool {
    arg.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
}

/// The `.eml` files among the URLs macOS opened the app with (`file://` only).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn eml_urls<'a>(urls: impl IntoIterator<Item = &'a tauri::Url>) -> Vec<PathBuf> {
    urls.into_iter()
        .filter(|url| url.scheme() == "file")
        .filter_map(|url| url.to_file_path().ok())
        .filter(|path| is_eml(path))
        .collect()
}

/// Opens each file in a window of its own (off the event loop: reading a file takes a moment).
pub(crate) fn open_files<R: Runtime>(app: &AppHandle<R>, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(core) = app.try_state::<AppCore>() else {
            tracing::warn!("an .eml file arrived before the core was ready");
            return;
        };
        for path in paths {
            let file = match core.open_eml_from_system(path).await {
                Ok(file) => file,
                Err(err) => {
                    tracing::warn!(code = ?err.code(), "an .eml file could not be taken");
                    continue;
                }
            };
            let theme = crate::commands::ui::theme(&core).await;
            if let Err(err) = crate::windows::open_eml_window(&app, &file, theme) {
                tracing::error!(%err, "the window of an .eml file could not be opened");
                core.close_eml_file(&file.id);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn eml_files_are_taken_from_the_arguments() {
        let cwd = Path::new("/Users/someone/Downloads");
        let found = eml_arguments(
            args(&[
                "--flag",
                "mailto:bob@example.org?subject=x.eml",
                "/abs/Letter.EML",
                "relative/Invoice.eml",
                "notes.txt",
                "https://example.org/a.eml",
                "-psn_0_12345",
            ]),
            Some(cwd),
        );
        assert_eq!(
            found,
            [
                PathBuf::from("/abs/Letter.EML"),
                cwd.join("relative/Invoice.eml")
            ]
        );
        assert_eq!(
            eml_arguments(args(&["plain.eml"]), None),
            [PathBuf::from("plain.eml")]
        );
        assert!(eml_arguments(args(&["archive.eml.zip", "eml"]), None).is_empty());
    }

    #[test]
    fn only_file_urls_of_eml_files_count() {
        let urls: Vec<tauri::Url> = [
            "file:///Users/someone/Mail%20export/Hello%20World.eml",
            "file:///Users/someone/picture.png",
            "mailto:bob@example.org",
            "https://example.org/x.eml",
        ]
        .iter()
        .map(|u| tauri::Url::parse(u).expect("url"))
        .collect();
        let found = eml_urls(&urls);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].ends_with("Mail export/Hello World.eml"),
            "{found:?}"
        );
    }
}
