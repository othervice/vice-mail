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
