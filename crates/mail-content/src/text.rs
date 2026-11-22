//! Plain text for the reading pane: lines with their quote depth and recognised links, as data.
//! The interface renders spans as text nodes and links as elements of its own, so no text from
//! a mail is ever interpreted as HTML. Also the HTML-to-text conversion for the text view and
//! the preview.

use url::Url;

use crate::dom::{self, Descend, NodeData, Visit};
use crate::guard;
use crate::html::LinkTarget;

/// Longest text that is turned into lines; the rest is cut (the parser already limits bodies).
pub const MAX_TEXT_BYTES: usize = 4 << 20;

/// Deepest quote level reported; deeper quotes count as this level.
const MAX_QUOTE_DEPTH: u8 = 32;

/// A text body prepared for display.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedText {
    pub lines: Vec<TextLine>,
    /// Targets of the links in `lines`, by index. The interface asks the core to open link `n`;
    /// the core checks it with [`crate::check_link`].
    pub links: Vec<LinkTarget>,
}

/// One line of text. Quote markers (`>`) are removed and counted in `quote_depth`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextLine {
    pub quote_depth: u8,
    pub spans: Vec<TextSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSpan {
    Text(String),
    /// A recognised URL or e-mail address; `index` points into [`RenderedText::links`].
    Link {
        index: u32,
        text: String,
    },
}

/// Splits text into lines, counts quote markers and recognises `http(s)://`, `www.`, `mailto:`
/// and bare e-mail addresses. Control characters other than tab are removed.
pub fn render_text(text: &str) -> RenderedText {
    let text = &text[..text.floor_char_boundary(MAX_TEXT_BYTES)];
    let mut rendered = RenderedText::default();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let (quote_depth, content) = split_quote(raw);
        let content: String = content
            .chars()
            .filter(|c| *c == '\t' || !c.is_control())
            .collect();
        let spans = linkify(&content, &mut rendered.links);
        rendered.lines.push(TextLine { quote_depth, spans });
    }
    rendered
}

/// Quote depth of a line (`>`, `>>`, `> >`) and the line without the markers.
fn split_quote(line: &str) -> (u8, &str) {
    let mut depth: u8 = 0;
    let mut rest = line;
    loop {
        if let Some(after) = rest.strip_prefix('>') {
            depth = depth.saturating_add(1);
            rest = after;
        } else if depth > 0
            && let Some(after) = rest.strip_prefix(" >")
        {
            depth = depth.saturating_add(1);
            rest = after;
        } else {
            break;
        }
    }
    if depth > 0 {
        rest = rest.strip_prefix(' ').unwrap_or(rest);
    }
    (depth.min(MAX_QUOTE_DEPTH), rest)
}

fn linkify(line: &str, links: &mut Vec<LinkTarget>) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut last = 0;
    let mut offset = 0;
    for token in line.split_inclusive(char::is_whitespace) {
        let start = offset;
        offset += token.len();
        let word = token.trim_end_matches(char::is_whitespace);
        let Some((from, to, href)) = find_link(word) else {
            continue;
        };
        let (from, to) = (start + from, start + to);
        if from > last {
            spans.push(TextSpan::Text(line[last..from].to_owned()));
        }
        let index = u32::try_from(links.len()).unwrap_or(u32::MAX);
        let text = line[from..to].to_owned();
        links.push(LinkTarget {
            index,
            href,
            text: text.clone(),
        });
        spans.push(TextSpan::Link { index, text });
        last = to;
    }
    if last < line.len() || spans.is_empty() {
        spans.push(TextSpan::Text(line[last..].to_owned()));
    }
    spans
}

/// Finds a link in a whitespace-free word: byte range and target.
fn find_link(word: &str) -> Option<(usize, usize, String)> {
    let lower = word.to_ascii_lowercase();
    let at_boundary = |pos: usize| {
        pos == 0
            || !word[..pos]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '/' || c == '@')
    };
    let start = ["https://", "http://", "mailto:", "www."]
        .iter()
        .filter_map(|prefix| lower.find(prefix))
        .filter(|pos| at_boundary(*pos))
        .min();
    if let Some(start) = start {
        // `<`, `>` and `"` cannot be part of a URL in text (RFC 3986 appendix C).
        let stop = word[start..]
            .find(['<', '>', '"'])
            .map_or(word.len(), |p| start + p);
        let end = start + trim_link_end(&word[start..stop]);
        let core = &word[start..end];
        let core_lower = core.to_ascii_lowercase();
        let href = if core_lower.starts_with("www.") {
            format!("https://{core}")
        } else {
            core.to_owned()
        };
        let valid = if core_lower.starts_with("mailto:") {
            core.len() > 7 && core.contains('@')
        } else {
            Url::parse(&href).is_ok_and(|url| {
                url.host_str()
                    .is_some_and(|host| host.contains('.') || host == "localhost")
            })
        };
        return valid.then_some((start, end, href));
    }
    // A bare e-mail address.
    let at = word.find('@')?;
    let begin = word[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || "._%+-".contains(*c))
        .last()
        .map_or(at, |(i, _)| i);
    let end = at + 1 + trim_link_end(&word[at + 1..]);
    let local = &word[begin..at];
    let domain = &word[at + 1..end];
    let labels: Vec<&str> = domain.split('.').collect();
    let valid_domain = labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && labels
            .last()
            .is_some_and(|tld| tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic));
    let local_ok = !local.is_empty() && !local.starts_with('.') && !local.ends_with('.');
    (valid_domain && local_ok).then(|| (begin, end, format!("mailto:{}", &word[begin..end])))
}

/// Length of a link candidate without trailing punctuation and unbalanced closing brackets.
fn trim_link_end(candidate: &str) -> usize {
    let mut end = candidate.len();
    loop {
        let Some(last) = candidate[..end].chars().next_back() else {
            return end;
        };
        let strip = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '>' | '<' => true,
            ')' => count(&candidate[..end], '(') < count(&candidate[..end], ')'),
            ']' => count(&candidate[..end], '[') < count(&candidate[..end], ']'),
            '}' => count(&candidate[..end], '{') < count(&candidate[..end], '}'),
            _ => false,
        };
        if !strip {
            return end;
        }
        end -= last.len_utf8();
    }
}

fn count(text: &str, wanted: char) -> usize {
    text.chars().filter(|c| *c == wanted).count()
}

/// Text of an HTML body for the text view: block structure as line breaks, `<blockquote>` as
/// `> ` quotes, link targets in angle brackets after the link text when the text does not show
/// them. Scripts, styles and other non-content is skipped.
pub fn html_to_text(html: &str) -> String {
    html_text(html, true)
}

/// Elements whose content is never text the reader sees.
const SKIPPED: &[&str] = &[
    "head", "script", "style", "template", "noscript", "title", "iframe", "object", "embed",
    "select", "textarea", "noembed", "noframes", "xmp", "button",
];

/// Elements that end a paragraph (blank line) or a line.
const PARAGRAPH: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "ul",
    "ol",
    "table",
    "hr",
    "pre",
    "dl",
    "figure",
];
const LINE: &[&str] = &[
    "div",
    "br",
    "li",
    "tr",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "aside",
    "main",
    "address",
    "center",
    "dt",
    "dd",
    "caption",
    "details",
    "summary",
    "figcaption",
    "form",
    "fieldset",
    "legend",
    "tbody",
    "thead",
    "tfoot",
];

pub(crate) fn html_text(html: &str, with_link_targets: bool) -> String {
    let html = &html[..html.floor_char_boundary(MAX_TEXT_BYTES * 2)];
    if guard::check(html).is_err() {
        let mut out = TextBuilder::default();
        for line in guard::strip_tags(html).split('\n') {
            out.line_break(1);
            out.text(line);
        }
        return out.finish();
    }
    let dom = dom::parse_document(html);
    let mut out = TextBuilder::default();
    let mut link_stack: Vec<(Option<String>, usize)> = Vec::new();
    dom::walk(&dom.document, |visit| match visit {
        Visit::Enter(node) => {
            match &node.data {
                NodeData::Text { contents } => {
                    out.text(&contents.borrow());
                    return Descend::Yes;
                }
                NodeData::Element { .. } => {}
                _ => return Descend::Yes,
            }
            let Some(name) = dom::html_name(node) else {
                // SVG and MathML: no reading text.
                return Descend::Skip;
            };
            if SKIPPED.contains(&name) {
                return Descend::Skip;
            }
            match name {
                "br" => out.line_break(1),
                "td" | "th" => out.cell(),
                "blockquote" => {
                    out.line_break(2);
                    out.quote_depth += 1;
                }
                "pre" => {
                    out.line_break(2);
                    out.pre_depth += 1;
                }
                "li" => {
                    out.line_break(1);
                    out.text("• ");
                }
                "a" => link_stack.push((dom::attribute(node, "href"), out.text.len())),
                _ if PARAGRAPH.contains(&name) => out.line_break(2),
                _ if LINE.contains(&name) => out.line_break(1),
                _ => {}
            }
            Descend::Yes
        }
        Visit::Leave(node) => {
            let Some(name) = dom::html_name(node) else {
                return Descend::Yes;
            };
            match name {
                "blockquote" => {
                    out.line_break(2);
                    out.quote_depth = out.quote_depth.saturating_sub(1);
                }
                "pre" => {
                    out.pre_depth = out.pre_depth.saturating_sub(1);
                    out.line_break(2);
                }
                "a" => {
                    if let Some((Some(href), start)) = link_stack.pop()
                        && with_link_targets
                    {
                        let shown = out.text.get(start..).unwrap_or("");
                        let target = href.trim();
                        let address = target.strip_prefix("mailto:").unwrap_or(target);
                        let wanted = ["http://", "https://", "mailto:"]
                            .iter()
                            .any(|p| target.to_ascii_lowercase().starts_with(p));
                        if wanted && !shown.contains(address) && !address.is_empty() {
                            out.text(&format!(" <{target}>"));
                        }
                    }
                }
                _ if PARAGRAPH.contains(&name) => out.line_break(2),
                _ if LINE.contains(&name) && name != "br" => out.line_break(1),
                _ => {}
            }
            Descend::Yes
        }
    });
    out.finish()
}

/// Builds text with lazily inserted line breaks, so no trailing blank lines pile up.
#[derive(Default)]
struct TextBuilder {
    text: String,
    pending_breaks: u8,
    pending_space: bool,
    at_line_start: bool,
    quote_depth: usize,
    last_depth: usize,
    pre_depth: usize,
    cell_started: bool,
}

impl TextBuilder {
    fn line_break(&mut self, breaks: u8) {
        if !self.text.is_empty() {
            self.pending_breaks = self.pending_breaks.max(breaks);
        }
        self.pending_space = false;
        self.cell_started = false;
    }

    fn cell(&mut self) {
        if self.cell_started {
            self.pending_space = true;
        }
        self.cell_started = true;
    }

    fn start_line(&mut self) {
        let depth = self.quote_depth.min(usize::from(MAX_QUOTE_DEPTH));
        for _ in 0..depth {
            self.text.push_str("> ");
        }
        self.last_depth = depth;
        self.at_line_start = false;
    }

    fn flush_breaks(&mut self) {
        if self.pending_breaks > 0 {
            while self.text.ends_with(' ') {
                self.text.pop();
            }
            self.text.push('\n');
            // Blank lines inside a quote keep their markers, so the quote stays one block.
            let depth = self.last_depth.min(self.quote_depth);
            for _ in 1..self.pending_breaks {
                let markers = "> ".repeat(depth);
                self.text.push_str(markers.trim_end());
                self.text.push('\n');
            }
            self.pending_breaks = 0;
            self.at_line_start = true;
            self.pending_space = false;
        }
        if self.text.is_empty() {
            self.at_line_start = true;
        }
    }

    fn text(&mut self, text: &str) {
        if self.pre_depth > 0 {
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    self.pending_breaks = self.pending_breaks.max(1);
                    if self.text.is_empty() {
                        self.text.push('\n');
                    }
                }
                if line.is_empty() {
                    continue;
                }
                self.flush_breaks();
                if self.at_line_start {
                    self.start_line();
                }
                self.text.push_str(line);
            }
            return;
        }
        for (i, word) in text
            .split(|c: char| c.is_ascii_whitespace() || c == '\u{a0}')
            .enumerate()
        {
            if i > 0 {
                self.pending_space = true;
            }
            if word.is_empty() {
                continue;
            }
            self.flush_breaks();
            if self.at_line_start {
                self.start_line();
            } else if self.pending_space {
                self.text.push(' ');
            }
            self.pending_space = false;
            self.text.push_str(word);
        }
    }

    fn finish(self) -> String {
        let mut text = self.text;
        while text.ends_with([' ', '\n']) {
            text.pop();
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &TextLine) -> String {
        line.spans
            .iter()
            .map(|s| match s {
                TextSpan::Text(t) | TextSpan::Link { text: t, .. } => t.as_str(),
            })
            .collect()
    }

    #[test]
    fn quotes_and_links_become_data() {
        let text = "Hi <b>Alice</b>,\r\nsee https://example.com/a_(b), or www.example.org.\n\
                    > quoted mailto:x@example.com\n>> deeper\n> > also deeper\n\
                    Write to info@example.com! Not a link: a@b, x.y, http://nohost";
        let r = render_text(text);
        assert_eq!(r.lines.len(), 6);
        assert_eq!(line_text(&r.lines[0]), "Hi <b>Alice</b>,");
        assert_eq!(
            r.lines[0].spans,
            [TextSpan::Text("Hi <b>Alice</b>,".into())],
            "markup stays text"
        );
        assert_eq!(
            r.lines.iter().map(|l| l.quote_depth).collect::<Vec<_>>(),
            [0, 0, 1, 2, 2, 0]
        );
        let hrefs: Vec<&str> = r.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(
            hrefs,
            [
                "https://example.com/a_(b)",
                "https://www.example.org",
                "mailto:x@example.com",
                "mailto:info@example.com"
            ]
        );
        assert_eq!(
            line_text(&r.lines[1]),
            "see https://example.com/a_(b), or www.example.org."
        );
        assert!(
            matches!(&r.lines[1].spans[1], TextSpan::Link { index: 0, text } if text == "https://example.com/a_(b)")
        );
        assert_eq!(line_text(&r.lines[2]), "quoted mailto:x@example.com");
    }

    #[test]
    fn javascript_and_control_characters_do_not_survive() {
        let r = render_text("javascript:alert(1) <https://ok.example/x>\u{0}\u{7}end");
        assert_eq!(r.links.len(), 1);
        assert_eq!(r.links[0].href, "https://ok.example/x");
        assert_eq!(
            line_text(&r.lines[0]),
            "javascript:alert(1) <https://ok.example/x>end"
        );
    }

    #[test]
    fn html_becomes_readable_text() {
        let html = "<html><head><title>T</title><style>p{color:red}</style></head><body>\
                    <h1>Title</h1><p>First <b>bold</b>\n paragraph.</p><p>Second&nbsp;one\
                    <br>next line</p><ul><li>one</li><li>two</li></ul>\
                    <blockquote><p>quoted</p><blockquote>deeper</blockquote></blockquote>\
                    <table><tr><td>a</td><td>b</td></tr></table>\
                    <a href='https://example.com/x'>Link</a> <a href='https://example.com/y'>https://example.com/y</a>\
                    <script>evil()</script><svg><text>svg</text></svg><pre>  keep\n   spaces</pre></body></html>";
        assert_eq!(
            html_to_text(html),
            "Title\n\nFirst bold paragraph.\n\nSecond one\nnext line\n\n• one\n• two\n\n\
             > quoted\n>\n> > deeper\n\na b\n\nLink <https://example.com/x> https://example.com/y\n\n  keep\n   spaces"
        );
    }
}
