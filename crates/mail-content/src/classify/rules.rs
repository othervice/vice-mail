//! The user's rules per sender: corrections ("always as …") and important senders.

use std::collections::HashMap;

use mail_model::{CategoryCorrection, MessageCategory, SenderKey, SenderKeyKind};

use super::headers::{ClassifyHeaders, list_identifier};
use crate::contacts::{normalize_address, normalize_domain};

/// The sender of a message as the rules see it: normalised From address, its domain, the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SenderIdentity {
    /// Key form of the first From address ([`crate::normalize_address`]).
    pub address: Option<String>,
    /// Its domain, ASCII lower case.
    pub domain: Option<String>,
    /// The `List-Id` identifier, lower case.
    pub list_id: Option<String>,
}

impl SenderIdentity {
    /// From a From address and a `List-Id` identifier (or the whole header value).
    pub fn new(from_email: Option<&str>, list_id: Option<&str>) -> Self {
        let normalized = from_email.and_then(normalize_address);
        Self {
            domain: normalized.as_ref().map(|a| a.domain.clone()),
            address: normalized.map(|a| a.key),
            list_id: list_id.and_then(list_identifier),
        }
    }

    pub fn of(headers: &ClassifyHeaders) -> Self {
        Self::new(headers.from_email(), headers.list_id.as_deref())
    }
}

/// Corrections and important marks, indexed for lookups. Keys that cannot be normalised (an
/// address without `@`, an empty list id) are ignored.
#[derive(Debug, Clone, Default)]
pub struct SenderRules {
    corrections: KeyIndex<CategoryCorrection>,
    important: KeyIndex<SenderKey>,
}

#[derive(Debug, Clone)]
struct KeyIndex<V> {
    addresses: HashMap<String, V>,
    domains: HashMap<String, V>,
    lists: HashMap<String, V>,
}

impl<V> Default for KeyIndex<V> {
    fn default() -> Self {
        Self {
            addresses: HashMap::new(),
            domains: HashMap::new(),
            lists: HashMap::new(),
        }
    }
}

impl<V> KeyIndex<V> {
    /// Later entries replace earlier ones with the same normalised key.
    fn insert(&mut self, key: &SenderKey, value: V) {
        let Some(normalized) = normalize_key(key) else {
            return;
        };
        let map = match key.kind {
            SenderKeyKind::Address => &mut self.addresses,
            SenderKeyKind::Domain => &mut self.domains,
            SenderKeyKind::ListId => &mut self.lists,
        };
        map.insert(normalized, value);
    }

    /// The address first, then the list, then the domain and its parents (most specific first).
    fn find(&self, sender: &SenderIdentity) -> Option<&V> {
        if let Some(value) = sender.address.as_ref().and_then(|a| self.addresses.get(a)) {
            return Some(value);
        }
        if let Some(value) = sender.list_id.as_ref().and_then(|l| self.lists.get(l)) {
            return Some(value);
        }
        let mut domain = sender.domain.as_deref()?;
        loop {
            if let Some(value) = self.domains.get(domain) {
                return Some(value);
            }
            domain = domain.split_once('.')?.1;
        }
    }
}

/// The normalised value of a key, as compared with [`SenderIdentity`].
pub fn normalize_key(key: &SenderKey) -> Option<String> {
    match key.kind {
        SenderKeyKind::Address => normalize_address(&key.value).map(|a| a.key),
        SenderKeyKind::Domain => normalize_domain(&key.value),
        SenderKeyKind::ListId => list_identifier(&key.value),
    }
}

impl SenderRules {
    pub fn new(corrections: &[CategoryCorrection], important: &[SenderKey]) -> Self {
        let mut rules = Self::default();
        for correction in corrections {
            rules
                .corrections
                .insert(&correction.key, correction.clone());
        }
        for key in important {
            rules.important.insert(key, key.clone());
        }
        rules
    }

    /// The user's category for this sender, if they set one.
    pub fn correction(&self, sender: &SenderIdentity) -> Option<&CategoryCorrection> {
        self.corrections.find(sender)
    }

    /// The important mark that applies to this sender, if any.
    pub fn important(&self, sender: &SenderIdentity) -> Option<&SenderKey> {
        self.important.find(sender)
    }

    /// The category the user set for this sender.
    pub fn category(&self, sender: &SenderIdentity) -> Option<MessageCategory> {
        self.correction(sender).map(|c| c.category)
    }
}
