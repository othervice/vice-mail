//! 1,000 records: time to encrypt, decrypt and plan, and the first and a repeated pass of two
//! devices. Numbers go to stderr (`--nocapture`); the bounds only catch gross regressions.
//! In debug builds the dependencies are unoptimised, so release numbers are far lower.

use std::time::{Duration, Instant};

use serde_json::json;

use super::*;

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

/// 1,000 records of a realistic mix (sizes from settings to 1 KiB signatures).
fn fill(device: &mut Device) {
    device.put(&theme(), json!("dark"));
    device.put(&language(), json!("de"));
    let text = "Best regards, Ann Example. ".repeat(40);
    for n in 0..300 {
        device.put(
            &signature(1000 + n),
            json!({ "html": format!("<p>{n}</p>{text}") }),
        );
    }
    for n in 0..300 {
        device.put(
            &snooze(n),
            json!({ "untilMs": START_MS + i64::try_from(n).unwrap_or(0) }),
        );
    }
    for n in 0..200 {
        let key = RecordKey::mail(
            RecordKind::Pin,
            account_id(2),
            &format!("<pin-{n}@example.org>"),
        )
        .expect("pin");
        device.put(&key, json!({ "pinnedAtMs": START_MS }));
    }
    for n in 0..150 {
        let key =
            RecordKey::uuid(RecordKind::Rule, uuid::Uuid::from_u128(0x7000 + n)).expect("rule");
        device.put(
            &key,
            rule_content(&format!("x{n}@example.org"), i64::try_from(n).unwrap_or(0)),
        );
    }
    for n in 0..48 {
        let key = RecordKey::uuid(RecordKind::SavedView, uuid::Uuid::from_u128(0x8000 + n))
            .expect("view");
        device.put(
            &key,
            json!({ "name": format!("View {n}"), "query": format!("from:x{n}@example.org is:unread"),
                    "position": n }),
        );
    }
    assert_eq!(device.state.records().len(), 1000);
}

#[tokio::test]
async fn thousand_records_first_sync_is_measured() {
    let folder = MemoryTransport::new();
    let mut a = Device::first(&folder, 1).await;
    fill(&mut a);

    let records: Vec<SyncRecord> = a
        .state
        .records()
        .values()
        .map(|r| r.record.clone())
        .collect();
    let start = Instant::now();
    let sealed: Vec<SealedRecord> = records
        .iter()
        .map(|r| a.key.seal(r).expect("seal"))
        .collect();
    let seal_time = start.elapsed();
    let bytes: usize = sealed.iter().map(|s| s.ciphertext.len()).sum();
    let start = Instant::now();
    for (record, sealed) in records.iter().zip(&sealed) {
        assert_eq!(
            &a.key
                .open(&sealed.header, &sealed.ciphertext)
                .expect("open"),
            record
        );
    }
    let open_time = start.elapsed();

    let start = Instant::now();
    let report = a.pass().await;
    let upload_time = start.elapsed();
    assert_eq!(report.uploaded, 1000);

    // Planning alone for a new device: every entry is new to it.
    let listing: Vec<RemoteEntry> = folder
        .entries()
        .into_iter()
        .map(|(handle, e)| RemoteEntry {
            handle,
            size: e.body.len() as u64,
            header: e.header,
        })
        .collect();
    let opened: Vec<(EntryHandle, Result<SyncRecord, OpenError>)> = folder
        .entries()
        .into_iter()
        .filter_map(|(handle, e)| {
            let header = RecordHeader::parse(&e.header)?;
            (!header.opaque_id.is_key_record()).then(|| (handle, a.key.open(&header, &e.body)))
        })
        .collect();
    let empty = BTreeMap::new();
    let ctx = PlanContext {
        now_ms: a.now(),
        last_complete_sync_ms: None,
        full_check: false,
    };
    let mut b = Device::joining(&folder, 2, a.now()).await;
    let start = Instant::now();
    let planner = Planner::new(&b.key, &empty, &listing, ctx);
    assert_eq!(planner.to_fetch().len(), 1000);
    let plan = planner.finish(opened, &mut b.hlc).expect("plan");
    let plan_time = start.elapsed();
    assert_eq!(plan.groups.len(), 1000);

    let start = Instant::now();
    let report = b.pass().await;
    let first_time = start.elapsed();
    assert_eq!(report.changed.len(), 1000);
    assert_eq!(a.live(), b.live());

    // A repeated pass downloads nothing and changes nothing.
    let ctx = PlanContext {
        now_ms: b.now(),
        last_complete_sync_ms: b.state.last_complete_sync_ms,
        full_check: false,
    };
    let listing_now: Vec<RemoteEntry> = folder
        .entries()
        .into_iter()
        .map(|(handle, e)| RemoteEntry {
            handle,
            size: e.body.len() as u64,
            header: e.header,
        })
        .collect();
    assert!(
        Planner::new(&b.key, b.state.records(), &listing_now, ctx)
            .to_fetch()
            .is_empty()
    );
    let start = Instant::now();
    let report = b.pass().await;
    let repeat_time = start.elapsed();
    assert!(report.is_quiet());
    let start = Instant::now();
    let report = a.pass_with(PassMode::FullCheck).await.expect("full check");
    let full_check_time = start.elapsed();
    assert!(report.is_quiet());

    eprintln!(
        "1,000 records ({} KiB encrypted): seal {}, open {}, plan for a new device {}, \
         first pass A (upload) {}, first pass B (download, open, plan, apply) {}, \
         repeated pass {}, full check {}",
        bytes / 1024,
        ms(seal_time),
        ms(open_time),
        ms(plan_time),
        ms(upload_time),
        ms(first_time),
        ms(repeat_time),
        ms(full_check_time),
    );
    assert!(
        first_time < Duration::from_secs(60),
        "first pass took {first_time:?}"
    );
}

/// The production scrypt factor: run with
/// `cargo test -p mail-engine --release settings_sync::tests::perf -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "pays the full scrypt cost (256 MiB); measured on demand"]
async fn default_work_factor_cost() {
    let key = SyncKey::generate();
    let version = Hlc::new(START_MS, 0, DeviceId::random()).expect("version");
    let start = Instant::now();
    let sealed = key
        .wrap(&passphrase(), version, WorkFactor::DEFAULT)
        .expect("wrap");
    let wrap_time = start.elapsed();
    let start = Instant::now();
    let unwrapped =
        SyncKey::unwrap(&sealed.header, &sealed.ciphertext, &passphrase()).expect("unwrap");
    let unwrap_time = start.elapsed();
    let start = Instant::now();
    let wrong = SyncKey::unwrap(
        &sealed.header,
        &sealed.ciphertext,
        &Secret::new("wrong".to_owned()),
    );
    let wrong_time = start.elapsed();
    assert!(matches!(wrong, Err(SettingsSyncError::WrongPassphrase)));
    assert_eq!(unwrapped.key_record_id(), key.key_record_id());
    eprintln!(
        "scrypt log2(N) = {}: wrap {}, unwrap {}, wrong passphrase {}",
        WorkFactor::DEFAULT.log_n(),
        ms(wrap_time),
        ms(unwrap_time),
        ms(wrong_time)
    );
}
