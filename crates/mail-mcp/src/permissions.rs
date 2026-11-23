//! Who may use the MCP server and what they see (stage 12 "Freigaben", decision 0010):
//! the master switch, the access level of every account, and the registered clients with their
//! keys. The app owns the settings and hands the host a `watch` receiver; every request is
//! checked against the current value, so switching off or revoking takes effect at once.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use mail_model::{AccountId, Secret};
use serde::{Deserialize, Serialize};

/// Random bytes from the operating system.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], RandomError> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|err| RandomError(err.to_string()))?;
    Ok(bytes)
}

/// The operating system gave no randomness (a broken system; nothing secure can be made).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no randomness from the operating system: {0}")]
pub struct RandomError(String);

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Exactly `N` bytes as `2 * N` lowercase hex digits; anything else is `None`.
pub(crate) fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let digits = text.as_bytes();
    if digits.len() != N * 2 {
        return None;
    }
    let value = |digit: u8| match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    };
    let mut bytes = [0_u8; N];
    let (pairs, _) = digits.as_chunks::<2>();
    for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
        *byte = (value(*high)? << 4) | value(*low)?;
    }
    Some(bytes)
}

/// Stable id of a registered AI client: 128 random bits, 32 lowercase hex digits. Not secret;
/// it names the client in the access log and in the handshake.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId([u8; 16]);

impl ClientId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0))
    }
}

impl fmt::Debug for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClientId({self})")
    }
}

impl FromStr for ClientId {
    type Err = TokenError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex::<16>(s).map(Self).ok_or(TokenError::Malformed)
    }
}

impl Serialize for ClientId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ClientId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The secret of a client: 256 random bits. The app keeps it in the system keychain; the AI
/// client has it in its configuration (inside the token). It is the HMAC key of the handshake
/// and never travels over the IPC channel. `Debug` prints `[redacted]`.
#[derive(Clone, Debug)]
pub struct ClientKey(Secret<[u8; 32]>);

impl ClientKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Secret::new(bytes))
    }

    /// Only for the keychain and the HMAC; never format or log it.
    pub fn expose_secret(&self) -> &[u8; 32] {
        self.0.expose_secret()
    }
}

/// What the AI client's configuration carries (environment variable `MAIL_MCP_TOKEN`):
/// `mcp1_<client id, 32 hex digits>_<key, 64 hex digits>`. The prefix lets secret scanners
/// recognise it. `Debug` and `Display` never show the key.
#[derive(Clone)]
pub struct ClientToken {
    pub id: ClientId,
    pub key: ClientKey,
}

const TOKEN_PREFIX: &str = "mcp1_";

impl ClientToken {
    /// A new client: fresh id and key.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self {
            id: ClientId(random_bytes()?),
            key: ClientKey::from_bytes(random_bytes()?),
        })
    }

    /// The token text for the client configuration. Contains the key: show it only in the
    /// setup dialog, never log it.
    pub fn expose_secret(&self) -> String {
        format!(
            "{TOKEN_PREFIX}{}_{}",
            hex(&self.id.0),
            hex(self.key.expose_secret())
        )
    }

    pub fn parse(text: &str) -> Result<Self, TokenError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TokenError::Missing);
        }
        let rest = text
            .strip_prefix(TOKEN_PREFIX)
            .ok_or(TokenError::Malformed)?;
        let (id, key) = rest.split_once('_').ok_or(TokenError::Malformed)?;
        Ok(Self {
            id: id.parse()?,
            key: ClientKey::from_bytes(parse_hex::<32>(key).ok_or(TokenError::Malformed)?),
        })
    }
}

impl fmt::Debug for ClientToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientToken")
            .field("id", &self.id)
            .field("key", &self.key)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("the token is missing")]
    Missing,
    #[error("the token is not of the form mcp1_<32 hex digits>_<64 hex digits>")]
    Malformed,
}

/// What AI clients may do with one account. Accounts without an entry are [`Hidden`].
///
/// [`Hidden`]: AccountAccess::Hidden
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountAccess {
    /// Invisible: no tool lists, finds or reads anything of it.
    #[default]
    Hidden,
    /// Listed, searchable, readable.
    Read,
    /// Also `create_draft`.
    ReadAndDrafts,
}

/// One registered AI client.
#[derive(Debug, Clone)]
pub struct ClientRecord {
    pub id: ClientId,
    /// Chosen by the user ("Claude Desktop"); shown in the access log.
    pub name: String,
    pub key: ClientKey,
}

/// Everything the host checks. The app keeps it current through the `watch` channel.
#[derive(Debug, Clone, Default)]
pub struct McpSettings {
    /// The master switch. Off (the default) means: no listening socket, every open connection
    /// is closed, every request refused. Turning it off is the emergency stop.
    pub enabled: bool,
    pub accounts: BTreeMap<AccountId, AccountAccess>,
    /// Registered clients. Revoking one removes it.
    pub clients: Vec<ClientRecord>,
}

impl McpSettings {
    pub fn access(&self, account: &AccountId) -> AccountAccess {
        self.accounts.get(account).copied().unwrap_or_default()
    }

    pub fn client(&self, id: &ClientId) -> Option<&ClientRecord> {
        self.clients.iter().find(|client| client.id == *id)
    }

    /// Removes a client; its open connections close and its token stops working at once.
    /// `false` if there was no such client.
    pub fn revoke(&mut self, id: &ClientId) -> bool {
        let before = self.clients.len();
        self.clients.retain(|client| client.id != *id);
        self.clients.len() != before
    }

    /// Emergency stop: everything off, every connection closed. Clients stay registered, so
    /// turning MCP on again needs no new setup.
    pub fn emergency_stop(&mut self) {
        self.enabled = false;
    }

    /// What `client` may do now, or why it may do nothing.
    pub fn grant(&self, client: &ClientId) -> Result<Grant, Denied> {
        if !self.enabled {
            return Err(Denied::Disabled);
        }
        let record = self.client(client).ok_or(Denied::UnknownClient)?;
        Ok(Grant {
            client: record.id,
            client_name: record.name.clone(),
            accounts: self
                .accounts
                .iter()
                .filter(|(_, access)| **access != AccountAccess::Hidden)
                .map(|(id, access)| (*id, *access))
                .collect(),
        })
    }
}

/// Why a client may do nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum Denied {
    #[error("MCP access is turned off")]
    Disabled,
    /// Never registered, or revoked.
    #[error("unknown or revoked client")]
    UnknownClient,
}

/// The rights of one client at one moment: the accounts it sees (never a hidden one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub client: ClientId,
    pub client_name: String,
    accounts: BTreeMap<AccountId, AccountAccess>,
}

impl Grant {
    /// Visible accounts, in id order.
    pub fn visible(&self) -> Vec<AccountId> {
        self.accounts.keys().copied().collect()
    }

    pub fn access(&self, account: &AccountId) -> AccountAccess {
        self.accounts.get(account).copied().unwrap_or_default()
    }

    pub fn can_read(&self, account: &AccountId) -> bool {
        self.access(account) != AccountAccess::Hidden
    }

    pub fn can_draft(&self, account: &AccountId) -> bool {
        self.access(account) == AccountAccess::ReadAndDrafts
    }

    /// Some visible account allows drafts: only then is `create_draft` listed.
    pub fn any_drafts(&self) -> bool {
        self.accounts
            .values()
            .any(|access| *access == AccountAccess::ReadAndDrafts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(n: u128) -> AccountId {
        AccountId::from_uuid(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn token_round_trip_and_redaction() {
        let token = ClientToken::generate().expect("random");
        let text = token.expose_secret();
        assert!(text.starts_with("mcp1_"));
        assert_eq!(text.len(), 5 + 32 + 1 + 64);
        let parsed = ClientToken::parse(&format!("  {text}\n")).expect("parse");
        assert_eq!(parsed.id, token.id);
        assert_eq!(parsed.key.expose_secret(), token.key.expose_secret());
        let debug = format!("{token:?}");
        assert!(!debug.contains(&hex(token.key.expose_secret())), "{debug}");
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let good = ClientToken::generate().expect("random").expose_secret();
        assert_eq!(ClientToken::parse("").err(), Some(TokenError::Missing));
        for bad in [
            good.replace("mcp1_", "mcp2_"),
            good.to_uppercase(),
            good[..good.len() - 1].to_owned(),
            format!("{good}0"),
            good.replacen('_', "-", 2),
            "mcp1_zz_zz".to_owned(),
        ] {
            assert_eq!(
                ClientToken::parse(&bad).err(),
                Some(TokenError::Malformed),
                "{bad}"
            );
        }
    }

    #[test]
    fn grants_follow_the_settings() {
        let token = ClientToken::generate().expect("random");
        let mut settings = McpSettings {
            enabled: false,
            accounts: BTreeMap::from([
                (account(1), AccountAccess::Read),
                (account(2), AccountAccess::Hidden),
                (account(3), AccountAccess::ReadAndDrafts),
            ]),
            clients: vec![ClientRecord {
                id: token.id,
                name: "Test".into(),
                key: token.key.clone(),
            }],
        };
        assert_eq!(settings.grant(&token.id), Err(Denied::Disabled));
        settings.enabled = true;
        let grant = settings.grant(&token.id).expect("grant");
        assert_eq!(grant.visible(), [account(1), account(3)]);
        assert!(grant.can_read(&account(1)) && !grant.can_draft(&account(1)));
        assert!(!grant.can_read(&account(2)) && !grant.can_read(&account(9)));
        assert!(grant.can_draft(&account(3)) && grant.any_drafts());
        settings.accounts.insert(account(3), AccountAccess::Read);
        assert!(!settings.grant(&token.id).expect("grant").any_drafts());
        assert!(settings.revoke(&token.id));
        assert!(!settings.revoke(&token.id));
        assert_eq!(settings.grant(&token.id), Err(Denied::UnknownClient));
    }
}
