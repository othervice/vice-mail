//! A linear pre-scan of mail HTML that refuses input the HTML parser would need quadratic time
//! or memory for, before it is parsed.
//!
//! html5ever, like the HTML standard, searches the stack of open elements for every start tag;
//! with deep nesting that is quadratic (measured, release build: 20,000 nested `<div>` take
//! 350 ms, 200,000 about 35 s). Formatting elements that are closed implicitly are re-created in
//! every following paragraph ("reconstruct the active formatting elements"): 512 unclosed `<b>`
//! and 2,000 paragraphs, 21 KB of input, become 7 MB of output. Browsers cap the depth at 512.
//!
//! The scan follows the tokenizer rules closely enough to see the same tags (quoted attribute
//! values, comments, raw-text elements, CDATA sections; raw text only for HTML elements, not
//! for the same names inside SVG and MathML) and estimates the depth from above: it only pops an
//! element when the parser certainly does (an end tag for the current element, and the usual
//! implied closings of `p`, `li`, `dd`/`dt`, `option`, `tr`, `td`/`th`). Where the scan read the
//! input differently from html5ever, nesting could hide behind the difference (stage 15 review,
//! `docs/SECURITY_MODEL.md`, finding C-1); `tests/fuzz.rs` compares the two.

/// More start tags than this are refused (a large newsletter has a few thousand).
pub(crate) const MAX_START_TAGS: usize = 150_000;
/// Estimated nesting depth above this is refused (newsletters stay below 100).
pub(crate) const MAX_DEPTH: usize = 1_500;
/// Upper bound of formatting elements the parser may re-create, summed over the document.
pub(crate) const MAX_FORMATTING_WORK: usize = 3_000_000;

/// Why HTML is not parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TooComplex {
    StartTags,
    Depth,
    Formatting,
}

const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "image", "img",
    "input", "keygen", "link", "meta", "param", "source", "track", "wbr",
];

const RAW_TEXT: &[&str] = &[
    "script", "style", "xmp", "iframe", "noembed", "noframes", "noscript", "textarea", "title",
];

const FORMATTING: &[&str] = &[
    "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u",
];

/// The "special" elements of the HTML standard (tree construction, "special" category).
const SPECIAL: &[&str] = &[
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "base",
    "basefont",
    "bgsound",
    "blockquote",
    "body",
    "br",
    "button",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "iframe",
    "img",
    "input",
    "keygen",
    "li",
    "link",
    "listing",
    "main",
    "marquee",
    "menu",
    "meta",
    "nav",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "ol",
    "p",
    "param",
    "plaintext",
    "pre",
    "script",
    "search",
    "section",
    "select",
    "source",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
    "wbr",
    "xmp",
    "mi",
    "mo",
    "mn",
    "ms",
    "mtext",
    "annotation-xml",
    "foreignobject",
    "desc",
];

/// Elements that end the search for an element "in scope".
const SCOPE: &[&str] = &[
    "applet",
    "caption",
    "html",
    "table",
    "td",
    "th",
    "marquee",
    "object",
    "template",
    "mi",
    "mo",
    "mn",
    "ms",
    "mtext",
    "annotation-xml",
    "foreignobject",
    "desc",
    "title",
];

/// Start tags that close an open `p` that is the current element.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "center",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
    "li",
    "dd",
    "dt",
    "listing",
    "plaintext",
    "xmp",
];

/// Checks `html`; `Err` means: do not parse it, show it as text.
pub(crate) fn check(html: &str) -> Result<(), TooComplex> {
    let mut scanner = Scanner::default();
    let mut result = Ok(());
    let complete = scan(html, &mut |event| {
        if result.is_err() {
            return false;
        }
        result = scanner.event(event);
        result.is_ok()
    });
    // The scan gave up on SVG/MathML content it cannot model.
    if !complete && result.is_ok() {
        result = Err(TooComplex::Depth);
    }
    result
}

/// Text of HTML without parsing it: tags removed, a line break for block elements, the common
/// entities decoded. For HTML that [`check`] refuses.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let _complete = scan(html, &mut |event| {
        match event {
            Event::Text(text) => decode_entities_into(text, &mut out),
            Event::Start { name, .. } | Event::End { name } => {
                if matches!(
                    name.as_str(),
                    "br" | "p"
                        | "div"
                        | "tr"
                        | "li"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "table"
                        | "blockquote"
                        | "pre"
                        | "hr"
                ) && !out.ends_with('\n')
                {
                    out.push('\n');
                }
            }
        }
        true
    });
    out
}

fn decode_entities_into(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest[1..].find(';').map(|i| i + 1).filter(|i| *i <= 10);
        let decoded = end.and_then(|end| {
            let entity = &rest[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32)
                    .filter(|c| !c.is_control() || *c == '\n'),
            }?;
            Some((ch, end))
        });
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
}

enum Event<'a> {
    Start { name: String, self_closing: bool },
    End { name: String },
    Text(&'a str),
}

/// Elements that put a marker on the list of active formatting elements: formatting elements
/// opened inside them are forgotten when they close.
const MARKERS: &[&str] = &[
    "td", "th", "caption", "marquee", "object", "applet", "template",
];

struct Open {
    name: String,
    /// This element opened a formatting frame (see [`MARKERS`]).
    frame: bool,
}

/// Formatting elements opened since the last marker, by name.
#[derive(Default)]
struct Frame {
    counts: std::collections::HashMap<String, usize>,
    total: usize,
}

struct Scanner {
    stack: Vec<Open>,
    frames: Vec<Frame>,
    start_tags: usize,
    work: usize,
    foreign_depth: usize,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            frames: vec![Frame::default()],
            start_tags: 0,
            work: 0,
            foreign_depth: 0,
        }
    }
}

impl Scanner {
    fn active_formatting(&self) -> usize {
        self.frames.last().map_or(0, |f| f.total)
    }

    fn event(&mut self, event: Event<'_>) -> Result<(), TooComplex> {
        match event {
            Event::Text(text) => {
                if !text.trim().is_empty() {
                    self.work = self.work.saturating_add(self.active_formatting());
                }
            }
            Event::Start { name, self_closing } => {
                self.start_tags += 1;
                if self.start_tags > MAX_START_TAGS {
                    return Err(TooComplex::StartTags);
                }
                self.work = self.work.saturating_add(self.active_formatting());
                self.implied_close(&name);
                if FORMATTING.contains(&name.as_str())
                    && let Some(frame) = self.frames.last_mut()
                {
                    *frame.counts.entry(name.clone()).or_default() += 1;
                    frame.total += 1;
                }
                let foreign_self_closing = self_closing && self.foreign_depth > 0;
                // The parser creates `html`, `head` and `body` once and ignores repeats.
                let structural = matches!(name.as_str(), "html" | "head" | "body");
                if !VOID.contains(&name.as_str()) && !foreign_self_closing && !structural {
                    if name == "svg" || name == "math" {
                        self.foreign_depth += 1;
                    }
                    let frame = MARKERS.contains(&name.as_str());
                    if frame {
                        self.frames.push(Frame::default());
                    }
                    self.stack.push(Open { name, frame });
                    if self.stack.len() > MAX_DEPTH {
                        return Err(TooComplex::Depth);
                    }
                }
            }
            Event::End { name } => {
                if let Some(frame) = self.frames.last_mut()
                    && let Some(count) = frame.counts.get_mut(&name)
                    && *count > 0
                {
                    *count -= 1;
                    frame.total -= 1;
                }
                self.end_tag(&name);
            }
        }
        if self.work > MAX_FORMATTING_WORK {
            return Err(TooComplex::Formatting);
        }
        Ok(())
    }

    fn top(&self) -> Option<&str> {
        self.stack.last().map(|open| open.name.as_str())
    }

    fn pop(&mut self) {
        if let Some(open) = self.stack.pop() {
            if open.name == "svg" || open.name == "math" {
                self.foreign_depth = self.foreign_depth.saturating_sub(1);
            }
            if open.frame && self.frames.len() > 1 {
                self.frames.pop();
            }
        }
    }

    fn pop_until(&mut self, index: usize) {
        while self.stack.len() > index {
            self.pop();
        }
    }

    /// Index of the nearest open element named `name`, searching from the top and stopping at
    /// any element in `stop` (or, with `stop_special`, at any special element).
    fn find_open(&self, names: &[&str], stop: &[&str], stop_special: bool) -> Option<usize> {
        for (index, open) in self.stack.iter().enumerate().rev() {
            let name = open.name.as_str();
            if names.contains(&name) {
                return Some(index);
            }
            if stop.contains(&name) || (stop_special && SPECIAL.contains(&name)) {
                return None;
            }
        }
        None
    }

    /// Closings the parser performs for these start tags (HTML standard, "in body", "in table",
    /// "in row", "in cell").
    fn implied_close(&mut self, name: &str) {
        if CLOSES_P.contains(&name) {
            let mut stop: Vec<&str> = SCOPE.to_vec();
            stop.push("button");
            if let Some(index) = self.find_open(&["p"], &stop, false) {
                self.pop_until(index);
            }
        }
        match name {
            "li" | "dd" | "dt" => {
                let names: &[&str] = if name == "li" { &["li"] } else { &["dd", "dt"] };
                for index in (0..self.stack.len()).rev() {
                    let open = self.stack[index].name.as_str();
                    if names.contains(&open) {
                        self.pop_until(index);
                        break;
                    }
                    if SPECIAL.contains(&open) && !matches!(open, "address" | "div" | "p") {
                        break;
                    }
                }
            }
            "option" => {
                if self.top() == Some("option") {
                    self.pop();
                }
            }
            "tr" | "td" | "th" | "tbody" | "thead" | "tfoot" => {
                while matches!(self.top(), Some("td" | "th")) {
                    self.pop();
                }
                if name != "td" && name != "th" && self.top() == Some("tr") {
                    self.pop();
                }
                if matches!(name, "tbody" | "thead" | "tfoot")
                    && matches!(self.top(), Some("tbody" | "thead" | "tfoot"))
                {
                    self.pop();
                }
            }
            _ => {}
        }
    }

    /// Pops what the parser certainly pops for this end tag: up to the nearest open element of
    /// that name, unless an element that stops the search lies above it (a special element for
    /// ordinary end tags, a scope boundary for special ones). `</body>` and `</html>` pop
    /// nothing; `</form>` removes only the form.
    fn end_tag(&mut self, name: &str) {
        if matches!(name, "body" | "html" | "head" | "br") {
            return;
        }
        let special = SPECIAL.contains(&name);
        let index = if special {
            let mut stop: Vec<&str> = match name {
                "td" | "th" | "tr" | "tbody" | "thead" | "tfoot" | "table" | "caption" => {
                    vec!["html", "table", "template"]
                }
                _ => SCOPE.to_vec(),
            };
            match name {
                "p" => stop.push("button"),
                "li" => stop.extend(["ol", "ul"]),
                _ => {}
            }
            stop.retain(|s| *s != name);
            self.find_open(&[name], &stop, false)
        } else {
            self.find_open(&[name], &[], true)
        };
        let Some(index) = index else {
            return;
        };
        if name == "form" {
            let open = self.stack.remove(index);
            if open.frame && self.frames.len() > 1 {
                self.frames.pop();
            }
            return;
        }
        self.pop_until(index);
    }
}

/// Walks the tags and text of `html` the way the HTML tokenizer splits them. `visit` returns
/// `false` to stop. Returns `false` when the scan gave up on SVG and MathML content: nested
/// deeper than [`MAX_DEPTH`], or ended in a way its model cannot follow ([`Foreign::end`]).
fn scan(html: &str, visit: &mut dyn FnMut(Event<'_>) -> bool) -> bool {
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    let mut foreign = Foreign::default();
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let next = bytes.get(i + 1).copied();
        let is_end = next == Some(b'/');
        let name_start = if is_end { i + 2 } else { i + 1 };
        let starts_tag = bytes.get(name_start).is_some_and(u8::is_ascii_alphabetic);
        if !starts_tag {
            match next {
                Some(b'!') => {
                    if text_start < i && !visit(Event::Text(&html[text_start..i])) {
                        return true;
                    }
                    let end = if html[i..].starts_with("<!--") {
                        i + 4 + comment_length(&html[i + 4..])
                    } else if foreign.cdata_sections() && html[i..].starts_with("<![CDATA[") {
                        // In SVG and MathML a CDATA section ends at `]]>`, not at `>`.
                        html[i + 9..]
                            .find("]]>")
                            .map_or(bytes.len(), |p| i + 9 + p + 3)
                    } else {
                        html[i..].find('>').map_or(bytes.len(), |p| i + p + 1)
                    };
                    i = end;
                    text_start = i;
                }
                Some(b'?') | Some(b'/') => {
                    if text_start < i && !visit(Event::Text(&html[text_start..i])) {
                        return true;
                    }
                    i = html[i..].find('>').map_or(bytes.len(), |p| i + p + 1);
                    text_start = i;
                }
                _ => i += 1,
            }
            continue;
        }
        if text_start < i && !visit(Event::Text(&html[text_start..i])) {
            return true;
        }
        let tag_start = i;
        let mut j = name_start;
        while j < bytes.len()
            && !matches!(
                bytes[j],
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>'
            )
        {
            j += 1;
        }
        let name = html[name_start..j].to_ascii_lowercase();
        // Attributes, respecting quotes.
        let mut self_closing = false;
        loop {
            while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
                j += 1;
            }
            match bytes.get(j) {
                None => break,
                Some(b'>') => {
                    j += 1;
                    break;
                }
                Some(b'/') => {
                    j += 1;
                    if bytes.get(j) == Some(&b'>') {
                        self_closing = true;
                        j += 1;
                        break;
                    }
                }
                Some(&first) => {
                    // An attribute name may start with `=` (`<a ="x">` has the attribute
                    // `="x"`); only later `=` signs start a value.
                    if first == b'=' {
                        j += 1;
                    }
                    while j < bytes.len()
                        && !matches!(
                            bytes[j],
                            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>' | b'='
                        )
                    {
                        j += 1;
                    }
                    while j < bytes.len()
                        && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
                    {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b'=') {
                        j += 1;
                        while j < bytes.len()
                            && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
                        {
                            j += 1;
                        }
                        match bytes.get(j) {
                            Some(&quote @ (b'"' | b'\'')) => {
                                j += 1;
                                while j < bytes.len() && bytes[j] != quote {
                                    j += 1;
                                }
                                j += 1;
                            }
                            _ => {
                                while j < bytes.len()
                                    && !matches!(
                                        bytes[j],
                                        b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'>'
                                    )
                                {
                                    j += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        i = j.min(bytes.len());
        text_start = i;
        if is_end {
            foreign.end(&name);
            if foreign.lost {
                return false;
            }
            if !visit(Event::End { name }) {
                return true;
            }
            continue;
        }
        // Raw text and `plaintext` exist only for HTML elements; inside SVG and MathML the
        // same names are ordinary elements whose content is markup.
        let html_element = foreign.start(&name, self_closing, &html[tag_start..i]);
        if foreign.lost || foreign.stack.len() > MAX_DEPTH {
            return false;
        }
        let raw_text = html_element && RAW_TEXT.contains(&name.as_str());
        let plaintext = html_element && name == "plaintext";
        if !visit(Event::Start {
            name: name.clone(),
            self_closing,
        }) {
            return true;
        }
        if plaintext {
            if i < bytes.len() {
                visit(Event::Text(&html[i..]));
            }
            return true;
        }
        if raw_text {
            match find_end_tag(&bytes[i..], name.as_bytes()) {
                Some(p) => {
                    i += p;
                    text_start = i;
                }
                None => return true,
            }
        }
    }
    if text_start < bytes.len() {
        visit(Event::Text(&html[text_start..]));
    }
    true
}

/// Length of a comment after its `<!--`, up to and including its end, as the tokenizer reads
/// it: `<!-->` and `<!--->` are complete (empty) comments, and `--!>` ends a comment as well as
/// `-->`. Without an end, the comment runs to the end of the input.
fn comment_length(rest: &str) -> usize {
    if rest.starts_with('>') {
        return 1;
    }
    if rest.starts_with("->") {
        return 2;
    }
    let plain = rest.find("-->").map(|p| p + 3);
    let bang = rest.find("--!>").map(|p| p + 4);
    match (plain, bang) {
        (Some(a), Some(b)) => a.min(b),
        (Some(end), None) | (None, Some(end)) => end,
        (None, None) => rest.len(),
    }
}

/// Start tags that end SVG or MathML content (HTML standard, "parsing tokens in foreign
/// content"); `font` only with `color`, `face` or `size`.
const BREAKOUT: &[&str] = &[
    "b",
    "big",
    "blockquote",
    "body",
    "br",
    "center",
    "code",
    "dd",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "hr",
    "i",
    "img",
    "li",
    "listing",
    "menu",
    "meta",
    "nobr",
    "ol",
    "p",
    "pre",
    "ruby",
    "s",
    "small",
    "span",
    "strong",
    "strike",
    "sub",
    "sup",
    "table",
    "tt",
    "u",
    "ul",
    "var",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An HTML element inside SVG or MathML (below an integration point).
    Html,
    /// An SVG or MathML element whose content is SVG or MathML.
    Foreign { svg: bool },
    /// `foreignObject`, `desc`, `title` in SVG; `annotation-xml` for HTML: content is HTML.
    HtmlIntegration,
    /// `mi`, `mo`, `mn`, `ms`, `mtext`: start tags inside are HTML (except `mglyph`,
    /// `malignmark`).
    MathText,
}

/// Where the tokenizer is with respect to SVG and MathML: the elements opened since the
/// outermost `svg` or `math` (empty in HTML content). Only as precise as the guard needs: it
/// decides whether `style`, `script`, `title`, `textarea` … are raw text (HTML elements) or
/// markup (foreign elements), and whether `<![CDATA[` starts a CDATA section.
#[derive(Default)]
struct Foreign {
    stack: Vec<(String, Kind)>,
    /// HTML elements open outside SVG and MathML, by name (an estimate from above): an end tag
    /// for one of them may end the foreign content around it.
    outer: std::collections::HashMap<String, usize>,
    /// The model cannot tell what the parser does next (see [`Foreign::end`]).
    lost: bool,
}

impl Foreign {
    fn current(&self) -> Option<Kind> {
        self.stack.last().map(|(_, kind)| *kind)
    }

    /// CDATA sections exist where the current node is an SVG or MathML element.
    fn cdata_sections(&self) -> bool {
        matches!(
            self.current(),
            Some(Kind::Foreign { .. } | Kind::HtmlIntegration | Kind::MathText)
        )
    }

    /// Whether a start tag `name` is processed by the HTML rules here.
    fn html_rules(&self, name: &str) -> bool {
        match self.current() {
            None | Some(Kind::Html | Kind::HtmlIntegration) => true,
            Some(Kind::MathText) => !matches!(name, "mglyph" | "malignmark"),
            Some(Kind::Foreign { .. }) => false,
        }
    }

    /// A start tag; `tag` is its source. Returns whether it creates an HTML element.
    fn start(&mut self, name: &str, self_closing: bool, tag: &str) -> bool {
        if !self.html_rules(name) {
            let lower = tag.to_ascii_lowercase();
            let font_breakout = name == "font"
                && ["color", "face", "size"]
                    .iter()
                    .any(|attribute| has_attribute(&lower, attribute));
            if BREAKOUT.contains(&name) || font_breakout {
                while matches!(self.current(), Some(Kind::Foreign { .. })) {
                    self.stack.pop();
                }
                return self.start(name, self_closing, tag);
            }
            // A new element takes the namespace of the current node: `<svg>` inside MathML is
            // a MathML element (and its `foreignObject` no integration point). Only
            // `annotation-xml` hands `<svg>` to the HTML rules, which start SVG.
            let svg = match self.stack.last() {
                Some((open, _)) if open == "annotation-xml" && name == "svg" => true,
                Some((_, Kind::Foreign { svg })) => *svg,
                _ => false,
            };
            if !self_closing {
                let kind = if svg && matches!(name, "foreignobject" | "desc" | "title") {
                    Kind::HtmlIntegration
                } else if !svg && matches!(name, "mi" | "mo" | "mn" | "ms" | "mtext") {
                    Kind::MathText
                } else if !svg
                    && name == "annotation-xml"
                    && (lower.contains("text/html") || lower.contains("application/xhtml+xml"))
                {
                    Kind::HtmlIntegration
                } else {
                    Kind::Foreign { svg }
                };
                self.stack.push((name.to_owned(), kind));
            }
            return false;
        }
        if name == "svg" || name == "math" {
            if !self_closing {
                self.stack
                    .push((name.to_owned(), Kind::Foreign { svg: name == "svg" }));
            }
            return false;
        }
        if VOID.contains(&name) || matches!(name, "html" | "head" | "body") {
            return true;
        }
        if self.stack.is_empty() {
            *self.outer.entry(name.to_owned()).or_default() += 1;
        } else {
            self.stack.push((name.to_owned(), Kind::Html));
        }
        true
    }

    /// An end tag (HTML standard: "any other end tag" in body and in foreign content). `</p>`
    /// and `</br>` end SVG and MathML content like the start tags in [`BREAKOUT`].
    fn end(&mut self, name: &str) {
        if self.stack.is_empty() {
            if let Some(count) = self.outer.get_mut(name) {
                *count = count.saturating_sub(1);
            }
            return;
        }
        if matches!(self.current(), Some(Kind::Foreign { .. })) {
            if matches!(name, "p" | "br") {
                while matches!(self.current(), Some(Kind::Foreign { .. })) {
                    self.stack.pop();
                }
                return;
            }
            // Through SVG and MathML elements to the nearest one of that name; at the first
            // HTML element the HTML rules take over.
            for index in (0..self.stack.len()).rev() {
                let (open, kind) = &self.stack[index];
                if open == name {
                    self.stack.truncate(index);
                    return;
                }
                if *kind == Kind::Html {
                    break;
                }
            }
        }
        // HTML rules: the nearest HTML element of that name, unless a special element (the
        // integration points are special) lies above it.
        for index in (0..self.stack.len()).rev() {
            let (open, kind) = &self.stack[index];
            if *kind == Kind::Html && open == name {
                self.stack.truncate(index);
                return;
            }
            let special = match kind {
                Kind::Html => SPECIAL.contains(&open.as_str()),
                Kind::HtmlIntegration | Kind::MathText => true,
                Kind::Foreign { .. } => open == "annotation-xml",
            };
            if special {
                return;
            }
        }
        // Out of SVG and MathML without a stop: if an HTML element of that name is still open
        // around them, the parser closes it and the foreign content with it; if the parser
        // closed it already (implied end tags, `</table>` …), it ignores the end tag. The count
        // is only an estimate, and guessing wrong either way would let markup hide (raw text
        // that is markup, or markup that is raw text): the scan gives up instead.
        if self.outer.get(name).is_some_and(|count| *count > 0) {
            self.lost = true;
        }
    }
}

/// Whether the source of a start tag (lower case) has an attribute `name`: the name after white
/// space or `/`, followed by `=`, white space, `/`, `>` or the end. A quoted value that looks
/// like an attribute counts too (an estimate).
fn has_attribute(tag: &str, name: &str) -> bool {
    tag.match_indices(name).any(|(at, _)| {
        let before = tag[..at].chars().next_back();
        let after = tag[at + name.len()..].chars().next();
        matches!(before, Some(' ' | '\t' | '\n' | '\r' | '\x0c' | '/'))
            && matches!(
                after,
                None | Some('=' | ' ' | '\t' | '\n' | '\r' | '\x0c' | '/' | '>')
            )
    })
}

/// Position of `</name` (ASCII case-insensitive) in `haystack`.
fn find_end_tag(haystack: &[u8], name: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = haystack.get(from..)?.windows(2).position(|w| w == b"</") {
        let at = from + offset;
        let candidate = haystack.get(at + 2..at + 2 + name.len())?;
        if candidate.eq_ignore_ascii_case(name) {
            return Some(at);
        }
        from = at + 2;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_html_passes() {
        let newsletter =
            "<html><body><table><tr><td><p>Hi <b>there</b><p>Second<ul><li>a<li>b</ul>\
                          <font><font>unclosed</td></tr></table><svg><path d='M0'/><path/></svg>"
                .repeat(2_000);
        assert_eq!(check(&newsletter), Ok(()));
        assert_eq!(check(&"<p>para".repeat(10_000)), Ok(()));
        // Misnested but common: cells and paragraphs with unclosed inline elements.
        assert_eq!(
            check(&"<table><tr><td><font><b>x</td><td><p><i>y</p></td></tr></table>".repeat(5_000)),
            Ok(())
        );
    }

    #[test]
    fn pathological_html_is_refused() {
        assert_eq!(
            check(&"<div>".repeat(MAX_DEPTH + 1)),
            Err(TooComplex::Depth)
        );
        // End tags the parser ignores do not lower the estimate.
        assert_eq!(
            check(&"<span><div></span>".repeat(1_000)),
            Err(TooComplex::Depth)
        );
        assert_eq!(
            check(&"<br>".repeat(MAX_START_TAGS + 1)),
            Err(TooComplex::StartTags)
        );
        let mut bomb = String::from("<p>");
        for i in 0..1_000 {
            bomb.push_str(&format!("<b id={i}>"));
        }
        bomb.push_str("</p>");
        bomb.push_str(&"<p>x</p>".repeat(3_000));
        // The form end tag removes only the form; `</body>` closes nothing.
        assert_eq!(
            check(&"<form><div></form>".repeat(MAX_DEPTH + 1)),
            Err(TooComplex::Depth)
        );
        assert_eq!(
            check(&"<div></body>".repeat(MAX_DEPTH + 1)),
            Err(TooComplex::Depth)
        );
        assert_eq!(check(&bomb), Err(TooComplex::Formatting));
    }

    #[test]
    fn tags_inside_quotes_comments_and_raw_text_do_not_count() {
        let quoted = format!("<div title=\"{}\">x</div>", "</div>".repeat(5_000));
        assert_eq!(check(&quoted), Ok(()));
        let hidden = format!(
            "<!-- {} --><style>{}</style>",
            "<div>".repeat(5_000),
            "<div>".repeat(5_000)
        );
        assert_eq!(check(&hidden), Ok(()));
    }

    /// Stage 15 review (finding C-1): where the scan read the input differently from the
    /// html5ever tokenizer, deep nesting behind the difference went unseen.
    #[test]
    fn tokenizer_differences_do_not_hide_nesting() {
        let deep = "<div>".repeat(MAX_DEPTH + 2);
        for prefix in [
            "<!-->",
            "<!--->",
            "<!-- --!>",
            "<!-- x --!>",
            "<a =\"",
            "<a ='",
            "<svg><style>",
            "<svg><title>",
            "<svg><script>",
            "<svg><textarea>",
            "<svg><noscript>",
            "<svg><xmp>",
            "<math><style>",
            "<math><title>",
            "<svg><![CDATA[ > <!-- ]]>",
            "<math><![CDATA[ <style> ]]>",
            "<svg><g><b><style><!--</style>",
            "<div><svg></div><style><!--</style>",
            "<svg><foreignObject><style><!--</style>",
            "<math><mtext><style><!--</style>",
            "<math><annotation-xml encoding=\"text/html\"><style><!--</style>",
            "<svg><font color=red><style><!--</style>",
            "<svg></p><style><!--</style>",
            // `svg` inside MathML is MathML: no integration point, no raw text.
            "<math><svg><foreignObject><title>",
            "<math><svg><desc><style>",
            // An end tag for an element the parser may or may not still have open.
            "<table><td><div></table><svg></div><style>",
            "<svg><font size=2><style><!--</style>",
        ] {
            assert_eq!(
                check(&format!("{prefix}{deep}")),
                Err(TooComplex::Depth),
                "{prefix}"
            );
        }
        // Elements the parser closes itself (`option`) pile up in the model of foreign content;
        // past the depth limit the scan gives up instead of getting slow.
        assert_eq!(
            check(&format!(
                "<svg><foreignObject>{}",
                "<option>".repeat(MAX_DEPTH + 1)
            )),
            Err(TooComplex::Depth)
        );
        // Still raw text where the parser treats it so.
        let hidden = format!("<style>{deep}</style><svg><foreignObject><style>{deep}</style>");
        assert_eq!(check(&hidden), Ok(()));
        let cdata = format!("<svg><![CDATA[{deep}]]></svg>");
        assert_eq!(check(&cdata), Ok(()));
        // `font` leaves SVG only with a color, face or size attribute, not with the word in a
        // value: here `style` stays an SVG element and the comment hides the rest from the
        // parser too.
        let font = format!("<svg><font class=\" color\"><style><!--</style>{deep}");
        assert_eq!(check(&font), Ok(()));
    }

    #[test]
    fn text_is_extracted_without_parsing() {
        assert_eq!(
            strip_tags("<p>a &amp; b</p><script>x()</script><div>c&nbsp;d&#x41;&#66;&bogus;</div>"),
            "\na & b\nc dAB&bogus;\n"
        );
    }
}
