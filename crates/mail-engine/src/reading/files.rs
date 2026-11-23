//! Writing attachments to disk (stage 3): saved where the user chose, or copied into the app's
//! own directory to be opened. Synchronous; the core runs these on Tokio's blocking pool.
//!
//! - Saving writes a temporary file next to the target, `sync_all`, then renames it over the
//!   target: the chosen path never holds half a file.
//! - Opening writes a read-only copy to `<open dir>/<random>/<sanitised name>`; the directory is
//!   emptied when the app starts.
//! - Windows: every written file gets the mark of the web (`Zone.Identifier` stream, zone 3 =
//!   internet), so Windows and Office treat it as downloaded (SmartScreen, Protected View).
//!   Written with `std::fs` into the alternate data stream, no `unsafe`.
//! - macOS (stage 15 review): every written file gets the quarantine attribute
//!   (`com.apple.quarantine`) like a browser download or an Apple Mail attachment, so Gatekeeper
//!   checks an app or installer before it runs, also one unpacked from an attached archive.
//!   Written with `/usr/bin/xattr` and fixed arguments, no `unsafe`, no extra dependency.
//! - The mark is set while the file is still writable: neither system lets the owner add it to
//!   a read-only file.

use std::io::Write;
use std::path::{Path, PathBuf};

use mail_model::CoreError;

/// Writes `bytes` to `target` atomically. Returns the final path.
pub(crate) fn save_atomically(target: &Path, bytes: &[u8]) -> Result<PathBuf, CoreError> {
    let io = |what: &'static str| {
        move |err: std::io::Error| CoreError::Internal(format!("{what}: {err}"))
    };
    let dir = target
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| CoreError::InvalidArgument("the target has no directory".into()))?;
    let name = target
        .file_name()
        .ok_or_else(|| CoreError::InvalidArgument("the target has no file name".into()))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(".{}.part", std::process::id()));
    let temp = dir.join(temp_name);
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, target)
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(io("save the attachment")(err));
    }
    mark_as_downloaded(target);
    Ok(target.to_path_buf())
}

/// Most names tried for one file of "save all" (`name (2).ext` … `name (999).ext`).
const MAX_NAME_TRIES: u32 = 999;

/// `name` with a number before its extension: `Report (2).pdf`, `notes (3)`.
fn numbered(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

/// Stage 16 (decision 0036), "save all": writes `bytes` into `dir` under `name`, or under
/// `name (2)`, `name (3)` … when the name is taken. An existing file is never replaced (the
/// name is claimed with `create_new`, also against a file that appears meanwhile). A failed
/// write removes what it started. Returns the path written.
pub(crate) fn save_new_in(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, CoreError> {
    let io = |err: std::io::Error| CoreError::Internal(format!("save the attachment: {err}"));
    if !dir.is_dir() {
        return Err(CoreError::InvalidArgument(
            "the chosen folder does not exist".into(),
        ));
    }
    for n in 1..=MAX_NAME_TRIES {
        let target = dir.join(numbered(name, n));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target);
        let mut file = match file {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(io(err)),
        };
        if let Err(err) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&target);
            return Err(io(err));
        }
        drop(file);
        mark_as_downloaded(&target);
        return Ok(target);
    }
    Err(CoreError::InvalidArgument(format!(
        "{MAX_NAME_TRIES} files named like {name:?} exist in the folder already"
    )))
}

/// Writes a read-only copy for opening: `<dir>/<random hex>/<name>`.
pub(crate) fn write_for_opening(
    dir: &Path,
    random: &str,
    name: &str,
    bytes: &[u8],
) -> Result<PathBuf, CoreError> {
    let io = |err: std::io::Error| {
        CoreError::Internal(format!("copy the attachment for opening: {err}"))
    };
    let folder = dir.join(random);
    std::fs::create_dir_all(&folder).map_err(io)?;
    let path = folder.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(io)?;
    file.write_all(bytes).map_err(io)?;
    file.sync_all().map_err(io)?;
    let mut permissions = file.metadata().map_err(io)?.permissions();
    permissions.set_readonly(true);
    drop(file);
    mark_as_downloaded(&path);
    std::fs::set_permissions(&path, permissions).map_err(io)?;
    Ok(path)
}

/// Empties the directory of copies made for opening (at start). Read-only flags are cleared
/// first (Windows refuses to delete read-only files). Failures are logged.
pub(crate) fn clear_open_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        make_writable(&path);
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(err) = removed {
            tracing::warn!(path = %path.display(), %err, "copy of an opened attachment could not be removed");
        }
    }
}

fn make_writable(path: &Path) {
    let set = |p: &Path| {
        if let Ok(meta) = std::fs::symlink_metadata(p)
            && !meta.file_type().is_symlink()
        {
            let mut permissions = meta.permissions();
            if permissions.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(p, permissions);
            }
        }
    };
    set(path);
    if path.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        for entry in entries.flatten() {
            make_writable(&entry.path());
        }
    }
}

/// The mark of the web on Windows, the quarantine attribute on macOS. A failure is logged; the
/// file stays usable (and opening an executable type still asks first).
#[cfg(windows)]
fn mark_as_downloaded(path: &Path) {
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    if let Err(err) = std::fs::write(&stream, b"[ZoneTransfer]\r\nZoneId=3\r\n") {
        // FAT and exFAT have no alternate data streams.
        tracing::warn!(path = %path.display(), %err, "mark of the web could not be written");
    }
}

/// The value Gatekeeper reads: flags (0x0081: downloaded, as browsers and Mail set it), the time
/// in hexadecimal Unix seconds, the agent's name.
#[cfg(target_os = "macos")]
fn quarantine_value(now: std::time::SystemTime) -> String {
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("0081;{secs:08x};Vice Mail;")
}

#[cfg(target_os = "macos")]
fn mark_as_downloaded(path: &Path) {
    let result = std::process::Command::new("/usr/bin/xattr")
        .arg("-w")
        .arg("com.apple.quarantine")
        .arg(quarantine_value(std::time::SystemTime::now()))
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => {
            tracing::warn!(path = %path.display(), %status, "quarantine attribute could not be written")
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "quarantine attribute could not be written")
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn mark_as_downloaded(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_replaces_the_target_atomically() {
        let dir = tempfile::TempDir::new().expect("temp");
        let target = dir.path().join("Rechnung.pdf");
        std::fs::write(&target, b"old").expect("old");
        let saved = save_atomically(&target, b"new content").expect("save");
        assert_eq!(saved, target);
        assert_eq!(std::fs::read(&target).expect("read"), b"new content");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .expect("dir")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "no temporary file left: {names:?}");
        assert!(save_atomically(&dir.path().join("missing/x.pdf"), b"x").is_err());
    }

    #[test]
    fn copies_for_opening_are_read_only_and_cleared() {
        let dir = tempfile::TempDir::new().expect("temp");
        let path = write_for_opening(dir.path(), "abc", "x.txt", b"hello").expect("write");
        assert_eq!(path, dir.path().join("abc").join("x.txt"));
        assert!(
            std::fs::metadata(&path)
                .expect("meta")
                .permissions()
                .readonly()
        );
        assert!(
            write_for_opening(dir.path(), "abc", "x.txt", b"again").is_err(),
            "never overwrites"
        );
        clear_open_dir(dir.path());
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 0);
        clear_open_dir(&dir.path().join("missing"));
    }

    /// Stage 15 review: on macOS every file the app writes from an attachment is quarantined,
    /// so Gatekeeper checks apps and installers in it (also unpacked from an archive).
    #[cfg(target_os = "macos")]
    #[test]
    fn written_attachments_are_quarantined_on_macos() {
        let quarantine = |path: &Path| {
            let out = std::process::Command::new("/usr/bin/xattr")
                .args(["-p", "com.apple.quarantine"])
                .arg(path)
                .output()
                .expect("xattr");
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        let dir = tempfile::TempDir::new().expect("temp");
        let saved = save_atomically(&dir.path().join("Invoice.zip"), b"PK").expect("save");
        let numbered = save_new_in(dir.path(), "Invoice.zip", b"PK").expect("save all");
        let opened = write_for_opening(dir.path(), "abc", "Setup.pkg", b"xar!").expect("open");
        for path in [&saved, &numbered, &opened] {
            let value = quarantine(path);
            assert!(
                value.starts_with("0081;") && value.ends_with(";Vice Mail;"),
                "{}: {value:?}",
                path.display()
            );
        }
        assert_eq!(
            quarantine_value(std::time::UNIX_EPOCH + std::time::Duration::from_secs(0x6560_0000)),
            "0081;65600000;Vice Mail;"
        );
        clear_open_dir(dir.path());
    }
}
