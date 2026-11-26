//! From the parsed query to what the store runs: FTS5 `MATCH` expressions for the word index
//! and the trigram index, and the other filters as data.
//!
//! FTS5 safety: every text term becomes one string literal (`"…"`, inner quotes doubled), so
//! nothing typed can turn into FTS5 operators, column filters or `NEAR`. The only syntax in the
//! expressions comes from here: `AND`, `OR`, `NOT`, parentheses, `*` and the column filters of
//! [`FtsColumn`]. Terms without a single token character are never emitted, because FTS5 treats
//! an empty phrase as "matches nothing" (`"a" AND ""` finds nothing).
//!
//! Stage 22 (decision 0074): besides the word index `message_fts` there is a trigram index
//! `message_fts_tri` over the same columns, in which a string matches as a substring
//! (`"uster"` finds "Muster"). An unquoted term of at least [`MIN_SUBSTRING_CHARS`] characters
//! matches in either index; shorter terms and quoted ones only in the word index. Where every
//! word hit is a substring hit too (a single word, the trigram index complete), the term uses
//! the trigram index alone, so the store usually runs one `MATCH`.

use super::parse::{is_token_char, last_word_chars};
use super::{
    Clause, Filter, MIN_PREFIX_CHARS, MailboxSpec, ParsedQuery, SearchContext, Span, Status,
    TextField, TextTerm,
};

/// Columns of the FTS5 tables `message_fts` and `message_fts_tri`, in this order (decisions 0003
/// and 0074). `mail-store` must create the tables with exactly these names: `from:`, `to:`,
/// `cc:`, `subject:` and `filename:` are FTS5 column filters on them.
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

/// Shortest unquoted term (in characters) that also matches as a substring (decision 0074): the
/// trigram index cannot find anything shorter.
pub const MIN_SUBSTRING_CHARS: usize = 3;

/// Decision 0074: the trigram index holds the mail text as well, not only sender, recipients,
/// copy, subject and file names. `mail-store` writes the body into it exactly when this is set.
pub const SUBSTRING_BODY: bool = true;

/// How the trigram index is used (decision 0074).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Substrings {
    /// Word index only, as before decision 0074.
    Off,
    /// The trigram index may miss messages (it is being filled): a term that can match as a
    /// substring matches in either index, so word hits never go missing.
    #[default]
    Filling,
    /// The trigram index holds every message: a single word (only letters and digits) matches
    /// in the trigram index alone, since each of its word hits is a substring hit there too.
    Complete,
}

/// Options for [`ParsedQuery::compile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompileOptions {
    /// Search as you type: when the input ends inside an unquoted word (no trailing space), that
    /// word matches as a prefix (`rech` finds "Rechnung"), if it has at least
    /// [`MIN_PREFIX_CHARS`] characters.
    pub prefix_last_word: bool,
    /// Decision 0074: whether and how terms match inside words.
    pub substrings: Substrings,
}

/// Messages that match `words` in the word index (`message_fts MATCH words`) or `trigrams` in
/// the trigram index (`message_fts_tri MATCH trigrams`). Neither set: no messages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextClause {
    pub words: Option<String>,
    pub trigrams: Option<String>,
}

impl TextClause {
    /// Neither expression is set.
    pub fn is_empty(&self) -> bool {
        self.words.is_none() && self.trigrams.is_none()
    }
}

/// The text part of a query (decision 0074): every clause of `all` must match and `none` must
/// not. In what [`ParsedQuery::compile`] makes, at most one clause has only `words` and one only
/// `trigrams` (the terms of one index are joined with `AND`); the others have both, and the store
/// takes one clause to walk and checks the rest per message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextMatch {
    pub all: Vec<TextClause>,
    /// The exclusions (`-rechnung`), joined with `OR` per index.
    pub none: TextClause,
}

impl TextMatch {
    /// No text condition.
    pub fn is_any(&self) -> bool {
        self.all.is_empty() && self.none.is_empty()
    }

    /// Messages that match `expr` in the word index.
    pub fn words(expr: impl Into<String>) -> Self {
        Self {
            all: vec![TextClause {
                words: Some(expr.into()),
                trigrams: None,
            }],
            none: TextClause::default(),
        }
    }

    /// Messages that do not match `expr` in the word index.
    pub fn excluding_words(expr: impl Into<String>) -> Self {
        Self {
            all: Vec::new(),
            none: TextClause {
                words: Some(expr.into()),
                trigrams: None,
            },
        }
    }

    /// Adds a condition on the word index: joined with `AND` to the clause with only words, or a
    /// new one.
    pub fn require_words(&mut self, expr: String) {
        match self
            .all
            .iter_mut()
            .find(|c| c.trigrams.is_none() && c.words.is_some())
        {
            Some(clause) => {
                if let Some(words) = &mut clause.words {
                    *words = format!("{words} AND {expr}");
                }
            }
            None => self.all.insert(
                0,
                TextClause {
                    words: Some(expr),
                    trigrams: None,
                },
            ),
        }
    }

    /// Adds an exclusion on the word index (joined with `OR` to the others).
    pub fn exclude_words(&mut self, expr: String) {
        self.none.words = Some(match self.none.words.take() {
            Some(words) => format!("{words} OR {expr}"),
            None => expr,
        });
    }
}

/// Stage 22 (decision 0074): a `contact:` term, resolved by the engine to the contact's
/// addresses ([`contact_match`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactTerm {
    /// As typed (usually an address).
    pub value: String,
    pub negated: bool,
    pub span: Span,
}

/// Decision 0074: messages from, to or in copy of any of `addresses`, for the word index: each
/// address a phrase of its words in the columns `from`, `to` and `cc` (`max@firma.de` does not
/// find `jan@firma.de`, whose first word is another). `None` if no address has a token
/// character.
pub fn contact_match(addresses: &[String]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for address in addresses {
        if !address.chars().any(is_token_char) {
            continue;
        }
        let phrase = format!("{{from to cc}} : {}", quoted(address));
        if !parts.contains(&phrase) {
            parts.push(phrase);
        }
    }
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(format!("({})", parts.join(" OR "))),
    }
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
    /// Stage 22 (decision 0074): `contact:` terms; the engine adds them to `text`.
    pub contacts: Vec<ContactTerm>,
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
        let mut text = TextBuilder::default();
        for clause in &self.clauses {
            let term = match clause {
                Clause::AnyOf(terms) => {
                    let parts: Vec<TextClause> = terms
                        .iter()
                        .filter_map(|t| match &t.filter {
                            Filter::Text(text) => text_clause(
                                text,
                                typing_at == Some(t.span.end) && !t.negated,
                                options.substrings,
                            ),
                            _ => None,
                        })
                        .collect();
                    text.require(any_of(parts));
                    continue;
                }
                Clause::Term(term) => term,
            };
            let positive = !term.negated;
            let mut conflict = false;
            match &term.filter {
                Filter::Text(value) => {
                    let open = typing_at == Some(term.span.end) && positive;
                    if let Some(clause) = text_clause(value, open, options.substrings) {
                        if positive {
                            text.require(clause);
                        } else {
                            text.exclude(clause);
                        }
                    }
                }
                Filter::Contact(value) => out.contacts.push(ContactTerm {
                    value: value.clone(),
                    negated: !positive,
                    span: term.span,
                }),
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
        out.text = text.finish();
        out
    }
}

/// Collects the text clauses of a query (see [`TextMatch`]).
#[derive(Default)]
struct TextBuilder {
    words: Vec<String>,
    trigrams: Vec<String>,
    both: Vec<TextClause>,
    none_words: Vec<String>,
    none_trigrams: Vec<String>,
}

impl TextBuilder {
    fn require(&mut self, clause: TextClause) {
        match clause {
            TextClause {
                words: Some(words),
                trigrams: None,
            } => self.words.push(words),
            TextClause {
                words: None,
                trigrams: Some(trigrams),
            } => self.trigrams.push(trigrams),
            TextClause {
                words: Some(_),
                trigrams: Some(_),
            } => self.both.push(clause),
            TextClause {
                words: None,
                trigrams: None,
            } => {}
        }
    }

    fn exclude(&mut self, clause: TextClause) {
        self.none_words.extend(clause.words);
        self.none_trigrams.extend(clause.trigrams);
    }

    fn finish(self) -> TextMatch {
        let joined = |parts: Vec<String>, op: &str| (!parts.is_empty()).then(|| parts.join(op));
        let mut all = Vec::new();
        if let Some(words) = joined(self.words, " AND ") {
            all.push(TextClause {
                words: Some(words),
                trigrams: None,
            });
        }
        if let Some(trigrams) = joined(self.trigrams, " AND ") {
            all.push(TextClause {
                words: None,
                trigrams: Some(trigrams),
            });
        }
        all.extend(self.both);
        TextMatch {
            all,
            none: TextClause {
                words: joined(self.none_words, " OR "),
                trigrams: joined(self.none_trigrams, " OR "),
            },
        }
    }
}

/// An `OR` group: the members' word expressions joined, and their trigram expressions.
fn any_of(parts: Vec<TextClause>) -> TextClause {
    let group = |items: Vec<String>| match items.len() {
        0 => None,
        1 => items.into_iter().next(),
        _ => Some(format!("({})", items.join(" OR "))),
    };
    let (mut words, mut trigrams) = (Vec::new(), Vec::new());
    for part in parts {
        words.extend(part.words);
        trigrams.extend(part.trigrams);
    }
    TextClause {
        words: group(words),
        trigrams: group(trigrams),
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

/// One text term as its clause: the word index expression and, for an unquoted term of at
/// least [`MIN_SUBSTRING_CHARS`] characters, the trigram one (decision 0074). `None` if the text
/// has nothing the tokenizer would index.
fn text_clause(term: &TextTerm, open: bool, substrings: Substrings) -> Option<TextClause> {
    let words = fts_term(term, open)?;
    let trigrams = (substrings != Substrings::Off
        && !term.quoted
        && substring_chars(&term.text) >= MIN_SUBSTRING_CHARS)
        .then(|| column_filter(term.field.column(), &term.text));
    // A single word: each word (or prefix) hit is a substring hit of the same folded text.
    let covered = match term.field.column() {
        None | Some(FtsColumn::Body) => SUBSTRING_BODY,
        Some(_) => true,
    };
    let alone = substrings == Substrings::Complete
        && trigrams.is_some()
        && covered
        && term.text.chars().all(is_token_char);
    Some(TextClause {
        words: (!alone).then_some(words),
        trigrams,
    })
}

/// Characters the trigram tokenizer sees: combining marks fold away with the diacritics.
fn substring_chars(text: &str) -> usize {
    text.chars().filter(|c| !is_combining_mark(*c)).count()
}

/// The blocks of combining diacritical marks, which `remove_diacritics` drops.
pub(super) fn is_combining_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
}

/// `"text"` as an FTS5 string: inner quotes doubled, control characters as spaces.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\"\""),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `{column} : "text"`, or the string alone for every column.
fn column_filter(column: Option<FtsColumn>, text: &str) -> String {
    match column {
        Some(column) => format!("{{{}}} : {}", column.name(), quoted(text)),
        None => quoted(text),
    }
}

/// One text term as FTS5 syntax for the word index: `{column} : "text"`, with ` *` for a
/// prefix. `None` if the text has nothing the tokenizer would index.
fn fts_term(term: &TextTerm, open: bool) -> Option<String> {
    if !term.text.chars().any(is_token_char) {
        return None;
    }
    let mut out = column_filter(term.field.column(), &term.text);
    // While typing, only a word the cursor is still in (`rech|`, not `rech.` or `"rech"`).
    let prefix = term.prefix
        || (open && !term.quoted && term.text.chars().next_back().is_some_and(is_token_char));
    if prefix && last_word_chars(&term.text) >= MIN_PREFIX_CHARS {
        out.push_str(" *");
    }
    Some(out)
}
