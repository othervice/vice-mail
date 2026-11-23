//! `RRULE` values (RFC 5545 section 3.3.10): parsed for the event description and for the yearly
//! rules of `VTIMEZONE` observances. Nothing is expanded except those yearly rules.

use jiff::civil::{Date, DateTime, Weekday};

use super::lines::short;

/// `FREQ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Freq {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A parsed rule. Invalid parts are left out; values outside their ranges are dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rule {
    pub freq: Option<Freq>,
    pub interval: u32,
    pub count: Option<u32>,
    /// `UNTIL` as written.
    pub until: Option<String>,
    pub by_day: Vec<(Option<i8>, Weekday)>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<i8>,
    pub by_set_pos: Vec<i16>,
    /// The value as written, cut.
    pub raw: String,
}

/// Entries kept per `BY…` list.
const MAX_LIST: usize = 64;
/// Bytes of the raw rule kept.
pub(super) const MAX_RULE_BYTES: usize = 256;

pub(super) fn parse_rule(value: &str) -> Rule {
    let mut rule = Rule {
        freq: None,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
        raw: short(value, MAX_RULE_BYTES),
    };
    for part in value.trim().split(';') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" if rule.freq.is_none() => rule.freq = parse_freq(val),
            "INTERVAL" => {
                rule.interval = val.parse::<u32>().ok().filter(|&i| i > 0).unwrap_or(1);
            }
            "COUNT" => rule.count = val.parse::<u32>().ok(),
            "UNTIL" => rule.until = Some(val.to_owned()),
            "BYDAY" => rule.by_day = list(val, parse_by_day),
            "BYMONTHDAY" => {
                rule.by_month_day = list(val, |v| {
                    v.parse::<i8>()
                        .ok()
                        .filter(|d| *d != 0 && (-31..=31).contains(d))
                });
            }
            "BYMONTH" => {
                rule.by_month = list(val, |v| {
                    v.parse::<i8>().ok().filter(|m| (1..=12).contains(m))
                });
            }
            "BYSETPOS" => {
                rule.by_set_pos = list(val, |v| {
                    v.parse::<i16>()
                        .ok()
                        .filter(|p| *p != 0 && (-366..=366).contains(p))
                });
            }
            _ => {}
        }
    }
    rule
}

/// Distinct valid entries, in order, at most [`MAX_LIST`].
fn list<T: PartialEq>(value: &str, parse: impl Fn(&str) -> Option<T>) -> Vec<T> {
    let mut out = Vec::new();
    for item in value.split(',').filter_map(|v| parse(v.trim())) {
        if out.len() >= MAX_LIST {
            break;
        }
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

fn parse_freq(value: &str) -> Option<Freq> {
    Some(match value.to_ascii_uppercase().as_str() {
        "SECONDLY" => Freq::Secondly,
        "MINUTELY" => Freq::Minutely,
        "HOURLY" => Freq::Hourly,
        "DAILY" => Freq::Daily,
        "WEEKLY" => Freq::Weekly,
        "MONTHLY" => Freq::Monthly,
        "YEARLY" => Freq::Yearly,
        _ => return None,
    })
}

/// `MO`, `-1SU`, `+2TU`: optional ordinal (1 to 53, signed) and a weekday.
fn parse_by_day(value: &str) -> Option<(Option<i8>, Weekday)> {
    let split = value.len().checked_sub(2)?;
    let (ordinal, day) = (value.get(..split)?, value.get(split..)?);
    let weekday = weekday(day)?;
    if ordinal.is_empty() {
        return Some((None, weekday));
    }
    let n = ordinal.parse::<i8>().ok()?;
    (n != 0 && (-53..=53).contains(&n)).then_some((Some(n), weekday))
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

pub(super) fn weekday_code(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "MO",
        Weekday::Tuesday => "TU",
        Weekday::Wednesday => "WE",
        Weekday::Thursday => "TH",
        Weekday::Friday => "FR",
        Weekday::Saturday => "SA",
        Weekday::Sunday => "SU",
    }
}

/// Onsets of a yearly rule per year; more are ignored (a time-zone rule has one).
const MAX_ONSETS_PER_YEAR: usize = 12;

/// The local date-times a yearly rule gives in `year`, at the time of day of `start`, in date
/// order (BYMONTH, then BYDAY with ordinals, or BYDAY filtered by BYMONTHDAY, or BYMONTHDAY, or
/// the day of `start`). Used for `VTIMEZONE` observances; BYSETPOS and other parts are ignored.
pub(super) fn yearly_onsets(rule: &Rule, start: DateTime, year: i16) -> Vec<DateTime> {
    let months: Vec<i8> = if rule.by_month.is_empty() {
        vec![start.month()]
    } else {
        rule.by_month.clone()
    };
    let plain: Vec<Weekday> = rule
        .by_day
        .iter()
        .filter(|(ordinal, _)| ordinal.is_none())
        .map(|&(_, weekday)| weekday)
        .collect();
    let mut days: Vec<Date> = Vec::new();
    for month in months {
        let Ok(first) = Date::new(year, month, 1) else {
            continue;
        };
        let length = first.days_in_month();
        // Which days of this month BYMONTHDAY names (negative: from the end).
        let mut named = [false; 32];
        for &day in &rule.by_month_day {
            let day = if day < 0 { length + 1 + day } else { day };
            if (1..=length).contains(&day)
                && let Some(slot) = named.get_mut(usize::from(day.unsigned_abs()))
            {
                *slot = true;
            }
        }
        let is_named = |day: i8| {
            named
                .get(usize::from(day.unsigned_abs()))
                .copied()
                .unwrap_or(false)
        };
        let date = |day: i8| Date::new(year, month, day).ok();
        if rule.by_day.is_empty() {
            if rule.by_month_day.is_empty() {
                days.extend(date(start.day()));
            } else {
                days.extend((1..=length).filter(|&d| is_named(d)).filter_map(date));
            }
        } else {
            for &(ordinal, weekday) in &rule.by_day {
                if let Some(n) = ordinal {
                    days.extend(first.nth_weekday_of_month(n, weekday).ok());
                }
            }
            if !plain.is_empty() {
                days.extend(
                    (1..=length)
                        .filter(|&d| rule.by_month_day.is_empty() || is_named(d))
                        .filter_map(date)
                        .filter(|d| plain.contains(&d.weekday())),
                );
            }
        }
        if days.len() >= MAX_ONSETS_PER_YEAR {
            break;
        }
    }
    days.sort_unstable();
    days.dedup();
    days.into_iter()
        .take(MAX_ONSETS_PER_YEAR)
        .map(|d| d.to_datetime(start.time()))
        .collect()
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    #[test]
    fn parses_rules() {
        let rule = parse_rule(
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,+2tu,-1SU,XX,0MO;UNTIL=20261231T235959Z;WKST=MO",
        );
        assert_eq!(rule.freq, Some(Freq::Weekly));
        assert_eq!(rule.interval, 2);
        assert_eq!(rule.until.as_deref(), Some("20261231T235959Z"));
        assert_eq!(
            rule.by_day,
            [
                (None, Weekday::Monday),
                (Some(2), Weekday::Tuesday),
                (Some(-1), Weekday::Sunday)
            ]
        );
        let rule =
            parse_rule("freq=monthly;bymonthday=1,-1,0,32;bymonth=13,2;bysetpos=-1;interval=0");
        assert_eq!(rule.freq, Some(Freq::Monthly));
        assert_eq!(rule.interval, 1);
        assert_eq!(rule.by_month_day, [1, -1]);
        assert_eq!(rule.by_month, [2]);
        assert_eq!(rule.by_set_pos, [-1]);
        assert_eq!(parse_rule("INTERVAL=2").freq, None);
        assert_eq!(parse_rule("").freq, None);
    }

    #[test]
    fn yearly_onsets_of_time_zone_rules() {
        let start = date(1601, 1, 1).at(3, 0, 0, 0);
        let eu_autumn = parse_rule("FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10");
        assert_eq!(
            yearly_onsets(&eu_autumn, start, 2026),
            [date(2026, 10, 25).at(3, 0, 0, 0)]
        );
        let us_spring = parse_rule("FREQ=YEARLY;BYDAY=2SU;BYMONTH=3");
        assert_eq!(
            yearly_onsets(&us_spring, start, 2026),
            [date(2026, 3, 8).at(3, 0, 0, 0)]
        );
        // Old style: the first Sunday on or after the 8th.
        let second_sunday =
            parse_rule("FREQ=YEARLY;BYMONTH=3;BYDAY=SU;BYMONTHDAY=8,9,10,11,12,13,14");
        assert_eq!(
            yearly_onsets(&second_sunday, start, 2026),
            [date(2026, 3, 8).at(3, 0, 0, 0)]
        );
        let fixed = parse_rule("FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=22");
        assert_eq!(
            yearly_onsets(&fixed, start, 2026),
            [date(2026, 3, 22).at(3, 0, 0, 0)]
        );
        let plain = parse_rule("FREQ=YEARLY");
        assert_eq!(
            yearly_onsets(&plain, date(2000, 4, 2).at(2, 0, 0, 0), 2026),
            [date(2026, 4, 2).at(2, 0, 0, 0)]
        );
        let leap = parse_rule("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=29");
        assert_eq!(yearly_onsets(&leap, start, 2026), []);
        let every_day = parse_rule("FREQ=YEARLY;BYDAY=MO,TU,WE,TH,FR,SA,SU");
        assert_eq!(
            yearly_onsets(&every_day, start, 2026).len(),
            MAX_ONSETS_PER_YEAR
        );
    }
}
