//! CSS from mails: `style` attributes and `<style>` blocks, re-serialised token by token with the
//! CSS tokenizer of Servo (`cssparser`, the one ammonia and wry already use). Parsing follows CSS
//! Syntax Level 3, so the browser sees the same rules and declarations we checked; there is no
//! pattern matching on text.
//!
//! What survives:
//! - qualified rules (selectors) and `@media` blocks (nested at most [`MAX_MEDIA_NESTING`])
//! - declarations of any property except `behavior`, `-moz-binding` and custom properties
//! - values made of plain tokens and the functions in [`SAFE_FUNCTIONS`]
//! - `url()` only as a `data:` image URI the caller provides (inline parts, images the user
//!   allowed); every other URL becomes `none`
//!
//! Everything else is dropped whole: `@import`, `@font-face`, all other at-rules, IE
//! `expression()`, `image-set()`, `attr()`, `var()`, `src()`, bad strings and URLs, unmatched
//! brackets, and escapes that cannot be represented. Strings are written with `<` escaped and
//! the stylesheet never contains `</`, so it cannot close its `<style>` element.

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, ToCss, Token,
    TokenSerializationType,
};

/// Callback for every URL in the CSS: returns the `data:` URI to use, or `None` to block it.
pub(crate) type UrlRewriter<'a> = dyn FnMut(&str) -> Option<String> + 'a;

/// How deep `@media` blocks may nest.
pub(crate) const MAX_MEDIA_NESTING: usize = 3;

/// How deep brackets and functions may nest inside one value or prelude.
const MAX_BLOCK_DEPTH: usize = 12;

/// Longest CSS that is looked at; the rest is ignored.
pub(crate) const MAX_CSS_BYTES: usize = 1 << 20;

/// Functions allowed in values: colours, maths, gradients, transforms, timing, shapes, filters,
/// counters and grid helpers. None of them can load anything or read the document.
const SAFE_FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklab",
    "oklch",
    "color",
    "color-mix",
    "calc",
    "min",
    "max",
    "clamp",
    "round",
    "abs",
    "sign",
    "mod",
    "rem",
    "linear-gradient",
    "radial-gradient",
    "conic-gradient",
    "repeating-linear-gradient",
    "repeating-radial-gradient",
    "repeating-conic-gradient",
    "-webkit-linear-gradient",
    "-webkit-radial-gradient",
    "-webkit-repeating-linear-gradient",
    "-webkit-repeating-radial-gradient",
    "-moz-linear-gradient",
    "-moz-radial-gradient",
    "translate",
    "translatex",
    "translatey",
    "translatez",
    "translate3d",
    "rotate",
    "rotatex",
    "rotatey",
    "rotatez",
    "rotate3d",
    "scale",
    "scalex",
    "scaley",
    "scalez",
    "scale3d",
    "skew",
    "skewx",
    "skewy",
    "matrix",
    "matrix3d",
    "perspective",
    "cubic-bezier",
    "steps",
    "rect",
    "inset",
    "circle",
    "ellipse",
    "polygon",
    "blur",
    "brightness",
    "contrast",
    "drop-shadow",
    "grayscale",
    "hue-rotate",
    "invert",
    "opacity",
    "saturate",
    "sepia",
    "counter",
    "counters",
    "minmax",
    "repeat",
    "fit-content",
];

/// Pseudo-class functions allowed in selectors.
const SELECTOR_FUNCTIONS: &[&str] = &[
    "not",
    "is",
    "where",
    "has",
    "matches",
    "-webkit-any",
    "-moz-any",
    "nth-child",
    "nth-last-child",
    "nth-of-type",
    "nth-last-of-type",
    "lang",
    "dir",
];

/// Properties that run code (old IE and Firefox) or that we never want.
const FORBIDDEN_PROPERTIES: &[&str] = &["behavior", "-ms-behavior", "-moz-binding", "binding"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Value,
    Selector,
    /// Inside `[…]` of a selector: the only place a selector may hold a string.
    AttributeSelector,
    MediaQuery,
}

/// Sanitises the value of a `style` attribute (a declaration list). Returns an empty string when
/// nothing is left.
pub(crate) fn sanitize_declarations(css: &str, urls: &mut UrlRewriter<'_>) -> String {
    let css = truncate(css);
    let mut input = Parser::new(css);
    let mut out = String::new();
    declaration_list(&mut input, urls, &mut out);
    // Harmless inside an attribute, but the same invariant as for stylesheets keeps the output
    // simple to check.
    if out.contains("</") {
        out = out.replace("</", "<\\/");
    }
    out
}

/// Sanitises a stylesheet (the text of `<style>` elements).
pub(crate) fn sanitize_stylesheet(css: &str, urls: &mut UrlRewriter<'_>) -> String {
    let css = truncate(css);
    let mut input = Parser::new(css);
    let mut out = String::new();
    rule_list(&mut input, urls, 0, &mut out);
    // Defence in depth for the HTML tokenizer: strings and identifiers are already escaped, but
    // no `</` may ever reach the raw text of a `<style>` element.
    if out.contains("</") {
        out = out.replace("</", "<\\/");
    }
    out
}

fn truncate(css: &str) -> &str {
    &css[..css.floor_char_boundary(MAX_CSS_BYTES)]
}

fn declaration_list<'i>(input: &mut Parser<'i>, urls: &mut UrlRewriter<'_>, out: &mut String) {
    let mut parser = Declarations { urls };
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

fn rule_list<'i>(
    input: &mut Parser<'i>,
    urls: &mut UrlRewriter<'_>,
    depth: usize,
    out: &mut String,
) {
    let mut parser = Rules { urls, depth };
    for rule in StyleSheetParser::new(input, &mut parser).flatten() {
        out.push_str(&rule);
    }
}

struct Declarations<'u, 'a> {
    urls: &'u mut UrlRewriter<'a>,
}

impl<'i> DeclarationParser<'i> for Declarations<'_, '_> {
    type Declaration = String;
    type Error = ();

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        _start: &ParserState,
    ) -> Result<String, ParseError<()>> {
        let lower = name.to_ascii_lowercase();
        // Real properties are ASCII identifiers (`color`, `-webkit-text-size-adjust`); custom
        // properties (`--x`) and everything else is dropped.
        let plain = lower.strip_prefix('-').unwrap_or(&lower);
        let is_property = plain.starts_with(|c: char| c.is_ascii_lowercase())
            && plain
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !is_property || FORBIDDEN_PROPERTIES.contains(&lower.as_str()) {
            return Err(ParseError::custom(()));
        }
        let mut value = String::new();
        write_tokens(input, Context::Value, self.urls, 0, &mut value)
            .map_err(|()| ParseError::custom(()))?;
        let value = value.trim();
        if value.is_empty() {
            return Err(ParseError::custom(()));
        }
        let mut out = String::new();
        let _ = cssparser::serialize_identifier(&name, &mut out);
        out.push(':');
        out.push_str(value);
        out.push(';');
        Ok(out)
    }
}

impl<'i> AtRuleParser<'i> for Declarations<'_, '_> {
    type Prelude = ();
    type AtRule = String;
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for Declarations<'_, '_> {
    type Prelude = ();
    type QualifiedRule = String;
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, String, ()> for Declarations<'_, '_> {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

struct Rules<'u, 'a> {
    urls: &'u mut UrlRewriter<'a>,
    depth: usize,
}

impl<'i> QualifiedRuleParser<'i> for Rules<'_, '_> {
    type Prelude = String;
    type QualifiedRule = String;
    type Error = ();

    fn parse_prelude(&mut self, input: &mut Parser<'i>) -> Result<String, ParseError<()>> {
        let mut prelude = String::new();
        write_tokens(input, Context::Selector, self.urls, 0, &mut prelude)
            .map_err(|()| ParseError::custom(()))?;
        let prelude = prelude.trim();
        if prelude.is_empty() {
            return Err(ParseError::custom(()));
        }
        Ok(prelude.to_owned())
    }

    fn parse_block(
        &mut self,
        prelude: String,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<String, ParseError<()>> {
        let mut body = String::new();
        declaration_list(input, self.urls, &mut body);
        if body.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("{prelude}{{{body}}}\n"))
    }
}

impl<'i> AtRuleParser<'i> for Rules<'_, '_> {
    type Prelude = String;
    type AtRule = String;
    type Error = ();

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<String, ParseError<()>> {
        if !name.eq_ignore_ascii_case("media") || self.depth >= MAX_MEDIA_NESTING {
            return Err(ParseError::custom(()));
        }
        let mut prelude = String::new();
        write_tokens(input, Context::MediaQuery, self.urls, 0, &mut prelude)
            .map_err(|()| ParseError::custom(()))?;
        Ok(prelude.trim().to_owned())
    }

    fn parse_block(
        &mut self,
        prelude: String,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<String, ParseError<()>> {
        let mut inner = String::new();
        rule_list(input, self.urls, self.depth + 1, &mut inner);
        if inner.is_empty() {
            return Ok(String::new());
        }
        let space = if prelude.is_empty() { "" } else { " " };
        Ok(format!("@media{space}{prelude}{{\n{inner}}}\n"))
    }
}

/// Writes the tokens of `input` (up to its end) to `out`, or fails if any token is not allowed
/// in `context`. URLs go through `urls`.
fn write_tokens<'i>(
    input: &mut Parser<'i>,
    context: Context,
    urls: &mut UrlRewriter<'_>,
    depth: usize,
    out: &mut String,
) -> Result<(), ()> {
    if depth > MAX_BLOCK_DEPTH {
        return Err(());
    }
    let mut previous = TokenSerializationType::Nothing;
    loop {
        let token = match input.next_including_whitespace() {
            Ok(token) => token.clone(),
            Err(_) => return Ok(()),
        };
        // Comments are dropped by the tokenizer; where two tokens would then merge into one
        // (`u/**/rl(`), keep them apart the way cssparser's own serializer does.
        let kind = token.serialization_type();
        if previous.needs_separator_when_before(kind) {
            out.push_str("/**/");
        }
        previous = kind;
        match token {
            Token::WhiteSpace(_) => out.push(' '),
            Token::Ident(ref name)
                if name.starts_with("--")
                    && matches!(context, Context::Selector | Context::AttributeSelector) =>
            {
                return Err(());
            }
            Token::Ident(_)
            | Token::Hash(_)
            | Token::IDHash(_)
            | Token::Number { .. }
            | Token::Percentage { .. }
            | Token::Dimension { .. }
            | Token::Colon
            | Token::Comma
            | Token::IncludeMatch
            | Token::DashMatch
            | Token::PrefixMatch
            | Token::SuffixMatch
            | Token::SubstringMatch => {
                let _ = token.to_css(out);
            }
            Token::Delim(c) => {
                // A lone backslash would start an escape together with the next token; `<`, `!`
                // and `@` have no place in a selector.
                let selector = matches!(context, Context::Selector | Context::AttributeSelector);
                if c == '\\' || (selector && matches!(c, '<' | '!' | '@')) {
                    return Err(());
                }
                out.push(c);
            }
            Token::QuotedString(_) if context == Context::Selector => return Err(()),
            Token::QuotedString(value) => write_string(&value, out),
            Token::UnquotedUrl(url) => {
                if context != Context::Value {
                    return Err(());
                }
                previous = write_url(&url, urls, out);
            }
            Token::Function(name) => {
                let lower = name.to_ascii_lowercase();
                if lower == "url" {
                    if context != Context::Value {
                        return Err(());
                    }
                    let url = input
                        .parse_nested_block(|inner| {
                            let url = inner.expect_string().map(|s| s.as_ref().to_owned())?;
                            inner.expect_exhausted()?;
                            Ok::<_, ParseError<()>>(url)
                        })
                        .map_err(|_| ())?;
                    previous = write_url(&url, urls, out);
                    continue;
                }
                let allowed = match context {
                    Context::Value | Context::MediaQuery => SAFE_FUNCTIONS,
                    Context::Selector | Context::AttributeSelector => SELECTOR_FUNCTIONS,
                };
                if !allowed.contains(&lower.as_str()) {
                    return Err(());
                }
                let _ = cssparser::serialize_identifier(&name, out);
                out.push('(');
                nested(input, context, urls, depth, out)?;
                out.push(')');
            }
            Token::ParenthesisBlock => {
                // `#y(` or `x(` is no function, but reads like one (`expression(`); a bracket
                // right after a name has no use in a value.
                if context == Context::Value
                    && out
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(());
                }
                out.push('(');
                nested(input, context, urls, depth, out)?;
                out.push(')');
            }
            Token::SquareBracketBlock => {
                let inner = match context {
                    Context::Selector => Context::AttributeSelector,
                    other => other,
                };
                out.push('[');
                nested(input, inner, urls, depth, out)?;
                out.push(']');
            }
            Token::AtKeyword(_)
            | Token::Semicolon
            | Token::CurlyBracketBlock
            | Token::BadUrl(_)
            | Token::BadString(_)
            | Token::CloseParenthesis
            | Token::CloseSquareBracket
            | Token::CloseCurlyBracket
            | Token::CDO
            | Token::CDC
            | Token::Comment(_) => return Err(()),
        }
    }
}

fn nested<'i>(
    input: &mut Parser<'i>,
    context: Context,
    urls: &mut UrlRewriter<'_>,
    depth: usize,
    out: &mut String,
) -> Result<(), ()> {
    input
        .parse_nested_block(|inner| {
            write_tokens(inner, context, urls, depth + 1, out).map_err(|()| ParseError::custom(()))
        })
        .map_err(|_: ParseError<()>| ())
}

/// `url("data:…")` for an allowed image, else `none` (valid wherever an image is expected).
/// Returns the kind of token written, for the separator rule.
fn write_url(url: &str, urls: &mut UrlRewriter<'_>, out: &mut String) -> TokenSerializationType {
    match urls(url.trim()) {
        Some(data_uri) => {
            out.push_str("url(");
            write_string(&data_uri, out);
            out.push(')');
            TokenSerializationType::UrlOrBadUrl
        }
        None => {
            out.push_str("none");
            TokenSerializationType::Ident
        }
    }
}

/// A double-quoted CSS string. Quotes, backslashes, control characters and `<`/`>` are written
/// as escapes, so the string can neither end early nor contain `</style>`.
fn write_string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\3c "),
            '>' => out.push_str("\\3e "),
            '\0' => out.push('\u{FFFD}'),
            c if c.is_control() => {
                out.push_str(&format!("\\{:x} ", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_all(css: &str) -> String {
        sanitize_declarations(css, &mut |_| None)
    }

    fn sheet(css: &str) -> String {
        sanitize_stylesheet(css, &mut |_| None)
    }

    #[test]
    fn plain_declarations_survive() {
        assert_eq!(
            block_all("color: red; font-size:12px ;margin:0 auto!important"),
            "color:red; font-size:12px; margin:0 auto!important;"
        );
        assert_eq!(
            block_all("background:linear-gradient(to right, #fff 0%, rgb(1,2,3) 100%)"),
            "background:linear-gradient(to right, #fff 0%, rgb(1,2,3) 100%);"
        );
        assert_eq!(
            block_all("width: calc(100% - 2em)"),
            "width:calc(100% - 2em);"
        );
    }

    #[test]
    fn urls_are_blocked_or_rewritten() {
        assert_eq!(
            block_all("background: #fff url(https://evil.example/a.png) no-repeat"),
            "background:#fff none no-repeat;"
        );
        assert_eq!(
            block_all("background-image:url('https://evil.example/a.png')"),
            "background-image:none;"
        );
        assert_eq!(
            block_all("background-image:URL(x)"),
            "background-image:none;"
        );
        assert_eq!(
            block_all("background-image:\\75 rl(x)"),
            "background-image:none;"
        );
        let mut seen = Vec::new();
        let out = sanitize_declarations("background:url(cid:logo)", &mut |url| {
            seen.push(url.to_owned());
            Some("data:image/png;base64,AAAA".to_owned())
        });
        assert_eq!(out, "background:url(\"data:image/png;base64,AAAA\");");
        assert_eq!(seen, ["cid:logo"]);
    }

    #[test]
    fn dangerous_constructs_are_dropped() {
        for css in [
            "width: expression(alert(1))",
            "behavior: url(x.htc)",
            "-moz-binding: url(x.xml#xss)",
            "background-image: image-set('https://evil.example/a.png' 1x)",
            "background-image: -webkit-image-set(url(a.png) 1x)",
            "content: attr(href)",
            "--x: url(https://evil.example/)",
            "color: var(--x)",
            "background: src('https://evil.example/')",
            "background: url(https://evil.example/ x)",
            "color: red \\\n",
            "a: {}",
        ] {
            assert_eq!(block_all(css), "", "{css}");
        }
        // One bad declaration does not take the good ones with it.
        assert_eq!(
            block_all("color:red; width:expression(alert(1)); margin:0"),
            "color:red; margin:0;"
        );
        // Found by the fuzz test: odd property names and `</` from two delimiters.
        assert_eq!(
            block_all("\u{0}-moz-bindingdata:x; c\\olor:red; -x-y:1"),
            "color:red; -x-y:1;"
        );
        assert_eq!(block_all("width: </ 1"), "width:<\\/ 1;");
    }

    #[test]
    fn stylesheets_keep_rules_and_media_only() {
        let css = "@import url(https://evil.example/a.css); @import 'b.css';\n\
                   @font-face { font-family: x; src: url(https://evil.example/f.woff) }\n\
                   @charset 'utf-8'; @namespace svg url(http://www.w3.org/2000/svg);\n\
                   .a, #b > p:not(.c) { color: red; background: url(https://e.example/x.png) }\n\
                   @media (max-width: 600px) { .a { width: 100% } @font-face { src: url(x) } }\n\
                   @keyframes k { from { color: red } }\n\
                   <!-- p { color: blue } -->";
        assert_eq!(
            sheet(css),
            ".a, #b > p:not(.c){color:red; background:none;}\n\
             @media (max-width: 600px){\n.a{width:100%;}\n}\n\
             p{color:blue;}\n"
        );
        assert_eq!(
            sheet("a[href^=\"http\"]{color:red}"),
            "a[href^=\"http\"]{color:red;}\n"
        );
        assert_eq!(sheet("a{background:url(x)}"), "a{background:none;}\n");
        assert_eq!(sheet("p:hover{}"), "");
        assert_eq!(sheet("u/**/rl(x){color:red}"), "");
        // Found by the fuzz test: markup and strings outside `[…]` are no selector.
        assert_eq!(sheet("</td><b id=\"\\75 rl(x)\">p{color:red}"), "");
        assert_eq!(sheet("p \"x\" {color:red}"), "");
        assert_eq!(sheet("--x{color:red}"), "");
        assert_eq!(block_all("width:#yexpression(1)"), "");
        assert_eq!(
            block_all("width:calc((1px + 2px) * 2)"),
            "width:calc((1px + 2px) * 2);"
        );
    }

    #[test]
    fn style_element_cannot_be_closed() {
        let out = sheet("p::after { content: \"</style><script>alert(1)</script>\" }");
        assert!(!out.to_ascii_lowercase().contains("</"), "{out}");
        assert_eq!(
            out,
            "p::after{content:\"\\3c /style\\3e \\3c script\\3e alert(1)\\3c /script\\3e \";}\n"
        );
        let out = sheet("p{font-family:\\3c\\2fstyle\\3e}");
        assert!(!out.contains("</"), "{out}");
    }

    #[test]
    fn media_nesting_is_bounded() {
        let mut css = String::new();
        for _ in 0..10 {
            css.push_str("@media screen{");
        }
        css.push_str("p{color:red}");
        for _ in 0..10 {
            css.push('}');
        }
        assert_eq!(sheet(&css), "");
        let deep = format!("width:{}1px{}", "calc(".repeat(200), ")".repeat(200));
        assert_eq!(block_all(&deep), "");
    }
}
