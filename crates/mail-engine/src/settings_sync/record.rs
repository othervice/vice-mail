//! Sync records: the registry of synchronised kinds, record ids, secret-free content and
//! tombstones.
//!
//! Only kinds in [`REGISTRY`] exist ([`RecordKind`] is closed), and of the `setting` table only
//! the keys in [`SYNCED_SETTINGS`]. Everything else stays on the device.

use std::collections::BTreeMap;
use std::fmt;

use mail_model::{AccountColor, AccountId, ServerSettings};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::RecordError;
use super::hlc::{DeviceId, Hlc};

/// What kind of data a record holds. Order and names are part of the format (`docs/SYNC.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    /// One registered setting (scope and key).
    Setting,
    Signature,
    Template,
    /// A filter rule.
    Rule,
    /// A saved search or view.
    SavedView,
    /// A snoozed message, by account and Message-ID.
    Snooze,
    /// A pinned message, by account and Message-ID.
    Pin,
    /// Account list entry: address, name, colour, server settings. Never passwords, never
    /// `writes_enabled` (see [`ACCOUNT_FIELDS`]).
    Account,
    /// A sender identity.
    Identity,
    /// AI and MCP settings (provider, base URL, model, permissions), never keys or tokens.
    AiSettings,
    /// One device of the sync: name, system, last seen.
    Device,
    /// A message marked as a task (open or done), by account and Message-ID (stage 9).
    Task,
    /// A sender rule: the category correction ("always as …") and/or the importance of an
    /// address, domain or list, by its normalised key (stage 9).
    SenderRule,
    /// An account group of the sidebar, with its members by account UUID (stage 7).
    AccountGroup,
    /// A sender whose external images always load, by account and normalised address.
    RemoteContent,
    /// The name the user gave a contact, by its normalised address.
    Contact,
    /// A message on the reading list ("Zum Lesen"), by account and Message-ID (stage 16).
    ReadingList,
    /// Favourite and colour of a mailbox, by account and decoded path (stage 16).
    MailboxPref,
    /// A blocked sender, by its normalised address (stage 16).
    BlockedSender,
    /// A pinned attachment, by account, Message-ID and attachment index (stage 16, 0036).
    AttachmentPin,
}

/// How concurrent versions of one record are merged (`docs/ARCHITECTURE.md`, Einstellungs-Sync).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeRule {
    /// The newer version wins, a tombstone like any other version.
    NewerWins,
    /// The newer version wins, but an edit the deleting device had not seen beats the delete.
    EditBeatsDelete,
    /// Like [`MergeRule::EditBeatsDelete`] per record, and among live versions every field
    /// takes its newest value.
    PerField,
}

/// The shape of a record id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdShape {
    /// `<scope>/<key>`: scope `global` or an account UUID, key from [`SYNCED_SETTINGS`].
    Setting,
    /// A lowercase hyphenated UUID, the same on every device.
    Uuid,
    /// `<account uuid>/<Message-ID without angle brackets>`. Never local ids or UIDs.
    Mail,
    /// `<address|domain|list_id>:<normalised value>` (sender rules).
    SenderKey,
    /// `<account uuid>/<normalised address>` (remote content senders).
    AccountAddress,
    /// A normalised address (contacts).
    Address,
    /// `<account uuid>/<decoded mailbox path>` (mailbox preferences, stage 16): the path may
    /// hold spaces, never controls.
    AccountPath,
    /// `<account uuid>/<Message-ID>/<attachment index>` (pinned attachments, stage 16): the
    /// index is the last segment, the Message-ID may hold `/` itself.
    MailPart,
}

/// What a winning tombstone does to a live local record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    /// The record is deleted locally.
    Apply,
    /// The user is asked first ([`super::Question::Removed`]); until then the record stays.
    Ask,
}

/// One entry of the registry.
#[derive(Debug)]
pub struct KindSpec {
    pub kind: RecordKind,
    /// Name inside the encrypted payload and input of the opaque id.
    pub name: &'static str,
    /// The local table the integration keeps in sync with these records.
    pub table: &'static str,
    pub merge: MergeRule,
    pub id: IdShape,
    pub removal: Removal,
    /// Largest serialised content. Bounds what one record can cost on every device.
    pub max_content_bytes: usize,
}

const KIB: usize = 1024;

/// The synchronised kinds, in [`RecordKind`] order. Nothing else is synchronised.
pub const REGISTRY: [KindSpec; 20] = [
    // 16 KiB since stage 8 (decision 0025): one platform's shortcut changes are one setting.
    spec(
        RecordKind::Setting,
        "setting",
        "setting",
        MergeRule::NewerWins,
        IdShape::Setting,
        Removal::Apply,
        16 * KIB,
    ),
    // A signature of the pool (stage 10, decision 0023), by its UUID (a stage 5 text
    // signature kept its identity's UUID); a deleted signature and a concurrent edit merge as
    // "edit beats delete". Up to 128 KiB of images in base64 plus the HTML.
    spec(
        RecordKind::Signature,
        "signature",
        "signature",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        256 * KIB,
    ),
    spec(
        RecordKind::Template,
        "template",
        "template",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        256 * KIB,
    ),
    spec(
        RecordKind::Rule,
        "rule",
        "rule",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        16 * KIB,
    ),
    spec(
        RecordKind::SavedView,
        "savedView",
        "saved_view",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        16 * KIB,
    ),
    spec(
        RecordKind::Snooze,
        "snooze",
        "snooze",
        MergeRule::NewerWins,
        IdShape::Mail,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::Pin,
        "pin",
        "pin",
        MergeRule::NewerWins,
        IdShape::Mail,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::Account,
        "account",
        "account",
        MergeRule::PerField,
        IdShape::Uuid,
        Removal::Ask,
        16 * KIB,
    ),
    spec(
        RecordKind::Identity,
        "identity",
        "identity",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        16 * KIB,
    ),
    spec(
        RecordKind::AiSettings,
        "aiSettings",
        "ai_profile",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        16 * KIB,
    ),
    spec(
        RecordKind::Device,
        "device",
        "sync_device",
        MergeRule::NewerWins,
        IdShape::Uuid,
        Removal::Apply,
        4 * KIB,
    ),
    spec(
        RecordKind::Task,
        "task",
        "task",
        MergeRule::NewerWins,
        IdShape::Mail,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::SenderRule,
        "senderRule",
        "sender_rule",
        MergeRule::NewerWins,
        IdShape::SenderKey,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::AccountGroup,
        "accountGroup",
        "account_group",
        MergeRule::EditBeatsDelete,
        IdShape::Uuid,
        Removal::Apply,
        16 * KIB,
    ),
    spec(
        RecordKind::RemoteContent,
        "remoteContent",
        "remote_content_sender",
        MergeRule::NewerWins,
        IdShape::AccountAddress,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::Contact,
        "contact",
        "contact",
        MergeRule::NewerWins,
        IdShape::Address,
        Removal::Apply,
        4 * KIB,
    ),
    // Stage 16 (decision 0033): the reading list like tasks, mailbox favourites and colours by
    // decoded path (as local rules name a mailbox), blocked senders by normalised address.
    spec(
        RecordKind::ReadingList,
        "readingList",
        "reading_list",
        MergeRule::NewerWins,
        IdShape::Mail,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::MailboxPref,
        "mailboxPref",
        "mailbox_pref",
        MergeRule::NewerWins,
        IdShape::AccountPath,
        Removal::Apply,
        KIB,
    ),
    spec(
        RecordKind::BlockedSender,
        "blockedSender",
        "blocked_sender",
        MergeRule::NewerWins,
        IdShape::Address,
        Removal::Apply,
        KIB,
    ),
    // Decision 0036: a pinned attachment keeps its name, type and size (names up to 1 KiB).
    spec(
        RecordKind::AttachmentPin,
        "attachmentPin",
        "attachment_pin",
        MergeRule::NewerWins,
        IdShape::MailPart,
        Removal::Apply,
        2 * KIB,
    ),
];

#[allow(clippy::too_many_arguments)]
const fn spec(
    kind: RecordKind,
    name: &'static str,
    table: &'static str,
    merge: MergeRule,
    id: IdShape,
    removal: Removal,
    max_content_bytes: usize,
) -> KindSpec {
    KindSpec {
        kind,
        name,
        table,
        merge,
        id,
        removal,
        max_content_bytes,
    }
}

/// Setting keys that are synchronised (decision 0022): theme, language, when an opened message
/// counts as read, the undo delay of sending, the notification settings, and (stage 8,
/// decision 0025) the default quote position of replies, the shortcut changes of each platform
/// and the text size. Every other key of
/// the `setting` table is a device setting: `prefetch.*` (what this machine keeps offline),
/// `inbox.settings` (categories and bundles on this screen, decision 0020),
/// `ui.recent_commands` and `ui.layout.*` (this device's palette history and window layouts).
pub const SYNCED_SETTINGS: &[&str] = &[
    "ui.theme",
    "ui.language",
    "reading.mark_read_on_open",
    "compose.send_delay_seconds",
    "notifications.settings",
    "compose.default_quote_position",
    "ui.shortcuts.macos",
    "ui.shortcuts.windows",
    "ui.text_size",
    // Stage 16 (decision 0033): "send and archive" for new replies, the read-receipt answer.
    "compose.archive_on_reply",
    "compose.read_receipts",
    // Stage 16 (decision 0036): the accent theme, like the text size.
    "ui.accent_theme",
    // Decision 0039: the conversation view, like the text size.
    "ui.conversations",
];

/// Fields of an account record, each with its own version. Passwords and tokens live in the
/// keychain and have no field. `writes_enabled` is deliberately not synchronised: write access
/// is switched on by the user per account on each device, and an account taken over from
/// another device starts read-only.
pub const ACCOUNT_FIELDS: &[&str] = &["email", "displayName", "color", "imap", "smtp"];

impl RecordKind {
    pub const ALL: [Self; 20] = [
        Self::Setting,
        Self::Signature,
        Self::Template,
        Self::Rule,
        Self::SavedView,
        Self::Snooze,
        Self::Pin,
        Self::Account,
        Self::Identity,
        Self::AiSettings,
        Self::Device,
        Self::Task,
        Self::SenderRule,
        Self::AccountGroup,
        Self::RemoteContent,
        Self::Contact,
        Self::ReadingList,
        Self::MailboxPref,
        Self::BlockedSender,
        Self::AttachmentPin,
    ];

    pub const fn spec(self) -> &'static KindSpec {
        &REGISTRY[self as usize]
    }

    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    /// The kind with this payload name, `None` if it is not registered.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

impl fmt::Display for RecordKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Scope of a synchronised setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingScope {
    Global,
    Account(AccountId),
}

/// Kind and id of a record: what the opaque id is derived from. Ordered by kind, then id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordKey {
    kind: RecordKind,
    id: String,
}

/// Longest Message-ID accepted (RFC 5322 line limit).
const MAX_MESSAGE_ID_BYTES: usize = 998;

impl RecordKey {
    /// A registered setting. Unregistered keys are refused: they are device settings.
    pub fn setting(scope: SettingScope, key: &str) -> Result<Self, RecordError> {
        let scope = match scope {
            SettingScope::Global => "global".to_owned(),
            SettingScope::Account(account) => account.to_string(),
        };
        Self::parse(RecordKind::Setting, &format!("{scope}/{key}"))
    }

    /// A record of a kind whose id is a UUID (signature, template, rule, saved view, account,
    /// identity, AI settings, device).
    pub fn uuid(kind: RecordKind, uuid: uuid::Uuid) -> Result<Self, RecordError> {
        Self::parse(kind, &uuid.hyphenated().to_string())
    }

    pub fn account(account: AccountId) -> Self {
        Self {
            kind: RecordKind::Account,
            id: account.to_string(),
        }
    }

    pub fn device(device: DeviceId) -> Self {
        Self {
            kind: RecordKind::Device,
            id: device.to_string(),
        }
    }

    /// A snooze or pin: the message by account UUID and Message-ID header (surrounding
    /// whitespace and one pair of angle brackets are removed).
    pub fn mail(
        kind: RecordKind,
        account: AccountId,
        message_id: &str,
    ) -> Result<Self, RecordError> {
        let trimmed = message_id.trim();
        let bare = trimmed
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(trimmed);
        Self::parse(kind, &format!("{account}/{bare}"))
    }

    /// Checks `id` against the shape of `kind` (also used for ids read from the payload).
    pub fn parse(kind: RecordKind, id: &str) -> Result<Self, RecordError> {
        let invalid = |reason: &str| RecordError::InvalidId {
            kind: kind.name(),
            reason: reason.to_owned(),
        };
        match kind.spec().id {
            IdShape::Uuid => {
                if !is_canonical_uuid(id) {
                    return Err(invalid("not a lowercase hyphenated UUID"));
                }
            }
            IdShape::Setting => {
                let (scope, key) = id.split_once('/').ok_or_else(|| invalid("no scope"))?;
                if scope != "global" && !is_canonical_uuid(scope) {
                    return Err(invalid("scope is neither global nor an account UUID"));
                }
                if !SYNCED_SETTINGS.contains(&key) {
                    return Err(RecordError::SettingNotSynced(key.to_owned()));
                }
            }
            IdShape::Mail | IdShape::AccountAddress => {
                let (account, message_id) =
                    id.split_once('/').ok_or_else(|| invalid("no account"))?;
                if !is_canonical_uuid(account) {
                    return Err(invalid("account is not a lowercase hyphenated UUID"));
                }
                if !is_token(message_id) {
                    return Err(invalid(
                        "the part after the account is empty, too long, or holds whitespace, \
                         controls or brackets",
                    ));
                }
            }
            IdShape::SenderKey => {
                let (kind, value) = id.split_once(':').ok_or_else(|| invalid("no key kind"))?;
                if !matches!(kind, "address" | "domain" | "list_id") {
                    return Err(invalid("key kind is not address, domain or list_id"));
                }
                if !is_token(value) {
                    return Err(invalid("key is empty, too long or holds whitespace"));
                }
            }
            IdShape::Address => {
                if !is_token(id) {
                    return Err(invalid("address is empty, too long or holds whitespace"));
                }
            }
            IdShape::MailPart => {
                let (account, rest) = id.split_once('/').ok_or_else(|| invalid("no account"))?;
                if !is_canonical_uuid(account) {
                    return Err(invalid("account is not a lowercase hyphenated UUID"));
                }
                let (message_id, index) = rest
                    .rsplit_once('/')
                    .ok_or_else(|| invalid("no attachment index"))?;
                if !is_token(message_id) {
                    return Err(invalid(
                        "the Message-ID is empty, too long, or holds whitespace, controls or \
                         brackets",
                    ));
                }
                if index.is_empty()
                    || !index.bytes().all(|b| b.is_ascii_digit())
                    || (index.len() > 1 && index.starts_with('0'))
                    || index.parse::<u32>().is_err()
                {
                    return Err(invalid("the attachment index is not a number"));
                }
            }
            IdShape::AccountPath => {
                let (account, path) = id.split_once('/').ok_or_else(|| invalid("no account"))?;
                if !is_canonical_uuid(account) {
                    return Err(invalid("account is not a lowercase hyphenated UUID"));
                }
                if path.trim().is_empty()
                    || path.len() > MAX_MESSAGE_ID_BYTES
                    || path.chars().any(char::is_control)
                {
                    return Err(invalid("the path is empty, too long or holds controls"));
                }
            }
        }
        Ok(Self {
            kind,
            id: id.to_owned(),
        })
    }

    pub const fn kind(&self) -> RecordKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for RecordKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.id)
    }
}

/// A Message-ID, normalised address or key: 1 to 998 bytes, no whitespace, controls or angle
/// brackets.
fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_MESSAGE_ID_BYTES
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
}

fn is_canonical_uuid(s: &str) -> bool {
    uuid::Uuid::try_parse(s).is_ok_and(|u| u.hyphenated().to_string() == s)
}

/// Validated record content (JSON). The only way to build one is [`SyncContent::new`] (or a
/// helper that calls it), which refuses secrets, so a record can never carry one:
///
/// - [`mail_model::Secret`] has no `Serialize`, so a struct holding one cannot be turned into
///   content at all ([`SyncContent::from_serialize`]).
/// - No object key anywhere in the content may name a password, passphrase, secret, token,
///   API or private key, credential, authorization or cookie (case, `_` and `-` ignored).
/// - No string may contain key material (an age secret key or a PEM private key).
/// - Account content only has the fields in [`ACCOUNT_FIELDS`], device content only
///   [`DeviceInfo`]; both are checked against typed structs without password fields.
/// - Content is bounded by the kind's `max_content_bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncContent {
    kind: RecordKind,
    value: Value,
}

/// Parts of object keys that mark a secret (compared on the lowercased key without `_`/`-`).
const SECRET_NAME_PARTS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "apikey",
    "accesskey",
    "privatekey",
    "credential",
    "authorization",
    "cookie",
];
/// Endings of object keys that mark a secret (`token`, `accessToken`, `refresh_token`), but not
/// `maxTokens`.
const SECRET_NAME_ENDINGS: &[&str] = &["token", "pwd"];
/// Whole object keys that mark a secret.
const SECRET_NAMES: &[&str] = &["pass", "otp"];
/// Markers of key material in string values.
const SECRET_VALUE_MARKERS: &[&str] = &["AGE-SECRET-KEY-", "PRIVATE KEY-----"];

impl SyncContent {
    /// Validates `value` for `kind` (see the type docs).
    pub fn new(kind: RecordKind, value: Value) -> Result<Self, RecordError> {
        let spec = kind.spec();
        let size = serde_json::to_vec(&value)
            .map_err(|err| RecordError::InvalidContent {
                kind: spec.name,
                reason: err.to_string(),
            })?
            .len();
        if size > spec.max_content_bytes {
            return Err(RecordError::TooLarge {
                kind: spec.name,
                size,
                max: spec.max_content_bytes,
            });
        }
        scan_for_secrets(spec.name, &value, &mut String::from("$"))?;
        check_structure(kind, &value)?;
        Ok(Self { kind, value })
    }

    /// Serialises `value` and validates it.
    pub fn from_serialize<T: Serialize>(kind: RecordKind, value: &T) -> Result<Self, RecordError> {
        let value = serde_json::to_value(value).map_err(|err| RecordError::InvalidContent {
            kind: kind.name(),
            reason: err.to_string(),
        })?;
        Self::new(kind, value)
    }

    /// Per-field content for [`MergeRule::PerField`] kinds.
    pub fn from_fields(
        kind: RecordKind,
        fields: &BTreeMap<String, FieldValue>,
    ) -> Result<Self, RecordError> {
        if kind.spec().merge != MergeRule::PerField {
            return Err(RecordError::InvalidContent {
                kind: kind.name(),
                reason: "kind has no per-field versions".to_owned(),
            });
        }
        Self::from_serialize(kind, fields)
    }

    pub const fn kind(&self) -> RecordKind {
        self.kind
    }

    pub const fn value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }

    /// Decodes the content into a typed value.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, RecordError> {
        T::deserialize(&self.value).map_err(|err| RecordError::InvalidContent {
            kind: self.kind.name(),
            reason: err.to_string(),
        })
    }

    /// The versioned fields of a [`MergeRule::PerField`] record, `None` for other kinds.
    pub fn fields(&self) -> Option<BTreeMap<String, FieldValue>> {
        if self.kind.spec().merge != MergeRule::PerField {
            return None;
        }
        self.decode().ok()
    }

    /// The plain field values of a per-field record (without versions).
    pub fn field_values(&self) -> Option<BTreeMap<String, Value>> {
        self.fields()
            .map(|fields| fields.into_iter().map(|(k, f)| (k, f.value)).collect())
    }
}

fn scan_for_secrets(
    kind: &'static str,
    value: &Value,
    path: &mut String,
) -> Result<(), RecordError> {
    match value {
        Value::Object(map) => {
            for (name, inner) in map {
                let normalized: String = name
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .map(|c| c.to_ascii_lowercase())
                    .collect();
                let len = path.len();
                path.push('.');
                path.push_str(name);
                let secret = SECRET_NAME_PARTS.iter().any(|p| normalized.contains(p))
                    || SECRET_NAME_ENDINGS.iter().any(|e| normalized.ends_with(e))
                    || SECRET_NAMES.contains(&normalized.as_str());
                if secret {
                    return Err(RecordError::SecretField {
                        kind,
                        path: path.clone(),
                    });
                }
                scan_for_secrets(kind, inner, path)?;
                path.truncate(len);
            }
        }
        Value::Array(items) => {
            for (i, inner) in items.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                scan_for_secrets(kind, inner, path)?;
                path.truncate(len);
            }
        }
        Value::String(text) => {
            if SECRET_VALUE_MARKERS.iter().any(|m| text.contains(m)) {
                return Err(RecordError::SecretValue {
                    kind,
                    path: path.clone(),
                });
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn check_structure(kind: RecordKind, value: &Value) -> Result<(), RecordError> {
    let invalid = |reason: String| RecordError::InvalidContent {
        kind: kind.name(),
        reason,
    };
    match kind {
        RecordKind::Setting => {
            if value.is_null() {
                return Err(invalid(
                    "a setting value is not null (delete it instead)".into(),
                ));
            }
        }
        RecordKind::Account => {
            let fields: BTreeMap<String, FieldValue> =
                BTreeMap::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            for (name, field) in &fields {
                check_account_field(name, &field.value).map_err(invalid)?;
            }
        }
        RecordKind::Device => {
            DeviceInfo::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::Snooze => {
            SnoozeInfo::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::Pin => {
            PinInfo::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::Task => {
            TaskInfo::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::ReadingList => {
            ReadingListInfo::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::MailboxPref => {
            let pref =
                MailboxPrefContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            if pref
                .color
                .is_some_and(|c| AccountColor::new(c.index()).is_none())
            {
                return Err(invalid("not a colour of the palette".into()));
            }
            if pref.favorite_position.is_none() && pref.color.is_none() {
                return Err(invalid(
                    "a mailbox preference sets a favourite or a colour (delete it instead)".into(),
                ));
            }
        }
        RecordKind::BlockedSender => {
            EmptyContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::AttachmentPin => {
            let pin =
                AttachmentPinContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            if pin.name.is_empty() || pin.name.chars().any(char::is_control) {
                return Err(invalid(
                    "the attachment name is empty or holds controls".into(),
                ));
            }
            if pin.mime_type.chars().any(char::is_control) {
                return Err(invalid("the attachment type holds controls".into()));
            }
        }
        RecordKind::Signature => {
            let signature =
                SignatureContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            if signature.text.is_none() && signature.html.is_none() {
                return Err(invalid("a signature needs text or html".into()));
            }
        }
        RecordKind::Template => {
            let template =
                TemplateContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            template.check_recipients().map_err(invalid)?;
        }
        RecordKind::Identity => {
            IdentityContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::Rule => {
            let rule = RuleContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            let definition = rule.definition.as_object();
            let shaped = definition.is_some_and(|d| {
                d.get("matchAll").is_some_and(Value::is_boolean)
                    && d.get("stop").is_some_and(Value::is_boolean)
                    && d.get("conditions").is_some_and(Value::is_array)
                    && d.get("actions").is_some_and(Value::is_array)
            });
            if !shaped {
                return Err(invalid(
                    "a rule definition has matchAll, conditions, actions and stop".into(),
                ));
            }
        }
        RecordKind::SenderRule => {
            let rule =
                SenderRuleContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            if rule.category.is_none() && !rule.important {
                return Err(invalid(
                    "a sender rule sets a category or importance (delete it instead)".into(),
                ));
            }
        }
        RecordKind::AccountGroup => {
            AccountGroupContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::RemoteContent => {
            EmptyContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
        }
        RecordKind::Contact => {
            let contact =
                ContactContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            if contact.user_name.is_none() && contact.merged_into.is_none() {
                return Err(invalid(
                    "a contact record has a name or a merge (delete it instead)".into(),
                ));
            }
        }
        // Stage 7 (decision 0026): a saved search, checked like one saved here.
        RecordKind::SavedView => {
            let view =
                SavedViewContent::deserialize(value).map_err(|err| invalid(err.to_string()))?;
            if mail_model::clean_saved_search_name(&view.name).as_deref() != Some(&view.name) {
                return Err(invalid("not a usable name for a saved search".into()));
            }
            if mail_model::clean_search_query(&view.query).as_deref() != Some(&view.query) {
                return Err(invalid("not a usable query for a saved search".into()));
            }
        }
        // No local table yet (AI settings): any object, kept as it is.
        RecordKind::AiSettings => {
            if !value.is_object() {
                return Err(invalid("content is not a JSON object".into()));
            }
        }
    }
    Ok(())
}

fn check_account_field(name: &str, value: &Value) -> Result<(), String> {
    let typed = |result: Result<(), serde_json::Error>| {
        result.map_err(|err| format!("field {name}: {err}"))
    };
    match name {
        "email" | "displayName" => typed(String::deserialize(value).map(drop)),
        "color" => match u8::deserialize(value).ok().and_then(AccountColor::new) {
            Some(_) => Ok(()),
            None => Err("field color: not an account colour index".to_owned()),
        },
        // Stage 14 (decision 0042): only in records of accounts that sign in with a provider;
        // the provider's name, never a token.
        "oauth" => typed(mail_model::OAuthProvider::deserialize(value).map(drop)),
        "imap" | "smtp" => typed(StrictServerSettings::deserialize(value).map(drop)),
        // Stage 14 (decision 0041): only in records of JMAP accounts.
        "protocol" => typed(mail_model::Protocol::deserialize(value).map(drop)),
        _ => Err(format!("unknown account field {name:?}")),
    }
}

/// [`ServerSettings`] with unknown fields refused, so a server entry cannot smuggle extra data.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)] // only deserialised to check the shape
struct StrictServerSettings {
    host: String,
    port: u16,
    security: mail_model::Security,
    username: String,
}

/// One field of a per-field record with the version that last changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldValue {
    pub version: Hlc,
    pub value: Value,
}

/// The synchronised part of an account. Built from the local account row; has no password,
/// token or `writes_enabled` field by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncedAccount {
    pub email: String,
    pub display_name: String,
    pub color: AccountColor,
    /// Stage 14 (decision 0042): only written for accounts that sign in with Microsoft or
    /// Google; a device taking the account over signs in itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<mail_model::OAuthProvider>,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
    /// Stage 14 (decision 0041): only written for JMAP accounts (records without it are IMAP,
    /// as before); fixed when the account is set up or taken over.
    #[serde(default, skip_serializing_if = "mail_model::Protocol::is_imap")]
    pub protocol: mail_model::Protocol,
}

impl SyncedAccount {
    /// The field values (keys are [`ACCOUNT_FIELDS`]).
    pub fn to_values(&self) -> Result<BTreeMap<String, Value>, RecordError> {
        let value = serde_json::to_value(self).map_err(|err| RecordError::InvalidContent {
            kind: RecordKind::Account.name(),
            reason: err.to_string(),
        })?;
        match value {
            Value::Object(map) => Ok(map.into_iter().collect()),
            _ => Err(RecordError::InvalidContent {
                kind: RecordKind::Account.name(),
                reason: "account did not serialise to an object".to_owned(),
            }),
        }
    }

    /// Reads an account record's content back.
    pub fn from_content(content: &SyncContent) -> Result<Self, RecordError> {
        let values = content
            .field_values()
            .ok_or_else(|| RecordError::InvalidContent {
                kind: content.kind().name(),
                reason: "not an account record".to_owned(),
            })?;
        Self::deserialize(Value::Object(values.into_iter().collect())).map_err(|err| {
            RecordError::InvalidContent {
                kind: RecordKind::Account.name(),
                reason: err.to_string(),
            }
        })
    }
}

/// Content of a device record (the device list in the sync status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceInfo {
    /// Name the user gave the device (default: the computer name).
    pub name: String,
    /// Operating system, for example `macOS 26.1`.
    pub system: String,
    /// Unix ms of the last completed pass. Updated at most daily, so the folder does not churn.
    pub last_seen_ms: i64,
}

/// Content of a snooze record. Whether it came back already is each device's own business
/// (every device's timer brings it back), so only the time is synchronised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnoozeInfo {
    /// Unix ms when the message comes back.
    pub until_ms: i64,
}

/// Content of a pin record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PinInfo {
    pub pinned_at_ms: i64,
}

/// Content of a task record: open (`None`) or done at this time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskInfo {
    pub done_at_ms: Option<i64>,
}

/// Content of a reading list record (stage 16): when it was added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingListInfo {
    pub added_at_ms: i64,
}

/// Content of a pinned attachment record (stage 16, decision 0036): when it was pinned and
/// what it was (sanitised name, declared type, size), so the list shows it without the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachmentPinContent {
    pub pinned_at_ms: i64,
    pub name: String,
    pub mime_type: String,
    pub size: u32,
}

/// Content of a mailbox preference record (stage 16): favourite position and colour, at least
/// one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailboxPrefContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite_position: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<AccountColor>,
}

/// Content of a signature record: a signature of the pool (stage 10, decision 0023). `html` is
/// the allowlisted editor HTML (absent for a stage 5 text signature), `images` the pictures it
/// shows as `cid:<contentId>`. A record of stage 11 (`{text}` only) still reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignatureContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<SignatureImageContent>,
}

/// An image of a signature record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignatureImageContent {
    pub id: String,
    pub content_id: String,
    pub file_name: String,
    pub mime: String,
    pub data_base64: String,
}

/// Content of a template record (stage 10). Stage 16: the recipients it prefills, each field
/// left out when empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateContent {
    pub name: String,
    pub subject: String,
    pub html: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<mail_model::Address>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<mail_model::Address>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<mail_model::Address>,
}

impl TemplateContent {
    /// At most [`mail_model::MAX_TEMPLATE_RECIPIENTS`] valid addresses together.
    fn check_recipients(&self) -> Result<(), String> {
        let all = || self.to.iter().chain(&self.cc).chain(&self.bcc);
        if all().count() > mail_model::MAX_TEMPLATE_RECIPIENTS {
            return Err(format!(
                "more than {} template recipients",
                mail_model::MAX_TEMPLATE_RECIPIENTS
            ));
        }
        match all().find(|a| mail_content::normalize_email(&a.email).is_err()) {
            Some(bad) => Err(format!("template recipient {:?} is not valid", bad.email)),
            None => Ok(()),
        }
    }
}

/// Content of an identity record (the signatures are records of their own; the identity names
/// its defaults by UUID, stage 10, absent when none is set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityContent {
    pub account: AccountId,
    pub email: String,
    pub name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_new: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_reply: Option<String>,
}

/// Content of a local rule. `definition` is the rule's conditions and actions as stored
/// (`{matchAll, conditions, actions, stop}`); conditions and actions this version does not
/// know are kept and skipped when rules run, like those of a newer version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleContent {
    pub name: String,
    pub position: i64,
    pub enabled: bool,
    pub definition: Value,
}

/// Content of a sender rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SenderRuleContent {
    pub category: Option<mail_model::MessageCategory>,
    pub important: bool,
}

/// Content of a saved search (stage 7, decision 0026): name and query as stored (cleaned), and
/// its place in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedViewContent {
    pub name: String,
    pub query: String,
    pub position: i64,
}

/// Content of an account group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountGroupContent {
    pub name: String,
    pub position: i64,
    pub accounts: Vec<AccountId>,
}

/// Content of a record whose existence is all there is (remote content senders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyContent {}

/// Content of a contact record (by normalised address): the name the user gave it and, for an
/// address the user merged into another contact, that contact's address (stage 10). At least
/// one of the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
}

/// A record: content or tombstone, with the version that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRecord {
    key: RecordKey,
    version: Hlc,
    body: RecordBody,
}

/// What a record holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordBody {
    Content(SyncContent),
    /// The record was deleted at the tombstone's version (its time is the deletion time,
    /// which starts the 180 days of retention). `deleted` is the version the deleting device
    /// had seen: an edit newer than that was concurrent and beats the delete
    /// ([`MergeRule::EditBeatsDelete`]).
    Tombstone {
        deleted: Hlc,
    },
}

impl SyncRecord {
    pub fn content(
        key: RecordKey,
        version: Hlc,
        content: SyncContent,
    ) -> Result<Self, RecordError> {
        if content.kind() != key.kind() {
            return Err(RecordError::InvalidContent {
                kind: key.kind().name(),
                reason: format!("content was validated as {}", content.kind()),
            });
        }
        Ok(Self {
            key,
            version,
            body: RecordBody::Content(content),
        })
    }

    pub fn tombstone(key: RecordKey, version: Hlc, deleted: Hlc) -> Result<Self, RecordError> {
        if deleted >= version {
            return Err(RecordError::TombstoneOrder { version, deleted });
        }
        Ok(Self {
            key,
            version,
            body: RecordBody::Tombstone { deleted },
        })
    }

    pub const fn key(&self) -> &RecordKey {
        &self.key
    }

    pub const fn kind(&self) -> RecordKind {
        self.key.kind
    }

    pub const fn version(&self) -> Hlc {
        self.version
    }

    pub const fn body(&self) -> &RecordBody {
        &self.body
    }

    pub const fn content_ref(&self) -> Option<&SyncContent> {
        match &self.body {
            RecordBody::Content(content) => Some(content),
            RecordBody::Tombstone { .. } => None,
        }
    }

    pub const fn is_tombstone(&self) -> bool {
        matches!(self.body, RecordBody::Tombstone { .. })
    }

    /// The version a tombstone deletes; `None` for content.
    pub const fn deleted(&self) -> Option<Hlc> {
        match self.body {
            RecordBody::Tombstone { deleted } => Some(deleted),
            RecordBody::Content(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn account_id() -> AccountId {
        "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id")
    }

    #[test]
    fn registry_lists_every_kind_once_in_enum_order() {
        for (i, kind) in RecordKind::ALL.into_iter().enumerate() {
            assert_eq!(REGISTRY.get(i).map(|s| s.kind), Some(kind));
            assert_eq!(RecordKind::from_name(kind.name()), Some(kind));
        }
        let mut names: Vec<&str> = REGISTRY.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), REGISTRY.len());
        assert_eq!(RecordKind::from_name("password"), None);
        assert_eq!(RecordKind::from_name("message"), None);
    }

    #[test]
    fn only_registered_settings_are_synced() {
        let key = RecordKey::setting(SettingScope::Global, "ui.theme").expect("theme");
        assert_eq!(key.id(), "global/ui.theme");
        assert_eq!(
            RecordKey::setting(SettingScope::Global, "prefetch.period"),
            Err(RecordError::SettingNotSynced("prefetch.period".into()))
        );
        let scoped = RecordKey::setting(SettingScope::Account(account_id()), "ui.language")
            .expect("account scope");
        assert_eq!(
            scoped.id(),
            "6f9619ff-8b86-4d11-b42d-00c04fc964ff/ui.language"
        );
        assert!(RecordKey::parse(RecordKind::Setting, "device/ui.theme").is_err());
    }

    #[test]
    fn mail_ids_use_account_uuid_and_message_id() {
        let key = RecordKey::mail(RecordKind::Snooze, account_id(), " <abc.123@example.org> ")
            .expect("snooze");
        assert_eq!(
            key.id(),
            "6f9619ff-8b86-4d11-b42d-00c04fc964ff/abc.123@example.org"
        );
        assert_eq!(
            RecordKey::mail(RecordKind::Pin, account_id(), "abc.123@example.org")
                .expect("pin")
                .id(),
            key.id()
        );
        for bad in ["", "<>", "a b@c", "a\u{7}@c"] {
            assert!(
                RecordKey::mail(RecordKind::Pin, account_id(), bad).is_err(),
                "{bad:?}"
            );
        }
        // A local message id or UID is not a valid record id.
        assert!(RecordKey::parse(RecordKind::Snooze, "1764000000").is_err());
        assert!(RecordKey::mail(RecordKind::Signature, account_id(), "a@b").is_err());
    }

    #[test]
    fn uuid_ids_are_canonical() {
        let uuid = uuid::Uuid::from_u128(7);
        let key = RecordKey::uuid(RecordKind::Signature, uuid).expect("signature");
        assert_eq!(key.id(), "00000000-0000-0000-0000-000000000007");
        assert!(
            RecordKey::parse(
                RecordKind::Signature,
                "00000000-0000-0000-0000-00000000000A"
            )
            .is_err()
        );
        assert!(RecordKey::parse(RecordKind::Signature, "global/ui.theme").is_err());
    }

    /// Decision 0036: a pinned attachment is `<account>/<Message-ID>/<index>`; the Message-ID
    /// may hold `/`, the index is a plain number. The content keeps name, type and size.
    #[test]
    fn attachment_pins_check_their_ids_and_content() {
        let account = account_id();
        for good in [
            format!("{account}/a@b/0"),
            format!("{account}/a/b@c/12"),
            format!("{account}/x@y/4294967295"),
        ] {
            assert!(
                RecordKey::parse(RecordKind::AttachmentPin, &good).is_ok(),
                "{good:?}"
            );
        }
        for bad in [
            format!("{account}/a@b"),
            format!("{account}/a@b/"),
            format!("{account}//1"),
            format!("{account}/a@b/01"),
            format!("{account}/a@b/-1"),
            format!("{account}/a@b/x"),
            format!("{account}/a@b/4294967296"),
            format!("{account}/a b@c/1"),
            "not-a-uuid/a@b/1".to_owned(),
        ] {
            assert!(
                RecordKey::parse(RecordKind::AttachmentPin, &bad).is_err(),
                "{bad:?}"
            );
        }
        let pin = json!({ "pinnedAtMs": 1, "name": "a.pdf", "mimeType": "application/pdf",
                          "size": 3 });
        assert!(SyncContent::new(RecordKind::AttachmentPin, pin).is_ok());
        for bad in [
            json!({ "pinnedAtMs": 1, "name": "a.pdf", "mimeType": "x" }),
            json!({ "pinnedAtMs": 1, "name": "", "mimeType": "x", "size": 1 }),
            json!({ "pinnedAtMs": 1, "name": "a\u{7}", "mimeType": "x", "size": 1 }),
            json!({ "pinnedAtMs": 1, "name": "a", "mimeType": "x", "size": 1, "path": "/" }),
        ] {
            assert!(
                SyncContent::new(RecordKind::AttachmentPin, bad.clone()).is_err(),
                "{bad}"
            );
        }
        assert_eq!(
            RecordKind::from_name("attachmentPin"),
            Some(RecordKind::AttachmentPin)
        );
        assert_eq!(RecordKind::AttachmentPin.spec().table, "attachment_pin");
    }

    #[test]
    fn stage_16_records_check_their_ids_and_content() {
        let account = account_id();
        let key = RecordKey::mail(RecordKind::ReadingList, account, "<read-1@x>").expect("mail");
        assert_eq!(key.id(), format!("{account}/read-1@x"));
        // A folder by decoded path: spaces and slashes allowed, controls and no path not.
        assert!(
            RecordKey::parse(RecordKind::MailboxPref, &format!("{account}/Sent Items")).is_ok()
        );
        assert!(RecordKey::parse(RecordKind::MailboxPref, &format!("{account}/A/B ü")).is_ok());
        for bad in [
            format!("{account}/"),
            format!("{account}/ "),
            format!("{account}/a\u{7}b"),
            "INBOX".to_owned(),
            "not-a-uuid/INBOX".to_owned(),
        ] {
            assert!(
                RecordKey::parse(RecordKind::MailboxPref, &bad).is_err(),
                "{bad:?}"
            );
        }
        assert!(RecordKey::parse(RecordKind::BlockedSender, "spam@x.example").is_ok());
        assert!(RecordKey::parse(RecordKind::BlockedSender, "two words@x").is_err());

        assert!(SyncContent::new(RecordKind::ReadingList, json!({ "addedAtMs": 1 })).is_ok());
        assert!(SyncContent::new(RecordKind::ReadingList, json!({})).is_err());
        assert!(SyncContent::new(RecordKind::MailboxPref, json!({ "color": 7 })).is_ok());
        assert!(
            SyncContent::new(RecordKind::MailboxPref, json!({ "favoritePosition": 3 })).is_ok()
        );
        for bad in [
            json!({}),
            json!({ "color": 8 }),
            json!({ "color": "green" }),
            json!({ "favoritePosition": 1, "extra": true }),
        ] {
            assert!(
                SyncContent::new(RecordKind::MailboxPref, bad.clone()).is_err(),
                "{bad}"
            );
        }
        assert!(SyncContent::new(RecordKind::BlockedSender, json!({})).is_ok());
        assert!(SyncContent::new(RecordKind::BlockedSender, json!({ "x": 1 })).is_err());
    }

    #[test]
    fn content_refuses_secret_fields_and_key_material() {
        for (kind, value) in [
            (
                RecordKind::Identity,
                json!({ "name": "A", "password": "x" }),
            ),
            (
                RecordKind::Identity,
                json!({ "smtp": { "Pass_Word": "x" } }),
            ),
            (
                RecordKind::AiSettings,
                json!({ "provider": "openai", "apiKey": "x" }),
            ),
            (
                RecordKind::AiSettings,
                json!({ "provider": "x", "api_key": "x" }),
            ),
            (
                RecordKind::AiSettings,
                json!({ "clients": [{ "name": "cli", "token": "x" }] }),
            ),
            (RecordKind::AiSettings, json!({ "refreshToken": "x" })),
            (
                RecordKind::Rule,
                json!({ "oauth": { "access-token": "x" } }),
            ),
            (RecordKind::Template, json!({ "clientSecret": "x" })),
            (RecordKind::Setting, json!({ "passphrase": "x" })),
            (RecordKind::Identity, json!({ "credentials": {} })),
            (RecordKind::SavedView, json!({ "pass": "x" })),
        ] {
            let err = SyncContent::new(kind, value.clone()).expect_err("refused");
            assert!(
                matches!(err, RecordError::SecretField { .. }),
                "{value}: {err:?}"
            );
            assert!(!err.to_string().contains("\"x\""), "{err}");
        }
        let err = SyncContent::new(
            RecordKind::Signature,
            json!({ "html": "AGE-SECRET-KEY-1QQQQ" }),
        )
        .expect_err("key material");
        assert_eq!(
            err,
            RecordError::SecretValue {
                kind: "signature",
                path: "$.html".into()
            }
        );
        // Ordinary fields that merely contain similar letters are fine.
        SyncContent::new(
            RecordKind::AiSettings,
            json!({ "maxTokens": 512, "model": "m" }),
        )
        .expect("maxTokens");
        SyncContent::new(
            RecordKind::AiSettings,
            json!({ "keyword": "passport", "passive": true }),
        )
        .expect("passport");
        SyncContent::new(
            RecordKind::SavedView,
            json!({ "name": "Reisepass", "query": "passport", "position": 0 }),
        )
        .expect("passport");
    }

    #[test]
    fn content_is_bounded_and_shaped_by_kind() {
        let big = "x".repeat(17 * KIB);
        assert!(matches!(
            SyncContent::new(RecordKind::Setting, json!(big)),
            Err(RecordError::TooLarge { .. })
        ));
        assert!(SyncContent::new(RecordKind::Setting, Value::Null).is_err());
        assert!(SyncContent::new(RecordKind::Signature, json!("not an object")).is_err());
        assert!(SyncContent::new(RecordKind::Snooze, json!({ "untilMs": 5 })).is_ok());
        assert!(SyncContent::new(RecordKind::Snooze, json!({ "until": 5 })).is_err());
        assert!(
            SyncContent::new(
                RecordKind::Device,
                json!({ "name": "Mac", "system": "macOS", "lastSeenMs": 1, "extra": 1 })
            )
            .is_err()
        );
    }

    #[test]
    fn account_content_has_only_the_listed_fields() {
        let device = DeviceId::from_uuid(uuid::Uuid::from_u128(1));
        let version = Hlc::new(1, 0, device).expect("version");
        let server =
            json!({ "host": "imap.example.org", "port": 993, "security": "tls", "username": "a" });
        let field = |value: Value| FieldValue { version, value };
        let mut fields = BTreeMap::new();
        fields.insert("email".to_owned(), field(json!("a@example.org")));
        fields.insert("imap".to_owned(), field(server.clone()));
        SyncContent::from_fields(RecordKind::Account, &fields).expect("account");

        let mut extra = fields.clone();
        extra.insert("writesEnabled".to_owned(), field(json!(true)));
        assert!(SyncContent::from_fields(RecordKind::Account, &extra).is_err());

        let mut smuggled = fields.clone();
        let mut with_password = server;
        with_password["secretValue"] = json!("x");
        smuggled.insert("smtp".to_owned(), field(with_password));
        assert!(SyncContent::from_fields(RecordKind::Account, &smuggled).is_err());
        let mut unknown = fields.clone();
        unknown.insert(
            "smtp".to_owned(),
            field(json!({ "host": "h", "port": 1, "security": "tls", "username": "u", "x": 1 })),
        );
        assert!(SyncContent::from_fields(RecordKind::Account, &unknown).is_err());

        // Stage 14 (decision 0042): the sign-in provider, by name only.
        let mut oauth = fields.clone();
        oauth.insert("oauth".to_owned(), field(json!("microsoft")));
        SyncContent::from_fields(RecordKind::Account, &oauth).expect("provider");
        oauth.insert("oauth".to_owned(), field(json!("yahoo")));
        assert!(SyncContent::from_fields(RecordKind::Account, &oauth).is_err());
        oauth.insert("oauth".to_owned(), field(json!({ "refreshToken": "x" })));
        assert!(SyncContent::from_fields(RecordKind::Account, &oauth).is_err());
    }

    #[test]
    fn tombstones_delete_older_versions_only() {
        let device = DeviceId::from_uuid(uuid::Uuid::from_u128(1));
        let key = RecordKey::uuid(RecordKind::Rule, uuid::Uuid::from_u128(3)).expect("key");
        let v1 = Hlc::new(1, 0, device).expect("v1");
        let v2 = Hlc::new(2, 0, device).expect("v2");
        assert!(SyncRecord::tombstone(key.clone(), v2, v1).is_ok());
        assert!(SyncRecord::tombstone(key.clone(), v1, v1).is_err());
        let content =
            SyncContent::new(RecordKind::Signature, json!({ "text": "Ann" })).expect("content");
        assert!(
            SyncRecord::content(key, v1, content).is_err(),
            "kind mismatch"
        );
    }

    /// Stage 7 (decision 0026): saved searches have a strict shape, checked like a local save.
    #[test]
    fn template_records_carry_checked_recipients() {
        let base = || json!({ "name": "T", "subject": "S", "html": "<p>T</p>" });
        assert!(
            SyncContent::new(RecordKind::Template, base()).is_ok(),
            "stage 10 shape"
        );
        let mut with = base();
        with["to"] = json!([{ "name": "Bob", "email": "bob@example.org" }]);
        with["bcc"] = json!([{ "name": null, "email": "archive@example.org" }]);
        let content = SyncContent::new(RecordKind::Template, with.clone()).expect("recipients");
        let decoded: TemplateContent = content.decode().expect("decode");
        assert_eq!(decoded.to.len(), 1);
        assert!(decoded.cc.is_empty());
        assert_eq!(
            serde_json::to_value(&decoded).expect("json"),
            with,
            "empty cc left out"
        );
        let mut bad = base();
        bad["cc"] = json!([{ "name": null, "email": "no address" }]);
        assert!(SyncContent::new(RecordKind::Template, bad).is_err());
        let mut many = base();
        many["to"] = json!(
            (0..=mail_model::MAX_TEMPLATE_RECIPIENTS)
                .map(|i| json!({ "name": null, "email": format!("p{i}@example.org") }))
                .collect::<Vec<_>>()
        );
        assert!(SyncContent::new(RecordKind::Template, many).is_err());
        let mut unknown = base();
        unknown["replyTo"] = json!([]);
        assert!(SyncContent::new(RecordKind::Template, unknown).is_err());
    }

    #[test]
    fn saved_views_are_shaped_like_saved_searches() {
        let ok =
            json!({ "name": "Rechnungen", "query": "from:bank has:attachment", "position": 3 });
        assert!(SyncContent::new(RecordKind::SavedView, ok).is_ok());
        for bad in [
            json!({ "query": "is:unread" }),
            json!({ "name": "x", "query": "y", "position": 0, "scope": "all" }),
            json!({ "name": "", "query": "y", "position": 0 }),
            json!({ "name": " padded ", "query": "y", "position": 0 }),
            json!({ "name": "x", "query": "  ", "position": 0 }),
            json!({ "name": "x", "query": "a".repeat(2049), "position": 0 }),
            json!({ "name": "x".repeat(101), "query": "y", "position": 0 }),
            json!({ "name": "x", "query": "y", "position": "first" }),
        ] {
            assert!(
                SyncContent::new(RecordKind::SavedView, bad.clone()).is_err(),
                "{bad}"
            );
        }
    }
}
