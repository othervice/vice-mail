//! Composing (stage 5), without IO: the outgoing message as bytes, and replies and forwards
//! as editor content.
//!
//! - [`build_message`]: an [`OutgoingMessage`] to a [`BuiltMessage`]: the bytes for the SMTP
//!   server (no `Bcc`), the copy for the Sent folder (with `Bcc`), and the envelope.
//!   `multipart/alternative` (text and mail HTML), `multipart/mixed` with attachments.
//! - [`mail_html`]: editor HTML to mail HTML (small allowlist, inline styles, no classes).
//! - [`new_message_id`], [`normalize_email`]: Message-IDs from caller randomness and the
//!   sender's domain; checked addresses with ASCII domains.
//! - [`prepare_reply`], [`prepare_forward`]: recipients, identity, subject, References and the
//!   quote of an [`Original`]; [`reply_subject`], [`forward_subject`],
//!   [`strip_subject_prefixes`], [`reply_recipients`], [`choose_identity`],
//!   [`reply_references`] on their own.

mod address;
mod build;
mod html;
mod reply;

#[cfg(test)]
mod tests;

pub use address::{new_message_id, normalize_email};
pub use build::{Attachment, BuiltMessage, ComposeError, OutgoingMessage, build_message};
pub use html::mail_html;
// The document model of mail HTML, for signatures and templates (`crate::templates`).
pub(crate) use html::{Block, Inline, fragment, html_blocks, text_blocks, trim_paragraphs};
pub use reply::{
    MAX_REFERENCES, Original, PreparedDraft, QuoteLanguage, QuoteOptions, ReplyKind,
    choose_identity, forward_subject, prepare_forward, prepare_reply, reply_recipients,
    reply_references, reply_subject, strip_subject_prefixes,
};
