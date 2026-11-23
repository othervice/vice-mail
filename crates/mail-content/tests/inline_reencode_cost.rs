//! Finding C-4 of the stage 15 review: what would it cost to re-encode the inline (`cid:`) and
//! allowed remote images of a mail through the attachment-preview pipeline (decode in Rust,
//! PNG/JPEG out) instead of passing their bytes to the web view's decoders?
//!
//! A typical HTML newsletter, built here (no network, no real mail): 32 images, 1x and 2x
//! ("retina") photos, a logo and icons with transparency, social icons, spacer and tracking
//! GIFs. The photos are JPEGs of a photo-like picture (gradients, shapes and grain) at quality 82,
//! so sizes and decode work are close to real newsletter photos. Measured: the current path
//! (`image_data_uri`, sniff and base64) and re-encoding (`render_preview`, which decodes, turns
//! upright, scales to at most 2048 px and encodes again), sequentially, best of five rounds.
//!
//! Ignored by default (a measurement, not a check):
//!
//! ```text
//! cargo test -p mail-content --test inline_reencode_cost -- --ignored --nocapture
//! cargo test -p mail-content --test inline_reencode_cost --release -- --ignored --nocapture
//! ```

use std::io::Cursor;
use std::time::{Duration, Instant};

use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use mail_content::{image_data_uri, render_preview};
use mail_model::{AttachmentPreview, PreviewKind};

/// A small deterministic generator (xorshift), so every run measures the same pictures.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn grain(&mut self, spread: i32) -> i32 {
        i32::try_from(self.next() % u64::try_from(2 * spread + 1).unwrap_or(1)).unwrap_or(0)
            - spread
    }
}

fn channel(value: i32) -> u8 {
    u8::try_from(value.clamp(0, 255)).unwrap_or(0)
}

/// A photo-like picture: two gradients, a few soft discs and grain, as a JPEG.
fn photo(width: u32, height: u32, seed: u64) -> Vec<u8> {
    let mut noise = Noise(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let discs: Vec<(i64, i64, i64, [i32; 3])> = (0..6)
        .map(|_| {
            (
                i64::try_from(noise.next() % u64::from(width)).unwrap_or(0),
                i64::try_from(noise.next() % u64::from(height)).unwrap_or(0),
                i64::try_from(noise.next() % u64::from(width.max(2) / 2) + 8).unwrap_or(8),
                [noise.grain(90), noise.grain(90), noise.grain(90)],
            )
        })
        .collect();
    let image = RgbImage::from_fn(width, height, |x, y| {
        let (xf, yf) = (i64::from(x), i64::from(y));
        let mut rgb = [
            i32::try_from(xf * 200 / i64::from(width)).unwrap_or(0) + 30,
            i32::try_from(yf * 180 / i64::from(height)).unwrap_or(0) + 40,
            i32::try_from((xf + yf) * 120 / i64::from(width + height)).unwrap_or(0) + 60,
        ];
        for (cx, cy, r, tint) in &discs {
            let d2 = (xf - cx).pow(2) + (yf - cy).pow(2);
            if d2 < r * r {
                for (c, t) in rgb.iter_mut().zip(tint) {
                    *c += t;
                }
            }
        }
        let g = noise.grain(10);
        Rgb([
            channel(rgb[0] + g),
            channel(rgb[1] + g),
            channel(rgb[2] + g),
        ])
    });
    let mut out = Vec::new();
    image
        .write_with_encoder(JpegEncoder::new_with_quality(&mut out, 82))
        .expect("jpeg");
    out
}

/// A flat graphic with transparent corners (logo, icon), as a PNG.
fn graphic(width: u32, height: u32, seed: u64) -> Vec<u8> {
    let colour = [
        channel(i32::try_from(seed * 53 % 256).unwrap_or(0)),
        channel(i32::try_from(seed * 97 % 256).unwrap_or(0)),
        channel(i32::try_from(seed * 31 % 256).unwrap_or(0)),
    ];
    let image = RgbaImage::from_fn(width, height, |x, y| {
        let (dx, dy) = (
            i64::from(x) - i64::from(width / 2),
            i64::from(y) - i64::from(height / 2),
        );
        let inside = dx * dx * i64::from(height * height) + dy * dy * i64::from(width * width)
            < i64::from(width * width) * i64::from(height * height) / 4;
        if inside {
            Rgba([colour[0], colour[1], colour[2], 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let mut out = Vec::new();
    image
        .write_with_encoder(PngEncoder::new(&mut out))
        .expect("png");
    out
}

fn gif(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 0]));
    let mut out = Vec::new();
    GifEncoder::new(Cursor::new(&mut out))
        .encode(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .expect("gif");
    out
}

/// 32 images of a typical marketing newsletter.
fn newsletter() -> Vec<(&'static str, Vec<u8>)> {
    let mut images = vec![
        ("hero 1200×600 (2x)", photo(1200, 600, 1)),
        ("logo 400×120 png", graphic(400, 120, 2)),
    ];
    for i in 0..4 {
        images.push(("article 1200×800 (2x)", photo(1200, 800, 10 + i)));
    }
    for i in 0..6 {
        images.push(("product 600×600", photo(600, 600, 20 + i)));
    }
    for i in 0..2 {
        images.push(("banner 600×200", photo(600, 200, 30 + i)));
    }
    for i in 0..10 {
        images.push(("icon 96×96 png", graphic(96, 96, 40 + i)));
    }
    for i in 0..6 {
        images.push(("social 64×64 png", graphic(64, 64, 60 + i)));
    }
    images.push(("spacer 1×1 gif", gif(1, 1)));
    images.push(("tracking 1×1 gif", gif(1, 1)));
    images
}

fn best_of<T>(rounds: usize, mut run: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut last = None;
    for _ in 0..rounds {
        let start = Instant::now();
        let value = run();
        best = best.min(start.elapsed());
        last = Some(value);
    }
    (best, last.expect("one round"))
}

#[test]
#[ignore = "a measurement; run with --ignored --nocapture"]
fn reencoding_a_newsletters_images() {
    let images = newsletter();
    let input: usize = images.iter().map(|(_, b)| b.len()).sum();
    let mut pixels = 0_u64;

    let (current, current_out) = best_of(5, || {
        images
            .iter()
            .map(|(_, bytes)| image_data_uri(bytes).map_or(0, |uri| uri.len()))
            .sum::<usize>()
    });

    let mut slowest = (Duration::ZERO, "");
    let (reencoded, reencoded_out) = best_of(5, || {
        let mut out = 0;
        for (name, bytes) in &images {
            let start = Instant::now();
            let preview = render_preview(PreviewKind::Image, bytes, 0).expect("preview");
            let took = start.elapsed();
            if took > slowest.0 {
                slowest = (took, name);
            }
            if let AttachmentPreview::Image { data_url, .. } = preview {
                out += data_url.len();
            }
        }
        out
    });
    for (_, bytes) in &images {
        let decoded = image::load_from_memory(bytes).expect("decode");
        pixels += u64::from(decoded.width()) * u64::from(decoded.height());
    }

    let build = if cfg!(debug_assertions) {
        "debug (dependencies unoptimised)"
    } else {
        "optimised"
    };
    println!("C-4 re-encoding cost, {build}");
    println!(
        "  {} images, {:.1} MP, {} KiB in",
        images.len(),
        pixels as f64 / 1e6,
        input / 1024
    );
    println!(
        "  current (sniff + base64):    {:>8.1} ms, {} KiB of data URLs",
        current.as_secs_f64() * 1000.0,
        current_out / 1024
    );
    println!(
        "  re-encoded (render_preview): {:>8.1} ms, {} KiB of data URLs, slowest image {:.1} ms ({})",
        reencoded.as_secs_f64() * 1000.0,
        reencoded_out / 1024,
        slowest.0.as_secs_f64() * 1000.0,
        slowest.1
    );
}
