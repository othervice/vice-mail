//! Message content, without IO: what a mail contains and how it may be shown.
//!
//! - [`parse`]: raw RFC 5322 bytes to a [`ParsedMessage`] (mail-parser underneath): headers,
//!   text and HTML bodies, attachments, inline images, preview, damage report. Never panics.
//! - [`render_html`]: mail HTML to a sanitised `srcdoc` for the sandboxed frame (security
//!   boundaries 1–3 and 5 of `docs/ARCHITECTURE.md`, decision 0005): ammonia with an allowlist,
//!   CSS re-serialised token by token, every `href` a placeholder carrying the render token,
//!   images only as `data:` URIs the core encoded again ([`reencode_images`], finding C-4), the
//!   mandatory [`MAIL_CSP`] first.
//!   Stage 21 (decision 0066): in the dark theme it adapts the mail's colours (light
//!   backgrounds dark, dark text light, images untouched) in the same pass
//!   ([`RenderOptions::scheme`], [`RenderedHtml::colors`]).
//! - [`render_text`]: plain text as lines and link spans, never as HTML.
//! - [`html_to_text`]: the text of an HTML body; [`html_to_visible_text`] without the text a
//!   reader does not see (hidden by CSS or attributes), for models; [`strip_hidden_chars`]:
//!   invisible Unicode characters out of any text for models.
//! - [`check_link`]: what to warn about before a link opens; only `http`, `https`, `mailto`.
//! - [`strip_tracking_parameters`], [`is_known_tracker_host`], [`is_pixel`]: tracker
//!   protection (stage 20, decision 0058); [`render_html`] counts the trackers it blocks.
//! - [`sanitize_filename`], [`is_executable_type`]: attachment names and open warnings.
//! - [`preview_from_text`], [`preview_from_html`]: the list preview.
//! - [`build_message`], [`mail_html`], [`prepare_reply`], [`prepare_forward`]: composing
//!   (stage 5, module `compose`): the outgoing MIME message (mail-builder), mail HTML from the
//!   editor, replies and forwards.
//! - [`attachment_mention`]: whether the user's own text speaks of an attachment (stage 20).
//! - [`parse_mailto`]: a `mailto:` link (RFC 6068) as the fields of a new draft, with hints.
//! - [`source_text`], [`header_fields`]: a message's source and header fields as plain text
//!   (stage 19); [`build_redirect`]: redirecting with `Resent-*` fields.
//! - [`read_calendar`], [`calendar_parts`]: calendar invitations (`text/calendar`, `.ics`) for
//!   the invitation card; time zones through a [`ZoneResolver`] the engine provides.
//! - [`spam_tokens`], [`spam_score`]: the learning spam filter's tokens of a message and its
//!   score from learned token counts (stage 22, decision 0077).
//! - [`read_vcards`], [`write_vcards`]: vCard files (2.1, 3.0, 4.0 read; 3.0, 4.0 written) for
//!   importing and exporting contacts and contact groups (stage 19).
//! - [`MboxReader`], [`write_mbox_message`]: mbox files (stage 19), read line by line with
//!   bounded memory (mboxrd and Thunderbird's mboxo), written as mboxrd.
//! - [`parse_profiles_ini`], [`parse_prefs_js`], [`thunderbird_accounts`]: Thunderbird's
//!   profiles and their accounts, identities, signatures and SMTP servers (stage 19, takeover;
//!   never passwords).
//!
//! The crate owns no randomness and no network: render tokens come from the engine
//! (`mail_model::RenderToken`), external images are fetched by the engine and passed in as
//! bytes ([`RemotePolicy::Allow`]), the random part of a Message-ID too ([`new_message_id`]).

mod attachment_preview;
mod attachment_reminder;
mod calendar;
mod classify;
mod colors;
mod compose;
mod contact_picture;
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
mod mbox;
mod parse;
mod preview;
mod reencode;
mod source;
mod spam;
mod templates;
mod text;
mod thunderbird;
mod trackers;
mod vcard;
mod visible;

pub use attachment_preview::{preview_kind, render_preview};
pub use attachment_reminder::attachment_mention;
pub use calendar::{
    CalendarPart, MAX_CALENDAR_ATTENDEES, MAX_CALENDAR_BYTES, MAX_CALENDAR_DESCRIPTION_BYTES,
    MAX_CALENDAR_EVENTS, MAX_CALENDAR_LINE_BYTES, MAX_CALENDAR_LINES, MAX_CALENDAR_PARTS,
    ZoneResolver, calendar_parts, read_calendar,
};
pub use classify::{
    CLASSIFY_HEADER_FIELDS, ClassifyHeaders, LocalSignals, SenderIdentity, SenderRules,
    apply_user_rules, classify, classify_automatic, is_no_reply_address, normalize_key,
};
pub use colors::{
    BACKGROUND_THRESHOLD, DARK_CANVAS, DARK_LINK, DARK_TEXT, MAIL_DARK_STYLE, TEXT_THRESHOLD,
};
pub use compose::{
    Attachment, BuiltMessage, BuiltRedirect, ComposeError, DispositionReport, ForeignDraftHtml,
    InlineImage, MAX_CONTENT_ID_CHARS, MAX_REFERENCES, Original, OutgoingMessage, PastedHtml,
    PreparedDraft, QuoteLanguage, QuoteOptions, ReceiptRequest, ReceivedReport, Redirect,
    ReplyKind, build_draft_message, build_message, build_redirect, choose_identity,
    clean_pasted_html, editor_html, foreign_draft_html, forward_subject, inline_image_ids,
    is_receipt, is_valid_content_id, mail_html, new_message_id, normalize_email, parse_report,
    prepare_forward, prepare_reply, receipt_request, receipt_subject, receipt_text,
    reply_recipients, reply_references, reply_subject, strip_subject_prefixes, text_to_editor_html,
};
pub use contact_picture::{PictureJpeg, is_jpeg, picture_crop, picture_source};
pub use contacts::{
    Contact, ContactAddress, ContactBook, ContactError, ContactMatch, ContactMerge, Direction,
    DuplicateKind, DuplicateSuggestion, HALF_LIFE_SECS, MAX_NAME_CHARS, MAX_NAMES_PER_CONTACT,
    NormalizedAddress, Observation, PersonName, RECEIVED_WEIGHT, Rank, SENT_WEIGHT, SeenName,
    Skipped, clean_display_name, contact_kind, counted_before_stage17, fold, fold_german,
    normalize_address, normalize_domain, observations, split_name, unicode_domain,
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
pub use mbox::{
    MAX_MBOX_MESSAGE_BYTES, MboxEntry, MboxReader, MozillaStatus, is_mbox_separator,
    mbox_from_line, write_mbox_message,
};
pub use parse::{
    AttachmentPart, InlinePart, ListHeaders, MAX_HTML_BODY_BYTES, MAX_INLINE_IMAGE_BYTES,
    MAX_TEXT_BODY_BYTES, MessageHeaders, ParseProblem, ParsedMessage, attachment_bytes, parse,
};
pub use preview::{PREVIEW_CHARS, preview_from_html, preview_from_text};
pub use reencode::{
    MAX_ANIMATION_BYTES, MAX_ANIMATION_FRAMES, MAX_ANIMATION_PIXELS, ReencodedImage,
    ReencodedImages, is_reencoded_data_url, reencode_image, reencode_images,
};
pub use source::{HeaderField, SourceText, header_fields, source_text};
pub use spam::{
    MAX_DISCRIMINATORS, MAX_SPAM_TEXT_BYTES, MAX_SPAM_TOKENS, MIN_PROB_STRENGTH, SpamCounts,
    SpamFields, SpamScore, SpamTokens, SpamTotals, UNKNOWN_WORD_PROB, UNKNOWN_WORD_STRENGTH,
    spam_score, spam_token_hash, spam_tokens, spam_tokens_of_fields, spam_tokens_of_raw,
    token_probability,
};
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
pub use thunderbird::{
    PrefValue, TbAccount, TbIdentity, TbSecurity, TbServer, TbServerKind, TbSignature, TbSmtp,
    ThunderbirdProfileEntry, parse_prefs_js, parse_profiles_ini, thunderbird_accounts,
};
pub use trackers::{
    CleanedUrl, KNOWN_TRACKER_HOSTS, MAX_TRACKER_HOSTS, PIXEL_MAX, TRACKING_PARAMETER_PREFIXES,
    TRACKING_PARAMETERS, TrackerKind, TrackerReport, is_known_tracker_host, is_pixel,
    is_tracking_parameter, strip_tracking_parameters,
};
pub use vcard::{
    MAX_VCARD_CARDS, MAX_VCARD_EMAILS, MAX_VCARD_FILE_BYTES, MAX_VCARD_LINE_BYTES,
    MAX_VCARD_MEMBERS, MAX_VCARD_PHOTO_BYTES, VcardCard, VcardFile, VcardKind, VcardMember,
    VcardProblem, VcardProblemKind, VcardVersion, read_vcards, write_vcards,
};
pub use visible::{
    HIDDEN_OPACITY, MATCH_BUDGET, MIN_BOX_PX, MIN_CONTRAST, MIN_FONT_PX, OFFSCREEN_PX,
    TRANSPARENT_ALPHA, VisibleText,
};
