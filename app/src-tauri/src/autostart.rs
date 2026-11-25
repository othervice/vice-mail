//! Stage 21 (decision 0070): "Start with the system". Off by default; on, the system starts the
//! app at login with [`crate::lifecycle::BACKGROUND_ARG`], so it runs without a window (sync,
//! popup, notifications, tray icon) until the user opens one.
//!
//! - **macOS:** a launch agent of the user, `~/Library/LaunchAgents/<bundle id>.plist`
//!   (`RunAtLoad`, the app's executable with `--background`, `AssociatedBundleIdentifiers` so
//!   System Settings → General → Login Items names the app). launchd reads it at the next login;
//!   nothing starts now. macOS 13 and later list it under "Allow in the Background", where the
//!   user can switch it off as well. `SMAppService` would be the newer way, but its main-app
//!   login item cannot pass an argument and `objc2-service-management` had no release in the last
//!   12 months (CLAUDE.md).
//! - **Windows:** the value `<product name>` under `HKCU\Software\Microsoft\Windows\
//!   CurrentVersion\Run` with the quoted executable and `--background`. Task Manager's
//!   "Startup apps" switch writes `…\Explorer\StartupApproved\Run\<name>` (first byte even:
//!   enabled, odd: disabled); switching on here writes "enabled" there, so this switch and Task
//!   Manager agree. The uninstaller removes both values (`windows/installer-hooks.nsh`).
//! - **Development builds** (debug, not the desktop test) do not register themselves: a login item
//!   pointing into `target/` would start a stale build. **Desktop-test builds** write a launch
//!   agent file into the test's own directory, never the user's.
//!
//! The registration is the truth: the switch shows what the system has, also after the user
//! changed it there. At start an enabled registration that names another executable (the app was
//! moved) is rewritten for this one.

use std::io;
use std::path::{Path, PathBuf};

/// What the system has registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Windows: not switched off in Task Manager. macOS: always (the switch in System Settings
    /// cannot be read without private API).
    pub approved: bool,
}

/// Where the login item lives: the launch agent file, the registry, or a test's memory.
pub trait LoginItems: Send + Sync {
    fn read(&self) -> io::Result<Option<Registration>>;
    fn write(&self, program: &Path, args: &[String]) -> io::Result<()>;
    fn remove(&self) -> io::Result<()>;
}

// ---------------------------------------------------------------------------------------------
// macOS (and the desktop test everywhere): a launch agent file

#[cfg_attr(all(windows, not(feature = "e2e")), allow(dead_code))]
/// A user launch agent at `path` with `label`.
pub struct LaunchAgent {
    pub path: PathBuf,
    pub label: String,
}

#[cfg_attr(all(windows, not(feature = "e2e")), allow(dead_code))]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg_attr(all(windows, not(feature = "e2e")), allow(dead_code))]
fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg_attr(all(windows, not(feature = "e2e")), allow(dead_code))]
/// The launch agent's property list.
pub fn launch_agent_plist(label: &str, program: &Path, args: &[String]) -> String {
    let mut arguments = format!(
        "        <string>{}</string>\n",
        xml_escape(&program.to_string_lossy())
    );
    for arg in args {
        arguments.push_str(&format!("        <string>{}</string>\n", xml_escape(arg)));
    }
    let label = xml_escape(label);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>AssociatedBundleIdentifiers</key>
    <array>
        <string>{label}</string>
    </array>
    <key>ProgramArguments</key>
    <array>
{arguments}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#
    )
}

#[cfg_attr(all(windows, not(feature = "e2e")), allow(dead_code))]
/// `ProgramArguments` of a property list this module wrote: the program first.
pub fn plist_arguments(text: &str) -> Option<Vec<String>> {
    let start = text.find("<key>ProgramArguments</key>")?;
    let rest = &text[start..];
    let open = rest.find("<array>")?;
    let close = rest.find("</array>")?;
    let array = rest.get(open + "<array>".len()..close)?;
    let mut values = Vec::new();
    let mut tail = array;
    while let Some(begin) = tail.find("<string>") {
        let after = &tail[begin + "<string>".len()..];
        let end = after.find("</string>")?;
        values.push(xml_unescape(&after[..end]));
        tail = &after[end + "</string>".len()..];
    }
    (!values.is_empty()).then_some(values)
}

impl LoginItems for LaunchAgent {
    fn read(&self) -> io::Result<Option<Registration>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let Some(mut values) = plist_arguments(&text) else {
            return Ok(None);
        };
        let program = PathBuf::from(values.remove(0));
        Ok(Some(Registration {
            program,
            args: values,
            approved: true,
        }))
    }

    fn write(&self, program: &Path, args: &[String]) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Written beside and renamed, so launchd never reads half a file.
        let partial = self.path.with_extension("plist.partial");
        std::fs::write(&partial, launch_agent_plist(&self.label, program, args))?;
        std::fs::rename(&partial, &self.path)
    }

    fn remove(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Windows: the Run key

#[cfg_attr(not(windows), allow(dead_code))]
/// The command line of the Run value: the program quoted, then the arguments (ours never contain
/// spaces or quotes).
pub fn command_line(program: &Path, args: &[String]) -> String {
    let mut line = format!("\"{}\"", program.to_string_lossy());
    for arg in args {
        line.push(' ');
        line.push_str(arg);
    }
    line
}

#[cfg_attr(not(windows), allow(dead_code))]
/// The program and arguments of a Run value (quoted program, or unquoted up to the first space).
pub fn parse_command_line(line: &str) -> Option<(PathBuf, Vec<String>)> {
    let line = line.trim();
    let (program, rest) = if let Some(quoted) = line.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], &quoted[end + 1..])
    } else {
        match line.split_once(' ') {
            Some((program, rest)) => (program, rest),
            None => (line, ""),
        }
    };
    if program.is_empty() {
        return None;
    }
    Some((
        PathBuf::from(program),
        rest.split_whitespace().map(str::to_owned).collect(),
    ))
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Task Manager's state of a Run entry (`StartupApproved\Run`): an even first byte is enabled,
/// an odd one disabled; no value means enabled.
pub fn approved(state: Option<&[u8]>) -> bool {
    state
        .and_then(|bytes| bytes.first())
        .is_none_or(|first| first % 2 == 0)
}

#[cfg_attr(not(windows), allow(dead_code))]
/// What this app writes to `StartupApproved\Run` when switched on: enabled, no time stamp.
pub const APPROVED_ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[cfg(windows)]
pub use run_key::RunKey;

#[cfg(windows)]
mod run_key {
    use std::io;
    use std::path::Path;

    use windows_registry::CURRENT_USER;

    use super::{
        APPROVED_ENABLED, LoginItems, Registration, approved, command_line, parse_command_line,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    /// The value `name` under the user's Run key.
    pub struct RunKey {
        pub name: String,
    }

    fn io_error(err: windows_registry::Result<()>) -> io::Result<()> {
        err.map_err(|e| io::Error::other(e.to_string()))
    }

    impl LoginItems for RunKey {
        fn read(&self) -> io::Result<Option<Registration>> {
            let Ok(run) = CURRENT_USER.open(RUN) else {
                return Ok(None);
            };
            let Ok(line) = run.get_string(&self.name) else {
                return Ok(None);
            };
            let Some((program, args)) = parse_command_line(&line) else {
                return Ok(None);
            };
            let state = CURRENT_USER
                .open(APPROVED)
                .ok()
                .and_then(|key| key.get_value(&self.name).ok());
            Ok(Some(Registration {
                program,
                args,
                approved: approved(state.as_deref()),
            }))
        }

        fn write(&self, program: &Path, args: &[String]) -> io::Result<()> {
            let run = CURRENT_USER
                .create(RUN)
                .map_err(|e| io::Error::other(e.to_string()))?;
            io_error(run.set_string(&self.name, command_line(program, args)))?;
            // Task Manager's switch follows ours.
            let approved = CURRENT_USER
                .create(APPROVED)
                .map_err(|e| io::Error::other(e.to_string()))?;
            io_error(approved.set_bytes(
                &self.name,
                windows_registry::Type::Bytes,
                &APPROVED_ENABLED,
            ))
        }

        fn remove(&self) -> io::Result<()> {
            for path in [RUN, APPROVED] {
                if let Ok(key) = CURRENT_USER.options().read().write().open(path) {
                    // Absent already is fine.
                    let _ = key.remove_value(&self.name);
                }
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The switch

/// "Start with the system" for this executable.
pub struct Autostart {
    items: Box<dyn LoginItems>,
    exe: PathBuf,
    /// Why this build cannot register itself, if it cannot.
    unavailable: Option<&'static str>,
}

impl Autostart {
    pub fn new(
        items: Box<dyn LoginItems>,
        exe: PathBuf,
        unavailable: Option<&'static str>,
    ) -> Self {
        Self {
            items,
            exe,
            unavailable,
        }
    }

    fn args() -> Vec<String> {
        vec![crate::lifecycle::BACKGROUND_ARG.to_owned()]
    }

    /// What the switch shows: registered (and not switched off in the system) or not.
    pub fn status(&self) -> mail_model::Autostart {
        let enabled = self.unavailable.is_none()
            && match self.items.read() {
                Ok(registration) => registration.is_some_and(|r| r.approved),
                Err(err) => {
                    tracing::warn!(%err, "login item unreadable");
                    false
                }
            };
        mail_model::Autostart {
            enabled,
            available: self.unavailable.is_none(),
        }
    }

    /// Registers or removes the login item.
    pub fn set(&self, enabled: bool) -> io::Result<mail_model::Autostart> {
        if let Some(reason) = self.unavailable {
            return Err(io::Error::new(io::ErrorKind::Unsupported, reason));
        }
        if enabled {
            self.items.write(&self.exe, &Self::args())?;
        } else {
            self.items.remove()?;
        }
        tracing::info!(enabled, "start with the system changed");
        Ok(self.status())
    }

    /// At start: an enabled registration that names another executable or other arguments is
    /// rewritten for this one (the app was moved or updated). Returns whether it rewrote.
    pub fn repair(&self) -> bool {
        if self.unavailable.is_some() {
            return false;
        }
        match self.items.read() {
            Ok(Some(registration))
                if registration.approved
                    && (registration.program != self.exe || registration.args != Self::args()) =>
            {
                match self.items.write(&self.exe, &Self::args()) {
                    Ok(()) => {
                        tracing::info!("login item now names this executable");
                        true
                    }
                    Err(err) => {
                        tracing::warn!(%err, "login item not repaired");
                        false
                    }
                }
            }
            Ok(_) => false,
            Err(err) => {
                tracing::warn!(%err, "login item unreadable");
                false
            }
        }
    }
}

#[cfg_attr(any(feature = "e2e", not(target_os = "macos")), allow(dead_code))]
/// Gatekeeper's App Translocation runs a quarantined app from a random read-only path; a login
/// item naming that path would break at the next start.
pub fn translocated(exe: &Path) -> bool {
    exe.to_string_lossy().contains("/AppTranslocation/")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// The system's registry or launch agent folder, in memory.
    #[derive(Default)]
    struct Fake {
        entry: Mutex<Option<Registration>>,
        broken: bool,
    }

    impl LoginItems for std::sync::Arc<Fake> {
        fn read(&self) -> io::Result<Option<Registration>> {
            if self.broken {
                return Err(io::Error::other("access denied"));
            }
            Ok(self.entry.lock().expect("lock").clone())
        }

        fn write(&self, program: &Path, args: &[String]) -> io::Result<()> {
            *self.entry.lock().expect("lock") = Some(Registration {
                program: program.to_owned(),
                args: args.to_vec(),
                approved: true,
            });
            Ok(())
        }

        fn remove(&self) -> io::Result<()> {
            *self.entry.lock().expect("lock") = None;
            Ok(())
        }
    }

    fn exe() -> PathBuf {
        PathBuf::from("/Applications/Vice Mail.app/Contents/MacOS/mail-app")
    }

    #[test]
    fn switching_on_and_off_registers_this_executable_in_the_background() {
        let fake = std::sync::Arc::new(Fake::default());
        let autostart = Autostart::new(Box::new(std::sync::Arc::clone(&fake)), exe(), None);
        assert_eq!(
            autostart.status(),
            mail_model::Autostart {
                enabled: false,
                available: true
            }
        );
        assert!(autostart.set(true).expect("on").enabled);
        assert_eq!(
            fake.entry.lock().expect("lock").clone(),
            Some(Registration {
                program: exe(),
                args: vec!["--background".into()],
                approved: true
            })
        );
        assert!(!autostart.set(false).expect("off").enabled);
        assert_eq!(*fake.entry.lock().expect("lock"), None);
    }

    #[test]
    fn the_system_s_own_switch_wins() {
        let fake = std::sync::Arc::new(Fake::default());
        *fake.entry.lock().expect("lock") = Some(Registration {
            program: exe(),
            args: vec!["--background".into()],
            approved: false,
        });
        let autostart = Autostart::new(Box::new(std::sync::Arc::clone(&fake)), exe(), None);
        assert!(!autostart.status().enabled, "switched off in Task Manager");
        assert!(!autostart.repair(), "a switched-off entry stays as it is");
        // Switching on here switches it on there too.
        assert!(autostart.set(true).expect("on").enabled);
        // Unreadable: shown as off, no repair.
        let broken = std::sync::Arc::new(Fake {
            broken: true,
            ..Fake::default()
        });
        let autostart = Autostart::new(Box::new(std::sync::Arc::clone(&broken)), exe(), None);
        assert!(!autostart.status().enabled);
        assert!(!autostart.repair());
    }

    #[test]
    fn a_moved_app_repairs_its_entry() {
        let fake = std::sync::Arc::new(Fake::default());
        *fake.entry.lock().expect("lock") = Some(Registration {
            program: PathBuf::from("/Users/me/Downloads/Vice Mail.app/Contents/MacOS/mail-app"),
            args: vec!["--background".into()],
            approved: true,
        });
        let autostart = Autostart::new(Box::new(std::sync::Arc::clone(&fake)), exe(), None);
        assert!(autostart.repair());
        assert_eq!(
            fake.entry
                .lock()
                .expect("lock")
                .as_ref()
                .map(|r| r.program.clone()),
            Some(exe())
        );
        assert!(!autostart.repair(), "nothing left to repair");
    }

    #[test]
    fn development_builds_do_not_register() {
        let fake = std::sync::Arc::new(Fake::default());
        let autostart = Autostart::new(
            Box::new(std::sync::Arc::clone(&fake)),
            exe(),
            Some("development build"),
        );
        assert_eq!(
            autostart.status(),
            mail_model::Autostart {
                enabled: false,
                available: false
            }
        );
        assert!(autostart.set(true).is_err());
        assert_eq!(*fake.entry.lock().expect("lock"), None);
    }

    #[test]
    fn the_launch_agent_round_trips_through_its_file() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let agent = LaunchAgent {
            path: dir.path().join("LaunchAgents/org.vicemail.desktop.plist"),
            label: "org.vicemail.desktop".into(),
        };
        assert_eq!(agent.read().expect("read"), None);
        let program = PathBuf::from("/Applications/A & B <x>.app/Contents/MacOS/mail-app");
        agent
            .write(&program, &["--background".into()])
            .expect("write");
        let text = std::fs::read_to_string(&agent.path).expect("plist");
        assert!(text.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(text.contains("<string>org.vicemail.desktop</string>"));
        assert!(text.contains("A &amp; B &lt;x&gt;.app"));
        assert!(
            !dir.path()
                .join("LaunchAgents/org.vicemail.desktop.plist.partial")
                .exists()
        );
        assert_eq!(
            agent.read().expect("read"),
            Some(Registration {
                program,
                args: vec!["--background".into()],
                approved: true
            })
        );
        agent.remove().expect("remove");
        agent.remove().expect("remove twice");
        assert_eq!(agent.read().expect("read"), None);
    }

    #[test]
    fn the_plist_is_valid_xml_for_plutil() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("agent.plist");
        std::fs::write(
            &path,
            launch_agent_plist("org.vicemail.desktop", &exe(), &["--background".into()]),
        )
        .expect("write");
        let status = std::process::Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&path)
            .status()
            .expect("plutil");
        assert!(status.success());
    }

    #[test]
    fn run_values_and_task_manager_states() {
        let program = PathBuf::from(r"C:\Users\me\AppData\Local\Vice Mail\mail-app.exe");
        let line = command_line(&program, &["--background".into()]);
        assert_eq!(
            line,
            r#""C:\Users\me\AppData\Local\Vice Mail\mail-app.exe" --background"#
        );
        assert_eq!(
            parse_command_line(&line),
            Some((program, vec!["--background".into()]))
        );
        assert_eq!(
            parse_command_line(r"C:\tools\app.exe -x"),
            Some((PathBuf::from(r"C:\tools\app.exe"), vec!["-x".into()]))
        );
        assert_eq!(parse_command_line(r#""unterminated"#), None);
        assert_eq!(parse_command_line(""), None);
        assert!(approved(None));
        assert!(approved(Some(&APPROVED_ENABLED)));
        assert!(approved(Some(&[6, 0, 0])));
        assert!(!approved(Some(&[3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])));
        assert!(approved(Some(&[])));
    }

    #[test]
    fn translocated_apps_are_recognised() {
        assert!(translocated(Path::new(
            "/private/var/folders/x/AppTranslocation/1234/d/Vice Mail.app/Contents/MacOS/mail-app"
        )));
        assert!(!translocated(&exe()));
    }
}
