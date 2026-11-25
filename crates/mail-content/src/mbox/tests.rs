//! Tests of the mbox reader and writer: round trips of the writer's mboxrd, Thunderbird's mboxo
//! with its status fields (the fixture profile in `testmail/thunderbird/`), LF and CRLF files,
//! leading garbage, messages over the limit and hostile input.

use std::time::Instant;

use super::*;

/// Reads a whole file the way the engine does: line by line with the line endings.
fn read_all(data: &[u8]) -> (Vec<MboxEntry>, u64) {
    read_with(MboxReader::new(), data)
}

fn read_with(mut reader: MboxReader, data: &[u8]) -> (Vec<MboxEntry>, u64) {
    let mut entries = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |i| i + 1);
        let (line, next) = rest.split_at(end);
        entries.extend(reader.push_line(line));
        rest = next;
    }
    entries.extend(reader.finish());
    (entries, reader.lines_before_first())
}

fn messages(entries: &[MboxEntry]) -> Vec<&[u8]> {
    entries
        .iter()
        .filter_map(|e| match e {
            MboxEntry::Message { bytes, .. } => Some(bytes.as_slice()),
            MboxEntry::TooLarge { .. } => None,
        })
        .collect()
}

fn mbox_of(raws: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for (n, raw) in raws.iter().enumerate() {
        let date = 1_763_805_600 + i64::try_from(n).expect("small") * 60;
        write_mbox_message(&mut out, Some("alice@vicemail.test"), date, raw);
    }
    out
}

const PLAIN: &[u8] = b"Message-ID: <a@x>\r\nFrom: a@x\r\nSubject: one\r\n\r\nHello.\r\n";

#[test]
fn written_messages_come_back_byte_for_byte() {
    let quoting: &[u8] = b"Subject: quoting\r\n\r\nFrom here on, a line that starts with From.\r\n\
        >From quoted once\r\n>>From quoted twice\r\n\r\nFrom x@y Sat Nov 22 10:00:00 2025\r\nend\r\n";
    let empty_body: &[u8] = b"Subject: empty\r\n\r\n";
    let header_only: &[u8] = b"Subject: no body at all\r\n";
    let trailing_blank: &[u8] = b"Subject: blank\r\n\r\nbody\r\n\r\n\r\n";
    let binary: &[u8] = b"Subject: 8bit\r\nContent-Transfer-Encoding: 8bit\r\n\r\n\xe4\xf6\xfc \x00\x01\xff\r\nFrom\xff \r\n";
    let raws = [
        PLAIN,
        quoting,
        empty_body,
        header_only,
        trailing_blank,
        binary,
    ];
    let file = mbox_of(&raws);
    // Every quoted line in the file has one more `>`, so no body line looks like a separator.
    let text = String::from_utf8_lossy(&file);
    assert!(text.contains("\n>From here on"));
    assert!(text.contains("\n>>From quoted once\n"));
    assert!(text.contains("\n>>>From quoted twice\n"));
    assert!(text.contains("\n>From x@y Sat Nov 22"));
    assert!(!file.contains(&b'\r'), "LF line endings in the file");
    let (entries, before) = read_all(&file);
    assert_eq!(before, 0);
    assert_eq!(messages(&entries), raws.to_vec());
    for entry in &entries {
        assert!(matches!(entry, MboxEntry::Message { mozilla: None, .. }));
    }
}

#[test]
fn a_message_without_a_final_line_break_gets_one() {
    let file = mbox_of(&[b"Subject: x\r\n\r\nlast line", PLAIN]);
    assert!(file.starts_with(b"From alice@vicemail.test "));
    let (entries, _) = read_all(&file);
    assert_eq!(
        messages(&entries),
        vec![&b"Subject: x\r\n\r\nlast line\r\n"[..], PLAIN]
    );
}

#[test]
fn an_empty_message_is_written_and_read_as_empty() {
    let mut file = Vec::new();
    write_mbox_message(&mut file, None, 0, b"");
    assert_eq!(file, b"From MAILER-DAEMON Thu Jan  1 00:00:00 1970\n\n");
    let (entries, _) = read_all(&file);
    assert_eq!(messages(&entries), vec![&b""[..]]);
}

#[test]
fn separator_lines_have_a_sender_and_an_asctime_date() {
    assert_eq!(
        mbox_from_line(Some("alice@vicemail.test"), 1_763_805_600),
        "From alice@vicemail.test Sat Nov 22 10:00:00 2025\n"
    );
    // Days are padded with a space; dates before 1970 or after 9999 become the epoch.
    assert_eq!(
        mbox_from_line(None, 1_762_041_600),
        "From MAILER-DAEMON Sun Nov  2 00:00:00 2025\n"
    );
    assert_eq!(
        mbox_from_line(None, -1),
        "From MAILER-DAEMON Thu Jan  1 00:00:00 1970\n"
    );
    assert_eq!(
        mbox_from_line(None, i64::MAX),
        "From MAILER-DAEMON Thu Jan  1 00:00:00 1970\n"
    );
    assert_eq!(
        mbox_from_line(None, 951_782_400),
        "From MAILER-DAEMON Tue Feb 29 00:00:00 2000\n"
    );
    // Unusable senders: empty, whitespace, control or direction characters, too long.
    for bad in ["", "a b@x", "a\tb", "a\nb", "a\u{202e}b", &"x".repeat(300)] {
        assert!(
            mbox_from_line(Some(bad), 0).starts_with("From MAILER-DAEMON "),
            "{bad:?}"
        );
    }
    assert!(is_mbox_separator(
        mbox_from_line(Some("x@y"), 1_763_805_600).as_bytes()
    ));
}

#[test]
fn separators_need_their_shape() {
    for line in [
        &b"From - Sat Nov 22 10:00:00 2025"[..],
        b"From - Sat Nov 22 10:00:00 2025\n",
        b"From - Sat Nov 22 10:00:00 2025\r\n",
        b"From x@y Sat Nov 22 10:00:00 2025",
        b"From MAILER-DAEMON Sat Nov  2 10:00:00 2025 +0100",
        b"From x@y Sat, 22 Nov 2025",
        b"From x@y 9:05",
    ] {
        assert!(is_mbox_separator(line), "{}", String::from_utf8_lossy(line));
    }
    for line in [
        &b"From here on we go"[..],
        b"From  Sat Nov 22 10:00:00 2025",
        b"From x@y",
        b"From x@y 12345 99:99",
        b"From x@y 3025",
        b"From: alice@x Sat Nov 22 10:00:00 2025",
        b">From - Sat Nov 22 10:00:00 2025",
        b"from - Sat Nov 22 10:00:00 2025",
        b"",
    ] {
        assert!(
            !is_mbox_separator(line),
            "{}",
            String::from_utf8_lossy(line)
        );
    }
}

#[test]
fn thunderbird_local_folders_report_their_status_without_the_fields() {
    let inbox = include_bytes!(
        "../../../../testmail/thunderbird/Profiles/t3stpr0f.default-release/Mail/Local Folders/Inbox"
    );
    let (entries, before) = read_all(inbox);
    assert_eq!(before, 0);
    assert_eq!(entries.len(), 3);
    let parsed: Vec<(String, Option<MozillaStatus>)> = entries
        .iter()
        .map(|e| match e {
            MboxEntry::Message { bytes, mozilla } => {
                (String::from_utf8(bytes.clone()).expect("utf-8"), *mozilla)
            }
            MboxEntry::TooLarge { .. } => panic!("no message is too large"),
        })
        .collect();
    for (text, _) in &parsed {
        assert!(!text.to_ascii_lowercase().contains("x-mozilla"), "{text}");
        assert!(text.starts_with("Message-ID: <tb-"), "{text}");
        assert!(text.ends_with("\r\n"), "{text:?}");
        assert!(
            !text.ends_with("\r\n\r\n"),
            "the separator's blank line: {text:?}"
        );
        assert!(!text.replace("\r\n", "").contains('\n'), "CRLF only");
    }
    assert!(parsed[0].0.contains("Subject: TB fixture 1\r\n"));
    assert_eq!(
        parsed[0].1,
        Some(MozillaStatus {
            read: true,
            replied: false,
            flagged: true,
            deleted: false,
        })
    );
    // mboxo quoting undone.
    assert!(
        parsed[1]
            .0
            .contains("\r\nFrom the archive, as it was written.\r\n")
    );
    assert_eq!(parsed[1].1, Some(MozillaStatus::default()));
    // Deleted (0x0009: read and expunged): reported, the caller skips it.
    assert!(parsed[2].0.contains("Subject: TB fixture 3\r\n"));
    assert_eq!(
        parsed[2].1,
        Some(MozillaStatus {
            read: true,
            deleted: true,
            ..MozillaStatus::default()
        })
    );
    let live: Vec<_> = parsed
        .iter()
        .filter(|(_, s)| !s.is_some_and(|s| s.deleted))
        .collect();
    assert_eq!(live.len(), 2);

    let archive = include_bytes!(
        "../../../../testmail/thunderbird/Profiles/t3stpr0f.default-release/Mail/Local Folders/Archives.sbd/2025"
    );
    let (entries, _) = read_all(archive);
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        &entries[1],
        MboxEntry::Message {
            mozilla: Some(MozillaStatus {
                read: true,
                replied: true,
                flagged: false,
                deleted: false
            }),
            ..
        }
    ));
}

#[test]
fn mozilla_fields_go_with_their_continuation_lines_and_only_in_the_header() {
    let file = b"From - Sat Nov 22 10:00:00 2025\n\
        X-Mozilla-Status: 0004\n\
        x-mozilla-keys: $label1\n\
        \t$label2\n\
        Subject: kept\n\
        \tfolded\n\
        X-Mozilla-Status2: 00800000\n\
        \n\
        X-Mozilla-Status: 0001 is body text here\n";
    let (entries, _) = read_all(file);
    assert_eq!(
        entries,
        vec![MboxEntry::Message {
            bytes: b"Subject: kept\r\n\tfolded\r\n\r\nX-Mozilla-Status: 0001 is body text here\r\n"
                .to_vec(),
            mozilla: Some(MozillaStatus {
                flagged: true,
                ..MozillaStatus::default()
            }),
        }]
    );
    // A status that is no hex number is dropped like the field, but reports nothing.
    let (entries, _) = read_all(b"From - Sat Nov 22 10:00:00 2025\nX-Mozilla-Status: zz\n\nx\n");
    assert_eq!(
        entries,
        vec![MboxEntry::Message {
            bytes: b"\r\nx\r\n".to_vec(),
            mozilla: None,
        }]
    );
}

#[test]
fn lf_and_crlf_files_read_the_same() {
    let lf = b"From a@x Sat Nov 22 10:00:00 2025\nSubject: 1\n\nbody\n\nFrom b@x Sat Nov 22 10:01:00 2025\nSubject: 2\n\nsecond\n";
    let crlf: Vec<u8> = String::from_utf8_lossy(lf)
        .replace('\n', "\r\n")
        .into_bytes();
    let (from_lf, _) = read_all(lf);
    let (from_crlf, _) = read_all(&crlf);
    assert_eq!(from_lf, from_crlf);
    assert_eq!(
        messages(&from_lf),
        vec![
            &b"Subject: 1\r\n\r\nbody\r\n"[..],
            &b"Subject: 2\r\n\r\nsecond\r\n"[..]
        ]
    );
}

#[test]
fn body_lines_that_look_like_separators_need_an_empty_line_before() {
    // mboxo from a writer that did not quote: only after an empty line does a dated `From `
    // line start a message; an undated one stays in the body.
    let file = b"From a@x Sat Nov 22 10:00:00 2025\nSubject: 1\n\nline\nFrom b@x Sat Nov 22 10:00:00 2025\n\nFrom here on, body.\n\nFrom c@x Sat Nov 22 10:02:00 2025\nSubject: 3\n\nthree\n";
    let (entries, _) = read_all(file);
    assert_eq!(
        messages(&entries),
        vec![
            &b"Subject: 1\r\n\r\nline\r\nFrom b@x Sat Nov 22 10:00:00 2025\r\n\r\nFrom here on, body.\r\n"[..],
            &b"Subject: 3\r\n\r\nthree\r\n"[..],
        ]
    );
}

#[test]
fn leading_garbage_is_counted_and_files_without_messages_give_none() {
    let file = b"garbage line\nmore\n\nFrom a@x Sat Nov 22 10:00:00 2025\nSubject: 1\n\nx\n";
    let (entries, before) = read_all(file);
    assert_eq!(before, 3);
    assert_eq!(messages(&entries), vec![&b"Subject: 1\r\n\r\nx\r\n"[..]]);
    // Before the first message no empty line is needed.
    let (entries, before) = read_all(b"junk\nFrom a@x Sat Nov 22 10:00:00 2025\n\nx\n");
    assert_eq!(before, 1);
    assert_eq!(entries.len(), 1);

    let (entries, before) = read_all(b"Subject: an eml file\r\n\r\nNot an mbox.\r\n");
    assert!(entries.is_empty());
    assert_eq!(before, 3);
    let (entries, before) = read_all(b"");
    assert!(entries.is_empty());
    assert_eq!(before, 0);
    let mut reader = MboxReader::default();
    assert_eq!(reader.finish(), None);
    assert_eq!(reader.finish(), None);
}

#[test]
fn messages_over_the_limit_are_counted_and_dropped() {
    let big = format!("Subject: big\r\n\r\n{}\r\n", "x".repeat(200));
    let file = mbox_of(&[PLAIN, big.as_bytes(), PLAIN]);
    let (entries, _) = read_with(MboxReader::with_limit(100), &file);
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[1],
        MboxEntry::TooLarge {
            bytes: u64::try_from(big.len()).expect("small"),
        }
    );
    assert_eq!(messages(&entries), vec![PLAIN, PLAIN]);
    // Exactly the limit still fits.
    let (entries, _) = read_with(MboxReader::with_limit(PLAIN.len()), &mbox_of(&[PLAIN]));
    assert_eq!(messages(&entries), vec![PLAIN]);
    let (entries, _) = read_with(MboxReader::with_limit(PLAIN.len() - 1), &mbox_of(&[PLAIN]));
    assert!(matches!(entries[..], [MboxEntry::TooLarge { .. }]));
}

#[test]
fn many_tiny_messages_and_hostile_input_stay_linear() {
    let mut file = Vec::new();
    for n in 0..20_000 {
        write_mbox_message(&mut file, Some("x@y"), n, b"S: x\r\n\r\nb\r\n");
    }
    let start = Instant::now();
    let (entries, _) = read_all(&file);
    assert_eq!(entries.len(), 20_000);
    // One long line of `>` and `From` fragments, a very long separator-like line, and NUL bytes.
    let mut hostile = b"From a@x Sat Nov 22 10:00:00 2025\n".to_vec();
    hostile.extend(std::iter::repeat_n(b'>', 2_000_000));
    hostile.extend_from_slice(b"From \n\n");
    hostile.extend_from_slice(b"From ");
    hostile.extend(std::iter::repeat_n(b'9', 2_000_000));
    hostile.extend_from_slice(b" 2025\n");
    hostile.extend(std::iter::repeat_n(0u8, 100_000));
    let (entries, _) = read_all(&hostile);
    assert_eq!(entries.len(), 2);
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}
