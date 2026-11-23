//! Putting a draft together: what the user wrote, the signature and the quote, in the order the
//! user chose, as editor HTML and as plain text with the signature separator.
//!
//! The composer keeps the three parts apart (the signature can be switched, the quote is its
//! own block) and calls [`assemble_draft`] for the message it saves or sends; the HTML goes to
//! [`crate::OutgoingMessage::html`], the text to [`crate::OutgoingMessage::text`]. A draft opened
//! again from the server has the signature in its body like any other text.

use super::signature::Signature;
use crate::compose::{Block, html_blocks};
use crate::text::html_to_text;

/// Where the answer goes relative to the quote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReplyPosition {
    /// Answer, then signature, then the quote ("top posting", the common default).
    #[default]
    AboveQuote,
    /// The quote, then the answer, then the signature.
    BelowQuote,
}

/// The quoted original: [`crate::PreparedDraft::html`] and [`crate::PreparedDraft::text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote<'a> {
    pub html: &'a str,
    pub text: &'a str,
}

/// The parts of a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftParts<'a> {
    /// Editor HTML of what the user wrote; empty for a new draft (an empty line is put where the
    /// cursor goes).
    pub body_html: &'a str,
    pub signature: Option<&'a Signature>,
    /// The quote of a reply or the forwarded message; forwards use [`ReplyPosition::AboveQuote`].
    pub quote: Option<Quote<'a>>,
    pub position: ReplyPosition,
}

/// A draft's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftBody {
    /// Editor HTML (an HTML fragment, allowlisted).
    pub html: String,
    /// Plain text; the signature follows a line `-- `, and stays above the quote unless the
    /// user answers below it.
    pub text: String,
}

/// See module docs.
pub fn assemble_draft(parts: &DraftParts<'_>) -> DraftBody {
    let mut body = html_blocks(parts.body_html);
    if body.is_empty() {
        body.push(Block::Paragraph(Vec::new()));
    }
    let signature = parts
        .signature
        .filter(|s| !s.is_empty())
        .map(Signature::blocks)
        .unwrap_or_default();
    let quote = parts.quote.map(|q| html_blocks(q.html)).unwrap_or_default();

    let mut blocks: Vec<Block> = Vec::new();
    if parts.position == ReplyPosition::BelowQuote {
        blocks.extend(quote.iter().cloned());
    }
    blocks.extend(body);
    blocks.extend(signature);
    if parts.position == ReplyPosition::AboveQuote {
        blocks.extend(quote);
    }
    let html = crate::compose::fragment(&blocks);

    let body_text = html_to_text(parts.body_html).trim_end().to_owned();
    let mut own = body_text;
    if let Some(signature) = parts.signature.filter(|s| !s.is_empty()) {
        own.push_str("\n\n");
        own.push_str(&signature.text_with_separator());
    }
    let quote_text = parts.quote.map(|q| q.text.trim_end()).unwrap_or_default();
    let text = match (parts.position, quote_text.is_empty()) {
        (_, true) => own,
        (ReplyPosition::AboveQuote, false) => format!("{own}\n\n{quote_text}"),
        (ReplyPosition::BelowQuote, false) => format!("{quote_text}\n\n{own}"),
    };
    DraftBody {
        html,
        text: format!("{}\n", text.trim_end_matches('\n')),
    }
}
