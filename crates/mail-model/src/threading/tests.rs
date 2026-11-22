//! Prepared threading cases: references (also arriving out of order, missing, broken, cyclic,
//! truncated, very long), Outlook replies without references, mailing lists, forwards, the
//! same subject without relation, reused Message-IDs, accounts, and the seed's long thread in
//! many orders.

use super::*;
use crate::{Address, Flags};

const ALICE: &str = "alice@vicemail.test";
const BOB: &str = "bob@vicemail.test";
const CAROL: &str = "carol@example.org";
const DAVE: &str = "dave@example.net";
const ERIK: &str = "erik@example.de";
const HOUR: i64 = 3600;
const DAY: i64 = 86_400;
/// 2026-03-02 08:15 UTC, the start of the seed's long thread.
const T0: i64 = 1_772_439_300;

fn msg(local: i64, id: &str, subject: &str, from: &str, to: &[&str], date: i64) -> ThreadMessage {
    ThreadMessage {
        message: MessageId(local),
        message_id: (!id.is_empty()).then(|| id.to_owned()),
        in_reply_to: Vec::new(),
        references: Vec::new(),
        subject: subject.to_owned(),
        participants: std::iter::once(from)
            .chain(to.iter().copied())
            .map(str::to_owned)
            .collect(),
        date,
        bulk: false,
    }
}

fn replying(mut m: ThreadMessage, in_reply_to: &[&str], references: &[&str]) -> ThreadMessage {
    m.in_reply_to = in_reply_to.iter().map(|s| (*s).to_owned()).collect();
    m.references = references.iter().map(|s| (*s).to_owned()).collect();
    m
}

fn index() -> ThreadIndex {
    ThreadIndex::new(ThreadConfig::new([ALICE]))
}

/// Inserts and returns the assignment that was applied.
fn add(index: &mut ThreadIndex, m: ThreadMessage) -> ThreadAssignment {
    let assignment = index.assign(&m);
    index.apply(m, &assignment);
    assignment
}

fn thread(index: &ThreadIndex, local: i64) -> ThreadId {
    index
        .thread_of(MessageId(local))
        .unwrap_or_else(|| panic!("message {local} not threaded"))
}

#[track_caller]
fn assert_together(index: &ThreadIndex, locals: &[i64]) {
    let first = thread(index, locals[0]);
    for local in locals {
        assert_eq!(thread(index, *local), first, "message {local}");
    }
}

#[track_caller]
fn assert_apart(index: &ThreadIndex, a: i64, b: i64) {
    assert_ne!(thread(index, a), thread(index, b), "{a} and {b}");
}

#[test]
fn replies_by_in_reply_to_and_references() {
    let mut ix = index();
    let s = "Angebot Projekt Phönix";
    let a = add(&mut ix, msg(1, "a@x", s, BOB, &[ALICE], T0));
    assert_eq!((a.target, a.reason), (ThreadTarget::New, ThreadReason::New));
    assert_eq!(a.link_ids, ["a@x"]);
    assert_eq!(a.subject_key.as_deref(), Some("angebot projekt phönix"));
    let b = add(
        &mut ix,
        replying(
            msg(2, "<b@x>", &format!("Re: {s}"), ALICE, &[BOB], T0 + HOUR),
            &["<a@x>"],
            &["<a@x>"],
        ),
    );
    assert_eq!(b.reason, ThreadReason::References);
    assert_eq!(b.link_ids, ["b@x", "a@x"]);
    // References only.
    add(
        &mut ix,
        replying(
            msg(3, "c@x", &format!("AW: {s}"), BOB, &[ALICE], T0 + 2 * HOUR),
            &[],
            &["a@x", "b@x"],
        ),
    );
    // In-Reply-To only (Outlook).
    add(
        &mut ix,
        replying(
            msg(
                4,
                "d@x",
                &format!("AW: AW: {s}"),
                ALICE,
                &[BOB],
                T0 + 3 * HOUR,
            ),
            &["c@x"],
            &[],
        ),
    );
    assert_together(&ix, &[1, 2, 3, 4]);
    assert_eq!(ix.len(), 1);
    assert_eq!(ix.messages_of(thread(&ix, 1)).len(), 4);
}

#[test]
fn reply_arriving_before_its_original() {
    let mut ix = index();
    let r = add(
        &mut ix,
        replying(
            msg(2, "r@x", "Re: Termin Dienstag", ALICE, &[BOB], T0 + DAY),
            &["o@x"],
            &["o@x"],
        ),
    );
    assert_eq!(r.target, ThreadTarget::New);
    let o = add(&mut ix, msg(1, "o@x", "Termin Dienstag", BOB, &[ALICE], T0));
    assert_eq!(o.reason, ThreadReason::References);
    assert_together(&ix, &[1, 2]);
}

#[test]
fn replies_to_a_missing_original_meet() {
    let mut ix = index();
    add(
        &mut ix,
        replying(
            msg(2, "r1@x", "Re: Umzug", CAROL, &[ALICE, BOB], T0),
            &["p@x"],
            &["p@x"],
        ),
    );
    add(
        &mut ix,
        replying(
            msg(3, "r2@x", "Re: Umzug", DAVE, &[ALICE, BOB], T0 + HOUR),
            &["p@x"],
            &["p@x"],
        ),
    );
    assert_together(&ix, &[2, 3]);
    add(
        &mut ix,
        msg(1, "p@x", "Umzug", BOB, &[ALICE, CAROL, DAVE], T0 - DAY),
    );
    assert_together(&ix, &[1, 2, 3]);
    assert_eq!(ix.len(), 1);
}

#[test]
fn a_missing_message_in_between_merges_two_threads() {
    let mut ix = index();
    add(&mut ix, msg(1, "a@x", "Vertrag", BOB, &[ALICE], T0));
    // C only references B (trimmed References), B is not there yet.
    add(
        &mut ix,
        replying(
            msg(3, "c@x", "Re: Vertrag", BOB, &[ALICE], T0 + 2 * DAY),
            &["b@x"],
            &["b@x"],
        ),
    );
    assert_apart(&ix, 1, 3);
    let (t1, t3) = (thread(&ix, 1), thread(&ix, 3));
    let b = add(
        &mut ix,
        replying(
            msg(2, "b@x", "Re: Vertrag", ALICE, &[BOB], T0 + DAY),
            &["a@x"],
            &["a@x"],
        ),
    );
    assert_eq!(b.target, ThreadTarget::Existing(t1));
    assert_eq!(b.absorbed, [t3]);
    assert_together(&ix, &[1, 2, 3]);
    assert_eq!(ix.len(), 1);

    // The survivor is the older thread id, whichever message started it.
    let mut ix = index();
    add(
        &mut ix,
        replying(
            msg(3, "c@x", "Re: Vertrag", BOB, &[ALICE], T0 + 2 * DAY),
            &["b@x"],
            &["b@x"],
        ),
    );
    add(&mut ix, msg(1, "a@x", "Vertrag", BOB, &[ALICE], T0));
    let b = add(
        &mut ix,
        replying(
            msg(2, "b@x", "Re: Vertrag", ALICE, &[BOB], T0 + DAY),
            &["a@x"],
            &["a@x"],
        ),
    );
    assert_eq!(b.target, ThreadTarget::Existing(ThreadId(1)));
    assert_eq!(b.absorbed, [ThreadId(2)]);
    // Later replies to any of them land in the merged thread.
    add(
        &mut ix,
        replying(
            msg(4, "d@x", "Re: Vertrag", ALICE, &[BOB], T0 + 3 * DAY),
            &["c@x"],
            &["c@x"],
        ),
    );
    assert_together(&ix, &[1, 2, 3, 4]);
}

#[test]
fn broken_references() {
    // Self-reference (seed: broken_reference-loop.eml).
    let mut ix = index();
    let looped = add(
        &mut ix,
        replying(
            msg(
                1,
                "<loop@seed>",
                "Re: References itself",
                DAVE,
                &[ALICE],
                T0,
            ),
            &["<loop@seed>"],
            &["<missing-parent@nowhere.example>", "<loop@seed>"],
        ),
    );
    assert_eq!(looped.target, ThreadTarget::New);
    assert_eq!(
        looped.link_ids,
        ["loop@seed", "missing-parent@nowhere.example"]
    );
    add(
        &mut ix,
        replying(
            msg(2, "x@x", "Re: References itself", ALICE, &[DAVE], T0 + HOUR),
            &["loop@seed"],
            &[],
        ),
    );
    add(
        &mut ix,
        msg(
            3,
            "missing-parent@nowhere.example",
            "References itself",
            DAVE,
            &[ALICE],
            T0 - HOUR,
        ),
    );
    assert_together(&ix, &[1, 2, 3]);

    // A cycle: A references B and B references A.
    let mut ix = index();
    add(
        &mut ix,
        replying(msg(1, "a@x", "Kreis", BOB, &[ALICE], T0), &[], &["b@x"]),
    );
    add(
        &mut ix,
        replying(
            msg(2, "b@x", "Re: Kreis", ALICE, &[BOB], T0 + HOUR),
            &["a@x"],
            &["a@x"],
        ),
    );
    add(
        &mut ix,
        replying(
            msg(3, "c@x", "Re: Kreis", BOB, &[ALICE], T0 + 2 * HOUR),
            &[],
            &["a@x", "b@x"],
        ),
    );
    assert_together(&ix, &[1, 2, 3]);
    assert_eq!(ix.len(), 1);

    // References in the wrong order.
    let mut ix = index();
    add(&mut ix, msg(1, "r@x", "Reihenfolge", BOB, &[ALICE], T0));
    add(
        &mut ix,
        replying(
            msg(2, "s@x", "Re: Reihenfolge", ALICE, &[BOB], T0 + HOUR),
            &["r@x"],
            &["r@x"],
        ),
    );
    add(
        &mut ix,
        replying(
            msg(3, "t@x", "Re: Reihenfolge", BOB, &[ALICE], T0 + 2 * HOUR),
            &[],
            &["s@x", "r@x"],
        ),
    );
    assert_together(&ix, &[1, 2, 3]);

    // Stray text, brackets, several ids in one entry, empty entries.
    let mut ix = index();
    add(
        &mut ix,
        msg(1, "root@x", "Unterlagen Steuer 2025", BOB, &[ALICE], T0),
    );
    let m = add(
        &mut ix,
        replying(
            msg(
                2,
                "m@x",
                "Re: Unterlagen Steuer 2025",
                ALICE,
                &[BOB],
                T0 + HOUR,
            ),
            &["Your message of Monday <root@x>"],
            &["", "   ", "<>", "no-at-sign", "<a@x><b@y>", "<b@y>, junk"],
        ),
    );
    assert_eq!(m.link_ids, ["m@x", "root@x", "b@y", "a@x"]);
    assert_together(&ix, &[1, 2]);

    // Unusable ids: too long, with spaces; no Message-ID at all.
    let long = format!("{}@x", "a".repeat(MAX_MESSAGE_ID_LEN));
    let mut ix = index();
    let m = add(
        &mut ix,
        replying(
            msg(1, &long, "Lang", BOB, &[ALICE], T0),
            &["<a b@x>"],
            &[&long],
        ),
    );
    assert!(m.link_ids.is_empty(), "{:?}", m.link_ids);
    let m = add(
        &mut ix,
        replying(msg(2, "", "Re: Lang", BOB, &[ALICE], T0), &["p@x"], &[]),
    );
    assert_eq!(m.link_ids, ["p@x"]);
}

#[test]
fn long_reference_chains_are_bounded() {
    let mut ix = index();
    add(&mut ix, msg(1, "root@x", "Kette", BOB, &[ALICE], T0));
    // 5000 unknown ids between the root and the parent.
    let mut references: Vec<String> = vec!["root@x".to_owned()];
    references.extend((0..5000).map(|i| format!("gone-{i}@x")));
    references.push("parent@x".to_owned());
    let refs: Vec<&str> = references.iter().map(String::as_str).collect();
    let m = add(
        &mut ix,
        replying(
            msg(2, "m@x", "Re: Kette", ALICE, &[BOB], T0 + DAY),
            &["parent@x"],
            &refs,
        ),
    );
    assert_eq!(m.reason, ThreadReason::References);
    assert!(m.link_ids.len() <= 1 + MAX_IN_REPLY_TO + REFERENCES_TAIL + REFERENCES_HEAD);
    assert_eq!(m.link_ids[..2], ["m@x", "parent@x"]);
    assert!(m.link_ids.contains(&"root@x".to_owned()));
    assert_together(&ix, &[1, 2]);

    // A chain of 200 replies, each with the full References list, trimmed every tenth time.
    let mut ix = index();
    let mut ids: Vec<String> = Vec::new();
    for i in 0..200 {
        let id = format!("c{i}@x");
        let mut refs: Vec<String> = ids.clone();
        if i % 10 == 0 && refs.len() > 11 {
            let tail = refs.split_off(refs.len() - 10);
            refs.truncate(1);
            refs.extend(tail);
        }
        let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
        let parent: Vec<&str> = ids.last().map(String::as_str).into_iter().collect();
        add(
            &mut ix,
            replying(
                msg(i, &id, "Re: Kette", BOB, &[ALICE], T0 + i * HOUR),
                &parent,
                &refs,
            ),
        );
        ids.push(id);
    }
    assert_eq!(ix.len(), 1);
}

/// An original from Bob and Alice's Outlook reply without any reference header.
fn outlook_pair(
    reply_from: &str,
    reply_to: &str,
    reply_date: i64,
) -> (ThreadMessage, ThreadMessage) {
    let s = "Angebot Projekt Phönix";
    (
        msg(1, "o@x", s, BOB, &[ALICE], T0),
        msg(
            2,
            "r@x",
            &format!("AW: {s}"),
            reply_from,
            &[reply_to],
            reply_date,
        ),
    )
}

#[test]
fn outlook_replies_without_references_thread_by_subject() {
    // Original first, then the reply; and the other way round.
    for reverse in [false, true] {
        let mut ix = index();
        let (o, r) = outlook_pair(ALICE, BOB, T0 + DAY);
        let (first, second) = if reverse { (r, o) } else { (o, r) };
        add(&mut ix, first);
        let second = add(&mut ix, second);
        assert_eq!(second.reason, ThreadReason::Subject, "reverse {reverse}");
        assert_together(&ix, &[1, 2]);
    }
    // A little clock skew is tolerated.
    let mut ix = index();
    let (o, r) = outlook_pair(ALICE, BOB, T0 - HOUR / 2);
    add(&mut ix, o);
    add(&mut ix, r);
    assert_together(&ix, &[1, 2]);
    // Several replies (also Bob answering Alice) join the same thread in any order.
    let s = "Angebot Projekt Phönix";
    let all = [
        msg(1, "o@x", s, BOB, &[ALICE], T0),
        msg(2, "r1@x", &format!("AW: {s}"), ALICE, &[BOB], T0 + DAY),
        msg(
            3,
            "r2@x",
            &format!("Re: AW: {s}"),
            BOB,
            &[ALICE, CAROL],
            T0 + 2 * DAY,
        ),
    ];
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [2, 0, 1]] {
        let mut ix = index();
        for i in order {
            add(&mut ix, all[i].clone());
        }
        assert_eq!(ix.len(), 1, "{order:?}");
    }
}

#[test]
fn subject_fallback_stays_careful() {
    let s = "Angebot Projekt Phönix";
    let cases: Vec<(&str, ThreadMessage, ThreadMessage)> = vec![
        (
            "more than 7 days apart",
            outlook_pair(ALICE, BOB, T0 + 8 * DAY).0,
            outlook_pair(ALICE, BOB, T0 + 8 * DAY).1,
        ),
        (
            "no shared participant",
            outlook_pair(ALICE, CAROL, T0 + DAY).0,
            outlook_pair(ALICE, CAROL, T0 + DAY).1,
        ),
        (
            "reply older than the original",
            outlook_pair(ALICE, BOB, T0 - 2 * DAY).0,
            outlook_pair(ALICE, BOB, T0 - 2 * DAY).1,
        ),
        (
            "two originals",
            msg(1, "o1@x", s, BOB, &[ALICE], T0),
            msg(2, "o2@x", s, BOB, &[ALICE], T0 + DAY),
        ),
        (
            "generic subject",
            msg(1, "o@x", "Hallo", BOB, &[ALICE], T0),
            msg(2, "r@x", "AW: Hallo", ALICE, &[BOB], T0 + DAY),
        ),
        (
            "single short word",
            msg(1, "o@x", "Projekt", BOB, &[ALICE], T0),
            msg(2, "r@x", "AW: Projekt", ALICE, &[BOB], T0 + DAY),
        ),
        (
            "only the own address in common",
            msg(1, "o@x", "Notiz Projekt Phönix", ALICE, &[ALICE], T0),
            msg(
                2,
                "r@x",
                "Re: Notiz Projekt Phönix",
                ALICE,
                &[ALICE],
                T0 + DAY,
            ),
        ),
        (
            "a forward is no reply",
            msg(1, "o@x", s, BOB, &[ALICE], T0),
            msg(2, "f@x", &format!("WG: {s}"), BOB, &[ALICE], T0 + DAY),
        ),
        (
            "unresolved references",
            msg(1, "o@x", s, BOB, &[ALICE], T0),
            replying(
                msg(2, "r@x", &format!("AW: {s}"), ALICE, &[BOB], T0 + DAY),
                &["other@x"],
                &[],
            ),
        ),
        (
            "subject differs after normalization",
            msg(1, "o@x", s, BOB, &[ALICE], T0),
            msg(
                2,
                "r@x",
                "AW: Angebot Projekt Phoenix",
                ALICE,
                &[BOB],
                T0 + DAY,
            ),
        ),
    ];
    for (name, first, second) in cases {
        for (a, b) in [(first.clone(), second.clone()), (second, first)] {
            let mut ix = index();
            add(&mut ix, a);
            let assignment = add(&mut ix, b);
            assert_eq!(assignment.target, ThreadTarget::New, "{name}");
            assert_eq!(ix.len(), 2, "{name}");
        }
    }
    // Bulk mail never threads by subject, on either side.
    for bulk_first in [false, true] {
        let mut ix = index();
        let (mut o, mut r) = outlook_pair(ALICE, BOB, T0 + DAY);
        if bulk_first {
            o.bulk = true;
        } else {
            r.bulk = true;
        }
        add(&mut ix, o);
        add(&mut ix, r);
        assert_eq!(ix.len(), 2);
    }
}

#[test]
fn closest_candidate_wins_and_threads_are_not_merged_by_subject() {
    let s = "Angebot Projekt Phönix";
    let mut ix = index();
    add(&mut ix, msg(1, "o1@x", s, BOB, &[ALICE], T0));
    add(&mut ix, msg(2, "o2@x", s, BOB, &[ALICE], T0 + 4 * DAY));
    assert_eq!(ix.len(), 2);
    let r = add(
        &mut ix,
        msg(3, "r@x", &format!("AW: {s}"), ALICE, &[BOB], T0 + 5 * DAY),
    );
    assert_eq!(r.target, ThreadTarget::Existing(thread(&ix, 2)));
    assert!(r.absorbed.is_empty());
    assert_eq!(ix.len(), 2);
}

#[test]
fn a_late_original_finds_the_reference_less_reply() {
    // A reply with references pins the original's thread before the original arrives; an
    // Outlook reply without references arrived even earlier.
    let s = "Angebot Projekt Phönix";
    let mut ix = index();
    add(
        &mut ix,
        msg(
            3,
            "outlook@x",
            &format!("AW: {s}"),
            DAVE,
            &[BOB],
            T0 + 3 * DAY,
        ),
    );
    add(
        &mut ix,
        replying(
            msg(2, "tb@x", &format!("Re: {s}"), ALICE, &[BOB], T0 + DAY),
            &["o@x"],
            &["o@x"],
        ),
    );
    assert_eq!(ix.len(), 2);
    let o = add(&mut ix, msg(1, "o@x", s, BOB, &[ALICE, DAVE], T0));
    assert_eq!(o.reason, ThreadReason::References);
    assert_eq!(o.absorbed.len(), 1);
    assert_together(&ix, &[1, 2, 3]);
}

#[test]
fn same_subject_without_relation_is_not_merged() {
    let a = "rechnung@firma-a.example";
    let b = "buchhaltung@firma-b.example";
    let mut ix = index();
    add(
        &mut ix,
        msg(1, "inv-a@firma-a", "Rechnung November", a, &[ALICE], T0),
    );
    add(
        &mut ix,
        msg(
            2,
            "inv-b@firma-b",
            "Rechnung November",
            b,
            &[ALICE],
            T0 + 2 * DAY,
        ),
    );
    assert_apart(&ix, 1, 2);
    // Alice answers company A from Outlook: it joins A, not B.
    let r = add(
        &mut ix,
        msg(3, "r@x", "AW: Rechnung November", ALICE, &[a], T0 + 3 * DAY),
    );
    assert_eq!(r.reason, ThreadReason::Subject);
    assert_together(&ix, &[1, 3]);
    assert_apart(&ix, 2, 3);
    // Company B answers its own invoice: joins B.
    add(
        &mut ix,
        msg(
            4,
            "r2@x",
            "Re: Rechnung November",
            b,
            &[ALICE],
            T0 + 3 * DAY,
        ),
    );
    assert_together(&ix, &[2, 4]);
    // The same company's invoice a month later and the bare word "Rechnung" stay apart.
    add(
        &mut ix,
        msg(
            5,
            "inv-a2@firma-a",
            "Rechnung November",
            a,
            &[ALICE],
            T0 + 31 * DAY,
        ),
    );
    add(
        &mut ix,
        msg(6, "x1@firma-a", "Rechnung", a, &[ALICE], T0 + 32 * DAY),
    );
    add(
        &mut ix,
        msg(7, "x2@x", "AW: Rechnung", ALICE, &[a], T0 + 32 * DAY + HOUR),
    );
    assert_eq!(ix.len(), 5);
}

#[test]
fn mailing_list_threads_stay_apart() {
    let list = "rust-users@lists.example";
    let members: Vec<String> = (0..12).map(|i| format!("member{i}@example.com")).collect();
    let cc: Vec<&str> = members.iter().map(String::as_str).collect();
    let post = |local: i64, id: &str, subject: &str, from: usize, date: i64| {
        let mut m = msg(
            local,
            id,
            subject,
            &members[from],
            &[&[list][..], &cc[..]].concat(),
            date,
        );
        m.bulk = true;
        m
    };
    let s = "[rust-users] Async traits are stable now";
    let mut ix = index();
    // Thread 1 with proper references.
    add(&mut ix, post(1, "l1@x", s, 0, T0));
    add(
        &mut ix,
        replying(
            post(2, "l2@x", &format!("Re: {s}"), 1, T0 + HOUR),
            &["l1@x"],
            &["l1@x"],
        ),
    );
    add(
        &mut ix,
        replying(
            post(3, "l3@x", &format!("Re: {s}"), 2, T0 + 2 * HOUR),
            &["l2@x"],
            &["l1@x", "l2@x"],
        ),
    );
    add(
        &mut ix,
        replying(
            post(
                4,
                "l4@x",
                "[rust-users] Re: Async traits are stable now",
                3,
                T0 + 3 * HOUR,
            ),
            &["l1@x"],
            &["l1@x"],
        ),
    );
    assert_together(&ix, &[1, 2, 3, 4]);
    // The same subject posted again two days later by someone else, and a reply to it without
    // references: no subject threading on lists.
    add(&mut ix, post(5, "l5@x", s, 4, T0 + 2 * DAY));
    add(
        &mut ix,
        post(6, "l6@x", &format!("Re: {s}"), 5, T0 + 2 * DAY + HOUR),
    );
    assert_apart(&ix, 1, 5);
    assert_apart(&ix, 1, 6);
    assert_apart(&ix, 5, 6);
    // A similar subject is another thread; its replies thread by references.
    let similar = "[rust-users] Async traits are stable now?";
    add(&mut ix, post(7, "l7@x", similar, 6, T0 + 3 * DAY));
    add(
        &mut ix,
        replying(
            post(8, "l8@x", &format!("Re: {similar}"), 7, T0 + 3 * DAY + HOUR),
            &["l7@x"],
            &["l7@x"],
        ),
    );
    assert_together(&ix, &[7, 8]);
    assert_apart(&ix, 1, 7);
    // A private reply off-list with references still joins thread 1.
    add(
        &mut ix,
        replying(
            msg(
                9,
                "p@x",
                &format!("Re: {s}"),
                ALICE,
                &[&members[0]],
                T0 + 4 * HOUR,
            ),
            &["l1@x"],
            &["l1@x"],
        ),
    );
    assert_together(&ix, &[1, 9]);
    assert_eq!(ix.len(), 4);
}

#[test]
fn forwards() {
    let s = "Planung Sommerfest";
    let mut ix = index();
    add(&mut ix, msg(1, "o@x", s, BOB, &[ALICE], T0));
    // Forward with References joins the conversation.
    add(
        &mut ix,
        replying(
            msg(2, "f1@x", &format!("Fwd: {s}"), ALICE, &[CAROL], T0 + HOUR),
            &[],
            &["o@x"],
        ),
    );
    assert_together(&ix, &[1, 2]);
    // Forward without references to someone new starts a conversation of its own, and so do
    // replies to it.
    add(
        &mut ix,
        msg(
            3,
            "f2@x",
            &format!("WG: {s}"),
            ALICE,
            &[DAVE],
            T0 + 2 * HOUR,
        ),
    );
    add(
        &mut ix,
        replying(
            msg(
                4,
                "r@x",
                &format!("Re: WG: {s}"),
                DAVE,
                &[ALICE],
                T0 + 3 * HOUR,
            ),
            &["f2@x"],
            &["f2@x"],
        ),
    );
    assert_apart(&ix, 1, 3);
    assert_together(&ix, &[3, 4]);
    // Forwarded as attachment (seed: mime_forward-rfc822.eml): a new message.
    add(
        &mut ix,
        msg(
            5,
            "mime.forward-rfc822@seed",
            "Fwd: Original message with attachment",
            CAROL,
            &[ALICE],
            T0,
        ),
    );
    assert_eq!(ix.len(), 3);
}

#[test]
fn reused_message_ids_do_not_merge_different_mail() {
    let mut ix = index();
    add(
        &mut ix,
        msg(1, "n@bulk", "Newsletter Mai", BOB, &[ALICE], T0),
    );
    let june = add(
        &mut ix,
        msg(2, "n@bulk", "Newsletter Juni", BOB, &[ALICE], T0 + 30 * DAY),
    );
    assert!(june.reused_message_id);
    assert_eq!(june.target, ThreadTarget::New);
    assert!(june.link_ids.is_empty());
    // A copy of the same message (another folder, list duplicate) joins it.
    let copy = add(
        &mut ix,
        msg(3, "n@bulk", "[list] Newsletter Mai", BOB, &[ALICE], T0),
    );
    assert!(!copy.reused_message_id);
    assert_together(&ix, &[1, 3]);
    assert_apart(&ix, 1, 2);
    // The seed's duplicate Message-ID (broken_duplicate-message-id.eml) next to thread message 3.
    let mut ix = index();
    let offsite = offsite_thread(100);
    for m in &offsite {
        add(&mut ix, m.clone());
    }
    let duplicate = add(
        &mut ix,
        msg(
            1,
            "<thread.offsite-03@seed.vicemail.test>",
            "Same Message-ID as thread message 3",
            DAVE,
            &[ALICE],
            T0 + 30 * DAY,
        ),
    );
    assert!(duplicate.reused_message_id);
    assert_eq!(ix.len(), 2);
}

#[test]
fn threads_never_cross_accounts() {
    // Each account has its own index (the store: `account_id = ?` in every lookup). The same
    // ids and subjects in another account do not link.
    let mut work = index();
    let mut private = ThreadIndex::new(ThreadConfig::new(["alice@privat.example"]));
    add(
        &mut work,
        msg(1, "o@x", "Angebot Projekt Phönix", BOB, &[ALICE], T0),
    );
    let reply = add(
        &mut private,
        replying(
            msg(
                2,
                "r@x",
                "AW: Angebot Projekt Phönix",
                "alice@privat.example",
                &[BOB],
                T0 + DAY,
            ),
            &["o@x"],
            &["o@x"],
        ),
    );
    assert_eq!(reply.target, ThreadTarget::New);
    let outlook = add(
        &mut private,
        msg(
            3,
            "r2@x",
            "AW: Angebot Projekt Phönix",
            BOB,
            &["alice@privat.example"],
            T0 + DAY,
        ),
    );
    // Joins the private account's own thread (by subject), never the work one.
    assert_eq!(outlook.reason, ThreadReason::Subject);
    assert_eq!((work.len(), private.len()), (1, 1));
}

/// The seed's 60-message thread (`testmail/seed.py`, `long_thread`): branches, message 12 with
/// References only, 22 with In-Reply-To only (Outlook), References trimmed to first + last 10
/// every tenth message, "AW:" from the German participant, a changed subject at 30.
fn offsite_thread(first_local: i64) -> Vec<ThreadMessage> {
    let people = [ALICE, BOB, CAROL, DAVE, ERIK];
    let subject = "Planning the 2026 offsite";
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    let mut out = Vec::new();
    for i in 0..60_usize {
        let sender = match i {
            0 => people[3],
            _ if i % 4 == 1 => people[0],
            _ => people[i % 5],
        };
        let parent = match i {
            0 => None,
            7 => Some(2),
            19 => Some(5),
            33 => Some(10),
            41 => Some(20),
            _ => Some(i - 1),
        };
        let refs: Vec<String> = parent
            .map(|p| {
                let (id, refs) = &entries[p];
                refs.iter().cloned().chain([id.clone()]).collect()
            })
            .unwrap_or_default();
        let subj = if i == 0 {
            subject.to_owned()
        } else if sender == people[4] {
            format!("AW: {subject}")
        } else if i == 30 {
            format!("Re: {subject} (was: venue shortlist)")
        } else {
            format!("Re: {subject}")
        };
        let id = format!("thread.offsite-{i:02}@seed.vicemail.test");
        let to: Vec<&str> = people[..3]
            .iter()
            .copied()
            .filter(|p| *p != sender)
            .collect();
        let n = i64::try_from(i).expect("small");
        let mut m = msg(first_local + n, &id, &subj, sender, &to, T0 + 7 * HOUR * n);
        if let Some(p) = parent
            && i != 12
        {
            m.in_reply_to = vec![entries[p].0.clone()];
        }
        if !refs.is_empty() && i != 22 {
            m.references = if i % 10 == 0 {
                let mut shown = vec![refs[0].clone()];
                shown.extend(refs[refs.len().saturating_sub(10)..].iter().cloned());
                shown.dedup();
                shown
            } else {
                refs.clone()
            };
        }
        entries.push((id, refs));
        out.push(m);
    }
    out
}

/// Fisher–Yates with xorshift64*.
fn shuffle<T>(items: &mut [T], mut seed: u64) {
    for i in (1..items.len()).rev() {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        let r = seed.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let j = usize::try_from(r % (i as u64 + 1)).expect("fits");
        items.swap(i, j);
    }
}

#[test]
fn the_long_seed_thread_in_any_order() {
    let mut corpus = offsite_thread(100);
    let s = "Planning the 2026 offsite";
    // An Outlook reply without references from inside the conversation: belongs to it.
    corpus.push(msg(
        200,
        "outlook@x",
        &format!("AW: {s}"),
        ERIK,
        &[ALICE, BOB],
        T0 + 3 * DAY,
    ));
    // Same subject, other people, no references: not part of it.
    corpus.push(msg(
        201,
        "frank@other",
        &format!("Re: {s}"),
        "frank@other.example",
        &["george@other.example"],
        T0 + 2 * DAY,
    ));
    // Same subject as a new original 40 days later.
    corpus.push(msg(202, "later@x", s, BOB, &[ALICE], T0 + 40 * DAY));
    // The Outlook triple and an unrelated pair, to mix the orders.
    corpus.push(msg(300, "o@x", "Angebot Projekt Phönix", BOB, &[ALICE], T0));
    corpus.push(msg(
        301,
        "r1@x",
        "AW: Angebot Projekt Phönix",
        ALICE,
        &[BOB],
        T0 + DAY,
    ));
    corpus.push(msg(
        302,
        "r2@x",
        "Re: AW: Angebot Projekt Phönix",
        BOB,
        &[ALICE],
        T0 + 2 * DAY,
    ));
    corpus.push(msg(
        400,
        "inv-a@a",
        "Rechnung November",
        "a@firma-a.example",
        &[ALICE],
        T0,
    ));
    corpus.push(msg(
        401,
        "inv-b@b",
        "Rechnung November",
        "b@firma-b.example",
        &[ALICE],
        T0 + DAY,
    ));

    let mut expected: Vec<Vec<MessageId>> = vec![
        (100..160).chain([200]).map(MessageId).collect(),
        vec![MessageId(201)],
        vec![MessageId(202)],
        vec![MessageId(300), MessageId(301), MessageId(302)],
        vec![MessageId(400)],
        vec![MessageId(401)],
    ];
    expected.sort();

    let mut orders = vec![corpus.clone()];
    let mut reversed = corpus.clone();
    reversed.reverse();
    orders.push(reversed);
    let mut by_date = corpus.clone();
    by_date.sort_by_key(|m| m.date);
    orders.push(by_date.clone());
    by_date.reverse();
    orders.push(by_date);
    for seed in 1..=40_u64 {
        let mut shuffled = corpus.clone();
        shuffle(&mut shuffled, seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        orders.push(shuffled);
    }
    for (n, order) in orders.into_iter().enumerate() {
        let mut ix = index();
        for m in order {
            ix.insert(m);
        }
        assert_eq!(ix.partition(), expected, "order {n}");
    }
}

#[test]
fn inserting_twice_changes_nothing() {
    let mut ix = index();
    let t = ix.insert(msg(1, "a@x", "Vertrag", BOB, &[ALICE], T0));
    assert_eq!(ix.insert(msg(1, "a@x", "Vertrag", BOB, &[ALICE], T0)), t);
    assert_eq!(ix.messages_of(t), [MessageId(1)]);
    assert!(!ix.is_empty());
}

#[test]
fn from_adapter_header_data() {
    let header = MessageHeader {
        uid: 7,
        modseq: None,
        flags: Flags::default(),
        size: 100,
        internal_date: T0,
        date: Some(T0 - HOUR),
        subject: "Re: [rust-users] Async".to_owned(),
        from: vec![Address {
            name: Some("Bob".to_owned()),
            email: BOB.to_owned(),
        }],
        to: vec![Address {
            name: None,
            email: "rust-users@lists.example".to_owned(),
        }],
        cc: vec![Address {
            name: None,
            email: CAROL.to_owned(),
        }],
        message_id: Some("m@x".to_owned()),
        in_reply_to: Some("p@x".to_owned()),
        references: vec!["r@x".to_owned(), "p@x".to_owned()],
        list_id: Some("Rust users <rust-users.lists.example>".to_owned()),
        list_unsubscribe: None,
        auto_submitted: None,
        precedence: None,
        has_attachments: false,
    };
    let m = ThreadMessage::from_header(MessageId(T0), &header);
    assert_eq!(m.participants, [BOB, "rust-users@lists.example", CAROL]);
    assert_eq!(m.in_reply_to, ["p@x"]);
    assert_eq!(m.date, T0 - HOUR);
    assert!(m.bulk);
}

#[test]
fn bulk_markers() {
    assert!(!is_bulk(None, None, None, None));
    assert!(is_bulk(Some("<x.example>"), None, None, None));
    assert!(!is_bulk(Some("  "), None, None, None));
    assert!(is_bulk(None, Some("<https://x/unsub>"), None, None));
    for p in ["bulk", "List", " junk "] {
        assert!(is_bulk(None, None, Some(p), None), "{p}");
    }
    assert!(!is_bulk(None, None, Some("first-class"), None));
    assert!(is_bulk(None, None, None, Some("auto-replied")));
    assert!(is_bulk(None, None, None, Some("auto-generated")));
    assert!(!is_bulk(None, None, None, Some("no")));
    assert!(!is_bulk(None, None, None, Some("No; reason")));
    assert!(is_bulk(None, None, None, Some("notified")));
}
