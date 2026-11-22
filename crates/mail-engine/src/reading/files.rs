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
    std::fs::set_permissions(&path, permissions).map_err(io)?;
    mark_as_downloaded(&path);
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

/// The mark of the web on Windows; nothing elsewhere (macOS has no equivalent for files an app
/// writes itself without the quarantine API).
#[cfg(windows)]
fn mark_as_downloaded(path: &Path) {
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    if let Err(err) = std::fs::write(&stream, b"[ZoneTransfer]\r\nZoneId=3\r\n") {
        // FAT and exFAT have no alternate data streams.
        tracing::warn!(path = %path.display(), %err, "mark of the web could not be written");
    }
}

#[cfg(not(windows))]
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
}
