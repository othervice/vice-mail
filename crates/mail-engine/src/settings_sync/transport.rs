//! Where the encrypted records live: the [`SettingsTransport`] trait and an in-memory folder
//! for tests. The IMAP folder (one message per record) is the first real transport.
//!
//! A transport stores opaque blobs with two header values and knows nothing about kinds,
//! merging or encryption. Changing a record is "write the new version, then delete the old
//! one"; a crash in between leaves two versions, which the next pass cleans up.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::error::TransportError;

/// The transport's own handle of a stored entry (the IMAP UID in the sync folder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryHandle(pub u64);

/// Header values as stored, not yet validated (the server can change them).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawHeader {
    pub opaque_id: String,
    pub version: String,
}

/// One entry of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub handle: EntryHandle,
    pub header: RawHeader,
    /// Size of the stored body in bytes.
    pub size: u64,
}

/// The storage behind the settings sync. Futures are `Send`, so a pass can run on a Tokio task.
pub trait SettingsTransport: Send {
    /// All entries with their headers, without bodies.
    fn list(&mut self) -> impl Future<Output = Result<Vec<RemoteEntry>, TransportError>> + Send;

    /// Bodies of `handles`. Entries deleted meanwhile are left out of the result.
    fn read(
        &mut self,
        handles: &[EntryHandle],
    ) -> impl Future<Output = Result<Vec<(EntryHandle, Vec<u8>)>, TransportError>> + Send;

    /// Stores a new entry. Atomic: afterwards it is listed completely or not at all.
    fn write(
        &mut self,
        header: &RawHeader,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<EntryHandle, TransportError>> + Send;

    /// Removes entries. Handles that are gone already are not an error.
    fn delete(
        &mut self,
        handles: &[EntryHandle],
    ) -> impl Future<Output = Result<(), TransportError>> + Send;
}

/// A stored entry of the [`MemoryTransport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEntry {
    pub header: RawHeader,
    pub body: Vec<u8>,
    /// Number of the write that stored it (1, 2, ...), standing in for the server's arrival time.
    pub write_seq: u64,
}

#[derive(Debug, Default)]
struct Folder {
    next_handle: u64,
    entries: BTreeMap<EntryHandle, StoredEntry>,
    /// Writes that still succeed before every further write fails (`None`: no limit).
    writes_left: Option<u64>,
    /// Deletes that still succeed before every further delete fails.
    deletes_left: Option<u64>,
    offline: bool,
    writes: u64,
    deletes: u64,
}

/// An in-memory folder shared by every clone (one folder, several simulated devices), with
/// fault injection for crash tests. For tests and previews; does no IO.
#[derive(Debug, Clone, Default)]
pub struct MemoryTransport {
    folder: Arc<Mutex<Folder>>,
}

impl MemoryTransport {
    pub fn new() -> Self {
        Self::default()
    }

    fn folder(&self) -> MutexGuard<'_, Folder> {
        self.folder.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A snapshot of the folder.
    pub fn entries(&self) -> Vec<(EntryHandle, StoredEntry)> {
        self.folder()
            .entries
            .iter()
            .map(|(h, e)| (*h, e.clone()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.folder().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.folder().entries.is_empty()
    }

    /// Successful writes and deletes so far (each deleted handle counts once).
    pub fn counters(&self) -> (u64, u64) {
        let folder = self.folder();
        (folder.writes, folder.deletes)
    }

    /// Stores an entry as the server would (replays, foreign entries, manipulated headers).
    pub fn insert_raw(&self, header: RawHeader, body: Vec<u8>) -> EntryHandle {
        let mut folder = self.folder();
        store(&mut folder, header, body)
    }

    /// Replaces the body or header of an entry in place (a server that tampers).
    pub fn modify(&self, handle: EntryHandle, f: impl FnOnce(&mut StoredEntry)) -> bool {
        match self.folder().entries.get_mut(&handle) {
            Some(entry) => {
                f(entry);
                true
            }
            None => false,
        }
    }

    /// Removes an entry behind the devices' backs (another mail client, data loss).
    pub fn remove_raw(&self, handle: EntryHandle) -> bool {
        self.folder().entries.remove(&handle).is_some()
    }

    /// After `n` more successful writes every write fails (`None` lifts the limit).
    pub fn fail_writes_after(&self, n: Option<u64>) {
        self.folder().writes_left = n;
    }

    /// After `n` more successful delete calls every delete fails (`None` lifts the limit).
    pub fn fail_deletes_after(&self, n: Option<u64>) {
        self.folder().deletes_left = n;
    }

    /// Every call fails while offline.
    pub fn set_offline(&self, offline: bool) {
        self.folder().offline = offline;
    }
}

fn store(folder: &mut Folder, header: RawHeader, body: Vec<u8>) -> EntryHandle {
    folder.next_handle += 1;
    let handle = EntryHandle(folder.next_handle);
    folder.writes += 1;
    let write_seq = folder.writes;
    folder.entries.insert(
        handle,
        StoredEntry {
            header,
            body,
            write_seq,
        },
    );
    handle
}

fn online(folder: &Folder) -> Result<(), TransportError> {
    if folder.offline {
        return Err(TransportError::Unavailable(
            "memory folder is offline".to_owned(),
        ));
    }
    Ok(())
}

impl SettingsTransport for MemoryTransport {
    async fn list(&mut self) -> Result<Vec<RemoteEntry>, TransportError> {
        let folder = self.folder();
        online(&folder)?;
        Ok(folder
            .entries
            .iter()
            .map(|(handle, entry)| RemoteEntry {
                handle: *handle,
                header: entry.header.clone(),
                size: entry.body.len() as u64,
            })
            .collect())
    }

    async fn read(
        &mut self,
        handles: &[EntryHandle],
    ) -> Result<Vec<(EntryHandle, Vec<u8>)>, TransportError> {
        let folder = self.folder();
        online(&folder)?;
        Ok(handles
            .iter()
            .filter_map(|h| folder.entries.get(h).map(|e| (*h, e.body.clone())))
            .collect())
    }

    async fn write(
        &mut self,
        header: &RawHeader,
        body: Vec<u8>,
    ) -> Result<EntryHandle, TransportError> {
        let mut folder = self.folder();
        online(&folder)?;
        match folder.writes_left {
            Some(0) => {
                return Err(TransportError::Unavailable(
                    "memory folder: injected write failure".to_owned(),
                ));
            }
            Some(n) => folder.writes_left = Some(n - 1),
            None => {}
        }
        Ok(store(&mut folder, header.clone(), body))
    }

    async fn delete(&mut self, handles: &[EntryHandle]) -> Result<(), TransportError> {
        let mut folder = self.folder();
        online(&folder)?;
        match folder.deletes_left {
            Some(0) => {
                return Err(TransportError::Unavailable(
                    "memory folder: injected delete failure".to_owned(),
                ));
            }
            Some(n) => folder.deletes_left = Some(n - 1),
            None => {}
        }
        for handle in handles {
            if folder.entries.remove(handle).is_some() {
                folder.deletes += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(id: &str) -> RawHeader {
        RawHeader {
            opaque_id: id.to_owned(),
            version: "v".to_owned(),
        }
    }

    #[tokio::test]
    async fn clones_share_one_folder() {
        let mut a = MemoryTransport::new();
        let mut b = a.clone();
        let handle = a
            .write(&header("x"), b"body".to_vec())
            .await
            .expect("write");
        let listed = b.list().await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].handle, handle);
        assert_eq!(listed[0].size, 4);
        assert_eq!(
            b.read(&[handle, EntryHandle(99)]).await.expect("read"),
            vec![(handle, b"body".to_vec())]
        );
        b.delete(&[handle, handle]).await.expect("delete");
        assert!(a.is_empty());
        assert_eq!(a.counters(), (1, 1));
    }

    #[tokio::test]
    async fn injected_failures() {
        let mut t = MemoryTransport::new();
        t.fail_writes_after(Some(1));
        t.write(&header("a"), vec![]).await.expect("first write");
        assert!(t.write(&header("b"), vec![]).await.is_err());
        t.fail_writes_after(None);
        t.fail_deletes_after(Some(0));
        let handles: Vec<EntryHandle> = t.entries().into_iter().map(|(h, _)| h).collect();
        assert!(t.delete(&handles).await.is_err());
        assert_eq!(t.len(), 1);
        t.set_offline(true);
        assert!(matches!(
            t.list().await,
            Err(TransportError::Unavailable(_))
        ));
    }
}
