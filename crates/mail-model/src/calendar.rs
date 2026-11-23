//! Calendar invitations (stage 9): what `mail_content::read_calendar` finds in a `text/calendar`
//! part or an `.ics` attachment, for the invitation card in the reading pane. Read only: no
//! replies, no own calendar, recurrences are described, not expanded.
//!
//! Times: [`CalendarTime::DateTime::utc`] is the instant, to be shown in the user's own time
//! zone; `local` and `zone` say how the file wrote it ("10:00 in New York"). All-day values are
//! dates without a zone. End values are exclusive, as in RFC 5545: an all-day event on
//! 1 December ends on `2026-12-02`.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

/// A calendar file as far as it could be read. Never an error: an unreadable file has no events
/// and says why in `problems`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInvite {
    /// `METHOD` of the iTIP message (RFC 5546) in upper case: `REQUEST` (invitation or update),
    /// `CANCEL`, `REPLY`, `PUBLISH`, `COUNTER`, … `None` for a plain calendar file.
    pub method: Option<String>,
    /// The events in file order (capped, see `mail_content::MAX_CALENDAR_EVENTS`). A recurring
    /// meeting with changed occurrences has one master event and one event per changed
    /// occurrence (with `recurrence_id`).
    pub events: Vec<CalendarEvent>,
    /// What was damaged or left out; empty for a well-formed file.
    pub problems: Vec<CalendarProblem>,
}

/// One `VEVENT`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub uid: Option<String>,
    /// The title; one line.
    pub summary: Option<String>,
    /// One line, as written (a room, an address or a meeting link).
    pub location: Option<String>,
    /// Plain text with `\n` line breaks, cut to a few KiB (then ending in `…`). Untrusted like
    /// any mail text: links in it are opened only through the link check.
    pub description: Option<String>,
    pub organizer: Option<CalendarPerson>,
    /// The first attendees in file order (capped, see `mail_content::MAX_CALENDAR_ATTENDEES`).
    pub attendees: Vec<CalendarPerson>,
    /// Every `ATTENDEE` of the event, also those beyond `attendees`.
    pub attendee_count: u32,
    /// `None` when the event has no readable `DTSTART` (allowed in `CANCEL` and `REPLY`).
    pub start: Option<CalendarTime>,
    /// Exclusive end, from `DTEND` or `DTSTART` + `DURATION`. An all-day event without either
    /// lasts one day; a timed one without either has no end (`None`).
    pub end: Option<CalendarTime>,
    pub recurrence: Option<CalendarRecurrence>,
    /// Set when this event changes or cancels one occurrence of a recurring event
    /// (`RECURRENCE-ID`): the original start of that occurrence.
    pub recurrence_id: Option<CalendarTime>,
    pub status: Option<EventStatus>,
    /// `SEQUENCE`: revision of the invitation, 0 for the first one.
    pub sequence: u32,
    /// The organizer cancelled it: `METHOD:CANCEL` or `STATUS:CANCELLED`.
    pub cancelled: bool,
}

/// Organizer or attendee (`CAL-ADDRESS` with its parameters).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarPerson {
    /// `CN`, the display name.
    pub name: Option<String>,
    /// The address of a `mailto:` value (or of the `EMAIL` parameter).
    pub email: Option<String>,
    /// `ROLE`; attendees only. `None` when missing (RFC 5545 then means `required`) or unknown.
    pub role: Option<AttendeeRole>,
    /// `PARTSTAT`; attendees only. `None` when missing or unknown.
    pub status: Option<ParticipationStatus>,
}

/// A point in time of an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CalendarTime {
    /// A calendar day without time or zone (`VALUE=DATE`, all-day events): `YYYY-MM-DD`.
    Date { date: String },
    /// A date and time.
    DateTime {
        /// The instant in Unix milliseconds. Show it in the user's own time zone.
        #[specta(type = Number)]
        utc: i64,
        /// The wall-clock time as the file wrote it, `YYYY-MM-DDTHH:MM:SS` (in `zone`; UTC for
        /// `zone: "UTC"`). For an end computed from `DURATION` it is start + duration.
        local: String,
        /// The zone `local` is in: `"UTC"`, an IANA name (`America/New_York`; also when the
        /// file used a Windows name such as `Eastern Standard Time` or a prefixed name such as
        /// `/mozilla.org/…/Europe/Berlin`), or the `TZID` as written when only the file's own
        /// `VTIMEZONE` defines it. `None` when floating.
        zone: Option<String>,
        /// No usable zone: a floating time (no `TZID`, no `Z`) or an unknown `TZID`
        /// (`unknown_time_zone` problem). `utc` was computed in the user's own time zone, so the
        /// wall-clock time stays as written.
        floating: bool,
    },
}

/// `RRULE`, described for the card ("every 2 weeks on Monday until 31 December"), not expanded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarRecurrence {
    pub frequency: RecurrenceFrequency,
    /// `INTERVAL`, at least 1: every `interval` days, weeks, months or years.
    pub interval: u32,
    /// `COUNT`: number of occurrences.
    pub count: Option<u32>,
    /// `UNTIL`: last possible occurrence (inclusive).
    pub until: Option<CalendarTime>,
    /// `BYDAY`, normalised: `MO`, `-1SU` (last Sunday), `2TU` (second Tuesday).
    pub by_day: Vec<String>,
    /// `BYMONTHDAY`: 1 to 31, or -1 (last day) to -31.
    pub by_month_day: Vec<i32>,
    /// `BYMONTH`: 1 to 12.
    pub by_month: Vec<u32>,
    /// `BYSETPOS`: which of the matching days (`-1` = the last).
    pub by_set_pos: Vec<i32>,
    /// The `RRULE` value as written (cut to 256 bytes), for what the fields above do not cover
    /// (`BYWEEKNO`, `BYHOUR`, …).
    pub rule: String,
}

/// `FREQ` of a recurrence rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceFrequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    /// `SECONDLY`, `MINUTELY` or `HOURLY`: shown from `rule`.
    Other,
}

/// `STATUS` of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Tentative,
    Confirmed,
    Cancelled,
}

/// `ROLE` of an attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AttendeeRole {
    Chair,
    /// `REQ-PARTICIPANT`.
    Required,
    /// `OPT-PARTICIPANT`.
    Optional,
    /// `NON-PARTICIPANT` (for information only).
    NonParticipant,
}

/// `PARTSTAT` of an attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ParticipationStatus {
    NeedsAction,
    Accepted,
    Declined,
    Tentative,
    Delegated,
}

/// Something about the calendar file that is damaged or was left out.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarProblem {
    pub code: CalendarProblemCode,
    /// What it is about, for diagnostics (not translated), cut to 64 bytes: the `TZID` for
    /// `unknown_time_zone`, the value as written for `bad_date`/`bad_duration`/`bad_recurrence`,
    /// the property name for `line_too_long`, `"2097152 bytes"` or `"100000 lines"` for
    /// `too_large`, the number of events found for `too_many_events`. `None` when there is
    /// nothing to add.
    pub detail: Option<String>,
}

/// Problem codes of a calendar file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CalendarProblemCode {
    /// Not a calendar: no `BEGIN:VCALENDAR` and no event.
    NotCalendar,
    /// A calendar without events (only tasks, free/busy data, time zones, or nothing).
    NoEvent,
    /// Larger than the input limit (bytes or lines): only the beginning was read.
    TooLarge,
    /// More events than the limit; the rest was skipped. `detail`: how many there were.
    TooManyEvents,
    /// The file ends inside a component (`END` missing, cut off): what was read is shown.
    Truncated,
    /// `BEGIN`/`END` do not match, components nested too deep, or an event outside a calendar.
    BrokenStructure,
    /// A content line that cannot be read (no colon, invalid name, unclosed quote); skipped.
    BrokenLine,
    /// A property line longer than the line limit; its value was cut.
    LineTooLong,
    /// Bytes that are not UTF-8 were replaced by U+FFFD.
    MalformedText,
    /// A date or date-time that cannot be read (`DTSTART`, `DTEND`, `RECURRENCE-ID`, `UNTIL`),
    /// or `DTSTART` and `DTEND` of different kinds; the value is left out.
    BadDate,
    /// `DURATION` cannot be read or is negative; the end is left out.
    BadDuration,
    /// `RRULE` without a readable `FREQ`; the recurrence is left out.
    BadRecurrence,
    /// `DTEND` is before `DTSTART`; the end is left out.
    EndBeforeStart,
    /// An event without `DTSTART` (outside `CANCEL`, `REPLY` and other replies).
    MissingStart,
    /// A `TZID` that is neither known to the system, nor a Windows zone name, nor defined by a
    /// `VTIMEZONE` in the file: the time is shown as floating (in the user's zone).
    UnknownTimeZone,
    /// The reader failed unexpectedly (a bug); nothing is shown.
    Internal,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format() {
        let invite = CalendarInvite {
            method: Some("REQUEST".into()),
            events: vec![CalendarEvent {
                summary: Some("Planning".into()),
                organizer: Some(CalendarPerson {
                    name: Some("Ada".into()),
                    email: Some("ada@example.com".into()),
                    role: None,
                    status: None,
                }),
                attendees: vec![CalendarPerson {
                    email: Some("bob@example.com".into()),
                    role: Some(AttendeeRole::NonParticipant),
                    status: Some(ParticipationStatus::NeedsAction),
                    ..CalendarPerson::default()
                }],
                attendee_count: 1,
                start: Some(CalendarTime::DateTime {
                    utc: 1_773_932_400_000,
                    local: "2026-03-19T11:00:00".into(),
                    zone: Some("America/New_York".into()),
                    floating: false,
                }),
                end: Some(CalendarTime::Date {
                    date: "2026-03-20".into(),
                }),
                recurrence: Some(CalendarRecurrence {
                    frequency: RecurrenceFrequency::Weekly,
                    interval: 2,
                    count: None,
                    until: None,
                    by_day: vec!["-1SU".into()],
                    by_month_day: vec![],
                    by_month: vec![3],
                    by_set_pos: vec![],
                    rule: "FREQ=WEEKLY;INTERVAL=2".into(),
                }),
                status: Some(EventStatus::Confirmed),
                ..CalendarEvent::default()
            }],
            problems: vec![CalendarProblem {
                code: CalendarProblemCode::UnknownTimeZone,
                detail: Some("Mars Standard Time".into()),
            }],
        };
        let value = serde_json::to_value(&invite).expect("json");
        let event = &value["events"][0];
        assert_eq!(value["method"], "REQUEST");
        assert_eq!(
            event["start"],
            json!({
                "kind": "dateTime",
                "utc": 1_773_932_400_000_i64,
                "local": "2026-03-19T11:00:00",
                "zone": "America/New_York",
                "floating": false
            })
        );
        assert_eq!(
            event["end"],
            json!({ "kind": "date", "date": "2026-03-20" })
        );
        assert_eq!(event["attendeeCount"], 1);
        assert_eq!(event["recurrenceId"], json!(null));
        assert_eq!(event["attendees"][0]["role"], "non_participant");
        assert_eq!(event["attendees"][0]["status"], "needs_action");
        assert_eq!(event["recurrence"]["byDay"], json!(["-1SU"]));
        assert_eq!(event["recurrence"]["frequency"], "weekly");
        assert_eq!(event["status"], "confirmed");
        assert_eq!(
            value["problems"],
            json!([{ "code": "unknown_time_zone", "detail": "Mars Standard Time" }])
        );
        let back: CalendarInvite = serde_json::from_value(value).expect("back");
        assert_eq!(back, invite);
    }
}
