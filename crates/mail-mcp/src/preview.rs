//! Attachment previews in a child process (finding C-2 of the stage 15 review, decision 0043).
//!
//! The decoders behind a preview (hayro for PDF, `image` for pictures) have no limits of their
//! own on memory and time: a PDF of 2 KB can expand to a gigabyte, and running out of memory
//! ends a process without a chance to recover. So the app does not decode these files itself.
//! It starts this helper as `mail-mcp --render-preview <kind> <page>`, writes the file to its
//! stdin and reads one JSON reply from its stdout (the PNG or JPEG as a data URL, or why there is
//! no preview). The parent ([`render_isolated`]) watches the child from outside:
//!
//! - **time:** after [`PreviewLimits::time`] the child is killed;
//! - **memory:** every [`PreviewLimits::poll`] the parent reads the child's resident memory
//!   (`/bin/ps -o rss=` on macOS and Linux, `tasklist` on Windows) and kills it above
//!   [`PreviewLimits::memory_bytes`]; between two looks a fast-growing child can pass the
//!   budget by what it allocates in that time (tens of MB);
//! - a killed child, a crash or a reply that is not one gives a typed [`PreviewError`]; the app
//!   goes on.
//!
//! The same helper re-encodes the images a mail shows (finding C-4, decision 0046):
//! `mail-mcp --reencode-images` reads a batch of images from stdin (each a 4-byte little-endian
//! length and the bytes), encodes each again with [`mail_content::reencode_images`] (in
//! parallel) and writes one JSON reply ([`reencode_isolated`] in the parent), under the same
//! watchdog. The app's process never decodes an image of a mail, and the web view's decoders see
//! only what the helper wrote.
//!
//! Stage 17 (decision 0050): a picture the user chose for a contact is decoded and encoded
//! here as well, `mail-mcp --contact-picture source` (the whole image for the cropper) or
//! `mail-mcp --contact-picture crop <x> <y> <size>` (the stored square), each reading one
//! file from stdin and writing one JSON reply ([`contact_picture_isolated`] in the parent).
//!
//! Nothing here uses `unsafe` or a new dependency. The Windows path (`tasklist`, `mail-mcp.exe`)
//! is written for Windows but has not run there.

use std::path::Path;
use std::process::{ExitCode, Stdio};
use std::time::{Duration, Instant};

use mail_content::{ImageBytes, ReencodedImage};
use mail_model::{AttachmentPreview, MAX_PREVIEW_INPUT_BYTES, PreviewError, PreviewKind};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// First argument of the preview mode.
pub const PREVIEW_ARG: &str = "--render-preview";

/// Only argument of the image mode (C-4).
pub const REENCODE_ARG: &str = "--reencode-images";

/// First argument of the contact picture mode (stage 17).
pub const PICTURE_ARG: &str = "--contact-picture";

/// Largest JPEG a contact picture step may answer (the cropper's source: 1024 px per side).
const MAX_PICTURE_REPLY_BYTES: usize = 4 * 1024 * 1024;

/// Largest reply read from the child: a data URL of a page at most 2048 px per side, or the
/// data URLs of a batch of images.
const MAX_REPLY_BYTES: u64 = 96 * 1024 * 1024;

/// Most images in one batch of the image mode.
pub const MAX_BATCH_IMAGES: usize = 512;

/// Most bytes of images in one batch (two renderings' worth of the renderer's 32 MiB budget:
/// inline images and the external ones the user allowed).
pub const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

/// Most bytes of data URLs in one reply; images past it are refused as too large.
const MAX_BATCH_REPLY_BYTES: usize = 80 * 1024 * 1024;

/// Limits of one preview child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewLimits {
    /// Resident memory above which the child is killed.
    pub memory_bytes: u64,
    /// Wall-clock time after which the child is killed.
    pub time: Duration,
    /// How often the parent looks at the child's memory and the clock.
    pub poll: Duration,
}

impl Default for PreviewLimits {
    /// 512 MiB and 20 s. An image preview at its pixel limit needs about 400 MiB; a PDF page
    /// renders in well under a second.
    fn default() -> Self {
        Self {
            memory_bytes: 512 * 1024 * 1024,
            time: Duration::from_secs(20),
            poll: Duration::from_millis(50),
        }
    }
}

/// The child's one line of output.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Reply {
    Ok { preview: AttachmentPreview },
    Unsupported { detail: String },
    TooLarge { detail: String },
    Damaged { detail: String },
}

fn kind_name(kind: PreviewKind) -> &'static str {
    match kind {
        PreviewKind::Image => "image",
        PreviewKind::Pdf => "pdf",
        PreviewKind::Text => "text",
    }
}

fn kind_from(name: &str) -> Option<PreviewKind> {
    match name {
        "image" => Some(PreviewKind::Image),
        "pdf" => Some(PreviewKind::Pdf),
        "text" => Some(PreviewKind::Text),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Child

/// The preview mode: `args` are the arguments after [`PREVIEW_ARG`] (`<kind> <page>`). Reads the
/// file from stdin, renders it in this process and writes one JSON line to stdout.
pub fn run_child(args: &[String]) -> ExitCode {
    let (Some(kind), Some(page)) = (
        args.first().and_then(|a| kind_from(a)),
        args.get(1).and_then(|p| p.parse::<u32>().ok()),
    ) else {
        eprintln!("mail-mcp: usage: {PREVIEW_ARG} <image|pdf|text> <page>");
        return ExitCode::from(2);
    };
    let mut bytes = Vec::new();
    let limit = u64::from(MAX_PREVIEW_INPUT_BYTES);
    if let Err(err) = std::io::Read::read_to_end(
        &mut std::io::Read::take(std::io::stdin().lock(), limit + 1),
        &mut bytes,
    ) {
        eprintln!("mail-mcp: reading the file failed: {err}");
        return ExitCode::FAILURE;
    }
    let reply = match mail_content::render_preview(kind, &bytes, page) {
        Ok(preview) => Reply::Ok { preview },
        Err(PreviewError::Unsupported(detail)) => Reply::Unsupported { detail },
        Err(PreviewError::TooLarge(detail)) => Reply::TooLarge { detail },
        Err(PreviewError::Damaged(detail)) => Reply::Damaged { detail },
    };
    let mut out = std::io::stdout().lock();
    match serde_json::to_writer(&mut out, &reply) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mail-mcp: writing the preview failed: {err}");
            ExitCode::FAILURE
        }
    }
}

/// One image of the image mode's reply.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ImageReply {
    Ok {
        data_url: String,
        width: u32,
        height: u32,
        frames: u32,
    },
    Unsupported {
        detail: String,
    },
    TooLarge {
        detail: String,
    },
    Damaged {
        detail: String,
    },
}

/// The image mode's reply: one entry per image, in the order of the request.
#[derive(Debug, Serialize, Deserialize)]
struct BatchReply {
    images: Vec<ImageReply>,
}

/// A batch as the image mode reads it: per image a 4-byte little-endian length and the bytes.
fn frame_batch(images: &[ImageBytes]) -> Vec<u8> {
    let mut out = Vec::with_capacity(images.iter().map(|i| i.len() + 4).sum());
    for image in images {
        out.extend_from_slice(&u32::try_from(image.len()).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(image);
    }
    out
}

/// The images of a batch, `None` if it is not one (cut short, too many, too large).
fn unframe_batch(mut input: &[u8]) -> Option<Vec<&[u8]>> {
    let mut images = Vec::new();
    while !input.is_empty() {
        let (length, rest) = input.split_first_chunk::<4>()?;
        let length = usize::try_from(u32::from_le_bytes(*length)).ok()?;
        if length > rest.len() || images.len() >= MAX_BATCH_IMAGES {
            return None;
        }
        let (image, rest) = rest.split_at(length);
        images.push(image);
        input = rest;
    }
    Some(images)
}

/// The image mode (no further arguments): reads a batch from stdin, re-encodes every image and
/// writes one JSON reply to stdout. A request that is not a batch ends with exit code 2.
pub fn run_reencode_child() -> ExitCode {
    let mut input = Vec::new();
    let limit = (MAX_BATCH_BYTES + 4 * MAX_BATCH_IMAGES) as u64;
    if let Err(err) = std::io::Read::read_to_end(
        &mut std::io::Read::take(std::io::stdin().lock(), limit + 1),
        &mut input,
    ) {
        eprintln!("mail-mcp: reading the images failed: {err}");
        return ExitCode::FAILURE;
    }
    let Some(images) = unframe_batch(&input).filter(|_| input.len() as u64 <= limit) else {
        eprintln!(
            "mail-mcp: {REENCODE_ARG} reads images as <u32 length><bytes>, at most {MAX_BATCH_IMAGES}"
        );
        return ExitCode::from(2);
    };
    let mut written = 0usize;
    let replies = mail_content::reencode_images(&images)
        .into_iter()
        .map(|result| match result {
            Ok(image) if written + image.data_url.len() > MAX_BATCH_REPLY_BYTES => {
                ImageReply::TooLarge {
                    detail: "the images of this message are too large together".into(),
                }
            }
            Ok(image) => {
                written += image.data_url.len();
                ImageReply::Ok {
                    data_url: image.data_url,
                    width: image.width,
                    height: image.height,
                    frames: image.frames,
                }
            }
            Err(PreviewError::Unsupported(detail)) => ImageReply::Unsupported { detail },
            Err(PreviewError::TooLarge(detail)) => ImageReply::TooLarge { detail },
            Err(PreviewError::Damaged(detail)) => ImageReply::Damaged { detail },
        })
        .collect();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    match serde_json::to_writer(&mut out, &BatchReply { images: replies })
        .map_err(std::io::Error::from)
        .and_then(|()| std::io::Write::flush(&mut out))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mail-mcp: writing the images failed: {err}");
            ExitCode::FAILURE
        }
    }
}

/// A step of the contact picture mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureStep {
    /// The whole image for the cropper ([`mail_content::picture_source`]).
    Source,
    /// The stored square ([`mail_content::picture_crop`]).
    Crop(mail_model::PictureCrop),
}

impl PictureStep {
    fn args(self) -> Vec<String> {
        match self {
            Self::Source => vec![PICTURE_ARG.to_owned(), "source".to_owned()],
            Self::Crop(crop) => vec![
                PICTURE_ARG.to_owned(),
                "crop".to_owned(),
                crop.x.to_string(),
                crop.y.to_string(),
                crop.size.to_string(),
            ],
        }
    }

    fn parse(args: &[String]) -> Option<Self> {
        match args {
            [step] if step == "source" => Some(Self::Source),
            [step, x, y, size] if step == "crop" => Some(Self::Crop(mail_model::PictureCrop {
                x: x.parse().ok()?,
                y: y.parse().ok()?,
                size: size.parse().ok()?,
            })),
            _ => None,
        }
    }

    /// Runs the step in this process.
    pub fn run(self, bytes: &[u8]) -> Result<mail_content::PictureJpeg, PreviewError> {
        match self {
            Self::Source => mail_content::picture_source(bytes),
            Self::Crop(crop) => mail_content::picture_crop(bytes, crop),
        }
    }
}

/// The contact picture mode's reply.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum PictureReply {
    Ok {
        /// `data:image/jpeg;base64,…`.
        data_url: String,
        width: u32,
        height: u32,
    },
    Unsupported {
        detail: String,
    },
    TooLarge {
        detail: String,
    },
    Damaged {
        detail: String,
    },
}

/// The contact picture mode: `args` are the arguments after [`PICTURE_ARG`] (`source`, or
/// `crop <x> <y> <size>`). Reads the image from stdin and writes one JSON reply to stdout.
pub fn run_picture_child(args: &[String]) -> ExitCode {
    let Some(step) = PictureStep::parse(args) else {
        eprintln!("mail-mcp: usage: {PICTURE_ARG} source | {PICTURE_ARG} crop <x> <y> <size>");
        return ExitCode::from(2);
    };
    let mut bytes = Vec::new();
    let limit = u64::from(mail_model::MAX_CONTACT_PICTURE_FILE_BYTES);
    if let Err(err) = std::io::Read::read_to_end(
        &mut std::io::Read::take(std::io::stdin().lock(), limit + 1),
        &mut bytes,
    ) {
        eprintln!("mail-mcp: reading the picture failed: {err}");
        return ExitCode::FAILURE;
    }
    let reply = match step.run(&bytes) {
        Ok(picture) => {
            let mut data_url = String::from("data:image/jpeg;base64,");
            data_url.push_str(&base64(&picture.jpeg));
            PictureReply::Ok {
                data_url,
                width: picture.width,
                height: picture.height,
            }
        }
        Err(PreviewError::Unsupported(detail)) => PictureReply::Unsupported { detail },
        Err(PreviewError::TooLarge(detail)) => PictureReply::TooLarge { detail },
        Err(PreviewError::Damaged(detail)) => PictureReply::Damaged { detail },
    };
    let mut out = std::io::stdout().lock();
    match serde_json::to_writer(&mut out, &reply) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mail-mcp: writing the picture failed: {err}");
            ExitCode::FAILURE
        }
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(BASE64[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

/// The JPEG inside a reply's data URL, `None` unless it is exactly
/// `data:image/jpeg;base64,<strict base64 of a JPEG>`.
fn jpeg_of(data_url: &str) -> Option<Vec<u8>> {
    let text = data_url.strip_prefix("data:image/jpeg;base64,")?;
    if !text.len().is_multiple_of(4) || text.len() / 4 * 3 > MAX_PICTURE_REPLY_BYTES {
        return None;
    }
    let value = |c: u8| BASE64.iter().position(|a| *a == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let chunks = text.as_bytes().chunks(4);
    let count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        let padding = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != count) {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk[..4 - padding].iter().enumerate() {
            n |= value(*c)? << (18 - 6 * i);
        }
        for i in 0..3 - padding {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    mail_content::is_jpeg(&out).then_some(out)
}

// ---------------------------------------------------------------------------------------------
// Parent

/// Runs a contact picture step in the helper at `helper` within `limits` (stage 17). The
/// result is the helper's JPEG, checked to be one (and for a crop, within the stored size).
///
/// # Errors
///
/// What the step refused, or as [`render_isolated`] when the helper could not start, was
/// stopped or answered something else.
pub async fn contact_picture_isolated(
    helper: &Path,
    bytes: Vec<u8>,
    step: PictureStep,
    limits: PreviewLimits,
) -> Result<mail_content::PictureJpeg, PreviewError> {
    let reply = run_watched(helper, &step.args(), bytes, limits).await?;
    match serde_json::from_slice::<PictureReply>(&reply) {
        Ok(PictureReply::Ok {
            data_url,
            width,
            height,
        }) => {
            let jpeg = jpeg_of(&data_url).ok_or_else(|| {
                PreviewError::Damaged("the image program answered something else".into())
            })?;
            let fits = match step {
                PictureStep::Source => {
                    width <= mail_model::CONTACT_PICTURE_SOURCE_SIZE
                        && height <= mail_model::CONTACT_PICTURE_SOURCE_SIZE
                }
                PictureStep::Crop(_) => {
                    jpeg.len() <= mail_model::MAX_CONTACT_PICTURE_BYTES
                        && width == mail_model::CONTACT_PICTURE_SIZE
                        && height == mail_model::CONTACT_PICTURE_SIZE
                }
            };
            if !fits || width == 0 || height == 0 {
                return Err(PreviewError::Damaged(
                    "the image program answered something else".into(),
                ));
            }
            Ok(mail_content::PictureJpeg {
                jpeg,
                width,
                height,
            })
        }
        Ok(PictureReply::Unsupported { detail }) => Err(PreviewError::Unsupported(detail)),
        Ok(PictureReply::TooLarge { detail }) => Err(PreviewError::TooLarge(detail)),
        Ok(PictureReply::Damaged { detail }) => Err(PreviewError::Damaged(detail)),
        Err(_) => Err(ended_unexpectedly()),
    }
}

/// Renders a preview in the helper at `helper` within `limits` (see the module docs).
pub async fn render_isolated(
    helper: &Path,
    kind: PreviewKind,
    bytes: Vec<u8>,
    page: u32,
    limits: PreviewLimits,
) -> Result<AttachmentPreview, PreviewError> {
    let args = [
        PREVIEW_ARG.to_owned(),
        kind_name(kind).to_owned(),
        page.to_string(),
    ];
    let reply = run_watched(helper, &args, bytes, limits).await?;
    match serde_json::from_slice::<Reply>(&reply) {
        Ok(Reply::Ok { preview }) => Ok(preview),
        Ok(Reply::Unsupported { detail }) => Err(PreviewError::Unsupported(detail)),
        Ok(Reply::TooLarge { detail }) => Err(PreviewError::TooLarge(detail)),
        Ok(Reply::Damaged { detail }) => Err(PreviewError::Damaged(detail)),
        Err(_) => Err(ended_unexpectedly()),
    }
}

/// Re-encodes the images of a mail in the helper at `helper` within `limits` (C-4): one child
/// per batch of at most [`MAX_BATCH_IMAGES`] images and [`MAX_BATCH_BYTES`] bytes (one batch
/// for any mail the renderer shows). The results are in the order of `images`; a data URL that
/// is not what the re-encoder writes is refused as damaged.
///
/// # Errors
///
/// When a batch fails as a whole: the helper could not start
/// ([`PreviewError::Unsupported`]), was stopped over its time or memory limit
/// ([`PreviewError::TooLarge`]), crashed or answered something else
/// ([`PreviewError::Damaged`]).
pub async fn reencode_isolated(
    helper: &Path,
    images: &[ImageBytes],
    limits: PreviewLimits,
) -> Result<Vec<Result<ReencodedImage, PreviewError>>, PreviewError> {
    let mut results = Vec::with_capacity(images.len());
    let mut rest = images;
    while !rest.is_empty() {
        let mut count = 0;
        let mut bytes = 0;
        while let Some(image) = rest.get(count)
            && count < MAX_BATCH_IMAGES
            && (count == 0 || bytes + image.len() <= MAX_BATCH_BYTES)
        {
            bytes += image.len();
            count += 1;
        }
        let (batch, later) = rest.split_at(count);
        rest = later;
        if bytes > MAX_BATCH_BYTES {
            // One image larger than a whole batch: the re-encoder refuses it anyway.
            results.push(Err(PreviewError::TooLarge(format!(
                "{bytes} bytes, at most {MAX_BATCH_BYTES}"
            ))));
            continue;
        }
        let reply = run_watched(
            helper,
            &[REENCODE_ARG.to_owned()],
            frame_batch(batch),
            limits,
        )
        .await?;
        let BatchReply { images: replies } =
            serde_json::from_slice(&reply).map_err(|_| ended_unexpectedly())?;
        if replies.len() != batch.len() {
            return Err(ended_unexpectedly());
        }
        results.extend(replies.into_iter().map(|reply| match reply {
            ImageReply::Ok {
                data_url,
                width,
                height,
                frames,
            } if mail_content::is_reencoded_data_url(&data_url) => Ok(ReencodedImage {
                data_url,
                width,
                height,
                frames,
            }),
            ImageReply::Ok { .. } => Err(PreviewError::Damaged(
                "the image program answered something else".into(),
            )),
            ImageReply::Unsupported { detail } => Err(PreviewError::Unsupported(detail)),
            ImageReply::TooLarge { detail } => Err(PreviewError::TooLarge(detail)),
            ImageReply::Damaged { detail } => Err(PreviewError::Damaged(detail)),
        }));
    }
    Ok(results)
}

fn ended_unexpectedly() -> PreviewError {
    PreviewError::Damaged("the preview program ended unexpectedly".into())
}

/// Starts `helper` with `args`, writes `input` to its stdin and returns its stdout once it ends
/// successfully, within `limits` (see the module docs).
async fn run_watched(
    helper: &Path,
    args: &[String],
    bytes: Vec<u8>,
    limits: PreviewLimits,
) -> Result<Vec<u8>, PreviewError> {
    let mut child = tokio::process::Command::new(helper)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| {
            PreviewError::Unsupported(format!("the preview program could not start: {err}"))
        })?;
    let pid = child.id();
    let started = Instant::now();
    // Feeding and reading run beside the watchdog; a child that stops reading or writing
    // cannot block the parent past the time limit.
    let writer = child.stdin.take().map(|mut stdin| {
        tokio::spawn(async move {
            // A child that ends early closes the pipe; the exit status tells why.
            let _ = stdin.write_all(&bytes).await;
        })
    });
    let reader = child.stdout.take().map(|stdout| {
        tokio::spawn(async move {
            let mut reply = Vec::new();
            let _ = stdout.take(MAX_REPLY_BYTES).read_to_end(&mut reply).await;
            reply
        })
    });
    let stop = |reason: String| PreviewError::TooLarge(reason);
    let outcome = loop {
        tokio::select! {
            status = child.wait() => break Ok(status),
            () = tokio::time::sleep(limits.poll) => {
                if started.elapsed() > limits.time {
                    break Err(stop(format!(
                        "the preview took longer than {} s and was stopped",
                        limits.time.as_secs_f32()
                    )));
                }
                if let Some(pid) = pid
                    && let Some(used) = resident_bytes(pid).await
                    && used > limits.memory_bytes
                {
                    break Err(stop(format!(
                        "the preview needed more than {} MiB and was stopped",
                        limits.memory_bytes / (1024 * 1024)
                    )));
                }
            }
        }
    };
    let status = match outcome {
        Ok(status) => status,
        Err(err) => {
            let _ = child.kill().await;
            if let Some(writer) = writer {
                writer.abort();
            }
            if let Some(reader) = reader {
                reader.abort();
            }
            tracing::info!(error = %err, "preview child stopped");
            return Err(err);
        }
    };
    if let Some(writer) = writer {
        let _ = writer.await;
    }
    let reply = match reader {
        Some(reader) => reader.await.unwrap_or_default(),
        None => Vec::new(),
    };
    if !matches!(status, Ok(s) if s.success()) {
        tracing::info!(?status, "preview child failed");
        return Err(ended_unexpectedly());
    }
    Ok(reply)
}

/// Resident memory of process `pid` in bytes, `None` if it cannot be read (the process ended,
/// or the system tool failed; the next look tries again).
pub async fn resident_bytes(pid: u32) -> Option<u64> {
    let output = if cfg!(windows) {
        // Untested on Windows: "image","pid","session","#","12.345 K" (separators per locale).
        tokio::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?
    } else {
        tokio::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?
    };
    if !output.status.success() {
        return None;
    }
    parse_resident_kib(&String::from_utf8_lossy(&output.stdout), cfg!(windows))
        .map(|kib| kib.saturating_mul(1024))
}

/// KiB from the output of `ps -o rss=` (a number) or `tasklist /FO CSV /NH` (the last field,
/// digits with locale separators and a unit).
fn parse_resident_kib(text: &str, tasklist: bool) -> Option<u64> {
    let field = if tasklist {
        text.lines()
            .find(|line| line.starts_with('"'))?
            .rsplit("\",\"")
            .next()?
    } else {
        text.trim()
    };
    let digits: String = field.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_figures_of_ps_and_tasklist_are_read() {
        assert_eq!(parse_resident_kib("  12345\n", false), Some(12_345));
        assert_eq!(parse_resident_kib("", false), None);
        assert_eq!(
            parse_resident_kib(
                "\"mail-mcp.exe\",\"4242\",\"Console\",\"1\",\"12.345 K\"\r\n",
                true
            ),
            Some(12_345)
        );
        assert_eq!(
            parse_resident_kib(
                "\"mail-mcp.exe\",\"4242\",\"Console\",\"1\",\"1,234,567 K\"",
                true
            ),
            Some(1_234_567)
        );
        // No such process: an information line without quotes.
        assert_eq!(
            parse_resident_kib(
                "INFO: No tasks are running which match the specified criteria.",
                true
            ),
            None
        );
    }

    #[test]
    fn batches_frame_and_unframe() {
        let images: Vec<ImageBytes> = vec![
            ImageBytes::from(&b"abc"[..]),
            ImageBytes::from(&b""[..]),
            ImageBytes::from(&b"\x89PNG"[..]),
        ];
        let framed = frame_batch(&images);
        let read = unframe_batch(&framed).expect("a batch");
        assert_eq!(read, [&b"abc"[..], b"", b"\x89PNG"]);
        assert_eq!(unframe_batch(&[]), Some(Vec::new()));
        // Cut short, a length past the end, a partial length.
        assert_eq!(unframe_batch(&framed[..framed.len() - 1]), None);
        assert_eq!(unframe_batch(&[9, 0, 0, 0, 1]), None);
        assert_eq!(unframe_batch(&[1, 0]), None);
        let many = frame_batch(&vec![ImageBytes::from(&b""[..]); MAX_BATCH_IMAGES + 1]);
        assert_eq!(unframe_batch(&many), None);
        let reply = BatchReply {
            images: vec![ImageReply::Damaged { detail: "x".into() }],
        };
        assert_eq!(
            serde_json::to_string(&reply).expect("json"),
            r#"{"images":[{"outcome":"damaged","detail":"x"}]}"#
        );
    }

    #[test]
    fn replies_round_trip() {
        let reply = Reply::TooLarge { detail: "x".into() };
        let text = serde_json::to_string(&reply).expect("json");
        assert_eq!(text, r#"{"outcome":"tooLarge","detail":"x"}"#);
        assert!(matches!(
            serde_json::from_str::<Reply>(&text),
            Ok(Reply::TooLarge { .. })
        ));
    }
}
