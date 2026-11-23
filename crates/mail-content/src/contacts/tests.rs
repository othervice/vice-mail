//! Tests of the contact directory: what is added, names, ranking, search, duplicates, merging.

use mail_model::{Address, MessageCategory};

use super::*;
use crate::parse::MessageHeaders;

const DAY: i64 = 86_400;
const NOW: i64 = 1_795_000_000;

fn book() -> ContactBook {
    ContactBook::new(&[
        "alice@vicemail.test".to_owned(),
        "Alice@Work.Example".to_owned(),
    ])
}

fn sent(book: &mut ContactBook, email: &str, name: Option<&str>, at: i64) {
    book.observe(Observation {
        email,
        name,
        direction: Direction::Sent,
        at,
    })
    .expect("added");
}

fn received(book: &mut ContactBook, email: &str, name: Option<&str>, at: i64) {
    book.observe(Observation {
        email,
        name,
        direction: Direction::Received,
        at,
    })
    .expect("added");
}

fn keys(matches: &[ContactMatch<'_>]) -> Vec<String> {
    matches
        .iter()
        .map(|m| m.address.address.key.clone())
        .collect()
}

#[test]
fn own_no_reply_and_invalid_addresses_are_left_out() {
    let mut b = book();
    let observe = |b: &mut ContactBook, email: &str| {
        b.observe(Observation {
            email,
            name: None,
            direction: Direction::Received,
            at: NOW,
        })
    };
    assert_eq!(
        observe(&mut b, "ALICE@vicemail.test"),
        Err(Skipped::OwnAddress)
    );
    assert_eq!(
        observe(&mut b, "alice+lists@vicemail.test"),
        Err(Skipped::OwnAddress)
    );
    assert_eq!(
        observe(&mut b, "alice@work.example"),
        Err(Skipped::OwnAddress)
    );
    assert_eq!(
        observe(&mut b, "noreply@dhl.example"),
        Err(Skipped::NoReply)
    );
    assert_eq!(
        observe(&mut b, "nicht-antworten@kasse.example"),
        Err(Skipped::NoReply)
    );
    assert_eq!(
        observe(&mut b, "MAILER-DAEMON@x.example"),
        Err(Skipped::NoReply)
    );
    assert_eq!(
        observe(&mut b, "undisclosed-recipients:;"),
        Err(Skipped::InvalidAddress)
    );
    assert_eq!(observe(&mut b, ""), Err(Skipped::InvalidAddress));
    assert!(b.contacts().is_empty());
}

#[test]
fn addresses_are_normalised_but_plus_addresses_stay_apart() {
    let mut b = book();
    sent(&mut b, "Hans.Mueller@Firma.EXAMPLE", None, NOW - 2 * DAY);
    received(&mut b, "hans.mueller@firma.example", None, NOW - DAY);
    sent(&mut b, "jürgen@müller.example", None, NOW);
    received(&mut b, "JÜRGEN@xn--mller-kva.example", None, NOW);
    sent(&mut b, "carol+shop@example.org", None, NOW);
    sent(&mut b, "carol@example.org", None, NOW);
    assert_eq!(b.contacts().len(), 4, "{:#?}", b.contacts());
    let hans = b.find("HANS.MUELLER@firma.example").expect("hans");
    assert_eq!((hans.primary().sent, hans.primary().received), (1, 1));
    // The newest spelling is written, the key is lower case.
    assert_eq!(hans.primary().address.email(), "hans.mueller@firma.example");
    assert_eq!(hans.last_used(), Some(NOW - DAY));
    let juergen = b.find("jürgen@müller.example").expect("jürgen");
    assert_eq!(juergen.primary().address.display(), "jürgen@müller.example");
    assert_eq!(
        juergen.primary().address.email(),
        "JÜRGEN@xn--mller-kva.example"
    );
    assert!(b.find("carol+shop@example.org").is_some());
    assert_ne!(
        b.find("carol+shop@example.org").map(Contact::key),
        b.find("carol@example.org").map(Contact::key),
        "never merged without the user"
    );
}

#[test]
fn the_most_frequent_name_wins_and_addresses_are_no_names() {
    let mut b = book();
    let email = "hans.mueller@firma.example";
    received(&mut b, email, Some("Hans Müller"), NOW - 3 * DAY);
    received(&mut b, email, Some("\"Hans Müller\""), NOW - 2 * DAY);
    sent(&mut b, email, Some("Müller, Hans"), NOW - DAY);
    sent(&mut b, email, Some("hans.mueller@firma.example"), NOW);
    sent(&mut b, email, Some("hans.mueller"), NOW);
    let hans = b.find(email).expect("hans");
    assert_eq!(hans.name(), Some("Hans Müller"));
    assert_eq!(hans.names.len(), 2, "{:?}", hans.names);
    // Equal counts: the newer one.
    let mut b = book();
    received(&mut b, email, Some("H. Müller"), NOW - DAY);
    received(&mut b, email, Some("Hans Müller"), NOW);
    assert_eq!(b.find(email).and_then(Contact::name), Some("Hans Müller"));
    // The user's name wins; clearing it returns to the seen names.
    b.set_name(email, Some("  Hansi  Müller ")).expect("set");
    assert_eq!(b.find(email).and_then(Contact::name), Some("Hansi Müller"));
    assert_eq!(
        keys(&b.search("hansi", 5)),
        [email],
        "the search sees the new name"
    );
    assert_eq!(
        b.set_name(email, Some("x@y.example")),
        Err(ContactError::InvalidName)
    );
    assert_eq!(
        b.set_name(email, Some("   ")),
        Err(ContactError::InvalidName)
    );
    b.set_name(email, None).expect("clear");
    assert_eq!(b.find(email).and_then(Contact::name), Some("Hans Müller"));
    assert!(b.search("hansi", 5).is_empty());
    assert_eq!(
        b.set_name("nobody@x.example", None),
        Err(ContactError::NotFound("nobody@x.example".into()))
    );
}

#[test]
fn ranking_by_frequency_and_recency_sent_over_received() {
    let mut b = book();
    // Weekly colleague over the last year.
    for week in 0..52 {
        sent(
            &mut b,
            "colleague@firma.example",
            None,
            NOW - 3 * DAY - week * 7 * DAY,
        );
    }
    // One message yesterday.
    sent(&mut b, "new@firma.example", None, NOW - DAY);
    // Received from three times in the last days, never written to.
    for day in 0..3 {
        received(&mut b, "chatty@firma.example", None, NOW - day * DAY);
    }
    // Written to very often, but two years ago.
    for i in 0..50 {
        sent(
            &mut b,
            "former@firma.example",
            None,
            NOW - 730 * DAY - i * DAY,
        );
    }
    let order = keys(&b.search("firma", 10));
    assert_eq!(
        order,
        [
            "colleague@firma.example",
            "new@firma.example",
            "chatty@firma.example",
            "former@firma.example"
        ]
    );
    // Four received weigh one sent.
    let mut b = book();
    for _ in 0..4 {
        received(&mut b, "a@x.example", None, NOW);
    }
    sent(&mut b, "b@x.example", None, NOW);
    let a = b.find("a@x.example").expect("a").rank();
    let one = b.find("b@x.example").expect("b").rank();
    assert!((a.score_at(NOW) - one.score_at(NOW)).abs() < 1e-9);
}

#[test]
fn prefix_search_in_names_and_addresses() {
    let mut b = book();
    sent(
        &mut b,
        "hans.mueller@firma.example",
        Some("Hans Müller"),
        NOW,
    );
    sent(&mut b, "zoe@uni.example", Some("Zoë Ångström"), NOW - DAY);
    sent(
        &mut b,
        "erika@müller.example",
        Some("Erika Mustermann"),
        NOW - 2 * DAY,
    );
    sent(
        &mut b,
        "l.w@example.net",
        Some("Łukasz Wiśniewski"),
        NOW - 3 * DAY,
    );
    received(
        &mut b,
        "anna@example.org",
        Some("Анна Иванова"),
        NOW - 4 * DAY,
    );
    let find = |q: &str| keys(&b.search(q, 10));
    assert_eq!(find("hans"), ["hans.mueller@firma.example"]);
    assert_eq!(
        find("MÜL"),
        ["hans.mueller@firma.example", "erika@xn--mller-kva.example"]
    );
    assert_eq!(
        find("mul"),
        ["hans.mueller@firma.example", "erika@xn--mller-kva.example"]
    );
    assert_eq!(
        find("mue"),
        ["hans.mueller@firma.example", "erika@xn--mller-kva.example"]
    );
    assert_eq!(find("zoe ang"), ["zoe@uni.example"]);
    assert_eq!(find("angstr"), ["zoe@uni.example"]);
    assert_eq!(find("lukasz"), ["l.w@example.net"]);
    assert_eq!(find("wisn"), ["l.w@example.net"]);
    assert_eq!(find("ива"), ["anna@example.org"]);
    assert_eq!(
        find("hans.mu"),
        ["hans.mueller@firma.example"],
        "address prefix"
    );
    assert_eq!(find("uni.ex"), ["zoe@uni.example"], "domain prefix");
    assert_eq!(find("xn--mller"), ["erika@xn--mller-kva.example"]);
    // Prefix, not substring; every word must match.
    assert!(find("ller").is_empty());
    assert!(find("hans erika").is_empty());
    assert!(find("   ").is_empty());
    assert_eq!(b.search("e", 2).len(), 2, "limit");
}

#[test]
fn search_returns_the_address_that_matched() {
    let mut b = book();
    sent(&mut b, "hans@firma.example", Some("Hans Müller"), NOW);
    sent(
        &mut b,
        "hans.privat@mail.example",
        Some("Hans Müller"),
        NOW - DAY,
    );
    b.merge(&ContactMerge {
        keep: "hans@firma.example".into(),
        absorb: "hans.privat@mail.example".into(),
    })
    .expect("merge");
    assert_eq!(keys(&b.search("hans", 5)), ["hans@firma.example"]);
    assert_eq!(
        keys(&b.search("hans priv", 5)),
        ["hans.privat@mail.example"]
    );
    assert_eq!(keys(&b.search("mail.ex", 5)), ["hans.privat@mail.example"]);
}

#[test]
fn duplicates_are_suggested_never_merged() {
    let mut b = book();
    sent(&mut b, "carol@example.org", Some("Carol Clark"), NOW);
    sent(&mut b, "carol+shop@example.org", None, NOW);
    sent(
        &mut b,
        "hans.mueller@firma.example",
        Some("Hans Müller"),
        NOW,
    );
    received(&mut b, "hans@privat.example", Some("Müller, Hans"), NOW);
    received(
        &mut b,
        "h.mueller@verein.example",
        Some("Hans P. Mueller"),
        NOW,
    );
    sent(&mut b, "Max.Muster@gmail.com", None, NOW);
    sent(&mut b, "maxmuster@googlemail.com", None, NOW);
    // One address, two different names.
    received(&mut b, "office@firma.example", Some("Sekretariat"), NOW);
    received(
        &mut b,
        "office@firma.example",
        Some("Erika Mustermann"),
        NOW - DAY,
    );
    received(
        &mut b,
        "office@firma.example",
        Some("ERIKA MUSTERMANN"),
        NOW - DAY,
    );
    // Single-word names are too common to suggest.
    sent(&mut b, "info@a.example", Some("Info"), NOW);
    sent(&mut b, "info@b.example", Some("Info"), NOW);
    let before = b.contacts().len();
    let suggestions = b.duplicates(&[]);
    let kinds: Vec<(String, Vec<String>)> = suggestions
        .iter()
        .map(|s| {
            let kind = match &s.kind {
                DuplicateKind::PlusAddress => "plus".to_owned(),
                DuplicateKind::SameMailbox => "mailbox".to_owned(),
                DuplicateKind::SameName { .. } => "name".to_owned(),
                DuplicateKind::NamesDiffer { names } => format!("names {names:?}"),
            };
            (kind, s.contacts.clone())
        })
        .collect();
    let pair = |a: &str, b: &str| vec![a.to_owned(), b.to_owned()];
    assert_eq!(
        kinds,
        [
            (
                "plus".to_owned(),
                pair("carol+shop@example.org", "carol@example.org")
            ),
            (
                "mailbox".to_owned(),
                pair("max.muster@gmail.com", "maxmuster@googlemail.com")
            ),
            (
                "name".to_owned(),
                pair("h.mueller@verein.example", "hans.mueller@firma.example")
            ),
            (
                "name".to_owned(),
                pair("h.mueller@verein.example", "hans@privat.example")
            ),
            (
                "name".to_owned(),
                pair("hans.mueller@firma.example", "hans@privat.example")
            ),
            (
                "names [\"Erika Mustermann\", \"Sekretariat\"]".to_owned(),
                vec!["office@firma.example".to_owned()]
            ),
        ]
    );
    assert_eq!(b.contacts().len(), before, "suggesting merges nothing");
    // Dismissed pairs (either order) are not suggested again.
    let dismissed = [
        (
            "carol@example.org".to_owned(),
            "carol+shop@example.org".to_owned(),
        ),
        (
            "office@firma.example".to_owned(),
            "office@firma.example".to_owned(),
        ),
    ];
    let left = b.duplicates(&dismissed);
    assert_eq!(left.len(), 4);
    assert!(left.iter().all(|s| !matches!(
        s.kind,
        DuplicateKind::PlusAddress | DuplicateKind::NamesDiffer { .. }
    )));
    // Choosing a name ends the name suggestion.
    b.set_name("office@firma.example", Some("Sekretariat Firma"))
        .expect("name");
    assert!(
        b.duplicates(&[])
            .iter()
            .all(|s| !matches!(s.kind, DuplicateKind::NamesDiffer { .. }))
    );
}

#[test]
fn merging_combines_everything_on_confirmation() {
    let mut b = book();
    sent(&mut b, "carol@example.org", Some("Carol Clark"), NOW - DAY);
    sent(&mut b, "carol+shop@example.org", Some("Carol"), NOW);
    received(&mut b, "carol+shop@example.org", Some("Carol Clark"), NOW);
    let rank_before = b
        .find("carol@example.org")
        .expect("c")
        .rank()
        .plus(b.find("carol+shop@example.org").expect("c").rank());
    b.merge(&ContactMerge {
        keep: "carol@example.org".into(),
        absorb: "CAROL+shop@example.org".into(),
    })
    .expect("merge");
    assert_eq!(b.contacts().len(), 1);
    let carol = b.find("carol+shop@example.org").expect("merged");
    assert_eq!(
        carol.key(),
        "carol@example.org",
        "kept the preferred address"
    );
    assert_eq!(carol.sent(), 2);
    assert_eq!(carol.name(), Some("Carol Clark"), "2 of 3 messages");
    assert!((carol.rank().key() - rank_before.key()).abs() < 1e-9);
    assert_eq!(
        b.merge(&ContactMerge {
            keep: "carol@example.org".into(),
            absorb: "carol+shop@example.org".into(),
        }),
        Err(ContactError::SameContact)
    );
    assert_eq!(
        b.merge(&ContactMerge {
            keep: "carol@example.org".into(),
            absorb: "nobody@example.org".into(),
        }),
        Err(ContactError::NotFound("nobody@example.org".into()))
    );
    // New messages to either address go to the merged contact.
    sent(&mut b, "carol+shop@example.org", None, NOW);
    assert_eq!(b.contacts().len(), 1);
    b.set_preferred("carol+shop@example.org")
        .expect("preferred");
    assert_eq!(
        b.contacts()[0].key(),
        "carol+shop@example.org",
        "preferred address moved first"
    );
}

#[test]
fn observations_of_a_message() {
    let address = |email: &str, name: Option<&str>| Address {
        name: name.map(str::to_owned),
        email: email.to_owned(),
    };
    let headers = MessageHeaders {
        from: vec![address("carol@example.org", Some("Carol"))],
        to: vec![
            address("alice@vicemail.test", None),
            address("dave@example.net", Some("Dave")),
        ],
        cc: vec![address("erin@example.net", None)],
        bcc: vec![address("frank@example.net", None)],
        ..MessageHeaders::default()
    };
    let sent: Vec<&str> = observations(&headers, Direction::Sent, MessageCategory::Person, NOW)
        .iter()
        .map(|o| o.email)
        .collect();
    assert_eq!(
        sent,
        [
            "alice@vicemail.test",
            "dave@example.net",
            "erin@example.net",
            "frank@example.net"
        ]
    );
    let received = observations(&headers, Direction::Received, MessageCategory::Person, NOW);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].name, Some("Carol"));
    for bulk in [MessageCategory::Newsletter, MessageCategory::Notification] {
        assert!(observations(&headers, Direction::Received, bulk, NOW).is_empty());
    }
    // Fed into a book, the own address drops out.
    let mut b = book();
    for o in observations(&headers, Direction::Sent, MessageCategory::Person, NOW) {
        let _ = b.observe(o);
    }
    assert_eq!(b.contacts().len(), 3);
}

#[test]
fn stored_contacts_are_loaded_without_duplicate_addresses() {
    let mut b = book();
    sent(&mut b, "a@x.example", Some("A A"), NOW);
    sent(&mut b, "b@x.example", Some("B B"), NOW);
    let mut stored = b.into_contacts();
    let mut copy = stored[0].clone();
    copy.addresses.push(stored[0].addresses[0].clone());
    stored.push(copy);
    stored.push(Contact {
        addresses: Vec::new(),
        names: Vec::new(),
        user_name: None,
    });
    let b = ContactBook::from_contacts(&[], stored);
    assert_eq!(b.contacts().len(), 2);
    assert_eq!(b.find("a@x.example").and_then(Contact::name), Some("A A"));
    let words = b.find("a@x.example").expect("a").search_words();
    assert!(words.contains(&"a@x.example".to_owned()), "{words:?}");
}
