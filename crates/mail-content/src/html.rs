//! The HTML pipeline (architecture, security boundaries 1–3 and 5; decision 0005).
//!
//! 1. A first parse of the whole document collects the `<style>` blocks and the attributes of
//!    `<body>` (ammonia works on a fragment and drops both).
//! 2. ammonia cleans the HTML with an allowlist of elements and attributes. Elements whose
//!    content is not shown (scripts, styles, frames, objects, forms fields, SVG, MathML, …) go
//!    with their content; other unknown elements are unwrapped.
//! 3. An attribute filter, run by ammonia on every kept attribute, rewrites what can reach the
//!    network: every `href` becomes `https://link.invalid/<n>` (the real target stays in
//!    [`RenderedHtml::links`]); image URLs (`src`, `srcset`, `background`, CSS `url()`) become
//!    `data:` URIs from inline parts or allowed images, a transparent pixel for blocked external
//!    images, or nothing. CSS goes through [`crate::css`].
//! 4. The result is parsed again and checked against the same allowlist (a regression in the
//!    sanitiser shows the mail as text instead of as HTML), and link texts are read.
//! 5. The document starts with [`MAIL_CSP`] as a meta tag, before anything from the mail.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ammonia::UrlRelative;
use url::Url;

use crate::css;
use crate::dom::{self, Descend, NodeData, Visit};
use crate::guard;
use crate::image::{self, ImageBytes, PLACEHOLDER_PIXEL};
use crate::text::html_to_text;
use mail_model::{RenderToken, link_placeholder, parse_link_placeholder};

/// Content Security Policy at the top of every `srcdoc` (decision 0005, point 2):
///
/// - `default-src 'none'`: no scripts (a second barrier besides the sandbox: with
///   `allow-same-origin` the app's `script-src 'self'` would otherwise apply), no frames, objects,
///   media, fonts, fetches or prefetches
/// - `img-src data:`: images only as `data:` URIs, which only the core writes (inline parts,
///   external images the user allowed, the placeholder pixel)
/// - `style-src 'unsafe-inline'`: the sanitised `<style>` block and `style` attributes apply
/// - `base-uri 'none'`, `form-action 'none'`: no `<base>`, no form submission, even if a later
///   change let one through
///
/// Compared with the spike, `font-src data:` is gone: `@font-face` is removed by the CSS
/// sanitiser, so fonts are never needed. A meta CSP cannot block navigations of the frame
/// itself; that is the job of the sandbox, the `link.invalid` placeholders and the navigation
/// hook.
pub const MAIL_CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";

/// Largest HTML that is rendered; longer input is cut (and [`RenderedHtml::truncated`] set).
pub const MAX_HTML_BYTES: usize = 8 << 20;

/// Elements that are kept (without their unsafe attributes).
pub(crate) const ALLOWED_TAGS: &[&str] = &[
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

/// Elements removed together with their content: what they contain is code, a nested
/// document, a form field, or text that would show up as markup.
pub(crate) const REMOVED_WITH_CONTENT: &[&str] = &[
    "script",
    "style",
    "title",
    "noscript",
    "template",
    "iframe",
    "frame",
    "frameset",
    "noframes",
    "object",
    "embed",
    "applet",
    "svg",
    "math",
    "select",
    "textarea",
    "datalist",
    "xmp",
    "noembed",
    "plaintext",
    "portal",
    "fencedframe",
    "head",
];

/// Attributes allowed on every kept element.
pub(crate) const GENERIC_ATTRIBUTES: &[&str] = &[
    "class",
    "id",
    "style",
    "title",
    "lang",
    "dir",
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "background",
    "border",
    "role",
    "hidden",
];

/// Attributes allowed on particular elements.
pub(crate) const TAG_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("a", &["href", "name"]),
    ("area", &["href", "alt", "shape", "coords"]),
    (
        "img",
        &[
            "src", "srcset", "sizes", "alt", "hspace", "vspace", "usemap",
        ],
    ),
    ("source", &["srcset", "sizes", "media", "type"]),
    ("map", &["name"]),
    (
        "table",
        &["cellpadding", "cellspacing", "summary", "frame", "rules"],
    ),
    (
        "td",
        &[
            "colspan", "rowspan", "nowrap", "headers", "scope", "abbr", "axis",
        ],
    ),
    (
        "th",
        &[
            "colspan", "rowspan", "nowrap", "headers", "scope", "abbr", "axis",
        ],
    ),
    ("col", &["span"]),
    ("colgroup", &["span"]),
    ("font", &["color", "face", "size"]),
    ("ol", &["start", "type", "reversed"]),
    ("ul", &["type"]),
    ("li", &["value", "type"]),
    ("time", &["datetime"]),
    ("data", &["value"]),
    ("del", &["datetime"]),
    ("ins", &["datetime"]),
    ("hr", &["size", "noshade", "color"]),
    ("details", &["open"]),
];

/// Attributes with simple values (sizes, colours, alignment, counts, font names). Values with
/// anything else (`:`, `(`, `/`, `<`, …) are dropped: none of them is a URL, and none needs it.
const PRESENTATIONAL: &[&str] = &[
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "hspace",
    "vspace",
    "color",
    "face",
    "size",
    "colspan",
    "rowspan",
    "span",
    "start",
    "type",
    "nowrap",
    "noshade",
    "frame",
    "rules",
    "scope",
    "shape",
    "coords",
    "text",
    "link",
    "vlink",
    "alink",
    "reversed",
    "open",
    "hidden",
];

/// Attribute name prefixes allowed everywhere (accessibility).
const GENERIC_PREFIXES: &[&str] = &["aria-"];

/// Attributes kept on the `<body>` element of the output.
const BODY_ATTRIBUTES: &[&str] = &[
    "bgcolor",
    "text",
    "link",
    "vlink",
    "alink",
    "background",
    "style",
    "class",
    "id",
    "dir",
    "lang",
];

/// Schemes ammonia lets through to the attribute filter. The filter rewrites every `href`
/// anyway; listing the common schemes means their targets are recorded and shown as "blocked"
/// instead of silently becoming plain text. Unlisted schemes turn the link into text.
const PASSED_SCHEMES: &[&str] = &[
    "http",
    "https",
    "mailto",
    "tel",
    "sms",
    "cid",
    "data",
    "ftp",
    "ftps",
    "sftp",
    "file",
    "news",
    "nntp",
    "irc",
    "ircs",
    "xmpp",
    "sip",
    "sips",
    "callto",
    "webcal",
    "geo",
    "magnet",
    "javascript",
    "vbscript",
    "about",
    "blob",
    "intent",
    "itms-apps",
    "zoommtg",
    "msteams",
    "slack",
    "tg",
    "whatsapp",
    "skype",
    "spotify",
    "facetime",
    "maps",
];

/// What happens with external images (`http`, `https`).
#[derive(Debug, Clone, Default)]
pub enum RemotePolicy {
    /// Every external image is replaced and listed in [`RenderedHtml::remote_resources`].
    #[default]
    Block,
    /// External images whose URL (as listed in `remote_resources`) is in the map are shown
    /// from these bytes, which the core fetched (without cookies or referrer). Missing ones
    /// stay blocked.
    Allow(HashMap<String, ImageBytes>),
}

/// Images of the message's inline parts, by Content-ID (without angle brackets).
#[derive(Debug, Clone, Default)]
pub struct InlineImages {
    by_content_id: HashMap<String, ImageBytes>,
}

impl InlineImages {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, content_id: &str, bytes: ImageBytes) {
        self.by_content_id
            .insert(normalize_content_id(content_id), bytes);
    }

    /// The image for a `cid:` URL (with or without the scheme, percent-encoded or not).
    pub fn get(&self, cid_url: &str) -> Option<&ImageBytes> {
        self.lookup(cid_url).map(|(_, bytes)| bytes)
    }

    /// Like [`InlineImages::get`], with the Content-ID that matched.
    fn lookup(&self, cid_url: &str) -> Option<(&str, &ImageBytes)> {
        let id = cid_url
            .get(..4)
            .filter(|scheme| scheme.eq_ignore_ascii_case("cid:"))
            .map_or(cid_url, |_| &cid_url[4..]);
        let decoded = percent_decode_str(id);
        let key = normalize_content_id(&decoded);
        if let Some((k, v)) = self.by_content_id.get_key_value(&key) {
            return Some((k.as_str(), v));
        }
        // Content-IDs are case-sensitive by RFC, but mailers disagree; a unique
        // case-insensitive match is accepted.
        let mut matches = self
            .by_content_id
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(&key));
        match (matches.next(), matches.next()) {
            (Some((k, v)), None) => Some((k.as_str(), v)),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.by_content_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_content_id.is_empty()
    }
}

fn normalize_content_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_owned()
}

fn percent_decode_str(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%'
            && let Some(value) = text
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(byte);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Options for [`render_html`].
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Token of this rendering. Every `href` becomes `https://link.invalid/<token>/<index>`
    /// (`mail_model::link_placeholder`); the type guarantees 32 lower-case hex digits, so the
    /// placeholder never needs escaping. The core draws a new random token per rendering.
    pub token: RenderToken,
    pub remote: RemotePolicy,
    pub inline: InlineImages,
    /// Largest single image shown, in bytes.
    pub max_image_bytes: usize,
    /// Largest total of all images in one document, in bytes.
    pub max_total_image_bytes: usize,
    /// Largest HTML input; longer input is cut.
    pub max_html_bytes: usize,
}

impl RenderOptions {
    /// Defaults for one rendering: external images blocked, no inline images, 8 MiB per image,
    /// 32 MiB in total, [`MAX_HTML_BYTES`] of HTML.
    pub fn new(token: RenderToken) -> Self {
        Self {
            token,
            remote: RemotePolicy::Block,
            inline: InlineImages::default(),
            max_image_bytes: 8 << 20,
            max_total_image_bytes: 32 << 20,
            max_html_bytes: MAX_HTML_BYTES,
        }
    }
}

/// Sanitised HTML ready for `<iframe sandbox="allow-same-origin" srcdoc="…">`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedHtml {
    /// The complete document, starting with the [`MAIL_CSP`] meta tag.
    pub srcdoc: String,
    /// Real link targets; link `n` appears in the document as
    /// `https://link.invalid/<token>/<n>`.
    pub links: Vec<LinkTarget>,
    /// Content-IDs of inline parts the document shows (the reading pane does not list these
    /// parts as attachments).
    pub inline_used: Vec<String>,
    /// External images that were blocked, each URL once, in document order. The core fetches
    /// these after "load once" and renders again with [`RemotePolicy::Allow`].
    pub remote_resources: Vec<RemoteRef>,
    /// Number of external image references that were blocked (a URL can occur several times).
    pub blocked_count: u32,
    /// Images that could not be shown: unknown format, too large, or over the total budget.
    pub omitted_images: u32,
    /// The HTML was longer than [`RenderOptions::max_html_bytes`] and was cut.
    pub truncated: bool,
    /// Set when the document shows the mail as text instead of as HTML, and why.
    pub fallback: Option<Fallback>,
}

/// Why a mail is shown as text although it has HTML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// Nesting, number of tags or formatting would make parsing take too long
    /// (see the guard in the crate source).
    TooComplex,
    /// The sanitised result did not pass the check against the allowlist (a sanitiser bug).
    SelfCheckFailed,
}

/// A link of a message (HTML or text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub index: u32,
    /// The target as written in the mail (trimmed). Check it with [`crate::check_link`] before
    /// opening.
    pub href: String,
    /// The visible text (whitespace collapsed, at most 200 characters); for image links the
    /// `alt` text.
    pub text: String,
}

/// An external image URL that was blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    /// Absolute `http`/`https` URL as serialised by the url crate (protocol-relative URLs become
    /// `https:`). This is the key for [`RemotePolicy::Allow`].
    pub url: String,
    /// How often the mail refers to it.
    pub occurrences: u32,
}

const LINK_TEXT_CHARS: usize = 200;

/// Sanitises mail HTML for display. Never panics on any input; see the module documentation
/// for the steps.
pub fn render_html(html: &str, options: &RenderOptions) -> RenderedHtml {
    let truncated = html.len() > options.max_html_bytes;
    let html = &html[..html.floor_char_boundary(options.max_html_bytes)];
    if guard::check(html).is_err() {
        return RenderedHtml {
            srcdoc: text_document(&guard::strip_tags(html)),
            truncated,
            fallback: Some(Fallback::TooComplex),
            ..RenderedHtml::default()
        };
    }

    let pipeline = Arc::new(Pipeline {
        options: options.clone(),
        state: Mutex::new(State::default()),
    });

    // 1. Style blocks and body attributes from a full document parse.
    let document = dom::parse_document(html);
    let mut css_text = String::new();
    dom::walk(&document.document, |visit| {
        if let Visit::Enter(node) = visit
            && dom::html_name(node) == Some("style")
        {
            css_text.push_str(&dom::text_content(node));
            css_text.push('\n');
            return Descend::Skip;
        }
        Descend::Yes
    });
    let stylesheet = css::sanitize_stylesheet(&css_text, &mut |url| pipeline.css_image(url));
    let mut body_attributes = String::new();
    if let Some(body) = dom::find_element(&document.document, "body")
        && let NodeData::Element { attrs, .. } = &body.data
    {
        for attr in attrs.borrow().iter() {
            let name = &*attr.name.local;
            if !BODY_ATTRIBUTES.contains(&name) {
                continue;
            }
            if let Some(value) = pipeline.attribute("body", name, &attr.value) {
                body_attributes.push(' ');
                body_attributes.push_str(name);
                body_attributes.push_str("=\"");
                escape_into(&value, &mut body_attributes);
                body_attributes.push('"');
            }
        }
    }
    drop(document);

    // 2. and 3. ammonia with the rewriting attribute filter.
    let filter = Arc::clone(&pipeline);
    let body = ammonia_builder()
        .attribute_filter(move |element, attribute, value| {
            filter.attribute(element, attribute, value).map(Cow::Owned)
        })
        .clean(html)
        .to_string();

    let state = std::mem::take(&mut *pipeline.state());

    // 4. Self-check and link texts.
    let mut link_texts: Vec<String> = vec![String::new(); state.links.len()];
    let verified = inspect_output(&body, &options.token, state.links.len(), &mut link_texts);

    if !verified {
        return RenderedHtml {
            srcdoc: text_document(&html_to_text(html)),
            truncated,
            fallback: Some(Fallback::SelfCheckFailed),
            ..RenderedHtml::default()
        };
    }

    // 5. The document.
    let mut srcdoc = String::with_capacity(body.len() + stylesheet.len() + 256);
    srcdoc.push_str(
        "<!DOCTYPE html>\n<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"",
    );
    srcdoc.push_str(MAIL_CSP);
    srcdoc.push_str("\"><meta name=\"referrer\" content=\"no-referrer\">");
    if !stylesheet.is_empty() {
        srcdoc.push_str("<style>\n");
        srcdoc.push_str(&stylesheet);
        srcdoc.push_str("</style>");
    }
    srcdoc.push_str("</head><body");
    srcdoc.push_str(&body_attributes);
    srcdoc.push('>');
    srcdoc.push_str(&body);
    srcdoc.push_str("</body></html>");

    let links = state
        .links
        .into_iter()
        .zip(link_texts)
        .enumerate()
        .map(|(index, (href, text))| LinkTarget {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            href,
            text,
        })
        .collect();
    RenderedHtml {
        srcdoc,
        links,
        inline_used: state.inline_used,
        remote_resources: state.remote,
        blocked_count: state.blocked,
        omitted_images: state.omitted,
        truncated,
        fallback: None,
    }
}

fn ammonia_builder() -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::empty();
    let tag_attributes: HashMap<&'static str, HashSet<&'static str>> = TAG_ATTRIBUTES
        .iter()
        .map(|(tag, attrs)| (*tag, attrs.iter().copied().collect()))
        .collect();
    builder
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .clean_content_tags(REMOVED_WITH_CONTENT.iter().copied().collect())
        .generic_attributes(GENERIC_ATTRIBUTES.iter().copied().collect())
        .generic_attribute_prefixes(GENERIC_PREFIXES.iter().copied().collect())
        .tag_attributes(tag_attributes)
        .url_schemes(PASSED_SCHEMES.iter().copied().collect())
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None)
        .strip_comments(true);
    builder
}

/// Escapes text for a double-quoted attribute value or element content.
fn escape_into(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
}

/// The mail as preformatted text, for when the HTML is not shown.
fn text_document(text: &str) -> String {
    let mut out = String::from(
        "<!DOCTYPE html>\n<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"",
    );
    out.push_str(MAIL_CSP);
    out.push_str("\"><meta name=\"referrer\" content=\"no-referrer\"></head><body><pre style=\"white-space:pre-wrap\">");
    escape_into(text, &mut out);
    out.push_str("</pre></body></html>");
    out
}

/// Parses the sanitised body as the browser will and checks it against the allowlist. Collects
/// link texts. Returns `false` if anything is outside the allowlist.
fn inspect_output(
    body: &str,
    token: &RenderToken,
    link_count: usize,
    link_texts: &mut [String],
) -> bool {
    let placeholder_index = |url: &str| {
        parse_link_placeholder(url)
            .filter(|(t, _)| t == token)
            .map(|(_, index)| index)
    };
    let dom = dom::parse_body_fragment(body);
    // The fragment parser puts the content under an <html> root of its own.
    let root = dom.document.children.borrow().first().cloned();
    let Some(root) = root else {
        return true;
    };
    let mut ok = true;
    let mut visitor = |visit: Visit<'_>| {
        let Visit::Enter(node) = visit else {
            return Descend::Yes;
        };
        let NodeData::Element { name, attrs, .. } = &node.data else {
            return Descend::Yes;
        };
        let tag = &*name.local;
        if name.ns != html5ever::ns!(html) || !ALLOWED_TAGS.contains(&tag) {
            ok = false;
            return Descend::Skip;
        }
        for attr in attrs.borrow().iter() {
            let attr_name = &*attr.name.local;
            let value = &*attr.value;
            if attr.name.ns != html5ever::ns!() || !is_allowed_attribute(tag, attr_name) {
                ok = false;
            }
            let value_ok = match attr_name {
                "href" => {
                    placeholder_index(value).is_some_and(|index| (index as usize) < link_count)
                }
                "src" | "background" => value.starts_with("data:image/"),
                "srcset" => image::parse_srcset(value)
                    .iter()
                    .all(|c| c.url.starts_with("data:image/")),
                "usemap" => value.starts_with('#'),
                _ => !attr_name.starts_with("on"),
            };
            if !value_ok {
                ok = false;
            }
        }
        if matches!(tag, "a" | "area")
            && let Some(index) = dom::attribute(node, "href").and_then(|h| placeholder_index(&h))
            && let Some(slot) = link_texts.get_mut(index as usize)
        {
            let mut text = if tag == "a" {
                dom::text_content(node)
            } else {
                String::new()
            };
            if text.trim().is_empty() {
                text = first_alt(node, tag);
            }
            *slot = dom::squash_whitespace(&text, LINK_TEXT_CHARS);
        }
        Descend::Yes
    };
    for child in root.children.borrow().iter() {
        dom::walk(child, &mut visitor);
    }
    ok
}

fn first_alt(node: &dom::Handle, tag: &str) -> String {
    if tag == "area" {
        return dom::attribute(node, "alt").unwrap_or_default();
    }
    let mut alt = String::new();
    dom::walk(node, |visit| {
        if let Visit::Enter(n) = visit
            && alt.is_empty()
            && dom::html_name(n) == Some("img")
        {
            alt = dom::attribute(n, "alt").unwrap_or_default();
        }
        Descend::Yes
    });
    alt
}

pub(crate) fn is_allowed_attribute(tag: &str, attribute: &str) -> bool {
    GENERIC_ATTRIBUTES.contains(&attribute)
        || GENERIC_PREFIXES.iter().any(|p| attribute.starts_with(p))
        || TAG_ATTRIBUTES
            .iter()
            .any(|(t, attrs)| *t == tag && attrs.contains(&attribute))
}

/// Shared between the attribute filter (called by ammonia, hence `Send + Sync` and a mutex)
/// and the steps around it.
struct Pipeline {
    options: RenderOptions,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    links: Vec<String>,
    inline_used: Vec<String>,
    remote: Vec<RemoteRef>,
    remote_index: HashMap<String, usize>,
    blocked: u32,
    omitted: u32,
    image_bytes: usize,
}

/// Outcome for one image URL.
enum Image {
    Data(String),
    /// External and not allowed: recorded in `remote_resources`.
    Blocked,
    /// Relative, unknown scheme, unknown `cid:`, not an image, or too large.
    Unavailable,
}

impl Pipeline {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Value for an allowed attribute, `None` to drop it.
    fn attribute(&self, element: &str, attribute: &str, value: &str) -> Option<String> {
        if attribute.starts_with("on") {
            return None;
        }
        match attribute {
            "href" if matches!(element, "a" | "area") => Some(self.link(value)),
            "href" => None,
            "src" if element == "img" => match self.image(value) {
                Image::Data(uri) => Some(uri),
                Image::Blocked => Some(PLACEHOLDER_PIXEL.to_owned()),
                Image::Unavailable => None,
            },
            "srcset" if matches!(element, "img" | "source") => {
                let kept: Vec<String> = image::parse_srcset(value)
                    .into_iter()
                    .filter_map(|candidate| match self.image(candidate.url) {
                        Image::Data(uri) if candidate.descriptor.is_empty() => Some(uri),
                        Image::Data(uri) => Some(format!("{uri} {}", candidate.descriptor)),
                        _ => None,
                    })
                    .collect();
                (!kept.is_empty()).then(|| kept.join(", "))
            }
            "background" => match self.image(value) {
                Image::Data(uri) => Some(uri),
                _ => None,
            },
            "style" => {
                let css = css::sanitize_declarations(value, &mut |url| self.css_image(url));
                (!css.is_empty()).then_some(css)
            }
            "usemap" => value
                .trim()
                .starts_with('#')
                .then(|| value.trim().to_owned()),
            "dir" => {
                let dir = value.trim().to_ascii_lowercase();
                matches!(dir.as_str(), "ltr" | "rtl" | "auto").then_some(dir)
            }
            "src" | "poster" | "action" | "formaction" | "data" | "cite" | "longdesc"
            | "srcdoc" | "target" | "ping" | "download" | "rel" | "http-equiv" => None,
            _ if PRESENTATIONAL.contains(&attribute) => {
                let value = value.trim();
                let simple = value.len() <= 128
                    && value.chars().all(|c| {
                        c.is_alphanumeric()
                            || matches!(c, '#' | '%' | '.' | ',' | '-' | ' ' | '\'' | '"' | '*')
                    });
                simple.then(|| value.to_owned())
            }
            _ if element == "body" || is_allowed_attribute(element, attribute) => {
                Some(value.to_owned())
            }
            _ => None,
        }
    }

    fn link(&self, href: &str) -> String {
        let mut state = self.state();
        let index = u32::try_from(state.links.len()).unwrap_or(u32::MAX);
        state.links.push(href.trim().chars().take(8192).collect());
        link_placeholder(&self.options.token, index)
    }

    fn css_image(&self, url: &str) -> Option<String> {
        match self.image(url) {
            Image::Data(uri) => Some(uri),
            _ => None,
        }
    }

    fn image(&self, url: &str) -> Image {
        let url = url.trim();
        let lower: String = url.chars().take(8).collect::<String>().to_ascii_lowercase();
        if lower.starts_with("cid:") {
            let Some((content_id, bytes)) = self.options.inline.lookup(url) else {
                return Image::Unavailable;
            };
            let image = self.encode(bytes);
            if matches!(image, Image::Data(_)) {
                let mut state = self.state();
                if !state.inline_used.iter().any(|used| used == content_id) {
                    state.inline_used.push(content_id.to_owned());
                }
            }
            return image;
        }
        if lower.starts_with("data:") {
            return match image::decode_data_url(url, self.options.max_image_bytes) {
                Some(bytes) => self.encode(&bytes),
                None => {
                    self.state().omitted += 1;
                    Image::Unavailable
                }
            };
        }
        let Some(remote) = remote_url(url) else {
            return Image::Unavailable;
        };
        if let RemotePolicy::Allow(allowed) = &self.options.remote
            && let Some(bytes) = allowed.get(&remote)
        {
            return self.encode(bytes);
        }
        let mut state = self.state();
        state.blocked += 1;
        match state.remote_index.get(&remote).copied() {
            Some(i) => {
                if let Some(entry) = state.remote.get_mut(i) {
                    entry.occurrences += 1;
                }
            }
            None => {
                let i = state.remote.len();
                state.remote_index.insert(remote.clone(), i);
                state.remote.push(RemoteRef {
                    url: remote,
                    occurrences: 1,
                });
            }
        }
        Image::Blocked
    }

    fn encode(&self, bytes: &[u8]) -> Image {
        let mut state = self.state();
        if bytes.len() > self.options.max_image_bytes
            || state.image_bytes + bytes.len() > self.options.max_total_image_bytes
        {
            state.omitted += 1;
            return Image::Unavailable;
        }
        match image::image_data_uri(bytes) {
            Some(uri) => {
                state.image_bytes += bytes.len();
                Image::Data(uri)
            }
            None => {
                state.omitted += 1;
                Image::Unavailable
            }
        }
    }
}

/// The absolute `http(s)` URL of an image reference, `None` for anything else.
fn remote_url(url: &str) -> Option<String> {
    let absolute = if url.starts_with("//") {
        Cow::Owned(format!("https:{url}"))
    } else {
        Cow::Borrowed(url)
    };
    let parsed = Url::parse(&absolute).ok()?;
    (matches!(parsed.scheme(), "http" | "https") && parsed.host().is_some())
        .then(|| parsed.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdeffedcba9876543210";

    fn token() -> RenderToken {
        RenderToken::from_random(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210)
    }

    fn options() -> RenderOptions {
        RenderOptions::new(token())
    }

    fn render(html: &str) -> RenderedHtml {
        render_html(html, &options())
    }

    fn body(rendered: &RenderedHtml) -> &str {
        let start = rendered.srcdoc.find("<body").unwrap();
        &rendered.srcdoc[start..]
    }

    #[test]
    fn document_starts_with_the_csp() {
        let r = render("<p>Hello</p>");
        assert!(r.srcdoc.starts_with(
            "<!DOCTYPE html>\n<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:;"
        ));
        assert!(r.srcdoc.ends_with("<body><p>Hello</p></body></html>"));
        assert_eq!(r.fallback, None);
    }

    #[test]
    fn links_are_rewritten_and_recorded() {
        let r = render(
            "<a href=\" https://example.com/a \" target=_blank ping=https://t.example/p download rel=noopener>Example <b>site</b></a>\
             <a href='mailto:x@example.com'>mail</a><a href='javascript:alert(1)'>js</a>\
             <a href='vmlink:42'>custom</a><a href='#top'><img alt='Top' src='cid:none'></a>",
        );
        assert_eq!(
            body(&r),
            format!(
                "<body><a href=\"https://link.invalid/{TOKEN}/0\">Example <b>site</b></a>\
                 <a href=\"https://link.invalid/{TOKEN}/1\">mail</a><a href=\"https://link.invalid/{TOKEN}/2\">js</a>\
                 <a>custom</a><a href=\"https://link.invalid/{TOKEN}/3\"><img alt=\"Top\"></a></body></html>"
            )
        );
        let links: Vec<(&str, &str)> = r
            .links
            .iter()
            .map(|l| (l.href.as_str(), l.text.as_str()))
            .collect();
        assert_eq!(
            links,
            [
                ("https://example.com/a", "Example site"),
                ("mailto:x@example.com", "mail"),
                ("javascript:alert(1)", "js"),
                ("#top", "Top"),
            ]
        );
    }

    #[test]
    fn placeholders_carry_this_renderings_token_only() {
        // A mail that writes placeholders itself (with another or a malformed token) gets them
        // rewritten like every other link; the self-check rejects foreign tokens.
        let foreign = "https://link.invalid/ffffffffffffffffffffffffffffffff/0";
        let r = render(&format!(
            "<a href='{foreign}'>forged</a><a href='https://link.invalid/x\"y/1'>broken</a>"
        ));
        assert!(!r.srcdoc.contains("ffffffff"), "{}", r.srcdoc);
        assert_eq!(r.links[0].href, foreign);
        for index in 0..2 {
            let placeholder = link_placeholder(&token(), index);
            assert!(r.srcdoc.contains(&format!("href=\"{placeholder}\"")));
            assert_eq!(parse_link_placeholder(&placeholder), Some((token(), index)));
        }
        let body = format!("<a href=\"{foreign}\">x</a>");
        let mut texts = vec![String::new()];
        assert!(!inspect_output(&body, &token(), 1, &mut texts));
        let own = format!("<a href=\"{}\">x</a>", link_placeholder(&token(), 0));
        assert!(inspect_output(&own, &token(), 1, &mut texts));
        assert!(
            !inspect_output(&own, &token(), 0, &mut texts),
            "index out of range"
        );
        assert_eq!(texts[0], "x");
    }

    #[test]
    fn presentational_attributes_keep_simple_values_only() {
        let r = render(
            "<table align=center width='100%' bgcolor='#fff' border=javascript:x><tr><td \
             height=10 valign=top><font face=\"'Arial', sans-serif\" color=red size=2>t</font></td></tr></table>",
        );
        assert!(body(&r).starts_with(
            "<body><table align=\"center\" width=\"100%\" bgcolor=\"#fff\"><tbody><tr><td height=\"10\" valign=\"top\"><font face=\"'Arial', sans-serif\" color=\"red\" size=\"2\">"
        ), "{}", body(&r));
    }

    #[test]
    fn external_images_are_blocked_and_listed() {
        let r = render(
            "<img src='https://img.example/a.png' width=600><img src='//img.example/a.png'>\
             <img srcset='https://img.example/b.png 2x'><table background='http://img.example/bg.png'><tr><td>x</td></tr></table>\
             <div style='background:url(https://img.example/c.png)'>c</div>",
        );
        assert_eq!(
            r.remote_resources,
            [
                RemoteRef {
                    url: "https://img.example/a.png".into(),
                    occurrences: 2
                },
                RemoteRef {
                    url: "https://img.example/b.png".into(),
                    occurrences: 1
                },
                RemoteRef {
                    url: "http://img.example/bg.png".into(),
                    occurrences: 1
                },
                RemoteRef {
                    url: "https://img.example/c.png".into(),
                    occurrences: 1
                },
            ]
        );
        assert_eq!(r.blocked_count, 5);
        assert!(!r.srcdoc.contains("img.example"), "{}", r.srcdoc);
        assert!(
            r.srcdoc
                .contains(&format!("<img src=\"{PLACEHOLDER_PIXEL}\" width=\"600\">"))
        );
        assert!(r.srcdoc.contains("style=\"background:none;\""));
    }

    #[test]
    fn allowed_and_inline_images_become_data_uris() {
        let gif: ImageBytes = image::decode_data_url(PLACEHOLDER_PIXEL, 100)
            .unwrap()
            .into();
        let mut inline = InlineImages::new();
        inline.insert("<logo@seed>", gif.clone());
        inline.insert("html@seed", Arc::from(&b"<html>not an image</html>"[..]));
        let mut allowed = HashMap::new();
        allowed.insert("https://img.example/a.png".to_owned(), gif);
        let options = RenderOptions {
            remote: RemotePolicy::Allow(allowed),
            inline,
            ..options()
        };
        let r = render_html(
            "<img src='cid:logo@seed'><img src='CID:logo%40seed'><img src='cid:html@seed'>\
             <img src='https://img.example/a.png'><img src='https://img.example/other.png'>\
             <img src='data:image/svg+xml,<svg onload=alert(1)>'>",
            &options,
        );
        assert_eq!(
            r.srcdoc
                .matches(&format!("src=\"{PLACEHOLDER_PIXEL}\""))
                .count(),
            4
        );
        assert_eq!(r.remote_resources.len(), 1);
        assert_eq!(r.omitted_images, 2);
        assert_eq!(r.inline_used, ["logo@seed"]);
        assert!(!r.srcdoc.contains("svg"));
    }

    #[test]
    fn style_blocks_and_body_attributes_survive_sanitised() {
        let r = render(
            "<html><head><style>p{color:red} @import url(x.css); .b{background:url(https://e.example/x.png)}</style></head>\
             <body bgcolor='#eee' onload='alert(1)' style='margin:0;behavior:url(x)'><p>t</p><style>.c{color:blue}</style></body></html>",
        );
        assert!(
            r.srcdoc.contains(
                "<style>\np{color:red;}\n.b{background:none;}\n.c{color:blue;}\n</style>"
            ),
            "{}",
            r.srcdoc
        );
        assert!(
            r.srcdoc
                .contains("<body bgcolor=\"#eee\" style=\"margin:0;\"><p>t</p></body>"),
            "{}",
            r.srcdoc
        );
        assert_eq!(r.remote_resources.len(), 1);
    }

    #[test]
    fn dangerous_elements_disappear_with_their_content() {
        let r = render(
            "<script>alert(1)</script><iframe src='/index.html'>x</iframe><object data='a'>o</object>\
             <form action='https://evil.example/'><input name=p><button>Log in</button></form>\
             <svg onload=alert(1)><script>alert(2)</script></svg><math><mi>m</mi></math>\
             <base href='https://evil.example/'><link rel=preconnect href='https://evil.example'>\
             <meta http-equiv=refresh content='0;url=https://evil.example'><p onclick='x()'>ok</p>",
        );
        assert_eq!(body(&r), "<body>Log in<p>ok</p></body></html>");
    }

    #[test]
    fn mutation_xss_from_the_seed_is_inert() {
        let r = render(
            "<math><mtext><table><mglyph><style><img src=x onerror=\"fetch('http://127.0.0.1:18999/x')\"></style></mglyph></table></mtext></math>\
             <svg><iframe><a title=\"</iframe><img src onerror=alert(1)>\">t</a></iframe></svg>\
             <noscript><p title=\"</noscript><img src=x onerror=alert(1)>\"></noscript>",
        );
        assert!(!r.srcdoc.contains("onerror"), "{}", r.srcdoc);
        assert!(!r.srcdoc.contains("127.0.0.1"), "{}", r.srcdoc);
        assert_eq!(r.fallback, None);
    }

    #[test]
    fn truncation_and_deep_nesting_are_survived() {
        let deep = format!(
            "{}x&amp;y{}",
            "<div>".repeat(50_000),
            "</div>".repeat(50_000)
        );
        let started = std::time::Instant::now();
        let r = render(&deep);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(r.fallback, Some(Fallback::TooComplex));
        assert!(r.srcdoc.contains("x&amp;y"), "{}", &r.srcdoc[..400]);
        assert!(r.srcdoc.starts_with(
            "<!DOCTYPE html>\n<html><head><meta http-equiv=\"Content-Security-Policy\""
        ));
        let moderate = format!("{}x{}", "<div>".repeat(1_000), "</div>".repeat(1_000));
        assert_eq!(render(&moderate).fallback, None);
        let options = RenderOptions {
            max_html_bytes: 10,
            ..options()
        };
        let r = render_html("<p>äöü äöü äöü</p>", &options);
        assert!(r.truncated);
    }
}
