//! Tests of the stage 3 storage: body files and their references, partial bodies, damaged
//! files, removal with accounts and orphans, the sweep, previews, reading-pane data, prefetch
//! candidates and settings, and the remote-content allowlist.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mail_model::{
    BodySection, Disposition, ErrorCode, FetchedSection, FlagUpdate, Flags, MailboxRole, MessageId,
    MessageStructure, MimePart, PrefetchPeriod, PrefetchSettings, UidSet,
};
use tempfile::TempDir;

use crate::migrations::MIGRATIONS;
use crate::store::Schema;
use crate::test_support::{
    Fixture, UV, count, db_path, fixture, header, new_account, remote, seen,
};
use crate::{
    BODIES_DIR, GLOBAL_SCOPE, MAX_PREVIEW_CHARS, MailboxBaseline, Store, StoreError, StoreOptions,
    SweepReport,
};

const D: i64 = 1_764_000_000;

/// A raw message of about `size` bytes with a recognisable body.
fn raw(tag: &str, size: usize) -> Vec<u8> {
    let mut text = format!("Subject: {tag}\r\nMessage-ID: <{tag}@example.org>\r\n\r\n");
    while text.len() < size {
        text.push_str("Lorem ipsum dolor sit amet, consectetur adipiscing elit. ");
        text.push_str(tag);
        text.push_str("\r\n");
    }
    text.into_bytes()
}

/// Every file below the bodies directory, relative, sorted.
fn files_on_disk(store: &Store) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).expect("below root");
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(store.bodies_dir(), store.bodies_dir(), &mut out);
    out.sort();
    out
}

/// Stores `n` headers in `mailbox` from `first_uid`, dates `D + uid * 60`; returns their ids.
async fn messages(
    f: &Fixture,
    mailbox: mail_model::MailboxId,
    first_uid: u32,
    n: u32,
) -> Vec<MessageId> {
    let headers: Vec<_> = (first_uid..first_uid + n)
        .map(|uid| {
            seen(header(
                uid,
                D + i64::from(uid) * 60,
                Some(&format!("{mailbox}-{uid}@x")),
            ))
        })
        .collect();
    f.store
        .store_headers(mailbox, UV, headers, None)
        .await
        .expect("store headers");
    let page = f
        .store
        .list_messages(mailbox, None, 500)
        .await
        .expect("list");
    let mut ids: Vec<MessageId> = page.items.iter().map(|i| i.id).collect();
    ids.sort();
    ids
}

fn path(s: &str) -> mail_model::PartPath {
    s.parse().expect("part path")
}

fn part(p: Option<&str>, depth: u16, mime: &str, size: u32) -> MimePart {
    MimePart {
        path: p.map(path),
        depth,
        mime_type: mime.into(),
        charset: None,
        file_name: None,
        disposition: None,
        content_id: None,
        encoding: "base64".into(),
        size,
    }
}

/// multipart/mixed: text/plain (1), application/pdf attachment (2).
fn large_structure() -> MessageStructure {
    let mut pdf = part(Some("2"), 1, "application/pdf", 3_000_000);
    pdf.disposition = Some(Disposition::Attachment);
    pdf.file_name = Some("a.pdf".into());
    MessageStructure {
        size: 3_100_000,
        parts: vec![
            part(None, 0, "multipart/mixed", 0),
            part(Some("1"), 1, "text/plain", 20),
            pdf,
        ],
    }
}

fn section(spec: &str, data: &[u8]) -> FetchedSection {
    FetchedSection {
        section: spec.parse().expect("section"),
        data: data.to_vec(),
    }
}

// ---------------------------------------------------------------------------------------------
// Complete bodies
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_body_is_a_compressed_file_referenced_from_the_database() {
    let f = fixture().await;
    let [id] = messages(&f, f.inbox, 1, 1).await[..] else {
        panic!("one message");
    };
    assert_eq!(f.store.body_info(id).await.expect("info"), None);
    assert_eq!(f.store.load_message(id).await.expect("load"), None);

    let data = raw("first", 50_000);
    let info = f.store.store_body(id, data.clone()).await.expect("store");
    assert!(info.complete);
    assert_eq!(info.raw_size, 50_000_u64.max(data.len() as u64));
    assert!(
        info.stored_size > 0 && info.stored_size < info.raw_size / 5,
        "{info:?}"
    );
    assert!(info.sections.is_empty() && info.structure.is_none());
    assert!(info.fetched_at > 0);
    assert_eq!(
        f.store.body_info(id).await.expect("info"),
        Some(info.clone())
    );
    assert_eq!(
        f.store.load_message(id).await.expect("load"),
        Some(data.clone())
    );

    // One file, in the account's directory next to the database; the database holds no body.
    assert_eq!(
        f.store.bodies_dir(),
        f.store.path().parent().expect("data dir").join(BODIES_DIR)
    );
    let files = files_on_disk(&f.store);
    let [file] = files.as_slice() else {
        panic!("one body file: {files:?}");
    };
    assert!(
        file.starts_with(&format!("{}/{}-", f.account, id.0)),
        "{file}"
    );
    assert!(file.ends_with(".zst"));
    let on_disk = std::fs::metadata(f.store.bodies_dir().join(file))
        .expect("file")
        .len();
    assert_eq!(on_disk, info.stored_size);
    let db_bytes: i64 = count(&f.store, "SELECT sum(length(file)) FROM message_body_part").await;
    assert!(db_bytes < 100);

    // Fetching again replaces the body and removes the old file.
    let newer = raw("second", 5_000);
    let replaced = f
        .store
        .store_body(id, newer.clone())
        .await
        .expect("re-store");
    assert_eq!(replaced.raw_size, newer.len() as u64);
    assert_eq!(f.store.load_message(id).await.expect("load"), Some(newer));
    let after = files_on_disk(&f.store);
    assert_eq!(after.len(), 1);
    assert_ne!(after[0], *file);

    assert!(f.store.delete_body(id).await.expect("delete"));
    assert!(!f.store.delete_body(id).await.expect("delete again"));
    assert!(files_on_disk(&f.store).is_empty());
    assert_eq!(f.store.body_info(id).await.expect("info"), None);
}

#[tokio::test]
async fn bodies_of_unknown_messages_leave_nothing_behind() {
    let f = fixture().await;
    let err = f
        .store
        .store_body(MessageId(42), raw("x", 100))
        .await
        .expect_err("unknown message");
    assert!(matches!(err, StoreError::NotFound(_)), "{err}");
    let err = f
        .store
        .store_body_sections(MessageId(42), large_structure(), vec![])
        .await
        .expect_err("unknown message");
    assert!(matches!(err, StoreError::NotFound(_)), "{err}");
    assert!(files_on_disk(&f.store).is_empty());
    assert_eq!(
        f.store.load_message(MessageId(42)).await.expect("load"),
        None
    );
    assert!(
        f.store
            .load_sections(MessageId(42), None)
            .await
            .expect("load")
            .is_empty()
    );
}

#[tokio::test]
async fn empty_and_large_bodies_round_trip() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 2).await;
    let big: Vec<u8> = (0..6_000_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    f.store.store_body(ids[0], Vec::new()).await.expect("empty");
    f.store.store_body(ids[1], big.clone()).await.expect("6 MB");
    assert_eq!(
        f.store.load_message(ids[0]).await.expect("load"),
        Some(Vec::new())
    );
    assert_eq!(f.store.load_message(ids[1]).await.expect("load"), Some(big));
}

#[tokio::test]
async fn concurrent_stores_of_one_message_leave_one_consistent_body() {
    let f = fixture().await;
    let [id] = messages(&f, f.inbox, 1, 1).await[..] else {
        panic!("one message");
    };
    let a = raw("same message", 200_000);
    let (first, second) = tokio::join!(
        f.store.store_body(id, a.clone()),
        f.store.store_body(id, a.clone())
    );
    first.expect("first");
    second.expect("second");
    assert_eq!(f.store.load_message(id).await.expect("load"), Some(a));
    assert_eq!(files_on_disk(&f.store).len(), 1, "the loser's file is gone");
}

// ---------------------------------------------------------------------------------------------
// Partial bodies
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn partial_bodies_store_sections_and_grow_on_request() {
    let f = fixture().await;
    let [id] = messages(&f, f.inbox, 1, 1).await[..] else {
        panic!("one message");
    };
    let structure = large_structure();
    let text_first = vec![
        section("HEADER", b"Subject: big\r\n\r\n"),
        section("1.MIME", b"Content-Type: text/plain\r\n\r\n"),
        section("1", b"Hello, see the PDF.\r\n"),
    ];
    let info = f
        .store
        .store_body_sections(id, structure.clone(), text_first.clone())
        .await
        .expect("text first");
    assert!(!info.complete);
    assert_eq!(info.structure.as_ref(), Some(&structure));
    let specs: Vec<String> = info.sections.iter().map(BodySection::spec).collect();
    assert_eq!(specs, ["HEADER", "1", "1.MIME"]);
    let raw_text: usize = text_first.iter().map(|s| s.data.len()).sum();
    assert_eq!(info.raw_size, raw_text as u64);
    assert_eq!(
        f.store.load_message(id).await.expect("load"),
        None,
        "not complete"
    );

    let mut all = f.store.load_sections(id, None).await.expect("all");
    all.sort_by(|a, b| a.section.cmp(&b.section));
    let mut want = text_first.clone();
    want.sort_by(|a, b| a.section.cmp(&b.section));
    assert_eq!(all, want);
    let only_text = f
        .store
        .load_sections(
            id,
            Some(vec![
                BodySection::Part(path("1")),
                BodySection::Part(path("2")),
            ]),
        )
        .await
        .expect("some");
    assert_eq!(only_text, [section("1", b"Hello, see the PDF.\r\n")]);

    // The attachment on request: a second file.
    let pdf = vec![
        section("2.MIME", b"Content-Type: application/pdf\r\n\r\n"),
        section("2", &[7u8; 10_000]),
    ];
    let info = f
        .store
        .store_body_sections(id, structure.clone(), pdf.clone())
        .await
        .expect("attachment");
    assert_eq!(info.sections.len(), 5);
    assert_eq!(files_on_disk(&f.store).len(), 2);
    let pdf_part = f
        .store
        .load_sections(id, Some(vec![BodySection::Part(path("2"))]))
        .await
        .expect("pdf");
    assert_eq!(pdf_part, [section("2", &[7u8; 10_000])]);

    // Fetching the text again moves those sections into a new file; the old one goes.
    f.store
        .store_body_sections(id, structure.clone(), text_first.clone())
        .await
        .expect("text again");
    assert_eq!(files_on_disk(&f.store).len(), 2);
    let info = f.store.body_info(id).await.expect("info").expect("stored");
    assert_eq!(
        info.raw_size,
        (raw_text + pdf.iter().map(|s| s.data.len()).sum::<usize>()) as u64
    );
    let disk: u64 = files_on_disk(&f.store)
        .iter()
        .map(|file| {
            std::fs::metadata(f.store.bodies_dir().join(file))
                .expect("file")
                .len()
        })
        .sum();
    assert_eq!(info.stored_size, disk);

    // The whole message replaces the partial body and its files.
    let whole = raw("big", 3_000);
    let info = f.store.store_body(id, whole.clone()).await.expect("whole");
    assert!(info.complete && info.sections.is_empty() && info.structure.is_none());
    assert_eq!(files_on_disk(&f.store).len(), 1);
    assert_eq!(
        f.store.load_message(id).await.expect("load"),
        Some(whole.clone())
    );
    assert!(
        f.store
            .load_sections(id, None)
            .await
            .expect("none")
            .is_empty()
    );

    // Sections after a complete body are discarded, without a file left behind.
    let info = f
        .store
        .store_body_sections(id, structure, text_first)
        .await
        .expect("discarded");
    assert!(info.complete);
    assert_eq!(files_on_disk(&f.store).len(), 1);
    assert_eq!(f.store.load_message(id).await.expect("load"), Some(whole));
}

// ---------------------------------------------------------------------------------------------
// Damaged and missing files
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn damaged_or_missing_files_are_forgotten_and_read_as_not_stored() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 3).await;
    for &id in &ids {
        f.store
            .store_body(id, raw(&id.0.to_string(), 20_000))
            .await
            .expect("store");
    }
    let files = files_on_disk(&f.store);
    let file_of = |id: MessageId| {
        files
            .iter()
            .find(|f| f.contains(&format!("/{}-", id.0)))
            .cloned()
            .expect("file of message")
    };

    // One flipped bit: the checksum catches it.
    let damaged = f.store.bodies_dir().join(file_of(ids[0]));
    let mut bytes = std::fs::read(&damaged).expect("read");
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x01;
    std::fs::write(&damaged, bytes).expect("damage");
    // Removed from outside.
    std::fs::remove_file(f.store.bodies_dir().join(file_of(ids[1]))).expect("remove");
    // Truncated to nothing.
    std::fs::write(f.store.bodies_dir().join(file_of(ids[2])), b"").expect("truncate");

    for &id in &ids {
        assert_eq!(f.store.load_message(id).await.expect("load"), None, "{id}");
        assert_eq!(
            f.store.body_info(id).await.expect("info"),
            None,
            "{id} forgotten"
        );
    }
    assert!(files_on_disk(&f.store).is_empty(), "damaged files removed");
    // The next fetch stores it again.
    f.store
        .store_body(ids[0], raw("again", 1_000))
        .await
        .expect("again");
    assert!(f.store.load_message(ids[0]).await.expect("load").is_some());
}

#[tokio::test]
async fn a_damaged_section_file_forgets_only_its_sections() {
    let f = fixture().await;
    let [id] = messages(&f, f.inbox, 1, 1).await[..] else {
        panic!("one message");
    };
    f.store
        .store_body_sections(
            id,
            large_structure(),
            vec![section("HEADER", b"Subject: x\r\n\r\n")],
        )
        .await
        .expect("header");
    let first = files_on_disk(&f.store);
    f.store
        .store_body_sections(id, large_structure(), vec![section("2", b"PDF")])
        .await
        .expect("attachment");
    let second: Vec<String> = files_on_disk(&f.store)
        .into_iter()
        .filter(|f| !first.contains(f))
        .collect();
    std::fs::write(f.store.bodies_dir().join(&second[0]), b"garbage").expect("damage");

    let loaded = f.store.load_sections(id, None).await.expect("load");
    assert_eq!(loaded, [section("HEADER", b"Subject: x\r\n\r\n")]);
    let info = f
        .store
        .body_info(id)
        .await
        .expect("info")
        .expect("still partial");
    assert_eq!(info.sections, [BodySection::Header]);
    assert_eq!(files_on_disk(&f.store), first);
}

// ---------------------------------------------------------------------------------------------
// Removal with accounts and orphans, sweep
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn removing_an_account_removes_its_body_files_and_permissions() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 2).await;
    for &id in &ids {
        f.store
            .store_body(id, raw("a", 1_000))
            .await
            .expect("store");
    }
    f.store
        .allow_remote_content(f.account, "news@example.org")
        .await
        .expect("allow");

    // A second account with a body of its own.
    let other = f
        .store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("bob")
        .id;
    f.store
        .sync_mailboxes(other, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let bob_inbox = f.store.mailboxes(other).await.expect("mailboxes")[0].id;
    f.store
        .reset_mailbox(
            bob_inbox,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    f.store
        .store_headers(bob_inbox, UV, vec![header(1, D + 7, Some("bob@x"))], None)
        .await
        .expect("bob header");
    let bob_message = f
        .store
        .list_messages(bob_inbox, None, 10)
        .await
        .expect("list")
        .items[0]
        .id;
    f.store
        .store_body(bob_message, raw("b", 1_000))
        .await
        .expect("bob body");

    assert!(f.store.delete_account(f.account).await.expect("delete"));
    let left = files_on_disk(&f.store);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].starts_with(&other.to_string()));
    assert!(!f.store.bodies_dir().join(f.account.to_string()).exists());
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM message_body").await,
        1
    );
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM remote_content_sender").await,
        0
    );
    assert!(
        f.store
            .load_message(bob_message)
            .await
            .expect("bob")
            .is_some()
    );
}

#[tokio::test]
async fn orphan_cleanup_removes_body_files_of_deleted_messages_only() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 3).await;
    for &id in &ids {
        f.store
            .store_body(id, raw("o", 2_000))
            .await
            .expect("store");
    }
    // UID 1 moved to the archive (same Message-ID: merged), UIDs 2 and 3 deleted.
    f.store
        .store_headers(
            f.archive,
            UV,
            vec![seen(header(50, D + 60, Some(&format!("{}-1@x", f.inbox))))],
            None,
        )
        .await
        .expect("moved copy");
    f.store
        .remove_uids(f.inbox, UV, UidSet::range(1, 3))
        .await
        .expect("remove");
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("orphans"),
        2
    );
    let left = files_on_disk(&f.store);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(f.store.load_message(ids[0]).await.expect("moved").is_some());
    assert_eq!(f.store.body_info(ids[1]).await.expect("info"), None);
}

#[tokio::test]
async fn sweep_removes_leftovers_but_not_bodies_in_use() {
    let f = fixture().await;
    let [id] = messages(&f, f.inbox, 1, 1).await[..] else {
        panic!("one message");
    };
    f.store
        .store_body(id, raw("kept", 1_000))
        .await
        .expect("store");
    let kept = files_on_disk(&f.store);
    let account_dir = f.store.bodies_dir().join(f.account.to_string());
    std::fs::write(
        account_dir.join(format!("{}-dead.zst", id.0)),
        b"crash leftover",
    )
    .expect("leftover");
    std::fs::write(account_dir.join(".tmp-1-2-x.zst"), b"half written").expect("temp");
    std::fs::create_dir_all(
        f.store
            .bodies_dir()
            .join("00000000-0000-4000-8000-000000000099"),
    )
    .expect("removed account");

    let young = f
        .store
        .sweep_body_files(Duration::from_secs(3_600))
        .await
        .expect("sweep");
    assert_eq!(young, SweepReport::default(), "young files stay");
    let report = f
        .store
        .sweep_body_files(Duration::ZERO)
        .await
        .expect("sweep");
    assert_eq!(
        (report.removed_files, report.removed_dirs, report.failed),
        (2, 1, 0)
    );
    assert_eq!(files_on_disk(&f.store), kept);
    assert!(f.store.load_message(id).await.expect("load").is_some());

    // A store without any body yet has no directory; the sweep is fine with that.
    let dir = TempDir::new().expect("temp dir");
    let empty = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    assert_eq!(
        empty.sweep_body_files(Duration::ZERO).await.expect("sweep"),
        SweepReport::default()
    );
}

// ---------------------------------------------------------------------------------------------
// Previews and reading-pane data
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn previews_summary_and_locations() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 2).await;
    let written = f
        .store
        .set_previews(vec![
            (ids[0], "  Hallo\r\n Welt  ".into()),
            (ids[1], "x".repeat(1_000)),
            (MessageId(1), "unknown".into()),
        ])
        .await
        .expect("previews");
    assert_eq!(written, 2);
    let summary = f
        .store
        .message_summary(ids[0])
        .await
        .expect("summary")
        .expect("exists");
    assert_eq!(summary.preview.as_deref(), Some("Hallo Welt"));
    assert_eq!(summary.account_id, f.account);
    assert_eq!(summary.subject, "Subject 1");
    assert_eq!(
        summary.from.as_ref().map(|a| a.email.as_str()),
        Some("sender@example.org")
    );
    assert_eq!(summary.to.len(), 1);
    assert_eq!(
        summary.message_id_header.as_deref(),
        Some(format!("{}-1@x", f.inbox).as_str())
    );
    assert_eq!(summary.date, D + 60);
    let long = f
        .store
        .message_summary(ids[1])
        .await
        .expect("summary")
        .expect("exists");
    assert_eq!(
        long.preview.map(|p| p.chars().count()),
        Some(MAX_PREVIEW_CHARS)
    );
    // Clearing.
    f.store
        .set_previews(vec![(ids[0], " ".into())])
        .await
        .expect("clear");
    assert_eq!(
        f.store
            .message_summary(ids[0])
            .await
            .expect("summary")
            .expect("exists")
            .preview,
        None
    );
    assert_eq!(
        f.store
            .message_summary(MessageId(1))
            .await
            .expect("summary"),
        None
    );

    // The same mail in the archive too (merged by Message-ID): two locations.
    f.store
        .store_headers(
            f.archive,
            UV,
            vec![header(9, D + 60, Some(&format!("{}-1@x", f.inbox)))],
            None,
        )
        .await
        .expect("copy");
    let locations = f.store.message_locations(ids[0]).await.expect("locations");
    let found: Vec<(MailboxRole, u32, Option<u32>)> = locations
        .iter()
        .map(|l| (l.role, l.uid, l.uid_validity.map(|v| v.0)))
        .collect();
    assert_eq!(
        found,
        [
            (MailboxRole::Inbox, 1, Some(UV.0)),
            (MailboxRole::Archive, 9, Some(UV.0))
        ]
    );
    assert!(
        locations
            .iter()
            .all(|l| l.selectable && !l.mailbox_name.is_empty())
    );
    assert!(
        f.store
            .message_locations(MessageId(1))
            .await
            .expect("none")
            .is_empty()
    );
}

// ---------------------------------------------------------------------------------------------
// Prefetch
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn prefetch_candidates_cover_inbox_sent_and_flagged_mail_without_body() {
    let f = fixture().await;
    let inbox = messages(&f, f.inbox, 1, 5).await; // D+60 .. D+300
    let sent = messages(&f, f.sent, 10, 2).await; // D+600, D+660
    let archive = messages(&f, f.archive, 20, 3).await; // D+1200 .. D+1320
    // Flag the newest archive message; flag an inbox one too (listed once).
    for (mailbox, uid) in [(f.archive, 22), (f.inbox, 5)] {
        f.store
            .update_flags(
                mailbox,
                UV,
                vec![FlagUpdate {
                    uid,
                    flags: Flags {
                        seen: true,
                        flagged: true,
                        ..Flags::default()
                    },
                    modseq: None,
                }],
            )
            .await
            .expect("flag");
    }
    // A complete and a partial body exist already.
    f.store
        .store_body(inbox[0], raw("done", 100))
        .await
        .expect("body");
    f.store
        .store_body_sections(
            inbox[1],
            large_structure(),
            vec![section("HEADER", b"S: x\r\n\r\n")],
        )
        .await
        .expect("partial");

    let all = f
        .store
        .prefetch_candidates(f.account, None, None, 100)
        .await
        .expect("candidates");
    let ids: Vec<MessageId> = all.iter().map(|c| c.message_id).collect();
    let mut want = vec![archive[2], sent[1], sent[0], inbox[4], inbox[3], inbox[2]];
    want.sort_by(|a, b| b.cmp(a));
    assert_eq!(
        ids, want,
        "newest first, archive only flagged, stored ones left out"
    );
    let flagged = all
        .iter()
        .find(|c| c.message_id == archive[2])
        .expect("flagged");
    assert_eq!((flagged.mailbox_id, flagged.uid), (f.archive, 22));
    assert!(all.iter().all(|c| c.size > 1_000));

    // Period, limit and continuation past failures.
    let recent = f
        .store
        .prefetch_candidates(f.account, Some(D + 600), None, 100)
        .await
        .expect("recent");
    assert_eq!(
        recent.iter().map(|c| c.message_id).collect::<Vec<_>>(),
        want[..3]
    );
    let first_two = f
        .store
        .prefetch_candidates(f.account, None, None, 2)
        .await
        .expect("limit");
    assert_eq!(
        first_two.iter().map(|c| c.message_id).collect::<Vec<_>>(),
        want[..2]
    );
    let rest = f
        .store
        .prefetch_candidates(f.account, None, Some(want[1]), 100)
        .await
        .expect("rest");
    assert_eq!(
        rest.iter().map(|c| c.message_id).collect::<Vec<_>>(),
        want[2..]
    );

    for bad in [0, crate::MAX_PREFETCH_BATCH + 1] {
        let err = f
            .store
            .prefetch_candidates(f.account, None, None, bad)
            .await
            .expect_err("bad limit");
        assert!(matches!(err, StoreError::InvalidArgument(_)), "{err}");
    }
    let err = f
        .store
        .prefetch_candidates(crate::test_support::account_id(77), None, None, 10)
        .await
        .expect_err("unknown account");
    assert!(matches!(err, StoreError::NotFound(_)), "{err}");
}

#[tokio::test]
async fn prefetch_settings_default_store_and_survive_garbage() {
    let f = fixture().await;
    assert_eq!(
        f.store.prefetch_settings().await.expect("defaults"),
        PrefetchSettings::default()
    );
    let wanted = PrefetchSettings {
        period: PrefetchPeriod::Days365,
        max_message_size: 25 * 1024 * 1024,
    };
    assert_eq!(
        f.store.set_prefetch_settings(wanted).await.expect("set"),
        wanted
    );
    assert_eq!(f.store.prefetch_settings().await.expect("get"), wanted);
    let all = PrefetchSettings {
        period: PrefetchPeriod::All,
        max_message_size: 0,
    };
    assert_eq!(f.store.set_prefetch_settings(all).await.expect("set"), all);

    let err = f
        .store
        .set_prefetch_settings(PrefetchSettings {
            max_message_size: PrefetchSettings::MAX_MESSAGE_SIZE + 1,
            ..wanted
        })
        .await
        .expect_err("too large");
    assert_eq!(
        mail_model::CoreError::from(err).code(),
        ErrorCode::InvalidArgument
    );
    assert_eq!(f.store.prefetch_settings().await.expect("unchanged"), all);

    // Values written by a newer or broken build fall back to the defaults, one by one.
    f.store
        .put_settings(
            GLOBAL_SCOPE,
            vec![
                ("prefetch.period".into(), "\"days14\"".into()),
                ("prefetch.maxMessageSize".into(), "1048576".into()),
            ],
        )
        .await
        .expect("garbage");
    assert_eq!(
        f.store.prefetch_settings().await.expect("fallback"),
        PrefetchSettings {
            period: PrefetchPeriod::Days30,
            max_message_size: 1_048_576,
        }
    );
}

// ---------------------------------------------------------------------------------------------
// Remote content allowlist
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn remote_content_allowlist_per_account_and_normalised_sender() {
    let f = fixture().await;
    let store = &f.store;
    assert!(
        !store
            .remote_content_allowed(f.account, "news@example.org")
            .await
            .expect("check")
    );
    assert!(
        store
            .allow_remote_content(f.account, " <News@Example.ORG> ")
            .await
            .expect("allow")
    );
    assert!(
        !store
            .allow_remote_content(f.account, "news@example.org")
            .await
            .expect("again")
    );
    for spelling in ["news@example.org", "NEWS@EXAMPLE.ORG", "<news@example.org>"] {
        assert!(
            store
                .remote_content_allowed(f.account, spelling)
                .await
                .expect("check"),
            "{spelling}"
        );
    }
    assert!(
        !store
            .remote_content_allowed(f.account, "other@example.org")
            .await
            .expect("check")
    );
    assert!(
        !store
            .remote_content_allowed(f.account, "not an address")
            .await
            .expect("check")
    );
    store
        .allow_remote_content(f.account, "a@shop.example")
        .await
        .expect("allow");
    assert_eq!(
        store.remote_content_senders(f.account).await.expect("list"),
        ["a@shop.example", "news@example.org"]
    );

    // Per account.
    let other = store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("bob")
        .id;
    assert!(
        !store
            .remote_content_allowed(other, "news@example.org")
            .await
            .expect("check")
    );

    assert!(
        store
            .revoke_remote_content(f.account, "NEWS@example.org")
            .await
            .expect("revoke")
    );
    assert!(
        !store
            .revoke_remote_content(f.account, "news@example.org")
            .await
            .expect("again")
    );
    assert!(
        !store
            .remote_content_allowed(f.account, "news@example.org")
            .await
            .expect("check")
    );

    let err = store
        .allow_remote_content(f.account, "a b@x")
        .await
        .expect_err("invalid");
    assert!(matches!(err, StoreError::InvalidArgument(_)), "{err}");
    let err = store
        .allow_remote_content(crate::test_support::account_id(77), "a@example.org")
        .await
        .expect_err("unknown account");
    assert!(matches!(err, StoreError::NotFound(_)), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Migration from stage 2
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_stage_2_database_with_mail_migrates_and_keeps_it() {
    let dir = TempDir::new().expect("temp dir");
    let path: PathBuf = db_path(&dir);
    let stage2 = Schema::from_vec(MIGRATIONS[..2].to_vec());
    let store = Store::open_with(&path, StoreOptions::default(), &stage2).expect("stage 2");
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(account, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let inbox = store.mailboxes(account).await.expect("mailboxes")[0].id;
    store
        .reset_mailbox(
            inbox,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    let headers: Vec<_> = (1..=20)
        .map(|uid| header(uid, D + i64::from(uid), Some(&format!("{uid}@m"))))
        .collect();
    store
        .store_headers(inbox, UV, headers, None)
        .await
        .expect("headers");
    let before = store.list_messages(inbox, None, 100).await.expect("list");
    drop(store);

    let store = Store::open(&path, StoreOptions::default()).expect("migrate");
    let report = store.open_report().clone();
    assert_eq!((report.previous_version, report.version), (2, 3));
    assert!(report.backup.is_some());
    assert_eq!(
        store.list_messages(inbox, None, 100).await.expect("list"),
        before
    );
    let columns: BTreeSet<String> = store
        .read(|c| {
            Ok(c.prepare("SELECT name FROM pragma_table_info('message')")?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<BTreeSet<_>, _>>()?)
        })
        .await
        .expect("columns");
    assert!(
        columns.contains("preview") && !columns.contains("body_ref"),
        "{columns:?}"
    );
    let id = before.items[0].id;
    store
        .store_body(id, raw("migrated", 500))
        .await
        .expect("body after migration");
    assert!(store.load_message(id).await.expect("load").is_some());
}

// ---------------------------------------------------------------------------------------------
// Packed bodies (prefetch batches)
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn small_bodies_of_one_batch_share_a_file_until_the_last_one_goes() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 12).await;
    let mut batch: Vec<(MessageId, Vec<u8>)> = ids[..10]
        .iter()
        .map(|id| (*id, raw(&format!("small {}", id.0), 20_000)))
        .collect();
    // One above the pack limit gets its own file; an unknown message is skipped.
    batch.push((ids[10], raw("large", crate::BODY_PACK_LIMIT + 1)));
    batch.push((MessageId(7), raw("unknown", 100)));
    let infos = f.store.store_bodies(batch.clone()).await.expect("batch");
    assert_eq!(
        infos.iter().map(|i| i.message).collect::<Vec<_>>(),
        ids[..11],
        "input order, unknown skipped"
    );
    let files = files_on_disk(&f.store);
    assert_eq!(files.len(), 2, "one pack and the large body: {files:?}");
    for (id, data) in &batch[..11] {
        assert_eq!(
            f.store.load_message(*id).await.expect("load").as_ref(),
            Some(data)
        );
    }
    let disk: u64 = files
        .iter()
        .map(|file| {
            std::fs::metadata(f.store.bodies_dir().join(file))
                .expect("file")
                .len()
        })
        .sum();
    assert_eq!(infos.iter().map(|i| i.stored_size).sum::<u64>(), disk);

    // Deleting some packed bodies keeps the file for the others.
    for id in &ids[..9] {
        assert!(f.store.delete_body(*id).await.expect("delete"));
    }
    assert_eq!(files_on_disk(&f.store).len(), 2);
    assert!(f.store.load_message(ids[9]).await.expect("load").is_some());
    // Re-storing the last packed body moves it to a new file; the pack is then unreferenced.
    f.store
        .store_body(ids[9], raw("again", 100))
        .await
        .expect("re-store");
    let after = files_on_disk(&f.store);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(
        !after
            .iter()
            .any(|f| files.contains(f) && !f.contains(&format!("/{}-", ids[10].0)))
    );
    assert_eq!(
        f.store.store_bodies(vec![]).await.expect("empty batch"),
        vec![]
    );
}

#[tokio::test]
async fn packed_bodies_follow_their_messages_on_orphan_cleanup_and_damage() {
    let f = fixture().await;
    let ids = messages(&f, f.inbox, 1, 4).await;
    let batch: Vec<_> = ids
        .iter()
        .map(|id| (*id, raw(&id.0.to_string(), 5_000)))
        .collect();
    f.store.store_bodies(batch).await.expect("batch");
    let pack = files_on_disk(&f.store);
    assert_eq!(pack.len(), 1);

    // Two of four deleted on the server: the pack stays for the other two.
    f.store
        .remove_uids(f.inbox, UV, UidSet::range(1, 2))
        .await
        .expect("remove");
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("orphans"),
        2
    );
    assert_eq!(files_on_disk(&f.store), pack);
    assert!(f.store.load_message(ids[2]).await.expect("load").is_some());

    // The pack is damaged: each message finds out on its own read; the file goes with the last.
    std::fs::write(f.store.bodies_dir().join(&pack[0]), b"garbage").expect("damage");
    assert_eq!(f.store.load_message(ids[2]).await.expect("load"), None);
    assert_eq!(
        files_on_disk(&f.store),
        pack,
        "still referenced by the other message"
    );
    assert_eq!(f.store.load_message(ids[3]).await.expect("load"), None);
    assert!(files_on_disk(&f.store).is_empty());
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM message_body").await,
        0
    );
}

#[tokio::test]
async fn bodies_of_two_accounts_are_never_packed_together() {
    let f = fixture().await;
    let alice = messages(&f, f.inbox, 1, 2).await;
    let other = f
        .store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("bob")
        .id;
    f.store
        .sync_mailboxes(other, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let bob_inbox = f.store.mailboxes(other).await.expect("mailboxes")[0].id;
    f.store
        .reset_mailbox(
            bob_inbox,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    f.store
        .store_headers(bob_inbox, UV, vec![header(1, D + 7, Some("bob@x"))], None)
        .await
        .expect("bob header");
    let bob = f
        .store
        .list_messages(bob_inbox, None, 10)
        .await
        .expect("list")
        .items[0]
        .id;
    f.store
        .store_bodies(vec![
            (alice[0], raw("a1", 1_000)),
            (bob, raw("b", 1_000)),
            (alice[1], raw("a2", 1_000)),
        ])
        .await
        .expect("batch");
    let files = files_on_disk(&f.store);
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files.iter().any(|f| f.starts_with(&other.to_string())));
    assert!(f.store.delete_account(other).await.expect("delete bob"));
    assert_eq!(files_on_disk(&f.store).len(), 1);
    assert!(
        f.store
            .load_message(alice[1])
            .await
            .expect("load")
            .is_some()
    );
}
