//! Unit tests of the compose service's pure parts.

use super::*;

#[test]
fn gmail_files_sent_mail_itself() {
    assert!(files_sent_itself("imap.gmail.com"));
    assert!(files_sent_itself("IMAP.googlemail.com."));
    assert!(!files_sent_itself("imap.example.org"));
    assert!(!files_sent_itself("notgmail.com"));
}

#[test]
fn request_ids_and_mime_types() {
    assert!(valid_request_id("6f9619ff-8b86-4d11-b42d-00c04fc964ff").is_ok());
    assert!(valid_request_id("").is_err());
    assert!(valid_request_id("a b").is_err());
    assert_eq!(mime_of("Rechnung.PDF"), "application/pdf");
    assert_eq!(mime_of("no-extension"), "application/octet-stream");
}

#[test]
fn saved_content_is_cleaned() {
    let update = DraftUpdate {
        identity_id: None,
        to: vec![
            Address {
                name: Some(" Bob\n ".into()),
                email: " bob@example.org ".into(),
            },
            Address {
                name: None,
                email: "   ".into(),
            },
        ],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Hallo\r\nBcc: x@example.org".into(),
        format: ComposeFormat::Html,
        body_html: "<p>x</p>".into(),
        body_text: String::new(),
        include_quote: true,
        quote_position: QuotePosition::Above,
        include_signature: true,
        signature_id: None,
    };
    let content = content_of(&update).expect("content");
    assert_eq!(
        content.to,
        vec![Address {
            name: Some("Bob".into()),
            email: "bob@example.org".into()
        }]
    );
    assert!(!content.subject.contains(['\r', '\n']));
}
