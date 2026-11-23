//! Finding F2 of the stage 15 review: what a pass downloads is bounded before anything is read.
//! An entry listed larger than a record can be is never downloaded, never counts as a version
//! and is never deleted; per pass at most so many entries and bytes are downloaded, and the
//! following passes work through the rest until the devices agree.

use serde_json::json;

use super::*;

/// One byte over the largest entry a pass downloads.
const OVERSIZED: usize = 2 * 1024 * 1024 + 1;

fn too_large(report: &PassReport) -> Vec<EntryHandle> {
    report
        .notices
        .iter()
        .filter_map(|n| match n {
            Notice::TooLarge { handle, .. } => Some(*handle),
            _ => None,
        })
        .collect()
}

fn postponed(report: &PassReport) -> Vec<EntryHandle> {
    report
        .notices
        .iter()
        .filter_map(|n| match n {
            Notice::Postponed { handle, .. } => Some(*handle),
            _ => None,
        })
        .collect()
}

fn handles(folder: &MemoryTransport) -> Vec<EntryHandle> {
    folder.entries().into_iter().map(|(h, _)| h).collect()
}

/// The entries of a record in the folder.
fn entries_of(folder: &MemoryTransport, device: &Device, key: &RecordKey) -> Vec<EntryHandle> {
    let id = device.key.opaque_id(key).to_string();
    folder
        .entries()
        .into_iter()
        .filter(|(_, e)| e.header.opaque_id == id)
        .map(|(h, _)| h)
        .collect()
}

/// A device with `count` signatures, synced.
async fn with_signatures(folder: &MemoryTransport, count: u128) -> Device {
    let mut a = Device::first(folder, 1).await;
    for n in 0..count {
        a.put(&signature(n), json!({ "text": format!("signature {n}") }));
    }
    a.pass().await;
    a
}

fn capped(entries: usize) -> FetchLimits {
    FetchLimits {
        max_entries: entries,
        ..FetchLimits::DEFAULT
    }
}

#[tokio::test]
async fn an_oversized_entry_is_neither_downloaded_nor_deleted_while_the_rest_syncs() {
    let folder = MemoryTransport::new();
    let mut a = Device::first(&folder, 1).await;
    a.put(&theme(), json!("dark"));
    a.put(&language(), json!("de"));
    a.put(&signature(1), json!({ "text": "s" }));
    a.pass().await;
    // The server adds entries too large to be a record: under the theme's id with a newer
    // version, under an unknown id, and as a newer copy of the key record.
    let later = a.hlc.now().expect("now").to_string();
    let big = vec![b'x'; OVERSIZED];
    let theme_id = a.key.opaque_id(&theme()).to_string();
    let header = |opaque_id: String| RawHeader {
        opaque_id,
        version: later.clone(),
    };
    let over_theme = folder.insert_raw(header(theme_id.clone()), big.clone());
    let over_unknown = folder.insert_raw(
        header("00112233445566778899aabbccddeeff".to_owned()),
        big.clone(),
    );
    let over_key = folder.insert_raw(header(a.key.key_record_id().to_string()), big);
    let oversized = [over_theme, over_unknown, over_key];

    // A new device joins with the real key record and gets everything else.
    let mut b = Device::joining(&folder, 2, a.now()).await;
    let report = b.pass().await;
    assert_eq!(too_large(&report), vec![over_theme, over_unknown]);
    for notice in &report.notices {
        assert!(notice.is_left_out() && !notice.waits(), "{notice:?}");
    }
    assert_eq!(
        b.value(&theme()),
        Some(json!("dark")),
        "the oversized newer version does not count"
    );
    assert_eq!(b.value(&language()), Some(json!("de")));
    assert_eq!(b.value(&signature(1)), Some(json!({ "text": "s" })));
    assert!(b.state.last_complete_sync_ms.is_some(), "a complete pass");

    // A changes the theme (its old entry goes as the loser); both run normal passes and full
    // checks. The oversized entries stay, unread, and the real key record stays too.
    a.advance(1_000);
    a.put(&theme(), json!("light"));
    settle(&mut [&mut a, &mut b]).await;
    for device in [&mut a, &mut b] {
        let report = device
            .pass_with(PassMode::FullCheck)
            .await
            .expect("full check");
        assert_eq!(too_large(&report), vec![over_theme, over_unknown]);
        assert!(report.is_quiet(), "{report:?}");
    }
    assert_eq!(a.live(), b.live());
    assert_eq!(b.value(&theme()), Some(json!("light")));
    assert_eq!(
        entries_per_record(&folder)[&theme_id],
        2,
        "new theme + oversized"
    );
    let left = handles(&folder);
    let reads = folder.reads();
    for handle in oversized {
        assert!(left.contains(&handle), "{handle:?} kept");
        assert!(!reads.contains(&handle), "{handle:?} never read");
    }
    let c = Device::joining(&folder, 3, a.now()).await;
    assert_eq!(c.key.key_record_id(), a.key.key_record_id());
    assert!(!folder.reads().contains(&over_key));

    // A folder whose only key record is too large is not read at all.
    let lone = MemoryTransport::new();
    lone.insert_raw(
        RawHeader {
            opaque_id: a.key.key_record_id().to_string(),
            version: later,
        },
        vec![b'x'; OVERSIZED],
    );
    let mut t = lone.clone();
    assert!(matches!(
        join(&mut t, &passphrase()).await,
        Err(SettingsSyncError::InvalidKeyRecord(_))
    ));
    assert!(lone.reads().is_empty());
}

#[tokio::test]
async fn the_per_pass_cap_leaves_the_rest_for_the_next_pass_and_the_devices_converge() {
    let folder = MemoryTransport::new();
    let mut a = with_signatures(&folder, 10).await;
    // A replayed copy of one record: whoever downloads that record deletes one of the two.
    let replayed = entries_of(&folder, &a, &signature(0))[0];
    let (_, copy) = folder
        .entries()
        .into_iter()
        .find(|(h, _)| *h == replayed)
        .expect("entry");
    folder.insert_raw(copy.header, copy.body);

    let mut b = Device::joining(&folder, 2, a.now()).await;
    b.transport.set_fetch_limits(capped(4));
    b.put(&language(), json!("de"));
    // 11 entries to download, at most 4 per pass.
    let mut passes = 0;
    loop {
        passes += 1;
        assert!(passes <= 3, "the cap must not stall the passes");
        let before = folder.reads().len();
        let report = b.pass().await;
        assert!(folder.reads().len() - before <= 4, "pass {passes}");
        let waiting = postponed(&report);
        for notice in &report.notices {
            assert!(notice.is_left_out() && notice.waits(), "{notice:?}");
        }
        if waiting.is_empty() {
            assert!(b.state.last_complete_sync_ms.is_some());
            break;
        }
        assert!(
            b.state.last_complete_sync_ms.is_none(),
            "a pass that postponed records is not complete"
        );
        let left = handles(&folder);
        for handle in &waiting {
            assert!(left.contains(handle), "postponed entries are never deleted");
        }
    }
    assert_eq!(passes, 3);
    for n in 0..10 {
        assert_eq!(
            b.value(&signature(n)),
            Some(json!({ "text": format!("signature {n}") }))
        );
    }
    assert_eq!(
        entries_of(&folder, &a, &signature(0)).len(),
        1,
        "replay removed"
    );
    assert_eq!(
        entries_of(&folder, &a, &language()).len(),
        1,
        "B's own uploaded"
    );

    // Both change records while B stays capped: they still end equal.
    a.put(&signature(9), json!({ "text": "changed on A" }));
    b.advance(10);
    b.put(&signature(3), json!({ "text": "changed on B" }));
    b.put(&theme(), json!("dark"));
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(a.live(), b.live());
    assert_eq!(
        a.value(&signature(3)),
        Some(json!({ "text": "changed on B" }))
    );
    assert_eq!(
        b.value(&signature(9)),
        Some(json!({ "text": "changed on A" }))
    );
}

#[tokio::test]
async fn the_byte_cap_works_like_the_entry_cap() {
    let folder = MemoryTransport::new();
    let mut a = with_signatures(&folder, 6).await;
    let sizes: BTreeMap<EntryHandle, u64> = folder
        .entries()
        .into_iter()
        .map(|(h, e)| (h, e.body.len() as u64))
        .collect();
    let largest = folder
        .entries()
        .into_iter()
        .filter(|(_, e)| !e.header.opaque_id.starts_with(KEY_RECORD_PREFIX))
        .map(|(_, e)| e.body.len() as u64)
        .max()
        .expect("entries");
    let mut b = Device::joining(&folder, 2, a.now()).await;
    b.transport.set_fetch_limits(FetchLimits {
        max_bytes: 2 * largest,
        ..FetchLimits::DEFAULT
    });
    let mut passes = 0;
    loop {
        passes += 1;
        assert!(passes <= 3, "at least two entries fit into every pass");
        let before = folder.reads().len();
        let report = b.pass().await;
        let read: u64 = folder.reads()[before..].iter().map(|h| sizes[h]).sum();
        assert!(read <= 2 * largest, "pass {passes} read {read} bytes");
        if postponed(&report).is_empty() {
            break;
        }
    }
    assert!(passes > 1, "the cap did apply");
    assert_eq!(a.live(), b.live());
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(a.live(), b.live());
}

#[tokio::test]
async fn a_record_larger_than_a_whole_pass_waits_and_the_rest_still_syncs() {
    let folder = MemoryTransport::new();
    let mut a = with_signatures(&folder, 3).await;
    // Five entries under one id: more than a pass may download (a hostile server).
    let id = "ffeeddccbbaa99887766554433221100";
    let mut planted = Vec::new();
    for _ in 0..5 {
        let header = RawHeader {
            opaque_id: id.to_owned(),
            version: a.hlc.now().expect("now").to_string(),
        };
        planted.push(folder.insert_raw(header, b"junk".to_vec()));
    }
    let mut b = Device::joining(&folder, 2, a.now()).await;
    b.transport.set_fetch_limits(capped(4));
    for _ in 0..2 {
        let report = b.pass().await;
        assert_eq!(postponed(&report), planted);
    }
    for n in 0..3 {
        assert!(
            b.value(&signature(n)).is_some(),
            "records that fit still come"
        );
    }
    assert_eq!(a.live(), b.live());
    let reads = folder.reads();
    let left = handles(&folder);
    for handle in &planted {
        assert!(
            left.contains(handle) && !reads.contains(handle),
            "{handle:?}"
        );
    }
    assert!(b.state.last_complete_sync_ms.is_none());
    // A device without the cap reads and rejects them, and leaves them there as well.
    let report = a.pass().await;
    assert_eq!(report.notices.len(), 5, "{report:?}");
    assert_eq!(handles(&folder), left);
}

#[tokio::test]
async fn a_full_check_under_the_cap_downloads_new_entries_first_and_holds_nothing_back() {
    let folder = MemoryTransport::new();
    let mut a = with_signatures(&folder, 6).await;
    let mut b = Device::joining(&folder, 2, a.now()).await;
    b.pass().await;
    b.put(&signature(7), json!({ "text": "new on B" }));
    b.pass().await;
    let new = entries_of(&folder, &b, &signature(7));

    // Room for three: the new entry, then two of the six re-checks; the rest count as known.
    a.transport.set_fetch_limits(capped(3));
    let before = folder.reads().len();
    let report = a.pass_with(PassMode::FullCheck).await.expect("full check");
    let read = folder.reads()[before..].to_vec();
    assert_eq!(read.len(), 3, "{read:?}");
    assert!(read.contains(&new[0]));
    assert!(report.notices.is_empty(), "{report:?}");
    assert_eq!(report.changed, vec![signature(7)]);
    assert_eq!(a.state.last_complete_sync_ms, Some(a.now()));
    assert_eq!(a.live(), b.live());

    // No room at all: a full check changes nothing and postpones only what is new.
    b.put(&signature(8), json!({ "text": "another" }));
    b.pass().await;
    a.transport.set_fetch_limits(capped(0));
    let report = a.pass_with(PassMode::FullCheck).await.expect("full check");
    assert_eq!(postponed(&report), entries_of(&folder, &b, &signature(8)));
    assert!(report.is_quiet(), "{report:?}");
}
