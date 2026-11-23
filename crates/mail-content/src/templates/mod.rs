//! Signatures and templates (stage 10, decision 0016), without IO.
//!
//! - [`Signature`]: HTML from the signature editor (or pasted) cleaned with the same allowlist as
//!   outgoing mail HTML ([`crate::mail_html`]), and its plain text. [`SanitizeNotes`] says what
//!   was removed (images, colours, fonts, tables).
//! - [`Template`], [`fill_template`]: subject and body with placeholders (`{{first_name}}`,
//!   `{{date}}`, `{{name|fallback}}`), replaced as text so no value can inject HTML.
//! - [`assemble_draft`]: what the user wrote, the signature and the quote in order; the plain
//!   text gets the signature separator `-- `, and the signature stays above the quote.

mod draft;
mod signature;
mod snippet;
mod template;

#[cfg(test)]
mod tests;

pub use draft::{DraftBody, DraftParts, Quote, ReplyPosition, assemble_draft};
pub use signature::{MAX_SIGNATURE_BYTES, SIGNATURE_SEPARATOR, Signature};
pub(crate) use snippet::paste_notes;
pub use snippet::{MAX_SNIPPET_INPUT_BYTES, SanitizeNotes, SnippetError};
pub use template::{
    FilledTemplate, LocalDate, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_SUBJECT_CHARS, Placeholder,
    Template, TemplateProblem, TemplateValues, fill_template,
};
