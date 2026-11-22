//! Fuzz-like tests with a fixed seed: mutated corpus mails, random tag soup, random CSS and
//! random file names. Nothing may panic, the sanitiser output must pass the independent checker
//! for every input, and sanitised file names must be harmless and stable.
//!
//! `MAIL_CONTENT_FUZZ_ROUNDS=<n>` multiplies the number of cases (default 1) for longer runs.

mod common;

use std::fs;

use common::{Mode, Rng, eml_files, violations};
use mail_content::{
    MAIL_CSP, ParseProblem, RenderOptions, check_link, parse, preview_from_html, render_html,
    render_text, sanitize_filename,
};

fn rounds() -> usize {
    std::env::var("MAIL_CONTENT_FUZZ_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

const MIME_TOKENS: &[&[u8]] = &[
    b"\r\n",
    b"\r\n\r\n",
    b"\n",
    b"--",
    b"--b1--",
    b"\r\n--b1\r\n",
    b"boundary=\"b1\"",
    b"Content-Type: multipart/mixed; boundary=\"b1\"\r\n",
    b"Content-Type: text/html\r\n",
    b"Content-Type: message/rfc822\r\n",
    b"Content-Transfer-Encoding: base64\r\n",
    b"Content-Transfer-Encoding: quoted-printable\r\n",
    b"=?utf-8?b?",
    b"=?iso-2022-jp?q?",
    b"?=",
    b"=\r\n",
    b"=C3",
    b"=XX",
    b"charset=utf-7",
    b"charset=\"utf-16\"",
    b"filename*0*=",
    b"filename*=utf-8''%00",
    b"\x00",
    b"\xff",
    b"\xc3",
    b"\x1b$B",
    b"<",
    b">",
    b"\"",
    b";",
    b"<script>",
    b"<img src=x onerror=alert(1)>",
    b"<svg onload=alert(1)>",
    b"<a href=\"",
    b"javascript:",
    b"cid:",
    b"</style>",
    b"<style>",
    b"@import",
    b"url(",
    b"<math><mtext>",
];

fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut data = input.to_vec();
    for _ in 0..1 + rng.below(8) {
        let len = data.len();
        match rng.below(6) {
            0 if len > 0 => {
                let i = rng.below(len);
                data[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(len + 1);
                let token = rng.pick(MIME_TOKENS);
                data.splice(i..i, token.iter().copied());
            }
            2 if len > 0 => {
                let start = rng.below(len);
                let end = (start + rng.below(64)).min(len);
                data.drain(start..end);
            }
            3 if len > 0 => {
                let start = rng.below(len);
                let end = (start + rng.below(256)).min(len);
                let chunk: Vec<u8> = data[start..end].to_vec();
                let at = rng.below(data.len() + 1);
                data.splice(at..at, chunk);
            }
            4 if len > 0 => data.truncate(rng.below(len)),
            _ => {
                let i = rng.below(len + 1);
                data.insert(i, rng.next() as u8);
            }
        }
    }
    data
}

fn check_message(raw: &[u8], what: &str) -> usize {
    let message = parse(raw);
    assert!(
        !message.problems.contains(&ParseProblem::Internal),
        "{what}: parser panicked"
    );
    assert!(message.preview.chars().count() <= mail_content::PREVIEW_CHARS);
    let _ = render_text(&message.display_text());
    for attachment in &message.attachments {
        assert!(
            common::harmless_file_name_problems(&attachment.file_name).is_empty(),
            "{what}: {:?}",
            attachment.file_name
        );
    }
    let mut documents = 0;
    if let Some(html) = &message.html {
        let rendered = render_html(
            html,
            &RenderOptions {
                inline: message.inline_images(),
                ..RenderOptions::new(common::token())
            },
        );
        let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
        assert!(found.is_empty(), "{what}: {found:?}");
        documents += 1;
    }
    documents
}

#[test]
fn mutated_mails_never_panic_and_stay_clean() {
    let mut seeds: Vec<Vec<u8>> = eml_files("seed")
        .into_iter()
        .chain(eml_files("hostile"))
        .map(|p| fs::read(p).expect("read fixture"))
        .filter(|raw| raw.len() < 64 * 1024)
        .collect();
    seeds.push(Vec::new());
    let mut rng = Rng::new(0x5EED_2026_1122);
    let mut cases = 0;
    let mut documents = 0;
    for round in 0..rounds() {
        for (i, seed) in seeds.iter().enumerate() {
            for j in 0..12 {
                let raw = mutate(&mut rng, seed);
                documents += check_message(&raw, &format!("round {round}, seed {i}, mutation {j}"));
                cases += 1;
            }
        }
    }
    eprintln!("mutated mails: {cases} cases, {documents} HTML documents sanitised");
}

const TAGS: &[&str] = &[
    "a",
    "img",
    "div",
    "p",
    "table",
    "tr",
    "td",
    "style",
    "script",
    "svg",
    "math",
    "mtext",
    "mglyph",
    "iframe",
    "object",
    "embed",
    "form",
    "input",
    "button",
    "select",
    "option",
    "textarea",
    "noscript",
    "template",
    "title",
    "xmp",
    "plaintext",
    "noembed",
    "base",
    "link",
    "meta",
    "body",
    "html",
    "head",
    "frameset",
    "picture",
    "source",
    "video",
    "foreignObject",
    "annotation-xml",
    "desc",
    "font",
    "center",
    "map",
    "area",
    "details",
    "summary",
    "b",
    "i",
];
const ATTRS: &[&str] = &[
    "href",
    "src",
    "srcset",
    "style",
    "onload",
    "onerror",
    "onclick",
    "background",
    "target",
    "ping",
    "download",
    "rel",
    "action",
    "formaction",
    "srcdoc",
    "xlink:href",
    "class",
    "id",
    "title",
    "data",
    "poster",
    "usemap",
    "dir",
    "encoding",
    "http-equiv",
    "content",
    "alt",
];
const VALUES: &[&str] = &[
    "javascript:alert(1)",
    "JaVaScRiPt:alert(1)",
    "jav&#x09;ascript:x",
    "http://127.0.0.1:18999/x.png",
    "//127.0.0.1:18999/p.gif",
    "cid:logo",
    "data:image/svg+xml,<svg onload=alert(1)>",
    "data:text/html,<script>alert(1)</script>",
    "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
    "#x",
    "/index.html",
    "background:url(http://127.0.0.1:18999/a.png)",
    "width:expression(alert(1))",
    "\\75 rl(http://x)",
    "</style><script>alert(1)</script>",
    "text/html",
    "\"><img src=x onerror=alert(1)>",
    "a.png 1x, http://127.0.0.1:18999/b.png 2x",
    "0;url=http://127.0.0.1:18999/",
    "_blank",
];
const TEXTS: &[&str] = &[
    "text",
    "<!--",
    "-->",
    "<![CDATA[",
    "]]>",
    "&lt;",
    "&amp;",
    "&#0;",
    "\u{0}",
    "\u{202e}",
    "</",
    "<",
    ">",
    "\"",
    "'",
    "p{background:url(http://127.0.0.1:18999/c.png)}",
    "@import 'x';",
];

fn tag_soup(rng: &mut Rng) -> String {
    let mut out = String::new();
    for _ in 0..1 + rng.below(40) {
        match rng.below(5) {
            0 | 1 => {
                out.push('<');
                out.push_str(rng.pick(TAGS));
                for _ in 0..rng.below(4) {
                    out.push(' ');
                    out.push_str(rng.pick(ATTRS));
                    match rng.below(3) {
                        0 => {}
                        1 => {
                            out.push_str("=\"");
                            out.push_str(rng.pick(VALUES));
                            out.push('"');
                        }
                        _ => {
                            out.push('=');
                            out.push_str(&rng.pick(VALUES).replace(' ', ""));
                        }
                    }
                }
                out.push('>');
            }
            2 => {
                out.push_str("</");
                out.push_str(rng.pick(TAGS));
                out.push('>');
            }
            _ => out.push_str(rng.pick(TEXTS)),
        }
    }
    out
}

#[test]
fn random_tag_soup_is_sanitised() {
    let mut rng = Rng::new(0x7A65_0001);
    let cases = 3000 * rounds();
    for case in 0..cases {
        let html = tag_soup(&mut rng);
        let rendered = render_html(&html, &RenderOptions::new(common::token()));
        assert_eq!(
            rendered.fallback, None,
            "case {case}: self-check fell back for {html:?}"
        );
        let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
        assert!(
            found.is_empty(),
            "case {case}: {found:?}\ninput: {html:?}\noutput: {}",
            rendered.srcdoc
        );
        let _ = preview_from_html(&html);
        for link in &rendered.links {
            let check = check_link(&link.text, &link.href);
            if let Some(url) = check.open_url {
                assert!(
                    url.starts_with("http://")
                        || url.starts_with("https://")
                        || url.starts_with("mailto:")
                );
            }
        }
    }
    eprintln!("tag soup: {cases} documents, no violation");
}

const CSS_TOKENS: &[&str] = &[
    "a",
    "p",
    ".x",
    "#y",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ";",
    ":",
    ",",
    " ",
    "\n",
    "@media",
    "@import",
    "@font-face",
    "@supports",
    "url(",
    "url(\"",
    "\"",
    "'",
    "http://127.0.0.1:18999/a",
    "cid:x",
    "data:image/png;base64,AAAA",
    "expression(",
    "behavior",
    "-moz-binding",
    "\\",
    "\\75 ",
    "\\3c ",
    "/*",
    "*/",
    "<!--",
    "-->",
    "</style>",
    "image-set(",
    "var(--x)",
    "--x",
    "!important",
    "calc(",
    "rgb(",
    "1px",
    "50%",
    "color",
    "background",
    "red",
    "\u{0}",
    "</",
];

#[test]
fn random_css_is_sanitised() {
    let mut rng = Rng::new(0xC55_2026);
    let cases = 3000 * rounds();
    for case in 0..cases {
        let mut css = String::new();
        for _ in 0..1 + rng.below(30) {
            css.push_str(rng.pick(CSS_TOKENS));
        }
        let escaped = css.replace('&', "&amp;").replace('"', "&quot;");
        let html = format!("<style>{css}</style><p style=\"{escaped}\">x</p>");
        let rendered = render_html(&html, &RenderOptions::new(common::token()));
        let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
        assert!(
            found.is_empty(),
            "case {case}: {found:?}\ncss: {css:?}\noutput: {}",
            rendered.srcdoc
        );
    }
    eprintln!("css: {cases} stylesheets and style attributes, no violation");
}

const NAME_PARTS: &[&str] = &[
    "a",
    "report",
    ".",
    "..",
    "/",
    "\\",
    ":",
    "C:",
    "CON",
    "aux",
    "nul",
    "COM1",
    "lpt9",
    "¹",
    ".txt",
    ".exe",
    " ",
    "\t",
    "\u{0}",
    "\u{7}",
    "\u{202e}",
    "\u{2066}",
    "\u{200b}",
    "\u{feff}",
    "\u{0301}",
    "e",
    "ä",
    "😀",
    "<",
    ">",
    "|",
    "?",
    "*",
    "\"",
    "$",
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
];

#[test]
fn random_file_names_become_harmless_and_stable() {
    let mut rng = Rng::new(0xF11E_2026);
    let cases = 20_000 * rounds();
    for _ in 0..cases {
        let mut name = String::new();
        for _ in 0..rng.below(24) {
            name.push_str(rng.pick(NAME_PARTS));
        }
        let clean = sanitize_filename(&name);
        let problems = common::harmless_file_name_problems(&clean);
        assert!(problems.is_empty(), "{name:?} -> {clean:?}: {problems:?}");
        assert_eq!(
            sanitize_filename(&clean),
            clean,
            "not idempotent for {name:?}"
        );
    }
    eprintln!("file names: {cases} random names, all harmless and stable");
}

#[test]
fn random_links_are_checked_without_panic() {
    let parts = [
        "http://",
        "https://",
        "mailto:",
        "javascript:",
        "data:",
        "xn--",
        "www.",
        "@",
        ".",
        "/",
        "?",
        "#",
        "%",
        "%00",
        "[",
        "]",
        ":",
        "80",
        "bank",
        "example",
        "com",
        "аррӏе",
        "ä",
        " ",
    ];
    let mut rng = Rng::new(0x11A4_2026);
    for _ in 0..20_000 * rounds() {
        let mut href = String::new();
        let mut text = String::new();
        for _ in 0..rng.below(10) {
            href.push_str(rng.pick(&parts));
            text.push_str(rng.pick(&parts));
        }
        let check = check_link(&text, &href);
        if let Some(url) = check.open_url {
            assert!(
                url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("mailto:"),
                "{href:?} -> {url}"
            );
        }
    }
}
