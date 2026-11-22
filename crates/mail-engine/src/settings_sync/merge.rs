//! Merge rules per kind (`docs/ARCHITECTURE.md`, Einstellungs-Sync), as pure functions.
//!
//! [`resolve`] picks the winner among all known versions of one record. It depends only on the
//! set of candidates, not on their order or on duplicates, so every device that sees the same
//! versions picks the same winner. Keeping only the winner and merging it with later versions
//! gives the same result as merging everything at once (checked in the tests), which is what
//! lets each device store just one version per record.
//!
//! - [`MergeRule::NewerWins`] (settings, snooze, pins, devices): the highest version wins,
//!   tombstone or not.
//! - [`MergeRule::EditBeatsDelete`] (signatures, templates, rules, views, identities, AI
//!   settings): a tombstone removes the versions up to the one its device had seen
//!   (`deleted`). Versions above that were not seen by the deleting device: they were edited
//!   concurrently (or re-created later) and beat the delete. Among the surviving versions the
//!   highest wins; if none survives, the tombstone with the highest `deleted` (then version).
//! - [`MergeRule::PerField`] (accounts): as before per record; among several surviving versions
//!   each field takes its newest value. If that equals an existing version, it wins; otherwise
//!   the merged content needs a new version ([`Resolution::Merged`]).

use std::collections::BTreeMap;

use super::record::{FieldValue, MergeRule, RecordKind, SyncContent, SyncRecord};

/// The outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<'a> {
    /// One of the candidates wins as it is.
    Existing(&'a SyncRecord),
    /// Fields of several versions combine into new content. The caller stores it under a new
    /// version from its clock (greater than every candidate).
    Merged(SyncContent),
}

/// The winner among `candidates` (all versions of one record, any order, duplicates allowed).
/// `None` only for an empty list.
pub fn resolve<'a>(kind: RecordKind, candidates: &[&'a SyncRecord]) -> Option<Resolution<'a>> {
    match kind.spec().merge {
        MergeRule::NewerWins => newest(candidates.iter().copied()).map(Resolution::Existing),
        MergeRule::EditBeatsDelete => {
            let (alive, tombstone) = survivors(candidates);
            newest(alive.into_iter())
                .or(tombstone)
                .map(Resolution::Existing)
        }
        MergeRule::PerField => {
            let (alive, tombstone) = survivors(candidates);
            if alive.is_empty() {
                return tombstone.map(Resolution::Existing);
            }
            merge_fields(kind, &alive)
        }
    }
}

/// The highest version (versions are unique per payload, so ties are copies of one record).
fn newest<'a>(records: impl Iterator<Item = &'a SyncRecord>) -> Option<&'a SyncRecord> {
    records.max_by_key(|r| r.version())
}

/// Contents not removed by a tombstone, and the strongest tombstone.
fn survivors<'a>(candidates: &[&'a SyncRecord]) -> (Vec<&'a SyncRecord>, Option<&'a SyncRecord>) {
    let tombstone = candidates
        .iter()
        .copied()
        .filter(|r| r.is_tombstone())
        .max_by_key(|r| (r.deleted(), r.version()));
    let removed_up_to = tombstone.and_then(SyncRecord::deleted);
    let alive = candidates
        .iter()
        .copied()
        .filter(|r| !r.is_tombstone() && removed_up_to.is_none_or(|d| r.version() > d))
        .collect();
    (alive, tombstone)
}

fn merge_fields<'a>(kind: RecordKind, alive: &[&'a SyncRecord]) -> Option<Resolution<'a>> {
    let fallback = || newest(alive.iter().copied()).map(Resolution::Existing);
    let parsed: Vec<(&SyncRecord, BTreeMap<String, FieldValue>)> = alive
        .iter()
        .filter_map(|r| {
            r.content_ref()
                .and_then(SyncContent::fields)
                .map(|f| (*r, f))
        })
        .collect();
    // Content is validated on construction, so this only happens for a record that is not
    // per-field after all: fall back to the newest version.
    if parsed.len() != alive.len() {
        return fallback();
    }
    let mut merged: BTreeMap<String, FieldValue> = BTreeMap::new();
    for (_, fields) in &parsed {
        for (name, field) in fields {
            match merged.get(name) {
                // Equal versions come from the same edit; the value comparison only keeps the
                // result deterministic if a broken writer reused a version.
                Some(current)
                    if (current.version, current.value.to_string())
                        >= (field.version, field.value.to_string()) => {}
                _ => {
                    merged.insert(name.clone(), field.clone());
                }
            }
        }
    }
    let existing = parsed
        .iter()
        .filter(|(_, fields)| *fields == merged)
        .map(|(record, _)| *record)
        .max_by_key(|r| r.version());
    if let Some(record) = existing {
        return Some(Resolution::Existing(record));
    }
    match SyncContent::from_fields(kind, &merged) {
        Ok(content) => Some(Resolution::Merged(content)),
        // The union of valid field sets is valid unless it grows beyond the size limit; then
        // the newest version wins whole.
        Err(_) => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::settings_sync::hlc::{DeviceId, Hlc};
    use crate::settings_sync::record::{RecordKey, SettingScope};

    fn v(millis: i64, device: u128) -> Hlc {
        Hlc::new(
            millis,
            0,
            DeviceId::from_uuid(uuid::Uuid::from_u128(device)),
        )
        .expect("v")
    }

    fn signature_key() -> RecordKey {
        RecordKey::uuid(RecordKind::Signature, uuid::Uuid::from_u128(5)).expect("key")
    }

    fn signature(version: Hlc, text: &str) -> SyncRecord {
        let content = SyncContent::new(RecordKind::Signature, json!({ "text": text })).expect("c");
        SyncRecord::content(signature_key(), version, content).expect("record")
    }

    fn deleted(key: RecordKey, version: Hlc, seen: Hlc) -> SyncRecord {
        SyncRecord::tombstone(key, version, seen).expect("tombstone")
    }

    fn winner<'a>(kind: RecordKind, candidates: &[&'a SyncRecord]) -> &'a SyncRecord {
        match resolve(kind, candidates) {
            Some(Resolution::Existing(r)) => r,
            other => panic!("expected an existing winner, got {other:?}"),
        }
    }

    #[test]
    fn settings_newer_version_wins_including_deletes() {
        let key = RecordKey::setting(SettingScope::Global, "ui.theme").expect("key");
        let set = |version, value: &str| {
            let content = SyncContent::new(RecordKind::Setting, json!(value)).expect("c");
            SyncRecord::content(key.clone(), version, content).expect("r")
        };
        let dark = set(v(10, 1), "dark");
        let light = set(v(10, 2), "light");
        let reset = deleted(key.clone(), v(11, 1), v(10, 1));
        assert_eq!(
            winner(RecordKind::Setting, &[&dark, &light]),
            &light,
            "device id breaks the tie"
        );
        assert_eq!(winner(RecordKind::Setting, &[&light, &dark]), &light);
        assert_eq!(
            winner(RecordKind::Setting, &[&dark, &light, &reset]),
            &reset
        );
        let later = set(v(12, 2), "light");
        assert_eq!(winner(RecordKind::Setting, &[&reset, &later]), &later);
        assert_eq!(resolve(RecordKind::Setting, &[]), None);
    }

    #[test]
    fn edit_beats_concurrent_delete() {
        let base = signature(v(10, 1), "base");
        // Device 1 deletes having seen `base`; device 2 edits `base` concurrently, earlier in
        // time than the delete.
        let delete = deleted(signature_key(), v(30, 1), v(10, 1));
        let edit = signature(v(20, 2), "edited");
        assert_eq!(
            winner(RecordKind::Signature, &[&base, &delete, &edit]),
            &edit
        );
        assert_eq!(winner(RecordKind::Signature, &[&delete, &edit]), &edit);
        // A delete after seeing the edit wins.
        let delete_after = deleted(signature_key(), v(40, 1), v(20, 2));
        assert_eq!(
            winner(RecordKind::Signature, &[&edit, &delete_after, &base]),
            &delete_after
        );
        // Plain newer-wins would have lost the edit.
        assert_eq!(
            winner(RecordKind::Setting, &[&delete, &edit]).version(),
            v(30, 1)
        );
    }

    #[test]
    fn two_deletes_of_the_same_version_agree() {
        let a = deleted(signature_key(), v(20, 1), v(10, 1));
        let b = deleted(signature_key(), v(21, 2), v(10, 1));
        assert_eq!(winner(RecordKind::Signature, &[&a, &b]), &b);
        assert_eq!(winner(RecordKind::Signature, &[&b, &a]), &b);
    }

    fn account_key() -> RecordKey {
        RecordKey::uuid(RecordKind::Account, uuid::Uuid::from_u128(9)).expect("key")
    }

    fn account(version: Hlc, fields: &[(&str, Hlc, Value)]) -> SyncRecord {
        let map = fields
            .iter()
            .map(|(name, version, value)| {
                (
                    (*name).to_owned(),
                    FieldValue {
                        version: *version,
                        value: value.clone(),
                    },
                )
            })
            .collect();
        let content = SyncContent::from_fields(RecordKind::Account, &map).expect("content");
        SyncRecord::content(account_key(), version, content).expect("record")
    }

    #[test]
    fn accounts_merge_field_by_field() {
        let base = [
            ("displayName", v(1, 1), json!("Alice")),
            ("color", v(1, 1), json!(2)),
        ];
        // Device 1 renames, device 2 recolours, concurrently.
        let renamed = account(
            v(5, 1),
            &[("displayName", v(5, 1), json!("Alice B.")), base[1].clone()],
        );
        let recoloured = account(v(6, 2), &[base[0].clone(), ("color", v(6, 2), json!(4))]);
        let merged = match resolve(RecordKind::Account, &[&renamed, &recoloured]) {
            Some(Resolution::Merged(content)) => content,
            other => panic!("expected merged content, got {other:?}"),
        };
        let values = merged.field_values().expect("fields");
        assert_eq!(values.get("displayName"), Some(&json!("Alice B.")));
        assert_eq!(values.get("color"), Some(&json!(4)));
        // Same result in the other order.
        assert_eq!(
            resolve(RecordKind::Account, &[&recoloured, &renamed]),
            Some(Resolution::Merged(merged.clone()))
        );
        // Once stored under a new version, merging again changes nothing.
        let stored = SyncRecord::content(account_key(), v(7, 1), merged).expect("stored");
        assert_eq!(
            winner(RecordKind::Account, &[&renamed, &stored, &recoloured]),
            &stored
        );
        // A version that already holds every newest field wins as it is.
        let ahead = account(
            v(8, 2),
            &[
                ("displayName", v(5, 1), json!("Alice B.")),
                ("color", v(6, 2), json!(4)),
            ],
        );
        assert_eq!(winner(RecordKind::Account, &[&stored, &ahead]), &ahead);
    }

    #[test]
    fn account_delete_and_concurrent_edit() {
        let a = account(v(1, 1), &[("displayName", v(1, 1), json!("A"))]);
        let removed = deleted(account_key(), v(5, 1), v(1, 1));
        let edited = account(v(3, 2), &[("displayName", v(3, 2), json!("B"))]);
        assert_eq!(winner(RecordKind::Account, &[&a, &removed]), &removed);
        assert_eq!(
            winner(RecordKind::Account, &[&a, &removed, &edited]),
            &edited
        );
    }

    /// Keeping only the winner and merging it with the next version gives the same result as
    /// merging all versions at once, in any order.
    #[test]
    fn folding_winners_equals_merging_everything() {
        let key = signature_key();
        let records = [
            signature(v(10, 1), "a"),
            signature(v(20, 2), "b"),
            deleted(key.clone(), v(25, 1), v(10, 1)),
            deleted(key.clone(), v(35, 3), v(20, 2)),
            signature(v(30, 3), "c"),
            deleted(key.clone(), v(40, 2), v(30, 3)),
            signature(v(31, 1), "d"),
        ];
        let all: Vec<&SyncRecord> = records.iter().collect();
        for kind in [RecordKind::Signature, RecordKind::Setting] {
            let expected = winner(kind, &all).clone();
            // All rotations and the reverse order as fold sequences.
            for start in 0..records.len() {
                for reverse in [false, true] {
                    let mut order: Vec<&SyncRecord> = all.clone();
                    order.rotate_left(start);
                    if reverse {
                        order.reverse();
                    }
                    let mut acc = order[0].clone();
                    for next in &order[1..] {
                        acc = winner(kind, &[&acc, next]).clone();
                    }
                    assert_eq!(acc, expected, "{kind}: start {start} reverse {reverse}");
                }
            }
        }
    }
}
