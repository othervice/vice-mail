//! Hybrid logical clock (Kulkarni et al., "Logical Physical Clocks", 2014): every record version
//! is (wall time in ms, counter, device id). It stays close to the wall clock, never runs
//! backwards on one device, orders a change after every version the device has seen, and
//! breaks ties by the device id, so all devices agree on one total order.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::HlcError;
use crate::clock::Clock;

/// How far a remote version may lie ahead of the local wall clock before it is deferred.
///
/// Without a bound, one device with a clock set to 2099 would win every conflict and drag every
/// other device's clock (and with it every later version) into 2099. Desktops keep their clocks
/// within well under a second by NTP; five minutes also covers a clock that has not resynced
/// yet after sleep or a small manual error, so honest devices are never held back. A record
/// beyond the bound is not dropped: it is deferred (reported as [`super::Notice::Deferred`]) and
/// merged as soon as the local clock is within the bound, which keeps the sync convergent.
pub const MAX_CLOCK_DRIFT_MS: i64 = 5 * 60 * 1000;

/// Largest millisecond value: 13 decimal digits (year 2286), so the text form has fixed width
/// and sorts like the numbers.
const MAX_MILLIS: i64 = 9_999_999_999_999;

/// Stable random id of one device in the settings sync (UUID v4). It only breaks ties between
/// versions with equal time and counter and names the device in the device list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(uuid::Uuid);

impl DeviceId {
    /// A new random id, created once when a device joins the sync.
    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    pub const fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    pub const fn as_uuid(&self) -> &uuid::Uuid {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for DeviceId {
    type Err = HlcError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Only the canonical lowercase hyphenated form, so one id has exactly one text form.
        match uuid::Uuid::try_parse(s) {
            Ok(uuid) if uuid.hyphenated().to_string() == s => Ok(Self(uuid)),
            _ => Err(HlcError::InvalidText(s.to_owned())),
        }
    }
}

/// One version: compared by time, then counter, then device id (derived `Ord`, field order).
///
/// Text form (headers, encrypted payload, field versions): `TTTTTTTTTTTTT-CCCCCCCC-<device>`
/// with 13 decimal digits of Unix milliseconds and 8 hex digits of counter, for example
/// `1764000000000-00000000-6f9619ff-8b86-4d11-b42d-00c04fc964ff`. Fixed width, so the text
/// sorts like the versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hlc {
    millis: i64,
    counter: u32,
    device: DeviceId,
}

impl Hlc {
    /// `None` if `millis` is negative or beyond the year 2286.
    pub const fn new(millis: i64, counter: u32, device: DeviceId) -> Option<Self> {
        if millis < 0 || millis > MAX_MILLIS {
            return None;
        }
        Some(Self {
            millis,
            counter,
            device,
        })
    }

    /// Unix milliseconds (logical, close to the wall clock of the writing device).
    pub const fn millis(&self) -> i64 {
        self.millis
    }

    pub const fn counter(&self) -> u32 {
        self.counter
    }

    /// The device that created this version.
    pub const fn device(&self) -> DeviceId {
        self.device
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:013}-{:08x}-{}",
            self.millis, self.counter, self.device
        )
    }
}

impl FromStr for Hlc {
    type Err = HlcError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || HlcError::InvalidText(s.to_owned());
        let (millis, rest) = s.split_once('-').ok_or_else(invalid)?;
        let (counter, device) = rest.split_once('-').ok_or_else(invalid)?;
        if millis.len() != 13 || !millis.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        if counter.len() != 8
            || !counter
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(invalid());
        }
        let millis: i64 = millis.parse().map_err(|_| invalid())?;
        let counter = u32::from_str_radix(counter, 16).map_err(|_| invalid())?;
        let device: DeviceId = device.parse().map_err(|_| invalid())?;
        Self::new(millis, counter, device).ok_or_else(invalid)
    }
}

impl Serialize for Hlc {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hlc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The clock of one device. Not `Clone`: one device has exactly one, and its state (the last
/// time and counter) must be persisted with the records and passed to [`HlcClock::resume`]
/// after a restart, so versions stay monotonic across restarts.
pub struct HlcClock {
    clock: Arc<dyn Clock>,
    device: DeviceId,
    /// Last issued or observed (time, counter).
    last: Option<(i64, u32)>,
}

impl fmt::Debug for HlcClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HlcClock")
            .field("device", &self.device)
            .field("last", &self.last)
            .finish_non_exhaustive()
    }
}

impl HlcClock {
    pub fn new(clock: Arc<dyn Clock>, device: DeviceId) -> Self {
        Self {
            clock,
            device,
            last: None,
        }
    }

    /// Continues after `last`, the highest version this device has issued or observed
    /// (for example the maximum over its `sync_record` rows).
    pub fn resume(clock: Arc<dyn Clock>, device: DeviceId, last: Option<Hlc>) -> Self {
        Self {
            clock,
            device,
            last: last.map(|v| (v.millis, v.counter)),
        }
    }

    pub const fn device(&self) -> DeviceId {
        self.device
    }

    /// Wall clock in ms, clamped into the representable range.
    pub fn wall_ms(&self) -> i64 {
        self.clock.now_ms().clamp(0, MAX_MILLIS)
    }

    /// The highest (time, counter) issued or observed so far, as a version of this device.
    pub fn last(&self) -> Option<Hlc> {
        self.last
            .and_then(|(millis, counter)| Hlc::new(millis, counter, self.device))
    }

    /// A new version for a local change: later than everything issued or observed before.
    pub fn now(&mut self) -> Result<Hlc, HlcError> {
        let wall = self.wall_ms();
        let (millis, counter) = match self.last {
            Some((last, counter)) if last >= wall => {
                (last, counter.checked_add(1).ok_or(HlcError::Overflow)?)
            }
            _ => (wall, 0),
        };
        let version = Hlc::new(millis, counter, self.device).ok_or(HlcError::Overflow)?;
        self.last = Some((millis, counter));
        Ok(version)
    }

    /// `Err(ClockAhead)` if `remote` lies more than [`MAX_CLOCK_DRIFT_MS`] ahead of the wall
    /// clock; such a version must not be merged yet.
    pub fn check(&self, remote: &Hlc) -> Result<(), HlcError> {
        let ahead_ms = remote.millis - self.wall_ms();
        if ahead_ms > MAX_CLOCK_DRIFT_MS {
            return Err(HlcError::ClockAhead { ahead_ms });
        }
        Ok(())
    }

    /// Takes a remote version into account (the receive rule of the HLC algorithm), so every
    /// later [`HlcClock::now`] is greater than `remote`. Refuses versions beyond the drift bound
    /// without changing the clock.
    pub fn observe(&mut self, remote: &Hlc) -> Result<(), HlcError> {
        self.check(remote)?;
        let wall = self.wall_ms();
        let (last, counter) = self.last.unwrap_or((-1, 0));
        let millis = last.max(remote.millis).max(wall);
        let next = if millis == last && millis == remote.millis {
            counter.max(remote.counter).checked_add(1)
        } else if millis == last {
            counter.checked_add(1)
        } else if millis == remote.millis {
            remote.counter.checked_add(1)
        } else {
            Some(0)
        };
        self.last = Some((millis, next.ok_or(HlcError::Overflow)?));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    #[derive(Debug, Default)]
    struct ManualClock(AtomicI64);

    impl Clock for ManualClock {
        fn now_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn device(n: u128) -> DeviceId {
        DeviceId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn clock_at(ms: i64) -> (Arc<ManualClock>, HlcClock) {
        let manual = Arc::new(ManualClock(AtomicI64::new(ms)));
        let hlc = HlcClock::new(Arc::clone(&manual) as Arc<dyn Clock>, device(1));
        (manual, hlc)
    }

    fn v(millis: i64, counter: u32, dev: u128) -> Hlc {
        Hlc::new(millis, counter, device(dev)).expect("valid")
    }

    #[test]
    fn now_follows_the_wall_clock_and_never_runs_backwards() {
        let (manual, mut hlc) = clock_at(1_000);
        assert_eq!(hlc.now().expect("now"), v(1_000, 0, 1));
        assert_eq!(hlc.now().expect("now"), v(1_000, 1, 1));
        manual.0.store(2_000, Ordering::SeqCst);
        assert_eq!(hlc.now().expect("now"), v(2_000, 0, 1));
        // The wall clock jumps back (NTP correction): versions keep growing.
        manual.0.store(500, Ordering::SeqCst);
        assert_eq!(hlc.now().expect("now"), v(2_000, 1, 1));
        assert_eq!(hlc.now().expect("now"), v(2_000, 2, 1));
    }

    #[test]
    fn observe_orders_later_changes_after_the_remote_version() {
        // Receive rule, all four cases.
        let (_, mut hlc) = clock_at(1_000);
        hlc.observe(&v(1_200, 7, 2))
            .expect("remote ahead of wall and last");
        assert_eq!(hlc.last(), Some(v(1_200, 8, 1)));
        hlc.observe(&v(1_200, 9, 2))
            .expect("equal time: max counter + 1");
        assert_eq!(hlc.last(), Some(v(1_200, 10, 1)));
        hlc.observe(&v(900, 50, 2))
            .expect("older remote: own counter + 1");
        assert_eq!(hlc.last(), Some(v(1_200, 11, 1)));
        let (manual, mut hlc) = clock_at(1_000);
        hlc.observe(&v(900, 3, 2))
            .expect("wall clock ahead of both");
        assert_eq!(hlc.last(), Some(v(1_000, 0, 1)));
        manual.0.store(1_000, Ordering::SeqCst);
        let mine = hlc.now().expect("now");
        assert!(mine > v(900, 3, 2));

        let (_, mut hlc) = clock_at(1_000);
        let remote = v(1_000, 41, 9);
        hlc.observe(&remote).expect("observe");
        assert!(hlc.now().expect("now") > remote);
    }

    #[test]
    fn versions_are_totally_ordered_by_time_counter_device() {
        let mut list = vec![v(2, 0, 1), v(1, 5, 2), v(1, 5, 1), v(1, 0, 9), v(2, 0, 0)];
        list.sort();
        assert_eq!(
            list,
            vec![v(1, 0, 9), v(1, 5, 1), v(1, 5, 2), v(2, 0, 0), v(2, 0, 1)]
        );
        // The text form sorts the same way.
        let mut text: Vec<String> = list.iter().map(ToString::to_string).collect();
        let sorted = text.clone();
        text.sort();
        assert_eq!(text, sorted);
    }

    #[test]
    fn versions_far_in_the_future_are_refused_without_moving_the_clock() {
        let (manual, mut hlc) = clock_at(1_000_000);
        hlc.now().expect("now");
        let before = hlc.last();
        let far = v(1_000_000 + MAX_CLOCK_DRIFT_MS + 1, 0, 2);
        assert_eq!(
            hlc.observe(&far),
            Err(HlcError::ClockAhead {
                ahead_ms: MAX_CLOCK_DRIFT_MS + 1
            })
        );
        assert_eq!(hlc.last(), before);
        // Within the bound it is accepted.
        hlc.observe(&v(1_000_000 + MAX_CLOCK_DRIFT_MS, 0, 2))
            .expect("at the bound");
        // Deferred, not lost: once the local clock catches up, the far version is accepted.
        manual.0.store(1_000_001, Ordering::SeqCst);
        hlc.observe(&far).expect("caught up");
    }

    #[test]
    fn resume_continues_after_the_persisted_version() {
        let manual = Arc::new(ManualClock(AtomicI64::new(10)));
        let mut hlc = HlcClock::resume(
            Arc::clone(&manual) as Arc<dyn Clock>,
            device(1),
            Some(v(5_000, 3, 7)),
        );
        assert_eq!(hlc.now().expect("now"), v(5_000, 4, 1));
    }

    #[test]
    fn counter_overflow_is_an_error() {
        let manual = Arc::new(ManualClock(AtomicI64::new(10)));
        let mut hlc = HlcClock::resume(
            Arc::clone(&manual) as Arc<dyn Clock>,
            device(1),
            Some(v(10, u32::MAX, 1)),
        );
        assert_eq!(hlc.now(), Err(HlcError::Overflow));
    }

    #[test]
    fn text_form_round_trips_and_rejects_variants() {
        let version = v(
            1_764_000_000_000,
            0x2a,
            0x6f96_19ff_8b86_4d11_b42d_00c0_4fc9_64ff,
        );
        let text = version.to_string();
        assert_eq!(
            text,
            "1764000000000-0000002a-6f9619ff-8b86-4d11-b42d-00c04fc964ff"
        );
        assert_eq!(text.parse::<Hlc>().expect("parse"), version);
        let json = serde_json::to_string(&version).expect("json");
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<Hlc>(&json).expect("json"), version);
        for bad in [
            "",
            "1764000000000-0000002a",
            "176400000000-0000002a-6f9619ff-8b86-4d11-b42d-00c04fc964ff",
            "1764000000000-0000002A-6f9619ff-8b86-4d11-b42d-00c04fc964ff",
            "1764000000000-0000002a-6F9619FF-8B86-4D11-B42D-00C04FC964FF",
            "1764000000000-0000002a-6f9619ff8b864d11b42d00c04fc964ff",
            "+764000000000-0000002a-6f9619ff-8b86-4d11-b42d-00c04fc964ff",
        ] {
            assert!(bad.parse::<Hlc>().is_err(), "{bad:?}");
        }
        assert!(Hlc::new(-1, 0, device(1)).is_none());
        assert!(Hlc::new(MAX_MILLIS + 1, 0, device(1)).is_none());
    }
}
