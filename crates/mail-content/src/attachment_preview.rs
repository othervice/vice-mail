//! Attachment preview (stage 16, decision 0036): bytes of an untrusted attachment in, something
//! the web view can show without running or loading anything out.
//!
//! - **Images** are recognised by their bytes (PNG, JPEG, GIF, WebP; never SVG), decoded here in
//!   Rust within [`MAX_PREVIEW_PIXELS`], turned upright (EXIF orientation), scaled to at most
//!   [`MAX_PREVIEW_EDGE`] per side and encoded again: PNG with transparency, JPEG without. The
//!   system's image decoders never see the attachment's own bytes.
//! - **PDF** pages are rasterised here (hayro, pure Rust: no script engine, no network, no
//!   form actions) to PNG, one page per call, at most [`MAX_PREVIEW_PDF_PAGES`].
//! - **Text** is decoded (UTF-8, UTF-16 with BOM, else Latin-1), controls other than tab and
//!   line breaks removed, cut after [`MAX_PREVIEW_TEXT_BYTES`]. The UI shows it as text.
//! - HTML, XHTML, XML and SVG are never previewed: they are documents with active content.
//!
//! A panic of a decoder on hostile input becomes [`PreviewError::Damaged`].

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};
use mail_model::{
    AttachmentPreview, MAX_PREVIEW_EDGE, MAX_PREVIEW_INPUT_BYTES, MAX_PREVIEW_PDF_PAGES,
    MAX_PREVIEW_PIXELS, MAX_PREVIEW_TEXT_BYTES, PreviewError, PreviewKind,
};

use crate::image::{ImageType, base64_encode_into, sniff_image};

/// JPEG quality of re-encoded photos.
const JPEG_QUALITY: u8 = 85;

/// Most memory the decoded picture may take, counted twice (the decoded image and the copy that
/// turning it upright or converting it for the encoder makes): finding C-9 of the stage 15
/// review. Checked from the header before anything is decoded, so a 64-megapixel RGBA or a
/// 16-bit image is refused instead of using over a gigabyte; it also stays below the 512 MiB
/// the preview process may use (C-2). An 8-bit RGB photo of 64 megapixels still fits.
pub const MAX_PREVIEW_DECODE_BYTES: u64 = 384 * 1024 * 1024;

/// PDF pages are rendered at twice their size in points at most (sharp on Retina screens).
const MAX_PDF_SCALE: f32 = 2.0;

/// Types that are documents with active content: never previewed, whatever else they claim.
fn is_active_document(mime: &str, extension: &str) -> bool {
    matches!(
        mime,
        "text/html" | "application/xhtml+xml" | "image/svg+xml" | "text/xml" | "application/xml"
    ) || matches!(
        extension,
        "html" | "htm" | "xhtml" | "shtml" | "mht" | "mhtml" | "svg" | "svgz" | "xml" | "xsl"
    )
}

fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// What a preview of this attachment shows, from its sanitised name and declared type; `None`
/// when the core does not preview it. The bytes decide again when the preview is made.
pub fn preview_kind(name: &str, mime_type: &str) -> Option<PreviewKind> {
    // Parameters (`; name=…`, `; charset=…`) do not change what the type is.
    let mime = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let extension = extension_of(name);
    if is_active_document(&mime, &extension) {
        return None;
    }
    let generic = matches!(
        mime.as_str(),
        "" | "application/octet-stream" | "application/x-download" | "binary/octet-stream"
    );
    match mime.as_str() {
        "image/png" | "image/jpeg" | "image/jpg" | "image/pjpeg" | "image/gif" | "image/webp" => {
            return Some(PreviewKind::Image);
        }
        "application/pdf" | "application/x-pdf" => return Some(PreviewKind::Pdf),
        "application/json" => return Some(PreviewKind::Text),
        _ => {}
    }
    if mime.starts_with("text/") {
        return Some(PreviewKind::Text);
    }
    if !generic {
        return None;
    }
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(PreviewKind::Image),
        "pdf" => Some(PreviewKind::Pdf),
        "txt" | "text" | "log" | "csv" | "tsv" | "md" | "markdown" | "json" => {
            Some(PreviewKind::Text)
        }
        _ => None,
    }
}

/// The preview of decoded attachment bytes. `page` is the 0-based PDF page (ignored otherwise).
pub fn render_preview(
    kind: PreviewKind,
    bytes: &[u8],
    page: u32,
) -> Result<AttachmentPreview, PreviewError> {
    let limit = usize::try_from(MAX_PREVIEW_INPUT_BYTES).unwrap_or(usize::MAX);
    if bytes.len() > limit {
        return Err(PreviewError::TooLarge(format!(
            "{} bytes, at most {MAX_PREVIEW_INPUT_BYTES}",
            bytes.len()
        )));
    }
    match kind {
        PreviewKind::Text => Ok(text_preview(bytes)),
        PreviewKind::Image => guarded(|| image_preview(bytes)),
        PreviewKind::Pdf => guarded(|| pdf_preview(bytes, page)),
    }
}

/// Runs a decoder; a panic on hostile input is a damaged file, not a crash.
fn guarded(
    render: impl FnOnce() -> Result<AttachmentPreview, PreviewError>,
) -> Result<AttachmentPreview, PreviewError> {
    catch_unwind(AssertUnwindSafe(render))
        .unwrap_or_else(|_| Err(PreviewError::Damaged("the decoder failed".into())))
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4 + 32);
    out.push_str("data:");
    out.push_str(mime);
    out.push_str(";base64,");
    base64_encode_into(bytes, &mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// Images

fn image_error(err: ImageError) -> PreviewError {
    match err {
        ImageError::Limits(limit) => PreviewError::TooLarge(limit.to_string()),
        ImageError::Unsupported(what) => PreviewError::Unsupported(what.to_string()),
        other => PreviewError::Damaged(other.to_string()),
    }
}

fn image_preview(bytes: &[u8]) -> Result<AttachmentPreview, PreviewError> {
    let format = match sniff_image(bytes) {
        Some(ImageType::Png) => ImageFormat::Png,
        Some(ImageType::Jpeg) => ImageFormat::Jpeg,
        Some(ImageType::Gif) => ImageFormat::Gif,
        Some(ImageType::Webp) => ImageFormat::WebP,
        _ => {
            return Err(PreviewError::Unsupported(
                "not a PNG, JPEG, GIF or WebP image".into(),
            ));
        }
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    // RGBA8 of the largest image, and as much again for the scaled copy and the encoder.
    limits.max_alloc = Some(MAX_PREVIEW_PIXELS * 4 * 2);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PREVIEW_PIXELS {
        return Err(PreviewError::TooLarge(format!(
            "{width}×{height} pixels, at most {MAX_PREVIEW_PIXELS}"
        )));
    }
    // C-9: the decoded size from the header (pixels × bytes per pixel of its colour type).
    let decoded = decoder.total_bytes();
    if decoded.saturating_mul(2) > MAX_PREVIEW_DECODE_BYTES {
        return Err(PreviewError::TooLarge(format!(
            "{width}×{height} pixels of {:?} need {} MiB decoded, at most {} MiB",
            decoder.color_type(),
            decoded / (1024 * 1024),
            MAX_PREVIEW_DECODE_BYTES / 2 / (1024 * 1024)
        )));
    }
    let orientation = decoder.orientation().map_err(image_error)?;
    let mut picture = DynamicImage::from_decoder(decoder).map_err(image_error)?;
    picture.apply_orientation(orientation);
    let scaled = picture.width() > MAX_PREVIEW_EDGE || picture.height() > MAX_PREVIEW_EDGE;
    if scaled {
        picture = picture.resize(MAX_PREVIEW_EDGE, MAX_PREVIEW_EDGE, FilterType::Triangle);
    }
    let mut out = Vec::new();
    // Transparency only where a pixel uses it: an opaque RGBA image is a photo like any other.
    let transparent =
        picture.color().has_alpha() && picture.to_rgba8().pixels().any(|p| p[3] < 255);
    let mime = if transparent {
        picture
            .to_rgba8()
            .write_with_encoder(PngEncoder::new(&mut out))
            .map_err(image_error)?;
        "image/png"
    } else {
        picture
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
            .map_err(image_error)?;
        "image/jpeg"
    };
    Ok(AttachmentPreview::Image {
        data_url: data_url(mime, &out),
        width: picture.width(),
        height: picture.height(),
        scaled,
    })
}

// ---------------------------------------------------------------------------------------------
// PDF

fn pdf_preview(bytes: &[u8], page: u32) -> Result<AttachmentPreview, PreviewError> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::{LoadPdfError, Pdf};
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

    // The header may follow some junk, but within the first KiB (as readers accept it).
    let head = &bytes[..bytes.len().min(1024)];
    if !head.windows(5).any(|w| w == b"%PDF-") {
        return Err(PreviewError::Unsupported("not a PDF document".into()));
    }
    let pdf = Pdf::new(bytes.to_vec()).map_err(|err| match err {
        LoadPdfError::Decryption(_) => {
            PreviewError::Damaged("the document is encrypted or protected".into())
        }
        LoadPdfError::Invalid => PreviewError::Damaged("the document could not be read".into()),
    })?;
    let pages = pdf.pages();
    let total = u32::try_from(pages.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return Err(PreviewError::Damaged("the document has no pages".into()));
    }
    let shown = total.min(MAX_PREVIEW_PDF_PAGES);
    if page >= shown {
        return Err(PreviewError::Damaged(format!(
            "page {} of {shown} requested",
            u64::from(page) + 1
        )));
    }
    let Some(current) = usize::try_from(page).ok().and_then(|i| pages.get(i)) else {
        return Err(PreviewError::Damaged("the page is missing".into()));
    };
    let (width, height) = current.render_dimensions();
    if !(width.is_finite() && height.is_finite()) || width < 1.0 || height < 1.0 {
        return Err(PreviewError::Damaged("the page has no size".into()));
    }
    #[allow(clippy::cast_precision_loss)] // 2048 is exact in f32
    let edge = MAX_PREVIEW_EDGE as f32;
    let scale = (edge / width.max(height)).min(MAX_PDF_SCALE);
    let pixmap = render(
        current,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    let (out_width, out_height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    if out_width == 0 || out_height == 0 {
        return Err(PreviewError::Damaged("the page rendered empty".into()));
    }
    let png = pixmap
        .into_png()
        .map_err(|err| PreviewError::Damaged(format!("encode the page: {err}")))?;
    Ok(AttachmentPreview::Pdf {
        data_url: data_url("image/png", &png),
        width: out_width,
        height: out_height,
        page,
        pages: shown,
        more_pages: total > shown,
    })
}

// ---------------------------------------------------------------------------------------------
// Text

fn text_preview(bytes: &[u8]) -> AttachmentPreview {
    let decoded = decode_text(bytes);
    let limit = usize::try_from(MAX_PREVIEW_TEXT_BYTES).unwrap_or(usize::MAX);
    let mut text = String::with_capacity(decoded.len().min(limit));
    let mut truncated = false;
    let mut chars = decoded.chars().peekable();
    while let Some(c) = chars.next() {
        let c = match c {
            // CRLF is one break, a lone CR (old Mac files) becomes one.
            '\r' if chars.peek() == Some(&'\n') => continue,
            '\r' => '\n',
            '\n' | '\t' => c,
            c if c.is_control() => continue,
            c => c,
        };
        if text.len() + c.len_utf8() > limit {
            truncated = true;
            break;
        }
        text.push(c);
    }
    AttachmentPreview::Text { text, truncated }
}

/// UTF-8 (BOM removed), UTF-16 with a BOM, else Latin-1 (every byte is a character).
fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], big: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if big {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return utf16(rest, false);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return utf16(rest, true);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

#[cfg(test)]
mod tests;
