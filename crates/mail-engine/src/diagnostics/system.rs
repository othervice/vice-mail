//! Facts about the system and the app's folders for the diagnostics export: the operating
//! system's version, folder sizes and the tail of the recent log files. Blocking file IO; the
//! caller runs it on a blocking thread.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::redact::Redactor;
use crate::logging::{LOG_FILE_PREFIX, LOG_FILE_SUFFIX};

/// Newest log files included.
pub(crate) const MAX_LOG_FILES: usize = 3;

/// Bytes of each log file included at most (its end), before and after redaction.
pub(crate) const MAX_LOG_BYTES_PER_FILE: usize = 256 * 1024;

/// Files counted in the data directory before counting stops (a guard, not an expected size).
const MAX_COUNTED_FILES: u64 = 2_000_000;

/// The operating system's version, if it can be read without asking anyone.
pub(crate) fn os_version() -> Option<String> {
    os_version_impl()
}

/// macOS: from the system's version file, as `sw_vers` reads it.
#[cfg(target_os = "macos")]
fn os_version_impl() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let version = plist_string(&plist, "ProductVersion")?;
    Some(match plist_string(&plist, "ProductBuildVersion") {
        Some(build) => format!("{version} ({build})"),
        None => version,
    })
}

/// The `<string>` after `<key>name</key>` in an XML property list; only plain version text.
#[cfg(any(target_os = "macos", test))]
fn plist_string(plist: &str, name: &str) -> Option<String> {
    let after_key = plist.split(&format!("<key>{name}</key>")).nth(1)?;
    let value = after_key
        .trim_start()
        .strip_prefix("<string>")?
        .split("</string>")
        .next()?
        .trim();
    is_version_text(value).then(|| value.to_owned())
}

/// Windows: `ver` of the command interpreter (`Microsoft Windows [Version 10.0.26100.2314]`),
/// without a console window.
#[cfg(windows)]
fn os_version_impl() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "ver"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text
        .split("Version")
        .nth(1)?
        .trim()
        .trim_end_matches(']')
        .trim();
    is_version_text(version).then(|| version.to_owned())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn os_version_impl() -> Option<String> {
    None
}

/// Digits, dots, letters, blanks and parentheses only, and short: nothing but a version.
pub(crate) fn is_version_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ' ' | '(' | ')' | '-'))
}

/// Size of a folder and of its top-level entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FolderSize {
    pub bytes: u64,
    pub files: u64,
    /// Top-level entries by name (names the app chose; others are summed up as `other`).
    pub entries: Vec<(String, u64, u64)>,
    /// Counting stopped at [`MAX_COUNTED_FILES`].
    pub incomplete: bool,
}

/// Walks `dir` without following links. `None` if it cannot be read at all.
pub(crate) fn folder_size(dir: &Path) -> Option<FolderSize> {
    let mut size = FolderSize::default();
    for entry in std::fs::read_dir(dir).ok()?.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let name = if is_plain_name(&name) {
            name
        } else {
            "other".to_owned()
        };
        let (mut bytes, mut files) = (0, 0);
        walk(&entry.path(), &mut bytes, &mut files, &mut size);
        size.bytes += bytes;
        size.files += files;
        match size.entries.iter_mut().find(|(n, _, _)| *n == name) {
            Some((_, b, f)) => {
                *b += bytes;
                *f += files;
            }
            None => size.entries.push((name, bytes, files)),
        }
    }
    size.entries.sort_by(|a, b| a.0.cmp(&b.0));
    Some(size)
}

fn walk(path: &Path, bytes: &mut u64, files: &mut u64, total: &mut FolderSize) {
    if total.files + *files >= MAX_COUNTED_FILES {
        total.incomplete = true;
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        if let Ok(children) = std::fs::read_dir(path) {
            for child in children.filter_map(Result::ok) {
                walk(&child.path(), bytes, files, total);
            }
        }
    } else if meta.is_file() {
        *bytes += meta.len();
        *files += 1;
    }
}

/// A name the app itself writes (letters, digits, `.`, `-`, `_`), never one a person chose.
pub(crate) fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// The end of one log file, redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogTail {
    pub name: String,
    pub size: u64,
    /// Redacted lines, at most [`MAX_LOG_BYTES_PER_FILE`] bytes together.
    pub lines: Vec<String>,
    /// Earlier lines were left out.
    pub cut: bool,
}

/// The newest [`MAX_LOG_FILES`] log files of `dir` (`app.YYYY-MM-DD.log`), newest first, each
/// with its last [`MAX_LOG_BYTES_PER_FILE`] bytes redacted line by line. A file that cannot be
/// read is listed with the error kind instead of its lines.
pub(crate) fn recent_logs(dir: &Path, redactor: &Redactor) -> Result<Vec<LogTail>, String> {
    let entries = std::fs::read_dir(dir).map_err(|err| io_kind(&err))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| is_log_name(name))
        .collect();
    // The date in the name sorts them in time.
    names.sort_by(|a, b| b.cmp(a));
    names.truncate(MAX_LOG_FILES);
    Ok(names
        .into_iter()
        .map(|name| match tail(&dir.join(&name), redactor) {
            Ok((size, lines, cut)) => LogTail {
                name,
                size,
                lines,
                cut,
            },
            Err(err) => LogTail {
                name,
                size: 0,
                lines: vec![format!("(the file could not be read: {})", io_kind(&err))],
                cut: false,
            },
        })
        .collect())
}

fn is_log_name(name: &str) -> bool {
    name.strip_prefix(&format!("{LOG_FILE_PREFIX}."))
        .and_then(|rest| rest.strip_suffix(&format!(".{LOG_FILE_SUFFIX}")))
        .is_some_and(|date| {
            !date.is_empty() && date.bytes().all(|b| b.is_ascii_digit() || b == b'-')
        })
}

/// The last bytes of `path`, from the first complete line, redacted; then cut from the front
/// until the redacted lines fit the budget again.
fn tail(path: &Path, redactor: &Redactor) -> std::io::Result<(u64, Vec<String>, bool)> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let budget = MAX_LOG_BYTES_PER_FILE as u64;
    let start = size.saturating_sub(budget);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity(usize::try_from(size - start).unwrap_or(0));
    file.take(budget).read_to_end(&mut bytes)?;
    let mut cut = start > 0;
    let text = String::from_utf8_lossy(&bytes);
    let mut text: &str = &text;
    if cut {
        // The first line is most likely a part; start after it.
        text = text.split_once('\n').map_or("", |(_, rest)| rest);
    }
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| redactor.line(line))
        .collect();
    let mut total: usize = lines.iter().map(|l| l.len() + 1).sum();
    let mut drop = 0;
    while total > MAX_LOG_BYTES_PER_FILE && drop < lines.len() {
        total -= lines.get(drop).map_or(0, |l| l.len() + 1);
        drop += 1;
    }
    if drop > 0 {
        lines.drain(..drop);
        cut = true;
    }
    Ok((size, lines, cut))
}

/// The kind of an IO error, without the path or anything else from the system's text.
pub(crate) fn io_kind(err: &std::io::Error) -> String {
    format!("{:?}", err.kind())
}

/// The value of an environment variable, if set and not empty.
pub(crate) fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// The user's home directory, as paths in the logs start with it.
pub(crate) fn home_dir() -> Option<String> {
    if cfg!(windows) {
        env("USERPROFILE")
    } else {
        env("HOME")
    }
}

/// The system's user name.
pub(crate) fn user_name() -> Option<String> {
    if cfg!(windows) {
        env("USERNAME")
    } else {
        env("USER")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_from_a_property_list() {
        let plist = "<dict>\n\t<key>ProductBuildVersion</key>\n\t<string>25B78</string>\n\
                     \t<key>ProductVersion</key>\n\t<string>26.1</string>\n</dict>";
        assert_eq!(
            plist_string(plist, "ProductVersion").as_deref(),
            Some("26.1")
        );
        assert_eq!(
            plist_string(plist, "ProductBuildVersion").as_deref(),
            Some("25B78")
        );
        assert_eq!(plist_string(plist, "Missing"), None);
        assert_eq!(
            plist_string(
                "<key>ProductVersion</key><string><b>x</b></string>",
                "ProductVersion"
            ),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_running_macos_version_is_found() {
        let version = os_version().expect("macOS version");
        assert!(
            version.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "{version}"
        );
    }

    #[test]
    fn counts_files_and_hides_names_a_person_chose() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("mail.sqlite3"), [0u8; 100]).expect("db");
        std::fs::create_dir_all(dir.path().join("bodies/ab")).expect("bodies");
        std::fs::write(dir.path().join("bodies/ab/1.zst"), [0u8; 10]).expect("body");
        std::fs::write(dir.path().join("bodies/ab/2.zst"), [0u8; 20]).expect("body");
        std::fs::write(dir.path().join("Rechnung März.pdf"), [0u8; 5]).expect("odd file");
        let size = folder_size(dir.path()).expect("size");
        assert_eq!((size.bytes, size.files, size.incomplete), (135, 4, false));
        assert_eq!(
            size.entries,
            vec![
                ("bodies".to_owned(), 30, 2),
                ("mail.sqlite3".to_owned(), 100, 1),
                ("other".to_owned(), 5, 1),
            ]
        );
        assert!(folder_size(&dir.path().join("missing")).is_none());
    }

    #[test]
    fn log_names_are_the_daily_files_only() {
        assert!(is_log_name("app.2026-11-23.log"));
        for name in [
            "app.log",
            "other.2026-11-23.log",
            "app.2026-11-23.log.bak",
            "app.x.log",
        ] {
            assert!(!is_log_name(name), "{name}");
        }
    }
}
