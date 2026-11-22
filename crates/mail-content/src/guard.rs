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
//! values, comments, raw-text elements) and estimates the depth from above: it only pops an
//! element when the parser certainly does (an end tag for the current element, and the usual
//! implied closings of `p`, `li`, `dd`/`dt`, `option`, `tr`, `td`/`th`).

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
    scan(html, &mut |event| {
        if result.is_err() {
            return false;
        }
        result = scanner.event(event);
        result.is_ok()
    });
    result
}

/// Text of HTML without parsing it: tags removed, a line break for block elements, the common
/// entities decoded. For HTML that [`check`] refuses.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    scan(html, &mut |event| {
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
/// `false` to stop.
fn scan(html: &str, visit: &mut dyn FnMut(Event<'_>) -> bool) {
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
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
                        return;
                    }
                    let end = if html[i..].starts_with("<!--") {
                        html[i + 4..]
                            .find("-->")
                            .map_or(bytes.len(), |p| i + 4 + p + 3)
                    } else {
                        html[i..].find('>').map_or(bytes.len(), |p| i + p + 1)
                    };
                    i = end;
                    text_start = i;
                }
                Some(b'?') | Some(b'/') => {
                    if text_start < i && !visit(Event::Text(&html[text_start..i])) {
                        return;
                    }
                    i = html[i..].find('>').map_or(bytes.len(), |p| i + p + 1);
                    text_start = i;
                }
                _ => i += 1,
            }
            continue;
        }
        if text_start < i && !visit(Event::Text(&html[text_start..i])) {
            return;
        }
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
                Some(_) => {
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
            if !visit(Event::End { name }) {
                return;
            }
            continue;
        }
        let raw_text = RAW_TEXT.contains(&name.as_str());
        let plaintext = name == "plaintext";
        if !visit(Event::Start {
            name: name.clone(),
            self_closing,
        }) {
            return;
        }
        if plaintext {
            if i < bytes.len() {
                visit(Event::Text(&html[i..]));
            }
            return;
        }
        if raw_text {
            match find_end_tag(&bytes[i..], name.as_bytes()) {
                Some(p) => {
                    i += p;
                    text_start = i;
                }
                None => return,
            }
        }
    }
    if text_start < bytes.len() {
        visit(Event::Text(&html[text_start..]));
    }
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

    #[test]
    fn text_is_extracted_without_parsing() {
        assert_eq!(
            strip_tags("<p>a &amp; b</p><script>x()</script><div>c&nbsp;d&#x41;&#66;&bogus;</div>"),
            "\na & b\nc dAB&bogus;\n"
        );
    }
}
