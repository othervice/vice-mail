//! Stored HTML snippets (signatures, templates) go through the allowlist of outgoing mail HTML
//! ([`crate::mail_html`]): only paragraphs, line breaks, bold, italic, underline, strike,
//! code, lists, quotes, preformatted text and `http`/`https`/`mailto` links remain. The notes
//! tell the user what was removed.

use std::fmt;

use crate::compose::{Block, fragment, html_blocks};
use crate::dom::{self, Descend, NodeData, Visit};
use crate::guard;

/// Largest input accepted (pasted HTML from word processors can be large).
pub const MAX_SNIPPET_INPUT_BYTES: usize = 2 << 20;

/// What sanitising changed, for a hint in the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizeNotes {
    /// Images (`img`, `picture`, `svg`) dropped: the allowlist has no images yet.
    pub images_removed: usize,
    /// Colours, fonts, sizes, alignment, tables or headings were simplified.
    pub formatting_simplified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnippetError {
    /// The input or the sanitised result is larger than allowed.
    TooLarge { bytes: usize, max: usize },
}

impl fmt::Display for SnippetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes, max } => {
                write!(f, "too large: {bytes} bytes, at most {max}")
            }
        }
    }
}

impl std::error::Error for SnippetError {}

/// The allowlisted blocks of `html`, refused when the result is over `max` bytes as an editor
/// fragment.
pub(crate) fn sanitize(
    html: &str,
    max: usize,
) -> Result<(Vec<Block>, SanitizeNotes), SnippetError> {
    if html.len() > MAX_SNIPPET_INPUT_BYTES {
        return Err(SnippetError::TooLarge {
            bytes: html.len(),
            max: MAX_SNIPPET_INPUT_BYTES,
        });
    }
    let notes = notes(html);
    let blocks = html_blocks(html);
    check_size(&blocks, max)?;
    Ok((blocks, notes))
}

pub(crate) fn check_size(blocks: &[Block], max: usize) -> Result<String, SnippetError> {
    let out = fragment(blocks);
    if out.len() > max {
        return Err(SnippetError::TooLarge {
            bytes: out.len(),
            max,
        });
    }
    Ok(out)
}

/// Elements whose look the allowlist does not keep.
const STYLED_ELEMENTS: &[&str] = &[
    "font", "table", "h1", "h2", "h3", "h4", "h5", "h6", "center", "hr", "sup", "sub", "small",
    "big", "mark", "img",
];
const STYLE_ATTRIBUTES: &[&str] = &[
    "style",
    "class",
    "color",
    "face",
    "size",
    "bgcolor",
    "align",
    "background",
    "width",
    "height",
];

/// What cleaning pasted HTML removes (the same report as for signatures and templates).
/// Pasted content keeps no image at all, not even a `cid:` one (it points into another mail).
pub(crate) fn paste_notes(html: &str) -> SanitizeNotes {
    notes_with(html, false)
}

fn notes(html: &str) -> SanitizeNotes {
    notes_with(html, true)
}

fn notes_with(html: &str, keep_cid_images: bool) -> SanitizeNotes {
    if guard::check(html).is_err() {
        return SanitizeNotes {
            images_removed: 0,
            formatting_simplified: true,
        };
    }
    let parsed = dom::parse_document(html);
    let mut notes = SanitizeNotes::default();
    dom::walk(&parsed.document, |visit| {
        if let Visit::Enter(node) = visit
            && let NodeData::Element { name, attrs, .. } = &node.data
        {
            let tag = name.local.as_ref();
            // Inline images of the message itself (`cid:`, stage 10) stay.
            let kept_image = keep_cid_images
                && tag == "img"
                && attrs.borrow().iter().any(|a| {
                    &*a.name.local == "src" && crate::compose::content_id_of(&a.value).is_some()
                });
            if matches!(tag, "img" | "picture" | "svg") && !kept_image {
                notes.images_removed += 1;
            }
            // Editor content keeps headings h1–h3 (stage 10).
            let kept_heading = keep_cid_images && matches!(tag, "h1" | "h2" | "h3");
            if kept_image {
                return Descend::Yes;
            }
            if (STYLED_ELEMENTS.contains(&tag) && !kept_heading)
                || attrs
                    .borrow()
                    .iter()
                    .any(|a| STYLE_ATTRIBUTES.contains(&a.name.local.as_ref()))
            {
                notes.formatting_simplified = true;
            }
            if tag == "svg" {
                return Descend::Skip;
            }
        }
        Descend::Yes
    });
    notes
}
