use super::*;

/// A tiny JPEG start (enough for `sniff_image`) followed by bytes of every value.
fn jpeg(len: usize) -> Vec<u8> {
    let mut bytes = b"\xFF\xD8\xFF\xE0".to_vec();
    bytes.extend((0..len).map(|i| u8::try_from(i % 251).expect("byte")));
    bytes
}

fn person(name: &str, emails: &[&str]) -> VcardCard {
    VcardCard {
        formatted_name: Some(name.to_owned()),
        emails: emails.iter().map(|e| (*e).to_owned()).collect(),
        ..VcardCard::default()
    }
}

fn kinds(file: &VcardFile) -> Vec<VcardProblemKind> {
    file.problems.iter().map(|p| p.kind).collect()
}

#[test]
fn individuals_and_groups_survive_a_round_trip_in_both_versions() {
    let cards = vec![
        VcardCard {
            uid: Some("0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01".into()),
            kind: VcardKind::Individual,
            formatted_name: Some("Müller, Jürgen; Dr. \\ Sohn".into()),
            emails: vec![
                "juergen@example.org".into(),
                "j.mueller@work.example".into(),
            ],
            note: Some("Line one,\nline two; with \\ backslash\n\tand a tab".into()),
            members: Vec::new(),
        },
        VcardCard {
            uid: Some("plain-uid-42".into()),
            formatted_name: Some(
                // Long enough to fold, with multi-byte characters across the fold points.
                "Ελένη Παπαδοπούλου-Χατζηγεωργίου 東京都千代田区 Ünïcödé 😀😀😀😀😀😀😀".into(),
            ),
            emails: vec!["eleni@example.gr".into()],
            ..VcardCard::default()
        },
        VcardCard {
            uid: Some("7e5d8c1a-0000-4000-8000-00000000abcd".into()),
            kind: VcardKind::Group,
            formatted_name: Some("Team, Berlin".into()),
            emails: Vec::new(),
            note: None,
            members: vec![
                VcardMember::Uid("0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01".into()),
                VcardMember::Email("outside@example.net".into()),
            ],
        },
    ];
    for version in [VcardVersion::V3, VcardVersion::V4] {
        let text = write_vcards(&cards, version);
        for line in text.split("\r\n") {
            assert!(
                line.len() <= 75,
                "{version:?}: line over 75 octets: {line:?}"
            );
        }
        assert!(!text.contains("\n\n"), "CRLF only");
        let read = read_vcards(text.as_bytes());
        assert_eq!(read.problems, Vec::new(), "{version:?}");
        assert_eq!(read.cards, cards, "{version:?}\n{text}");
    }
}

#[test]
fn written_cards_follow_each_version() {
    let mut group = person("Friends", &[]);
    group.kind = VcardKind::Group;
    group.members = vec![VcardMember::Email("a@example.org".into())];
    let alice = person("Alice Liddell", &["alice@example.org", "a2@example.org"]);
    let v4 = write_vcards(&[alice.clone(), group.clone()], VcardVersion::V4);
    assert!(v4.starts_with("BEGIN:VCARD\r\nVERSION:4.0\r\n"), "{v4}");
    assert!(v4.contains("N:Liddell;Alice;;;\r\n"), "{v4}");
    assert!(v4.contains("EMAIL;PREF=1:alice@example.org\r\nEMAIL:a2@example.org\r\n"));
    assert!(!v4.contains("PHOTO"), "no pictures (decision 0072): {v4}");
    assert!(v4.contains("KIND:group\r\n"));
    assert!(v4.contains("MEMBER:mailto:a@example.org\r\n"));
    let v3 = write_vcards(&[alice, group], VcardVersion::V3);
    assert!(v3.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\n"), "{v3}");
    assert!(v3.contains("EMAIL;TYPE=INTERNET,pref:alice@example.org\r\n"));
    assert!(v3.contains("EMAIL;TYPE=INTERNET:a2@example.org\r\n"));
    assert!(!v3.contains("PHOTO"), "no pictures (decision 0072): {v3}");
    assert!(v3.contains("X-ADDRESSBOOKSERVER-KIND:group\r\nFN:Friends\r\nN:Friends;;;;\r\n"));
    assert!(v3.contains("X-ADDRESSBOOKSERVER-MEMBER:mailto:a@example.org\r\n"));
    assert!(v3.ends_with("END:VCARD\r\n"));
}

#[test]
fn every_card_gets_a_name() {
    let only_email = VcardCard {
        emails: vec!["x@example.org".into()],
        ..VcardCard::default()
    };
    let only_uid = VcardCard {
        uid: Some("u-1".into()),
        kind: VcardKind::Group,
        ..VcardCard::default()
    };
    let text = write_vcards(
        &[only_email, only_uid, VcardCard::default()],
        VcardVersion::V4,
    );
    assert!(text.contains("FN:x@example.org\r\n"));
    assert!(text.contains("FN:u-1\r\n"));
    assert!(text.contains("FN:Unnamed\r\n"));
    assert!(text.contains("UID:u-1\r\n"));
}

#[test]
fn apple_contacts_3_0() {
    let file = concat!(
        "BEGIN:VCARD\r\n",
        "VERSION:3.0\r\n",
        "PRODID:-//Apple Inc.//macOS 26.1//EN\r\n",
        "N:Appleseed;Johnny;;;\r\n",
        "FN:Johnny Appleseed\r\n",
        "ORG:Apple Inc.;\r\n",
        "item1.EMAIL;type=INTERNET;type=HOME:johnny@home.example\r\n",
        "item2.EMAIL;type=INTERNET;type=WORK;type=pref:johnny@apple.example\r\n",
        "item2.X-ABLabel:_$!<Work>!$_\r\n",
        "TEL;type=CELL;type=VOICE;type=pref:+1 555 0100\r\n",
        "NOTE:Met at WWDC\\, 2026\r\n",
        "PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBk\r\n",
        " SEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgN\r\n",
        "UID:A1B2C3D4-0000-4000-8000-000000000001\r\n",
        "X-ABUID:A1B2C3D4-0000-4000-8000-000000000001:ABPerson\r\n",
        "END:VCARD\r\n",
        "BEGIN:VCARD\r\n",
        "VERSION:3.0\r\n",
        "PRODID:-//Apple Inc.//macOS 26.1//EN\r\n",
        "N:Garden club;;;;\r\n",
        "FN:Garden club\r\n",
        "X-ADDRESSBOOKSERVER-KIND:group\r\n",
        "X-ADDRESSBOOKSERVER-MEMBER:urn:uuid:A1B2C3D4-0000-4000-8000-000000000001\r\n",
        "X-ADDRESSBOOKSERVER-MEMBER:urn:uuid:A1B2C3D4-0000-4000-8000-000000000002\r\n",
        "UID:F00D-group\r\n",
        "END:VCARD\r\n",
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.problems, Vec::new());
    assert_eq!(read.cards.len(), 2);
    let johnny = &read.cards[0];
    assert_eq!(johnny.formatted_name.as_deref(), Some("Johnny Appleseed"));
    assert_eq!(
        johnny.emails,
        vec!["johnny@apple.example", "johnny@home.example"],
        "the preferred address first"
    );
    assert_eq!(johnny.note.as_deref(), Some("Met at WWDC, 2026"));
    assert_eq!(
        johnny.uid.as_deref(),
        Some("A1B2C3D4-0000-4000-8000-000000000001")
    );
    let club = &read.cards[1];
    assert_eq!(club.kind, VcardKind::Group);
    assert_eq!(club.formatted_name.as_deref(), Some("Garden club"));
    assert_eq!(
        club.members,
        vec![
            VcardMember::Uid("A1B2C3D4-0000-4000-8000-000000000001".into()),
            VcardMember::Uid("A1B2C3D4-0000-4000-8000-000000000002".into()),
        ]
    );
}

#[test]
fn google_contacts_3_0() {
    let file = concat!(
        "BEGIN:VCARD\n",
        "VERSION:3.0\n",
        "FN:Grace Hopper\n",
        "N:Hopper;Grace;;;\n",
        "EMAIL;TYPE=INTERNET;TYPE=WORK:grace@navy.example\n",
        "EMAIL;TYPE=INTERNET:GRACE@NAVY.EXAMPLE\n",
        "EMAIL;TYPE=INTERNET;TYPE=HOME:grace@home.example\n",
        "PHOTO:https://lh3.googleusercontent.com/contacts/abc\n",
        "CATEGORIES:myContacts\n",
        "END:VCARD\n",
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.cards.len(), 1);
    let grace = &read.cards[0];
    assert_eq!(grace.formatted_name.as_deref(), Some("Grace Hopper"));
    assert_eq!(
        grace.emails,
        vec!["grace@navy.example", "grace@home.example"],
        "each address once, ignoring case"
    );
    // A photo is skipped like any property the app does not read (a URL is never fetched).
    assert_eq!(read.problems, Vec::new());
}

#[test]
fn thunderbird_4_0() {
    let file = concat!(
        "BEGIN:VCARD\r\n",
        "VERSION:4.0\r\n",
        "N:Lovelace;Ada;;;\r\n",
        "FN:Ada Lovelace\r\n",
        "EMAIL;PREF=2:ada@second.example\r\n",
        "EMAIL:ada@third.example\r\n",
        "EMAIL;PREF=1:ada@first.example\r\n",
        "NOTE:Analytical engine\\nNotes\r\n",
        "UID:urn:uuid:5b1f6f3a-4a40-4b38-9c1c-6f5f2b9d1e01\r\n",
        "PHOTO:data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD\r\n",
        "END:VCARD\r\n",
        "BEGIN:VCARD\r\n",
        "VERSION:4.0\r\n",
        "KIND:group\r\n",
        "FN:Engines\r\n",
        "MEMBER:urn:uuid:5b1f6f3a-4a40-4b38-9c1c-6f5f2b9d1e01\r\n",
        "MEMBER:mailto:charles@babbage.example\r\n",
        "MEMBER:mailto:CHARLES@babbage.example\r\n",
        "END:VCARD\r\n",
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.problems, Vec::new());
    let ada = &read.cards[0];
    assert_eq!(
        ada.emails,
        vec![
            "ada@first.example",
            "ada@second.example",
            "ada@third.example"
        ]
    );
    assert_eq!(ada.note.as_deref(), Some("Analytical engine\nNotes"));
    assert_eq!(
        ada.uid.as_deref(),
        Some("5b1f6f3a-4a40-4b38-9c1c-6f5f2b9d1e01")
    );
    let engines = &read.cards[1];
    assert_eq!(engines.kind, VcardKind::Group);
    assert_eq!(
        engines.members,
        vec![
            VcardMember::Uid("5b1f6f3a-4a40-4b38-9c1c-6f5f2b9d1e01".into()),
            VcardMember::Email("charles@babbage.example".into()),
        ]
    );
}

#[test]
fn outlook_2_1_with_quoted_printable_and_windows_1252() {
    let mut file = Vec::new();
    file.extend_from_slice(b"BEGIN:VCARD\r\nVERSION:2.1\r\n");
    file.extend_from_slice(b"N;CHARSET=Windows-1252:M\xfcller;J\xfcrgen\r\n");
    // No FN: the name comes from N.
    file.extend_from_slice(b"EMAIL;PREF;INTERNET:juergen@example.de\r\n");
    file.extend_from_slice(b"EMAIL;INTERNET:jm@example.de\r\n");
    file.extend_from_slice(
        b"NOTE;CHARSET=Windows-1252;ENCODING=QUOTED-PRINTABLE:Gr=FC=DFe aus K=F6ln,=0D=0A=\r\n",
    );
    file.extend_from_slice(b"zweite Zeile mit =80 und =\r\nEnde\r\n");
    file.extend_from_slice(b"PHOTO;JPEG;ENCODING=BASE64:\r\n");
    file.extend_from_slice(b"    /9j/4AAQSkZJRgABAQAAAQABAAD\r\n");
    file.extend_from_slice(b"    /2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBk\r\n\r\n");
    file.extend_from_slice(b"END:VCARD\r\n");
    let read = read_vcards(&file);
    assert_eq!(read.problems, Vec::new());
    assert_eq!(read.cards.len(), 1);
    let card = &read.cards[0];
    assert_eq!(card.formatted_name.as_deref(), Some("Jürgen Müller"));
    assert_eq!(card.emails, vec!["juergen@example.de", "jm@example.de"]);
    assert_eq!(
        card.note.as_deref(),
        Some("Grüße aus Köln,\nzweite Zeile mit € und Ende")
    );
}

#[test]
fn bytes_that_are_not_utf8_are_read_as_windows_1252() {
    let file = b"BEGIN:VCARD\nVERSION:3.0\nFN:Fran\xe7ois \x80\nEMAIL:f@example.fr\nEND:VCARD\n";
    let read = read_vcards(file);
    assert_eq!(read.cards[0].formatted_name.as_deref(), Some("François €"));
}

#[test]
fn folding_inside_a_character_and_a_byte_order_mark() {
    // "Zoë" with the two bytes of "ë" on two physical lines.
    let file = b"\xEF\xBB\xBFBEGIN:VCARD\r\nVERSION:4.0\r\nFN:Zo\xc3\r\n \xab Example\r\nEMAIL:z@example.org\r\nEND:VCARD\r\n";
    let read = read_vcards(file);
    assert_eq!(read.problems, Vec::new());
    assert_eq!(read.cards[0].formatted_name.as_deref(), Some("Zoë Example"));
}

#[test]
fn quoted_and_listed_parameters() {
    let file = concat!(
        "BEGIN:VCARD\n",
        "VERSION:4.0\n",
        "FN:Q\n",
        "EMAIL;TYPE=\"work,home\":second@example.org\n",
        "EMAIL;TYPE=\"work,pref\":first@example.org\n",
        "EMAIL;TYPE=INTERNET,pref:also-first@example.org\n",
        "EMAIL;LABEL=\"a:b;c\":third@example.org\n",
        "END:VCARD\n",
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(
        read.cards[0].emails,
        vec![
            "first@example.org",
            "also-first@example.org",
            "second@example.org",
            "third@example.org"
        ]
    );
}

#[test]
fn names_and_notes_are_cleaned() {
    let file = "BEGIN:VCARD\nVERSION:4.0\nFN:  Evil\u{202E}fdp.exe \\n  Name  \nNOTE:a\u{2066}b\u{7}c\\nd\nEMAIL:mailto:e@example.org\nEND:VCARD\n";
    let read = read_vcards(file.as_bytes());
    let card = &read.cards[0];
    assert_eq!(card.formatted_name.as_deref(), Some("Evilfdp.exe Name"));
    assert_eq!(card.note.as_deref(), Some("abc\nd"));
    assert_eq!(card.emails, vec!["e@example.org"]);

    let long = format!(
        "BEGIN:VCARD\nVERSION:4.0\nFN:{}\nNOTE:{}\nEMAIL:x@example.org\nEND:VCARD\n",
        "n".repeat(500),
        "m".repeat(5_000)
    );
    let read = read_vcards(long.as_bytes());
    let card = &read.cards[0];
    assert_eq!(
        card.formatted_name.as_ref().map(|n| n.chars().count()),
        Some(crate::MAX_NAME_CHARS)
    );
    assert_eq!(
        card.note.as_ref().map(|n| n.chars().count()),
        Some(mail_model::MAX_CONTACT_NOTE_CHARS)
    );
}

#[test]
fn cards_without_name_address_or_members_are_dropped() {
    let file = concat!(
        "BEGIN:VCARD\nVERSION:3.0\nTEL:+1 555 0100\nEND:VCARD\n",
        "BEGIN:VCARD\nVERSION:3.0\nEMAIL:not-an-address\nEND:VCARD\n",
        "BEGIN:VCARD\nVERSION:3.0\nEMAIL:only@example.org\nEND:VCARD\n",
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.problems, Vec::new());
    assert_eq!(read.cards, vec![person_without_name("only@example.org")]);
}

fn person_without_name(email: &str) -> VcardCard {
    VcardCard {
        emails: vec![email.to_owned()],
        ..VcardCard::default()
    }
}

#[test]
fn a_card_without_end_is_read_and_reported() {
    let file = "BEGIN:VCARD\nVERSION:3.0\nFN:Cut off\nEMAIL:cut@example.org\n";
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.cards, vec![person("Cut off", &["cut@example.org"])]);
    assert_eq!(kinds(&read), vec![VcardProblemKind::Malformed]);
}

#[test]
fn nested_cards_are_skipped() {
    let mut file = String::from("BEGIN:VCARD\nVERSION:2.1\nFN:Outer\nEMAIL:outer@example.org\n");
    for _ in 0..50_000 {
        file.push_str("AGENT:BEGIN:VCARD\nBEGIN:VCARD\nFN:Inner\nEMAIL:inner@example.org\n");
    }
    for _ in 0..50_000 {
        file.push_str("END:VCARD\n");
    }
    file.push_str("NOTE:after the agents\nEND:VCARD\n");
    file.push_str("BEGIN:VCARD\nFN:Next\nEND:VCARD\n");
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.problems, Vec::new());
    let mut outer = person("Outer", &["outer@example.org"]);
    outer.note = Some("after the agents".into());
    assert_eq!(read.cards, vec![outer, person("Next", &[])]);
}

#[test]
fn garbage_and_binary_input_give_nothing() {
    assert_eq!(read_vcards(b""), VcardFile::default());
    assert_eq!(
        read_vcards(b"just some text\nwith: colons\n"),
        VcardFile::default()
    );
    let binary: Vec<u8> = (0..200_000u32)
        .map(|i| u8::try_from(i.wrapping_mul(2_654_435_761) >> 24).expect("byte"))
        .collect();
    let read = read_vcards(&binary);
    assert!(read.cards.is_empty());
    let mut half = b"BEGIN:VCARD\nFN:".to_vec();
    half.extend_from_slice(&binary);
    let read = read_vcards(&half);
    assert!(read.problems.len() <= 32);
}

#[test]
fn too_many_cards_stop_at_the_limit() {
    let mut file = String::new();
    for i in 0..=MAX_VCARD_CARDS {
        file.push_str(&format!(
            "BEGIN:VCARD\nVERSION:3.0\nFN:P{i}\nEMAIL:p{i}@example.org\nEND:VCARD\n"
        ));
    }
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.cards.len(), MAX_VCARD_CARDS);
    assert_eq!(
        read.problems,
        vec![VcardProblem {
            card: MAX_VCARD_CARDS,
            kind: VcardProblemKind::TooManyCards
        }]
    );
}

#[test]
fn a_line_over_the_limit_is_skipped() {
    let file = format!(
        "BEGIN:VCARD\nVERSION:3.0\nFN:Long\nNOTE:{}\nEMAIL:long@example.org\nEND:VCARD\n",
        "x".repeat(MAX_VCARD_LINE_BYTES + 10)
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.cards, vec![person("Long", &["long@example.org"])]);
    assert_eq!(kinds(&read), vec![VcardProblemKind::Malformed]);

    // A folded line over the limit: every continuation is consumed with it.
    let mut folded = String::from("BEGIN:VCARD\nFN:Folded\nNOTE:");
    for _ in 0..(MAX_VCARD_LINE_BYTES / 70 + 10) {
        folded.push_str(&"y".repeat(70));
        folded.push_str("\n ");
    }
    folded.push_str("end\nEMAIL:folded@example.org\nEND:VCARD\n");
    let read = read_vcards(folded.as_bytes());
    assert_eq!(read.cards, vec![person("Folded", &["folded@example.org"])]);
    assert_eq!(kinds(&read), vec![VcardProblemKind::Malformed]);
}

#[test]
fn a_file_over_the_limit_is_cut() {
    let card = "BEGIN:VCARD\nVERSION:3.0\nFN:Filler\nEMAIL:f@example.org\nNOTE:".to_owned()
        + &"z".repeat(2_000)
        + "\nEND:VCARD\n";
    let count = MAX_VCARD_FILE_BYTES / card.len() + 2;
    let file = card.repeat(count);
    let read = read_vcards(file.as_bytes());
    assert!(read.cards.len() < count);
    assert!(read.cards.len() >= count - 3);
    let last = read.problems.last().expect("a problem");
    assert_eq!(last.kind, VcardProblemKind::TooLarge);
}

#[test]
fn photos_are_skipped_without_a_problem() {
    // Stage 22 (decision 0072): contacts have no pictures. A photo, inline of 4 MiB (the line
    // still fits), a link, not an image or not even base64, is neither decoded nor reported;
    // the rest of the card is read.
    let mut big = String::new();
    crate::image::base64_encode_into(&jpeg(4 * 1024 * 1024), &mut big);
    let file = format!(
        "BEGIN:VCARD\nVERSION:3.0\nFN:Big\nPHOTO;ENCODING=b;TYPE=JPEG:{big}\nEMAIL:big@example.org\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:4.0\nFN:Svg\nPHOTO:data:image/svg+xml;base64,PHN2Zy8+\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:4.0\nFN:Broken\nPHOTO;ENCODING=b:not*base64\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:4.0\nFN:Link\nPHOTO:https://tracker.example/p.png\nEND:VCARD\n"
    );
    let read = read_vcards(file.as_bytes());
    assert_eq!(read.problems, Vec::new());
    assert_eq!(read.cards.len(), 4);
    assert_eq!(read.cards[0].emails, vec!["big@example.org"]);
    let written = write_vcards(&read.cards, VcardVersion::V4);
    assert!(!written.contains("PHOTO"), "{written}");
}

#[test]
fn many_addresses_and_members_are_capped() {
    let mut file = String::from("BEGIN:VCARD\nVERSION:4.0\nKIND:group\nFN:Crowd\n");
    for i in 0..5_000 {
        file.push_str(&format!("EMAIL:e{i}@example.org\n"));
    }
    file.push_str("EMAIL;PREF=1:last-but-preferred@example.org\n");
    for i in 0..5_000 {
        file.push_str(&format!("MEMBER:mailto:m{i}@example.org\n"));
    }
    file.push_str("END:VCARD\n");
    let read = read_vcards(file.as_bytes());
    let card = &read.cards[0];
    assert_eq!(card.emails.len(), MAX_VCARD_EMAILS);
    assert_eq!(card.emails[0], "e0@example.org", "beyond the candidates");
    assert_eq!(card.members.len(), MAX_VCARD_MEMBERS);
}

#[test]
fn the_problem_list_is_bounded() {
    // Since photos are skipped (decision 0072), no file yields a problem per card cheaply; the
    // bound is the reader's.
    let mut reader = Reader::default();
    for card in 0..100 {
        reader.problem(card, VcardProblemKind::Malformed);
    }
    assert_eq!(reader.problems.len(), MAX_PROBLEMS);
}
