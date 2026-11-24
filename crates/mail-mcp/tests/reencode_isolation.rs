//! Finding C-4 (decision 0046): the images of a mail are re-encoded in the helper, one batch per
//! rendering, watched from outside like attachment previews. Against the real `mail-mcp` binary
//! of this build.
//!
//! The measurement of the newsletter through the helper (start included) is ignored by default:
//!
//! ```text
//! cargo test -p mail-mcp --test reencode_isolation --release -- --ignored --nocapture
//! ```

#[path = "../../mail-content/tests/newsletter/mod.rs"]
mod newsletter;

use std::io::Cursor;
use std::path::Path;
use std::time::{Duration, Instant};

use image::codecs::gif::GifDecoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{AnimationDecoder, Rgba, RgbaImage};
use mail_content::{ImageBytes, image_data_uri};
use mail_mcp::preview::{PreviewLimits, reencode_isolated};
use mail_model::PreviewError;

const HELPER: &str = env!("CARGO_BIN_EXE_mail-mcp");

fn png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
    let picture = RgbaImage::from_fn(width, height, |x, _| {
        Rgba([u8::try_from(x % 256).unwrap_or(0), 90, 160, alpha])
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

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG header that claims 20000×20000 pixels (refused before anything is decoded).
fn huge_png() -> Vec<u8> {
    let mut chunk = b"IHDR".to_vec();
    chunk.extend_from_slice(&20_000u32.to_be_bytes());
    chunk.extend_from_slice(&20_000u32.to_be_bytes());
    chunk.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n\0\0\0\x0d".to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&crc32(&chunk).to_be_bytes());
    out.extend_from_slice(b"\0\0\0\0IDAT\x35\xaf\x06\x1e");
    out
}

fn base64_decode(text: &str) -> Vec<u8> {
    let value = |b: u8| match b {
        b'A'..=b'Z' => b - b'A',
        b'a'..=b'z' => b - b'a' + 26,
        b'0'..=b'9' => b - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in text.bytes().take_while(|b| *b != b'=') {
        acc = (acc << 6) | u32::from(value(byte));
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).unwrap_or(0));
        }
    }
    out
}

fn payload(data_url: &str) -> Vec<u8> {
    base64_decode(data_url.split_once(";base64,").expect("base64").1)
}

#[tokio::test]
async fn a_batch_is_re_encoded_in_the_helper() {
    let still = png(64, 48, 255);
    let banner = newsletter::animated_banner(120, 40, 5);
    let images: Vec<ImageBytes> = vec![
        ImageBytes::from(still.as_slice()),
        ImageBytes::from(banner.as_slice()),
        ImageBytes::from([b"GIF89a".as_slice(), &[0xFF; 64]].concat()),
        ImageBytes::from(huge_png()),
        ImageBytes::from(&b"<svg onload=alert(1)>"[..]),
    ];
    let started = Instant::now();
    let results = reencode_isolated(Path::new(HELPER), &images, PreviewLimits::default())
        .await
        .expect("the batch ran");
    eprintln!("five images through the helper: {:?}", started.elapsed());
    assert_eq!(results.len(), 5);

    // A still picture: new bytes, the same picture size.
    let image = results[0].as_ref().expect("re-encoded");
    assert!(image.data_url.starts_with("data:image/jpeg;base64,"));
    assert_ne!(Some(&image.data_url), image_data_uri(&still).as_ref());
    let decoded = image::load_from_memory(&payload(&image.data_url)).expect("decodes");
    assert_eq!((decoded.width(), decoded.height()), (64, 48));

    // The animation stays one.
    let image = results[1].as_ref().expect("re-encoded");
    assert!(image.data_url.starts_with("data:image/gif;base64,"));
    assert_eq!(image.frames, 5);
    let frames = GifDecoder::new(Cursor::new(payload(&image.data_url)))
        .expect("gif")
        .into_frames()
        .collect_frames()
        .expect("frames");
    assert_eq!(frames.len(), 5, "more than one frame after re-encoding");

    // Broken, oversized and foreign images are refused, typed.
    assert!(matches!(results[2], Err(PreviewError::Damaged(_))));
    assert!(matches!(results[3], Err(PreviewError::TooLarge(_))));
    assert!(matches!(results[4], Err(PreviewError::Unsupported(_))));
}

#[tokio::test]
async fn a_missing_or_failing_helper_fails_the_batch() {
    let images = vec![ImageBytes::from(png(4, 4, 255))];
    let missing = reencode_isolated(
        Path::new("/nonexistent/mail-mcp"),
        &images,
        PreviewLimits::default(),
    )
    .await;
    assert!(
        matches!(missing, Err(PreviewError::Unsupported(_))),
        "{missing:?}"
    );
    #[cfg(unix)]
    {
        // A program that ends with an error and answers nothing.
        let failing = reencode_isolated(
            Path::new("/usr/bin/false"),
            &images,
            PreviewLimits::default(),
        )
        .await;
        assert!(
            matches!(failing, Err(PreviewError::Damaged(_))),
            "{failing:?}"
        );
    }
    // Stopped over the time limit (a limit no real batch meets).
    let limits = PreviewLimits {
        time: Duration::ZERO,
        poll: Duration::from_millis(1),
        ..PreviewLimits::default()
    };
    let banner = newsletter::animated_banner(600, 200, 24);
    let stopped = reencode_isolated(Path::new(HELPER), &[ImageBytes::from(banner)], limits).await;
    assert!(
        matches!(stopped, Err(PreviewError::TooLarge(_))),
        "{stopped:?}"
    );
    // Nothing to do: no helper is started.
    let none = reencode_isolated(
        Path::new("/nonexistent/mail-mcp"),
        &[],
        PreviewLimits::default(),
    )
    .await;
    assert_eq!(none, Ok(Vec::new()));
}

/// The 32-image newsletter through the helper, start included, as the core runs it for the
/// first view of such a mail. Budget (decision 0046): under about 150 ms in a release build.
#[tokio::test]
#[ignore = "a measurement; run with --release --ignored --nocapture"]
async fn the_newsletter_through_the_helper() {
    let images: Vec<ImageBytes> = newsletter::newsletter()
        .into_iter()
        .map(|(_, bytes)| ImageBytes::from(bytes))
        .collect();
    let mut times = Vec::new();
    // The first child process of this process (any program), then the helper's first start.
    let started = Instant::now();
    let _ = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "/bin/echo" })
        .output()
        .await;
    let first_child = started.elapsed();
    let started = Instant::now();
    let first = reencode_isolated(
        Path::new(HELPER),
        &[ImageBytes::from(png(1, 1, 255))],
        PreviewLimits::default(),
    )
    .await
    .expect("ran");
    let first_start = started.elapsed();
    assert!(first[0].is_ok());
    for _ in 0..7 {
        let started = Instant::now();
        let results = reencode_isolated(Path::new(HELPER), &images, PreviewLimits::default())
            .await
            .expect("the batch ran");
        times.push(started.elapsed());
        assert!(results.iter().all(Result::is_ok), "{results:?}");
    }
    let started = Instant::now();
    let _ = reencode_isolated(Path::new(HELPER), &[], PreviewLimits::default()).await;
    let nothing = started.elapsed();
    let started = Instant::now();
    let tiny = reencode_isolated(
        Path::new(HELPER),
        &[ImageBytes::from(png(1, 1, 255))],
        PreviewLimits::default(),
    )
    .await
    .expect("ran");
    let start_only = started.elapsed();
    assert!(tiny[0].is_ok());
    times.sort();
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "C-4 newsletter (32 images) through the helper, {build}: first child of the process (echo) {:.1} ms, first helper start (1×1 image) {:.1} ms; newsletter best {:.1} ms, median {:.1} ms, worst {:.1} ms; one 1×1 image (start and reply) {:.1} ms; empty batch {:.1} ms",
        first_child.as_secs_f64() * 1000.0,
        first_start.as_secs_f64() * 1000.0,
        times[0].as_secs_f64() * 1000.0,
        times[times.len() / 2].as_secs_f64() * 1000.0,
        times[times.len() - 1].as_secs_f64() * 1000.0,
        start_only.as_secs_f64() * 1000.0,
        nothing.as_secs_f64() * 1000.0
    );
}
