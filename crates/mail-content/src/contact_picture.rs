//! Contact pictures (stage 17, decision 0050): an image file the user chose for a contact,
//! decoded like an attachment preview and encoded again as JPEG by the core. The bytes of the
//! file never reach the web view's decoders. Both steps are meant to run in the helper process
//! (`mail-mcp --contact-picture`, decision 0046), under its time and memory watchdog:
//!
//! - [`picture_source`]: the whole image, upright, at most [`CONTACT_PICTURE_SOURCE_SIZE`] px per
//!   side, for the cropper in the UI.
//! - [`picture_crop`]: the square the user cut out of that source, [`CONTACT_PICTURE_SIZE`] px
//!   per side, as a JPEG of at most [`MAX_CONTACT_PICTURE_BYTES`] (the quality is lowered step
//!   by step until it fits).
//!
//! Only PNG, JPEG, GIF and WebP are read (recognised by their bytes, never SVG); an animation
//! gives its first frame; transparent pixels are laid on white. The decoder limits are the
//! preview's (pixels, decoded bytes, finding C-9) and a file larger than
//! [`MAX_CONTACT_PICTURE_FILE_BYTES`] is refused before decoding. A decoder panic is a damaged
//! picture.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, Rgb, RgbImage};
use mail_model::{
    CONTACT_PICTURE_SIZE, CONTACT_PICTURE_SOURCE_SIZE, MAX_CONTACT_PICTURE_BYTES,
    MAX_CONTACT_PICTURE_FILE_BYTES, PictureCrop, PreviewError,
};

use crate::attachment_preview::{decode_still, guarded, image_error};

/// Quality of the source shown in the cropper.
const SOURCE_QUALITY: u8 = 85;

/// Qualities tried for the stored picture, best first.
const PICTURE_QUALITIES: [u8; 5] = [85, 75, 65, 50, 35];

/// A JPEG the core wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureJpeg {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Whether `bytes` start like a JPEG (SOI and the first marker). Every stored and synchronised
/// contact picture is one.
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}

fn check_file_size(bytes: &[u8]) -> Result<(), PreviewError> {
    let limit = usize::try_from(MAX_CONTACT_PICTURE_FILE_BYTES).unwrap_or(usize::MAX);
    if bytes.len() > limit {
        return Err(PreviewError::TooLarge(format!(
            "{} bytes, at most {MAX_CONTACT_PICTURE_FILE_BYTES}",
            bytes.len()
        )));
    }
    if bytes.is_empty() {
        return Err(PreviewError::Unsupported("the file is empty".into()));
    }
    Ok(())
}

/// The picture on white: transparent pixels take the white below them.
fn on_white(picture: &DynamicImage) -> RgbImage {
    if !picture.color().has_alpha() {
        return picture.to_rgb8();
    }
    let rgba = picture.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (target, source) in out.pixels_mut().zip(rgba.pixels()) {
        let alpha = u32::from(source[3]);
        let blend = |channel: u8| {
            let value = (u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255;
            u8::try_from(value).unwrap_or(u8::MAX)
        };
        *target = Rgb([blend(source[0]), blend(source[1]), blend(source[2])]);
    }
    out
}

fn jpeg(picture: &RgbImage, quality: u8) -> Result<Vec<u8>, PreviewError> {
    let mut out = Vec::new();
    picture
        .write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality))
        .map_err(image_error)?;
    Ok(out)
}

/// The image for the cropper: decoded within the limits, upright, on white, scaled to at most
/// [`CONTACT_PICTURE_SOURCE_SIZE`] px per side, as JPEG.
///
/// # Errors
///
/// [`PreviewError::TooLarge`] for a file or image over the limits,
/// [`PreviewError::Unsupported`] for anything but PNG, JPEG, GIF and WebP,
/// [`PreviewError::Damaged`] when it cannot be decoded.
pub fn picture_source(bytes: &[u8]) -> Result<PictureJpeg, PreviewError> {
    check_file_size(bytes)?;
    guarded(|| {
        let mut picture = decode_still(bytes)?;
        if picture.width() == 0 || picture.height() == 0 {
            return Err(PreviewError::Damaged("the image has no pixels".into()));
        }
        if picture.width() > CONTACT_PICTURE_SOURCE_SIZE
            || picture.height() > CONTACT_PICTURE_SOURCE_SIZE
        {
            picture = picture.resize(
                CONTACT_PICTURE_SOURCE_SIZE,
                CONTACT_PICTURE_SOURCE_SIZE,
                FilterType::Triangle,
            );
        }
        let rgb = on_white(&picture);
        Ok(PictureJpeg {
            jpeg: jpeg(&rgb, SOURCE_QUALITY)?,
            width: rgb.width(),
            height: rgb.height(),
        })
    })
}

/// The square `crop` of `bytes` (in its pixels; clamped to the image) as the stored picture:
/// [`CONTACT_PICTURE_SIZE`] px per side, a JPEG of at most [`MAX_CONTACT_PICTURE_BYTES`].
///
/// # Errors
///
/// As [`picture_source`]; [`PreviewError::Unsupported`] for an empty square, and
/// [`PreviewError::TooLarge`] when no quality makes the JPEG small enough.
pub fn picture_crop(bytes: &[u8], crop: PictureCrop) -> Result<PictureJpeg, PreviewError> {
    check_file_size(bytes)?;
    guarded(|| {
        let picture = decode_still(bytes)?;
        let (width, height) = (picture.width(), picture.height());
        let size = crop.size.min(width).min(height);
        if size == 0 {
            return Err(PreviewError::Unsupported("the square is empty".into()));
        }
        let x = crop.x.min(width - size);
        let y = crop.y.min(height - size);
        let square = picture.crop_imm(x, y, size, size).resize_exact(
            CONTACT_PICTURE_SIZE,
            CONTACT_PICTURE_SIZE,
            FilterType::CatmullRom,
        );
        let rgb = on_white(&square);
        for quality in PICTURE_QUALITIES {
            let out = jpeg(&rgb, quality)?;
            if out.len() <= MAX_CONTACT_PICTURE_BYTES {
                return Ok(PictureJpeg {
                    jpeg: out,
                    width: CONTACT_PICTURE_SIZE,
                    height: CONTACT_PICTURE_SIZE,
                });
            }
        }
        Err(PreviewError::TooLarge(format!(
            "the picture stays over {MAX_CONTACT_PICTURE_BYTES} bytes"
        )))
    })
}

#[cfg(test)]
mod tests {
    use image::codecs::png::PngEncoder;
    use image::{ImageEncoder, Rgba, RgbaImage};

    use super::*;

    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> Rgba<u8>) -> Vec<u8> {
        let image = RgbaImage::from_fn(width, height, pixel);
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .expect("png");
        out
    }

    fn decoded(jpeg: &[u8]) -> RgbImage {
        image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
            .expect("jpeg")
            .to_rgb8()
    }

    #[test]
    fn a_large_image_is_scaled_for_the_cropper_and_cut_to_a_square() {
        // Left half red, right half blue, 3000 × 1500.
        let file = png(3000, 1500, |x, _| {
            if x < 1500 {
                Rgba([200, 0, 0, 255])
            } else {
                Rgba([0, 0, 200, 255])
            }
        });
        let source = picture_source(&file).expect("source");
        assert!(is_jpeg(&source.jpeg));
        assert_eq!((source.width, source.height), (1024, 512));
        assert_eq!(decoded(&source.jpeg).dimensions(), (1024, 512));
        // The right half of the source as the picture: blue.
        let picture = picture_crop(
            &source.jpeg,
            PictureCrop {
                x: 512,
                y: 0,
                size: 512,
            },
        )
        .expect("crop");
        assert!(is_jpeg(&picture.jpeg));
        assert!(picture.jpeg.len() <= MAX_CONTACT_PICTURE_BYTES);
        let pixels = decoded(&picture.jpeg);
        assert_eq!(pixels.dimensions(), (256, 256));
        let centre = pixels.get_pixel(128, 128);
        assert!(centre[2] > 150 && centre[0] < 50, "{centre:?}");
    }

    #[test]
    fn a_square_past_the_edge_is_clamped_and_transparency_is_white() {
        let file = png(100, 60, |_, _| Rgba([0, 0, 0, 0]));
        let picture = picture_crop(
            &file,
            PictureCrop {
                x: 90,
                y: 50,
                size: 500,
            },
        )
        .expect("crop");
        let pixel = *decoded(&picture.jpeg).get_pixel(10, 10);
        assert!(pixel.0.iter().all(|c| *c > 240), "{pixel:?}");
        let empty = picture_crop(
            &file,
            PictureCrop {
                x: 0,
                y: 0,
                size: 0,
            },
        );
        assert!(matches!(empty, Err(PreviewError::Unsupported(_))));
    }

    #[test]
    fn crafted_or_foreign_files_are_refused() {
        // Not an image.
        assert!(matches!(
            picture_source(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            Err(PreviewError::Unsupported(_))
        ));
        assert!(matches!(
            picture_source(b""),
            Err(PreviewError::Unsupported(_))
        ));
        // A PNG signature with a damaged header.
        let mut broken = png(10, 10, |_, _| Rgba([1, 2, 3, 255]));
        broken.truncate(40);
        assert!(matches!(
            picture_source(&broken),
            Err(PreviewError::Damaged(_))
        ));
        // A PNG header that claims 60,000 × 60,000 pixels: refused before decoding.
        let mut huge = png(1, 1, |_, _| Rgba([0, 0, 0, 255]));
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert!(matches!(
            picture_source(&huge),
            Err(PreviewError::TooLarge(_) | PreviewError::Damaged(_))
        ));
        // A file over the size limit is refused unread.
        let big = vec![0u8; MAX_CONTACT_PICTURE_FILE_BYTES as usize + 1];
        assert!(matches!(
            picture_source(&big),
            Err(PreviewError::TooLarge(_))
        ));
        assert!(!is_jpeg(b"\x89PNG"));
    }

    #[test]
    fn noise_still_fits_the_stored_size() {
        // Pseudo-random noise compresses worst.
        let file = png(512, 512, |x, y| {
            let mut seed = (x * 512 + y + 1).wrapping_mul(0x2545_f491);
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let [a, b, c, _] = seed.to_le_bytes();
            Rgba([a, b, c, 255])
        });
        let picture = picture_crop(
            &file,
            PictureCrop {
                x: 0,
                y: 0,
                size: 512,
            },
        )
        .expect("crop");
        assert!(picture.jpeg.len() <= MAX_CONTACT_PICTURE_BYTES);
    }
}
