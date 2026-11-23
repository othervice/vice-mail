//! Tests of the calendar reader: invitations as Outlook/Exchange, Google, Apple, Thunderbird
//! send them, time zones (IANA, Windows names, the file's own `VTIMEZONE`, floating, UTC) with
//! real DST rules from the system's time-zone database, recurrences, and broken or hostile files.
//!
//! The helper [`read`] calls the reader without its `catch_unwind` guard, so a panic fails the
//! test instead of turning into the `internal` problem.

use std::time::Instant;

use jiff::Timestamp;
use jiff::civil::{DateTime, date};
use jiff::tz::TimeZone;
use mail_model::{
    AttendeeRole, CalendarEvent, CalendarInvite, CalendarPerson, CalendarProblem,
    CalendarProblemCode as Code, CalendarTime, EventStatus, ParticipationStatus,
    RecurrenceFrequency,
};

use super::time::{Zones, naive_seconds};
use super::windows::WINDOWS_ZONES;
use super::*;

/// The system's time-zone database; the user lives in Berlin.
fn berlin(tzid: Option<&str>, local: DateTime) -> Option<i64> {
    let zone = TimeZone::get(tzid.unwrap_or("Europe/Berlin")).ok()?;
    zone.to_ambiguous_timestamp(local)
        .compatible()
        .ok()
        .map(|t| t.as_second())
}

/// A resolver that knows no zone at all; the user lives in UTC.
fn no_zones(tzid: Option<&str>, local: DateTime) -> Option<i64> {
    match tzid {
        None => naive_seconds(local),
        Some(_) => None,
    }
}

fn read(data: &[u8], zones: &dyn ZoneResolver) -> CalendarInvite {
    let invite = super::read(data, zones);
    assert!(
        !invite.problems.iter().any(|p| p.code == Code::Internal),
        "{:?}",
        invite.problems
    );
    invite
}

/// A calendar around `body` (lines separated by `\n`, sent as CRLF).
fn cal(body: &str) -> Vec<u8> {
    format!("BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//Vice Mail//Test//EN\n{body}\nEND:VCALENDAR\n")
        .replace('\n', "\r\n")
        .into_bytes()
}

/// A calendar with one event made of `lines`.
fn event_cal(lines: &str) -> Vec<u8> {
    cal(&format!(
        "BEGIN:VEVENT\nUID:test@vicemail.test\nDTSTAMP:20261120T101010Z\n{lines}\nEND:VEVENT"
    ))
}

fn only(invite: &CalendarInvite) -> &CalendarEvent {
    assert_eq!(invite.events.len(), 1, "{invite:?}");
    &invite.events[0]
}

fn ms(utc: &str) -> i64 {
    utc.parse::<Timestamp>()
        .expect("timestamp")
        .as_millisecond()
}

fn day(date: &str) -> CalendarTime {
    CalendarTime::Date { date: date.into() }
}

fn zoned(utc: &str, local: &str, zone: &str) -> CalendarTime {
    CalendarTime::DateTime {
        utc: ms(utc),
        local: local.into(),
        zone: Some(zone.into()),
        floating: false,
    }
}

fn floating(utc: &str, local: &str) -> CalendarTime {
    CalendarTime::DateTime {
        utc: ms(utc),
        local: local.into(),
        zone: None,
        floating: true,
    }
}

fn codes(invite: &CalendarInvite) -> Vec<Code> {
    invite.problems.iter().map(|p| p.code).collect()
}

fn problem(code: Code, detail: &str) -> CalendarProblem {
    CalendarProblem {
        code,
        detail: Some(detail.into()),
    }
}

/// Outlook's definition of "W. Europe Standard Time" (Exchange 2010 to Microsoft 365).
const OUTLOOK_BERLIN: &str = "BEGIN:VTIMEZONE
TZID:W. Europe Standard Time
BEGIN:STANDARD
DTSTART:16010101T030000
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:16010101T020000
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
END:DAYLIGHT
END:VTIMEZONE";

/// Outlook's definition of US Eastern time under a display name it sometimes uses as TZID.
const OUTLOOK_EASTERN: &str = "BEGIN:VTIMEZONE
TZID:(UTC-05:00) Eastern Time (US & Canada)
BEGIN:STANDARD
DTSTART:16010101T020000
TZOFFSETFROM:-0400
TZOFFSETTO:-0500
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:16010101T020000
TZOFFSETFROM:-0500
TZOFFSETTO:-0400
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3
END:DAYLIGHT
END:VTIMEZONE";

/// A Teams meeting invitation as Exchange Online sends it (shortened).
fn outlook_invite() -> Vec<u8> {
    let body = format!(
        "METHOD:REQUEST
{OUTLOOK_BERLIN}
BEGIN:VEVENT
ORGANIZER;CN=Erika Mustermann:mailto:erika.mustermann@example.de
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Max Muster:mailto:max@ex
 ample.de
ATTENDEE;ROLE=OPT-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=\"Doe, Jane\":MAILTO:jane@
 example.com
DESCRIPTION;LANGUAGE=de-DE:Agenda:\\n1. Zahlen\\n2. Planung\\n\\n______________
 __________________________________________________________________\\nMicrosof
 t Teams-Besprechung\\nJetzt an der Besprechung teilnehmen <https://teams.mic
 rosoft.com/l/meetup-join/19%3ameeting>\\n
UID:040000008200E00074C5B7101A82E00800000000A0B1C2D3E4F5A60100000000000000001000
 0000F3A1B2C3D4E5F60718293A4B5C6D7E8F
SUMMARY;LANGUAGE=de-DE:Quartalsplanung
DTSTART;TZID=W. Europe Standard Time:20260330T090000
DTEND;TZID=W. Europe Standard Time:20260330T103000
CLASS:PUBLIC
PRIORITY:5
DTSTAMP:20260301T120000Z
TRANSP:OPAQUE
STATUS:CONFIRMED
SEQUENCE:2
LOCATION;LANGUAGE=de-DE:Microsoft Teams-Besprechung
X-MICROSOFT-CDO-APPT-SEQUENCE:2
X-MICROSOFT-CDO-BUSYSTATUS:TENTATIVE
X-MICROSOFT-CDO-INTENDEDSTATUS:BUSY
X-MICROSOFT-CDO-ALLDAYEVENT:FALSE
X-MICROSOFT-CDO-IMPORTANCE:1
X-MICROSOFT-CDO-INSTTYPE:0
X-MICROSOFT-SKYPETEAMSMEETINGURL:https://teams.microsoft.com/l/meetup-join/19%3ameeting
X-MICROSOFT-DONOTFORWARDMEETING:FALSE
X-MICROSOFT-DISALLOW-COUNTER:FALSE
BEGIN:VALARM
DESCRIPTION:REMINDER
TRIGGER;RELATED=START:-PT15M
ACTION:DISPLAY
END:VALARM
END:VEVENT"
    );
    cal(&body)
}

// --- all-day events --------------------------------------------------------------------------

#[test]
fn all_day_single_day_without_end_lasts_one_day() {
    let invite = read(
        &event_cal("DTSTART;VALUE=DATE:20261201\nSUMMARY:Team offsite"),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(event.start, Some(day("2026-12-01")));
    assert_eq!(event.end, Some(day("2026-12-02")));
    assert_eq!(event.summary.as_deref(), Some("Team offsite"));
    assert_eq!(event.uid.as_deref(), Some("test@vicemail.test"));
    // Without VALUE=DATE (common) the same.
    let invite = read(&event_cal("DTSTART:20261231"), &berlin);
    assert_eq!(only(&invite).end, Some(day("2027-01-01")));
}

#[test]
fn all_day_over_several_days_has_an_exclusive_end() {
    let invite = read(
        &event_cal("DTSTART;VALUE=DATE:20261230\nDTEND;VALUE=DATE:20270102"),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(event.start, Some(day("2026-12-30")));
    assert_eq!(event.end, Some(day("2027-01-02")));
    // DTEND = DTSTART (seen in the wild) is one day.
    let invite = read(
        &event_cal("DTSTART;VALUE=DATE:20261201\nDTEND;VALUE=DATE:20261201"),
        &berlin,
    );
    assert_eq!(only(&invite).end, Some(day("2026-12-02")));
    // DURATION on a date.
    let invite = read(
        &event_cal("DTSTART;VALUE=DATE:20261201\nDURATION:P3D"),
        &berlin,
    );
    assert_eq!(only(&invite).end, Some(day("2026-12-04")));
}

#[test]
fn outlook_all_day_flag_turns_midnights_into_dates() {
    let invite = read(
        &cal(&format!(
            "{OUTLOOK_BERLIN}\nBEGIN:VEVENT\nUID:x\n\
             DTSTART;TZID=W. Europe Standard Time:20261201T000000\n\
             DTEND;TZID=W. Europe Standard Time:20261203T000000\n\
             X-MICROSOFT-CDO-ALLDAYEVENT:TRUE\nEND:VEVENT"
        )),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(event.start, Some(day("2026-12-01")));
    assert_eq!(event.end, Some(day("2026-12-03")));
}

// --- recurrences ----------------------------------------------------------------------------

#[test]
fn weekly_with_until() {
    let invite = read(
        &event_cal(
            "DTSTART;TZID=Europe/Berlin:20261102T090000\nDTEND;TZID=Europe/Berlin:20261102T093000\n\
             RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;UNTIL=20261231T225959Z;WKST=MO",
        ),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(
        event.start,
        Some(zoned(
            "2026-11-02T08:00:00Z",
            "2026-11-02T09:00:00",
            "Europe/Berlin"
        ))
    );
    let recurrence = event.recurrence.as_ref().expect("recurrence");
    assert_eq!(recurrence.frequency, RecurrenceFrequency::Weekly);
    assert_eq!(recurrence.interval, 2);
    assert_eq!(recurrence.by_day, ["MO", "WE"]);
    assert_eq!(recurrence.count, None);
    assert_eq!(
        recurrence.until,
        Some(zoned("2026-12-31T22:59:59Z", "2026-12-31T22:59:59", "UTC"))
    );
    assert_eq!(
        recurrence.rule,
        "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;UNTIL=20261231T225959Z;WKST=MO"
    );
}

#[test]
fn weekly_with_count() {
    let invite = read(
        &event_cal(
            "DTSTART;TZID=Europe/Berlin:20261103T140000\nRRULE:FREQ=WEEKLY;COUNT=10;BYDAY=TU",
        ),
        &berlin,
    );
    let recurrence = only(&invite).recurrence.as_ref().expect("recurrence");
    assert_eq!(invite.problems, []);
    assert_eq!(recurrence.frequency, RecurrenceFrequency::Weekly);
    assert_eq!(recurrence.interval, 1);
    assert_eq!(recurrence.count, Some(10));
    assert_eq!(recurrence.until, None);
    assert_eq!(recurrence.by_day, ["TU"]);
}

#[test]
fn monthly_and_yearly_rules_and_local_until() {
    // A local UNTIL (Outlook) is read in the zone of DTSTART.
    let invite = read(
        &event_cal(
            "DTSTART;TZID=America/New_York:20261127T100000\n\
             RRULE:FREQ=MONTHLY;BYDAY=-1FR;UNTIL=20270625T100000",
        ),
        &berlin,
    );
    let recurrence = only(&invite).recurrence.as_ref().expect("recurrence");
    assert_eq!(recurrence.frequency, RecurrenceFrequency::Monthly);
    assert_eq!(recurrence.by_day, ["-1FR"]);
    assert_eq!(
        recurrence.until,
        Some(zoned(
            "2027-06-25T14:00:00Z",
            "2027-06-25T10:00:00",
            "America/New_York"
        ))
    );
    let invite = read(
        &event_cal(
            "DTSTART;VALUE=DATE:20260320\nRRULE:FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=20;BYSETPOS=-1;UNTIL=20300320",
        ),
        &berlin,
    );
    let recurrence = only(&invite).recurrence.as_ref().expect("recurrence");
    assert_eq!(recurrence.frequency, RecurrenceFrequency::Yearly);
    assert_eq!(recurrence.by_month, [3]);
    assert_eq!(recurrence.by_month_day, [20]);
    assert_eq!(recurrence.by_set_pos, [-1]);
    assert_eq!(recurrence.until, Some(day("2030-03-20")));
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nRRULE:FREQ=HOURLY;INTERVAL=4"),
        &berlin,
    );
    let recurrence = only(&invite).recurrence.as_ref().expect("recurrence");
    assert_eq!(recurrence.frequency, RecurrenceFrequency::Other);
    assert_eq!(recurrence.interval, 4);
}

#[test]
fn unreadable_rules_are_reported() {
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nRRULE:INTERVAL=2;BYDAY=MO"),
        &berlin,
    );
    assert_eq!(only(&invite).recurrence, None);
    assert_eq!(
        invite.problems,
        [problem(Code::BadRecurrence, "INTERVAL=2;BYDAY=MO")]
    );
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nRRULE:FREQ=DAILY;UNTIL=2026-13-45"),
        &berlin,
    );
    let recurrence = only(&invite).recurrence.as_ref().expect("recurrence");
    assert_eq!(recurrence.until, None);
    assert_eq!(invite.problems, [problem(Code::BadDate, "2026-13-45")]);
}

// --- time zones ------------------------------------------------------------------------------

#[test]
fn foreign_zone_across_the_dst_boundaries() {
    // The US switches on 8 March 2026, the EU on 29 March: in between New York is 4 hours
    // behind UTC, Berlin still 1 hour ahead.
    let invite = read(
        &cal(
            "BEGIN:VEVENT\nUID:a\nDTSTART;TZID=America/New_York:20260305T100000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:b\nDTSTART;TZID=America/New_York:20260320T100000\n\
              DTEND;TZID=America/New_York:20260320T110000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:c\nDTSTART;TZID=America/New_York:20260308T023000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:d\nDTSTART;TZID=America/New_York:20261101T013000\nEND:VEVENT",
        ),
        &berlin,
    );
    assert_eq!(invite.problems, []);
    let starts: Vec<_> = invite.events.iter().map(|e| e.start.clone()).collect();
    assert_eq!(
        starts,
        [
            // Standard time: UTC-5.
            Some(zoned(
                "2026-03-05T15:00:00Z",
                "2026-03-05T10:00:00",
                "America/New_York"
            )),
            // Daylight time: UTC-4. A viewer in Berlin sees 15:00, not 16:00.
            Some(zoned(
                "2026-03-20T14:00:00Z",
                "2026-03-20T10:00:00",
                "America/New_York"
            )),
            // 02:30 does not exist on 8 March: read with the offset before the gap (03:30 EDT).
            Some(zoned(
                "2026-03-08T07:30:00Z",
                "2026-03-08T02:30:00",
                "America/New_York"
            )),
            // 01:30 occurs twice on 1 November: the earlier one (EDT).
            Some(zoned(
                "2026-11-01T05:30:00Z",
                "2026-11-01T01:30:00",
                "America/New_York"
            )),
        ]
    );
    assert_eq!(
        invite.events[1].end,
        Some(zoned(
            "2026-03-20T15:00:00Z",
            "2026-03-20T11:00:00",
            "America/New_York"
        ))
    );
}

#[test]
fn outlook_invite_with_windows_zone_name() {
    let invite = read(&outlook_invite(), &berlin);
    assert_eq!(invite.problems, []);
    assert_eq!(invite.method.as_deref(), Some("REQUEST"));
    let event = only(&invite);
    // The Windows name maps to Europe/Berlin; 30 March is already summer time (UTC+2).
    assert_eq!(
        event.start,
        Some(zoned(
            "2026-03-30T07:00:00Z",
            "2026-03-30T09:00:00",
            "Europe/Berlin"
        ))
    );
    assert_eq!(
        event.end,
        Some(zoned(
            "2026-03-30T08:30:00Z",
            "2026-03-30T10:30:00",
            "Europe/Berlin"
        ))
    );
    assert_eq!(event.summary.as_deref(), Some("Quartalsplanung"));
    assert_eq!(
        event.location.as_deref(),
        Some("Microsoft Teams-Besprechung")
    );
    let description = event.description.as_deref().expect("description");
    assert!(
        description.starts_with("Agenda:\n1. Zahlen\n2. Planung\n\n____"),
        "{description}"
    );
    assert!(description.ends_with(
        "Microsoft Teams-Besprechung\nJetzt an der Besprechung teilnehmen <https://teams.microsoft.com/l/meetup-join/19%3ameeting>"
    ));
    assert_eq!(event.status, Some(EventStatus::Confirmed));
    assert_eq!(event.sequence, 2);
    assert!(!event.cancelled);
    assert!(event.uid.as_deref().is_some_and(|u| u.len() > 100));
    assert_eq!(
        event.organizer,
        Some(CalendarPerson {
            name: Some("Erika Mustermann".into()),
            email: Some("erika.mustermann@example.de".into()),
            role: None,
            status: None,
        })
    );
    assert_eq!(
        event.attendees,
        [
            CalendarPerson {
                name: Some("Max Muster".into()),
                email: Some("max@example.de".into()),
                role: Some(AttendeeRole::Required),
                status: Some(ParticipationStatus::NeedsAction),
            },
            CalendarPerson {
                name: Some("Doe, Jane".into()),
                email: Some("jane@example.com".into()),
                role: Some(AttendeeRole::Optional),
                status: Some(ParticipationStatus::NeedsAction),
            },
        ]
    );
    assert_eq!(event.attendee_count, 2);
}

#[test]
fn outlook_invite_resolved_from_its_vtimezone_alone() {
    // The resolver knows nothing: the Windows name does not resolve, the VTIMEZONE does.
    let invite = read(&outlook_invite(), &no_zones);
    assert_eq!(invite.problems, []);
    let event = only(&invite);
    assert_eq!(
        event.start,
        Some(zoned(
            "2026-03-30T07:00:00Z",
            "2026-03-30T09:00:00",
            "W. Europe Standard Time"
        ))
    );
    // A TZID nobody knows but the file (Outlook display names), quoted because of the colon.
    let invite = read(
        &cal(&format!(
            "{OUTLOOK_EASTERN}\nBEGIN:VEVENT\nUID:x\n\
             DTSTART;TZID=\"(UTC-05:00) Eastern Time (US & Canada)\":20260320T100000\n\
             DTEND;TZID=\"(UTC-05:00) Eastern Time (US & Canada)\":20260320T110000\nEND:VEVENT"
        )),
        &berlin,
    );
    assert_eq!(invite.problems, []);
    assert_eq!(
        only(&invite).start,
        Some(zoned(
            "2026-03-20T14:00:00Z",
            "2026-03-20T10:00:00",
            "(UTC-05:00) Eastern Time (US & Canada)"
        ))
    );
}

/// Every half hour of 2025 to 2027 (gaps and folds included): the file's VTIMEZONE gives the
/// same instant as the time-zone database with jiff's "compatible" rule.
#[test]
fn vtimezone_matches_the_time_zone_database_every_half_hour() {
    for (definition, tzid, iana) in [
        (OUTLOOK_BERLIN, "W. Europe Standard Time", "Europe/Berlin"),
        (
            OUTLOOK_EASTERN,
            "(UTC-05:00) Eastern Time (US & Canada)",
            "America/New_York",
        ),
    ] {
        let mut reader = Reader::default();
        for line in Unfolder::new(definition.replace('\n', "\r\n").as_bytes(), 1000) {
            reader.line(&line);
        }
        reader.end_of_input();
        assert_eq!(reader.zones.len(), 1);
        let mut zones = Zones::new(&no_zones, &reader.zones, None);
        let mut problems = Problems::default();
        let database = TimeZone::get(iana).expect("zone");
        let mut local = date(2025, 1, 1).at(0, 0, 0, 0);
        let end = date(2028, 1, 1).at(0, 0, 0, 0);
        let mut checked = 0;
        while local < end {
            let expected = database
                .to_ambiguous_timestamp(local)
                .compatible()
                .expect("instant")
                .as_second();
            let instant = zones.resolve(local, Some(tzid), &mut problems);
            assert_eq!(instant.seconds, expected, "{iana} {local}");
            assert!(!instant.floating);
            local = local
                .checked_add(jiff::SignedDuration::from_mins(30))
                .expect("next");
            checked += 1;
        }
        assert_eq!(checked, 3 * 365 * 48);
        assert_eq!(problems.0, []);
    }
}

#[test]
fn vtimezone_with_rdates_and_without_rules() {
    // India: one fixed offset. Historical definitions use RDATE instead of RRULE.
    let invite = read(
        &cal(
            "BEGIN:VTIMEZONE\nTZID:India\nBEGIN:STANDARD\nDTSTART:19700101T000000\n\
              TZOFFSETFROM:+0530\nTZOFFSETTO:+0530\nEND:STANDARD\nEND:VTIMEZONE\n\
              BEGIN:VTIMEZONE\nTZID:Old Berlin\nBEGIN:DAYLIGHT\nDTSTART:20250330T020000\n\
              RDATE:20260329T020000,20270328T020000\nTZOFFSETFROM:+0100\nTZOFFSETTO:+0200\n\
              END:DAYLIGHT\nBEGIN:STANDARD\nDTSTART:20251026T030000\nRDATE:20261025T030000\n\
              TZOFFSETFROM:+0200\nTZOFFSETTO:+0100\nEND:STANDARD\nEND:VTIMEZONE\n\
              BEGIN:VEVENT\nUID:a\nDTSTART;TZID=India:20261201T100000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:b\nDTSTART;TZID=Old Berlin:20260715T100000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:c\nDTSTART;TZID=Old Berlin:20261201T100000\nEND:VEVENT",
        ),
        &no_zones,
    );
    assert_eq!(invite.problems, []);
    let starts: Vec<_> = invite
        .events
        .iter()
        .map(|e| match &e.start {
            Some(CalendarTime::DateTime { utc, .. }) => *utc,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        starts,
        [
            ms("2026-12-01T04:30:00Z"),
            ms("2026-07-15T08:00:00Z"),
            ms("2026-12-01T09:00:00Z")
        ]
    );
}

#[test]
fn vendor_prefixed_zone_names() {
    let invite = read(
        &event_cal(
            "DTSTART;TZID=/mozilla.org/20050126_1/Europe/Berlin:20261201T100000\n\
             DTEND;TZID=/citadel.org/20190914_1/America/New_York:20261201T050000",
        ),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(
        event.start,
        Some(zoned(
            "2026-12-01T09:00:00Z",
            "2026-12-01T10:00:00",
            "Europe/Berlin"
        ))
    );
    assert_eq!(
        event.end,
        Some(zoned(
            "2026-12-01T10:00:00Z",
            "2026-12-01T05:00:00",
            "America/New_York"
        ))
    );
}

#[test]
fn unknown_zone_is_reported_and_read_as_floating() {
    let invite = read(
        &event_cal(
            "DTSTART;TZID=Mars/Olympus_Mons:20261201T100000\n\
             DTEND;TZID=Mars/Olympus_Mons:20261201T110000",
        ),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(
        invite.problems,
        [problem(Code::UnknownTimeZone, "Mars/Olympus_Mons")]
    );
    assert_eq!(
        event.start,
        Some(floating("2026-12-01T09:00:00Z", "2026-12-01T10:00:00"))
    );
    assert_eq!(
        event.end,
        Some(floating("2026-12-01T10:00:00Z", "2026-12-01T11:00:00"))
    );
    // A Windows name the resolver cannot resolve and the file does not define.
    let invite = read(
        &event_cal("DTSTART;TZID=Tokyo Standard Time:20261201T100000"),
        &no_zones,
    );
    assert_eq!(
        invite.problems,
        [problem(Code::UnknownTimeZone, "Tokyo Standard Time")]
    );
}

#[test]
fn windows_names_map_to_existing_iana_zones() {
    for (index, (windows, iana)) in WINDOWS_ZONES.iter().enumerate() {
        assert!(TimeZone::get(iana).is_ok(), "{windows} → {iana}");
        assert!(
            !WINDOWS_ZONES
                .iter()
                .skip(index + 1)
                .any(|(other, _)| other.eq_ignore_ascii_case(windows)),
            "{windows} twice"
        );
    }
    assert!(WINDOWS_ZONES.len() > 100);
    let invite = read(
        &event_cal(
            "DTSTART;TZID=Pacific Standard Time:20260710T090000\n\
             DTEND;TZID=tokyo standard time:20260711T010000",
        ),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(
        event.start,
        Some(zoned(
            "2026-07-10T16:00:00Z",
            "2026-07-10T09:00:00",
            "America/Los_Angeles"
        ))
    );
    assert_eq!(
        event.end,
        Some(zoned(
            "2026-07-10T16:00:00Z",
            "2026-07-11T01:00:00",
            "Asia/Tokyo"
        ))
    );
}

#[test]
fn floating_times_are_read_in_the_users_zone() {
    let invite = read(
        &event_cal("DTSTART:20261201T100000\nDTEND:20261201T110000"),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(
        event.start,
        Some(floating("2026-12-01T09:00:00Z", "2026-12-01T10:00:00"))
    );
    assert_eq!(
        event.end,
        Some(floating("2026-12-01T10:00:00Z", "2026-12-01T11:00:00"))
    );
    // In summer the user's zone is UTC+2.
    let invite = read(&event_cal("DTSTART:20260701T100000"), &berlin);
    assert_eq!(
        only(&invite).start,
        Some(floating("2026-07-01T08:00:00Z", "2026-07-01T10:00:00"))
    );
}

#[test]
fn x_wr_timezone_applies_to_floating_times_only() {
    let invite = read(
        &cal(
            "X-WR-TIMEZONE:America/New_York\nBEGIN:VEVENT\nUID:a\nDTSTART:20261201T100000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:b\nDTSTART;TZID=Europe/Berlin:20261201T100000\nEND:VEVENT\n\
              BEGIN:VEVENT\nUID:c\nDTSTART:20261201T100000Z\nEND:VEVENT",
        ),
        &berlin,
    );
    assert_eq!(invite.problems, []);
    let starts: Vec<_> = invite.events.iter().map(|e| e.start.clone()).collect();
    assert_eq!(
        starts,
        [
            Some(zoned(
                "2026-12-01T15:00:00Z",
                "2026-12-01T10:00:00",
                "America/New_York"
            )),
            Some(zoned(
                "2026-12-01T09:00:00Z",
                "2026-12-01T10:00:00",
                "Europe/Berlin"
            )),
            Some(zoned("2026-12-01T10:00:00Z", "2026-12-01T10:00:00", "UTC")),
        ]
    );
    // An X-WR-TIMEZONE that does not resolve is ignored without a problem.
    let invite = read(
        &cal(
            "X-WR-TIMEZONE:Nowhere/Special\nBEGIN:VEVENT\nUID:a\nDTSTART:20261201T100000\nEND:VEVENT",
        ),
        &berlin,
    );
    assert_eq!(invite.problems, []);
    assert_eq!(
        only(&invite).start,
        Some(floating("2026-12-01T09:00:00Z", "2026-12-01T10:00:00"))
    );
}

#[test]
fn utc_times() {
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nDTEND;TZID=UTC:20261201T113000"),
        &no_zones,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(
        event.start,
        Some(zoned("2026-12-01T10:00:00Z", "2026-12-01T10:00:00", "UTC"))
    );
    assert_eq!(
        event.end,
        Some(zoned("2026-12-01T11:30:00Z", "2026-12-01T11:30:00", "UTC"))
    );
}

#[test]
fn duration_instead_of_dtend() {
    // P1D is nominal: the same wall-clock time the next day, 23 hours across the EU switch.
    let invite = read(
        &event_cal("DTSTART;TZID=Europe/Berlin:20260328T120000\nDURATION:P1D"),
        &berlin,
    );
    assert_eq!(invite.problems, []);
    assert_eq!(
        only(&invite).end,
        Some(zoned(
            "2026-03-29T10:00:00Z",
            "2026-03-29T12:00:00",
            "Europe/Berlin"
        ))
    );
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nDURATION:PT1H30M"),
        &berlin,
    );
    assert_eq!(
        only(&invite).end,
        Some(zoned("2026-12-01T11:30:00Z", "2026-12-01T11:30:00", "UTC"))
    );
    let invite = read(
        &event_cal("DTSTART;TZID=Europe/Berlin:20261201T100000\nDURATION:P1W"),
        &berlin,
    );
    assert_eq!(
        only(&invite).end,
        Some(zoned(
            "2026-12-08T09:00:00Z",
            "2026-12-08T10:00:00",
            "Europe/Berlin"
        ))
    );
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nDURATION:-PT1H"),
        &berlin,
    );
    assert_eq!(only(&invite).end, None);
    assert_eq!(invite.problems, [problem(Code::BadDuration, "-PT1H")]);
    // DTEND wins over DURATION.
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nDURATION:PT1H\nDTEND:20261201T120000Z"),
        &berlin,
    );
    assert_eq!(
        only(&invite).end,
        Some(zoned("2026-12-01T12:00:00Z", "2026-12-01T12:00:00", "UTC"))
    );
    // A timed event without end or duration has no end.
    let invite = read(&event_cal("DTSTART:20261201T100000Z"), &berlin);
    assert_eq!(only(&invite).end, None);
}

// --- methods, people, text ---------------------------------------------------------------------

#[test]
fn cancellation() {
    let invite = read(
        &cal(
            "METHOD:CANCEL\nBEGIN:VEVENT\nUID:meeting-1\nSEQUENCE:3\nSTATUS:CANCELLED\n\
              SUMMARY:Abgesagt: Planung\nDTSTART;TZID=Europe/Berlin:20261201T100000\n\
              RECURRENCE-ID;TZID=Europe/Berlin:20261201T100000\nEND:VEVENT",
        ),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(invite.method.as_deref(), Some("CANCEL"));
    assert!(event.cancelled);
    assert_eq!(event.status, Some(EventStatus::Cancelled));
    assert_eq!(event.sequence, 3);
    assert_eq!(
        event.recurrence_id,
        Some(zoned(
            "2026-12-01T09:00:00Z",
            "2026-12-01T10:00:00",
            "Europe/Berlin"
        ))
    );
    // CANCEL needs no DTSTART; REQUEST does.
    let invite = read(
        &cal("METHOD:cancel\nBEGIN:VEVENT\nUID:meeting-1\nEND:VEVENT"),
        &berlin,
    );
    assert_eq!(invite.problems, []);
    assert!(only(&invite).cancelled);
    assert_eq!(only(&invite).start, None);
    let invite = read(
        &cal("METHOD:REQUEST\nBEGIN:VEVENT\nUID:meeting-1\nSTATUS:cancelled\nEND:VEVENT"),
        &berlin,
    );
    assert_eq!(codes(&invite), [Code::MissingStart]);
    assert!(only(&invite).cancelled);
}

#[test]
fn organizer_and_attendees() {
    let mut lines = String::from(
        "DTSTART:20261201T100000Z\n\
         ORGANIZER;CN=\"Müller, Anna\";SENT-BY=\"mailto:assist@example.de\":mailto:anna@example.de\n\
         ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=CHAIR;PARTSTAT=ACCEPTED;CN=Anna:mailto:anna@example.de\n\
         ATTENDEE;ROLE=NON-PARTICIPANT;PARTSTAT=DECLINED:MAILTO:bob@example.com?subject=x\n\
         ATTENDEE;PARTSTAT=TENTATIVE;EMAIL=carol@example.org;CN=Carol:urn:uuid:1234\n\
         ATTENDEE;PARTSTAT=DELEGATED;ROLE=X-SPECIAL;CN=Room 1:invalid:nomail\n",
    );
    for n in 0..56 {
        lines.push_str(&format!(
            "ATTENDEE;CN=Guest {n}:mailto:guest{n}@example.net\n"
        ));
    }
    let invite = read(&event_cal(lines.trim_end()), &berlin);
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(
        event.organizer,
        Some(CalendarPerson {
            name: Some("Müller, Anna".into()),
            email: Some("anna@example.de".into()),
            role: None,
            status: None,
        })
    );
    assert_eq!(event.attendee_count, 60);
    assert_eq!(event.attendees.len(), MAX_CALENDAR_ATTENDEES);
    assert_eq!(
        event.attendees[..4],
        [
            CalendarPerson {
                name: Some("Anna".into()),
                email: Some("anna@example.de".into()),
                role: Some(AttendeeRole::Chair),
                status: Some(ParticipationStatus::Accepted),
            },
            CalendarPerson {
                name: None,
                email: Some("bob@example.com".into()),
                role: Some(AttendeeRole::NonParticipant),
                status: Some(ParticipationStatus::Declined),
            },
            CalendarPerson {
                name: Some("Carol".into()),
                email: Some("carol@example.org".into()),
                role: None,
                status: Some(ParticipationStatus::Tentative),
            },
            CalendarPerson {
                name: Some("Room 1".into()),
                email: None,
                role: None,
                status: Some(ParticipationStatus::Delegated),
            },
        ]
    );
    assert_eq!(
        event.attendees[49].email.as_deref(),
        Some("guest45@example.net")
    );
}

#[test]
fn folded_lines_and_escaped_text() {
    let mut data = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:fold\r\n".to_vec();
    // Folded with space and tab, inside a UTF-8 character, escapes across a fold.
    data.extend_from_slice(
        b"SUMMARY:Besprechung in M\xC3\r\n \xBCnchen\\, Raum 3\\; Haus B \\\r\n \\ Pfad C:\\\\temp\r\n",
    );
    data.extend_from_slice(b"LOCATION:Gro\xC3\x9Fer Saal\\,\r\n\t2. OG\r\n");
    data.extend_from_slice(
        b"DESCRIPTION:Zeile 1\\nZeile 2\\N\\nTab\there\\, Ende\\\\n\r\n  mit Leerzeichen\r\n",
    );
    // LF only from here on.
    data.extend_from_slice(b"DTSTART:20261201T100000Z\nEND:VEVENT\nEND:VCALENDAR\n");
    let invite = read(&data, &berlin);
    let event = only(&invite);
    assert_eq!(invite.problems, []);
    assert_eq!(
        event.summary.as_deref(),
        Some("Besprechung in München, Raum 3; Haus B \\ Pfad C:\\temp")
    );
    assert_eq!(event.location.as_deref(), Some("Großer Saal,2. OG"));
    assert_eq!(
        event.description.as_deref(),
        Some("Zeile 1\nZeile 2\n\nTab\there, Ende\\n mit Leerzeichen")
    );
}

#[test]
fn long_descriptions_are_cut() {
    let text = "Wort ".repeat(2000);
    let invite = read(
        &event_cal(&format!("DTSTART:20261201T100000Z\nDESCRIPTION:{text}")),
        &berlin,
    );
    let description = only(&invite).description.clone().expect("description");
    assert!(description.len() <= MAX_CALENDAR_DESCRIPTION_BYTES + '…'.len_utf8());
    assert!(description.starts_with("Wort Wort"));
    assert!(description.ends_with('…'));
    assert_eq!(invite.problems, []);
}

// --- broken and hostile files ------------------------------------------------------------------

/// Deterministic pseudo-random bytes (xorshift).
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

#[test]
fn empty_and_garbage_files() {
    let invite = read(b"", &berlin);
    assert_eq!(
        invite,
        CalendarInvite {
            method: None,
            events: vec![],
            problems: vec![CalendarProblem {
                code: Code::NotCalendar,
                detail: None
            }],
        }
    );
    let invite = read(&noise(7, 64 << 10), &berlin);
    assert!(invite.events.is_empty());
    assert!(codes(&invite).contains(&Code::NotCalendar));
    assert!(codes(&invite).contains(&Code::MalformedText));
    let invite = read(
        b"Hello, this is a plain text file.\nNot a calendar.\n",
        &berlin,
    );
    assert_eq!(codes(&invite), [Code::BrokenLine, Code::NotCalendar]);
    let invite = read(
        b"BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nDTSTART:20261201\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        &berlin,
    );
    assert_eq!(codes(&invite), [Code::NoEvent]);
    // The public entry point with its guard gives the same.
    assert_eq!(read_calendar(b"", &berlin), read(b"", &berlin));
}

#[test]
fn truncated_files_still_show_their_events() {
    let full = outlook_invite();
    let text = String::from_utf8(full).expect("utf-8");
    // Cut right after DTEND: no END:VEVENT, no END:VCALENDAR.
    let cut = text.find("CLASS:PUBLIC").expect("marker");
    let invite = read(&text.as_bytes()[..cut], &berlin);
    let event = only(&invite);
    assert_eq!(codes(&invite), [Code::Truncated]);
    assert_eq!(event.summary.as_deref(), Some("Quartalsplanung"));
    assert!(event.end.is_some());
    // Cut anywhere: never a panic.
    for end in (0..text.len()).step_by(7) {
        let _ = read(&text.as_bytes()[..end], &berlin);
    }
    // Only END:VCALENDAR missing.
    let invite = read(
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261201\r\nEND:VEVENT\r\n",
        &berlin,
    );
    assert_eq!(codes(&invite), [Code::Truncated]);
    assert_eq!(invite.events.len(), 1);
}

#[test]
fn mismatched_and_misplaced_components() {
    let invite = read(
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261201\r\nEND:VCALENDAR\r\nEND:VEVENT\r\n",
        &berlin,
    );
    assert_eq!(invite.events.len(), 1);
    assert_eq!(codes(&invite), [Code::BrokenStructure]);
    // An event without the calendar around it is still read.
    let invite = read(
        b"BEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261201\r\nEND:VEVENT\r\n",
        &berlin,
    );
    assert_eq!(invite.events.len(), 1);
    assert_eq!(codes(&invite), [Code::BrokenStructure]);
    // An alarm's description does not replace the event's.
    let invite = read(
        &event_cal(
            "DTSTART:20261201\nBEGIN:VALARM\nDESCRIPTION:Reminder\nEND:VALARM\nDESCRIPTION:Real",
        ),
        &berlin,
    );
    assert_eq!(only(&invite).description.as_deref(), Some("Real"));
    // Deep nesting is cut off at the depth limit.
    let mut deep = b"BEGIN:VCALENDAR\r\n".to_vec();
    for _ in 0..10_000 {
        deep.extend_from_slice(b"BEGIN:VEVENT\r\n");
    }
    let invite = read(&deep, &berlin);
    assert!(codes(&invite).contains(&Code::BrokenStructure));
    assert!(codes(&invite).contains(&Code::Truncated));
    assert_eq!(invite.events.len(), 1);
}

#[test]
fn bad_dates() {
    let invite = read(
        &event_cal("DTSTART:20261332T250000\nDTEND:20261201T100000Z\nSUMMARY:Broken"),
        &berlin,
    );
    let event = only(&invite);
    assert_eq!(event.start, None);
    assert_eq!(event.end, None);
    assert_eq!(event.summary.as_deref(), Some("Broken"));
    assert_eq!(invite.problems, [problem(Code::BadDate, "20261332T250000")]);
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nDTEND:20260231T100000Z"),
        &berlin,
    );
    assert_eq!(only(&invite).end, None);
    assert_eq!(
        invite.problems,
        [problem(Code::BadDate, "20260231T100000Z")]
    );
    let invite = read(
        &event_cal("DTSTART:20261201T100000Z\nDTEND:20261201T090000Z"),
        &berlin,
    );
    assert_eq!(only(&invite).end, None);
    assert_eq!(codes(&invite), [Code::EndBeforeStart]);
    let invite = read(
        &event_cal("DTSTART;VALUE=DATE:20261201\nDTEND:20261201T090000Z"),
        &berlin,
    );
    assert_eq!(only(&invite).end, None);
    assert_eq!(
        invite.problems,
        [problem(Code::BadDate, "20261201T090000Z")]
    );
    let invite = read(&event_cal("DTSTART:"), &berlin);
    assert_eq!(codes(&invite), [Code::BadDate]);
}

#[test]
fn broken_lines_and_bytes() {
    let mut data =
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nthis line has no colon\r\n".to_vec();
    data.extend_from_slice(b"ATTENDEE;CN=\"unclosed:mailto:x@y\r\nSUMMARY:Caf\xE9\r\n");
    data.extend_from_slice(b"DTSTART:20261201\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
    let invite = read(&data, &berlin);
    let event = only(&invite);
    assert_eq!(codes(&invite), [Code::BrokenLine, Code::MalformedText]);
    assert_eq!(event.summary.as_deref(), Some("Caf\u{FFFD}"));
    assert_eq!(event.attendee_count, 0);
}

#[test]
fn huge_input_is_cut_at_the_limit() {
    let mut data = event_cal("DTSTART:20261201\nSUMMARY:First");
    while data.len() <= MAX_CALENDAR_BYTES * 2 {
        data.extend_from_slice(b"X-PADDING:0123456789012345678901234567890123456789\r\n");
    }
    let started = Instant::now();
    let invite = read(&data, &berlin);
    assert_eq!(only(&invite).summary.as_deref(), Some("First"));
    assert_eq!(
        invite.problems,
        [problem(Code::TooLarge, &format!("{} bytes", data.len()))]
    );
    // Garbage beyond the limit without any line break.
    let invite = read(&vec![b'A'; 3 << 20], &berlin);
    assert!(codes(&invite).contains(&Code::TooLarge));
    assert!(codes(&invite).contains(&Code::NotCalendar));
    assert!(started.elapsed().as_secs() < 10);
}

#[test]
fn a_line_of_one_mebibyte() {
    let mut data =
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261201\r\nDESCRIPTION:".to_vec();
    data.extend(std::iter::repeat_n(b'a', MAX_CALENDAR_BYTES - 200));
    data.extend_from_slice(b"\r\nX-ALT-DESC;FMTTYPE=text/html:");
    let invite = read(&data, &berlin);
    let event = only(&invite);
    assert_eq!(
        codes(&invite),
        [Code::LineTooLong, Code::Truncated],
        "{:?}",
        invite.problems
    );
    assert_eq!(invite.problems[0].detail.as_deref(), Some("DESCRIPTION"));
    let description = event.description.as_deref().expect("description");
    assert!(description.len() <= MAX_CALENDAR_DESCRIPTION_BYTES + 3);
    // A long line of a property that is not read (Outlook's HTML description, inline
    // attachments) is no problem.
    let mut long_html = "<p>x</p>".repeat(20_000);
    long_html.insert_str(0, "X-ALT-DESC;FMTTYPE=text/html:");
    let invite = read(
        &event_cal(&format!("DTSTART:20261201\n{long_html}")),
        &berlin,
    );
    assert_eq!(invite.problems, []);
}

#[test]
fn thousands_of_events() {
    let mut body = String::new();
    for n in 0..5000 {
        body.push_str(&format!(
            "BEGIN:VEVENT\nUID:{n}\nDTSTART;TZID=Europe/Berlin:20261201T100000\nSUMMARY:Event {n}\nEND:VEVENT\n"
        ));
    }
    let data = cal(body.trim_end());
    assert!(data.len() < MAX_CALENDAR_BYTES);
    let invite = read(&data, &berlin);
    assert_eq!(invite.events.len(), MAX_CALENDAR_EVENTS);
    assert_eq!(invite.events[19].summary.as_deref(), Some("Event 19"));
    assert_eq!(invite.problems, [problem(Code::TooManyEvents, "5000")]);
}

#[test]
fn too_many_lines() {
    let mut data = b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261201\r\n".to_vec();
    for _ in 0..MAX_CALENDAR_LINES {
        data.extend_from_slice(b"X:\n");
    }
    let invite = read(&data, &berlin);
    assert_eq!(invite.events.len(), 1);
    assert_eq!(
        invite.problems,
        [
            problem(Code::TooLarge, &format!("{MAX_CALENDAR_LINES} lines")),
            CalendarProblem {
                code: Code::Truncated,
                detail: None
            }
        ]
    );
}

#[test]
fn hostile_time_zones_stay_cheap() {
    // The maximum of definitions, observances and onsets, used by every time of every event,
    // each in a different year.
    let mut body = String::new();
    for zone in 0..20 {
        body.push_str(&format!("BEGIN:VTIMEZONE\nTZID:Z{zone}\n"));
        for observance in 0..32 {
            body.push_str(&format!(
                "BEGIN:STANDARD\nDTSTART:16010101T0{}0000\nTZOFFSETFROM:+0{}00\nTZOFFSETTO:+0{}00\n\
                 RRULE:FREQ=YEARLY;BYMONTH=1,2,3,4,5,6,7,8,9,10,11,12;BYDAY=MO,TU,WE,TH,FR,SA,SU,1MO,-1SU;BYMONTHDAY=1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,-1,-2\n\
                 RDATE:{}\nEND:STANDARD\n",
                observance % 10,
                observance % 10,
                (observance + 1) % 10,
                ["20200101T000000"; 64].join(",")
            ));
        }
        body.push_str("END:VTIMEZONE\n");
    }
    for event in 0..20 {
        let year = 2000 + event;
        body.push_str(&format!(
            "BEGIN:VEVENT\nUID:{event}\nDTSTART;TZID=Z{event}:{year}0601T100000\n\
             DTEND;TZID=Z{event}:{}0601T100000\nRECURRENCE-ID;TZID=Z{event}:{}0601T100000\n\
             RRULE:FREQ=DAILY;UNTIL={}0101T000000\nEND:VEVENT\n",
            year + 1,
            year + 2,
            year + 3
        ));
    }
    let data = cal(body.trim_end());
    assert!(data.len() < MAX_CALENDAR_BYTES);
    let started = Instant::now();
    let invite = read(&data, &no_zones);
    assert_eq!(invite.events.len(), MAX_CALENDAR_EVENTS);
    assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
}

#[test]
fn mutated_invitations_never_panic() {
    let original = outlook_invite();
    let random = noise(42, 4 * 4000);
    for round in 0..4000 {
        let mut data = original.clone();
        let pick = |i: usize| usize::from(random[(round * 4 + i) % random.len()]);
        let position = (pick(0) * 256 + pick(1)) % data.len();
        match pick(2) % 4 {
            0 => data[position] = random[round],
            1 => data.truncate(position),
            2 => data.insert(
                position,
                [b'\r', b'\n', b' ', b';', b':', b'"', b'\\', 0xFF][pick(3) % 8],
            ),
            _ => {
                data.remove(position);
            }
        }
        let _ = read(&data, &berlin);
        let _ = read(&data, &no_zones);
    }
}

// --- calendar parts of a message ---------------------------------------------------------------

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    let mut wrapped = String::new();
    for (i, c) in out.chars().enumerate() {
        if i > 0 && i % 76 == 0 {
            wrapped.push_str("\r\n");
        }
        wrapped.push(c);
    }
    wrapped
}

#[test]
fn calendar_parts_of_an_invitation_message() {
    let utf8 = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:m\r\n\
                SUMMARY:Treffen in München\r\nDTSTART:20261201T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let latin1 = utf8.replace("München", "M=FCnchen");
    let other = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:o\r\nSUMMARY:Other\r\nDTSTART:20261202\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let forwarded = "From: x@example.com\r\nSubject: fwd\r\nContent-Type: text/calendar\r\n\r\nBEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";
    let message = format!(
        "From: Erika <erika@example.de>\r\nTo: max@example.de\r\nSubject: Einladung\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n\
         --outer\r\nContent-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n\
         --alt\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nEinladung\r\n\
         --alt\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Einladung</p>\r\n\
         --alt\r\nContent-Type: text/calendar; charset=\"iso-8859-1\"; method=REQUEST\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\r\n{latin1}\r\n--alt--\r\n\
         --outer\r\nContent-Type: application/ics; name=\"invite.ics\"\r\n\
         Content-Disposition: attachment; filename=\"invite.ics\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{}\r\n\
         --outer\r\nContent-Type: application/octet-stream; name=\"other.ICS\"\r\n\
         Content-Disposition: attachment; filename=\"other.ICS\"\r\n\r\n{other}\r\n\
         --outer\r\nContent-Type: message/rfc822\r\n\r\n{forwarded}\r\n--outer--\r\n",
        base64(utf8.as_bytes())
    );
    let parts = calendar_parts(message.as_bytes());
    assert_eq!(parts.len(), 2, "{parts:?}");
    let first = String::from_utf8(parts[0].data.clone()).expect("utf-8");
    assert!(first.contains("SUMMARY:Treffen in München"), "{first}");
    assert_eq!(parts[1].data.trim_ascii(), other.as_bytes().trim_ascii());
    assert!(parts[0].part_id < parts[1].part_id);
    let invite = read(&parts[0].data, &berlin);
    assert_eq!(invite.method.as_deref(), Some("REQUEST"));
    assert_eq!(only(&invite).summary.as_deref(), Some("Treffen in München"));
    // The part id finds the same bytes for "open in calendar".
    let saved = crate::attachment_bytes(message.as_bytes(), parts[1].part_id).expect("part");
    assert_eq!(saved, parts[1].data);
}

#[test]
fn calendar_parts_of_other_messages() {
    assert_eq!(calendar_parts(b""), []);
    assert_eq!(calendar_parts(&noise(3, 10_000)), []);
    assert_eq!(
        calendar_parts(b"From: a@b\r\nSubject: hi\r\n\r\nNo calendar here.\r\n"),
        []
    );
    // A message that is itself a calendar (some servers send that).
    let parts = calendar_parts(
        b"From: a@b\r\nContent-Type: text/calendar; method=REQUEST\r\n\r\nBEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
    );
    assert_eq!(parts.len(), 1);
    assert!(parts[0].data.starts_with(b"BEGIN:VCALENDAR"));
    // Many distinct parts: capped. Empty parts are skipped.
    let mut message =
        String::from("From: a@b\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n");
    message.push_str("--b\r\nContent-Type: text/calendar\r\n\r\n\r\n");
    for n in 0..10 {
        message.push_str(&format!(
            "--b\r\nContent-Type: text/calendar\r\n\r\nBEGIN:VCALENDAR\r\nUID:{n}\r\nEND:VCALENDAR\r\n"
        ));
    }
    message.push_str("--b--\r\n");
    assert_eq!(calendar_parts(message.as_bytes()).len(), MAX_CALENDAR_PARTS);
}
