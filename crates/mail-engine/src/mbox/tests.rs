//! Stage 19 (decision 0057): mbox export and import through `AppCore` with the fake IMAP server
//! (appends through the outbox) and the real store.

use std::time::Duration;

use mail_content::{MboxEntry, MboxReader};
use mail_model::{
    ComposeSettings, ErrorCode, MailboxId, MailboxRole, MboxJob, MboxJobId, MboxJobState,
};
use tempfile::TempDir;
use tokio::time::Instant;

use crate::compose::tests::with_core::{Bed, drafts_server};

const FIXTURE_INBOX: &[u8] = include_bytes!(
    "../../../../testmail/thunderbird/Profiles/t3stpr0f.default-release/Mail/Local Folders/Inbox"
);

async fn mailbox(bed: &Bed, role: MailboxRole) -> MailboxId {
    bed.core
        .list_mailboxes(bed.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == role)
        .expect("mailbox")
        .id
}

async fn ended(bed: &Bed, id: &MboxJobId) -> MboxJob {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let job = bed
            .core
            .list_mbox_jobs()
            .into_iter()
            .find(|j| j.id == *id)
            .expect("job");
        if job.state != MboxJobState::Running {
            return job;
        }
        assert!(Instant::now() < deadline, "the job did not end: {job:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn messages(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut reader = MboxReader::new();
    let mut out = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if let Some(MboxEntry::Message { bytes, .. }) = reader.push_line(line) {
            out.push(bytes);
        }
    }
    if let Some(MboxEntry::Message { bytes, .. }) = reader.finish() {
        out.push(bytes);
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_exports_to_an_mbox_that_reads_back_byte_for_byte() {
    let server = drafts_server();
    server.deliver("INBOX", 2, false);
    let tricky = b"From: a@example.org\r\nSubject: tricky\r\nMessage-ID: <3@fake.test>\r\n\r\nFrom here on\r\n>From quoted\r\n".to_vec();
    server.deliver_raw("INBOX", &tricky);
    let bed = Bed::on(server).await;
    let inbox = mailbox(&bed, MailboxRole::Inbox).await;
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("inbox.mbox");
    assert_eq!(
        bed.core.mbox_export_name(inbox).await.expect("name"),
        "INBOX.mbox"
    );
    let job = bed
        .core
        .start_mbox_export(inbox, path.clone())
        .await
        .expect("start");
    assert_eq!(job.total, Some(3));
    let job = ended(&bed, &job.id).await;
    assert_eq!(job.state, MboxJobState::Done, "{job:?}");
    assert_eq!(job.done, 3);
    let file = std::fs::read(&path).expect("read");
    assert!(file.starts_with(b"From "), "mboxrd separator");
    let read = messages(&file);
    assert_eq!(read.len(), 3);
    assert!(read.contains(&tricky), "the body's From lines survive");
    // A second export of the same folder while one runs is refused; afterwards it is fine.
    let again = bed
        .core
        .start_mbox_export(inbox, dir.path().join("again.mbox"))
        .await
        .expect("again");
    let again = ended(&bed, &again.id).await;
    assert_eq!(again.state, MboxJobState::Done);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thunderbird_folder_imports_through_the_outbox_with_its_flags() {
    let server = drafts_server();
    server.with(|s| s.append = true);
    let bed = Bed::on(server).await;
    let archive = mailbox(&bed, MailboxRole::Archive).await;
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("Inbox");
    std::fs::write(&path, FIXTURE_INBOX).expect("write");

    // Read-only: refused before the file is read.
    bed.core
        .set_writes_enabled(bed.account, false)
        .await
        .expect("writes off");
    let err = bed
        .core
        .start_mbox_import(archive, path.clone())
        .await
        .expect_err("read only");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    bed.core
        .set_writes_enabled(bed.account, true)
        .await
        .expect("writes on");

    let job = bed
        .core
        .start_mbox_import(archive, path)
        .await
        .expect("start");
    let job = ended(&bed, &job.id).await;
    assert_eq!(job.state, MboxJobState::Done, "{job:?}");
    let ops = bed
        .core
        .list_operations(bed.account, None, 50)
        .await
        .expect("ops");
    assert_eq!(
        (job.done, job.skipped, job.queued),
        (2, 1, 0),
        "the deleted one is skipped: {ops:#?}"
    );
    bed.settled().await;
    let stored = bed.server.read(|s| {
        s.mailboxes["Archive"]
            .messages
            .values()
            .map(|m| {
                (
                    String::from_utf8_lossy(&crate::sync::fake::FakeServer::raw_of(m)).into_owned(),
                    m.flags.clone(),
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(stored.len(), 2);
    for (raw, _) in &stored {
        assert!(!raw.contains("X-Mozilla-Status"), "{raw}");
        assert!(raw.contains("TB fixture"), "{raw}");
    }
    let first = stored
        .iter()
        .find(|(raw, _)| raw.contains("TB fixture 1"))
        .expect("first");
    assert!(first.1.seen && first.1.flagged, "{:?}", first.1);
    let second = stored
        .iter()
        .find(|(raw, _)| raw.contains("TB fixture 2"))
        .expect("second");
    assert!(
        second.0.contains("\r\nFrom the archive"),
        "unquoted: {}",
        second.0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_import_stops_and_takes_its_waiting_appends_out() {
    let server = drafts_server();
    server.with(|s| s.append = true);
    let bed = Bed::on(server).await;
    bed.core
        .set_compose_settings(ComposeSettings::default())
        .await
        .expect("settings");
    let archive = mailbox(&bed, MailboxRole::Archive).await;
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("big.mbox");
    let mut file = Vec::new();
    for n in 0..400 {
        mail_content::write_mbox_message(
            &mut file,
            Some("a@example.org"),
            1_795_439_100,
            format!("Subject: n{n}\r\nMessage-ID: <big.{n}@example.org>\r\n\r\nbody {n}\r\n")
                .as_bytes(),
        );
    }
    std::fs::write(&path, &file).expect("write");
    let job = bed
        .core
        .start_mbox_import(archive, path)
        .await
        .expect("start");
    bed.core.cancel_mbox_job(job.id.clone()).expect("cancel");
    let job = ended(&bed, &job.id).await;
    assert_eq!(job.state, MboxJobState::Cancelled, "{job:?}");
    assert!(job.done < 400, "{job:?}");
    assert_eq!(job.queued, 0);
    bed.settled().await;
    let counts = bed.core.outbox_counts(bed.account).await.expect("counts");
    assert_eq!(counts.pending, 0, "nothing waits in the outbox");
    let unknown = bed
        .core
        .cancel_mbox_job(MboxJobId::from_random(1))
        .expect_err("unknown");
    assert_eq!(unknown.code(), ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_that_are_not_mbox_fail_the_job_with_a_reason() {
    let server = drafts_server();
    let bed = Bed::on(server).await;
    let archive = mailbox(&bed, MailboxRole::Archive).await;
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, "just some text\nwithout any separator\n").expect("write");
    let job = bed
        .core
        .start_mbox_import(archive, path)
        .await
        .expect("start");
    let job = ended(&bed, &job.id).await;
    assert_eq!(job.state, MboxJobState::Failed);
    assert_eq!(
        job.error.as_ref().map(|e| e.code),
        Some(ErrorCode::InvalidArgument)
    );
}
