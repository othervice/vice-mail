//! A set of IMAP UIDs, kept as sorted, disjoint, non-adjacent inclusive ranges.

use std::fmt;

use crate::Uid;

/// Set of UIDs. `Display` writes IMAP sequence-set syntax (`1:5,7,9:12`); the full range
/// [`UidSet::all`] is written as `1:*`. An empty set displays as nothing and must not be sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct UidSet {
    /// Sorted by start; gaps of at least one UID between ranges; `start <= end`; no UID 0.
    ranges: Vec<(Uid, Uid)>,
}

impl UidSet {
    pub const fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    /// Every possible UID (`1:*`).
    pub fn all() -> Self {
        Self::range(1, Uid::MAX)
    }

    /// `first..=last`. Empty if `last < first`; UID 0 is dropped.
    pub fn range(first: Uid, last: Uid) -> Self {
        let first = first.max(1);
        if last < first {
            Self::new()
        } else {
            Self {
                ranges: vec![(first, last)],
            }
        }
    }

    /// Builds a set from UIDs in any order, with duplicates. UID 0 is dropped.
    pub fn from_uids<I: IntoIterator<Item = Uid>>(uids: I) -> Self {
        let mut uids: Vec<Uid> = uids.into_iter().filter(|u| *u != 0).collect();
        uids.sort_unstable();
        uids.dedup();
        let mut set = Self::new();
        for uid in uids {
            set.push_ascending(uid, uid);
        }
        set
    }

    /// Builds a set from inclusive ranges in any order, overlapping or not.
    pub fn from_ranges<I: IntoIterator<Item = (Uid, Uid)>>(ranges: I) -> Self {
        let mut ranges: Vec<(Uid, Uid)> = ranges
            .into_iter()
            .map(|(a, b)| (a.min(b).max(1), a.max(b)))
            .filter(|(a, b)| a <= b)
            .collect();
        ranges.sort_unstable();
        let mut set = Self::new();
        for (a, b) in ranges {
            set.push_ascending(a, b);
        }
        set
    }

    /// Appends a range that starts at or after the start of the last range, merging overlaps.
    fn push_ascending(&mut self, first: Uid, last: Uid) {
        match self.ranges.last_mut() {
            Some((_, end)) if first <= end.saturating_add(1) => *end = (*end).max(last),
            _ => self.ranges.push((first, last)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Number of UIDs in the set.
    pub fn len(&self) -> u64 {
        self.ranges
            .iter()
            .map(|(a, b)| u64::from(*b) - u64::from(*a) + 1)
            .sum()
    }

    pub fn contains(&self, uid: Uid) -> bool {
        let idx = self.ranges.partition_point(|(_, end)| *end < uid);
        self.ranges
            .get(idx)
            .is_some_and(|(start, end)| *start <= uid && uid <= *end)
    }

    pub fn min(&self) -> Option<Uid> {
        self.ranges.first().map(|(a, _)| *a)
    }

    pub fn max(&self) -> Option<Uid> {
        self.ranges.last().map(|(_, b)| *b)
    }

    /// The ranges, ascending.
    pub fn ranges(&self) -> &[(Uid, Uid)] {
        &self.ranges
    }

    /// All UIDs, ascending. Do not call on [`UidSet::all`].
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Uid> + '_ {
        self.ranges.iter().flat_map(|(a, b)| *a..=*b)
    }

    /// Splits off the `n` highest UIDs: returns `(highest, rest)`. This is how the first sync
    /// walks from new to old: `let (window, rest) = remaining.split_highest(500);`.
    pub fn split_highest(&self, n: u64) -> (Self, Self) {
        let mut remaining = n;
        let mut high = Vec::new();
        let mut rest = self.ranges.clone();
        while remaining > 0 {
            let Some((a, b)) = rest.pop() else { break };
            let size = u64::from(b) - u64::from(a) + 1;
            if size <= remaining {
                high.push((a, b));
                remaining -= size;
            } else {
                // remaining < size <= 2^32, so the cut point fits into a UID.
                let cut = u32::try_from(u64::from(b) + 1 - remaining).unwrap_or(a);
                high.push((cut, b));
                rest.push((a, cut - 1));
                remaining = 0;
            }
        }
        high.reverse();
        (Self { ranges: high }, Self { ranges: rest })
    }

    /// UIDs in `self` but not in `other` (for example: stored locally, gone on the server).
    pub fn difference(&self, other: &Self) -> Self {
        let mut out = Self::new();
        let mut j = 0;
        for &(start, end) in &self.ranges {
            let mut cur = start;
            // Skip ranges of `other` that end before `cur`.
            while other.ranges.get(j).is_some_and(|(_, e)| *e < cur) {
                j += 1;
            }
            let mut k = j;
            let mut done = false;
            while let Some(&(os, oe)) = other.ranges.get(k) {
                if os > end {
                    break;
                }
                if os > cur {
                    out.push_ascending(cur, os - 1);
                }
                if oe >= end {
                    done = true;
                    break;
                }
                cur = oe + 1;
                k += 1;
            }
            if !done {
                out.push_ascending(cur, end);
            }
        }
        out
    }
}

impl fmt::Display for UidSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (a, b)) in self.ranges.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            match (a, b) {
                (a, b) if a == b => write!(f, "{a}")?,
                (a, &Uid::MAX) => write!(f, "{a}:*")?,
                (a, b) => write!(f, "{a}:{b}")?,
            }
        }
        Ok(())
    }
}

impl FromIterator<Uid> for UidSet {
    fn from_iter<I: IntoIterator<Item = Uid>>(iter: I) -> Self {
        Self::from_uids(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_and_formats() {
        let set = UidSet::from_uids([9, 1, 2, 3, 3, 0, 5, 10, 11, 12]);
        assert_eq!(set.to_string(), "1:3,5,9:12");
        assert_eq!(set.len(), 8);
        assert_eq!((set.min(), set.max()), (Some(1), Some(12)));
        assert!(set.contains(10) && !set.contains(4) && !set.contains(13) && !set.contains(0));
        assert_eq!(set.iter().collect::<Vec<_>>(), [1, 2, 3, 5, 9, 10, 11, 12]);

        assert_eq!(UidSet::all().to_string(), "1:*");
        assert_eq!(UidSet::range(5, Uid::MAX).to_string(), "5:*");
        assert!(UidSet::range(5, 4).is_empty());
        assert_eq!(UidSet::range(0, 2).to_string(), "1:2");
        assert_eq!(UidSet::new().to_string(), "");

        let merged = UidSet::from_ranges([(10, 20), (1, 3), (4, 4), (15, 30), (40, 35)]);
        assert_eq!(merged.to_string(), "1:4,10:30,35:40");
        assert_eq!(UidSet::all().len(), u64::from(Uid::MAX));
    }

    #[test]
    fn split_highest_walks_from_new_to_old() {
        let set = UidSet::from_ranges([(1, 10), (20, 22), (30, 30)]);
        let (high, rest) = set.split_highest(5);
        assert_eq!(high.to_string(), "10,20:22,30");
        assert_eq!(high.iter().rev().collect::<Vec<_>>(), [30, 22, 21, 20, 10]);
        assert_eq!(rest.to_string(), "1:9");

        let (all, none) = set.split_highest(100);
        assert_eq!(all, set);
        assert!(none.is_empty());
        let (none, same) = set.split_highest(0);
        assert!(none.is_empty());
        assert_eq!(same, set);

        // Windows of three cover every UID exactly once, newest window first.
        let mut remaining = set.clone();
        let mut seen = Vec::new();
        while !remaining.is_empty() {
            let (window, rest) = remaining.split_highest(3);
            assert!(window.len() <= 3);
            assert!(rest.max() < window.min());
            seen.extend(window.iter().rev());
            remaining = rest;
        }
        assert_eq!(seen, set.iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn difference_finds_removed_uids() {
        let local = UidSet::from_ranges([(1, 10), (20, 30)]);
        let server = UidSet::from_uids([2, 3, 4, 9, 10, 11, 25]);
        let gone = local.difference(&server);
        assert_eq!(gone.to_string(), "1,5:8,20:24,26:30");
        assert_eq!(local.difference(&UidSet::new()), local);
        assert!(local.difference(&local).is_empty());
        assert!(local.difference(&UidSet::all()).is_empty());
        assert!(UidSet::new().difference(&local).is_empty());
        let edge = UidSet::range(5, Uid::MAX).difference(&UidSet::range(1, 6));
        assert_eq!(edge.to_string(), "7:*");
    }
}
