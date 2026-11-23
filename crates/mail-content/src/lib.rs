//! Message content, without IO: what a mail contains and how it may be shown.
//!
//! - [`parse`]: raw RFC 5322 bytes to a [`ParsedMessage`] (mail-parser underneath): headers,
//!   text and HTML bodies, attachments, inline images, preview, damage report. Never panics.
//! - [`render_html`]: mail HTML to a sanitised `srcdoc` for the sandboxed frame (security
//!   boundaries 1–3 and 5 of `docs/ARCHITECTURE.md`, decision 0005): ammonia with an allowlist,
//!   CSS re-serialised token by token, every `href` a placeholder carrying the render token,
//!   images only as `data:` URIs, the mandatory [`MAIL_CSP`] first.
//! - [`render_text`]: plain text as lines and link spans, never as HTML.
//! - [`check_link`]: what to warn about before a link opens; only `http`, `https`, `mailto`.
//! - [`sanitize_filename`], [`is_executable_type`]: attachment names and open warnings.
//! - [`preview_from_text`], [`preview_from_html`]: the list preview.
//! - [`build_message`], [`mail_html`], [`prepare_reply`], [`prepare_forward`]: composing
//!   (stage 5, module `compose`): the outgoing MIME message (mail-builder), mail HTML from the
//!   editor, replies and forwards.
//!
//! The crate owns no randomness and no network: render tokens come from the engine
//! (`mail_model::RenderToken`), external images are fetched by the engine and passed in as
//! bytes ([`RemotePolicy::Allow`]), the random part of a Message-ID too ([`new_message_id`]).

mod classify;
mod compose;
mod contacts;
mod css;
mod dom;
mod filename;
mod guard;
mod html;
mod image;
mod links;
mod parse;
mod preview;
mod templates;
mod text;

pub use classify::{
    CLASSIFY_HEADER_FIELDS, ClassifyHeaders, LocalSignals, SenderIdentity, SenderRules,
    apply_user_rules, classify, classify_automatic, is_no_reply_address, normalize_key,
};
pub use compose::{
    Attachment, BuiltMessage, ComposeError, MAX_REFERENCES, Original, OutgoingMessage,
    PreparedDraft, QuoteLanguage, QuoteOptions, ReplyKind, build_message, choose_identity,
    forward_subject, mail_html, new_message_id, normalize_email, prepare_forward, prepare_reply,
    reply_recipients, reply_references, reply_subject, strip_subject_prefixes,
};
pub use contacts::{
    Contact, ContactAddress, ContactBook, ContactError, ContactMatch, ContactMerge, Direction,
    DuplicateKind, DuplicateSuggestion, HALF_LIFE_SECS, MAX_NAME_CHARS, MAX_NAMES_PER_CONTACT,
    NormalizedAddress, Observation, PersonName, RECEIVED_WEIGHT, Rank, SENT_WEIGHT, SeenName,
    Skipped, clean_display_name, fold, fold_german, normalize_address, normalize_domain,
    observations, split_name, unicode_domain,
};
pub use filename::{
    FALLBACK_NAME, MAX_NAME_BYTES, attachment_file_name, is_executable_type, sanitize_filename,
};
pub use html::{
    Fallback, InlineImages, LinkTarget, MAIL_CSP, MAX_HTML_BYTES, RemotePolicy, RemoteRef,
    RenderOptions, RenderedHtml, render_html,
};
pub use image::{ImageBytes, ImageType, sniff_image};
pub use links::{LinkCheck, LinkWarning, check_link};
pub use parse::{
    AttachmentPart, InlinePart, ListHeaders, MAX_HTML_BODY_BYTES, MAX_INLINE_IMAGE_BYTES,
    MAX_TEXT_BODY_BYTES, MessageHeaders, ParseProblem, ParsedMessage, attachment_bytes, parse,
};
pub use preview::{PREVIEW_CHARS, preview_from_html, preview_from_text};
pub use templates::{
    DraftBody, DraftParts, FilledTemplate, LocalDate, MAX_SIGNATURE_BYTES, MAX_SNIPPET_INPUT_BYTES,
    MAX_TEMPLATE_BYTES, MAX_TEMPLATE_SUBJECT_CHARS, Placeholder, Quote, ReplyPosition,
    SIGNATURE_SEPARATOR, SanitizeNotes, Signature, SnippetError, Template, TemplateProblem,
    TemplateValues, assemble_draft, fill_template,
};
pub use text::{MAX_TEXT_BYTES, RenderedText, TextLine, TextSpan, html_to_text, render_text};
