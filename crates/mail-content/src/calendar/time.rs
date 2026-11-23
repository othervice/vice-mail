//! DATE, DATE-TIME, DURATION and UTC-OFFSET values, `VTIMEZONE` definitions, and the order in
//! which a `TZID` is resolved (see the module documentation of `calendar`).

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::Offset;
use mail_model::CalendarProblemCode;

use super::Problems;
use super::ZoneResolver;
use super::lines::short;
use super::rrule::{Freq, Rule, yearly_onsets};
use super::windows::windows_zone;

/// A DATE or DATE-TIME value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Value {
    Date(Date),
    /// A wall-clock time: floating or in a `TZID` (`utc: false`), or UTC (`Z`).
    DateTime {
        local: DateTime,
        utc: bool,
    },
}

/// `20261201`, `20261201T100000`, `20261201T100000Z`; also `20261201T1000` and ISO 8601 with
/// separators (`2026-12-01T10:00:00Z`), which some producers write. Of a list (`RDATE`,
/// `EXDATE`) only the first value. A leap second (`60`) counts as `59`.
pub(super) fn parse_value(text: &str) -> Option<Value> {
    let text = text.trim();
    let text = text.split(',').next().unwrap_or_default().trim();
    let bytes = text.as_bytes();
    let compact: String;
    let text = if bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-') {
        compact = text.chars().filter(|c| !matches!(c, '-' | ':')).collect();
        compact.as_str()
    } else {
        text
    };
    let (text, utc) = match text.strip_suffix(['Z', 'z']) {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    let year = i16::try_from(number(text.get(0..4)?)?).ok()?;
    let month = i8::try_from(number(text.get(4..6)?)?).ok()?;
    let day = i8::try_from(number(text.get(6..8)?)?).ok()?;
    let date = Date::new(year, month, day).ok()?;
    let rest = text.get(8..)?;
    if rest.is_empty() {
        return (!utc).then_some(Value::Date(date));
    }
    let time = rest.strip_prefix(['T', 't'])?;
    let second = match time.len() {
        6 => number(time.get(4..6)?)?,
        4 => 0,
        _ => return None,
    };
    let hour = i8::try_from(number(time.get(0..2)?)?).ok()?;
    let minute = i8::try_from(number(time.get(2..4)?)?).ok()?;
    let second = i8::try_from(second.min(59)).ok()?;
    let time = Time::new(hour, minute, second, 0).ok()?;
    Some(Value::DateTime {
        local: date.to_datetime(time),
        utc,
    })
}

/// A run of ASCII digits.
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A `DURATION`: nominal days (weeks included) and exact seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Duration {
    pub days: i64,
    pub seconds: i64,
}

/// Largest number accepted per duration component (about 27,000 years of days).
const MAX_DURATION_PART: i64 = 10_000_000;

/// `P1W`, `P2D`, `PT1H30M`, `P1DT12H`, `+PT15M`. `None` for anything else, also for a negative
/// duration (an event cannot end before it starts).
pub(super) fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim().to_ascii_uppercase();
    let text = text.strip_prefix('+').unwrap_or(&text);
    if text.starts_with('-') {
        return None;
    }
    let mut rest = text.strip_prefix('P')?;
    let mut duration = Duration {
        days: 0,
        seconds: 0,
    };
    let mut in_time = false;
    let mut any = false;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('T') {
            if in_time {
                return None;
            }
            in_time = true;
            rest = after;
            continue;
        }
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let value = i64::from(number(rest.get(..digits)?)?);
        if value > MAX_DURATION_PART {
            return None;
        }
        let unit = rest.get(digits..digits + 1)?;
        match (in_time, unit) {
            (false, "W") => duration.days += value * 7,
            (false, "D") => duration.days += value,
            (true, "H") => duration.seconds += value * 3600,
            (true, "M") => duration.seconds += value * 60,
            (true, "S") => duration.seconds += value,
            _ => return None,
        }
        any = true;
        rest = rest.get(digits + 1..)?;
    }
    any.then_some(duration)
}

/// `+0100`, `-0500`, `+053000`; also `+01:00` and `+01`. Seconds east of UTC, below 24 hours.
pub(super) fn parse_offset(text: &str) -> Option<i32> {
    let text = text.trim();
    let (sign, digits) = match text.get(..1)? {
        "+" => (1, text.get(1..)?),
        "-" => (-1, text.get(1..)?),
        _ => return None,
    };
    let digits: String = digits.chars().filter(|c| *c != ':').collect();
    let (hours, minutes, seconds) = match digits.len() {
        2 => (number(&digits)?, 0, 0),
        4 => (number(digits.get(..2)?)?, number(digits.get(2..)?)?, 0),
        6 => (
            number(digits.get(..2)?)?,
            number(digits.get(2..4)?)?,
            number(digits.get(4..)?)?,
        ),
        _ => return None,
    };
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let total = i32::try_from(hours * 3600 + minutes * 60 + seconds).ok()?;
    Some(sign * total)
}

/// Seconds since the Unix epoch of a wall-clock time read as UTC.
pub(super) fn naive_seconds(local: DateTime) -> Option<i64> {
    Offset::UTC.to_timestamp(local).ok().map(|t| t.as_second())
}

/// `VTIMEZONE` observances kept per definition, and `RDATE` values per observance.
pub(super) const MAX_OBSERVANCES: usize = 32;
pub(super) const MAX_RDATES: usize = 64;

/// A `VTIMEZONE` of the file.
#[derive(Debug, Default)]
pub(super) struct ZoneDefinition {
    pub tzid: String,
    pub observances: Vec<Observance>,
}

/// A `STANDARD` or `DAYLIGHT` observance.
#[derive(Debug, Default)]
pub(super) struct Observance {
    /// First onset, local time in `from`.
    pub start: Option<DateTime>,
    pub from: Option<i32>,
    pub to: Option<i32>,
    pub rule: Option<Rule>,
    pub rdates: Vec<Value>,
}

/// From `at` (Unix seconds) on, the offset is `to` instead of `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Transition {
    at: i64,
    from: i32,
    to: i32,
}

impl ZoneDefinition {
    /// The onsets that matter for wall-clock times in `year`: every `DTSTART` and `RDATE`, and
    /// the onsets of yearly rules in the years around `year` (clamped to the years the rule is
    /// in force, so a rule that ended still gives its last onsets). Sorted, one per instant.
    pub fn transitions(&self, year: i16) -> Vec<Transition> {
        let mut out = Vec::new();
        for observance in &self.observances {
            let (Some(start), Some(from), Some(to)) =
                (observance.start, observance.from, observance.to)
            else {
                continue;
            };
            let onset = |local: DateTime| naive_seconds(local).map(|n| n - i64::from(from));
            let instant = |value: Value| match value {
                Value::Date(date) => onset(date.to_datetime(Time::midnight())),
                Value::DateTime { local, utc: true } => naive_seconds(local),
                Value::DateTime { local, utc: false } => onset(local),
            };
            let mut push = |at: Option<i64>| {
                if let Some(at) = at {
                    out.push(Transition { at, from, to });
                }
            };
            push(onset(start));
            for &rdate in &observance.rdates {
                push(instant(rdate));
            }
            let Some(rule) = observance
                .rule
                .as_ref()
                .filter(|r| r.freq == Some(Freq::Yearly))
            else {
                continue;
            };
            let until = rule.until.as_deref().and_then(parse_value);
            let until_instant = until.and_then(|value| match value {
                Value::Date(date) => onset(date.to_datetime(Time::constant(23, 59, 59, 0))),
                other => instant(other),
            });
            let first = i32::from(start.year());
            let mut last = 9999;
            if let Some(count) = rule.count {
                let count = i32::try_from(count).unwrap_or(i32::MAX);
                last = last.min(first.saturating_add(count).saturating_sub(1));
            }
            if let Some(until) = until {
                let year = match until {
                    Value::Date(date) => date.year(),
                    Value::DateTime { local, .. } => local.year(),
                };
                last = last.min(i32::from(year));
            }
            if last < first {
                continue;
            }
            let center = i32::from(year).clamp(first, last);
            for y in (center - 1).max(first)..=(center + 1).min(last) {
                let Ok(y) = i16::try_from(y) else { continue };
                for local in yearly_onsets(rule, start, y) {
                    if local <= start {
                        continue;
                    }
                    let at = onset(local);
                    if until_instant.is_some_and(|u| at.is_some_and(|a| a > u)) {
                        continue;
                    }
                    push(at);
                }
            }
        }
        out.sort_by_key(|t| t.at);
        out.dedup_by_key(|t| t.at);
        out
    }
}

/// The instant of a wall-clock time under these transitions. A time that occurs twice (clocks
/// set back) gives the earlier instant; a time that does not occur (clocks set forward) is read
/// with the offset before the gap, so 02:30 becomes 03:30 (RFC 5545 section 3.3.5; jiff calls
/// both "compatible"). Before the first onset its `TZOFFSETFROM` applies.
pub(super) fn local_to_utc(transitions: &[Transition], local: DateTime) -> Option<i64> {
    let target = naive_seconds(local)?;
    let first = transitions.first()?;
    let mut best: Option<i64> = None;
    let before_first = target - i64::from(first.from);
    if before_first < first.at {
        best = Some(before_first);
    }
    for (index, transition) in transitions.iter().enumerate() {
        let instant = target - i64::from(transition.to);
        let next = transitions.get(index + 1).map(|n| n.at);
        if instant >= transition.at && next.is_none_or(|n| instant < n) {
            best = Some(best.map_or(instant, |b| b.min(instant)));
        }
    }
    if best.is_some() {
        return best;
    }
    transitions
        .iter()
        .find(|t| target - i64::from(t.from) >= t.at && target - i64::from(t.to) < t.at)
        .map(|t| target - i64::from(t.from))
}

/// An instant and how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Instant {
    /// Unix seconds.
    pub seconds: i64,
    pub zone: Option<String>,
    pub floating: bool,
}

/// How a `TZID` is resolved, decided once per name.
#[derive(Debug, Clone)]
enum Choice {
    Utc,
    /// An IANA name the resolver knows.
    Named(String),
    /// The file's own `VTIMEZONE` with this index.
    Defined(usize),
    Unknown,
}

/// Names that mean UTC, whatever the resolver knows.
const UTC_NAMES: &[&str] = &[
    "UTC",
    "UT",
    "GMT",
    "Z",
    "Zulu",
    "Universal",
    "Etc/UTC",
    "Etc/UCT",
    "Etc/GMT",
    "Etc/Universal",
    "Etc/Zulu",
];

/// Longest zone name passed to the resolver.
const MAX_ZONE_NAME: usize = 64;

/// Resolves wall-clock times of one file.
pub(super) struct Zones<'a> {
    resolver: &'a dyn ZoneResolver,
    definitions: &'a [ZoneDefinition],
    /// `X-WR-TIMEZONE`, for floating times.
    default_zone: Option<String>,
    choices: Vec<(String, Choice)>,
    transitions: Vec<((usize, i16), Vec<Transition>)>,
}

impl<'a> Zones<'a> {
    pub fn new(
        resolver: &'a dyn ZoneResolver,
        definitions: &'a [ZoneDefinition],
        default_zone: Option<String>,
    ) -> Self {
        Self {
            resolver,
            definitions,
            default_zone,
            choices: Vec::new(),
            transitions: Vec::new(),
        }
    }

    /// The instant of a wall-clock time in `tzid`, or floating when `tzid` is `None`. An unknown
    /// `TZID` is reported and read as floating.
    pub fn resolve(
        &mut self,
        local: DateTime,
        tzid: Option<&str>,
        problems: &mut Problems,
    ) -> Instant {
        match tzid.map(str::trim).filter(|t| !t.is_empty()) {
            Some(tzid) => {
                if let Some(instant) = self.in_zone(tzid, local) {
                    return instant;
                }
                problems.push(
                    CalendarProblemCode::UnknownTimeZone,
                    Some(short(tzid, MAX_ZONE_NAME)),
                );
            }
            None => {
                if let Some(zone) = self.default_zone.clone()
                    && let Some(instant) = self.in_zone(&zone, local)
                {
                    return instant;
                }
            }
        }
        Instant {
            seconds: self
                .resolver
                .to_utc(None, local)
                .or_else(|| naive_seconds(local))
                .unwrap_or_default(),
            zone: None,
            floating: true,
        }
    }

    fn in_zone(&mut self, tzid: &str, local: DateTime) -> Option<Instant> {
        let choice = match self.choices.iter().find(|(name, _)| name == tzid) {
            Some((_, choice)) => choice.clone(),
            None => {
                let choice = self.choose(tzid, local);
                if self.choices.len() < 64 {
                    self.choices.push((tzid.to_owned(), choice.clone()));
                }
                choice
            }
        };
        let (seconds, zone) = match choice {
            Choice::Utc => (naive_seconds(local)?, "UTC".to_owned()),
            Choice::Named(name) => (self.resolver.to_utc(Some(&name), local)?, name),
            Choice::Defined(index) => {
                let transitions = self.transitions(index, local.year())?;
                (
                    local_to_utc(transitions, local)?,
                    short(tzid, MAX_ZONE_NAME),
                )
            }
            Choice::Unknown => return None,
        };
        Some(Instant {
            seconds,
            zone: Some(zone),
            floating: false,
        })
    }

    /// Resolution order: UTC names; the resolver with the name as written, then without a
    /// vendor prefix; the Windows table; the file's `VTIMEZONE`.
    fn choose(&mut self, tzid: &str, local: DateTime) -> Choice {
        if UTC_NAMES.iter().any(|n| n.eq_ignore_ascii_case(tzid)) {
            return Choice::Utc;
        }
        for candidate in iana_candidates(tzid) {
            if self.resolver.to_utc(Some(candidate), local).is_some() {
                return Choice::Named(candidate.to_owned());
            }
        }
        if let Some(iana) = windows_zone(tzid)
            && self.resolver.to_utc(Some(iana), local).is_some()
        {
            return Choice::Named(iana.to_owned());
        }
        let index = self
            .definitions
            .iter()
            .position(|d| d.tzid == tzid)
            .or_else(|| {
                self.definitions
                    .iter()
                    .position(|d| d.tzid.trim().eq_ignore_ascii_case(tzid))
            });
        if let Some(index) = index
            && self
                .transitions(index, local.year())
                .is_some_and(|t| local_to_utc(t, local).is_some())
        {
            return Choice::Defined(index);
        }
        Choice::Unknown
    }

    fn transitions(&mut self, index: usize, year: i16) -> Option<&[Transition]> {
        let key = (index, year);
        let position = match self.transitions.iter().position(|(k, _)| *k == key) {
            Some(position) => position,
            None => {
                let built = self.definitions.get(index)?.transitions(year);
                if self.transitions.len() >= 128 {
                    self.transitions.clear();
                }
                self.transitions.push((key, built));
                self.transitions.len() - 1
            }
        };
        self.transitions
            .get(position)
            .map(|(_, t)| t.as_slice())
            .filter(|t| !t.is_empty())
    }
}

/// Names to ask the resolver for: the name as written if it looks like an IANA name, then for a
/// vendor-prefixed name (`/mozilla.org/20050126_1/Europe/Berlin`, `/citadel.org/…/`,
/// `/freeassociation.sourceforge.net/Tzfile/…`) its tails, longest first. The resolver only
/// ever sees plausible names: ASCII letters, digits, `/`, `_`, `+`, `-`, starting with a
/// letter, no `.`, no empty segment, at most 64 bytes.
fn iana_candidates(tzid: &str) -> Vec<&str> {
    let mut out = Vec::new();
    if plausible_iana(tzid) {
        out.push(tzid);
    }
    if tzid.starts_with('/') {
        for (index, _) in tzid.match_indices('/') {
            let Some(tail) = tzid.get(index + 1..) else {
                continue;
            };
            if plausible_iana(tail) && !out.contains(&tail) {
                out.push(tail);
            }
            if out.len() >= 4 {
                break;
            }
        }
    }
    out
}

fn plausible_iana(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ZONE_NAME
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
        && name.split('/').all(|segment| !segment.is_empty())
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    #[test]
    fn values() {
        let dt = |y, m, d, h, mi, s| date(y, m, d).at(h, mi, s, 0);
        assert_eq!(
            parse_value("20261201"),
            Some(Value::Date(date(2026, 12, 1)))
        );
        assert_eq!(
            parse_value("20261201T101500Z"),
            Some(Value::DateTime {
                local: dt(2026, 12, 1, 10, 15, 0),
                utc: true
            })
        );
        assert_eq!(
            parse_value(" 20261201t1015 "),
            Some(Value::DateTime {
                local: dt(2026, 12, 1, 10, 15, 0),
                utc: false
            })
        );
        assert_eq!(
            parse_value("2026-12-01T10:15:30Z"),
            Some(Value::DateTime {
                local: dt(2026, 12, 1, 10, 15, 30),
                utc: true
            })
        );
        assert_eq!(
            parse_value("20261231T235960Z"),
            Some(Value::DateTime {
                local: dt(2026, 12, 31, 23, 59, 59),
                utc: true
            })
        );
        assert_eq!(
            parse_value("20260101,20260102"),
            Some(Value::Date(date(2026, 1, 1)))
        );
        for bad in [
            "20261332T250000",
            "20260230",
            "20261201T240000",
            "20261201Z",
            "2026120",
            "20261201T10",
            "2026120aT100000",
            "",
            "ääääääääää",
            "+2026-12-01",
        ] {
            assert_eq!(parse_value(bad), None, "{bad}");
        }
    }

    #[test]
    fn durations() {
        let d = |days, seconds| Some(Duration { days, seconds });
        assert_eq!(parse_duration("PT1H30M"), d(0, 5400));
        assert_eq!(parse_duration("P1W"), d(7, 0));
        assert_eq!(parse_duration("+P1DT12H"), d(1, 43_200));
        assert_eq!(parse_duration("pt15m"), d(0, 900));
        assert_eq!(parse_duration("P0D"), d(0, 0));
        for bad in [
            "-PT1H",
            "P",
            "PT",
            "1H",
            "PT1D",
            "P1H",
            "PT1H1",
            "P99999999999D",
            "PTT1H",
        ] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0500"), Some(-18_000));
        assert_eq!(parse_offset("+053000"), Some(19_800));
        assert_eq!(parse_offset("+05:45"), Some(20_700));
        assert_eq!(parse_offset("-03"), Some(-10_800));
        for bad in ["0100", "+2400", "+0160", "+1", "", "+01000", "+ab00"] {
            assert_eq!(parse_offset(bad), None, "{bad}");
        }
    }

    #[test]
    fn candidates_for_prefixed_names() {
        assert_eq!(iana_candidates("Europe/Berlin"), ["Europe/Berlin"]);
        assert_eq!(
            iana_candidates("/mozilla.org/20050126_1/Europe/Berlin"),
            ["Europe/Berlin", "Berlin"]
        );
        assert_eq!(
            iana_candidates("/freeassociation.sourceforge.net/Tzfile/America/New_York"),
            ["Tzfile/America/New_York", "America/New_York", "New_York"]
        );
        assert!(iana_candidates("W. Europe Standard Time").is_empty());
        assert!(iana_candidates("../../etc/passwd").is_empty());
        assert!(iana_candidates("Europe//Berlin").is_empty());
        assert!(iana_candidates("/").is_empty());
    }
}
