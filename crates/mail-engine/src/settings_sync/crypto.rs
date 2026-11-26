//! Encryption with age (age-encryption.org/v1) and the opaque ids the server sees. No own
//! cryptography: age does key wrapping (scrypt recipient), record encryption (X25519 recipient,
//! ChaCha20-Poly1305) and integrity; HMAC-SHA-256 (RustCrypto `hmac` + `sha2`) derives ids.
//!
//! - **Sync key:** a random age X25519 identity, created once at setup ([`SyncKey::generate`]).
//! - **Key record:** that identity, encrypted to the passphrase with age's scrypt recipient at
//!   work factor [`WorkFactor::DEFAULT`]. It lies in the folder like a record, under the opaque
//!   id [`SyncKey::key_record_id`] (`key-` + 32 hex digits, derived from the key, so a device
//!   can tell whether the folder still uses its key without the passphrase).
//! - **Records:** encrypted to the identity's public key. The plaintext is a JSON envelope
//!   with format, opaque id, version, kind, id and content (or the deleted version), padded
//!   with spaces to a multiple of 256 bytes so sizes only leak coarsely.
//! - **Opaque id:** the first 16 bytes of `HMAC-SHA-256(id_key, kind || 0x00 || id)` in hex,
//!   with `id_key = HMAC-SHA-256(age secret key text, label)`. Stable per record (so versions of
//!   one record can be grouped without decrypting), unlinkable to kind and id without the key.
//! - **Header check:** opening a record checks the envelope's opaque id and version against the
//!   header and recomputes the opaque id from kind and id. A server that swaps headers, replays
//!   a record under a newer version or renames it is detected; the record is rejected.

use std::fmt;

use age::secrecy::{ExposeSecret, SecretString};
use hmac::{Hmac, KeyInit, Mac};
use mail_model::Secret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use super::error::SettingsSyncError;
use super::hlc::Hlc;
use super::record::{RecordBody, RecordKey, RecordKind, SyncContent, SyncRecord};
use super::transport::RawHeader;

/// Version of the payload format. A payload with a higher number comes from a newer app; it is
/// left alone (never deleted, never merged) and reported.
pub const FORMAT_VERSION: u32 = 1;

/// Prefix of key record ids; record ids are plain hex and never start with it.
pub const KEY_RECORD_PREFIX: &str = "key-";

/// Stage 22 (decision 0076): prefix of the entries of a password transfer between devices
/// (`super::transfer`). Passes leave them out like key records.
pub const TRANSFER_PREFIX: &str = "xfer-";

/// Plaintexts are padded to a multiple of this many bytes.
pub(super) const PAD_BLOCK: usize = 256;

/// Largest ciphertext that is decrypted at all (the largest content is 256 KiB).
pub(super) const MAX_CIPHERTEXT_BYTES: usize = 1024 * 1024;

const LABEL_ID_KEY: &[u8] = b"vice-mail settings-sync v1 opaque-id key";
const LABEL_KEY_RECORD: &[u8] = b"vice-mail settings-sync v1 key-record id";

type HmacSha256 = Hmac<Sha256>;

/// Opaque id of a record (32 lowercase hex digits), of a key record (`key-` + 32 hex) or of a
/// password transfer entry (`xfer-` + 32 hex, stage 22).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpaqueId(pub(super) String);

impl OpaqueId {
    /// Validates the text form.
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s
            .strip_prefix(KEY_RECORD_PREFIX)
            .or_else(|| s.strip_prefix(TRANSFER_PREFIX))
            .unwrap_or(s);
        let ok = hex.len() == 32 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        ok.then(|| Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_key_record(&self) -> bool {
        self.0.starts_with(KEY_RECORD_PREFIX)
    }

    /// Stage 22: an entry of a password transfer, not a record.
    pub fn is_transfer(&self) -> bool {
        self.0.starts_with(TRANSFER_PREFIX)
    }
}

impl fmt::Display for OpaqueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The validated header of a stored entry: all the server learns besides size and time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecordHeader {
    pub opaque_id: OpaqueId,
    pub version: Hlc,
}

impl RecordHeader {
    pub fn to_raw(&self) -> RawHeader {
        RawHeader {
            opaque_id: self.opaque_id.to_string(),
            version: self.version.to_string(),
        }
    }

    /// `None` if either value is malformed.
    pub fn parse(raw: &RawHeader) -> Option<Self> {
        Some(Self {
            opaque_id: OpaqueId::parse(&raw.opaque_id)?,
            version: raw.version.parse().ok()?,
        })
    }
}

/// An encrypted record ready for [`super::SettingsTransport::write`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedRecord {
    pub header: RecordHeader,
    pub ciphertext: Vec<u8>,
}

/// scrypt work factor of the key record, as log2(N) (r = 8, p = 1, fixed by age).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkFactor(u8);

impl WorkFactor {
    /// log2(N) = 18: 256 MiB of memory, about 0.35 s on an Apple M4 (release build, measured
    /// in `docs/SYNC.md`) and around a second on older desktops. The key is unwrapped once per
    /// device (when it joins, and after a passphrase change), so the delay is paid rarely, while
    /// every guess of an attacker who copies the folder costs the same memory and time. A
    /// fixed value instead of age's per-machine calibration (about one second) keeps slow
    /// devices able to unwrap what fast ones wrote.
    pub const DEFAULT: Self = Self(18);

    /// Highest work factor accepted when unwrapping (1 GiB). A forged key record with a huge
    /// factor would otherwise make a device spend hours and tens of GiB (age's own limit is
    /// "calibrated + 4", which is 24 or more on fast machines).
    pub const MAX: Self = Self(20);

    /// Values below [`WorkFactor::DEFAULT`] only for tests (the test suite uses 10).
    pub const fn new(log_n: u8) -> Option<Self> {
        if log_n >= 10 && log_n <= Self::MAX.0 {
            Some(Self(log_n))
        } else {
            None
        }
    }

    pub const fn log_n(self) -> u8 {
        self.0
    }
}

/// Why a stored entry could not be opened. The entry is ignored and reported; the other
/// entries are synced as usual.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// Not encrypted to this sync key (written with another key, or not an age file of ours).
    #[error("not encrypted to this sync key")]
    WrongKey,
    /// The ciphertext was damaged or modified (authentication failed).
    #[error("the ciphertext is damaged or was modified")]
    Damaged,
    /// The payload is authentic but its opaque id or version is not the header's.
    #[error("the encrypted {0} does not match the header")]
    HeaderMismatch(&'static str),
    /// Authentic, matching, but not a valid record (bad JSON, id or content).
    #[error("the payload is malformed: {0}")]
    Malformed(String),
    /// From a newer app version (format or kind unknown here); left on the server.
    #[error("written by a newer app version: {0}")]
    Unsupported(String),
    /// Of a kind this version retired ([`super::record::RETIRED_KINDS`], decision 0072):
    /// ignored without a notice to the user, left on the server for older versions.
    #[error("a record of the retired kind {0:?}")]
    Retired(String),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    format: u32,
    opaque_id: String,
    version: Hlc,
    kind: String,
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deleted: Option<Hlc>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyEnvelope {
    format: u32,
    opaque_id: String,
    version: Hlc,
    /// The age X25519 secret key (`AGE-SECRET-KEY-1...`).
    identity: String,
}

/// The settings sync key of one sync folder. Its `Debug` output is redacted.
pub struct SyncKey {
    pub(super) identity: age::x25519::Identity,
    pub(super) recipient: age::x25519::Recipient,
    pub(super) id_key: [u8; 32],
}

impl fmt::Debug for SyncKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SyncKey([redacted])")
    }
}

impl SyncKey {
    /// A new random key (setup and reset).
    pub fn generate() -> Self {
        Self::from_identity(age::x25519::Identity::generate())
    }

    fn from_identity(identity: age::x25519::Identity) -> Self {
        let secret = identity.to_string();
        let id_key = hmac(secret.expose_secret().as_bytes(), &[LABEL_ID_KEY]);
        let recipient = identity.to_public();
        Self {
            identity,
            recipient,
            id_key,
        }
    }

    /// The key as text (`AGE-SECRET-KEY-1...`), for the system keychain.
    pub fn export(&self) -> Secret<String> {
        Secret::new(self.identity.to_string().expose_secret().to_owned())
    }

    /// The key from its [`SyncKey::export`] text.
    pub fn import(text: &Secret<String>) -> Result<Self, SettingsSyncError> {
        text.expose_secret()
            .parse::<age::x25519::Identity>()
            .map(Self::from_identity)
            .map_err(|_| SettingsSyncError::InvalidKeyRecord("not an age X25519 key".to_owned()))
    }

    /// The opaque id of a record: what the server sees instead of kind and id.
    pub fn opaque_id(&self, key: &RecordKey) -> OpaqueId {
        let mac = hmac(
            &self.id_key,
            &[key.kind().name().as_bytes(), &[0], key.id().as_bytes()],
        );
        OpaqueId(hex(mac.get(..16).unwrap_or(&mac)))
    }

    /// The opaque id of this key's key record.
    pub fn key_record_id(&self) -> OpaqueId {
        let mac = hmac(&self.id_key, &[LABEL_KEY_RECORD]);
        OpaqueId(format!(
            "{KEY_RECORD_PREFIX}{}",
            hex(mac.get(..16).unwrap_or(&mac))
        ))
    }

    /// Encrypts a record. Its header carries only the opaque id and the version.
    pub fn seal(&self, record: &SyncRecord) -> Result<SealedRecord, SettingsSyncError> {
        let header = RecordHeader {
            opaque_id: self.opaque_id(record.key()),
            version: record.version(),
        };
        let (content, deleted) = match record.body() {
            RecordBody::Content(content) => (Some(content.value().clone()), None),
            RecordBody::Tombstone { deleted } => (None, Some(*deleted)),
        };
        let envelope = Envelope {
            format: FORMAT_VERSION,
            opaque_id: header.opaque_id.to_string(),
            version: header.version,
            kind: record.kind().name().to_owned(),
            id: record.key().id().to_owned(),
            content,
            deleted,
        };
        let plaintext = padded_json(&envelope)?;
        let ciphertext = age::encrypt(&self.recipient, &plaintext)
            .map_err(|err| SettingsSyncError::Encrypt(err.to_string()))?;
        Ok(SealedRecord { header, ciphertext })
    }

    /// Decrypts and checks a record against its header.
    pub fn open(&self, header: &RecordHeader, ciphertext: &[u8]) -> Result<SyncRecord, OpenError> {
        if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(OpenError::Malformed(format!(
                "{} bytes is more than a record can be",
                ciphertext.len()
            )));
        }
        let plaintext = age::decrypt(&self.identity, ciphertext).map_err(|err| match err {
            age::DecryptError::NoMatchingKeys => OpenError::WrongKey,
            age::DecryptError::UnknownFormat => {
                OpenError::Unsupported("unknown age format".to_owned())
            }
            _ => OpenError::Damaged,
        })?;
        let value: Value = serde_json::from_slice(&plaintext)
            .map_err(|err| OpenError::Malformed(format!("payload JSON: {err}")))?;
        let format = value.get("format").and_then(Value::as_u64);
        match format {
            Some(f) if f == u64::from(FORMAT_VERSION) => {}
            Some(f) if f > u64::from(FORMAT_VERSION) => {
                return Err(OpenError::Unsupported(format!("payload format {f}")));
            }
            _ => return Err(OpenError::Malformed("payload format missing".to_owned())),
        }
        let envelope: Envelope = serde_json::from_value(value)
            .map_err(|err| OpenError::Malformed(format!("payload: {err}")))?;
        if envelope.opaque_id != header.opaque_id.as_str() {
            return Err(OpenError::HeaderMismatch("opaque id"));
        }
        if envelope.version != header.version {
            return Err(OpenError::HeaderMismatch("version"));
        }
        if RecordKind::is_retired(&envelope.kind) {
            return Err(OpenError::Retired(envelope.kind));
        }
        let kind = RecordKind::from_name(&envelope.kind)
            .ok_or_else(|| OpenError::Unsupported(format!("record kind {:?}", envelope.kind)))?;
        let key = RecordKey::parse(kind, &envelope.id)
            .map_err(|err| OpenError::Malformed(err.to_string()))?;
        if self.opaque_id(&key) != header.opaque_id {
            return Err(OpenError::HeaderMismatch("kind and id"));
        }
        let record = match (envelope.content, envelope.deleted) {
            (Some(value), None) => SyncContent::new(kind, value)
                .and_then(|content| SyncRecord::content(key, envelope.version, content)),
            (None, Some(deleted)) => SyncRecord::tombstone(key, envelope.version, deleted),
            _ => {
                return Err(OpenError::Malformed(
                    "payload needs exactly one of content and deleted".to_owned(),
                ));
            }
        };
        record.map_err(|err| OpenError::Malformed(err.to_string()))
    }

    /// The key record: this key, encrypted with the passphrase.
    pub fn wrap(
        &self,
        passphrase: &Secret<String>,
        version: Hlc,
        work_factor: WorkFactor,
    ) -> Result<SealedRecord, SettingsSyncError> {
        if passphrase.expose_secret().is_empty() {
            return Err(SettingsSyncError::EmptyPassphrase);
        }
        let header = RecordHeader {
            opaque_id: self.key_record_id(),
            version,
        };
        let envelope = KeyEnvelope {
            format: FORMAT_VERSION,
            opaque_id: header.opaque_id.to_string(),
            version,
            identity: self.identity.to_string().expose_secret().to_owned(),
        };
        let plaintext = padded_json(&envelope)?;
        let mut recipient =
            age::scrypt::Recipient::new(SecretString::from(passphrase.expose_secret().clone()));
        recipient.set_work_factor(work_factor.log_n());
        let ciphertext = age::encrypt(&recipient, &plaintext)
            .map_err(|err| SettingsSyncError::Encrypt(err.to_string()))?;
        Ok(SealedRecord { header, ciphertext })
    }

    /// Unwraps a key record with the passphrase. [`SettingsSyncError::WrongPassphrase`] if it
    /// does not fit; nothing else happens in that case.
    pub fn unwrap(
        header: &RecordHeader,
        ciphertext: &[u8],
        passphrase: &Secret<String>,
    ) -> Result<Self, SettingsSyncError> {
        let invalid = |reason: &str| SettingsSyncError::InvalidKeyRecord(reason.to_owned());
        if !header.opaque_id.is_key_record() {
            return Err(invalid("not a key record"));
        }
        if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(invalid("too large"));
        }
        let mut identity =
            age::scrypt::Identity::new(SecretString::from(passphrase.expose_secret().clone()));
        identity.set_max_work_factor(WorkFactor::MAX.log_n());
        let plaintext = age::decrypt(&identity, ciphertext).map_err(|err| match err {
            // The passphrase-derived key does not open the file key.
            age::DecryptError::DecryptionFailed | age::DecryptError::KeyDecryptionFailed => {
                SettingsSyncError::WrongPassphrase
            }
            age::DecryptError::ExcessiveWork { .. } => invalid("scrypt work factor too high"),
            age::DecryptError::NoMatchingKeys => invalid("not encrypted with a passphrase"),
            age::DecryptError::UnknownFormat => invalid("unknown age format"),
            _ => invalid("damaged"),
        })?;
        let value: Value =
            serde_json::from_slice(&plaintext).map_err(|_| invalid("payload is not JSON"))?;
        match value.get("format").and_then(Value::as_u64) {
            Some(f) if f == u64::from(FORMAT_VERSION) => {}
            Some(_) => return Err(invalid("written by a newer app version")),
            None => return Err(invalid("payload format missing")),
        }
        let envelope: KeyEnvelope =
            serde_json::from_value(value).map_err(|_| invalid("malformed payload"))?;
        let key = Secret::new(envelope.identity);
        let key = Self::import(&key)?;
        if envelope.opaque_id != header.opaque_id.as_str()
            || key.key_record_id() != header.opaque_id
        {
            return Err(invalid("opaque id does not match the header"));
        }
        if envelope.version != header.version {
            return Err(invalid("version does not match the header"));
        }
        Ok(key)
    }
}

pub(super) fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = match HmacSha256::new_from_slice(key) {
        Ok(mac) => mac,
        // HMAC takes keys of any length (longer ones are hashed first).
        Err(_) => unreachable!("HMAC accepts keys of every length"),
    };
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// JSON, padded with spaces (valid trailing JSON whitespace) to a multiple of [`PAD_BLOCK`].
pub(super) fn padded_json<T: Serialize>(value: &T) -> Result<Vec<u8>, SettingsSyncError> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|err| SettingsSyncError::Encrypt(err.to_string()))?;
    let padded = bytes.len().div_ceil(PAD_BLOCK).max(1) * PAD_BLOCK;
    bytes.resize(padded, b' ');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings_sync::hlc::DeviceId;
    use crate::settings_sync::record::SettingScope;

    fn v(millis: i64) -> Hlc {
        Hlc::new(millis, 0, DeviceId::from_uuid(uuid::Uuid::from_u128(1))).expect("v")
    }

    fn theme(version: Hlc, value: &str) -> SyncRecord {
        let key = RecordKey::setting(SettingScope::Global, "ui.theme").expect("key");
        let content = SyncContent::new(RecordKind::Setting, json!(value)).expect("content");
        SyncRecord::content(key, version, content).expect("record")
    }

    fn passphrase(text: &str) -> Secret<String> {
        Secret::new(text.to_owned())
    }

    fn test_factor() -> WorkFactor {
        WorkFactor::new(10).expect("factor")
    }

    #[test]
    fn records_round_trip_and_headers_hold_nothing_else() {
        let key = SyncKey::generate();
        let record = theme(v(5), "dark");
        let sealed = key.seal(&record).expect("seal");
        assert_eq!(sealed.header.version, v(5));
        assert_eq!(sealed.header.opaque_id, key.opaque_id(record.key()));
        let raw = sealed.header.to_raw();
        assert!(!raw.opaque_id.contains("theme") && !raw.opaque_id.contains("setting"));
        assert_eq!(RecordHeader::parse(&raw), Some(sealed.header.clone()));
        assert_eq!(
            key.open(&sealed.header, &sealed.ciphertext),
            Ok(record.clone())
        );

        let gone = SyncRecord::tombstone(record.key().clone(), v(9), v(5)).expect("tombstone");
        let sealed = key.seal(&gone).expect("seal tombstone");
        assert_eq!(key.open(&sealed.header, &sealed.ciphertext), Ok(gone));
    }

    #[test]
    fn opaque_ids_are_stable_per_key_and_record() {
        let key = SyncKey::generate();
        let other = SyncKey::generate();
        let theme_key = RecordKey::setting(SettingScope::Global, "ui.theme").expect("theme");
        let language = RecordKey::setting(SettingScope::Global, "ui.language").expect("language");
        let id = key.opaque_id(&theme_key);
        assert_eq!(id, key.opaque_id(&theme_key));
        assert_ne!(id, key.opaque_id(&language));
        assert_ne!(id, other.opaque_id(&theme_key));
        assert_eq!(id.as_str().len(), 32);
        assert!(!id.is_key_record());
        assert!(key.key_record_id().is_key_record());
        assert_ne!(key.key_record_id(), other.key_record_id());
        // The same id under another kind is another record.
        let uuid = uuid::Uuid::from_u128(3);
        assert_ne!(
            key.opaque_id(&RecordKey::uuid(RecordKind::Signature, uuid).expect("sig")),
            key.opaque_id(&RecordKey::uuid(RecordKind::Template, uuid).expect("tpl"))
        );
        // Export and import keep the ids.
        let again = SyncKey::import(&key.export()).expect("import");
        assert_eq!(again.opaque_id(&theme_key), id);
        assert_eq!(format!("{key:?}"), "SyncKey([redacted])");
    }

    #[test]
    fn swapped_or_changed_headers_are_detected() {
        let key = SyncKey::generate();
        let dark = key.seal(&theme(v(5), "dark")).expect("seal");
        let lang = {
            let k = RecordKey::setting(SettingScope::Global, "ui.language").expect("key");
            let c = SyncContent::new(RecordKind::Setting, json!("de")).expect("c");
            key.seal(&SyncRecord::content(k, v(6), c).expect("r"))
                .expect("seal")
        };
        // Replayed under a newer version.
        let newer = RecordHeader {
            version: v(50),
            ..dark.header.clone()
        };
        assert_eq!(
            key.open(&newer, &dark.ciphertext),
            Err(OpenError::HeaderMismatch("version"))
        );
        // Moved under another record's id.
        assert_eq!(
            key.open(&lang.header, &dark.ciphertext),
            Err(OpenError::HeaderMismatch("opaque id"))
        );
        // A flipped bit.
        let mut damaged = dark.ciphertext.clone();
        if let Some(last) = damaged.last_mut() {
            *last ^= 1;
        }
        assert_eq!(key.open(&dark.header, &damaged), Err(OpenError::Damaged));
        // Another key.
        let other = SyncKey::generate();
        assert_eq!(
            other.open(&dark.header, &dark.ciphertext),
            Err(OpenError::WrongKey)
        );
    }

    #[test]
    fn envelope_kind_and_id_must_match_the_opaque_id() {
        // A device with the key writes a payload whose opaque id names another record (a bug
        // or a hostile device): rejected, because the opaque id is recomputed.
        let key = SyncKey::generate();
        let lang = RecordKey::setting(SettingScope::Global, "ui.language").expect("key");
        let header = RecordHeader {
            opaque_id: key.opaque_id(&lang),
            version: v(5),
        };
        let envelope = Envelope {
            format: FORMAT_VERSION,
            opaque_id: header.opaque_id.to_string(),
            version: v(5),
            kind: "setting".into(),
            id: "global/ui.theme".into(),
            content: Some(json!("dark")),
            deleted: None,
        };
        let ciphertext =
            age::encrypt(&key.recipient, &padded_json(&envelope).expect("json")).expect("enc");
        assert_eq!(
            key.open(&header, &ciphertext),
            Err(OpenError::HeaderMismatch("kind and id"))
        );
    }

    #[test]
    fn newer_formats_and_kinds_are_unsupported_not_damaged() {
        let key = SyncKey::generate();
        let identity =
            RecordKey::parse(RecordKind::Identity, "00000000-0000-0000-0000-000000000001")
                .expect("identity key");
        let header = RecordHeader {
            opaque_id: key.opaque_id(&identity),
            version: v(5),
        };
        let seal = |value: Value| {
            age::encrypt(&key.recipient, &serde_json::to_vec(&value).expect("json")).expect("enc")
        };
        let newer = seal(json!({ "format": 2, "anything": true }));
        assert!(matches!(
            key.open(&header, &newer),
            Err(OpenError::Unsupported(_))
        ));
        let kind = seal(json!({
            "format": 1, "opaqueId": header.opaque_id.as_str(), "version": v(5),
            "kind": "calendar", "id": "x", "content": {}
        }));
        assert!(matches!(
            key.open(&header, &kind),
            Err(OpenError::Unsupported(_))
        ));
        // Stage 22 (decision 0072): a contact picture of an older version is retired, whatever
        // its content.
        let picture = seal(json!({
            "format": 1, "opaqueId": header.opaque_id.as_str(), "version": v(5),
            "kind": "contactPicture", "id": "ada@example.org", "content": { "jpeg": "/9j/4A==" }
        }));
        assert_eq!(
            key.open(&header, &picture),
            Err(OpenError::Retired("contactPicture".to_owned()))
        );
        let secret = seal(json!({
            "format": 1, "opaqueId": header.opaque_id.as_str(), "version": v(5),
            "kind": "identity", "id": "00000000-0000-0000-0000-000000000001",
            "content": { "password": "x" }
        }));
        assert!(matches!(
            key.open(&header, &secret),
            Err(OpenError::Malformed(_))
        ));
    }

    #[test]
    fn plaintexts_are_padded_to_whole_blocks() {
        // age adds a random "grease" stanza to every header, so ciphertext sizes vary by up
        // to a few hundred bytes anyway; the padding hides the content length within a block.
        let short = padded_json(&json!({ "value": "a" })).expect("short");
        let longer = padded_json(&json!({ "value": "b".repeat(100) })).expect("longer");
        let long = padded_json(&json!({ "value": "c".repeat(300) })).expect("long");
        assert_eq!(short.len(), PAD_BLOCK);
        assert_eq!(longer.len(), PAD_BLOCK);
        assert_eq!(long.len(), 2 * PAD_BLOCK);
        let parsed: Value = serde_json::from_slice(&long).expect("padding is JSON whitespace");
        assert_eq!(parsed["value"].as_str().map(str::len), Some(300));
    }

    #[test]
    fn key_record_unwraps_with_the_right_passphrase_only() {
        let key = SyncKey::generate();
        let sealed = key
            .wrap(
                &passphrase("correct horse battery staple"),
                v(1),
                test_factor(),
            )
            .expect("wrap");
        assert_eq!(sealed.header.opaque_id, key.key_record_id());
        let text = String::from_utf8_lossy(&sealed.ciphertext);
        assert!(text.contains("scrypt"), "age scrypt stanza");
        assert!(!text.contains("AGE-SECRET-KEY"));

        let unwrapped = SyncKey::unwrap(
            &sealed.header,
            &sealed.ciphertext,
            &passphrase("correct horse battery staple"),
        )
        .expect("unwrap");
        let probe = RecordKey::setting(SettingScope::Global, "ui.theme").expect("key");
        assert_eq!(unwrapped.opaque_id(&probe), key.opaque_id(&probe));

        let wrong = SyncKey::unwrap(&sealed.header, &sealed.ciphertext, &passphrase("wrong"));
        assert!(
            matches!(wrong, Err(SettingsSyncError::WrongPassphrase)),
            "{wrong:?}"
        );
        assert!(matches!(
            key.wrap(&passphrase(""), v(1), test_factor()),
            Err(SettingsSyncError::EmptyPassphrase)
        ));
        // The header is bound to the key record.
        let moved = RecordHeader {
            version: v(2),
            ..sealed.header.clone()
        };
        assert!(matches!(
            SyncKey::unwrap(
                &moved,
                &sealed.ciphertext,
                &passphrase("correct horse battery staple")
            ),
            Err(SettingsSyncError::InvalidKeyRecord(_))
        ));
    }

    #[test]
    fn default_work_factor_is_written_into_the_key_record() {
        // Wrapping is cheap to check without paying the full scrypt cost: age writes log2(N)
        // into the stanza; unwrapping at 18 is measured in the performance test.
        let key = SyncKey::generate();
        let sealed = key
            .wrap(&passphrase("p"), v(1), test_factor())
            .expect("wrap");
        assert!(String::from_utf8_lossy(&sealed.ciphertext).contains(" 10\n"));
        assert_eq!(WorkFactor::DEFAULT.log_n(), 18);
        assert!(WorkFactor::new(9).is_none());
        assert!(WorkFactor::new(21).is_none());
    }

    #[test]
    fn excessive_work_factor_is_refused_before_any_work() {
        // A key record claiming log2(N) = 40 must fail at once instead of allocating TiBs.
        let key = SyncKey::generate();
        let sealed = key
            .wrap(&passphrase("p"), v(1), test_factor())
            .expect("wrap");
        let mut forged = sealed.ciphertext.clone();
        let at = forged
            .windows(4)
            .position(|w| w == b" 10\n")
            .expect("work factor in the stanza");
        forged[at + 1] = b'4';
        let start = std::time::Instant::now();
        let result = SyncKey::unwrap(&sealed.header, &forged, &passphrase("p"));
        assert!(
            matches!(result, Err(SettingsSyncError::InvalidKeyRecord(_))),
            "{result:?}"
        );
        assert!(start.elapsed().as_secs() < 5);
    }
}
