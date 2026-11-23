//! The part of the CSS cascade the visibility check needs: declarations of the properties that
//! can hide text, `<style>` rules with their selectors, specificity and `!important`.
//!
//! Input is always CSS the sanitiser (`crate::css`) already produced, so the check sees exactly
//! the rules and declarations the reading pane applies.
//!
//! Selectors: type, universal, class, ID and attribute selectors, all four combinators, the
//! structural pseudo-classes (`:first-child`, `:nth-child()`, …), `:not()`, `:is()`,
//! `:where()`, `:root`, `:link`. Matching follows the sanitised output the pane shows: elements
//! the sanitiser unwraps are not there (their children belong to the next kept ancestor), and
//! attributes it removes or rewrites (`href`, `src`, `style`) cannot be compared. Conservative
//! where the check cannot know: user-action states (`:hover`, `:focus`, `:visited`, …) never
//! match, as in a pane nobody touches; every other pseudo-class this module does not evaluate
//! (`:has()`, `:lang()`, unknown ones) counts as matching, so a hiding rule cannot escape the
//! check behind an unusual selector. Pseudo-elements (`::before`, `::first-line`) never match an
//! element.

use std::collections::HashMap;
use std::rc::Rc;

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};

use super::values::media_matches;
use crate::dom::{self, Handle, NodeData};
use crate::html::{ALLOWED_TAGS, BODY_ATTRIBUTES, is_allowed_attribute};

/// The longhand properties the check reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Prop {
    Display,
    Visibility,
    ContentVisibility,
    Opacity,
    Filter,
    FontSize,
    Color,
    TextFill,
    TextShadow,
    TextStroke,
    BackgroundColor,
    BackgroundImage,
    BackgroundClip,
    Height,
    MaxHeight,
    Width,
    MaxWidth,
    OverflowX,
    OverflowY,
    Position,
    Top,
    Right,
    Bottom,
    Left,
    MarginTop,
    MarginLeft,
    TextIndent,
    Clip,
    ClipPath,
    Transform,
    Translate,
    Scale,
}

pub(super) const PROP_COUNT: usize = Prop::Scale as usize + 1;

/// CSS properties (as written) and the longhands they set. A shorthand's value is kept whole;
/// the evaluator picks its part by the property name.
const PROPERTIES: &[(&str, &[Prop])] = &[
    ("display", &[Prop::Display]),
    ("visibility", &[Prop::Visibility]),
    ("content-visibility", &[Prop::ContentVisibility]),
    ("opacity", &[Prop::Opacity]),
    ("filter", &[Prop::Filter]),
    ("-webkit-filter", &[Prop::Filter]),
    ("font-size", &[Prop::FontSize]),
    ("font", &[Prop::FontSize]),
    ("color", &[Prop::Color]),
    ("-webkit-text-fill-color", &[Prop::TextFill]),
    ("text-shadow", &[Prop::TextShadow]),
    ("-webkit-text-stroke", &[Prop::TextStroke]),
    ("-webkit-text-stroke-width", &[Prop::TextStroke]),
    (
        "background",
        &[
            Prop::BackgroundColor,
            Prop::BackgroundImage,
            Prop::BackgroundClip,
        ],
    ),
    ("background-color", &[Prop::BackgroundColor]),
    ("background-image", &[Prop::BackgroundImage]),
    ("background-clip", &[Prop::BackgroundClip]),
    ("-webkit-background-clip", &[Prop::BackgroundClip]),
    ("height", &[Prop::Height]),
    ("block-size", &[Prop::Height]),
    ("max-height", &[Prop::MaxHeight]),
    ("max-block-size", &[Prop::MaxHeight]),
    ("width", &[Prop::Width]),
    ("inline-size", &[Prop::Width]),
    ("max-width", &[Prop::MaxWidth]),
    ("max-inline-size", &[Prop::MaxWidth]),
    ("overflow", &[Prop::OverflowX, Prop::OverflowY]),
    ("overflow-x", &[Prop::OverflowX]),
    ("overflow-y", &[Prop::OverflowY]),
    ("overflow-inline", &[Prop::OverflowX]),
    ("overflow-block", &[Prop::OverflowY]),
    ("position", &[Prop::Position]),
    ("top", &[Prop::Top]),
    ("right", &[Prop::Right]),
    ("bottom", &[Prop::Bottom]),
    ("left", &[Prop::Left]),
    ("inset", &[Prop::Top, Prop::Right, Prop::Bottom, Prop::Left]),
    ("inset-block", &[Prop::Top, Prop::Bottom]),
    ("inset-inline", &[Prop::Left, Prop::Right]),
    ("inset-block-start", &[Prop::Top]),
    ("inset-block-end", &[Prop::Bottom]),
    ("inset-inline-start", &[Prop::Left]),
    ("inset-inline-end", &[Prop::Right]),
    ("margin", &[Prop::MarginTop, Prop::MarginLeft]),
    ("margin-top", &[Prop::MarginTop]),
    ("margin-left", &[Prop::MarginLeft]),
    ("margin-block", &[Prop::MarginTop]),
    ("margin-block-start", &[Prop::MarginTop]),
    ("margin-inline", &[Prop::MarginLeft]),
    ("margin-inline-start", &[Prop::MarginLeft]),
    ("text-indent", &[Prop::TextIndent]),
    ("clip", &[Prop::Clip]),
    ("clip-path", &[Prop::ClipPath]),
    ("-webkit-clip-path", &[Prop::ClipPath]),
    ("transform", &[Prop::Transform]),
    ("-webkit-transform", &[Prop::Transform]),
    ("-ms-transform", &[Prop::Transform]),
    ("translate", &[Prop::Translate]),
    ("scale", &[Prop::Scale]),
];

/// One declaration of a property the check reads.
#[derive(Debug, Clone)]
pub(super) struct Declaration {
    pub prop: Prop,
    /// The property as written (canonical spelling from [`PROPERTIES`]), so a shorthand's
    /// value can be taken apart.
    pub name: &'static str,
    pub value: Rc<str>,
    pub important: bool,
}

/// The declarations of a sanitised declaration list (a `style` attribute or a rule body) that
/// the check reads, in order.
pub(super) fn parse_declarations(css: &str) -> Vec<Declaration> {
    let mut input = Parser::new(css);
    let mut parser = Declarations { out: Vec::new() };
    for result in RuleBodyParser::new(&mut input, &mut parser) {
        let _ = result;
    }
    parser.out
}

struct Declarations {
    out: Vec<Declaration>,
}

impl<'i> DeclarationParser<'i> for Declarations {
    type Declaration = ();
    type Error = ();

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        _start: &ParserState,
    ) -> Result<(), ParseError<()>> {
        let lower = name.to_ascii_lowercase();
        let known = PROPERTIES.iter().find(|(n, _)| *n == lower);
        let start = input.position();
        loop {
            let state = input.state();
            match input.next_including_whitespace() {
                Err(_) => break,
                Ok(Token::Delim('!')) => {
                    input.reset(&state);
                    break;
                }
                Ok(_) => {}
            }
        }
        let value = input.slice_from(start).trim();
        let important = input.try_parse(cssparser::parse_important).is_ok();
        while input.next().is_ok() {}
        if let Some((name, props)) = known
            && !value.is_empty()
        {
            let value: Rc<str> = Rc::from(value);
            for prop in *props {
                self.out.push(Declaration {
                    prop: *prop,
                    name,
                    value: Rc::clone(&value),
                    important,
                });
            }
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for Declarations {
    type Prelude = ();
    type AtRule = ();
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for Declarations {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, (), ()> for Declarations {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------------------------
// Selectors

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Equals,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Debug, Clone)]
enum Simple {
    Id(String),
    Class(String),
    Attribute {
        name: String,
        op: AttrOp,
        value: String,
        ignore_case: bool,
    },
    Pseudo(Pseudo),
}

#[derive(Debug, Clone)]
enum Pseudo {
    /// A state nobody has put the pane in (`:hover`, `:focus`, `:visited`, …).
    Never,
    /// Not evaluated: counts as matching.
    Maybe,
    Link,
    Root,
    Empty,
    /// `:nth-child(an+b)` and relatives; `:first-child` is `0n+1`.
    Nth {
        a: i32,
        b: i32,
        from_end: bool,
        of_type: bool,
    },
    Not(Vec<Selector>),
    Is(Vec<Selector>),
}

#[derive(Debug, Clone, Default)]
struct Compound {
    /// Lower-case element name; `None` for `*` or none.
    tag: Option<String>,
    simples: Vec<Simple>,
}

/// A complex selector, compounds from left to right.
#[derive(Debug, Clone)]
pub(super) struct Selector {
    compounds: Vec<Compound>,
    /// `combinators[i]` stands between `compounds[i]` and `compounds[i + 1]`.
    combinators: Vec<Combinator>,
    specificity: u32,
    /// Selects a pseudo-element, never an element.
    pseudo_element: bool,
    /// Keys (`#id`, `.class`, `tag`) some ancestor must have for the selector to match: a
    /// cheap test before the ancestors are searched, as browsers do with a Bloom filter.
    ancestor_keys: Vec<String>,
    /// Shortened to its rightmost compound ([`MAX_COMPOUNDS`]): a match only counts as
    /// possible.
    shortened: bool,
}

/// Longest chain of compounds matched exactly; matching recurses once per compound, so a
/// longer selector is reduced to its rightmost compound (and then over-matches, which hides
/// more rather than less).
const MAX_COMPOUNDS: usize = 32;

fn specificity(ids: u32, classes: u32, types: u32) -> u32 {
    (ids.min(1023) << 20) | (classes.min(1023) << 10) | types.min(1023)
}

/// The most specific selector of a list (for `:is()` and `:not()`).
fn max_specificity(list: &[Selector]) -> u32 {
    list.iter().map(|s| s.specificity).max().unwrap_or(0)
}

/// A selector list. `None`: invalid (the browser drops the rule too).
fn parse_selector_list(input: &mut Parser<'_>) -> Option<Vec<Selector>> {
    let mut list = Vec::new();
    loop {
        list.push(parse_complex(input)?);
        match input.next() {
            Err(_) => return Some(list),
            Ok(Token::Comma) => {}
            Ok(_) => return None,
        }
    }
}

fn parse_complex(input: &mut Parser<'_>) -> Option<Selector> {
    let mut selector = Selector {
        compounds: Vec::new(),
        combinators: Vec::new(),
        specificity: 0,
        pseudo_element: false,
        ancestor_keys: Vec::new(),
        shortened: false,
    };
    let (mut ids, mut classes, mut types) = (0u32, 0u32, 0u32);
    let mut current: Option<Compound> = None;
    let mut pending: Option<Combinator> = None;
    loop {
        let state = input.state();
        let token = match input.next_including_whitespace() {
            Ok(token) => token.clone(),
            Err(_) => break,
        };
        let combinator = match token {
            Token::Comma => {
                input.reset(&state);
                break;
            }
            Token::WhiteSpace(_) => Some(Combinator::Descendant),
            Token::Delim('>') => Some(Combinator::Child),
            Token::Delim('+') => Some(Combinator::Adjacent),
            Token::Delim('~') => Some(Combinator::Sibling),
            _ => None,
        };
        if let Some(combinator) = combinator {
            if let Some(compound) = current.take() {
                selector.compounds.push(compound);
            } else if selector.compounds.is_empty() {
                if combinator == Combinator::Descendant {
                    continue;
                }
                return None;
            }
            pending = match (pending, combinator) {
                (Some(Combinator::Descendant) | None, c) => Some(c),
                (Some(p), Combinator::Descendant) => Some(p),
                _ => return None,
            };
            continue;
        }
        if current.is_none() && !selector.compounds.is_empty() {
            selector.combinators.push(pending.take()?);
        }
        let compound = current.get_or_insert_with(Compound::default);
        let starts = compound.tag.is_none() && compound.simples.is_empty();
        match token {
            Token::Ident(ref name) if starts => {
                compound.tag = Some(name.to_ascii_lowercase());
                types += 1;
            }
            Token::Delim('*') if starts => {}
            Token::IDHash(ref id) => {
                compound.simples.push(Simple::Id(id.to_string()));
                ids += 1;
            }
            Token::Delim('.') => match input.next_including_whitespace() {
                Ok(Token::Ident(class)) => {
                    compound.simples.push(Simple::Class(class.to_string()));
                    classes += 1;
                }
                _ => return None,
            },
            Token::SquareBracketBlock => {
                let attribute = input.parse_nested_block(|inner| {
                    parse_attribute(inner).ok_or_else(|| ParseError::<()>::custom(()))
                });
                compound.simples.push(attribute.ok()?);
                classes += 1;
            }
            Token::Colon => {
                let (simple, weight) = parse_pseudo(input, &mut selector.pseudo_element)?;
                if let Some(simple) = simple {
                    compound.simples.push(simple);
                }
                let (i, c, t) = weight;
                ids += i;
                classes += c;
                types += t;
            }
            _ => return None,
        }
    }
    match current {
        Some(compound) => selector.compounds.push(compound),
        None if pending.is_some_and(|p| p != Combinator::Descendant) => return None,
        None => {
            // A trailing descendant "combinator" was only white space.
            if selector.combinators.len() >= selector.compounds.len() {
                selector.combinators.pop();
            }
        }
    }
    if selector.compounds.is_empty() || selector.combinators.len() + 1 != selector.compounds.len() {
        return None;
    }
    selector.specificity = specificity(ids, classes, types);
    if selector.compounds.len() > MAX_COMPOUNDS {
        selector.compounds.drain(..selector.compounds.len() - 1);
        selector.combinators.clear();
        selector.shortened = true;
    }
    // A compound left of a descendant or child combinator is an ancestor of the subject (one
    // left of only sibling combinators is not).
    let last = selector.compounds.len() - 1;
    for (index, compound) in selector.compounds[..last].iter().enumerate() {
        let ancestor = selector.combinators[index..]
            .iter()
            .any(|c| matches!(c, Combinator::Descendant | Combinator::Child));
        if ancestor {
            selector.ancestor_keys.extend(compound_keys(compound));
        }
    }
    Some(selector)
}

/// The keys an element must have to match `compound`.
fn compound_keys(compound: &Compound) -> Vec<String> {
    let mut keys: Vec<String> = compound.tag.iter().cloned().collect();
    for simple in &compound.simples {
        match simple {
            Simple::Id(id) => keys.push(format!("#{id}")),
            Simple::Class(class) => keys.push(format!(".{class}")),
            _ => {}
        }
    }
    keys
}

/// The keys of the kept ancestors of the current element, counted.
#[derive(Default)]
pub(super) struct AncestorKeys(HashMap<String, u32>);

impl AncestorKeys {
    /// Adds an element's keys (before its children are entered) and returns them for
    /// [`Self::remove`].
    pub(super) fn add(&mut self, node: &Handle) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(tag) = dom::html_name(node) {
            keys.push(tag.to_owned());
        }
        if let Some(id) = kept_attribute(node, "id") {
            keys.push(format!("#{id}"));
        }
        if let Some(classes) = kept_attribute(node, "class") {
            for class in classes.split_ascii_whitespace() {
                let key = format!(".{class}");
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        for key in &keys {
            *self.0.entry(key.clone()).or_default() += 1;
        }
        keys
    }

    pub(super) fn remove(&mut self, keys: &[String]) {
        for key in keys {
            if let Some(count) = self.0.get_mut(key) {
                *count -= 1;
                if *count == 0 {
                    self.0.remove(key);
                }
            }
        }
    }

    fn contains_all(&self, keys: &[String]) -> bool {
        keys.iter().all(|key| self.0.contains_key(key))
    }
}

fn parse_attribute(input: &mut Parser<'_>) -> Option<Simple> {
    let name = match input.next() {
        Ok(Token::Ident(name)) => name.to_ascii_lowercase(),
        _ => return None,
    };
    let op = match input.next() {
        Err(_) => {
            return Some(Simple::Attribute {
                name,
                op: AttrOp::Exists,
                value: String::new(),
                ignore_case: false,
            });
        }
        Ok(Token::Delim('=')) => AttrOp::Equals,
        Ok(Token::IncludeMatch) => AttrOp::Includes,
        Ok(Token::DashMatch) => AttrOp::DashMatch,
        Ok(Token::PrefixMatch) => AttrOp::Prefix,
        Ok(Token::SuffixMatch) => AttrOp::Suffix,
        Ok(Token::SubstringMatch) => AttrOp::Substring,
        Ok(_) => return None,
    };
    let value = match input.next() {
        Ok(Token::Ident(value) | Token::QuotedString(value)) => value.to_string(),
        _ => return None,
    };
    let ignore_case = match input.next() {
        Err(_) => false,
        Ok(Token::Ident(flag)) if flag.eq_ignore_ascii_case("i") => true,
        Ok(Token::Ident(flag)) if flag.eq_ignore_ascii_case("s") => false,
        Ok(_) => return None,
    };
    input.is_exhausted().then_some(Simple::Attribute {
        name,
        op,
        value,
        ignore_case,
    })
}

/// User-action and form states: never true in a reading pane nobody touches (the sanitiser
/// removes form fields).
const NEVER_PSEUDO: &[&str] = &[
    "hover",
    "active",
    "focus",
    "focus-within",
    "focus-visible",
    "visited",
    "target",
    "target-within",
    "checked",
    "indeterminate",
    "placeholder-shown",
    "autofill",
    "-webkit-autofill",
    "user-invalid",
    "user-valid",
    "fullscreen",
    "modal",
    "popover-open",
    "picture-in-picture",
    "playing",
    "paused",
    "seeking",
    "buffering",
    "stalled",
    "muted",
    "volume-locked",
    "drag-over",
];

/// Legacy pseudo-elements that may be written with one colon.
const LEGACY_PSEUDO_ELEMENTS: &[&str] = &["before", "after", "first-line", "first-letter"];

type Weight = (u32, u32, u32);

/// The pseudo-class or pseudo-element after a `:`. `Some((None, …))`: no condition to check.
fn parse_pseudo(
    input: &mut Parser<'_>,
    pseudo_element: &mut bool,
) -> Option<(Option<Simple>, Weight)> {
    let token = input.next_including_whitespace().ok()?.clone();
    let class = (0, 1, 0);
    match token {
        Token::Colon => {
            match input.next_including_whitespace() {
                Ok(Token::Ident(_) | Token::Function(_)) => {}
                _ => return None,
            }
            *pseudo_element = true;
            Some((None, (0, 0, 1)))
        }
        Token::Ident(ref name) => {
            let name = name.to_ascii_lowercase();
            if LEGACY_PSEUDO_ELEMENTS.contains(&name.as_str()) {
                *pseudo_element = true;
                return Some((None, (0, 0, 1)));
            }
            let nth = |a, b, from_end, of_type| Pseudo::Nth {
                a,
                b,
                from_end,
                of_type,
            };
            let pseudo = match name.as_str() {
                n if NEVER_PSEUDO.contains(&n) => Pseudo::Never,
                "link" | "any-link" | "-webkit-any-link" => Pseudo::Link,
                "root" | "scope" => Pseudo::Root,
                "empty" => Pseudo::Empty,
                "first-child" => nth(0, 1, false, false),
                "last-child" => nth(0, 1, true, false),
                "first-of-type" => nth(0, 1, false, true),
                "last-of-type" => nth(0, 1, true, true),
                "only-child" => Pseudo::Is(Vec::new()),
                "only-of-type" => Pseudo::Is(Vec::new()),
                _ => Pseudo::Maybe,
            };
            // `:only-child` is `:first-child:last-child`; written out below.
            let simple = match name.as_str() {
                "only-child" | "only-of-type" => {
                    let of_type = name == "only-of-type";
                    let both = Compound {
                        tag: None,
                        simples: vec![
                            Simple::Pseudo(nth(0, 1, false, of_type)),
                            Simple::Pseudo(nth(0, 1, true, of_type)),
                        ],
                    };
                    Simple::Pseudo(Pseudo::Is(vec![Selector {
                        compounds: vec![both],
                        combinators: Vec::new(),
                        specificity: 0,
                        pseudo_element: false,
                        ancestor_keys: Vec::new(),
                        shortened: false,
                    }]))
                }
                _ => Simple::Pseudo(pseudo),
            };
            Some((Some(simple), class))
        }
        Token::Function(ref name) => {
            let name = name.to_ascii_lowercase();
            let result = input.parse_nested_block(|inner| {
                Ok::<_, ParseError<()>>(parse_pseudo_function(&name, inner))
            });
            let (pseudo, weight) = result.unwrap_or((Pseudo::Maybe, class));
            Some((Some(Simple::Pseudo(pseudo)), weight))
        }
        _ => None,
    }
}

fn parse_pseudo_function(name: &str, input: &mut Parser<'_>) -> (Pseudo, Weight) {
    let class = (0, 1, 0);
    let weight_of = |list: &[Selector]| {
        let s = max_specificity(list);
        (s >> 20, (s >> 10) & 1023, s & 1023)
    };
    let result = match name {
        "not" | "is" | "matches" | "-webkit-any" | "-moz-any" | "where" => {
            match parse_selector_list(input) {
                Some(list) => {
                    let weight = if name == "where" {
                        (0, 0, 0)
                    } else {
                        weight_of(&list)
                    };
                    let pseudo = if name == "not" {
                        Pseudo::Not(list)
                    } else {
                        Pseudo::Is(list)
                    };
                    (pseudo, weight)
                }
                // `:is()` and `:where()` forgive invalid selectors; whatever it was, it is
                // not evaluated.
                None => (Pseudo::Maybe, class),
            }
        }
        "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
            match cssparser::parse_nth(input) {
                Ok((a, b)) if input.is_exhausted() => (
                    Pseudo::Nth {
                        a,
                        b,
                        from_end: name.contains("last"),
                        of_type: name.ends_with("of-type"),
                    },
                    class,
                ),
                // `of S` and anything else.
                _ => (Pseudo::Maybe, class),
            }
        }
        _ => (Pseudo::Maybe, class),
    };
    while input.next().is_ok() {}
    result
}

// ---------------------------------------------------------------------------------------------
// Matching

/// Result of matching: `Maybe` counts as a match (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tri {
    No,
    Maybe,
    Yes,
}

impl Tri {
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::No, _) | (_, Self::No) => Self::No,
            (Self::Yes, Self::Yes) => Self::Yes,
            _ => Self::Maybe,
        }
    }

    fn not(self) -> Self {
        match self {
            Self::No => Self::Yes,
            Self::Yes => Self::No,
            Self::Maybe => Self::Maybe,
        }
    }
}

/// Work left for selector matching in one document; `None` from a match means it ran out.
pub(super) struct Budget(pub u64);

impl Budget {
    fn spend(&mut self) -> Option<()> {
        self.0 = self.0.checked_sub(1)?;
        Some(())
    }
}

/// Whether the reading pane shows this element (the sanitiser keeps it) rather than only its
/// content.
pub(super) fn is_kept(node: &Handle) -> bool {
    dom::html_name(node)
        .is_some_and(|name| matches!(name, "html" | "body") || ALLOWED_TAGS.contains(&name))
}

/// An attribute as the reading pane has it: `None` if the sanitiser removes it.
pub(super) fn kept_attribute(node: &Handle, name: &str) -> Option<String> {
    let tag = dom::html_name(node)?;
    let kept = match tag {
        "html" => false,
        "body" => BODY_ATTRIBUTES.contains(&name),
        _ => is_allowed_attribute(tag, name),
    };
    if kept {
        dom::attribute(node, name)
    } else {
        None
    }
}

/// The nearest kept ancestor.
fn kept_parent(node: &Handle) -> Option<Handle> {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if !matches!(candidate.data, NodeData::Element { .. }) {
            return None;
        }
        if is_kept(&candidate) {
            return Some(candidate);
        }
        parent = candidate.parent();
    }
    None
}

/// Kept element siblings before and after `node` (among the children of its parent node).
fn siblings(node: &Handle) -> (Vec<Handle>, Vec<Handle>) {
    let Some(parent) = node.parent() else {
        return (Vec::new(), Vec::new());
    };
    let children = parent.children.borrow();
    let elements: Vec<&Handle> = children
        .iter()
        .filter(|c| matches!(c.data, NodeData::Element { .. }) && is_kept(c))
        .collect();
    let Some(position) = elements.iter().position(|c| Rc::ptr_eq(c, node)) else {
        return (Vec::new(), Vec::new());
    };
    (
        elements[..position].iter().map(|c| Rc::clone(c)).collect(),
        elements[position + 1..]
            .iter()
            .map(|c| Rc::clone(c))
            .collect(),
    )
}

fn matches_compound(compound: &Compound, node: &Handle, budget: &mut Budget) -> Option<Tri> {
    budget.spend()?;
    let tag = dom::html_name(node).unwrap_or("");
    if compound.tag.as_deref().is_some_and(|wanted| wanted != tag) {
        return Some(Tri::No);
    }
    let mut result = Tri::Yes;
    for simple in &compound.simples {
        let this = match simple {
            Simple::Id(id) => bool_tri(kept_attribute(node, "id").as_deref() == Some(id.as_str())),
            Simple::Class(class) => bool_tri(
                kept_attribute(node, "class")
                    .is_some_and(|value| value.split_ascii_whitespace().any(|c| c == class)),
            ),
            Simple::Attribute {
                name,
                op,
                value,
                ignore_case,
            } => attribute_matches(node, name, *op, value, *ignore_case),
            Simple::Pseudo(pseudo) => pseudo_matches(pseudo, node, tag, budget)?,
        };
        result = result.and(this);
        if result == Tri::No {
            break;
        }
    }
    Some(result)
}

fn bool_tri(value: bool) -> Tri {
    if value { Tri::Yes } else { Tri::No }
}

/// Attributes whose value the sanitiser rewrites: present or not, the value is unknown.
const REWRITTEN: &[&str] = &["href", "src", "srcset", "background", "style"];

fn attribute_matches(
    node: &Handle,
    name: &str,
    op: AttrOp,
    wanted: &str,
    ignore_case: bool,
) -> Tri {
    let Some(actual) = kept_attribute(node, name) else {
        return Tri::No;
    };
    if op == AttrOp::Exists {
        return Tri::Yes;
    }
    if REWRITTEN.contains(&name) {
        return Tri::Maybe;
    }
    let (actual, wanted) = if ignore_case {
        (actual.to_lowercase(), wanted.to_lowercase())
    } else {
        (actual, wanted.to_owned())
    };
    bool_tri(match op {
        AttrOp::Exists => true,
        AttrOp::Equals => actual == wanted,
        AttrOp::Includes => actual.split_ascii_whitespace().any(|w| w == wanted),
        AttrOp::DashMatch => actual == wanted || actual.starts_with(&format!("{wanted}-")),
        AttrOp::Prefix => !wanted.is_empty() && actual.starts_with(&wanted),
        AttrOp::Suffix => !wanted.is_empty() && actual.ends_with(&wanted),
        AttrOp::Substring => !wanted.is_empty() && actual.contains(&wanted),
    })
}

fn pseudo_matches(pseudo: &Pseudo, node: &Handle, tag: &str, budget: &mut Budget) -> Option<Tri> {
    Some(match pseudo {
        Pseudo::Never => Tri::No,
        Pseudo::Maybe => Tri::Maybe,
        Pseudo::Link => {
            bool_tri(matches!(tag, "a" | "area") && dom::attribute(node, "href").is_some())
        }
        Pseudo::Root => bool_tri(tag == "html"),
        Pseudo::Empty => bool_tri(
            node.children
                .borrow()
                .iter()
                .all(|child| match &child.data {
                    NodeData::Element { .. } => false,
                    NodeData::Text { contents } => contents.borrow().is_empty(),
                    _ => true,
                }),
        ),
        Pseudo::Nth {
            a,
            b,
            from_end,
            of_type,
        } => {
            let (before, after) = siblings(node);
            let others = if *from_end { after } else { before };
            let count = others
                .iter()
                .filter(|other| !*of_type || dom::html_name(other) == Some(tag))
                .count();
            budget.spend()?;
            let n = i64::try_from(count).unwrap_or(i64::MAX) + 1;
            let (a, b) = (i64::from(*a), i64::from(*b));
            bool_tri(if a == 0 {
                n == b
            } else {
                (n - b) % a == 0 && (n - b) / a >= 0
            })
        }
        Pseudo::Not(list) => any_matches(list, node, budget)?.not(),
        Pseudo::Is(list) => any_matches(list, node, budget)?,
    })
}

fn any_matches(list: &[Selector], node: &Handle, budget: &mut Budget) -> Option<Tri> {
    let mut best = Tri::No;
    for selector in list {
        match selector.matches(node, budget)? {
            Tri::Yes => return Some(Tri::Yes),
            Tri::Maybe => best = Tri::Maybe,
            Tri::No => {}
        }
    }
    Some(best)
}

/// Results of `Selector::matches_from` by (node, compound index), so descendant combinators
/// stay linear in the depth instead of exponential in the number of compounds.
type Memo = HashMap<(usize, usize), Tri>;

impl Selector {
    fn matches(&self, node: &Handle, budget: &mut Budget) -> Option<Tri> {
        if self.pseudo_element {
            return Some(Tri::No);
        }
        let mut memo = Memo::new();
        let result = self.matches_from(self.compounds.len() - 1, node, budget, &mut memo)?;
        Some(if self.shortened && result == Tri::Yes {
            Tri::Maybe
        } else {
            result
        })
    }

    /// Whether `compounds[..=index]` matches with `compounds[index]` on `node`.
    fn matches_from(
        &self,
        index: usize,
        node: &Handle,
        budget: &mut Budget,
        memo: &mut Memo,
    ) -> Option<Tri> {
        // Two compounds are matched in one pass; only longer chains revisit states.
        if self.compounds.len() <= 2 {
            return self.compute_from(index, node, budget, memo);
        }
        let key = (Rc::as_ptr(node).addr(), index);
        if let Some(known) = memo.get(&key) {
            return Some(*known);
        }
        let result = self.compute_from(index, node, budget, memo)?;
        memo.insert(key, result);
        Some(result)
    }

    fn compute_from(
        &self,
        index: usize,
        node: &Handle,
        budget: &mut Budget,
        memo: &mut Memo,
    ) -> Option<Tri> {
        let here = matches_compound(&self.compounds[index], node, budget)?;
        if here == Tri::No || index == 0 {
            return Some(here);
        }
        let candidates: Vec<Handle> = match self.combinators[index - 1] {
            Combinator::Child => kept_parent(node).into_iter().collect(),
            Combinator::Descendant => {
                let mut ancestors = Vec::new();
                let mut current = kept_parent(node);
                while let Some(ancestor) = current {
                    budget.spend()?;
                    current = kept_parent(&ancestor);
                    ancestors.push(ancestor);
                }
                ancestors
            }
            Combinator::Adjacent => siblings(node).0.pop().into_iter().collect(),
            Combinator::Sibling => {
                let mut before = siblings(node).0;
                before.reverse();
                before
            }
        };
        let mut best = Tri::No;
        for candidate in &candidates {
            match self.matches_from(index - 1, candidate, budget, memo)? {
                Tri::Yes => return Some(here.and(Tri::Yes)),
                Tri::Maybe => best = Tri::Maybe,
                Tri::No => {}
            }
        }
        Some(here.and(best))
    }

    /// The key the rule is filed under: the rightmost compound's ID, else a class, else its
    /// element name.
    fn key(&self) -> Key {
        let Some(compound) = self.compounds.last() else {
            return Key::Universal;
        };
        for simple in &compound.simples {
            if let Simple::Id(id) = simple {
                return Key::Id(id.clone());
            }
        }
        for simple in &compound.simples {
            if let Simple::Class(class) = simple {
                return Key::Class(class.clone());
            }
        }
        match &compound.tag {
            Some(tag) => Key::Tag(tag.clone()),
            None => Key::Universal,
        }
    }
}

enum Key {
    Id(String),
    Class(String),
    Tag(String),
    Universal,
}

// ---------------------------------------------------------------------------------------------
// Style sheets

struct Rule {
    selectors: Vec<Selector>,
    declarations: Vec<Declaration>,
}

/// The rules of a document's sanitised style sheet that set a property the check reads, filed
/// by the key of each selector.
#[derive(Default)]
pub(super) struct StyleSheet {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<(usize, usize)>>,
    by_class: HashMap<String, Vec<(usize, usize)>>,
    by_tag: HashMap<String, Vec<(usize, usize)>>,
    universal: Vec<(usize, usize)>,
}

/// A declaration that applies to an element, with its place in the cascade.
#[derive(Clone, Copy)]
pub(super) struct Applied<'a> {
    pub declaration: &'a Declaration,
    /// `(important, origin, specificity, rule, position)`; the largest wins. Origin 0:
    /// user-agent defaults and presentational attributes, 1: style sheet, 2: `style` attribute.
    pub rank: (bool, u8, u32, u32, u32),
}

impl StyleSheet {
    /// Parses a sanitised style sheet; `@media` blocks count when [`media_matches`].
    pub(super) fn parse(css: &str) -> Self {
        let mut sheet = Self::default();
        let mut input = Parser::new(css);
        let mut parser = Rules { sheet: &mut sheet };
        for result in StyleSheetParser::new(&mut input, &mut parser) {
            let _ = result;
        }
        sheet
    }

    fn add(&mut self, selectors: Vec<Selector>, declarations: Vec<Declaration>) {
        if declarations.is_empty() {
            return;
        }
        let rule = self.rules.len();
        for (index, selector) in selectors.iter().enumerate() {
            if selector.pseudo_element {
                continue;
            }
            let entry = (rule, index);
            match selector.key() {
                Key::Id(id) => self.by_id.entry(id).or_default().push(entry),
                Key::Class(class) => self.by_class.entry(class).or_default().push(entry),
                Key::Tag(tag) => self.by_tag.entry(tag).or_default().push(entry),
                Key::Universal => self.universal.push(entry),
            }
        }
        self.rules.push(Rule {
            selectors,
            declarations,
        });
    }

    /// Declarations of all rules that match `node`, appended to `out`. `None`: the budget ran
    /// out.
    pub(super) fn applied<'a>(
        &'a self,
        node: &Handle,
        ancestors: &AncestorKeys,
        budget: &mut Budget,
        out: &mut Vec<Applied<'a>>,
    ) -> Option<()> {
        if self.rules.is_empty() {
            return Some(());
        }
        let tag = dom::html_name(node).unwrap_or("");
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        if let Some(id) = kept_attribute(node, "id")
            && let Some(list) = self.by_id.get(&id)
        {
            candidates.extend(list);
        }
        if let Some(classes) = kept_attribute(node, "class") {
            let mut seen: Vec<&str> = Vec::new();
            for class in classes.split_ascii_whitespace() {
                if seen.contains(&class) {
                    continue;
                }
                seen.push(class);
                if let Some(list) = self.by_class.get(class) {
                    candidates.extend(list);
                }
            }
        }
        if let Some(list) = self.by_tag.get(tag) {
            candidates.extend(list);
        }
        candidates.extend(&self.universal);
        candidates.sort_unstable();
        // Per rule, the most specific matching selector counts.
        let mut best: Option<(usize, u32)> = None;
        let flush = |best: Option<(usize, u32)>, out: &mut Vec<Applied<'a>>| {
            if let Some((rule, specificity)) = best {
                let rule_index = u32::try_from(rule).unwrap_or(u32::MAX);
                for (position, declaration) in self.rules[rule].declarations.iter().enumerate() {
                    out.push(Applied {
                        declaration,
                        rank: (
                            declaration.important,
                            1,
                            specificity,
                            rule_index,
                            u32::try_from(position).unwrap_or(u32::MAX),
                        ),
                    });
                }
            }
        };
        for (rule, index) in candidates {
            let selector = &self.rules[rule].selectors[index];
            budget.spend()?;
            if !ancestors.contains_all(&selector.ancestor_keys)
                || selector.matches(node, budget)? == Tri::No
            {
                continue;
            }
            match best {
                Some((current, specificity)) if current == rule => {
                    best = Some((rule, specificity.max(selector.specificity)));
                }
                _ => {
                    flush(best, out);
                    best = Some((rule, selector.specificity));
                }
            }
        }
        flush(best, out);
        Some(())
    }
}

struct Rules<'s> {
    sheet: &'s mut StyleSheet,
}

impl<'i> QualifiedRuleParser<'i> for Rules<'_> {
    type Prelude = Option<Vec<Selector>>;
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude(
        &mut self,
        input: &mut Parser<'i>,
    ) -> Result<Option<Vec<Selector>>, ParseError<()>> {
        let selectors = parse_selector_list(input);
        while input.next().is_ok() {}
        Ok(selectors)
    }

    fn parse_block(
        &mut self,
        prelude: Option<Vec<Selector>>,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<(), ParseError<()>> {
        let start = input.position();
        while input.next().is_ok() {}
        if let Some(selectors) = prelude {
            let declarations = parse_declarations(input.slice_from(start));
            self.sheet.add(selectors, declarations);
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for Rules<'_> {
    type Prelude = bool;
    type AtRule = ();
    type Error = ();

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<bool, ParseError<()>> {
        let start = input.position();
        while input.next().is_ok() {}
        Ok(name.eq_ignore_ascii_case("media") && media_matches(input.slice_from(start)))
    }

    fn parse_block(
        &mut self,
        applies: bool,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<(), ParseError<()>> {
        if applies {
            let mut inner = Rules {
                sheet: &mut *self.sheet,
            };
            for result in StyleSheetParser::new(input, &mut inner) {
                let _ = result;
            }
        }
        while input.next().is_ok() {}
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(dom: &dom::Dom, id: &str) -> Handle {
        let mut found = None;
        dom::walk(&dom.document, |visit| {
            if let dom::Visit::Enter(node) = visit
                && dom::attribute(node, "id").as_deref() == Some(id)
            {
                found = Some(node.clone());
            }
            dom::Descend::Yes
        });
        found.expect("element")
    }

    /// The ancestor keys of `node`, as the walk would have collected them.
    fn ancestors_of(node: &Handle) -> AncestorKeys {
        let mut keys = AncestorKeys::default();
        let mut current = kept_parent(node);
        while let Some(ancestor) = current {
            keys.add(&ancestor);
            current = kept_parent(&ancestor);
        }
        keys
    }

    fn matched(css: &str, html: &str, id: &str) -> Vec<String> {
        let sheet = StyleSheet::parse(css);
        let dom = dom::parse_document(html);
        let node = element(&dom, id);
        let mut out = Vec::new();
        sheet
            .applied(
                &node,
                &ancestors_of(&node),
                &mut Budget(1_000_000),
                &mut out,
            )
            .expect("budget");
        out.sort_by_key(|a| a.rank);
        out.iter()
            .map(|a| format!("{}:{}", a.declaration.name, a.declaration.value))
            .collect()
    }

    #[test]
    fn declarations_keep_relevant_properties_and_importance() {
        let declarations = parse_declarations(
            "color:red; margin:0 auto!important; foo:bar; background:#fff none;",
        );
        let described: Vec<String> = declarations
            .iter()
            .map(|d| {
                format!(
                    "{:?}={}{}",
                    d.prop,
                    d.value,
                    if d.important { "!" } else { "" }
                )
            })
            .collect();
        assert_eq!(
            described,
            [
                "Color=red",
                "MarginTop=0 auto!",
                "MarginLeft=0 auto!",
                "BackgroundColor=#fff none",
                "BackgroundImage=#fff none",
                "BackgroundClip=#fff none"
            ]
        );
    }

    #[test]
    fn selectors_match_like_a_browser() {
        let html = "<html><body><div class='a b' id='outer'><p id='p1' class='x'>1</p>\
                    <p id='p2' lang='de-AT'>2</p><span id='s'>3</span></div>\
                    <custom><i id='i'>4</i></custom></body></html>";
        let css = ".a .x{color:red}\n#outer > p{opacity:0}\n.b>#p2{display:none}\n\
                   p + p{left:1px}\np ~ span{top:1px}\n[lang|=de]{width:1px}\n\
                   p:first-child{height:1px}\np:nth-child(2n){height:2px}\n\
                   p:not(.x){max-height:0}\na:hover, p:hover{display:none}\n\
                   p::before{display:none}\np:has(b){scale:0}\ndiv i{color:blue}\n\
                   body > i{color:green}\n*{clip:auto}\n@media print{p{display:none}}\n\
                   @media screen{#p1{font-size:0}}";
        assert_eq!(
            matched(css, html, "p1"),
            [
                "clip:auto",
                "height:1px",
                "scale:0",
                "color:red",
                "font-size:0",
                "opacity:0"
            ]
        );
        assert_eq!(
            matched(css, html, "p2"),
            [
                "clip:auto",
                "left:1px",
                "width:1px",
                "height:2px",
                "max-height:0",
                "scale:0",
                "opacity:0",
                "display:none"
            ]
        );
        assert_eq!(matched(css, html, "s"), ["clip:auto", "top:1px"]);
        // The unwrapped element is not there: `i` is a child of body, with no `div` around it.
        assert_eq!(matched(css, html, "i"), ["clip:auto", "color:green"]);
    }

    #[test]
    fn specificity_orders_rules() {
        let html = "<html><body><p id='p' class='c'>x</p></body></html>";
        let sheet = StyleSheet::parse("#p{color:red}\n.c{color:blue !important}\np{color:green}");
        let dom = dom::parse_document(html);
        let node = element(&dom, "p");
        let mut out = Vec::new();
        sheet
            .applied(&node, &ancestors_of(&node), &mut Budget(10_000), &mut out)
            .expect("budget");
        let winner = out.iter().max_by_key(|a| a.rank).expect("winner");
        assert_eq!(&*winner.declaration.value, "blue");
        let mut out = Vec::new();
        assert!(
            sheet
                .applied(&node, &ancestors_of(&node), &mut Budget(1), &mut out)
                .is_none(),
            "budget runs out"
        );
    }
}
