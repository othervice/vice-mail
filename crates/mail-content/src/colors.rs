//! A mail's colours on the reading surface (stage 21, decision 0066).
//!
//! **Where it runs.** After the sanitiser ([`crate::css`], the attribute filter of
//! [`crate::render_html`]): the pass reads the already cleaned CSS and attribute values again
//! with the same tokenizer and changes three things, nothing else.
//!
//! - Colour values (named colours, `#rgb[a]`, `#rrggbb[aa]`, `rgb()`/`rgba()`, `hsl()`/`hsla()`)
//!   of colour properties are replaced, when the mail is adapted, by a colour the pass writes
//!   itself: `#rrggbb`, or `rgba(r, g, b, a)` when the original had transparency. Every other
//!   byte is copied as it is (selectors, other values, `!important`, `url()`, comments).
//! - `color-scheme` and `supported-color-schemes` declarations are taken out: the core sets the
//!   frame's scheme itself, so a mail cannot make its text follow a scheme the canvas does not.
//! - `@media` queries on `prefers-color-scheme` are decided here for the scheme rendered (a
//!   query that matches loses that condition, one that cannot match is dropped): the frame's own
//!   preference would follow the web view's appearance, not the canvas.
//!
//! It adds no property, selector, at-rule or URL, so it adds nothing to what the sanitiser lets
//! through (`docs/ARCHITECTURE.md`, "Security boundaries").
//!
//! **The mapping.** Backgrounds (and lines and shadows, which sit on them) that are light become
//! dark; text that is dark becomes light; everything else stays. Lightness is turned around on
//! the scale `v = ln(Y + 0.05)` of the WCAG relative luminance `Y`, on which a contrast ratio is
//! a distance. Hue and chroma stay (OKLab; chroma is reduced only where the new lightness has no
//! such colour in sRGB); greys take the tint of the dark theme, so a white mail lands exactly on
//! the dark reading surface ([`DARK_CANVAS`]) and black text exactly on the app's text colour
//! ([`DARK_TEXT`]).
//!
//! - Backgrounds with `Y > 0.14` ([`BACKGROUND_THRESHOLD`]) are mirrored below that point and
//!   squeezed so white lands on the canvas.
//! - Text with `Y < 0.24` ([`TEXT_THRESHOLD`]) is mirrored above that point, at most up to the
//!   app's text colour.
//!
//! With these points, for every pair of a text colour and a background colour of a mail the
//! contrast afterwards is at least the smaller of the contrast before and 4.5:1 (WCAG AA): a
//! pair that was readable stays readable, and no pair gets worse below that. The test
//! `contrast_never_drops_below_aa_or_the_original` checks it on a grid of colours. The colours
//! written are found by bisection so that text never ends darker and a background never lighter
//! than the mapping says, which keeps the bound after rounding to 8 bits. Applying the pass to
//! its own output changes nothing (a mapped background is below, a mapped text above the point
//! where mapping starts).
//!
//! Limits, stated plainly: images (also `background-image: url()`) keep their colours, so dark
//! text drawn into an image or a transparent logo with dark letters stays as it is; text the
//! mail puts on an image as background becomes light like any other text; colours given as
//! `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`, `color()` or `color-mix()` stay; very dark
//! lines stay dark. "Show original colours" shows the mail as written.

use std::collections::HashMap;

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, SourcePosition, StyleSheetParser,
    Token,
};
use mail_model::{MailColors, MailScheme};

use crate::dom::{self, Descend, Visit};

/// The canvas of an adapted mail: the dark theme's reading surface (`--ui-surface` in
/// `app/src/styles/tokens.css`), so the mail and the pane around it are one surface. Stage 23,
/// step 3 (decision 0084): the reading pane has no card any more; before, the raised card.
pub const DARK_CANVAS: [u8; 3] = [0x14, 0x18, 0x1e];
/// Text of an adapted mail without a colour of its own, and the lightest text the pass writes
/// (`--ui-text-primary` of the dark theme).
pub const DARK_TEXT: [u8; 3] = [0xe8, 0xed, 0xf3];
/// Links without a colour of their own in an adapted mail (`--ui-accent` of the dark theme).
pub const DARK_LINK: [u8; 3] = [0x6a, 0xa8, 0xff];
/// Backgrounds with a higher relative luminance become dark.
pub const BACKGROUND_THRESHOLD: f64 = 0.14;
/// Text with a lower relative luminance becomes light.
pub const TEXT_THRESHOLD: f64 = 0.24;

/// Default style of a frame on the dark canvas, after [`crate::html::MAIL_DEFAULT_STYLE`]:
/// the scheme, the canvas, the text and link colours of [`DARK_CANVAS`], [`DARK_TEXT`] and
/// [`DARK_LINK`]. `:where()` gives it no specificity, so every colour of the mail wins.
pub const MAIL_DARK_STYLE: &str = ":where(html){color-scheme:dark;background-color:#14181e;color:#e8edf3}:where(a:link){color:#6aa8ff}";

/// Chroma (OKLab) below which a colour counts as grey and takes the theme's tint.
const NEUTRAL_CHROMA: f64 = 0.04;

/// What a colour is drawn as, which decides which way it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Role {
    /// Text and what is drawn with it (decorations, strokes).
    Text,
    /// Backgrounds, and borders, outlines and shadows (they sit on a background).
    Background,
}

/// What the pass knows about the colour schemes a mail declares (`<meta name="color-scheme">`,
/// `supported-color-schemes`, the `color-scheme` property).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SchemeFacts {
    /// Some declaration names `dark`: the mail brings dark colours of its own.
    pub(crate) dark: bool,
    /// Some declaration says `only light`: the mail asks not to be shown dark.
    pub(crate) only_light: bool,
}

impl SchemeFacts {
    /// One declaration's value (`light dark`, `only light`, …).
    fn add(&mut self, value: &str) {
        let mut dark = false;
        let mut light = false;
        let mut only = false;
        for word in value.split(|c: char| c.is_whitespace() || c == ',') {
            match word.to_ascii_lowercase().as_str() {
                "dark" => dark = true,
                "light" => light = true,
                "only" => only = true,
                _ => {}
            }
        }
        self.dark |= dark;
        self.only_light |= only && light && !dark;
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.dark |= other.dark;
        self.only_light |= other.only_light;
    }
}

/// How a mail is shown for `scheme`, given what it declares. A mail that asks for light only is
/// shown as written; one with dark colours of its own keeps them.
pub(crate) fn decide(scheme: MailScheme, facts: SchemeFacts) -> MailColors {
    match scheme {
        MailScheme::Light => MailColors::Original,
        MailScheme::Dark if facts.only_light => MailColors::Original,
        MailScheme::Dark if facts.dark => MailColors::OwnDark,
        MailScheme::Dark => MailColors::Adapted,
    }
}

/// The schemes the `<meta>` elements and the `style` attributes of `<html>` and `<body>` of a
/// parsed mail declare.
pub(crate) fn document_facts(document: &dom::Dom) -> SchemeFacts {
    let mut facts = SchemeFacts::default();
    dom::walk(&document.document, |visit| {
        let Visit::Enter(node) = visit else {
            return Descend::Yes;
        };
        match dom::html_name(node) {
            Some("meta") => {
                let name = dom::attribute(node, "name").unwrap_or_default();
                let name = name.trim();
                if name.eq_ignore_ascii_case("color-scheme")
                    || name.eq_ignore_ascii_case("supported-color-schemes")
                {
                    facts.add(&dom::attribute(node, "content").unwrap_or_default());
                }
            }
            Some("html" | "body") => {
                if let Some(style) = dom::attribute(node, "style") {
                    facts.merge(css_facts(&style));
                }
            }
            _ => {}
        }
        Descend::Yes
    });
    facts
}

/// The schemes `color-scheme` and `supported-color-schemes` declarations anywhere in `css` (a
/// stylesheet or a declaration list) name.
pub(crate) fn css_facts(css: &str) -> SchemeFacts {
    let mut facts = SchemeFacts::default();
    let mut parser = Parser::new(css);
    scan_facts(&mut parser, &mut facts, 0);
    facts
}

fn scan_facts(input: &mut Parser<'_>, facts: &mut SchemeFacts, depth: usize) {
    // `color-scheme` `:` value up to `;`, at any depth (rules, `@media` blocks, attributes).
    let mut property = false;
    let mut value: Option<String> = None;
    while let Ok(token) = input.next_including_whitespace() {
        match token.clone() {
            Token::WhiteSpace(_) => {}
            Token::Ident(name) if value.is_some() => {
                if let Some(v) = value.as_mut() {
                    v.push(' ');
                    v.push_str(&name);
                }
            }
            Token::Ident(name) => {
                property = name.eq_ignore_ascii_case("color-scheme")
                    || name.eq_ignore_ascii_case("supported-color-schemes");
            }
            Token::Colon if property => {
                property = false;
                value = Some(String::new());
            }
            Token::Semicolon => {
                if let Some(v) = value.take() {
                    facts.add(&v);
                }
                property = false;
            }
            Token::CurlyBracketBlock
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::Function(_) => {
                if let Some(v) = value.take() {
                    facts.add(&v);
                }
                property = false;
                if depth < 16 {
                    let _ = input.parse_nested_block(|inner| {
                        scan_facts(inner, facts, depth + 1);
                        Ok::<_, ParseError<()>>(())
                    });
                }
            }
            _ => property = false,
        }
    }
    if let Some(v) = value {
        facts.add(&v);
    }
}

/// The colour pass of one rendering. See the module documentation.
pub(crate) struct ColorPass {
    /// The scheme `prefers-color-scheme` queries are decided for.
    scheme: MailScheme,
    /// Colours are adapted ([`MailColors::Adapted`]).
    adapter: Option<Adapter>,
}

impl ColorPass {
    pub(crate) fn new(colors: MailColors) -> Self {
        let (scheme, adapter) = match colors {
            MailColors::Original => (MailScheme::Light, None),
            MailColors::OwnDark => (MailScheme::Dark, None),
            MailColors::Adapted => (MailScheme::Dark, Some(Adapter::new())),
        };
        Self { scheme, adapter }
    }

    /// A sanitised stylesheet (the output of `css::sanitize_stylesheet`).
    pub(crate) fn stylesheet(&mut self, css: &str) -> String {
        let mut parser = Parser::new(css);
        let mut out = String::new();
        rule_list(&mut parser, self, &mut out);
        out
    }

    /// A sanitised declaration list (a `style` attribute after `css::sanitize_declarations`).
    pub(crate) fn declarations(&mut self, css: &str) -> String {
        let mut parser = Parser::new(css);
        let mut out = String::new();
        declaration_list(&mut parser, self, &mut out);
        out
    }

    /// The value of a kept attribute of `element`: `style` and the colour attributes
    /// (`bgcolor`, `color` of `font` and `hr`, `text`/`link`/`vlink`/`alink` of `body`).
    pub(crate) fn attribute(&mut self, element: &str, attribute: &str, value: String) -> String {
        if attribute == "style" {
            return self.declarations(&value);
        }
        let role = match (element, attribute) {
            (_, "bgcolor") | ("hr", "color") => Role::Background,
            ("font", "color") | ("body", "text" | "link" | "vlink" | "alink") => Role::Text,
            _ => return value,
        };
        let Some(adapter) = self.adapter.as_mut() else {
            return value;
        };
        match legacy_color(&value) {
            Some(rgb) => {
                let adapted = adapter.adapt(rgb, role);
                if adapted == rgb { value } else { hex6(adapted) }
            }
            None => value,
        }
    }

    /// `value` of a declaration of `property` with its colours adapted; `None` if unchanged.
    fn value(&mut self, property: &str, value: &str) -> Option<String> {
        let adapter = self.adapter.as_mut()?;
        let role = role_of(property)?;
        rewrite_value(value, role, adapter)
    }
}

/// Which way the colours of `property` move; `None` for properties without colours to adapt.
fn role_of(property: &str) -> Option<Role> {
    let p = property.to_ascii_lowercase();
    let p = p.strip_prefix("-webkit-").unwrap_or(&p);
    match p {
        "color"
        | "text-fill-color"
        | "text-stroke"
        | "text-stroke-color"
        | "text-decoration"
        | "text-decoration-color"
        | "text-emphasis"
        | "text-emphasis-color"
        | "caret-color" => Some(Role::Text),
        "background" | "background-color" | "background-image" | "box-shadow" | "text-shadow"
        | "outline" | "outline-color" | "column-rule" | "column-rule-color" => {
            Some(Role::Background)
        }
        border
            if border.starts_with("border")
                && !["radius", "width", "style", "image", "collapse", "spacing"]
                    .iter()
                    .any(|part| border.contains(part)) =>
        {
            Some(Role::Background)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Stylesheets and declaration lists
// ---------------------------------------------------------------------------------------------

fn declaration_list(input: &mut Parser<'_>, pass: &mut ColorPass, out: &mut String) {
    let mut parser = Declarations { pass };
    for declaration in RuleBodyParser::new(input, &mut parser).flatten() {
        if declaration.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&declaration);
    }
}

fn rule_list(input: &mut Parser<'_>, pass: &mut ColorPass, out: &mut String) {
    let mut parser = Rules { pass };
    for rule in StyleSheetParser::new(input, &mut parser).flatten() {
        out.push_str(&rule);
    }
}

/// Everything up to the end of `input`, as written.
fn rest<'i>(input: &mut Parser<'i>) -> &'i str {
    let start = input.position();
    while input.next_including_whitespace_and_comments().is_ok() {}
    input.slice_from(start)
}

struct Declarations<'p> {
    pass: &'p mut ColorPass,
}

impl<'i> DeclarationParser<'i> for Declarations<'_> {
    type Declaration = String;
    type Error = ();

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        _start: &ParserState,
    ) -> Result<String, ParseError<()>> {
        let value = rest(input).trim();
        if name.eq_ignore_ascii_case("color-scheme")
            || name.eq_ignore_ascii_case("supported-color-schemes")
        {
            // Out: the core sets the frame's scheme (module documentation).
            return Ok(String::new());
        }
        let mut out = String::new();
        let _ = cssparser::serialize_identifier(&name, &mut out);
        out.push(':');
        match self.pass.value(&name, value) {
            Some(adapted) => out.push_str(&adapted),
            None => out.push_str(value),
        }
        out.push(';');
        Ok(out)
    }
}

impl<'i> AtRuleParser<'i> for Declarations<'_> {
    type Prelude = ();
    type AtRule = String;
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for Declarations<'_> {
    type Prelude = ();
    type QualifiedRule = String;
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, String, ()> for Declarations<'_> {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

struct Rules<'p> {
    pass: &'p mut ColorPass,
}

impl<'i> QualifiedRuleParser<'i> for Rules<'_> {
    type Prelude = String;
    type QualifiedRule = String;
    type Error = ();

    fn parse_prelude(&mut self, input: &mut Parser<'i>) -> Result<String, ParseError<()>> {
        Ok(rest(input).trim().to_owned())
    }

    fn parse_block(
        &mut self,
        prelude: String,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<String, ParseError<()>> {
        let mut body = String::new();
        declaration_list(input, self.pass, &mut body);
        if body.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("{prelude}{{{body}}}\n"))
    }
}

impl<'i> AtRuleParser<'i> for Rules<'_> {
    type Prelude = String;
    type AtRule = String;
    type Error = ();

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<String, ParseError<()>> {
        // The sanitiser keeps no other at-rule; anything else is dropped here too.
        if !name.eq_ignore_ascii_case("media") {
            return Err(ParseError::custom(()));
        }
        let prelude = rest(input).trim().to_owned();
        match resolve_media(&prelude, self.pass.scheme) {
            Some(prelude) => Ok(prelude),
            // Cannot match on this canvas: the whole block goes.
            None => Err(ParseError::custom(())),
        }
    }

    fn parse_block(
        &mut self,
        prelude: String,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<String, ParseError<()>> {
        let mut inner = String::new();
        rule_list(input, self.pass, &mut inner);
        if inner.is_empty() {
            return Ok(String::new());
        }
        let space = if prelude.is_empty() { "" } else { " " };
        Ok(format!("@media{space}{prelude}{{\n{inner}}}\n"))
    }
}

/// A sanitised `@media` prelude with its `prefers-color-scheme` conditions decided for
/// `scheme`: `None` when no query of the list can match, else the queries that can, without
/// those conditions (`all` for one that had nothing else). Queries with `not` or `or` around such
/// a condition are not evaluated and count as not matching; queries without one stay as they
/// are.
fn resolve_media(prelude: &str, scheme: MailScheme) -> Option<String> {
    let mut parser = Parser::new(prelude);
    // Top-level tokens of each query: (raw text, kind).
    let mut queries: Vec<Vec<(String, Part)>> = vec![Vec::new()];
    loop {
        let start = parser.position();
        let token = match parser.next() {
            Ok(token) => token.clone(),
            Err(_) => break,
        };
        let part = match &token {
            Token::Comma => {
                queries.push(Vec::new());
                continue;
            }
            Token::Ident(word) if word.eq_ignore_ascii_case("and") => Part::And,
            Token::Ident(word)
                if word.eq_ignore_ascii_case("not") || word.eq_ignore_ascii_case("or") =>
            {
                Part::Other(true)
            }
            Token::ParenthesisBlock => parser
                .parse_nested_block(|inner| Ok::<_, ParseError<()>>(scheme_condition(inner)))
                .ok()
                .flatten()
                .map_or(Part::Other(false), Part::Scheme),
            Token::Function(_) | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                // Consumed, so the raw text below holds the whole block.
                let _ = parser.parse_nested_block(|inner| {
                    while inner.next().is_ok() {}
                    Ok::<_, ParseError<()>>(())
                });
                Part::Other(false)
            }
            _ => Part::Other(false),
        };
        let raw = parser.slice_from(start).trim().to_owned();
        if let Some(query) = queries.last_mut() {
            query.push((raw, part));
        }
    }
    let mut kept = Vec::new();
    let mut changed = false;
    for query in queries {
        let conditions: Vec<&Option<MailScheme>> = query
            .iter()
            .filter_map(|(_, part)| match part {
                Part::Scheme(s) => Some(s),
                _ => None,
            })
            .collect();
        if conditions.is_empty() {
            if !query.is_empty() {
                kept.push(join(query.iter().map(|(raw, _)| raw.as_str())));
            }
            continue;
        }
        changed = true;
        let negated = query
            .iter()
            .any(|(_, part)| matches!(part, Part::Other(true)));
        if negated || conditions.iter().any(|s| **s != Some(scheme)) {
            continue;
        }
        // Drop the conditions and the `and` that joined each to the rest.
        let mut parts: Vec<&(String, Part)> = Vec::new();
        let mut skip_and = false;
        for item in &query {
            match item.1 {
                Part::Scheme(_) => {
                    if matches!(parts.last(), Some((_, Part::And))) {
                        parts.pop();
                    } else {
                        skip_and = true;
                    }
                }
                Part::And if skip_and => skip_and = false,
                _ => {
                    skip_and = false;
                    parts.push(item);
                }
            }
        }
        let rest: Vec<&str> = parts
            .iter()
            .map(|(raw, _)| raw.as_str())
            .filter(|raw| !raw.eq_ignore_ascii_case("only"))
            .collect();
        kept.push(if rest.is_empty() {
            "all".to_owned()
        } else {
            join(rest.into_iter())
        });
    }
    if !changed {
        return Some(prelude.to_owned());
    }
    (!kept.is_empty()).then(|| kept.join(", "))
}

fn join<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts.collect::<Vec<_>>().join(" ")
}

/// A top-level part of a media query.
enum Part {
    /// `and`.
    And,
    /// `(prefers-color-scheme: …)`: `light`, `dark`, or `None` for any other value (never
    /// matches).
    Scheme(Option<MailScheme>),
    /// Anything else; `true` for `not` and `or`.
    Other(bool),
}

/// The inside of a parenthesis of a media query: `Some(scheme)` for `prefers-color-scheme:
/// light|dark`, `Some(None)` for that feature with any other value (`no-preference`, which
/// neither canvas matches), `None` for any other condition.
fn scheme_condition(input: &mut Parser<'_>) -> Option<Option<MailScheme>> {
    let feature = input.expect_ident_cloned().ok()?;
    if !feature.eq_ignore_ascii_case("prefers-color-scheme") {
        return None;
    }
    if input.expect_colon().is_err() {
        return Some(None);
    }
    let value = input.expect_ident_cloned().ok();
    if input.next().is_ok() {
        return Some(None);
    }
    Some(match value {
        Some(v) if v.eq_ignore_ascii_case("dark") => Some(MailScheme::Dark),
        Some(v) if v.eq_ignore_ascii_case("light") => Some(MailScheme::Light),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

/// `value` with every colour adapted for `role`; `None` when nothing changed. Colours inside
/// functions (gradients) are adapted too; `url()` is left alone.
fn rewrite_value(value: &str, role: Role, adapter: &mut Adapter) -> Option<String> {
    let mut parser = Parser::new(value);
    let mut edits = Vec::new();
    collect_colors(&mut parser, role, adapter, &mut edits, 0);
    if edits.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(value.len() + 16);
    let mut at = 0;
    for (start, end, text) in edits {
        let (Some(before), Some(after)) = (value.get(at..start), value.get(end..)) else {
            return None;
        };
        out.push_str(before);
        // Keep the tokens apart: `solid#fff8` must not become `solidrgba(…)`.
        if text.starts_with(|c: char| c.is_ascii_alphabetic())
            && out.chars().next_back().is_some_and(is_name_char)
        {
            out.push(' ');
        }
        out.push_str(&text);
        if after.starts_with(|c: char| is_name_char(c) || c == '(') {
            out.push(' ');
        }
        at = end;
    }
    out.push_str(value.get(at..)?);
    Some(out)
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

type Edits = Vec<(usize, usize, String)>;

fn collect_colors(
    input: &mut Parser<'_>,
    role: Role,
    adapter: &mut Adapter,
    edits: &mut Edits,
    depth: usize,
) {
    loop {
        let start = input.position();
        let token = match input.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return,
        };
        match token {
            Token::Ident(name) => {
                if let Some(rgb) = named_color(&name) {
                    edit(
                        start,
                        input.position(),
                        Color::opaque(rgb),
                        role,
                        adapter,
                        edits,
                    );
                }
            }
            Token::Hash(digits) | Token::IDHash(digits) => {
                if let Some(color) = hex_color(&digits) {
                    edit(start, input.position(), color, role, adapter, edits);
                }
            }
            Token::Function(name) => {
                let lower = name.to_ascii_lowercase();
                match lower.as_str() {
                    "rgb" | "rgba" | "hsl" | "hsla" => {
                        let color = input
                            .parse_nested_block(|inner| {
                                Ok::<_, ParseError<()>>(function_color(&lower, inner))
                            })
                            .ok()
                            .flatten();
                        if let Some(color) = color {
                            edit(start, input.position(), color, role, adapter, edits);
                        }
                    }
                    "url" => {}
                    _ if depth < 12 => {
                        let _ = input.parse_nested_block(|inner| {
                            collect_colors(inner, role, adapter, edits, depth + 1);
                            Ok::<_, ParseError<()>>(())
                        });
                    }
                    _ => {}
                }
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock
                if depth < 12 =>
            {
                let _ = input.parse_nested_block(|inner| {
                    collect_colors(inner, role, adapter, edits, depth + 1);
                    Ok::<_, ParseError<()>>(())
                });
            }
            _ => {}
        }
    }
}

fn edit(
    start: SourcePosition,
    end: SourcePosition,
    color: Color,
    role: Role,
    adapter: &mut Adapter,
    edits: &mut Edits,
) {
    if color.alpha <= 0.0 {
        return;
    }
    let adapted = adapter.adapt(color.rgb, role);
    if adapted == color.rgb {
        return;
    }
    edits.push((
        start.byte_index(),
        end.byte_index(),
        format_color(adapted, color.alpha),
    ));
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Color {
    rgb: [u8; 3],
    /// 0 to 1.
    alpha: f64,
}

impl Color {
    const fn opaque(rgb: [u8; 3]) -> Self {
        Self { rgb, alpha: 1.0 }
    }
}

fn hex6([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn format_color(rgb: [u8; 3], alpha: f64) -> String {
    if alpha >= 1.0 {
        return hex6(rgb);
    }
    let [r, g, b] = rgb;
    let mut a = format!("{:.3}", alpha.clamp(0.0, 1.0));
    while a.ends_with('0') {
        a.pop();
    }
    if a.ends_with('.') {
        a.pop();
    }
    format!("rgba({r}, {g}, {b}, {a})")
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` (without the `#`).
fn hex_color(digits: &str) -> Option<Color> {
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    let nibble = |i: usize| {
        u8::from_str_radix(digits.get(i..=i)?, 16)
            .ok()
            .map(|v| v * 17)
    };
    let (rgb, alpha) = match digits.len() {
        3 => ([nibble(0)?, nibble(1)?, nibble(2)?], 255),
        4 => ([nibble(0)?, nibble(1)?, nibble(2)?], nibble(3)?),
        6 => ([byte(0)?, byte(2)?, byte(4)?], 255),
        8 => ([byte(0)?, byte(2)?, byte(4)?], byte(6)?),
        _ => return None,
    };
    Some(Color {
        rgb,
        alpha: f64::from(alpha) / 255.0,
    })
}

/// A colour attribute (`bgcolor`, `<font color>`): a named colour, `#rgb`, `#rrggbb`, or six or
/// three hex digits without `#` (as HTML's legacy colour parsing reads them). Anything else is
/// left to the web view.
fn legacy_color(value: &str) -> Option<[u8; 3]> {
    let value = value.trim();
    if let Some(rgb) = named_color(value) {
        return Some(rgb);
    }
    let digits = value.strip_prefix('#').unwrap_or(value);
    match digits.len() {
        3 | 6 => hex_color(digits).map(|c| c.rgb),
        _ => None,
    }
}

/// One argument of `rgb()` or `hsl()`.
#[derive(Debug, Clone, Copy)]
enum Arg {
    Number(f64),
    /// As a fraction (`50%` is 0.5).
    Percent(f64),
    /// In degrees.
    Angle(f64),
}

/// `rgb()`, `rgba()`, `hsl()`, `hsla()` in legacy (commas) or modern (spaces, `/ alpha`)
/// syntax; `None` for anything this pass does not read (it then stays as it is).
fn function_color(name: &str, input: &mut Parser<'_>) -> Option<Color> {
    let mut args = Vec::new();
    let mut alpha_at = None;
    while let Ok(token) = input.next() {
        let arg = match token {
            Token::Comma => continue,
            Token::Delim('/') => {
                alpha_at = Some(args.len());
                continue;
            }
            Token::Number { value, .. } => Arg::Number(exact(*value)),
            Token::Percentage { unit_value, .. } => Arg::Percent(exact(*unit_value)),
            Token::Dimension { value, unit, .. } => {
                let value = f64::from(*value);
                let degrees = match unit.to_ascii_lowercase().as_str() {
                    "deg" => value,
                    "grad" => value * 0.9,
                    "rad" => value.to_degrees(),
                    "turn" => value * 360.0,
                    _ => return None,
                };
                Arg::Angle(degrees)
            }
            Token::Ident(word) if word.eq_ignore_ascii_case("none") => Arg::Number(0.0),
            _ => return None,
        };
        args.push(arg);
    }
    if alpha_at.is_some_and(|at| at != 3) || !(3..=4).contains(&args.len()) {
        return None;
    }
    let alpha = match args.get(3) {
        None => 1.0,
        Some(Arg::Number(a)) => a.clamp(0.0, 1.0),
        Some(Arg::Percent(a)) => a.clamp(0.0, 1.0),
        Some(Arg::Angle(_)) => return None,
    };
    let rgb = if name.starts_with("rgb") {
        let channel = |arg: Arg| match arg {
            Arg::Number(v) => Some(v.clamp(0.0, 255.0)),
            Arg::Percent(v) => Some((v * 255.0).clamp(0.0, 255.0)),
            Arg::Angle(_) => None,
        };
        let rgb = [channel(args[0])?, channel(args[1])?, channel(args[2])?];
        rgb.map(|v| v.round() as u8)
    } else {
        let hue = match args[0] {
            Arg::Number(h) | Arg::Angle(h) => h,
            Arg::Percent(_) => return None,
        };
        // Percentages, or plain numbers (modern syntax) meaning percent.
        let fraction = |arg: Arg| match arg {
            Arg::Percent(v) => Some(v.clamp(0.0, 1.0)),
            Arg::Number(v) => Some((v / 100.0).clamp(0.0, 1.0)),
            Arg::Angle(_) => None,
        };
        hsl_to_rgb(hue, fraction(args[1])?, fraction(args[2])?)
    };
    Some(Color { rgb, alpha })
}

/// A number of the tokenizer (`f32`) as the decimal it was written as, to six places (`0.2`
/// stays `0.2`, not `0.200000003`).
fn exact(value: f32) -> f64 {
    (f64::from(value) * 1e6).round() / 1e6
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [u8; 3] {
    let h = hue.rem_euclid(360.0) / 30.0;
    let a = saturation * lightness.min(1.0 - lightness);
    let f = |n: f64| {
        let k = (n + h) % 12.0;
        let v = lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [f(0.0), f(8.0), f(4.0)]
}

/// The CSS named colours (CSS Color 4), sorted; `transparent` and `currentcolor` are no colour
/// to adapt.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

fn named_color(name: &str) -> Option<[u8; 3]> {
    if name.len() > 20 {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let i = NAMED_COLORS
        .binary_search_by(|(n, _)| (*n).cmp(lower.as_str()))
        .ok()?;
    let value = NAMED_COLORS.get(i)?.1;
    let [_, r, g, b] = value.to_be_bytes();
    Some([r, g, b])
}

// ---------------------------------------------------------------------------------------------
// The mapping
// ---------------------------------------------------------------------------------------------

/// The WCAG relative luminance of an sRGB colour.
pub(crate) fn luminance(rgb: [u8; 3]) -> f64 {
    let [r, g, b] = rgb.map(linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The WCAG contrast ratio of two colours (1 to 21).
#[cfg(test)]
pub(crate) fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (ya, yb) = (luminance(a), luminance(b));
    (ya.max(yb) + 0.05) / (ya.min(yb) + 0.05)
}

fn linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(linear: f64) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// OKLab of an sRGB colour (Björn Ottosson, 2020).
fn oklab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(linear);
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// Linear sRGB of an OKLab colour (may be outside 0 to 1).
fn oklab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s,
    ]
}

fn in_gamut(linear: [f64; 3]) -> bool {
    linear.iter().all(|c| (-1e-7..=1.0 + 1e-7).contains(c))
}

/// The sRGB colour of OKLab `(l, a, b)`; outside sRGB its chroma is reduced (hue and lightness
/// kept) until it fits.
fn to_srgb(l: f64, a: f64, b: f64) -> [u8; 3] {
    let mut lin = oklab_to_linear(l, a, b);
    if !in_gamut(lin) {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..24 {
            let k = f64::midpoint(lo, hi);
            if in_gamut(oklab_to_linear(l, a * k, b * k)) {
                lo = k;
            } else {
                hi = k;
            }
        }
        lin = oklab_to_linear(l, a * lo, b * lo);
    }
    lin.map(encode)
}

/// The mapping of one rendering, with the colours it already computed.
pub(crate) struct Adapter {
    v_text: f64,
    v_max: f64,
    theta_background: f64,
    theta_text: f64,
    /// How much the light backgrounds are squeezed so white lands on the canvas (< 1).
    squeeze: f64,
    canvas_tint: [f64; 2],
    text_tint: [f64; 2],
    cache: HashMap<([u8; 3], Role), [u8; 3]>,
}

/// The scale contrast is a distance on.
fn scale(y: f64) -> f64 {
    (y + 0.05).ln()
}

impl Adapter {
    pub(crate) fn new() -> Self {
        let v_canvas = scale(luminance(DARK_CANVAS));
        let v_max = scale(1.0);
        let theta_background = scale(BACKGROUND_THRESHOLD);
        let canvas = oklab(DARK_CANVAS);
        let text = oklab(DARK_TEXT);
        Self {
            v_text: scale(luminance(DARK_TEXT)),
            v_max,
            theta_background,
            theta_text: scale(TEXT_THRESHOLD),
            squeeze: (theta_background - v_canvas) / (v_max - theta_background),
            canvas_tint: [canvas[1], canvas[2]],
            text_tint: [text[1], text[2]],
            cache: HashMap::new(),
        }
    }

    /// The colour `rgb` becomes as `role`; itself if it stays.
    pub(crate) fn adapt(&mut self, rgb: [u8; 3], role: Role) -> [u8; 3] {
        if let Some(done) = self.cache.get(&(rgb, role)) {
            return *done;
        }
        let adapted = match self.target(luminance(rgb), role) {
            Some(target) => self.with_luminance(rgb, target, role),
            None => rgb,
        };
        self.cache.insert((rgb, role), adapted);
        adapted
    }

    /// The luminance `y` maps to, `None` if it stays.
    fn target(&self, y: f64, role: Role) -> Option<f64> {
        let v = scale(y);
        let mapped = match role {
            Role::Background if v > self.theta_background => {
                self.theta_background - self.squeeze * (v - self.theta_background)
            }
            Role::Text if v < self.theta_text => (2.0 * self.theta_text - v).min(self.v_text),
            _ => return None,
        };
        debug_assert!(mapped <= self.v_max + 1e-9);
        Some((mapped.exp() - 0.05).clamp(0.0, 1.0))
    }

    /// `rgb` with its hue and chroma (greys: the theme's tint) at the lightness whose
    /// luminance is `target`: for text the darkest colour at least that light, for a background
    /// the lightest at most that light.
    fn with_luminance(&self, rgb: [u8; 3], target: f64, role: Role) -> [u8; 3] {
        let [_, a, b] = oklab(rgb);
        let tint = match role {
            Role::Background => self.canvas_tint,
            Role::Text => self.text_tint,
        };
        let grey = (1.0 - a.hypot(b) / NEUTRAL_CHROMA).clamp(0.0, 1.0);
        let (a, b) = (a + grey * tint[0], b + grey * tint[1]);
        let at = |l: f64| to_srgb(l, a, b);
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        match role {
            Role::Text => {
                // Invariant: `hi` is light enough.
                if luminance(at(hi)) < target {
                    return [255, 255, 255];
                }
                for _ in 0..40 {
                    let mid = f64::midpoint(lo, hi);
                    if luminance(at(mid)) >= target {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                at(hi)
            }
            Role::Background => {
                // Invariant: `lo` is dark enough.
                if luminance(at(lo)) > target {
                    return [0, 0, 0];
                }
                for _ in 0..40 {
                    let mid = f64::midpoint(lo, hi);
                    if luminance(at(mid)) <= target {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                at(lo)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapted() -> ColorPass {
        ColorPass::new(MailColors::Adapted)
    }

    fn parse_hex(text: &str) -> [u8; 3] {
        legacy_color(text).expect("a colour")
    }

    #[test]
    fn the_constants_are_the_dark_theme_tokens() {
        // tokens.css: the dark card, its text and its accent. The mail and its card are one
        // surface, the default text and links are the app's.
        let tokens = include_str!("../../../app/src/styles/tokens.css");
        let dark = &tokens[tokens
            .find(":root[data-theme=\"dark\"] {")
            .expect("dark block")..];
        let token = |name: &str| {
            let at = dark.find(&format!("{name}:")).expect(name);
            let value = dark[at + name.len() + 1..]
                .split(';')
                .next()
                .unwrap()
                .trim();
            parse_hex(value)
        };
        assert_eq!(token("--ui-surface"), DARK_CANVAS);
        assert_eq!(token("--ui-text-primary"), DARK_TEXT);
        assert_eq!(token("--ui-accent"), DARK_LINK);
        assert_eq!(
            MAIL_DARK_STYLE,
            format!(
                ":where(html){{color-scheme:dark;background-color:{};color:{}}}:where(a:link){{color:{}}}",
                hex6(DARK_CANVAS),
                hex6(DARK_TEXT),
                hex6(DARK_LINK)
            )
        );
        // The default text and links are readable on the canvas.
        assert!(contrast(DARK_TEXT, DARK_CANVAS) >= 7.0);
        assert!(contrast(DARK_LINK, DARK_CANVAS) >= 4.5);
    }

    #[test]
    fn white_becomes_the_canvas_and_black_text_the_app_text() {
        let mut a = Adapter::new();
        assert_eq!(a.adapt([255, 255, 255], Role::Background), DARK_CANVAS);
        assert_eq!(a.adapt([0, 0, 0], Role::Text), DARK_TEXT);
        // Dark backgrounds and light text stay.
        assert_eq!(a.adapt([0, 0, 0], Role::Background), [0, 0, 0]);
        assert_eq!(
            a.adapt([0x20, 0x30, 0x60], Role::Background),
            [0x20, 0x30, 0x60]
        );
        assert_eq!(a.adapt([255, 255, 255], Role::Text), [255, 255, 255]);
        assert_eq!(a.adapt([0xaa, 0xbb, 0xcc], Role::Text), [0xaa, 0xbb, 0xcc]);
        // Light greys get darker than the canvas would be for a darker grey: order is kept.
        let f4 = a.adapt([0xf4, 0xf4, 0xf4], Role::Background);
        assert!(luminance(f4) > luminance(DARK_CANVAS), "{f4:?}");
        assert!(luminance(f4) < BACKGROUND_THRESHOLD);
        let grey33 = a.adapt([0x33, 0x33, 0x33], Role::Text);
        let grey77 = a.adapt([0x77, 0x77, 0x77], Role::Text);
        assert!(
            luminance(grey33) > luminance(grey77),
            "{grey33:?} {grey77:?}"
        );
    }

    #[test]
    fn hue_and_chroma_stay() {
        let mut a = Adapter::new();
        for (rgb, role) in [
            ([0xff, 0xeb, 0x3b], Role::Background),
            ([0xe8, 0xf0, 0xfe], Role::Background),
            ([0x1a, 0x73, 0xe8], Role::Background),
            ([0xcc, 0x00, 0x00], Role::Text),
            ([0x00, 0x66, 0x33], Role::Text),
            ([0x55, 0x1a, 0x8b], Role::Text),
        ] {
            let out = a.adapt(rgb, role);
            let [_, a0, b0] = oklab(rgb);
            let [_, a1, b1] = oklab(out);
            if a0.hypot(b0) < NEUTRAL_CHROMA || a1.hypot(b1) < 0.02 {
                continue;
            }
            let hue = |a: f64, b: f64| b.atan2(a).to_degrees();
            let diff = (hue(a0, b0) - hue(a1, b1) + 540.0).rem_euclid(360.0) - 180.0;
            assert!(diff.abs() < 8.0, "{rgb:?} -> {out:?}: hue moved {diff}");
        }
    }

    /// Text colours and background colours on a grid of 8 steps per channel (512 each) plus
    /// the greys: for every pair the contrast after is at least the smaller of the contrast
    /// before and 4.5:1.
    #[test]
    fn contrast_never_drops_below_aa_or_the_original() {
        let mut a = Adapter::new();
        let steps = [0u8, 36, 73, 109, 146, 182, 219, 255];
        let mut colors = Vec::new();
        for r in steps {
            for g in steps {
                for b in steps {
                    colors.push([r, g, b]);
                }
            }
        }
        for v in (0..=255).step_by(5) {
            colors.push([v, v, v]);
        }
        let texts: Vec<_> = colors
            .iter()
            .map(|c| (*c, a.adapt(*c, Role::Text)))
            .collect();
        let backgrounds: Vec<_> = colors
            .iter()
            .map(|c| (*c, a.adapt(*c, Role::Background)))
            .collect();
        let mut worst = f64::INFINITY;
        for (t, t2) in &texts {
            for (b, b2) in &backgrounds {
                let before = contrast(*t, *b);
                let after = contrast(*t2, *b2);
                let floor = before.min(4.5);
                assert!(
                    after >= floor - 1e-9,
                    "text {t:?} on {b:?}: {before:.2} became {after:.2} ({t2:?} on {b2:?})"
                );
                worst = worst.min(after / floor);
            }
        }
        assert!(worst >= 1.0, "{worst}");
    }

    #[test]
    fn colours_in_declarations_are_adapted_and_the_rest_is_kept() {
        let mut pass = adapted();
        let out = pass.declarations(
            "color:#333; background:#fff none no-repeat; font-size:12px; border:1px solid #dddddd!important;",
        );
        assert!(out.starts_with("color:#"), "{out}");
        assert!(!out.contains("#333;") && !out.contains("#fff "), "{out}");
        assert!(
            out.contains(" none no-repeat; font-size:12px; border:1px solid #"),
            "{out}"
        );
        assert!(out.ends_with("!important;"), "{out}");
        assert!(
            out.contains(&format!("background:{} none", hex6(DARK_CANVAS))),
            "{out}"
        );
        // Named, rgb, rgba, hsl, hsla, 4- and 8-digit hex.
        let out = pass.declarations(
            "color:black; background-color:rgb(255, 255, 255); border-color:rgba(255,255,255,0.5); \
             outline-color:hsl(0 0% 100%); text-decoration-color:hsla(0, 0%, 0%, .25); \
             column-rule-color:#ffff; caret-color:#00000080;",
        );
        assert_eq!(
            out,
            format!(
                "color:{text}; background-color:{canvas}; border-color:rgba(20, 24, 30, 0.5); \
                 outline-color:{canvas}; text-decoration-color:rgba(232, 237, 243, 0.25); \
                 column-rule-color:{canvas}; caret-color:rgba(232, 237, 243, 0.502);",
                text = hex6(DARK_TEXT),
                canvas = hex6(DARK_CANVAS)
            )
        );
    }

    #[test]
    fn what_is_no_colour_to_adapt_stays() {
        let mut pass = adapted();
        for css in [
            "color:transparent;",
            "color:currentColor;",
            "color:inherit;",
            "background-color:canvas;",
            "color:oklch(0.2 0.1 200);",
            "background:none;",
            "background-image:url(\"data:image/png;base64,AAAA\");",
            "font-family:white;",
            "width:100px;",
            "color:rgba(0, 0, 0, 0);",
            "color:#gg0000;",
        ] {
            assert_eq!(pass.declarations(css), css, "{css}");
        }
        // Colours inside `url()` are no colours.
        let css = "background:url(\"data:image/png;base64,ff0000\") #ffffff;";
        let out = pass.declarations(css);
        assert!(
            out.starts_with("background:url(\"data:image/png;base64,ff0000\") #"),
            "{out}"
        );
    }

    #[test]
    fn gradients_are_adapted_like_backgrounds() {
        let mut pass = adapted();
        let out = pass.declarations(
            "background:linear-gradient(to right, #ffffff 0%, rgb(240,240,240) 100%);",
        );
        assert_eq!(
            out.matches("linear-gradient(to right, #").count(),
            1,
            "{out}"
        );
        assert!(
            !out.contains("#ffffff") && !out.contains("rgb(240"),
            "{out}"
        );
        // A dark gradient behind light text stays as it is.
        let dark = "background-image:linear-gradient(#000000, #102030);";
        assert_eq!(pass.declarations(dark), dark);
    }

    #[test]
    fn tokens_stay_apart_after_a_replacement() {
        let mut pass = adapted();
        // `solid#fff8` is two tokens; its replacement must not glue to `solid`.
        let out = pass.declarations("border:1px solid#ffffff80;");
        assert!(out.contains("solid rgba("), "{out}");
        let out = pass.declarations("border:rgb(255,255,255)solid 1px;");
        assert!(out.contains(" solid 1px"), "{out}");
    }

    #[test]
    fn colour_attributes_are_adapted() {
        let mut pass = adapted();
        assert_eq!(
            pass.attribute("td", "bgcolor", "#FFFFFF".into()),
            hex6(DARK_CANVAS)
        );
        assert_eq!(
            pass.attribute("table", "bgcolor", "white".into()),
            hex6(DARK_CANVAS)
        );
        assert_eq!(
            pass.attribute("td", "bgcolor", "ffffff".into()),
            hex6(DARK_CANVAS)
        );
        assert_eq!(
            pass.attribute("font", "color", "#000".into()),
            hex6(DARK_TEXT)
        );
        assert_eq!(
            pass.attribute("body", "text", "black".into()),
            hex6(DARK_TEXT)
        );
        assert_ne!(pass.attribute("body", "link", "#0000ee".into()), "#0000ee");
        // Not colours, or not this pass's.
        assert_eq!(
            pass.attribute("td", "bgcolor", "chucknorris".into()),
            "chucknorris"
        );
        assert_eq!(pass.attribute("td", "width", "#fff".into()), "#fff");
        assert_eq!(pass.attribute("div", "color", "#000".into()), "#000");
        // Light text and dark backgrounds stay.
        assert_eq!(pass.attribute("font", "color", "#ffffff".into()), "#ffffff");
        assert_eq!(pass.attribute("td", "bgcolor", "#000000".into()), "#000000");
        // Original and own dark colours: nothing is adapted.
        let mut original = ColorPass::new(MailColors::Original);
        assert_eq!(original.attribute("td", "bgcolor", "#fff".into()), "#fff");
        let mut own = ColorPass::new(MailColors::OwnDark);
        assert_eq!(own.attribute("font", "color", "#000".into()), "#000");
        assert_eq!(own.declarations("color:#000;"), "color:#000;");
    }

    #[test]
    fn stylesheets_keep_their_rules_and_adapt_their_colours() {
        let mut pass = adapted();
        let out = pass.stylesheet(
            ".a, #b > p:not(.c){color:#222222; background:#ffffff;}\n\
             @media (max-width: 600px){\n.a{width:100%; color:red !important;}\n}\n",
        );
        assert!(out.starts_with(".a, #b > p:not(.c){color:#"), "{out}");
        assert!(
            out.contains("@media (max-width: 600px){\n.a{width:100%; color:#"),
            "{out}"
        );
        assert!(out.contains(" !important;}\n}\n"), "{out}");
        assert!(
            !out.contains("#222222") && !out.contains("#ffffff"),
            "{out}"
        );
    }

    #[test]
    fn the_mails_colour_scheme_is_taken_out() {
        for colors in [
            MailColors::Original,
            MailColors::Adapted,
            MailColors::OwnDark,
        ] {
            let mut pass = ColorPass::new(colors);
            assert_eq!(
                pass.declarations(
                    "color-scheme:light dark; supported-color-schemes:light dark; width:1px;"
                ),
                "width:1px;"
            );
            assert_eq!(
                pass.stylesheet(":root{color-scheme:light dark;}\np{margin:0;}\n"),
                "p{margin:0;}\n"
            );
        }
    }

    #[test]
    fn prefers_color_scheme_is_decided_for_the_canvas() {
        let css = "p{margin:0;}\n\
                   @media (prefers-color-scheme: dark){\n.d{display:block;}\n}\n\
                   @media (prefers-color-scheme: light){\n.l{display:block;}\n}\n\
                   @media screen and (prefers-color-scheme: dark) and (max-width: 600px){\n.m{display:block;}\n}\n\
                   @media print, (prefers-color-scheme: dark){\n.p{display:block;}\n}\n\
                   @media not all and (prefers-color-scheme: dark){\n.n{display:block;}\n}\n\
                   @media (prefers-color-scheme: no-preference){\n.x{display:block;}\n}\n";
        let mut light = ColorPass::new(MailColors::Original);
        assert_eq!(
            light.stylesheet(css),
            "p{margin:0;}\n\
             @media all{\n.l{display:block;}\n}\n\
             @media print{\n.p{display:block;}\n}\n"
        );
        let mut dark = ColorPass::new(MailColors::OwnDark);
        assert_eq!(
            dark.stylesheet(css),
            "p{margin:0;}\n\
             @media all{\n.d{display:block;}\n}\n\
             @media screen and (max-width: 600px){\n.m{display:block;}\n}\n\
             @media print, all{\n.p{display:block;}\n}\n"
        );
        // Media queries without a colour scheme stay as written.
        assert_eq!(
            dark.stylesheet("@media only screen and (max-width: 600px){\np{margin:0;}\n}\n"),
            "@media only screen and (max-width: 600px){\np{margin:0;}\n}\n"
        );
        assert_eq!(
            resolve_media(
                "only screen and (prefers-color-scheme: dark)",
                MailScheme::Dark
            )
            .as_deref(),
            Some("screen")
        );
        assert_eq!(
            resolve_media(
                "(prefers-color-scheme: dark) and (max-width: 1px)",
                MailScheme::Dark
            )
            .as_deref(),
            Some("(max-width: 1px)")
        );
        assert_eq!(
            resolve_media("(prefers-color-scheme: dark)", MailScheme::Light),
            None
        );
    }

    #[test]
    fn the_pass_is_idempotent() {
        let css = ".a{color:#333; background:linear-gradient(#fff, #eee); border:1px solid #ccc;}\n\
                   @media (prefers-color-scheme: dark){\n.b{color:white; background:rgba(255,255,255,.8);}\n}\n\
                   .c{color:hsl(210 50% 20%); background-color:#ffeb3b; box-shadow:0 1px 2px rgba(0,0,0,.2);}\n";
        for colors in [
            MailColors::Original,
            MailColors::Adapted,
            MailColors::OwnDark,
        ] {
            let mut pass = ColorPass::new(colors);
            let once = pass.stylesheet(css);
            assert_eq!(pass.stylesheet(&once), once, "{colors:?}");
            let decls = "color:#123456; background:#fafafa url(\"data:image/gif;base64,AA\"); color-scheme:dark;";
            let once = pass.declarations(decls);
            assert_eq!(pass.declarations(&once), once, "{colors:?}");
            let attr = pass.attribute("td", "bgcolor", "#f0f0f0".into());
            assert_eq!(pass.attribute("td", "bgcolor", attr.clone()), attr);
        }
        // On the grid: a mapped colour maps to itself.
        let mut a = Adapter::new();
        for v in 0..=255u8 {
            for role in [Role::Text, Role::Background] {
                for rgb in [[v, v, v], [v, 255 - v, v / 2], [255, v, 0]] {
                    let once = a.adapt(rgb, role);
                    assert_eq!(a.adapt(once, role), once, "{rgb:?} {role:?}");
                }
            }
        }
    }

    #[test]
    fn declared_schemes_are_found() {
        assert_eq!(
            css_facts(":root{color-scheme:light dark;}"),
            SchemeFacts {
                dark: true,
                only_light: false
            }
        );
        assert!(css_facts("@media all{\nbody{supported-color-schemes:light dark;}\n}\n").dark);
        assert_eq!(
            css_facts(":root{color-scheme:only light;}"),
            SchemeFacts {
                dark: false,
                only_light: true
            }
        );
        assert_eq!(css_facts("color-scheme:light;"), SchemeFacts::default());
        assert_eq!(css_facts("p{color:dark;}"), SchemeFacts::default());
        let doc = dom::parse_document(
            "<html><head><meta name=\"color-scheme\" content=\"light dark\"></head><body>x</body></html>",
        );
        assert!(document_facts(&doc).dark);
        let doc = dom::parse_document(
            "<html><head><meta name=\"supported-color-schemes\" content=\"light only\"></head><body>x</body></html>",
        );
        assert!(document_facts(&doc).only_light);
        let doc = dom::parse_document("<body style=\"color-scheme: dark light\">x</body>");
        assert!(document_facts(&doc).dark);
        assert_eq!(
            decide(
                MailScheme::Light,
                SchemeFacts {
                    dark: true,
                    only_light: false
                }
            ),
            MailColors::Original
        );
        assert_eq!(
            decide(MailScheme::Dark, SchemeFacts::default()),
            MailColors::Adapted
        );
        assert_eq!(
            decide(
                MailScheme::Dark,
                SchemeFacts {
                    dark: true,
                    only_light: false
                }
            ),
            MailColors::OwnDark
        );
        assert_eq!(
            decide(
                MailScheme::Dark,
                SchemeFacts {
                    dark: true,
                    only_light: true
                }
            ),
            MailColors::Original
        );
    }

    #[test]
    fn colour_syntax_is_read() {
        assert_eq!(hex_color("fff").map(|c| c.rgb), Some([255, 255, 255]));
        assert_eq!(hex_color("1a2b3c").map(|c| c.rgb), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(hex_color("1a2b3c80").map(|c| c.alpha), Some(128.0 / 255.0));
        assert_eq!(hex_color("12345"), None);
        assert_eq!(named_color("WhiteSmoke"), Some([0xf5, 0xf5, 0xf5]));
        assert_eq!(named_color("transparent"), None);
        let read = |css: &str| {
            let mut parser = Parser::new(css);
            let name = parser
                .expect_function()
                .map(|n| n.to_ascii_lowercase())
                .ok()?;
            parser
                .parse_nested_block(|inner| Ok::<_, ParseError<()>>(function_color(&name, inner)))
                .ok()
                .flatten()
        };
        assert_eq!(read("rgb(10, 20, 30)"), Some(Color::opaque([10, 20, 30])));
        assert_eq!(
            read("rgb(10 20 30 / 50%)"),
            Some(Color {
                rgb: [10, 20, 30],
                alpha: 0.5
            })
        );
        assert_eq!(
            read("rgba(100%, 0%, 0%, 0.2)"),
            Some(Color {
                rgb: [255, 0, 0],
                alpha: 0.2
            })
        );
        assert_eq!(
            read("hsl(120, 100%, 25%)"),
            Some(Color::opaque([0, 128, 0]))
        );
        assert_eq!(
            read("hsl(0.5turn 100% 50%)"),
            Some(Color::opaque([0, 255, 255]))
        );
        assert_eq!(
            read("hsla(240deg, 100%, 50%, .5)"),
            Some(Color {
                rgb: [0, 0, 255],
                alpha: 0.5
            })
        );
        assert_eq!(read("rgb(1, 2)"), None);
        assert_eq!(read("rgb(var(--x))"), None);
        assert_eq!(read("hsl(10px 1% 1%)"), None);
    }
}
