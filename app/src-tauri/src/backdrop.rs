//! Stage 22 (decision 0079): the Windows 11 look. Every app window gets Mica, the material of
//! Windows 11's own apps, in its title bar; the main window also behind the page's chrome.
//!
//! - **Title bar:** the native one stays (native caption buttons, so the snap layouts appear on
//!   the maximise button). With Mica the system draws it in the material, light or dark as the
//!   window's theme says (`window_theme`, the app's theme choice), and the main window's frame
//!   continues it seamlessly below.
//! - **Main window:** built transparent, so the page decides where Mica shows: the UI leaves the
//!   frame, the icon rail and the docked folder column transparent and keeps list, reading pane,
//!   composer and every panel opaque (`get_window_backdrop` tells it; app.css).
//! - **Other windows** (message, composer, `.eml`, source): Mica in the title bar only; their page
//!   is opaque as before. The new-mail popup keeps its own look.
//! - **Fallback:** before Windows 11 (build 22000), with transparency effects off or high contrast
//!   on, when the window is built, nothing changes: no effect, an opaque window, today's colours.
//!   A change of those settings while a window is open reaches the page through CSS
//!   (`prefers-reduced-transparency`, `forced-colors`), which then paints the chrome opaque again.
//!
//! Tauri's own APIs only (window effects through window-vibrancy, `transparent`); no `unsafe`.

use std::collections::HashMap;
use std::sync::Mutex;

use mail_model::WindowBackdrop;
use tauri::{AppHandle, Manager, Runtime, WebviewWindowBuilder};

/// First Windows 11 build; DWM draws Mica from here on (window-vibrancy uses the documented
/// `DWMWA_SYSTEMBACKDROP_TYPE` from 22523, an older attribute before).
pub const FIRST_MICA_BUILD: u32 = 22_000;

/// What the system says about its look when a window is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemLook {
    /// Windows build number; `None` if unknown (never Mica then).
    pub build: Option<u32>,
    /// Settings → Personalisation → Colours → Transparency effects.
    pub transparency: bool,
    /// A contrast theme is on: the system paints its own colours, no materials.
    pub high_contrast: bool,
}

/// Mica when the system can draw it and the user has not turned it off.
pub fn choose(look: SystemLook) -> WindowBackdrop {
    let windows_11 = look.build.is_some_and(|build| build >= FIRST_MICA_BUILD);
    if windows_11 && look.transparency && !look.high_contrast {
        WindowBackdrop::Mica
    } else {
        WindowBackdrop::None
    }
}

/// The backdrop for a window built now: Windows reads its settings, every other system has none.
pub fn current() -> WindowBackdrop {
    #[cfg(windows)]
    {
        choose(system_look())
    }
    #[cfg(not(windows))]
    {
        WindowBackdrop::None
    }
}

/// Reads the look from the registry (safe API, no `unsafe`). A value that cannot be read counts
/// as Windows' default (transparency on, high contrast off); an unknown build never gets Mica.
#[cfg(windows)]
fn system_look() -> SystemLook {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
    let build = LOCAL_MACHINE
        .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|key| key.get_string("CurrentBuildNumber"))
        .ok()
        .and_then(|text| text.trim().parse().ok());
    let transparency_off = CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("EnableTransparency"))
        .is_ok_and(|value| value == 0);
    // HIGHCONTRAST.dwFlags as text; bit 0 is HCF_HIGHCONTRASTON.
    let high_contrast = CURRENT_USER
        .open(r"Control Panel\Accessibility\HighContrast")
        .and_then(|key| key.get_string("Flags"))
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .is_some_and(|flags| flags & 1 != 0);
    let look = SystemLook {
        build,
        transparency: !transparency_off,
        high_contrast,
    };
    tracing::debug!(?look, "system look");
    look
}

/// Which part of a window Mica may fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The title bar and the page's transparent chrome (the main window).
    Page,
    /// The title bar only; the page stays opaque.
    TitleBar,
}

/// The window effects of Mica. `Mica` (not `MicaDark`/`MicaLight`) follows the window's theme,
/// which `window_theme` keeps on the app's choice, so it turns dark and light with the app.
#[cfg(windows)]
fn mica() -> tauri::utils::config::WindowEffectsConfig {
    tauri::window::EffectsBuilder::new()
        .effect(tauri::window::Effect::Mica)
        .build()
}

/// Styles a window that is about to be built: with Mica the effect, and for [`Reach::Page`] a
/// transparent window and web view. Returns what the page will have behind it.
pub fn style<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    backdrop: WindowBackdrop,
    reach: Reach,
) -> (WebviewWindowBuilder<'a, R, M>, WindowBackdrop) {
    match backdrop {
        WindowBackdrop::None => (builder, WindowBackdrop::None),
        #[cfg(windows)]
        WindowBackdrop::Mica => {
            let builder = builder.effects(mica());
            match reach {
                Reach::Page => (builder.transparent(true), WindowBackdrop::Mica),
                Reach::TitleBar => (builder, WindowBackdrop::None),
            }
        }
        #[cfg(not(windows))]
        WindowBackdrop::Mica => {
            let _ = reach;
            (builder, WindowBackdrop::None)
        }
    }
}

/// What each window's page has behind it, by label (`get_window_backdrop`).
#[derive(Debug, Default)]
pub struct Backdrops(Mutex<HashMap<String, WindowBackdrop>>);

impl Backdrops {
    pub fn set(&self, label: &str, backdrop: WindowBackdrop) {
        if let Ok(mut map) = self.0.lock() {
            if backdrop == WindowBackdrop::None {
                map.remove(label);
            } else {
                map.insert(label.to_owned(), backdrop);
            }
        }
    }

    pub fn get(&self, label: &str) -> WindowBackdrop {
        self.0
            .lock()
            .ok()
            .and_then(|map| map.get(label).copied())
            .unwrap_or_default()
    }
}

/// Records the backdrop of a window just built (and forgets it when the window is gone).
pub fn remember<R: Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    backdrop: WindowBackdrop,
) {
    let Some(backdrops) = app.try_state::<Backdrops>() else {
        return;
    };
    backdrops.set(window.label(), backdrop);
    if backdrop != WindowBackdrop::None {
        let app = app.clone();
        let label = window.label().to_owned();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed)
                && let Some(backdrops) = app.try_state::<Backdrops>()
            {
                backdrops.set(&label, WindowBackdrop::None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS_11: SystemLook = SystemLook {
        build: Some(26_100),
        transparency: true,
        high_contrast: false,
    };

    #[test]
    fn mica_only_on_windows_11_with_transparency_and_without_high_contrast() {
        assert_eq!(choose(WINDOWS_11), WindowBackdrop::Mica);
        assert_eq!(
            choose(SystemLook {
                build: Some(FIRST_MICA_BUILD),
                ..WINDOWS_11
            }),
            WindowBackdrop::Mica,
            "the first Windows 11 build"
        );
        for look in [
            // Windows 10 (22H2 is build 19045).
            SystemLook {
                build: Some(19_045),
                ..WINDOWS_11
            },
            SystemLook {
                build: None,
                ..WINDOWS_11
            },
            SystemLook {
                transparency: false,
                ..WINDOWS_11
            },
            SystemLook {
                high_contrast: true,
                ..WINDOWS_11
            },
        ] {
            assert_eq!(choose(look), WindowBackdrop::None, "{look:?}");
        }
    }

    #[test]
    fn other_systems_have_no_backdrop() {
        if !cfg!(windows) {
            assert_eq!(current(), WindowBackdrop::None);
        }
    }

    #[test]
    fn backdrops_are_kept_per_window() {
        let backdrops = Backdrops::default();
        assert_eq!(backdrops.get("main"), WindowBackdrop::None);
        backdrops.set("main", WindowBackdrop::Mica);
        assert_eq!(backdrops.get("main"), WindowBackdrop::Mica);
        assert_eq!(backdrops.get("message-x-1"), WindowBackdrop::None);
        backdrops.set("main", WindowBackdrop::None);
        assert_eq!(backdrops.get("main"), WindowBackdrop::None);
    }
}
