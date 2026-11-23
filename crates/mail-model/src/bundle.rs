//! Bundle rows of the inbox (stage 9, `docs/DESIGN.md`, decision 0016): notifications and
//! newsletters of one date section become one row with a counter and chips of the most frequent
//! senders. Pure logic over one page of a keyset list.
//!
//! - A bundle is keyed by category and date section ([`BundleKey`]); the section number comes
//!   from the caller ([`BundleItem::section`]: "today", "yesterday", "this week", a month, in the
//!   user's time zone), so this module needs no clock or time zone.
//! - A bundle row stands where its newest member is: the list is newest first, so that is the
//!   position of its first member.
//! - Keyset paging: [`BundleState`] carries the bundles already started to the next page. Items of
//!   such a bundle come back as [`BundleRow::More`], to be merged into the row of the earlier
//!   page; the summary then covers every member so far. Merging the pages gives exactly the rows
//!   of one pass over the whole list, wherever the pages are cut (tested).
//! - Deterministic: the output depends only on the items, their order and the options.
//! - A bundle can have a single member. The UI may draw it like a normal row; it must stay a
//!   bundle because later pages can add members.
//! - Items that must stay visible ([`BundleItem::keep_single`]: important senders, pinned) and
//!   people's messages are never bundled.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{Address, MessageCategory};

/// Chips shown on a bundle row by default.
pub const DEFAULT_BUNDLE_CHIPS: usize = 3;
/// Distinct senders counted per bundle; further senders only count towards the total.
pub const MAX_BUNDLE_SENDERS: usize = 256;

/// What the bundler needs to know about a list row.
pub trait BundleItem {
    fn category(&self) -> MessageCategory;
    /// Never bundled: an important sender (Priority block), a pinned message.
    fn keep_single(&self) -> bool;
    /// First `From` address.
    fn sender(&self) -> Option<&Address>;
    fn unread(&self) -> bool;
    /// Date section of the row; rows of one section are contiguous in the list.
    fn section(&self) -> i64;
}

/// Which categories are bundled. [`BundleOptions::off`] when the user turned categories off:
/// the inbox is then purely chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleOptions {
    pub notifications: bool,
    pub newsletters: bool,
    /// Chips per bundle row.
    pub max_chips: usize,
}

impl Default for BundleOptions {
    fn default() -> Self {
        Self {
            notifications: true,
            newsletters: true,
            max_chips: DEFAULT_BUNDLE_CHIPS,
        }
    }
}

impl BundleOptions {
    pub const fn off() -> Self {
        Self {
            notifications: false,
            newsletters: false,
            max_chips: DEFAULT_BUNDLE_CHIPS,
        }
    }

    fn bundles(&self, category: MessageCategory) -> bool {
        match category {
            MessageCategory::Person => false,
            MessageCategory::Notification => self.notifications,
            MessageCategory::Newsletter => self.newsletters,
        }
    }
}

/// One bundle: a category within a date section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BundleKey {
    pub category: MessageCategory,
    #[specta(type = Number)]
    pub section: i64,
}

/// A sender chip on a bundle row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SenderChip {
    /// The address as first seen (newest message), with the first display name seen.
    pub sender: Address,
    pub count: u32,
    pub unread: u32,
}

/// Counter and chips of a bundle, over every member seen so far.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BundleSummary {
    pub key: BundleKey,
    pub count: u32,
    pub unread: u32,
    /// Most frequent senders first; equal counts: the one with the newer message first.
    pub senders: Vec<SenderChip>,
}

/// A bundle's members on this page (newest first) and its summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle<T> {
    pub summary: BundleSummary,
    pub members: Vec<T>,
}

/// One row of a bundled page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleRow<T> {
    Message(T),
    /// A bundle that starts on this page, at the position of its newest member.
    Bundle(Bundle<T>),
    /// More members of a bundle an earlier page started: append them to that row and replace
    /// its summary.
    More(Bundle<T>),
}

/// Bundles started on earlier pages. Opaque; serialisable so it can travel with a cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleState {
    tallies: Vec<Tally>,
    /// Items seen so far, to order senders by their newest message.
    seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Tally {
    key: BundleKey,
    count: u32,
    unread: u32,
    senders: Vec<SenderTally>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SenderTally {
    /// Lower-case address, the identity of the chip.
    id: String,
    sender: Address,
    count: u32,
    unread: u32,
    first_seen: u64,
}

impl Tally {
    fn add(&mut self, sender: Option<&Address>, unread: bool, seen: u64) {
        self.count = self.count.saturating_add(1);
        self.unread = self.unread.saturating_add(u32::from(unread));
        let Some(sender) = sender else {
            return;
        };
        let id = sender.email.trim().to_lowercase();
        if id.is_empty() {
            return;
        }
        let room = self.senders.len() < MAX_BUNDLE_SENDERS;
        match self.senders.iter_mut().find(|s| s.id == id) {
            Some(known) => {
                known.count = known.count.saturating_add(1);
                known.unread = known.unread.saturating_add(u32::from(unread));
                if known.sender.name.is_none() && sender.name.is_some() {
                    known.sender.name.clone_from(&sender.name);
                }
            }
            None if room => self.senders.push(SenderTally {
                id,
                sender: Address {
                    name: sender.name.clone(),
                    email: sender.email.trim().to_owned(),
                },
                count: 1,
                unread: u32::from(unread),
                first_seen: seen,
            }),
            None => {}
        }
    }

    fn summary(&self, max_chips: usize) -> BundleSummary {
        let mut senders: Vec<&SenderTally> = self.senders.iter().collect();
        senders.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then(a.first_seen.cmp(&b.first_seen))
                .then_with(|| a.id.cmp(&b.id))
        });
        BundleSummary {
            key: self.key,
            count: self.count,
            unread: self.unread,
            senders: senders
                .into_iter()
                .take(max_chips)
                .map(|s| SenderChip {
                    sender: s.sender.clone(),
                    count: s.count,
                    unread: s.unread,
                })
                .collect(),
        }
    }
}

impl BundleState {
    /// Forgets the bundles of every date section but `section` (stage 9). Sections are
    /// contiguous in the list, so once a page ends in `section`, earlier sections can get no
    /// more members; this keeps the state that travels with a cursor small.
    pub fn keep_section(&mut self, section: i64) {
        self.tallies.retain(|t| t.key.section == section);
    }

    /// No bundle is open.
    pub fn is_empty(&self) -> bool {
        self.tallies.is_empty()
    }
}

/// Bundles one page of a list (newest first); `state` comes from the previous page (default for
/// the first) and is updated for the next one.
pub fn bundle_page<T: BundleItem>(
    items: Vec<T>,
    state: &mut BundleState,
    options: &BundleOptions,
) -> Vec<BundleRow<T>> {
    let mut rows: Vec<BundleRow<T>> = Vec::with_capacity(items.len());
    // Row index of each bundle on this page.
    let mut on_page: HashMap<BundleKey, usize> = HashMap::new();
    for item in items {
        state.seen += 1;
        let category = item.category();
        if item.keep_single() || !options.bundles(category) {
            rows.push(BundleRow::Message(item));
            continue;
        }
        let key = BundleKey {
            category,
            section: item.section(),
        };
        let started_before = state.tallies.iter().position(|t| t.key == key);
        let tally_index = match started_before {
            Some(index) => index,
            None => {
                state.tallies.push(Tally {
                    key,
                    count: 0,
                    unread: 0,
                    senders: Vec::new(),
                });
                state.tallies.len() - 1
            }
        };
        let seen = state.seen;
        state.tallies[tally_index].add(item.sender(), item.unread(), seen);
        match on_page.get(&key) {
            Some(&row) => match &mut rows[row] {
                BundleRow::Bundle(bundle) | BundleRow::More(bundle) => bundle.members.push(item),
                BundleRow::Message(_) => {}
            },
            None => {
                on_page.insert(key, rows.len());
                let bundle = Bundle {
                    summary: placeholder_summary(key),
                    members: vec![item],
                };
                // A bundle with an earlier tally but no row yet on this page was started by an
                // earlier page.
                let earlier = started_before.is_some();
                rows.push(if earlier {
                    BundleRow::More(bundle)
                } else {
                    BundleRow::Bundle(bundle)
                });
            }
        }
    }
    for row in &mut rows {
        if let BundleRow::Bundle(bundle) | BundleRow::More(bundle) = row
            && let Some(tally) = state.tallies.iter().find(|t| t.key == bundle.summary.key)
        {
            bundle.summary = tally.summary(options.max_chips);
        }
    }
    rows
}

fn placeholder_summary(key: BundleKey) -> BundleSummary {
    BundleSummary {
        key,
        count: 0,
        unread: 0,
        senders: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Row {
        id: u32,
        category: MessageCategory,
        keep: bool,
        sender: Option<Address>,
        unread: bool,
        section: i64,
    }

    impl BundleItem for Row {
        fn category(&self) -> MessageCategory {
            self.category
        }
        fn keep_single(&self) -> bool {
            self.keep
        }
        fn sender(&self) -> Option<&Address> {
            self.sender.as_ref()
        }
        fn unread(&self) -> bool {
            self.unread
        }
        fn section(&self) -> i64 {
            self.section
        }
    }

    fn row(id: u32, category: MessageCategory, sender: &str, section: i64) -> Row {
        Row {
            id,
            category,
            keep: false,
            sender: Some(Address {
                name: None,
                email: sender.into(),
            }),
            unread: id.is_multiple_of(2),
            section,
        }
    }

    use MessageCategory::{Newsletter as N, Notification as B, Person as P};

    /// A list newest first: two sections, mixed categories, repeated senders.
    fn sample() -> Vec<Row> {
        let mut rows = vec![
            row(1, P, "carol@example.org", 2),
            row(2, B, "notifications@github.com", 2),
            row(3, B, "ci@ci.example", 2),
            row(4, N, "news@news.example", 2),
            row(5, B, "notifications@github.com", 2),
            row(6, P, "dave@example.net", 2),
            row(7, B, "Notifications@GitHub.com", 2),
            row(8, N, "info@verein.example", 2),
            row(9, B, "ci@ci.example", 1),
            row(10, P, "carol@example.org", 1),
            row(11, B, "orders@shop.example", 1),
            row(12, N, "news@news.example", 1),
            row(13, N, "news@news.example", 1),
        ];
        rows[6].sender = Some(Address {
            name: Some("GitHub".into()),
            email: "Notifications@GitHub.com".into(),
        });
        let mut important = row(14, B, "alerts@bank.example", 1);
        important.keep = true;
        rows.push(important);
        rows.push(row(15, B, "ci@ci.example", 1));
        rows
    }

    fn ids(rows: &[BundleRow<Row>]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                BundleRow::Message(m) => m.id.to_string(),
                BundleRow::Bundle(b) => format!(
                    "B{:?}{}[{}]",
                    b.summary.key.category,
                    b.summary.key.section,
                    b.members
                        .iter()
                        .map(|m| m.id.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                BundleRow::More(b) => format!("M{:?}", b.summary.key),
            })
            .collect()
    }

    #[test]
    fn bundles_stand_at_their_newest_member() {
        let mut state = BundleState::default();
        let rows = bundle_page(sample(), &mut state, &BundleOptions::default());
        assert_eq!(
            ids(&rows),
            [
                "1",
                "BNotification2[2,3,5,7]",
                "BNewsletter2[4,8]",
                "6",
                "BNotification1[9,11,15]",
                "10",
                "BNewsletter1[12,13]",
                "14",
            ]
        );
        let BundleRow::Bundle(github) = &rows[1] else {
            panic!("bundle expected");
        };
        assert_eq!(github.summary.count, 4);
        assert_eq!(github.summary.unread, 1, "only id 2 is unread");
        // Three messages from GitHub (case ignored), the name from the later message, the first
        // spelling of the address.
        assert_eq!(
            github.summary.senders[0],
            SenderChip {
                sender: Address {
                    name: Some("GitHub".into()),
                    email: "notifications@github.com".into()
                },
                count: 3,
                unread: 1,
            }
        );
        assert_eq!(github.summary.senders[1].sender.email, "ci@ci.example");
    }

    #[test]
    fn chips_are_ordered_by_count_then_recency_and_capped() {
        let rows: Vec<Row> = ["a@x", "b@x", "c@x", "d@x", "d@x", "c@x"]
            .iter()
            .enumerate()
            .map(|(i, s)| row(i as u32 + 1, N, s, 0))
            .collect();
        let mut state = BundleState::default();
        let out = bundle_page(rows, &mut state, &BundleOptions::default());
        let BundleRow::Bundle(bundle) = &out[0] else {
            panic!("bundle expected");
        };
        let chips: Vec<&str> = bundle
            .summary
            .senders
            .iter()
            .map(|c| c.sender.email.as_str())
            .collect();
        // c and d twice (c's newest message is newer), then a before b.
        assert_eq!(chips, ["c@x", "d@x", "a@x"]);
        assert_eq!(bundle.summary.count, 6);
    }

    #[test]
    fn categories_off_or_partly_off() {
        let mut state = BundleState::default();
        let rows = bundle_page(sample(), &mut state, &BundleOptions::off());
        assert!(rows.iter().all(|r| matches!(r, BundleRow::Message(_))));
        assert_eq!(rows.len(), 15);

        let only_news = BundleOptions {
            notifications: false,
            ..BundleOptions::default()
        };
        let rows = bundle_page(sample(), &mut BundleState::default(), &only_news);
        assert_eq!(
            rows.iter()
                .filter(|r| matches!(r, BundleRow::Bundle(_)))
                .count(),
            2
        );
    }

    /// Appends `More` rows to their bundle, as the UI does.
    fn merge(pages: Vec<Vec<BundleRow<Row>>>) -> Vec<BundleRow<Row>> {
        let mut out: Vec<BundleRow<Row>> = Vec::new();
        for page in pages {
            for row in page {
                match row {
                    BundleRow::More(more) => {
                        let target = out.iter_mut().find_map(|r| match r {
                            BundleRow::Bundle(b) if b.summary.key == more.summary.key => Some(b),
                            _ => None,
                        });
                        let target = target.expect("More without an earlier bundle");
                        target.members.extend(more.members);
                        target.summary = more.summary;
                    }
                    other => out.push(other),
                }
            }
        }
        out
    }

    #[test]
    fn pages_cut_anywhere_merge_to_the_single_pass() {
        let all = sample();
        let options = BundleOptions::default();
        let single = bundle_page(all.clone(), &mut BundleState::default(), &options);
        for size in 1..=all.len() {
            let mut state = BundleState::default();
            let pages: Vec<Vec<BundleRow<Row>>> = all
                .chunks(size)
                .map(|chunk| bundle_page(chunk.to_vec(), &mut state, &options))
                .collect();
            assert_eq!(merge(pages), single, "page size {size}");
        }
        // Uneven cuts, and the state survives serialisation (it may travel with a cursor).
        for cut in 0..all.len() {
            let mut state = BundleState::default();
            let first = bundle_page(all[..cut].to_vec(), &mut state, &options);
            let json = serde_json::to_string(&state).expect("json");
            let mut state: BundleState = serde_json::from_str(&json).expect("state");
            let second = bundle_page(all[cut..].to_vec(), &mut state, &options);
            assert_eq!(merge(vec![first, second]), single, "cut at {cut}");
        }
    }

    #[test]
    fn a_later_page_continues_a_bundle() {
        let all = sample();
        let mut state = BundleState::default();
        let options = BundleOptions::default();
        let _ = bundle_page(all[..4].to_vec(), &mut state, &options);
        let rest = bundle_page(all[4..].to_vec(), &mut state, &options);
        let BundleRow::More(more) = &rest[0] else {
            panic!("More expected: {:?}", ids(&rest));
        };
        assert_eq!(more.members.len(), 2, "ids 5 and 7");
        assert_eq!(more.summary.count, 4, "the summary counts page 1 too");
    }

    #[test]
    fn many_senders_are_counted_but_not_listed() {
        let rows: Vec<Row> = (0..MAX_BUNDLE_SENDERS as u32 + 10)
            .map(|i| row(i, B, &format!("s{i}@x"), 0))
            .collect();
        let out = bundle_page(rows, &mut BundleState::default(), &BundleOptions::default());
        let BundleRow::Bundle(bundle) = &out[0] else {
            panic!("bundle expected");
        };
        assert_eq!(bundle.summary.count as usize, MAX_BUNDLE_SENDERS + 10);
        assert_eq!(bundle.summary.senders.len(), DEFAULT_BUNDLE_CHIPS);
    }
}
