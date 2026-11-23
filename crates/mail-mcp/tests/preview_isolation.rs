//! Finding C-2 (decision 0043): attachment previews run in the helper, watched from outside.
//! Against the real `mail-mcp` binary of this build.
//!
//! `fixtures/flate-bomb-1g.pdf` (2,253 bytes) is one page whose content stream is 1 GiB of zero
//! bytes behind two `FlateDecode` filters; rendered in one process it reaches 1.1 GB of memory
//! in 0.4 s (release build). Generated with Python's `zlib` as described in decision 0043.

use std::path::Path;
use std::time::{Duration, Instant};

use mail_mcp::preview::{PreviewLimits, render_isolated, resident_bytes};
use mail_model::{AttachmentPreview, PreviewError, PreviewKind};

const HELPER: &str = env!("CARGO_BIN_EXE_mail-mcp");
const BOMB: &[u8] = include_bytes!("fixtures/flate-bomb-1g.pdf");

/// A one-page PDF with a filled rectangle, with a correct cross-reference table.
fn small_pdf() -> Vec<u8> {
    let content = b"0 0 1 rg 20 20 100 60 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            String::from_utf8_lossy(content)
        ),
    ];
    let mut out = String::from("%PDF-1.7\n");
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

async fn parent_memory() -> u64 {
    resident_bytes(std::process::id())
        .await
        .expect("the parent's memory is readable")
}

#[tokio::test]
async fn a_normal_pdf_and_image_render_in_the_helper() {
    let started = Instant::now();
    let preview = render_isolated(
        Path::new(HELPER),
        PreviewKind::Pdf,
        small_pdf(),
        0,
        PreviewLimits::default(),
    )
    .await
    .expect("rendered");
    let AttachmentPreview::Pdf {
        data_url, pages, ..
    } = preview
    else {
        panic!("a PDF preview: {preview:?}");
    };
    assert!(data_url.starts_with("data:image/png;base64,"));
    assert_eq!(pages, 1);
    eprintln!("a one-page PDF through the helper: {:?}", started.elapsed());

    // Not a PDF at all: the helper's own answer, typed.
    let err = render_isolated(
        Path::new(HELPER),
        PreviewKind::Pdf,
        b"just text".to_vec(),
        0,
        PreviewLimits::default(),
    )
    .await
    .expect_err("not a PDF");
    assert!(matches!(err, PreviewError::Unsupported(_)), "{err:?}");
}

#[tokio::test]
async fn a_pdf_decompression_bomb_is_stopped_and_the_app_is_not_touched() {
    let before = parent_memory().await;
    let limits = PreviewLimits::default();
    let started = Instant::now();
    let err = render_isolated(
        Path::new(HELPER),
        PreviewKind::Pdf,
        BOMB.to_vec(),
        0,
        limits,
    )
    .await
    .expect_err("stopped");
    let took = started.elapsed();
    assert!(
        matches!(&err, PreviewError::TooLarge(reason) if reason.contains("MiB")),
        "{err:?}"
    );
    assert!(took < limits.time, "{took:?}");
    let after = parent_memory().await;
    assert!(
        after < before + 64 * 1024 * 1024,
        "the parent grew from {before} to {after} bytes"
    );
    eprintln!("bomb stopped after {took:?}; parent {before} -> {after} bytes");
}

#[tokio::test]
async fn a_preview_over_the_time_limit_is_stopped() {
    let limits = PreviewLimits {
        time: Duration::ZERO,
        ..PreviewLimits::default()
    };
    let err = render_isolated(
        Path::new(HELPER),
        PreviewKind::Pdf,
        BOMB.to_vec(),
        0,
        limits,
    )
    .await
    .expect_err("stopped");
    assert!(
        matches!(&err, PreviewError::TooLarge(reason) if reason.contains("longer than")),
        "{err:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_helper_that_fails_or_is_missing_gives_a_typed_error() {
    let err = render_isolated(
        Path::new("/usr/bin/false"),
        PreviewKind::Pdf,
        small_pdf(),
        0,
        PreviewLimits::default(),
    )
    .await
    .expect_err("failed");
    assert!(matches!(err, PreviewError::Damaged(_)), "{err:?}");
    let err = render_isolated(
        Path::new("/nonexistent/mail-mcp"),
        PreviewKind::Pdf,
        small_pdf(),
        0,
        PreviewLimits::default(),
    )
    .await
    .expect_err("missing");
    assert!(matches!(err, PreviewError::Unsupported(_)), "{err:?}");
}
