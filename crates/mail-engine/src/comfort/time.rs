//! Times in the user's zone: the date sections of the inbox and the snooze menu (stage 9).
//! Pure functions over a jiff time zone, so tests can pin zone and clock.
//!
//! - A **section** starts at local midnight: today (also for dates in the future), yesterday,
//!   earlier this week (Monday first), then the calendar month. Sections are monotone in the
//!   date, so the rows of one section stay contiguous in a newest-first list.
//! - **Snooze times** are local wall-clock times (08:00 tomorrow, Saturday 09:00, Monday 08:00);
//!   a local time skipped by a daylight-saving change moves forward, a repeated one takes the
//!   earlier instant (jiff's "compatible" rule). "Later today" adds three real hours and rounds
//!   up to the next full local hour; it is offered only while that is still today.

use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, ZonedDateTime as Zoned};
use mail_model::{CoreError, DateSection, DateSectionKind, SnoozeChoice, SnoozeTimes};

/// Earliest snooze: a minute ahead (a time in the past would return at once).
pub const MIN_SNOOZE_AHEAD_MS: i64 = 60_000;
/// Latest snooze: five years ahead.
pub const MAX_SNOOZE_AHEAD_MS: i64 = 5 * 366 * 24 * 3_600_000;

fn zoned(ms: i64, zone: &TimeZone) -> Option<Zoned> {
    Timestamp::from_millisecond(ms)
        .ok()
        .map(|ts| ts.to_zoned_datetime(zone.clone()))
}

/// Local midnight of `date` in Unix ms (a skipped midnight moves forward).
fn midnight(date: Date, zone: &TimeZone) -> Option<i64> {
    date.to_zoned_datetime(zone.clone())
        .ok()
        .map(|z| z.timestamp().as_millisecond())
}

fn at_local(date: Date, hour: i8, zone: &TimeZone) -> Option<i64> {
    let time = Time::new(hour, 0, 0, 0).ok()?;
    date.to_datetime(time)
        .to_zoned_datetime(zone.clone())
        .ok()
        .map(|z| z.timestamp().as_millisecond())
}

/// The section of a message dated `date_ms`, seen at `now_ms`.
pub fn section(date_ms: i64, now_ms: i64, zone: &TimeZone) -> DateSection {
    let fallback = DateSection {
        kind: DateSectionKind::Today,
        start: 0,
    };
    let (Some(now), Some(date)) = (zoned(now_ms, zone), zoned(date_ms, zone)) else {
        return fallback;
    };
    let today = now.date();
    let day = date.date();
    let yesterday = today.yesterday().unwrap_or(today);
    let monday = today
        .checked_sub(i64::from(today.weekday().to_monday_zero_offset()).days())
        .unwrap_or(today);
    let (kind, start_date) = if day >= today {
        (DateSectionKind::Today, today)
    } else if day == yesterday {
        (DateSectionKind::Yesterday, yesterday)
    } else if day >= monday {
        (DateSectionKind::ThisWeek, monday)
    } else {
        (DateSectionKind::Month, day.first_of_month())
    };
    DateSection {
        kind,
        start: midnight(start_date, zone).unwrap_or(0),
    }
}

/// The times the snooze menu offers at `now_ms`.
pub fn snooze_times(now_ms: i64, zone: &TimeZone) -> SnoozeTimes {
    let at = |choice| until(choice, now_ms, zone).ok();
    SnoozeTimes {
        later_today: at(SnoozeChoice::LaterToday),
        tomorrow: at(SnoozeChoice::Tomorrow).unwrap_or(now_ms + 24 * 3_600_000),
        weekend: at(SnoozeChoice::Weekend).unwrap_or(now_ms + 7 * 24 * 3_600_000),
        next_week: at(SnoozeChoice::NextWeek).unwrap_or(now_ms + 7 * 24 * 3_600_000),
    }
}

/// When a snooze made at `now_ms` with `choice` returns, Unix ms.
pub fn until(choice: SnoozeChoice, now_ms: i64, zone: &TimeZone) -> Result<i64, CoreError> {
    let invalid = |what: &str| CoreError::InvalidArgument(format!("snooze: {what}"));
    let now = zoned(now_ms, zone).ok_or_else(|| invalid("the clock is out of range"))?;
    let today = now.date();
    let result = match choice {
        SnoozeChoice::LaterToday => {
            let later = now_ms + 3 * 3_600_000;
            let later_zoned = zoned(later, zone).ok_or_else(|| invalid("out of range"))?;
            // Up to the next full local hour.
            let rounded = if later_zoned.minute() == 0
                && later_zoned.second() == 0
                && later_zoned.subsec_nanosecond() == 0
            {
                later
            } else {
                let next_hour = later_zoned
                    .with()
                    .minute(0)
                    .second(0)
                    .subsec_nanosecond(0)
                    .build()
                    .ok()
                    .and_then(|z| z.checked_add(1.hour()).ok())
                    .ok_or_else(|| invalid("out of range"))?;
                next_hour.timestamp().as_millisecond()
            };
            let same_day = zoned(rounded, zone).is_some_and(|z| z.date() == today);
            if !same_day {
                return Err(invalid("it is too late today"));
            }
            rounded
        }
        SnoozeChoice::Tomorrow => {
            let tomorrow = today.tomorrow().map_err(|_| invalid("out of range"))?;
            at_local(tomorrow, 8, zone).ok_or_else(|| invalid("out of range"))?
        }
        SnoozeChoice::Weekend => {
            let offset = match today.weekday() {
                Weekday::Saturday => 7,
                Weekday::Sunday => 6,
                other => 5 - i64::from(other.to_monday_zero_offset()),
            };
            let saturday = today
                .checked_add(offset.days())
                .map_err(|_| invalid("out of range"))?;
            at_local(saturday, 9, zone).ok_or_else(|| invalid("out of range"))?
        }
        SnoozeChoice::NextWeek => {
            let offset = 7 - i64::from(today.weekday().to_monday_zero_offset());
            let monday = today
                .checked_add(offset.days())
                .map_err(|_| invalid("out of range"))?;
            at_local(monday, 8, zone).ok_or_else(|| invalid("out of range"))?
        }
        SnoozeChoice::At { at } => at,
    };
    if result < now_ms + MIN_SNOOZE_AHEAD_MS {
        return Err(invalid("the time must be at least a minute ahead"));
    }
    if result > now_ms + MAX_SNOOZE_AHEAD_MS {
        return Err(invalid("the time must be within five years"));
    }
    Ok(result)
}

/// Minutes after local midnight at `now_ms` (for quiet hours).
pub fn minute_of_day(now_ms: i64, zone: &TimeZone) -> u16 {
    zoned(now_ms, zone).map_or(0, |z| {
        u16::try_from(i32::from(z.hour()) * 60 + i32::from(z.minute())).unwrap_or(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str) -> TimeZone {
        TimeZone::get(name).expect("tzdb")
    }

    /// Unix ms of a local wall-clock time.
    fn local(zone: &TimeZone, y: i16, mo: i8, d: i8, h: i8, mi: i8) -> i64 {
        jiff::civil::date(y, mo, d)
            .at(h, mi, 0, 0)
            .to_zoned_datetime(zone.clone())
            .expect("valid")
            .timestamp()
            .as_millisecond()
    }

    fn wall(ms: i64, zone: &TimeZone) -> String {
        zoned(ms, zone)
            .expect("in range")
            .strftime("%a %Y-%m-%d %H:%M %Z")
            .to_string()
    }

    #[test]
    fn menu_times_in_berlin() {
        let berlin = zone("Europe/Berlin");
        // Wednesday 2026-11-18 10:20.
        let now = local(&berlin, 2026, 11, 18, 10, 20);
        let w = |c| wall(until(c, now, &berlin).expect("valid"), &berlin);
        assert_eq!(w(SnoozeChoice::LaterToday), "Wed 2026-11-18 14:00 CET");
        assert_eq!(w(SnoozeChoice::Tomorrow), "Thu 2026-11-19 08:00 CET");
        assert_eq!(w(SnoozeChoice::Weekend), "Sat 2026-11-21 09:00 CET");
        assert_eq!(w(SnoozeChoice::NextWeek), "Mon 2026-11-23 08:00 CET");
        // On a full hour, "later today" stays on the hour.
        let on_the_hour = local(&berlin, 2026, 11, 18, 11, 0);
        assert_eq!(
            wall(
                until(SnoozeChoice::LaterToday, on_the_hour, &berlin).expect("ok"),
                &berlin
            ),
            "Wed 2026-11-18 14:00 CET"
        );
        // Late in the evening there is no "later today".
        let late = local(&berlin, 2026, 11, 18, 21, 30);
        assert!(until(SnoozeChoice::LaterToday, late, &berlin).is_err());
        assert_eq!(snooze_times(late, &berlin).later_today, None);
        // On the weekend, "weekend" is the next one and "next week" is Monday.
        let saturday = local(&berlin, 2026, 11, 21, 12, 0);
        assert_eq!(
            wall(
                until(SnoozeChoice::Weekend, saturday, &berlin).expect("ok"),
                &berlin
            ),
            "Sat 2026-11-28 09:00 CET"
        );
        let sunday = local(&berlin, 2026, 11, 22, 12, 0);
        assert_eq!(
            wall(
                until(SnoozeChoice::Weekend, sunday, &berlin).expect("ok"),
                &berlin
            ),
            "Sat 2026-11-28 09:00 CET"
        );
        assert_eq!(
            wall(
                until(SnoozeChoice::NextWeek, sunday, &berlin).expect("ok"),
                &berlin
            ),
            "Mon 2026-11-23 08:00 CET"
        );
        let monday = local(&berlin, 2026, 11, 23, 7, 0);
        assert_eq!(
            wall(
                until(SnoozeChoice::NextWeek, monday, &berlin).expect("ok"),
                &berlin
            ),
            "Mon 2026-11-30 08:00 CET"
        );
    }

    #[test]
    fn daylight_saving_changes() {
        let berlin = zone("Europe/Berlin");
        // Saturday before the spring change (2026-03-29, 02:00 → 03:00).
        let before = local(&berlin, 2026, 3, 28, 20, 0);
        let tomorrow = until(SnoozeChoice::Tomorrow, before, &berlin).expect("ok");
        assert_eq!(wall(tomorrow, &berlin), "Sun 2026-03-29 08:00 CEST");
        // 12 wall-clock hours, but only 11 real hours.
        assert_eq!(tomorrow - before, 11 * 3_600_000);
        // "Later today" at 00:30: three real hours later is 04:30 summer time, rounded to 05:00.
        let night = local(&berlin, 2026, 3, 29, 0, 30);
        assert_eq!(
            wall(
                until(SnoozeChoice::LaterToday, night, &berlin).expect("ok"),
                &berlin
            ),
            "Sun 2026-03-29 05:00 CEST"
        );
        // Autumn (2026-10-25, 03:00 → 02:00): Monday 08:00 is 25 real hours after Sunday 07:00.
        let sunday = local(&berlin, 2026, 10, 25, 7, 0);
        let monday = until(SnoozeChoice::NextWeek, sunday, &berlin).expect("ok");
        assert_eq!(wall(monday, &berlin), "Mon 2026-10-26 08:00 CET");
        assert_eq!(monday - sunday, 25 * 3_600_000);
        // A wall-clock time that does not exist (02:30 on the spring day) moves forward.
        let skipped = jiff::civil::date(2026, 3, 29)
            .at(2, 30, 0, 0)
            .to_zoned_datetime(berlin.clone())
            .expect("compatible");
        assert_eq!(skipped.strftime("%H:%M %Z").to_string(), "03:30 CEST");
    }

    #[test]
    fn other_zones() {
        // The same instant gives different local times: New York is still on Monday evening.
        let new_york = zone("America/New_York");
        let tokyo = zone("Asia/Tokyo");
        let now = local(&new_york, 2026, 11, 16, 20, 0);
        assert_eq!(
            wall(
                until(SnoozeChoice::Tomorrow, now, &new_york).expect("ok"),
                &new_york
            ),
            "Tue 2026-11-17 08:00 EST"
        );
        // In Tokyo it is already Tuesday 10:00, so tomorrow is Wednesday.
        assert_eq!(
            wall(
                until(SnoozeChoice::Tomorrow, now, &tokyo).expect("ok"),
                &tokyo
            ),
            "Wed 2026-11-18 08:00 JST"
        );
        // Between the US (2026-03-08) and the EU (2026-03-29) spring changes.
        let ny_spring = local(&new_york, 2026, 3, 8, 1, 30);
        assert_eq!(
            wall(
                until(SnoozeChoice::Tomorrow, ny_spring, &new_york).expect("ok"),
                &new_york
            ),
            "Mon 2026-03-09 08:00 EDT"
        );
        // UTC has no changes.
        let utc = TimeZone::UTC;
        let now = local(&utc, 2026, 6, 1, 23, 59);
        assert_eq!(
            wall(until(SnoozeChoice::Tomorrow, now, &utc).expect("ok"), &utc),
            "Tue 2026-06-02 08:00 UTC"
        );
    }

    #[test]
    fn custom_times_are_checked() {
        let berlin = zone("Europe/Berlin");
        let now = local(&berlin, 2026, 11, 18, 10, 0);
        assert!(until(SnoozeChoice::At { at: now - 1 }, now, &berlin).is_err());
        assert!(until(SnoozeChoice::At { at: now + 59_000 }, now, &berlin).is_err());
        assert_eq!(
            until(SnoozeChoice::At { at: now + 60_000 }, now, &berlin).expect("ok"),
            now + 60_000
        );
        assert!(
            until(
                SnoozeChoice::At {
                    at: now + MAX_SNOOZE_AHEAD_MS + 1
                },
                now,
                &berlin
            )
            .is_err()
        );
    }

    #[test]
    fn sections() {
        let berlin = zone("Europe/Berlin");
        // Thursday 2026-11-19 09:00.
        let now = local(&berlin, 2026, 11, 19, 9, 0);
        let kind = |ms| section(ms, now, &berlin).kind;
        assert_eq!(
            kind(local(&berlin, 2026, 11, 19, 0, 1)),
            DateSectionKind::Today
        );
        assert_eq!(kind(now + 3_600_000 * 48), DateSectionKind::Today);
        assert_eq!(
            kind(local(&berlin, 2026, 11, 18, 23, 59)),
            DateSectionKind::Yesterday
        );
        assert_eq!(
            kind(local(&berlin, 2026, 11, 16, 0, 0)),
            DateSectionKind::ThisWeek
        );
        assert_eq!(
            kind(local(&berlin, 2026, 11, 15, 23, 59)),
            DateSectionKind::Month
        );
        assert_eq!(
            section(local(&berlin, 2026, 11, 2, 12, 0), now, &berlin).start,
            local(&berlin, 2026, 11, 1, 0, 0)
        );
        assert_eq!(
            section(local(&berlin, 2026, 10, 31, 12, 0), now, &berlin).start,
            local(&berlin, 2026, 10, 1, 0, 0)
        );
        // Monotone: going back in time, the section start never grows.
        let mut last = i64::MAX;
        let mut t = now + 3_600_000;
        while t > local(&berlin, 2026, 6, 1, 0, 0) {
            let start = section(t, now, &berlin).start;
            assert!(start <= last, "monotone at {}", wall(t, &berlin));
            last = start;
            t -= 3 * 3_600_000 + 7 * 60_000;
        }
        // On a Monday, yesterday (Sunday) is its own section and "this week" starts today.
        let monday = local(&berlin, 2026, 11, 23, 9, 0);
        assert_eq!(
            section(local(&berlin, 2026, 11, 22, 9, 0), monday, &berlin).kind,
            DateSectionKind::Yesterday
        );
        assert_eq!(
            section(local(&berlin, 2026, 11, 21, 9, 0), monday, &berlin).kind,
            DateSectionKind::Month
        );
    }

    #[test]
    fn quiet_minutes() {
        let berlin = zone("Europe/Berlin");
        assert_eq!(
            minute_of_day(local(&berlin, 2026, 7, 1, 22, 15), &berlin),
            22 * 60 + 15
        );
        let quiet = mail_model::QuietHours {
            enabled: true,
            start_minute: 22 * 60,
            end_minute: 7 * 60,
        };
        assert!(quiet.contains(23 * 60));
        assert!(quiet.contains(0));
        assert!(quiet.contains(6 * 60 + 59));
        assert!(!quiet.contains(7 * 60));
        assert!(!quiet.contains(12 * 60));
        let day = mail_model::QuietHours {
            enabled: true,
            start_minute: 12 * 60,
            end_minute: 13 * 60,
        };
        assert!(day.contains(12 * 60 + 30) && !day.contains(13 * 60));
        assert!(!mail_model::QuietHours::default().contains(23 * 60));
    }
}
