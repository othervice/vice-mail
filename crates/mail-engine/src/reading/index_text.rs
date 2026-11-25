//! Text of stored bodies for the full-text index (stage 7): what the reading pane shows as
//! text, and the attachment names, parsed the same way the pane parses (`view::Content`).

use mail_model::{FetchedSection, MessageStructure};
use mail_store::{BodyText, BodyTextSource, IndexedAttachment, ReceivedReceipt};

use super::view::{AttachmentSource, Content};

/// The store's [`BodyTextSource`], backed by `mail-content`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContentText;

impl BodyTextSource for ContentText {
    fn complete(&self, raw: &[u8]) -> BodyText {
        let mut text = text_of(&Content::complete(raw));
        // Stage 16 (decision 0033): a read receipt is recorded for the message it reports on.
        text.receipt = mail_content::parse_report(raw).map(|report| ReceivedReceipt {
            original_message_id: report.original_message_id,
            recipient: report.final_recipient,
            disposition: report.disposition,
        });
        text
    }

    fn partial(&self, structure: &MessageStructure, sections: &[FetchedSection]) -> BodyText {
        text_of(&Content::partial(structure, sections))
    }
}

fn text_of(content: &Content) -> BodyText {
    BodyText {
        body: content.parsed.display_text().into_owned(),
        attachments: content
            .attachments
            .iter()
            .map(|a| a.info.name.clone())
            .collect(),
        // Stage 20 (decision 0060): the same list, with the pane's indexes, for the view.
        attachment_index: content
            .attachments
            .iter()
            .map(|a| IndexedAttachment {
                name: a.info.name.clone(),
                mime_type: a.info.mime_type.clone(),
                size: u64::from(a.info.size),
                part: match &a.source {
                    AttachmentSource::Whole { .. } => None,
                    AttachmentSource::Section { path } => Some(path.to_string()),
                },
            })
            .collect(),
        receipt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_attachment_names_of_a_message() {
        let raw = b"From: a@example.org\r\nSubject: Rechnung\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p>Hallo <b>Welt</b></p>\r\n--b\r\nContent-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"Kontoauszug 2026.pdf\"\r\n\r\nJVBERi0=\r\n--b--\r\n";
        let text = ContentText.complete(raw);
        assert!(
            text.body.contains("Hallo") && text.body.contains("Welt"),
            "{text:?}"
        );
        assert!(!text.body.contains("<b>"));
        assert_eq!(text.attachments, ["Kontoauszug 2026.pdf"]);
    }
}
