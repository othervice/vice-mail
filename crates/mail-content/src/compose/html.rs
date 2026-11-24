//! Mail HTML for outgoing messages.
//!
//! Editor HTML (TipTap) and the HTML of a quoted original are read into a small document model
//! — paragraphs of text runs with marks (bold, italic, underline, strike, code, link), lists,
//! quotes, preformatted blocks — and written out again with a fixed allowlist: `p`, `br`,
//! `strong`, `em`, `u`, `s`, `ul`, `ol`, `li`, `blockquote`, `a` (`http`, `https`, `mailto`
//! only), `pre`, `code`, and (stage 10) `img` only with a `cid:` source (an inline part of the
//! message itself) and its `width`/`height`. Every style is inline, there are no classes, ids,
//! scripts or other attributes. Anything else in the input is dropped (scripts, styles, forms,
//! other images, frames) or unwrapped (spans, divs, tables, headings become paragraph
//! boundaries).
//!
//! The walk over the parsed input is iterative and the nesting of quotes and lists is capped,
//! so hostile or very deep HTML cannot overflow the stack; HTML the guard refuses is read as
//! text instead.

use std::collections::HashMap;

use url::Url;

use crate::dom::{self, Descend, NodeData, Visit};
use crate::guard;
use crate::text::render_text;

/// Deepest nesting of quotes and lists kept; deeper ones only split paragraphs.
const MAX_NESTING: usize = 48;

const FONT: &str = "font-family:-apple-system,'Segoe UI',Helvetica,Arial,sans-serif;\
                    font-size:14px;line-height:1.4";
const MONO: &str = "font-family:Menlo,Consolas,'Courier New',monospace";
const STYLE_P: &str = "margin:0 0 1em 0";
const STYLE_P_IN_ITEM: &str = "margin:0";
const STYLE_QUOTE: &str = "margin:0 0 1em 0;padding:0 0 0 1em;border-left:2px solid #cccccc";
const STYLE_LIST: &str = "margin:0 0 1em 0;padding:0 0 0 2em";
const STYLE_LIST_IN_ITEM: &str = "margin:0;padding:0 0 0 2em";
const STYLE_ITEM: &str = "margin:0";
const STYLE_H1: &str = "margin:0 0 0.5em 0;font-size:1.6em;font-weight:bold";
const STYLE_H2: &str = "margin:0 0 0.5em 0;font-size:1.3em;font-weight:bold";
const STYLE_H3: &str = "margin:0 0 0.5em 0;font-size:1.1em;font-weight:bold";
const STYLE_PRE: &str = "margin:0 0 1em 0;padding:8px;background-color:#f4f4f4;\
                         font-family:Menlo,Consolas,'Courier New',monospace;font-size:13px;\
                         white-space:pre-wrap";

/// Elements whose content is never shown.
const SKIPPED: &[&str] = &[
    "head", "script", "style", "template", "noscript", "title", "iframe", "object", "embed",
    "select", "textarea", "noembed", "noframes", "xmp", "button", "img", "picture", "video",
    "audio", "canvas", "map", "input", "option", "datalist", "meter", "progress",
];

/// Elements that end a paragraph (their content becomes paragraphs of its own).
const BLOCKS: &[&str] = &[
    "div",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "aside",
    "main",
    "address",
    "center",
    "figure",
    "figcaption",
    "dl",
    "dt",
    "dd",
    "table",
    "caption",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "details",
    "summary",
    "fieldset",
    "legend",
    "form",
    "hr",
    "hgroup",
    "menu",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "body",
    "html",
];

/// Inline formatting of a run of text; written in this field order (link outermost).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Marks {
    pub(crate) link: Option<String>,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Inline {
    Text(String, Marks),
    Break,
    /// An inline image of the message itself (`cid:`, stage 10). Images from anywhere else
    /// (`http`, `data:`, `file:`) never get into the model.
    Image(Image),
}

/// An inline image: the Content-ID of its MIME part and the size the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub(crate) content_id: String,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) alt: String,
}

/// Largest width or height kept for an inline image, in CSS pixels.
const MAX_IMAGE_SIDE: u32 = 4096;
/// Longest Content-ID accepted.
pub const MAX_CONTENT_ID_CHARS: usize = 128;

/// Whether `id` can be the Content-ID of an inline image: `left@right`, ASCII letters, digits
/// and `.`, `_`, `-`, `+`, `$`, `=` only, at most [`MAX_CONTENT_ID_CHARS`]. Strict on purpose:
/// the id goes into `cid:` URLs and a MIME header without any escaping.
pub fn is_valid_content_id(id: &str) -> bool {
    let allowed = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-+$=".contains(&b))
    };
    id.len() <= MAX_CONTENT_ID_CHARS
        && id
            .split_once('@')
            .is_some_and(|(left, right)| allowed(left) && allowed(right) && !right.contains('@'))
}

/// The Content-ID of a `cid:` URL, if it is one we accept.
pub(crate) fn content_id_of(src: &str) -> Option<String> {
    let src = src.trim();
    src.get(..4).filter(|p| p.eq_ignore_ascii_case("cid:"))?;
    let id = src.get(4..)?;
    is_valid_content_id(id).then(|| id.to_owned())
}

/// A width or height from an `img` attribute (`300`, `300px`) or a `style` declaration.
fn image_side(value: &str) -> Option<u32> {
    let value = value.trim();
    let digits = value.strip_suffix("px").unwrap_or(value).trim();
    let n: u32 = digits.parse().ok()?;
    (1..=MAX_IMAGE_SIDE).contains(&n).then_some(n)
}

/// `width`/`height` from an `img` element: the attributes, else `width:`/`height:` in px from
/// its style (what resizable editor images write).
fn image_of(node: &dom::Handle) -> Option<Image> {
    let content_id = dom::attribute(node, "src").and_then(|src| content_id_of(&src))?;
    Some(image_with(node, content_id))
}

/// The image element `node` with `content_id` as its source: size and alternative text.
fn image_with(node: &dom::Handle, content_id: String) -> Image {
    let style = dom::attribute(node, "style").unwrap_or_default();
    let from_style = |name: &str| {
        style.split(';').find_map(|decl| {
            let (key, value) = decl.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| image_side(value))
                .flatten()
        })
    };
    let side = |name: &str| {
        dom::attribute(node, name)
            .and_then(|v| image_side(&v))
            .or_else(|| from_style(name))
    };
    let alt: String = dom::attribute(node, "alt")
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    Image {
        content_id,
        width: side("width"),
        height: side("height"),
        alt: alt.trim().to_owned(),
    }
}

/// Text alignment the composer can set (stage 10); left is the default and not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Center,
    Right,
    Justify,
}

impl Align {
    fn css(self) -> &'static str {
        match self {
            Self::Center => "text-align:center",
            Self::Right => "text-align:right",
            Self::Justify => "text-align:justify",
        }
    }

    /// From `style="text-align: …"` or the old `align` attribute of an element.
    fn of(node: &dom::Handle) -> Option<Self> {
        let parse = |value: &str| match value.trim().to_ascii_lowercase().as_str() {
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            "justify" => Some(Self::Justify),
            _ => None,
        };
        let from_style = dom::attribute(node, "style").and_then(|style| {
            style.split(';').find_map(|decl| {
                let (key, value) = decl.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case("text-align")
                    .then(|| parse(value))
                    .flatten()
            })
        });
        from_style.or_else(|| dom::attribute(node, "align").and_then(|a| parse(&a)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    /// An empty paragraph is an empty line the user typed.
    Paragraph(Vec<Inline>),
    /// Stage 10, editor content only: a paragraph the user aligned (never empty).
    AlignedParagraph(Align, Vec<Inline>),
    /// Stage 10, editor content only: a heading of level 1 to 3 (never empty). In HTML of
    /// other origin headings stay bold paragraphs.
    Heading {
        level: u8,
        align: Option<Align>,
        inlines: Vec<Inline>,
    },
    Pre(String),
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
}

/// The editor HTML as a complete mail HTML document (see module docs).
pub fn mail_html(editor_html: &str) -> String {
    document(&html_blocks(editor_html))
}

/// HTML of any origin (a draft another program saved, pasted content) as editor HTML: the same
/// allowlist as outgoing mail, without styles. Empty paragraphs are dropped as in a received
/// mail. Safe to hand to the composer: no script, no event attribute, no external image.
pub fn editor_html(html: &str) -> String {
    let mut blocks = html_blocks_with(html, false);
    trim_paragraphs(&mut blocks);
    fragment(&blocks)
}

/// A draft another program saved, as editor HTML (see [`foreign_draft_html`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignDraftHtml {
    /// The allowlist of [`editor_html`], plus the draft's own inline images as `cid:` images
    /// with the Content-IDs the caller gave them.
    pub editor_html: String,
    /// The Content-IDs, as the message names them, of the inline images `editor_html` shows,
    /// in order, each once.
    pub shown: Vec<String>,
    /// Images and other media the editor cannot keep, which saving would lose: an image from
    /// the web or a `data:` URL, a `cid:` of no part in the caller's map, an image in
    /// preformatted text, inside a link or as a background, a video, an embedded object, an SVG
    /// drawing. Also counted when the HTML had to be read as text.
    pub dropped: usize,
}

/// Elements whose whole content is a picture or a player the editor cannot show.
const MEDIA: &[&str] = &[
    "picture", "video", "audio", "canvas", "object", "embed", "iframe",
];

/// HTML of a draft another program saved (opened from the Drafts folder), as editor HTML: the
/// allowlist and the dropped empty paragraphs of [`editor_html`], but the draft's own inline
/// images stay. `images` maps the Content-ID of each inline part the caller can keep (as the
/// message names it, without `<>`) to the Content-ID it has in the draft (the same one, or a
/// new valid one: other programs write ids this module would not accept, such as `ii_…` without
/// `@`). A `cid:` URL is matched as the reading pane matches it: percent-decoded, exactly, else
/// one case-insensitive match. What the editor cannot keep is counted in
/// [`ForeignDraftHtml::dropped`], so the caller can leave the original alone.
pub fn foreign_draft_html(html: &str, images: &HashMap<String, String>) -> ForeignDraftHtml {
    if guard::check(html).is_err() {
        let mut blocks = text_blocks(&guard::strip_tags(html));
        trim_paragraphs(&mut blocks);
        let lower = html.to_ascii_lowercase();
        return ForeignDraftHtml {
            editor_html: fragment(&blocks),
            shown: Vec::new(),
            dropped: lower.matches("<img").count()
                + MEDIA
                    .iter()
                    .map(|m| lower.matches(&format!("<{m}")).count())
                    .sum::<usize>(),
        };
    }
    let dom = dom::parse_document(html);
    let mut collector = Collector::new(false);
    collector.foreign = Some(Foreign {
        images: images.clone(),
        shown: Vec::new(),
        dropped: 0,
    });
    dom::walk(&dom.document, |visit| match visit {
        Visit::Enter(node) => collector.enter(node),
        Visit::Leave(node) => {
            collector.leave(node);
            Descend::Yes
        }
    });
    let foreign = collector.foreign.take().unwrap_or_default();
    let mut blocks = collector.finish();
    trim_paragraphs(&mut blocks);
    ForeignDraftHtml {
        editor_html: fragment(&blocks),
        shown: foreign.shown,
        dropped: foreign.dropped,
    }
}

/// Percent-decoding of a `cid:` URL (RFC 2392); invalid escapes stay as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(Some(hi)), Some(Some(lo))) = (
                bytes.get(i + 1).map(|b| hex(*b)),
                bytes.get(i + 2).map(|b| hex(*b)),
            )
        {
            out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'?'));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The inline images a foreign draft may keep, and what the walk found.
#[derive(Debug, Default)]
struct Foreign {
    images: HashMap<String, String>,
    shown: Vec<String>,
    dropped: usize,
}

impl Foreign {
    /// The (original, new) Content-ID of an image source, if it is one of `images`.
    fn lookup(&self, src: &str) -> Option<(String, String)> {
        let src = src.trim();
        let rest = src
            .get(..4)
            .filter(|p| p.eq_ignore_ascii_case("cid:"))
            .and_then(|_| src.get(4..))?;
        let decoded = percent_decode(rest);
        let key = decoded
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        if let Some((original, new)) = self.images.get_key_value(key) {
            return Some((original.clone(), new.clone()));
        }
        let mut found = self
            .images
            .iter()
            .filter(|(original, _)| original.eq_ignore_ascii_case(key));
        match (found.next(), found.next()) {
            (Some((original, new)), None) => Some((original.clone(), new.clone())),
            _ => None,
        }
    }
}

/// Plain text as editor HTML (paragraphs, `>` quotes, links), for a text quote shown in the
/// rich-text composer.
pub fn text_to_editor_html(text: &str) -> String {
    let mut blocks = text_blocks(text);
    trim_paragraphs(&mut blocks);
    fragment(&blocks)
}

/// Blocks of editor HTML: empty paragraphs (empty lines the user typed) are kept, except at
/// the start and the end.
pub(crate) fn html_blocks(html: &str) -> Vec<Block> {
    let mut blocks = html_blocks_with(html, true);
    trim_paragraphs(&mut blocks);
    blocks
}

/// Blocks of an HTML text. `keep_empty`: editor content: an empty `<p>` stays an empty line
/// and `cid:` images stay (inline images of the draft or signature); in HTML of other origin
/// (a received mail, pasted content) empty paragraphs are dropped (mostly layout, an image or
/// a spacer) and so are all images.
pub(crate) fn html_blocks_with(html: &str, keep_empty: bool) -> Vec<Block> {
    if guard::check(html).is_err() {
        return text_blocks(&guard::strip_tags(html));
    }
    let dom = dom::parse_document(html);
    let mut collector = Collector::new(keep_empty);
    dom::walk(&dom.document, |visit| match visit {
        Visit::Enter(node) => collector.enter(node),
        Visit::Leave(node) => {
            collector.leave(node);
            Descend::Yes
        }
    });
    collector.finish()
}

/// Blocks of plain text: paragraphs at blank lines, `>` quotes as nested quotes, recognised
/// URLs and addresses as links. Runs of spaces are kept with non-breaking spaces.
pub(crate) fn text_blocks(text: &str) -> Vec<Block> {
    let rendered = render_text(text);
    // One frame per open quote level: finished blocks and the open paragraph.
    let mut stack: Vec<(Vec<Block>, Vec<Inline>)> = vec![(Vec::new(), Vec::new())];
    for line in &rendered.lines {
        let depth = usize::from(line.quote_depth).min(MAX_NESTING);
        while stack.len() > depth + 1 {
            close_text_quote(&mut stack);
        }
        while stack.len() < depth + 1 {
            if let Some((blocks, paragraph)) = stack.last_mut() {
                end_paragraph(blocks, paragraph, false);
            }
            stack.push((Vec::new(), Vec::new()));
        }
        let Some((blocks, paragraph)) = stack.last_mut() else {
            break;
        };
        let blank = line.spans.iter().all(|span| match span {
            crate::TextSpan::Text(t) => t.trim().is_empty(),
            crate::TextSpan::Link { .. } => false,
        });
        if blank {
            end_paragraph(blocks, paragraph, false);
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(Inline::Break);
        }
        for span in &line.spans {
            match span {
                crate::TextSpan::Text(text) => {
                    paragraph.push(Inline::Text(keep_spaces(text), Marks::default()));
                }
                crate::TextSpan::Link { index, text } => {
                    let href = usize::try_from(*index)
                        .ok()
                        .and_then(|i| rendered.links.get(i))
                        .and_then(|link| allowed_href(&link.href));
                    let marks = Marks {
                        link: href,
                        ..Marks::default()
                    };
                    paragraph.push(Inline::Text(text.clone(), marks));
                }
            }
        }
    }
    while stack.len() > 1 {
        close_text_quote(&mut stack);
    }
    let Some((mut blocks, mut paragraph)) = stack.pop() else {
        return Vec::new();
    };
    end_paragraph(&mut blocks, &mut paragraph, false);
    blocks
}

/// Removes empty paragraphs at the start and the end.
pub(crate) fn trim_paragraphs(blocks: &mut Vec<Block>) {
    let empty = |b: &Block| matches!(b, Block::Paragraph(inlines) if inlines.is_empty());
    while blocks.last().is_some_and(empty) {
        blocks.pop();
    }
    let leading = blocks.iter().take_while(|b| empty(b)).count();
    blocks.drain(..leading);
}

fn close_text_quote(stack: &mut Vec<(Vec<Block>, Vec<Inline>)>) {
    let Some((mut blocks, mut paragraph)) = stack.pop() else {
        return;
    };
    end_paragraph(&mut blocks, &mut paragraph, false);
    if let Some((parent, parent_paragraph)) = stack.last_mut()
        && !blocks.is_empty()
    {
        end_paragraph(parent, parent_paragraph, false);
        parent.push(Block::Quote(blocks));
    }
}

/// Plain text spaces survive HTML: a run of n spaces becomes n-1 non-breaking spaces and one
/// space; a leading space becomes non-breaking. Tabs become four spaces first.
fn keep_spaces(text: &str) -> String {
    let text = text.replace('\t', "    ");
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c != ' ' {
            out.push(c);
            i += 1;
            continue;
        }
        let run = chars[i..].iter().take_while(|c| **c == ' ').count();
        if run == 1 && i > 0 {
            out.push(' ');
        } else {
            for _ in 1..run {
                out.push('\u{a0}');
            }
            out.push(if i == 0 && run == 1 { '\u{a0}' } else { ' ' });
        }
        i += run;
    }
    out
}

/// A link target that may go into an outgoing mail: absolute `http`, `https` or `mailto`.
pub(crate) fn allowed_href(href: &str) -> Option<String> {
    let url = Url::parse(href.trim()).ok()?;
    matches!(url.scheme(), "http" | "https" | "mailto").then(|| url.as_str().to_owned())
}

/// Ends the open paragraph: kept if it has visible content, or if it was an explicit `<p>`
/// (then it is an empty line).
fn end_paragraph(blocks: &mut Vec<Block>, paragraph: &mut Vec<Inline>, explicit: bool) {
    let inlines = normalize(std::mem::take(paragraph));
    if !inlines.is_empty() || explicit {
        blocks.push(Block::Paragraph(inlines));
    }
}

/// Collapses white space (not non-breaking spaces), merges runs with equal marks, trims the
/// paragraph and drops empty runs. A paragraph of only white space becomes empty.
fn normalize(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            Inline::Break => out.push(Inline::Break),
            Inline::Image(image) => out.push(Inline::Image(image)),
            Inline::Text(text, marks) => {
                let mut collapsed = String::with_capacity(text.len());
                let mut space = false;
                for c in text.chars() {
                    if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                        space = true;
                    } else {
                        if space {
                            collapsed.push(' ');
                            space = false;
                        }
                        collapsed.push(c);
                    }
                }
                if space {
                    collapsed.push(' ');
                }
                // A space after a space or a line break is not shown.
                let after_space = match out.last() {
                    None | Some(Inline::Break) => true,
                    Some(Inline::Image(_)) => false,
                    Some(Inline::Text(prev, _)) => prev.ends_with(' '),
                };
                if after_space && collapsed.starts_with(' ') {
                    collapsed.remove(0);
                }
                if collapsed.is_empty() {
                    continue;
                }
                match out.last_mut() {
                    Some(Inline::Text(prev, prev_marks)) if *prev_marks == marks => {
                        prev.push_str(&collapsed);
                    }
                    _ => out.push(Inline::Text(collapsed, marks)),
                }
            }
        }
    }
    // Trailing spaces, also before a line break.
    for i in 0..out.len() {
        let next_is_break = matches!(out.get(i + 1), None | Some(Inline::Break));
        if next_is_break && let Some(Inline::Text(text, _)) = out.get_mut(i) {
            while text.ends_with(' ') {
                text.pop();
            }
        }
    }
    out.retain(|inline| !matches!(inline, Inline::Text(text, _) if text.is_empty()));
    // Only line breaks: nothing visible.
    if out.iter().all(|inline| matches!(inline, Inline::Break)) {
        out.clear();
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Root,
    Quote,
    List {
        ordered: bool,
    },
    /// A list item; `implicit` for content that sat in a list outside any `<li>`.
    Item {
        implicit: bool,
    },
}

struct Container {
    kind: Kind,
    blocks: Vec<Block>,
    items: Vec<Vec<Block>>,
    paragraph: Vec<Inline>,
    /// The open paragraph is an explicit `<p>`.
    explicit: bool,
    /// Stage 10: alignment and heading level of the open paragraph (editor content).
    align: Option<Align>,
    heading: Option<u8>,
}

impl Container {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            blocks: Vec::new(),
            items: Vec::new(),
            paragraph: Vec::new(),
            explicit: false,
            align: None,
            heading: None,
        }
    }

    fn end_paragraph(&mut self) {
        let explicit = std::mem::take(&mut self.explicit);
        let align = self.align.take();
        match self.heading.take() {
            Some(level) => {
                let inlines = normalize(std::mem::take(&mut self.paragraph));
                if !inlines.is_empty() {
                    self.blocks.push(Block::Heading {
                        level,
                        align,
                        inlines,
                    });
                }
            }
            None => match align {
                Some(align) => {
                    let inlines = normalize(std::mem::take(&mut self.paragraph));
                    if inlines.is_empty() {
                        if explicit {
                            self.blocks.push(Block::Paragraph(inlines));
                        }
                    } else {
                        self.blocks.push(Block::AlignedParagraph(align, inlines));
                    }
                }
                None => end_paragraph(&mut self.blocks, &mut self.paragraph, explicit),
            },
        }
    }
}

/// What entering an element did, undone when leaving it.
enum Opened {
    Nothing,
    Mark,
    Paragraph,
    Boundary { bold: bool },
    Container,
    Pre,
    Cell,
}

struct Collector {
    containers: Vec<Container>,
    marks: Vec<Marks>,
    opened: Vec<Opened>,
    /// Text of the `<pre>` being read, and how deep `<pre>`s nest.
    pre: String,
    pre_depth: usize,
    /// Empty `<p>`s are kept as empty lines.
    keep_empty: bool,
    /// A draft of another program: its own inline images stay ([`foreign_draft_html`]).
    foreign: Option<Foreign>,
}

impl Collector {
    fn new(keep_empty: bool) -> Self {
        Self {
            containers: vec![Container::new(Kind::Root)],
            marks: Vec::new(),
            opened: Vec::new(),
            pre: String::new(),
            pre_depth: 0,
            keep_empty,
            foreign: None,
        }
    }

    /// Counts media of a foreign draft the editor cannot keep.
    fn drop_media(&mut self) {
        if let Some(foreign) = &mut self.foreign {
            foreign.dropped += 1;
        }
    }

    fn marks(&self) -> Marks {
        self.marks.last().cloned().unwrap_or_default()
    }

    fn push_mark(&mut self, change: impl FnOnce(&mut Marks)) -> Opened {
        let mut marks = self.marks();
        change(&mut marks);
        self.marks.push(marks);
        Opened::Mark
    }

    /// The container inline content and blocks go into; opens an implicit item in a list.
    fn target(&mut self) -> &mut Container {
        if matches!(
            self.containers.last().map(|c| c.kind),
            Some(Kind::List { .. })
        ) {
            self.containers
                .push(Container::new(Kind::Item { implicit: true }));
        }
        let last = self.containers.len() - 1;
        &mut self.containers[last]
    }

    fn inline(&mut self, inline: Inline) {
        // White space between list items must not open an item of its own.
        let blank = matches!(&inline, Inline::Text(text, _) if text.trim().is_empty());
        if blank
            && matches!(
                self.containers.last().map(|c| c.kind),
                Some(Kind::List { .. })
            )
        {
            return;
        }
        self.target().paragraph.push(inline);
    }

    fn end_paragraph(&mut self) {
        if let Some(container) = self.containers.last_mut() {
            container.end_paragraph();
        }
    }

    fn push_block(&mut self, block: Block) {
        let target = self.target();
        target.end_paragraph();
        target.blocks.push(block);
    }

    fn nesting(&self) -> usize {
        self.containers.len()
    }

    fn open(&mut self, kind: Kind) -> Opened {
        if self.nesting() >= MAX_NESTING {
            self.end_paragraph();
            return Opened::Boundary { bold: false };
        }
        if matches!(kind, Kind::Item { .. }) {
            self.close_implicit_item();
        } else {
            self.target().end_paragraph();
        }
        self.containers.push(Container::new(kind));
        Opened::Container
    }

    fn close_implicit_item(&mut self) {
        if matches!(
            self.containers.last().map(|c| c.kind),
            Some(Kind::Item { implicit: true })
        ) {
            self.close_container();
        }
    }

    /// Pops the top container and adds what it holds to its parent.
    fn close_container(&mut self) {
        if self.containers.len() <= 1 {
            return;
        }
        let Some(mut container) = self.containers.pop() else {
            return;
        };
        container.end_paragraph();
        match container.kind {
            Kind::Root => {}
            Kind::Quote => {
                if !container.blocks.is_empty() {
                    self.push_block(Block::Quote(container.blocks));
                }
            }
            Kind::List { ordered } => {
                if !container.items.is_empty() {
                    self.push_block(Block::List {
                        ordered,
                        items: container.items,
                    });
                }
            }
            Kind::Item { .. } if container.blocks.is_empty() => {}
            Kind::Item { .. } => {
                if let Some(list) = self.containers.last_mut()
                    && matches!(list.kind, Kind::List { .. })
                {
                    list.items.push(container.blocks);
                } else {
                    // Not inside a list any more: keep the content.
                    for block in container.blocks {
                        self.push_block(block);
                    }
                }
            }
        }
    }

    fn enter(&mut self, node: &dom::Handle) -> Descend {
        match &node.data {
            NodeData::Text { contents } => {
                let text = contents.borrow();
                if self.pre_depth > 0 {
                    self.pre.push_str(&text);
                } else {
                    let marks = self.marks();
                    self.inline(Inline::Text(text.to_string(), marks));
                }
                return Descend::Yes;
            }
            NodeData::Element { .. } => {}
            NodeData::Document => return Descend::Yes,
            _ => return Descend::Skip,
        }
        // SVG and MathML: nothing to show (an SVG drawing of a foreign draft is lost).
        let Some(name) = dom::html_name(node) else {
            if let NodeData::Element { name, .. } = &node.data
                && &*name.local == "svg"
            {
                self.drop_media();
            }
            return Descend::Skip;
        };
        // A picture behind the content of a foreign draft (`background`, CSS `url(…)`).
        if self.foreign.is_some()
            && (dom::attribute(node, "background").is_some()
                || dom::attribute(node, "style")
                    .is_some_and(|style| style.to_ascii_lowercase().contains("url(")))
        {
            self.drop_media();
        }
        // Word's list bullets and numbers (`<span style="mso-list:Ignore">·</span>`).
        if dom::attribute(node, "style").is_some_and(|style| {
            style
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .to_ascii_lowercase()
                .contains("mso-list:ignore")
        }) {
            return Descend::Skip;
        }
        if name == "img" && self.pre_depth == 0 {
            // Only editor content (the composer's own HTML, signatures) has inline images of
            // its own; in HTML of any other origin (a received mail being quoted, pasted
            // content) a `cid:` points into someone else's message. A draft of another program
            // keeps the images of its own parts the caller named.
            if self.keep_empty
                && let Some(image) = image_of(node)
            {
                self.inline(Inline::Image(image));
            } else if let Some(foreign) = &self.foreign {
                let found = dom::attribute(node, "src").and_then(|src| foreign.lookup(&src));
                let linked = self.marks().link.is_some();
                match found {
                    Some((original, new)) => {
                        if let Some(foreign) = &mut self.foreign {
                            if !foreign.shown.contains(&original) {
                                foreign.shown.push(original);
                            }
                            // Shown, but the editor keeps no link on an image.
                            if linked {
                                foreign.dropped += 1;
                            }
                        }
                        self.inline(Inline::Image(image_with(node, new)));
                    }
                    None => self.drop_media(),
                }
            }
            return Descend::Skip;
        }
        if self.foreign.is_some()
            && (MEDIA.contains(&name) || (name == "img" && self.pre_depth > 0))
        {
            self.drop_media();
        }
        if SKIPPED.contains(&name) {
            return Descend::Skip;
        }
        let opened = if self.pre_depth > 0 {
            match name {
                "br" => {
                    self.pre.push('\n');
                    Opened::Nothing
                }
                "pre" => {
                    self.pre_depth += 1;
                    Opened::Pre
                }
                _ => Opened::Nothing,
            }
        } else {
            self.enter_element(node, name)
        };
        self.opened.push(opened);
        Descend::Yes
    }

    fn enter_element(&mut self, node: &dom::Handle, name: &str) -> Opened {
        match name {
            "br" => {
                self.inline(Inline::Break);
                Opened::Nothing
            }
            "b" | "strong" => self.push_mark(|m| m.bold = true),
            "i" | "em" | "cite" | "dfn" | "var" => self.push_mark(|m| m.italic = true),
            "u" | "ins" => self.push_mark(|m| m.underline = true),
            "s" | "strike" | "del" => self.push_mark(|m| m.strike = true),
            "code" | "kbd" | "samp" | "tt" => self.push_mark(|m| m.code = true),
            "a" => {
                let href = dom::attribute(node, "href").and_then(|h| allowed_href(&h));
                self.push_mark(|m| m.link = href)
            }
            "p" => {
                let keep_empty = self.keep_empty;
                let align = keep_empty.then(|| Align::of(node)).flatten();
                let target = self.target();
                target.end_paragraph();
                target.explicit = keep_empty;
                target.align = align;
                Opened::Paragraph
            }
            // Stage 10: headings of the composer's own content stay headings.
            "h1" | "h2" | "h3" if self.keep_empty => {
                let align = Align::of(node);
                let level = name.as_bytes().get(1).map_or(1, |b| b - b'0');
                let target = self.target();
                target.end_paragraph();
                target.heading = Some(level);
                target.align = align;
                Opened::Paragraph
            }
            "blockquote" => self.open(Kind::Quote),
            "ul" | "ol" => self.open(Kind::List {
                ordered: name == "ol",
            }),
            "li" if matches!(
                self.containers.last().map(|c| c.kind),
                Some(Kind::List { .. } | Kind::Item { implicit: true })
            ) =>
            {
                self.open(Kind::Item { implicit: false })
            }
            "pre" => {
                self.target().end_paragraph();
                self.pre_depth = 1;
                self.pre.clear();
                Opened::Pre
            }
            "td" | "th" => Opened::Cell,
            _ if BLOCKS.contains(&name) || name == "li" => {
                self.end_paragraph();
                let bold = name.len() == 2 && name.starts_with('h') && name != "hr";
                if bold {
                    self.push_mark(|m| m.bold = true);
                }
                Opened::Boundary { bold }
            }
            _ => Opened::Nothing,
        }
    }

    fn leave(&mut self, node: &dom::Handle) {
        if !matches!(node.data, NodeData::Element { .. }) || dom::html_name(node).is_none() {
            return;
        }
        let Some(opened) = self.opened.pop() else {
            return;
        };
        match opened {
            Opened::Nothing => {}
            Opened::Mark => {
                self.marks.pop();
            }
            Opened::Paragraph => {
                if let Some(container) = self.containers.last_mut() {
                    container.end_paragraph();
                }
            }
            Opened::Boundary { bold } => {
                if bold {
                    self.marks.pop();
                }
                self.end_paragraph();
            }
            Opened::Container => {
                self.close_implicit_item();
                self.close_container();
            }
            Opened::Pre => {
                self.pre_depth = self.pre_depth.saturating_sub(1);
                if self.pre_depth == 0 {
                    let text = std::mem::take(&mut self.pre).replace("\r\n", "\n");
                    let text = text.trim_end_matches('\n').to_owned();
                    if !text.trim().is_empty() {
                        self.push_block(Block::Pre(text));
                    }
                }
            }
            Opened::Cell => {
                self.inline(Inline::Text(" ".into(), Marks::default()));
            }
        }
    }

    fn finish(mut self) -> Vec<Block> {
        while self.containers.len() > 1 {
            self.close_implicit_item();
            self.close_container();
        }
        let Some(mut root) = self.containers.pop() else {
            return Vec::new();
        };
        root.end_paragraph();
        root.blocks
    }
}

/// Where a block sits; paragraphs and lists inside list items get no outer margins.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Flow,
    Item,
}

/// A complete mail HTML document: doctype, charset, a wrapper with the font, the blocks.
pub(crate) fn document(blocks: &[Block]) -> String {
    let mut out = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n\
         <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">\n\
         </head>\n<body>\n",
    );
    out.push_str("<div style=\"");
    out.push_str(FONT);
    out.push_str("\">\n");
    write_blocks(blocks, Context::Flow, true, &mut out);
    out.push_str("</div>\n</body>\n</html>\n");
    out
}

/// Blocks as an HTML fragment without styles (content for the editor).
pub(crate) fn fragment(blocks: &[Block]) -> String {
    let mut out = String::new();
    write_blocks(blocks, Context::Flow, false, &mut out);
    out
}

/// Like [`open_tag`], but the editor fragment keeps the alignment (`unstyled`), so it survives
/// being read again.
fn open_aligned(out: &mut String, tag: &str, style: &str, unstyled: &str, styled: bool) {
    if styled {
        open_tag(out, tag, style, true);
    } else {
        open_tag(out, tag, unstyled, true);
    }
}

fn open_tag(out: &mut String, tag: &str, style: &str, styled: bool) {
    out.push('<');
    out.push_str(tag);
    if styled && !style.is_empty() {
        out.push_str(" style=\"");
        out.push_str(style);
        out.push('"');
    }
    out.push('>');
}

fn write_blocks(blocks: &[Block], context: Context, styled: bool, out: &mut String) {
    for (index, block) in blocks.iter().enumerate() {
        match block {
            Block::Paragraph(inlines) if context == Context::Item && index == 0 => {
                // The first paragraph of an item is the item's own text.
                write_inlines(inlines, styled, out);
                if inlines.is_empty() {
                    out.push_str("<br>");
                }
            }
            Block::Paragraph(inlines) => {
                let style = if context == Context::Item {
                    STYLE_P_IN_ITEM
                } else {
                    STYLE_P
                };
                open_tag(out, "p", style, styled);
                if inlines.is_empty() {
                    out.push_str("<br>");
                } else {
                    write_inlines(inlines, styled, out);
                }
                out.push_str("</p>");
            }
            Block::AlignedParagraph(align, inlines) => {
                let base = if context == Context::Item {
                    STYLE_P_IN_ITEM
                } else {
                    STYLE_P
                };
                let style = format!("{base};{}", align.css());
                open_aligned(out, "p", &style, align.css(), styled);
                write_inlines(inlines, styled, out);
                out.push_str("</p>");
            }
            Block::Heading {
                level,
                align,
                inlines,
            } => {
                let tag = format!("h{}", level.clamp(&1, &3));
                let mut style = match level {
                    1 => STYLE_H1,
                    2 => STYLE_H2,
                    _ => STYLE_H3,
                }
                .to_owned();
                if let Some(align) = align {
                    style.push(';');
                    style.push_str(align.css());
                }
                open_aligned(out, &tag, &style, align.map_or("", Align::css), styled);
                write_inlines(inlines, styled, out);
                out.push_str("</");
                out.push_str(&tag);
                out.push('>');
            }
            Block::Pre(text) => {
                open_tag(out, "pre", STYLE_PRE, styled);
                escape_into(text, out);
                out.push_str("</pre>");
            }
            Block::Quote(children) => {
                open_tag(out, "blockquote", STYLE_QUOTE, styled);
                out.push('\n');
                write_blocks(children, Context::Flow, styled, out);
                out.push_str("</blockquote>");
            }
            Block::List { ordered, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let style = if context == Context::Item {
                    STYLE_LIST_IN_ITEM
                } else {
                    STYLE_LIST
                };
                open_tag(out, tag, style, styled);
                out.push('\n');
                for item in items {
                    open_tag(out, "li", STYLE_ITEM, styled);
                    write_blocks(item, Context::Item, styled, out);
                    out.push_str("</li>\n");
                }
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
        }
        if context == Context::Flow {
            out.push('\n');
        }
    }
}

/// Mark tags, outermost first, for `marks`.
fn mark_tags(marks: &Marks) -> Vec<MarkTag<'_>> {
    let mut tags = Vec::new();
    if let Some(href) = &marks.link {
        tags.push(MarkTag::Link(href));
    }
    for (on, tag) in [
        (marks.bold, "strong"),
        (marks.italic, "em"),
        (marks.underline, "u"),
        (marks.strike, "s"),
        (marks.code, "code"),
    ] {
        if on {
            tags.push(MarkTag::Plain(tag));
        }
    }
    tags
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkTag<'a> {
    Link(&'a str),
    Plain(&'static str),
}

impl MarkTag<'_> {
    fn open(self, styled: bool, out: &mut String) {
        match self {
            Self::Link(href) => {
                out.push_str("<a href=\"");
                escape_attribute_into(href, out);
                out.push_str("\">");
            }
            Self::Plain("code") if styled => {
                out.push_str("<code style=\"");
                out.push_str(MONO);
                out.push_str("\">");
            }
            Self::Plain(tag) => {
                out.push('<');
                out.push_str(tag);
                out.push('>');
            }
        }
    }

    fn close(self, out: &mut String) {
        out.push_str("</");
        out.push_str(match self {
            Self::Link(_) => "a",
            Self::Plain(tag) => tag,
        });
        out.push('>');
    }
}

fn write_inlines(inlines: &[Inline], styled: bool, out: &mut String) {
    let mut open: Vec<MarkTag<'_>> = Vec::new();
    for inline in inlines {
        let wanted = match inline {
            Inline::Text(_, marks) => mark_tags(marks),
            // A line break and an image keep the open marks.
            Inline::Break | Inline::Image(_) => open.clone(),
        };
        let common = open.iter().zip(&wanted).take_while(|(a, b)| a == b).count();
        while open.len() > common {
            if let Some(tag) = open.pop() {
                tag.close(out);
            }
        }
        for tag in wanted.iter().skip(common) {
            tag.open(styled, out);
            open.push(*tag);
        }
        match inline {
            Inline::Text(text, _) => escape_into(text, out),
            Inline::Break => out.push_str("<br>"),
            Inline::Image(image) => write_image(image, styled, out),
        }
    }
    while let Some(tag) = open.pop() {
        tag.close(out);
    }
}

/// `<img src="cid:…">` with the chosen size; in mail HTML it never grows wider than the window.
fn write_image(image: &Image, styled: bool, out: &mut String) {
    out.push_str("<img src=\"cid:");
    out.push_str(&image.content_id);
    out.push_str("\" alt=\"");
    escape_attribute_into(&image.alt, out);
    out.push('"');
    for (name, value) in [("width", image.width), ("height", image.height)] {
        if let Some(value) = value {
            out.push_str(&format!(" {name}=\"{value}\""));
        }
    }
    if styled {
        out.push_str(" style=\"max-width:100%;height:auto\"");
    }
    out.push('>');
}

/// Content-IDs of the inline images in editor or mail HTML, in order, each once.
pub fn inline_image_ids(html: &str) -> Vec<String> {
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Paragraph(inlines)
                | Block::AlignedParagraph(_, inlines)
                | Block::Heading { inlines, .. } => {
                    for inline in inlines {
                        if let Inline::Image(image) = inline
                            && !out.contains(&image.content_id)
                        {
                            out.push(image.content_id.clone());
                        }
                    }
                }
                Block::Pre(_) => {}
                Block::List { items, .. } => {
                    for item in items {
                        walk(item, out);
                    }
                }
                Block::Quote(children) => walk(children, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&html_blocks(html), &mut out);
    out
}

pub(crate) fn escape_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
}

fn escape_attribute_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
}

/// One quoted line: `> text`, `>> text` for lines already quoted, `>` for empty ones.
pub(crate) fn quote_line(line: &str) -> String {
    if line.is_empty() {
        ">".to_owned()
    } else if line.starts_with('>') {
        format!(">{line}")
    } else {
        format!("> {line}")
    }
}
