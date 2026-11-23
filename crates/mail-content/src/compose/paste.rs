//! Pasted HTML (from Word, Outlook, a web page) as editor HTML (stage 10).
//!
//! The clipboard's HTML is untrusted like a mail: it goes through the same allowlist as
//! outgoing mail HTML ([`crate::mail_html`]), so no script, style sheet, event attribute, form,
//! frame or external reference survives. Images are dropped (a pasted image refers to a web
//! address or a local file of another program; inline images come only from files the core
//! reads), links keep only `http`, `https` and `mailto` targets. Word's list bullets
//! (`mso-list:Ignore`) and conditional comments disappear; empty paragraphs (Word's spacing)
//! are dropped. [`SanitizeNotes`] says what was removed, so the composer can tell the user.

use super::html::{fragment, html_blocks_with, trim_paragraphs};
use crate::templates::{MAX_SNIPPET_INPUT_BYTES, SanitizeNotes, SnippetError, paste_notes};

/// Cleaned pasted content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedHtml {
    /// Editor HTML (allowlisted, without styles).
    pub html: String,
    pub notes: SanitizeNotes,
}

/// See module docs. Input larger than [`MAX_SNIPPET_INPUT_BYTES`] is refused.
pub fn clean_pasted_html(html: &str) -> Result<PastedHtml, SnippetError> {
    if html.len() > MAX_SNIPPET_INPUT_BYTES {
        return Err(SnippetError::TooLarge {
            bytes: html.len(),
            max: MAX_SNIPPET_INPUT_BYTES,
        });
    }
    let notes = paste_notes(html);
    // Not editor content: empty paragraphs (Word's spacing) and every image are dropped.
    let mut blocks = html_blocks_with(html, false);
    trim_paragraphs(&mut blocks);
    Ok(PastedHtml {
        html: fragment(&blocks),
        notes,
    })
}
