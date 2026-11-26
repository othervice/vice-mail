//! Table tests for the search language: every filter, aliases, combinations, malformed input,
//! the FTS5 expression and the structured filters, and a fuzz-style run against panics and
//! FTS5 syntax injection.

use super::*;
use crate::MailboxRole;

const UTC: FixedOffset = FixedOffset(0);
/// 2026-11-22 12:00:00 UTC.
const NOW: i64 = 1_795_348_800;
const DAY: i64 = 86_400;
/// 2026-11-22 00:00:00 UTC.
const TODAY: i64 = NOW - DAY / 2;

fn ctx() -> SearchContext<'static> {
    SearchContext {
        now: NOW,
        time_zone: &UTC,
        slash_dates: SlashDateOrder::DayFirst,
    }
}

fn day(year: i32, month: u8, day: u8) -> i64 {
    CivilDate::new(year, month, day).expect("valid").start(&UTC)
}

fn describe_term(term: &Term) -> String {
    let mut out = String::new();
    if term.negated {
        out.push('-');
    }
    let date = |d: &CivilDate| format!("{:04}-{:02}-{:02}", d.year, d.month, d.day);
    match &term.filter {
        Filter::Text(text) => {
            out.push_str(match text.field {
                TextField::Any => "",
                TextField::Subject => "subject:",
                TextField::From => "from:",
                TextField::To => "to:",
                TextField::Cc => "cc:",
                TextField::Filename => "filename:",
            });
            if text.quoted {
                out.push_str(&format!("\"{}\"", text.text));
            } else {
                out.push_str(&text.text);
            }
            if text.prefix {
                out.push('*');
            }
        }
        Filter::In(spec) => {
            out.push_str(&format!("in:{}", spec.name));
            if let Some(role) = spec.role {
                out.push_str(&format!("({role:?})"));
            }
        }
        Filter::Account(account) => out.push_str(&format!("account:{account}")),
        Filter::HasAttachment => out.push_str("has:attachment"),
        Filter::Is(status) => out.push_str(&format!("is:{status:?}")),
        Filter::Before(d) => out.push_str(&format!("before:{}", date(d))),
        Filter::After(d) => out.push_str(&format!("after:{}", date(d))),
        Filter::OlderThan(age) => out.push_str(&format!("older_than:{}{:?}", age.amount, age.unit)),
        Filter::NewerThan(age) => out.push_str(&format!("newer_than:{}{:?}", age.amount, age.unit)),
        Filter::Larger(bytes) => out.push_str(&format!("larger:{bytes}")),
        Filter::Contact(value) => out.push_str(&format!("contact:{value}")),
    }
    out
}

fn describe(query: &ParsedQuery) -> String {
    query
        .clauses
        .iter()
        .map(|clause| match clause {
            Clause::Term(term) => describe_term(term),
            Clause::AnyOf(terms) => format!(
                "({})",
                terms
                    .iter()
                    .map(describe_term)
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn describe_errors(input: &str, query: &ParsedQuery) -> String {
    query
        .errors
        .iter()
        .map(|e| format!("{:?}:{}", e.kind, &input[e.span.start..e.span.end]))
        .collect::<Vec<_>>()
        .join(", ")
}

/// (input, terms, errors as `Kind:text`).
const PARSE_CASES: &[(&str, &str, &str)] = &[
    // Free text, phrases, prefixes.
    ("", "", ""),
    ("   \t ", "", ""),
    ("rechnung", "rechnung", ""),
    ("Rechnung November", "Rechnung November", ""),
    ("Grüße aus Köln", "Grüße aus Köln", ""),
    ("\"vielen dank\"", "\"vielen dank\"", ""),
    ("\"vielen dank\"*", "\"vielen dank\"*", ""),
    ("rech*", "rech*", ""),
    ("rech**", "rech*", ""),
    ("a*b", "a*b", ""),
    ("re*", "re", "PrefixTooShort:re*"),
    (
        "\"vielen da\"*",
        "\"vielen da\"",
        "PrefixTooShort:\"vielen da\"*",
    ),
    ("e-mail", "e-mail", ""),
    ("alice@example.com", "alice@example.com", ""),
    ("foo\"bar baz\"", "foo \"bar baz\"", ""),
    ("tab\there\u{0}nul", "tab here nul", ""),
    ("\"ctl\u{7}x\"", "\"ctl x\"", ""),
    // Things that look like filters but are text.
    ("10:30", "10:30", ""),
    ("Re: Angebot", "Re: Angebot", ""),
    ("AW: WG: Angebot", "AW: WG: Angebot", ""),
    ("https://example.com/x", "https://example.com/x", ""),
    ("form:bob", "form:bob", "UnknownFilter:form"),
    ("-foo:bar", "-foo:bar", "UnknownFilter:foo"),
    ("foo:\"bar baz\"", "foo:\"bar baz\"", "UnknownFilter:foo"),
    ("x1:y", "x1:y", ""),
    ("{from}:x", "{from}:x", ""),
    ("NEAR(a b)", "NEAR(a b)", ""),
    // Exclusion.
    ("-spam", "-spam", ""),
    ("-\"kein spam\"", "-\"kein spam\"", ""),
    ("-from:werbung", "-from:werbung", ""),
    ("--x", "--x", ""),
    ("a - b", "a b", "MissingTerm:-"),
    ("-", "", "MissingTerm:-"),
    // Nothing searchable.
    ("!!!", "", "NothingToSearch:!!!"),
    ("🎉 party", "party", "NothingToSearch:🎉"),
    ("\"\"", "", "NothingToSearch:\"\""),
    ("*", "", "NothingToSearch:*"),
    ("from:!!!", "", "NothingToSearch:from:!!!"),
    // Quotes.
    (
        "\"unclosed phrase",
        "\"unclosed phrase\"",
        "UnclosedQuote:\"unclosed phrase",
    ),
    ("a \"b", "a \"b\"", "UnclosedQuote:\"b"),
    ("\"", "", "UnclosedQuote:\", NothingToSearch:\""),
    ("from:\"Max", "from:\"Max\"", "UnclosedQuote:\"Max"),
    // Text filters and their aliases, keys in any case.
    ("from:alice", "from:alice", ""),
    ("FROM:alice", "from:alice", ""),
    ("von:alice", "from:alice", ""),
    ("Von:alice", "from:alice", ""),
    ("from:alice@example.com", "from:alice@example.com", ""),
    ("from:\"Max Mustermann\"", "from:\"Max Mustermann\"", ""),
    ("from: bob", "from:bob", ""),
    ("from: \"Max M\"", "from:\"Max M\"", ""),
    ("to:bob", "to:bob", ""),
    ("an:bob", "to:bob", ""),
    ("cc:carol", "cc:carol", ""),
    ("CC:carol", "cc:carol", ""),
    ("subject:rechnung", "subject:rechnung", ""),
    (
        "betreff:\"Rechnung November\"",
        "subject:\"Rechnung November\"",
        "",
    ),
    ("subject:rech*", "subject:rech*", ""),
    ("filename:pdf", "filename:pdf", ""),
    (
        "datei:Rechnung_2024-00815.pdf",
        "filename:Rechnung_2024-00815.pdf",
        "",
    ),
    // in:
    ("in:inbox", "in:inbox(Inbox)", ""),
    ("in:INBOX", "in:INBOX(Inbox)", ""),
    ("in:Posteingang", "in:Posteingang(Inbox)", ""),
    ("in:sent", "in:sent(Sent)", ""),
    ("in:gesendet", "in:gesendet(Sent)", ""),
    ("in:drafts", "in:drafts(Drafts)", ""),
    ("in:Entwürfe", "in:Entwürfe(Drafts)", ""),
    ("in:archive", "in:archive(Archive)", ""),
    ("in:archiv", "in:archiv(Archive)", ""),
    ("in:trash", "in:trash(Trash)", ""),
    ("in:Papierkorb", "in:Papierkorb(Trash)", ""),
    ("in:junk", "in:junk(Junk)", ""),
    ("in:spam", "in:spam(Junk)", ""),
    ("in:Projects/2026", "in:Projects/2026", ""),
    (
        "ordner:\"Projekte/Überprüfung\"",
        "in:Projekte/Überprüfung",
        "",
    ),
    ("-in:trash", "-in:trash(Trash)", ""),
    ("in:", "", "MissingValue:in:"),
    ("in:\" \"", "", "MissingValue:in:\" \""),
    // account:
    (
        "account:alice@vicemail.test",
        "account:alice@vicemail.test",
        "",
    ),
    ("konto:Arbeit", "account:Arbeit", ""),
    ("account:\"Alice Archer\"", "account:Alice Archer", ""),
    ("-account:privat", "-account:privat", ""),
    // has: and is:
    ("has:attachment", "has:attachment", ""),
    ("has:attachments", "has:attachment", ""),
    ("hat:anhang", "has:attachment", ""),
    ("HAS:Anhänge", "has:attachment", ""),
    ("-has:attachment", "-has:attachment", ""),
    ("has:pdf", "", "UnknownValue:pdf"),
    ("is:unread", "is:Unread", ""),
    ("ist:ungelesen", "is:Unread", ""),
    ("is:read", "is:Read", ""),
    ("ist:gelesen", "is:Read", ""),
    ("is:flagged", "is:Flagged", ""),
    ("is:starred", "is:Flagged", ""),
    ("ist:markiert", "is:Flagged", ""),
    ("is:snoozed", "is:Snoozed", ""),
    ("ist:zurückgestellt", "is:Snoozed", ""),
    ("-is:unread", "-is:Unread", ""),
    ("is:foo", "", "UnknownValue:foo"),
    ("is:", "", "MissingValue:is:"),
    // Dates.
    ("before:2026-11-22", "before:2026-11-22", ""),
    ("vor:22.11.2026", "before:2026-11-22", ""),
    ("after:1.3.26", "after:2026-03-01", ""),
    ("after:22.11.", "after:2026-11-22", ""),
    ("nach:gestern", "after:2026-11-21", ""),
    ("after:today", "after:2026-11-22", ""),
    ("before:heute", "before:2026-11-22", ""),
    ("before:\"22. Nov 2026\"", "before:2026-11-22", ""),
    ("after:\"Nov 22, 2026\"", "after:2026-11-22", ""),
    ("after:22.Nov.2026", "after:2026-11-22", ""),
    ("after:\"3. März\"", "after:2026-03-03", ""),
    ("before: 22.11.2026", "before:2026-11-22", ""),
    ("after:01/02/2026", "after:2026-02-01", ""),
    ("-before:2026-01-01", "-before:2026-01-01", ""),
    ("before:31.02.2026", "", "InvalidDate:31.02.2026"),
    ("after:xyz", "", "InvalidDate:xyz"),
    ("after:11/22/2026", "", "InvalidDate:11/22/2026"),
    ("before:\"22. Foo 2026\"", "", "InvalidDate:22. Foo 2026"),
    ("before:2026-11", "", "InvalidDate:2026-11"),
    ("before:", "", "MissingValue:before:"),
    (
        "before:\"22.11.2026",
        "before:2026-11-22",
        "UnclosedQuote:\"22.11.2026",
    ),
    // Ages.
    ("older_than:7d", "older_than:7Day", ""),
    ("newer_than:2w", "newer_than:2Week", ""),
    ("newer_than:3m", "newer_than:3Month", ""),
    ("older_than:1y", "older_than:1Year", ""),
    ("OLDER_THAN:10T", "older_than:10Day", ""),
    ("-newer_than:1j", "-newer_than:1Year", ""),
    ("older_than:7x", "", "InvalidAge:7x"),
    ("newer_than:0d", "", "InvalidAge:0d"),
    ("newer_than:d", "", "InvalidAge:d"),
    // Missing values.
    ("from:", "", "MissingValue:from:"),
    ("from:\"\"", "", "MissingValue:from:\"\""),
    ("-from:", "", "MissingValue:-from:"),
    ("from: to:bob", "to:bob", "MissingValue:from:"),
    ("from: -bob", "-bob", "MissingValue:from:"),
    ("from: OR a", "a", "MissingValue:from:, MisplacedOr:OR"),
    // OR and AND.
    ("rechnung OR invoice", "(rechnung | invoice)", ""),
    ("a b OR c", "a (b | c)", ""),
    ("a OR b c", "(a | b) c", ""),
    ("a OR b OR c", "(a | b | c)", ""),
    ("from:alice OR from:bob", "(from:alice | from:bob)", ""),
    ("grüße OR grüsse", "(grüße | grüsse)", ""),
    ("\"vielen dank\" OR danke", "(\"vielen dank\" | danke)", ""),
    ("a or b", "a or b", ""),
    ("a \"OR\" b", "a \"OR\" b", ""),
    ("-OR", "-OR", ""),
    ("a AND b", "a b", ""),
    ("AND", "", ""),
    ("OR a", "a", "MisplacedOr:OR"),
    ("a OR", "a", "MisplacedOr:OR"),
    ("a OR OR b", "(a | b)", "MisplacedOr:OR"),
    ("OR", "", "MisplacedOr:OR"),
    (
        "in:inbox OR rechnung",
        "in:inbox(Inbox) rechnung",
        "OrNeedsText:OR",
    ),
    ("a OR is:unread", "a is:Unread", "OrNeedsText:OR"),
    ("a OR -b", "a -b", "OrWithExclusion:OR"),
    ("-a OR b", "-a b", "OrWithExclusion:OR"),
    ("a OR !!! b", "(a | b)", "NothingToSearch:!!!"),
    // Combinations.
    (
        "from:alice subject:rechnung has:attachment is:unread after:2026-01-01 -in:trash",
        "from:alice subject:rechnung has:attachment is:Unread after:2026-01-01 -in:trash(Trash)",
        "",
    ),
    (
        "von:alice betreff:\"Rechnung November\" -ist:gelesen nach:1.11.2026 vor:22.11.2026",
        "from:alice subject:\"Rechnung November\" -is:Read after:2026-11-01 before:2026-11-22",
        "",
    ),
    (
        "from:bob rech* -spam in:inbox older_than:7d",
        "from:bob rech* -spam in:inbox(Inbox) older_than:7Day",
        "",
    ),
    (
        "rechnung OR invoice from:shop -has:attachment newer_than:3m",
        "(rechnung | invoice) from:shop -has:attachment newer_than:3Month",
        "",
    ),
    (
        "is:foo from: \"unclosed",
        "from:\"unclosed\"",
        "UnknownValue:foo, UnclosedQuote:\"unclosed",
    ),
    (
        "form:x before:32.1.2026 re* OR",
        "form:x re",
        "UnknownFilter:form, InvalidDate:32.1.2026, PrefixTooShort:re*, MisplacedOr:OR",
    ),
];

#[test]
fn parse_table() {
    let mut failures = Vec::new();
    for (input, terms, errors) in PARSE_CASES {
        let query = parse(input, &ctx());
        let got = (describe(&query), describe_errors(input, &query));
        if got != ((*terms).to_owned(), (*errors).to_owned()) {
            failures.push(format!(
                "{input:?}\n   want {terms:?} / {errors:?}\n   got  {:?} / {:?}",
                got.0, got.1
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn slash_dates_follow_the_locale() {
    let us = SearchContext {
        slash_dates: SlashDateOrder::MonthFirst,
        ..ctx()
    };
    let query = parse("after:01/02/2026 before:11/22/2026", &us);
    assert_eq!(describe(&query), "after:2026-01-02 before:2026-11-22");
    let query = parse("after:01/02/2026 before:11/22/2026", &ctx());
    assert_eq!(describe(&query), "after:2026-02-01");
    assert_eq!(
        describe_errors("after:01/02/2026 before:11/22/2026", &query),
        "InvalidDate:11/22/2026"
    );
}

#[test]
fn spans_point_into_the_input() {
    let input = "über -from:\"Jürgen G\" 😀x* in:Archiv";
    let query = parse(input, &ctx());
    let terms: Vec<&Term> = query.terms().collect();
    let spans: Vec<(&str, &str)> = terms
        .iter()
        .map(|t| {
            (
                &input[t.span.start..t.span.end],
                &input[t.value_span.start..t.value_span.end],
            )
        })
        .collect();
    assert_eq!(
        spans,
        [
            ("über", "über"),
            ("-from:\"Jürgen G\"", "Jürgen G"),
            ("😀x*", "😀x"),
            ("in:Archiv", "Archiv"),
        ]
    );
    assert_eq!(
        describe(&query),
        "über -from:\"Jürgen G\" 😀x in:Archiv(Archive)"
    );
    // The prefix is too short ("x"), reported at the term.
    assert_eq!(describe_errors(input, &query), "PrefixTooShort:😀x*");
    // UTF-16 offsets for the UI: "ü" is one unit, "😀" two.
    assert_eq!(terms[0].span.to_utf16(input), (0, 4));
    assert_eq!(terms[1].span.to_utf16(input), (5, 21));
    assert_eq!(terms[2].span.to_utf16(input), (22, 26));
    assert_eq!(Span::new(1, 2).to_utf16("ü"), (0, 1));
    assert_eq!(Span::new(0, 99).to_utf16("ab"), (0, 2));
    // `from: bob` covers both words, and an `OR` group spans its members.
    let input = "x from: bob OR to:c";
    let query = parse(input, &ctx());
    assert_eq!(describe(&query), "x (from:bob | to:c)");
    let span = query.clauses[1].span();
    assert_eq!(&input[span.start..span.end], "from: bob OR to:c");
}

#[test]
fn long_input_is_cut_and_reported() {
    let input = "wort ".repeat(500);
    let query = parse(&input, &ctx());
    assert_eq!(query.terms().count(), MAX_TERMS);
    let kinds: Vec<SearchErrorKind> = query.errors.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [SearchErrorKind::TooManyTerms, SearchErrorKind::TooLong]
    );
    assert_eq!(
        query.errors[1].span,
        Span::new(MAX_QUERY_BYTES, input.len())
    );
    assert_eq!(query.errors[0].span.start, MAX_TERMS * 5);
    // Cutting never splits a character.
    let input = "ä".repeat(MAX_QUERY_BYTES);
    let query = parse(&input, &ctx());
    assert_eq!(query.errors[0].kind, SearchErrorKind::TooLong);
    assert!(input.is_char_boundary(query.errors[0].span.start));
    // A cut input is not "still typing".
    let compiled = query.compile(
        &ctx(),
        CompileOptions {
            prefix_last_word: true,
            substrings: Substrings::Off,
        },
    );
    assert!(text_match(&compiled).starts_with("match:"));
    assert!(!text_match(&compiled).ends_with('*'));
}

/// The text of a query with the word index only, in the form of one expression (as before
/// decision 0074): `match:` the positive terms, `NOT` the exclusions; `exclude:` exclusions only.
fn text_match(compiled: &CompiledQuery) -> String {
    let text = &compiled.text;
    assert!(
        text.all.len() <= 1
            && text.all.iter().all(|c| c.trigrams.is_none())
            && text.none.trigrams.is_none(),
        "word index only: {text:?}"
    );
    let words = text.all.first().and_then(|c| c.words.clone());
    match (words, &text.none.words) {
        (None, None) => "any".to_owned(),
        (Some(w), None) => format!("match:{w}"),
        (Some(w), Some(n)) => format!("match:({w}) NOT ({n})"),
        (None, Some(n)) => format!("exclude:{n}"),
    }
}

/// Every expression of a text match.
fn expressions(text: &TextMatch) -> Vec<&str> {
    text.all
        .iter()
        .chain(std::iter::once(&text.none))
        .flat_map(|c| [c.words.as_deref(), c.trigrams.as_deref()])
        .flatten()
        .collect()
}

/// Decision 0074: `words:` and `tri:` clauses (one index), `either:` (word | trigram), then the
/// exclusions per index, separated by ` ; `.
fn describe_text(text: &TextMatch) -> String {
    let mut parts = Vec::new();
    for clause in &text.all {
        parts.push(match (&clause.words, &clause.trigrams) {
            (Some(w), None) => format!("words:{w}"),
            (None, Some(t)) => format!("tri:{t}"),
            (Some(w), Some(t)) => format!("either:{w} | {t}"),
            (None, None) => "empty".to_owned(),
        });
    }
    if let Some(w) = &text.none.words {
        parts.push(format!("not-words:{w}"));
    }
    if let Some(t) = &text.none.trigrams {
        parts.push(format!("not-tri:{t}"));
    }
    if parts.is_empty() {
        "any".to_owned()
    } else {
        parts.join(" ; ")
    }
}

/// (input, FTS text with `prefix_last_word` off, with it on).
const FTS_CASES: &[(&str, &str, &str)] = &[
    ("", "any", "any"),
    ("is:unread in:inbox", "any", "any"),
    ("rechnung", r#"match:"rechnung""#, r#"match:"rechnung" *"#),
    ("rechnung ", r#"match:"rechnung""#, r#"match:"rechnung""#),
    ("re", r#"match:"re""#, r#"match:"re""#),
    (
        "Rechnung November",
        r#"match:"Rechnung" AND "November""#,
        r#"match:"Rechnung" AND "November" *"#,
    ),
    (
        "\"vielen dank\"",
        r#"match:"vielen dank""#,
        r#"match:"vielen dank""#,
    ),
    ("rech*", r#"match:"rech" *"#, r#"match:"rech" *"#),
    ("grüße", r#"match:"grüße""#, r#"match:"grüße" *"#),
    (
        "from:bob",
        r#"match:{from} : "bob""#,
        r#"match:{from} : "bob" *"#,
    ),
    (
        "to:x cc:y subject:z filename:pdf",
        r#"match:{to} : "x" AND {cc} : "y" AND {subject} : "z" AND {attachments} : "pdf""#,
        r#"match:{to} : "x" AND {cc} : "y" AND {subject} : "z" AND {attachments} : "pdf" *"#,
    ),
    (
        "a b OR c",
        r#"match:"a" AND ("b" OR "c")"#,
        r#"match:"a" AND ("b" OR "c")"#,
    ),
    (
        "rechnung OR angeb",
        r#"match:("rechnung" OR "angeb")"#,
        r#"match:("rechnung" OR "angeb" *)"#,
    ),
    (
        "rechnung -spam",
        r#"match:("rechnung") NOT ("spam")"#,
        r#"match:("rechnung") NOT ("spam")"#,
    ),
    (
        "rechnung -spam -from:werbung",
        r#"match:("rechnung") NOT ("spam" OR {from} : "werbung")"#,
        r#"match:("rechnung") NOT ("spam" OR {from} : "werbung")"#,
    ),
    ("-spam", r#"exclude:"spam""#, r#"exclude:"spam""#),
    (
        "-spam -\"kein interesse\" is:unread",
        r#"exclude:"spam" OR "kein interesse""#,
        r#"exclude:"spam" OR "kein interesse""#,
    ),
    (
        "rechnung in:inbox",
        r#"match:"rechnung""#,
        r#"match:"rechnung""#,
    ),
    // Injection attempts stay inside string literals.
    (
        "foo:\"bar\"",
        r#"match:"foo:""bar""""#,
        r#"match:"foo:""bar""""#,
    ),
    (
        "{from}:x NEAR(a b) ^start col:x",
        r#"match:"{from}:x" AND "NEAR(a" AND "b)" AND "^start" AND "col:x""#,
        r#"match:"{from}:x" AND "NEAR(a" AND "b)" AND "^start" AND "col:x""#,
    ),
    (
        "a NOT b",
        r#"match:"a" AND "NOT" AND "b""#,
        r#"match:"a" AND "NOT" AND "b""#,
    ),
    ("a*b", r#"match:"a*b""#, r#"match:"a*b""#),
    ("!!! 🎉", "any", "any"),
    (
        "\"vielen dank",
        r#"match:"vielen dank""#,
        r#"match:"vielen dank""#,
    ),
];

#[test]
fn fts_table() {
    let mut failures = Vec::new();
    for (input, closed, open) in FTS_CASES {
        let query = parse(input, &ctx());
        for (prefix_last_word, want) in [(false, closed), (true, open)] {
            let options = CompileOptions {
                prefix_last_word,
                substrings: Substrings::Off,
            };
            let got = text_match(&query.compile(&ctx(), options));
            if got != *want {
                failures.push(format!(
                    "{input:?} (prefix_last_word {prefix_last_word})\n   want {want}\n   got  {got}"
                ));
            }
            for expr in expressions(&query.compile(&ctx(), options).text) {
                check_fts_syntax(expr);
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn compiled(input: &str) -> CompiledQuery {
    let query = parse(input, &ctx());
    assert!(query.errors.is_empty(), "{input}: {:?}", query.errors);
    query.compile(&ctx(), CompileOptions::default())
}

#[test]
fn structured_filters() {
    let q = compiled("account:a -account:b in:inbox -in:trash in:Projects/2026");
    assert_eq!(q.accounts, ["a"]);
    assert_eq!(q.excluded_accounts, ["b"]);
    let names: Vec<(&str, Option<MailboxRole>)> = q
        .mailboxes
        .iter()
        .map(|m| (m.name.as_str(), m.role))
        .collect();
    assert_eq!(
        names,
        [("inbox", Some(MailboxRole::Inbox)), ("Projects/2026", None)]
    );
    assert_eq!(q.excluded_mailboxes[0].role, Some(MailboxRole::Trash));
    assert!(!q.matches_nothing());
    assert!(q.text.is_any());

    let q = compiled("is:unread is:flagged -is:snoozed has:attachment");
    assert_eq!(
        (q.unread, q.flagged, q.snoozed, q.has_attachment),
        (Some(true), Some(true), Some(false), Some(true))
    );
    let q = compiled("-is:unread -has:attachment");
    assert_eq!((q.unread, q.has_attachment), (Some(false), Some(false)));
    let q = compiled("is:read");
    assert_eq!(q.unread, Some(false));
    let q = compiled("-is:read is:unread");
    assert_eq!(q.unread, Some(true));
    assert!(q.conflicts.is_empty());

    assert!(compiled("").is_unrestricted());
    assert!(!compiled("is:unread").is_unrestricted());
    // A dropped term leaves nothing to restrict.
    assert!(
        parse("is:foo", &ctx())
            .compile(&ctx(), CompileOptions::default())
            .is_unrestricted()
    );
}

#[test]
fn contradictions_are_marked() {
    for (input, conflicting) in [
        ("is:unread is:read", "is:read"),
        ("is:read -is:read", "-is:read"),
        ("has:attachment -has:attachment", "-has:attachment"),
        ("is:flagged x -is:flagged", "-is:flagged"),
        ("in:inbox -in:Posteingang", "-in:Posteingang"),
        ("-in:Projekte in:projekte", "in:projekte"),
        ("account:A -account:a", "-account:a"),
        ("after:2026-11-22 before:2026-11-01", "before:2026-11-01"),
        ("after:today before:today", "before:today"),
        ("newer_than:7d older_than:1m", "older_than:1m"),
    ] {
        let q = compiled(input);
        assert!(q.matches_nothing(), "{input}");
        let spans: Vec<&str> = q.conflicts.iter().map(|s| &input[s.start..s.end]).collect();
        assert_eq!(spans, [conflicting], "{input}");
    }
    let q = compiled("in:inbox -in:trash is:unread -is:flagged");
    assert!(q.conflicts.is_empty());
}

#[test]
fn dates_become_rowid_ranges() {
    let range = |input: &str| compiled(input).dates;
    let r = |from: Option<i64>, until: Option<i64>| RowidRange { from, until };
    assert_eq!(range("rechnung"), r(None, None));
    assert_eq!(range("after:2026-11-01"), r(Some(day(2026, 11, 1)), None));
    assert_eq!(range("before:2026-11-01"), r(None, Some(day(2026, 11, 1))));
    assert_eq!(range("-before:2026-11-01"), r(Some(day(2026, 11, 1)), None));
    assert_eq!(range("-after:2026-11-01"), r(None, Some(day(2026, 11, 1))));
    assert_eq!(
        range("after:1.11.2026 before:22.11.2026"),
        r(Some(day(2026, 11, 1)), Some(day(2026, 11, 22)))
    );
    // Several bounds narrow the range.
    assert_eq!(
        range("after:2026-11-01 after:2026-11-10 before:2026-12-01 before:2026-11-20"),
        r(Some(day(2026, 11, 10)), Some(day(2026, 11, 20)))
    );
    assert_eq!(range("after:today"), r(Some(TODAY), None));
    assert_eq!(range("after:yesterday"), r(Some(TODAY - DAY), None));
    assert_eq!(range("newer_than:7d"), r(Some(NOW - 7 * DAY), None));
    assert_eq!(range("older_than:7d"), r(None, Some(NOW - 7 * DAY)));
    assert_eq!(range("-older_than:2w"), r(Some(NOW - 14 * DAY), None));
    assert_eq!(range("-newer_than:2w"), r(None, Some(NOW - 14 * DAY)));
    assert_eq!(
        range("newer_than:1m"),
        r(Some(day(2026, 10, 22) + DAY / 2), None)
    );
    assert_eq!(
        range("older_than:1y"),
        r(None, Some(day(2025, 11, 22) + DAY / 2))
    );
    // Local midnight in the user's time zone (Berlin in winter, UTC+1).
    let berlin = FixedOffset(3600);
    let context = SearchContext {
        time_zone: &berlin,
        ..ctx()
    };
    let q = parse("after:22.11.2026", &context).compile(&context, CompileOptions::default());
    assert_eq!(q.dates.from, Some(day(2026, 11, 22) - 3600));
}

/// Checks that an expression only uses the FTS5 syntax compile emits: string literals with at
/// least one token character, `AND`/`OR`/`NOT`, parentheses, ` *` and known column filters.
fn check_fts_syntax(expr: &str) {
    let columns: Vec<&str> = FtsColumn::ALL.iter().map(|c| c.name()).collect();
    let mut rest = expr;
    let mut depth = 0_i32;
    let mut previous_was_operand = false;
    while let Some(c) = rest.chars().next() {
        match c {
            ' ' => rest = &rest[1..],
            '"' => {
                let mut end = None;
                let bytes = rest.as_bytes();
                let mut i = 1;
                while i < bytes.len() {
                    if bytes[i] == b'"' {
                        if bytes.get(i + 1) == Some(&b'"') {
                            i += 2;
                            continue;
                        }
                        end = Some(i);
                        break;
                    }
                    i += 1;
                }
                let end = end.unwrap_or_else(|| panic!("unterminated string in {expr:?}"));
                let content = &rest[1..end];
                assert!(
                    content.chars().any(super::parse::is_token_char),
                    "empty phrase in {expr:?}"
                );
                assert!(!content.chars().any(char::is_control), "{expr:?}");
                assert!(!previous_was_operand, "two operands in a row in {expr:?}");
                previous_was_operand = true;
                rest = &rest[end + 1..];
            }
            '(' => {
                depth += 1;
                previous_was_operand = false;
                rest = &rest[1..];
            }
            ')' => {
                depth -= 1;
                assert!(depth >= 0, "unbalanced in {expr:?}");
                rest = &rest[1..];
            }
            '*' => {
                assert!(previous_was_operand, "stray * in {expr:?}");
                rest = &rest[1..];
            }
            '{' => {
                let end = rest.find('}').expect("closing brace");
                let names: Vec<&str> = rest[1..end].split(' ').collect();
                assert!(
                    !names.is_empty() && names.iter().all(|n| columns.contains(n)),
                    "column in {expr:?}"
                );
                let after = rest[end + 1..].strip_prefix(" : ").expect("column colon");
                assert!(
                    after.starts_with('"'),
                    "column filter before a string in {expr:?}"
                );
                assert!(!previous_was_operand, "{expr:?}");
                rest = after;
            }
            _ => {
                let word: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase())
                    .collect();
                assert!(
                    matches!(word.as_str(), "AND" | "OR" | "NOT"),
                    "unexpected {rest:?} in {expr:?}"
                );
                assert!(previous_was_operand, "operator without operand in {expr:?}");
                previous_was_operand = false;
                rest = &rest[word.len()..];
            }
        }
    }
    assert_eq!(depth, 0, "unbalanced in {expr:?}");
    assert!(previous_was_operand, "ends with an operator: {expr:?}");
}

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const FRAGMENTS: &[&str] = &[
    " ",
    " ",
    " ",
    "\t",
    "\n",
    "\u{0}",
    "\u{7f}",
    "\u{a0}",
    "\u{2028}",
    "-",
    "--",
    "\"",
    "\"\"",
    "*",
    "**",
    ":",
    "::",
    "OR",
    "AND",
    "NOT",
    "NEAR",
    "(",
    ")",
    "{",
    "}",
    "^",
    "+",
    ".",
    ",",
    "/",
    "@",
    "_",
    "a",
    "rech",
    "nung",
    "Grüße",
    "ß",
    "é",
    "e\u{301}",
    "\u{301}",
    "😀",
    "\u{e000}",
    "\u{202e}",
    "中文",
    "١٢",
    "7",
    "26",
    "2026",
    "11",
    "22",
    "from:",
    "to:",
    "cc:",
    "subject:",
    "in:",
    "account:",
    "filename:",
    "has:",
    "is:",
    "before:",
    "after:",
    "older_than:",
    "newer_than:",
    "von:",
    "an:",
    "betreff:",
    "ist:",
    "hat:",
    "vor:",
    "nach:",
    "unread",
    "attachment",
    "inbox",
    "7d",
    "3m",
    "0d",
    "22.11.2026",
    "22.11.",
    "2026-11-22",
    "01/02/2026",
    "Nov",
    "März",
    "today",
    "gestern",
    "https://",
    "Re:",
    "x1:",
    "contact:",
    "kontakt:",
    "max@firma.de",
];

#[test]
fn random_input_never_panics_and_never_injects() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let contexts = [
        ctx(),
        SearchContext {
            slash_dates: SlashDateOrder::MonthFirst,
            now: -1,
            ..ctx()
        },
        SearchContext {
            now: i64::MAX / 4,
            ..ctx()
        },
    ];
    for round in 0..20_000 {
        let mut input = String::new();
        let parts = rng.below(14) + 1;
        for _ in 0..parts {
            if rng.below(8) == 0 {
                // Any scalar value, including unassigned and private-use ones.
                let c = char::from_u32((rng.next() % 0x11_0000) as u32).unwrap_or('\u{fffd}');
                input.push(c);
            } else {
                input.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
            }
        }
        if round % 997 == 0 {
            input = input.repeat(200);
        }
        let context = &contexts[round % contexts.len()];
        let query = parse(&input, context);
        for span in query
            .errors
            .iter()
            .map(|e| e.span)
            .chain(query.terms().flat_map(|t| [t.span, t.value_span]))
        {
            assert!(
                span.start <= span.end && span.end <= input.len(),
                "{input:?}"
            );
            assert!(input.is_char_boundary(span.start) && input.is_char_boundary(span.end));
            let _ = span.to_utf16(&input);
        }
        assert!(query.terms().count() <= MAX_TERMS);
        assert!(
            query
                .errors
                .windows(2)
                .all(|w| w[0].span.start <= w[1].span.start)
        );
        for prefix_last_word in [false, true] {
            for substrings in [Substrings::Off, Substrings::Filling, Substrings::Complete] {
                let options = CompileOptions {
                    prefix_last_word,
                    substrings,
                };
                let compiled = query.compile(context, options);
                for expr in expressions(&compiled.text) {
                    check_fts_syntax(expr);
                }
                for contact in &compiled.contacts {
                    if let Some(expr) = contact_match(std::slice::from_ref(&contact.value)) {
                        check_fts_syntax(&expr);
                    }
                }
            }
        }
    }
}

/// Stage 20 (decision 0060): `larger:` finds the messages that fill the mailbox.
#[test]
fn larger_takes_sizes_with_units_and_compiles_to_bounds() {
    let parsed = parse(
        "larger:10M größer:500k -larger:2G larger:1,5m larger:12345",
        &ctx(),
    );
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let terms: Vec<String> = parsed
        .clauses
        .iter()
        .filter_map(|c| match c {
            Clause::Term(t) => Some(describe_term(t)),
            Clause::AnyOf(_) => None,
        })
        .collect();
    assert_eq!(
        terms,
        [
            "larger:10485760",
            "larger:512000",
            "-larger:2147483648",
            "larger:1572864",
            "larger:12345"
        ]
    );
    let compiled = parsed.compile(&ctx(), CompileOptions::default());
    assert_eq!(compiled.size_at_least, Some(10_485_760));
    assert_eq!(compiled.size_below, Some(2_147_483_648));
    assert!(compiled.conflicts.is_empty());
    let conflict = parse("larger:5M -larger:1M", &ctx()).compile(&ctx(), CompileOptions::default());
    assert_eq!(conflict.conflicts.len(), 1);
    for bad in ["larger:big", "larger:5X", "larger:-3M", "larger:2T"] {
        assert!(!parse(bad, &ctx()).errors.is_empty(), "{bad}");
    }
}

/// (input, typing, text with the trigram index being filled, with it complete); decision 0074.
const SUBSTRING_CASES: &[(&str, bool, &str, &str)] = &[
    (
        "uster",
        false,
        r#"either:"uster" | "uster""#,
        r#"tri:"uster""#,
    ),
    // Shorter than three characters, quoted: words only.
    ("us", false, r#"words:"us""#, r#"words:"us""#),
    ("us", true, r#"words:"us""#, r#"words:"us""#),
    (
        "\"muster\"",
        false,
        r#"words:"muster""#,
        r#"words:"muster""#,
    ),
    (
        "\"vielen dank\"",
        false,
        r#"words:"vielen dank""#,
        r#"words:"vielen dank""#,
    ),
    // The word being typed: a prefix in the word index, a substring in the trigram one.
    ("rech", true, r#"either:"rech" * | "rech""#, r#"tri:"rech""#),
    (
        "rech*",
        false,
        r#"either:"rech" * | "rech""#,
        r#"tri:"rech""#,
    ),
    ("1234", false, r#"either:"1234" | "1234""#, r#"tri:"1234""#),
    // More than one word: both indexes, also when complete.
    (
        "bob@example.com",
        false,
        r#"either:"bob@example.com" | "bob@example.com""#,
        r#"either:"bob@example.com" | "bob@example.com""#,
    ),
    (
        "a-b",
        false,
        r#"either:"a-b" | "a-b""#,
        r#"either:"a-b" | "a-b""#,
    ),
    // Combining marks do not count.
    (
        "u\u{308}s",
        false,
        "words:\"u\u{308}s\"",
        "words:\"u\u{308}s\"",
    ),
    // Columns.
    (
        "from:uster",
        false,
        r#"either:{from} : "uster" | {from} : "uster""#,
        r#"tri:{from} : "uster""#,
    ),
    (
        "filename:2026",
        false,
        r#"either:{attachments} : "2026" | {attachments} : "2026""#,
        r#"tri:{attachments} : "2026""#,
    ),
    // Terms of one index are joined; mixed ones stay clauses of their own.
    (
        "max mustermann",
        false,
        r#"either:"max" | "max" ; either:"mustermann" | "mustermann""#,
        r#"tri:"max" AND "mustermann""#,
    ),
    (
        "ab muster",
        false,
        r#"words:"ab" ; either:"muster" | "muster""#,
        r#"words:"ab" ; tri:"muster""#,
    ),
    // OR: the members' expressions per index.
    (
        "uster OR ab",
        false,
        r#"either:("uster" OR "ab") | "uster""#,
        r#"either:"ab" | "uster""#,
    ),
    (
        "uster OR muster",
        false,
        r#"either:("uster" OR "muster") | ("uster" OR "muster")"#,
        r#"tri:("uster" OR "muster")"#,
    ),
    // Exclusions per index.
    (
        "-uster",
        false,
        r#"not-words:"uster" ; not-tri:"uster""#,
        r#"not-tri:"uster""#,
    ),
    (
        "rechnung -uster -ab",
        false,
        r#"either:"rechnung" | "rechnung" ; not-words:"uster" OR "ab" ; not-tri:"uster""#,
        r#"tri:"rechnung" ; not-words:"ab" ; not-tri:"uster""#,
    ),
    // Injection attempts stay inside string literals in both indexes.
    (
        "{from}:x",
        false,
        r#"either:"{from}:x" | "{from}:x""#,
        r#"either:"{from}:x" | "{from}:x""#,
    ),
    (
        "NEAR(abc",
        false,
        r#"either:"NEAR(abc" | "NEAR(abc""#,
        r#"either:"NEAR(abc" | "NEAR(abc""#,
    ),
    (
        "foo:\"bar\"",
        false,
        r#"either:"foo:""bar""" | "foo:""bar""""#,
        r#"either:"foo:""bar""" | "foo:""bar""""#,
    ),
    ("is:unread", false, "any", "any"),
];

#[test]
fn substring_table() {
    let mut failures = Vec::new();
    for (input, typing, filling, complete) in SUBSTRING_CASES {
        let query = parse(input, &ctx());
        for (substrings, want) in [
            (Substrings::Filling, filling),
            (Substrings::Complete, complete),
        ] {
            let compiled = query.compile(
                &ctx(),
                CompileOptions {
                    prefix_last_word: *typing,
                    substrings,
                },
            );
            let got = describe_text(&compiled.text);
            if got != *want {
                failures.push(format!(
                    "{input:?} ({substrings:?}, typing {typing})\n   want {want}\n   got  {got}"
                ));
            }
            for expr in expressions(&compiled.text) {
                check_fts_syntax(expr);
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    // The default is the safe one: word hits never go missing while the index fills.
    assert_eq!(CompileOptions::default().substrings, Substrings::Filling);
}

/// Decision 0074: `contact:` is a filter of its own; the engine resolves it.
#[test]
fn contact_terms() {
    let query = parse("contact:max@firma.de -kontakt:spam@x.de rechnung", &ctx());
    assert!(query.errors.is_empty(), "{:?}", query.errors);
    let compiled = query.compile(&ctx(), CompileOptions::default());
    let contacts: Vec<(&str, bool)> = compiled
        .contacts
        .iter()
        .map(|c| (c.value.as_str(), c.negated))
        .collect();
    assert_eq!(contacts, [("max@firma.de", false), ("spam@x.de", true)]);
    assert_eq!(
        &"contact:max@firma.de -kontakt:spam@x.de rechnung"
            [compiled.contacts[1].span.start..compiled.contacts[1].span.end],
        "-kontakt:spam@x.de"
    );
    assert!(!compiled.is_unrestricted());
    assert!(
        !parse("contact:a", &ctx())
            .compile(&ctx(), CompileOptions::default())
            .is_unrestricted()
    );
    // Nothing to search, no value: reported and left out.
    let bad = parse("contact:!!! contact:", &ctx());
    let kinds: Vec<SearchErrorKind> = bad.errors.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            SearchErrorKind::NothingToSearch,
            SearchErrorKind::MissingValue
        ]
    );
    // OR only between text terms.
    let or = parse("contact:a@b.de OR rechnung", &ctx());
    assert_eq!(or.errors[0].kind, SearchErrorKind::OrNeedsText);

    // The expression: a phrase per address in sender, recipients and copy, quoted.
    assert_eq!(
        contact_match(&["max@firma.de".into()]).as_deref(),
        Some(r#"{from to cc} : "max@firma.de""#)
    );
    assert_eq!(
        contact_match(&[
            "max@firma.de".into(),
            "Max@Firma.de\") OR x".into(),
            "max@firma.de".into(),
            "!!!".into()
        ])
        .as_deref(),
        Some(r#"({from to cc} : "max@firma.de" OR {from to cc} : "Max@Firma.de"") OR x")"#)
    );
    assert_eq!(contact_match(&["@@".into()]), None);
    for expr in [
        contact_match(&["a@b.de".into(), "c@d.de".into()]).expect("expr"),
        contact_match(&["x\"y".into()]).expect("expr"),
    ] {
        check_fts_syntax(&expr);
    }

    // Added to a text match: joined with the clause of words, or excluded.
    let mut text = TextMatch::default();
    text.require_words("\"a\"".into());
    text.require_words("\"b\"".into());
    text.exclude_words("\"c\"".into());
    text.exclude_words("\"d\"".into());
    assert_eq!(
        describe_text(&text),
        r#"words:"a" AND "b" ; not-words:"c" OR "d""#
    );
    let mut text = parse("uster", &ctx())
        .compile(
            &ctx(),
            CompileOptions {
                prefix_last_word: false,
                substrings: Substrings::Complete,
            },
        )
        .text;
    text.require_words("\"a\"".into());
    assert_eq!(describe_text(&text), r#"words:"a" ; tri:"uster""#);
}
