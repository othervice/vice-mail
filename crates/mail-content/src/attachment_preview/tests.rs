use image::{ImageBuffer, Rgb, Rgba};

use super::*;

fn png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
    let picture = ImageBuffer::from_pixel(width, height, Rgba([10u8, 20, 30, alpha]));
    let mut out = Vec::new();
    picture
        .write_with_encoder(PngEncoder::new(&mut out))
        .expect("png");
    out
}

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let picture = ImageBuffer::from_pixel(width, height, Rgb([200u8, 100, 50]));
    let mut out = Vec::new();
    picture
        .write_with_encoder(JpegEncoder::new_with_quality(&mut out, 90))
        .expect("jpeg");
    out
}

fn decode_data_url(url: &str) -> (String, DynamicImage) {
    let (head, payload) = url.split_once(";base64,").expect("data url");
    let mime = head.strip_prefix("data:").expect("data:").to_owned();
    let bytes = crate::image::base64_decode(payload).expect("base64");
    (mime, image::load_from_memory(&bytes).expect("decodes"))
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

/// A PNG header that claims 20000×20000 pixels (no image data needed to refuse it).
fn huge_png_header() -> Vec<u8> {
    png_header(20_000, 20_000, 8, 6)
}

/// A PNG header of `width`×`height` with `depth` bits and colour type `colour` (6: RGBA, 2: RGB).
fn png_header(width: u32, height: u32, depth: u8, colour: u8) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = b"IHDR".to_vec();
    chunk.extend_from_slice(&width.to_be_bytes());
    chunk.extend_from_slice(&height.to_be_bytes());
    chunk.extend_from_slice(&[depth, colour, 0, 0, 0]);
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&crc32(&chunk).to_be_bytes());
    // An empty IDAT: the decoder reads up to the first one before it reports the size.
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(b"IDAT");
    out.extend_from_slice(&crc32(b"IDAT").to_be_bytes());
    out
}

/// A two-page PDF: page 1 is 200×100 pt with a blue square at (10, 10)–(60, 60), page 2 is
/// empty. Built with a correct cross-reference table.
pub(super) fn two_page_pdf() -> Vec<u8> {
    let content = "0 0 1 rg 10 10 50 50 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 5 0 R \
         /OpenAction << /S /JavaScript /JS (app.alert(1)) >> >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

#[test]
fn kinds_come_from_type_and_name_and_never_cover_active_documents() {
    assert_eq!(preview_kind("a.png", "image/png"), Some(PreviewKind::Image));
    assert_eq!(preview_kind("a", "image/jpeg"), Some(PreviewKind::Image));
    assert_eq!(
        preview_kind("a.png", "Image/PNG; name=\"a.png\""),
        Some(PreviewKind::Image)
    );
    assert_eq!(preview_kind("x", "text/html; charset=utf-8"), None);
    assert_eq!(
        preview_kind("a.pdf", "application/pdf"),
        Some(PreviewKind::Pdf)
    );
    assert_eq!(
        preview_kind("notes.txt", "text/plain"),
        Some(PreviewKind::Text)
    );
    assert_eq!(
        preview_kind("data.csv", "text/csv"),
        Some(PreviewKind::Text)
    );
    assert_eq!(
        preview_kind("scan.PDF", "application/octet-stream"),
        Some(PreviewKind::Pdf)
    );
    assert_eq!(
        preview_kind("photo.JPG", "application/octet-stream"),
        Some(PreviewKind::Image)
    );
    // Active content is never previewed, whatever the other half says.
    for (name, mime) in [
        ("page.html", "text/html"),
        ("page.txt", "text/html"),
        ("logo.svg", "image/svg+xml"),
        ("logo.png", "image/svg+xml"),
        ("trick.svg", "image/png"),
        ("trick.html", "text/plain"),
        ("feed.xml", "text/xml"),
        ("page.xhtml", "application/xhtml+xml"),
    ] {
        assert_eq!(preview_kind(name, mime), None, "{name} {mime}");
    }
    // Other formats are not previewed either.
    assert_eq!(preview_kind("setup.exe", "application/x-msdownload"), None);
    assert_eq!(
        preview_kind("a.docx", "application/vnd.openxmlformats"),
        None
    );
    assert_eq!(preview_kind("a.heic", "image/heic"), None);
    assert_eq!(
        preview_kind("archive.zip", "application/octet-stream"),
        None
    );
}

#[test]
fn images_are_decoded_and_encoded_again_by_the_core() {
    // Transparent: PNG again, same size.
    let Ok(AttachmentPreview::Image {
        data_url,
        width,
        height,
        scaled,
    }) = render_preview(PreviewKind::Image, &png(40, 30, 128), 0)
    else {
        panic!("png preview");
    };
    assert_eq!((width, height, scaled), (40, 30, false));
    let (mime, decoded) = decode_data_url(&data_url);
    assert_eq!(mime, "image/png");
    assert_eq!((decoded.width(), decoded.height()), (40, 30));
    // Opaque: JPEG, and an opaque PNG becomes JPEG too (smaller for photos).
    for bytes in [jpeg(64, 48), png(64, 48, 255)] {
        let Ok(AttachmentPreview::Image { data_url, .. }) =
            render_preview(PreviewKind::Image, &bytes, 0)
        else {
            panic!("jpeg preview");
        };
        assert!(data_url.starts_with("data:image/jpeg;base64,"));
    }
}

#[test]
fn large_images_are_scaled_and_huge_ones_refused() {
    let Ok(AttachmentPreview::Image {
        data_url,
        width,
        height,
        scaled,
    }) = render_preview(PreviewKind::Image, &jpeg(4096, 1024), 0)
    else {
        panic!("preview");
    };
    assert!(scaled);
    assert_eq!((width, height), (MAX_PREVIEW_EDGE, 512));
    let (_, decoded) = decode_data_url(&data_url);
    assert_eq!((decoded.width(), decoded.height()), (MAX_PREVIEW_EDGE, 512));

    let huge = render_preview(PreviewKind::Image, &huge_png_header(), 0);
    assert!(matches!(huge, Err(PreviewError::TooLarge(_))), "{huge:?}");
    // C-9: within the pixel limit, but too much memory once decoded: refused from the header.
    // 8000×8000 RGBA (256 MB) and 6000×6000 at 16 bits per channel (288 MB).
    for (width, height, depth, colour) in [(8000, 8000, 8, 6), (6000, 6000, 16, 6)] {
        let header = png_header(width, height, depth, colour);
        let refused = render_preview(PreviewKind::Image, &header, 0);
        assert!(
            matches!(&refused, Err(PreviewError::TooLarge(reason)) if reason.contains("decoded")),
            "{width}×{height}: {refused:?}"
        );
    }
    // 8-bit RGB of the same 7000×7000 (147 MB) passes the check and fails later on the
    // missing image data: the budget is not the reason.
    let rgb = render_preview(PreviewKind::Image, &png_header(7000, 7000, 8, 2), 0);
    assert!(
        !matches!(&rgb, Err(PreviewError::TooLarge(reason)) if reason.contains("decoded")),
        "{rgb:?}"
    );
    let too_many_bytes = vec![0u8; usize::try_from(MAX_PREVIEW_INPUT_BYTES).expect("usize") + 1];
    assert!(matches!(
        render_preview(PreviewKind::Text, &too_many_bytes, 0),
        Err(PreviewError::TooLarge(_))
    ));
}

#[test]
fn images_that_are_not_what_they_claim_are_refused() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
    assert!(matches!(
        render_preview(PreviewKind::Image, svg, 0),
        Err(PreviewError::Unsupported(_))
    ));
    let mut broken = png(20, 20, 255);
    broken.truncate(40);
    broken.extend_from_slice(&[0xAB; 64]);
    assert!(matches!(
        render_preview(PreviewKind::Image, &broken, 0),
        Err(PreviewError::Damaged(_))
    ));
    assert!(matches!(
        render_preview(PreviewKind::Pdf, b"<html>not a pdf</html>", 0),
        Err(PreviewError::Unsupported(_))
    ));
}

#[test]
fn pdf_pages_are_rasterised_one_at_a_time_without_running_scripts() {
    let pdf = two_page_pdf();
    let Ok(AttachmentPreview::Pdf {
        data_url,
        width,
        height,
        page,
        pages,
        more_pages,
    }) = render_preview(PreviewKind::Pdf, &pdf, 0)
    else {
        panic!("pdf preview");
    };
    assert_eq!(
        (width, height, page, pages, more_pages),
        (400, 200, 0, 2, false)
    );
    let (mime, decoded) = decode_data_url(&data_url);
    assert_eq!(mime, "image/png");
    let rgba = decoded.to_rgba8();
    // The square (PDF origin bottom left, twice the size): blue inside, white outside.
    let inside = rgba.get_pixel(70, 130);
    assert!(
        inside[2] > 200 && inside[0] < 50 && inside[1] < 50,
        "{inside:?}"
    );
    let outside = rgba.get_pixel(300, 50);
    assert!(
        outside[0] > 240 && outside[1] > 240 && outside[2] > 240,
        "{outside:?}"
    );

    let Ok(AttachmentPreview::Pdf {
        width,
        height,
        page,
        ..
    }) = render_preview(PreviewKind::Pdf, &pdf, 1)
    else {
        panic!("second page");
    };
    assert_eq!((width, height, page), (200, 400, 1));
    assert!(matches!(
        render_preview(PreviewKind::Pdf, &pdf, 2),
        Err(PreviewError::Damaged(_))
    ));
    let mut cut = pdf.clone();
    cut.truncate(20);
    assert!(matches!(
        render_preview(PreviewKind::Pdf, &cut, 0),
        Err(PreviewError::Damaged(_))
    ));
}

#[test]
fn text_is_decoded_cleaned_and_cut() {
    let text = |bytes: &[u8]| match render_preview(PreviewKind::Text, bytes, 0) {
        Ok(AttachmentPreview::Text { text, truncated }) => (text, truncated),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        text("Grüße\r\nZeile 2\rZeile 3\u{7}\t!".as_bytes()),
        ("Grüße\nZeile 2\nZeile 3\t!".to_owned(), false)
    );
    // Not UTF-8: Latin-1.
    assert_eq!(text(b"Gr\xFC\xDFe").0, "Grüße");
    // UTF-8 and UTF-16 with BOM.
    assert_eq!(text(b"\xEF\xBB\xBFok").0, "ok");
    assert_eq!(text(b"\xFF\xFEo\0k\0").0, "ok");
    assert_eq!(text(b"\xFE\xFF\0o\0k").0, "ok");
    // Cut at the limit on a character boundary.
    let limit = usize::try_from(MAX_PREVIEW_TEXT_BYTES).expect("usize");
    let long = "ä".repeat(limit);
    let (cut, truncated) = text(long.as_bytes());
    assert!(truncated);
    assert_eq!(cut.len(), limit);
    assert!(cut.chars().all(|c| c == 'ä'));
}
