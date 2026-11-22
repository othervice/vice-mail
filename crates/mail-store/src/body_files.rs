//! Body files (decision 0009): zstd compression, atomic writes, reads, removal and the sweep of
//! files nothing refers to. Synchronous; `body_api` runs it on Tokio's blocking pool, never on a
//! database thread or a Tokio worker.
//!
//! Layout: `<bodies>/<account uuid>/<message id>-<unique>.zst`. The unique part (time and a
//! counter) makes every write a new file: a re-fetch, two concurrent fetches of one message or
//! a reused message id never overwrite a file another row still points to. The database row is
//! written after the file and removed before it, so a crash leaves at most an unreferenced file,
//! which [`sweep`] removes later.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use mail_model::MessageId;

/// zstd level (decision 0009): the library default; level 1 saves little time, higher levels
/// little space on mail.
pub(crate) const ZSTD_LEVEL: i32 = 3;

/// Largest decompressed file this build reads back (a guard against a corrupt size field).
pub(crate) const MAX_FILE_SIZE: u64 = 1 << 30;

/// Prefix of files being written. Never referenced by the database.
const TEMP_PREFIX: &str = ".tmp-";

static UNIQUE: AtomicU64 = AtomicU64::new(0);

/// Relative path of a new body file of `message` in the account's directory.
pub(crate) fn new_file_name(account_uuid: &str, message: MessageId, now_ms: i64) -> String {
    let n = UNIQUE.fetch_add(1, Ordering::Relaxed);
    format!("{account_uuid}/{}-{now_ms:x}{n:x}.zst", message.0)
}

/// zstd frame with content size and checksum, so corruption is detected on read.
pub(crate) fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut compressor = zstd::bulk::Compressor::new(ZSTD_LEVEL)?;
    compressor.set_parameter(zstd::stream::raw::CParameter::ChecksumFlag(true))?;
    compressor.compress(data)
}

/// Inverse of [`compress`]. Fails on a damaged or foreign file (checksum, size, format).
pub(crate) fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let size = zstd::zstd_safe::get_frame_content_size(data)
        .map_err(|err| format!("not a zstd frame: {err:?}"))?
        .ok_or_else(|| "zstd frame without content size".to_owned())?;
    if size > MAX_FILE_SIZE {
        return Err(format!("content size {size} is implausible"));
    }
    let capacity = usize::try_from(size).map_err(|err| err.to_string())?;
    let out = zstd::bulk::decompress(data, capacity).map_err(|err| err.to_string())?;
    if out.len() != capacity {
        return Err(format!(
            "decompressed {} bytes, frame says {capacity}",
            out.len()
        ));
    }
    Ok(out)
}

/// Writes `data` to `root/relative` atomically: a temporary file in the same directory,
/// written, flushed to disk (`sync_all`), renamed over the target, then the directory synced
/// (Unix), so after a crash the target is complete or absent. Returns the bytes written.
pub(crate) fn write_atomically(root: &Path, relative: &str, data: &[u8]) -> io::Result<u64> {
    let target = resolve(root, relative)?;
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other("body file without directory"))?;
    fs::create_dir_all(dir)?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("body file name is not UTF-8"))?;
    let temp = dir.join(format!(
        "{TEMP_PREFIX}{}-{:x}-{name}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &target)?;
        sync_dir(dir)?;
        u64::try_from(data.len()).map_err(io::Error::other)
    })();
    if written.is_err() {
        // Best effort; a leftover temporary file is removed by the sweep.
        let _ = fs::remove_file(&temp);
    }
    written
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// Windows cannot open a directory as a file without extra flags; NTFS journals the rename.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn read(root: &Path, relative: &str) -> io::Result<Vec<u8>> {
    fs::read(resolve(root, relative)?)
}

/// Removes these files; missing ones count as removed. Returns the files that could not be
/// removed (logged; the sweep tries again).
pub(crate) fn remove_files(root: &Path, files: &[String]) -> Vec<String> {
    let mut failed = Vec::new();
    for relative in files {
        let result = resolve(root, relative).and_then(|path| match fs::remove_file(&path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        });
        if let Err(err) = result {
            tracing::warn!(file = %relative, error = %err, "could not remove body file");
            failed.push(relative.clone());
        }
    }
    failed
}

/// Removes the directory of an account with everything in it. A missing directory is fine.
pub(crate) fn remove_account_dir(root: &Path, account_uuid: &str) -> io::Result<()> {
    let dir = resolve(root, account_uuid)?;
    match fs::remove_dir_all(&dir) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// What [`sweep`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Files nothing in the database refers to (including temporary files).
    pub removed_files: usize,
    /// Directories of accounts that no longer exist.
    pub removed_dirs: usize,
    pub removed_bytes: u64,
    /// Entries that could not be removed (logged).
    pub failed: usize,
}

/// Removes, below `root`, account directories not in `accounts` and files not in `referenced`
/// (relative paths), but only entries last modified more than `min_age` ago: a file written a
/// moment ago may be about to be referenced.
pub(crate) fn sweep(
    root: &Path,
    accounts: &HashSet<String>,
    referenced: &HashSet<String>,
    min_age: Duration,
) -> io::Result<SweepReport> {
    let mut report = SweepReport::default();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(report),
        Err(err) => return Err(err),
    };
    let now = SystemTime::now();
    let old_enough = |meta: &fs::Metadata| {
        meta.modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= min_age)
    };
    for entry in entries {
        let entry = entry?;
        let meta = entry.metadata()?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !meta.is_dir() {
            // Nothing but account directories belongs at the top level.
            if old_enough(&meta) {
                remove_counted(&entry.path(), meta.len(), &mut report);
            }
            continue;
        }
        if !accounts.contains(&name) {
            if old_enough(&meta) {
                let bytes = dir_size(&entry.path());
                match fs::remove_dir_all(entry.path()) {
                    Ok(()) => {
                        report.removed_dirs += 1;
                        report.removed_bytes += bytes;
                    }
                    Err(err) => {
                        tracing::warn!(dir = %name, error = %err, "could not remove body directory");
                        report.failed += 1;
                    }
                }
            }
            continue;
        }
        for file in fs::read_dir(entry.path())? {
            let file = file?;
            let meta = file.metadata()?;
            let Some(file_name) = file.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let relative = format!("{name}/{file_name}");
            if meta.is_file() && !referenced.contains(&relative) && old_enough(&meta) {
                remove_counted(&file.path(), meta.len(), &mut report);
            }
        }
    }
    Ok(report)
}

fn remove_counted(path: &Path, bytes: u64, report: &mut SweepReport) {
    match fs::remove_file(path) {
        Ok(()) => {
            report.removed_files += 1;
            report.removed_bytes += bytes;
        }
        Err(err) => {
            tracing::warn!(file = %path.display(), error = %err, "could not remove body file");
            report.failed += 1;
        }
    }
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .filter(fs::Metadata::is_file)
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// `root/relative`, refusing anything but plain names separated by `/` (no `..`, no absolute
/// paths, no backslashes), so a damaged database row can never point outside `root`.
fn resolve(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let valid = !relative.is_empty()
        && relative.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
        });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid body file path {relative:?}"),
        ));
    }
    let mut path = root.to_path_buf();
    path.extend(relative.split('/'));
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_round_trip_and_corruption_is_detected() {
        let data = b"Subject: hello\r\n\r\n".repeat(500);
        let compressed = compress(&data).expect("compress");
        assert!(compressed.len() < data.len() / 10);
        assert_eq!(decompress(&compressed).expect("decompress"), data);
        assert_eq!(
            decompress(&compress(b"").expect("empty")).expect("empty"),
            b""
        );

        // One flipped bit in the payload: the content checksum catches it.
        let mut damaged = compressed.clone();
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x10;
        assert!(decompress(&damaged).is_err());
        // Truncated, foreign data, empty file.
        assert!(decompress(&compressed[..compressed.len() - 4]).is_err());
        assert!(decompress(b"Subject: not compressed").is_err());
        assert!(decompress(b"").is_err());
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bodies");
        let name = new_file_name("acc", MessageId(42), 1_000);
        assert!(name.starts_with("acc/42-"), "{name}");
        assert_ne!(name, new_file_name("acc", MessageId(42), 1_000));
        assert_eq!(write_atomically(&root, &name, b"first").expect("write"), 5);
        assert_eq!(write_atomically(&root, &name, b"second").expect("write"), 6);
        assert_eq!(read(&root, &name).expect("read"), b"second");
        let entries: Vec<String> = fs::read_dir(root.join("acc"))
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 1, "{entries:?}");

        assert!(remove_files(&root, &[name.clone(), "acc/missing.zst".into()]).is_empty());
        assert!(read(&root, &name).is_err());
        remove_account_dir(&root, "acc").expect("remove dir");
        remove_account_dir(&root, "acc").expect("missing dir is fine");
        assert!(!root.join("acc").exists());
    }

    #[test]
    fn paths_cannot_leave_the_root() {
        let root = Path::new("/data/bodies");
        assert_eq!(
            resolve(root, "acc/1-ab.zst").expect("valid"),
            root.join("acc").join("1-ab.zst")
        );
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "acc/../../x",
            "acc/./x",
            "acc//x",
            "acc\\x",
            "acc/x y",
            "C:x",
        ] {
            assert!(resolve(root, bad).is_err(), "{bad:?}");
            assert!(read(root, bad).is_err(), "{bad:?}");
        }
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(write_atomically(dir.path(), "../escape.zst", b"x").is_err());
        assert!(
            !dir.path()
                .parent()
                .expect("parent")
                .join("escape.zst")
                .exists()
        );
    }

    #[test]
    fn sweep_removes_only_old_unreferenced_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        for (relative, data) in [
            ("a/1-x.zst", "kept, referenced"),
            ("a/2-x.zst", "unreferenced"),
            ("a/.tmp-1-2-3-x.zst", "leftover temporary file"),
            ("gone/9-x.zst", "account removed"),
        ] {
            write_atomically(&root, relative, data.as_bytes()).expect("write");
        }
        fs::write(root.join("stray.txt"), "x").expect("stray file");
        let accounts: HashSet<String> = ["a".to_owned()].into();
        let referenced: HashSet<String> = ["a/1-x.zst".to_owned()].into();

        // Everything is younger than an hour: nothing goes.
        let report =
            sweep(&root, &accounts, &referenced, Duration::from_secs(3600)).expect("sweep");
        assert_eq!(report, SweepReport::default());

        let report = sweep(&root, &accounts, &referenced, Duration::ZERO).expect("sweep");
        assert_eq!(report.removed_files, 3, "{report:?}");
        assert_eq!(report.removed_dirs, 1);
        assert_eq!(report.failed, 0);
        assert!(report.removed_bytes > 0);
        assert!(root.join("a/1-x.zst").exists());
        assert!(!root.join("a/2-x.zst").exists());
        assert!(!root.join("gone").exists());

        let missing = sweep(
            &root.join("nothing"),
            &accounts,
            &referenced,
            Duration::ZERO,
        )
        .expect("missing root is fine");
        assert_eq!(missing, SweepReport::default());
    }
}
