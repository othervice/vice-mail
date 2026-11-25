//! Composing (stage 5), without IO: the outgoing message as bytes, and replies and forwards
//! as editor content.
//!
//! - [`build_message`]: an [`OutgoingMessage`] to a [`BuiltMessage`]: the bytes for the SMTP
//!   server (no `Bcc`), the copy for the Sent folder (with `Bcc`), and the envelope.
//!   `multipart/alternative` (text and mail HTML), `multipart/mixed` with attachments.
//! - [`mail_html`]: editor HTML to mail HTML (small allowlist, inline styles, no classes);
//!   inline images only as `cid:` parts ([`InlineImage`], `multipart/related`, stage 10).
//! - [`clean_pasted_html`]: pasted HTML (Word, web pages) as editor HTML, same allowlist.
//! - [`new_message_id`], [`normalize_email`]: Message-IDs from caller randomness and the
//!   sender's domain; checked addresses with ASCII domains.
//! - [`prepare_reply`], [`prepare_forward`]: recipients, identity, subject, References and the
//!   quote of an [`Original`]; [`reply_subject`], [`forward_subject`],
//!   [`strip_subject_prefixes`], [`reply_recipients`], [`choose_identity`],
//!   [`reply_references`] on their own.
//! - [`build_redirect`]: a message redirected to new recipients with `Resent-*` fields
//!   (stage 19, decision 0055).

mod address;
mod build;
mod html;
mod mdn;
mod paste;
mod redirect;
mod reply;

#[cfg(test)]
mod foreign_tests;
#[cfg(test)]
mod tests;

pub use address::{new_message_id, normalize_email};
pub use build::{
    Attachment, BuiltMessage, ComposeError, InlineImage, OutgoingMessage, build_draft_message,
    build_message,
};
pub use html::{
    ForeignDraftHtml, MAX_CONTENT_ID_CHARS, editor_html, foreign_draft_html, inline_image_ids,
    is_valid_content_id, mail_html, text_to_editor_html,
};
pub use mdn::{
    DispositionReport, ReceiptRequest, ReceivedReport, is_receipt, parse_report, receipt_request,
    receipt_subject, receipt_text,
};
pub use paste::{PastedHtml, clean_pasted_html};
pub use redirect::{BuiltRedirect, Redirect, build_redirect};
// The document model of mail HTML, for signatures and templates (`crate::templates`).
pub(crate) use html::{
    Block, Inline, content_id_of, fragment, html_blocks, text_blocks, trim_paragraphs,
};
pub use reply::{
    MAX_REFERENCES, Original, PreparedDraft, QuoteLanguage, QuoteOptions, ReplyKind,
    choose_identity, forward_subject, prepare_forward, prepare_reply, reply_recipients,
    reply_references, reply_subject, strip_subject_prefixes,
};
