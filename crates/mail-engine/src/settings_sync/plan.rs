//! Planning a pass, without IO: what to download, upload, delete and apply locally.
//!
//! A pass has two steps, both pure:
//! 1. [`Planner::new`] compares the listing (headers only) with the local records and decides
//!    which entries must be downloaded ([`Planner::to_fetch`]). An entry whose version equals
//!    the local or last synced version, or that provably loses against the local version
//!    from its header alone, is not downloaded.
//! 2. [`Planner::finish`] takes the opened (decrypted and checked) entries and resolves every
//!    record with the merge rules into a [`SyncPlan`].
//!
//! Rules beyond the merge rules (`docs/SYNC.md`):
//! - **Order of execution:** per record, the winner is uploaded first and the losers are
//!   deleted only after that succeeded, so a crash leaves a duplicate, never a gap.
//! - **Absent records:** a local record that was synced before and is no longer on the server
//!   was either lost (deleted by another client, data loss) or deleted elsewhere with a
//!   tombstone that has expired since. It counts as deleted (stale) only if both its version
//!   and this device's last complete pass are older than the retention: otherwise a tombstone
//!   would still be on the server. Stale records are forgotten locally (accounts: the user is
//!   asked), everything else is uploaded again.
//! - **Tombstones:** kept on the server for [`TOMBSTONE_RETENTION_MS`] after the deletion,
//!   then removed by the next pass of any device. Locally a tombstone stays, so an old version
//!   that comes back later (a replay by the server, a device that was offline for longer) is
//!   recognised as deleted; the deletion is then announced again with a fresh tombstone.
//! - **Deferred entries:** versions further ahead than [`MAX_CLOCK_DRIFT_MS`] leave their whole
//!   record untouched until the local clock catches up.
//! - **Rejected entries** never count as a version of their record and are reported. Damaged
//!   or misplaced ones (authentication failed, header mismatch) are deleted once a valid winner
//!   of their record is stored; entries of another key, of a newer app version or with
//!   malformed content are left on the server. An entry damaged under an unchanged header is
//!   only noticed by a device that downloads it: other devices, or a full check
//!   ([`PlanContext::full_check`]) on the device that wrote it.
//! - **Download limits** ([`FetchLimits`], finding F2 of the stage 15 review), decided from
//!   the listed sizes before anything is read. An entry listed larger than a record can be is
//!   never downloaded, never counts as a version and is never deleted (like an entry of another
//!   key); it is reported ([`Notice::TooLarge`]). Per pass at most `max_entries` entries and
//!   `max_bytes` bytes are downloaded: records in the order of their opaque ids, each with all
//!   of its new entries or not at all. A record that does not fit waits as a whole, like a
//!   deferred one, and is reported ([`Notice::Postponed`]); later records that fit still come.
//!   Records downloaded once are known in the next pass, so repeated passes work through the
//!   folder (as long as entries that stay unreadable do not fill a pass alone). A pass that
//!   postponed records does not count as complete. A full check downloads the entries it only
//!   re-checks after that, as far as room is left; the others are treated as known.

use std::collections::BTreeMap;

use super::crypto::{OpaqueId, OpenError, RecordHeader, SyncKey};
use super::error::{HlcError, SettingsSyncError};
use super::hlc::{Hlc, HlcClock, MAX_CLOCK_DRIFT_MS};
use super::merge::{Resolution, resolve};
use super::record::{MergeRule, RecordKey, Removal, SyncRecord};
use super::transport::{EntryHandle, FetchLimits, RemoteEntry};

/// Tombstones stay on the server for 180 days after the deletion, so a device that was away
/// for up to half a year still learns about it.
pub const TOMBSTONE_RETENTION_MS: i64 = 180 * 24 * 60 * 60 * 1000;

/// One local record as the integration keeps it (`sync_record` plus the data table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRecord {
    /// The current local state: content, or a tombstone after a local or synced delete.
    pub record: SyncRecord,
    /// The version last known to be on the server, `None` if never synced.
    pub synced: Option<Hlc>,
}

impl LocalRecord {
    /// Changed locally since the last sync (or never synced).
    pub fn is_dirty(&self) -> bool {
        self.synced != Some(self.record.version())
    }
}

/// Time inputs of a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanContext {
    /// Wall clock of this device, Unix ms.
    pub now_ms: i64,
    /// When this device last listed the complete folder in a successful pass.
    pub last_complete_sync_ms: Option<i64>,
    /// Download and check every entry, also those known from their header. Finds entries
    /// damaged on the server under an unchanged header (which only a full check can tell);
    /// the integration runs it now and then, for example once a day.
    pub full_check: bool,
    /// What this pass may download (see the module docs).
    pub limits: FetchLimits,
}

impl PlanContext {
    /// Tombstones deleted before this time are expired.
    pub const fn horizon_ms(&self) -> i64 {
        self.now_ms.saturating_sub(TOMBSTONE_RETENTION_MS)
    }
}

/// Something about a single entry the user or the log should know; the pass goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The header values do not parse (not written by this app, or changed by the server).
    InvalidHeader { handle: EntryHandle },
    /// The version lies too far in the future (the writing device's clock is ahead). Merged
    /// once this device's clock is within [`MAX_CLOCK_DRIFT_MS`].
    Deferred {
        handle: EntryHandle,
        version: Hlc,
        ahead_ms: i64,
    },
    /// Could not be opened or did not match its header; ignored and left on the server.
    Rejected {
        handle: EntryHandle,
        error: OpenError,
    },
    /// Listed larger than any record can be ([`FetchLimits::max_entry_bytes`]): not
    /// downloaded, ignored and left on the server.
    TooLarge { handle: EntryHandle, size: u64 },
    /// Over the limits of this pass ([`FetchLimits::max_entries`], [`FetchLimits::max_bytes`]):
    /// not downloaded; its record waits for the next pass.
    Postponed { handle: EntryHandle, size: u64 },
}

impl Notice {
    pub const fn handle(&self) -> EntryHandle {
        match self {
            Self::InvalidHeader { handle }
            | Self::Deferred { handle, .. }
            | Self::Rejected { handle, .. }
            | Self::TooLarge { handle, .. }
            | Self::Postponed { handle, .. } => *handle,
        }
    }

    /// The entry was left out of this pass, and the user should know (the status counts it).
    pub const fn is_left_out(&self) -> bool {
        matches!(
            self,
            Self::InvalidHeader { .. }
                | Self::Rejected { .. }
                | Self::TooLarge { .. }
                | Self::Postponed { .. }
        )
    }

    /// Its record waits for a later pass: the next one must run even if nothing else changed.
    pub const fn waits(&self) -> bool {
        matches!(self, Self::Deferred { .. } | Self::Postponed { .. })
    }
}

/// A decision only the user can make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Another device removed this record (an account), or it vanished from the server long
    /// ago. It stays here until the user decides: remove it here too
    /// ([`super::LocalState::accept_removal`]) or keep it on all devices
    /// ([`super::LocalState::keep`]). `tombstone` is the removal, `None` if it has expired.
    Removed {
        key: RecordKey,
        tombstone: Option<SyncRecord>,
    },
}

impl Question {
    pub const fn key(&self) -> &RecordKey {
        match self {
            Self::Removed { key, .. } => key,
        }
    }
}

/// What happens to the local record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalChange {
    /// Store this record (content or tombstone); it is then in sync at its version.
    Apply(SyncRecord),
    /// The local record is unchanged and now in sync at this version.
    MarkSynced(Hlc),
    /// Remove the record and its sync state (stale, see module docs).
    Forget,
}

/// The work for one record. Executed in order: upload, then delete, then the local change;
/// if the upload fails, nothing else of this group happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlan {
    pub key: RecordKey,
    pub upload: Option<SyncRecord>,
    pub delete: Vec<EntryHandle>,
    pub local: Option<LocalChange>,
}

/// The result of planning. Groups are ordered by record key, notices by handle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncPlan {
    pub groups: Vec<GroupPlan>,
    pub questions: Vec<Question>,
    pub notices: Vec<Notice>,
}

impl SyncPlan {
    /// Nothing to upload, delete or change locally.
    pub fn is_quiet(&self) -> bool {
        self.groups.is_empty()
    }
}

/// An entry to download and open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    pub handle: EntryHandle,
    pub header: RecordHeader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryState {
    /// Same version as the local record or its last synced version.
    Known,
    /// Loses against the local record, decided from the header.
    Dominated,
    Fetch,
    Deferred,
    /// Listed larger than a record can be: ignored, never downloaded or deleted.
    TooLarge,
    /// Over the limits of this pass: its record waits.
    Postponed,
}

#[derive(Debug, Clone)]
struct Entry {
    handle: EntryHandle,
    header: RecordHeader,
    /// The listed size.
    size: u64,
    state: EntryState,
    /// A full check downloads an entry that is known (or dominated) only to check it: its
    /// state in a normal pass, which it falls back to when the limits leave no room.
    recheck: Option<EntryState>,
}

/// Plans one pass; see the module docs.
#[derive(Debug)]
pub struct Planner<'a> {
    local: &'a BTreeMap<RecordKey, LocalRecord>,
    ctx: PlanContext,
    by_opaque: BTreeMap<OpaqueId, &'a RecordKey>,
    groups: BTreeMap<OpaqueId, Vec<Entry>>,
    notices: Vec<Notice>,
}

impl<'a> Planner<'a> {
    /// Step 1. `listing` is the transport's listing; key records in it are skipped.
    pub fn new(
        key: &SyncKey,
        local: &'a BTreeMap<RecordKey, LocalRecord>,
        listing: &[RemoteEntry],
        ctx: PlanContext,
    ) -> Self {
        let by_opaque: BTreeMap<OpaqueId, &RecordKey> =
            local.keys().map(|k| (key.opaque_id(k), k)).collect();
        let mut groups: BTreeMap<OpaqueId, Vec<Entry>> = BTreeMap::new();
        let mut notices = Vec::new();
        for entry in listing {
            let Some(header) = RecordHeader::parse(&entry.header) else {
                notices.push(Notice::InvalidHeader {
                    handle: entry.handle,
                });
                continue;
            };
            if header.opaque_id.is_key_record() {
                continue;
            }
            let ahead_ms = header.version.millis() - ctx.now_ms;
            let local_record = by_opaque.get(&header.opaque_id).and_then(|k| local.get(*k));
            let mut recheck = None;
            let state = if !ctx.limits.fits(entry.size) {
                // Never downloaded, so it cannot hold its record back either.
                notices.push(Notice::TooLarge {
                    handle: entry.handle,
                    size: entry.size,
                });
                EntryState::TooLarge
            } else if ahead_ms > MAX_CLOCK_DRIFT_MS {
                notices.push(Notice::Deferred {
                    handle: entry.handle,
                    version: header.version,
                    ahead_ms,
                });
                EntryState::Deferred
            } else {
                let known = match local_record {
                    Some(lr)
                        if header.version == lr.record.version()
                            || Some(header.version) == lr.synced =>
                    {
                        Some(EntryState::Known)
                    }
                    Some(lr) if dominated_by(lr, header.version) => Some(EntryState::Dominated),
                    _ => None,
                };
                match known {
                    Some(known) if ctx.full_check => {
                        recheck = Some(known);
                        EntryState::Fetch
                    }
                    Some(known) => known,
                    None => EntryState::Fetch,
                }
            };
            groups
                .entry(header.opaque_id.clone())
                .or_default()
                .push(Entry {
                    handle: entry.handle,
                    header,
                    size: entry.size,
                    state,
                    recheck,
                });
        }
        apply_limits(&mut groups, ctx.limits, &mut notices);
        Self {
            local,
            ctx,
            by_opaque,
            groups,
            notices,
        }
    }

    /// The entries to download and open, by handle.
    pub fn to_fetch(&self) -> Vec<FetchRequest> {
        let mut out: Vec<FetchRequest> = self
            .groups
            .values()
            .flatten()
            .filter(|e| e.state == EntryState::Fetch)
            .map(|e| FetchRequest {
                handle: e.handle,
                header: e.header.clone(),
            })
            .collect();
        out.sort_by_key(|r| r.handle);
        out
    }

    /// Step 2. `opened` holds the result of opening each fetched entry (entries deleted
    /// meanwhile may be missing). Observes every accepted version on `hlc` and takes new
    /// versions from it for merged and re-announced records.
    pub fn finish(
        self,
        opened: Vec<(EntryHandle, Result<SyncRecord, OpenError>)>,
        hlc: &mut HlcClock,
    ) -> Result<SyncPlan, SettingsSyncError> {
        let mut opened: BTreeMap<EntryHandle, Result<SyncRecord, OpenError>> =
            opened.into_iter().collect();
        let mut plan = SyncPlan {
            notices: self.notices,
            ..SyncPlan::default()
        };
        let mut work: BTreeMap<RecordKey, Work<'a>> = BTreeMap::new();
        for (opaque, entries) in self.groups {
            let mut group = Work::default();
            let mut key = self.by_opaque.get(&opaque).map(|k| (*k).clone());
            for entry in entries {
                match entry.state {
                    EntryState::Known | EntryState::Dominated => {
                        group
                            .present
                            .push((entry.handle, entry.header.version, false));
                    }
                    EntryState::Deferred | EntryState::Postponed => group.held = true,
                    // Reported already; never a version of the record, never deleted.
                    EntryState::TooLarge => {}
                    EntryState::Fetch => match opened.remove(&entry.handle) {
                        None => {}
                        Some(Err(error)) => {
                            if matches!(error, OpenError::Damaged | OpenError::HeaderMismatch(_)) {
                                group.garbage.push(entry.handle);
                            }
                            plan.notices.push(Notice::Rejected {
                                handle: entry.handle,
                                error,
                            });
                        }
                        // Opened records are checked against their header, so they belong
                        // to this opaque id and carry the header's version.
                        Some(Ok(record)) => match hlc.observe(&record.version()) {
                            Ok(()) => {
                                key.get_or_insert_with(|| record.key().clone());
                                group.present.push((entry.handle, record.version(), true));
                                group.fetched.push(record);
                            }
                            Err(HlcError::ClockAhead { ahead_ms }) => {
                                plan.notices.push(Notice::Deferred {
                                    handle: entry.handle,
                                    version: record.version(),
                                    ahead_ms,
                                });
                                group.held = true;
                            }
                            Err(other) => return Err(other.into()),
                        },
                    },
                }
            }
            // Without a local record and without an opened entry there is nothing to merge.
            if let Some(key) = key {
                group.local = self.local.get(&key);
                work.insert(key, group);
            }
        }
        for (key, record) in self.local {
            work.entry(key.clone()).or_insert_with(|| Work {
                local: Some(record),
                ..Work::default()
            });
        }
        for (key, group) in work {
            decide(key, group, self.ctx, hlc, &mut plan)?;
        }
        plan.notices.sort_by_key(Notice::handle);
        Ok(plan)
    }
}

/// Applies the limits of one pass to what [`Planner::new`] wants to download (see the module
/// docs): first the entries records need, all of a record's or none, in the order of the opaque
/// ids; then the re-checks of a full check, one by one, while room is left.
fn apply_limits(
    groups: &mut BTreeMap<OpaqueId, Vec<Entry>>,
    limits: FetchLimits,
    notices: &mut Vec<Notice>,
) {
    let mut entries_left = limits.max_entries;
    let mut bytes_left = limits.max_bytes;
    for entries in groups.values_mut() {
        let (count, bytes) = entries
            .iter()
            .filter(|e| e.state == EntryState::Fetch && e.recheck.is_none())
            .fold((0_usize, 0_u64), |(n, b), e| {
                (n + 1, b.saturating_add(e.size))
            });
        if count == 0 {
            continue;
        }
        if count <= entries_left && bytes <= bytes_left {
            entries_left -= count;
            bytes_left -= bytes;
            continue;
        }
        // A record is decided only with all of its new entries: it waits as a whole.
        for entry in entries.iter_mut().filter(|e| e.state == EntryState::Fetch) {
            match entry.recheck.take() {
                Some(known) => entry.state = known,
                None => {
                    entry.state = EntryState::Postponed;
                    notices.push(Notice::Postponed {
                        handle: entry.handle,
                        size: entry.size,
                    });
                }
            }
        }
    }
    for entry in groups.values_mut().flatten() {
        let Some(known) = entry.recheck else {
            continue;
        };
        if entry.state != EntryState::Fetch {
            continue;
        }
        if entries_left > 0 && entry.size <= bytes_left {
            entries_left -= 1;
            bytes_left -= entry.size;
        } else {
            // Not checked this time; as in a normal pass.
            entry.state = known;
        }
    }
}

/// Whether a remote version provably loses against the local record, from its header alone.
fn dominated_by(local: &LocalRecord, remote: Hlc) -> bool {
    let record = &local.record;
    match record.kind().spec().merge {
        // Fields of an older version can still be newer than the local ones.
        MergeRule::PerField => false,
        MergeRule::NewerWins => remote < record.version(),
        // Local content: an older version loses, and an older tombstone deleted at most what
        // is older still. Local tombstone: versions it deleted (and tombstones of those) lose.
        MergeRule::EditBeatsDelete => match record.deleted() {
            None => remote < record.version(),
            Some(deleted) => remote <= deleted,
        },
    }
}

#[derive(Debug, Default)]
struct Work<'a> {
    local: Option<&'a LocalRecord>,
    /// Usable entries on the server: handle, version, whether it was downloaded and checked.
    present: Vec<(EntryHandle, Hlc, bool)>,
    /// Opened entries.
    fetched: Vec<SyncRecord>,
    /// Damaged or misplaced entries under this opaque id: removed once a valid winner is
    /// stored, so the folder heals. (Entries of newer versions, malformed ones, those of
    /// another key and those too large to download stay.)
    garbage: Vec<EntryHandle>,
    /// An entry was deferred (clock ahead) or postponed (limits of the pass): the record is
    /// left as it is in this pass.
    held: bool,
}

fn decide(
    key: RecordKey,
    work: Work<'_>,
    ctx: PlanContext,
    hlc: &mut HlcClock,
    plan: &mut SyncPlan,
) -> Result<(), SettingsSyncError> {
    if work.held {
        return Ok(());
    }
    let horizon = ctx.horizon_ms();
    let spec = key.kind().spec();
    if work.present.is_empty() {
        if let Some(local) = work.local {
            decide_absent(key, local, work.garbage, ctx, plan);
        }
        return Ok(());
    }

    let mut candidates: Vec<&SyncRecord> = work.fetched.iter().collect();
    if let Some(local) = work.local {
        candidates.push(&local.record);
    }
    let Some(resolution) = resolve(key.kind(), &candidates) else {
        return Ok(());
    };
    let mut winner = match resolution {
        Resolution::Existing(record) => record.clone(),
        Resolution::Merged(content) => SyncRecord::content(key.clone(), hlc.now()?, content)?,
    };

    let local_is_live = work.local.is_some_and(|l| !l.record.is_tombstone());
    if spec.removal == Removal::Ask && winner.is_tombstone() && local_is_live {
        plan.questions.push(Question::Removed {
            key,
            tombstone: Some(winner),
        });
        return Ok(());
    }

    let on_server = work.present.iter().any(|(_, v, _)| *v == winner.version());
    let all: Vec<EntryHandle> = work.present.iter().map(|(h, _, _)| *h).collect();
    let (upload, delete) = match winner.deleted() {
        Some(deleted) if winner.version().millis() < horizon => {
            if on_server {
                // The tombstone has done its job: remove it (and its losers) from the server.
                (None, all)
            } else {
                // Older versions are on the server again (a replay, or a device that missed
                // the deletion) while the tombstone has expired there. Deleting them silently
                // would let a device that adopted them upload them again and again; announce
                // the deletion again for another retention period instead.
                winner = SyncRecord::tombstone(key.clone(), hlc.now()?, deleted)?;
                (Some(winner.clone()), all)
            }
        }
        _ => {
            // One copy of the winner stays (a checked one if there is one, else the oldest);
            // other versions and further copies of the winner (replays) go.
            let keep = work
                .present
                .iter()
                .filter(|(_, v, _)| *v == winner.version())
                .min_by_key(|(h, _, checked)| (!checked, *h))
                .map(|(h, _, _)| *h);
            let losers = work
                .present
                .iter()
                .map(|(h, _, _)| *h)
                .filter(|h| Some(*h) != keep)
                .collect();
            ((!on_server).then(|| winner.clone()), losers)
        }
    };
    let local = match work.local {
        Some(local) if local.record == winner => (local.synced != Some(winner.version()))
            .then_some(LocalChange::MarkSynced(winner.version())),
        _ => Some(LocalChange::Apply(winner)),
    };
    let mut delete = delete;
    delete.extend(work.garbage);
    push_group(plan, key, upload, delete, local);
    Ok(())
}

/// A local record with no usable entry on the server (see module docs).
fn decide_absent(
    key: RecordKey,
    local: &LocalRecord,
    garbage: Vec<EntryHandle>,
    ctx: PlanContext,
    plan: &mut SyncPlan,
) {
    let version = local.record.version();
    let recent = version.millis() >= ctx.horizon_ms();
    let mark = (local.synced != Some(version)).then_some(LocalChange::MarkSynced(version));
    let upload = Some(local.record.clone());
    if local.record.is_tombstone() {
        // An expired tombstone is gone from the server as planned; a recent one was lost.
        if recent {
            push_group(plan, key, upload, garbage, mark);
        }
        return;
    }
    let stale = local.synced.is_some()
        && !recent
        && ctx
            .last_complete_sync_ms
            .is_none_or(|t| t < ctx.horizon_ms());
    if !stale {
        push_group(plan, key, upload, garbage, mark);
    } else if key.kind().spec().removal == Removal::Ask {
        plan.questions.push(Question::Removed {
            key,
            tombstone: None,
        });
    } else {
        push_group(plan, key, None, Vec::new(), Some(LocalChange::Forget));
    }
}

fn push_group(
    plan: &mut SyncPlan,
    key: RecordKey,
    upload: Option<SyncRecord>,
    mut delete: Vec<EntryHandle>,
    local: Option<LocalChange>,
) {
    if upload.is_none() && delete.is_empty() && local.is_none() {
        return;
    }
    delete.sort_unstable();
    plan.groups.push(GroupPlan {
        key,
        upload,
        delete,
        local,
    });
}
