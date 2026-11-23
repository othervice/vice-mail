//! The contact directory for autocomplete (decision 0016), fed from stored header data:
//! messages the user sent (From is an own address: every To and Cc recipient) and messages
//! received from people (the sender, only for the category Person). A background job reads
//! the messages not read yet (`message.contacts_seen`), in batches, and the store keeps one row
//! per address. Blind copies are not in the stored headers and are not counted.

use std::collections::{BTreeSet, HashMap};

use mail_content::{
    ClassifyHeaders, Contact, ContactAddress, ContactBook, Direction, LocalSignals, MessageHeaders,
    NormalizedAddress, Rank, SeenName, classify_automatic, fold, normalize_address, observations,
    unicode_domain,
};
use mail_model::{
    ContactNameRecord, ContactSuggestion, CoreError, Flags, MessageCategory, MessageHeader,
};
use mail_store::{ContactSource, Store, StoredContact};

/// Messages read into the directory per transaction.
pub(crate) const CONTACTS_BATCH: u32 = 500;

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
            last_sent: stored.last_sent,
            last_received: stored.last_received,
            rank: Rank::from_key(stored.rank),
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

/// One row per address (merges come with stage 10; until then a contact has one address).
fn to_rows(contact: &Contact, groups: &HashMap<String, Option<i64>>) -> Vec<StoredContact> {
    let mut search = String::new();
    for word in contact.search_words() {
        search.push(' ');
        search.push_str(&word);
    }
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
        })
        .collect()
}

fn headers_of(source: &ContactSource) -> MessageHeaders {
    MessageHeaders {
        from: source.from.iter().cloned().collect(),
        to: source.to.clone(),
        cc: source.cc.clone(),
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

/// Reads one batch of messages into the directory. Returns how many messages it read (0: done).
pub(crate) async fn run_batch(store: &Store, own: &[String]) -> Result<usize, CoreError> {
    let sources = store.contacts_todo(CONTACTS_BATCH).await?;
    if sources.is_empty() {
        return Ok(0);
    }
    let own_owned = own.to_vec();
    let sources_for_work = sources.clone();
    // Classification and folding are CPU work.
    let planned = tokio::task::spawn_blocking(move || {
        let mut keys = BTreeSet::new();
        let mut per_message = Vec::with_capacity(sources_for_work.len());
        for source in &sources_for_work {
            let headers = headers_of(source);
            let sent = source
                .from
                .as_ref()
                .is_some_and(|from| is_own(&from.email, &own_owned));
            let (direction, category) = if sent {
                (Direction::Sent, MessageCategory::Person)
            } else {
                (Direction::Received, category_of(source, &own_owned))
            };
            let observed: Vec<(String, Option<String>)> =
                observations(&headers, direction, category, source.date)
                    .iter()
                    .map(|o| (o.email.to_owned(), o.name.map(str::to_owned)))
                    .collect();
            for (email, _) in &observed {
                if let Some(address) = normalize_address(email) {
                    keys.insert(address.key);
                }
            }
            per_message.push((direction, source.date, observed));
        }
        (keys, per_message)
    })
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
        for (direction, at, observed) in &per_message {
            for (email, name) in observed {
                let observation = mail_content::Observation {
                    email,
                    name: name.as_deref(),
                    direction: *direction,
                    at: *at,
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
    Ok(sources.len())
}

/// Autocomplete: contacts whose names or address words start with every word of `query`.
pub(crate) async fn search(
    store: &Store,
    query: &str,
    limit: u32,
) -> Result<Vec<ContactSuggestion>, CoreError> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(fold)
        .filter(|w| !w.is_empty())
        .take(mail_store::MAX_SEARCH_WORDS)
        .collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let found = store.search_contacts(words, limit).await?;
    Ok(found
        .iter()
        .map(|stored| {
            let contact = to_contact(stored);
            ContactSuggestion {
                name: contact.name().map(str::to_owned),
                email: format!("{}@{}", stored.local_part, unicode_domain(&stored.domain)),
                sent: stored.sent,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_round_trip_through_the_book() {
        let own = vec!["me@example.org".to_owned()];
        let mut book = ContactBook::new(&own);
        book.observe(mail_content::Observation {
            email: "Hans.Mueller@Müller.example",
            name: Some("Hans Müller"),
            direction: Direction::Sent,
            at: 1_764_000_000,
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
