//! Stage 17 (decision 0050): a picture the user chose for a contact is decoded and encoded in
//! the helper, under the same watchdog as previews and the images of mails (decision 0046).
//! Against the real `mail-mcp` binary of this build.

use std::time::Duration;

use image::codecs::png::PngEncoder;
use image::{Rgba, RgbaImage};
use mail_mcp::preview::{PictureStep, PreviewLimits, contact_picture_isolated};
use mail_model::{CONTACT_PICTURE_SIZE, MAX_CONTACT_PICTURE_BYTES, PictureCrop, PreviewError};

const HELPER: &str = env!("CARGO_BIN_EXE_mail-mcp");

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            u8::try_from(x % 256).unwrap_or(0),
            u8::try_from(y % 256).unwrap_or(0),
            160,
            255,
        ])
    });
    let mut out = Vec::new();
    picture
        .write_with_encoder(PngEncoder::new(&mut out))
        .expect("png");
    out
}

async fn run(bytes: Vec<u8>, step: PictureStep) -> Result<mail_content::PictureJpeg, PreviewError> {
    contact_picture_isolated(
        std::path::Path::new(HELPER),
        bytes,
        step,
        PreviewLimits::default(),
    )
    .await
}

#[tokio::test]
async fn a_chosen_picture_is_scaled_and_cut_in_the_helper() {
    let source = run(png(2000, 1200), PictureStep::Source)
        .await
        .expect("source");
    assert_eq!((source.width, source.height), (1024, 614));
    assert!(mail_content::is_jpeg(&source.jpeg));
    let picture = run(
        source.jpeg,
        PictureStep::Crop(PictureCrop {
            x: 100,
            y: 0,
            size: 614,
        }),
    )
    .await
    .expect("crop");
    assert_eq!(
        (picture.width, picture.height),
        (CONTACT_PICTURE_SIZE, CONTACT_PICTURE_SIZE)
    );
    assert!(picture.jpeg.len() <= MAX_CONTACT_PICTURE_BYTES);
    let decoded = image::load_from_memory(&picture.jpeg).expect("jpeg");
    assert_eq!(decoded.width(), CONTACT_PICTURE_SIZE);
}

#[tokio::test]
async fn crafted_and_foreign_files_are_refused_with_a_typed_error() {
    // A PNG header that claims 60,000 × 60,000 pixels.
    let mut huge = png(1, 1);
    huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
    huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
    let refused = run(huge, PictureStep::Source).await;
    assert!(
        matches!(
            refused,
            Err(PreviewError::TooLarge(_) | PreviewError::Damaged(_))
        ),
        "{refused:?}"
    );
    let mut cut = png(40, 40);
    cut.truncate(60);
    assert!(matches!(
        run(cut, PictureStep::Source).await,
        Err(PreviewError::Damaged(_))
    ));
    assert!(matches!(
        run(b"<svg/>".to_vec(), PictureStep::Source).await,
        Err(PreviewError::Unsupported(_))
    ));
    // Over the time limit the helper is stopped; the caller goes on.
    let stopped = contact_picture_isolated(
        std::path::Path::new(HELPER),
        png(3000, 3000),
        PictureStep::Source,
        PreviewLimits {
            time: Duration::from_millis(1),
            ..PreviewLimits::default()
        },
    )
    .await;
    assert!(
        matches!(stopped, Err(PreviewError::TooLarge(_))),
        "{stopped:?}"
    );
    // A helper that is not there: nothing is decoded in this process.
    let missing = contact_picture_isolated(
        std::path::Path::new("/nonexistent/mail-mcp"),
        png(10, 10),
        PictureStep::Source,
        PreviewLimits::default(),
    )
    .await;
    assert!(matches!(missing, Err(PreviewError::Unsupported(_))));
}
