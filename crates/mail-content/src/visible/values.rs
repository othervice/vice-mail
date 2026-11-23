//! CSS values for the visibility check: lengths (with `calc()`, `min()`, `max()`, `clamp()`),
//! numbers, colours in every syntax the CSS sanitiser keeps, legacy colour attributes
//! (`bgcolor`, `<font color>`), and media queries.
//!
//! Everything is evaluated for a reference reading pane of [`VIEWPORT_WIDTH`] ×
//! [`VIEWPORT_HEIGHT`] CSS px in the light appearance (HTML mails are shown on their own
//! background and not recoloured, `docs/DESIGN.md`). What cannot be evaluated is `None` or
//! [`Color::Unknown`]; the caller decides which way that counts.

use cssparser::{ParseError, Parser, Token};

/// Width of the reading pane assumed for media queries, viewport units and percentages of
/// horizontal offsets.
pub(super) const VIEWPORT_WIDTH: f32 = 800.0;
/// Height assumed likewise.
pub(super) const VIEWPORT_HEIGHT: f32 = 1000.0;
/// The screen assumed for `device-width` and `device-height`.
const SCREEN_WIDTH: f32 = 1440.0;
const SCREEN_HEIGHT: f32 = 900.0;
/// The initial font size.
pub(super) const ROOT_FONT_PX: f32 = 16.0;
/// How deep maths functions and media conditions may nest (the sanitiser allows 12).
const MAX_DEPTH: usize = 16;

/// What relative units mean for one value.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Basis {
    /// Font size of the element in px (`em`, `ex`, `ch`); `None`: unknown.
    pub em: Option<f32>,
    /// What 100 % is in px; `None`: unknown.
    pub percent: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Quantity {
    Number(f32),
    Length(f32),
}

type Failed = ParseError<()>;

fn failed() -> Failed {
    ParseError::custom(())
}

fn finite(quantity: Quantity) -> Option<Quantity> {
    let value = match quantity {
        Quantity::Number(v) | Quantity::Length(v) => v,
    };
    value.is_finite().then_some(quantity)
}

/// One length component (a dimension, a percentage, `0` or a maths function) in px.
pub(super) fn length(input: &mut Parser<'_>, basis: Basis) -> Option<f32> {
    match quantity(input, basis, 0)? {
        Quantity::Length(px) => Some(px),
        Quantity::Number(0.0) => Some(0.0),
        Quantity::Number(_) => None,
    }
}

/// A whole value that is exactly one length.
pub(super) fn length_value(value: &str, basis: Basis) -> Option<f32> {
    let mut input = Parser::new(value);
    let px = length(&mut input, basis)?;
    input.is_exhausted().then_some(px)
}

/// A whole value that is one number or percentage (`0.5`, `50%`, `calc(1 / 2)`).
pub(super) fn number_value(value: &str) -> Option<f32> {
    let mut input = Parser::new(value);
    let number = number(&mut input)?;
    input.is_exhausted().then_some(number)
}

/// One number or percentage component, as a number (`50%` is `0.5`).
pub(super) fn number(input: &mut Parser<'_>) -> Option<f32> {
    let state = input.state();
    if let Ok(Token::Percentage { unit_value, .. }) = input.next() {
        return Some(*unit_value);
    }
    input.reset(&state);
    match quantity(input, Basis::default(), 0)? {
        Quantity::Number(n) => Some(n),
        Quantity::Length(_) => None,
    }
}

fn quantity(input: &mut Parser<'_>, basis: Basis, depth: usize) -> Option<Quantity> {
    let token = input.next().ok()?.clone();
    match token {
        Token::Number { value, .. } => finite(Quantity::Number(value)),
        Token::Dimension {
            value, ref unit, ..
        } => finite(Quantity::Length(value * unit_px(unit, basis)?)),
        Token::Percentage { unit_value, .. } => {
            finite(Quantity::Length(unit_value * basis.percent?))
        }
        Token::Function(ref name) if depth < MAX_DEPTH => {
            let name = name.to_ascii_lowercase();
            input
                .parse_nested_block(|inner| math(&name, inner, basis, depth + 1).ok_or_else(failed))
                .ok()
                .and_then(finite)
        }
        Token::ParenthesisBlock if depth > 0 && depth < MAX_DEPTH => input
            .parse_nested_block(|inner| match sum(inner, basis, depth + 1) {
                Some(value) if inner.is_exhausted() => Ok(value),
                _ => Err(failed()),
            })
            .ok(),
        _ => None,
    }
}

fn math(name: &str, input: &mut Parser<'_>, basis: Basis, depth: usize) -> Option<Quantity> {
    match name {
        "calc" | "-webkit-calc" | "-moz-calc" => {
            let value = sum(input, basis, depth)?;
            input.is_exhausted().then_some(value)
        }
        "min" | "max" | "clamp" => {
            let values: Vec<Quantity> = input
                .parse_comma_separated(|item| sum(item, basis, depth).ok_or_else(failed))
                .ok()?;
            let length = matches!(values.first(), Some(Quantity::Length(_)));
            let mut numbers = Vec::with_capacity(values.len());
            for value in values {
                match (value, length) {
                    (Quantity::Length(v), true) | (Quantity::Number(v), false) => numbers.push(v),
                    _ => return None,
                }
            }
            let result = match (name, numbers.as_slice()) {
                ("min", [first, rest @ ..]) => rest.iter().fold(*first, |a, b| a.min(*b)),
                ("max", [first, rest @ ..]) => rest.iter().fold(*first, |a, b| a.max(*b)),
                ("clamp", [low, value, high]) => value.min(*high).max(*low),
                _ => return None,
            };
            Some(if length {
                Quantity::Length(result)
            } else {
                Quantity::Number(result)
            })
        }
        "abs" => {
            let value = sum(input, basis, depth)?;
            input.is_exhausted().then_some(match value {
                Quantity::Number(v) => Quantity::Number(v.abs()),
                Quantity::Length(v) => Quantity::Length(v.abs()),
            })
        }
        _ => None,
    }
}

fn sum(input: &mut Parser<'_>, basis: Basis, depth: usize) -> Option<Quantity> {
    let mut total = product(input, basis, depth)?;
    loop {
        let sign = match input.next() {
            Err(_) => return Some(total),
            Ok(Token::Delim('+')) => 1.0,
            Ok(Token::Delim('-')) => -1.0,
            Ok(_) => return None,
        };
        let rhs = product(input, basis, depth)?;
        total = match (total, rhs) {
            (Quantity::Number(a), Quantity::Number(b)) => Quantity::Number(a + sign * b),
            (Quantity::Length(a), Quantity::Length(b)) => Quantity::Length(a + sign * b),
            _ => return None,
        };
    }
}

fn product(input: &mut Parser<'_>, basis: Basis, depth: usize) -> Option<Quantity> {
    let mut value = quantity(input, basis, depth)?;
    loop {
        let state = input.state();
        match input.next() {
            Ok(Token::Delim('*')) => {
                value = match (value, quantity(input, basis, depth)?) {
                    (Quantity::Number(a), Quantity::Number(b)) => Quantity::Number(a * b),
                    (Quantity::Number(a), Quantity::Length(b))
                    | (Quantity::Length(b), Quantity::Number(a)) => Quantity::Length(a * b),
                    _ => return None,
                };
            }
            Ok(Token::Delim('/')) => {
                let Quantity::Number(divisor) = quantity(input, basis, depth)? else {
                    return None;
                };
                if divisor == 0.0 {
                    return None;
                }
                value = match value {
                    Quantity::Number(a) => Quantity::Number(a / divisor),
                    Quantity::Length(a) => Quantity::Length(a / divisor),
                };
            }
            _ => {
                input.reset(&state);
                return Some(value);
            }
        }
    }
}

/// A dimension token (`-9999px`, `2em`) in px.
pub(super) fn dimension_px(value: f32, unit: &str, basis: Basis) -> Option<f32> {
    let px = value * unit_px(unit, basis)?;
    px.is_finite().then_some(px)
}

fn unit_px(unit: &str, basis: Basis) -> Option<f32> {
    let unit = unit.to_ascii_lowercase();
    Some(match unit.as_str() {
        "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "em" | "ic" => basis.em?,
        "ex" | "ch" => basis.em? * 0.5,
        "cap" => basis.em? * 0.7,
        "lh" => basis.em? * 1.2,
        "rem" | "ric" => ROOT_FONT_PX,
        "rex" | "rch" => ROOT_FONT_PX * 0.5,
        "rlh" => ROOT_FONT_PX * 1.2,
        "vw" | "svw" | "lvw" | "dvw" | "vi" | "svi" | "lvi" | "dvi" => VIEWPORT_WIDTH / 100.0,
        "vh" | "svh" | "lvh" | "dvh" | "vb" | "svb" | "lvb" | "dvb" => VIEWPORT_HEIGHT / 100.0,
        "vmin" | "svmin" | "lvmin" | "dvmin" => VIEWPORT_WIDTH.min(VIEWPORT_HEIGHT) / 100.0,
        "vmax" | "svmax" | "lvmax" | "dvmax" => VIEWPORT_WIDTH.max(VIEWPORT_HEIGHT) / 100.0,
        _ => return None,
    })
}

/// An angle in degrees.
fn angle_deg(value: f32, unit: &str) -> Option<f32> {
    let unit = unit.to_ascii_lowercase();
    Some(match unit.as_str() {
        "deg" => value,
        "rad" => value.to_degrees(),
        "grad" => value * 0.9,
        "turn" => value * 360.0,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// Colours

/// A colour in sRGB, every channel and the alpha in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub(super) const WHITE: Self = Self::hex(0xFF_FF_FF);
    pub(super) const BLACK: Self = Self::hex(0);
    /// `a:link` in WebKit and Blink.
    pub(super) const LINK: Self = Self::hex(0x00_00_EE);
    /// The background of `<mark>`.
    pub(super) const MARK: Self = Self::hex(0xFF_FF_00);
    const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    const fn hex(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
            g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
            b: (rgb & 0xFF) as f32 / 255.0,
            a: 1.0,
        }
    }

    fn clamped(r: f32, g: f32, b: f32, a: f32) -> Option<Self> {
        let channel = |v: f32| v.is_finite().then(|| v.clamp(0.0, 1.0));
        Some(Self {
            r: channel(r)?,
            g: channel(g)?,
            b: channel(b)?,
            a: channel(a)?,
        })
    }

    /// This colour drawn over an opaque `below`.
    pub(super) fn over(self, below: Self) -> Self {
        let mix = |top: f32, bottom: f32| top * self.a + bottom * (1.0 - self.a);
        Self {
            r: mix(self.r, below.r),
            g: mix(self.g, below.g),
            b: mix(self.b, below.b),
            a: 1.0,
        }
    }

    /// Relative luminance (WCAG 2).
    fn luminance(self) -> f32 {
        let linear = |c: f32| {
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }
}

/// Contrast ratio of two opaque colours (WCAG 2): 1 for equal colours, 21 for black on white.
pub(super) fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Color {
    Rgba(Rgba),
    /// `currentcolor`.
    Current,
    /// A valid colour this module does not evaluate (system accent colours, `color-mix()` with
    /// `currentcolor`).
    Unknown,
    /// Relative colour syntax (`rgb(from white r g b)`): valid, not evaluated, and not used by
    /// any mail tool, so only an attempt to get around this check would use it.
    Relative,
}

/// A whole value that is one colour. `None`: not a colour (the browser ignores it too).
pub(super) fn color_value(value: &str) -> Option<Color> {
    let mut input = Parser::new(value);
    let color = color(&mut input)?;
    input.is_exhausted().then_some(color)
}

/// One colour component.
pub(super) fn color(input: &mut Parser<'_>) -> Option<Color> {
    let token = input.next().ok()?.clone();
    match token {
        Token::Hash(ref hex) | Token::IDHash(ref hex) => hex_color(hex).map(Color::Rgba),
        Token::Ident(ref name) => keyword_color(&name.to_ascii_lowercase()),
        Token::Function(ref name) => {
            let name = name.to_ascii_lowercase();
            if !COLOR_FUNCTIONS.contains(&name.as_str()) {
                return None;
            }
            input
                .parse_nested_block(|inner| color_function(&name, inner, 0).ok_or_else(failed))
                .ok()
        }
        _ => None,
    }
}

const COLOR_FUNCTIONS: &[&str] = &[
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
];

fn hex_color(hex: &str) -> Option<Rgba> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u32::from_str_radix(&hex[i..=i], 16).ok();
    let pair = |i: usize| u32::from_str_radix(&hex[i..i + 2], 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => (
            digit(0)? * 17,
            digit(1)? * 17,
            digit(2)? * 17,
            if hex.len() == 4 { digit(3)? * 17 } else { 255 },
        ),
        6 | 8 => (
            pair(0)?,
            pair(2)?,
            pair(4)?,
            if hex.len() == 8 { pair(6)? } else { 255 },
        ),
        _ => return None,
    };
    Rgba::clamped(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

/// Named colours, system colours (light appearance) and the colour keywords.
fn keyword_color(name: &str) -> Option<Color> {
    match name {
        "transparent" => return Some(Color::Rgba(Rgba::TRANSPARENT)),
        "currentcolor" => return Some(Color::Current),
        _ => {}
    }
    if let Ok(i) = NAMED_COLORS.binary_search_by(|(n, _)| (*n).cmp(name)) {
        return Some(Color::Rgba(Rgba::hex(NAMED_COLORS[i].1)));
    }
    let rgb = match name {
        "canvas" | "field" | "window" | "buttonhighlight" | "-webkit-text-background" => 0xFF_FF_FF,
        "canvastext" | "fieldtext" | "buttontext" | "windowtext" | "menutext" | "captiontext"
        | "infotext" | "marktext" | "highlighttext" | "selecteditemtext" | "-webkit-text" => 0,
        "linktext" | "-webkit-link" => 0x00_00_EE,
        "visitedtext" => 0x55_1A_8B,
        "activetext" | "-webkit-activelink" => 0xFF_00_00,
        "mark" => 0xFF_FF_00,
        "graytext" => 0x80_80_80,
        "buttonface"
        | "buttonborder"
        | "threedface"
        | "menu"
        | "scrollbar"
        | "highlight"
        | "selecteditem"
        | "accentcolor"
        | "accentcolortext"
        | "activeborder"
        | "activecaption"
        | "appworkspace"
        | "background"
        | "inactiveborder"
        | "inactivecaption"
        | "inactivecaptiontext"
        | "infobackground"
        | "threeddarkshadow"
        | "threedhighlight"
        | "threedlightshadow"
        | "threedshadow"
        | "windowframe"
        | "buttonshadow"
        | "-webkit-focus-ring-color" => return Some(Color::Unknown),
        _ => return None,
    };
    Some(Color::Rgba(Rgba::hex(rgb)))
}

#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Number(f32),
    /// 0 to 100.
    Percent(f32),
    /// Degrees.
    Angle(f32),
    Slash,
    Comma,
    Color(Color),
    Word(String),
}

fn color_function(name: &str, input: &mut Parser<'_>, depth: usize) -> Option<Color> {
    let mut args = Vec::new();
    while let Ok(token) = input.next() {
        let token = token.clone();
        let arg = match token {
            Token::Number { value, .. } => Arg::Number(value),
            Token::Percentage { unit_value, .. } => Arg::Percent(unit_value * 100.0),
            Token::Dimension {
                value, ref unit, ..
            } => Arg::Angle(angle_deg(value, unit)?),
            Token::Ident(ref word) => {
                let word = word.to_ascii_lowercase();
                if word == "from" {
                    // Relative colour syntax; skip the rest of the arguments.
                    while input.next().is_ok() {}
                    return Some(Color::Relative);
                }
                if word == "none" {
                    Arg::Number(0.0)
                } else if let Some(color) = keyword_color(&word) {
                    Arg::Color(color)
                } else {
                    Arg::Word(word)
                }
            }
            Token::Delim('/') => Arg::Slash,
            Token::Comma => Arg::Comma,
            Token::Hash(ref hex) | Token::IDHash(ref hex) => {
                Arg::Color(Color::Rgba(hex_color(hex)?))
            }
            Token::Function(ref function) if depth < MAX_DEPTH => {
                let function = function.to_ascii_lowercase();
                if COLOR_FUNCTIONS.contains(&function.as_str()) {
                    let color = input
                        .parse_nested_block(|inner| {
                            color_function(&function, inner, depth + 1).ok_or_else(failed)
                        })
                        .ok()?;
                    Arg::Color(color)
                } else {
                    // Maths: percentages evaluate as "percent of 100".
                    let basis = Basis {
                        em: None,
                        percent: Some(100.0),
                    };
                    let value = input
                        .parse_nested_block(|inner| {
                            math(&function, inner, basis, depth + 1).ok_or_else(failed)
                        })
                        .ok()?;
                    match value {
                        Quantity::Number(n) => Arg::Number(n),
                        Quantity::Length(p) => Arg::Percent(p),
                    }
                }
            }
            _ => return None,
        };
        args.push(arg);
        if args.len() > 32 {
            return None;
        }
    }
    match name {
        "rgb" | "rgba" => rgb(&args),
        "hsl" | "hsla" => hsl(&args),
        "hwb" => hwb(&args),
        "lab" | "lch" | "oklab" | "oklch" => lab_family(name, &args),
        "color" => color_space(&args),
        "color-mix" => color_mix(&args),
        _ => None,
    }
}

/// The three channels and the alpha of a colour function, modern (`r g b / a`) or legacy
/// (`r, g, b, a`) syntax.
fn channels(args: &[Arg]) -> Option<([&Arg; 3], f32)> {
    let legacy = args.contains(&Arg::Comma);
    let (values, alpha): (Vec<&Arg>, Option<&Arg>) = if legacy {
        let values: Vec<&Arg> = args.iter().filter(|a| **a != Arg::Comma).collect();
        match values.len() {
            3 => (values, None),
            4 => (values[..3].to_vec(), Some(values[3])),
            _ => return None,
        }
    } else {
        match args.iter().position(|a| *a == Arg::Slash) {
            Some(slash) if args.len() == slash + 2 => {
                (args[..slash].iter().collect(), Some(&args[slash + 1]))
            }
            Some(_) => return None,
            None => (args.iter().collect(), None),
        }
    };
    let [a, b, c] = values.as_slice() else {
        return None;
    };
    let alpha = match alpha {
        None => 1.0,
        Some(Arg::Number(n)) => *n,
        Some(Arg::Percent(p)) => p / 100.0,
        Some(_) => return None,
    };
    Some(([*a, *b, *c], alpha.clamp(0.0, 1.0)))
}

fn hue(arg: &Arg) -> Option<f32> {
    match arg {
        Arg::Number(deg) | Arg::Angle(deg) => Some(*deg),
        _ => None,
    }
}

/// A channel as a fraction: numbers scaled by `number_scale`, percentages by 1/100.
fn fraction(arg: &Arg, number_scale: f32) -> Option<f32> {
    match arg {
        Arg::Number(n) => Some(n * number_scale),
        Arg::Percent(p) => Some(p / 100.0),
        _ => None,
    }
}

fn rgb(args: &[Arg]) -> Option<Color> {
    let ([r, g, b], alpha) = channels(args)?;
    let scale = 1.0 / 255.0;
    Rgba::clamped(
        fraction(r, scale)?,
        fraction(g, scale)?,
        fraction(b, scale)?,
        alpha,
    )
    .map(Color::Rgba)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let f = |n: f32| {
        let k = (n + h / 30.0).rem_euclid(12.0);
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    (f(0.0), f(8.0), f(4.0))
}

fn hsl(args: &[Arg]) -> Option<Color> {
    let ([h, s, l], alpha) = channels(args)?;
    let (r, g, b) = hsl_to_rgb(hue(h)?, fraction(s, 0.01)?, fraction(l, 0.01)?);
    Rgba::clamped(r, g, b, alpha).map(Color::Rgba)
}

fn hwb(args: &[Arg]) -> Option<Color> {
    let ([h, w, b], alpha) = channels(args)?;
    let (white, black) = (
        fraction(w, 0.01)?.clamp(0.0, 1.0),
        fraction(b, 0.01)?.clamp(0.0, 1.0),
    );
    if white + black >= 1.0 {
        let gray = white / (white + black);
        return Rgba::clamped(gray, gray, gray, alpha).map(Color::Rgba);
    }
    let (r, g, bl) = hsl_to_rgb(hue(h)?, 1.0, 0.5);
    let scale = 1.0 - white - black;
    Rgba::clamped(
        r * scale + white,
        g * scale + white,
        bl * scale + white,
        alpha,
    )
    .map(Color::Rgba)
}

fn gamma(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

fn from_linear(r: f32, g: f32, b: f32, alpha: f32) -> Option<Color> {
    Rgba::clamped(gamma(r), gamma(g), gamma(b), alpha).map(Color::Rgba)
}

/// CIE XYZ (D65) to linear sRGB.
fn xyz_d65_to_linear(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (
        3.240_97 * x - 1.537_383 * y - 0.498_611 * z,
        -0.969_244 * x + 1.875_968 * y + 0.041_555 * z,
        0.055_63 * x - 0.203_977 * y + 1.056_972 * z,
    )
}

/// CIE XYZ (D50) to D65 (Bradford).
fn d50_to_d65(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (
        0.955_473_4 * x - 0.023_098_5 * y + 0.063_259_3 * z,
        -0.028_369_7 * x + 1.009_995_5 * y + 0.021_041_4 * z,
        0.012_314 * x - 0.020_507_7 * y + 1.330_366 * z,
    )
}

fn lab_family(name: &str, args: &[Arg]) -> Option<Color> {
    let ([l, x, y], alpha) = channels(args)?;
    let ok = name.starts_with("ok");
    // Lightness: CIE 0..100, OK 0..1; percentages map to the full range.
    let lightness = match l {
        Arg::Number(n) => *n,
        Arg::Percent(p) => {
            if ok {
                p / 100.0
            } else {
                *p
            }
        }
        _ => return None,
    };
    // Percent reference of a/b (lab) and chroma (lch).
    let (ab_ref, chroma_ref) = if ok { (0.4, 0.4) } else { (125.0, 150.0) };
    let (a, b) = if name.ends_with("lch") {
        let chroma = match x {
            Arg::Number(n) => *n,
            Arg::Percent(p) => p / 100.0 * chroma_ref,
            _ => return None,
        };
        let h = hue(y)?.to_radians();
        (chroma * h.cos(), chroma * h.sin())
    } else {
        let axis = |arg: &Arg| match arg {
            Arg::Number(n) => Some(*n),
            Arg::Percent(p) => Some(p / 100.0 * ab_ref),
            _ => None,
        };
        (axis(x)?, axis(y)?)
    };
    if ok {
        let l_ = lightness + 0.396_337_8 * a + 0.215_803_76 * b;
        let m_ = lightness - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = lightness - 0.089_484_18 * a - 1.291_485_5 * b;
        let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
        return from_linear(
            4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
            -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
            -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
            alpha,
        );
    }
    let fy = (lightness + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inverse = |t: f32| {
        let delta = 6.0 / 29.0;
        if t > delta {
            t.powi(3)
        } else {
            3.0 * delta * delta * (t - 4.0 / 29.0)
        }
    };
    let (x, y, z) = d50_to_d65(0.964_22 * inverse(fx), inverse(fy), 0.825_21 * inverse(fz));
    let (r, g, b) = xyz_d65_to_linear(x, y, z);
    from_linear(r, g, b, alpha)
}

/// `color(<space> c1 c2 c3 / a)`. RGB spaces other than sRGB are read as sRGB: close enough to
/// tell near-equal colours apart.
fn color_space(args: &[Arg]) -> Option<Color> {
    let (Arg::Word(space), rest) = args.split_first()? else {
        return None;
    };
    let ([a, b, c], alpha) = channels(rest)?;
    let (a, b, c) = (fraction(a, 1.0)?, fraction(b, 1.0)?, fraction(c, 1.0)?);
    match space.as_str() {
        "srgb" | "display-p3" | "a98-rgb" | "prophoto-rgb" | "rec2020" => {
            Rgba::clamped(a, b, c, alpha).map(Color::Rgba)
        }
        "srgb-linear" => from_linear(a, b, c, alpha),
        "xyz" | "xyz-d65" => {
            let (r, g, b) = xyz_d65_to_linear(a, b, c);
            from_linear(r, g, b, alpha)
        }
        "xyz-d50" => {
            let (x, y, z) = d50_to_d65(a, b, c);
            let (r, g, b) = xyz_d65_to_linear(x, y, z);
            from_linear(r, g, b, alpha)
        }
        _ => Some(Color::Unknown),
    }
}

/// `color-mix(in <space>, c1 p1?, c2 p2?)`, mixed in sRGB whatever the space: mixes of near-equal
/// colours stay near-equal in every space.
fn color_mix(args: &[Arg]) -> Option<Color> {
    let parts: Vec<&[Arg]> = args.split(|a| *a == Arg::Comma).collect();
    let [method, first, second] = parts.as_slice() else {
        return None;
    };
    if method.first() != Some(&Arg::Word("in".to_owned())) {
        return None;
    }
    let item = |part: &[Arg]| -> Option<(Color, Option<f32>)> {
        let mut color = None;
        let mut percent = None;
        for arg in part {
            match arg {
                Arg::Color(c) if color.is_none() => color = Some(*c),
                Arg::Percent(p) if percent.is_none() => percent = Some(p / 100.0),
                _ => return None,
            }
        }
        Some((color?, percent))
    };
    let (c1, p1) = item(first)?;
    let (c2, p2) = item(second)?;
    let (Color::Rgba(c1), Color::Rgba(c2)) = (c1, c2) else {
        return Some(match (c1, c2) {
            (Color::Relative, _) | (_, Color::Relative) => Color::Relative,
            _ => Color::Unknown,
        });
    };
    let (p1, p2) = match (p1, p2) {
        (None, None) => (0.5, 0.5),
        (Some(p), None) => (p, 1.0 - p),
        (None, Some(p)) => (1.0 - p, p),
        (Some(a), Some(b)) => (a, b),
    };
    let total = p1 + p2;
    if total <= 0.0 {
        return None;
    }
    let (w1, w2) = (p1 / total, p2 / total);
    let alpha = c1.a * w1 + c2.a * w2;
    let channel = |a: f32, b: f32| {
        if alpha > 0.0 {
            (a * c1.a * w1 + b * c2.a * w2) / alpha
        } else {
            0.0
        }
    };
    Rgba::clamped(
        channel(c1.r, c2.r),
        channel(c1.g, c2.g),
        channel(c1.b, c2.b),
        alpha * total.min(1.0),
    )
    .map(Color::Rgba)
}

/// A colour attribute (`bgcolor`, `<font color>`, `<body text>`) by the HTML standard's rules
/// for parsing a legacy colour value. `None`: transparent or empty.
pub(super) fn legacy_color(value: &str) -> Option<Rgba> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    if value.is_empty() || value.eq_ignore_ascii_case("transparent") {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if let Ok(i) = NAMED_COLORS.binary_search_by(|(n, _)| (*n).cmp(lower.as_str())) {
        return Some(Rgba::hex(NAMED_COLORS[i].1));
    }
    if value.len() == 4
        && value.starts_with('#')
        && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return hex_color(&value[1..]);
    }
    let mut chars: Vec<char> = Vec::new();
    for c in value.chars() {
        if u32::from(c) > 0xFFFF {
            chars.extend(['0', '0']);
        } else {
            chars.push(c);
        }
    }
    chars.truncate(128);
    if chars.first() == Some(&'#') {
        chars.remove(0);
    }
    let mut digits: Vec<u8> = chars
        .iter()
        .map(|c| {
            if c.is_ascii_hexdigit() {
                *c as u8
            } else {
                b'0'
            }
        })
        .collect();
    while digits.is_empty() || !digits.len().is_multiple_of(3) {
        digits.push(b'0');
    }
    let len = digits.len() / 3;
    let mut parts: Vec<&[u8]> = digits.chunks(len).collect();
    let mut start = len.saturating_sub(8);
    let mut width = len - start;
    while width > 2 && parts.iter().all(|p| p[start] == b'0') {
        start += 1;
        width -= 1;
    }
    for part in &mut parts {
        *part = &part[start..start + width.min(2)];
    }
    let channel = |part: &[u8]| {
        let text = std::str::from_utf8(part).ok()?;
        u32::from_str_radix(text, 16).ok()
    };
    let (r, g, b) = (channel(parts[0])?, channel(parts[1])?, channel(parts[2])?);
    Some(Rgba::hex((r << 16) | (g << 8) | b))
}

/// CSS named colours, sorted by name.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

// ---------------------------------------------------------------------------------------------
// Media queries

/// Whether the rules of `@media <prelude>` apply in the reference reading pane: a screen of
/// [`VIEWPORT_WIDTH`] × [`VIEWPORT_HEIGHT`] px, light appearance, a pointer that can hover.
/// A query that cannot be evaluated counts as matching, so its rules are applied: hiding rules
/// should not escape the check by an unusual condition.
pub(super) fn media_matches(prelude: &str) -> bool {
    let mut input = Parser::new(prelude);
    if input.is_exhausted() {
        return true;
    }
    let queries: Result<Vec<Option<bool>>, ParseError<()>> =
        input.parse_comma_separated(|query| Ok(media_query(query, 0)));
    match queries {
        Ok(results) => results.iter().any(|r| *r != Some(false)),
        Err(_) => true,
    }
}

fn and3(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn or3(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// One media query or condition: `[not|only] type [and (…)]*`, `(…) and|or (…)`, `not (…)`.
fn media_query(input: &mut Parser<'_>, depth: usize) -> Option<bool> {
    if depth > MAX_DEPTH {
        while input.next().is_ok() {}
        return None;
    }
    let mut negate = false;
    let mut or = false;
    let mut first = true;
    let mut valid = true;
    let mut acc: Option<Option<bool>> = None;
    while let Ok(token) = input.next() {
        let token = token.clone();
        let term = match token {
            Token::Ident(ref word) => match word.to_ascii_lowercase().as_str() {
                "not" if first => {
                    negate = true;
                    None
                }
                "only" if first => None,
                "and" => {
                    or = false;
                    None
                }
                "or" => {
                    or = true;
                    None
                }
                media_type => Some(Some(matches!(media_type, "all" | "screen"))),
            },
            Token::ParenthesisBlock => {
                let result = input
                    .parse_nested_block(|inner| {
                        Ok::<_, ParseError<()>>(media_condition(inner, depth + 1))
                    })
                    .unwrap_or(None);
                Some(result)
            }
            _ => {
                valid = false;
                None
            }
        };
        if let Some(term) = term {
            acc = Some(match acc {
                None => term,
                Some(previous) if or => or3(previous, term),
                Some(previous) => and3(previous, term),
            });
        }
        first = false;
    }
    if !valid {
        return None;
    }
    let result = acc.unwrap_or(Some(true));
    if negate { result.map(|r| !r) } else { result }
}

/// The inside of `( … )`: a nested condition, `name: value`, a boolean feature, or a range.
fn media_condition(input: &mut Parser<'_>, depth: usize) -> Option<bool> {
    let start = input.state();
    let first = input.next().ok()?.clone();
    let result = match first {
        Token::ParenthesisBlock => {
            input.reset(&start);
            media_query(input, depth + 1)
        }
        Token::Ident(ref word) if word.eq_ignore_ascii_case("not") => {
            input.reset(&start);
            media_query(input, depth + 1)
        }
        Token::Ident(ref name) => {
            let name = name.to_ascii_lowercase();
            let after = input.state();
            match input.next() {
                Err(_) => boolean_feature(&name),
                Ok(Token::Colon) => feature(&name, input),
                Ok(_) => {
                    input.reset(&after);
                    range(Some(name), input)
                }
            }
        }
        _ => {
            input.reset(&start);
            range(None, input)
        }
    };
    while input.next().is_ok() {}
    result
}

fn boolean_feature(name: &str) -> Option<bool> {
    match name {
        "color" | "hover" | "any-hover" | "pointer" | "any-pointer" | "width" | "height" => {
            Some(true)
        }
        "monochrome" | "grid" => Some(false),
        _ => None,
    }
}

/// Size of a range feature in the reference pane.
fn size_feature(name: &str) -> Option<f32> {
    Some(match name {
        "width" => VIEWPORT_WIDTH,
        "height" => VIEWPORT_HEIGHT,
        "device-width" => SCREEN_WIDTH,
        "device-height" => SCREEN_HEIGHT,
        _ => return None,
    })
}

const MEDIA_BASIS: Basis = Basis {
    em: Some(ROOT_FONT_PX),
    percent: None,
};

fn feature(name: &str, input: &mut Parser<'_>) -> Option<bool> {
    let (prefix, base) = match name.split_once('-') {
        Some((prefix @ ("min" | "max"), base)) => (Some(prefix), base),
        _ => (None, name),
    };
    if let Some(actual) = size_feature(base) {
        let wanted = length(input, MEDIA_BASIS)?;
        return Some(match prefix {
            Some("min") => actual >= wanted,
            Some("max") => actual <= wanted,
            _ => (actual - wanted).abs() < 0.5,
        });
    }
    match (prefix, base) {
        (None, "prefers-color-scheme") => Some(ident(input)? == "light"),
        (None, "hover" | "any-hover") => Some(ident(input)? == "hover"),
        (None, "pointer" | "any-pointer") => Some(ident(input)? == "fine"),
        (Some("min"), "color") => Some(number(input)? <= 8.0),
        (None, "monochrome") => Some(number(input)? == 0.0),
        (Some("min"), "monochrome") => Some(number(input)? <= 0.0),
        _ => None,
    }
}

fn ident(input: &mut Parser<'_>) -> Option<String> {
    match input.next() {
        Ok(Token::Ident(word)) => Some(word.to_ascii_lowercase()),
        _ => None,
    }
}

/// `width >= 600px`, `400px <= width < 700px`, `600px < width`.
fn range(name: Option<String>, input: &mut Parser<'_>) -> Option<bool> {
    // Tokens: values, comparison operators, and the feature name.
    #[derive(Clone, Copy)]
    enum Item {
        Value(f32),
        Op(char, bool),
        Name(f32),
    }
    let mut items = Vec::new();
    if let Some(name) = name {
        items.push(Item::Name(size_feature(&name)?));
    }
    loop {
        let state = input.state();
        let token = match input.next() {
            Ok(token) => token.clone(),
            Err(_) => break,
        };
        match token {
            Token::Delim(c @ ('<' | '>' | '=')) => {
                let state = input.state();
                let or_equal =
                    c != '=' && matches!(input.next_including_whitespace(), Ok(Token::Delim('=')));
                if !or_equal {
                    input.reset(&state);
                }
                items.push(Item::Op(c, or_equal));
            }
            Token::Ident(ref name) => {
                items.push(Item::Name(size_feature(&name.to_ascii_lowercase())?));
            }
            _ => {
                input.reset(&state);
                items.push(Item::Value(length(input, MEDIA_BASIS)?));
            }
        }
        if items.len() > 5 {
            return None;
        }
    }
    let value = |item: Item| match item {
        Item::Value(v) | Item::Name(v) => Some(v),
        Item::Op(..) => None,
    };
    let compare = |a: f32, op: char, or_equal: bool, b: f32| match op {
        '<' => a < b || (or_equal && a <= b),
        '>' => a > b || (or_equal && a >= b),
        _ => (a - b).abs() < 0.5,
    };
    if items.len() < 3 || items.len() % 2 == 0 {
        return None;
    }
    let mut result = true;
    for i in (0..items.len() - 2).step_by(2) {
        let Item::Op(op, or_equal) = items[i + 1] else {
            return None;
        };
        result &= compare(value(items[i])?, op, or_equal, value(items[i + 2])?);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: &str, em: f32) -> Option<f32> {
        length_value(
            value,
            Basis {
                em: Some(em),
                percent: Some(200.0),
            },
        )
    }

    fn rgba(value: &str) -> Rgba {
        match color_value(value) {
            Some(Color::Rgba(c)) => c,
            other => panic!("{value}: {other:?}"),
        }
    }

    fn close(a: Rgba, b: Rgba) -> bool {
        (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
    }

    #[test]
    fn lengths_and_maths() {
        assert_eq!(px("0", 16.0), Some(0.0));
        assert_eq!(px("12px", 16.0), Some(12.0));
        assert_eq!(px("1.5em", 10.0), Some(15.0));
        assert_eq!(px("50%", 16.0), Some(100.0));
        assert_eq!(px("3pt", 16.0), Some(4.0));
        assert_eq!(px("calc(1px - 1px)", 16.0), Some(0.0));
        assert_eq!(px("calc((2px + 1px) * 2)", 16.0), Some(6.0));
        assert_eq!(px("min(0px, 10px)", 16.0), Some(0.0));
        assert_eq!(px("clamp(0px, 5px, 1px)", 16.0), Some(1.0));
        assert_eq!(px("-webkit-calc(10px / 2)", 16.0), Some(5.0));
        assert_eq!(px("calc(-9999px + 1em)", 1.0), Some(-9998.0));
        assert_eq!(px("12", 16.0), None, "unitless lengths are invalid");
        assert_eq!(px("1px 2px", 16.0), None);
        assert_eq!(px("calc(1px * 2px)", 16.0), None);
        assert_eq!(px("calc(1px / 0)", 16.0), None);
        assert_eq!(px("auto", 16.0), None);
        assert_eq!(number_value("0.5"), Some(0.5));
        assert_eq!(number_value("50%"), Some(0.5));
        assert_eq!(number_value("calc(1 / 4)"), Some(0.25));
        let deep = format!("{}1px{}", "calc(".repeat(40), ")".repeat(40));
        assert_eq!(px(&deep, 16.0), None, "nesting is bounded");
    }

    #[test]
    fn colours_in_every_syntax() {
        let white = Rgba::WHITE;
        for value in [
            "#fff",
            "#FFFFFF",
            "#ffff",
            "#ffffffff",
            "white",
            "WHITE",
            "rgb(255, 255, 255)",
            "rgba(100%, 100%, 100%, 1)",
            "rgb(255 255 255 / 100%)",
            "rgb(calc(255) 255 255)",
            "hsl(0, 0%, 100%)",
            "hsl(120deg 50% 100%)",
            "hwb(0 100% 0%)",
            "lab(100 0 0)",
            "lch(100% 0 0)",
            "oklab(1 0 0)",
            "oklch(100% 0 0deg)",
            "color(srgb 1 1 1)",
            "color(xyz 0.9505 1 1.089)",
            "color-mix(in srgb, white, white 30%)",
            "Canvas",
        ] {
            assert!(close(rgba(value), white), "{value}: {:?}", rgba(value));
        }
        assert!(close(rgba("snow"), Rgba::hex(0xFFFAFA)));
        assert!(close(rgba("hsl(240 100% 50%)"), Rgba::hex(0x0000FF)));
        assert!(close(
            rgba("oklch(0.628 0.2577 29.23)"),
            Rgba::hex(0xFF0000)
        ));
        assert!(close(rgba("lab(54.29 80.8 69.89)"), Rgba::hex(0xFF0000)));
        assert_eq!(rgba("transparent").a, 0.0);
        assert_eq!(rgba("rgba(0,0,0,0)").a, 0.0);
        assert_eq!(rgba("#0000").a, 0.0);
        assert!((rgba("rgb(0 0 0 / 50%)").a - 0.5).abs() < 0.001);
        assert_eq!(color_value("currentColor"), Some(Color::Current));
        assert_eq!(color_value("rgb(from white r g b)"), Some(Color::Relative));
        assert_eq!(
            color_value("color-mix(in srgb, currentcolor, white)"),
            Some(Color::Unknown)
        );
        assert_eq!(color_value("AccentColor"), Some(Color::Unknown));
        for invalid in ["#ff", "notacolor", "rgb(1, 2)", "calc(1)", "1px"] {
            assert_eq!(color_value(invalid), None, "{invalid}");
        }
        assert!(contrast(Rgba::WHITE, Rgba::BLACK) > 20.9);
        assert!((contrast(Rgba::WHITE, Rgba::WHITE) - 1.0).abs() < 0.001);
        let half = Rgba {
            a: 0.5,
            ..Rgba::BLACK
        }
        .over(Rgba::WHITE);
        assert!((half.r - 0.5).abs() < 0.01);
    }

    #[test]
    fn legacy_colour_attributes() {
        let hex = |rgb: u32| Some(Rgba::hex(rgb));
        assert_eq!(legacy_color("#ffffff"), hex(0xFFFFFF));
        assert_eq!(legacy_color("ffffff"), hex(0xFFFFFF));
        assert_eq!(legacy_color(" White "), hex(0xFFFFFF));
        assert_eq!(legacy_color("#fff"), hex(0xFFFFFF));
        assert_eq!(legacy_color("fff"), hex(0x0F0F0F));
        // The well-known oddity of the legacy rules.
        assert_eq!(legacy_color("chucknorris"), hex(0xC00000));
        assert_eq!(legacy_color("#1a2b3c4"), hex(0x1AB340));
        assert_eq!(legacy_color(""), None);
        assert_eq!(legacy_color("transparent"), None);
    }

    #[test]
    fn media_queries_for_the_reading_pane() {
        for matching in [
            "",
            "screen",
            "all",
            "only screen and (min-width: 600px)",
            "(max-width: 1000px)",
            "screen and (prefers-color-scheme: light)",
            "print, screen",
            "not print",
            "(width >= 600px)",
            "(400px <= width <= 900px)",
            "(orientation: portrait)",
            "(-webkit-min-device-pixel-ratio: 2)",
            "(min-width: 0) or (max-width: 1px)",
            "(hover: hover)",
            "screen and (max-device-width: 2000px)",
        ] {
            assert!(media_matches(matching), "{matching:?}");
        }
        for not_matching in [
            "print",
            "only screen and (max-width: 600px)",
            "(max-width: 37.5em)",
            "screen and (prefers-color-scheme: dark)",
            "not screen",
            "(width < 600px)",
            "(min-width: 900px)",
            "speech",
            "not all and (min-width: 1px)",
            "screen and (max-device-width: 480px)",
        ] {
            assert!(!media_matches(not_matching), "{not_matching:?}");
        }
    }
}
