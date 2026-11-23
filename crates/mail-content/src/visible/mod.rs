//! Text a reader of the mail does not see, so it can be left out of what models get
//! ([`crate::html_to_visible_text`]; decision 0015, "Offen": per CSS versteckter Text).
//!
//! HTML mails hide text from people with CSS and attributes: preheaders with `display:none`,
//! white on white, `font-size:0`, boxes of height 0, text moved off the screen. A model reads it
//! all, so instructions for an AI assistant hide there. The check walks the same DOM as the
//! text view and, for every element the reading pane shows, computes the few properties that
//! can hide text from the sanitised `<style>` rules ([`cascade`]), the sanitised `style`
//! attribute and the presentational attributes, the way the pane does: cascade with
//! specificity and `!important`, inheritance, the user-agent defaults that matter (link colour,
//! `<mark>`, `hidden`).
//!
//! An element and everything in it is hidden by `display:none`, the `hidden` attribute (unless
//! the mail sets `display`), `content-visibility:hidden`, an opacity of at most
//! [`HIDDEN_OPACITY`] (also `filter:opacity()`, multiplied through the ancestors), a box of at
//! most [`MIN_BOX_PX`] with overflow clipped (`height`, `max-height`, `width`, `max-width`, not
//! on inline elements and table parts, where browsers ignore them), an offset of at least
//! [`OFFSCREEN_PX`] off the left or top edge (`left`/`top`/`right`/`bottom` when positioned,
//! `margin-left`, `margin-top`, `translate`), `scale(0)`, `clip: rect()` of no area when
//! absolutely positioned, and `clip-path` of no area (`inset(50%)`, `circle(0)`).
//!
//! A text is hidden where it inherits `visibility:hidden`/`collapse`, a font size below
//! [`MIN_FONT_PX`], a `text-indent` of at least [`OFFSCREEN_PX`] to the left, a text colour with
//! an alpha of at most [`TRANSPARENT_ALPHA`], or a colour with a contrast ratio below
//! [`MIN_CONTRAST`] to its backdrop: the nearest background colour of the element or an
//! ancestor (`bgcolor` included, alpha blended), white where none is set. A background image or
//! gradient makes the backdrop unknown, and then only transparency counts; text drawn by
//! `text-shadow`, `-webkit-text-stroke` or `background-clip:text` is visible whatever its
//! colour. `<template>` and `<noscript>` are never shown either and are counted as hidden.
//!
//! Decisions:
//! - `aria-hidden` alone hides nothing: sighted readers see the text. Visually hidden text for
//!   screen readers (`sr-only`) is left out, because it is hidden the same way.
//! - The thresholds are the points where text stops being readable, not where it is merely
//!   small or faint: 4 px is less than half the smallest size mails use for fine print (9 px);
//!   a contrast of 1.1 is `#f2f2f2` on white, far below the 4.5 accessibility needs; text at 5 %
//!   opacity is the same. A smaller font or a fainter colour is visible to someone who looks.
//! - Media queries are evaluated for a reading pane 800 px wide in the light appearance
//!   ([`values::media_matches`]); conditions it cannot evaluate count as true.
//! - Selectors: see [`cascade`]. Matching stops after [`MATCH_BUDGET`] steps per mail; the rest
//!   of the mail then counts as hidden (fail closed), so complexity is no way around the check.
//! - HTML the text view does not parse (too deep or too many tags, `crate::guard`) is shown as
//!   plain text in the reading pane too; nothing in it is hidden from the reader, and nothing
//!   is left out.
//!
//! Limits: text over a background image, text placed over an `<img>` or another element with
//! positioning, negative `letter-spacing`, `::first-line` and other pseudo-element styles,
//! `polygon()` clip paths and offsets of less than [`OFFSCREEN_PX`] combined with a small box
//! are not recognised. `@media (prefers-color-scheme: dark)` rules are not applied, so text that
//! only dark mode hides is kept. Siblings of elements the sanitiser unwraps are approximated.

mod cascade;
mod values;

#[cfg(test)]
mod tests;

use cascade::{AncestorKeys, Applied, Budget, Declaration, PROP_COUNT, Prop, StyleSheet};
use cssparser::{Parser, Token};
use values::{Basis, Color, Rgba, VIEWPORT_HEIGHT, VIEWPORT_WIDTH};

use crate::css;
use crate::dom::{self, Descend, Dom, Handle, NodeData, Visit};
use crate::guard;

/// Text of an HTML body without the text a reader does not see.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleText {
    pub text: String,
    /// Characters (not counting white space) of the text left out because it is hidden.
    pub hidden_chars: usize,
}

/// Text below this font size (CSS px) is hidden.
pub const MIN_FONT_PX: f32 = 4.0;
/// An element at this opacity or less is hidden.
pub const HIDDEN_OPACITY: f32 = 0.05;
/// Text with this alpha or less is hidden.
pub const TRANSPARENT_ALPHA: f32 = 0.05;
/// Text with less contrast to its backdrop is hidden.
pub const MIN_CONTRAST: f32 = 1.1;
/// A clipped box this small (CSS px) or smaller hides its content.
pub const MIN_BOX_PX: f32 = 1.0;
/// Content moved this far (CSS px) off the left or top edge is hidden.
pub const OFFSCREEN_PX: f32 = 500.0;
/// Selector matching steps per mail. A large newsletter needs a few hundred thousand.
pub const MATCH_BUDGET: u64 = 10_000_000;

/// Stands in for every image URL in CSS: the check only needs to know there is one.
const IMAGE: &str = "data:,";

/// Elements that are inline by default: sizes and overflow do not apply to them.
const INLINE: &[&str] = &[
    "a", "abbr", "acronym", "b", "bdi", "bdo", "big", "cite", "code", "data", "del", "dfn", "em",
    "font", "i", "ins", "kbd", "mark", "q", "rp", "rt", "s", "samp", "small", "span", "strike",
    "strong", "sub", "sup", "time", "tt", "u", "var", "br", "wbr",
];

/// Table parts: browsers grow them to their content, so a height does not clip.
const TABLE_PARTS: &[&str] = &[
    "table", "thead", "tbody", "tfoot", "tr", "td", "th", "col", "colgroup",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Paint {
    Rgba(Rgba),
    /// A colour that is not evaluated: counts as visible.
    Unknown,
    /// Relative colour syntax: counts as hidden ([`Color::Relative`]).
    Relative,
}

/// What an element passes on to its content.
#[derive(Clone, Debug)]
struct Frame {
    color: Paint,
    /// `-webkit-text-fill-color`; `None`: the text colour.
    fill: Option<Paint>,
    shadow: bool,
    stroke: bool,
    /// An ancestor paints its background into the glyphs (`background-clip:text`).
    clip_text: bool,
    font_px: Option<f32>,
    visibility_hidden: bool,
    indent_hidden: bool,
    /// The colour behind the content; `None`: unknown (an image).
    backdrop: Option<Rgba>,
    opacity: f32,
}

impl Frame {
    fn root() -> Self {
        Self {
            color: Paint::Rgba(Rgba::BLACK),
            fill: None,
            shadow: false,
            stroke: false,
            clip_text: false,
            font_px: Some(values::ROOT_FONT_PX),
            visibility_hidden: false,
            indent_hidden: false,
            backdrop: Some(Rgba::WHITE),
            opacity: 1.0,
        }
    }

    fn text_hidden(&self) -> bool {
        if self.visibility_hidden
            || self.indent_hidden
            || self.font_px.is_some_and(|px| px < MIN_FONT_PX)
        {
            return true;
        }
        if self.shadow || self.stroke || self.clip_text {
            return false;
        }
        match self.fill.unwrap_or(self.color) {
            Paint::Relative => true,
            Paint::Unknown => false,
            Paint::Rgba(color) => {
                color.a <= TRANSPARENT_ALPHA
                    || self
                        .backdrop
                        .is_some_and(|b| values::contrast(color.over(b), b) < MIN_CONTRAST)
            }
        }
    }
}

/// The visibility state during one walk over a document.
pub(crate) struct Visibility {
    sheet: StyleSheet,
    /// One per entered element: what it passes on, and the keys it added to `ancestors`.
    frames: Vec<(Frame, Vec<String>)>,
    ancestors: AncestorKeys,
    budget: Budget,
    exhausted: bool,
    link_color: Rgba,
    hidden_chars: usize,
}

impl Visibility {
    /// Reads the document's `<style>` blocks the way `crate::render_html` does.
    pub(crate) fn new(document: &Dom) -> Self {
        let css_text = crate::html::style_text(document);
        let sanitized = css::sanitize_stylesheet(&css_text, &mut |_| Some(IMAGE.to_owned()));
        Self {
            sheet: StyleSheet::parse(&sanitized),
            frames: Vec::new(),
            ancestors: AncestorKeys::default(),
            budget: Budget(MATCH_BUDGET),
            exhausted: false,
            link_color: Rgba::LINK,
            hidden_chars: 0,
        }
    }

    pub(crate) fn hidden_chars(&self) -> usize {
        self.hidden_chars
    }

    /// Matching steps used so far.
    #[cfg(test)]
    fn steps_used(&self) -> u64 {
        MATCH_BUDGET - self.budget.0
    }

    /// Enters an HTML element. `false`: the element and its content are hidden (counted, not
    /// entered; no [`Self::leave`] follows).
    pub(crate) fn enter(&mut self, node: &Handle, name: &str) -> bool {
        let parent = self
            .frames
            .last()
            .map_or_else(Frame::root, |(frame, _)| frame.clone());
        if self.exhausted || !cascade::is_kept(node) {
            self.frames.push((parent, Vec::new()));
            return true;
        }
        let mut applied: Vec<Applied<'_>> = Vec::new();
        if self
            .sheet
            .applied(node, &self.ancestors, &mut self.budget, &mut applied)
            .is_none()
        {
            self.exhausted = true;
            self.frames.push((parent, Vec::new()));
            return true;
        }
        let inline = cascade::kept_attribute(node, "style")
            .map(|style| {
                let sanitized = css::sanitize_declarations(&style, &mut |_| Some(IMAGE.to_owned()));
                cascade::parse_declarations(&sanitized)
            })
            .unwrap_or_default();
        for (position, declaration) in inline.iter().enumerate() {
            applied.push(Applied {
                declaration,
                rank: (
                    declaration.important,
                    2,
                    0,
                    0,
                    u32::try_from(position).unwrap_or(u32::MAX),
                ),
            });
        }
        let mut winners: [Option<Applied<'_>>; PROP_COUNT] = [None; PROP_COUNT];
        for item in applied {
            let slot = &mut winners[item.declaration.prop as usize];
            if slot.is_none_or(|current| item.rank >= current.rank) {
                *slot = Some(item);
            }
        }
        if name == "body"
            && let Some(link) =
                cascade::kept_attribute(node, "link").and_then(|v| values::legacy_color(&v))
        {
            self.link_color = link;
        }
        let style = Style {
            node,
            name,
            winners: &winners,
        };
        match style.frame(&parent, self.link_color) {
            Some(frame) => {
                let keys = self.ancestors.add(node);
                self.frames.push((frame, keys));
                true
            }
            None => {
                self.hidden_chars += subtree_chars(node);
                false
            }
        }
    }

    pub(crate) fn leave(&mut self) {
        if let Some((_, keys)) = self.frames.pop() {
            self.ancestors.remove(&keys);
        }
    }

    /// Whether text at the current position is hidden.
    pub(crate) fn text_hidden(&self) -> bool {
        self.exhausted
            || self
                .frames
                .last()
                .is_some_and(|(frame, _)| frame.text_hidden())
    }

    /// Counts a text that is left out.
    pub(crate) fn hide_text(&mut self, text: &str) {
        self.hidden_chars += visible_chars(text);
    }

    /// Counts the content of a `<template>` or `<noscript>`, which is never shown.
    pub(crate) fn hide_unrendered(&mut self, node: &Handle) {
        self.hidden_chars += unrendered_chars(node);
    }
}

fn visible_chars(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

/// Text in `node` and its descendants that the text view would have shown.
fn subtree_chars(node: &Handle) -> usize {
    let mut count = 0;
    dom::walk(node, |visit| {
        let Visit::Enter(current) = visit else {
            return Descend::Yes;
        };
        match &current.data {
            NodeData::Text { contents } => {
                count += visible_chars(&contents.borrow());
                Descend::Yes
            }
            NodeData::Element { .. } => match dom::html_name(current) {
                None => Descend::Skip,
                Some("template" | "noscript") => {
                    count += unrendered_chars(current);
                    Descend::Skip
                }
                Some(name) if crate::text::SKIPPED.contains(&name) => Descend::Skip,
                Some(_) => Descend::Yes,
            },
            _ => Descend::Yes,
        }
    });
    count
}

fn unrendered_chars(node: &Handle) -> usize {
    match (&node.data, dom::html_name(node)) {
        (
            NodeData::Element {
                template_contents, ..
            },
            Some("template"),
        ) => template_contents.borrow().as_ref().map_or(0, subtree_chars),
        // With scripting on (the parser's default) `<noscript>` holds its markup as text.
        (_, Some("noscript")) => visible_chars(&guard::strip_tags(&dom::text_content(node))),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------------------------
// One element

/// CSS-wide keywords.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wide {
    Inherit,
    Initial,
    /// `revert`, `revert-layer`: back to the user-agent value.
    Revert,
}

fn wide(value: &str) -> Option<Wide> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inherit" => Some(Wide::Inherit),
        "initial" => Some(Wide::Initial),
        // `unset` is `inherit` for inherited properties and `initial` for the others; the
        // callers handle it as `inherit` where that matters and nowhere else.
        "unset" => Some(Wide::Inherit),
        "revert" | "revert-layer" => Some(Wide::Revert),
        _ => None,
    }
}

/// The top-level components of a value (`0 auto`, `rect(0 0 0 0)`, `0/0`).
fn components(value: &str) -> Vec<&str> {
    let mut input = Parser::new(value);
    let mut out = Vec::new();
    loop {
        let start = input.position();
        match input.next_including_whitespace() {
            Err(_) => break,
            Ok(Token::WhiteSpace(_)) => continue,
            Ok(
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock,
            ) => {
                let _ = input.parse_nested_block(|inner| {
                    while inner.next().is_ok() {}
                    Ok::<(), cssparser::ParseError<()>>(())
                });
            }
            Ok(_) => {}
        }
        out.push(input.slice_from(start));
        if out.len() > 64 {
            break;
        }
    }
    out
}

/// The part of a declaration's value that sets `prop` (`margin: 0 0 0 -9999px` → left).
fn part(declaration: &Declaration, prop: Prop) -> Option<&str> {
    let value: &str = &declaration.value;
    let pick = |indexes: &[usize]| -> Option<&str> {
        let parts = components(value);
        if parts.len() > 4 {
            return None;
        }
        indexes.iter().find_map(|i| parts.get(*i).copied())
    };
    match (declaration.name, prop) {
        ("overflow", Prop::OverflowX) | ("margin" | "inset", Prop::Top) => pick(&[0]),
        ("overflow", Prop::OverflowY) => pick(&[1, 0]),
        ("margin", Prop::MarginTop) => pick(&[0]),
        ("margin", Prop::MarginLeft) | ("inset", Prop::Left) => pick(&[3, 1, 0]),
        ("inset", Prop::Right) => pick(&[1, 0]),
        ("inset", Prop::Bottom) => pick(&[2, 0]),
        ("inset-block", Prop::Top) | ("inset-inline", Prop::Left) => pick(&[0]),
        ("inset-block", Prop::Bottom) | ("inset-inline", Prop::Right) => pick(&[1, 0]),
        ("margin-block" | "margin-inline", _) => pick(&[0]),
        _ => Some(value),
    }
}

fn keyword(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

/// The declarations that won the cascade for one element.
struct Style<'a, 'd> {
    node: &'a Handle,
    name: &'a str,
    winners: &'a [Option<Applied<'d>>; PROP_COUNT],
}

impl Style<'_, '_> {
    fn get(&self, prop: Prop) -> Option<&Declaration> {
        self.winners[prop as usize].map(|applied| applied.declaration)
    }

    /// The winning value for `prop` (its part of a shorthand), unless it is a CSS-wide keyword.
    fn value(&self, prop: Prop) -> Option<&str> {
        let declaration = self.get(prop)?;
        let value = part(declaration, prop)?;
        wide(value).is_none().then_some(value)
    }

    fn wide(&self, prop: Prop) -> Option<Wide> {
        self.get(prop)
            .and_then(|declaration| part(declaration, prop))
            .and_then(wide)
    }

    fn attribute(&self, name: &str) -> Option<String> {
        cascade::kept_attribute(self.node, name)
    }

    /// The element's frame, or `None` if it hides its content.
    fn frame(&self, parent: &Frame, link_color: Rgba) -> Option<Frame> {
        let display = self.value(Prop::Display).map(keyword);
        if display.as_deref() == Some("none")
            || (self.get(Prop::Display).is_none() && self.attribute("hidden").is_some())
            || self.value(Prop::ContentVisibility).map(keyword).as_deref() == Some("hidden")
        {
            return None;
        }
        let opacity = parent.opacity * self.opacity();
        if opacity <= HIDDEN_OPACITY {
            return None;
        }
        let font_px = self.font_px(parent.font_px);
        let display = display.as_deref();
        let inline = match display {
            Some(d) => d == "inline" || d == "contents",
            None => INLINE.contains(&self.name),
        };
        let table_part = match display {
            Some(d) => d.starts_with("table") && d != "table-caption",
            None => TABLE_PARTS.contains(&self.name),
        };
        if self.box_hidden(font_px, inline, table_part) {
            return None;
        }
        let color = self.color(parent, link_color);
        let mut frame = Frame {
            color,
            fill: self.fill(parent),
            shadow: self.flag(Prop::TextShadow, parent.shadow, |v| keyword(v) != "none"),
            stroke: self.flag(Prop::TextStroke, parent.stroke, |v| {
                components(v).first().is_some_and(|w| {
                    values::length_value(w, Basis::default()).is_none_or(|px| px > 0.0)
                })
            }),
            clip_text: parent.clip_text || self.clips_text(),
            font_px,
            visibility_hidden: self.visibility(parent.visibility_hidden),
            indent_hidden: self.indent_hidden(parent.indent_hidden, font_px),
            backdrop: None,
            opacity,
        };
        frame.backdrop = self.backdrop(parent.backdrop, color);
        Some(frame)
    }

    fn opacity(&self) -> f32 {
        let own = self
            .value(Prop::Opacity)
            .and_then(values::number_value)
            .map_or(1.0, |v| v.clamp(0.0, 1.0));
        let filter = self.value(Prop::Filter).map_or(1.0, |value| {
            let mut product = 1.0;
            let mut input = Parser::new(value);
            while let Ok(token) = input.next() {
                if let Token::Function(name) = token
                    && name.eq_ignore_ascii_case("opacity")
                {
                    let amount = input
                        .parse_nested_block(|inner| {
                            let amount = values::number(inner).unwrap_or(1.0);
                            while inner.next().is_ok() {}
                            Ok::<f32, cssparser::ParseError<()>>(amount)
                        })
                        .unwrap_or(1.0);
                    product *= amount.clamp(0.0, 1.0);
                }
            }
            product
        });
        own * filter
    }

    fn font_px(&self, parent: Option<f32>) -> Option<f32> {
        // User-agent defaults and `<font size>`.
        let factor = match self.name {
            "small" | "sub" | "sup" => 1.0 / 1.2,
            "big" => 1.2,
            "h1" => 2.0,
            "h2" => 1.5,
            "h3" => 1.17,
            "h5" => 0.83,
            "h6" => 0.67,
            _ => 1.0,
        };
        let mut px = parent.map(|p| p * factor);
        if self.name == "font"
            && let Some(size) = self.attribute("size").and_then(|s| legacy_font_size(&s))
        {
            px = Some(size);
        }
        let Some(declaration) = self.get(Prop::FontSize) else {
            return px;
        };
        let value = &*declaration.value;
        match wide(value) {
            Some(Wide::Inherit) => return parent,
            Some(Wide::Initial) => return Some(values::ROOT_FONT_PX),
            Some(Wide::Revert) => return px,
            None => {}
        }
        let basis = Basis {
            em: parent,
            percent: parent,
        };
        let size = |component: &str| -> Option<Option<f32>> {
            let word = keyword(component);
            let px = match word.as_str() {
                "xx-small" => Some(9.0),
                "x-small" => Some(10.0),
                "small" => Some(13.0),
                "medium" => Some(16.0),
                "large" => Some(18.0),
                "x-large" => Some(24.0),
                "xx-large" => Some(32.0),
                "xxx-large" => Some(48.0),
                "larger" => parent.map(|p| p * 1.2),
                "smaller" => parent.map(|p| p / 1.2),
                _ => {
                    let mut input = Parser::new(component);
                    // A length or percentage; a plain number is a font weight in `font`.
                    let starts_like_length = match input.next() {
                        Ok(
                            Token::Dimension { .. } | Token::Percentage { .. } | Token::Function(_),
                        ) => true,
                        Ok(Token::Number { value, .. }) => *value == 0.0,
                        _ => false,
                    };
                    if !starts_like_length {
                        return None;
                    }
                    let length = values::length_value(component, basis);
                    if length.is_some_and(|px| px < 0.0) {
                        return None;
                    }
                    length
                }
            };
            Some(px)
        };
        let found = if declaration.name == "font" {
            components(value).into_iter().find_map(size)
        } else {
            size(value)
        };
        match found {
            Some(size) => size,
            // Invalid: the browser ignores the declaration.
            None => px,
        }
    }

    fn box_hidden(&self, font_px: Option<f32>, inline: bool, table_part: bool) -> bool {
        let horizontal = Basis {
            em: font_px,
            percent: Some(VIEWPORT_WIDTH),
        };
        let vertical = Basis {
            em: font_px,
            percent: Some(VIEWPORT_HEIGHT),
        };
        let length = |prop: Prop, basis: Basis| {
            self.value(prop)
                .and_then(|v| values::length_value(v, basis))
        };
        let far_left =
            |prop: Prop, basis: Basis| length(prop, basis).is_some_and(|px| px <= -OFFSCREEN_PX);
        if !inline && !table_part {
            let clipped = [Prop::OverflowX, Prop::OverflowY]
                .iter()
                .any(|prop| self.value(*prop).is_some_and(|v| keyword(v) != "visible"));
            let sizes = Basis {
                em: font_px,
                percent: None,
            };
            if clipped
                && [Prop::Height, Prop::MaxHeight, Prop::Width, Prop::MaxWidth]
                    .iter()
                    .any(|prop| length(*prop, sizes).is_some_and(|px| px <= MIN_BOX_PX))
            {
                return true;
            }
            if far_left(Prop::MarginTop, horizontal) || self.transform_hidden() {
                return true;
            }
        }
        if !table_part && far_left(Prop::MarginLeft, horizontal) {
            return true;
        }
        let position = self.value(Prop::Position).map(keyword);
        let position = position.as_deref();
        if matches!(position, Some("absolute" | "fixed" | "relative")) {
            let off = |near: Prop, far: Prop, basis: Basis| match length(near, basis) {
                Some(px) => px <= -OFFSCREEN_PX,
                None => length(far, basis).is_some_and(|px| px >= OFFSCREEN_PX),
            };
            if off(Prop::Left, Prop::Right, horizontal) || off(Prop::Top, Prop::Bottom, vertical) {
                return true;
            }
        }
        if matches!(position, Some("absolute" | "fixed"))
            && self
                .value(Prop::Clip)
                .is_some_and(|v| clip_rect_empty(v, font_px))
        {
            return true;
        }
        self.value(Prop::ClipPath)
            .is_some_and(|v| clip_path_empty(v, font_px))
    }

    fn transform_hidden(&self) -> bool {
        let basis = Basis {
            em: None,
            percent: None,
        };
        let far = |v: Option<f32>| v.is_some_and(|px| px <= -OFFSCREEN_PX);

        if let Some(value) = self.value(Prop::Translate) {
            let parts = components(value);
            if parts
                .iter()
                .take(2)
                .any(|p| far(values::length_value(p, basis)))
            {
                return true;
            }
        }
        if let Some(value) = self.value(Prop::Scale) {
            let parts = components(value);
            if parts
                .iter()
                .take(2)
                .any(|p| values::number_value(p) == Some(0.0))
            {
                return true;
            }
        }
        let Some(value) = self.value(Prop::Transform) else {
            return false;
        };
        let mut input = Parser::new(value);
        while let Ok(token) = input.next() {
            let Token::Function(name) = token else {
                continue;
            };
            let name = name.to_ascii_lowercase();
            let args = input
                .parse_nested_block(|inner| {
                    Ok::<_, cssparser::ParseError<()>>(arguments(inner, None))
                })
                .unwrap_or_default();
            let length = |i: usize| args.get(i).and_then(|a| a.px);
            let number = |i: usize| args.get(i).and_then(|a| a.number);
            let hidden = match name.as_str() {
                "scale" => number(0) == Some(0.0) || number(1) == Some(0.0),
                "scalex" | "scaley" => number(0) == Some(0.0),
                "scale3d" => number(0) == Some(0.0) || number(1) == Some(0.0),
                "translate" | "translate3d" => far(length(0)) || far(length(1)),
                "translatex" | "translatey" => far(length(0)),
                "matrix" => {
                    (number(0) == Some(0.0) && number(1) == Some(0.0))
                        || (number(2) == Some(0.0) && number(3) == Some(0.0))
                        || far(number(4))
                        || far(number(5))
                }
                _ => false,
            };
            if hidden {
                return true;
            }
        }
        false
    }

    fn color(&self, parent: &Frame, link_color: Rgba) -> Paint {
        let mut color = parent.color;
        let legacy = |name: &str| self.attribute(name).and_then(|v| values::legacy_color(&v));
        match self.name {
            "body" => {
                if let Some(text) = legacy("text") {
                    color = Paint::Rgba(text);
                }
            }
            "font" => {
                if let Some(c) = legacy("color") {
                    color = Paint::Rgba(c);
                }
            }
            "a" | "area" if dom::attribute(self.node, "href").is_some() => {
                color = Paint::Rgba(link_color);
            }
            "mark" => color = Paint::Rgba(Rgba::BLACK),
            _ => {}
        }
        let user_agent = color;
        let Some(declaration) = self.get(Prop::Color) else {
            return color;
        };
        match wide(&declaration.value) {
            Some(Wide::Inherit) => return parent.color,
            Some(Wide::Initial) => return Paint::Rgba(Rgba::BLACK),
            Some(Wide::Revert) => return user_agent,
            None => {}
        }
        match values::color_value(&declaration.value) {
            Some(Color::Rgba(c)) => Paint::Rgba(c),
            // In `color` itself, `currentcolor` is the inherited colour.
            Some(Color::Current) => parent.color,
            Some(Color::Unknown) => Paint::Unknown,
            Some(Color::Relative) => Paint::Relative,
            None => user_agent,
        }
    }

    fn fill(&self, parent: &Frame) -> Option<Paint> {
        let Some(declaration) = self.get(Prop::TextFill) else {
            return parent.fill;
        };
        match wide(&declaration.value) {
            Some(Wide::Inherit) => return parent.fill,
            Some(Wide::Initial | Wide::Revert) => return None,
            None => {}
        }
        match values::color_value(&declaration.value) {
            Some(Color::Rgba(c)) => Some(Paint::Rgba(c)),
            Some(Color::Current) => None,
            Some(Color::Unknown) => Some(Paint::Unknown),
            Some(Color::Relative) => Some(Paint::Relative),
            None => parent.fill,
        }
    }

    /// An inherited on/off property: `inherit` keeps the parent's, `initial` is off.
    fn flag(&self, prop: Prop, inherited: bool, on: impl Fn(&str) -> bool) -> bool {
        match (self.wide(prop), self.value(prop)) {
            (Some(Wide::Inherit), _) => inherited,
            (Some(_), _) => false,
            (None, Some(value)) => on(value),
            (None, None) => inherited,
        }
    }

    fn clips_text(&self) -> bool {
        self.value(Prop::BackgroundClip).is_some_and(|value| {
            components(value)
                .iter()
                .any(|c| c.eq_ignore_ascii_case("text"))
        })
    }

    fn visibility(&self, inherited: bool) -> bool {
        match self.wide(Prop::Visibility) {
            Some(Wide::Inherit) => return inherited,
            Some(_) => return false,
            None => {}
        }
        match self.value(Prop::Visibility).map(keyword).as_deref() {
            Some("hidden" | "collapse") => true,
            Some("visible") => false,
            _ => inherited,
        }
    }

    fn indent_hidden(&self, inherited: bool, font_px: Option<f32>) -> bool {
        match self.wide(Prop::TextIndent) {
            Some(Wide::Inherit) => return inherited,
            Some(_) => return false,
            None => {}
        }
        let Some(value) = self.value(Prop::TextIndent) else {
            return inherited;
        };
        let basis = Basis {
            em: font_px,
            percent: Some(VIEWPORT_WIDTH),
        };
        components(value)
            .iter()
            .find_map(|c| values::length_value(c, basis))
            .is_some_and(|px| px <= -OFFSCREEN_PX)
    }

    fn backdrop(&self, parent: Option<Rgba>, color: Paint) -> Option<Rgba> {
        if self.attribute("background").is_some() || self.background_image() {
            return None;
        }
        let background = self.background_color();
        let resolved = match background {
            None => return parent,
            Some(Color::Rgba(c)) => c,
            Some(Color::Current) => match color {
                Paint::Rgba(c) => c,
                _ => return None,
            },
            Some(Color::Unknown | Color::Relative) => return None,
        };
        if resolved.a <= 0.0 {
            parent
        } else if resolved.a >= 1.0 {
            Some(resolved)
        } else {
            parent.map(|below| resolved.over(below))
        }
    }

    fn background_image(&self) -> bool {
        let Some(declaration) = self.get(Prop::BackgroundImage) else {
            return false;
        };
        match wide(&declaration.value) {
            // The parent's image is not known here.
            Some(Wide::Inherit) => return true,
            Some(_) => return false,
            None => {}
        }
        let mut input = Parser::new(&declaration.value);
        while let Ok(token) = input.next() {
            match token {
                Token::UnquotedUrl(_) => return true,
                Token::Function(name) if is_image_function(name) => return true,
                _ => {}
            }
        }
        false
    }

    fn background_color(&self) -> Option<Color> {
        let user_agent = self
            .attribute("bgcolor")
            .and_then(|v| values::legacy_color(&v))
            .map(Color::Rgba)
            .or((self.name == "mark").then_some(Color::Rgba(Rgba::MARK)));
        let Some(declaration) = self.get(Prop::BackgroundColor) else {
            return user_agent;
        };
        match wide(&declaration.value) {
            Some(Wide::Inherit) => return Some(Color::Unknown),
            Some(Wide::Initial) => return None,
            Some(Wide::Revert) => return user_agent,
            None => {}
        }
        if declaration.name == "background" {
            // The colour is in the last layer; a shorthand without one sets transparent.
            return Some(
                components(last_layer(&declaration.value))
                    .into_iter()
                    .find_map(values::color_value)
                    .unwrap_or(Color::Rgba(Rgba {
                        a: 0.0,
                        ..Rgba::BLACK
                    })),
            );
        }
        values::color_value(&declaration.value).or(user_agent)
    }
}

/// One argument of a transform or shape function.
#[derive(Clone, Copy, Debug, Default)]
struct Argument {
    /// As a length in px.
    px: Option<f32>,
    /// As a plain number.
    number: Option<f32>,
    /// As a percentage (fraction).
    percent: Option<f32>,
}

/// The arguments of a function, comma- or space-separated, up to a keyword (`round`, `at`).
fn arguments(input: &mut Parser<'_>, font_px: Option<f32>) -> Vec<Argument> {
    let basis = Basis {
        em: font_px,
        percent: None,
    };
    let mut out = Vec::new();
    while let Ok(token) = input.next() {
        let argument = match *token {
            Token::Comma => continue,
            Token::Number { value, .. } => Argument {
                px: (value == 0.0).then_some(0.0),
                number: Some(value),
                percent: None,
            },
            Token::Dimension {
                value, ref unit, ..
            } => Argument {
                px: values::dimension_px(value, unit, basis),
                ..Argument::default()
            },
            Token::Percentage { unit_value, .. } => Argument {
                percent: Some(unit_value),
                ..Argument::default()
            },
            Token::Ident(ref word)
                if word.eq_ignore_ascii_case("round") || word.eq_ignore_ascii_case("at") =>
            {
                break;
            }
            _ => Argument::default(),
        };
        out.push(argument);
        if out.len() > 16 {
            break;
        }
    }
    while input.next().is_ok() {}
    out
}

/// Functions that make an image in a background.
fn is_image_function(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "url"
        || name.contains("gradient")
        || matches!(
            name.as_str(),
            "image"
                | "image-set"
                | "-webkit-image-set"
                | "cross-fade"
                | "-webkit-cross-fade"
                | "element"
                | "-moz-element"
                | "paint"
                | "-webkit-canvas"
        )
}

/// The last comma-separated layer of a `background` value.
fn last_layer(value: &str) -> &str {
    let mut input = Parser::new(value);
    let mut start = 0;
    while let Ok(token) = input.next() {
        if matches!(token, Token::Comma) {
            start = input.position().byte_index();
        }
    }
    &value[start..]
}

/// `<font size=N>` (1–7, or relative to 3 with a sign) in px.
fn legacy_font_size(value: &str) -> Option<f32> {
    let value = value.trim();
    let (relative, digits) = match value.as_bytes().first() {
        Some(b'+') => (Some(1), &value[1..]),
        Some(b'-') => (Some(-1), &value[1..]),
        _ => (None, value),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    let n: i32 = digits.parse().ok()?;
    let size = match relative {
        Some(sign) => 3 + sign * n,
        None => n,
    }
    .clamp(1, 7);
    Some(match size {
        1 => 10.0,
        2 => 13.0,
        3 => 16.0,
        4 => 18.0,
        5 => 24.0,
        6 => 32.0,
        _ => 48.0,
    })
}

/// `clip: rect(top, right, bottom, left)` with no width or height.
fn clip_rect_empty(value: &str, font_px: Option<f32>) -> bool {
    let mut input = Parser::new(value);
    let Ok(Token::Function(name)) = input.next() else {
        return false;
    };
    if !name.eq_ignore_ascii_case("rect") {
        return false;
    }
    let basis = Basis {
        em: font_px,
        percent: None,
    };
    let edges = input
        .parse_nested_block(|inner| {
            let mut edges = Vec::new();
            while !inner.is_exhausted() && edges.len() < 4 {
                let state = inner.state();
                let edge = values::length(inner, basis);
                if edge.is_none() {
                    inner.reset(&state);
                    let _ = inner.next();
                }
                edges.push(edge);
                let state = inner.state();
                if !matches!(inner.next(), Ok(Token::Comma)) {
                    inner.reset(&state);
                }
            }
            while inner.next().is_ok() {}
            Ok::<_, cssparser::ParseError<()>>(edges)
        })
        .unwrap_or_default();
    let [top, right, bottom, left] = edges.as_slice() else {
        return false;
    };
    let empty = |low: &Option<f32>, high: &Option<f32>| match (low, high) {
        (Some(low), Some(high)) => high - low <= MIN_BOX_PX,
        _ => false,
    };
    empty(left, right) || empty(top, bottom)
}

/// `clip-path` that leaves nothing: `inset()` of 50 % from both sides, `circle(0)`,
/// `ellipse(0 …)`.
fn clip_path_empty(value: &str, font_px: Option<f32>) -> bool {
    let mut input = Parser::new(value);
    while let Ok(token) = input.next() {
        let Token::Function(name) = token else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        let lengths = input
            .parse_nested_block(|inner| {
                Ok::<_, cssparser::ParseError<()>>(arguments(inner, font_px))
            })
            .unwrap_or_default();
        let zero = |i: usize| {
            lengths
                .get(i)
                .is_some_and(|a| a.px == Some(0.0) || a.percent == Some(0.0))
        };
        let percent = |i: usize| lengths.get(i).and_then(|a| a.percent);
        let hidden = match name.as_str() {
            "inset" => {
                let side = |indexes: &[usize]| indexes.iter().find_map(|i| percent(*i));
                let (top, right) = (side(&[0]), side(&[1, 0]));
                let (bottom, left) = (side(&[2, 0]), side(&[3, 1, 0]));
                let sum =
                    |a: Option<f32>, b: Option<f32>| a.zip(b).is_some_and(|(a, b)| a + b >= 1.0);
                sum(top, bottom) || sum(left, right)
            }
            "circle" => zero(0),
            "ellipse" => zero(0) || zero(1),
            _ => false,
        };
        if hidden {
            return true;
        }
    }
    false
}
