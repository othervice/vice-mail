//! Stage 22 (decision 0077): the pure parts of the learning spam filter.
//!
//! - [`spam_tokens`]: the tokens of a message, as 64-bit hashes ([`spam_token_hash`]): words of
//!   the subject and the text, sender, Reply-To and Return-Path addresses and domains, the hosts
//!   of links, the names of the header fields, hints from `Received`, the verdicts of
//!   `Authentication-Results` and of a server's own spam filter, content types and attachment
//!   kinds. Each token counts once per message. [`spam_tokens_of_fields`] gives the subset that
//!   the stored header data of a message without a stored body allows; its tokens are spelled
//!   exactly like the full set's, so both can be learned and scored with one model.
//! - [`spam_score`]: the probability that a message is spam from the learned counts of its
//!   tokens: Gary Robinson's token probabilities with Fisher's chi-square combining, the method
//!   of SpamBayes and of Thunderbird's adaptive junk filter, with the SpamBayes defaults.
//!
//! Nothing here stores, fetches or decides what to do with a message; that is the engine's.

mod score;
mod tokens;

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod tests;

pub use score::{
    MAX_DISCRIMINATORS, MIN_PROB_STRENGTH, SpamCounts, SpamScore, SpamTotals, UNKNOWN_WORD_PROB,
    UNKNOWN_WORD_STRENGTH, spam_score, token_probability,
};
pub use tokens::{
    MAX_SPAM_TEXT_BYTES, MAX_SPAM_TOKENS, SpamFields, SpamTokens, spam_token_hash, spam_tokens,
    spam_tokens_of_fields, spam_tokens_of_raw,
};
