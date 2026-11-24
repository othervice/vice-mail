use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::{AnimationDecoder, Delay, ExtendedColorType, Frame, Rgba, RgbaImage};

use super::*;
use crate::image::{base64_decode, image_data_uri};

/// A PNG written with other settings than the re-encoder's, so the bytes differ even where
/// the pixels are the same.
fn png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
    let picture = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            u8::try_from(x % 256).unwrap_or(0),
            40,
            u8::try_from(y % 256).unwrap_or(0),
            alpha,
        ])
    });
    let mut out = Vec::new();
    picture
        .write_with_encoder(PngEncoder::new_with_quality(
            &mut out,
            CompressionType::Best,
            FilterType::Paeth,
        ))
        .expect("png");
    out
}

/// A GIF of `frames` frames, each a solid colour of its own, with a comment before the trailer
/// (bytes the re-encoder does not write).
fn animated_gif(width: u32, height: u32, frames: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 30);
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        for n in 0..frames {
            let shade = u8::try_from(n * 37 % 256).unwrap_or(0);
            let buffer = RgbaImage::from_pixel(width, height, Rgba([shade, 255 - shade, 90, 255]));
            encoder
                .encode_frame(Frame::from_parts(
                    buffer,
                    0,
                    0,
                    Delay::from_numer_denom_ms(80, 1),
                ))
                .expect("frame");
        }
    }
    assert_eq!(out.pop(), Some(0x3B), "the trailer");
    out.extend_from_slice(b"\x21\xFE\x05hello\x00\x3B");
    out
}

fn chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
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

/// An animated WebP (the `image` crate writes none): lossless frames from the still encoder,
/// wrapped in `VP8X`, `ANIM` and `ANMF` chunks as the container specification says.
fn animated_webp(width: u32, height: u32, frames: u32) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    let mut vp8x = vec![0x12, 0, 0, 0]; // alpha and animation
    vp8x.extend_from_slice(&u24(width - 1));
    vp8x.extend_from_slice(&u24(height - 1));
    body.extend(chunk(b"VP8X", &vp8x));
    body.extend(chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
    for n in 0..frames {
        let shade = u8::try_from(n * 61 % 256).unwrap_or(0);
        let picture = RgbaImage::from_pixel(width, height, Rgba([shade, 30, 255 - shade, 255]));
        let mut still = Vec::new();
        WebPEncoder::new_lossless(&mut still)
            .encode(picture.as_raw(), width, height, ExtendedColorType::Rgba8)
            .expect("webp");
        assert_eq!(&still[12..16], b"VP8L", "the simple lossless format");
        let mut anmf = Vec::new();
        anmf.extend_from_slice(&u24(0));
        anmf.extend_from_slice(&u24(0));
        anmf.extend_from_slice(&u24(width - 1));
        anmf.extend_from_slice(&u24(height - 1));
        anmf.extend_from_slice(&u24(100));
        anmf.push(0x02); // no blending, no disposal
        anmf.extend_from_slice(&still[12..]);
        body.extend(chunk(b"ANMF", &anmf));
    }
    chunk(b"RIFF", &body)
}

fn decoded(url: &str) -> Vec<u8> {
    let (_, payload) = url.split_once(";base64,").expect("data url");
    base64_decode(payload).expect("base64")
}

fn gif_frames(bytes: &[u8]) -> usize {
    GifDecoder::new(Cursor::new(bytes))
        .expect("gif")
        .into_frames()
        .collect_frames()
        .expect("frames")
        .len()
}

#[test]
fn a_still_image_is_encoded_again_never_passed_on() {
    for (bytes, mime) in [
        (png(40, 30, 255), "data:image/jpeg;base64,"),
        (png(40, 30, 100), "data:image/png;base64,"),
    ] {
        let image = reencode_image(&bytes).expect("re-encoded");
        assert!(
            image.data_url.starts_with(mime),
            "{}",
            &image.data_url[..30]
        );
        assert_ne!(Some(&image.data_url), image_data_uri(&bytes).as_ref());
        assert_ne!(decoded(&image.data_url), bytes, "not the original bytes");
        let picture = image::load_from_memory(&decoded(&image.data_url)).expect("decodes");
        assert_eq!((picture.width(), picture.height()), (40, 30));
        assert_eq!((image.width, image.height, image.frames), (40, 30, 1));
        assert!(is_reencoded_data_url(&image.data_url));
    }
}

#[test]
fn an_animated_gif_stays_animated() {
    let bytes = animated_gif(24, 16, 3);
    let image = reencode_image(&bytes).expect("re-encoded");
    assert!(image.data_url.starts_with("data:image/gif;base64,"));
    assert_eq!((image.width, image.height, image.frames), (24, 16, 3));
    let out = decoded(&image.data_url);
    assert_ne!(out, bytes, "written again, not passed on");
    assert_eq!(gif_frames(&out), 3);
    let loops = |bytes: &[u8]| bytes.windows(11).any(|w| w == b"NETSCAPE2.0");
    assert!(loops(&out), "it loops as the original does");
    // Without a loop extension a browser plays it once: none is written either.
    let once: Vec<u8> = {
        let at = bytes
            .windows(11)
            .position(|w| w == b"NETSCAPE2.0")
            .expect("extension");
        // The application extension: 0x21 0xFF 0x0B, 11 bytes, a 3-byte sub-block, terminator.
        [&bytes[..at - 3], &bytes[at + 11 + 5..]].concat()
    };
    let image = reencode_image(&once).expect("re-encoded");
    assert_eq!(image.frames, 3);
    assert!(!loops(&decoded(&image.data_url)));
}

#[test]
fn an_animated_webp_becomes_an_animated_gif() {
    let bytes = animated_webp(20, 10, 2);
    assert!(
        WebPDecoder::new(Cursor::new(&bytes))
            .expect("webp")
            .has_animation(),
        "the probe is an animation"
    );
    let image = reencode_image(&bytes).expect("re-encoded");
    assert!(image.data_url.starts_with("data:image/gif;base64,"));
    assert_eq!((image.width, image.height, image.frames), (20, 10, 2));
    assert_eq!(gif_frames(&decoded(&image.data_url)), 2);
}

#[test]
fn one_frame_or_an_animation_over_the_limits_shows_its_first_frame() {
    // A GIF of one frame is a still picture.
    let single = reencode_image(&animated_gif(8, 8, 1)).expect("still");
    assert_eq!(single.frames, 1);
    assert!(single.data_url.starts_with("data:image/jpeg;base64,"));
    // More frames than allowed.
    let long = animated_gif(2, 2, MAX_ANIMATION_FRAMES + 1);
    let image = reencode_image(&long).expect("first frame");
    assert_eq!((image.width, image.height, image.frames), (2, 2, 1));
    assert!(!image.data_url.starts_with("data:image/gif"));
    // Wider than the edge: the first frame, scaled.
    let wide = animated_gif(MAX_PREVIEW_EDGE + 100, 4, 2);
    let image = reencode_image(&wide).expect("first frame");
    assert_eq!((image.width, image.frames), (MAX_PREVIEW_EDGE, 1));
}

#[test]
fn broken_oversized_and_foreign_images_are_refused() {
    let broken = [b"GIF89a".as_slice(), &[0xFF; 40]].concat();
    assert!(matches!(
        reencode_image(&broken),
        Err(PreviewError::Damaged(_) | PreviewError::Unsupported(_))
    ));
    let truncated = &png(64, 64, 255)[..80];
    assert!(reencode_image(truncated).is_err());
    // A header that claims 20000×20000 pixels: refused before decoding.
    let mut huge = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    huge.extend_from_slice(&20_000u32.to_be_bytes());
    huge.extend_from_slice(&20_000u32.to_be_bytes());
    huge.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    assert!(reencode_image(&huge).is_err());
    assert!(matches!(
        reencode_image(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
        Err(PreviewError::Unsupported(_))
    ));
    let bmp = [b"BM".as_slice(), &[0; 60]].concat();
    assert!(matches!(
        reencode_image(&bmp),
        Err(PreviewError::Unsupported(_))
    ));
}

#[test]
fn a_batch_keeps_the_order_and_matches_one_by_one() {
    let images = vec![
        png(300, 200, 255),
        b"not an image".to_vec(),
        animated_gif(10, 10, 2),
        png(5, 5, 0),
        png(1200, 40, 255),
    ];
    let batch = reencode_images(&images);
    assert_eq!(batch.len(), images.len());
    for (bytes, result) in images.iter().zip(&batch) {
        assert_eq!(result, &reencode_image(bytes));
    }
    assert!(batch[1].is_err());
    assert_eq!(batch[2].as_ref().map(|i| i.frames), Ok(2));
    assert!(reencode_images::<Vec<u8>>(&[]).is_empty());
}

#[test]
fn only_re_encoded_data_urls_are_taken() {
    for ok in [
        "data:image/png;base64,iVBORw0KGgo=",
        "data:image/jpeg;base64,/9j/4AAQ",
        "data:image/gif;base64,R0lGODlhAQABAA==",
    ] {
        assert!(is_reencoded_data_url(ok), "{ok}");
    }
    for bad in [
        "data:image/webp;base64,UklGRg==",
        "data:image/svg+xml;base64,PHN2Zz4=",
        "data:image/png;base64,",
        "data:image/png;base64,iVBOR\"onerror=x",
        "data:image/png;base64,iVBORw0KGgo===",
        "data:image/png;base64,iVB=ORw0",
        "data:image/png,iVBORw0KGgo=",
        "https://example.com/a.png",
    ] {
        assert!(!is_reencoded_data_url(bad), "{bad}");
    }
    let original: ImageBytes = Arc::from(png(2, 2, 255).as_slice());
    let mut images = ReencodedImages::new();
    assert!(!images.insert(
        original.clone(),
        Arc::from("data:image/webp;base64,UklGRg==")
    ));
    assert_eq!(images.lookup(&original), Some(None), "recorded as refused");
    assert!(images.insert(
        original.clone(),
        Arc::from("data:image/png;base64,iVBORw0KGgo=")
    ));
    assert!(matches!(images.lookup(&original), Some(Some(_))));
    assert_eq!(
        images.get(&original),
        Some("data:image/png;base64,iVBORw0KGgo=")
    );
    assert_eq!(images.lookup(b"other"), None);
    images.refuse(original.clone());
    assert_eq!(images.get(&original), None);
}

#[test]
fn the_memory_budget_lets_one_large_image_run_alone() {
    let budget = Budget::new(100);
    let a = budget.take(60);
    let b = budget.take(40);
    assert_eq!(*budget.free.lock().expect("lock"), 0);
    drop(a);
    // More than the whole budget is clipped to it, so it can always run.
    drop(b);
    let c = budget.take(1_000);
    assert_eq!(c.amount, 100);
    drop(c);
    assert_eq!(*budget.free.lock().expect("lock"), 100);
    assert!(memory_estimate(&png(100, 50, 255)) >= 100 * 50 * 4);
    assert_eq!(memory_estimate(b"junk"), 0);
}
