//! From the parsed query to what the store runs: one FTS5 `MATCH` expression and the other
//! filters as data.
//!
//! FTS5 safety: every text term becomes one string literal (`"…"`, inner quotes doubled), so
//! nothing typed can turn into FTS5 operators, column filters or `NEAR`. The only syntax in the
//! expression comes from here: `AND`, `OR`, `NOT`, parentheses, `*` and the column filters of
//! [`FtsColumn`]. Terms without a single token character are never emitted, because FTS5 treats
//! an empty phrase as "matches nothing" (`"a" AND ""` finds nothing).

use super::parse::{is_token_char, last_word_chars};
use super::{
    Clause, Filter, MIN_PREFIX_CHARS, MailboxSpec, ParsedQuery, SearchContext, Span, Status,
    TextField, TextTerm,
};

/// Columns of the FTS5 table `message_fts`, in this order (decision 0003). `mail-store` must
/// create the table with exactly these names: `from:`, `to:`, `cc:`, `subject:` and `filename:`
/// are FTS5 column filters on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FtsColumn {
    Subject,
    From,
    To,
    Cc,
    Body,
    /// Attachment file names.
    Attachments,
}

impl FtsColumn {
    pub const ALL: [FtsColumn; 6] = [
        Self::Subject,
        Self::From,
        Self::To,
        Self::Cc,
        Self::Body,
        Self::Attachments,
    ];

    /// Column name. `from` and `to` are SQL keywords: quote them in `CREATE VIRTUAL TABLE` and
    /// `INSERT`; inside a `MATCH` expression they are plain FTS5 barewords.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::From => "from",
            Self::To => "to",
            Self::Cc => "cc",
            Self::Body => "body",
            Self::Attachments => "attachments",
        }
    }
}

impl TextField {
    /// The FTS column searched; `None` for free text (all columns).
    pub const fn column(self) -> Option<FtsColumn> {
        match self {
            Self::Any => None,
            Self::Subject => Some(FtsColumn::Subject),
            Self::From => Some(FtsColumn::From),
            Self::To => Some(FtsColumn::To),
            Self::Cc => Some(FtsColumn::Cc),
            Self::Filename => Some(FtsColumn::Attachments),
        }
    }
}

/// Options for [`ParsedQuery::compile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompileOptions {
    /// Search as you type: when the input ends inside an unquoted word (no trailing space), that
    /// word matches as a prefix (`rech` finds "Rechnung"), if it has at least
    /// [`MIN_PREFIX_CHARS`] characters.
    pub prefix_last_word: bool,
}

/// The text part of a query for `message_fts MATCH ?`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TextMatch {
    /// No text condition.
    #[default]
    Any,
    /// `rowid IN (… WHERE message_fts MATCH expr)`: the expression may end in `NOT (…)`.
    Match(String),
    /// Only exclusions (`-rechnung`). FTS5 has no unary NOT, so the store keeps messages that do
    /// *not* match: `rowid NOT IN (SELECT rowid FROM message_fts WHERE message_fts MATCH expr)`.
    Exclude(String),
}

/// Date filter as a range of `message.id`, which is the sort date in Unix seconds (decision
/// 0003). Exact to the second except for messages whose id was moved up by a collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowidRange {
    /// `rowid >= from`.
    pub from: Option<i64>,
    /// `rowid < until`.
    pub until: Option<i64>,
}

impl RowidRange {
    /// No id can be in the range.
    pub fn is_empty(&self) -> bool {
        matches!((self.from, self.until), (Some(from), Some(until)) if from >= until)
    }

    fn at_least(&mut self, value: i64) {
        self.from = Some(self.from.map_or(value, |from| from.max(value)));
    }

    fn below(&mut self, value: i64) {
        self.until = Some(self.until.map_or(value, |until| until.min(value)));
    }
}

/// What the store needs to run a search. All conditions must hold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompiledQuery {
    pub text: TextMatch,
    /// `account:` values (address, name or id as typed); the engine resolves them.
    pub accounts: Vec<String>,
    /// `-account:` values.
    pub excluded_accounts: Vec<String>,
    /// `in:` mailboxes; the message must lie in each.
    pub mailboxes: Vec<MailboxSpec>,
    /// `-in:` mailboxes; the message must lie in none.
    pub excluded_mailboxes: Vec<MailboxSpec>,
    pub dates: RowidRange,
    /// `Some(true)`: unread somewhere (`is:unread`); `Some(false)`: `is:read`/`-is:unread`.
    pub unread: Option<bool>,
    pub flagged: Option<bool>,
    pub snoozed: Option<bool>,
    pub has_attachment: Option<bool>,
    /// Stage 20 (decision 0060): `larger:` (at least this many bytes) and `-larger:` (fewer).
    pub size_at_least: Option<u64>,
    pub size_below: Option<u64>,
    /// Terms that contradict earlier ones (`is:unread is:read`, `after:` later than `before:`,
    /// `in:inbox -in:inbox`). If any, the query has no results; the UI can mark these spans.
    pub conflicts: Vec<Span>,
}

impl CompiledQuery {
    /// Nothing can match; the store can skip the query.
    pub fn matches_nothing(&self) -> bool {
        !self.conflicts.is_empty() || self.dates.is_empty()
    }

    /// No condition at all (empty query).
    pub fn is_unrestricted(&self) -> bool {
        self == &Self::default()
    }
}

impl ParsedQuery {
    /// Turns the query into FTS5 text and filters. Dates are resolved against `context` (local
    /// midnight, `now` for ages).
    pub fn compile(&self, context: &SearchContext<'_>, options: CompileOptions) -> CompiledQuery {
        let mut out = CompiledQuery::default();
        let typing_at = (options.prefix_last_word && self.open_end).then_some(self.input_len);
        let mut positives = Vec::new();
        let mut negatives = Vec::new();
        for clause in &self.clauses {
            let term = match clause {
                Clause::AnyOf(terms) => {
                    let parts: Vec<String> = terms
                        .iter()
                        .filter_map(|t| match &t.filter {
                            Filter::Text(text) => {
                                fts_term(text, typing_at == Some(t.span.end) && !t.negated)
                            }
                            _ => None,
                        })
                        .collect();
                    match parts.len() {
                        0 => {}
                        1 => positives.extend(parts),
                        _ => positives.push(format!("({})", parts.join(" OR "))),
                    }
                    continue;
                }
                Clause::Term(term) => term,
            };
            let positive = !term.negated;
            let mut conflict = false;
            match &term.filter {
                Filter::Text(text) => {
                    let open = typing_at == Some(term.span.end) && positive;
                    if let Some(fts) = fts_term(text, open) {
                        if positive {
                            positives.push(fts);
                        } else {
                            negatives.push(fts);
                        }
                    }
                }
                Filter::In(spec) => {
                    let (list, other) = if positive {
                        (&mut out.mailboxes, &out.excluded_mailboxes)
                    } else {
                        (&mut out.excluded_mailboxes, &out.mailboxes)
                    };
                    conflict = other.iter().any(|m| same_mailbox(m, spec));
                    list.push(spec.clone());
                }
                Filter::Account(account) => {
                    let (list, other) = if positive {
                        (&mut out.accounts, &out.excluded_accounts)
                    } else {
                        (&mut out.excluded_accounts, &out.accounts)
                    };
                    conflict = other
                        .iter()
                        .any(|a| a.to_lowercase() == account.to_lowercase());
                    list.push(account.clone());
                }
                Filter::HasAttachment => conflict = !set(&mut out.has_attachment, positive),
                Filter::Larger(bytes) => {
                    if positive {
                        out.size_at_least =
                            Some(out.size_at_least.map_or(*bytes, |b| b.max(*bytes)));
                    } else {
                        out.size_below = Some(out.size_below.map_or(*bytes, |b| b.min(*bytes)));
                    }
                    conflict = matches!((out.size_at_least, out.size_below), (Some(low), Some(high)) if low >= high);
                }
                Filter::Is(Status::Unread) => conflict = !set(&mut out.unread, positive),
                Filter::Is(Status::Read) => conflict = !set(&mut out.unread, !positive),
                Filter::Is(Status::Flagged) => conflict = !set(&mut out.flagged, positive),
                Filter::Is(Status::Snoozed) => conflict = !set(&mut out.snoozed, positive),
                Filter::After(_)
                | Filter::Before(_)
                | Filter::NewerThan(_)
                | Filter::OlderThan(_) => {
                    let (instant, lower) = match &term.filter {
                        Filter::After(day) => (day.start(context.time_zone), true),
                        Filter::Before(day) => (day.start(context.time_zone), false),
                        Filter::NewerThan(age) => {
                            (age.before(context.now, context.time_zone), true)
                        }
                        Filter::OlderThan(age) => {
                            (age.before(context.now, context.time_zone), false)
                        }
                        _ => continue,
                    };
                    let was_empty = out.dates.is_empty();
                    // `-after:D` is `before:D`, `-newer_than:7d` is `older_than:7d`.
                    if lower == positive {
                        out.dates.at_least(instant);
                    } else {
                        out.dates.below(instant);
                    }
                    conflict = !was_empty && out.dates.is_empty();
                }
            }
            if conflict {
                out.conflicts.push(term.span);
            }
        }
        out.text = match (positives.is_empty(), negatives.is_empty()) {
            (true, true) => TextMatch::Any,
            (false, true) => TextMatch::Match(positives.join(" AND ")),
            (false, false) => TextMatch::Match(format!(
                "({}) NOT ({})",
                positives.join(" AND "),
                negatives.join(" OR ")
            )),
            (true, false) => TextMatch::Exclude(negatives.join(" OR ")),
        };
        out
    }
}

/// Sets a yes/no filter; `false` if it was already set to the opposite.
fn set(slot: &mut Option<bool>, value: bool) -> bool {
    match slot {
        Some(existing) => *existing == value,
        None => {
            *slot = Some(value);
            true
        }
    }
}

fn same_mailbox(a: &MailboxSpec, b: &MailboxSpec) -> bool {
    match (a.role, b.role) {
        (Some(x), Some(y)) => x == y,
        _ => a.name.to_lowercase() == b.name.to_lowercase(),
    }
}

/// One text term as FTS5 syntax: `{column} : "text"`, with ` *` for a prefix. `None` if the
/// text has nothing the tokenizer would index.
fn fts_term(term: &TextTerm, open: bool) -> Option<String> {
    if !term.text.chars().any(is_token_char) {
        return None;
    }
    let mut out = String::with_capacity(term.text.len() + 20);
    if let Some(column) = term.field.column() {
        out.push('{');
        out.push_str(column.name());
        out.push_str("} : ");
    }
    out.push('"');
    for c in term.text.chars() {
        match c {
            '"' => out.push_str("\"\""),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    // While typing, only a word the cursor is still in (`rech|`, not `rech.` or `"rech"`).
    let prefix = term.prefix
        || (open && !term.quoted && term.text.chars().next_back().is_some_and(is_token_char));
    if prefix && last_word_chars(&term.text) >= MIN_PREFIX_CHARS {
        out.push_str(" *");
    }
    Some(out)
}
