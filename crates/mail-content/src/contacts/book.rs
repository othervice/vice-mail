//! The contact directory, built from the addresses of sent and received messages.

use std::collections::HashMap;
use std::fmt;

use mail_model::{ContactKind, MessageCategory};

use super::address::{NormalizedAddress, normalize_address, unicode_domain};
use super::fold::{fold, fold_german, words};
use super::name::{MAX_NAME_CHARS, clean_display_name};
use super::rank::{RECEIVED_WEIGHT, Rank, SENT_WEIGHT};
use crate::classify::is_no_reply_address;
use crate::classify::patterns::{SenderPattern, sender_pattern};
use crate::parse::MessageHeaders;

/// Display names remembered per contact (the least used are forgotten first).
pub const MAX_NAMES_PER_CONTACT: usize = 16;
/// Largest group of contacts with the same name for which merges are suggested.
const MAX_SAME_NAME_GROUP: usize = 10;

/// How an address was in a message: the user wrote to it, it wrote to the user, or it was in
/// the message otherwise (copied with the user, or as Reply-To).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Sent,
    Received,
    /// Stage 17: To or Cc of a received message, Reply-To of any, a second From.
    Seen,
}

/// One address seen in one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation<'a> {
    pub email: &'a str,
    /// The display name next to it, as in the header.
    pub name: Option<&'a str>,
    pub direction: Direction,
    /// Date of the message (Unix seconds).
    pub at: i64,
    /// Stage 17: the message is the user's own or from a person (category
    /// [`MessageCategory::Person`]), not a newsletter, notification or list mail. Only such
    /// observations make an address a person ([`ContactKind::Person`]); a received one counts
    /// for the rank only then.
    pub personal: bool,
}

/// The observations of one message (stage 17: every address of From, To, Cc, Bcc and
/// Reply-To). `direction` is the message's: [`Direction::Sent`] for the user's own (every
/// recipient is [`Direction::Sent`], Reply-To is [`Direction::Seen`]),
/// [`Direction::Received`] otherwise (the first From is [`Direction::Received`], every other
/// address [`Direction::Seen`]). Own addresses are left out by [`ContactBook::observe`].
pub fn observations<'a>(
    headers: &'a MessageHeaders,
    direction: Direction,
    category: MessageCategory,
    at: i64,
) -> Vec<Observation<'a>> {
    let personal = direction == Direction::Sent || category == MessageCategory::Person;
    let observe = move |direction: Direction| {
        move |a: &'a mail_model::Address| Observation {
            email: a.email.as_str(),
            name: a.name.as_deref(),
            direction,
            at,
            personal,
        }
    };
    match direction {
        Direction::Sent => headers
            .to
            .iter()
            .chain(&headers.cc)
            .chain(&headers.bcc)
            .map(observe(Direction::Sent))
            .chain(headers.reply_to.iter().map(observe(Direction::Seen)))
            .collect(),
        Direction::Received | Direction::Seen => headers
            .from
            .iter()
            .take(1)
            .map(observe(Direction::Received))
            .chain(
                headers
                    .from
                    .iter()
                    .skip(1)
                    .chain(&headers.to)
                    .chain(&headers.cc)
                    .chain(&headers.reply_to)
                    .map(observe(Direction::Seen)),
            )
            .collect(),
    }
}

/// Whether the directory of stage 10 counted this observation already (a sent recipient, or
/// the sender of a mail from a person, never an address nobody reads). The stage 17 backfill
/// adds only the others of the messages read before (decision 0050).
pub fn counted_before_stage17(observation: &Observation<'_>) -> bool {
    let counted = match observation.direction {
        Direction::Sent => true,
        Direction::Received => observation.personal,
        Direction::Seen => false,
    };
    counted && !is_no_reply_address(observation.email)
}

/// What kind of correspondent an address is (stage 17): nobody reads it, or a program sends
/// from it and the user never wrote to it: [`ContactKind::System`]; a newsletter address or one
/// only seen in mail that is not from a person: [`ContactKind::List`]; else a person.
pub fn contact_kind(email: &str, personal: bool, sent: u32) -> ContactKind {
    match sender_pattern(email) {
        Some(SenderPattern::NoReply(_)) => ContactKind::System,
        _ if sent > 0 => ContactKind::Person,
        Some(SenderPattern::Automated(_)) => ContactKind::System,
        Some(SenderPattern::Newsletter(_)) => ContactKind::List,
        None if personal => ContactKind::Person,
        None => ContactKind::List,
    }
}

/// Why an observation was not added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skipped {
    InvalidAddress,
    /// One of the user's own addresses.
    OwnAddress,
}

/// One address of a contact and how it was used.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactAddress {
    pub address: NormalizedAddress,
    /// Messages the user sent to it.
    pub sent: u32,
    /// Messages received from it.
    pub received: u32,
    /// Stage 17: other messages it was in ([`Direction::Seen`]).
    pub seen: u32,
    pub last_sent: Option<i64>,
    pub last_received: Option<i64>,
    pub last_seen: Option<i64>,
    pub rank: Rank,
    /// Stage 17: seen in a personal observation ([`Observation::personal`]).
    pub personal: bool,
}

impl ContactAddress {
    /// Stage 17: person, list or system address.
    pub fn kind(&self) -> ContactKind {
        contact_kind(&self.address.key, self.personal, self.sent)
    }
}

/// A display name seen for a contact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenName {
    pub name: String,
    pub count: u32,
    pub last_seen: i64,
}

/// A person (or a role address) with one or more addresses.
#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    /// The preferred address first; never empty.
    pub addresses: Vec<ContactAddress>,
    pub names: Vec<SeenName>,
    /// The name the user set; wins over every seen name.
    pub user_name: Option<String>,
}

impl Contact {
    /// The key of the preferred address: the contact's identity.
    pub fn key(&self) -> &str {
        &self.primary().address.key
    }

    pub fn primary(&self) -> &ContactAddress {
        &self.addresses[0]
    }

    /// The user's name, else the most used seen name: spellings that differ only in case,
    /// accents, word order or initials count together, and the most frequent spelling of the
    /// most used group is shown (ties: the newer, then alphabetical order).
    pub fn name(&self) -> Option<&str> {
        if let Some(name) = &self.user_name {
            return Some(name);
        }
        self.name_groups().first().map(|g| g.best.name.as_str())
    }

    /// Seen names grouped by [`name_key`], the most used group first.
    fn name_groups(&self) -> Vec<NameGroup<'_>> {
        let mut groups: Vec<NameGroup<'_>> = Vec::new();
        for seen in &self.names {
            let key = group_key(&seen.name);
            match groups.iter_mut().find(|g| g.key == key) {
                Some(group) => {
                    group.total = group.total.saturating_add(seen.count);
                    group.last_seen = group.last_seen.max(seen.last_seen);
                    if better(seen, group.best) {
                        group.best = seen;
                    }
                }
                None => groups.push(NameGroup {
                    key,
                    total: seen.count,
                    last_seen: seen.last_seen,
                    best: seen,
                }),
            }
        }
        groups.sort_by(|a, b| {
            b.total
                .cmp(&a.total)
                .then(b.last_seen.cmp(&a.last_seen))
                .then_with(|| a.best.name.cmp(&b.best.name))
        });
        groups
    }

    /// All addresses together.
    pub fn rank(&self) -> Rank {
        self.addresses
            .iter()
            .fold(Rank::NONE, |rank, a| rank.plus(a.rank))
    }

    pub fn sent(&self) -> u32 {
        self.addresses.iter().map(|a| a.sent).sum()
    }

    /// The newest message to, from or with any address.
    pub fn last_used(&self) -> Option<i64> {
        self.addresses
            .iter()
            .flat_map(|a| [a.last_sent, a.last_received, a.last_seen])
            .flatten()
            .max()
    }

    /// Folded words for prefix search (names, addresses, local-part words, domain labels), for
    /// an index column; [`ContactBook::search`] uses the same words.
    pub fn search_words(&self) -> Vec<String> {
        let mut out = self.name_words();
        for address in &self.addresses {
            address_words(&address.address, &mut out);
        }
        out
    }

    fn name_words(&self) -> Vec<String> {
        let mut out = Vec::new();
        for name in self
            .user_name
            .iter()
            .chain(self.names.iter().map(|n| &n.name))
        {
            words(name, &mut out);
        }
        out
    }

    fn add_name(&mut self, name: &str, at: i64) {
        match self.names.iter_mut().find(|n| n.name == name) {
            Some(seen) => {
                seen.count = seen.count.saturating_add(1);
                seen.last_seen = seen.last_seen.max(at);
            }
            None => self.names.push(SeenName {
                name: name.to_owned(),
                count: 1,
                last_seen: at,
            }),
        }
        if self.names.len() > MAX_NAMES_PER_CONTACT
            && let Some(weakest) = self
                .names
                .iter()
                .enumerate()
                .min_by_key(|(_, n)| (n.count, n.last_seen))
                .map(|(i, _)| i)
        {
            self.names.remove(weakest);
        }
    }
}

struct NameGroup<'a> {
    key: Vec<String>,
    total: u32,
    last_seen: i64,
    best: &'a SeenName,
}

/// Within a group: the more frequent spelling, then the newer, then one in mixed case (not
/// `ERIKA MUSTERMANN`), then alphabetical order.
fn better(candidate: &SeenName, best: &SeenName) -> bool {
    let mixed =
        |name: &str| name.chars().any(char::is_uppercase) && name.chars().any(char::is_lowercase);
    candidate
        .count
        .cmp(&best.count)
        .then(candidate.last_seen.cmp(&best.last_seen))
        .then(mixed(&candidate.name).cmp(&mixed(&best.name)))
        .then_with(|| best.name.cmp(&candidate.name))
        .is_gt()
}

/// [`name_key`], or the folded name when it has no word of two letters.
fn group_key(name: &str) -> Vec<String> {
    let key = name_key(name);
    if key.is_empty() {
        vec![fold(name)]
    } else {
        key
    }
}

fn address_words(address: &NormalizedAddress, out: &mut Vec<String>) {
    let mut push = |word: String| {
        if !word.is_empty() && !out.contains(&word) {
            out.push(word);
        }
    };
    push(fold(&address.display()));
    push(address.key.clone());
    let domain = unicode_domain(&address.domain);
    push(fold(&domain));
    for part in address
        .local
        .split(['.', '_', '-', '+'])
        .chain(domain.split('.'))
        .chain(address.domain.split('.'))
    {
        push(fold(part));
        push(fold_german(part));
    }
}

/// An autocomplete result: the contact and the address that matched (else the preferred one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactMatch<'a> {
    pub contact: &'a Contact,
    pub address: &'a ContactAddress,
}

/// A possible duplicate, for the user to confirm or dismiss. Nothing is merged without
/// [`ContactBook::merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateSuggestion {
    pub kind: DuplicateKind,
    /// Keys of the contacts concerned: one for [`DuplicateKind::NamesDiffer`], else two.
    pub contacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateKind {
    /// `alice+shop@x` and `alice@x`.
    PlusAddress,
    /// The same mailbox at the provider (Gmail ignores dots; `googlemail.com`).
    SameMailbox,
    /// The same name (case, accents, order of first and last name and initials ignored) at two
    /// addresses.
    SameName { name: String },
    /// One address seen with different names: the user may pick one.
    NamesDiffer { names: Vec<String> },
}

impl DuplicateKind {
    /// Order of suggestions: the surest first.
    fn order(&self) -> u8 {
        match self {
            Self::PlusAddress => 0,
            Self::SameMailbox => 1,
            Self::SameName { .. } => 2,
            Self::NamesDiffer { .. } => 3,
        }
    }
}

/// Merging two contacts, as the operation the user confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactMerge {
    /// Any address of the contact that stays (its preferred address stays preferred).
    pub keep: String,
    /// Any address of the contact merged into it.
    pub absorb: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactError {
    /// No contact has this address.
    NotFound(String),
    /// Both addresses belong to the same contact already.
    SameContact,
    /// The name is empty or an address.
    InvalidName,
}

impl fmt::Display for ContactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(address) => write!(f, "no contact with the address {address:?}"),
            Self::SameContact => f.write_str("both addresses belong to the same contact"),
            Self::InvalidName => f.write_str("not a usable name"),
        }
    }
}

impl std::error::Error for ContactError {}

/// The directory: contacts, an index from address key to contact, and the folded search words
/// of each contact (kept up to date on every change, so a search folds only the query).
#[derive(Debug, Clone, Default)]
pub struct ContactBook {
    contacts: Vec<Contact>,
    index: HashMap<String, usize>,
    /// Parallel to `contacts`.
    search: Vec<SearchEntry>,
    own: Vec<NormalizedAddress>,
}

#[derive(Debug, Clone, Default)]
struct SearchEntry {
    names: Vec<String>,
    /// Parallel to the contact's addresses.
    addresses: Vec<Vec<String>>,
}

impl SearchEntry {
    fn of(contact: &Contact) -> Self {
        Self {
            names: contact.name_words(),
            addresses: contact
                .addresses
                .iter()
                .map(|a| {
                    let mut words = Vec::new();
                    address_words(&a.address, &mut words);
                    words
                })
                .collect(),
        }
    }
}

impl ContactBook {
    /// An empty directory for a user with these addresses (never added as contacts).
    pub fn new(own_addresses: &[String]) -> Self {
        Self {
            contacts: Vec::new(),
            index: HashMap::new(),
            search: Vec::new(),
            own: own_addresses
                .iter()
                .filter_map(|a| normalize_address(a))
                .collect(),
        }
    }

    /// A directory from stored contacts. Contacts without addresses are dropped; an address
    /// that occurs twice stays with the first contact.
    pub fn from_contacts(own_addresses: &[String], contacts: Vec<Contact>) -> Self {
        let mut book = Self::new(own_addresses);
        for mut contact in contacts {
            contact
                .addresses
                .retain(|a| !book.index.contains_key(&a.address.key));
            let mut seen: Vec<String> = Vec::new();
            contact.addresses.retain(|a| {
                let fresh = !seen.contains(&a.address.key);
                seen.push(a.address.key.clone());
                fresh
            });
            if contact.addresses.is_empty() {
                continue;
            }
            book.search.push(SearchEntry::of(&contact));
            book.contacts.push(contact);
            book.reindex_last();
        }
        book
    }

    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    pub fn into_contacts(self) -> Vec<Contact> {
        self.contacts
    }

    /// The contact with this address (any spelling).
    pub fn find(&self, email: &str) -> Option<&Contact> {
        let key = normalize_address(email)?.key;
        self.index.get(&key).map(|&i| &self.contacts[i])
    }

    /// Adds one observation. Plus addresses and different spellings of the provider's mailbox
    /// stay separate contacts (see [`ContactBook::duplicates`]).
    pub fn observe(&mut self, observation: Observation<'_>) -> Result<(), Skipped> {
        let address = normalize_address(observation.email).ok_or(Skipped::InvalidAddress)?;
        let untagged = address.without_tag();
        if self
            .own
            .iter()
            .any(|own| own.key == address.key || untagged.as_deref() == Some(own.key.as_str()))
        {
            return Err(Skipped::OwnAddress);
        }
        let name = observation
            .name
            .and_then(|n| clean_display_name(n, &address.key));
        let mut words_changed = false;
        let index = match self.index.get(&address.key) {
            Some(&index) => index,
            None => {
                words_changed = true;
                self.contacts.push(Contact {
                    addresses: vec![ContactAddress {
                        address: address.clone(),
                        sent: 0,
                        received: 0,
                        seen: 0,
                        last_sent: None,
                        last_received: None,
                        last_seen: None,
                        rank: Rank::NONE,
                        personal: false,
                    }],
                    names: Vec::new(),
                    user_name: None,
                });
                self.search.push(SearchEntry::default());
                self.reindex_last();
                self.contacts.len() - 1
            }
        };
        let contact = &mut self.contacts[index];
        let at = observation.at;
        if let Some(entry) = contact
            .addresses
            .iter_mut()
            .find(|a| a.address.key == address.key)
        {
            // The newest spelling of the local part is the one to write to.
            let newest = [entry.last_sent, entry.last_received, entry.last_seen]
                .into_iter()
                .flatten()
                .max()
                .is_none_or(|last| at >= last);
            if newest {
                entry.address.local_as_written = address.local_as_written;
            }
            match observation.direction {
                Direction::Sent => {
                    entry.sent = entry.sent.saturating_add(1);
                    entry.last_sent = Some(entry.last_sent.map_or(at, |l| l.max(at)));
                    entry.rank = entry.rank.plus(Rank::of(SENT_WEIGHT, at));
                }
                Direction::Received => {
                    entry.received = entry.received.saturating_add(1);
                    entry.last_received = Some(entry.last_received.map_or(at, |l| l.max(at)));
                    // Stage 17: newsletters and notifications are listed, but do not rank.
                    if observation.personal {
                        entry.rank = entry.rank.plus(Rank::of(RECEIVED_WEIGHT, at));
                    }
                }
                Direction::Seen => {
                    entry.seen = entry.seen.saturating_add(1);
                    entry.last_seen = Some(entry.last_seen.map_or(at, |l| l.max(at)));
                }
            }
            entry.personal |= observation.personal;
        }
        if let Some(name) = name {
            words_changed |= !contact.names.iter().any(|n| n.name == name);
            contact.add_name(&name, at);
        }
        if words_changed {
            self.search[index] = SearchEntry::of(&self.contacts[index]);
        }
        Ok(())
    }

    /// Sets the name the user chose (`None` returns to the seen names).
    pub fn set_name(&mut self, email: &str, name: Option<&str>) -> Result<(), ContactError> {
        let index = self.index_of(email)?;
        let name = match name {
            Some(name) => {
                let trimmed: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
                if trimmed.is_empty()
                    || trimmed.contains('@')
                    || trimmed.chars().any(char::is_control)
                {
                    return Err(ContactError::InvalidName);
                }
                Some(trimmed.chars().take(MAX_NAME_CHARS).collect())
            }
            None => None,
        };
        self.contacts[index].user_name = name;
        self.search[index] = SearchEntry::of(&self.contacts[index]);
        Ok(())
    }

    /// Makes `email` the preferred address of its contact.
    pub fn set_preferred(&mut self, email: &str) -> Result<(), ContactError> {
        let key = normalize_address(email)
            .ok_or_else(|| ContactError::NotFound(email.to_owned()))?
            .key;
        let index = self.index_of(email)?;
        let addresses = &mut self.contacts[index].addresses;
        if let Some(position) = addresses.iter().position(|a| a.address.key == key) {
            let preferred = addresses.remove(position);
            addresses.insert(0, preferred);
        }
        self.search[index] = SearchEntry::of(&self.contacts[index]);
        Ok(())
    }

    /// Merges two contacts the user confirmed as one person: addresses, counts, ranks and names
    /// are combined; the user's name of `keep` wins, else that of `absorb`.
    pub fn merge(&mut self, merge: &ContactMerge) -> Result<(), ContactError> {
        let keep = self.index_of(&merge.keep)?;
        let absorb = self.index_of(&merge.absorb)?;
        if keep == absorb {
            return Err(ContactError::SameContact);
        }
        let absorbed = self.contacts.remove(absorb);
        self.search.remove(absorb);
        let keep = if absorb < keep { keep - 1 } else { keep };
        let target = &mut self.contacts[keep];
        target.addresses.extend(absorbed.addresses);
        if target.user_name.is_none() {
            target.user_name = absorbed.user_name;
        }
        for seen in absorbed.names {
            match target.names.iter_mut().find(|n| n.name == seen.name) {
                Some(known) => {
                    known.count = known.count.saturating_add(seen.count);
                    known.last_seen = known.last_seen.max(seen.last_seen);
                }
                None => target.names.push(seen),
            }
        }
        while target.names.len() > MAX_NAMES_PER_CONTACT {
            let Some(weakest) = target
                .names
                .iter()
                .enumerate()
                .min_by_key(|(_, n)| (n.count, n.last_seen))
                .map(|(i, _)| i)
            else {
                break;
            };
            target.names.remove(weakest);
        }
        self.search[keep] = SearchEntry::of(&self.contacts[keep]);
        self.reindex();
        Ok(())
    }

    /// Prefix search for autocomplete: every word of `query` must start a word of a name or an
    /// address (case, accents and German umlaut spellings ignored). Highest rank first; at most
    /// `limit` results.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ContactMatch<'_>> {
        let tokens: Vec<String> = query.split_whitespace().map(fold).collect();
        if tokens.is_empty() || limit == 0 {
            return Vec::new();
        }
        let starts = |words: &[String], token: &str| words.iter().any(|w| w.starts_with(token));
        let mut found: Vec<ContactMatch<'_>> = Vec::new();
        for (contact, entry) in self.contacts.iter().zip(&self.search) {
            let all_match = tokens.iter().all(|t| {
                starts(&entry.names, t) || entry.addresses.iter().any(|words| starts(words, t))
            });
            if !all_match {
                continue;
            }
            let address = contact
                .addresses
                .iter()
                .zip(&entry.addresses)
                .find(|(_, own)| {
                    tokens
                        .iter()
                        .all(|t| starts(&entry.names, t) || starts(own, t))
                })
                .map_or_else(|| contact.primary(), |(a, _)| a);
            found.push(ContactMatch { contact, address });
        }
        found.sort_by(|a, b| {
            b.contact
                .rank()
                .key()
                .total_cmp(&a.contact.rank().key())
                .then_with(|| a.contact.key().cmp(b.contact.key()))
        });
        found.truncate(limit);
        found
    }

    /// Possible duplicates, except pairs the user dismissed (`dismissed`: pairs of contact keys
    /// in either order; a key paired with itself dismisses [`DuplicateKind::NamesDiffer`]).
    pub fn duplicates(&self, dismissed: &[(String, String)]) -> Vec<DuplicateSuggestion> {
        let is_dismissed = |a: &str, b: &str| {
            dismissed
                .iter()
                .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
        };
        let mut out: Vec<DuplicateSuggestion> = Vec::new();
        let pair = |kind: DuplicateKind, a: &str, b: &str, out: &mut Vec<DuplicateSuggestion>| {
            if a == b || is_dismissed(a, b) {
                return;
            }
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            let contacts = vec![a.to_owned(), b.to_owned()];
            if !out.iter().any(|s| s.contacts == contacts) {
                out.push(DuplicateSuggestion { kind, contacts });
            }
        };
        for contact in &self.contacts {
            for address in &contact.addresses {
                if let Some(base) = address.address.without_tag()
                    && let Some(&other) = self.index.get(&base)
                {
                    pair(
                        DuplicateKind::PlusAddress,
                        contact.key(),
                        self.contacts[other].key(),
                        &mut out,
                    );
                }
            }
        }
        let mut mailboxes: HashMap<String, Vec<&str>> = HashMap::new();
        for contact in &self.contacts {
            for address in &contact.addresses {
                if let Some(mailbox) = address.address.provider_mailbox() {
                    let keys = mailboxes.entry(mailbox).or_default();
                    if !keys.contains(&contact.key()) {
                        keys.push(contact.key());
                    }
                }
            }
        }
        for keys in mailboxes.values() {
            for (i, a) in keys.iter().enumerate() {
                for b in &keys[i + 1..] {
                    pair(DuplicateKind::SameMailbox, a, b, &mut out);
                }
            }
        }
        let mut by_name: HashMap<Vec<String>, Vec<&Contact>> = HashMap::new();
        for contact in &self.contacts {
            if let Some(name) = contact.name() {
                let key = name_key(name);
                if key.len() >= 2 {
                    by_name.entry(key).or_default().push(contact);
                }
            }
        }
        for group in by_name.values() {
            if group.len() < 2 || group.len() > MAX_SAME_NAME_GROUP {
                continue;
            }
            for (i, a) in group.iter().enumerate() {
                for b in &group[i + 1..] {
                    let name = a.name().unwrap_or_default().to_owned();
                    pair(DuplicateKind::SameName { name }, a.key(), b.key(), &mut out);
                }
            }
        }
        for contact in &self.contacts {
            if contact.user_name.is_some() || is_dismissed(contact.key(), contact.key()) {
                continue;
            }
            let groups = contact.name_groups();
            if groups.len() >= 2 {
                out.push(DuplicateSuggestion {
                    kind: DuplicateKind::NamesDiffer {
                        names: groups.iter().map(|g| g.best.name.clone()).collect(),
                    },
                    contacts: vec![contact.key().to_owned()],
                });
            }
        }
        out.sort_by(|a, b| {
            a.kind
                .order()
                .cmp(&b.kind.order())
                .then_with(|| a.contacts.cmp(&b.contacts))
        });
        out
    }

    fn index_of(&self, email: &str) -> Result<usize, ContactError> {
        normalize_address(email)
            .and_then(|a| self.index.get(&a.key).copied())
            .ok_or_else(|| ContactError::NotFound(email.to_owned()))
    }

    fn reindex_last(&mut self) {
        let index = self.contacts.len() - 1;
        for address in &self.contacts[index].addresses {
            self.index.insert(address.address.key.clone(), index);
        }
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (index, contact) in self.contacts.iter().enumerate() {
            for address in &contact.addresses {
                self.index.insert(address.address.key.clone(), index);
            }
        }
    }
}

/// A name for comparing: folded words (umlauts spelled out) without initials, sorted, so
/// `Müller, Hans`, `hans MUELLER` and `Hans P. Müller` are equal.
fn name_key(name: &str) -> Vec<String> {
    let folded = fold_german(name);
    let mut words: Vec<String> = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 1)
        .map(str::to_owned)
        .collect();
    words.sort();
    words.dedup();
    words
}
