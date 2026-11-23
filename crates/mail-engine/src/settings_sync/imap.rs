//! [`SettingsTransport`] over the IMAP folder (decision 0022, `docs/SYNC.md`).
//!
//! - One message per record version: two header fields carry the opaque id and the version,
//!   the body is the age ciphertext in age's ASCII armor (7-bit text, so every server stores it
//!   as it is; anyone holding the key can also open it with the `age` tool).
//! - Listing reads only those two fields (`BODY.PEEK[HEADER.FIELDS (...)]`); messages without
//!   both are not ours and stay invisible to the sync, so it never deletes them.
//! - The entry handle is the message's UID; one transport lives for one pass on one selected
//!   folder, so UIDs cannot change underneath it.
//! - Bodies read once are kept for the pass: when a pass plans again after resolving records
//!   that exist on both sides under different ids (joining), nothing is downloaded twice.

use std::collections::HashMap;
use std::io::{Read, Write};

use mail_model::{
    BackendError, SYNC_ID_HEADER, SYNC_VERSION_HEADER, SyncFolder, SyncFolderEntry, Uid, UidSet,
};

use super::error::TransportError;
use super::transport::{EntryHandle, RawHeader, RemoteEntry, SettingsTransport};

/// Subject of every entry: what someone who finds the folder in another mail client sees.
const SUBJECT: &str = "Settings sync data (encrypted, do not edit)";

/// A pass's view of the sync folder.
pub(crate) struct ImapTransport<'a, F: SyncFolder> {
    folder: &'a mut F,
    now_ms: i64,
    bodies: HashMap<EntryHandle, Vec<u8>>,
}

impl<'a, F: SyncFolder> ImapTransport<'a, F> {
    /// `folder` must be open (SELECT); `now_ms` dates the messages this pass writes.
    pub(crate) fn new(folder: &'a mut F, now_ms: i64) -> Self {
        Self {
            folder,
            now_ms,
            bodies: HashMap::new(),
        }
    }
}

pub(crate) fn transport_error(err: BackendError) -> TransportError {
    if err.is_transient() {
        TransportError::Unavailable(err.to_string())
    } else {
        TransportError::Refused(err.to_string())
    }
}

/// The two header values of an entry, if it has both.
pub(crate) fn parse_header(block: &[u8]) -> Option<RawHeader> {
    let text = String::from_utf8_lossy(block);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = fields.last_mut() {
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            fields.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let get = |wanted: &str| {
        let wanted = wanted.to_ascii_lowercase();
        fields
            .iter()
            .find(|(name, _)| *name == wanted)
            .map(|(_, v)| v.clone())
    };
    Some(RawHeader {
        opaque_id: get(SYNC_ID_HEADER)?,
        version: get(SYNC_VERSION_HEADER)?,
    })
}

/// A header value we write: printable ASCII only (ids and versions are).
fn header_value(value: &str) -> Result<&str, TransportError> {
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic()) {
        Ok(value)
    } else {
        Err(TransportError::Refused(
            "header value is not printable ASCII".into(),
        ))
    }
}

fn rfc2822(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%a, %d %b %Y %H:%M:%S +0000")
        .to_string()
}

/// The message of one entry: the header fields and the armored ciphertext, CRLF throughout.
pub(crate) fn build_message(
    header: &RawHeader,
    ciphertext: &[u8],
    now_ms: i64,
) -> Result<Vec<u8>, TransportError> {
    let mut armored = Vec::new();
    {
        let mut writer =
            age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
                .map_err(|err| TransportError::Refused(format!("armor: {err}")))?;
        writer
            .write_all(ciphertext)
            .map_err(|err| TransportError::Refused(format!("armor: {err}")))?;
        writer
            .finish()
            .map_err(|err| TransportError::Refused(format!("armor: {err}")))?;
    }
    let body = String::from_utf8_lossy(&armored).replace("\r\n", "\n");
    let mut message = format!(
        "Date: {}\r\nSubject: {SUBJECT}\r\n{SYNC_ID_HEADER}: {}\r\n{SYNC_VERSION_HEADER}: {}\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=us-ascii\r\n\
         Content-Transfer-Encoding: 7bit\r\n\r\n",
        rfc2822(now_ms),
        header_value(&header.opaque_id)?,
        header_value(&header.version)?,
    );
    for line in body.lines() {
        message.push_str(line);
        message.push_str("\r\n");
    }
    Ok(message.into_bytes())
}

/// The ciphertext of an entry's body. A body that is not armored is passed on unchanged; the
/// decryption then rejects it like any damaged entry.
pub(crate) fn dearmor(body: &[u8]) -> Vec<u8> {
    let mut reader = age::armor::ArmoredReader::new(body);
    let mut out = Vec::new();
    match reader.read_to_end(&mut out) {
        Ok(_) => out,
        Err(_) => body.to_vec(),
    }
}

fn uid_of(handle: EntryHandle) -> Option<Uid> {
    Uid::try_from(handle.0).ok().filter(|uid| *uid > 0)
}

impl<F: SyncFolder> SettingsTransport for ImapTransport<'_, F> {
    async fn list(&mut self) -> Result<Vec<RemoteEntry>, TransportError> {
        let entries: Vec<SyncFolderEntry> = self
            .folder
            .list(&[SYNC_ID_HEADER, SYNC_VERSION_HEADER])
            .await
            .map_err(transport_error)?;
        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                parse_header(&entry.header).map(|header| RemoteEntry {
                    handle: EntryHandle(u64::from(entry.uid)),
                    header,
                    size: u64::from(entry.size),
                })
            })
            .collect())
    }

    async fn read(
        &mut self,
        handles: &[EntryHandle],
    ) -> Result<Vec<(EntryHandle, Vec<u8>)>, TransportError> {
        let missing = UidSet::from_uids(
            handles
                .iter()
                .filter(|h| !self.bodies.contains_key(h))
                .filter_map(|h| uid_of(*h)),
        );
        if !missing.is_empty() {
            for (uid, body) in self.folder.read(&missing).await.map_err(transport_error)? {
                self.bodies
                    .insert(EntryHandle(u64::from(uid)), dearmor(&body));
            }
        }
        Ok(handles
            .iter()
            .filter_map(|h| self.bodies.get(h).map(|body| (*h, body.clone())))
            .collect())
    }

    async fn write(
        &mut self,
        header: &RawHeader,
        body: Vec<u8>,
    ) -> Result<EntryHandle, TransportError> {
        let message = build_message(header, &body, self.now_ms)?;
        let uid = self
            .folder
            .append(&message)
            .await
            .map_err(transport_error)?;
        // Without UIDPLUS the UID is unknown; the pass does not need it (it lists again).
        Ok(EntryHandle(uid.map_or(0, u64::from)))
    }

    async fn delete(&mut self, handles: &[EntryHandle]) -> Result<(), TransportError> {
        let uids = UidSet::from_uids(handles.iter().filter_map(|h| uid_of(*h)));
        if uids.is_empty() {
            return Ok(());
        }
        self.folder.delete(&uids).await.map_err(transport_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trip() {
        let header = RawHeader {
            opaque_id: "0123456789abcdef0123456789abcdef".into(),
            version: "1764000000000-00000000-6f9619ff-8b86-4d11-b42d-00c04fc964ff".into(),
        };
        let ciphertext: Vec<u8> = (0..=255u8).cycle().take(700).collect();
        let message = build_message(&header, &ciphertext, 1_764_000_000_000).expect("build");
        let text = String::from_utf8(message.clone()).expect("ascii");
        assert!(
            text.starts_with("Date: Mon, 24 Nov 2025 16:00:00 +0000\r\n"),
            "{text}"
        );
        assert!(text.contains("-----BEGIN AGE ENCRYPTED FILE-----\r\n"));
        assert!(!text.replace("\r\n", "").contains('\n'), "CRLF only");
        let split = text.find("\r\n\r\n").expect("header end");
        let parsed = parse_header(&message[..split + 2]).expect("header");
        assert_eq!(parsed, header);
        assert_eq!(dearmor(&message[split + 4..]), ciphertext);
        // Folded header lines, other case, other fields.
        let folded =
            b"x-settings-sync-id:\r\n 0123\r\nX-Other: 1\r\nX-Settings-Sync-Version: v\r\n\r\n";
        assert_eq!(
            parse_header(folded),
            Some(RawHeader {
                opaque_id: "0123".into(),
                version: "v".into()
            })
        );
        assert_eq!(parse_header(b"Subject: hello\r\n\r\n"), None);
        // A body that is not armor stays as it is (and fails to decrypt later).
        assert_eq!(dearmor(b"tampered"), b"tampered".to_vec());
        assert!(
            build_message(
                &RawHeader {
                    opaque_id: "a\r\nBcc: x".into(),
                    version: "v".into()
                },
                b"x",
                0
            )
            .is_err()
        );
    }
}
