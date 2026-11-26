//! Stage 22 (decision 0075): on macOS every secret of the app lives in one keychain item, the
//! *vault*, instead of one item per secret.
//!
//! Why: the macOS builds are signed ad hoc until they carry a Developer ID (stage 15). Every
//! update changes the code signature, and macOS then asks once per keychain item whether the
//! new version may use it. With one item it asks once per update.
//!
//! - **Format:** the item's secret is JSON, `{"format":1,"entries":{name: secret},"retired":[…]}`.
//!   `entries` maps the names the engine uses ([`CredentialRef`]) to their secrets; `retired`
//!   lists names whose old item (below) could not be deleted and must never be read again.
//! - **One writer:** every read and write goes through one lock in this process; the vault is
//!   read from the keychain once and kept in memory. A write is read-modify-write.
//! - **Generations:** a write never changes the item in place. The new content goes into a new
//!   item (`vault`, then `vault.2`, `vault.3`, …), is read back, and only an identical read
//!   counts as stored; then the older items are deleted. Readers take the highest generation
//!   listed (names come from attributes, which shows no dialog). A failed or damaged write
//!   therefore leaves the earlier content as the newest item. It also copes with macOS
//!   refusing to change or delete an item an older build created ("Invalid attempt to change
//!   the owner of this item", seen in `perf.e2e.mjs`): the new item belongs to this build, and
//!   the older one stays unused where macOS does not let it go.
//! - **Old items** (one per secret, before stage 22): [`VaultStore::migrate`] moves the names
//!   the database knows into the vault at start and deletes each old item only after the vault
//!   write was verified. A read that misses in the vault still looks for an old item and moves
//!   it then. A denied dialog, a failed write or a crash in between therefore never loses a
//!   secret: until the vault holds it, the old item stays where it was.
//!
//! No secret reaches an error or a log line: errors of this module have fixed texts, and
//! values are only handled as [`Secret<String>`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use mail_model::{CoreError, Secret};
use serde_json::{Map, Value};

use super::{CredentialRef, SecretStore};

/// Name of the vault's first generation; later ones are `vault.2`, `vault.3`, …
pub const VAULT_ITEM: &str = "vault";

/// Version of the vault's JSON. A vault with a higher number comes from a newer app: it is
/// neither read nor overwritten.
const VAULT_FORMAT: u64 = 1;

/// The platform store as it is: one item per name under the app's service.
pub trait ItemStore: Send + Sync + 'static {
    /// The item's secret, `None` if there is no item of that name.
    fn read(&self, name: &str) -> Result<Option<Secret<String>>, CoreError>;

    /// Creates the item or replaces its secret.
    fn write(&self, name: &str, value: &Secret<String>) -> Result<(), CoreError>;

    /// Deletes the item; `false` if there was none.
    fn remove(&self, name: &str) -> Result<bool, CoreError>;

    /// Names of every item under the service, from their attributes: no secret is read, so
    /// macOS shows no dialog.
    fn names(&self) -> Result<Vec<String>, CoreError>;
}

/// What [`VaultStore::migrate`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Old items whose secret is in the vault now.
    pub moved: usize,
    /// Of those, old items macOS did not let this build delete (they stay unused).
    pub kept_old: usize,
    /// Old items that could not be read (dialog denied, keychain locked); tried again later.
    pub failed: usize,
}

/// The generation of a vault item name: `vault` is 1, `vault.<n>` is n (n ≥ 2).
fn generation_of(name: &str) -> Option<u32> {
    if name == VAULT_ITEM {
        return Some(1);
    }
    let n: u32 = name
        .strip_prefix(VAULT_ITEM)?
        .strip_prefix('.')?
        .parse()
        .ok()?;
    (n >= 2 && name == item_name(n)).then_some(n)
}

fn item_name(generation: u32) -> String {
    if generation <= 1 {
        VAULT_ITEM.to_owned()
    } else {
        format!("{VAULT_ITEM}.{generation}")
    }
}

/// Names of the vault's own items are not names of secrets.
fn reserved(name: &str) -> bool {
    name == VAULT_ITEM || name.starts_with("vault.")
}

fn unreadable() -> CoreError {
    CoreError::Keychain("the keychain entry with the app's secrets is unreadable".into())
}

/// The vault's content.
#[derive(Clone, Default)]
struct Vault {
    entries: BTreeMap<String, Secret<String>>,
    retired: BTreeSet<String>,
}

impl Vault {
    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.retired.is_empty()
    }

    fn encode(&self) -> Result<Secret<String>, CoreError> {
        let entries: Map<String, Value> = self
            .entries
            .iter()
            .map(|(name, secret)| (name.clone(), Value::String(secret.expose_secret().clone())))
            .collect();
        let mut root = Map::new();
        root.insert("format".into(), Value::from(VAULT_FORMAT));
        root.insert("entries".into(), Value::Object(entries));
        if !self.retired.is_empty() {
            root.insert(
                "retired".into(),
                Value::Array(self.retired.iter().cloned().map(Value::String).collect()),
            );
        }
        serde_json::to_string(&Value::Object(root))
            .map(Secret::new)
            .map_err(|_| CoreError::Internal("the app's secrets could not be encoded".into()))
    }

    /// Parses the vault's JSON. Errors never quote the text.
    fn decode(text: &Secret<String>) -> Result<Self, CoreError> {
        let value: Value = serde_json::from_str(text.expose_secret()).map_err(|_| unreadable())?;
        let Value::Object(mut root) = value else {
            return Err(unreadable());
        };
        match root.get("format").and_then(Value::as_u64) {
            Some(VAULT_FORMAT) => {}
            Some(n) if n > VAULT_FORMAT => {
                return Err(CoreError::Keychain(
                    "the keychain entry with the app's secrets was written by a newer version"
                        .into(),
                ));
            }
            _ => return Err(unreadable()),
        }
        let mut vault = Self::default();
        match root.remove("entries") {
            Some(Value::Object(entries)) => {
                for (name, secret) in entries {
                    let Value::String(secret) = secret else {
                        return Err(unreadable());
                    };
                    vault.entries.insert(name, Secret::new(secret));
                }
            }
            _ => return Err(unreadable()),
        }
        match root.remove("retired") {
            None => {}
            Some(Value::Array(names)) => {
                for name in names {
                    let Value::String(name) = name else {
                        return Err(unreadable());
                    };
                    vault.retired.insert(name);
                }
            }
            Some(_) => return Err(unreadable()),
        }
        Ok(vault)
    }
}

/// The vault as this process knows it.
struct Loaded {
    /// Generation of the item it lives in; 0 while no item exists.
    generation: u32,
    vault: Vault,
}

/// A [`SecretStore`] that keeps every secret in one item of `I` (see the module docs).
pub struct VaultStore<I: ItemStore> {
    items: I,
    state: Mutex<Option<Loaded>>,
}

impl<I: ItemStore> std::fmt::Debug for VaultStore<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultStore").finish_non_exhaustive()
    }
}

type Guard<'a> = MutexGuard<'a, Option<Loaded>>;

impl<I: ItemStore> VaultStore<I> {
    /// Does not touch the keychain yet.
    pub fn new(items: I) -> Self {
        Self {
            items,
            state: Mutex::new(None),
        }
    }

    /// The items below (tests look at what is stored).
    pub fn items(&self) -> &I {
        &self.items
    }

    fn lock(&self) -> Guard<'_> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The generations of vault items in the keychain, lowest first.
    fn generations(&self) -> Result<Vec<u32>, CoreError> {
        let mut found: Vec<u32> = self
            .items
            .names()?
            .iter()
            .filter_map(|name| generation_of(name))
            .collect();
        found.sort_unstable();
        found.dedup();
        Ok(found)
    }

    /// Reads the vault on first use: the highest generation listed.
    fn loaded<'a>(&self, guard: &'a mut Guard<'_>) -> Result<&'a mut Loaded, CoreError> {
        if guard.is_none() {
            let mut attempt = 0;
            let loaded = loop {
                attempt += 1;
                let Some(&generation) = self.generations()?.last() else {
                    break Loaded {
                        generation: 0,
                        vault: Vault::default(),
                    };
                };
                match self.items.read(&item_name(generation))? {
                    Some(text) => {
                        break Loaded {
                            generation,
                            vault: Vault::decode(&text)?,
                        };
                    }
                    // Deleted between the listing and the read: list again once.
                    None if attempt == 1 => {}
                    None => return Err(unreadable()),
                }
            };
            **guard = Some(loaded);
        }
        guard.as_mut().ok_or_else(unreadable)
    }

    /// Stores `vault` as the new content: written into a new generation, read back, and only
    /// then the older generations are deleted (where macOS lets this build delete them). On
    /// failure nothing changed: `loaded` and the newest readable item hold the earlier content.
    fn persist(&self, loaded: &mut Loaded, vault: Vault) -> Result<(), CoreError> {
        let generations = self.generations()?;
        let highest = generations
            .last()
            .copied()
            .unwrap_or(0)
            .max(loaded.generation);
        if vault.is_empty() && self.remove_all(&generations, highest) {
            loaded.generation = 0;
            loaded.vault = vault;
            return Ok(());
        }
        let text = vault.encode()?;
        let next = highest + 1;
        let name = item_name(next);
        self.items.write(&name, &text)?;
        match self.items.read(&name) {
            Ok(Some(back)) if back.expose_secret() == text.expose_secret() => {}
            other => {
                if let Err(err) = self.items.remove(&name) {
                    tracing::warn!(error = %err, "a damaged keychain entry of the app's secrets could not be removed");
                }
                return Err(match other {
                    Err(err) => err,
                    _ => CoreError::Keychain(
                        "the keychain did not keep the app's secrets as written".into(),
                    ),
                });
            }
        }
        loaded.generation = next;
        loaded.vault = vault;
        for older in generations.into_iter().filter(|g| *g < next) {
            if let Err(err) = self.items.remove(&item_name(older)) {
                // An item of an older build: never read again, the newest one wins.
                tracing::debug!(error = %err, generation = older, "an older keychain entry of the app's secrets stays");
            }
        }
        Ok(())
    }

    /// Nothing left to keep: deletes every vault item, the newest last. `false` if an older
    /// one stays (it may hold earlier secrets): the caller then writes an empty vault as the
    /// newest, so no leftover can become the newest again.
    fn remove_all(&self, generations: &[u32], highest: u32) -> bool {
        let older_gone = generations
            .iter()
            .filter(|g| **g != highest)
            .all(|g| self.items.remove(&item_name(*g)).is_ok());
        older_gone && (highest == 0 || self.items.remove(&item_name(highest)).is_ok())
    }

    /// Deletes the old item of `name` if it may be read again (best effort).
    fn remove_old(&self, name: &str) -> Result<bool, CoreError> {
        self.items.remove(name)
    }

    /// Moves the old items of `names` (every entry the database knows of) into the vault, then
    /// deletes them. See the module docs. Names already in the vault are skipped.
    pub fn migrate(&self, names: &[CredentialRef]) -> Result<MigrationReport, CoreError> {
        let mut guard = self.lock();
        let loaded = self.loaded(&mut guard)?;
        let present: BTreeSet<String> = self.items.names()?.into_iter().collect();
        let mut report = MigrationReport::default();
        let mut moved: BTreeMap<String, Secret<String>> = BTreeMap::new();
        for name in names.iter().map(CredentialRef::as_str) {
            if reserved(name)
                || moved.contains_key(name)
                || loaded.vault.entries.contains_key(name)
                || loaded.vault.retired.contains(name)
                || !present.contains(name)
            {
                continue;
            }
            match self.items.read(name) {
                Ok(Some(secret)) => {
                    moved.insert(name.to_owned(), secret);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(credential = %name, error = %err, "an old keychain entry could not be read; it stays");
                    report.failed += 1;
                }
            }
        }
        if moved.is_empty() {
            return Ok(report);
        }
        let mut vault = loaded.vault.clone();
        vault.entries.extend(moved.clone());
        self.persist(loaded, vault)?;
        for name in moved.keys() {
            report.moved += 1;
            if let Err(err) = self.remove_old(name) {
                tracing::info!(credential = %name, error = %err, "an old keychain entry stays (unused)");
                report.kept_old += 1;
            }
        }
        Ok(report)
    }
}

impl<I: ItemStore> SecretStore for VaultStore<I> {
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError> {
        if secret.expose_secret().is_empty() {
            return Err(CoreError::InvalidArgument("the password is empty".into()));
        }
        let name = key.as_str();
        if reserved(name) {
            return Err(CoreError::InvalidArgument(format!(
                "{name:?} is reserved in the keychain"
            )));
        }
        let mut guard = self.lock();
        let loaded = self.loaded(&mut guard)?;
        let mut vault = loaded.vault.clone();
        vault.entries.insert(name.to_owned(), secret.clone());
        self.persist(loaded, vault)?;
        // An old item of the same name would only hold an earlier secret.
        if !loaded.vault.retired.contains(name)
            && let Err(err) = self.remove_old(name)
        {
            tracing::debug!(credential = %name, error = %err, "an old keychain entry stays (unused)");
        }
        Ok(())
    }

    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        let name = key.as_str();
        if reserved(name) {
            return Ok(None);
        }
        let mut guard = self.lock();
        let loaded = self.loaded(&mut guard)?;
        if let Some(secret) = loaded.vault.entries.get(name) {
            return Ok(Some(secret.clone()));
        }
        if loaded.vault.retired.contains(name) {
            return Ok(None);
        }
        // An old item that the migration has not moved yet: moved now.
        let Some(secret) = self.items.read(name)? else {
            return Ok(None);
        };
        let mut vault = loaded.vault.clone();
        vault.entries.insert(name.to_owned(), secret.clone());
        match self.persist(loaded, vault) {
            Ok(()) => {
                if let Err(err) = self.remove_old(name) {
                    tracing::info!(credential = %name, error = %err, "an old keychain entry stays (unused)");
                }
            }
            Err(err) => {
                tracing::warn!(credential = %name, error = %err, "an old keychain entry could not be moved; it stays");
            }
        }
        Ok(Some(secret))
    }

    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        let name = key.as_str();
        if reserved(name) {
            return Ok(false);
        }
        let mut guard = self.lock();
        let loaded = self.loaded(&mut guard)?;
        let mut existed = false;
        if loaded.vault.entries.contains_key(name) {
            let mut vault = loaded.vault.clone();
            vault.entries.remove(name);
            self.persist(loaded, vault)?;
            existed = true;
        }
        if loaded.vault.retired.contains(name) {
            return Ok(existed);
        }
        match self.remove_old(name) {
            Ok(old) => Ok(existed || old),
            Err(err) => {
                // It cannot go: it must never be read again instead.
                tracing::info!(credential = %name, error = %err, "an old keychain entry stays; retired");
                let mut vault = loaded.vault.clone();
                vault.retired.insert(name.to_owned());
                self.persist(loaded, vault)?;
                Ok(true)
            }
        }
    }

    fn migrate(&self, names: &[CredentialRef]) -> Result<MigrationReport, CoreError> {
        Self::migrate(self, names)
    }
}

#[cfg(test)]
mod tests;
