//! Contacts (stage 10, decision 0016): a local directory from the addresses of sent and
//! received messages, without IO.
//!
//! - [`normalize_address`]: the key of an address (local part lower case, domain ASCII lower
//!   case via IDNA); plus addresses stay distinct unless the user merges them.
//! - [`ContactBook`]: [`ContactBook::observe`] adds what one message shows ([`observations`]:
//!   since stage 17 every address of From, To, Cc, Bcc and Reply-To); own addresses are left
//!   out, list and system addresses are marked ([`contact_kind`]). Names: the user's, else the
//!   most frequent seen display name, never an address ([`clean_display_name`]).
//! - [`Rank`]: frequency and recency with exponential decay; sent weighs more than received.
//! - [`ContactBook::search`]: prefix search over names and addresses, folded ([`fold`]).
//! - [`ContactBook::duplicates`]: suggestions only; [`ContactBook::merge`] merges what the user
//!   confirmed.
//! - [`split_name`]: first and last name, for template placeholders.

mod address;
mod book;
mod fold;
mod name;
mod rank;

#[cfg(test)]
mod tests;

pub use address::{NormalizedAddress, normalize_address, normalize_domain, unicode_domain};
pub use book::{
    Contact, ContactAddress, ContactBook, ContactError, ContactMatch, ContactMerge, Direction,
    DuplicateKind, DuplicateSuggestion, MAX_NAMES_PER_CONTACT, Observation, SeenName, Skipped,
    contact_kind, counted_before_stage17, observations,
};
pub use fold::{fold, fold_german};
pub use name::{MAX_NAME_CHARS, PersonName, clean_display_name, split_name};
pub use rank::{HALF_LIFE_SECS, RECEIVED_WEIGHT, Rank, SENT_WEIGHT};
