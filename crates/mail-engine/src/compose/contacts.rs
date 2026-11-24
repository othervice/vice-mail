//! The contact directory for autocomplete (decision 0016), fed from stored header data:
//! messages the user sent (From is an own address: every To and Cc recipient) and messages
//! received from people (the sender, only for the category Person). A background job reads
//! the messages not read yet (`message.contacts_seen`), in batches, and the store keeps one row
//! per address. Blind copies are not in the stored headers and are not counted.
//!
//! Stage 10 (decision 0023): the directory for the UI. Ranked pages (a merged contact once),
//! names the user gives, duplicate suggestions from `mail_content::ContactBook`, merges only
//! with the user's confirmation, and the commands on [`crate::AppCore`].
//!
//! Stage 17 (decision 0050): every address of From, To, Cc and Reply-To becomes a contact;
//! list and system addresses get their kind and are left out of autocomplete and, by default,
//! of the contacts view. Messages read by the rules of stage 10 are read again in the
//! background (oldest first, never blocking the start), adding only what those rules left
//! out, so ranks stay as they were; Reply-To of such a message comes from its stored body.

use std::collections::{BTreeSet, HashMap};

use std::collections::HashSet;

use mail_content::{
    ClassifyHeaders, Contact, ContactAddress, ContactBook, Direction, DuplicateKind, LocalSignals,
    MessageHeaders, NormalizedAddress, Rank, SeenName, classify_automatic, counted_before_stage17,
    fold, is_no_reply_address, normalize_address, observations, unicode_domain,
};
use mail_model::{
    ContactAddressInfo, ContactDuplicate, ContactEntry, ContactFilter, ContactKind,
    ContactMergeRequest, ContactNameRecord, ContactPage, ContactRequestError, ContactSuggestion,
    CoreError, DuplicateReason, Flags, MAX_CONTACT_PAGE, MAX_DUPLICATE_SCAN, MessageCategory,
    MessageHeader,
};
use mail_store::{
    ContactCursor, ContactSource, MergeOutcome, SeparateOutcome, Store, StoredContact,
    StoredContactGroup,
};

use crate::core::Connector;

/// Messages read into the directory per transaction.
pub(crate) const CONTACTS_BATCH: u32 = 500;

/// Stage 17: the largest stored mail the backfill reads Reply-To from.
const MAX_BACKFILL_BODY_BYTES: u64 = 1024 * 1024;

/// Stage 17: contact rows per step when the kinds are recomputed.
const KIND_BATCH: u32 = 2_000;

fn to_contact(stored: &StoredContact) -> Contact {
    let local = stored
        .key
        .rsplit_once('@')
        .map_or_else(|| stored.key.clone(), |(l, _)| l.to_owned());
    Contact {
        addresses: vec![ContactAddress {
            address: NormalizedAddress {
                key: stored.key.clone(),
                local_as_written: stored.local_part.clone(),
                local,
                domain: stored.domain.clone(),
            },
            sent: stored.sent,
            received: stored.received,
            seen: stored.seen,
            last_sent: stored.last_sent,
            last_received: stored.last_received,
            last_seen: stored.last_seen,
            rank: Rank::from_key(stored.rank),
            personal: stored.personal,
        }],
        names: stored
            .names
            .iter()
            .map(|n| SeenName {
                name: n.name.clone(),
                count: n.count,
                last_seen: n.last_seen,
            })
            .collect(),
        user_name: stored.user_name.clone(),
    }
}

/// The folded search words of a contact as stored (` word word …`).
fn search_text(contact: &Contact) -> String {
    let mut search = String::new();
    for word in contact.search_words() {
        search.push(' ');
        search.push_str(&word);
    }
    search
}

/// One row per address; merged addresses keep their own rows (stage 10: they share a group).
fn to_rows(contact: &Contact, groups: &HashMap<String, Option<i64>>) -> Vec<StoredContact> {
    let search = search_text(contact);
    contact
        .addresses
        .iter()
        .map(|a| StoredContact {
            key: a.address.key.clone(),
            local_part: a.address.local_as_written.clone(),
            domain: a.address.domain.clone(),
            group: groups.get(&a.address.key).copied().flatten(),
            sent: a.sent,
            received: a.received,
            last_sent: a.last_sent,
            last_received: a.last_received,
            rank: a.rank.key(),
            names: contact
                .names
                .iter()
                .map(|n| ContactNameRecord {
                    name: n.name.clone(),
                    count: n.count,
                    last_seen: n.last_seen,
                })
                .collect(),
            user_name: contact.user_name.clone(),
            search: search.clone(),
            seen: a.seen,
            last_seen: a.last_seen,
            personal: a.personal,
            kind: a.kind().code(),
            // The directory job never writes the user's note (`UPSERT_CONTACT`).
            note: None,
        })
        .collect()
}

fn headers_of(source: &ContactSource) -> MessageHeaders {
    MessageHeaders {
        from: source.from.iter().cloned().collect(),
        to: source.to.clone(),
        cc: source.cc.clone(),
        reply_to: source.reply_to.clone(),
        subject: Some(source.subject.clone()),
        ..MessageHeaders::default()
    }
}

/// The category the classifier gives the stored header data (rules that need more headers do
/// not fire; the store keeps those of decision 0016's `from_message_header`).
fn category_of(source: &ContactSource, own: &[String]) -> MessageCategory {
    let header = MessageHeader {
        uid: 1,
        modseq: None,
        flags: Flags::default(),
        size: 0,
        internal_date: source.date,
        date: Some(source.date),
        subject: source.subject.clone(),
        from: source.from.iter().cloned().collect(),
        to: source.to.clone(),
        cc: source.cc.clone(),
        message_id: None,
        in_reply_to: None,
        references: Vec::new(),
        list_id: source.list_id.clone(),
        list_unsubscribe: source.list_unsubscribe.clone(),
        auto_submitted: source.auto_submitted.clone(),
        precedence: source.precedence.clone(),
        has_attachments: false,
        category_headers: Vec::new(),
        gmail: None,
    };
    let headers = ClassifyHeaders::from_message_header(&header);
    classify_automatic(
        &headers,
        &LocalSignals {
            own_addresses: own,
            sent_to_sender: 0,
            replies_to_own: false,
        },
    )
    .category
}

fn is_own(address: &str, own: &[String]) -> bool {
    let Some(address) = normalize_address(address) else {
        return false;
    };
    let untagged = address.without_tag();
    own.iter()
        .filter_map(|o| normalize_address(o))
        .any(|o| o.key == address.key || untagged.as_deref() == Some(o.key.as_str()))
}

/// The user's own addresses: every identity of every account.
pub(crate) async fn own_addresses(store: &Store) -> Result<Vec<String>, CoreError> {
    let mut own = Vec::new();
    for account in store.accounts().await? {
        own.push(account.email.clone());
        for identity in store.identities(account.id).await? {
            own.push(identity.email);
        }
    }
    own.sort();
    own.dedup();
    Ok(own)
}

/// Stage 17: Reply-To of a message read before stage 17, from its stored mail (whole and at
/// most [`MAX_BACKFILL_BODY_BYTES`]); empty otherwise.
async fn stored_reply_to(
    store: &Store,
    message: mail_model::MessageId,
) -> Vec<mail_model::Address> {
    let Ok(Some(info)) = store.body_info(message).await else {
        return Vec::new();
    };
    if !info.complete || info.raw_size > MAX_BACKFILL_BODY_BYTES {
        return Vec::new();
    }
    let Ok(Some(raw)) = store.load_message(message).await else {
        return Vec::new();
    };
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
        .unwrap_or(raw.len());
    tokio::task::spawn_blocking(move || ClassifyHeaders::from_raw(&raw[..end]).reply_to)
        .await
        .unwrap_or_default()
}

/// Stage 17: the kinds of every row, once per run of the backfill (rows of stage 10 that it
/// never touches keep the kind they get here).
pub(crate) async fn recompute_kinds(store: &Store) -> Result<(), CoreError> {
    let mut after = 0;
    loop {
        let rows = store.contact_kind_rows(after, KIND_BATCH).await?;
        let Some(last) = rows.last() else {
            return Ok(());
        };
        after = last.0;
        let changed: Vec<(i64, i64)> = rows
            .iter()
            .filter_map(|(row, key, personal, sent, kind)| {
                let new = mail_content::contact_kind(key, *personal, *sent).code();
                (new != *kind).then_some((*row, new))
            })
            .collect();
        if !changed.is_empty() {
            store.set_contact_kinds(changed).await?;
        }
        tokio::task::yield_now().await;
    }
}

/// One address of a message as the directory job adds it: address, name, direction, personal.
type Observed = (String, Option<String>, Direction, bool);

/// What a batch adds: the keys of every address, and per message its date and observations.
/// Stage 17: of a message read before (`backfill`), only what stage 10 left out.
fn plan(
    sources: &[ContactSource],
    own: &[String],
) -> (BTreeSet<String>, Vec<(i64, Vec<Observed>)>) {
    let mut keys = BTreeSet::new();
    let mut per_message = Vec::with_capacity(sources.len());
    for source in sources {
        let headers = headers_of(source);
        let sent = source
            .from
            .as_ref()
            .is_some_and(|from| is_own(&from.email, own));
        let (direction, category) = if sent {
            (Direction::Sent, MessageCategory::Person)
        } else {
            (Direction::Received, category_of(source, own))
        };
        let observed: Vec<Observed> = observations(&headers, direction, category, source.date)
            .iter()
            .filter(|o| !source.backfill || !counted_before_stage17(o))
            .map(|o| {
                (
                    o.email.to_owned(),
                    o.name.map(str::to_owned),
                    o.direction,
                    o.personal,
                )
            })
            .collect();
        for (email, ..) in &observed {
            if let Some(address) = normalize_address(email) {
                keys.insert(address.key);
            }
        }
        per_message.push((source.date, observed));
    }
    (keys, per_message)
}

/// Reads one batch of messages into the directory: new messages first, then (stage 17) the
/// backfill of messages read by the rules of stage 10. Returns how many messages it read (0:
/// done) and whether they were the backfill's.
pub(crate) async fn run_batch(store: &Store, own: &[String]) -> Result<(usize, bool), CoreError> {
    let mut sources = store.contacts_todo(CONTACTS_BATCH).await?;
    if sources.is_empty() {
        sources = store.contacts_backfill(CONTACTS_BATCH).await?;
        for source in sources.iter_mut().filter(|s| s.reply_to.is_empty()) {
            source.reply_to = stored_reply_to(store, source.message).await;
        }
    }
    if sources.is_empty() {
        return Ok((0, false));
    }
    let backfill = sources.iter().any(|s| s.backfill);
    let own_owned = own.to_vec();
    let sources_for_work = sources.clone();
    // Classification and folding are CPU work.
    let planned = tokio::task::spawn_blocking(move || plan(&sources_for_work, &own_owned))
        .await
        .map_err(|err| CoreError::Internal(format!("contacts task failed: {err}")))?;
    let (keys, per_message) = planned;
    let stored = store.contacts_by_keys(keys.into_iter().collect()).await?;
    let groups: HashMap<String, Option<i64>> =
        stored.iter().map(|c| (c.key.clone(), c.group)).collect();
    let own_owned = own.to_vec();
    let rows = tokio::task::spawn_blocking(move || {
        let mut book =
            ContactBook::from_contacts(&own_owned, stored.iter().map(to_contact).collect());
        let mut touched = BTreeSet::new();
        for (at, observed) in &per_message {
            for (email, name, direction, personal) in observed {
                let observation = mail_content::Observation {
                    email,
                    name: name.as_deref(),
                    direction: *direction,
                    at: *at,
                    personal: *personal,
                };
                if book.observe(observation).is_ok()
                    && let Some(address) = normalize_address(email)
                {
                    touched.insert(address.key);
                }
            }
        }
        let mut rows = Vec::new();
        for contact in book.contacts() {
            if contact
                .addresses
                .iter()
                .any(|a| touched.contains(&a.address.key))
            {
                rows.extend(to_rows(contact, &groups));
            }
        }
        rows
    })
    .await
    .map_err(|err| CoreError::Internal(format!("contacts task failed: {err}")))?;
    let seen = sources.iter().map(|s| s.message).collect();
    store.record_contacts(rows, seen).await?;
    Ok((sources.len(), backfill))
}

fn query_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(fold)
        .filter(|w| !w.is_empty())
        .take(mail_store::MAX_SEARCH_WORDS)
        .collect()
}

/// Autocomplete: contacts whose names or address words start with every word of `query`.
/// A merged contact comes once, with its preferred address and its name (stage 10).
pub(crate) async fn search(
    store: &Store,
    query: &str,
    limit: u32,
) -> Result<Vec<ContactSuggestion>, CoreError> {
    let words = query_words(query);
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let found = store.search_contacts(words, limit).await?;
    let mut groups_seen: BTreeSet<i64> = BTreeSet::new();
    let mut out = Vec::with_capacity(found.len());
    for stored in &found {
        // Stage 17: list and system addresses the user never wrote to are no suggestions.
        if is_no_reply_address(&stored.key)
            || (stored.kind != ContactKind::Person.code() && stored.sent == 0)
        {
            continue;
        }
        let Some(group) = stored.group else {
            let contact = to_contact(stored);
            out.push(ContactSuggestion {
                name: contact.name().map(str::to_owned),
                email: email_of(stored),
                sent: stored.sent,
            });
            continue;
        };
        if !groups_seen.insert(group) {
            continue;
        }
        let Some(merged) = store.contact_group(stored.key.clone()).await? else {
            continue;
        };
        let entry = entry_of(&merged, &Extras::default());
        out.push(ContactSuggestion {
            name: entry.name,
            email: entry.email,
            sent: entry.sent,
        });
    }
    Ok(out)
}

// -- the directory (stage 10, decision 0023) ---------------------------------------------------

/// The address to write to: the local part as last written, the Unicode domain.
pub(crate) fn email_of(stored: &StoredContact) -> String {
    format!("{}@{}", stored.local_part, unicode_domain(&stored.domain))
}

/// The rows of a merged contact as one `Contact`: every address (the head's first), the seen
/// names together, the head's name (else a merged address's).
fn group_contact(group: &StoredContactGroup) -> Contact {
    let mut members = group.members.iter().map(to_contact);
    let mut contact = members.next().unwrap_or_else(|| to_contact(group.head()));
    for other in members {
        contact.addresses.extend(other.addresses);
        if contact.user_name.is_none() {
            contact.user_name = other.user_name;
        }
        for seen in other.names {
            match contact.names.iter_mut().find(|n| n.name == seen.name) {
                Some(known) => {
                    known.count = known.count.saturating_add(seen.count);
                    known.last_seen = known.last_seen.max(seen.last_seen);
                }
                None => contact.names.push(seen),
            }
        }
    }
    contact
}

fn ms(seconds: Option<i64>) -> Option<i64> {
    seconds.map(|s| s.saturating_mul(1000))
}

/// Stage 17: what an entry shows besides its rows: the pictures (address key → revision) and
/// the blocked addresses.
#[derive(Debug, Default)]
pub(crate) struct Extras {
    pub(crate) pictures: HashMap<String, i64>,
    pub(crate) blocked: HashSet<String>,
}

impl Extras {
    pub(crate) async fn load(store: &Store) -> Result<Self, CoreError> {
        Ok(Self {
            pictures: store.contact_pictures().await?.into_iter().collect(),
            blocked: store
                .blocked_senders()
                .await?
                .into_iter()
                .map(|b| b.address)
                .collect(),
        })
    }
}

pub(crate) fn entry_of(group: &StoredContactGroup, extras: &Extras) -> ContactEntry {
    let contact = group_contact(group);
    let addresses: Vec<ContactAddressInfo> = group
        .members
        .iter()
        .map(|m| ContactAddressInfo {
            email: email_of(m),
            sent: m.sent,
            received: m.received,
            seen: m.seen,
            last_used: ms([m.last_sent, m.last_received, m.last_seen]
                .into_iter()
                .flatten()
                .max()),
            blocked: extras.blocked.contains(&m.key),
        })
        .collect();
    let head = group.head();
    ContactEntry {
        key: head.key.clone(),
        name: contact.name().map(str::to_owned),
        user_name: contact.user_name.clone(),
        email: email_of(head),
        sent: contact.sent(),
        received: contact.addresses.iter().map(|a| a.received).sum(),
        last_used: ms(contact.last_used()),
        kind: ContactKind::from_code(head.kind).unwrap_or(ContactKind::Person),
        note: group.members.iter().find_map(|m| m.note.clone()),
        picture: group
            .members
            .iter()
            .find_map(|m| extras.pictures.get(&m.key).copied()),
        blocked: addresses.iter().any(|a| a.blocked),
        addresses,
    }
}

/// Stage 17: the duplicate check looks at people only (a newsletter and a shop's receipts
/// with the same name are not one person).
fn is_person(group: &StoredContactGroup) -> bool {
    group.head().kind == ContactKind::Person.code()
}

fn key_of(email: &str) -> Result<String, CoreError> {
    normalize_address(email)
        .map(|a| a.key)
        .ok_or_else(|| CoreError::Contact(ContactRequestError::NotFound(email.to_owned())))
}

async fn group_by_email(store: &Store, email: &str) -> Result<StoredContactGroup, CoreError> {
    store
        .contact_group(key_of(email)?)
        .await?
        .ok_or_else(|| CoreError::Contact(ContactRequestError::NotFound(email.to_owned())))
}

/// One page of the directory (see [`mail_model::ContactPage`]).
pub(crate) async fn list(
    store: &Store,
    query: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
    filter: ContactFilter,
) -> Result<ContactPage, CoreError> {
    let limit = limit.clamp(1, MAX_CONTACT_PAGE);
    let after = match cursor {
        None => None,
        Some(text) => Some(ContactCursor::decode(text).ok_or_else(|| {
            CoreError::InvalidArgument(format!("not a contact cursor: {text:?}"))
        })?),
    };
    let words = query.map(query_words).unwrap_or_default();
    let people_only = filter == ContactFilter::People;
    let page = store.contact_page(words, after, limit, people_only).await?;
    let extras = Extras::load(store).await?;
    Ok(ContactPage {
        items: page.groups.iter().map(|g| entry_of(g, &extras)).collect(),
        next: page.next.map(ContactCursor::encode),
    })
}

/// The contact with the address `email`.
pub(crate) async fn get(store: &Store, email: &str) -> Result<ContactEntry, CoreError> {
    let group = group_by_email(store, email).await?;
    Ok(entry_of(&group, &Extras::load(store).await?))
}

/// Stage 17: the contact (all its rows) with the address `email`.
pub(crate) async fn group_of(store: &Store, email: &str) -> Result<StoredContactGroup, CoreError> {
    group_by_email(store, email).await
}

/// Sets (or with `None` clears) the name the user gives the contact with the address `email`.
pub(crate) async fn set_name(
    store: &Store,
    email: &str,
    name: Option<&str>,
) -> Result<ContactEntry, CoreError> {
    let group = group_by_email(store, email).await?;
    let head = group.head();
    let mut book = ContactBook::from_contacts(&[], vec![to_contact(head)]);
    book.set_name(&email_of(head), name)
        .map_err(|err| match err {
            mail_content::ContactError::InvalidName => {
                CoreError::Contact(ContactRequestError::InvalidName)
            }
            other => CoreError::Contact(ContactRequestError::NotFound(other.to_string())),
        })?;
    let named = &book.contacts()[0];
    let stored = store
        .set_contact_name(
            head.key.clone(),
            named.user_name.clone(),
            search_text(named),
        )
        .await?;
    if !stored {
        return Err(CoreError::Contact(ContactRequestError::NotFound(
            email.to_owned(),
        )));
    }
    get(store, &head.key).await
}

/// Possible duplicates the user has not dismissed, the surest first.
pub(crate) async fn duplicates(store: &Store) -> Result<Vec<ContactDuplicate>, CoreError> {
    let own = own_addresses(store).await?;
    let groups: Vec<StoredContactGroup> = store
        .contact_groups(MAX_DUPLICATE_SCAN)
        .await?
        .into_iter()
        .filter(is_person)
        .collect();
    let dismissed = store.dismissed_contact_pairs().await?;
    let extras = Extras::load(store).await?;
    tokio::task::spawn_blocking(move || {
        let by_key: HashMap<String, &StoredContactGroup> =
            groups.iter().map(|g| (g.head().key.clone(), g)).collect();
        let book = ContactBook::from_contacts(&own, groups.iter().map(group_contact).collect());
        book.duplicates(&dismissed)
            .into_iter()
            .filter_map(|suggestion| {
                let contacts: Vec<ContactEntry> = suggestion
                    .contacts
                    .iter()
                    .filter_map(|key| by_key.get(key).map(|g| entry_of(g, &extras)))
                    .collect();
                if contacts.len() != suggestion.contacts.len() {
                    return None;
                }
                let (reason, name, names) = match suggestion.kind {
                    DuplicateKind::PlusAddress => (DuplicateReason::PlusAddress, None, Vec::new()),
                    DuplicateKind::SameMailbox => (DuplicateReason::SameMailbox, None, Vec::new()),
                    DuplicateKind::SameName { name } => {
                        (DuplicateReason::SameName, Some(name), Vec::new())
                    }
                    DuplicateKind::NamesDiffer { names } => {
                        (DuplicateReason::NamesDiffer, None, names)
                    }
                };
                Some(ContactDuplicate {
                    reason,
                    name,
                    names,
                    contacts,
                })
            })
            .collect()
    })
    .await
    .map_err(|err| CoreError::Internal(format!("contacts task failed: {err}")))
}

/// Hides a suggestion for good: two contact keys, or one ("different names" of one contact).
pub(crate) async fn dismiss(store: &Store, contacts: &[String]) -> Result<(), CoreError> {
    let (a, b) = match contacts {
        [one] => (key_of(one)?, key_of(one)?),
        [a, b] => (key_of(a)?, key_of(b)?),
        _ => {
            return Err(CoreError::InvalidArgument(
                "a duplicate suggestion has one or two contacts".into(),
            ));
        }
    };
    store.dismiss_contact_pair(a, b).await?;
    Ok(())
}

/// Merges contacts, only with the user's confirmation (`confirmed`); nothing changes without.
pub(crate) async fn merge(
    store: &Store,
    request: &ContactMergeRequest,
) -> Result<ContactEntry, CoreError> {
    if !request.confirmed {
        return Err(CoreError::Contact(ContactRequestError::NotConfirmed));
    }
    let keep = key_of(&request.keep)?;
    let mut absorb = Vec::new();
    for email in &request.merge {
        let key = key_of(email)?;
        if key != keep && !absorb.contains(&key) {
            absorb.push(key);
        }
    }
    if absorb.is_empty() {
        return Err(CoreError::Contact(ContactRequestError::SameContact));
    }
    match store.merge_contacts(keep, absorb).await? {
        MergeOutcome::Merged(head) => {
            tracing::info!("contacts merged");
            get(store, &head).await
        }
        MergeOutcome::NotFound(key) => Err(CoreError::Contact(ContactRequestError::NotFound(key))),
        MergeOutcome::SameContact => Err(CoreError::Contact(ContactRequestError::SameContact)),
    }
}

/// Takes the address `email` out of its merged contact (the preferred address takes the whole
/// contact apart). Returns the contact of `email` afterwards.
pub(crate) async fn separate(store: &Store, email: &str) -> Result<ContactEntry, CoreError> {
    let key = key_of(email)?;
    match store.separate_contact(key.clone()).await? {
        SeparateOutcome::Separated => get(store, &key).await,
        SeparateOutcome::NotFound => Err(CoreError::Contact(ContactRequestError::NotFound(
            email.to_owned(),
        ))),
        SeparateOutcome::NotMerged => Err(CoreError::Contact(ContactRequestError::NotMerged(
            email.to_owned(),
        ))),
    }
}

// -- commands ----------------------------------------------------------------------------------

impl<C: Connector> crate::AppCore<C> {
    /// One page of the contact directory, highest ranked first; `query` filters by word
    /// prefixes of names and addresses; `cursor` from the previous page.
    pub async fn list_contacts(
        &self,
        query: Option<String>,
        cursor: Option<String>,
        limit: u32,
        filter: ContactFilter,
    ) -> Result<ContactPage, CoreError> {
        let store = &self.compose_ref()?.inner.store;
        list(store, query.as_deref(), cursor.as_deref(), limit, filter).await
    }

    /// The contact with this address (any of its addresses).
    pub async fn get_contact(&self, email: String) -> Result<ContactEntry, CoreError> {
        get(&self.compose_ref()?.inner.store, &email).await
    }

    /// Sets or clears (`None`) the name the user gives a contact; synchronised.
    pub async fn set_contact_name(
        &self,
        email: String,
        name: Option<String>,
    ) -> Result<ContactEntry, CoreError> {
        set_name(&self.compose_ref()?.inner.store, &email, name.as_deref()).await
    }

    /// Possible duplicates, for the user to merge (with confirmation) or dismiss.
    pub async fn contact_duplicates(&self) -> Result<Vec<ContactDuplicate>, CoreError> {
        duplicates(&self.compose_ref()?.inner.store).await
    }

    /// Hides a duplicate suggestion on this device for good.
    pub async fn dismiss_contact_duplicate(&self, contacts: Vec<String>) -> Result<(), CoreError> {
        dismiss(&self.compose_ref()?.inner.store, &contacts).await
    }

    /// Merges contacts the user confirmed as one person (`confirmation_required` otherwise).
    pub async fn merge_contacts(
        &self,
        request: ContactMergeRequest,
    ) -> Result<ContactEntry, CoreError> {
        merge(&self.compose_ref()?.inner.store, &request).await
    }

    /// Takes an address out of a merged contact.
    pub async fn separate_contact(&self, email: String) -> Result<ContactEntry, CoreError> {
        separate(&self.compose_ref()?.inner.store, &email).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use mail_model::ErrorCode;
    use tempfile::TempDir;

    use super::*;
    use crate::credentials::{Credentials, MemorySecretStore};
    use crate::sync::SyncConfig;
    use crate::sync::fake::{FakeConnector, FakeServer};
    use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

    const DAY: i64 = 86_400;
    /// "Now" of the tests: 23 November 2026.
    const NOW: i64 = 1_795_392_000;

    struct Seen<'a> {
        email: &'a str,
        name: Option<&'a str>,
        direction: Direction,
        days_ago: i64,
    }

    fn sent<'a>(email: &'a str, name: &'a str, days_ago: i64) -> Seen<'a> {
        Seen {
            email,
            name: Some(name),
            direction: Direction::Sent,
            days_ago,
        }
    }

    fn received<'a>(email: &'a str, name: &'a str, days_ago: i64) -> Seen<'a> {
        Seen {
            email,
            name: Some(name),
            direction: Direction::Received,
            days_ago,
        }
    }

    /// The directory job's path: observations into a book, rows into the store.
    async fn observe(store: &Store, seen: &[Seen<'_>]) {
        let mut book = ContactBook::new(&["me@example.org".to_owned()]);
        for s in seen {
            let _ = book.observe(mail_content::Observation {
                email: s.email,
                name: s.name,
                direction: s.direction,
                at: NOW - s.days_ago * DAY,
                personal: true,
            });
        }
        let rows: Vec<StoredContact> = book
            .contacts()
            .iter()
            .flat_map(|c| to_rows(c, &HashMap::new()))
            .collect();
        store
            .record_contacts(rows, Vec::new())
            .await
            .expect("record");
    }

    async fn store() -> (TempDir, Store) {
        let dir = TempDir::new().expect("dir");
        let store = Store::open(
            &dir.path().join("mail.db"),
            mail_store::StoreOptions::default(),
        )
        .expect("open");
        (dir, store)
    }

    fn emails(page: &ContactPage) -> Vec<&str> {
        page.items.iter().map(|c| c.email.as_str()).collect()
    }

    #[tokio::test]
    async fn the_directory_ranks_by_frequency_and_recency_without_no_reply_addresses() {
        let (_dir, store) = store().await;
        observe(
            &store,
            &[
                // Three received this week: 3 × 0.25.
                received("carol@x.example", "Carol Clark", 1),
                received("carol@x.example", "Carol Clark", 2),
                received("carol@x.example", "Carol Clark", 3),
                // One sent today: 1, above Carol.
                sent("dave@x.example", "Dave", 0),
                // Ten sent a year ago: far below.
                sent("erin@x.example", "Erin", 365),
                sent("erin@x.example", "Erin", 366),
                sent("erin@x.example", "Erin", 367),
                sent("erin@x.example", "Erin", 368),
                sent("erin@x.example", "Erin", 369),
                // Sent weekly for two months: the top.
                sent("frank@x.example", "Frank", 0),
                sent("frank@x.example", "Frank", 7),
                sent("frank@x.example", "Frank", 14),
                sent("frank@x.example", "Frank", 21),
                sent("frank@x.example", "Frank", 28),
                sent("frank@x.example", "Frank", 35),
                // Nobody reads these: never recorded.
                sent("noreply@shop.example", "Shop", 0),
                received("no-reply@bank.example", "Bank", 0),
            ],
        )
        .await;
        // A row stored before a pattern was known is not listed either.
        let mut old = to_rows(
            &{
                let mut book = ContactBook::new(&[]);
                book.observe(mail_content::Observation {
                    email: "info@x.example",
                    name: None,
                    direction: Direction::Sent,
                    at: NOW,
                    personal: true,
                })
                .expect("observe");
                book.into_contacts().remove(0)
            },
            &HashMap::new(),
        );
        old[0].key = "donotreply@shop.example".into();
        old[0].local_part = "donotreply".into();
        old[0].domain = "shop.example".into();
        old[0].search = " donotreply@shop.example donotreply".into();
        store
            .record_contacts(old, Vec::new())
            .await
            .expect("record");
        // Stage 17: the backfill recomputes the kinds of such rows.
        recompute_kinds(&store).await.expect("kinds");

        let page = list(&store, None, None, 50, ContactFilter::People)
            .await
            .expect("list");
        assert_eq!(
            emails(&page),
            [
                "frank@x.example",
                "dave@x.example",
                "carol@x.example",
                "erin@x.example"
            ]
        );
        let carol = &page.items[2];
        assert_eq!((carol.sent, carol.received), (0, 3));
        assert_eq!(carol.name.as_deref(), Some("Carol Clark"));
        assert_eq!(carol.last_used, Some((NOW - DAY) * 1000));
        // Paging continues where the last page ended.
        let first = list(&store, None, None, 2, ContactFilter::People)
            .await
            .expect("page");
        assert_eq!(emails(&first), ["frank@x.example", "dave@x.example"]);
        let rest = list(
            &store,
            None,
            first.next.as_deref(),
            50,
            ContactFilter::People,
        )
        .await
        .expect("page");
        assert_eq!(emails(&rest), ["carol@x.example", "erin@x.example"]);
        assert!(rest.next.is_none());
        // Stage 17: addresses nobody reads are contacts too, marked, and listed on request.
        let all = list(&store, None, None, 50, ContactFilter::All)
            .await
            .expect("all");
        let system: Vec<&str> = all
            .items
            .iter()
            .filter(|c| c.kind == ContactKind::System)
            .map(|c| c.email.as_str())
            .collect();
        assert_eq!(system.len(), 3, "{system:?}");
        assert!(system.contains(&"donotreply@shop.example"));
        // Search and autocomplete leave out no-reply addresses as well.
        let found = list(&store, Some("d"), None, 50, ContactFilter::People)
            .await
            .expect("search");
        assert_eq!(emails(&found), ["dave@x.example"]);
        let suggestions = search(&store, "d", 10).await.expect("autocomplete");
        let suggested: Vec<_> = suggestions.iter().map(|s| s.email.as_str()).collect();
        assert_eq!(suggested, ["dave@x.example"]);
        let err = list(&store, None, Some("nonsense"), 10, ContactFilter::People)
            .await
            .expect_err("bad cursor");
        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[tokio::test]
    async fn duplicates_are_suggested_and_a_dismissed_pair_does_not_come_back() {
        let (_dir, store) = store().await;
        observe(
            &store,
            &[
                // Same name, similar address.
                sent("hans.mueller@x.example", "Hans Müller", 1),
                received("hans@y.example", "Müller, Hans", 2),
                // Plus address and base address.
                sent("anna+shop@z.example", "Anna", 3),
                sent("anna@z.example", "Anna Z", 4),
                // One address, different names.
                received("bob@w.example", "Bob Smith", 5),
                received("bob@w.example", "Robert Jones", 6),
            ],
        )
        .await;
        let found = duplicates(&store).await.expect("duplicates");
        let summary: Vec<(DuplicateReason, Vec<&str>)> = found
            .iter()
            .map(|d| {
                (
                    d.reason,
                    d.contacts.iter().map(|c| c.key.as_str()).collect(),
                )
            })
            .collect();
        assert!(
            summary.contains(&(
                DuplicateReason::PlusAddress,
                vec!["anna+shop@z.example", "anna@z.example"]
            )),
            "{summary:?}"
        );
        assert!(
            summary.contains(&(
                DuplicateReason::SameName,
                vec!["hans.mueller@x.example", "hans@y.example"]
            )),
            "{summary:?}"
        );
        let bob = found
            .iter()
            .find(|d| d.reason == DuplicateReason::NamesDiffer)
            .expect("names differ");
        assert_eq!(bob.contacts.len(), 1);
        assert_eq!(bob.names.len(), 2, "{:?}", bob.names);
        let same_name = found
            .iter()
            .find(|d| d.reason == DuplicateReason::SameName)
            .expect("same name");
        assert!(same_name.name.is_some());

        // Nothing is merged by suggesting.
        let page = list(&store, None, None, 50, ContactFilter::People)
            .await
            .expect("list");
        assert_eq!(page.items.len(), 5);

        dismiss(
            &store,
            &[
                "hans@y.example".to_owned(),
                "hans.mueller@x.example".to_owned(),
            ],
        )
        .await
        .expect("dismiss");
        dismiss(&store, &["bob@w.example".to_owned()])
            .await
            .expect("dismiss names");
        let again = duplicates(&store).await.expect("duplicates");
        assert!(
            again.iter().all(|d| d.reason != DuplicateReason::SameName
                && d.reason != DuplicateReason::NamesDiffer),
            "{again:?}"
        );
        assert_eq!(again.len(), 1, "the plus address stays: {again:?}");
        let err = dismiss(&store, &[]).await.expect_err("no contact");
        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[tokio::test]
    async fn contacts_are_never_merged_without_confirmation() {
        let (_dir, store) = store().await;
        observe(
            &store,
            &[
                sent("hans.mueller@x.example", "Hans Müller", 1),
                sent("hans.mueller@x.example", "Hans Müller", 2),
                received("hans@y.example", "Müller, Hans", 3),
            ],
        )
        .await;
        let before = list(&store, None, None, 50, ContactFilter::People)
            .await
            .expect("list");
        let request = ContactMergeRequest {
            keep: "Hans.Mueller@x.example".into(),
            merge: vec!["hans@y.example".into()],
            confirmed: false,
        };
        let err = merge(&store, &request).await.expect_err("not confirmed");
        assert_eq!(err.code(), ErrorCode::ConfirmationRequired);
        assert_eq!(
            list(&store, None, None, 50, ContactFilter::People)
                .await
                .expect("list"),
            before,
            "nothing changed"
        );
        assert_eq!(search(&store, "hans", 10).await.expect("search").len(), 2);

        let merged = merge(
            &store,
            &ContactMergeRequest {
                confirmed: true,
                ..request
            },
        )
        .await
        .expect("merge");
        assert_eq!(merged.key, "hans.mueller@x.example");
        assert_eq!(merged.addresses.len(), 2);
        assert_eq!((merged.sent, merged.received), (2, 1));
        let after = list(&store, None, None, 50, ContactFilter::People)
            .await
            .expect("list");
        assert_eq!(emails(&after), ["hans.mueller@x.example"], "listed once");
        let suggestions = search(&store, "hans", 10).await.expect("search");
        assert_eq!(suggestions.len(), 1, "autocomplete shows the contact once");
        assert_eq!(suggestions[0].email, "hans.mueller@x.example");
        // Searching by the merged address also gives the preferred one.
        let by_other = search(&store, "hans@y", 10).await.expect("search");
        assert_eq!(by_other.len(), 1);
        assert_eq!(by_other[0].email, "hans.mueller@x.example");
        // No duplicate suggestion for what is one contact now.
        assert!(
            duplicates(&store)
                .await
                .expect("duplicates")
                .iter()
                .all(|d| d.reason != DuplicateReason::SameName)
        );

        // Merging what is one contact, or an unknown address, is refused.
        let err = merge(
            &store,
            &ContactMergeRequest {
                keep: "hans@y.example".into(),
                merge: vec!["hans.mueller@x.example".into()],
                confirmed: true,
            },
        )
        .await
        .expect_err("same");
        assert_eq!(err.code(), ErrorCode::InvalidArgument);
        let err = merge(
            &store,
            &ContactMergeRequest {
                keep: "hans@y.example".into(),
                merge: vec!["nobody@y.example".into()],
                confirmed: true,
            },
        )
        .await
        .expect_err("unknown");
        assert_eq!(err.code(), ErrorCode::NotFound);

        // Separated again.
        let alone = separate(&store, "hans@y.example").await.expect("separate");
        assert_eq!(alone.addresses.len(), 1);
        assert_eq!(
            list(&store, None, None, 50, ContactFilter::People)
                .await
                .expect("list")
                .items
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn names_can_be_edited_and_cleared() {
        let (_dir, store) = store().await;
        observe(&store, &[received("bob@w.example", "Bob Smith", 1)]).await;
        let named = set_name(&store, "Bob@W.example", Some("  Robert   Smith "))
            .await
            .expect("name");
        assert_eq!(named.name.as_deref(), Some("Robert Smith"));
        assert_eq!(named.user_name.as_deref(), Some("Robert Smith"));
        // The new name is found by autocomplete.
        let found = search(&store, "rob", 10).await.expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_deref(), Some("Robert Smith"));
        for bad in ["", "bob@w.example"] {
            let err = set_name(&store, "bob@w.example", Some(bad))
                .await
                .expect_err("bad name");
            assert_eq!(err.code(), ErrorCode::InvalidArgument, "{bad:?}");
        }
        let cleared = set_name(&store, "bob@w.example", None)
            .await
            .expect("clear");
        assert_eq!(cleared.user_name, None);
        assert_eq!(cleared.name.as_deref(), Some("Bob Smith"));
        let err = set_name(&store, "nobody@w.example", Some("X"))
            .await
            .expect_err("unknown");
        assert_eq!(err.code(), ErrorCode::NotFound);
    }

    #[test]
    fn contact_records_hold_a_name_or_a_merge() {
        use crate::settings_sync::{RecordKind, SyncContent};
        use serde_json::json;
        for ok in [
            json!({ "userName": "A" }),
            json!({ "mergedInto": "a@b" }),
            json!({ "userName": "A", "mergedInto": "a@b" }),
        ] {
            assert!(
                SyncContent::new(RecordKind::Contact, ok.clone()).is_ok(),
                "{ok}"
            );
        }
        for bad in [json!({}), json!({ "group": 3 }), json!({ "userName": 1 })] {
            assert!(
                SyncContent::new(RecordKind::Contact, bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn a_confirmed_merge_and_a_name_are_captured_for_the_settings_sync() {
        let dir = TempDir::new().expect("dir");
        let mut services = CoreServices::with_connector(
            Credentials::new(Arc::new(MemorySecretStore::new())),
            FakeConnector(FakeServer::new()),
        );
        services.sync = SyncConfig {
            poll_interval: Duration::from_secs(600),
            settings_sync_auto: false,
            ..SyncConfig::default()
        };
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
        .await;
        let store = core.store_for_tests().clone();
        observe(
            &store,
            &[
                sent("hans.mueller@x.example", "Hans Müller", 1),
                received("hans@y.example", "Müller, Hans", 2),
            ],
        )
        .await;
        let refused = core
            .merge_contacts(ContactMergeRequest {
                keep: "hans.mueller@x.example".into(),
                merge: vec!["hans@y.example".into()],
                confirmed: false,
            })
            .await
            .expect_err("not confirmed");
        assert_eq!(refused.code(), ErrorCode::ConfirmationRequired);
        let contact_records = |rows: Vec<mail_store::SyncRecordRow>| {
            rows.into_iter()
                .filter(|r| r.kind == "contact")
                .map(|r| (r.id, r.content))
                .collect::<Vec<_>>()
        };
        assert!(
            contact_records(store.sync_records().await.expect("rows")).is_empty(),
            "nothing captured for a refused merge"
        );
        core.merge_contacts(ContactMergeRequest {
            keep: "hans.mueller@x.example".into(),
            merge: vec!["hans@y.example".into()],
            confirmed: true,
        })
        .await
        .expect("merge");
        core.set_contact_name("hans@y.example".into(), Some("Hans M.".into()))
            .await
            .expect("name");
        assert_eq!(
            contact_records(store.sync_records().await.expect("rows")),
            [
                (
                    "hans.mueller@x.example".to_owned(),
                    Some(serde_json::json!({ "userName": "Hans M." }))
                ),
                (
                    "hans@y.example".to_owned(),
                    Some(serde_json::json!({ "mergedInto": "hans.mueller@x.example" }))
                ),
            ]
        );
        let page = core
            .list_contacts(None, None, 10, ContactFilter::People)
            .await
            .expect("list");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name.as_deref(), Some("Hans M."));
        core.shutdown().await;
    }

    fn source(
        message: i64,
        from: &str,
        to: &[&str],
        cc: &[&str],
        reply_to: &[&str],
        extra: &[(&str, &str)],
    ) -> ContactSource {
        let address = |email: &str| mail_model::Address {
            name: Some(email.split('@').next().unwrap_or_default().to_uppercase()),
            email: email.to_owned(),
        };
        let header = |name: &str| {
            extra
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        };
        ContactSource {
            message: mail_model::MessageId(message),
            account: "00000000-0000-4000-8000-000000000001".parse().expect("id"),
            from: Some(address(from)),
            to: to.iter().map(|e| address(e)).collect(),
            cc: cc.iter().map(|e| address(e)).collect(),
            reply_to: reply_to.iter().map(|e| address(e)).collect(),
            backfill: false,
            subject: format!("Message {message}"),
            list_id: header("List-Id"),
            list_unsubscribe: header("List-Unsubscribe"),
            auto_submitted: header("Auto-Submitted"),
            precedence: header("Precedence"),
            date: NOW - message * DAY,
        }
    }

    fn observe_all(book: &mut ContactBook, planned: &[(i64, Vec<Observed>)]) {
        for (at, observed) in planned {
            for (email, name, direction, personal) in observed {
                let _ = book.observe(mail_content::Observation {
                    email,
                    name: name.as_deref(),
                    direction: *direction,
                    at: *at,
                    personal: *personal,
                });
            }
        }
    }

    /// The contacts of a book by key: counts, dates, kind and the rank (rounded: the same
    /// weights added in another order).
    /// Key, sent, received, seen, last used, kind, rank.
    type Summary = (String, u32, u32, u32, Option<i64>, i64, String);

    fn summary(book: &ContactBook) -> Vec<Summary> {
        let mut out: Vec<_> = book
            .contacts()
            .iter()
            .map(|c| {
                let a = c.primary();
                (
                    a.address.key.clone(),
                    a.sent,
                    a.received,
                    a.seen,
                    c.last_used(),
                    a.kind().code(),
                    format!("{:.9}", a.rank.key()),
                )
            })
            .collect();
        out.sort();
        out
    }

    /// Stage 17: every address of From, To, Cc and Reply-To; a message read by the rules of
    /// stage 10 adds only what those left out, so stage 10 plus the backfill is what reading
    /// the messages now gives.
    #[test]
    fn the_backfill_adds_what_stage_10_left_out() {
        let own = vec!["me@example.org".to_owned()];
        let sources = vec![
            // A person writes to the user and others, with a Reply-To.
            source(
                1,
                "carol@x.example",
                &["me@example.org", "dave@x.example"],
                &["erin@x.example"],
                &["team@x.example"],
                &[],
            ),
            // The user's own mail.
            source(
                2,
                "me@example.org",
                &["frank@y.example"],
                &["dave@x.example"],
                &[],
                &[],
            ),
            // A newsletter.
            source(
                3,
                "news@shop.example",
                &["me@example.org"],
                &[],
                &["help@shop.example"],
                &[
                    ("List-Id", "<weekly.shop.example>"),
                    ("List-Unsubscribe", "<https://shop.example/u>"),
                    ("Precedence", "bulk"),
                ],
            ),
            // A bounce.
            source(
                4,
                "MAILER-DAEMON@x.example",
                &["me@example.org"],
                &[],
                &[],
                &[("Auto-Submitted", "auto-replied")],
            ),
        ];
        let (keys, fresh) = plan(&sources, &own);
        assert_eq!(
            keys.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "carol@x.example",
                "dave@x.example",
                "erin@x.example",
                "frank@y.example",
                "help@shop.example",
                "mailer-daemon@x.example",
                "me@example.org",
                "news@shop.example",
                "team@x.example",
            ]
        );
        let mut now = ContactBook::new(&own);
        observe_all(&mut now, &fresh);

        // Stage 10 read the same messages: the sender of the person's mail and the
        // recipients of the user's own.
        let stage10: Vec<(i64, Vec<Observed>)> = fresh
            .iter()
            .map(|(at, observed)| {
                let kept = observed
                    .iter()
                    .filter(|(email, name, direction, personal)| {
                        counted_before_stage17(&mail_content::Observation {
                            email,
                            name: name.as_deref(),
                            direction: *direction,
                            at: *at,
                            personal: *personal,
                        })
                    })
                    .cloned()
                    .collect();
                (*at, kept)
            })
            .collect();
        let mut before = ContactBook::new(&own);
        observe_all(&mut before, &stage10);
        assert_eq!(before.contacts().len(), 3, "carol, dave and frank");
        let backfill: Vec<ContactSource> = sources
            .iter()
            .cloned()
            .map(|s| ContactSource {
                backfill: true,
                ..s
            })
            .collect();
        let (_, delta) = plan(&backfill, &own);
        observe_all(&mut before, &delta);
        assert_eq!(summary(&before), summary(&now));

        let kinds: HashMap<String, i64> = summary(&now)
            .into_iter()
            .map(|(key, .., kind, _)| (key, kind))
            .collect();
        assert_eq!(kinds["news@shop.example"], ContactKind::List.code());
        assert_eq!(kinds["help@shop.example"], ContactKind::List.code());
        assert_eq!(kinds["mailer-daemon@x.example"], ContactKind::System.code());
        assert_eq!(kinds["erin@x.example"], ContactKind::Person.code());
        // Only the person's sender and the user's recipients rank.
        let erin = now.find("erin@x.example").expect("erin");
        assert_eq!((erin.primary().seen, erin.rank()), (1, Rank::NONE));
        let dave = now.find("dave@x.example").expect("dave");
        assert_eq!((dave.primary().sent, dave.primary().seen), (1, 1));
    }

    #[test]
    fn rows_round_trip_through_the_book() {
        let own = vec!["me@example.org".to_owned()];
        let mut book = ContactBook::new(&own);
        book.observe(mail_content::Observation {
            email: "Hans.Mueller@Müller.example",
            name: Some("Hans Müller"),
            direction: Direction::Sent,
            at: 1_764_000_000,
            personal: true,
        })
        .expect("observe");
        let rows = to_rows(&book.contacts()[0], &HashMap::new());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.key, "hans.mueller@xn--mller-kva.example");
        assert_eq!(row.local_part, "Hans.Mueller");
        assert_eq!(row.sent, 1);
        assert!(row.search.starts_with(' '));
        assert!(row.search.contains(" hans"), "{}", row.search);
        let back = to_contact(row);
        assert_eq!(back.name(), Some("Hans Müller"));
        assert_eq!(back.primary().rank, book.contacts()[0].primary().rank);
        assert!(is_own("me+lists@example.org", &own));
        assert!(!is_own("you@example.org", &own));
    }
}
