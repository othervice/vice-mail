//! Calendar arithmetic (proleptic Gregorian, no time zone database) and the date forms of the
//! search language.
//!
//! Accepted for `before:` and `after:` (decision 0013):
//! - ISO `2026-11-22`, also `2026/11/22` and `2026.11.22`
//! - German `22.11.2026`, `22.11.26`, `22.11.` (current year)
//! - year last with `/` or `-` (`11/22/2026`, `22-11-2026`): read by [`SlashDateOrder`], never guessed
//! - month names in German and English: `22. Nov 2026`, `22.Nov.2026`, `22-nov-2026`,
//!   `Nov 22, 2026`, `22. März`, `November 22` (without year: current year)
//! - `today`/`heute`, `yesterday`/`gestern`
//!
//! Two-digit years follow POSIX `%y`: 69–99 are 1969–1999, 00–68 are 2000–2068.

use super::{SearchContext, SlashDateOrder};

const SECONDS_PER_DAY: i64 = 86_400;
/// Largest amount in `older_than:`/`newer_than:`.
const MAX_AGE_AMOUNT: u32 = 99_999;
const MIN_YEAR: i32 = 1900;
const MAX_YEAR: i32 = 9999;

/// Offset of local time from UTC. The engine implements it from the system's time zone; it is
/// asked for the offset at a given instant, so dates across a daylight saving change start at
/// the right local midnight.
pub trait TimeZone {
    /// Seconds east of UTC at the instant `utc` (Unix seconds); Berlin in winter is 3600.
    fn utc_offset(&self, utc: i64) -> i32;
}

/// A time zone without daylight saving changes (UTC, tests, or the current offset when nothing
/// better is known).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FixedOffset(pub i32);

impl TimeZone for FixedOffset {
    fn utc_offset(&self, _utc: i64) -> i32 {
        self.0
    }
}

/// A calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl CivilDate {
    /// `None` unless the day exists (`31.02.` does not).
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month))
            .then_some(Self { year, month, day })
    }

    /// Days since 1970-01-01 (negative before). Howard Hinnant's `days_from_civil`.
    pub fn days(self) -> i64 {
        let (month, day) = (i64::from(self.month), i64::from(self.day));
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// Inverse of [`CivilDate::days`]. Saturates years outside `i32`.
    pub fn from_days(days: i64) -> Self {
        let z = days.saturating_add(719_468);
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            year: i32::try_from(year).unwrap_or(if year < 0 { i32::MIN } else { i32::MAX }),
            // Both are in range by construction.
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }

    /// The local day `now` falls on.
    pub fn today(now: i64, time_zone: &dyn TimeZone) -> Self {
        let local = now.saturating_add(i64::from(time_zone.utc_offset(now)));
        Self::from_days(local.div_euclid(SECONDS_PER_DAY))
    }

    /// Unix seconds of local midnight at the start of this day.
    pub fn start(self, time_zone: &dyn TimeZone) -> i64 {
        local_to_utc(self.days().saturating_mul(SECONDS_PER_DAY), time_zone)
    }

    /// The same day `months` months earlier; the 31st becomes the last day of a shorter month.
    fn months_earlier(self, months: i64) -> Self {
        let index = i64::from(self.year) * 12 + i64::from(self.month) - 1 - months;
        let year = index.div_euclid(12);
        let year = i32::try_from(year).unwrap_or(if year < 0 { i32::MIN } else { i32::MAX });
        let month = u8::try_from(index.rem_euclid(12) + 1).unwrap_or(1);
        let day = self.day.min(days_in_month(year, month));
        Self { year, month, day }
    }
}

const fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// UTC instant of a local wall-clock time (seconds since the local epoch). Two steps find the
/// offset in force at that instant; in a gap (clock jumps forward) the result is off by the jump.
fn local_to_utc(local: i64, time_zone: &dyn TimeZone) -> i64 {
    let guess = local.saturating_sub(i64::from(time_zone.utc_offset(local)));
    local.saturating_sub(i64::from(time_zone.utc_offset(guess)))
}

/// Unit of `older_than:`/`newer_than:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgeUnit {
    Day,
    Week,
    Month,
    Year,
}

/// `7d`, `2w`, `3m`, `1y`: a span of calendar time back from now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Age {
    pub amount: u32,
    pub unit: AgeUnit,
}

impl Age {
    /// The instant `self` before `now`, in calendar terms: `1m` on 31 March at 10:00 is
    /// 28/29 February at 10:00 local time; `7d` keeps the local time across a DST change.
    pub fn before(self, now: i64, time_zone: &dyn TimeZone) -> i64 {
        let local = now.saturating_add(i64::from(time_zone.utc_offset(now)));
        let (days, seconds) = (
            local.div_euclid(SECONDS_PER_DAY),
            local.rem_euclid(SECONDS_PER_DAY),
        );
        let amount = i64::from(self.amount);
        let days = match self.unit {
            AgeUnit::Day => days - amount,
            AgeUnit::Week => days - 7 * amount,
            AgeUnit::Month => CivilDate::from_days(days).months_earlier(amount).days(),
            AgeUnit::Year => CivilDate::from_days(days)
                .months_earlier(12 * amount)
                .days(),
        };
        local_to_utc(
            days.saturating_mul(SECONDS_PER_DAY).saturating_add(seconds),
            time_zone,
        )
    }
}

/// Reads `7d`, `2w`, `3m`, `1y` (also `t` for Tage and `j` for Jahre, any case).
pub(super) fn parse_age(value: &str) -> Option<Age> {
    let split = value.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = value.split_at(split);
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let amount: u32 = digits.parse().ok()?;
    let unit = match unit.to_ascii_lowercase().as_str() {
        "d" | "t" => AgeUnit::Day,
        "w" => AgeUnit::Week,
        "m" => AgeUnit::Month,
        "y" | "j" => AgeUnit::Year,
        _ => return None,
    };
    (1..=MAX_AGE_AMOUNT)
        .contains(&amount)
        .then_some(Age { amount, unit })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part<'a> {
    Number(&'a str),
    Word(&'a str),
}

/// Splits a date into numbers and words; returns them with the separator text before each
/// (the first is empty) and the separator text after the last.
fn split_date(value: &str) -> Option<(Vec<(&str, Part<'_>)>, &str)> {
    let mut parts = Vec::new();
    let mut separator_start = 0;
    let mut chars = value.char_indices().peekable();
    while let Some(&(start, c)) = chars.peek() {
        let digit = c.is_ascii_digit();
        if digit || c.is_alphabetic() {
            let mut end = start;
            while let Some(&(i, c)) = chars.peek() {
                if (digit && c.is_ascii_digit()) || (!digit && c.is_alphabetic()) {
                    end = i + c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let text = &value[start..end];
            let part = if digit {
                Part::Number(text)
            } else {
                Part::Word(text)
            };
            parts.push((&value[separator_start..start], part));
            separator_start = end;
        } else if matches!(c, '.' | ',' | '-' | '/') || c.is_whitespace() {
            chars.next();
        } else {
            return None;
        }
    }
    Some((parts, &value[separator_start..]))
}

fn month_number(word: &str) -> Option<u8> {
    let month = match word.to_lowercase().as_str() {
        "jan" | "january" | "januar" | "jän" | "jänner" => 1,
        "feb" | "february" | "februar" | "feber" => 2,
        "mar" | "march" | "mär" | "märz" | "mrz" | "maerz" => 3,
        "apr" | "april" => 4,
        "may" | "mai" => 5,
        "jun" | "june" | "juni" => 6,
        "jul" | "july" | "juli" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" | "okt" | "oktober" => 10,
        "nov" | "november" => 11,
        "dec" | "december" | "dez" | "dezember" => 12,
        _ => return None,
    };
    Some(month)
}

fn small_number(text: &str) -> Option<u8> {
    (1..=2)
        .contains(&text.len())
        .then(|| text.parse().ok())
        .flatten()
}

/// A year with four digits, or two digits by the POSIX rule.
fn year_number(text: &str) -> Option<i32> {
    let year: i32 = text.parse().ok()?;
    let year = match text.len() {
        4 => year,
        2 if year >= 69 => 1900 + year,
        2 => 2000 + year,
        _ => return None,
    };
    (MIN_YEAR..=MAX_YEAR).contains(&year).then_some(year)
}

/// Separator between month names and numbers: spaces, dots, commas, dashes.
fn is_word_separator(separator: &str) -> bool {
    !separator.is_empty()
        && separator
            .chars()
            .all(|c| matches!(c, '.' | ',' | '-') || c.is_whitespace())
}

/// Reads a date for `before:`/`after:`. `None` if it is not one of the accepted forms or the
/// day does not exist.
pub(super) fn parse_date(value: &str, context: &SearchContext<'_>) -> Option<CivilDate> {
    let today = CivilDate::today(context.now, context.time_zone);
    match value.to_lowercase().as_str() {
        "today" | "heute" => return Some(today),
        "yesterday" | "gestern" => return Some(CivilDate::from_days(today.days() - 1)),
        _ => {}
    }
    let (parts, trailing) = split_date(value)?;
    if !matches!(trailing, "" | ".") || parts.first().is_some_and(|(sep, _)| !sep.is_empty()) {
        return None;
    }
    let separators: Vec<&str> = parts.iter().skip(1).map(|(sep, _)| *sep).collect();
    let parts: Vec<Part<'_>> = parts.iter().map(|(_, part)| *part).collect();
    let (year, month, day) = match parts.as_slice() {
        // Numbers only: the separators must be equal and a single character.
        [Part::Number(a), Part::Number(b), rest @ ..]
            if rest.len() <= 1 && rest.iter().all(|p| matches!(p, Part::Number(_))) =>
        {
            let separator = separators[0];
            if separators.iter().any(|s| *s != separator) {
                return None;
            }
            let third = match rest {
                [Part::Number(c)] => Some(*c),
                _ => None,
            };
            match (separator, third) {
                (_, Some(c)) if a.len() == 4 => {
                    if !matches!(separator, "-" | "/" | ".") || !trailing.is_empty() {
                        return None;
                    }
                    (year_number(a)?, small_number(b)?, small_number(c)?)
                }
                (".", Some(c)) if trailing.is_empty() => {
                    (year_number(c)?, small_number(b)?, small_number(a)?)
                }
                (".", None) => (today.year, small_number(b)?, small_number(a)?),
                ("/" | "-", third) => {
                    if !trailing.is_empty() {
                        return None;
                    }
                    let year = match third {
                        Some(c) => year_number(c)?,
                        None => today.year,
                    };
                    let (first, second) = (small_number(a)?, small_number(b)?);
                    match context.slash_dates {
                        SlashDateOrder::DayFirst => (year, second, first),
                        SlashDateOrder::MonthFirst => (year, first, second),
                    }
                }
                _ => return None,
            }
        }
        // Day, month name, optional year.
        [Part::Number(d), Part::Word(m), rest @ ..] if rest.len() <= 1 => {
            if !separators.iter().all(|s| is_word_separator(s)) {
                return None;
            }
            let year = match rest {
                [Part::Number(y)] => year_number(y)?,
                [] => today.year,
                _ => return None,
            };
            (year, month_number(m)?, small_number(d)?)
        }
        // Month name, day, optional year.
        [Part::Word(m), Part::Number(d), rest @ ..] if rest.len() <= 1 => {
            if !separators.iter().all(|s| is_word_separator(s)) {
                return None;
            }
            let year = match rest {
                [Part::Number(y)] => year_number(y)?,
                [] => today.year,
                _ => return None,
            };
            (year, month_number(m)?, small_number(d)?)
        }
        _ => return None,
    };
    CivilDate::new(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTC: FixedOffset = FixedOffset(0);
    /// 2026-11-22 12:00:00 UTC.
    const NOW: i64 = 1_795_348_800;

    fn context(order: SlashDateOrder) -> SearchContext<'static> {
        SearchContext {
            now: NOW,
            time_zone: &UTC,
            slash_dates: order,
        }
    }

    /// (input, expected year-month-day).
    type DateCase = (&'static str, Option<(i32, u8, u8)>);

    fn date(year: i32, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).expect("valid date")
    }

    #[test]
    fn calendar_round_trips_and_known_days() {
        assert_eq!(date(1970, 1, 1).days(), 0);
        assert_eq!(date(2026, 11, 22).days(), 20_779);
        assert_eq!(date(2000, 2, 29).days(), 11_016);
        assert_eq!(date(1969, 12, 31).days(), -1);
        assert_eq!(CivilDate::today(NOW, &UTC), date(2026, 11, 22));
        for days in (-800_000..800_000).step_by(997) {
            assert_eq!(CivilDate::from_days(days).days(), days, "{days}");
        }
        assert_eq!(CivilDate::new(2026, 2, 29), None);
        assert_eq!(CivilDate::new(2024, 2, 29), Some(date(2024, 2, 29)));
        assert_eq!(CivilDate::new(1900, 2, 29), None);
        assert_eq!(CivilDate::new(2026, 13, 1), None);
        assert_eq!(CivilDate::new(2026, 4, 31), None);
        assert_eq!(CivilDate::new(2026, 1, 0), None);
    }

    #[test]
    fn local_midnight_follows_the_offset_in_force() {
        /// Berlin 2026: CEST (UTC+2) from 29 March 01:00 UTC to 25 October 01:00 UTC.
        struct Berlin;
        impl TimeZone for Berlin {
            fn utc_offset(&self, utc: i64) -> i32 {
                if (1_774_746_000..1_792_890_000).contains(&utc) {
                    7200
                } else {
                    3600
                }
            }
        }
        // 22.11.2026 00:00 CET = 21.11.2026 23:00 UTC.
        assert_eq!(date(2026, 11, 22).start(&Berlin), 1_795_302_000);
        // 1.7.2026 00:00 CEST = 30.6.2026 22:00 UTC.
        assert_eq!(date(2026, 7, 1).start(&Berlin), 1_782_856_800);
        // Local day of an instant shortly after midnight local time.
        assert_eq!(CivilDate::today(1_795_302_000, &Berlin), date(2026, 11, 22));
        assert_eq!(
            CivilDate::today(1_795_302_000 - 1, &Berlin),
            date(2026, 11, 21)
        );
        // Going back keeps the wall-clock time: from 1.11. 10:00 CET (09:00 UTC), 7d is
        // 25.10. 10:00 CET (after the change), 2w is 18.10. 10:00 CEST (08:00 UTC).
        let first_november_10 = 1_793_523_600;
        let age = |amount, unit| Age { amount, unit };
        assert_eq!(
            age(7, AgeUnit::Day).before(first_november_10, &Berlin),
            first_november_10 - 7 * 86_400
        );
        assert_eq!(
            age(2, AgeUnit::Week).before(first_november_10, &Berlin),
            first_november_10 - 14 * 86_400 - 3600
        );
    }

    #[test]
    fn ages_count_calendar_months_and_years() {
        let age = |amount, unit| Age { amount, unit };
        // 31 March 10:00 UTC minus one month: 28 February 10:00.
        let march_31 = date(2026, 3, 31).start(&UTC) + 36_000;
        assert_eq!(
            age(1, AgeUnit::Month).before(march_31, &UTC),
            date(2026, 2, 28).start(&UTC) + 36_000
        );
        assert_eq!(
            age(1, AgeUnit::Year).before(date(2024, 2, 29).start(&UTC), &UTC),
            date(2023, 2, 28).start(&UTC)
        );
        assert_eq!(
            age(3, AgeUnit::Month).before(NOW, &UTC),
            date(2026, 8, 22).start(&UTC) + 43_200
        );
        assert_eq!(age(7, AgeUnit::Day).before(NOW, &UTC), NOW - 7 * 86_400);
        assert_eq!(age(2, AgeUnit::Week).before(NOW, &UTC), NOW - 14 * 86_400);
        // The largest amount does not overflow.
        let far = age(MAX_AGE_AMOUNT, AgeUnit::Year).before(NOW, &UTC);
        assert!(far < 0);
    }

    #[test]
    fn age_forms() {
        let cases: &[(&str, Option<(u32, AgeUnit)>)] = &[
            ("7d", Some((7, AgeUnit::Day))),
            ("7D", Some((7, AgeUnit::Day))),
            ("10t", Some((10, AgeUnit::Day))),
            ("2w", Some((2, AgeUnit::Week))),
            ("3m", Some((3, AgeUnit::Month))),
            ("1y", Some((1, AgeUnit::Year))),
            ("1j", Some((1, AgeUnit::Year))),
            ("99999d", Some((99_999, AgeUnit::Day))),
            ("0d", None),
            ("100000d", None),
            ("d", None),
            ("7", None),
            ("7x", None),
            ("7dd", None),
            ("-7d", None),
            ("7 d", None),
            ("", None),
            ("٣d", None),
        ];
        for (input, expected) in cases {
            let got = parse_age(input).map(|a| (a.amount, a.unit));
            assert_eq!(got, *expected, "{input:?}");
        }
    }

    #[test]
    fn date_forms() {
        let day_first: &[DateCase] = &[
            ("2026-11-22", Some((2026, 11, 22))),
            ("2026/11/22", Some((2026, 11, 22))),
            ("2026.11.22", Some((2026, 11, 22))),
            ("2026-1-5", Some((2026, 1, 5))),
            ("22.11.2026", Some((2026, 11, 22))),
            ("1.3.2026", Some((2026, 3, 1))),
            ("22.11.26", Some((2026, 11, 22))),
            ("22.11.68", Some((2068, 11, 22))),
            ("22.11.69", Some((1969, 11, 22))),
            ("22.11.99", Some((1999, 11, 22))),
            ("22.11.", Some((2026, 11, 22))),
            ("22.11", Some((2026, 11, 22))),
            ("29.2.2024", Some((2024, 2, 29))),
            ("01/02/2026", Some((2026, 2, 1))),
            ("22/11/2026", Some((2026, 11, 22))),
            ("22-11-2026", Some((2026, 11, 22))),
            ("22/11/26", Some((2026, 11, 22))),
            ("22/11", Some((2026, 11, 22))),
            ("22. Nov 2026", Some((2026, 11, 22))),
            ("22.Nov.2026", Some((2026, 11, 22))),
            ("22-nov-2026", Some((2026, 11, 22))),
            ("22 November 2026", Some((2026, 11, 22))),
            ("22. November", Some((2026, 11, 22))),
            ("3. März 2026", Some((2026, 3, 3))),
            ("3. maerz 2026", Some((2026, 3, 3))),
            ("1. Mai 2026", Some((2026, 5, 1))),
            ("15. Okt. 2025", Some((2025, 10, 15))),
            ("24. Dez", Some((2026, 12, 24))),
            ("Nov 22 2026", Some((2026, 11, 22))),
            ("Nov 22, 2026", Some((2026, 11, 22))),
            ("November 22", Some((2026, 11, 22))),
            ("Dec 1, 25", Some((2025, 12, 1))),
            ("today", Some((2026, 11, 22))),
            ("Heute", Some((2026, 11, 22))),
            ("yesterday", Some((2026, 11, 21))),
            ("GESTERN", Some((2026, 11, 21))),
            // Invalid days and forms.
            ("31.02.2026", None),
            ("29.2.2026", None),
            ("2026-13-01", None),
            ("2026-11", None),
            ("2026", None),
            ("11/22/2026", None),
            ("22.11-2026", None),
            ("22.11.2026.", None),
            ("22..11.2026", None),
            (".22.11.2026", None),
            ("22.11.202", None),
            ("22.11.20261", None),
            ("123.11.2026", None),
            ("0.11.2026", None),
            ("22. Foo 2026", None),
            ("Nov", None),
            ("Nov 2026", None),
            ("22 Nov 2026 10:00", None),
            ("2026-11-22T10:00", None),
            ("22/Nov/2026", None),
            ("tomorrow", None),
            ("1.1.1899", None),
            ("", None),
            ("..", None),
            ("22.11.2026 extra", None),
        ];
        let ctx = context(SlashDateOrder::DayFirst);
        for (input, expected) in day_first {
            let got = parse_date(input, &ctx).map(|d| (d.year, d.month, d.day));
            assert_eq!(got, *expected, "{input:?}");
        }
        let month_first: &[DateCase] = &[
            ("01/02/2026", Some((2026, 1, 2))),
            ("11/22/2026", Some((2026, 11, 22))),
            ("11-22-26", Some((2026, 11, 22))),
            ("22/11/2026", None),
            // Unambiguous forms ignore the order.
            ("2026-01-02", Some((2026, 1, 2))),
            ("01.02.2026", Some((2026, 2, 1))),
        ];
        let ctx = context(SlashDateOrder::MonthFirst);
        for (input, expected) in month_first {
            let got = parse_date(input, &ctx).map(|d| (d.year, d.month, d.day));
            assert_eq!(got, *expected, "{input:?}");
        }
    }
}
