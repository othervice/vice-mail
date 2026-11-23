//! Shared test helpers: the corpus folders and an independent checker for sanitised HTML.
//!
//! The checker re-parses a document with `markup5ever_rcdom` (the DOM html5ever's own
//! conformance tests use, not the crate's DOM) and checks it structurally with its own lists,
//! written separately from the sanitiser's allowlist.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use html5ever::tendril::TendrilSink;
use html5ever::{ParseOpts, ns, parse_document};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

pub fn corpus_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testmail/corpus")
        .join(name)
}

/// All `.eml` files of a corpus folder, sorted by name.
pub fn eml_files(name: &str) -> Vec<PathBuf> {
    let dir = corpus_dir(name);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "eml"))
        .collect();
    files.sort();
    files
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Value of a header in the raw header block (unfolded), for the corpus' X-Corpus-* fields.
pub fn raw_header(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    let unfolded = head.replace("\r\n ", " ").replace("\r\n\t", " ");
    unfolded.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

/// Elements the output may contain inside `<body>`.
const BODY_ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "area",
    "article",
    "aside",
    "b",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "main",
    "map",
    "mark",
    "nav",
    "ol",
    "p",
    "picture",
    "pre",
    "q",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "section",
    "small",
    "source",
    "span",
    "strike",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "time",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];

/// Named explicitly, so a violation message says what slipped through.
const FORBIDDEN_ELEMENTS: &[&str] = &[
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
    "select",
    "textarea",
    "option",
    "meta",
    "base",
    "link",
    "svg",
    "math",
    "video",
    "audio",
    "track",
    "canvas",
    "template",
    "noscript",
    "portal",
    "fencedframe",
    "marquee",
    "bgsound",
    "isindex",
    "keygen",
    "title",
    "xmp",
    "plaintext",
    "noembed",
    "noframes",
    "dialog",
    "slot",
];

/// Attributes that must never appear (besides every `on*`).
const FORBIDDEN_ATTRIBUTES: &[&str] = &[
    "target",
    "ping",
    "download",
    "rel",
    "action",
    "formaction",
    "srcdoc",
    "xlink:href",
    "poster",
    "data",
    "codebase",
    "code",
    "lowsrc",
    "dynsrc",
    "http-equiv",
    "content",
    "autofocus",
    "contenteditable",
    "form",
    "cite",
    "longdesc",
    "referrerpolicy",
    "nonce",
    "integrity",
    "crossorigin",
    "is",
    "slot",
    "popover",
    "popovertarget",
    "srclang",
    "manifest",
    "xmlns",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Raw mail HTML (positive control: must have violations).
    Input,
    /// Sanitised document from `render_html`.
    Output,
}

/// The render token of the tests (32 lower-case hex digits).
pub const TOKEN: &str = "0123456789abcdeffedcba9876543210";

pub fn token() -> mail_model::RenderToken {
    mail_model::RenderToken::parse(TOKEN).expect("valid token")
}

/// Everything in `html` that a sanitised mail must not contain. Links must be placeholders of
/// the rendering [`TOKEN`].
pub fn violations(html: &str, mode: Mode, csp: &str) -> Vec<String> {
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html);
    let mut found = Vec::new();
    let mut stack: Vec<(Handle, Vec<String>)> = vec![(dom.document.clone(), Vec::new())];
    let mut head_elements: Vec<(String, Vec<(String, String)>)> = Vec::new();
    while let Some((node, path)) = stack.pop() {
        let mut child_path = path.clone();
        if let NodeData::Element { name, attrs, .. } = &node.data {
            let tag = name.local.to_string();
            let attrs: Vec<(String, String)> = attrs
                .borrow()
                .iter()
                .map(|a| {
                    let full = match &a.name.prefix {
                        Some(prefix) => format!("{prefix}:{}", a.name.local),
                        None => a.name.local.to_string(),
                    };
                    (full, a.value.to_string())
                })
                .collect();
            let in_head = path.last().is_some_and(|p| p == "head");
            let structural = matches!(tag.as_str(), "html" | "head" | "body");
            if name.ns != ns!(html) {
                found.push(format!("element in foreign namespace: {tag}"));
            }
            if in_head {
                head_elements.push((tag.clone(), attrs.clone()));
            }
            let allowed_in_head = in_head && matches!(tag.as_str(), "meta" | "style");
            if !structural && !allowed_in_head {
                if FORBIDDEN_ELEMENTS.contains(&tag.as_str()) {
                    found.push(format!("forbidden element <{tag}>"));
                } else if !BODY_ELEMENTS.contains(&tag.as_str()) {
                    found.push(format!("element outside the allowlist <{tag}>"));
                }
            }
            if tag == "style" {
                let css = text_of(&node);
                for v in css_violations(&css, true) {
                    found.push(format!("<style>: {v}"));
                }
                if !in_head {
                    found.push("style element outside head".into());
                }
            }
            for (attr, value) in &attrs {
                if allowed_in_head && tag == "meta" {
                    continue;
                }
                found.extend(attribute_violations(&tag, attr, value));
            }
            child_path.push(tag);
        }
        let children = node.children.borrow();
        for child in children.iter().rev() {
            stack.push((child.clone(), child_path.clone()));
        }
    }
    if mode == Mode::Output {
        let expected_csp = (
            "http-equiv".to_owned(),
            "Content-Security-Policy".to_owned(),
        );
        match head_elements.first() {
            Some((tag, attrs))
                if tag == "meta"
                    && attrs.contains(&expected_csp)
                    && attrs.contains(&("content".to_owned(), csp.to_owned())) => {}
            other => found.push(format!("head does not start with the CSP meta: {other:?}")),
        }
        let metas = head_elements.iter().filter(|(t, _)| t == "meta").count();
        if metas != 2 {
            found.push(format!(
                "{metas} meta elements in head (expected CSP and referrer)"
            ));
        }
        if !html.starts_with(
            "<!DOCTYPE html>\n<html><head><meta http-equiv=\"Content-Security-Policy\"",
        ) {
            found.push("document does not start with the CSP".into());
        }
    }
    found
}

fn attribute_violations(tag: &str, attr: &str, value: &str) -> Vec<String> {
    let mut found = Vec::new();
    let squashed: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    if attr.starts_with("on") {
        found.push(format!("event handler {attr} on <{tag}>"));
    }
    if FORBIDDEN_ATTRIBUTES.contains(&attr) {
        found.push(format!("forbidden attribute {attr} on <{tag}>"));
    }
    // Free text (titles, alt texts, ids) may say anything; every other attribute may not carry
    // a script URL.
    let free_text = matches!(
        attr,
        "title" | "alt" | "id" | "class" | "name" | "abbr" | "summary" | "axis" | "headers"
            | "value" | "datetime" | "lang"
            // CSS: `javascript:` outside `url()` is an identifier and a colon; `url()` itself
            // is checked in css_violations.
            | "style"
    ) || attr.starts_with("aria-")
        // ARIA like `aria-*`: a token list, never a URL (stage 15 campaign: `role` with the
        // text `javascript:` in a mutated mail is harmless).
        || attr == "role";
    if !free_text && (squashed.contains("javascript:") || squashed.contains("vbscript:")) {
        found.push(format!("script URL in {attr} on <{tag}>"));
    }
    match attr {
        "href" => {
            let ok = value
                .strip_prefix("https://link.invalid/")
                .and_then(|rest| rest.split_once('/'))
                .is_some_and(|(token, n)| {
                    token == TOKEN && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                });
            if !ok {
                found.push(format!("href not rewritten on <{tag}>: {value}"));
            }
        }
        "src" | "background" => {
            if !is_image_data_uri(value) {
                found.push(format!(
                    "{attr} not a data: image on <{tag}>: {}",
                    short(value)
                ));
            }
        }
        "srcset" => {
            for candidate in value.split(", ") {
                let url = candidate.split(' ').next().unwrap_or("");
                if !is_image_data_uri(url) {
                    found.push(format!(
                        "srcset candidate not a data: image: {}",
                        short(url)
                    ));
                }
            }
        }
        "usemap" if !value.starts_with('#') => found.push(format!("usemap {value}")),
        "style" => {
            for v in css_violations(value, false) {
                found.push(format!("style on <{tag}>: {v}"));
            }
        }
        _ => {}
    }
    found
}

fn short(text: &str) -> String {
    text.chars().take(80).collect()
}

/// `data:image/<raster>;base64,<payload>`, nothing else.
pub fn is_image_data_uri(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("data:image/") else {
        return false;
    };
    let Some((kind, payload)) = rest.split_once(";base64,") else {
        return false;
    };
    // C-4: the four formats the sniffer accepts; BMP, AVIF and ICO never reach the web view.
    matches!(kind, "png" | "jpeg" | "gif" | "webp")
        && payload
            .trim_end_matches('=')
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// Functions that load something, read the document or run code (IE).
const FORBIDDEN_CSS_FUNCTIONS: &[&str] = &[
    "expression",
    "image-set",
    "-webkit-image-set",
    "var",
    "attr",
    "src",
    "image",
    "cross-fade",
    "-webkit-cross-fade",
    "element",
    "-moz-element",
    "paint",
    "env",
];
const FORBIDDEN_AT_RULES: &[&str] = &["import", "font-face", "namespace", "charset"];
const FORBIDDEN_PROPERTIES: &[&str] = &["behavior", "-ms-behavior", "-moz-binding", "binding"];

/// Everything in CSS a sanitised mail must not contain, checked on the CSS tokens (escapes are
/// resolved by the tokenizer, as in the browser): no comment, no forbidden function or
/// at-rule, no custom or code-running property, every `url()` a `data:` image.
pub fn css_violations(css: &str, stylesheet: bool) -> Vec<String> {
    let mut found = Vec::new();
    if css.contains("</") {
        found.push("`</` inside CSS".into());
    }
    let mut parser = cssparser::Parser::new(css);
    // At the top of a stylesheet stand rules (`a:hover{…}`), in an attribute declarations.
    css_tokens(&mut parser, &mut found, 0, !stylesheet);
    found
}

fn css_tokens(
    parser: &mut cssparser::Parser<'_>,
    found: &mut Vec<String>,
    depth: usize,
    declarations: bool,
) {
    use cssparser::Token;
    if depth > 64 {
        found.push("CSS nested too deeply".into());
        return;
    }
    let mut declaration_start = true;
    loop {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return,
        };
        let mut nested = false;
        match &token {
            Token::WhiteSpace(_) => continue,
            // The sanitiser writes only empty comments, as token separators.
            Token::Comment(text) if !text.is_empty() => found.push("CSS comment".into()),
            Token::Comment(_) => continue,
            Token::AtKeyword(name) => {
                let name = name.to_ascii_lowercase();
                if FORBIDDEN_AT_RULES.contains(&name.as_str()) {
                    found.push(format!("CSS @{name}"));
                }
            }
            Token::UnquotedUrl(url) => {
                if !is_image_data_uri(url) {
                    found.push(format!("CSS url() not a data: image: {}", short(url)));
                }
            }
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                if name == "url" {
                    let url = parser
                        .parse_nested_block(|inner| {
                            let url = inner.expect_string().map(|s| s.to_string())?;
                            inner.expect_exhausted()?;
                            Ok::<_, cssparser::ParseError<()>>(url)
                        })
                        .unwrap_or_default();
                    if !is_image_data_uri(&url) {
                        found.push(format!("CSS url() not a data: image: {}", short(&url)));
                    }
                } else {
                    if FORBIDDEN_CSS_FUNCTIONS.contains(&name.as_str()) {
                        found.push(format!("CSS function {name}()"));
                    }
                    nested = true;
                }
            }
            Token::Ident(name) if declaration_start && declarations => {
                let name = name.to_ascii_lowercase();
                let is_declaration = parser
                    .try_parse(|p| {
                        p.skip_whitespace();
                        p.expect_colon()
                    })
                    .is_ok();
                if is_declaration
                    && (name.starts_with("--") || FORBIDDEN_PROPERTIES.contains(&name.as_str()))
                {
                    found.push(format!("CSS declares {name}"));
                }
            }
            Token::BadUrl(_) | Token::BadString(_) => found.push("CSS bad token".into()),
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                nested = true;
            }
            _ => {}
        }
        if nested {
            let curly = matches!(token, Token::CurlyBracketBlock);
            let _ = parser.parse_nested_block(|inner| {
                if curly {
                    css_tokens(inner, found, depth + 1, true);
                } else {
                    css_values(inner, found, depth + 1);
                }
                Ok::<_, cssparser::ParseError<()>>(())
            });
        }
        declaration_start = matches!(token, Token::Semicolon | Token::CurlyBracketBlock);
    }
}

/// Inside brackets and functions: the same token rules, but no declarations.
fn css_values(parser: &mut cssparser::Parser<'_>, found: &mut Vec<String>, depth: usize) {
    // A value block cannot start a declaration; reuse the walker after a dummy token.
    let mut nested_found = Vec::new();
    css_tokens(parser, &mut nested_found, depth, false);
    found.extend(
        nested_found
            .into_iter()
            .filter(|v| !v.starts_with("CSS declares")),
    );
}

/// Visible text of a document (body only).
pub fn visible_text(html: &str) -> String {
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html);
    let mut out = String::new();
    let mut stack = vec![dom.document.clone()];
    while let Some(node) = stack.pop() {
        if let NodeData::Element { name, .. } = &node.data
            && (&*name.local == "head" || &*name.local == "style")
        {
            continue;
        }
        if let NodeData::Text { contents } = &node.data {
            out.push_str(&contents.borrow());
        }
        let children = node.children.borrow();
        stack.extend(children.iter().rev().cloned());
    }
    out
}

fn text_of(node: &Handle) -> String {
    let mut out = String::new();
    for child in node.children.borrow().iter() {
        if let NodeData::Text { contents } = &child.data {
            out.push_str(&contents.borrow());
        }
    }
    out
}

/// What a harmless file name looks like, checked independently of the sanitiser.
pub fn harmless_file_name_problems(name: &str) -> Vec<String> {
    let mut found = Vec::new();
    if name.is_empty() {
        found.push("empty".into());
    }
    if name.len() > 200 {
        found.push(format!("{} bytes", name.len()));
    }
    for c in name.chars() {
        if c.is_control() {
            found.push(format!("control character U+{:04X}", u32::from(c)));
        }
        if matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{200B}' | '\u{FEFF}')
        {
            found.push(format!(
                "invisible or bidi character U+{:04X}",
                u32::from(c)
            ));
        }
        if matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            found.push(format!("character {c:?}"));
        }
    }
    if name == "." || name == ".." || name.starts_with('.') {
        found.push("leading dot".into());
    }
    if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
        found.push("leading/trailing dot or space".into());
    }
    let base = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((base.starts_with("COM") || base.starts_with("LPT"))
        && base.len() == 4
        && base.as_bytes()[3].is_ascii_digit());
    if reserved {
        found.push("reserved Windows name".into());
    }
    let nfc: String = icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
        .normalize(name)
        .into_owned();
    if nfc != name {
        found.push("not NFC".into());
    }
    found
}

/// Deterministic pseudo-random numbers (xorshift64*), so failures can be reproduced.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}
