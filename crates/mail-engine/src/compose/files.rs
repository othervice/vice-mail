//! Files of the compose directory (`compose/` next to the database, decision 0018):
//!
//! - `drafts/<draft>/<attachment id>`: the core's own copies of attached files, taken when they
//!   were added. Removed with the draft.
//! - `outgoing/<draft>/message.eml` and `sent.eml`: the frozen bytes of a send (for the SMTP
//!   server, and the copy for Sent with `Bcc`). Kept while an unfinished `send` or `append`
//!   operation names them; a sweep removes the rest.
//!
//! Writes are atomic (temporary file, `fsync`, rename), so a crash leaves either the old state
//! or the complete new file. Paths are built from validated ids only ([`DraftId`] is a UUID,
//! attachment ids are digits), never from names the user or a mail chose.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use mail_model::{AppendSource, CoreError, DraftId, OperationPayload};
use mail_store::Store;
use sha2::{Digest, Sha256};

/// Name of the compose directory next to the database.
pub const COMPOSE_DIR: &str = "compose";
const DRAFTS: &str = "drafts";
const OUTGOING: &str = "outgoing";
/// File names of a frozen send.
pub(crate) const MESSAGE_FILE: &str = "message.eml";
pub(crate) const SENT_FILE: &str = "sent.eml";

/// The compose directory of the store's data directory.
pub(crate) fn compose_dir(store: &Store) -> PathBuf {
    store
        .path()
        .parent()
        .map_or_else(|| PathBuf::from(COMPOSE_DIR), |dir| dir.join(COMPOSE_DIR))
}

pub(crate) fn io_error(context: &str) -> impl FnOnce(io::Error) -> CoreError + '_ {
    move |err| CoreError::Internal(format!("{context}: {err}"))
}

/// `drafts/<draft>/<id>`, relative to the compose directory.
pub(crate) fn attachment_path(draft: &DraftId, id: &str) -> String {
    format!("{DRAFTS}/{}/{id}", draft.as_str())
}

/// `outgoing/<draft>/<name>`, relative to the compose directory.
pub(crate) fn outgoing_path(draft: &DraftId, name: &str) -> String {
    format!("{OUTGOING}/{}/{name}", draft.as_str())
}

/// A path relative to the compose directory, refusing anything that could leave it.
pub(crate) fn resolve(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let ok = !relative.is_empty()
        && !relative.starts_with('/')
        && relative.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
        });
    if !ok {
        return Err(CoreError::Internal(format!(
            "not a compose file path: {relative:?}"
        )));
    }
    Ok(root.join(relative))
}

/// Writes `bytes` atomically to `path` (parents created).
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::Internal("a compose file without a directory".into()))?;
    fs::create_dir_all(parent).map_err(io_error("create compose directory"))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    {
        let mut file = fs::File::create(&tmp).map_err(io_error("create compose file"))?;
        file.write_all(bytes)
            .map_err(io_error("write compose file"))?;
        file.sync_all().map_err(io_error("sync compose file"))?;
    }
    fs::rename(&tmp, path).map_err(io_error("rename compose file"))?;
    Ok(())
}

pub(crate) fn read(root: &Path, relative: &str) -> Result<Vec<u8>, CoreError> {
    let path = resolve(root, relative)?;
    fs::read(&path).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            CoreError::NotFound(format!("compose file {relative}"))
        } else {
            CoreError::Internal(format!("read compose file {relative}: {err}"))
        }
    })
}

/// Copies the file at `source` (chosen by the user) into the draft's directory under `id`,
/// reading at most `limit` bytes. Returns the size and the SHA-256 (hex).
pub(crate) fn copy_in(
    root: &Path,
    draft: &DraftId,
    id: &str,
    source: &Path,
    limit: u64,
) -> Result<(u64, String), CoreError> {
    let metadata = fs::metadata(source)
        .map_err(|err| CoreError::InvalidArgument(format!("{}: {err}", source.display())))?;
    if !metadata.is_file() {
        return Err(CoreError::InvalidArgument(format!(
            "{} is not a file",
            source.display()
        )));
    }
    if metadata.len() > limit {
        return Err(CoreError::Draft(
            mail_model::DraftError::AttachmentTooLarge {
                size: metadata.len(),
                limit,
            },
        ));
    }
    let mut file = fs::File::open(source)
        .map_err(|err| CoreError::InvalidArgument(format!("{}: {err}", source.display())))?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    Read::take(&mut file, limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io_error("read attachment"))?;
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > limit {
        return Err(CoreError::Draft(
            mail_model::DraftError::AttachmentTooLarge { size, limit },
        ));
    }
    let hash = sha256_hex(&bytes);
    let target = resolve(root, &attachment_path(draft, id))?;
    write_atomically(&target, &bytes)?;
    Ok((size, hash))
}

/// Writes attachment bytes (a forwarded original's, a draft opened from the server) into the
/// draft's directory. Returns the SHA-256.
pub(crate) fn write_attachment(
    root: &Path,
    draft: &DraftId,
    id: &str,
    bytes: &[u8],
) -> Result<String, CoreError> {
    let target = resolve(root, &attachment_path(draft, id))?;
    write_atomically(&target, bytes)?;
    Ok(sha256_hex(bytes))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Removes one attachment copy (a missing file is fine).
pub(crate) fn remove_file(root: &Path, relative: &str) {
    if let Ok(path) = resolve(root, relative)
        && let Err(err) = fs::remove_file(&path)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(error = %err, "removing a compose file failed");
    }
}

/// Removes the draft's directory of attachments.
pub(crate) fn remove_draft_dir(root: &Path, draft: &DraftId) {
    remove_dir(&root.join(DRAFTS).join(draft.as_str()));
}

/// Removes the frozen bytes of a send.
pub(crate) fn remove_outgoing_dir(root: &Path, draft: &DraftId) {
    remove_dir(&root.join(OUTGOING).join(draft.as_str()));
}

fn remove_dir(path: &Path) {
    if let Err(err) = fs::remove_dir_all(path)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(error = %err, "removing a compose directory failed");
    }
}

/// The draft ids that have a directory under `drafts/` or `outgoing/`.
fn dirs_in(root: &Path, kind: &str) -> Vec<DraftId> {
    let Ok(entries) = fs::read_dir(root.join(kind)) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().and_then(DraftId::parse))
        .collect()
}

/// Removes `outgoing/<draft>/` directories no unfinished operation names, and `drafts/<draft>/`
/// directories of drafts that no longer exist. Called at start and after sends finish.
pub(crate) async fn sweep(store: &Store) {
    let root = compose_dir(store);
    let payloads = match store.unfinished_compose_payloads().await {
        Ok(payloads) => payloads,
        Err(err) => {
            tracing::warn!(error = %err, "compose sweep: unfinished operations unreadable");
            return;
        }
    };
    let mut needed: HashSet<String> = HashSet::new();
    for payload in &payloads {
        match payload {
            OperationPayload::Send(job) => {
                needed.insert(job.draft.as_str().to_owned());
            }
            OperationPayload::Append {
                source: AppendSource::File { file },
                ..
            } => {
                if let Some(dir) = file.split('/').nth(1) {
                    needed.insert(dir.to_owned());
                }
            }
            _ => {}
        }
    }
    let outgoing = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || dirs_in(&root, OUTGOING))
            .await
            .unwrap_or_default()
    };
    for draft in outgoing {
        if !needed.contains(draft.as_str()) {
            remove_outgoing_dir(&root, &draft);
        }
    }
    let drafts = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || dirs_in(&root, DRAFTS))
            .await
            .unwrap_or_default()
    };
    for draft in drafts {
        match store.draft(draft.clone()).await {
            Ok(None) => remove_draft_dir(&root, &draft),
            Ok(Some(_)) => {}
            Err(err) => tracing::warn!(error = %err, "compose sweep: draft unreadable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_compose_directory() {
        let root = Path::new("/data/compose");
        let draft = DraftId::from_random(7);
        assert_eq!(
            resolve(root, &attachment_path(&draft, "3")).expect("path"),
            root.join("drafts").join(draft.as_str()).join("3")
        );
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "drafts/../../x",
            "drafts//x",
            "a\\b",
            "ä",
        ] {
            assert!(resolve(root, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn atomic_writes_and_copies() {
        let dir = tempfile::TempDir::new().expect("dir");
        let root = dir.path().join("compose");
        let draft = DraftId::from_random(9);
        let source = dir.path().join("Grüße 🎉.txt");
        fs::write(&source, b"hello").expect("source");
        let (size, hash) = copy_in(&root, &draft, "1", &source, 100).expect("copy");
        assert_eq!(size, 5);
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            read(&root, &attachment_path(&draft, "1")).expect("read"),
            b"hello"
        );
        let err = copy_in(&root, &draft, "2", &source, 4).expect_err("too large");
        assert_eq!(err.code(), mail_model::ErrorCode::AttachmentTooLarge);
        assert!(
            copy_in(&root, &draft, "3", dir.path(), 100).is_err(),
            "a directory"
        );
        remove_draft_dir(&root, &draft);
        assert!(read(&root, &attachment_path(&draft, "1")).is_err());
    }
}
