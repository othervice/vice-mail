//! The search language (stage 7, decision 0013): free text, phrases, `-` to exclude, `OR`
//! between text terms, and the filters of `docs/SPEC.md`. Pure logic without IO, used by the
//! engine, the store and later the MCP server.
//!
//! Two steps:
//! 1. [`parse`] turns the input into a [`ParsedQuery`]: the terms with their position in the
//!    input (for chips and error marks) and every problem found, each with its position. It never
//!    fails: problems are reported and the term is dropped or read as text, so the rest of the
//!    query still searches.
//! 2. [`ParsedQuery::compile`] turns that into a [`CompiledQuery`] for the store: one FTS5 `MATCH`
//!    expression (quoted, so no input can inject FTS5 syntax) plus the other filters as data
//!    (accounts, mailboxes, a `rowid` range for dates as in decision 0003, flags, attachments).
//!    SQL for the non-FTS parts is built by `mail-store`.
//!
//! Text is passed on as typed: case and diacritics are folded by the FTS5 tokenizer
//! (`unicode61 remove_diacritics 2`), not here. Only control characters count as whitespace.

mod compile;
mod date;
mod highlight;
mod parse;
mod server;
#[cfg(test)]
mod tests;

pub use compile::{CompileOptions, CompiledQuery, FtsColumn, RowidRange, TextMatch};
pub use date::{Age, AgeUnit, CivilDate, FixedOffset, TimeZone};
pub use highlight::{HighlightField, HighlightTerm, fold, highlight};
pub use parse::{
    Clause, Filter, MailboxSpec, ParsedQuery, SearchError, SearchErrorKind, Status, Term,
    TextField, TextTerm, parse,
};
pub use server::{ServerCriterion, ServerQuery};

/// Longest input in bytes; the rest is ignored and reported ([`SearchErrorKind::TooLong`]).
pub const MAX_QUERY_BYTES: usize = 2048;
/// Most terms per query; further terms are ignored and reported ([`SearchErrorKind::TooManyTerms`]).
pub const MAX_TERMS: usize = 64;
/// Shortest word before `*` (in characters). Shorter prefixes match too many index terms to be
/// fast without a prefix index (decision 0003 measured `rech*` at 12 ms over 500,000 mails).
pub const MIN_PREFIX_CHARS: usize = 3;

/// A range of the input in bytes (`start..end`), for slicing in Rust. The UI counts in UTF-16
/// code units: convert with [`Span::to_utf16`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Smallest span covering both.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// The same range in UTF-16 code units of `input` (JavaScript string indices). Offsets
    /// outside `input` or inside a character are moved to the nearest character boundary below.
    pub fn to_utf16(self, input: &str) -> (usize, usize) {
        (
            utf16_offset(input, self.start),
            utf16_offset(input, self.end),
        )
    }
}

fn utf16_offset(input: &str, byte: usize) -> usize {
    let mut byte = byte.min(input.len());
    while !input.is_char_boundary(byte) {
        byte -= 1;
    }
    input[..byte].encode_utf16().count()
}

/// How dates with slashes or dashes and the year last (`01/02/2026`) are read. ISO dates
/// (`2026-11-22`), dots (`22.11.2026`) and month names are unambiguous and ignore this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SlashDateOrder {
    /// `01/02/2026` is 1 February (German, British and most other locales).
    #[default]
    DayFirst,
    /// `01/02/2026` is 2 January (United States).
    MonthFirst,
}

/// What parsing needs to know about the user's world. Dates depend on it: `today`, `22.11.`
/// (current year) and `newer_than:7d` are relative to `now`, and calendar days start at local
/// midnight.
#[derive(Clone, Copy)]
pub struct SearchContext<'a> {
    /// Current time, Unix seconds.
    pub now: i64,
    /// The user's time zone.
    pub time_zone: &'a dyn TimeZone,
    pub slash_dates: SlashDateOrder,
}

impl std::fmt::Debug for SearchContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchContext")
            .field("now", &self.now)
            .field("utc_offset", &self.time_zone.utc_offset(self.now))
            .field("slash_dates", &self.slash_dates)
            .finish()
    }
}
