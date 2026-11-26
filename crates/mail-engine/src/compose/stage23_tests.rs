//! Stage 23 (decision 0082): files dropped on a window reach the core as bytes, in pieces, and
//! are taken into a draft or a signature like files from the system dialog.

use mail_model::{
    CoreError, DraftError, ErrorCode, MAX_ATTACHMENT_BYTES, MAX_UPLOAD_CHUNK_BYTES, SignatureInput,
    UploadId,
};

use super::files::COMPOSE_DIR;
use super::stage10_tests::Bed;
use super::uploads::UPLOADS_DIR;

/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Hands `bytes` over like the page does: announced, then in pieces of `piece` bytes.
async fn upload(bed: &Bed, name: &str, bytes: &[u8], piece: usize) -> UploadId {
    let size = u64::try_from(bytes.len()).expect("size");
    let id = bed
        .core
        .begin_file_upload(name.into(), size)
        .await
        .expect("begin");
    let mut offset = 0_u64;
    for chunk in bytes.chunks(piece.max(1)) {
        offset = bed
            .core
            .append_file_upload(id.clone(), offset, chunk.to_vec())
            .await
            .expect("append");
    }
    assert_eq!(offset, size);
    id
}

fn staged(bed: &Bed) -> usize {
    std::fs::read_dir(
        bed.dir
            .path()
            .join("data")
            .join(COMPOSE_DIR)
            .join(UPLOADS_DIR),
    )
    .map(Iterator::count)
    .unwrap_or(0)
}

#[tokio::test]
async fn a_dropped_file_is_attached_from_its_bytes_and_its_upload_is_gone() {
    let bed = Bed::new().await;
    let draft = bed.draft("Report", "<p>Attached</p>").await;
    let content: Vec<u8> = (0..10_000_u32).map(|i| (i % 251) as u8).collect();
    let id = upload(&bed, "Q3/../Bericht 2026.pdf", &content, 4_096).await;
    assert_eq!(staged(&bed), 1);

    let saved = bed
        .core
        .add_uploaded_attachments(draft.id.clone(), vec![id.clone()])
        .await
        .expect("attach");
    assert_eq!(saved.attachments.len(), 1);
    let attachment = &saved.attachments[0];
    assert_eq!(
        attachment.file_name, "Bericht 2026.pdf",
        "only the name, sanitised"
    );
    assert_eq!(
        attachment.mime, "application/pdf",
        "from the name, as for picked files"
    );
    assert_eq!(attachment.size, 10_000);
    assert_eq!(staged(&bed), 0, "the upload is used up");

    // The same upload a second time: unknown, nothing changes.
    let err = bed
        .core
        .add_uploaded_attachments(draft.id.clone(), vec![id])
        .await
        .expect_err("used up");
    assert_eq!(err.code(), ErrorCode::NotFound);
    let draft = bed.core.get_draft(draft.id).await.expect("draft");
    assert_eq!(draft.attachments.len(), 1);
}

#[tokio::test]
async fn uploads_meet_the_limits_of_picked_files() {
    let bed = Bed::new().await;
    let draft = bed.draft("Limits", "<p>x</p>").await;

    // Larger than any draft takes: refused before a byte is sent.
    let err = bed
        .core
        .begin_file_upload("huge.iso".into(), MAX_ATTACHMENT_BYTES + 1)
        .await
        .expect_err("too large");
    assert!(matches!(
        err,
        CoreError::Draft(DraftError::AttachmentTooLarge { .. })
    ));

    // A piece larger than allowed, a piece at the wrong place, more than announced.
    let id = bed
        .core
        .begin_file_upload("a.txt".into(), 3)
        .await
        .expect("begin");
    let big = vec![0_u8; usize::try_from(MAX_UPLOAD_CHUNK_BYTES).expect("size") + 1];
    let err = bed
        .core
        .append_file_upload(id.clone(), 0, big)
        .await
        .expect_err("piece too large");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let err = bed
        .core
        .append_file_upload(id.clone(), 1, b"b".to_vec())
        .await
        .expect_err("wrong offset");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let err = bed
        .core
        .append_file_upload(id.clone(), 0, b"abcd".to_vec())
        .await
        .expect_err("more than announced");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);

    // Incomplete: not attached, and nothing else of the request is taken either.
    bed.core
        .append_file_upload(id.clone(), 0, b"ab".to_vec())
        .await
        .expect("piece");
    let whole = upload(&bed, "whole.txt", b"done", 2).await;
    let err = bed
        .core
        .add_uploaded_attachments(draft.id.clone(), vec![whole.clone(), id.clone()])
        .await
        .expect_err("incomplete");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert!(
        bed.core
            .get_draft(draft.id.clone())
            .await
            .expect("draft")
            .attachments
            .is_empty()
    );

    // Across the bridge the pieces come as base64: anything else is refused.
    let err = bed
        .core
        .append_file_upload_base64(id.clone(), 2, "not base64!")
        .await
        .expect_err("not base64");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let long = "A".repeat(usize::try_from(MAX_UPLOAD_CHUNK_BYTES).expect("size") * 2);
    let err = bed
        .core
        .append_file_upload_base64(id.clone(), 2, &long)
        .await
        .expect_err("too long");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    let other = bed
        .core
        .begin_file_upload("b64.txt".into(), 5)
        .await
        .expect("begin");
    assert_eq!(
        bed.core
            .append_file_upload_base64(other.clone(), 0, "aGVsbG8=")
            .await
            .expect("base64 piece"),
        5
    );
    bed.core.discard_file_upload(other).await.expect("discard");

    // Given up: discarded, then unknown.
    bed.core
        .discard_file_upload(id.clone())
        .await
        .expect("discard");
    let err = bed
        .core
        .append_file_upload(id, 2, b"c".to_vec())
        .await
        .expect_err("discarded");
    assert_eq!(err.code(), ErrorCode::NotFound);

    // An id the core never gave is unknown.
    let err = bed
        .core
        .add_uploaded_attachments(draft.id.clone(), vec![UploadId::from_random(42)])
        .await
        .expect_err("unknown");
    assert_eq!(err.code(), ErrorCode::NotFound);

    // The complete one still attaches.
    let saved = bed
        .core
        .add_uploaded_attachments(draft.id, vec![whole])
        .await
        .expect("attach");
    assert_eq!(saved.attachments.len(), 1);
    assert_eq!(staged(&bed), 0);
}

#[tokio::test]
async fn dropped_images_become_inline_images_or_signature_images_after_the_same_checks() {
    let bed = Bed::new().await;
    let draft = bed.draft("Inline", "<p>x</p>").await;

    // Not an image by its bytes, whatever the name says: refused, and the upload is gone.
    let fake = upload(&bed, "evil.png", b"<svg onload=alert(1)>", 8).await;
    let err = bed
        .core
        .add_uploaded_inline_images(draft.id.clone(), vec![fake])
        .await
        .expect_err("not an image");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert_eq!(staged(&bed), 0);

    let png = upload(&bed, "chart.png", PNG, 16).await;
    let added = bed
        .core
        .add_uploaded_inline_images(draft.id.clone(), vec![png])
        .await
        .expect("inline");
    assert_eq!(added.added.len(), 1);
    assert_eq!(added.added[0].mime, "image/png");
    assert_eq!(added.added[0].file_name, "chart.png");
    assert!(added.added[0].content_id.is_some());

    // The signature editor: the same bytes as a signature image.
    let signature = bed
        .core
        .save_signature(SignatureInput {
            id: None,
            name: "Logo".into(),
            html: "<p>Alice</p>".into(),
        })
        .await
        .expect("signature")
        .signature;
    let logo = upload(&bed, "logo.png", PNG, 1_000).await;
    let info = bed
        .core
        .add_uploaded_signature_images(signature.id.clone(), vec![logo])
        .await
        .expect("signature image");
    assert_eq!(info.images.len(), 1);
    assert_eq!(info.images[0].file_name, "logo.png");
    let big = upload(&bed, "big.png", &vec![0_u8; 70 * 1024], 64 * 1024).await;
    let err = bed
        .core
        .add_uploaded_signature_images(signature.id, vec![big])
        .await
        .expect_err("too large for a signature");
    assert_eq!(err.code(), ErrorCode::AttachmentTooLarge);
    assert_eq!(staged(&bed), 0);
}
