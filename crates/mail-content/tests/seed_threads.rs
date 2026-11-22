//! Threading (`mail_model::threading`, stage 7) over the seed corpus, parsed with
//! [`mail_content::parse`]. Lives here rather than in `mail-model` because `mail-model` cannot
//! depend on `mail-content` (which depends on it).
//!
//! - `seed_fixtures_thread_apart`: the fixtures in `testmail/corpus/seed/` are unrelated mails
//!   (plus a self-referencing one); none may be merged, in any order.
//! - `full_seed_threads_like_its_references` (ignored, needs `python3`): a fresh dump of
//!   `testmail/seed.py 1000` with the 60-message thread and 855 filler mails, compared with an
//!   independent grouping by references, in date order and in reverse date order (the order of
//!   the initial sync).

mod common;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use mail_content::{ParsedMessage, parse};
use mail_model::MessageId;
use mail_model::threading::{
    ThreadConfig, ThreadIndex, ThreadMessage, ThreadReason, ThreadTarget, is_bulk,
};

const ALICE: &str = "alice@vicemail.test";

fn thread_message(local: i64, parsed: &ParsedMessage) -> ThreadMessage {
    let h = &parsed.headers;
    ThreadMessage {
        message: MessageId(local),
        message_id: h.message_id.clone(),
        in_reply_to: h.in_reply_to.clone(),
        references: h.references.clone(),
        subject: h.subject.clone().unwrap_or_default(),
        participants: h
            .from
            .iter()
            .chain(&h.to)
            .chain(&h.cc)
            .map(|a| a.email.clone())
            .collect(),
        date: h.date.unwrap_or(0),
        bulk: is_bulk(
            h.list.id.as_deref(),
            h.list.unsubscribe.first().map(String::as_str),
            h.precedence.as_deref(),
            h.auto_submitted.as_deref(),
        ),
    }
}

fn load(files: &[PathBuf]) -> Vec<(String, ThreadMessage)> {
    files
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let raw = fs::read(path).expect("read fixture");
            let local = i64::try_from(i).expect("small") + 1;
            (common::file_name(path), thread_message(local, &parse(&raw)))
        })
        .collect()
}

#[test]
fn seed_fixtures_thread_apart() {
    let messages = load(&common::eml_files("seed"));
    assert!(messages.len() >= 70);
    for reverse in [false, true] {
        let mut index = ThreadIndex::new(ThreadConfig::new([ALICE]));
        let mut order: Vec<&(String, ThreadMessage)> = messages.iter().collect();
        if reverse {
            order.reverse();
        }
        for (name, message) in order {
            let assignment = index.assign(message);
            assert_eq!(
                assignment.target,
                ThreadTarget::New,
                "{name}: {assignment:?}"
            );
            if name == "broken_reference-loop.eml" {
                assert_eq!(
                    assignment.link_ids,
                    [
                        "broken.reference-loop@seed.vicemail.test",
                        "missing-parent@nowhere.example"
                    ]
                );
            }
            index.apply(message.clone(), &assignment);
        }
        assert_eq!(index.len(), messages.len());
    }
    // List, newsletter and automatic mails are recognised as bulk (`List-Id`,
    // `List-Unsubscribe`, `Precedence`, `Auto-Submitted`), nothing else.
    for (name, message) in &messages {
        let expected = ["list_", "auto_", "html_newsletter-", "mime_digest"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        assert_eq!(message.bulk, expected, "{name}");
    }
}

/// Groups messages by their Message-ID, In-Reply-To and References, all of them, without any
/// of the threading rules: the seed sets these headers on every reply it means.
fn reference_groups(messages: &[(String, ThreadMessage)]) -> Vec<Vec<MessageId>> {
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut root = i;
        while parent[root] != root {
            root = parent[root];
        }
        parent[i] = root;
        root
    }
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    let mut node = |id: &str, parent: &mut Vec<usize>| -> usize {
        let id = id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_owned();
        *ids.entry(id).or_insert_with(|| {
            parent.push(parent.len());
            parent.len() - 1
        })
    };
    let mut message_nodes = Vec::new();
    for (name, m) in messages {
        let own = parent.len();
        parent.push(own);
        message_nodes.push(own);
        // The seed's deliberate duplicate reuses thread message 3's id for another mail.
        let own_id = m
            .message_id
            .as_deref()
            .filter(|_| name != "broken_duplicate-message-id.eml");
        for id in own_id
            .into_iter()
            .chain(m.in_reply_to.iter().map(String::as_str))
            .chain(m.references.iter().map(String::as_str))
        {
            let other = node(id, &mut parent);
            let (a, b) = (find(&mut parent, own), find(&mut parent, other));
            parent[a] = b;
        }
    }
    let mut groups: BTreeMap<usize, Vec<MessageId>> = BTreeMap::new();
    for (i, (_, m)) in messages.iter().enumerate() {
        let root = find(&mut parent, message_nodes[i]);
        groups.entry(root).or_default().push(m.message);
    }
    let mut groups: Vec<Vec<MessageId>> = groups
        .into_values()
        .map(|mut g| {
            g.sort();
            g
        })
        .collect();
    groups.sort();
    groups
}

#[test]
#[ignore = "needs python3; run with --ignored"]
fn full_seed_threads_like_its_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("mail-content-threads-{}", std::process::id()));
    let status = std::process::Command::new("python3")
        .arg(root.join("testmail/seed.py"))
        .args(["1000", "--dump"])
        .arg(&out)
        .status()
        .expect("run python3");
    assert!(status.success());
    let mut files: Vec<PathBuf> = fs::read_dir(out.join("alice"))
        .expect("dump")
        .map(|e| e.expect("entry").path())
        .collect();
    files.sort();
    let messages = load(&files);
    let _ = fs::remove_dir_all(&out);
    assert!(messages.len() >= 990, "{}", messages.len());

    let names: HashMap<MessageId, &str> = messages
        .iter()
        .map(|(n, m)| (m.message, n.as_str()))
        .collect();
    let thread_all = |order: &[&(String, ThreadMessage)]| {
        let mut index = ThreadIndex::new(ThreadConfig::new([ALICE]));
        let mut by_subject = Vec::new();
        for (name, message) in order {
            let assignment = index.assign(message);
            if assignment.reason == ThreadReason::Subject {
                by_subject.push((name.clone(), message.subject.clone()));
            }
            index.apply(message.clone(), &assignment);
        }
        (index, by_subject)
    };
    // A rebuild runs in date order.
    let mut order: Vec<&(String, ThreadMessage)> = messages.iter().collect();
    order.sort_by_key(|(name, m)| (m.date, name.clone()));
    let (index, by_subject) = thread_all(&order);
    let offsite: Vec<MessageId> = messages
        .iter()
        .filter(|(n, _)| n.starts_with("thread_offsite-"))
        .map(|(_, m)| m.message)
        .collect();
    assert_eq!(offsite.len(), 60);
    let offsite_thread = index.thread_of(offsite[0]).expect("threaded");
    assert_eq!(
        index.messages_of(offsite_thread),
        offsite,
        "the long thread is one thread"
    );

    // Every thread is exactly a reference group: nothing joined by subject or split.
    let expected = reference_groups(&messages);
    let got = index.partition();
    let describe = |groups: &[Vec<MessageId>]| -> Vec<Vec<&str>> {
        groups
            .iter()
            .filter(|g| g.len() > 1)
            .map(|g| g.iter().map(|m| names[m]).collect())
            .collect()
    };
    // The filler has about 50 `Re:`/`AW:` mails without references (random subjects from random
    // senders): none of them meets the subject rules, so none may be joined.
    assert!(by_subject.is_empty(), "joined by subject: {by_subject:#?}");
    assert_eq!(describe(&got), describe(&expected));
    assert_eq!(got, expected);

    // The initial sync goes from new to old. Then the duplicate (September) is seen before thread
    // message 3 (March) and keeps the reused id, so the replies that name it join its thread:
    // the first message with an id owns it. Everything else is the same.
    order.reverse();
    let (index, _) = thread_all(&order);
    let duplicate = messages
        .iter()
        .find(|(n, _)| n == "broken_duplicate-message-id.eml")
        .map(|(_, m)| m.message)
        .expect("duplicate in the seed");
    assert_eq!(index.thread_of(duplicate), index.thread_of(offsite[0]));
    let without = |groups: Vec<Vec<MessageId>>| -> Vec<Vec<MessageId>> {
        let mut groups: Vec<Vec<MessageId>> = groups
            .into_iter()
            .map(|g| {
                g.into_iter()
                    .filter(|m| *m != duplicate)
                    .collect::<Vec<_>>()
            })
            .filter(|g| !g.is_empty())
            .collect();
        groups.sort();
        groups
    };
    assert_eq!(without(index.partition()), without(expected));
}
