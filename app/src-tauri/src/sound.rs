//! Stage 18 (decision 0051): the notification sound of stage 16 for the new-mail popup. A system
//! notification carries its sound itself; the popup is the app's own window, so the shell plays
//! the chosen sound, by the same names (`mail_model::MACOS_SOUNDS`, `WINDOWS_SOUNDS`), with safe
//! bindings only:
//! - **macOS**: the alert sound of `/System/Library/Sounds` by name (`NSSound soundNamed:`);
//!   `Default` is the user's alert sound (`NSBeep`), as `NSUserNotificationDefaultSoundName`
//!   plays it for a notification.
//! - **Windows**: the toast sounds `ms-winsoundevent:Notification.*` are the sound scheme's
//!   events `Notification.Default`, `.IM`, `.Mail`, `.Reminder` and `.SMS`. Their file is read
//!   from the scheme (`HKCU\AppEvents\Schemes\Apps\.Default\<event>\.Current`) and played with
//!   WinRT's `MediaPlayer` (category "alerts", not in the media controls). An event set to
//!   "(None)" stays silent, as the toast would.
//! - Elsewhere nothing (the app is built for macOS and Windows).
//!
//! A failure is logged and otherwise ignored: a missing sound never stops the popup.

use tauri::{AppHandle, Runtime};

/// Plays the sound `name` of this platform's list (unknown names play `Default`).
pub(crate) fn play<R: Runtime>(app: &AppHandle<R>, name: &str) {
    #[cfg(target_os = "macos")]
    on_macos::play(app, name);
    #[cfg(windows)]
    {
        let _ = app;
        on_windows::play(name);
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (app, name);
    }
}

/// The Windows sound-scheme event of a toast sound name (`Mail` → `Notification.Mail`).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_event(name: &str) -> &'static str {
    match name {
        "IM" => "Notification.IM",
        "Mail" => "Notification.Mail",
        "Reminder" => "Notification.Reminder",
        "SMS" => "Notification.SMS",
        _ => "Notification.Default",
    }
}

/// Expands `%NAME%` references of a registry path (`REG_EXPAND_SZ`) with `lookup`; an unknown
/// name stays as written.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn expand_env(value: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match lookup(name).filter(|_| !name.is_empty()) {
                    Some(found) => out.push_str(&found),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `file:` URL of a sound file path from the scheme. Old schemes name a bare file of
/// `%SystemRoot%\Media`; an empty value means "(None)".
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn sound_file_url(value: &str, system_root: Option<&str>) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let unc = value.strip_prefix("\\\\");
    let drive = value
        .as_bytes()
        .get(1..3)
        .is_some_and(|b| b[0] == b':' && (b[1] == b'\\' || b[1] == b'/'));
    let (mut url, path) = match unc {
        // `\\server\share\x.wav` is `file://server/share/x.wav`.
        Some(rest) => (String::from("file://"), rest.to_owned()),
        None if drive => (String::from("file:///"), value.to_owned()),
        None => (
            String::from("file:///"),
            format!("{}\\Media\\{value}", system_root?.trim_end_matches('\\')),
        ),
    };
    for ch in path.chars() {
        match ch {
            '\\' => url.push('/'),
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '%' => url.push_str("%25"),
            '?' => url.push_str("%3F"),
            _ => url.push(ch),
        }
    }
    Some(url)
}

#[cfg(target_os = "macos")]
mod on_macos {
    use objc2_app_kit::{NSBeep, NSSound};
    use objc2_foundation::NSString;
    use tauri::{AppHandle, Runtime};

    pub(super) fn play<R: Runtime>(app: &AppHandle<R>, name: &str) {
        let name = name.to_owned();
        // AppKit wants its main thread; the sound plays on its own once started.
        let result = app.run_on_main_thread(move || {
            if name == "Default" {
                NSBeep();
                return;
            }
            match NSSound::soundNamed(&NSString::from_str(&name)) {
                Some(sound) => {
                    if !sound.play() {
                        tracing::debug!(sound = %name, "the sound did not start");
                    }
                }
                None => {
                    tracing::debug!(sound = %name, "no system sound of this name; the alert sound plays");
                    NSBeep();
                }
            }
        });
        if let Err(err) = result {
            tracing::debug!(%err, "sound not played");
        }
    }
}

#[cfg(windows)]
mod on_windows {
    use std::sync::{Mutex, PoisonError};

    use windows::Foundation::Uri;
    use windows::Media::Core::MediaSource;
    use windows::Media::Playback::{MediaPlayer, MediaPlayerAudioCategory};
    use windows::core::HSTRING;

    /// The player of the last sound: it stops when dropped, so it lives until the next one.
    static PLAYER: Mutex<Option<MediaPlayer>> = Mutex::new(None);

    pub(super) fn play(name: &str) {
        let event = super::windows_event(name);
        let Some(url) = scheme_file(event) else {
            tracing::debug!(event, "no sound set for this event");
            return;
        };
        if let Err(err) = start(&url) {
            tracing::debug!(%err, event, "sound not played");
        }
    }

    /// The file the current sound scheme plays for `event`, as a `file:` URL.
    fn scheme_file(event: &str) -> Option<String> {
        let path = format!(r"AppEvents\Schemes\Apps\.Default\{event}\.Current");
        let value = windows_registry::CURRENT_USER
            .open(&path)
            .and_then(|key| key.get_string(""))
            .ok()?;
        let lookup = |name: &str| std::env::var(name).ok();
        let expanded = super::expand_env(&value, lookup);
        let root = std::env::var("SystemRoot").ok();
        super::sound_file_url(&expanded, root.as_deref())
    }

    fn start(url: &str) -> windows::core::Result<()> {
        let player = MediaPlayer::new()?;
        player.SetAudioCategory(MediaPlayerAudioCategory::Alerts)?;
        // Not a media session: no entry in the system's media controls.
        player.CommandManager()?.SetIsEnabled(false)?;
        let source = MediaSource::CreateFromUri(&Uri::CreateUri(&HSTRING::from(url))?)?;
        player.SetSource(&source)?;
        player.Play()?;
        let previous = PLAYER
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(player);
        if let Some(previous) = previous {
            let _ = previous.Close();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_sounds_are_scheme_events() {
        for name in mail_model::WINDOWS_SOUNDS {
            assert!(windows_event(name).starts_with("Notification."));
        }
        assert_eq!(windows_event("Mail"), "Notification.Mail");
        assert_eq!(windows_event("Default"), "Notification.Default");
        assert_eq!(windows_event("Fanfare"), "Notification.Default");
    }

    #[test]
    fn scheme_paths_expand_and_become_file_urls() {
        let lookup = |name: &str| (name == "SystemRoot").then(|| r"C:\Windows".to_owned());
        assert_eq!(
            expand_env(r"%SystemRoot%\media\Windows Notify Email.wav", lookup),
            r"C:\Windows\media\Windows Notify Email.wav"
        );
        assert_eq!(expand_env("%Unknown%\\a.wav", lookup), "%Unknown%\\a.wav");
        assert_eq!(expand_env("100%", lookup), "100%");
        assert_eq!(expand_env("%%x", lookup), "%%x");
        assert_eq!(
            sound_file_url(r"C:\Windows\media\Windows Notify Email.wav", None).as_deref(),
            Some("file:///C:/Windows/media/Windows%20Notify%20Email.wav")
        );
        assert_eq!(
            sound_file_url("ding.wav", Some(r"C:\Windows\")).as_deref(),
            Some("file:///C:/Windows/Media/ding.wav")
        );
        assert_eq!(sound_file_url("ding.wav", None), None);
        assert_eq!(sound_file_url("  ", Some(r"C:\Windows")), None, "(None)");
        assert_eq!(
            sound_file_url(r"\\server\share\a#1.wav", None).as_deref(),
            Some("file://server/share/a%231.wav")
        );
    }
}
