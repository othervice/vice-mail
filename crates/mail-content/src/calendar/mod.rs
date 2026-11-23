//! Calendar invitations (stage 9): an iCalendar reader (RFC 5545, the part Outlook, Exchange,
//! Teams, Google Calendar, Apple Calendar, Thunderbird and Zoom send) that turns a
//! `text/calendar` part or an `.ics` attachment into a [`CalendarInvite`] for the invitation
//! card. Pure: no IO, no clock, no randomness; the time-zone database is behind
//! [`ZoneResolver`], which the engine implements. Read only: nothing is answered, written or
//! expanded (recurrences are described, see [`CalendarRecurrence`]).
//!
//! - [`calendar_parts`] finds the calendar parts of a message; [`read_calendar`] reads one.
//! - Never panics and never fails: what cannot be read is left out and reported as a
//!   [`CalendarProblem`]; an unreadable file gives no events and `not_calendar`. A
//!   `catch_unwind` guard turns a bug into the `internal` problem.
//! - Hard limits keep hostile files cheap: [`MAX_CALENDAR_BYTES`] of input (cut at the last line
//!   break before it), [`MAX_CALENDAR_LINES`] content lines, [`MAX_CALENDAR_LINE_BYTES`] per
//!   unfolded line, [`MAX_CALENDAR_EVENTS`] events, [`MAX_CALENDAR_ATTENDEES`] attendees per
//!   event, nesting depth 8, 32 `VTIMEZONE`s with 32 observances and 64 `RDATE`s each, 32
//!   problems. Work is linear in the input.
//!
//! **Time zones.** A `TZID` is resolved in this order, once per name and file:
//! 1. names that mean UTC (`UTC`, `GMT`, `Etc/UTC`, …);
//! 2. the [`ZoneResolver`] with the name as written (IANA names), then with the tails of a
//!    vendor-prefixed name (`/mozilla.org/20050126_1/Europe/Berlin` → `Europe/Berlin`;
//!    `/citadel.org/…`, `/freeassociation.sourceforge.net/…`);
//! 3. the Windows zone names of Outlook and Exchange (`W. Europe Standard Time`, CLDR
//!    `windowsZones`) mapped to IANA, then the resolver;
//! 4. the file's own `VTIMEZONE` with that `TZID`: its `STANDARD`/`DAYLIGHT` observances
//!    (`DTSTART`, `TZOFFSETFROM`, `TZOFFSETTO`, yearly `RRULE` with `BYMONTH` and `BYDAY` such as
//!    `-1SU` or `2SU`, or `BYMONTHDAY`, and `RDATE`) give the offset at that wall-clock time;
//! 5. otherwise `unknown_time_zone`, and the time is read as floating.
//!
//! The system database comes before the file's `VTIMEZONE` on purpose: Outlook's definitions
//! carry only the current rule (dates before a rule change come out wrong), and the database
//! knows more than any file. A wall-clock time that occurs twice (clocks set back) gives the
//! earlier instant; one that does not exist (clocks set forward) is read with the offset before
//! the gap, so 02:30 becomes 03:30. That is RFC 5545 section 3.3.5 and what jiff calls
//! "compatible"; resolvers must do the same.
//!
//! Floating times (no `TZID`, no `Z`) are read in the user's own zone (`tzid: None`), except
//! when the calendar has `X-WR-TIMEZONE` (Google, Apple) and that zone resolves: then in that
//! zone. Floating times in invitations are a producer's mistake; the producer's calendar zone is
//! the better guess for what was meant, and Apple Calendar and Google Calendar read them the
//! same way. A `TZID` always wins over `X-WR-TIMEZONE`.
//!
//! Outlook's older all-day events (`X-MICROSOFT-CDO-ALLDAYEVENT:TRUE` with local midnight
//! date-times) become dates.

mod lines;
mod parts;
mod rrule;
mod time;
mod windows;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::panic::{self, AssertUnwindSafe};

use jiff::civil::{DateTime, Time};
use jiff::{SignedDuration, Span};
use mail_model::{
    AttendeeRole, CalendarEvent, CalendarInvite, CalendarPerson, CalendarProblem,
    CalendarProblemCode, CalendarRecurrence, CalendarTime, EventStatus, ParticipationStatus,
    RecurrenceFrequency,
};

use lines::{Line, RawLine, Unfolder, clean_block, clean_line, parse_line, short, unescape_text};
pub use parts::{CalendarPart, MAX_CALENDAR_PARTS, calendar_parts};
use rrule::{Freq, MAX_RULE_BYTES, parse_rule, weekday_code};
use time::{
    MAX_OBSERVANCES, MAX_RDATES, Observance, Value, ZoneDefinition, Zones, naive_seconds,
    parse_duration, parse_offset, parse_value,
};

/// Input read per calendar file; longer input is cut (`too_large`).
pub const MAX_CALENDAR_BYTES: usize = 1 << 20;
/// Content lines read per file; the rest is skipped (`too_large`).
pub const MAX_CALENDAR_LINES: usize = 100_000;
/// Longest unfolded content line kept; longer lines are cut (`line_too_long` for the
/// properties that are read; `ATTACH` and `X-ALT-DESC` may be long without a problem).
pub const MAX_CALENDAR_LINE_BYTES: usize = 64 << 10;
/// Events read per file; further events are counted (`too_many_events`).
pub const MAX_CALENDAR_EVENTS: usize = 20;
/// Attendees kept per event; all are counted in `attendee_count`.
pub const MAX_CALENDAR_ATTENDEES: usize = 50;
/// Longest description kept (then cut, ending in `…`).
pub const MAX_CALENDAR_DESCRIPTION_BYTES: usize = 4 << 10;

/// Summary and location.
const MAX_TEXT_BYTES: usize = 1024;
const MAX_UID_BYTES: usize = 256;
const MAX_NAME_BYTES: usize = 256;
const MAX_EMAIL_BYTES: usize = 254;
/// Raw values kept for later parsing (dates, durations, TZIDs).
const MAX_VALUE_BYTES: usize = 256;
const MAX_METHOD_BYTES: usize = 32;
const MAX_DEPTH: usize = 8;
const MAX_ZONES: usize = 32;
const MAX_PROBLEMS: usize = 32;
const MAX_DETAIL_BYTES: usize = 64;

/// Turns a wall-clock time into an instant with the system's time-zone database (the engine
/// implements it with jiff). Called with plausible IANA names only (ASCII letters, digits, `/`,
/// `_`, `+`, `-`, starting with a letter, no `.`, at most 64 bytes), so it never sees a path.
pub trait ZoneResolver {
    /// Unix seconds of `local` in zone `tzid` (an IANA name such as `America/New_York`), or in
    /// the user's own zone when `tzid` is `None` (floating times). `None` when the zone is
    /// unknown. A time that occurs twice gives the earlier instant, one in a gap is read with
    /// the offset before the gap (jiff: `to_ambiguous_timestamp(local).compatible()`).
    fn to_utc(&self, tzid: Option<&str>, local: DateTime) -> Option<i64>;
}

impl<F> ZoneResolver for F
where
    F: Fn(Option<&str>, DateTime) -> Option<i64>,
{
    fn to_utc(&self, tzid: Option<&str>, local: DateTime) -> Option<i64> {
        self(tzid, local)
    }
}

/// Reads a calendar file (UTF-8; other bytes become U+FFFD). Never panics; see the module
/// documentation for limits and time zones.
pub fn read_calendar(data: &[u8], zones: &dyn ZoneResolver) -> CalendarInvite {
    panic::catch_unwind(AssertUnwindSafe(|| read(data, zones))).unwrap_or_else(|_| CalendarInvite {
        method: None,
        events: Vec::new(),
        problems: vec![CalendarProblem {
            code: CalendarProblemCode::Internal,
            detail: None,
        }],
    })
}

fn read(data: &[u8], resolver: &dyn ZoneResolver) -> CalendarInvite {
    let mut reader = Reader::default();
    let mut data = data;
    if data.len() > MAX_CALENDAR_BYTES {
        reader.problems.push(
            CalendarProblemCode::TooLarge,
            Some(format!("{} bytes", data.len())),
        );
        let cut = data.get(..MAX_CALENDAR_BYTES).unwrap_or_default();
        // Up to the last line break, so no half line (or half character) is read.
        data = match cut.iter().rposition(|&b| b == b'\n') {
            Some(end) => cut.get(..=end).unwrap_or_default(),
            None => cut,
        };
    }
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    for (index, line) in Unfolder::new(data, MAX_CALENDAR_LINE_BYTES).enumerate() {
        if index >= MAX_CALENDAR_LINES {
            reader.problems.push(
                CalendarProblemCode::TooLarge,
                Some(format!("{MAX_CALENDAR_LINES} lines")),
            );
            break;
        }
        reader.line(&line);
    }
    reader.end_of_input();
    reader.build(resolver)
}

/// Problems found so far: distinct, capped.
#[derive(Debug, Default)]
struct Problems(Vec<CalendarProblem>);

impl Problems {
    fn push(&mut self, code: CalendarProblemCode, detail: Option<String>) {
        if self.0.len() >= MAX_PROBLEMS {
            return;
        }
        let detail = detail
            .map(|d| short(&d, MAX_DETAIL_BYTES))
            .filter(|d| !d.is_empty());
        if !self.0.iter().any(|p| p.code == code && p.detail == detail) {
            self.0.push(CalendarProblem { code, detail });
        }
    }
}

/// What a component on the stack is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Calendar,
    Event,
    Zone,
    Observance,
    /// Everything else (`VALARM`, `VTODO`, unknown components, events over the limit): skipped.
    Other,
}

#[derive(Debug)]
struct Frame {
    name: String,
    kind: Kind,
}

/// A date property as written, parsed when the whole file (all `VTIMEZONE`s) is known.
#[derive(Debug, Default)]
struct RawTime {
    value: String,
    tzid: Option<String>,
    /// `VALUE=DATE`.
    date_only: bool,
}

#[derive(Debug, Default)]
struct RawEvent {
    uid: Option<String>,
    summary: Option<String>,
    location: Option<String>,
    description: Option<String>,
    start: Option<RawTime>,
    end: Option<RawTime>,
    duration: Option<String>,
    recurrence_id: Option<RawTime>,
    rrule: Option<String>,
    organizer: Option<CalendarPerson>,
    attendees: Vec<CalendarPerson>,
    attendee_count: u32,
    status: Option<EventStatus>,
    sequence: Option<u32>,
    /// `X-MICROSOFT-CDO-ALLDAYEVENT:TRUE`.
    all_day: bool,
}

#[derive(Debug, Default)]
struct Reader {
    stack: Vec<Frame>,
    /// `BEGIN`s beyond the depth limit not yet closed.
    overflow: usize,
    saw_calendar: bool,
    method: Option<String>,
    default_zone: Option<String>,
    events: Vec<RawEvent>,
    events_seen: usize,
    event: Option<RawEvent>,
    zones: Vec<ZoneDefinition>,
    zone: Option<ZoneDefinition>,
    observance: Option<Observance>,
    problems: Problems,
}

impl Reader {
    fn line(&mut self, raw: &RawLine) {
        let text = String::from_utf8_lossy(&raw.bytes);
        if matches!(text, Cow::Owned(_)) {
            self.problems.push(CalendarProblemCode::MalformedText, None);
        }
        if text.trim().is_empty() {
            return;
        }
        let Some(line) = parse_line(&text) else {
            self.problems.push(CalendarProblemCode::BrokenLine, None);
            return;
        };
        match line.name.as_str() {
            "BEGIN" => self.begin(line.value),
            "END" => self.end(line.value),
            _ => {
                if self.property(&line) && raw.cut {
                    self.problems
                        .push(CalendarProblemCode::LineTooLong, Some(line.name.clone()));
                }
            }
        }
    }

    fn begin(&mut self, value: &str) {
        let name = value.trim().to_ascii_uppercase();
        if self.stack.len() >= MAX_DEPTH {
            self.overflow += 1;
            self.problems
                .push(CalendarProblemCode::BrokenStructure, None);
            return;
        }
        let parent = self.stack.last().map(|f| f.kind);
        let kind = match (name.as_str(), parent) {
            ("VCALENDAR", None) => {
                self.saw_calendar = true;
                Kind::Calendar
            }
            ("VEVENT", Some(Kind::Calendar) | None) => {
                if parent.is_none() {
                    self.problems
                        .push(CalendarProblemCode::BrokenStructure, None);
                }
                self.events_seen += 1;
                if self.events_seen <= MAX_CALENDAR_EVENTS {
                    self.event = Some(RawEvent::default());
                    Kind::Event
                } else {
                    Kind::Other
                }
            }
            ("VTIMEZONE", Some(Kind::Calendar) | None) if self.zones.len() < MAX_ZONES => {
                self.zone = Some(ZoneDefinition::default());
                Kind::Zone
            }
            ("STANDARD" | "DAYLIGHT", Some(Kind::Zone)) => {
                self.observance = Some(Observance::default());
                Kind::Observance
            }
            _ => Kind::Other,
        };
        self.stack.push(Frame { name, kind });
    }

    fn end(&mut self, value: &str) {
        if self.overflow > 0 {
            self.overflow -= 1;
            return;
        }
        let name = value.trim().to_ascii_uppercase();
        let Some(position) = self.stack.iter().rposition(|f| f.name == name) else {
            self.problems
                .push(CalendarProblemCode::BrokenStructure, None);
            return;
        };
        if position + 1 != self.stack.len() {
            self.problems
                .push(CalendarProblemCode::BrokenStructure, None);
        }
        while self.stack.len() > position {
            if let Some(frame) = self.stack.pop() {
                self.close(frame.kind);
            }
        }
    }

    fn close(&mut self, kind: Kind) {
        match kind {
            Kind::Event => {
                if let Some(event) = self.event.take() {
                    self.events.push(event);
                }
            }
            Kind::Zone => {
                if let Some(zone) = self.zone.take()
                    && !zone.tzid.is_empty()
                {
                    self.zones.push(zone);
                }
            }
            Kind::Observance => {
                if let Some(observance) = self.observance.take()
                    && let Some(zone) = self.zone.as_mut()
                    && zone.observances.len() < MAX_OBSERVANCES
                {
                    zone.observances.push(observance);
                }
            }
            Kind::Calendar | Kind::Other => {}
        }
    }

    /// The file ended: close what is open (a cut-off file still shows its events).
    fn end_of_input(&mut self) {
        if !self.stack.is_empty() || self.overflow > 0 {
            self.problems.push(CalendarProblemCode::Truncated, None);
        }
        while let Some(frame) = self.stack.pop() {
            self.close(frame.kind);
        }
    }

    /// Handles a property of the current component. `true` if it is one that is read.
    fn property(&mut self, line: &Line<'_>) -> bool {
        match self.stack.last().map(|f| f.kind) {
            Some(Kind::Calendar) => self.calendar_property(line),
            Some(Kind::Event) => match self.event.as_mut() {
                Some(event) => event_property(event, line),
                None => false,
            },
            Some(Kind::Zone) => match self.zone.as_mut() {
                Some(zone) if line.name == "TZID" => {
                    if zone.tzid.is_empty() {
                        zone.tzid = cut(line.value.trim(), MAX_VALUE_BYTES);
                    }
                    true
                }
                _ => false,
            },
            Some(Kind::Observance) => match self.observance.as_mut() {
                Some(observance) => observance_property(observance, line),
                None => false,
            },
            Some(Kind::Other) | None => false,
        }
    }

    fn calendar_property(&mut self, line: &Line<'_>) -> bool {
        match line.name.as_str() {
            "METHOD" => {
                if self.method.is_none() {
                    let method: String = line
                        .value
                        .trim()
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .take(MAX_METHOD_BYTES)
                        .collect::<String>()
                        .to_ascii_uppercase();
                    self.method = (!method.is_empty()).then_some(method);
                }
                true
            }
            "X-WR-TIMEZONE" => {
                if self.default_zone.is_none() {
                    let zone = cut(unescape_text(line.value).trim(), MAX_VALUE_BYTES);
                    self.default_zone = (!zone.is_empty()).then_some(zone);
                }
                true
            }
            _ => false,
        }
    }

    fn build(self, resolver: &dyn ZoneResolver) -> CalendarInvite {
        let Reader {
            saw_calendar,
            method,
            default_zone,
            events,
            events_seen,
            zones: definitions,
            mut problems,
            ..
        } = self;
        if events_seen > MAX_CALENDAR_EVENTS {
            problems.push(
                CalendarProblemCode::TooManyEvents,
                Some(events_seen.to_string()),
            );
        }
        if events.is_empty() {
            let code = if saw_calendar {
                CalendarProblemCode::NoEvent
            } else {
                CalendarProblemCode::NotCalendar
            };
            problems.push(code, None);
        }
        let mut zones = Zones::new(resolver, &definitions, default_zone);
        let context = EventContext {
            cancel: method.as_deref() == Some("CANCEL"),
            // iTIP methods whose events need no DTSTART (RFC 5546 section 3.2).
            start_optional: matches!(
                method.as_deref(),
                Some("CANCEL" | "REPLY" | "REFRESH" | "DECLINECOUNTER")
            ),
        };
        let events = events
            .into_iter()
            .map(|raw| build_event(raw, context, &mut zones, &mut problems))
            .collect();
        CalendarInvite {
            method,
            events,
            problems: problems.0,
        }
    }
}

fn set_once<T>(slot: &mut Option<T>, make: impl FnOnce() -> Option<T>) {
    if slot.is_none() {
        *slot = make();
    }
}

/// At most `max` bytes, on a character boundary.
fn cut(text: &str, max: usize) -> String {
    text.get(..text.floor_char_boundary(max))
        .unwrap_or_default()
        .to_owned()
}

fn raw_time(line: &Line<'_>) -> RawTime {
    RawTime {
        value: cut(line.value.trim(), MAX_VALUE_BYTES),
        tzid: line
            .param("TZID")
            .map(|t| cut(t.trim(), MAX_VALUE_BYTES))
            .filter(|t| !t.is_empty()),
        date_only: line
            .param("VALUE")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("DATE")),
    }
}

fn event_property(event: &mut RawEvent, line: &Line<'_>) -> bool {
    let value = line.value;
    match line.name.as_str() {
        "UID" => set_once(&mut event.uid, || {
            clean_line(&unescape_text(value), MAX_UID_BYTES)
        }),
        "SUMMARY" => set_once(&mut event.summary, || {
            clean_line(&unescape_text(value), MAX_TEXT_BYTES)
        }),
        "LOCATION" => set_once(&mut event.location, || {
            clean_line(&unescape_text(value), MAX_TEXT_BYTES)
        }),
        "DESCRIPTION" => set_once(&mut event.description, || {
            clean_block(&unescape_text(value), MAX_CALENDAR_DESCRIPTION_BYTES)
        }),
        "DTSTART" => set_once(&mut event.start, || Some(raw_time(line))),
        "DTEND" => set_once(&mut event.end, || Some(raw_time(line))),
        "RECURRENCE-ID" => set_once(&mut event.recurrence_id, || Some(raw_time(line))),
        "DURATION" => set_once(&mut event.duration, || {
            Some(cut(value.trim(), MAX_VALUE_BYTES))
        }),
        "RRULE" => set_once(&mut event.rrule, || {
            Some(cut(value.trim(), 4 * MAX_RULE_BYTES))
        }),
        "ORGANIZER" => set_once(&mut event.organizer, || Some(person(line, false))),
        "ATTENDEE" => {
            event.attendee_count = event.attendee_count.saturating_add(1);
            if event.attendees.len() < MAX_CALENDAR_ATTENDEES {
                event.attendees.push(person(line, true));
            }
        }
        "STATUS" => set_once(&mut event.status, || {
            match value.trim().to_ascii_uppercase().as_str() {
                "TENTATIVE" => Some(EventStatus::Tentative),
                "CONFIRMED" => Some(EventStatus::Confirmed),
                "CANCELLED" => Some(EventStatus::Cancelled),
                _ => None,
            }
        }),
        "SEQUENCE" => set_once(&mut event.sequence, || value.trim().parse().ok()),
        "X-MICROSOFT-CDO-ALLDAYEVENT" | "X-MICROSOFT-MSNCALENDAR-ALLDAYEVENT" => {
            event.all_day = value.trim().eq_ignore_ascii_case("TRUE");
        }
        _ => return false,
    }
    true
}

fn observance_property(observance: &mut Observance, line: &Line<'_>) -> bool {
    let value = line.value;
    match line.name.as_str() {
        "DTSTART" => set_once(&mut observance.start, || match parse_value(value)? {
            Value::Date(date) => Some(date.to_datetime(Time::midnight())),
            Value::DateTime { local, .. } => Some(local),
        }),
        "TZOFFSETFROM" => set_once(&mut observance.from, || parse_offset(value)),
        "TZOFFSETTO" => set_once(&mut observance.to, || parse_offset(value)),
        "RRULE" => set_once(&mut observance.rule, || Some(parse_rule(value))),
        "RDATE" => {
            for item in value.split(',') {
                if observance.rdates.len() >= MAX_RDATES {
                    break;
                }
                observance.rdates.extend(parse_value(item));
            }
        }
        _ => return false,
    }
    true
}

/// Organizer or attendee: `CN`, the `mailto:` address (or `EMAIL`), `ROLE`, `PARTSTAT`.
fn person(line: &Line<'_>, attendee: bool) -> CalendarPerson {
    let name = line
        .param("CN")
        .and_then(|cn| clean_line(cn, MAX_NAME_BYTES));
    let email = address(line.value).or_else(|| line.param("EMAIL").and_then(address));
    if !attendee {
        return CalendarPerson {
            name,
            email,
            role: None,
            status: None,
        };
    }
    let role =
        line.param("ROLE")
            .and_then(|role| match role.trim().to_ascii_uppercase().as_str() {
                "CHAIR" => Some(AttendeeRole::Chair),
                "REQ-PARTICIPANT" => Some(AttendeeRole::Required),
                "OPT-PARTICIPANT" => Some(AttendeeRole::Optional),
                "NON-PARTICIPANT" => Some(AttendeeRole::NonParticipant),
                _ => None,
            });
    let status = line.param("PARTSTAT").and_then(|status| {
        match status.trim().to_ascii_uppercase().as_str() {
            "NEEDS-ACTION" => Some(ParticipationStatus::NeedsAction),
            "ACCEPTED" => Some(ParticipationStatus::Accepted),
            "DECLINED" => Some(ParticipationStatus::Declined),
            "TENTATIVE" => Some(ParticipationStatus::Tentative),
            "DELEGATED" => Some(ParticipationStatus::Delegated),
            _ => None,
        }
    });
    CalendarPerson {
        name,
        email,
        role,
        status,
    }
}

/// The address of `mailto:a@b` (any case; a `?query` is dropped), or a bare `a@b`.
fn address(value: &str) -> Option<String> {
    let value = value.trim();
    let value = match value.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("mailto:") => value.get(7..)?,
        _ => value,
    };
    let value = value.split('?').next().unwrap_or_default().trim();
    let (local, domain) = value.rsplit_once('@')?;
    let valid = !local.is_empty()
        && !domain.is_empty()
        && value.len() <= MAX_EMAIL_BYTES
        && !value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | ','));
    valid.then(|| value.to_owned())
}

#[derive(Debug, Clone, Copy)]
struct EventContext {
    /// `METHOD:CANCEL`.
    cancel: bool,
    /// The method allows events without `DTSTART`.
    start_optional: bool,
}

fn build_event(
    raw: RawEvent,
    context: EventContext,
    zones: &mut Zones<'_>,
    problems: &mut Problems,
) -> CalendarEvent {
    let start_value = raw
        .start
        .as_ref()
        .and_then(|t| value_of(t, raw.all_day, problems));
    if raw.start.is_none() && !context.start_optional {
        problems.push(CalendarProblemCode::MissingStart, None);
    }
    let start_tzid = raw.start.as_ref().and_then(|t| t.tzid.as_deref());
    let start = start_value.map(|v| calendar_time(v, start_tzid, zones, problems));
    let end = match (&start_value, &start) {
        (Some(start_value), Some(start)) => {
            end_of(&raw, *start_value, start_tzid, start, zones, problems)
        }
        _ => None,
    };
    let recurrence_id = raw.recurrence_id.as_ref().and_then(|t| {
        let value = value_of(t, raw.all_day, problems)?;
        Some(calendar_time(value, t.tzid.as_deref(), zones, problems))
    });
    let recurrence = raw
        .rrule
        .as_deref()
        .and_then(|rule| recurrence(rule, start_tzid, zones, problems));
    let cancelled = context.cancel || raw.status == Some(EventStatus::Cancelled);
    CalendarEvent {
        uid: raw.uid,
        summary: raw.summary,
        location: raw.location,
        description: raw.description,
        organizer: raw.organizer,
        attendees: raw.attendees,
        attendee_count: raw.attendee_count,
        start,
        end,
        recurrence,
        recurrence_id,
        status: raw.status,
        sequence: raw.sequence.unwrap_or(0),
        cancelled,
    }
}

/// The value of a date property, or `bad_date`. `VALUE=DATE` and Outlook's all-day flag turn
/// a date-time into a date.
fn value_of(raw: &RawTime, all_day: bool, problems: &mut Problems) -> Option<Value> {
    let Some(value) = parse_value(&raw.value) else {
        problems.push(CalendarProblemCode::BadDate, Some(raw.value.clone()));
        return None;
    };
    Some(match value {
        Value::DateTime { local, .. } if raw.date_only => Value::Date(local.date()),
        Value::DateTime { local, utc: false } if all_day && local.time() == Time::midnight() => {
            Value::Date(local.date())
        }
        other => other,
    })
}

fn calendar_time(
    value: Value,
    tzid: Option<&str>,
    zones: &mut Zones<'_>,
    problems: &mut Problems,
) -> CalendarTime {
    match value {
        Value::Date(date) => CalendarTime::Date {
            date: format_date(date),
        },
        Value::DateTime { local, utc: true } => CalendarTime::DateTime {
            utc: millis(naive_seconds(local).unwrap_or_default()),
            local: format_datetime(local),
            zone: Some("UTC".to_owned()),
            floating: false,
        },
        Value::DateTime { local, utc: false } => {
            let instant = zones.resolve(local, tzid, problems);
            CalendarTime::DateTime {
                utc: millis(instant.seconds),
                local: format_datetime(local),
                zone: instant.zone,
                floating: instant.floating,
            }
        }
    }
}

/// `DTEND`, else `DTSTART` + `DURATION`, else one day for all-day events.
fn end_of(
    raw: &RawEvent,
    start_value: Value,
    start_tzid: Option<&str>,
    start: &CalendarTime,
    zones: &mut Zones<'_>,
    problems: &mut Problems,
) -> Option<CalendarTime> {
    if let Some(raw_end) = &raw.end {
        let end_value = value_of(raw_end, raw.all_day, problems)?;
        let end = calendar_time(end_value, raw_end.tzid.as_deref(), zones, problems);
        return match (start, &end) {
            (CalendarTime::Date { date: s }, CalendarTime::Date { date: e }) => {
                if e == s {
                    // DTEND = DTSTART for an all-day event: one day, as calendars show it.
                    next_day(start_value)
                } else if e < s {
                    problems.push(CalendarProblemCode::EndBeforeStart, None);
                    None
                } else {
                    Some(end)
                }
            }
            (CalendarTime::DateTime { utc: s, .. }, CalendarTime::DateTime { utc: e, .. }) => {
                if e < s {
                    problems.push(CalendarProblemCode::EndBeforeStart, None);
                    None
                } else {
                    Some(end)
                }
            }
            _ => {
                problems.push(CalendarProblemCode::BadDate, Some(raw_end.value.clone()));
                None
            }
        };
    }
    if let Some(text) = &raw.duration {
        let Some(duration) = parse_duration(text) else {
            problems.push(CalendarProblemCode::BadDuration, Some(text.clone()));
            return None;
        };
        let end = add_duration(start_value, start_tzid, duration, zones, problems);
        if end.is_none() {
            problems.push(CalendarProblemCode::BadDuration, Some(text.clone()));
        }
        return end;
    }
    match start_value {
        Value::Date(_) => next_day(start_value),
        Value::DateTime { .. } => None,
    }
}

fn next_day(value: Value) -> Option<CalendarTime> {
    match value {
        Value::Date(date) => Some(CalendarTime::Date {
            date: format_date(date.tomorrow().ok()?),
        }),
        Value::DateTime { .. } => None,
    }
}

/// Days are nominal (the same wall-clock time on a later day, 23 or 25 hours across a DST
/// change), the time part exact (RFC 5545 section 3.3.6). For an all-day event the time part
/// counts in whole days, at least one.
fn add_duration(
    start_value: Value,
    tzid: Option<&str>,
    duration: time::Duration,
    zones: &mut Zones<'_>,
    problems: &mut Problems,
) -> Option<CalendarTime> {
    match start_value {
        Value::Date(date) => {
            let days = duration.days + (duration.seconds + 86_399) / 86_400;
            let end = date
                .checked_add(Span::new().try_days(days.max(1)).ok()?)
                .ok()?;
            Some(CalendarTime::Date {
                date: format_date(end),
            })
        }
        Value::DateTime { local, utc } => {
            let day_end = local
                .checked_add(Span::new().try_days(duration.days).ok()?)
                .ok()?;
            let exact = SignedDuration::from_secs(duration.seconds);
            let end_local = day_end.checked_add(exact).ok()?;
            let (seconds, zone, floating) = if utc {
                (naive_seconds(day_end)?, Some("UTC".to_owned()), false)
            } else {
                let instant = zones.resolve(day_end, tzid, problems);
                (instant.seconds, instant.zone, instant.floating)
            };
            let seconds = seconds.checked_add(duration.seconds)?;
            Some(CalendarTime::DateTime {
                utc: millis(seconds),
                local: format_datetime(end_local),
                zone,
                floating,
            })
        }
    }
}

fn recurrence(
    text: &str,
    start_tzid: Option<&str>,
    zones: &mut Zones<'_>,
    problems: &mut Problems,
) -> Option<CalendarRecurrence> {
    let rule = parse_rule(text);
    let Some(freq) = rule.freq else {
        problems.push(CalendarProblemCode::BadRecurrence, Some(text.to_owned()));
        return None;
    };
    let frequency = match freq {
        Freq::Daily => RecurrenceFrequency::Daily,
        Freq::Weekly => RecurrenceFrequency::Weekly,
        Freq::Monthly => RecurrenceFrequency::Monthly,
        Freq::Yearly => RecurrenceFrequency::Yearly,
        Freq::Secondly | Freq::Minutely | Freq::Hourly => RecurrenceFrequency::Other,
    };
    let until = rule
        .until
        .as_deref()
        .and_then(|until| match parse_value(until) {
            // A local UNTIL (Outlook writes them) is in the zone of DTSTART.
            Some(value) => Some(calendar_time(value, start_tzid, zones, problems)),
            None => {
                problems.push(CalendarProblemCode::BadDate, Some(until.to_owned()));
                None
            }
        });
    Some(CalendarRecurrence {
        frequency,
        interval: rule.interval,
        count: rule.count,
        until,
        by_day: rule
            .by_day
            .iter()
            .map(|&(ordinal, day)| match ordinal {
                Some(n) => format!("{n}{}", weekday_code(day)),
                None => weekday_code(day).to_owned(),
            })
            .collect(),
        by_month_day: rule.by_month_day.iter().map(|&d| i32::from(d)).collect(),
        by_month: rule
            .by_month
            .iter()
            .filter_map(|&m| u32::try_from(m).ok())
            .collect(),
        by_set_pos: rule.by_set_pos.iter().map(|&p| i32::from(p)).collect(),
        rule: rule.raw,
    })
}

fn millis(seconds: i64) -> i64 {
    seconds.saturating_mul(1000)
}

fn format_date(date: jiff::civil::Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

fn format_datetime(local: DateTime) -> String {
    format!(
        "{}T{:02}:{:02}:{:02}",
        format_date(local.date()),
        local.hour(),
        local.minute(),
        local.second()
    )
}
