//! Fuzz-like tests with a fixed seed: mutated corpus mails, random tag soup, random CSS,
//! random file names, mutated attachments for the preview decoders (images, PDF, text), the
//! composer's HTML cleaners, MIME structure bombs and the HTML guard against the real parser. Nothing may panic, the sanitiser output must pass the independent
//! checker for every input, every image in it must be one the re-encoder wrote (finding C-4),
//! sanitised file names must be harmless and stable, and previews may only ever be re-encoded
//! PNG/JPEG or cleaned text within their limits.
//!
//! Campaign controls (stage 15, `docs/SECURITY_MODEL.md`), all optional:
//! - `MAIL_CONTENT_FUZZ_ROUNDS=<n>` multiplies the number of cases (default 1).
//! - `MAIL_CONTENT_FUZZ_SEED=<n>` varies every generator's fixed seed: a different stream of
//!   inputs, reproducible with the same value (default 0, the seeds of the regular test run).
//! - `MAIL_CONTENT_FUZZ_SECONDS=<n>` keeps each test running whole rounds until that many seconds
//!   have passed (a time-boxed campaign; the rounds are then a minimum).
//!
//! A failure names the round and case; the same seed replays it.

mod common;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use common::{Mode, Rng, eml_files, violations};
use mail_content::{
    MAIL_CSP, ParseProblem, RenderOptions, attachment_bytes, calendar_parts, check_link,
    clean_pasted_html, html_to_text, html_to_visible_text, mail_html, parse, parse_mailto,
    parse_report, preview_from_html, preview_kind, render_html, render_preview, render_text,
    sanitize_filename, strip_hidden_chars,
};
use mail_model::{
    AttachmentPreview, MAX_PREVIEW_EDGE, MAX_PREVIEW_TEXT_BYTES, MailScheme, PreviewError,
    PreviewKind,
};

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

fn rounds() -> usize {
    env_number("MAIL_CONTENT_FUZZ_ROUNDS")
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(1)
}

/// The generator for a test: its fixed seed, varied by `MAIL_CONTENT_FUZZ_SEED`.
fn seeded(base: u64) -> Rng {
    let extra = env_number("MAIL_CONTENT_FUZZ_SEED").unwrap_or(0);
    Rng::new(base ^ extra.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Rounds of one test: at least [`rounds`], and with `MAIL_CONTENT_FUZZ_SECONDS` more until the
/// time is up.
struct Campaign {
    start: Instant,
    budget: Option<Duration>,
    rounds: usize,
}

impl Campaign {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            budget: env_number("MAIL_CONTENT_FUZZ_SECONDS").map(Duration::from_secs),
            rounds: rounds(),
        }
    }

    fn more(&self, round: usize) -> bool {
        round < self.rounds || self.budget.is_some_and(|b| self.start.elapsed() < b)
    }
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
    // What the MCP server and the built-in AI get (decision 0021), and the other readers of raw
    // mail: calendar invitations, returned read receipts.
    let visible = message.visible_text();
    assert!(visible.text.len() <= mail_content::MAX_TEXT_BYTES, "{what}");
    let _ = strip_hidden_chars(&visible.text);
    let _ = calendar_parts(raw);
    let _ = parse_report(raw);
    for attachment in &message.attachments {
        assert!(
            common::harmless_file_name_problems(&attachment.file_name).is_empty(),
            "{what}: {:?}",
            attachment.file_name
        );
        // Only the cheap text preview here; the decoders have their own tests below.
        if preview_kind(&attachment.file_name, &attachment.mime) == Some(PreviewKind::Text)
            && let Some(bytes) = attachment_bytes(raw, attachment.part_id)
        {
            check_preview(PreviewKind::Text, &bytes, 0, what);
        }
    }
    let mut documents = 0;
    if let Some(html) = &message.html {
        // Stage 21: the dark theme's colour pass runs on the same output and must keep it clean.
        for scheme in [MailScheme::Light, MailScheme::Dark] {
            let (rendered, written) = common::render_reencoded(
                html,
                &RenderOptions {
                    inline: message.inline_images(),
                    scheme,
                    ..RenderOptions::new(common::token())
                },
            );
            let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
            assert!(found.is_empty(), "{what} ({scheme:?}): {found:?}");
            let foreign = common::foreign_images(&rendered.srcdoc, &written);
            assert!(foreign.is_empty(), "{what}: not re-encoded: {foreign:?}");
            documents += 1;
        }
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
    let mut rng = seeded(0x5EED_2026_1122);
    let campaign = Campaign::new();
    let mut cases = 0;
    let mut documents = 0;
    let mut round = 0;
    while campaign.more(round) {
        for (i, seed) in seeds.iter().enumerate() {
            for j in 0..12 {
                let raw = mutate(&mut rng, seed);
                documents += check_message(&raw, &format!("round {round}, seed {i}, mutation {j}"));
                cases += 1;
            }
        }
        round += 1;
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
    // A real PNG: shown only as the re-encoder's output (C-4).
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
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
    let mut rng = seeded(0x7A65_0001);
    let campaign = Campaign::new();
    let mut cases = 0;
    while campaign.more(cases / 3000) {
        let case = cases;
        cases += 1;
        let html = tag_soup(&mut rng);
        let (rendered, written) =
            common::render_reencoded(&html, &RenderOptions::new(common::token()));
        let foreign = common::foreign_images(&rendered.srcdoc, &written);
        assert!(
            foreign.is_empty(),
            "case {case}: not re-encoded: {foreign:?}"
        );
        // The guard may refuse SVG/MathML soup it cannot model (shown as text, finding C-1);
        // the sanitiser's self-check must never fail.
        assert_ne!(
            rendered.fallback,
            Some(mail_content::Fallback::SelfCheckFailed),
            "case {case}: self-check fell back for {html:?}"
        );
        let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
        assert!(
            found.is_empty(),
            "case {case}: {found:?}\ninput: {html:?}\noutput: {}",
            rendered.srcdoc
        );
        let _ = preview_from_html(&html);
        let _ = html_to_text(&html);
        let visible = html_to_visible_text(&html);
        assert!(visible.text.len() <= mail_content::MAX_TEXT_BYTES);
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
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
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
    let mut rng = seeded(0xC55_2026);
    let campaign = Campaign::new();
    let mut cases = 0;
    while campaign.more(cases / 3000) {
        let case = cases;
        cases += 1;
        let mut css = String::new();
        for _ in 0..1 + rng.below(30) {
            css.push_str(rng.pick(CSS_TOKENS));
        }
        let escaped = css.replace('&', "&amp;").replace('"', "&quot;");
        let html = format!("<style>{css}</style><p style=\"{escaped}\">x</p>");
        let (rendered, written) =
            common::render_reencoded(&html, &RenderOptions::new(common::token()));
        let foreign = common::foreign_images(&rendered.srcdoc, &written);
        assert!(
            foreign.is_empty(),
            "case {case}: not re-encoded: {foreign:?}"
        );
        let found = violations(&rendered.srcdoc, Mode::Output, MAIL_CSP);
        assert!(
            found.is_empty(),
            "case {case}: {found:?}\ncss: {css:?}\noutput: {}",
            rendered.srcdoc
        );
        // Hidden-text rules read the same CSS for what goes to a model.
        let _ = html_to_visible_text(&html);
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
    let mut rng = seeded(0xF11E_2026);
    let campaign = Campaign::new();
    let mut cases = 0;
    while campaign.more(cases / 20_000) {
        cases += 1;
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
    let mut rng = seeded(0x11A4_2026);
    let campaign = Campaign::new();
    let mut cases = 0;
    while campaign.more(cases / 20_000) {
        cases += 1;
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
    eprintln!("links: {cases} random links checked");
}

// ---------------------------------------------------------------------------------------------
// Composer HTML (stage 10): outgoing mail HTML and pasted HTML go through the composer's own
// allowlist. Whatever goes in, nothing active, styled from outside or loading from the network
// may come out.

/// Element names the composer must never emit.
const COMPOSER_FORBIDDEN: &[&str] = &[
    "script",
    "style",
    "iframe",
    "frame",
    "frameset",
    "object",
    "embed",
    "applet",
    "form",
    "input",
    "button",
    "textarea",
    "select",
    "link",
    "meta",
    "base",
    "svg",
    "math",
    "video",
    "audio",
    "source",
    "picture",
    "track",
    "template",
    "noscript",
    "xmp",
    "plaintext",
    "title",
    "portal",
    "dialog",
];

/// Problems of composer output (`images`: whether `cid:` images may stay).
fn composer_violations(html: &str, images: bool) -> Vec<String> {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{NodeData, RcDom};

    let dom =
        html5ever::parse_document(RcDom::default(), html5ever::ParseOpts::default()).one(html);
    let mut found = Vec::new();
    let mut stack = vec![dom.document.clone()];
    while let Some(node) = stack.pop() {
        if let NodeData::Element { name, attrs, .. } = &node.data {
            let tag = name.local.to_string();
            // The document's own charset declaration (`mail_html` writes a whole document).
            let charset = tag == "meta"
                && attrs.borrow().iter().all(|attr| {
                    let value = attr.value.to_ascii_lowercase();
                    match &*attr.name.local {
                        "http-equiv" => value == "content-type",
                        "content" => value == "text/html; charset=utf-8",
                        "charset" => value == "utf-8",
                        _ => false,
                    }
                });
            if COMPOSER_FORBIDDEN.contains(&tag.as_str()) && !charset {
                found.push(format!("element <{tag}>"));
            }
            if tag == "img" && !images {
                found.push("an image in pasted content".into());
            }
            for attr in attrs.borrow().iter() {
                let key = attr.name.local.to_string().to_ascii_lowercase();
                let value: String = attr
                    .value
                    .chars()
                    .filter(|c| !c.is_whitespace() && !c.is_control())
                    .collect::<String>()
                    .to_ascii_lowercase();
                if charset {
                    continue;
                }
                let bad = key.starts_with("on")
                    || matches!(
                        key.as_str(),
                        "srcset"
                            | "background"
                            | "action"
                            | "formaction"
                            | "ping"
                            | "poster"
                            | "srcdoc"
                            | "data"
                            | "xmlns"
                            | "http-equiv"
                    )
                    || (key == "href"
                        && !(value.starts_with("http:")
                            || value.starts_with("https:")
                            || value.starts_with("mailto:")))
                    || (key == "src" && !(images && tag == "img" && value.starts_with("cid:")))
                    || (key == "style" && (value.contains("url(") || value.contains("@import")));
                if bad {
                    found.push(format!("<{tag} {key}={:?}>", &*attr.value));
                }
            }
        }
        stack.extend(node.children.borrow().iter().cloned());
    }
    found
}

#[test]
fn composer_html_cleaners_never_let_active_content_through() {
    let mut rng = seeded(0xC0_4E05_2026);
    let campaign = Campaign::new();
    let mut cases = 0;
    while campaign.more(cases / 2000) {
        let case = cases;
        cases += 1;
        let html = tag_soup(&mut rng);
        let outgoing = mail_html(&html);
        let found = composer_violations(&outgoing, true);
        assert!(
            found.is_empty(),
            "case {case}: mail_html {found:?}\ninput: {html:?}\noutput: {outgoing}"
        );
        if let Ok(pasted) = clean_pasted_html(&html) {
            let found = composer_violations(&pasted.html, false);
            assert!(
                found.is_empty(),
                "case {case}: clean_pasted_html {found:?}\ninput: {html:?}\noutput: {}",
                pasted.html
            );
        }
        let mut uri = String::from("mailto:");
        uri.push_str(&html.chars().take(200).collect::<String>());
        let _ = parse_mailto(&uri);
    }
    eprintln!("composer HTML: {cases} documents through mail_html and clean_pasted_html");
}

// ---------------------------------------------------------------------------------------------
// Attachment preview decoders (stage 16, decision 0036): the core decodes untrusted images and
// PDF and hands the web view only re-encoded PNG/JPEG or cleaned text.

thread_local! {
    /// Set while a preview decoder runs on this thread: its panics are counted, not printed.
    static IN_DECODER: Cell<bool> = const { Cell::new(false) };
}

/// Panics inside the decoders, by location. `render_preview` contains them (they become
/// `preview_damaged`, decision 0036), so they do not fail the test; a campaign reports them as
/// findings in the decoders.
static DECODER_PANICS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

fn count_decoder_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if IN_DECODER.with(Cell::get) {
                let at = info
                    .location()
                    .map(|l| format!("{}:{}", l.file(), l.line()))
                    .unwrap_or_default();
                let mut panics = DECODER_PANICS
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *panics.entry(at).or_default() += 1;
            } else {
                default(info);
            }
        }));
    });
}

fn report_decoder_panics(what: &str) {
    let panics = DECODER_PANICS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (at, count) in panics.iter() {
        eprintln!("{what}: contained decoder panic at {at} ({count}×)");
    }
}

/// Renders a preview and checks what the web view would get; returns how long it took and
/// whether there was a preview.
/// An image of a mail (C-4): a re-encoded PNG, JPEG or, for an animation, GIF within the edge
/// and the frame limit, or a typed error.
fn check_reencoded(bytes: &[u8], what: &str) -> (Duration, bool) {
    IN_DECODER.with(|flag| flag.set(true));
    let start = Instant::now();
    let result = mail_content::reencode_image(bytes);
    let took = start.elapsed();
    IN_DECODER.with(|flag| flag.set(false));
    let Ok(image) = result else {
        return (took, false);
    };
    let url = &image.data_url;
    assert!(
        url.starts_with("data:image/png;base64,iVBORw0KGgo")
            || url.starts_with("data:image/jpeg;base64,/9j/")
            || (image.frames > 1 && url.starts_with("data:image/gif;base64,R0lGODlh")),
        "{what}: {} frames, {}",
        image.frames,
        &url[..url.len().min(48)]
    );
    assert!(mail_content::is_reencoded_data_url(url), "{what}");
    assert!(
        (1..=MAX_PREVIEW_EDGE).contains(&image.width)
            && (1..=MAX_PREVIEW_EDGE).contains(&image.height)
            && (1..=mail_content::MAX_ANIMATION_FRAMES).contains(&image.frames),
        "{what}: {}×{}, {} frames",
        image.width,
        image.height,
        image.frames
    );
    (took, true)
}

fn check_preview(kind: PreviewKind, bytes: &[u8], page: u32, what: &str) -> (Duration, bool) {
    IN_DECODER.with(|flag| flag.set(true));
    let start = Instant::now();
    let result = render_preview(kind, bytes, page);
    let took = start.elapsed();
    IN_DECODER.with(|flag| flag.set(false));
    let edge = MAX_PREVIEW_EDGE + 1; // rounding of a scaled PDF page
    let shown = result.is_ok();
    match result {
        Ok(AttachmentPreview::Image {
            data_url,
            width,
            height,
            ..
        }) => {
            assert_eq!(kind, PreviewKind::Image, "{what}");
            assert!(
                data_url.starts_with("data:image/png;base64,iVBORw0KGgo")
                    || data_url.starts_with("data:image/jpeg;base64,/9j/"),
                "{what}: {}",
                &data_url[..data_url.len().min(48)]
            );
            assert!(
                (1..=MAX_PREVIEW_EDGE).contains(&width) && (1..=MAX_PREVIEW_EDGE).contains(&height),
                "{what}: {width}×{height}"
            );
        }
        Ok(AttachmentPreview::Pdf {
            data_url,
            width,
            height,
            page: shown,
            pages,
            ..
        }) => {
            assert_eq!(kind, PreviewKind::Pdf, "{what}");
            assert!(
                data_url.starts_with("data:image/png;base64,iVBORw0KGgo"),
                "{what}"
            );
            assert!(
                (1..=edge).contains(&width) && (1..=edge).contains(&height),
                "{what}: {width}×{height}"
            );
            assert!(
                shown == page && page < pages,
                "{what}: page {shown} of {pages}"
            );
        }
        Ok(AttachmentPreview::Text { text, .. }) => {
            assert_eq!(kind, PreviewKind::Text, "{what}");
            let limit = usize::try_from(MAX_PREVIEW_TEXT_BYTES).expect("usize");
            assert!(text.len() <= limit, "{what}: {} bytes", text.len());
            assert!(
                text.chars()
                    .all(|c| c == '\n' || c == '\t' || !c.is_control()),
                "{what}: control character"
            );
        }
        Err(
            PreviewError::Unsupported(_) | PreviewError::TooLarge(_) | PreviewError::Damaged(_),
        ) => {}
    }
    (took, shown)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Recomputes the CRC of every PNG chunk, so mutations reach the decoder behind the checksum.
fn fix_png_crcs(data: &mut [u8]) {
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return;
    }
    let mut at = 8;
    while at + 12 <= data.len() {
        let len = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let Some(end) = usize::try_from(len)
            .ok()
            .and_then(|len| (at + 8).checked_add(len))
        else {
            return;
        };
        if end + 4 > data.len() {
            return;
        }
        let crc = crc32(&data[at + 4..end]);
        data[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        at = end + 4;
    }
}

const INTERESTING: &[u32] = &[
    0,
    1,
    0x7F,
    0x80,
    0xFF,
    0x100,
    0x7FFF,
    0x8000,
    0xFFFF,
    0x1_0000,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
];

/// Byte-level mutations with format tokens and interesting integers (sizes, counts, offsets).
fn mutate_binary(rng: &mut Rng, input: &[u8], tokens: &[&[u8]]) -> Vec<u8> {
    let mut data = input.to_vec();
    for _ in 0..1 + rng.below(6) {
        let len = data.len();
        match rng.below(7) {
            0 if len > 0 => {
                let i = rng.below(len);
                data[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(len + 1);
                let token = rng.pick(tokens);
                data.splice(i..i, token.iter().copied());
            }
            2 if len > 4 => {
                let i = rng.below(len - 4);
                let value = *rng.pick(INTERESTING);
                let bytes = if rng.below(2) == 0 {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                };
                let width = [1, 2, 4][rng.below(3)];
                data[i..i + width].copy_from_slice(&bytes[4 - width..]);
            }
            3 if len > 0 => {
                let start = rng.below(len);
                let end = (start + rng.below(64)).min(len);
                data.drain(start..end);
            }
            4 if len > 0 => {
                let start = rng.below(len);
                let end = (start + rng.below(256)).min(len);
                let chunk: Vec<u8> = data[start..end].to_vec();
                let at = rng.below(data.len() + 1);
                data.splice(at..at, chunk);
            }
            5 if len > 0 => data.truncate(rng.below(len)),
            _ => {
                let i = rng.below(len + 1);
                data.insert(i, rng.next() as u8);
            }
        }
    }
    data
}

const IMAGE_TOKENS: &[&[u8]] = &[
    b"IHDR",
    b"IDAT",
    b"IEND",
    b"PLTE",
    b"tRNS",
    b"acTL",
    b"fcTL",
    b"fdAT",
    b"eXIf",
    b"iCCP",
    b"\xFF\xD8",
    b"\xFF\xC0",
    b"\xFF\xC2",
    b"\xFF\xC4",
    b"\xFF\xDA",
    b"\xFF\xDB",
    b"\xFF\xDD",
    b"\xFF\xE1",
    b"\xFF\xD9",
    b"Exif\0\0MM\0*\0\0\0\x08",
    b"Exif\0\0II*\0\x08\0\0\0",
    b"\x01\x12\0\x03\0\0\0\x01\0\x06\0\0",
    b"GIF89a",
    b"NETSCAPE2.0",
    b"\x21\xF9\x04",
    b"\x2C",
    b"RIFF",
    b"WEBP",
    b"VP8 ",
    b"VP8L",
    b"VP8X",
    b"ANIM",
    b"ANMF",
    b"ALPH",
    b"\xFF\xFF\xFF\xFF",
    b"\0\0\0\0",
];

fn image_seeds() -> Vec<Vec<u8>> {
    use image::codecs::gif::GifEncoder;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::codecs::webp::WebPEncoder;
    use image::{Delay, Frame, ImageBuffer, Luma, Rgb, Rgba};

    let rgba = ImageBuffer::from_fn(12, 9, |x, y| {
        Rgba([
            (x * 20) as u8,
            (y * 25) as u8,
            128,
            if x > 6 { 255 } else { (y * 28) as u8 },
        ])
    });
    let rgb = ImageBuffer::from_fn(16, 8, |x, y| Rgb([(x * 15) as u8, (y * 30) as u8, 64]));
    let gray = ImageBuffer::from_fn(8, 8, |x, y| Luma([((x + y) * 16) as u8]));
    let deep = ImageBuffer::from_fn(5, 4, |x, y| {
        Rgba([(x * 9000) as u16, (y * 9000) as u16, 7, 65535])
    });
    let mut seeds = Vec::new();
    let mut out = Vec::new();
    rgba.write_with_encoder(PngEncoder::new(&mut out))
        .expect("png");
    seeds.push(std::mem::take(&mut out));
    gray.write_with_encoder(PngEncoder::new(&mut out))
        .expect("gray png");
    seeds.push(std::mem::take(&mut out));
    deep.write_with_encoder(PngEncoder::new(&mut out))
        .expect("16-bit png");
    seeds.push(std::mem::take(&mut out));
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut out, 80))
        .expect("jpeg");
    // An EXIF block that turns the photo (orientation 6) right after the start marker.
    let exif =
        b"\xFF\xE1\x00\x22Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
    let mut rotated = out[..2].to_vec();
    rotated.extend_from_slice(exif);
    rotated.extend_from_slice(&out[2..]);
    seeds.push(std::mem::take(&mut out));
    seeds.push(rotated);
    gray.write_with_encoder(JpegEncoder::new_with_quality(&mut out, 60))
        .expect("gray jpeg");
    seeds.push(std::mem::take(&mut out));
    {
        let mut gif = GifEncoder::new(&mut out);
        gif.set_repeat(image::codecs::gif::Repeat::Infinite)
            .expect("repeat");
        gif.encode_frames(vec![
            Frame::new(rgba.clone()),
            Frame::from_parts(rgba.clone(), 3, 2, Delay::from_numer_denom_ms(100, 1)),
        ])
        .expect("gif");
    }
    seeds.push(std::mem::take(&mut out));
    rgba.write_with_encoder(WebPEncoder::new_lossless(&mut out))
        .expect("webp");
    seeds.push(std::mem::take(&mut out));
    seeds
}

#[test]
fn mutated_images_are_refused_or_reencoded() {
    count_decoder_panics();
    let seeds = image_seeds();
    let mut rng = seeded(0x1A6E_2026);
    let campaign = Campaign::new();
    let (mut cases, mut shown, mut in_mail, mut round) = (0, 0, 0, 0);
    let mut slowest = (Duration::ZERO, String::new());
    while campaign.more(round) {
        for (i, seed) in seeds.iter().enumerate() {
            for j in 0..40 {
                let mut data = mutate_binary(&mut rng, seed, IMAGE_TOKENS);
                if rng.below(4) != 0 {
                    fix_png_crcs(&mut data);
                }
                let what = format!("round {round}, image seed {i}, mutation {j}");
                let (took, ok) = check_preview(PreviewKind::Image, &data, 0, &what);
                shown += usize::from(ok);
                // The same bytes as an image inside a mail (C-4: animations stay animated).
                let (took_in_mail, ok) = check_reencoded(&data, &what);
                in_mail += usize::from(ok);
                if took.max(took_in_mail) > slowest.0 {
                    slowest = (took.max(took_in_mail), what);
                }
                cases += 1;
            }
        }
        round += 1;
    }
    report_decoder_panics("images");
    eprintln!(
        "images: {cases} mutated images, {shown} re-encoded as previews, {in_mail} as mail images, slowest {:?} ({})",
        slowest.0, slowest.1
    );
}

const PDF_TOKENS: &[&[u8]] = &[
    b"<<",
    b">>",
    b"[",
    b"]",
    b"(",
    b")",
    b" 0 R",
    b" 3 0 R",
    b"obj\n",
    b"endobj\n",
    b"stream\n",
    b"\nendstream",
    b"/Length 99999999",
    b"/Length -1",
    b"/Type /Page",
    b"/Type /Pages",
    b"/Kids [3 0 R 3 0 R]",
    b"/Parent 2 0 R",
    b"/Count 2000000000",
    b"/MediaBox [0 0 99999 99999]",
    b"/MediaBox [0 0 1e30 1]",
    b"/MediaBox [0 0 -5 0]",
    b"/Rotate 90",
    b"/UserUnit 75000",
    b"/Filter /FlateDecode",
    b"/Filter /LZWDecode",
    b"/Filter /DCTDecode",
    b"/Filter /JBIG2Decode",
    b"/Filter /CCITTFaxDecode",
    b"/Filter /ASCIIHexDecode",
    b"/Filter /ASCII85Decode",
    b"/Filter /RunLengthDecode",
    b"/DecodeParms << /Predictor 15 /Columns 99999 >>",
    b"/Width 65535 /Height 65535",
    b"/BitsPerComponent 16",
    b"/ColorSpace /DeviceCMYK",
    b"/ColorSpace [/Indexed /DeviceRGB 255 <00>]",
    b"/Encrypt 1 0 R",
    b"/Prev 0",
    b"xref\n",
    b"trailer\n",
    b"startxref\n",
    b"%%EOF",
    b" q ",
    b" Q ",
    b" 1e38 0 0 1e38 0 0 cm ",
    b" /X Do ",
    b" /Im1 Do ",
    b" /Sh1 sh ",
    b" BT /F1 1e9 Tf (A) Tj ET ",
    b" 0 0 1e9 1e9 re W n ",
    b" 10 10 m 1e9 1e9 l S ",
    b" [1e-9] 0 d ",
    b"1e308",
    b"-2147483648",
    b"4294967296",
];

/// A PDF with a correct cross-reference table from numbered object bodies (1 is the catalog).
fn pdf(objects: &[String]) -> Vec<u8> {
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// One page (object 3) with `content` (object 4) and `resources`; `extra` are objects 5….
fn page_pdf(content: &str, resources: &str, extra: &[String]) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /OpenAction << /S /JavaScript /JS (app.alert(1)) >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R \
             /Resources << {resources} >> >>"
        ),
        stream("", content),
    ];
    objects.extend_from_slice(extra);
    pdf(&objects)
}

fn pdf_seeds() -> Vec<Vec<u8>> {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned();
    let image = format!(
        "<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Length 12 >>\nstream\n{}\nendstream",
        "\u{1}\u{2}\u{3}\u{4}\u{5}\u{6}\u{7}\u{8}\u{9}\u{a}\u{b}\u{c}"
    );
    let shading = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
                   /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>"
        .to_owned();
    vec![
        page_pdf("0 0 1 rg 10 10 50 50 re f", "", &[]),
        page_pdf(
            "BT /F1 24 Tf 10 50 Td (Hello, PDF) Tj ET",
            "/Font << /F1 5 0 R >>",
            &[font],
        ),
        page_pdf(
            "q 100 0 0 80 10 10 cm /Im1 Do Q",
            "/XObject << /Im1 5 0 R >>",
            &[image],
        ),
        page_pdf("/Sh1 sh", "/Shading << /Sh1 5 0 R >>", &[shading]),
        // A form that draws itself: the renderer must stop, not recurse until the stack ends.
        page_pdf(
            "/X Do",
            "/XObject << /X 5 0 R >>",
            &[stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100] \
                 /Resources << /XObject << /X 5 0 R >> >>",
                "/X Do",
            )],
        ),
        // Deeply nested graphics states.
        page_pdf(
            &format!("{}0 1 0 rg 5 5 20 20 re f", "q ".repeat(5000)),
            "",
            &[],
        ),
        // A page tree that contains itself.
        pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [2 0 R 3 0 R] /Count 2 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 50 50] >>".to_owned(),
        ]),
    ]
}

#[test]
fn hostile_pdf_structures_are_contained() {
    count_decoder_panics();
    for (i, seed) in pdf_seeds().iter().enumerate() {
        check_preview(PreviewKind::Pdf, seed, 0, &format!("pdf seed {i}"));
    }
    report_decoder_panics("pdf seeds");
}

#[test]
fn mutated_pdfs_are_refused_or_rasterised() {
    count_decoder_panics();
    let seeds = pdf_seeds();
    let mut rng = seeded(0x0D0C_2026);
    let campaign = Campaign::new();
    let (mut cases, mut rendered, mut round) = (0, 0, 0);
    let mut slowest = (Duration::ZERO, String::new());
    while campaign.more(round) {
        for (i, seed) in seeds.iter().enumerate() {
            for j in 0..6 {
                let data = mutate_binary(&mut rng, seed, PDF_TOKENS);
                let what = format!("round {round}, pdf seed {i}, mutation {j}");
                let page = u32::from(rng.below(4) == 0);
                let (took, ok) = check_preview(PreviewKind::Pdf, &data, page, &what);
                rendered += usize::from(ok);
                if took > slowest.0 {
                    slowest = (took, what);
                }
                cases += 1;
            }
        }
        round += 1;
    }
    report_decoder_panics("pdf");
    eprintln!(
        "pdf: {cases} mutated documents, {rendered} rendered, slowest {:?} ({})",
        slowest.0, slowest.1
    );
}

#[test]
fn random_bytes_as_text_previews_stay_clean_and_bounded() {
    let mut rng = seeded(0x7E47_2026);
    let campaign = Campaign::new();
    let tokens: &[&[u8]] = &[
        b"\xEF\xBB\xBF",
        b"\xFF\xFE",
        b"\xFE\xFF",
        b"\xD8\x00",
        b"\x00\xDC",
        b"\r",
        b"\n",
        b"\r\n",
        b"\x1B[31m",
        b"\xE2\x80\xAE",
        b"\xC3",
        b"\x00",
        "äöü€😀".as_bytes(),
    ];
    let mut cases = 0;
    while campaign.more(cases / 2000) {
        cases += 1;
        let mut data = Vec::new();
        for _ in 0..rng.below(64) {
            if rng.below(2) == 0 {
                data.extend_from_slice(rng.pick(tokens));
            } else {
                data.push(rng.next() as u8);
            }
        }
        check_preview(PreviewKind::Text, &data, 0, &format!("text case {cases}"));
    }
    eprintln!("text previews: {cases} random inputs");
}

// ---------------------------------------------------------------------------------------------
// Structure bombs: nesting and part counts far beyond any real mail. The parser's limits must
// end them on a normal thread stack (Tokio's blocking threads have 2 MiB) and in linear time.

fn nested_multipart(depth: usize) -> Vec<u8> {
    let mut out = String::from("From: a@example.org\r\nSubject: deep\r\nMIME-Version: 1.0\r\n");
    for level in 0..depth {
        out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"b{level}\"\r\n\r\n--b{level}\r\n"
        ));
    }
    out.push_str("Content-Type: text/html\r\n\r\n<p>bottom</p>\r\n");
    for level in (0..depth).rev() {
        out.push_str(&format!("--b{level}--\r\n"));
    }
    out.into_bytes()
}

fn nested_rfc822(depth: usize) -> Vec<u8> {
    let mut out = String::new();
    for _ in 0..depth {
        out.push_str("From: a@example.org\r\nContent-Type: message/rfc822\r\n\r\n");
    }
    out.push_str("From: b@example.org\r\nContent-Type: text/plain\r\n\r\nbottom\r\n");
    out.into_bytes()
}

fn many_parts(count: usize) -> Vec<u8> {
    let mut out = String::from(
        "From: a@example.org\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"x\"\r\n\r\n",
    );
    for i in 0..count {
        out.push_str(&format!(
            "--x\r\nContent-Type: image/png\r\nContent-ID: <i{i}>\r\nContent-Disposition: attachment; filename=\"a{i}.png\"\r\n\r\nAA==\r\n"
        ));
    }
    out.push_str("--x--\r\n");
    out.into_bytes()
}

#[test]
fn structure_bombs_end_within_the_limits() {
    for (what, raw) in [
        ("multipart nested 20,000 deep", nested_multipart(20_000)),
        ("message/rfc822 nested 20,000 deep", nested_rfc822(20_000)),
        ("100,000 parts", many_parts(100_000)),
    ] {
        // A thread with the stack of Tokio's blocking pool, as in the app.
        let result = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let start = Instant::now();
                check_message(&raw, what);
                start.elapsed()
            })
            .expect("thread")
            .join();
        let took = result.unwrap_or_else(|_| panic!("{what}: failed"));
        eprintln!("{what}: {took:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// The HTML guard against the real parser (stage 15 review, finding C-1). The guard is a linear
// pre-scan that must see at least the nesting html5ever builds; where its tokenizer disagrees
// with html5ever's (comments, attributes, raw text, foreign content), a mail could hide deep
// nesting behind the disagreement and make the parser quadratic again. Differential check:
// whenever `render_html` parses (no `TooComplex` fallback), the tree html5ever builds may not
// be deeper than the guard's limit.

/// `guard::MAX_DEPTH` in the crate.
const GUARD_MAX_DEPTH: usize = 1_500;

const DESYNC_TOKENS: &[&str] = &[
    "<!--",
    "<!-->",
    "<!--->",
    "-->",
    "--!>",
    "<!-- --!>",
    "<!-- -- >",
    "<a =\"",
    "<a ='",
    "<a b=\"",
    "<a b='",
    "<a b=c",
    "\"",
    "'",
    "<svg>",
    "<math>",
    "</svg>",
    "</math>",
    "<style>",
    "</style>",
    "<title>",
    "</title>",
    "<script>",
    "</script>",
    "<textarea>",
    "</textarea>",
    "<noscript>",
    "</noscript>",
    "<xmp>",
    "<iframe>",
    "<noembed>",
    "<noframes>",
    "<template>",
    "</template>",
    "<![CDATA[",
    "]]>",
    "<!DOCTYPE html>",
    "<!x",
    "<?x",
    "</ x>",
    "</",
    "<a/b>",
    "<b>",
    "<p>",
    "<table>",
    "<td>",
    "<select>",
    "<option>",
    "<foreignObject>",
    "<desc>",
    "<mi>",
    "<mtext>",
    "<annotation-xml encoding=\"text/html\">",
    "<font color=red>",
    "<font size=1>",
    "<font class=\"color\">",
    "<div>",
    "</div>",
    "<span>",
    "</span>",
    "</table>",
    "<li>",
    "</li>",
    "</td>",
    "</p>",
    "</br>",
    "</foreignObject>",
    "</title>",
    "<br/>",
    "<svg/>",
    ">",
    "<",
    "=",
    " ",
    "\n",
    "\u{0}",
];

/// Depth of the deepest element html5ever builds for `html` (independent DOM, iterative walk).
fn parsed_depth(html: &str) -> usize {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::RcDom;

    let dom =
        html5ever::parse_document(RcDom::default(), html5ever::ParseOpts::default()).one(html);
    let mut deepest = 0;
    let mut stack = vec![(dom.document.clone(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        for child in node.children.borrow().iter() {
            stack.push((child.clone(), depth + 1));
        }
    }
    deepest
}

#[test]
fn the_guard_sees_the_nesting_the_parser_builds() {
    let mut rng = seeded(0x6A4D_2026);
    let campaign = Campaign::new();
    let mut cases = 0;
    let mut parsed = 0;
    while campaign.more(cases / 40) {
        let case = cases;
        cases += 1;
        let mut html = String::new();
        for _ in 0..rng.below(7) {
            html.push_str(rng.pick(DESYNC_TOKENS));
        }
        let payload = if rng.below(2) == 0 {
            "<div>"
        } else {
            "<span><i>"
        };
        html.push_str(&payload.repeat(GUARD_MAX_DEPTH + 200 + rng.below(600)));
        for _ in 0..rng.below(3) {
            html.push_str(rng.pick(DESYNC_TOKENS));
        }
        let rendered = render_html(&html, &RenderOptions::new(common::token()));
        if rendered.fallback.is_some() {
            continue;
        }
        parsed += 1;
        let depth = parsed_depth(&html);
        assert!(
            depth <= GUARD_MAX_DEPTH + 8,
            "case {case}: the guard let through a tree {depth} deep; prefix: {:?}",
            &html[..html.len().min(160)]
        );
    }
    eprintln!("guard vs parser: {cases} documents, {parsed} parsed, none deeper than the limit");
}

/// Replays one file through the preview decoders, for triage of a campaign finding or a sample:
/// `MAIL_CONTENT_PREVIEW_FILE=<path> [MAIL_CONTENT_PREVIEW_KIND=image|pdf|text]
/// cargo test -p mail-content --test fuzz -- --ignored replay_preview_file --nocapture`.
#[test]
#[ignore = "manual: needs MAIL_CONTENT_PREVIEW_FILE"]
fn replay_preview_file() {
    let Ok(path) = std::env::var("MAIL_CONTENT_PREVIEW_FILE") else {
        return;
    };
    let kind = match std::env::var("MAIL_CONTENT_PREVIEW_KIND").as_deref() {
        Ok("image") => PreviewKind::Image,
        Ok("text") => PreviewKind::Text,
        _ => PreviewKind::Pdf,
    };
    let bytes = fs::read(&path).expect("read the file");
    count_decoder_panics();
    let (took, shown) = check_preview(kind, &bytes, 0, &path);
    report_decoder_panics("replay");
    eprintln!("{path}: {kind:?}, preview {shown}, {took:?}");
}
