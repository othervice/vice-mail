//! Finding C-4 of the stage 15 review (decision 0046): what does it cost to re-encode the
//! inline (`cid:`) and allowed remote images of a mail (decode in Rust, PNG/JPEG or animated
//! GIF out) instead of passing their bytes to the web view's decoders?
//!
//! The newsletter of `newsletter/mod.rs` (32 images). Measured in this process, best of five
//! rounds: the old path (`image_data_uri`, sniff and base64), re-encoding one by one
//! (`reencode_image`) and the batch on the machine's cores (`reencode_images`, what the helper
//! runs); and an animated banner. The cost through the helper process, start included, is
//! measured in `crates/mail-mcp/tests/reencode_isolation.rs`.
//!
//! Ignored by default (a measurement, not a check):
//!
//! ```text
//! cargo test -p mail-content --test inline_reencode_cost -- --ignored --nocapture
//! cargo test -p mail-content --test inline_reencode_cost --release -- --ignored --nocapture
//! ```

mod newsletter;

use std::time::{Duration, Instant};

use mail_content::{image_data_uri, reencode_image, reencode_images};

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

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[test]
#[ignore = "a measurement; run with --ignored --nocapture"]
fn reencoding_a_newsletters_images() {
    let images = newsletter::newsletter();
    let input: usize = images.iter().map(|(_, b)| b.len()).sum();
    let mut pixels = 0_u64;
    for (_, bytes) in &images {
        let decoded = image::load_from_memory(bytes).expect("decode");
        pixels += u64::from(decoded.width()) * u64::from(decoded.height());
    }

    let (old, old_out) = best_of(5, || {
        images
            .iter()
            .map(|(_, bytes)| image_data_uri(bytes).map_or(0, |uri| uri.len()))
            .sum::<usize>()
    });

    let mut slowest = (Duration::ZERO, "");
    let (one_by_one, out) = best_of(5, || {
        let mut out = 0;
        for (name, bytes) in &images {
            let start = Instant::now();
            let image = reencode_image(bytes).expect("re-encoded");
            let took = start.elapsed();
            if took > slowest.0 {
                slowest = (took, name);
            }
            out += image.data_url.len();
        }
        out
    });

    let bytes: Vec<&[u8]> = images.iter().map(|(_, b)| b.as_slice()).collect();
    let (batch, batch_out) = best_of(5, || {
        reencode_images(&bytes)
            .into_iter()
            .map(|r| r.expect("re-encoded").data_url.len())
            .sum::<usize>()
    });
    assert_eq!(batch_out, out, "the batch writes what one by one writes");

    let banner = newsletter::animated_banner(600, 200, 24);
    let (animated, banner_out) = best_of(5, || reencode_image(&banner).expect("re-encoded"));
    assert_eq!(banner_out.frames, 24, "the banner stays animated");
    let webp = newsletter::animated_webp(400, 300, 12);
    let (quantised, webp_out) = best_of(3, || reencode_image(&webp).expect("re-encoded"));
    assert_eq!(webp_out.frames, 12, "the WebP stays animated");

    let build = if cfg!(debug_assertions) {
        "debug (dependencies unoptimised)"
    } else {
        "optimised"
    };
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    println!("C-4 re-encoding cost in this process, {build}, {cores} cores");
    println!(
        "  {} images, {:.1} MP, {} KiB in",
        images.len(),
        pixels as f64 / 1e6,
        input / 1024
    );
    println!(
        "  old path (sniff + base64):  {:>8.1} ms, {} KiB of data URLs",
        ms(old),
        old_out / 1024
    );
    println!(
        "  re-encoded one by one:      {:>8.1} ms, {} KiB of data URLs, slowest image {:.1} ms ({})",
        ms(one_by_one),
        out / 1024,
        ms(slowest.0),
        slowest.1
    );
    println!("  re-encoded as a batch:      {:>8.1} ms", ms(batch));
    println!(
        "  animated banner 600×200, 24 frames ({} KiB in, {} KiB out): {:.1} ms",
        banner.len() / 1024,
        banner_out.data_url.len() * 3 / 4 / 1024,
        ms(animated)
    );
    println!(
        "  animated WebP 400×300, 12 photo frames ({} KiB in, {} KiB GIF out): {:.1} ms",
        webp.len() / 1024,
        webp_out.data_url.len() * 3 / 4 / 1024,
        ms(quantised)
    );
}
