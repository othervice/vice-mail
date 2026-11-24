//! Stage 19 (decision 0055) through `AppCore` with the fake IMAP server and the recording SMTP
//! sender of `tests::with_core`: redirecting a message and editing one as new.

use mail_model::{
    Address, ComposeSettings, ErrorCode, IdentityId, MailboxRole, MessageId, RedirectRequest,
    SendState,
};

use super::tests::with_core::{Bed, drafts_server};

fn to(email: &str) -> Vec<Address> {
    vec![Address {
        name: Some("Carol".into()),
        email: email.into(),
    }]
}

impl Bed {
    async fn inbox_message(&self) -> (MessageId, String) {
        let inbox = self
            .core
            .list_mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == MailboxRole::Inbox)
            .expect("inbox")
            .id;
        self.until("a message in the inbox", || async {
            let item = self
                .core
                .list_messages(inbox, None, 10)
                .await
                .ok()?
                .items
                .into_iter()
                .next()?;
            Some((item.id, item.subject))
        })
        .await
    }

    fn mailbox_len(&self, name: &str) -> usize {
        self.server
            .read(|s| s.mailboxes.get(name).map_or(0, |mb| mb.messages.len()))
    }

    fn sent_count(&self) -> usize {
        self.sender.sent.lock().expect("sent").len()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_goes_out_unchanged_with_resent_fields_and_no_sent_copy() {
    let server = drafts_server();
    let uids = server.deliver("INBOX", 1, false);
    let bed = Bed::on(server).await;
    let (message, subject) = bed.inbox_message().await;
    let original = bed
        .core
        .get_message_source(message)
        .await
        .expect("source")
        .text;

    let mail = bed
        .core
        .redirect_message(
            RedirectRequest {
                message_id: message,
                identity_id: None,
                to: to("carol@example.net"),
                cc: Vec::new(),
                bcc: to("dave@example.net"),
            },
            "redirect-1".into(),
        )
        .await
        .expect("redirect");
    assert!(mail.redirect);
    assert_eq!(mail.draft_id, None, "a redirect has no draft");
    assert_eq!(mail.subject, subject);
    assert_eq!(mail.recipient_count, 2);
    bed.until("the redirect to be sent", || async {
        (bed.outgoing(mail.operation_id).await.state == SendState::Sent).then_some(())
    })
    .await;
    bed.settled().await;
    let bytes = bed.sender.sent.lock().expect("sent")[0].clone();
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(text.starts_with("Resent-Date: "), "{text}");
    assert!(
        text.contains("Resent-From: \"Alice\" <alice@fake.test>\r\n"),
        "{text}"
    );
    assert!(text.contains("Resent-To: \"Carol\" <carol@example.net>\r\n"));
    assert!(text.contains("Resent-Message-ID: <"));
    assert!(!text.contains("dave@"), "Bcc only in the envelope");
    // The original follows byte for byte (the source shows it with LF line ends).
    assert!(
        text.ends_with(&original.replace('\n', "\r\n")),
        "{text:?}\n{original:?}"
    );
    // No copy in Sent; the original is marked forwarded.
    assert_eq!(bed.mailbox_len("Sent"), 0);
    let flags = bed.server.read(|s| {
        s.mailboxes["INBOX"].messages[&uids[0]]
            .flags
            .keywords
            .clone()
    });
    assert!(
        flags.iter().any(|k| k.eq_ignore_ascii_case("$Forwarded")),
        "{flags:?}"
    );
    // The same press again queues nothing new.
    let again = bed
        .core
        .redirect_message(
            RedirectRequest {
                message_id: message,
                identity_id: None,
                to: to("carol@example.net"),
                cc: Vec::new(),
                bcc: Vec::new(),
            },
            "redirect-1".into(),
        )
        .await
        .expect("same request");
    assert_eq!(again.operation_id, mail.operation_id);
    bed.settled().await;
    assert_eq!(bed.sent_count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_waits_for_the_undo_delay_and_can_be_cancelled() {
    let server = drafts_server();
    server.deliver("INBOX", 1, false);
    let bed = Bed::on(server).await;
    bed.core
        .set_compose_settings(ComposeSettings {
            send_delay_seconds: 30,
            ..ComposeSettings::default()
        })
        .await
        .expect("delay");
    let (message, _) = bed.inbox_message().await;
    let mail = bed
        .core
        .redirect_message(
            RedirectRequest {
                message_id: message,
                identity_id: None,
                to: to("carol@example.net"),
                cc: Vec::new(),
                bcc: Vec::new(),
            },
            "redirect-2".into(),
        )
        .await
        .expect("redirect");
    assert_eq!(mail.state, SendState::Scheduled);
    assert!(!mail.scheduled, "the undo delay, not a chosen time");
    bed.core
        .cancel_redirect(mail.operation_id)
        .await
        .expect("undo");
    assert!(
        bed.core
            .list_outgoing(bed.account, None, 10)
            .await
            .expect("outgoing")
            .is_empty()
    );
    let err = bed
        .core
        .cancel_redirect(mail.operation_id)
        .await
        .expect_err("gone");
    assert_eq!(err.code(), ErrorCode::NotFound);
    assert_eq!(bed.sent_count(), 0, "nothing went out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redirects_are_refused_before_anything_is_queued() {
    let server = drafts_server();
    server.deliver("INBOX", 1, false);
    let bed = Bed::on(server).await;
    let (message, _) = bed.inbox_message().await;
    let request = |to: Vec<Address>, identity: Option<IdentityId>| RedirectRequest {
        message_id: message,
        identity_id: identity,
        to,
        cc: Vec::new(),
        bcc: Vec::new(),
    };
    let err = bed
        .core
        .redirect_message(request(Vec::new(), None), "r-a".into())
        .await
        .expect_err("no recipients");
    assert_eq!(err.code(), ErrorCode::InvalidAddress);
    let err = bed
        .core
        .redirect_message(request(to("not an address"), None), "r-b".into())
        .await
        .expect_err("bad address");
    assert_eq!(err.code(), ErrorCode::InvalidAddress);
    let err = bed
        .core
        .redirect_message(
            request(to("carol@example.net"), Some(IdentityId(9_999))),
            "r-c".into(),
        )
        .await
        .expect_err("foreign identity");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let many: Vec<Address> = (0..=mail_model::MAX_REDIRECT_RECIPIENTS)
        .map(|n| Address {
            name: None,
            email: format!("r{n}@example.net"),
        })
        .collect();
    let err = bed
        .core
        .redirect_message(request(many, None), "r-d".into())
        .await
        .expect_err("too many");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    // Read-only: refused as such.
    bed.core
        .set_writes_enabled(bed.account, false)
        .await
        .expect("writes off");
    let err = bed
        .core
        .redirect_message(request(to("carol@example.net"), None), "r-e".into())
        .await
        .expect_err("read only");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    assert!(
        bed.core
            .list_outgoing(bed.account, None, 10)
            .await
            .expect("outgoing")
            .is_empty()
    );
    assert_eq!(bed.sent_count(), 0);
}

/// A message the user sent, answering another one, with an attachment.
const SENT_REPLY: &[u8] = b"From: Alice <alice@fake.test>\r\n\
To: Bob <bob@friends.example>\r\n\
Cc: carol@example.net\r\n\
Subject: Re: Plans\r\n\
Message-ID: <1@fake.test>\r\n\
In-Reply-To: <plans.1@friends.example>\r\n\
References: <plans.1@friends.example>\r\n\
Date: Mon, 23 Nov 2026 12:00:00 +0000\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
See the list attached.\r\n\
--b1\r\n\
Content-Type: text/csv; name=\"list.csv\"\r\n\
Content-Disposition: attachment; filename=\"list.csv\"\r\n\
\r\n\
a,b\r\n\
--b1--\r\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_as_new_copies_a_sent_message_without_its_reply_reference() {
    let server = drafts_server();
    server.deliver_raw("Sent", SENT_REPLY);
    let bed = Bed::on(server).await;
    let sent = bed
        .core
        .list_mailboxes(bed.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == MailboxRole::Sent)
        .expect("sent")
        .id;
    let message = bed
        .until("the sent message", || async {
            Some(
                bed.core
                    .list_messages(sent, None, 10)
                    .await
                    .ok()?
                    .items
                    .first()?
                    .id,
            )
        })
        .await;
    let draft = bed
        .core
        .create_draft(mail_model::NewDraft::EditAsNew {
            message_id: message,
        })
        .await
        .expect("edit as new");
    assert_eq!(draft.kind, mail_model::DraftKind::New);
    assert_eq!(draft.subject, "Re: Plans");
    assert_eq!(
        draft
            .to
            .iter()
            .map(|a| a.email.as_str())
            .collect::<Vec<_>>(),
        ["bob@friends.example"]
    );
    assert_eq!(draft.cc[0].email, "carol@example.net");
    assert!(
        draft.body_text.contains("See the list attached."),
        "{draft:?}"
    );
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].file_name, "list.csv");
    assert!(
        !draft.include_signature,
        "the text has its signature already"
    );
    assert_eq!(draft.original_message_id, None);
    assert!(!draft.original_kept);

    // Sent: a new message with a new Message-ID, no reply reference, the attachment.
    let mail = bed
        .core
        .send_draft(draft.id.clone(), "edit-as-new-1".into())
        .await
        .expect("send");
    bed.until("the copy to be sent", || async {
        (bed.outgoing(mail.operation_id).await.state == SendState::Sent).then_some(())
    })
    .await;
    bed.settled().await;
    let bytes = bed.sender.sent.lock().expect("sent")[0].clone();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("In-Reply-To"), "{text}");
    assert!(!text.contains("References"), "{text}");
    assert!(!text.contains("<1@fake.test>"), "a Message-ID of its own");
    assert!(text.contains("filename=\"list.csv\""), "{text}");
    // The original stays in Sent.
    let subjects = bed.server.read(|s| {
        s.mailboxes["Sent"]
            .messages
            .values()
            .map(|m| {
                String::from_utf8_lossy(&crate::sync::fake::FakeServer::raw_of(m)).into_owned()
            })
            .collect::<Vec<_>>()
    });
    assert!(
        subjects.iter().any(|raw| raw.contains("<1@fake.test>")),
        "the original stays: {subjects:?}"
    );
}
