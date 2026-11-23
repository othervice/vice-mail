//! The header data the category rules read, from a raw header block (or a whole message), from
//! a parsed message, or from the header data the sync already stores.

use mail_model::{Address, MessageHeader};
use mail_parser::{AddressList, MessageParser};

use crate::parse::MessageHeaders;

/// Header fields the rules read. The adapter fetches exactly these at arrival
/// (`BODY.PEEK[HEADER.FIELDS (…)]`) and passes the bytes to [`ClassifyHeaders::from_raw`].
pub const CLASSIFY_HEADER_FIELDS: &[&str] = &[
    "From",
    "Sender",
    "Reply-To",
    "Subject",
    "Return-Path",
    "Content-Type",
    "List-Id",
    "List-Unsubscribe",
    "List-Post",
    "Auto-Submitted",
    "Precedence",
    "X-Autoreply",
    "X-Autorespond",
    "X-Mailer",
    "Feedback-ID",
    "X-CSA-Complaints",
    "X-Report-Abuse",
    "X-Campaign",
    "X-CampaignID",
    "X-Campaign-Id",
    "X-Newsletter",
    "X-MC-User",
    "X-Mailjet-Campaign",
    "X-Mailin-Campaign",
    "X-SES-Outgoing",
    "X-SG-EID",
    "X-Mailgun-Sid",
    "X-PM-Message-Id",
    "X-Mandrill-User",
    "X-MSFBL",
    "X-MJ-Mid",
    "X-SFMC-Stack",
    "X-GitHub-Reason",
    "X-GitLab-Project",
    "X-GitLab-NotificationReason",
    "X-Gitea-Reason",
    "X-Forgejo-Reason",
    "X-JIRA-FingerPrint",
    "X-Bugzilla-Reason",
    "X-Redmine-Project",
    "X-Phabricator-Sent-This-Message",
    "X-Gerrit-MessageType",
    "X-Jenkins-Job",
    "X-Discourse-Post-Id",
    "X-Launchpad-Notification-Type",
    "X-Facebook-Notify",
];

/// Most extra fields kept, and the longest value kept of each.
const MAX_FIELDS: usize = 64;
const MAX_VALUE_CHARS: usize = 200;
const MAX_ADDRESSES: usize = 20;

/// What the rules see of a message. Text is decoded (RFC 2047) and free of control characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassifyHeaders {
    pub from: Vec<Address>,
    pub sender: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: Option<String>,
    /// The address inside `<…>` of `Return-Path`; `Some("")` for the null path `<>`.
    pub return_path: Option<String>,
    /// `type/subtype` of the message, lower case.
    pub content_type: Option<String>,
    /// `report-type` of a `multipart/report`, lower case.
    pub report_type: Option<String>,
    /// The identifier inside `<…>` of `List-Id`.
    pub list_id: Option<String>,
    pub list_unsubscribe: bool,
    /// `List-Post` as written (`<mailto:…>`, or `NO` for announcement lists).
    pub list_post: Option<String>,
    pub auto_submitted: Option<String>,
    pub precedence: Option<String>,
    /// The other fields of [`CLASSIFY_HEADER_FIELDS`] (and `X-Campaign…`, `X-Newsletter…`)
    /// found in the message: name in lower case, value unfolded and trimmed.
    pub fields: Vec<(String, String)>,
}

impl ClassifyHeaders {
    /// From a raw header block or a whole message. Never panics; unreadable input gives empty
    /// headers.
    pub fn from_raw(raw: &[u8]) -> Self {
        std::panic::catch_unwind(|| from_raw(raw)).unwrap_or_default()
    }

    /// From a parsed message (without `Return-Path`, `Content-Type` and the extra fields).
    pub fn from_parsed(headers: &MessageHeaders) -> Self {
        Self {
            from: headers.from.clone(),
            sender: headers.sender.clone(),
            reply_to: headers.reply_to.clone(),
            subject: headers.subject.clone(),
            list_id: headers.list.id.clone(),
            list_unsubscribe: !headers.list.unsubscribe.is_empty(),
            list_post: (!headers.list.post.is_empty()).then(|| headers.list.post.join(", ")),
            auto_submitted: headers.auto_submitted.clone(),
            precedence: headers.precedence.clone(),
            ..Self::default()
        }
    }

    /// From the header data the sync stores ([`MessageHeader`]): From, Subject and the raw
    /// `List-Id`, `List-Unsubscribe`, `Auto-Submitted` and `Precedence` values. Enough for the
    /// main rules; for all of them use [`ClassifyHeaders::from_raw`] on
    /// [`CLASSIFY_HEADER_FIELDS`].
    pub fn from_message_header(header: &MessageHeader) -> Self {
        Self::from_stored(
            &header.from,
            &header.subject,
            header.list_id.as_deref(),
            header.list_unsubscribe.as_deref(),
            header.auto_submitted.as_deref(),
            header.precedence.as_deref(),
        )
    }

    fn from_stored(
        from: &[Address],
        subject: &str,
        list_id: Option<&str>,
        list_unsubscribe: Option<&str>,
        auto_submitted: Option<&str>,
        precedence: Option<&str>,
    ) -> Self {
        Self {
            from: from.to_vec(),
            subject: non_empty(subject),
            list_id: list_id.and_then(list_identifier),
            list_unsubscribe: list_unsubscribe.is_some_and(|v| !v.trim().is_empty()),
            auto_submitted: auto_submitted.and_then(non_empty),
            precedence: precedence.and_then(non_empty),
            ..Self::default()
        }
    }

    /// The value of an extra field (name in any case).
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The first `From` address.
    pub fn from_email(&self) -> Option<&str> {
        self.from
            .iter()
            .map(|a| a.email.trim())
            .find(|e| !e.is_empty())
    }
}

fn from_raw(raw: &[u8]) -> ClassifyHeaders {
    let Some(message) = MessageParser::new().parse_headers(raw) else {
        return ClassifyHeaders::default();
    };
    let mut out = ClassifyHeaders {
        from: addresses(message.from()),
        sender: addresses(message.sender()),
        reply_to: addresses(message.reply_to()),
        subject: message.subject().map(clean_text).filter(|s| !s.is_empty()),
        ..ClassifyHeaders::default()
    };
    if let Some(content_type) = message.content_type() {
        let mime = match content_type.subtype() {
            Some(sub) => format!("{}/{}", content_type.ctype(), sub),
            None => content_type.ctype().to_owned(),
        };
        out.content_type = Some(clean_text(&mime).to_ascii_lowercase()).filter(|m| !m.is_empty());
        out.report_type = content_type
            .attribute("report-type")
            .map(|r| clean_text(r).to_ascii_lowercase())
            .filter(|r| !r.is_empty());
    }
    for header in message.headers().iter() {
        let name = header.raw_name().trim().to_ascii_lowercase();
        let value = clean_text(&String::from_utf8_lossy(header.raw_value()));
        match name.as_str() {
            "return-path" if out.return_path.is_none() => {
                out.return_path = Some(return_path(&value));
            }
            "list-id" if out.list_id.is_none() => out.list_id = list_identifier(&value),
            "list-unsubscribe" => out.list_unsubscribe |= !value.is_empty(),
            "list-post" if out.list_post.is_none() => out.list_post = non_empty(&value),
            "auto-submitted" if out.auto_submitted.is_none() => {
                out.auto_submitted = non_empty(&value);
            }
            "precedence" if out.precedence.is_none() => out.precedence = non_empty(&value),
            _ if is_signal_field(&name) && out.fields.len() < MAX_FIELDS => {
                let value: String = value.chars().take(MAX_VALUE_CHARS).collect();
                out.fields.push((name, value));
            }
            _ => {}
        }
    }
    out
}

/// Fields kept in [`ClassifyHeaders::fields`].
fn is_signal_field(lower_name: &str) -> bool {
    const HANDLED: &[&str] = &[
        "from",
        "sender",
        "reply-to",
        "subject",
        "return-path",
        "content-type",
        "list-id",
        "list-unsubscribe",
        "list-post",
        "auto-submitted",
        "precedence",
    ];
    if HANDLED.contains(&lower_name) {
        return false;
    }
    lower_name.starts_with("x-campaign")
        || lower_name.starts_with("x-newsletter")
        || CLASSIFY_HEADER_FIELDS
            .iter()
            .any(|f| f.eq_ignore_ascii_case(lower_name))
}

fn addresses(list: Option<AddressList<'_>>) -> Vec<Address> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.mailboxes()
        .filter_map(|mailbox| {
            let name = mailbox.name().map(clean_text).filter(|n| !n.is_empty());
            let email = mailbox.address().map(clean_text).unwrap_or_default();
            (name.is_some() || !email.is_empty()).then_some(Address { name, email })
        })
        .take(MAX_ADDRESSES)
        .collect()
}

/// Control characters (folding included) become spaces, bidirectional controls go (C-8: rules
/// see the subject and names as the reader does); trimmed.
fn clean_text(text: &str) -> String {
    let mapped: String = text
        .chars()
        .filter(|c| !mail_model::is_bidi_control(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    mapped.trim().to_owned()
}

fn non_empty(value: &str) -> Option<String> {
    let value = clean_text(value);
    (!value.is_empty()).then_some(value)
}

/// The address of a `Return-Path` value: inside `<…>` (empty for `<>`), else the bare value.
fn return_path(value: &str) -> String {
    match (value.find('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => value[start + 1..end].trim().to_owned(),
        _ => value.trim().to_owned(),
    }
}

/// The identifier of a `List-Id` value (`Name <id>` or a bare id), lower case.
pub(crate) fn list_identifier(value: &str) -> Option<String> {
    let value = clean_text(value);
    let inner = match (value.rfind('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => &value[start + 1..end],
        _ => value.as_str(),
    };
    let id: String = inner
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_headers_are_read() {
        let raw = b"Return-Path: <bounces+123-alice=x@em.sendgrid.net>\r\n\
            From: =?utf-8?q?J=C3=BCrgen?= <j@example.com>\r\n\
            Subject: Hi\r\n\
            List-Id: Example News\r\n <Weekly.News.Example>\r\n\
            List-Unsubscribe: <https://x/u>\r\n\
            List-Post: NO\r\n\
            Content-Type: multipart/report; report-type=Delivery-Status; boundary=x\r\n\
            X-CSA-Complaints: csa-complaints@eco.de\r\n\
            X-Campaign-Ref: 42\r\n\
            X-GitHub-Reason: mention\r\n\
            X-Unrelated: 1\r\n\r\nbody";
        let h = ClassifyHeaders::from_raw(raw);
        assert_eq!(h.from[0].name.as_deref(), Some("Jürgen"));
        assert_eq!(
            h.return_path.as_deref(),
            Some("bounces+123-alice=x@em.sendgrid.net")
        );
        assert_eq!(h.list_id.as_deref(), Some("weekly.news.example"));
        assert!(h.list_unsubscribe);
        assert_eq!(h.list_post.as_deref(), Some("NO"));
        assert_eq!(h.content_type.as_deref(), Some("multipart/report"));
        assert_eq!(h.report_type.as_deref(), Some("delivery-status"));
        assert_eq!(h.field("x-csa-complaints"), Some("csa-complaints@eco.de"));
        assert_eq!(h.field("X-Campaign-Ref"), Some("42"));
        assert_eq!(h.field("X-GitHub-Reason"), Some("mention"));
        assert_eq!(h.field("X-Unrelated"), None);
    }

    #[test]
    fn null_return_path_and_garbage() {
        let h = ClassifyHeaders::from_raw(b"Return-Path: <>\r\nFrom: a@b\r\n\r\n");
        assert_eq!(h.return_path.as_deref(), Some(""));
        assert_eq!(ClassifyHeaders::from_raw(b""), ClassifyHeaders::default());
        let _ = ClassifyHeaders::from_raw(b"\xff\xfe\x00:::\r\n\r\n");
    }

    #[test]
    fn stored_header_data_is_enough_for_the_main_rules() {
        // `from_message_header` passes the stored raw values through `from_stored`.
        let from = [Address {
            name: None,
            email: "news@news.example".into(),
        }];
        let h = ClassifyHeaders::from_stored(
            &from,
            "",
            Some("Example News <weekly.news.example>"),
            Some("<https://x/u>"),
            None,
            Some(" bulk "),
        );
        assert_eq!(h.list_id.as_deref(), Some("weekly.news.example"));
        assert!(h.list_unsubscribe);
        assert_eq!(h.precedence.as_deref(), Some("bulk"));
        assert_eq!(h.subject, None);
        assert_eq!(h.from_email(), Some("news@news.example"));
        assert_eq!(
            list_identifier("bare.id.example").as_deref(),
            Some("bare.id.example")
        );
        assert_eq!(list_identifier("  "), None);
    }
}
