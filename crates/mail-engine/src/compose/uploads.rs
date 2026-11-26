//! Stage 23 (decision 0082): files dragged onto a window, handed over by the page as bytes.
//!
//! The windows no longer take drops through the shell (Tauri's handler would swallow every
//! drag inside the page), so a file dropped on the composer or the signature editor reaches the
//! page as an HTML `File`: its name, its size and its bytes, never its path. The page hands the
//! bytes over in pieces ([`MAX_UPLOAD_CHUNK_BYTES`]): `begin` announces name and size, `append`
//! writes each piece at its offset, and a command of the composer or the signature editor then
//! takes the complete uploads like files from the system dialog, with the same limits and
//! checks. Until then they lie in `compose/uploads/<upload id>` (a path built from the core's own
//! id only); a taken, discarded or idle upload is removed, and a start removes them all.
//!
//! Limits against a runaway page: one upload at most [`MAX_UPLOAD_BYTES`], at most
//! [`MAX_OPEN_UPLOADS`] at a time and [`MAX_PENDING_BYTES`] announced together; an upload that
//! got no piece for [`IDLE_LIFETIME`] is dropped when the next one begins.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use mail_model::{
    CoreError, DraftError, MAX_ATTACHMENTS, MAX_UPLOAD_BYTES, MAX_UPLOAD_CHUNK_BYTES, UploadId,
};

use super::files::io_error;

/// The staging directory inside the compose directory.
pub(crate) const UPLOADS_DIR: &str = "uploads";
/// An upload that got no piece for this long is dropped.
pub(crate) const IDLE_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// Most uploads open at a time.
pub(crate) const MAX_OPEN_UPLOADS: usize = MAX_ATTACHMENTS;
/// Most bytes all open uploads may announce together.
pub(crate) const MAX_PENDING_BYTES: u64 = 2 * MAX_UPLOAD_BYTES;

/// A file a command copies into a draft or a signature: where the core reads it and the name it
/// gets (sanitised again where it is used).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub path: PathBuf,
    pub name: String,
}

impl SourceFile {
    /// A file the user chose in the system dialog: named as on disk.
    pub(crate) fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self { path, name }
    }
}

#[derive(Debug)]
struct Upload {
    name: String,
    size: u64,
    /// Bytes reserved by `append` calls (written, or being written).
    written: u64,
    touched: Instant,
}

/// The open uploads of this run.
#[derive(Debug)]
pub(crate) struct Uploads {
    dir: PathBuf,
    open: Mutex<HashMap<UploadId, Upload>>,
}

/// Uploads a command took: their files, removed when this is dropped (after the command copied
/// them, or failed).
#[derive(Debug)]
pub(crate) struct TakenUploads {
    files: Vec<SourceFile>,
}

impl TakenUploads {
    pub(crate) fn files(&self) -> Vec<SourceFile> {
        self.files.clone()
    }
}

impl Drop for TakenUploads {
    fn drop(&mut self) {
        for file in &self.files {
            remove(&file.path);
        }
    }
}

fn remove(path: &Path) {
    if let Err(err) = fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %err, "an upload's file could not be removed");
    }
}

fn unknown(id: &UploadId) -> CoreError {
    CoreError::NotFound(format!("upload {id}"))
}

impl Uploads {
    /// The uploads of the compose directory `root`.
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            dir: root.join(UPLOADS_DIR),
            open: Mutex::default(),
        }
    }

    fn path(&self, id: &UploadId) -> PathBuf {
        self.dir.join(id.as_str())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<UploadId, Upload>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Removes every staged file (at start: no upload outlives the run that began it).
    pub(crate) fn clear(&self) {
        self.lock().clear();
        if let Err(err) = fs::remove_dir_all(&self.dir)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error = %err, "leftover uploads could not be removed");
        }
    }

    /// Announces a file of `size` bytes named `name`; answers the id its pieces go to.
    pub(crate) fn begin(&self, name: &str, size: u64) -> Result<UploadId, CoreError> {
        self.begin_at(name, size, Instant::now())
    }

    pub(crate) fn begin_at(
        &self,
        name: &str,
        size: u64,
        now: Instant,
    ) -> Result<UploadId, CoreError> {
        if size > MAX_UPLOAD_BYTES {
            return Err(CoreError::Draft(DraftError::AttachmentTooLarge {
                size,
                limit: MAX_UPLOAD_BYTES,
            }));
        }
        let id = UploadId::from_random(uuid::Uuid::new_v4().as_u128());
        let mut open = self.lock();
        let idle: Vec<UploadId> = open
            .iter()
            .filter(|(_, upload)| now.saturating_duration_since(upload.touched) > IDLE_LIFETIME)
            .map(|(id, _)| id.clone())
            .collect();
        for stale in idle {
            open.remove(&stale);
            remove(&self.path(&stale));
        }
        if open.len() >= MAX_OPEN_UPLOADS {
            return Err(CoreError::InvalidArgument(format!(
                "at most {MAX_OPEN_UPLOADS} files at a time"
            )));
        }
        let pending = open
            .values()
            .map(|upload| upload.size)
            .fold(size, u64::saturating_add);
        if pending > MAX_PENDING_BYTES {
            return Err(CoreError::Draft(DraftError::AttachmentTooLarge {
                size: pending,
                limit: MAX_PENDING_BYTES,
            }));
        }
        fs::create_dir_all(&self.dir).map_err(io_error("create the upload directory"))?;
        fs::File::create_new(self.path(&id)).map_err(io_error("create an upload"))?;
        open.insert(
            id.clone(),
            Upload {
                name: mail_content::sanitize_filename(name),
                size,
                written: 0,
                touched: now,
            },
        );
        Ok(id)
    }

    /// Writes `bytes` at `offset` (the bytes written so far); answers how many are written now.
    /// Blocking file IO: call it off the async threads.
    pub(crate) fn append(
        &self,
        id: &UploadId,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, CoreError> {
        self.append_at(id, offset, bytes, Instant::now())
    }

    pub(crate) fn append_at(
        &self,
        id: &UploadId,
        offset: u64,
        bytes: &[u8],
        now: Instant,
    ) -> Result<u64, CoreError> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len > MAX_UPLOAD_CHUNK_BYTES {
            return Err(CoreError::InvalidArgument(format!(
                "a piece of an upload has at most {MAX_UPLOAD_CHUNK_BYTES} bytes"
            )));
        }
        // The piece's place is reserved before the write, so a second call with the same offset
        // cannot write over it.
        let end = {
            let mut open = self.lock();
            let upload = open.get_mut(id).ok_or_else(|| unknown(id))?;
            if offset != upload.written {
                return Err(CoreError::InvalidArgument(format!(
                    "upload {id}: a piece at {offset}, expected one at {}",
                    upload.written
                )));
            }
            let end = offset.saturating_add(len);
            if end > upload.size {
                return Err(CoreError::InvalidArgument(format!(
                    "upload {id}: more bytes than the {} announced",
                    upload.size
                )));
            }
            upload.written = end;
            upload.touched = now;
            end
        };
        let written = fs::OpenOptions::new()
            .append(true)
            .open(self.path(id))
            .and_then(|mut file| file.write_all(bytes));
        if let Err(err) = written {
            self.discard(id);
            return Err(io_error("write an upload")(err));
        }
        Ok(end)
    }

    /// Drops an upload and its file; an unknown id is no error (it may have been taken).
    pub(crate) fn discard(&self, id: &UploadId) {
        if self.lock().remove(id).is_some() {
            remove(&self.path(id));
        }
    }

    /// Takes complete uploads for a command, in the given order. Nothing is taken unless every
    /// one is known, complete and named once.
    pub(crate) fn take(&self, ids: &[UploadId]) -> Result<TakenUploads, CoreError> {
        let mut open = self.lock();
        for (i, id) in ids.iter().enumerate() {
            let upload = open.get(id).ok_or_else(|| unknown(id))?;
            if upload.written != upload.size {
                return Err(CoreError::InvalidArgument(format!(
                    "upload {id} has {} of {} bytes",
                    upload.written, upload.size
                )));
            }
            if ids[..i].contains(id) {
                return Err(CoreError::InvalidArgument(format!(
                    "upload {id} named twice"
                )));
            }
        }
        let files = ids
            .iter()
            .filter_map(|id| {
                open.remove(id).map(|upload| SourceFile {
                    path: self.path(id),
                    name: upload.name,
                })
            })
            .collect();
        Ok(TakenUploads { files })
    }

    #[cfg(test)]
    fn open_count(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(uploads: &Uploads) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&uploads.dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn pieces_make_a_file_that_is_taken_once_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path());
        let id = uploads.begin("Bericht 2026.pdf", 5).unwrap();
        assert_eq!(uploads.append(&id, 0, b"%PD").unwrap(), 3);
        assert!(
            uploads.take(std::slice::from_ref(&id)).is_err(),
            "an incomplete upload is not taken"
        );
        assert!(
            uploads.append(&id, 0, b"F-").is_err(),
            "a piece at the wrong offset is refused"
        );
        assert!(
            uploads.append(&id, 3, b"F-1").is_err(),
            "more than announced is refused"
        );
        assert_eq!(uploads.append(&id, 3, b"F-").unwrap(), 5);
        let taken = uploads.take(std::slice::from_ref(&id)).unwrap();
        let files = taken.files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "Bericht 2026.pdf");
        assert_eq!(fs::read(&files[0].path).unwrap(), b"%PDF-");
        assert_eq!(
            files[0].path,
            dir.path().join(UPLOADS_DIR).join(id.as_str()),
            "the file's path is built from the core's id only"
        );
        assert!(
            uploads.take(std::slice::from_ref(&id)).is_err(),
            "taken once"
        );
        drop(taken);
        assert!(staged(&uploads).is_empty(), "the file goes with the taker");
    }

    #[test]
    fn a_name_never_becomes_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path());
        let id = uploads.begin("../../../.ssh/id_ed25519", 2).unwrap();
        uploads.append(&id, 0, b"ok").unwrap();
        assert_eq!(staged(&uploads), vec![id.as_str().to_owned()]);
        let taken = uploads.take(std::slice::from_ref(&id)).unwrap();
        let file = &taken.files()[0];
        assert!(!file.name.contains('/') && !file.name.contains('\\'));
        assert!(file.path.starts_with(dir.path().join(UPLOADS_DIR)));
        assert!(!dir.path().join(".ssh").exists());
    }

    #[test]
    fn limits_hold() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path());
        assert!(matches!(
            uploads.begin("huge.bin", MAX_UPLOAD_BYTES + 1),
            Err(CoreError::Draft(DraftError::AttachmentTooLarge { limit, .. })) if limit == MAX_UPLOAD_BYTES
        ));
        let id = uploads.begin("a.bin", 4).unwrap();
        let big = vec![0_u8; usize::try_from(MAX_UPLOAD_CHUNK_BYTES).unwrap() + 1];
        assert!(uploads.append(&id, 0, &big).is_err(), "a piece too large");
        let unknown = UploadId::from_random(7);
        assert!(matches!(
            uploads.append(&unknown, 0, b"x"),
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(
            uploads.take(std::slice::from_ref(&unknown)),
            Err(CoreError::NotFound(_))
        ));
        uploads.append(&id, 0, b"abcd").unwrap();
        assert!(
            uploads.take(&[id.clone(), id.clone()]).is_err(),
            "the same upload twice"
        );
        assert_eq!(uploads.open_count(), 1, "nothing was taken");
        // Two uploads of the largest size fill what may be announced together.
        uploads.discard(&id);
        let first = uploads.begin("1.bin", MAX_UPLOAD_BYTES).unwrap();
        let _second = uploads.begin("2.bin", MAX_UPLOAD_BYTES).unwrap();
        assert!(matches!(
            uploads.begin("3.bin", 1),
            Err(CoreError::Draft(DraftError::AttachmentTooLarge { limit, .. })) if limit == MAX_PENDING_BYTES
        ));
        uploads.discard(&first);
        uploads.begin("3.bin", 1).unwrap();
    }

    #[test]
    fn idle_uploads_go_and_a_start_clears_all() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path());
        let start = Instant::now();
        let old = uploads.begin_at("old.txt", 3, start).unwrap();
        let busy = uploads.begin_at("busy.txt", 3, start).unwrap();
        uploads
            .append_at(&busy, 0, b"a", start + IDLE_LIFETIME)
            .unwrap();
        let later = start + IDLE_LIFETIME + Duration::from_secs(1);
        let _new = uploads.begin_at("new.txt", 1, later).unwrap();
        assert!(matches!(
            uploads.append_at(&old, 0, b"abc", later),
            Err(CoreError::NotFound(_))
        ));
        assert_eq!(uploads.open_count(), 2, "the busy one stays");
        assert_eq!(staged(&uploads).len(), 2);
        uploads.clear();
        assert_eq!(uploads.open_count(), 0);
        assert!(staged(&uploads).is_empty());
    }

    #[test]
    fn too_many_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path());
        for i in 0..MAX_OPEN_UPLOADS {
            uploads.begin(&format!("{i}.txt"), 0).unwrap();
        }
        assert!(matches!(
            uploads.begin("one more.txt", 0),
            Err(CoreError::InvalidArgument(_))
        ));
    }
}
