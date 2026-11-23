//! Acceptance (stage 9, categories): the seed corpus `testmail/corpus/seed/` sorted by the rules
//! of decision 0016 and compared with a human's labels. The hit rate is printed
//! (`cargo test -p mail-content --test classify_seed -- --nocapture`); misses the rules cannot
//! see are listed in `KNOWN_MISSES`, so every other difference fails the test.

mod common;

use std::fs;

use common::{eml_files, file_name};
use mail_content::{ClassifyHeaders, LocalSignals, SenderRules, classify, parse};
use mail_model::MessageCategory::{self, Newsletter, Notification, Person};

/// What a person would expect for each fixture.
fn expected(name: &str) -> MessageCategory {
    match name {
        "html_broken-markup.eml" | "html_external-everything.eml" | "html_hostile.eml" => {
            Notification
        }
        "mime_digest.eml" => Newsletter,
        _ if name.starts_with("auto_") => Notification,
        _ if name.starts_with("html_newsletter-") || name.starts_with("list_") => Newsletter,
        _ if name.starts_with("broken_")
            || name.starts_with("charset_")
            || name.starts_with("mime_") =>
        {
            Person
        }
        _ => panic!("{name}: no expected category; add one"),
    }
}

/// A phishing mail from `service@bank.example` without any header of a program: the rules see
/// a person (decision 0016, limits).
const KNOWN_MISSES: &[&str] = &["html_hostile.eml"];

#[test]
fn seed_corpus_is_sorted_as_expected() {
    let own = vec!["alice@vicemail.test".to_owned()];
    let local = LocalSignals {
        own_addresses: &own,
        ..LocalSignals::default()
    };
    let rules = SenderRules::default();
    let files = eml_files("seed");
    assert!(files.len() >= 70, "seed fixtures missing: {}", files.len());
    let mut hits = 0;
    let mut misses = Vec::new();
    let mut per_category = [0usize; 3];
    for path in &files {
        let name = file_name(path);
        let raw = fs::read(path).expect("read fixture");
        let headers = ClassifyHeaders::from_raw(&raw);
        let decision = classify(&headers, &local, &rules);
        // The parsed message gives the same answer for the headers it carries.
        let from_parsed = classify(
            &ClassifyHeaders::from_parsed(&parse(&raw).headers),
            &local,
            &rules,
        );
        if headers.content_type.as_deref() != Some("multipart/report")
            && headers.return_path.is_none()
            && headers.fields.is_empty()
        {
            assert_eq!(from_parsed.category, decision.category, "{name}");
        }
        let want = expected(&name);
        per_category[want as usize] += 1;
        if decision.category == want {
            hits += 1;
        } else {
            misses.push(format!(
                "{name}: {:?} ({:?}), expected {want:?}",
                decision.category, decision.reason
            ));
        }
    }
    let rate = hits as f64 * 100.0 / files.len() as f64;
    println!(
        "categories on the seed corpus: {hits}/{} = {rate:.1} % \
         (person {}, notification {}, newsletter {})",
        files.len(),
        per_category[0],
        per_category[1],
        per_category[2]
    );
    let unexpected: Vec<&String> = misses
        .iter()
        .filter(|m| !KNOWN_MISSES.iter().any(|k| m.starts_with(k)))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:#?}");
    assert_eq!(misses.len(), KNOWN_MISSES.len(), "{misses:#?}");
}
