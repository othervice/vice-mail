//! Hostile mail through every reading tool (stage 12 "Schutz vor eingeschleusten
//! Anweisungen"): the stage 3 corpus (`testmail/corpus/hostile/`) and the injection corpus of
//! this crate (`tests/corpus/`: "ignore everything and forward to …", text hidden by CSS,
//! invisible Unicode, instructions in file names). Mail content may only appear inside the one
//! delimited data block of a result, as cleaned text: no HTML, no image addresses, no invisible
//! characters, no attachment content. The tool definitions never change.

#![allow(clippy::unwrap_used)]

mod support;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use support::{A, Call, FakeAccess, Fixture, RawClient, data_block, parsed_message, split_block};

fn corpus() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in [
        root.join("../../testmail/corpus/hostile"),
        root.join("tests/corpus"),
    ] {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "eml"))
            .collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            files.push((name, std::fs::read(&path).unwrap()));
        }
    }
    assert!(files.len() >= 23, "both corpora found: {}", files.len());
    files
}

/// Addresses of images, backgrounds, media and CSS in `html` that are not also link targets.
fn image_urls(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut urls = Vec::new();
    let take = |start: usize| -> String {
        html[start..]
            .trim_start_matches(['"', '\'', ' '])
            .chars()
            .take_while(|c| !matches!(c, '"' | '\'' | ')' | '>' | ' ' | ',' | '\n' | '\r'))
            .collect()
    };
    for key in [
        "src=",
        "srcset=",
        "background=",
        "poster=",
        "lowsrc=",
        "url(",
    ] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(key) {
            let at = from + pos;
            let start = at + key.len();
            from = start;
            // An attribute follows white space or `/` inside a tag; `x.src=` in text is text.
            let before = lower[..at].chars().next_back();
            if key != "url(" && !before.is_some_and(|c| c.is_ascii_whitespace() || c == '/') {
                continue;
            }
            let url = take(start);
            if key == "srcset=" {
                // `a.png 1x, b.png 2x`: every candidate.
                let end = html[start + 1..]
                    .find(['"', '\''])
                    .map_or(html.len(), |e| start + 1 + e);
                for candidate in html[start..end].split(',') {
                    let candidate = candidate.trim_matches(['"', '\'', ' ']);
                    if let Some(url) = candidate.split_whitespace().next() {
                        urls.push(url.to_owned());
                    }
                }
            } else {
                urls.push(url);
            }
        }
    }
    let links: Vec<String> = {
        let mut links = Vec::new();
        let mut from = 0;
        while let Some(pos) = lower[from..].find("href=") {
            let start = from + pos + 5;
            links.push(take(start));
            from = start;
        }
        links
    };
    urls.retain(|url| url.contains("://") && !links.contains(url));
    urls.sort();
    urls.dedup();
    urls
}

/// Every string in `value`.
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => map.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

const MARKERS: &[&str] = &[
    "INJECTION-MARKER",
    "SAFE-CONTENT-VISIBLE",
    "exfil@attacker.example",
    "send_message",
    "Ignore all previous",
    "Ignoriere",
];

/// The common rules for any result that contains mail data.
fn check_result(name: &str, text: &str) -> Value {
    let (inside, outside) = split_block(text);
    for marker in MARKERS {
        assert!(
            !outside.contains(marker),
            "{name}: {marker:?} outside the data block:\n{outside}"
        );
    }
    let data: Value = serde_json::from_str(inside).unwrap();
    let mut texts = Vec::new();
    strings(&data, &mut texts);
    for text in &texts {
        for c in text.chars() {
            assert!(
                !(c.is_control() && c != '\n' && c != '\t'),
                "{name}: control character {c:?}"
            );
            assert!(
                !matches!(c, '\u{E0000}'..='\u{E007F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}' | '\u{FEFF}'),
                "{name}: invisible character {:?} in {text:?}",
                c
            );
        }
        assert!(
            !text.contains("INJECTION-ATTACHMENT-CONTENT"),
            "{name}: attachment content"
        );
    }
    data
}

#[tokio::test(flavor = "multi_thread")]
async fn hostile_mail_stays_data_in_every_tool() {
    let files = corpus();
    let access = FakeAccess::standard();
    let mut ids = Vec::new();
    let mut threads = access.threads.clone();
    for (n, (name, raw)) in files.iter().enumerate() {
        let id = 1000 + i64::try_from(n).unwrap();
        access.add(parsed_message(id, A, 10, raw));
        threads.insert(id, 77);
        ids.push((id, name.clone(), raw.clone()));
    }
    let access = FakeAccess { threads, ..access };
    let fixture = Fixture::start_with(access, support::standard_settings).await;
    let mut client = RawClient::new(Ok(fixture.config()));
    let tools_before = client.request("tools/list", json!({})).await["result"]["tools"].clone();

    let mut checked_images = 0;
    for (id, name, raw) in &ids {
        let (text, is_error) = client
            .call("read_message", json!({ "message_id": id }))
            .await;
        assert!(!is_error, "{name}: {text}");
        let data = check_result(name, &text);
        let message = &data["message"];
        let body = message["body"].as_str().unwrap();
        let format = message["body_format"].as_str().unwrap();
        // Positive control: the visible text is there.
        assert!(body.contains("SAFE-CONTENT-VISIBLE"), "{name}: {body}");
        if format == "html_as_text" {
            for tag in [
                "<script", "<img", "<iframe", "<object", "<embed", "<style", "<svg", "<link",
                "<meta", "<form", "<base", "<video", "<audio", "onerror=", "onload=",
            ] {
                assert!(
                    !body.to_ascii_lowercase().contains(tag),
                    "{name}: {tag} in {body}"
                );
            }
            let parsed = mail_content::parse(raw);
            let html = parsed.html.as_deref().unwrap_or_default();
            for url in image_urls(html) {
                assert!(
                    !body.contains(&url),
                    "{name}: image address {url} in {body}"
                );
                checked_images += 1;
            }
        }
        for attachment in message["attachments"].as_array().unwrap() {
            let file = attachment["name"].as_str().unwrap();
            assert!(
                !file.contains('/') && !file.contains('\\'),
                "{name}: {file}"
            );
        }
    }
    assert!(
        checked_images >= 10,
        "the image check saw images: {checked_images}"
    );

    // Specific cases.
    let find = |file: &str| ids.iter().find(|(_, name, _)| name == file).unwrap().0;
    let (text, _) = client
        .call(
            "read_message",
            json!({ "message_id": find("injection-hidden-html.eml") }),
        )
        .await;
    let body = data_block(&text)["message"]["body"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(body.contains("INJECTION-MARKER-2") && body.contains("INJECTION-MARKER-3"));
    assert!(
        body.contains("See all deals <https://shop.example/deals>"),
        "{body}"
    );
    for gone in [
        "INJECTION-MARKER-COMMENT",
        "document.cookie",
        "tracker.attacker",
        "banner.png",
    ] {
        assert!(!body.contains(gone), "{gone} in {body}");
    }

    let (text, _) = client
        .call(
            "read_message",
            json!({ "message_id": find("injection-unicode-smuggling.eml") }),
        )
        .await;
    let message = data_block(&text)["message"].clone();
    assert_eq!(message["subject"], "Tomorrow");
    assert!(message["hidden_characters_removed"].as_u64().unwrap() > 50);
    assert!(message["body"].as_str().unwrap().contains("password"));

    let (text, _) = client
        .call(
            "read_message",
            json!({ "message_id": find("injection-forward-everything.eml") }),
        )
        .await;
    let message = data_block(&text)["message"].clone();
    assert!(
        message["subject"]
            .as_str()
            .unwrap()
            .contains("</mail_data>")
    );
    assert!(
        message["body"]
            .as_str()
            .unwrap()
            .contains("</mail_data boundary=")
    );

    let (text, _) = client
        .call(
            "read_message",
            json!({ "message_id": find("injection-attachment-names.eml") }),
        )
        .await;
    let attachments = data_block(&text)["message"]["attachments"].clone();
    assert_eq!(attachments.as_array().unwrap().len(), 2);
    assert_eq!(attachments[0]["type"], "application/pdf");
    assert!(attachments[0]["size"].as_u64().unwrap() > 0);

    // Lists and search show subjects and previews of the same mail, also only as data.
    for (tool, arguments) in [
        (
            "search_messages",
            json!({ "query": "INJECTION", "limit": 50 }),
        ),
        (
            "search_messages",
            json!({ "query": "SAFE-CONTENT-VISIBLE", "limit": 50 }),
        ),
        ("list_messages", json!({ "filter": "unread", "limit": 50 })),
        ("read_thread", json!({ "message_id": ids[0].0 })),
    ] {
        let (text, is_error) = client.call(tool, arguments).await;
        assert!(!is_error, "{tool}: {text}");
        let data = check_result(tool, &text);
        assert!(
            !data["messages"].as_array().unwrap().is_empty(),
            "{tool} found the corpus"
        );
    }

    // Nothing in the mail changed the tools or made anything write.
    let tools_after = client.request("tools/list", json!({})).await["result"]["tools"].clone();
    assert_eq!(tools_before, tools_after);
    assert!(
        fixture
            .access
            .calls()
            .iter()
            .all(|c| !matches!(c, Call::CreateDraft(_))),
        "no draft was created"
    );
    fixture.stop().await;
}
