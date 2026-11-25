//! Stage 21 (decision 0068): the unread count as pixels, drawn for the exact size the system shows
//! it at, so the digits stay sharp.
//!
//! - **Taskbar overlay** (Windows): Windows shows the overlay at the small-icon size, 16 logical
//!   pixels (16 px at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 %). An image of another size is
//!   resampled and its digits blur, so [`overlay`] draws a red disc of exactly that size.
//! - **Notification-area icon** (Windows): the app icon of the small-icon size with the count in
//!   a pill at its bottom right ([`on_icon`]).
//!
//! The digits come from two bitmap fonts (3×5 and 5×7 cells) scaled by whole numbers only, so
//! every digit pixel is fully white on a fully opaque background: no grey edges. For each image
//! the largest font and scale that fit are chosen; more than 99 is "99+". Only the disc and pill
//! edges are anti-aliased (4×4 samples per pixel). No image files and no font library.

/// Red of the badge, as before (stage 9); white digits on it have 4.6:1.
const RED: [u8; 4] = [0xD9, 0x30, 0x25, 0xFF];
const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];

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
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let side = (f64::from(logical) * scale).round() as u32;
    side
}

// ---------------------------------------------------------------------------------------------
// Fonts

/// A bitmap font: `rows` lines of `width` bits per glyph, the most significant bit on the left.
struct Font {
    width: usize,
    rows: usize,
    glyph: fn(char) -> Option<&'static [u8]>,
}

const SMALL: Font = Font {
    width: 3,
    rows: 5,
    glyph: small_glyph,
};

const LARGE: Font = Font {
    width: 5,
    rows: 7,
    glyph: large_glyph,
};

fn small_glyph(c: char) -> Option<&'static [u8]> {
    Some(match c {
        '0' => &[0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => &[0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => &[0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => &[0b111, 0b001, 0b011, 0b001, 0b111],
        '4' => &[0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => &[0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => &[0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => &[0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => &[0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => &[0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => &[0b000, 0b010, 0b111, 0b010, 0b000],
        _ => return None,
    })
}

fn large_glyph(c: char) -> Option<&'static [u8]> {
    Some(match c {
        '0' => &[
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => &[
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => &[
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => &[
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => &[
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => &[
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => &[
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => &[
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => &[
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => &[
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '+' => &[
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        _ => return None,
    })
}

/// A font at a whole-number scale; glyphs are one cell (`scale` pixels) apart.
#[derive(Clone, Copy)]
struct Face {
    font: &'static Font,
    scale: usize,
}

impl Face {
    /// Largest first: the 5×7 font at 3× and 2×, the 3×5 font at 2×, then both at 1×.
    const ALL: [Face; 5] = [
        Face {
            font: &LARGE,
            scale: 3,
        },
        Face {
            font: &LARGE,
            scale: 2,
        },
        Face {
            font: &SMALL,
            scale: 2,
        },
        Face {
            font: &LARGE,
            scale: 1,
        },
        Face {
            font: &SMALL,
            scale: 1,
        },
    ];

    fn height(self) -> usize {
        self.font.rows * self.scale
    }

    fn width_of(self, text: &str) -> usize {
        let glyphs = text.chars().count();
        if glyphs == 0 {
            return 0;
        }
        glyphs * self.font.width * self.scale + (glyphs - 1) * self.scale
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

    /// Paints `colour` over the pixel with `coverage` (0..=16 of 16 samples), "source over".
    fn blend(&mut self, x: usize, y: usize, colour: [u8; 4], coverage: u32) {
        if coverage == 0 {
            return;
        }
        let under = self.at(x, y);
        let a = coverage * u32::from(colour[3]) / 16;
        let ua = u32::from(under[3]) * (255 - a) / 255;
        let out_a = a + ua;
        if out_a == 0 {
            return;
        }
        let mix = |c: u8, u: u8| -> u8 {
            u8::try_from((u32::from(c) * a + u32::from(u) * ua) / out_a).unwrap_or(u8::MAX)
        };
        self.put(
            x,
            y,
            [
                mix(colour[0], under[0]),
                mix(colour[1], under[1]),
                mix(colour[2], under[2]),
                u8::try_from(out_a).unwrap_or(u8::MAX),
            ],
        );
    }

    /// Makes the pixel transparent by `coverage` of 16 (the cut-out around the pill).
    fn erase(&mut self, x: usize, y: usize, coverage: u32) {
        let mut px = self.at(x, y);
        let keep = 16 - coverage.min(16);
        px[3] = u8::try_from(u32::from(px[3]) * keep / 16).unwrap_or(0);
        self.put(x, y, px);
    }
}

/// A rounded rectangle (`radius` up to half the height: a pill or a disc) in pixel space.
#[derive(Clone, Copy, Debug)]
struct Shape {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    radius: f64,
}

impl Shape {
    fn grown(self, by: f64) -> Self {
        Self {
            x0: self.x0 - by,
            y0: self.y0 - by,
            x1: self.x1 + by,
            y1: self.y1 + by,
            radius: self.radius + by,
        }
    }

    fn contains(self, x: f64, y: f64) -> bool {
        if x < self.x0 || x > self.x1 || y < self.y0 || y > self.y1 {
            return false;
        }
        let cx = x.clamp(self.x0 + self.radius, self.x1 - self.radius);
        let cy = y.clamp(self.y0 + self.radius, self.y1 - self.radius);
        let (dx, dy) = (x - cx, y - cy);
        dx * dx + dy * dy <= self.radius * self.radius
    }

    /// How many of 4×4 samples of pixel `(x, y)` lie inside.
    fn coverage(self, x: usize, y: usize) -> u32 {
        let mut inside = 0;
        for sy in 0..4u32 {
            for sx in 0..4u32 {
                #[allow(clippy::cast_precision_loss)]
                let (px, py) = (
                    x as f64 + (f64::from(sx) + 0.5) / 4.0,
                    y as f64 + (f64::from(sy) + 0.5) / 4.0,
                );
                if self.contains(px, py) {
                    inside += 1;
                }
            }
        }
        inside
    }

    fn fill(self, image: &mut Rgba, colour: [u8; 4]) {
        for y in 0..image.h() {
            for x in 0..image.w() {
                image.blend(x, y, colour, self.coverage(x, y));
            }
        }
    }

    fn cut(self, image: &mut Rgba) {
        for y in 0..image.h() {
            for x in 0..image.w() {
                image.erase(x, y, self.coverage(x, y));
            }
        }
    }

    /// Whether the whole pixel square `(x, y)` lies inside (all four corners).
    fn covers(self, x: usize, y: usize) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (x as f64, y as f64);
        [(x, y), (x + 1.0, y), (x, y + 1.0), (x + 1.0, y + 1.0)]
            .into_iter()
            .all(|(px, py)| self.contains(px, py))
    }
}

/// The pixels `text` lights with `face`, its top-left corner at `(left, top)`.
fn lit_pixels(face: Face, text: &str, left: usize, top: usize) -> Vec<(usize, usize)> {
    let mut lit = Vec::new();
    let mut x0 = left;
    for c in text.chars() {
        if let Some(rows) = (face.font.glyph)(c) {
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..face.font.width {
                    if bits & (1 << (face.font.width - 1 - col)) == 0 {
                        continue;
                    }
                    for dy in 0..face.scale {
                        for dx in 0..face.scale {
                            lit.push((x0 + col * face.scale + dx, top + row * face.scale + dy));
                        }
                    }
                }
            }
        }
        x0 += (face.font.width + 1) * face.scale;
    }
    lit
}

/// Draws `text` with `face` with its top-left corner at `(left, top)`, in solid white.
fn draw_text(image: &mut Rgba, face: Face, text: &str, left: usize, top: usize) {
    for (x, y) in lit_pixels(face, text, left, top) {
        image.put(x, y, WHITE);
    }
}

/// Whether `text` with `face`, placed at `(left, top)`, stands on solid ground: every lit pixel
/// and its eight neighbours lie wholly inside `shape`, so no digit touches the soft edge.
fn on_solid_ground(shape: Shape, face: Face, text: &str, left: usize, top: usize) -> bool {
    lit_pixels(face, text, left, top).into_iter().all(|(x, y)| {
        (0..3).all(|dy| {
            (0..3).all(|dx| match (x.checked_sub(1), y.checked_sub(1)) {
                (Some(nx), Some(ny)) => shape.covers(nx + dx, ny + dy),
                _ => false,
            })
        })
    })
}

/// Where text of `width`×`height` sits centred on a pixel grid of `span` from `start`.
fn centred(start: usize, span: usize, size: usize) -> usize {
    start + span.saturating_sub(size) / 2
}

/// The taskbar overlay: the count in white on red, `size`×`size` pixels. A disc, or for text the
/// disc would only take in a smaller face ("99+" from 20 px on), a rounded rectangle as wide as
/// the image.
pub fn overlay(count: u32, size: u32) -> Rgba {
    let n = size.max(8) as usize;
    let text = label(count.max(1));
    let mut image = Rgba::transparent(n, n);
    #[allow(clippy::cast_precision_loss)]
    let side = n as f64;
    let disc = Shape {
        x0: 0.0,
        y0: 0.0,
        x1: side,
        y1: side,
        radius: side / 2.0,
    };
    // A rounded rectangle as wide as the image, three quarters as high, for long text.
    let wide_h = (side * 0.75).round().max(8.0);
    let wide_top = ((side - wide_h) / 2.0).floor();
    let wide = Shape {
        x0: 0.0,
        y0: wide_top,
        x1: side,
        y1: wide_top + wide_h,
        radius: (wide_h * 0.2).max(2.0),
    };
    // The largest face that stands on solid ground in the shape and is at most 64 % of the side
    // high (so the shape keeps a visible rim).
    let best = |shape: Shape| {
        Face::ALL.into_iter().find(|face| {
            let (w, h) = (face.width_of(&text), face.height());
            #[allow(clippy::cast_precision_loss)]
            let fits_height = h as f64 <= side * 0.64;
            fits_height
                && w <= n
                && h <= n
                && on_solid_ground(shape, *face, &text, centred(0, n, w), centred(0, n, h))
        })
    };
    let (shape, face) = match (best(disc), best(wide)) {
        (Some(round), Some(long)) if long.height() > round.height() => (wide, long),
        (Some(round), _) => (disc, round),
        (None, Some(long)) => (wide, long),
        (None, None) => (wide, Face::ALL[4]),
    };
    shape.fill(&mut image, RED);
    let (w, h) = (face.width_of(&text), face.height());
    draw_text(&mut image, face, &text, centred(0, n, w), centred(0, n, h));
    image
}

/// The notification-area icon: `icon` (square RGBA) with the count in a red pill at its bottom
/// right, cut out of the icon by one pixel so it stands apart. `count` 0 returns the icon as is.
pub fn on_icon(icon: &Rgba, count: u32) -> Rgba {
    let mut image = icon.clone();
    if count == 0 || icon.width == 0 || icon.width != icon.height {
        return image;
    }
    let n = image.w();
    let text = label(count);
    // The badge is about 9/16 of the icon high (a circle for one digit, wider for more, at most
    // as wide as the icon); its text gets the largest face that stands on solid ground in it.
    let pill_h = (n * 9).div_ceil(16).max(7);
    let pad = (pill_h / 8).max(1);
    // `side`: room left and right of the text; more than `pad` when the corners need it.
    let place = |face: Face, side: usize| {
        let (w, h) = (face.width_of(&text), face.height());
        let pill_w = (w + 2 * side).max(pill_h).min(n);
        let (x0, y0) = (n - pill_w, n - pill_h);
        #[allow(clippy::cast_precision_loss)]
        let pill = Shape {
            x0: x0 as f64,
            y0: y0 as f64,
            x1: n as f64,
            y1: n as f64,
            // A circle for one glyph; longer text gets tighter corners.
            radius: if text.chars().count() == 1 {
                pill_h as f64 / 2.0
            } else {
                pill_h as f64 * 0.35
            },
        };
        (pill, centred(x0, pill_w, w), centred(y0, pill_h, h))
    };
    let Some((face, side)) = Face::ALL
        .into_iter()
        .flat_map(|face| (pad..=pad + 2).map(move |side| (face, side)))
        .find(|&(face, side)| {
            let (pill, left, top) = place(face, side);
            // Digits at most 36 % of the icon high, so the badge leaves the icon recognisable.
            face.height() * 100 <= n * 36
                && face.width_of(&text) + 2 * side <= n
                && on_solid_ground(pill, face, &text, left, top)
        })
    else {
        return image;
    };
    let (pill, left, top) = place(face, side);
    pill.grown(1.0).cut(&mut image);
    pill.fill(&mut image, RED);
    draw_text(&mut image, face, &text, left, top);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The image as text: `#` white, `o` the red background (opaque), `.` transparent, `+` an
    /// anti-aliased edge pixel.
    fn ascii(image: &Rgba) -> String {
        let mut out = String::new();
        for y in 0..image.h() {
            for x in 0..image.w() {
                let px = image.at(x, y);
                out.push(match px {
                    WHITE => '#',
                    RED => 'o',
                    [_, _, _, 0] => '.',
                    _ => '+',
                });
            }
            out.push('\n');
        }
        out
    }

    const SIZES: [u32; 6] = [16, 20, 24, 28, 32, 40];
    const COUNTS: [u32; 9] = [1, 7, 9, 10, 42, 88, 99, 100, 12_345];

    #[test]
    fn sizes_follow_the_display_scale() {
        assert_eq!(
            [1.0, 1.25, 1.5, 1.75, 2.0, 2.5].map(overlay_size),
            [16, 20, 24, 28, 32, 40]
        );
        assert_eq!([1.0, 1.25, 1.5, 2.0].map(tray_size), [16, 20, 24, 32]);
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
        assert_eq!(overlay(100, 16), overlay(5_000, 16));
    }

    #[test]
    fn overlay_has_the_size_asked_for_and_pixel_exact_digits() {
        for size in SIZES {
            for count in COUNTS {
                let image = overlay(count, size);
                assert_eq!((image.width, image.height), (size, size));
                assert_eq!(image.pixels.len(), (size * size * 4) as usize);
                let n = size as usize;
                let mut white = 0;
                for y in 0..n {
                    for x in 0..n {
                        let px = image.at(x, y);
                        match px {
                            WHITE => {
                                white += 1;
                                // A digit pixel stands on solid ground: all eight neighbours are
                                // opaque red or white, never the soft edge of the shape.
                                for (dx, dy) in [
                                    (-1, -1),
                                    (0, -1),
                                    (1, -1),
                                    (-1, 0),
                                    (1, 0),
                                    (-1, 1),
                                    (0, 1),
                                    (1, 1),
                                ] {
                                    let (nx, ny) =
                                        (x.checked_add_signed(dx), y.checked_add_signed(dy));
                                    let around = nx.zip(ny).map(|(nx, ny)| image.at(nx, ny));
                                    assert!(
                                        matches!(around, Some(RED | WHITE)),
                                        "{size}px {count}: ({x},{y}) next to {around:?}"
                                    );
                                }
                            }
                            // Opaque means solid red: no grey digit edges.
                            [_, _, _, 255] => assert_eq!(px, RED, "{size}px {count}: ({x},{y})"),
                            // Only the shape's edge is soft, in the same red.
                            [_, _, _, 0] => {}
                            _ => assert_eq!(px[..3], RED[..3], "{size}px {count}: ({x},{y})"),
                        }
                    }
                }
                assert!(white >= 7, "{size}px {count}: digits drawn");
            }
        }
    }

    #[test]
    fn digits_are_centred_and_apart() {
        for size in SIZES {
            for count in COUNTS {
                let image = overlay(count, size);
                let n = size as usize;
                let columns: Vec<bool> = (0..n)
                    .map(|x| (0..n).any(|y| image.at(x, y) == WHITE))
                    .collect();
                let rows: Vec<bool> = (0..n)
                    .map(|y| (0..n).any(|x| image.at(x, y) == WHITE))
                    .collect();
                let span = |v: &[bool]| {
                    let first = v.iter().position(|&b| b).unwrap_or(0);
                    let last = v.iter().rposition(|&b| b).unwrap_or(0);
                    (first, last)
                };
                let (left, right) = span(&columns);
                let (top, bottom) = span(&rows);
                // Centred to the pixel (glyphs like "1" and "7" leave a column of their cell
                // empty, so allow one cell of a 1× face).
                assert!(
                    left.abs_diff(n - 1 - right) <= 3,
                    "{size}px {count}: {left}..{right}"
                );
                assert!(
                    top.abs_diff(n - 1 - bottom) <= 2,
                    "{size}px {count}: {top}..{bottom}"
                );
                // Glyphs never touch: between the outer columns of two glyphs there is a gap.
                let glyphs = label(count).chars().count();
                let mut runs = 0;
                let mut inside = false;
                for &lit in &columns[left..=right] {
                    if lit && !inside {
                        runs += 1;
                    }
                    inside = lit;
                }
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
            let image = overlay(count, size);
            let n = size as usize;
            (0..n)
                .filter(|&y| (0..n).any(|x| image.at(x, y) == WHITE))
                .count()
        };
        for count in [7, 42, 100] {
            let heights: Vec<usize> = [16, 24, 32].iter().map(|&s| height(count, s)).collect();
            assert!(
                heights.windows(2).all(|w| w[0] <= w[1]),
                "{count}: {heights:?}"
            );
            assert!(heights[2] > heights[0], "{count}: {heights:?}");
        }
    }

    /// Golden images at 100 %: what Windows shows on the taskbar button, pixel for pixel. One
    /// digit bold (the 3×5 font at 2×), two in the 5×7 font, "99+" in the 3×5 font on a rounded
    /// rectangle as wide as the image.
    #[test]
    fn golden_16px() {
        assert_eq!(
            ascii(&overlay(7, 16)),
            "\
....+++oo+++....
..++oooooooo++..
.++oooooooooo++.
.+ooo######ooo+.
+oooo######oooo+
+oooooooo##oooo+
+oooooooo##oooo+
ooooooo##ooooooo
ooooooo##ooooooo
+oooooo##oooooo+
+oooooo##oooooo+
+oooooo##oooooo+
.+ooooo##ooooo+.
.++oooooooooo++.
..++oooooooo++..
....+++oo+++....
"
        );
        assert_eq!(
            ascii(&overlay(42, 16)),
            "\
....+++oo+++....
..++oooooooo++..
.++oooooooooo++.
.+oooooooooooo+.
+oooo#ooo###ooo+
+ooo##oo#ooo#oo+
+oo#o#oooooo#oo+
oo#oo#ooooo#oooo
oo#####ooo#ooooo
+oooo#ooo#ooooo+
+oooo#oo#####oo+
+oooooooooooooo+
.+oooooooooooo+.
.++oooooooooo++.
..++oooooooo++..
....+++oo+++....
"
        );
        assert_eq!(
            ascii(&overlay(100, 16)),
            "\
................
................
++oooooooooooo++
+oooooooooooooo+
oooooooooooooooo
oo###o###ooooooo
oo#o#o#o#oo#oooo
oo###o###o###ooo
oooo#ooo#oo#oooo
oo###o###ooooooo
oooooooooooooooo
oooooooooooooooo
+oooooooooooooo+
++oooooooooooo++
................
................
"
        );
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
            assert_eq!(on_icon(&icon, 0), icon, "no count, no badge");
            for count in COUNTS {
                let image = on_icon(&icon, count);
                assert_eq!((image.width, image.height), (icon.width, icon.height));
                // The top-left quarter is the icon, untouched.
                for y in 0..size / 3 {
                    for x in 0..size / 3 {
                        assert_eq!(image.at(x, y), icon.at(x, y), "{size}px {count}");
                    }
                }
                // The badge is red in the bottom-right quarter, the digits solid white on it.
                let red = (size / 2..size)
                    .flat_map(|y| (size / 2..size).map(move |x| (x, y)))
                    .filter(|&(x, y)| image.at(x, y) == RED)
                    .count();
                assert!(red >= size, "{size}px {count}: {red} red pixels");
                let white: Vec<(usize, usize)> = (0..size)
                    .flat_map(|y| (0..size).map(move |x| (x, y)))
                    .filter(|&(x, y)| image.at(x, y) == WHITE)
                    .collect();
                assert!(!white.is_empty(), "{size}px {count}");
                // Digits in the lower part, each on the badge's red (or next to another digit
                // pixel), never on the icon.
                for &(x, y) in &white {
                    assert!(y >= size / 3, "{size}px {count}: ({x},{y}) too high");
                    for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                        assert!(
                            matches!(image.at(nx, ny), RED | WHITE),
                            "{size}px {count}: ({x},{y}) not on the badge"
                        );
                    }
                }
            }
        }
    }

    /// Writes the overlays and tray badges as raw RGBA (`<count>-<size>.rgba`) to
    /// `BADGE_PREVIEW_DIR`, for looking at them (not part of the suite).
    #[test]
    #[ignore = "writes preview files"]
    fn write_previews() {
        let Some(dir) = std::env::var_os("BADGE_PREVIEW_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        for size in SIZES {
            for count in [1, 7, 42, 99, 100] {
                let image = overlay(count, size);
                std::fs::write(
                    dir.join(format!("overlay-{count}-{size}.rgba")),
                    &image.pixels,
                )
                .expect("write preview");
                println!("{size} px, {count}:\n{}", ascii(&image));
            }
        }
        // Tray icons: the app icon's own entries (`icon-<size>.rgba`, decoded by the caller).
        for size in [16u32, 20, 24, 32] {
            let Ok(pixels) = std::fs::read(dir.join(format!("icon-{size}.rgba"))) else {
                continue;
            };
            let icon = Rgba {
                width: size,
                height: size,
                pixels,
            };
            for count in [0, 3, 42, 100] {
                let image = on_icon(&icon, count);
                std::fs::write(dir.join(format!("tray-{count}-{size}.rgba")), &image.pixels)
                    .expect("write preview");
            }
        }
    }
}
