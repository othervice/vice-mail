//! Lexer and parser of the search language.
//!
//! Grammar (decision 0013), terms separated by whitespace:
//! - `word`, `"a phrase"`: free text in every indexed field; `word*`/`"phrase"*` matches the
//!   start of the last word (at least [`MIN_PREFIX_CHARS`] characters)
//! - `key:value`, `key:"value with spaces"`, also `key: value`; keys ignore case, German aliases
//!   `von an betreff datei ordner konto hat ist vor nach kontakt`
//! - `-term` excludes (also before filters and phrases)
//! - `a OR b` (upper case) between text terms; binds tighter than the implicit AND, so
//!   `a b OR c` is `a AND (b OR c)`. No parentheses. `AND` (upper case) may be written and
//!   changes nothing; `NOT` is an ordinary word (exclusion is `-`).
//!
//! A word that only looks like a filter stays text: `10:30` (the key has digits), `Re:` (no
//! value), `https://…` (value starts with `//`). An unknown key with a value (`form:bob`) is
//! reported and searched as text.

use super::date::{parse_age, parse_date};
use super::{Age, CivilDate, MAX_QUERY_BYTES, MAX_TERMS, MIN_PREFIX_CHARS, SearchContext, Span};
use crate::MailboxRole;

/// The result of [`parse`]: the terms, all of which must match, and every problem found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Clauses in input order; all must match.
    pub clauses: Vec<Clause>,
    /// Problems sorted by position. Empty for a well-formed query.
    pub errors: Vec<SearchError>,
    /// Length of the input that was read (shorter than the input only if it was too long).
    pub(super) input_len: usize,
    /// The input ends inside a term (no trailing whitespace): the user is still typing it.
    pub(super) open_end: bool,
}

impl ParsedQuery {
    /// Every term in input order, including the members of `OR` groups.
    pub fn terms(&self) -> impl Iterator<Item = &Term> {
        self.clauses.iter().flat_map(|clause| match clause {
            Clause::Term(term) => std::slice::from_ref(term),
            Clause::AnyOf(terms) => terms.as_slice(),
        })
    }

    /// No terms (empty input, or nothing usable in it).
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }
}

/// One condition of the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clause {
    Term(Term),
    /// `a OR b OR …`: at least one must match. Only positive text terms (free text, `from:`,
    /// `to:`, `cc:`, `subject:`, `filename:`), at least two.
    AnyOf(Vec<Term>),
}

impl Clause {
    /// From the first to the last term.
    pub fn span(&self) -> Span {
        match self {
            Clause::Term(term) => term.span,
            Clause::AnyOf(terms) => match (terms.first(), terms.last()) {
                (Some(first), Some(last)) => first.span.to(last.span),
                _ => Span::new(0, 0),
            },
        }
    }
}

/// One term as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub filter: Filter,
    /// Written with a leading `-`.
    pub negated: bool,
    /// The whole term: `-`, key, value and quotes.
    pub span: Span,
    /// The value without quotes (the text for free text).
    pub value_span: Span,
}

/// What a term asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// Free text or a text field filter; searched in the FTS index.
    Text(TextTerm),
    /// `in:` a mailbox.
    In(MailboxSpec),
    /// `account:` as typed (address, name or id); the engine resolves it.
    Account(String),
    /// `has:attachment`.
    HasAttachment,
    /// `is:unread`, `is:read`, `is:flagged`, `is:snoozed`.
    Is(Status),
    /// `before:` a day: sorted before local midnight at its start.
    Before(CivilDate),
    /// `after:` a day: on that day or later.
    After(CivilDate),
    /// `older_than:7d`: before the instant 7 days ago.
    OlderThan(Age),
    /// `newer_than:7d`: at or after the instant 7 days ago.
    NewerThan(Age),
    /// Stage 20 (decision 0060): `larger:10M`, the message is at least this many bytes
    /// (RFC822.SIZE); `-larger:` smaller than that. Units `K`, `M`, `G` (1024-based), `B`.
    Larger(u64),
    /// Stage 22 (decision 0074): `contact:` an address (as typed): mail from, to or in copy of
    /// any address of the contact that has it. The engine resolves the contact's addresses;
    /// a value that is no address is searched as words in sender, recipients and copy.
    Contact(String),
}

/// Text to find. Passed to FTS5 as one quoted string: several words in it are a phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextTerm {
    pub field: TextField,
    /// As typed (control characters become spaces), without quotes and the prefix `*`.
    pub text: String,
    /// Written in quotes.
    pub quoted: bool,
    /// The last word matches as a prefix (`rech*`).
    pub prefix: bool,
}

/// Where text is searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    /// Every indexed field (free text).
    Any,
    Subject,
    From,
    To,
    Cc,
    /// Attachment file names.
    Filename,
}

/// `in:` value. `role` is set when the name is a role word (`inbox`, `posteingang`, `sent`,
/// `gesendet`, …); the store then picks the mailbox with that role, otherwise it matches the
/// name (full path or last part, ignoring case).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MailboxSpec {
    pub name: String,
    pub role: Option<MailboxRole>,
}

/// `is:` values. `is:read` is the same as `-is:unread`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Unread,
    Read,
    Flagged,
    Snoozed,
}

/// A problem at a place in the input. The UI marks `span` and shows a text for `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchError {
    pub kind: SearchErrorKind,
    pub span: Span,
}

/// What is wrong. Every kind leaves the rest of the query usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchErrorKind {
    /// A `"` without its closing `"`; the phrase runs to the end of the input.
    UnclosedQuote,
    /// A filter without value (`from:`); the term is ignored.
    MissingValue,
    /// `key:value` with an unknown key (span: the key); searched as text.
    UnknownFilter,
    /// A value the filter does not know (`is:foo`, `has:foo`); the term is ignored.
    UnknownValue,
    /// Not a date (`before:31.02.2026`, `after:xyz`); the term is ignored.
    InvalidDate,
    /// Not an age like `7d`, `2w`, `3m`, `1y`; the term is ignored.
    InvalidAge,
    /// Neither letters nor digits (`!!!`, an emoji): nothing the index could find; ignored.
    NothingToSearch,
    /// `*` after fewer than [`MIN_PREFIX_CHARS`] characters; the word is searched whole.
    PrefixTooShort,
    /// A `-` with nothing after it; ignored.
    MissingTerm,
    /// `OR` at the start, at the end or twice; ignored.
    MisplacedOr,
    /// `OR` next to a filter other than text (`in:`, `is:`, dates, …); read as AND.
    OrNeedsText,
    /// `OR` next to an excluded term (`a OR -b`); read as AND.
    OrWithExclusion,
    /// Input longer than [`super::MAX_QUERY_BYTES`]; the rest is ignored.
    TooLong,
    /// More than [`super::MAX_TERMS`] terms; the rest is ignored.
    TooManyTerms,
}

/// Parses a query. Never fails: problems are listed in [`ParsedQuery::errors`] and the terms
/// that could be read are in [`ParsedQuery::clauses`].
pub fn parse(input: &str, context: &SearchContext<'_>) -> ParsedQuery {
    let mut errors = Vec::new();
    let mut len = input.len();
    if len > MAX_QUERY_BYTES {
        len = MAX_QUERY_BYTES;
        while !input.is_char_boundary(len) {
            len -= 1;
        }
        errors.push(SearchError {
            kind: SearchErrorKind::TooLong,
            span: Span::new(len, input.len()),
        });
    }
    let text = &input[..len];
    let mut lexer = Lexer {
        input: text,
        pos: 0,
    };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next(&mut errors) {
        tokens.push(token);
    }

    let mut entries = Vec::new();
    let mut items = 0;
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            Token::LoneDash(span) => errors.push(SearchError {
                kind: SearchErrorKind::MissingTerm,
                span,
            }),
            Token::Or(span) => entries.push(Entry::Or(span)),
            Token::And => {}
            Token::Item(mut item) => {
                if items == MAX_TERMS {
                    errors.push(SearchError {
                        kind: SearchErrorKind::TooManyTerms,
                        span: Span::new(item.span.start, text.len()),
                    });
                    break;
                }
                items += 1;
                // `from: bob`: a known filter without value takes the next plain term.
                let takes_next = item
                    .key
                    .as_ref()
                    .is_some_and(|(key, _)| known_key(key).is_some())
                    && !item.value.quoted
                    && item.value.text.is_empty()
                    && matches!(tokens.peek(), Some(Token::Item(next)) if next.key.is_none() && !next.negated);
                if takes_next && let Some(Token::Item(next)) = tokens.next() {
                    item.span = item.span.to(next.span);
                    item.value = next.value;
                }
                if let Some(term) = term(text, item, context, &mut errors) {
                    entries.push(Entry::Term(term));
                }
            }
        }
    }

    let clauses = group(entries, &mut errors);
    errors.sort_by_key(|e| (e.span.start, e.span.end));
    ParsedQuery {
        clauses,
        errors,
        input_len: len,
        open_end: len == input.len() && text.chars().next_back().is_some_and(|c| !is_space(c)),
    }
}

/// Whitespace and control characters separate terms.
fn is_space(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}

/// A character the FTS5 tokenizer (`unicode61`, categories L*, N*, Co) keeps in a token.
pub(super) fn is_token_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

/// Characters of the last word in `text`, counted as the tokenizer would split it.
pub(super) fn last_word_chars(text: &str) -> usize {
    text.chars()
        .rev()
        .skip_while(|&c| !is_token_char(c))
        .take_while(|&c| is_token_char(c))
        .count()
}

#[derive(Debug)]
enum Token {
    Or(Span),
    /// `AND` (upper case) is the default between terms and changes nothing.
    And,
    LoneDash(Span),
    Item(Item),
}

#[derive(Debug)]
struct Item {
    span: Span,
    negated: bool,
    /// The key as typed and its span.
    key: Option<(String, Span)>,
    value: Value,
}

#[derive(Debug)]
struct Value {
    text: String,
    /// Without quotes.
    span: Span,
    quoted: bool,
    /// `*` right after the closing quote.
    star: bool,
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn next(&mut self, errors: &mut Vec<SearchError>) -> Option<Token> {
        while let Some(c) = self.peek().filter(|&c| is_space(c)) {
            self.pos += c.len_utf8();
        }
        let start = self.pos;
        self.peek()?;
        let negated = self.peek() == Some('-');
        if negated {
            self.pos += 1;
            if self.peek().is_none_or(is_space) {
                return Some(Token::LoneDash(Span::new(start, self.pos)));
            }
        }
        if self.peek() == Some('"') {
            let value = self.quoted(errors);
            return Some(Token::Item(Item {
                span: Span::new(start, self.pos),
                negated,
                key: None,
                value,
            }));
        }
        let bare_start = self.pos;
        while let Some(c) = self.peek().filter(|&c| !is_space(c) && c != '"') {
            self.pos += c.len_utf8();
        }
        let bare_end = self.pos;
        let bare = &self.input[bare_start..bare_end];
        if !negated && bare == "OR" {
            return Some(Token::Or(Span::new(start, bare_end)));
        }
        if !negated && bare == "AND" {
            return Some(Token::And);
        }
        if let Some(colon) = bare.find(':') {
            let key = &bare[..colon];
            if !key.is_empty() && key.chars().all(|c| c.is_alphabetic() || c == '_') {
                let value_start = bare_start + colon + 1;
                let value = if value_start == bare_end && self.peek() == Some('"') {
                    self.quoted(errors)
                } else {
                    Value {
                        text: self.input[value_start..bare_end].to_owned(),
                        span: Span::new(value_start, bare_end),
                        quoted: false,
                        star: false,
                    }
                };
                return Some(Token::Item(Item {
                    span: Span::new(start, self.pos),
                    negated,
                    key: Some((key.to_owned(), Span::new(bare_start, bare_start + colon))),
                    value,
                }));
            }
        }
        Some(Token::Item(Item {
            span: Span::new(start, bare_end),
            negated,
            key: None,
            value: Value {
                text: bare.to_owned(),
                span: Span::new(bare_start, bare_end),
                quoted: false,
                star: false,
            },
        }))
    }

    /// A quoted string starting at the current `"`; reports a missing closing quote.
    fn quoted(&mut self, errors: &mut Vec<SearchError>) -> Value {
        let open = self.pos;
        let content_start = open + 1;
        let (content_end, next) = match self.input[content_start..].find('"') {
            Some(offset) => (content_start + offset, content_start + offset + 1),
            None => {
                errors.push(SearchError {
                    kind: SearchErrorKind::UnclosedQuote,
                    span: Span::new(open, self.input.len()),
                });
                (self.input.len(), self.input.len())
            }
        };
        self.pos = next;
        let star = self.peek() == Some('*');
        while self.peek() == Some('*') {
            self.pos += 1;
        }
        let text = self.input[content_start..content_end]
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        Value {
            text,
            span: Span::new(content_start, content_end),
            quoted: true,
            star,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Key {
    Text(TextField),
    In,
    Account,
    Has,
    Is,
    Before,
    After,
    OlderThan,
    NewerThan,
    Larger,
    Contact,
}

/// Stage 20: `10M`, `500k`, `2G`, `1,5M`/`1.5M`, `12345` (bytes), `20MB`: bytes, 1024-based;
/// `None` for anything else or more than 1 TiB.
fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_lowercase().replace(',', ".");
    let value = value.strip_suffix('b').unwrap_or(&value);
    let (number, factor) = match value.chars().last()? {
        'k' => (&value[..value.len() - 1], 1024_f64),
        'm' => (&value[..value.len() - 1], 1024_f64 * 1024.0),
        'g' => (&value[..value.len() - 1], 1024_f64 * 1024.0 * 1024.0),
        c if c.is_ascii_digit() => (value, 1.0),
        _ => return None,
    };
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let amount: f64 = number.parse().ok()?;
    let bytes = (amount * factor).round();
    // Within 1 TiB, so the cast below is exact enough and never wraps.
    (bytes.is_finite() && (0.0..=1_099_511_627_776.0).contains(&bytes)).then(|| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bytes = bytes as u64;
        bytes
    })
}

fn known_key(key: &str) -> Option<Key> {
    let key = match key.to_lowercase().as_str() {
        "from" | "von" => Key::Text(TextField::From),
        "to" | "an" => Key::Text(TextField::To),
        "cc" => Key::Text(TextField::Cc),
        "subject" | "betreff" => Key::Text(TextField::Subject),
        "filename" | "datei" => Key::Text(TextField::Filename),
        "in" | "ordner" => Key::In,
        "account" | "konto" => Key::Account,
        "has" | "hat" => Key::Has,
        "is" | "ist" => Key::Is,
        "before" | "vor" => Key::Before,
        "after" | "nach" => Key::After,
        "older_than" => Key::OlderThan,
        "newer_than" => Key::NewerThan,
        "larger" | "größer" | "groesser" => Key::Larger,
        "contact" | "kontakt" => Key::Contact,
        _ => return None,
    };
    Some(key)
}

fn mailbox_role(name: &str) -> Option<MailboxRole> {
    let role = match name.to_lowercase().as_str() {
        "inbox" | "posteingang" => MailboxRole::Inbox,
        "sent" | "gesendet" => MailboxRole::Sent,
        "drafts" | "draft" | "entwürfe" | "entwuerfe" | "entwurf" => MailboxRole::Drafts,
        "archive" | "archiv" => MailboxRole::Archive,
        "trash" | "papierkorb" => MailboxRole::Trash,
        "junk" | "spam" => MailboxRole::Junk,
        _ => return None,
    };
    Some(role)
}

fn status(value: &str) -> Option<Status> {
    let status = match value.to_lowercase().as_str() {
        "unread" | "ungelesen" => Status::Unread,
        "read" | "gelesen" => Status::Read,
        "flagged" | "starred" | "markiert" => Status::Flagged,
        "snoozed" | "zurückgestellt" | "zurueckgestellt" => Status::Snoozed,
        _ => return None,
    };
    Some(status)
}

fn is_attachment_word(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "attachment" | "attachments" | "anhang" | "anhänge" | "anhaenge"
    )
}

fn term(
    input: &str,
    item: Item,
    context: &SearchContext<'_>,
    errors: &mut Vec<SearchError>,
) -> Option<Term> {
    let Item {
        span,
        negated,
        key,
        value,
    } = item;
    let Some((key_text, key_span)) = key else {
        return text_term(TextField::Any, value, span, negated, errors);
    };
    let Some(key) = known_key(&key_text) else {
        // Not a filter: search the whole term as typed. Only report it when it was clearly
        // meant as one (`form:bob`), not for `Re:` or a URL.
        let plain = value.text.is_empty() || (!value.quoted && value.text.starts_with("//"));
        if !plain {
            errors.push(SearchError {
                kind: SearchErrorKind::UnknownFilter,
                span: key_span,
            });
        }
        let whole = Span::new(key_span.start, span.end);
        let raw = &input[whole.start..whole.end];
        let value = Value {
            text: raw
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect(),
            span: whole,
            quoted: false,
            star: false,
        };
        return text_term(TextField::Any, value, span, negated, errors);
    };
    let trimmed = value.text.trim();
    if trimmed.is_empty() {
        errors.push(SearchError {
            kind: SearchErrorKind::MissingValue,
            span,
        });
        return None;
    }
    let invalid = |kind| SearchError {
        kind,
        span: value.span,
    };
    let filter = match key {
        Key::Text(field) => return text_term(field, value, span, negated, errors),
        Key::In => Filter::In(MailboxSpec {
            name: trimmed.to_owned(),
            role: mailbox_role(trimmed),
        }),
        Key::Account => Filter::Account(trimmed.to_owned()),
        Key::Contact => {
            if !trimmed.chars().any(is_token_char) {
                errors.push(SearchError {
                    kind: SearchErrorKind::NothingToSearch,
                    span,
                });
                return None;
            }
            Filter::Contact(trimmed.to_owned())
        }
        Key::Has | Key::Is => {
            let filter = match key {
                Key::Has => is_attachment_word(trimmed).then_some(Filter::HasAttachment),
                _ => status(trimmed).map(Filter::Is),
            };
            let Some(filter) = filter else {
                errors.push(invalid(SearchErrorKind::UnknownValue));
                return None;
            };
            filter
        }
        Key::Before | Key::After => {
            let Some(date) = parse_date(trimmed, context) else {
                errors.push(invalid(SearchErrorKind::InvalidDate));
                return None;
            };
            if matches!(key, Key::Before) {
                Filter::Before(date)
            } else {
                Filter::After(date)
            }
        }
        Key::Larger => {
            let Some(bytes) = parse_size(trimmed) else {
                errors.push(invalid(SearchErrorKind::UnknownValue));
                return None;
            };
            Filter::Larger(bytes)
        }
        Key::OlderThan | Key::NewerThan => {
            let Some(age) = parse_age(trimmed) else {
                errors.push(invalid(SearchErrorKind::InvalidAge));
                return None;
            };
            if matches!(key, Key::OlderThan) {
                Filter::OlderThan(age)
            } else {
                Filter::NewerThan(age)
            }
        }
    };
    Some(Term {
        filter,
        negated,
        span,
        value_span: value.span,
    })
}

fn text_term(
    field: TextField,
    value: Value,
    span: Span,
    negated: bool,
    errors: &mut Vec<SearchError>,
) -> Option<Term> {
    let Value {
        mut text,
        span: mut value_span,
        quoted,
        star,
    } = value;
    let mut prefix = star;
    if !quoted {
        let kept = text.trim_end_matches('*').len();
        if kept < text.len() {
            prefix = true;
            text.truncate(kept);
            value_span.end = value_span.start + kept;
        }
    }
    if !text.chars().any(is_token_char) {
        errors.push(SearchError {
            kind: SearchErrorKind::NothingToSearch,
            span,
        });
        return None;
    }
    if prefix && last_word_chars(&text) < MIN_PREFIX_CHARS {
        errors.push(SearchError {
            kind: SearchErrorKind::PrefixTooShort,
            span,
        });
        prefix = false;
    }
    Some(Term {
        filter: Filter::Text(TextTerm {
            field,
            text,
            quoted,
            prefix,
        }),
        negated,
        span,
        value_span,
    })
}

enum Entry {
    Or(Span),
    Term(Term),
}

/// Joins terms around `OR` into groups; everything else is AND.
fn group(entries: Vec<Entry>, errors: &mut Vec<SearchError>) -> Vec<Clause> {
    let mut clauses: Vec<Clause> = Vec::new();
    let mut pending_or: Option<Span> = None;
    for entry in entries {
        match entry {
            Entry::Or(span) => {
                if clauses.is_empty() || pending_or.is_some() {
                    errors.push(SearchError {
                        kind: SearchErrorKind::MisplacedOr,
                        span,
                    });
                } else {
                    pending_or = Some(span);
                }
            }
            Entry::Term(term) => {
                let Some(or_span) = pending_or.take() else {
                    clauses.push(Clause::Term(term));
                    continue;
                };
                let problem = match clauses.last() {
                    Some(Clause::Term(previous)) => or_problem(previous).or(or_problem(&term)),
                    _ => or_problem(&term),
                };
                if let Some(kind) = problem {
                    errors.push(SearchError {
                        kind,
                        span: or_span,
                    });
                    clauses.push(Clause::Term(term));
                    continue;
                }
                match clauses.pop() {
                    Some(Clause::Term(previous)) => {
                        clauses.push(Clause::AnyOf(vec![previous, term]));
                    }
                    Some(Clause::AnyOf(mut terms)) => {
                        terms.push(term);
                        clauses.push(Clause::AnyOf(terms));
                    }
                    None => clauses.push(Clause::Term(term)),
                }
            }
        }
    }
    if let Some(span) = pending_or {
        errors.push(SearchError {
            kind: SearchErrorKind::MisplacedOr,
            span,
        });
    }
    clauses
}

/// Why `term` cannot stand next to `OR`, if it cannot.
fn or_problem(term: &Term) -> Option<SearchErrorKind> {
    if !matches!(term.filter, Filter::Text(_)) {
        Some(SearchErrorKind::OrNeedsText)
    } else if term.negated {
        Some(SearchErrorKind::OrWithExclusion)
    } else {
        None
    }
}
