//! Images inside mail HTML, re-encoded (finding C-4 of the stage 15 review, decision 0046).
//!
//! The web view's image decoders (ImageIO/WebKit on macOS, WebView2's on Windows) never see
//! bytes from a mail. Every image a rendering shows (inline `cid:` parts, external images the
//! user allowed, `data:` URLs written into the HTML) is decoded here in Rust within the limits of
//! the attachment preview ([`MAX_PREVIEW_PIXELS`], [`MAX_PREVIEW_DECODE_BYTES`]) and encoded
//! again, at most [`MAX_PREVIEW_EDGE`] per side:
//!
//! - a still picture as PNG (where a pixel is transparent) or JPEG, as the attachment preview;
//! - an animated GIF or WebP as an animated GIF, frame by frame, while the animation stays within
//!   [`MAX_ANIMATION_FRAMES`], [`MAX_ANIMATION_PIXELS`] and [`MAX_ANIMATION_BYTES`] and fits
//!   [`MAX_PREVIEW_EDGE`]; otherwise its first frame as a still picture.
//!
//! The product runs this in the `mail-mcp` helper process, one batch per rendering, under the
//! watchdog of the attachment preview (memory and time); [`reencode_images`] spreads a batch
//! over the machine's cores within one memory budget. The renderer gets the results as
//! [`ReencodedImages`]: an image without a re-encoding there is not shown.
//!
//! [`MAX_PREVIEW_DECODE_BYTES`]: crate::attachment_preview::MAX_PREVIEW_DECODE_BYTES

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::webp::WebPDecoder;
use image::metadata::LoopCount;
use image::{
    AnimationDecoder, DynamicImage, Frame, Frames, ImageDecoder, ImageFormat, ImageReader,
};
use mail_model::{MAX_PREVIEW_EDGE, MAX_PREVIEW_INPUT_BYTES, PreviewError};

use crate::attachment_preview::{
    MAX_PREVIEW_DECODE_BYTES, check_decoded_size, data_url, decode_limits, decode_still,
    encode_still, guarded, image_error, image_format,
};
use crate::image::ImageBytes;

/// Most frames of an animation that stays animated; a longer one shows its first frame.
pub const MAX_ANIMATION_FRAMES: u32 = 300;

/// Most pixels of all frames of an animation together (width × height × frames). Bounds the
/// work: every frame is a whole picture to encode, about 20 ms per megapixel for GIF frames and
/// 100 ms where the quantiser runs (more than 256 colours, as in WebP), measured in a release
/// build on an M4 (decision 0046). A larger animation shows its first frame.
pub const MAX_ANIMATION_PIXELS: u64 = 12 * 1024 * 1024;

/// Largest animated GIF written; a larger one is replaced by its first frame.
pub const MAX_ANIMATION_BYTES: usize = 16 * 1024 * 1024;

/// Quantiser speed for animation frames with more than 256 colours (1 best, 30 fastest).
/// Frames of a GIF have at most 256 colours and need no quantiser.
const GIF_SPEED: i32 = 10;

/// Most threads of one batch.
const MAX_THREADS: usize = 8;

/// Memory the images decoded at the same time may need together (estimated from their
/// headers). A single image needs at most this much (C-9), so it can always run.
const BATCH_MEMORY: u64 = MAX_PREVIEW_DECODE_BYTES;

/// Stack of a worker thread (the decoders do not recurse deeply; this is a margin).
const STACK_BYTES: usize = 4 * 1024 * 1024;

/// An image as the core encoded it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReencodedImage {
    /// `data:image/png;base64,…`, `data:image/jpeg;base64,…` or, for an animation,
    /// `data:image/gif;base64,…`; see [`is_reencoded_data_url`].
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    /// Frames written: 1 for a still picture.
    pub frames: u32,
}

/// Decodes one image of a mail and encodes it again (see the module docs). A panic of a decoder
/// on hostile input is [`PreviewError::Damaged`].
pub fn reencode_image(bytes: &[u8]) -> Result<ReencodedImage, PreviewError> {
    let limit = usize::try_from(MAX_PREVIEW_INPUT_BYTES).unwrap_or(usize::MAX);
    if bytes.len() > limit {
        return Err(PreviewError::TooLarge(format!(
            "{} bytes, at most {MAX_PREVIEW_INPUT_BYTES}",
            bytes.len()
        )));
    }
    guarded(|| match image_format(bytes)? {
        ImageFormat::Gif => gif(bytes),
        ImageFormat::WebP => webp(bytes),
        _ => still(decode_still(bytes)?),
    })
}

/// [`reencode_image`] for a batch, on up to eight of the machine's cores. The images that are
/// decoded at the same time need at most [`MAX_PREVIEW_DECODE_BYTES`] together (estimated from
/// their headers); the largest start first. The results are in the order of `images`.
///
/// [`MAX_PREVIEW_DECODE_BYTES`]: crate::attachment_preview::MAX_PREVIEW_DECODE_BYTES
pub fn reencode_images<B: AsRef<[u8]> + Sync>(
    images: &[B],
) -> Vec<Result<ReencodedImage, PreviewError>> {
    let threads = std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(MAX_THREADS)
        .min(images.len());
    if threads <= 1 {
        return images.iter().map(|b| reencode_image(b.as_ref())).collect();
    }
    let mut order: Vec<(usize, u64)> = images
        .iter()
        .enumerate()
        .map(|(index, bytes)| (index, memory_estimate(bytes.as_ref())))
        .collect();
    order.sort_by_key(|&(_, cost)| std::cmp::Reverse(cost));
    let results: Vec<Mutex<Option<Result<ReencodedImage, PreviewError>>>> =
        images.iter().map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    let budget = Budget::new(BATCH_MEMORY);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let worker = || {
                while let Some(&(index, cost)) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let _held = budget.take(cost);
                    let result = images
                        .get(index)
                        .map(|bytes| reencode_image(bytes.as_ref()));
                    if let (Some(slot), Some(result)) = (results.get(index), result) {
                        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
                    }
                }
            };
            let spawned = std::thread::Builder::new()
                .name("reencode".into())
                .stack_size(STACK_BYTES)
                .spawn_scoped(scope, worker);
            if spawned.is_err() {
                // The threads that run take all images; none at all: below, one by one.
                break;
            }
        }
    });
    results
        .into_iter()
        .zip(images)
        .map(|(slot, bytes)| {
            slot.into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .unwrap_or_else(|| reencode_image(bytes.as_ref()))
        })
        .collect()
}

/// Memory an image needs while it is re-encoded, from its header: the decoded picture, a copy
/// (upright, converted or the previous frame) and the scaled picture. 0 when the header does not
/// read (it fails at once).
fn memory_estimate(bytes: &[u8]) -> u64 {
    let Ok(format) = image_format(bytes) else {
        return 0;
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(decode_limits());
    reader.into_decoder().map_or(0, |decoder| {
        let (width, height) = decoder.dimensions();
        (u64::from(width) * u64::from(height) * 4)
            .saturating_mul(3)
            .min(BATCH_MEMORY)
    })
}

/// Memory shared by the threads of a batch.
struct Budget {
    free: Mutex<u64>,
    freed: Condvar,
    total: u64,
}

/// Memory taken from a [`Budget`] until dropped.
struct Held<'a> {
    budget: &'a Budget,
    amount: u64,
}

impl Budget {
    fn new(total: u64) -> Self {
        Self {
            free: Mutex::new(total),
            freed: Condvar::new(),
            total,
        }
    }

    /// Waits until `amount` (at most the whole budget) is free and takes it.
    fn take(&self, amount: u64) -> Held<'_> {
        let amount = amount.min(self.total);
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        while *free < amount {
            free = self
                .freed
                .wait(free)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *free -= amount;
        Held {
            budget: self,
            amount,
        }
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let mut free = self
            .budget
            .free
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *free += self.amount;
        self.budget.freed.notify_all();
    }
}

// ---------------------------------------------------------------------------------------------
// Still pictures and animations

fn still(picture: DynamicImage) -> Result<ReencodedImage, PreviewError> {
    let still = encode_still(picture)?;
    Ok(ReencodedImage {
        data_url: still.data_url,
        width: still.width,
        height: still.height,
        frames: 1,
    })
}

fn gif(bytes: &[u8]) -> Result<ReencodedImage, PreviewError> {
    let mut decoder = GifDecoder::new(Cursor::new(bytes)).map_err(image_error)?;
    decoder.set_limits(decode_limits()).map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    check_decoded_size(width, height, decoder.total_bytes(), decoder.color_type())?;
    // Without a loop extension a browser plays a GIF once (`image` reads that as "forever").
    let loops = bytes
        .windows(11)
        .any(|w| w == b"NETSCAPE2.0" || w == b"ANIMEXTS1.0");
    let repeat = loops.then(|| repeat_of(decoder.loop_count()));
    animation(decoder.into_frames(), width, height, repeat)
}

fn webp(bytes: &[u8]) -> Result<ReencodedImage, PreviewError> {
    let mut decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(image_error)?;
    if !decoder.has_animation() {
        return still(decode_still(bytes)?);
    }
    decoder.set_limits(decode_limits()).map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    // Every frame is decoded as a whole RGBA canvas.
    check_decoded_size(
        width,
        height,
        u64::from(width) * u64::from(height) * 4,
        "an animation",
    )?;
    let repeat = Some(repeat_of(decoder.loop_count()));
    animation(decoder.into_frames(), width, height, repeat)
}

fn repeat_of(count: LoopCount) -> Repeat {
    match count {
        LoopCount::Infinite => Repeat::Infinite,
        LoopCount::Finite(n) => Repeat::Finite(u16::try_from(n.get()).unwrap_or(u16::MAX)),
    }
}

/// The frames of a GIF or WebP (each a whole canvas) as an animated GIF, or the first frame as a
/// still picture: when there is only one, when the animation is over the limits, or when it
/// breaks off after the first frame (a later broken frame ends the animation there, as in a
/// browser). `repeat`: the loop count to write, `None` for none (played once).
fn animation(
    frames: Frames<'_>,
    width: u32,
    height: u32,
    repeat: Option<Repeat>,
) -> Result<ReencodedImage, PreviewError> {
    let mut frames = frames;
    let first = match frames.next() {
        Some(Ok(frame)) => frame,
        Some(Err(err)) => return Err(image_error(err)),
        None => return Err(PreviewError::Damaged("the image has no frames".into())),
    };
    let still_of = |frame: Frame| still(DynamicImage::ImageRgba8(frame.into_buffer()));
    let second = match frames.next() {
        Some(Ok(frame)) => frame,
        _ => return still_of(first),
    };
    let canvas = u64::from(width) * u64::from(height);
    if width > MAX_PREVIEW_EDGE || height > MAX_PREVIEW_EDGE || canvas == 0 {
        return still_of(first);
    }
    let kept = Frame::from_parts(first.buffer().clone(), 0, 0, first.delay());
    let mut out = Capped::new(MAX_ANIMATION_BYTES);
    let mut count: u32 = 0;
    let written = {
        let mut encoder = GifEncoder::new_with_speed(&mut out, GIF_SPEED);
        let mut result = match repeat {
            Some(repeat) => encoder.set_repeat(repeat).map_err(image_error),
            None => Ok(()),
        };
        for frame in [Ok(first), Ok(second)].into_iter().chain(frames) {
            if result.is_err() {
                break;
            }
            let frame = match frame {
                Ok(frame) => frame,
                // A broken frame after the first two ends the animation there.
                Err(_) => break,
            };
            count += 1;
            if count > MAX_ANIMATION_FRAMES || u64::from(count) * canvas > MAX_ANIMATION_PIXELS {
                result = Err(PreviewError::TooLarge("the animation is too long".into()));
                break;
            }
            // Every frame is a whole canvas (the decoders compose them); any other size is not
            // an animation this code wrote.
            if frame.buffer().dimensions() != (width, height) {
                result = Err(PreviewError::Damaged("a frame has another size".into()));
                break;
            }
            result = encoder.encode_frame(frame).map_err(image_error);
        }
        result
    };
    // The encoder wrote the trailer when it was dropped; a full buffer refused it.
    match (written, out.into_inner()) {
        (Ok(()), Some(bytes)) if count >= 2 => Ok(ReencodedImage {
            data_url: data_url("image/gif", &bytes),
            width,
            height,
            frames: count,
        }),
        _ => still_of(kept),
    }
}

/// A buffer that refuses to grow past its limit.
struct Capped {
    bytes: Vec<u8>,
    limit: usize,
    full: bool,
}

impl Capped {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            full: false,
        }
    }

    /// The bytes, `None` if a write went past the limit.
    fn into_inner(self) -> Option<Vec<u8>> {
        (!self.full).then_some(self.bytes)
    }
}

impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.full || self.bytes.len() + buf.len() > self.limit {
            self.full = true;
            return Err(std::io::Error::other("the animation is too large"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// What the renderer gets

/// Whether `url` is what this module writes: `data:image/png`, `data:image/jpeg` or
/// `data:image/gif`, base64, nothing else. The renderer takes no other image URL, so a reply of
/// the helper process can never put anything else into a document.
pub fn is_reencoded_data_url(url: &str) -> bool {
    let Some(payload) = [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/gif;base64,",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix)) else {
        return false;
    };
    let data = payload.trim_end_matches('=');
    !payload.is_empty()
        && payload.len() % 4 == 0
        && payload.len() - data.len() <= 2
        && data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// Re-encoded images for [`crate::RenderOptions::images`], by their original bytes.
///
/// An image whose bytes are here with a data URL is shown as that URL; one recorded as refused,
/// or not here at all, is not shown (the element stays without a source, as for any image that
/// cannot be shown), and one that is not here is listed in
/// [`crate::RenderedHtml::pending_images`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReencodedImages {
    by_original: HashMap<ImageBytes, Option<Arc<str>>>,
}

impl ReencodedImages {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the re-encoding of `original`. A `data_url` that [`is_reencoded_data_url`]
    /// refuses is recorded as a refusal; returns whether it was taken.
    pub fn insert(&mut self, original: ImageBytes, data_url: Arc<str>) -> bool {
        let ok = is_reencoded_data_url(&data_url);
        self.by_original.insert(original, ok.then_some(data_url));
        ok
    }

    /// Records that `original` could not be re-encoded: it is not shown.
    pub fn refuse(&mut self, original: ImageBytes) {
        self.by_original.insert(original, None);
    }

    /// Takes over the entries of `other`.
    pub fn extend(&mut self, other: Self) {
        self.by_original.extend(other.by_original);
    }

    /// The data URL recorded for `original`; `None` when it was refused or is not here.
    pub fn get(&self, original: &[u8]) -> Option<&str> {
        self.by_original.get(original).and_then(Option::as_deref)
    }

    /// `Some(Some(url))` re-encoded, `Some(None)` refused, `None` unknown.
    pub(crate) fn lookup(&self, original: &[u8]) -> Option<Option<&Arc<str>>> {
        self.by_original.get(original).map(Option::as_ref)
    }

    /// Images recorded (re-encoded or refused).
    pub fn len(&self) -> usize {
        self.by_original.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_original.is_empty()
    }
}

#[cfg(test)]
mod tests;
