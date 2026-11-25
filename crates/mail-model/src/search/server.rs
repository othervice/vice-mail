//! The query as IMAP SEARCH criteria (RFC 3501 section 6.4.4), for "search on the server": the
//! messages whose content is not stored locally.
//!
//! The criteria are a superset of what the query means; the engine filters the found messages
//! locally afterwards with everything else (exact dates as a rowid range, `has:attachment`,
//! accounts, mailboxes):
//! - Text terms become `TEXT`, `FROM`, `TO`, `CC`, `SUBJECT` (substring matches on the server,
//!   so usually more than the local word search); `filename:` becomes `TEXT` (part headers are
//!   part of the text). `OR` groups become nested `OR`, excluded terms `NOT`.
//! - Dates become `SINCE`/`BEFORE` on whole days (the server's days): `after:D` is `SINCE D`,
//!   `before:D` is `BEFORE D`; ages are widened to the whole day.
//! - `is:unread`/`is:read`/`is:flagged` become `UNSEEN`/`SEEN`/`FLAGGED`/`UNFLAGGED`.
//! - `has:attachment` and `is:snoozed` have no IMAP form: they are reported as not on the server
//!   and only applied locally. `in:` and `account:` choose where to search.

use super::{CivilDate, Clause, Filter, ParsedQuery, SearchContext, Span, Status, TextField};

/// One IMAP search key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerCriterion {
    /// `TEXT`: header or body contains the string.
    Text(String),
    From(String),
    To(String),
    Cc(String),
    Subject(String),
    /// `SINCE`: internal date on or after the day.
    Since(CivilDate),
    /// `BEFORE`: internal date before the day.
    Before(CivilDate),
    /// `SEEN` (true) or `UNSEEN` (false).
    Seen(bool),
    /// `FLAGGED` or `UNFLAGGED`.
    Flagged(bool),
    Not(Box<ServerCriterion>),
    Or(Box<ServerCriterion>, Box<ServerCriterion>),
}

/// The criteria (all must match) and the terms the server cannot evaluate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerQuery {
    pub criteria: Vec<ServerCriterion>,
    /// Spans of terms applied locally only (`has:attachment`, `is:snoozed`).
    pub local_only: Vec<Span>,
}

impl ServerQuery {
    /// Nothing to send: without a criterion a SEARCH would list every message.
    pub fn is_empty(&self) -> bool {
        self.criteria.is_empty()
    }
}

impl ParsedQuery {
    /// The query as IMAP search criteria, see the module docs. Dates are resolved against
    /// `context` like [`ParsedQuery::compile`].
    pub fn server_query(&self, context: &SearchContext<'_>) -> ServerQuery {
        let mut out = ServerQuery::default();
        for clause in &self.clauses {
            match clause {
                Clause::AnyOf(terms) => {
                    let mut parts: Vec<ServerCriterion> = terms
                        .iter()
                        .filter_map(|t| match &t.filter {
                            Filter::Text(text) => Some(text_criterion(text.field, &text.text)),
                            _ => None,
                        })
                        .collect();
                    let Some(mut combined) = parts.pop() else {
                        continue;
                    };
                    while let Some(next) = parts.pop() {
                        combined = ServerCriterion::Or(Box::new(next), Box::new(combined));
                    }
                    out.criteria.push(combined);
                }
                Clause::Term(term) => {
                    let positive = !term.negated;
                    let criterion = match &term.filter {
                        Filter::Text(text) => Some(text_criterion(text.field, &text.text)),
                        Filter::Is(Status::Unread) => Some(ServerCriterion::Seen(!positive)),
                        Filter::Is(Status::Read) => Some(ServerCriterion::Seen(positive)),
                        Filter::Is(Status::Flagged) => Some(ServerCriterion::Flagged(positive)),
                        Filter::After(day) => Some(date(*day, true, positive)),
                        Filter::Before(day) => Some(date(*day, false, positive)),
                        Filter::NewerThan(age) => {
                            let instant = age.before(context.now, context.time_zone);
                            let day = CivilDate::today(instant, context.time_zone);
                            Some(widened(day, true, positive))
                        }
                        Filter::OlderThan(age) => {
                            let instant = age.before(context.now, context.time_zone);
                            let day = CivilDate::today(instant, context.time_zone);
                            Some(widened(day, false, positive))
                        }
                        Filter::HasAttachment | Filter::Is(Status::Snoozed) | Filter::Larger(_) => {
                            out.local_only.push(term.span);
                            None
                        }
                        Filter::In(_) | Filter::Account(_) => None,
                    };
                    let Some(criterion) = criterion else { continue };
                    // Flags and dates carry their polarity already.
                    let negate = !positive && matches!(term.filter, Filter::Text(_));
                    out.criteria.push(if negate {
                        ServerCriterion::Not(Box::new(criterion))
                    } else {
                        criterion
                    });
                }
            }
        }
        out
    }
}

fn text_criterion(field: TextField, text: &str) -> ServerCriterion {
    let text = text.to_owned();
    match field {
        TextField::Any | TextField::Filename => ServerCriterion::Text(text),
        TextField::From => ServerCriterion::From(text),
        TextField::To => ServerCriterion::To(text),
        TextField::Cc => ServerCriterion::Cc(text),
        TextField::Subject => ServerCriterion::Subject(text),
    }
}

/// `after:D` (lower) is `SINCE D`, `before:D` is `BEFORE D`; negated they swap.
fn date(day: CivilDate, lower: bool, positive: bool) -> ServerCriterion {
    if lower == positive {
        ServerCriterion::Since(day)
    } else {
        ServerCriterion::Before(day)
    }
}

/// An instant's day as a bound that includes the whole day: `SINCE day` for a lower bound,
/// `BEFORE day + 1` for an upper one.
fn widened(day: CivilDate, lower: bool, positive: bool) -> ServerCriterion {
    if lower == positive {
        ServerCriterion::Since(day)
    } else {
        ServerCriterion::Before(CivilDate::from_days(day.days() + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{FixedOffset, SlashDateOrder, parse};

    fn query(input: &str) -> ServerQuery {
        let ctx = SearchContext {
            // 2026-11-22 12:00 UTC
            now: 1_795_348_800,
            time_zone: &FixedOffset(3600),
            slash_dates: SlashDateOrder::DayFirst,
        };
        parse(input, &ctx).server_query(&ctx)
    }

    fn day(y: i32, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d).expect("date")
    }

    #[test]
    fn terms_become_search_keys() {
        use ServerCriterion::*;
        let q = query("rechnung from:bob -spam is:unread after:1.11.2026 -before:3.11.2026");
        assert_eq!(
            q.criteria,
            [
                Text("rechnung".into()),
                From("bob".into()),
                Not(Box::new(Text("spam".into()))),
                Seen(false),
                Since(day(2026, 11, 1)),
                Since(day(2026, 11, 3)),
            ]
        );
        assert!(q.local_only.is_empty());

        let q = query("a OR b OR subject:c is:read -is:flagged");
        assert_eq!(
            q.criteria,
            [
                Or(
                    Box::new(Text("a".into())),
                    Box::new(Or(
                        Box::new(Text("b".into())),
                        Box::new(Subject("c".into()))
                    ))
                ),
                Seen(true),
                Flagged(false),
            ]
        );

        // Ages cover the whole day; has:attachment is local only.
        let q = query("newer_than:2d older_than:1d has:attachment in:inbox account:x");
        assert_eq!(
            q.criteria,
            [Since(day(2026, 11, 20)), Before(day(2026, 11, 22))]
        );
        assert_eq!(q.local_only.len(), 1);
        assert!(query("in:inbox").is_empty());
    }
}
