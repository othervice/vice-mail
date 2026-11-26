//! Stage 21 (decision 0068), redrawn in stage 22 (decision 0080): the unread count as pixels,
//! drawn for the exact size the system shows it at.
//!
//! - **Taskbar overlay** (Windows): Windows shows the overlay at the small-icon size, 16 logical
//!   pixels (16 px at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 %). An image of another size is
//!   resampled and its digits blur, so [`overlay`] draws exactly that size: a red disc for one
//!   digit, a rounded rectangle as wide as the image for more when that takes larger digits.
//! - **Notification-area icon** (Windows): the app icon of the small-icon size with the count in
//!   a pill at its bottom right ([`on_icon`]).
//!
//! The digits are glyphs of a real font: Selawik Semibold, Microsoft's open-source stand-in for
//! Segoe UI (`fonts/badge-digits.ttf`, a subset under the SIL Open Font License written by
//! `scripts/badge-font.py`). skrifa's auto-hinter fits every glyph to the pixel grid of exactly
//! the size it is shown at (the top and the bottom of the digits and their stems, edges and
//! widths, on whole pixels), a small scanline rasteriser computes the share of each pixel the
//! outline covers, and glyphs are only ever moved by whole pixels, so the fitting holds. The
//! "+" of "99+" is two bars on the pixel grid. All counts with as many glyphs get the same
//! digit size (no jumping from 41 to 42), the largest at which every one of them stands on
//! solid red with a rim around it. The disc and the pill are drawn with the same rasteriser.
//! White is mixed into the red in linear light, as the screen adds light, so the soft edge
//! pixels keep the weight of the stroke instead of turning it thin and dark.

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{
    DrawSettings, Engine, HintingInstance, HintingOptions, OutlineGlyphCollection, OutlinePen,
    Target,
};
use skrifa::{FontRef, MetadataProvider};

/// Red of the badge, as before (stage 9); white digits on it have 4.6:1.
const RED: [u8; 4] = [0xD9, 0x30, 0x25, 0xFF];
const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];

/// The digits and the capitals the auto-hinter measures (`scripts/badge-font.py`).
const FONT: &[u8] = include_bytes!("../fonts/badge-digits.ttf");
/// Font units per em, and the height of the digits (= capitals, OS/2 `sCapHeight`) in them.
const UNITS_PER_EM: f32 = 2048.0;
const FIGURE_HEIGHT: f32 = 1434.0;
/// Width of the digits' stems in Selawik Semibold, as a share of their height (190 of 1434 units).
const STEM: f32 = 0.13;
/// Smallest height of the digits in pixels; below that they are no longer legible.
const MIN_DIGIT_HEIGHT: usize = 5;
/// Coverage below this counts as empty (rounding noise of the rasteriser).
const EMPTY: f32 = 0.5 / 255.0;
/// Coverage above this counts as full.
const FULL: f32 = 1.0 - 0.5 / 255.0;

/// What the badge says for `count` (> 0): the number, or "99+".
pub fn label(count: u32) -> String {
    if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    }
}

/// The side of the taskbar overlay for a display scale: the small-icon size, 16 logical pixels.
pub fn overlay_size(scale: f64) -> u32 {
    scaled(16, scale)
}

/// The side of the notification-area icon for a display scale (also 16 logical pixels).
pub fn tray_size(scale: f64) -> u32 {
    scaled(16, scale)
}

/// The side of the window icon for a display scale: the taskbar's icon size, 24 logical pixels
/// (Windows 10 and 11), so the taskbar shows the icon's own drawing of that size unscaled.
pub fn window_icon_size(scale: f64) -> u32 {
    scaled(24, scale)
}

fn scaled(logical: u32, scale: f64) -> u32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale.clamp(1.0, 4.0)
    } else {
        1.0
    };
    // At most 4× (64 px for 16): `as` cannot overflow here.
    (f64::from(logical) * scale).round() as u32
}

/// The count could not be drawn: the embedded font is unreadable or a glyph is missing. Never
/// happens with the font this crate is built with (tested); callers log it and show no count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadgeError(String);

impl std::fmt::Display for BadgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the unread count could not be drawn: {}", self.0)
    }
}

impl std::error::Error for BadgeError {}

// ---------------------------------------------------------------------------------------------
// Coverage

/// How much of each pixel a shape covers, 0.0 to 1.0, row by row.
#[derive(Debug, Clone, PartialEq)]
struct Mask {
    width: usize,
    height: usize,
    coverage: Vec<f32>,
}

impl Mask {
    /// Coverage at `(x, y)`; nothing outside.
    fn at(&self, x: isize, y: isize) -> f32 {
        match (usize::try_from(x), usize::try_from(y)) {
            (Ok(x), Ok(y)) if x < self.width && y < self.height => {
                self.coverage[y * self.width + x]
            }
            _ => 0.0,
        }
    }

    /// The smallest rectangle `(left, top, width, height)` holding every covered pixel.
    fn ink(&self) -> Option<(usize, usize, usize, usize)> {
        let lit = |x: usize, y: usize| self.coverage[y * self.width + x] > EMPTY;
        let rows: Vec<usize> = (0..self.height)
            .filter(|&y| (0..self.width).any(|x| lit(x, y)))
            .collect();
        let columns: Vec<usize> = (0..self.width)
            .filter(|&x| (0..self.height).any(|y| lit(x, y)))
            .collect();
        let (top, bottom) = (*rows.first()?, *rows.last()?);
        let (left, right) = (*columns.first()?, *columns.last()?);
        Some((left, top, right - left + 1, bottom - top + 1))
    }

    fn crop(&self, left: usize, top: usize, width: usize, height: usize) -> Self {
        let mut coverage = Vec::with_capacity(width * height);
        for y in top..top + height {
            let start = y * self.width + left;
            coverage.extend_from_slice(&self.coverage[start..start + width]);
        }
        Self {
            width,
            height,
            coverage,
        }
    }
}

/// A point in pixel space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Point {
    x: f32,
    y: f32,
}

fn point(x: f32, y: f32) -> Point {
    Point { x, y }
}

/// Sample lines per pixel row of the rasteriser; across each line coverage is exact.
const SUBROWS: usize = 32;

#[derive(Clone, Copy, Debug)]
enum Segment {
    Line(Point, Point),
    Quad(Point, Point, Point),
    Cubic(Point, Point, Point, Point),
}

/// An outline in pixel space, y downwards; also the pen skrifa draws glyphs with (y upwards).
#[derive(Debug, Default)]
struct Path {
    segments: Vec<Segment>,
    start: Option<Point>,
    current: Point,
}

impl Path {
    fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.segments.iter().flat_map(|s| match *s {
            Segment::Line(a, b) => vec![a, b],
            Segment::Quad(a, b, c) => vec![a, b, c],
            Segment::Cubic(a, b, c, d) => vec![a, b, c, d],
        })
    }

    /// The outline as straight edges: curves cut into pieces of at most about half a pixel.
    fn edges(&self) -> Vec<(Point, Point)> {
        fn pieces(control: &[Point]) -> usize {
            let length: f32 = control
                .windows(2)
                .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
                .sum();
            ((length * 2.0).ceil() as usize).clamp(1, 64)
        }
        let mut edges = Vec::new();
        for segment in &self.segments {
            match *segment {
                Segment::Line(a, b) => edges.push((a, b)),
                Segment::Quad(a, b, c) => {
                    let n = pieces(&[a, b, c]);
                    let at = |t: f32| {
                        let u = 1.0 - t;
                        point(
                            u * u * a.x + 2.0 * u * t * b.x + t * t * c.x,
                            u * u * a.y + 2.0 * u * t * b.y + t * t * c.y,
                        )
                    };
                    let mut last = a;
                    for i in 1..=n {
                        let next = if i == n { c } else { at(i as f32 / n as f32) };
                        edges.push((last, next));
                        last = next;
                    }
                }
                Segment::Cubic(a, b, c, d) => {
                    let n = pieces(&[a, b, c, d]);
                    let at = |t: f32| {
                        let u = 1.0 - t;
                        let (w0, w1, w2, w3) =
                            (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        point(
                            w0 * a.x + w1 * b.x + w2 * c.x + w3 * d.x,
                            w0 * a.y + w1 * b.y + w2 * c.y + w3 * d.y,
                        )
                    };
                    let mut last = a;
                    for i in 1..=n {
                        let next = if i == n { d } else { at(i as f32 / n as f32) };
                        edges.push((last, next));
                        last = next;
                    }
                }
            }
        }
        edges
    }

    /// The coverage of the closed outline on a `width`×`height` grid (non-zero winding):
    /// [`SUBROWS`] sample lines per pixel row, and on each line the spans inside the outline
    /// added to every pixel with the exact share of it they cross. Edges on whole pixels (the
    /// hinted glyphs, the shapes' straight sides) give exactly full or empty pixels.
    fn rasterise(&self, width: usize, height: usize) -> Mask {
        let edges = self.edges();
        let mut coverage = vec![0.0f32; width * height];
        let weight = 1.0 / SUBROWS as f32;
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        for row in 0..height {
            let cells = &mut coverage[row * width..(row + 1) * width];
            for sub in 0..SUBROWS {
                let y = row as f32 + (sub as f32 + 0.5) * weight;
                crossings.clear();
                for &(a, b) in &edges {
                    // Half-open in y, so a vertex shared by two edges counts once.
                    let (upward, low, high) = if a.y < b.y { (1, a, b) } else { (-1, b, a) };
                    if low.y <= y && y < high.y {
                        let t = (y - low.y) / (high.y - low.y);
                        crossings.push((low.x + t * (high.x - low.x), upward));
                    }
                }
                crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
                let mut winding = 0;
                for pair in crossings.windows(2) {
                    winding += pair[0].1;
                    if winding != 0 {
                        add_span(cells, pair[0].0, pair[1].0, weight);
                    }
                }
            }
        }
        for c in &mut coverage {
            *c = c.clamp(0.0, 1.0);
        }
        Mask {
            width,
            height,
            coverage,
        }
    }

    fn shifted(&self, dx: f32, dy: f32) -> Self {
        let at = |p: Point| point(p.x + dx, p.y + dy);
        Self {
            segments: self
                .segments
                .iter()
                .map(|s| match *s {
                    Segment::Line(a, b) => Segment::Line(at(a), at(b)),
                    Segment::Quad(a, b, c) => Segment::Quad(at(a), at(b), at(c)),
                    Segment::Cubic(a, b, c, d) => Segment::Cubic(at(a), at(b), at(c), at(d)),
                })
                .collect(),
            start: None,
            current: Point::default(),
        }
    }
}

/// Adds the span `x0..x1` of one sample line to the pixels of a row, each with the share of it
/// the span covers, times `weight`.
fn add_span(cells: &mut [f32], x0: f32, x1: f32, weight: f32) {
    let x0 = x0.max(0.0);
    let x1 = x1.min(cells.len() as f32);
    if x1 <= x0 {
        return;
    }
    let first = x0.floor() as usize;
    let last = (x1.ceil() as usize).min(cells.len());
    for (i, cell) in cells.iter_mut().enumerate().take(last).skip(first) {
        let left = (i as f32).max(x0);
        let right = ((i + 1) as f32).min(x1);
        if right > left {
            *cell += (right - left) * weight;
        }
    }
}

impl OutlinePen for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let p = point(x, -y);
        self.start = Some(p);
        self.current = p;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = point(x, -y);
        self.segments.push(Segment::Line(self.current, p));
        self.current = p;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let p = point(x, -y);
        self.segments
            .push(Segment::Quad(self.current, point(cx0, -cy0), p));
        self.current = p;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p = point(x, -y);
        self.segments.push(Segment::Cubic(
            self.current,
            point(cx0, -cy0),
            point(cx1, -cy1),
            p,
        ));
        self.current = p;
    }

    fn close(&mut self) {
        if let Some(start) = self.start.take() {
            if start != self.current {
                self.segments.push(Segment::Line(self.current, start));
            }
            self.current = start;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Text

/// One glyph's coverage, trimmed to its ink; `top` is the row of its first line relative to the
/// baseline (negative: above it).
#[derive(Debug, Clone)]
struct Glyph {
    mask: Mask,
    top: isize,
}

/// The font at one size, hinted for it.
struct Face<'a> {
    font: FontRef<'a>,
    outlines: OutlineGlyphCollection<'a>,
    hinting: HintingInstance,
}

impl Face<'static> {
    fn new(ppem: f32) -> Result<Self, BadgeError> {
        let font = FontRef::new(FONT).map_err(|e| BadgeError(format!("font: {e}")))?;
        let outlines = font.outline_glyphs();
        // The auto-hinter (Selawik has no instructions of its own) in its strongest mode, the
        // one FreeType uses for black-and-white text (FT_LOAD_TARGET_MONO): the alignment zones,
        // the stems' edges and the stems' widths in both directions on whole pixels. Drawn with
        // anti-aliasing, the stems stay solid white and only curves and diagonals get soft
        // pixels; the gentler modes leave stems 1.4 or 1.6 px wide, a grey column beside each.
        let hinting = HintingInstance::new(
            &outlines,
            Size::new(ppem),
            LocationRef::default(),
            HintingOptions {
                engine: Engine::Auto(None),
                target: Target::Mono,
            },
        )
        .map_err(|e| BadgeError(format!("hinting at {ppem} ppem: {e}")))?;
        Ok(Self {
            font,
            outlines,
            hinting,
        })
    }

    fn glyph(&self, c: char) -> Result<Glyph, BadgeError> {
        let missing = || BadgeError(format!("no glyph for {c:?}"));
        let id = self.font.charmap().map(c).ok_or_else(missing)?;
        let outline = self.outlines.get(id).ok_or_else(missing)?;
        let mut path = Path::default();
        outline
            .draw(DrawSettings::hinted(&self.hinting, false), &mut path)
            .map_err(|e| BadgeError(format!("{c:?}: {e}")))?;
        path.close();
        // The pen's origin is a whole pixel; keep it so (the hinting depends on it).
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in path.points() {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        if x0 > x1 {
            return Err(missing());
        }
        let (left, top) = (x0.floor(), y0.floor());
        let width = (x1.ceil() - left) as usize;
        let height = (y1.ceil() - top) as usize;
        let mask = path.shifted(-left, -top).rasterise(width, height);
        let (ink_left, ink_top, ink_width, ink_height) = mask.ink().ok_or_else(missing)?;
        Ok(Glyph {
            mask: mask.crop(ink_left, ink_top, ink_width, ink_height),
            top: top as isize + ink_top as isize,
        })
    }
}

/// The glyphs of the count with digits `height` pixels high, each hinted for that size.
struct Glyphs {
    height: usize,
    /// Pixels between the ink of two glyphs.
    gap: usize,
    glyphs: Vec<(char, Glyph)>,
}

impl Glyphs {
    fn new(height: usize) -> Result<Self, BadgeError> {
        let ppem = height as f32 * UNITS_PER_EM / FIGURE_HEIGHT;
        let face = Face::new(ppem)?;
        let mut glyphs = Vec::with_capacity(11);
        for c in '0'..='9' {
            glyphs.push((c, face.glyph(c)?));
        }
        glyphs.push(('+', plus(height)));
        Ok(Self {
            height,
            // Ink to ink, as tight as the digits of a badge sit: one pixel up to 16 px high.
            gap: ((height as f32 * 0.07).round() as usize).max(1),
            glyphs,
        })
    }

    fn get(&self, c: char) -> Result<&Glyph, BadgeError> {
        self.glyphs
            .iter()
            .find(|(g, _)| *g == c)
            .map(|(_, glyph)| glyph)
            .ok_or_else(|| BadgeError(format!("no glyph for {c:?}")))
    }
}

/// The plus of "99+" for digits `height` pixels high: two bars as thick as a digit's stem, about
/// 60 % of the digits' height long, centred on it (an odd pixel left over goes below). Drawn
/// rather than taken from the font: the font's plus is as wide as a digit and a half, and at
/// these sizes the hinter makes it 5 px wide where the digits are 4.
fn plus(height: usize) -> Glyph {
    let stem = ((height as f32 * STEM).round() as usize).max(1);
    let target = height as f32 * 0.62;
    // Arms of equal length on both sides of the bar: length and thickness of the same parity.
    let length = [target.floor() as usize, target.ceil() as usize]
        .into_iter()
        .filter(|l| (l + stem).is_multiple_of(2))
        .min_by(|a, b| {
            (*a as f32 - target)
                .abs()
                .total_cmp(&(*b as f32 - target).abs())
        })
        .unwrap_or(stem)
        .max(stem);
    let arm = (length - stem) / 2;
    let coverage = (0..length * length)
        .map(|i| {
            let (x, y) = (i % length, i / length);
            let across = (arm..arm + stem).contains(&y);
            let down = (arm..arm + stem).contains(&x);
            if across || down { 1.0 } else { 0.0 }
        })
        .collect();
    let h = height as isize;
    Glyph {
        mask: Mask {
            width: length,
            height: length,
            coverage,
        },
        top: -h + (h - length as isize).div_euclid(2),
    }
}

/// Every label with as many glyphs as `text`: all of them are drawn at the same size, so the
/// digits do not change size from one count to the next.
fn siblings(text: &str) -> Vec<String> {
    match text.chars().count() {
        1 => (1..=9).map(|n: u32| n.to_string()).collect(),
        2 => (10..=99).map(|n: u32| n.to_string()).collect(),
        _ => vec![label(100)],
    }
}

/// `text` set in `glyphs`: its coverage, trimmed to the ink, and where the digits stand in it
/// (`top`: the first row of the digits' height, which round digits overshoot).
#[derive(Debug, Clone)]
struct Line {
    mask: Mask,
    top: isize,
    height: usize,
}

impl Line {
    fn new(text: &str, glyphs: &Glyphs) -> Result<Self, BadgeError> {
        let set = text
            .chars()
            .map(|c| glyphs.get(c))
            .collect::<Result<Vec<_>, _>>()?;
        let digits_top = -(glyphs.height as isize);
        let top = set
            .iter()
            .map(|g| g.top)
            .min()
            .unwrap_or(digits_top)
            .min(digits_top);
        let bottom = set
            .iter()
            .map(|g| g.top + g.mask.height as isize)
            .max()
            .unwrap_or(0)
            .max(0);
        let width = set.iter().map(|g| g.mask.width).sum::<usize>()
            + glyphs.gap * set.len().saturating_sub(1);
        let rows = (bottom - top) as usize;
        let mut mask = Mask {
            width,
            height: rows,
            coverage: vec![0.0; width * rows],
        };
        let mut x = 0;
        for glyph in set {
            let y0 = (glyph.top - top) as usize;
            for gy in 0..glyph.mask.height {
                for gx in 0..glyph.mask.width {
                    let c = glyph.mask.coverage[gy * glyph.mask.width + gx];
                    let i = (y0 + gy) * width + x + gx;
                    mask.coverage[i] = (mask.coverage[i] + c).min(1.0);
                }
            }
            x += glyph.mask.width + glyphs.gap;
        }
        Ok(Self {
            mask,
            top: digits_top - top,
            height: glyphs.height,
        })
    }

    /// Where the line goes (its mask's top-left corner) to sit centred in the `width`×`height`
    /// box at `(x0, y0)`: the ink centred across, the digits' height centred up and down (an odd
    /// pixel left over goes below). Whole pixels only.
    fn centred_in(&self, x0: usize, y0: usize, width: usize, height: usize) -> (isize, isize) {
        let left = x0 as isize + (width as isize - self.mask.width as isize).div_euclid(2);
        let digits_top = y0 as isize + (height as isize - self.height as isize).div_euclid(2);
        (left, digits_top - self.top)
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// An RGBA image, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    fn transparent(width: usize, height: usize) -> Self {
        Self {
            width: u32::try_from(width).unwrap_or(0),
            height: u32::try_from(height).unwrap_or(0),
            pixels: vec![0; width * height * 4],
        }
    }

    fn w(&self) -> usize {
        self.width as usize
    }

    fn h(&self) -> usize {
        self.height as usize
    }

    fn put(&mut self, x: usize, y: usize, colour: [u8; 4]) {
        let i = (y * self.w() + x) * 4;
        if let Some(px) = self.pixels.get_mut(i..i + 4) {
            px.copy_from_slice(&colour);
        }
    }

    /// Pixel at `(x, y)`, transparent outside.
    pub fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.w() + x) * 4;
        match self.pixels.get(i..i + 4) {
            Some(px) if x < self.w() => [px[0], px[1], px[2], px[3]],
            _ => [0; 4],
        }
    }

    /// Paints `colour` over the pixel with opacity `alpha` (0.0 to 1.0), "source over".
    fn blend(&mut self, x: usize, y: usize, colour: [u8; 3], alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let under = self.at(x, y);
        let a = alpha.min(1.0);
        let ua = f32::from(under[3]) / 255.0 * (1.0 - a);
        let out = a + ua;
        let mix = |c: u8, u: u8| -> u8 {
            ((f32::from(c) * a + f32::from(u) * ua) / out)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        self.put(
            x,
            y,
            [
                mix(colour[0], under[0]),
                mix(colour[1], under[1]),
                mix(colour[2], under[2]),
                (out * 255.0).round().clamp(0.0, 255.0) as u8,
            ],
        );
    }

    /// Makes the pixel transparent by `coverage` (the cut-out around the pill).
    fn erase(&mut self, x: usize, y: usize, coverage: f32) {
        let mut px = self.at(x, y);
        px[3] = (f32::from(px[3]) * (1.0 - coverage.clamp(0.0, 1.0))).round() as u8;
        self.put(x, y, px);
    }
}

/// sRGB channel to linear light and back.
fn linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb(l: f32) -> u8 {
    let c = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// White over red with `coverage`, mixed in linear light.
fn ink_on_red(coverage: f32) -> [u8; 3] {
    let t = coverage.clamp(0.0, 1.0);
    let mix = |i: usize| srgb(linear(RED[i]) * (1.0 - t) + linear(WHITE[i]) * t);
    [mix(0), mix(1), mix(2)]
}

/// A rounded rectangle (`radius` up to half the height: a pill or a disc) in pixel space.
#[derive(Clone, Copy, Debug)]
struct Shape {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
}

impl Shape {
    fn grown(self, by: f32) -> Self {
        Self {
            x0: self.x0 - by,
            y0: self.y0 - by,
            x1: self.x1 + by,
            y1: self.y1 + by,
            radius: self.radius + by,
        }
    }

    /// The outline: straight sides and quarter circles (cubic Béziers, 0.03 % off a circle).
    fn path(self) -> Path {
        const K: f32 = 0.552_284_8;
        let Self {
            x0,
            y0,
            x1,
            y1,
            radius,
        } = self;
        let r = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let k = K * r;
        let mut path = Path::default();
        // `OutlinePen` takes y upwards; this shape is in pixel space, so negate on the way in.
        path.move_to(x0 + r, -y0);
        path.line_to(x1 - r, -y0);
        path.curve_to(x1 - r + k, -y0, x1, -(y0 + r - k), x1, -(y0 + r));
        path.line_to(x1, -(y1 - r));
        path.curve_to(x1, -(y1 - r + k), x1 - r + k, -y1, x1 - r, -y1);
        path.line_to(x0 + r, -y1);
        path.curve_to(x0 + r - k, -y1, x0, -(y1 - r + k), x0, -(y1 - r));
        path.line_to(x0, -(y0 + r));
        path.curve_to(x0, -(y0 + r - k), x0 + r - k, -y0, x0 + r, -y0);
        path.close();
        path
    }

    fn mask(self, width: usize, height: usize) -> Mask {
        self.path().rasterise(width, height)
    }
}

/// Whether `line` at `(left, top)` stands on solid ground in `shape`: every pixel it touches
/// and its four neighbours lie wholly inside, so no digit runs into the soft edge and a rim of
/// red stays around the digits.
fn on_solid_ground(shape: &Mask, line: &Line, left: isize, top: isize) -> bool {
    (0..line.mask.height as isize).all(|y| {
        (0..line.mask.width as isize).all(|x| {
            let (sx, sy) = (left + x, top + y);
            line.mask.at(x, y) <= EMPTY
                || [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)]
                    .into_iter()
                    .all(|(dx, dy)| shape.at(sx + dx, sy + dy) >= FULL)
        })
    })
}

/// Paints `shape` in red with `line` in white on it at `(left, top)`.
fn paint(image: &mut Rgba, shape: &Mask, line: &Line, left: isize, top: isize) {
    for y in 0..image.h() {
        for x in 0..image.w() {
            let (xi, yi) = (x as isize, y as isize);
            let s = shape.at(xi, yi);
            if s <= 0.0 {
                continue;
            }
            let t = line.mask.at(xi - left, yi - top);
            image.blend(x, y, ink_on_red(t), s);
        }
    }
}

/// The digit heights to try for a badge whose text area is `limit` pixels high, largest first.
fn heights(limit: usize) -> impl Iterator<Item = usize> {
    (MIN_DIGIT_HEIGHT..=limit.max(MIN_DIGIT_HEIGHT)).rev()
}

/// The taskbar overlay: the count in white on red, `size`×`size` pixels. A disc for one digit;
/// for more, a rounded rectangle as wide as the image when it takes larger digits than the disc.
pub fn overlay(count: u32, size: u32) -> Result<Rgba, BadgeError> {
    let n = size.max(8) as usize;
    let text = label(count.max(1));
    let side = n as f32;
    let disc = Shape {
        x0: 0.0,
        y0: 0.0,
        x1: side,
        y1: side,
        radius: side / 2.0,
    };
    // A rounded rectangle as wide as the image, three quarters as high, for longer text; with
    // tighter corners where the round ones leave too little room ("99+" at 16 px).
    let wide_h = (side * 0.75).round().max(8.0);
    let wide_top = ((side - wide_h) / 2.0).floor();
    let wide = |round: f32| Shape {
        x0: 0.0,
        y0: wide_top,
        x1: side,
        y1: wide_top + wide_h,
        radius: (wide_h * round).max(2.0),
    };
    let shapes = if text.chars().count() == 1 {
        vec![disc]
    } else {
        vec![disc, wide(0.3), wide(0.15)]
    };
    // For each shape, the largest digits with which every label of this length stands on solid
    // ground, at most 60 % of the side high (so the shape keeps a visible rim).
    let labels = siblings(&text);
    let limit = (side * 0.6).floor() as usize;
    let fit = |shape: Shape| -> Result<Option<(Mask, Glyphs)>, BadgeError> {
        let mask = shape.mask(n, n);
        for height in heights(limit) {
            let glyphs = Glyphs::new(height)?;
            let mut all = true;
            for other in &labels {
                let line = Line::new(other, &glyphs)?;
                let (left, top) = line.centred_in(0, 0, n, n);
                all = on_solid_ground(&mask, &line, left, top);
                if !all {
                    break;
                }
            }
            if all {
                return Ok(Some((mask, glyphs)));
            }
        }
        Ok(None)
    };
    // The shape with the largest digits; on a tie the earlier (rounder) one.
    let mut best: Option<(Mask, Glyphs)> = None;
    for shape in shapes {
        if let Some(found) = fit(shape)?
            && best
                .as_ref()
                .is_none_or(|(_, glyphs)| found.1.height > glyphs.height)
        {
            best = Some(found);
        }
    }
    let (mask, glyphs) = match best {
        Some(best) => best,
        // Only below 16 px, which Windows never asks for: the smallest digits on the tightest
        // shape, even if they touch its edge.
        None => (wide(0.15).mask(n, n), Glyphs::new(MIN_DIGIT_HEIGHT)?),
    };
    let line = Line::new(&text, &glyphs)?;
    let (left, top) = line.centred_in(0, 0, n, n);
    let mut image = Rgba::transparent(n, n);
    paint(&mut image, &mask, &line, left, top);
    Ok(image)
}

/// The notification-area icon: `icon` (square RGBA) with the count in a red pill at its bottom
/// right, cut out of the icon by one pixel so it stands apart. `count` 0 returns the icon as is.
pub fn on_icon(icon: &Rgba, count: u32) -> Result<Rgba, BadgeError> {
    let mut image = icon.clone();
    if count == 0 || icon.width == 0 || icon.width != icon.height {
        return Ok(image);
    }
    let n = image.w();
    let text = label(count);
    let labels = siblings(&text);
    // The badge is 5/8 of the icon high: a circle for one digit, a pill for more (at most as
    // wide as the icon), as wide as the widest label of the same length needs, with the largest
    // digits with which all of them stand on solid ground. Where a pill's round ends leave too
    // little room (three glyphs at 16 px), its corners get tighter; larger digits win over
    // rounder corners.
    let pill_h = (n * 5).div_ceil(8).max(7);
    let pad = (pill_h / 8).max(1);
    let roundness: &[f32] = if text.chars().count() == 1 {
        &[0.5]
    } else {
        &[0.5, 0.3, 0.15]
    };
    for height in heights(pill_h - 2) {
        let glyphs = Glyphs::new(height)?;
        let lines = labels
            .iter()
            .map(|other| Line::new(other, &glyphs))
            .collect::<Result<Vec<_>, _>>()?;
        let widest = lines.iter().map(|l| l.mask.width).max().unwrap_or(0);
        for &round in roundness {
            for side in pad..=pad + 2 {
                let width = (widest + 2 * side).max(pill_h);
                if width > n {
                    break;
                }
                let (x0, y0) = (n - width, n - pill_h);
                let shape = Shape {
                    x0: x0 as f32,
                    y0: y0 as f32,
                    x1: n as f32,
                    y1: n as f32,
                    radius: pill_h as f32 * round,
                };
                let mask = shape.mask(n, n);
                let stands = |line: &Line| {
                    let (left, top) = line.centred_in(x0, y0, width, pill_h);
                    on_solid_ground(&mask, line, left, top)
                };
                if lines.iter().all(stands) {
                    let cut = shape.grown(1.0).mask(n, n);
                    for y in 0..n {
                        for x in 0..n {
                            image.erase(x, y, cut.at(x as isize, y as isize));
                        }
                    }
                    let line = Line::new(&text, &glyphs)?;
                    let (left, top) = line.centred_in(x0, y0, width, pill_h);
                    paint(&mut image, &mask, &line, left, top);
                    return Ok(image);
                }
            }
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
    const SIZES: [u32; 6] = [16, 20, 24, 28, 32, 40];
    const COUNTS: [u32; 12] = [1, 4, 7, 9, 10, 11, 42, 77, 88, 99, 100, 12_345];

    /// WCAG contrast of two sRGB colours.
    fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
        let luminance = |c: [u8; 3]| {
            let l = |v: u8| f64::from(linear(v));
            0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
        };
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// How white a pixel of the badge is: 0 red, 1 white (by its green channel, which red
    /// lacks, in linear light as it was mixed).
    fn whiteness(px: [u8; 4]) -> f32 {
        (linear(px[1]) - linear(RED[1])) / (1.0 - linear(RED[1]))
    }

    /// Whether `px` lies on the way from red to white (allowing rounding).
    fn red_to_white(px: [u8; 4]) -> bool {
        let t = whiteness(px).clamp(0.0, 1.0);
        let expected = ink_on_red(t);
        (0..3).all(|i| px[i].abs_diff(expected[i]) <= 3) && px[0] >= RED[0] - 1
    }

    fn lit(image: &Rgba, threshold: f32) -> Vec<(usize, usize)> {
        (0..image.h())
            .flat_map(|y| (0..image.w()).map(move |x| (x, y)))
            .filter(|&(x, y)| image.at(x, y)[3] == 255 && whiteness(image.at(x, y)) > threshold)
            .collect()
    }

    #[test]
    fn sizes_follow_the_display_scale() {
        assert_eq!(
            [1.0, 1.25, 1.5, 1.75, 2.0, 2.5].map(overlay_size),
            [16, 20, 24, 28, 32, 40]
        );
        assert_eq!(SCALES.map(tray_size), [16, 20, 24, 32]);
        assert_eq!(
            [1.0, 1.25, 1.5, 2.0, 2.5].map(window_icon_size),
            [24, 30, 36, 48, 60]
        );
        // Nonsense from the system falls back to 100 %, more than 400 % is capped.
        assert_eq!(overlay_size(f64::NAN), 16);
        assert_eq!(overlay_size(0.0), 16);
        assert_eq!(overlay_size(9.0), 64);
    }

    #[test]
    fn more_than_99_reads_99_plus() {
        assert_eq!(label(1), "1");
        assert_eq!(label(99), "99");
        assert_eq!(label(100), "99+");
        assert_eq!(
            overlay(100, 16).expect("drawn"),
            overlay(5_000, 16).expect("drawn")
        );
    }

    #[test]
    fn the_font_has_every_glyph_of_the_count() {
        let face = Face::new(20.0).expect("the embedded font loads");
        for c in "0123456789".chars() {
            let glyph = face.glyph(c).expect("glyph");
            assert!(glyph.mask.width > 0 && glyph.mask.height > 0, "{c:?}");
        }
        assert!(face.glyph('x').is_err(), "not in the subset");
        assert!(face.glyph('+').is_err(), "drawn, not from the font");
    }

    /// The auto-hinter puts the digits' baseline and top on whole pixels: a "7" (flat top) and a
    /// "2" (flat foot) at `h` pixels span exactly `h` rows, fully covered at both ends.
    #[test]
    fn digits_stand_on_the_pixel_grid() {
        for height in MIN_DIGIT_HEIGHT..=20 {
            let line = Line::new("72", &Glyphs::new(height).expect("glyphs")).expect("line");
            let rows = line.mask.height;
            let top = usize::try_from(line.top).expect("top");
            assert!(top + height <= rows, "{height}: {top} + {height} > {rows}");
            let row_max = |y: usize| {
                (0..line.mask.width)
                    .map(|x| line.mask.coverage[y * line.mask.width + x])
                    .fold(0.0f32, f32::max)
            };
            // The top bar of the 7 and the foot of the 2: a full row each.
            assert!(row_max(top) > 0.95, "{height}: top row {}", row_max(top));
            assert!(
                row_max(top + height - 1) > 0.95,
                "{height}: bottom row {}",
                row_max(top + height - 1)
            );
            // Nothing of them above or below the digits' height.
            for y in (0..top).chain(top + height..rows) {
                assert!(row_max(y) < 0.3, "{height}: row {y} {}", row_max(y));
            }
        }
    }

    #[test]
    fn overlay_has_the_size_asked_for_with_digits_on_solid_red() {
        for size in SIZES {
            for count in COUNTS {
                let image = overlay(count, size).expect("drawn");
                assert_eq!((image.width, image.height), (size, size));
                assert_eq!(image.pixels.len(), (size * size * 4) as usize);
                let n = size as usize;
                // The centre is opaque: a disc or a wide pill, never a ring.
                assert_eq!(image.at(n / 2, n / 2)[3], 255, "{size}px {count}");
                for y in 0..n {
                    for x in 0..n {
                        let px = image.at(x, y);
                        match px[3] {
                            // No stray pixels: every opaque pixel is red, white or between.
                            255 => assert!(red_to_white(px), "{size}px {count}: ({x},{y}) {px:?}"),
                            0 => {}
                            // The soft edge is the red itself, never digits.
                            _ => assert_eq!(&px[..3], &RED[..3], "{size}px {count}: ({x},{y})"),
                        }
                    }
                }
                // Digits inside the disc: every pixel with white is opaque, and so are the four
                // neighbours of every pixel that is more white than red.
                for (x, y) in lit(&image, 0.5) {
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let around = x
                            .checked_add_signed(dx)
                            .zip(y.checked_add_signed(dy))
                            .map(|(nx, ny)| image.at(nx, ny)[3]);
                        assert_eq!(around, Some(255), "{size}px {count}: ({x},{y}) at the edge");
                    }
                }
            }
        }
    }

    #[test]
    fn digits_are_legible_crisp_and_centred() {
        assert!(contrast([RED[0], RED[1], RED[2]], [255; 3]) >= 4.5);
        for size in SIZES {
            for count in COUNTS {
                let image = overlay(count, size).expect("drawn");
                let n = size as usize;
                let any = lit(&image, 0.01);
                let solid = lit(&image, 0.9);
                // Crisp, not grey mush: most of the white is in (nearly) full white pixels, which
                // have the contrast of white on the red; soft pixels only where curves cross.
                let ink = |pixels: &[(usize, usize)]| -> f32 {
                    pixels.iter().map(|&(x, y)| whiteness(image.at(x, y))).sum()
                };
                assert!(
                    ink(&solid) >= ink(&any) * 0.4,
                    "{size}px {count}: {} of {} white in full pixels",
                    ink(&solid),
                    ink(&any)
                );
                assert!(solid.len() >= 6, "{size}px {count}: digits drawn");
                // Centred: the ink's margins differ by at most one pixel left/right and two
                // up/down (round digits overshoot the grid by up to a pixel).
                let (x0, x1) = (
                    any.iter().map(|p| p.0).min().expect("ink"),
                    any.iter().map(|p| p.0).max().expect("ink"),
                );
                let (y0, y1) = (
                    any.iter().map(|p| p.1).min().expect("ink"),
                    any.iter().map(|p| p.1).max().expect("ink"),
                );
                assert!(x0.abs_diff(n - 1 - x1) <= 1, "{size}px {count}: {x0}..{x1}");
                assert!(y0.abs_diff(n - 1 - y1) <= 2, "{size}px {count}: {y0}..{y1}");
                // Glyphs never touch: as many runs of lit columns as glyphs.
                let columns: Vec<bool> = (x0..=x1)
                    .map(|x| (0..n).any(|y| whiteness(image.at(x, y)) > 0.25))
                    .collect();
                let runs = columns
                    .iter()
                    .zip(std::iter::once(&false).chain(columns.iter()))
                    .filter(|(now, before)| **now && !**before)
                    .count();
                let glyphs = label(count).chars().count();
                assert!(
                    runs >= glyphs,
                    "{size}px {count}: {runs} runs for {glyphs} glyphs"
                );
            }
        }
    }

    #[test]
    fn larger_screens_get_larger_digits() {
        let height = |count, size| {
            let image = overlay(count, size).expect("drawn");
            let n = size as usize;
            (0..n)
                .filter(|&y| (0..n).any(|x| whiteness(image.at(x, y)) > 0.5))
                .count()
        };
        for count in [7, 42, 100] {
            let heights: Vec<usize> = [16, 20, 24, 32].iter().map(|&s| height(count, s)).collect();
            assert!(
                heights.windows(2).all(|w| w[0] <= w[1]),
                "{count}: {heights:?}"
            );
            assert!(heights[3] > heights[0], "{count}: {heights:?}");
        }
        // One digit at 100 % is at least 8 px high, two at least 7, "99+" at least 5.
        assert!(height(7, 16) >= 8, "{}", height(7, 16));
        assert!(height(42, 16) >= 7, "{}", height(42, 16));
        assert!(height(100, 16) >= 5, "{}", height(100, 16));
    }

    #[test]
    fn the_tray_badge_sits_at_the_bottom_right_and_keeps_the_icon() {
        for size in [16usize, 20, 24, 32] {
            // A plain grey icon.
            let icon = Rgba {
                width: size as u32,
                height: size as u32,
                pixels: [0x40, 0x40, 0x40, 0xFF].repeat(size * size),
            };
            assert_eq!(
                on_icon(&icon, 0).expect("drawn"),
                icon,
                "no count, no badge"
            );
            for count in COUNTS {
                let image = on_icon(&icon, count).expect("drawn");
                assert_eq!((image.width, image.height), (icon.width, icon.height));
                // The top-left third is the icon, untouched.
                for y in 0..size / 3 {
                    for x in 0..size / 3 {
                        assert_eq!(image.at(x, y), icon.at(x, y), "{size}px {count}");
                    }
                }
                // The badge is red in the bottom-right quarter, the digits white on it.
                let red = (size / 2..size)
                    .flat_map(|y| (size / 2..size).map(move |x| (x, y)))
                    .filter(|&(x, y)| image.at(x, y) == RED)
                    .count();
                assert!(red >= size, "{size}px {count}: {red} red pixels");
                let white: Vec<(usize, usize)> = (0..size)
                    .flat_map(|y| (0..size).map(move |x| (x, y)))
                    .filter(|&(x, y)| {
                        let px = image.at(x, y);
                        red_to_white(px) && whiteness(px) > 0.9
                    })
                    .collect();
                assert!(!white.is_empty(), "{size}px {count}: digits drawn");
                // Digits in the lower part, on the badge's red, never on the icon: each lit
                // pixel's neighbours are red, white or between.
                let on_badge = |px: [u8; 4]| px[3] == 255 && red_to_white(px);
                for (x, y) in (0..size)
                    .flat_map(|y| (0..size).map(move |x| (x, y)))
                    .filter(|&(x, y)| on_badge(image.at(x, y)) && whiteness(image.at(x, y)) > 0.01)
                {
                    assert!(y >= size / 3, "{size}px {count}: ({x},{y}) too high");
                    for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                        assert!(
                            red_to_white(image.at(nx, ny)),
                            "{size}px {count}: ({x},{y}) not on the badge"
                        );
                    }
                }
            }
        }
    }

    /// Writes the overlays and tray badges as raw RGBA (`overlay-<count>-<size>.rgba`,
    /// `tray-<count>-<size>.rgba`) to `BADGE_PREVIEW_DIR`, for looking at them (not part of the
    /// suite). Tray icons need the app icon's own entries there (`icon-<size>.rgba`).
    #[test]
    #[ignore = "writes preview files"]
    fn write_previews() {
        let Some(dir) = std::env::var_os("BADGE_PREVIEW_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let counts = (1..=100).chain([12_345]);
        for size in SCALES.map(overlay_size) {
            for count in counts.clone() {
                let image = overlay(count, size).expect("drawn");
                std::fs::write(
                    dir.join(format!("overlay-{count}-{size}.rgba")),
                    &image.pixels,
                )
                .expect("write preview");
            }
            let Ok(pixels) = std::fs::read(dir.join(format!("icon-{size}.rgba"))) else {
                continue;
            };
            let icon = Rgba {
                width: size,
                height: size,
                pixels,
            };
            for count in counts.clone() {
                let image = on_icon(&icon, count).expect("drawn");
                std::fs::write(dir.join(format!("tray-{count}-{size}.rgba")), &image.pixels)
                    .expect("write preview");
            }
        }
    }
}
