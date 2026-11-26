//! Stage 22 (decision 0076): passwords from another device of the same settings sync.
//!
//! An account taken over through the sync starts without its password (decision 0040). A new
//! device can ask a device that has it, through the sync folder, in four short-lived entries:
//!
//! 1. **request** (new device): a random request id, its device name, the accounts it waits for
//!    with their servers, and a *commitment* to a fresh X25519 public key,
//!    `SHA-256(label, request id, public key)`. The private key exists only in memory.
//! 2. **challenge** (a device the user opened "Give passwords" on): a random nonce.
//! 3. **reveal** (new device, for the first challenge only): the public key, naming the
//!    challenge it answers.
//! 4. **response** (the giving device, after the user compared the codes and chose accounts):
//!    the passwords, encrypted with age to that public key, each with the account's servers;
//!    or "declined".
//!
//! Both devices show the same six-digit code, `SHA-256(label, request id, public key, nonce)`.
//! The commitment fixes the key before the nonce is known and the nonce is chosen after the
//! commitment, so whoever sits in the folder (with the sync key) cannot search for a key that
//! gives the same code on both sides: a substituted key shows another code with a chance of one
//! in a million. Without the commitment a six-digit code over a key the attacker picks could be
//! matched in about a million tries, well under a second.
//!
//! Every entry is encrypted to the sync key like a record (so the mail server reads nothing and
//! cannot write one), lies under an opaque id `xfer-` + 32 hex digits derived from kind, request
//! id and writing device, and carries the request's expiry (15 minutes). Passes leave them out
//! of the merge; any device deletes expired ones. Object keys stay neutral (`value`, `sealed`):
//! the passwords are only inside the age file sealed to the new device's key, and the check of
//! sync records against secret-named keys ([`super::record`]) is unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use mail_model::{AccountId, CredentialKind, Secret, ServerSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::crypto::{
    MAX_CIPHERTEXT_BYTES, OpaqueId, OpenError, RecordHeader, SealedRecord, SyncKey,
    TRANSFER_PREFIX, hex, hmac, padded_json,
};
use super::error::SettingsSyncError;
use super::hlc::{DeviceId, Hlc, MAX_CLOCK_DRIFT_MS};
use super::transport::{EntryHandle, RemoteEntry};

/// How long a request (and everything that answers it) is valid.
pub const TRANSFER_LIFETIME_MS: i64 = 15 * 60 * 1000;

/// Other devices delete expired entries this much later, so a clock ahead of the writer's does
/// not delete them early (the same bound as for versions).
pub const TRANSFER_CLEANUP_GRACE_MS: i64 = MAX_CLOCK_DRIFT_MS;

/// Format of the entries' JSON.
const TRANSFER_FORMAT: u32 = 1;

/// Entries listed larger than this are never downloaded (a response for 200 accounts is about
/// 100 KiB in armor).
pub const MAX_TRANSFER_ENTRY_BYTES: u64 = 512 * 1024;

/// At most this many transfer entries are downloaded per pass (the newest).
pub const MAX_TRANSFER_ENTRIES: usize = 64;

/// At most this many accounts per request.
pub const MAX_TRANSFER_ACCOUNTS: usize = 200;

/// Device names are at most 64 characters (the device list's rule).
const MAX_DEVICE_NAME_CHARS: usize = 64;

const LABEL_ID: &[u8] = b"vice-mail settings-sync v1 transfer id";
const LABEL_COMMITMENT: &[u8] = b"vice-mail password transfer v1 commitment";
const LABEL_CODE: &[u8] = b"vice-mail password transfer v1 code";

/// Random id of one request: 32 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequestId(String);

impl RequestId {
    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    pub fn parse(s: &str) -> Option<Self> {
        let ok = s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        ok.then(|| Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RequestId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| "request id is not 32 hex digits".to_owned())
    }
}

impl From<RequestId> for String {
    fn from(value: RequestId) -> Self {
        value.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An account the new device waits for, with the servers it has for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WantedAccount {
    pub account: AccountId,
    pub sign_in: CredentialKind,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
}

/// What one entry says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TransferMessage {
    Request {
        device_name: String,
        commitment: String,
        accounts: Vec<WantedAccount>,
    },
    Challenge {
        device_name: String,
        nonce: String,
    },
    Reveal {
        public_key: String,
        challenge_device: String,
        challenge_nonce: String,
    },
    Response {
        device_name: String,
        declined: bool,
        /// The age file (ASCII armor) sealed to the revealed key; absent when declined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sealed: Option<String>,
    },
}

impl TransferMessage {
    pub const fn kind(&self) -> TransferKind {
        match self {
            Self::Request { .. } => TransferKind::Request,
            Self::Challenge { .. } => TransferKind::Challenge,
            Self::Reveal { .. } => TransferKind::Reveal,
            Self::Response { .. } => TransferKind::Response,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TransferKind {
    Request,
    Challenge,
    Reveal,
    Response,
}

impl TransferKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Challenge => "challenge",
            Self::Reveal => "reveal",
            Self::Response => "response",
        }
    }
}

/// One entry of a transfer as written (the envelope without format, opaque id and version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferEntry {
    pub request_id: RequestId,
    /// The device that wrote it.
    pub device: DeviceId,
    /// Unix ms; the request's expiry, copied into every answer.
    pub expires_ms: i64,
    pub message: TransferMessage,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    format: u32,
    opaque_id: String,
    version: Hlc,
    request_id: RequestId,
    device: String,
    expires_ms: i64,
    message: TransferMessage,
}

impl SyncKey {
    /// The opaque id of a transfer entry: `xfer-` + the first 16 bytes (hex) of
    /// `HMAC-SHA-256(id_key, label, kind, request id, device)`.
    pub fn transfer_id(
        &self,
        kind: TransferKind,
        request: &RequestId,
        device: DeviceId,
    ) -> OpaqueId {
        let device = device.to_string();
        let mac = hmac(
            &self.id_key,
            &[
                LABEL_ID,
                &[0],
                kind.name().as_bytes(),
                &[0],
                request.as_str().as_bytes(),
                &[0],
                device.as_bytes(),
            ],
        );
        OpaqueId(format!(
            "{TRANSFER_PREFIX}{}",
            hex(mac.get(..16).unwrap_or(&mac))
        ))
    }

    /// Encrypts a transfer entry to the sync key, like a record.
    pub fn seal_transfer(
        &self,
        entry: &TransferEntry,
        version: Hlc,
    ) -> Result<SealedRecord, SettingsSyncError> {
        let header = RecordHeader {
            opaque_id: self.transfer_id(entry.message.kind(), &entry.request_id, entry.device),
            version,
        };
        let envelope = Envelope {
            format: TRANSFER_FORMAT,
            opaque_id: header.opaque_id.to_string(),
            version,
            request_id: entry.request_id.clone(),
            device: entry.device.to_string(),
            expires_ms: entry.expires_ms,
            message: entry.message.clone(),
        };
        let plaintext = padded_json(&envelope)?;
        let ciphertext = age::encrypt(&self.recipient, &plaintext)
            .map_err(|err| SettingsSyncError::Encrypt(err.to_string()))?;
        Ok(SealedRecord { header, ciphertext })
    }

    /// Decrypts a transfer entry and checks it against its header (opaque id recomputed from
    /// kind, request and device; version equal) and its shape.
    pub fn open_transfer(
        &self,
        header: &RecordHeader,
        ciphertext: &[u8],
    ) -> Result<TransferEntry, OpenError> {
        if !header.opaque_id.is_transfer() {
            return Err(OpenError::Malformed("not a transfer entry".to_owned()));
        }
        if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(OpenError::Malformed("too large".to_owned()));
        }
        let plaintext = age::decrypt(&self.identity, ciphertext).map_err(|err| match err {
            age::DecryptError::NoMatchingKeys => OpenError::WrongKey,
            age::DecryptError::UnknownFormat => {
                OpenError::Unsupported("unknown age format".to_owned())
            }
            _ => OpenError::Damaged,
        })?;
        let value: Value = serde_json::from_slice(&plaintext)
            .map_err(|_| OpenError::Malformed("payload is not JSON".to_owned()))?;
        match value.get("format").and_then(Value::as_u64) {
            Some(f) if f == u64::from(TRANSFER_FORMAT) => {}
            Some(f) if f > u64::from(TRANSFER_FORMAT) => {
                return Err(OpenError::Unsupported(format!("transfer format {f}")));
            }
            _ => return Err(OpenError::Malformed("payload format missing".to_owned())),
        }
        // Never quote the payload: a malformed one may hold anything.
        let envelope: Envelope = serde_json::from_value(value)
            .map_err(|_| OpenError::Malformed("transfer entry has another shape".to_owned()))?;
        if envelope.opaque_id != header.opaque_id.as_str() {
            return Err(OpenError::HeaderMismatch("opaque id"));
        }
        if envelope.version != header.version {
            return Err(OpenError::HeaderMismatch("version"));
        }
        let device: DeviceId = envelope
            .device
            .parse()
            .map_err(|_| OpenError::Malformed("device id".to_owned()))?;
        let entry = TransferEntry {
            request_id: envelope.request_id,
            device,
            expires_ms: envelope.expires_ms,
            message: envelope.message,
        };
        if self.transfer_id(entry.message.kind(), &entry.request_id, entry.device)
            != header.opaque_id
        {
            return Err(OpenError::HeaderMismatch("kind, request and device"));
        }
        check_shape(&entry).map_err(OpenError::Malformed)?;
        Ok(entry)
    }
}

fn valid_name(name: &str) -> bool {
    let chars = name.chars().count();
    chars > 0 && chars <= MAX_DEVICE_NAME_CHARS && !name.chars().any(char::is_control)
}

fn valid_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn check_shape(entry: &TransferEntry) -> Result<(), String> {
    match &entry.message {
        TransferMessage::Request {
            device_name,
            commitment,
            accounts,
        } => {
            if !valid_name(device_name) {
                return Err("device name".to_owned());
            }
            if !valid_hex(commitment) {
                return Err("commitment".to_owned());
            }
            if accounts.is_empty() || accounts.len() > MAX_TRANSFER_ACCOUNTS {
                return Err("accounts".to_owned());
            }
        }
        TransferMessage::Challenge { device_name, nonce } => {
            if !valid_name(device_name) || !valid_hex(nonce) {
                return Err("challenge".to_owned());
            }
        }
        TransferMessage::Reveal {
            public_key,
            challenge_device,
            challenge_nonce,
        } => {
            if public_key.parse::<age::x25519::Recipient>().is_err()
                || challenge_device.parse::<DeviceId>().is_err()
                || !valid_hex(challenge_nonce)
            {
                return Err("reveal".to_owned());
            }
        }
        TransferMessage::Response {
            device_name,
            declined,
            sealed,
        } => {
            if !valid_name(device_name) || *declined == sealed.is_some() {
                return Err("response".to_owned());
            }
        }
    }
    Ok(())
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// The request's commitment to `public_key` (hex).
pub fn commitment(request: &RequestId, public_key: &str) -> String {
    hex(&sha256(&[
        LABEL_COMMITMENT,
        &[0],
        request.as_str().as_bytes(),
        &[0],
        public_key.as_bytes(),
    ]))
}

/// The code both devices show: six digits, as `123 456`.
pub fn code(request: &RequestId, public_key: &str, nonce: &str) -> String {
    let digest = sha256(&[
        LABEL_CODE,
        &[0],
        request.as_str().as_bytes(),
        &[0],
        public_key.as_bytes(),
        &[0],
        nonce.as_bytes(),
    ]);
    let n = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// A fresh nonce for a challenge (64 hex digits).
pub fn nonce() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// One password as the giving device sends it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GivenItem {
    pub account: AccountId,
    pub sign_in: CredentialKind,
    /// The servers the password was entered for on the giving device.
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
    value: String,
}

impl fmt::Debug for GivenItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GivenItem")
            .field("account", &self.account)
            .field("sign_in", &self.sign_in)
            .finish_non_exhaustive()
    }
}

impl GivenItem {
    pub fn new(
        account: AccountId,
        sign_in: CredentialKind,
        imap: ServerSettings,
        smtp: ServerSettings,
        value: &Secret<String>,
    ) -> Self {
        Self {
            account,
            sign_in,
            imap,
            smtp,
            value: value.expose_secret().clone(),
        }
    }

    pub fn secret(&self) -> Secret<String> {
        Secret::new(self.value.clone())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedItems {
    format: u32,
    request_id: RequestId,
    items: Vec<GivenItem>,
}

/// Why the passwords of a response could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnsealError {
    /// Not sealed to this request's key (another key, or tampered with).
    #[error("the passwords were not sealed to this device's key")]
    WrongKey,
    #[error("the sealed passwords are damaged")]
    Damaged,
    #[error("the sealed passwords belong to another request")]
    OtherRequest,
}

/// Seals `items` to the revealed `public_key` (age X25519, ASCII armor).
pub fn seal_items(
    public_key: &str,
    request: &RequestId,
    items: Vec<GivenItem>,
) -> Result<String, SettingsSyncError> {
    let recipient: age::x25519::Recipient = public_key
        .parse()
        .map_err(|_| SettingsSyncError::Encrypt("not an age X25519 public key".to_owned()))?;
    let payload = SealedItems {
        format: TRANSFER_FORMAT,
        request_id: request.clone(),
        items,
    };
    let plaintext = padded_json(&payload)?;
    let ciphertext = age::encrypt(&recipient, &plaintext)
        .map_err(|err| SettingsSyncError::Encrypt(err.to_string()))?;
    let mut armored = Vec::new();
    {
        use std::io::Write;
        let encrypt = |err: std::io::Error| SettingsSyncError::Encrypt(err.to_string());
        let mut writer =
            age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
                .map_err(encrypt)?;
        writer.write_all(&ciphertext).map_err(encrypt)?;
        writer.finish().map_err(encrypt)?;
    }
    String::from_utf8(armored).map_err(|_| SettingsSyncError::Encrypt("armor".to_owned()))
}

/// Opens the passwords of a response with the request's private key.
pub fn open_items(
    identity: &age::x25519::Identity,
    request: &RequestId,
    sealed: &str,
) -> Result<Vec<GivenItem>, UnsealError> {
    use std::io::Read;
    let mut ciphertext = Vec::new();
    age::armor::ArmoredReader::new(sealed.as_bytes())
        .read_to_end(&mut ciphertext)
        .map_err(|_| UnsealError::Damaged)?;
    let plaintext = age::decrypt(identity, &ciphertext).map_err(|err| match err {
        age::DecryptError::NoMatchingKeys => UnsealError::WrongKey,
        _ => UnsealError::Damaged,
    })?;
    let payload: SealedItems =
        serde_json::from_slice(&plaintext).map_err(|_| UnsealError::Damaged)?;
    if payload.format != TRANSFER_FORMAT {
        return Err(UnsealError::Damaged);
    }
    if &payload.request_id != request {
        return Err(UnsealError::OtherRequest);
    }
    Ok(payload.items)
}

/// The key pair of a request; the private half never leaves memory.
pub struct RequestKey {
    identity: age::x25519::Identity,
    public_key: String,
}

impl fmt::Debug for RequestKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RequestKey([redacted])")
    }
}

impl RequestKey {
    pub fn generate() -> Self {
        let identity = age::x25519::Identity::generate();
        let public_key = identity.to_public().to_string();
        Self {
            identity,
            public_key,
        }
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub fn open(&self, request: &RequestId, sealed: &str) -> Result<Vec<GivenItem>, UnsealError> {
        open_items(&self.identity, request, sealed)
    }
}

// ---------------------------------------------------------------------------------------------
// What a device does with the transfer entries of a listing

/// A transfer entry of the folder, opened.
#[derive(Debug, Clone)]
pub struct OpenedEntry {
    pub handle: EntryHandle,
    pub header: RecordHeader,
    pub entry: TransferEntry,
}

/// The transfer entries of a listing: which to download, which to delete without reading
/// (expired by their header's time, which any device can tell).
#[derive(Debug, Default)]
pub struct TransferListing {
    pub fetch: Vec<(EntryHandle, RecordHeader)>,
    pub expired: Vec<EntryHandle>,
}

/// Sorts the transfer entries of `listing` (see [`TransferListing`]).
pub fn transfer_listing(listing: &[RemoteEntry], now_ms: i64) -> TransferListing {
    let mut out = TransferListing::default();
    let mut candidates = Vec::new();
    for entry in listing {
        let Some(header) = RecordHeader::parse(&entry.header) else {
            continue;
        };
        if !header.opaque_id.is_transfer() {
            continue;
        }
        if header.version.millis() < now_ms - TRANSFER_LIFETIME_MS - TRANSFER_CLEANUP_GRACE_MS {
            out.expired.push(entry.handle);
        } else if entry.size <= MAX_TRANSFER_ENTRY_BYTES {
            candidates.push((entry.handle, header));
        }
    }
    candidates.sort_by_key(|(handle, _)| std::cmp::Reverse(*handle));
    candidates.truncate(MAX_TRANSFER_ENTRIES);
    candidates.sort_by_key(|(handle, _)| *handle);
    out.fetch = candidates;
    out
}

/// The request this device asked (the new device's side).
#[derive(Debug)]
pub struct OwnRequest {
    pub request_id: RequestId,
    pub key: RequestKey,
    pub expires_ms: i64,
    pub accounts: Vec<WantedAccount>,
    /// The challenge this request answered, once one came.
    pub chosen: Option<Chosen>,
}

/// The giving device this request answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub device: DeviceId,
    pub device_name: String,
    pub nonce: String,
    pub code: String,
}

/// A request of another device this device answered with a challenge (the giving side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Giving {
    pub nonce: String,
    pub state: GivingState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GivingState {
    /// Challenge written; waiting for the new device to reveal its key.
    Connecting,
    /// The key fits the commitment: the code can be compared.
    Confirm { public_key: String, code: String },
    /// The new device answered another device's challenge.
    OtherDevice,
    /// The revealed key does not fit the commitment (tampered with).
    Failed,
    /// The response is written.
    Answered { declined: bool },
}

/// A request of another device seen in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenRequest {
    pub request_id: RequestId,
    pub device: DeviceId,
    pub device_name: String,
    pub expires_ms: i64,
    pub accounts: Vec<WantedAccount>,
    pub commitment: String,
}

/// What one look at the transfer entries decided.
#[derive(Debug, Default)]
pub struct TransferPlan {
    pub writes: Vec<TransferEntry>,
    pub deletes: Vec<EntryHandle>,
    /// The answer to this device's request, from the device it chose.
    pub response: Option<TransferMessage>,
    /// This device's request is gone from the folder (deleted elsewhere).
    pub own_missing: bool,
    pub seen: Vec<SeenRequest>,
}

/// Decides what to do with the opened transfer entries (`entries`, sorted by handle) for this
/// device (`me`, named `my_name`): clean up, answer a challenge, look for the response, check
/// revealed keys. Updates `own` and `giving` in place.
pub fn plan_transfers(
    entries: &[OpenedEntry],
    now_ms: i64,
    me: DeviceId,
    own: Option<&mut OwnRequest>,
    giving: &mut BTreeMap<RequestId, Giving>,
) -> TransferPlan {
    let mut plan = TransferPlan::default();
    let expired = |e: &OpenedEntry| e.entry.expires_ms + TRANSFER_CLEANUP_GRACE_MS < now_ms;
    let own_id = own.as_ref().map(|o| o.request_id.clone());
    // This device's request and reveal of a request it no longer has (the app restarted, or
    // the exchange ended): the private key is gone, so they go at once.
    let stale_own = |e: &OpenedEntry| {
        e.entry.device == me
            && matches!(
                e.entry.message.kind(),
                TransferKind::Request | TransferKind::Reveal
            )
            && own_id.as_ref() != Some(&e.entry.request_id)
    };
    let requests: BTreeMap<&RequestId, &OpenedEntry> = entries
        .iter()
        .filter(|e| e.entry.message.kind() == TransferKind::Request)
        .filter(|e| !expired(e) && !stale_own(e))
        .map(|e| (&e.entry.request_id, e))
        .collect();
    let deleted: BTreeSet<EntryHandle> = entries
        .iter()
        .filter(|e| expired(e) || stale_own(e) || !requests.contains_key(&e.entry.request_id))
        .map(|e| e.handle)
        .collect();
    plan.deletes.extend(deleted.iter().copied());
    let live = || entries.iter().filter(|e| !deleted.contains(&e.handle));

    if let Some(own) = own {
        plan.own_missing = !requests
            .get(&own.request_id)
            .is_some_and(|r| r.entry.device == me);
        if own.chosen.is_none()
            && let Some(first) = live()
                .filter(|e| e.entry.request_id == own.request_id && e.entry.device != me)
                .filter_map(|e| match &e.entry.message {
                    TransferMessage::Challenge { device_name, nonce } => Some((
                        e.header.version,
                        e.handle,
                        e.entry.device,
                        device_name,
                        nonce,
                    )),
                    _ => None,
                })
                .min_by_key(|(version, handle, ..)| (*version, *handle))
        {
            let (_, _, device, device_name, nonce) = first;
            own.chosen = Some(Chosen {
                device,
                device_name: device_name.clone(),
                nonce: nonce.clone(),
                code: code(&own.request_id, own.key.public_key(), nonce),
            });
        }
        if let Some(chosen) = &own.chosen {
            let revealed = live().any(|e| {
                e.entry.device == me
                    && e.entry.request_id == own.request_id
                    && e.entry.message.kind() == TransferKind::Reveal
            });
            if !revealed && !plan.own_missing {
                plan.writes.push(TransferEntry {
                    request_id: own.request_id.clone(),
                    device: me,
                    expires_ms: own.expires_ms,
                    message: TransferMessage::Reveal {
                        public_key: own.key.public_key().to_owned(),
                        challenge_device: chosen.device.to_string(),
                        challenge_nonce: chosen.nonce.clone(),
                    },
                });
            }
            plan.response = live()
                .filter(|e| e.entry.request_id == own.request_id && e.entry.device == chosen.device)
                .find_map(|e| match &e.entry.message {
                    message @ TransferMessage::Response { .. } => Some(message.clone()),
                    _ => None,
                });
        }
    }

    for (request_id, request) in &requests {
        let TransferMessage::Request {
            device_name,
            commitment: committed,
            accounts,
        } = &request.entry.message
        else {
            continue;
        };
        if request.entry.device == me || deleted.contains(&request.handle) {
            continue;
        }
        plan.seen.push(SeenRequest {
            request_id: (*request_id).clone(),
            device: request.entry.device,
            device_name: device_name.clone(),
            expires_ms: request.entry.expires_ms,
            accounts: accounts.clone(),
            commitment: committed.clone(),
        });
        let Some(state) = giving.get_mut(*request_id) else {
            continue;
        };
        if state.state != GivingState::Connecting {
            continue;
        }
        let reveal = live().find_map(|e| match &e.entry.message {
            TransferMessage::Reveal {
                public_key,
                challenge_device,
                challenge_nonce,
            } if e.entry.request_id == **request_id && e.entry.device == request.entry.device => {
                Some((public_key, challenge_device, challenge_nonce))
            }
            _ => None,
        });
        if let Some((public_key, challenge_device, challenge_nonce)) = reveal {
            state.state = if *challenge_device != me.to_string() || *challenge_nonce != state.nonce
            {
                GivingState::OtherDevice
            } else if commitment(request_id, public_key) != *committed {
                GivingState::Failed
            } else {
                GivingState::Confirm {
                    public_key: public_key.clone(),
                    code: code(request_id, public_key, &state.nonce),
                }
            };
        }
    }
    // Answered requests that are gone, and requests that went away, are forgotten.
    giving.retain(|id, _| requests.contains_key(id));
    plan
}

#[cfg(test)]
mod tests;
