//! Stage 20 through `AppCore` with the fake IMAP server, the recording sender and the clock of
//! the stage 10 tests: the attachment reminder (decision 0058).

use mail_model::ComposeSettings;

use super::stage10_tests::Bed;

#[tokio::test]
async fn the_attachment_reminder_reads_only_the_users_own_text() {
    let bed = Bed::new().await;
    let draft = bed
        .draft("Report", "<p>Hi Bob,</p><p>I attached the report.</p>")
        .await;
    assert_eq!(
        bed.core
            .check_attachment_reminder(draft.id.clone())
            .await
            .expect("check")
            .as_deref(),
        Some("attached")
    );

    // A quote in the text (a blockquote) and a negation do not count.
    let quoted = bed
        .write(
            &draft,
            "Report",
            "<p>Thanks!</p><blockquote><p>Please see the attachment.</p></blockquote>",
        )
        .await;
    assert_eq!(
        bed.core
            .check_attachment_reminder(quoted.id.clone())
            .await
            .expect("check"),
        None
    );
    let german = bed
        .write(&draft, "Bericht", "<p>Diesmal ohne Anhang, bis morgen.</p>")
        .await;
    assert_eq!(
        bed.core
            .check_attachment_reminder(german.id.clone())
            .await
            .expect("check"),
        None
    );

    // With an attachment there is nothing to remind of.
    let german = bed
        .write(&draft, "Bericht", "<p>Im Anhang der Bericht.</p>")
        .await;
    assert_eq!(
        bed.core
            .check_attachment_reminder(german.id.clone())
            .await
            .expect("check")
            .as_deref(),
        Some("Anhang")
    );
    let file = bed.dir.path().join("Bericht.pdf");
    std::fs::write(&file, b"%PDF-1.4\n").expect("file");
    bed.core
        .add_draft_attachments(draft.id.clone(), vec![file])
        .await
        .expect("attach");
    assert_eq!(
        bed.core
            .check_attachment_reminder(draft.id.clone())
            .await
            .expect("check"),
        None
    );

    // The setting off: never a reminder.
    let other = bed.draft("Other", "<p>See attached.</p>").await;
    let settings = bed.core.get_compose_settings().await.expect("settings");
    assert!(settings.attachment_reminder, "on by default");
    bed.core
        .set_compose_settings(ComposeSettings {
            attachment_reminder: false,
            ..settings
        })
        .await
        .expect("off");
    assert!(
        !bed.core
            .get_compose_settings()
            .await
            .expect("settings")
            .attachment_reminder
    );
    assert_eq!(
        bed.core
            .check_attachment_reminder(other.id.clone())
            .await
            .expect("check"),
        None
    );
}

/// Stage 20, task B (decision 0059): the reminder chosen in the composer counts once the server
/// accepted the message; the Sent copy carries it.
#[tokio::test]
async fn a_reminder_chosen_when_sending_is_recorded_once_accepted() {
    let bed = Bed::new().await;
    let draft = bed.draft("Offer", "<p>What do you think?</p>").await;
    let due = bed.now() + 3 * 24 * 3_600_000;
    let options = mail_model::DraftSendOptions {
        archive_original: false,
        request_read_receipt: false,
        follow_up_at: Some(due),
    };
    let saved = bed
        .core
        .set_draft_send_options(draft.id.clone(), options)
        .await
        .expect("options");
    assert_eq!(saved.follow_up_at, Some(due));
    // A time not ahead is refused.
    let err = bed
        .core
        .set_draft_send_options(
            draft.id.clone(),
            mail_model::DraftSendOptions {
                follow_up_at: Some(bed.now() - 1),
                ..options
            },
        )
        .await
        .expect_err("past");
    assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument);
    assert!(
        bed.core
            .list_follow_ups(mail_model::MarkScope::All)
            .await
            .expect("waiting")
            .is_empty(),
        "nothing before the send"
    );
    bed.core
        .send_draft(draft.id.clone(), "req-offer".into())
        .await
        .expect("send");
    // The fake server names messages `<serial>@fake.test`, so its Sent copy does not carry the
    // draft's Message-ID; the reminder is checked in the store here (the server test against
    // the test server checks the Sent copy, `tests/compose.rs`).
    let store = bed.core.store_ref().expect("store");
    let recorded = bed
        .until("the reminder of the sent message", || async {
            store.next_follow_up_due().await.expect("due")
        })
        .await;
    assert_eq!(recorded, due);
    assert!(!bed.sender.sent_with("Offer").is_empty());
}
