//! Acceptance (stage 3): the sanitised output of the whole hostile corpus contains none of the
//! forbidden elements, and every dangerous file name becomes a harmless one.
//!
//! Every `.eml` in `testmail/corpus/hostile/` is checked, new files automatically. Each file
//! says what it attacks (`X-Corpus-Vector`), what must happen (`X-Corpus-Expect`) and, in
//! `X-Corpus-Check`, counts the test verifies (`links=`, `remote=`, `text=`, `data-images=`,
//! `warned-links=`, `text-links=`, `attachments=`). The seed's HTML mails are checked too.
//!
//! Positive control: the raw HTML of every file must fail the same checker, otherwise the file
//! (or the checker) proves nothing.

mod common;

use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

use common::{Mode, eml_files, file_name, raw_header, violations, visible_text};
use mail_content::{
    LinkWarning, MAIL_CSP, ParseProblem, RemotePolicy, RenderOptions, check_link, parse,
    render_html, render_text,
};

/// A valid 1×1 PNG, standing in for every external image the user allowed.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn checks(raw: &[u8]) -> HashMap<String, String> {
    raw_header(raw, "X-Corpus-Check")
        .unwrap_or_default()
        .split(';')
        .filter_map(|item| {
            let (key, value) = item.split_once('=')?;
            Some((key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn expect_count(file: &str, checks: &HashMap<String, String>, key: &str, actual: usize) {
    if let Some(expected) = checks.get(key) {
        let expected: usize = expected.parse().expect("number in X-Corpus-Check");
        assert_eq!(actual, expected, "{file}: {key}");
    }
}

#[test]
fn sanitised_corpus_contains_nothing_forbidden() {
    let mut files = eml_files("hostile");
    let seed_html: Vec<_> = eml_files("seed")
        .into_iter()
        .filter(|p| file_name(p).starts_with("html_"))
        .collect();
    assert!(files.len() >= 19, "hostile corpus is missing");
    assert!(seed_html.len() >= 7, "seed HTML fixtures are missing");
    files.extend(seed_html);

    let mut documents = 0;
    let mut forbidden = 0;
    let mut html_files = 0;
    for path in &files {
        let name = file_name(path);
        let raw = fs::read(path).expect("read corpus file");
        let checks = checks(&raw);
        let message = parse(&raw);
        assert!(
            !message.problems.contains(&ParseProblem::Internal),
            "{name}: parser failure"
        );

        if let Some(html) = &message.html {
            html_files += 1;
            let raw_found = violations(html, Mode::Input, MAIL_CSP);
            assert!(
                !raw_found.is_empty(),
                "{name}: positive control failed, the raw HTML has nothing hostile"
            );

            let blocked = render_html(
                html,
                &RenderOptions {
                    inline: message.inline_images(),
                    ..RenderOptions::new(common::token())
                },
            );
            // Everything external allowed: the core fetched each listed URL.
            let fetched: HashMap<String, Arc<[u8]>> = blocked
                .remote_resources
                .iter()
                .map(|r| (r.url.clone(), Arc::from(PNG)))
                .collect();
            let allowed = render_html(
                html,
                &RenderOptions {
                    remote: RemotePolicy::Allow(fetched),
                    inline: message.inline_images(),
                    ..RenderOptions::new(common::token())
                },
            );
            for (policy, rendered) in [("blocked", &blocked), ("allowed", &allowed)] {
                assert_eq!(
                    rendered.fallback, None,
                    "{name} ({policy}): self-check fell back to text"
                );
                let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
                forbidden += found.len();
                documents += 1;
                assert!(
                    found.is_empty(),
                    "{name} ({policy}): {found:#?}\n{}",
                    rendered.srcdoc
                );
            }
            assert!(
                allowed.remote_resources.is_empty(),
                "{name}: allowed images still blocked"
            );
            assert_eq!(
                allowed.links, blocked.links,
                "{name}: links depend on the image policy"
            );

            expect_count(&name, &checks, "links", blocked.links.len());
            expect_count(&name, &checks, "remote", blocked.remote_resources.len());
            let data_images = blocked.srcdoc.matches("data:image/").count()
                - blocked.srcdoc.matches("data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7").count();
            expect_count(&name, &checks, "data-images", data_images);
            let warned = blocked
                .links
                .iter()
                .filter(|l| !check_link(&l.text, &l.href).warnings.is_empty())
                .count();
            expect_count(&name, &checks, "warned-links", warned);
            if let Some(marker) = checks.get("text") {
                assert!(
                    visible_text(&blocked.srcdoc).contains(marker.as_str()),
                    "{name}: benign content was removed"
                );
            }
            for link in &blocked.links {
                let check = check_link(&link.text, &link.href);
                if let Some(url) = &check.open_url {
                    let lower = url.to_ascii_lowercase();
                    assert!(
                        lower.starts_with("http://")
                            || lower.starts_with("https://")
                            || lower.starts_with("mailto:"),
                        "{name}: {url} would be opened"
                    );
                } else {
                    assert!(
                        check.warnings.iter().any(|w| matches!(
                            w,
                            LinkWarning::Blocked { .. } | LinkWarning::Invalid
                        )),
                        "{name}: unopenable link without reason"
                    );
                }
            }
        }

        let text = message.display_text();
        let rendered_text = render_text(&text);
        expect_count(&name, &checks, "text-links", rendered_text.links.len());
        for link in &rendered_text.links {
            let lower = link.href.to_ascii_lowercase();
            assert!(
                lower.starts_with("http://")
                    || lower.starts_with("https://")
                    || lower.starts_with("mailto:"),
                "{name}: text link {}",
                link.href
            );
        }
        if message.html.is_none()
            && let Some(marker) = checks.get("text")
        {
            assert!(text.contains(marker.as_str()), "{name}: text body lost");
        }

        expect_count(&name, &checks, "attachments", message.attachments.len());
        for attachment in &message.attachments {
            let problems = common::harmless_file_name_problems(&attachment.file_name);
            assert!(
                problems.is_empty(),
                "{name}: {:?} {problems:?}",
                attachment.file_name
            );
        }
    }
    assert!(html_files >= 24, "only {html_files} HTML files checked");
    eprintln!(
        "hostile corpus: {} files, {html_files} with HTML, {documents} sanitised documents, forbidden elements/attributes found: {forbidden}",
        files.len()
    );
    assert_eq!(forbidden, 0);
}

#[test]
fn dangerous_attachment_names_become_harmless() {
    let path = common::corpus_dir("hostile").join("attachment-names.eml");
    let message = parse(&fs::read(path).expect("read corpus file"));
    let names: Vec<&str> = message
        .attachments
        .iter()
        .map(|a| a.file_name.as_str())
        .collect();
    let long = format!("{}.txt", "x".repeat(196));
    let expected: Vec<&str> = vec![
        "x",
        "windows.ini",
        "evil.sh",
        "evil.bat",
        "C_evil.bat",
        "passwd",
        "_CON",
        "_aux.txt",
        "_NUL .txt",
        "_COM1",
        "_lpt9.log",
        "evil.txt.exe",
        "attachment",
        "invoicefdp.exe",
        "reportfdp.exe.txt",
        "zerowidth.txt",
        "tab andbell.txt",
        "trailing dots and spaces",
        "evil.exe",
        "hidden",
        "a_b_c_d_e_f_g.txt",
        &long,
        "attachment",
        "attachment",
        "attachment",
        "encoded.txt",
        "\u{c4}rger.txt",
    ];
    assert_eq!(names, expected);
    let executable: Vec<&str> = message
        .attachments
        .iter()
        .filter(|a| a.executable)
        .map(|a| a.file_name.as_str())
        .collect();
    assert_eq!(
        executable,
        [
            "evil.sh",
            "evil.bat",
            "C_evil.bat",
            "evil.txt.exe",
            "invoicefdp.exe",
            "evil.exe"
        ]
    );
    assert!(
        message
            .attachments
            .iter()
            .all(|a| a.name_changed || a.file_name == "x")
    );
}
