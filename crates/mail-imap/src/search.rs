//! `UID SEARCH` with criteria (stage 7, "search on the server").
//!
//! Strings go as quoted strings: the command layer sends one line per command (a literal would
//! need a continuation, and the read-only check refuses line breaks). Control characters become
//! spaces; `"` and `\` are escaped. A non-ASCII string adds `CHARSET UTF-8` (RFC 3501 6.4.4);
//! servers that refuse 8-bit quoted strings answer BAD, which reaches the engine as
//! `Rejected` for that mailbox.

use mail_model::search::{CivilDate, ServerCriterion};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The command line (without tag) for `criteria`; `ALL` without any.
pub(crate) fn search_line(criteria: &[ServerCriterion]) -> String {
    let mut keys = String::new();
    let mut utf8 = false;
    for criterion in criteria {
        if !keys.is_empty() {
            keys.push(' ');
        }
        write_key(&mut keys, criterion, &mut utf8);
    }
    if keys.is_empty() {
        keys.push_str("ALL");
    }
    if utf8 {
        format!("UID SEARCH CHARSET UTF-8 {keys}")
    } else {
        format!("UID SEARCH {keys}")
    }
}

fn write_key(out: &mut String, criterion: &ServerCriterion, utf8: &mut bool) {
    let mut string = |out: &mut String, key: &str, value: &str| {
        out.push_str(key);
        out.push(' ');
        quoted(out, value, utf8);
    };
    match criterion {
        ServerCriterion::Text(v) => string(out, "TEXT", v),
        ServerCriterion::From(v) => string(out, "FROM", v),
        ServerCriterion::To(v) => string(out, "TO", v),
        ServerCriterion::Cc(v) => string(out, "CC", v),
        ServerCriterion::Subject(v) => string(out, "SUBJECT", v),
        ServerCriterion::Since(day) => {
            out.push_str("SINCE ");
            out.push_str(&imap_date(*day));
        }
        ServerCriterion::Before(day) => {
            out.push_str("BEFORE ");
            out.push_str(&imap_date(*day));
        }
        ServerCriterion::Seen(true) => out.push_str("SEEN"),
        ServerCriterion::Seen(false) => out.push_str("UNSEEN"),
        ServerCriterion::Flagged(true) => out.push_str("FLAGGED"),
        ServerCriterion::Flagged(false) => out.push_str("UNFLAGGED"),
        ServerCriterion::Not(inner) => {
            out.push_str("NOT ");
            write_group(out, inner, utf8);
        }
        ServerCriterion::Or(a, b) => {
            out.push_str("OR ");
            write_group(out, a, utf8);
            out.push(' ');
            write_group(out, b, utf8);
        }
    }
}

/// One key; nested `OR`/`NOT` in parentheses, so the grammar stays unambiguous.
fn write_group(out: &mut String, criterion: &ServerCriterion, utf8: &mut bool) {
    let nested = matches!(criterion, ServerCriterion::Or(..) | ServerCriterion::Not(_));
    if nested {
        out.push('(');
    }
    write_key(out, criterion, utf8);
    if nested {
        out.push(')');
    }
}

fn quoted(out: &mut String, value: &str, utf8: &mut bool) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => {
                *utf8 |= !c.is_ascii();
                out.push(c);
            }
        }
    }
    out.push('"');
}

/// `date` in RFC 3501 form, `1-Nov-2026`. Years outside 1..=9999 are clamped.
fn imap_date(day: CivilDate) -> String {
    let month = MONTHS
        .get(usize::from(day.month.saturating_sub(1)))
        .copied()
        .unwrap_or("Jan");
    format!("{}-{month}-{}", day.day, day.year.clamp(1, 9999))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::permit;

    fn day(y: i32, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d).expect("date")
    }

    #[test]
    fn criteria_become_one_safe_line() {
        use ServerCriterion::*;
        let line = search_line(&[
            Text("rechnung \"2026\"".into()),
            From("bob\\x".into()),
            Not(Box::new(Subject(
                "a\r\nUID STORE 1 +FLAGS (\\Deleted)".into(),
            ))),
            Or(Box::new(Seen(false)), Box::new(Flagged(true))),
            Since(day(2026, 11, 1)),
            Before(day(2027, 1, 31)),
        ]);
        assert_eq!(
            line,
            "UID SEARCH TEXT \"rechnung \\\"2026\\\"\" FROM \"bob\\\\x\" \
             NOT SUBJECT \"a  UID STORE 1 +FLAGS (\\\\Deleted)\" OR UNSEEN FLAGGED \
             SINCE 1-Nov-2026 BEFORE 31-Jan-2027"
        );
        // A single line that passes the read-only allowlist.
        assert!(!line.contains(['\r', '\n']));
        assert!(permit(mail_model::Access::ReadOnly, &line).is_ok());

        assert_eq!(
            search_line(&[Text("Grüße".into())]),
            "UID SEARCH CHARSET UTF-8 TEXT \"Grüße\""
        );
        assert_eq!(search_line(&[]), "UID SEARCH ALL");
        assert_eq!(
            search_line(&[Not(Box::new(Or(
                Box::new(Text("a".into())),
                Box::new(Text("b".into()))
            )))]),
            "UID SEARCH NOT (OR TEXT \"a\" TEXT \"b\")"
        );
    }
}
