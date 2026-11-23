//! Stage 10 in the store (decision 0023): the migration of the stage 5 text signature, the
//! signature pool with images, defaults per identity, the signature a draft resolves to,
//! templates, their change capture and applying them from the settings sync.

use std::sync::{Arc, Mutex, PoisonError};

use mail_model::{DraftKind, SignatureId, TemplateId};
use serde_json::{Value, json};

use crate::migrations::MIGRATIONS;
use crate::store::Schema;
use crate::test_support::{db_path, fixture, new_account};
use crate::{
    CaptureOutcome, CapturedChange, SignatureRow, Store, StoreOptions, StoredSignatureImage,
    SyncApply, SyncCapture, SyncRecordRow, TableWrite, TemplateRow,
};

/// Versions every change with a counter (unchanged content keeps its version, a gone row with
/// a stored record becomes a tombstone, a gone row without one is nothing).
#[derive(Default)]
struct Recorder {
    counter: Mutex<u64>,
}

impl SyncCapture for Recorder {
    fn capture(&self, changes: Vec<CapturedChange>) -> CaptureOutcome {
        let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
        let mut rows = Vec::new();
        for change in changes {
            let unchanged = change
                .stored
                .as_ref()
                .is_some_and(|s| s.content == change.current);
            if unchanged || (change.current.is_none() && change.stored.is_none()) {
                continue;
            }
            *counter += 1;
            rows.push(SyncRecordRow {
                deleted_version: change
                    .current
                    .is_none()
                    .then(|| change.stored.as_ref().map(|s| s.version.clone()))
                    .flatten(),
                kind: change.kind,
                id: change.id,
                content: change.current,
                version: format!("{:013}", *counter),
                synced_version: None,
            });
        }
        CaptureOutcome {
            rows,
            hlc_last: Some(format!("{:013}", *counter)),
        }
    }
}

fn sig(n: u128) -> SignatureId {
    SignatureId::from_random(n)
}

fn image(n: u32) -> StoredSignatureImage {
    StoredSignatureImage {
        id: n.to_string(),
        content_id: format!("sig-{n:032x}@signature"),
        file_name: format!("logo{n}.png"),
        mime: "image/png".into(),
        data_base64: "iVBORw0KGgo=".into(),
    }
}

fn row(
    id: &SignatureId,
    name: &str,
    html: &str,
    images: Vec<StoredSignatureImage>,
) -> SignatureRow {
    SignatureRow {
        id: id.clone(),
        name: name.into(),
        html: Some(html.into()),
        text: name.into(),
        images,
    }
}

#[tokio::test]
async fn the_text_signature_of_stage_5_becomes_a_signature_with_the_identity_uuid() {
    let dir = tempfile::TempDir::new().expect("dir");
    let path = db_path(&dir);
    let stage11 = Schema::from_vec(MIGRATIONS[..9].to_vec());
    let store = Store::open_with(&path, StoreOptions::default(), &stage11).expect("stage 11");
    let recorder = Arc::new(Recorder::default());
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("account")
        .id;
    // As stage 5 and 11 wrote it: the text in `identity.signature`, its sync record by the
    // identity's UUID (written here directly: today's capture reads the stage 10 columns).
    let uuid: String = store
        .write(|tx| {
            tx.execute(
                "UPDATE identity SET signature = 'Alice\nTel. 1' WHERE is_default = 1",
                [],
            )?;
            let uuid: String =
                tx.query_row("SELECT uuid FROM identity WHERE is_default = 1", [], |r| {
                    r.get(0)
                })?;
            tx.execute(
                "INSERT INTO sync_record (kind, id, version, content, updated_at)
                 VALUES ('signature', ?1, '0000000000000', json_object('text', 'Alice' || char(10) || 'Tel. 1'), 0)",
                [&uuid],
            )?;
            Ok(uuid)
        })
        .await
        .expect("stage 5 signature");
    let before = store
        .sync_record("signature", &uuid)
        .await
        .expect("read")
        .expect("captured at stage 11");
    assert_eq!(before.content, Some(json!({ "text": "Alice\nTel. 1" })));
    drop(store);

    let store = Store::open(&path, StoreOptions::default()).expect("migrated");
    store.set_sync_capture(Arc::clone(&recorder) as Arc<dyn SyncCapture>);
    store.flush_sync_capture().await.expect("flush");
    let id = SignatureId::parse(&uuid).expect("uuid");
    let migrated = store
        .signature(id.clone())
        .await
        .expect("read")
        .expect("signature");
    assert_eq!(migrated.name, "alice@example.org");
    assert_eq!(migrated.text, "Alice\nTel. 1");
    assert_eq!(migrated.html, None, "text only, HTML derived by the engine");
    let identity = store.identities(account).await.expect("identities")[0].id;
    let defaults = store.signature_defaults(identity).await.expect("defaults");
    assert_eq!(
        (defaults.new, defaults.reply),
        (Some(id.clone()), Some(id.clone()))
    );
    let identities = store.compose_identities(account).await.expect("compose");
    assert_eq!(identities[0].1.as_deref(), Some("Alice\nTel. 1"));
    let old_column: Option<String> = store
        .read(|conn| Ok(conn.query_row("SELECT signature FROM identity", [], |r| r.get(0))?))
        .await
        .expect("column");
    assert_eq!(old_column, None);

    // The record keeps its id and gets the new shape; the identity names its defaults.
    let after = store
        .sync_record("signature", &uuid)
        .await
        .expect("read")
        .expect("record");
    assert_eq!(
        after.content,
        Some(json!({ "name": "alice@example.org", "text": "Alice\nTel. 1" }))
    );
    assert!(after.version > before.version);
    let identity_record = store
        .sync_record("identity", &uuid)
        .await
        .expect("read")
        .expect("identity record");
    let content = identity_record.content.expect("content");
    assert_eq!(content["signatureNew"], json!(uuid));
    assert_eq!(content["signatureReply"], json!(uuid));
}

#[tokio::test]
async fn signatures_defaults_and_the_signature_of_a_draft() {
    let f = fixture().await;
    let store = &f.store;
    let identity = store.identities(f.account).await.expect("identities")[0].id;
    let (work, short) = (sig(1), sig(2));
    let saved = store
        .put_signature(row(
            &work,
            "Work",
            "<p>Alice <img src=\"cid:sig-00000000000000000000000000000001@signature\"></p>",
            vec![image(1)],
        ))
        .await
        .expect("work");
    assert_eq!(saved.images, vec![image(1)]);
    store
        .put_signature(row(&short, "Short", "<p>A.</p>", Vec::new()))
        .await
        .expect("short");
    // Changing keeps the place in the order.
    store
        .put_signature(row(&work, "Work 2", "<p>Alice</p>", Vec::new()))
        .await
        .expect("change");
    let all = store.signatures().await.expect("list");
    assert_eq!(
        all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Work 2", "Short"]
    );
    assert!(all[0].images.is_empty());

    // No defaults yet: a draft gets none unless it chose one.
    assert_eq!(
        store
            .signature_for(None, Some(identity), DraftKind::New)
            .await
            .expect("none"),
        None
    );
    store
        .set_signature_defaults(identity, Some(work.clone()), Some(short.clone()))
        .await
        .expect("defaults");
    let resolved = |chosen: Option<SignatureId>, kind| async move {
        store
            .signature_for(chosen, Some(identity), kind)
            .await
            .expect("resolve")
            .map(|s| s.name)
    };
    assert_eq!(
        resolved(None, DraftKind::New).await.as_deref(),
        Some("Work 2")
    );
    assert_eq!(
        resolved(None, DraftKind::Reply).await.as_deref(),
        Some("Short")
    );
    assert_eq!(
        resolved(None, DraftKind::Forward).await.as_deref(),
        Some("Short")
    );
    assert_eq!(
        resolved(Some(work.clone()), DraftKind::Reply)
            .await
            .as_deref(),
        Some("Work 2"),
        "the composer's choice"
    );
    assert_eq!(
        resolved(Some(sig(99)), DraftKind::Reply).await.as_deref(),
        Some("Short"),
        "a choice that is gone falls back to the default"
    );
    // A default must exist.
    assert!(matches!(
        store
            .set_signature_defaults(identity, Some(sig(99)), None)
            .await,
        Err(crate::StoreError::NotFound(_))
    ));

    // Deleting a signature: the defaults that name it mean "none" (the identity row and its
    // sync record stay as they are).
    assert!(store.delete_signature(short.clone()).await.expect("delete"));
    assert!(!store.delete_signature(short).await.expect("again"));
    let defaults = store.signature_defaults(identity).await.expect("defaults");
    assert_eq!((defaults.new, defaults.reply), (Some(work), None));
    assert_eq!(resolved(None, DraftKind::Reply).await, None);
}

#[tokio::test]
async fn the_text_signature_command_keeps_working_on_the_pool() {
    let f = fixture().await;
    let store = &f.store;
    let identity = store.identities(f.account).await.expect("identities")[0].id;
    store
        .set_identity_signature(identity, Some("Regards".into()))
        .await
        .expect("set");
    let defaults = store.signature_defaults(identity).await.expect("defaults");
    let id = defaults.new.clone().expect("default");
    assert_eq!(defaults.reply, Some(id.clone()));
    assert_eq!(
        store
            .signature(id.clone())
            .await
            .expect("read")
            .expect("row")
            .text,
        "Regards"
    );
    store
        .set_identity_signature(identity, None)
        .await
        .expect("remove");
    assert_eq!(store.signature(id).await.expect("read"), None);
    let defaults = store.signature_defaults(identity).await.expect("defaults");
    assert_eq!((defaults.new, defaults.reply), (None, None));
}

#[tokio::test]
async fn templates_are_stored_captured_and_applied() {
    let dir = tempfile::TempDir::new().expect("dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    store.set_sync_capture(Arc::new(Recorder::default()) as Arc<dyn SyncCapture>);
    let id = TemplateId::from_random(7);
    store
        .put_template(TemplateRow {
            id: id.clone(),
            name: "Thanks".into(),
            subject: "Thanks, {{first_name}}".into(),
            html: "<p>Hi {{first_name}}</p>".into(),
        })
        .await
        .expect("template");
    let record = store
        .sync_record("template", id.as_str())
        .await
        .expect("read")
        .expect("captured");
    assert_eq!(
        record.content,
        Some(
            json!({ "name": "Thanks", "subject": "Thanks, {{first_name}}",
                     "html": "<p>Hi {{first_name}}</p>" })
        )
    );
    assert!(store.delete_template(id.clone()).await.expect("delete"));
    let record = store
        .sync_record("template", id.as_str())
        .await
        .expect("read")
        .expect("tombstone");
    assert!(record.content.is_none() && record.deleted_version.is_some());

    // From another device: a template and a signature with an image land in their tables.
    let other = TemplateId::from_random(8);
    let signature = sig(9);
    let put = |kind: &str, id: &str, value: Value| SyncApply {
        kind: kind.into(),
        id: id.into(),
        expected: None,
        row: Some(SyncRecordRow {
            kind: kind.into(),
            id: id.into(),
            version: "1764000000000-00000000-x".into(),
            synced_version: Some("1764000000000-00000000-x".into()),
            content: Some(value.clone()),
            deleted_version: None,
        }),
        table: TableWrite::Put(value),
        rekey_from: None,
    };
    let outcome = store
        .apply_sync(
            vec![
                put(
                    "template",
                    other.as_str(),
                    json!({ "name": "B", "subject": "S", "html": "<p>B</p>" }),
                ),
                put(
                    "signature",
                    signature.as_str(),
                    json!({ "name": "Logo", "text": "L", "html": "<p>L</p>",
                            "images": [image(1).to_json()] }),
                ),
            ],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(outcome.applied.len(), 2, "{outcome:?}");
    assert!(outcome.kinds.contains("template") && outcome.kinds.contains("signature"));
    assert_eq!(store.templates().await.expect("list")[0].name, "B");
    let applied = store
        .signature(signature.clone())
        .await
        .expect("read")
        .expect("signature");
    assert_eq!(applied.images, vec![image(1)]);
    // Applying is not a local change: the record keeps the pass's version.
    assert_eq!(
        store
            .sync_record("signature", signature.as_str())
            .await
            .expect("read")
            .expect("row")
            .version,
        "1764000000000-00000000-x"
    );
}
