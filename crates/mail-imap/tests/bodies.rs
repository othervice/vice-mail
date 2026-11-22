//! Stage 3 against the test server: bodies are fetched with `BODY.PEEK` only and never change
//! the read state. Needs the seeded test server (see `tests/common/mod.rs`):
//!
//! ```text
//! scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000
//! cargo test -p mail-imap --test bodies -- --include-ignored
//! scripts/testmail.sh stop
//! ```

mod common;

use std::collections::HashMap;
use std::time::Instant;

use mail_imap::test_support::TestEnv;
use mail_imap::{ImapConnector, WireTap};
use mail_model::{
    Access, BodyFetchPlan, BodySection, FlagUpdate, LARGE_MESSAGE_THRESHOLD, MailBackend,
    MailConnector, MessageHeader, Security, UidSet,
};

/// Flags by UID (a fresh read from the server each time).
fn by_uid(updates: Vec<FlagUpdate>) -> HashMap<u32, FlagUpdate> {
    updates.into_iter().map(|u| (u.uid, u)).collect()
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn bodies_are_fetched_with_peek_and_leave_flags_alone() {
    let env = TestEnv::require();
    let tap = WireTap::new();
    let config = env.config(&env.alice, Security::Tls, Access::ReadOnly);
    let mut backend = ImapConnector::new()
        .with_wire_tap(tap.clone())
        .connect(&config)
        .await
        .expect("connect read-only");

    let mut checked_unseen = 0;
    let mut whole_bytes = 0usize;
    let mut large_checked = 0;
    for folder in ["INBOX", "Archive"] {
        let mailbox = backend.open_mailbox(folder).await.expect("examine");
        let all = backend.search_uids(&mailbox).await.expect("search");
        let before = by_uid(
            backend
                .fetch_flags(&mailbox, &all, None)
                .await
                .expect("flags before"),
        );
        let headers: Vec<MessageHeader> = {
            let mut headers = Vec::new();
            let mut rest = all.clone();
            while !rest.is_empty() {
                let (window, remaining) = rest.split_highest(500);
                headers.extend(
                    backend
                        .fetch_headers(&mailbox, &window)
                        .await
                        .expect("headers"),
                );
                rest = remaining;
            }
            headers
        };
        let size_of: HashMap<u32, u32> = headers.iter().map(|h| (h.uid, h.size)).collect();

        // Whole bodies of every unread message below the threshold, in batches of ~1 MB.
        let unseen: Vec<u32> = headers
            .iter()
            .filter(|h| !h.flags.seen && h.size <= LARGE_MESSAGE_THRESHOLD)
            .map(|h| h.uid)
            .collect();
        for batch in unseen.chunks(50) {
            let uids = UidSet::from_uids(batch.iter().copied());
            let started = Instant::now();
            let messages = backend
                .fetch_messages(&mailbox, &uids)
                .await
                .expect("bodies");
            assert_eq!(messages.len(), batch.len(), "{folder}: every UID came back");
            for message in &messages {
                assert_eq!(
                    u32::try_from(message.data.len()).ok(),
                    size_of.get(&message.uid).copied(),
                    "{folder} UID {}: BODY[] has RFC822.SIZE bytes",
                    message.uid
                );
                whole_bytes += message.data.len();
            }
            checked_unseen += messages.len();
            println!(
                "{folder}: {} bodies in {:?}",
                messages.len(),
                started.elapsed()
            );
        }

        // Large messages: structure, text first, then the whole message.
        for header in headers.iter().filter(|h| h.size > LARGE_MESSAGE_THRESHOLD) {
            let structure = backend
                .fetch_structure(&mailbox, header.uid)
                .await
                .expect("structure")
                .expect("exists");
            assert_eq!(structure.size, header.size);
            let BodyFetchPlan::Sections(plan) = structure.fetch_plan(LARGE_MESSAGE_THRESHOLD)
            else {
                panic!("{folder} UID {}: large message fetched whole", header.uid);
            };
            assert!(
                plan.len() >= 3,
                "header plus at least one text part: {plan:?}"
            );
            let started = Instant::now();
            let sections = backend
                .fetch_sections(&mailbox, header.uid, &plan)
                .await
                .expect("sections")
                .expect("exists");
            let text_first = started.elapsed();
            let fetched: usize = sections.iter().map(|s| s.data.len()).sum();
            assert_eq!(sections.len(), plan.len(), "{sections:?}");
            assert!(
                sections
                    .iter()
                    .any(|s| s.section == BodySection::Header && !s.data.is_empty()),
                "{sections:?}"
            );
            assert!(
                fetched < 100_000,
                "only text parts: {fetched} bytes of {}",
                header.size
            );
            let started = Instant::now();
            let whole = backend
                .fetch_messages(&mailbox, &UidSet::range(header.uid, header.uid))
                .await
                .expect("whole");
            println!(
                "{folder} UID {} ({} bytes): text parts {fetched} bytes in {text_first:?}, \
                 whole message in {:?}",
                header.uid,
                header.size,
                started.elapsed()
            );
            assert_eq!(whole.len(), 1);
            assert_eq!(u32::try_from(whole[0].data.len()).ok(), Some(header.size));
            large_checked += 1;
        }

        // The server's flags did not change, read back over the same and a new connection.
        let after = by_uid(
            backend
                .fetch_flags(&mailbox, &all, None)
                .await
                .expect("flags after"),
        );
        assert_eq!(before, after, "{folder}: flags changed");
        let mut fresh = ImapConnector::new()
            .connect(&config)
            .await
            .expect("second connection");
        let reopened = fresh.open_mailbox(folder).await.expect("examine");
        let independent = by_uid(
            fresh
                .fetch_flags(&reopened, &all, None)
                .await
                .expect("flags, second connection"),
        );
        let still_unseen = unseen
            .iter()
            .filter(|uid| independent.get(uid).is_some_and(|u| !u.flags.seen))
            .count();
        assert_eq!(
            still_unseen,
            unseen.len(),
            "{folder}: fetched mail stays unread"
        );
        fresh.logout().await.expect("logout");
    }
    println!(
        "checked {checked_unseen} unread bodies ({whole_bytes} bytes), {large_checked} large messages"
    );
    assert!(checked_unseen > 50, "the seed has unread mail to test with");
    // `large/zip-5mb` (6.8 MB) in Archive; `large/pdf-1mb` (2.05 MB) stays below the threshold.
    assert!(
        large_checked >= 1,
        "the seed has a message above the threshold"
    );

    // Only reading commands went out, and every body section was a PEEK.
    let verbs = tap.verbs();
    let reading = [
        "LOGIN",
        "ENABLE",
        "EXAMINE",
        "UID SEARCH",
        "UID FETCH",
        "LOGOUT",
    ];
    for verb in &verbs {
        assert!(
            reading.contains(&verb.as_str()),
            "unexpected command {verb} in {verbs:?}"
        );
    }
    for line in tap.lines() {
        let upper = line.to_ascii_uppercase();
        assert!(
            !upper.replace("BODY.PEEK[", "").contains("BODY["),
            "non-PEEK body section: {line}"
        );
        assert!(!upper.contains("BINARY["), "{line}");
    }
    let peeks = tap
        .lines()
        .iter()
        .filter(|l| l.contains("BODY.PEEK[]"))
        .count();
    assert!(peeks > 0, "positive control: the tap saw the body fetches");
    backend.logout().await.expect("logout");
}
