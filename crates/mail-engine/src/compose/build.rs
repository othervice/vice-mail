//! A stored draft as a message: the composer's parts put together (decision 0016: what the
//! user wrote, the identity's text signature, the quote) and handed to `mail_content`'s
//! builder, for the Drafts folder (lenient: no recipient needed) or for sending (strict).

use std::path::Path;

use mail_content::{
    Attachment, DraftParts, InlineImage, OutgoingMessage, Quote, ReplyPosition,
    SIGNATURE_SEPARATOR, Signature,
};
use mail_model::{Address, ComposeFormat, CoreError, DraftError, Identity, QuotePosition};
use mail_store::StoredDraft;

use super::files;

/// The sending identity of a draft and the signature the draft uses (stage 10: the draft's
/// choice or the identity's default for its kind, see `super::snippets::with_signature`).
#[derive(Debug, Clone)]
pub(crate) struct Sender {
    pub(crate) identity: Identity,
    pub(crate) signature: Option<super::snippets::DraftSignature>,
}

/// Picks the draft's identity among the account's (with signatures); `None` if it was deleted.
pub(crate) fn sender_of(
    draft: &StoredDraft,
    identities: &[(Identity, Option<String>)],
) -> Option<Sender> {
    let id = draft.identity?;
    identities
        .iter()
        .find(|(identity, _)| identity.id == id)
        .map(|(identity, _)| Sender {
            identity: identity.clone(),
            signature: None,
        })
}

/// The account's default identity (for a draft whose identity is gone).
pub(crate) fn default_sender(identities: &[(Identity, Option<String>)]) -> Option<Sender> {
    identities
        .iter()
        .find(|(identity, _)| identity.is_default)
        .or_else(|| identities.first())
        .map(|(identity, _)| Sender {
            identity: identity.clone(),
            signature: None,
        })
}

/// The body as (text, editor HTML) of the message.
fn body(draft: &StoredDraft, sender: &Sender) -> (String, Option<String>) {
    let signature: Option<&Signature> = sender
        .signature
        .as_ref()
        .filter(|_| draft.include_signature)
        .map(|s| &s.signature)
        .filter(|s| !s.is_empty());
    let quote_text = draft
        .quote_text
        .as_deref()
        .filter(|_| draft.include_quote)
        .unwrap_or_default();
    let quote_html = draft
        .quote_html
        .clone()
        .filter(|_| draft.include_quote)
        .or_else(|| {
            (!quote_text.is_empty()).then(|| mail_content::text_to_editor_html(quote_text))
        });
    match draft.format {
        ComposeFormat::Html => {
            let assembled = mail_content::assemble_draft(&DraftParts {
                body_html: &draft.body_html,
                signature,
                quote: quote_html.as_deref().map(|html| Quote {
                    html,
                    text: quote_text,
                }),
                position: match draft.quote_position {
                    QuotePosition::Above => ReplyPosition::AboveQuote,
                    QuotePosition::Below => ReplyPosition::BelowQuote,
                },
            });
            (assembled.text, Some(assembled.html))
        }
        ComposeFormat::Text => {
            let mut own = draft.body_text.replace("\r\n", "\n").trim_end().to_owned();
            if let Some(signature) = signature {
                own.push_str("\n\n");
                own.push_str(SIGNATURE_SEPARATOR);
                own.push('\n');
                own.push_str(&signature.text);
            }
            let quote = quote_text.trim_end();
            let text = match (draft.quote_position, quote.is_empty()) {
                (_, true) => own,
                (QuotePosition::Above, false) => format!("{own}\n\n{quote}"),
                (QuotePosition::Below, false) => format!("{quote}\n\n{own}"),
            };
            (format!("{}\n", text.trim_end_matches('\n')), None)
        }
    }
}

/// The draft's files as message parts: attachments, and the inline images its HTML shows
/// (stage 10; only those the HTML refers to go out, `mail_content::build_message` checks).
#[derive(Debug, Default)]
pub(crate) struct Files {
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) inline_images: Vec<InlineImage>,
}

/// Reads the draft's copies of its attachments and inline images.
pub(crate) fn files_of(root: &Path, draft: &StoredDraft) -> Result<Files, CoreError> {
    let mut out = Files::default();
    for file in &draft.attachments {
        let bytes = files::read(root, &file.file)?;
        match &file.content_id {
            Some(content_id) => out.inline_images.push(InlineImage {
                content_id: content_id.clone(),
                file_name: file.file_name.clone(),
                mime: file.mime.clone(),
                bytes,
            }),
            None => out.attachments.push(Attachment {
                file_name: file.file_name.clone(),
                mime: file.mime.clone(),
                bytes,
            }),
        }
    }
    Ok(out)
}

/// The message of `draft` from `sender`, dated `date` (Unix seconds).
pub(crate) fn outgoing(
    draft: &StoredDraft,
    sender: &Sender,
    files: Files,
    date: i64,
) -> OutgoingMessage {
    let (text, html) = body(draft, sender);
    let mut inline_images = files.inline_images;
    // The signature's own images (stage 10); only those its HTML shows are written.
    if draft.include_signature
        && draft.format == ComposeFormat::Html
        && let Some(signature) = &sender.signature
    {
        inline_images.extend(signature.images.iter().cloned());
    }
    let name = sender.identity.name.trim();
    OutgoingMessage {
        from: Address {
            name: (!name.is_empty()).then(|| name.to_owned()),
            email: sender.identity.email.clone(),
        },
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        bcc: draft.bcc.clone(),
        reply_to: sender
            .identity
            .reply_to
            .iter()
            .filter(|r| !r.trim().is_empty())
            .map(|r| Address {
                name: None,
                email: r.clone(),
            })
            .collect(),
        subject: draft.subject.clone(),
        text: Some(text),
        html,
        attachments: files.attachments,
        inline_images,
        message_id: draft.message_id_header.clone(),
        in_reply_to: draft.in_reply_to.clone(),
        references: draft.references.clone(),
        date,
        user_agent: None,
        // Stage 16: the read-receipt request names the sending identity.
        disposition_notification_to: draft.request_receipt.then(|| sender.identity.email.clone()),
        report: None,
    }
}

/// The bytes of the draft's version for the Drafts folder.
pub(crate) fn draft_bytes(
    root: &Path,
    draft: &StoredDraft,
    sender: &Sender,
) -> Result<Vec<u8>, CoreError> {
    let files = files_of(root, draft)?;
    let message = outgoing(draft, sender, files, draft.updated_at / 1000);
    mail_content::build_draft_message(&message).map_err(compose_error)
}

pub(crate) fn compose_error(err: mail_content::ComposeError) -> CoreError {
    match err {
        mail_content::ComposeError::NoRecipients => CoreError::Draft(DraftError::NoRecipients),
        mail_content::ComposeError::InvalidAddress(address) => {
            CoreError::Draft(DraftError::InvalidAddress(address))
        }
        mail_content::ComposeError::InvalidMessageId(id) => {
            CoreError::Internal(format!("the draft's Message-ID {id:?} is not valid"))
        }
    }
}
