//! Stage 19, block C (decision 0057): taking over accounts, identities, signatures and local
//! folders from a Thunderbird profile. Passwords are never read: Thunderbird keeps them in
//! `logins.json` and `key4.db`, which the core never opens; an account taken over goes through
//! the normal setup, where the user enters the password, and starts read-only like every new one.

use serde::{Deserialize, Serialize};

use crate::{AccountId, ServerSettings};

/// Most local folders a profile lists.
pub const MAX_TAKEOVER_FOLDERS: u32 = 500;

/// Largest signature file read (`sig_file` of an identity).
pub const MAX_SIGNATURE_FILE_BYTES: u32 = 64 * 1024;

/// A parsed profile of this session: 32 lowercase hex digits from the random source. The UI
/// never sees a path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct TakeoverToken(String);

impl TakeoverToken {
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == 32
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(text.to_owned()))
    }

    pub fn from_random(bits: u128) -> Self {
        Self(format!("{bits:032x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A profile listed in Thunderbird's `profiles.ini` (at the system's usual place).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThunderbirdProfile {
    /// Its position in `profiles.ini`, to open it.
    pub index: u32,
    pub name: String,
    /// The profile Thunderbird starts with.
    pub is_default: bool,
}

/// What kind of account Thunderbird had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TakeoverAccountKind {
    Imap,
    /// POP3 accounts cannot be taken over (the app speaks IMAP and JMAP); their identities
    /// and signatures are shown.
    Pop3,
    /// News, feeds and other kinds.
    Other,
}

/// A signature of an identity, as text for the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverSignature {
    /// HTML (cleaned when it is saved) or plain text.
    pub html: bool,
    /// The signature's text for the preview, at most 2,000 characters.
    pub preview: String,
    /// Read from a file the profile names (`sig_file`).
    pub from_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverIdentity {
    pub email: String,
    pub name: String,
    pub reply_to: Option<String>,
    pub signature: Option<TakeoverSignature>,
    /// A file signature that could not be read (missing, too large, not text).
    pub signature_unreadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverAccount {
    /// Its position in the preview, to take it over.
    pub index: u32,
    pub kind: TakeoverAccountKind,
    /// The account's name in Thunderbird.
    pub name: String,
    /// The first identity's address and name: the new account's.
    pub email: String,
    pub display_name: String,
    /// Incoming server (IMAP accounts only); `security` is TLS or STARTTLS (a plain
    /// connection in Thunderbird becomes STARTTLS: the app never sends a password unencrypted).
    pub imap: Option<ServerSettings>,
    pub smtp: Option<ServerSettings>,
    /// Thunderbird connected without encryption; the app will use STARTTLS.
    pub was_unencrypted: bool,
    pub identities: Vec<TakeoverIdentity>,
    /// An account of the app has this address already.
    pub already_set_up: bool,
}

/// A local folder of the profile (an mbox file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverFolder {
    pub index: u32,
    /// Path below "Local Folders", `/`-separated (`Archives/2025`).
    pub path: String,
    /// Size of the file in bytes.
    #[specta(type = specta_typescript::Number)]
    pub bytes: u64,
}

/// What a profile holds that the app can take over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverPreview {
    pub token: TakeoverToken,
    /// The profile's folder name (never its full path).
    pub profile_name: String,
    pub accounts: Vec<TakeoverAccount>,
    pub folders: Vec<TakeoverFolder>,
}

/// What opening a profile answered: none at the usual place, the user closed the folder
/// dialog, or the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TakeoverOpen {
    Cancelled,
    Opened { preview: TakeoverPreview },
}

/// What taking over an account's identities and signatures did, after the account was set up.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverApplied {
    pub account_id: Option<AccountId>,
    /// Identities added (the account's own address is updated, not added).
    pub identities: u32,
    /// Signatures created and set as the identities' defaults.
    pub signatures: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_32_hex_digits() {
        let token = TakeoverToken::from_random(7);
        assert_eq!(TakeoverToken::parse(token.as_str()), Some(token));
        assert!(TakeoverToken::parse("x").is_none());
    }
}
