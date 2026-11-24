//! A typical HTML newsletter's images, built in code (no network, no real mail), shared by the
//! re-encoding measurements of finding C-4 (`mail-content/tests/inline_reencode_cost.rs` and
//! `mail-mcp/tests/reencode_isolation.rs`): 32 images, 1x and 2x ("retina") photos, a logo and
//! icons with transparency, social icons, spacer and tracking GIFs. The photos are JPEGs of a
//! photo-like picture (gradients, shapes and grain) at quality 82, so sizes and decode work are
//! close to real newsletter photos. Also an animated banner (GIF) and an animated WebP of
//! photo-like frames (more than 256 colours: the quantiser runs) for the cost of animations.

#![allow(dead_code)]

use std::io::Cursor;

use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{Delay, ExtendedColorType, Frame, Rgb, RgbImage, Rgba, RgbaImage};

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
pub fn photo(width: u32, height: u32, seed: u64) -> Vec<u8> {
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
pub fn graphic(width: u32, height: u32, seed: u64) -> Vec<u8> {
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

pub fn gif(width: u32, height: u32) -> Vec<u8> {
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
pub fn newsletter() -> Vec<(&'static str, Vec<u8>)> {
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

/// An animated banner: `frames` frames of `width`×`height`, a bar moving over a flat
/// background (at most 256 colours per frame, as real GIF banners), 80 ms each, looping.
pub fn animated_banner(width: u32, height: u32, frames: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 30);
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        for n in 0..frames {
            let bar = n * width / frames.max(1);
            let picture = RgbaImage::from_fn(width, height, |x, y| {
                if x >= bar && x < bar + width / 8 {
                    Rgba([230, 80, 20, 255])
                } else {
                    let shade = u8::try_from(y * 64 / height.max(1)).unwrap_or(0);
                    Rgba([20 + shade, 40, 90, 255])
                }
            });
            encoder
                .encode_frame(Frame::from_parts(
                    picture,
                    0,
                    0,
                    Delay::from_numer_denom_ms(80, 1),
                ))
                .expect("frame");
        }
    }
    out
}

fn riff_chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn u24(value: u32) -> [u8; 3] {
    let [a, b, c, _] = value.to_le_bytes();
    [a, b, c]
}

/// An animated WebP of photo-like frames (the `image` crate writes no animated WebP: lossless
/// frames from its still encoder in `ANMF` chunks, as the container specification says).
pub fn animated_webp(width: u32, height: u32, frames: u32) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    let mut vp8x = vec![0x02, 0, 0, 0]; // animation
    vp8x.extend_from_slice(&u24(width - 1));
    vp8x.extend_from_slice(&u24(height - 1));
    body.extend(riff_chunk(b"VP8X", &vp8x));
    body.extend(riff_chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
    for n in 0..frames {
        let picture = image::load_from_memory(&photo(width, height, 100 + u64::from(n)))
            .expect("photo")
            .to_rgb8();
        let mut still = Vec::new();
        WebPEncoder::new_lossless(&mut still)
            .encode(picture.as_raw(), width, height, ExtendedColorType::Rgb8)
            .expect("webp");
        let mut anmf = Vec::new();
        for value in [0, 0, width - 1, height - 1, 80] {
            anmf.extend_from_slice(&u24(value));
        }
        anmf.push(0x02); // no blending, no disposal
        anmf.extend_from_slice(&still[12..]);
        body.extend(riff_chunk(b"ANMF", &anmf));
    }
    riff_chunk(b"RIFF", &body)
}
