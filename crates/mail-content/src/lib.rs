//! Message content, without IO: what a mail contains and how it may be shown.
//!
//! - [`parse`]: raw RFC 5322 bytes to a [`ParsedMessage`] (mail-parser underneath): headers,
//!   text and HTML bodies, attachments, inline images, preview, damage report. Never panics.
//! - [`render_html`]: mail HTML to a sanitised `srcdoc` for the sandboxed frame (security
//!   boundaries 1–3 and 5 of `docs/ARCHITECTURE.md`, decision 0005): ammonia with an allowlist,
//!   CSS re-serialised token by token, every `href` a placeholder carrying the render token,
//!   images only as `data:` URIs, the mandatory [`MAIL_CSP`] first.
//! - [`render_text`]: plain text as lines and link spans, never as HTML.
//! - [`html_to_text`]: the text of an HTML body; [`html_to_visible_text`] without the text a
//!   reader does not see (hidden by CSS or attributes), for models; [`strip_hidden_chars`]:
//!   invisible Unicode characters out of any text for models.
//! - [`check_link`]: what to warn about before a link opens; only `http`, `https`, `mailto`.
//! - [`sanitize_filename`], [`is_executable_type`]: attachment names and open warnings.
//! - [`preview_from_text`], [`preview_from_html`]: the list preview.
//! - [`build_message`], [`mail_html`], [`prepare_reply`], [`prepare_forward`]: composing
//!   (stage 5, module `compose`): the outgoing MIME message (mail-builder), mail HTML from the
//!   editor, replies and forwards.
//! - [`parse_mailto`]: a `mailto:` link (RFC 6068) as the fields of a new draft, with hints.
//! - [`read_calendar`], [`calendar_parts`]: calendar invitations (`text/calendar`, `.ics`) for
//!   the invitation card; time zones through a [`ZoneResolver`] the engine provides.
//!
//! The crate owns no randomness and no network: render tokens come from the engine
//! (`mail_model::RenderToken`), external images are fetched by the engine and passed in as
//! bytes ([`RemotePolicy::Allow`]), the random part of a Message-ID too ([`new_message_id`]).

mod calendar;
mod classify;
mod compose;
mod contacts;
mod css;
mod dom;
mod filename;
mod guard;
mod hidden_chars;
mod html;
mod image;
mod links;
mod mailto;
mod parse;
mod preview;
mod templates;
mod text;
mod visible;

pub use calendar::{
    CalendarPart, MAX_CALENDAR_ATTENDEES, MAX_CALENDAR_BYTES, MAX_CALENDAR_DESCRIPTION_BYTES,
    MAX_CALENDAR_EVENTS, MAX_CALENDAR_LINE_BYTES, MAX_CALENDAR_LINES, MAX_CALENDAR_PARTS,
    ZoneResolver, calendar_parts, read_calendar,
};
pub use classify::{
    CLASSIFY_HEADER_FIELDS, ClassifyHeaders, LocalSignals, SenderIdentity, SenderRules,
    apply_user_rules, classify, classify_automatic, is_no_reply_address, normalize_key,
};
pub use compose::{
    Attachment, BuiltMessage, ComposeError, InlineImage, MAX_CONTENT_ID_CHARS, MAX_REFERENCES,
    Original, OutgoingMessage, PastedHtml, PreparedDraft, QuoteLanguage, QuoteOptions, ReplyKind,
    build_draft_message, build_message, choose_identity, clean_pasted_html, editor_html,
    forward_subject, inline_image_ids, is_valid_content_id, mail_html, new_message_id,
    normalize_email, prepare_forward, prepare_reply, reply_recipients, reply_references,
    reply_subject, strip_subject_prefixes, text_to_editor_html,
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
pub use hidden_chars::{StrippedText, is_hidden_char, strip_hidden_chars};
pub use html::{
    Fallback, InlineImages, LinkTarget, MAIL_CSP, MAX_HTML_BYTES, RemotePolicy, RemoteRef,
    RenderOptions, RenderedHtml, render_html,
};
pub use image::{ImageBytes, ImageType, image_data_uri, sniff_image};
pub use links::{LinkCheck, LinkWarning, check_link};
pub use mailto::{
    MAX_MAILTO_BODY_BYTES, MAX_MAILTO_NOTICE_VALUE_CHARS, MAX_MAILTO_URI_BYTES,
    MAX_MAILTO_VALUE_NOTICES, MailtoError, parse_mailto,
};
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
pub use text::{
    MAX_TEXT_BYTES, RenderedText, TextLine, TextSpan, html_to_text, html_to_visible_text,
    render_text,
};
pub use visible::{
    HIDDEN_OPACITY, MATCH_BUDGET, MIN_BOX_PX, MIN_CONTRAST, MIN_FONT_PX, OFFSCREEN_PX,
    TRANSPARENT_ALPHA, VisibleText,
};
