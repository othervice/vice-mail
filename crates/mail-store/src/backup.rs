//! Backup copies taken before a migration. `VACUUM INTO` writes a consistent snapshot through
//! the open connection (WAL contents included), unlike a file copy next to a live WAL.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::StoreError;

/// Number of backups kept per database file.
pub const BACKUPS_KEPT: usize = 3;

/// Directory next to the database that holds its backups.
pub fn backup_dir(db_path: &Path) -> PathBuf {
    db_path.with_file_name("backups")
}

/// Writes a backup of the database behind `conn` (schema version `version`) and prunes old
/// backups down to [`BACKUPS_KEPT`]. Returns the path of the new backup.
pub(crate) fn create(
    conn: &Connection,
    db_path: &Path,
    version: usize,
) -> Result<PathBuf, StoreError> {
    let dir = backup_dir(db_path);
    fs::create_dir_all(&dir).map_err(StoreError::io("create backup directory"))?;
    let stem = file_stem(db_path);

    // Name: <stem>-<unix ms, zero padded>-v<schema version>.sqlite3, so names sort by age.
    let mut millis = now_millis();
    let target = loop {
        let candidate = dir.join(format!("{stem}-{millis:015}-v{version}.sqlite3"));
        if !candidate.exists() && !partial_path(&candidate).exists() {
            break candidate;
        }
        millis += 1;
    };

    // Write to a temporary name first, so an interrupted backup never looks complete.
    let partial = partial_path(&target);
    let partial_str = partial
        .to_str()
        .ok_or_else(|| StoreError::Io {
            context: "backup path",
            source: std::io::Error::other("path is not valid UTF-8"),
        })?
        .to_owned();
    if let Err(err) = conn.execute("VACUUM INTO ?1", [&partial_str]) {
        let _ = fs::remove_file(&partial);
        return Err(StoreError::Sqlite {
            context: "write backup",
            source: err,
        });
    }
    fs::File::open(&partial)
        .and_then(|f| f.sync_all())
        .map_err(StoreError::io("flush backup"))?;
    fs::rename(&partial, &target).map_err(StoreError::io("finish backup"))?;

    prune(&dir, &stem);
    Ok(target)
}

/// Lists the complete backups of `db_path`, oldest first.
pub fn list(db_path: &Path) -> Vec<PathBuf> {
    list_in(&backup_dir(db_path), &file_stem(db_path))
}

fn list_in(dir: &Path, stem: &str) -> Vec<PathBuf> {
    let prefix = format!("{stem}-");
    let mut found: Vec<(u64, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let rest = name.strip_prefix(&prefix)?.strip_suffix(".sqlite3")?;
            let (millis, version) = rest.split_once("-v")?;
            version.parse::<usize>().ok()?;
            Some((millis.parse::<u64>().ok()?, entry.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

/// Deletes all but the newest [`BACKUPS_KEPT`] backups. A failure here must not stop the app
/// from opening, so it is logged instead of returned.
fn prune(dir: &Path, stem: &str) {
    let backups = list_in(dir, stem);
    let excess = backups.len().saturating_sub(BACKUPS_KEPT);
    for old in &backups[..excess] {
        match fs::remove_file(old) {
            Ok(()) => tracing::info!(path = %old.display(), "removed old database backup"),
            Err(err) => {
                tracing::warn!(path = %old.display(), %err, "could not remove old database backup");
            }
        }
    }
}

fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    target.with_file_name(name)
}

fn file_stem(db_path: &Path) -> String {
    db_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "database".to_owned())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
