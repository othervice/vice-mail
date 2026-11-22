//! The acceptance cases of stage 11 on the level of the logic: two (or three) devices with
//! separate local state over one in-memory folder.

use serde_json::json;

use super::*;

async fn pair() -> (MemoryTransport, Device, Device) {
    let folder = MemoryTransport::new();
    let a = Device::first(&folder, 1).await;
    let b = Device::joining(&folder, 2, START_MS).await;
    (folder, a, b)
}

#[tokio::test]
async fn different_settings_on_two_devices_both_stay() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    b.put(&language(), json!("de"));
    settle(&mut [&mut a, &mut b]).await;
    for device in [&a, &b] {
        assert_eq!(device.value(&theme()), Some(json!("dark")));
        assert_eq!(device.value(&language()), Some(json!("de")));
    }
    assert_eq!(entries_per_record(&folder).values().sum::<usize>(), 2);
}

#[tokio::test]
async fn same_setting_on_two_devices_ends_equal() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    b.advance(10);
    b.put(&theme(), json!("light"));
    // A syncs first: its older value goes up, then B's newer one replaces it.
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(a.value(&theme()), Some(json!("light")));
    assert_eq!(b.value(&theme()), Some(json!("light")));
    assert_eq!(a.live(), b.live());
    assert_eq!(
        entries_per_record(&folder)
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![1]
    );

    // Same wall time on both: the device id breaks the tie, identically everywhere.
    a.set_time(START_MS + 1_000);
    b.set_time(START_MS + 1_000);
    a.put(&theme(), json!("system"));
    b.put(&theme(), json!("dark"));
    settle(&mut [&mut b, &mut a]).await;
    assert_eq!(a.value(&theme()), b.value(&theme()));
    assert_eq!(
        a.value(&theme()),
        Some(json!("dark")),
        "device 2 > device 1"
    );
}

#[tokio::test]
async fn deleted_signature_edited_elsewhere_stays_edited() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&signature(1), json!({ "text": "Regards" }));
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(b.value(&signature(1)), Some(json!({ "text": "Regards" })));

    // Concurrently: A deletes, B edits. The delete is even later in time.
    b.advance(10);
    b.put(&signature(1), json!({ "text": "Best regards" }));
    a.advance(20);
    a.delete(&signature(1));
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(
        a.value(&signature(1)),
        Some(json!({ "text": "Best regards" }))
    );
    assert_eq!(
        b.value(&signature(1)),
        Some(json!({ "text": "Best regards" }))
    );

    // A delete made after seeing the edit removes it everywhere.
    a.delete(&signature(1));
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(a.value(&signature(1)), None);
    assert_eq!(b.value(&signature(1)), None);
    assert_eq!(
        entries_per_record(&folder).values().sum::<usize>(),
        1,
        "one tombstone"
    );
}

#[tokio::test]
async fn long_offline_device_brings_old_changes_newer_states_and_tombstones_win() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    a.put(&signature(1), json!({ "text": "one" }));
    a.put(&signature(2), json!({ "text": "two" }));
    settle(&mut [&mut a, &mut b]).await;

    // B goes offline and changes things on day 1; A changes on day 2.
    b.advance(DAY_MS);
    b.put(&theme(), json!("light"));
    b.put(&language(), json!("de"));
    a.advance(2 * DAY_MS);
    a.put(&theme(), json!("system"));
    a.delete(&signature(1));
    a.pass().await;

    // B comes back after 100 days (within the retention).
    b.advance(99 * DAY_MS);
    a.set_time(b.now());
    settle(&mut [&mut b, &mut a]).await;
    for device in [&a, &b] {
        assert_eq!(
            device.value(&theme()),
            Some(json!("system")),
            "newer state wins"
        );
        assert_eq!(device.value(&signature(1)), None, "tombstone works");
        assert_eq!(device.value(&signature(2)), Some(json!({ "text": "two" })));
        assert_eq!(
            device.value(&language()),
            Some(json!("de")),
            "untouched change stays"
        );
    }
    assert_eq!(a.live(), b.live());
    assert!(entries_per_record(&folder).values().all(|n| *n == 1));
}

#[tokio::test]
async fn expired_tombstones_do_not_bring_deleted_records_back() {
    let folder = MemoryTransport::new();
    let mut a = Device::first(&folder, 1).await;
    let mut b = Device::joining(&folder, 2, START_MS).await;
    let mut c = Device::joining(&folder, 3, START_MS).await;
    for n in 1..=4 {
        a.put(&signature(n), json!({ "text": format!("s{n}") }));
    }
    settle(&mut [&mut a, &mut b, &mut c]).await;

    // B goes offline for 250 days and edits signature 2 early on (older than the retention
    // by the time it returns). A deletes signatures 1 and 2 on day 1.
    b.advance(DAY_MS / 2);
    b.put(&signature(2), json!({ "text": "edited offline" }));
    a.advance(DAY_MS);
    a.delete(&signature(1));
    a.delete(&signature(2));
    c.advance(DAY_MS);
    settle(&mut [&mut a, &mut c]).await;
    assert_eq!(c.value(&signature(1)), None);

    // Day 200: the tombstones expire and A's next pass removes them from the server.
    a.set_time(START_MS + 200 * DAY_MS);
    c.set_time(a.now());
    let report = a.pass().await;
    assert_eq!(report.deleted, 2, "expired tombstones removed");
    c.pass().await;
    assert_eq!(entries_per_record(&folder).len(), 2);

    // Day 250: B returns. Signature 1 (unchanged since the last sync) and signature 2 (edited
    // offline, but older than the retention) are stale: forgotten, not uploaded again.
    b.set_time(START_MS + 250 * DAY_MS);
    a.set_time(b.now());
    c.set_time(b.now());
    let report = b.pass().await;
    assert_eq!(report.uploaded, 0, "{report:?}");
    assert_eq!(b.value(&signature(1)), None);
    assert_eq!(b.value(&signature(2)), None);
    settle(&mut [&mut a, &mut b, &mut c]).await;
    for device in [&a, &b, &c] {
        assert_eq!(device.value(&signature(1)), None);
        assert_eq!(device.value(&signature(2)), None);
        assert!(device.value(&signature(3)).is_some());
    }
    assert_eq!(a.live(), b.live());

    // A record vanishing from the server while the device synced recently is data loss, not
    // an expired delete: it is uploaded again.
    let (handle, _) = folder
        .entries()
        .into_iter()
        .find(|(_, e)| e.header.opaque_id == a.key.opaque_id(&signature(3)).to_string())
        .expect("signature 3 on the server");
    assert!(folder.remove_raw(handle));
    let report = a.pass().await;
    assert_eq!(report.uploaded, 1);
    settle(&mut [&mut a, &mut b, &mut c]).await;
    assert!(c.value(&signature(3)).is_some());
}

#[tokio::test]
async fn an_edit_newer_than_the_retention_beats_an_expired_delete() {
    // Consistent with "edit beats delete": B never saw the delete, and its edit is recent.
    let (_, mut a, mut b) = pair().await;
    a.put(&signature(1), json!({ "text": "s" }));
    settle(&mut [&mut a, &mut b]).await;
    a.advance(DAY_MS);
    a.delete(&signature(1));
    a.pass().await;
    a.set_time(START_MS + 200 * DAY_MS);
    a.pass().await;
    b.set_time(START_MS + 210 * DAY_MS);
    b.put(&signature(1), json!({ "text": "edited much later" }));
    a.set_time(b.now());
    settle(&mut [&mut b, &mut a]).await;
    assert_eq!(
        a.value(&signature(1)),
        Some(json!({ "text": "edited much later" }))
    );
}

#[tokio::test]
async fn modified_record_is_rejected_and_reported_the_rest_syncs() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    a.put(&language(), json!("de"));
    a.put(&signature(1), json!({ "text": "s" }));
    a.pass().await;
    let find = |key: &RecordKey| {
        let id = a.key.opaque_id(key).to_string();
        folder
            .entries()
            .into_iter()
            .find(|(_, e)| e.header.opaque_id == id)
            .map(|(h, _)| h)
            .expect("entry")
    };
    // One ciphertext damaged, one moved under a newer version header.
    let damaged = find(&theme());
    folder.modify(damaged, |e| {
        if let Some(byte) = e.body.last_mut() {
            *byte ^= 0x40;
        }
    });
    let renamed = find(&signature(1));
    folder.modify(renamed, |e| {
        let header = RecordHeader::parse(&e.header).expect("header");
        let later = Hlc::new(header.version.millis() + 1, 0, header.version.device()).expect("v");
        e.header.version = later.to_string();
    });

    let report = b.pass().await;
    assert_eq!(b.value(&language()), Some(json!("de")), "the rest syncs");
    assert_eq!(b.value(&theme()), None);
    assert_eq!(b.value(&signature(1)), None);
    let rejected: Vec<(EntryHandle, OpenError)> = report
        .notices
        .iter()
        .filter_map(|n| match n {
            Notice::Rejected { handle, error } => Some((*handle, error.clone())),
            _ => None,
        })
        .collect();
    let mut expected = vec![
        (damaged, OpenError::Damaged),
        (renamed, OpenError::HeaderMismatch("version")),
    ];
    expected.sort_by_key(|(h, _)| *h);
    assert_eq!(rejected, expected);

    // A's normal pass sees the renamed entry (new version) as not its own: it uploads the
    // signature again and deletes the misplaced copy. The damaged theme still carries A's own
    // header, so only a full check on A finds it.
    let report = a.pass().await;
    assert_eq!((report.uploaded, report.deleted), (1, 1), "{report:?}");
    let report = a.pass_with(PassMode::FullCheck).await.expect("full check");
    assert_eq!((report.uploaded, report.deleted), (1, 1), "{report:?}");
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(b.value(&theme()), Some(json!("dark")));
    assert_eq!(b.value(&signature(1)), Some(json!({ "text": "s" })));
    assert_eq!(a.live(), b.live());
    assert_eq!(
        folder.len(),
        4,
        "key record and three records, the bad copies are gone"
    );
}

#[tokio::test]
async fn old_version_played_in_again_is_ignored() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    a.put(&signature(1), json!({ "text": "s" }));
    a.pass().await;
    let old: Vec<StoredEntry> = folder.entries().into_iter().map(|(_, e)| e).collect();
    a.advance(1_000);
    a.put(&theme(), json!("light"));
    a.delete(&signature(1));
    settle(&mut [&mut a, &mut b]).await;

    // The server (or anyone with access to the folder) appends the old ciphertexts again.
    for entry in &old {
        if !entry.header.opaque_id.starts_with(KEY_RECORD_PREFIX) {
            folder.insert_raw(entry.header.clone(), entry.body.clone());
        }
    }
    settle(&mut [&mut b, &mut a]).await;
    for device in [&a, &b] {
        assert_eq!(device.value(&theme()), Some(json!("light")));
        assert_eq!(device.value(&signature(1)), None);
    }
    assert!(
        entries_per_record(&folder).values().all(|n| *n == 1),
        "replays removed"
    );

    // Even after the tombstone expired and was removed from the server, the local tombstones
    // recognise a replay of the deleted signature: it is removed and the deletion announced
    // again, so a device that might have picked the replay up learns about it.
    a.set_time(START_MS + 200 * DAY_MS);
    b.set_time(a.now());
    settle(&mut [&mut a, &mut b]).await;
    let sig = a.key.opaque_id(&signature(1)).to_string();
    assert!(
        !entries_per_record(&folder).contains_key(&sig),
        "tombstone expired"
    );
    for entry in &old {
        if entry.header.opaque_id == sig {
            folder.insert_raw(entry.header.clone(), entry.body.clone());
        }
    }
    // A new device cannot tell a replay from a record: it takes it over at first ...
    let mut c = Device::joining(&folder, 3, a.now()).await;
    c.pass().await;
    assert!(c.value(&signature(1)).is_some());
    // ... and drops it as soon as a device that knows the deletion has announced it again.
    settle(&mut [&mut b, &mut a, &mut c]).await;
    for device in [&a, &b, &c] {
        assert_eq!(device.value(&signature(1)), None);
    }
    let left: Vec<StoredEntry> = folder
        .entries()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| e.header.opaque_id == sig)
        .collect();
    assert_eq!(left.len(), 1, "replay removed, deletion announced again");
    let header = RecordHeader::parse(&left[0].header).expect("header");
    assert!(
        a.key
            .open(&header, &left[0].body)
            .expect("open")
            .is_tombstone()
    );
}

#[tokio::test]
async fn duplicate_left_by_an_interrupted_replace_is_cleaned_up() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    settle(&mut [&mut a, &mut b]).await;

    // The new version is appended, then the connection drops before the old one is deleted.
    a.put(&theme(), json!("light"));
    folder.fail_deletes_after(Some(0));
    let report = a.pass().await;
    assert_eq!(report.uploaded, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(
        entries_per_record(&folder)
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![2]
    );
    folder.fail_deletes_after(None);

    // The next pass of any device keeps the winner and removes the other version.
    let report = b.pass().await;
    assert_eq!(report.deleted, 1);
    assert_eq!(b.value(&theme()), Some(json!("light")));
    assert_eq!(
        entries_per_record(&folder)
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![1]
    );
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(a.live(), b.live());
}

#[tokio::test]
async fn failed_upload_changes_nothing_and_is_retried() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    folder.fail_writes_after(Some(0));
    let report = a.pass().await;
    assert_eq!(report.uploaded, 0);
    assert_eq!(report.failed.len(), 1);
    assert!(a.state.get(&theme()).is_some_and(LocalRecord::is_dirty));
    folder.fail_writes_after(None);
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(b.value(&theme()), Some(json!("dark")));
}

#[tokio::test]
async fn wrong_passphrase_is_reported_and_nothing_is_overwritten() {
    let folder = MemoryTransport::new();
    let mut a = Device::first(&folder, 1).await;
    a.put(&theme(), json!("dark"));
    a.pass().await;
    let before = folder.entries();
    let counters = folder.counters();

    let mut t = folder.clone();
    let err = join(&mut t, &Secret::new("wrong passphrase".to_owned()))
        .await
        .expect_err("wrong passphrase");
    assert!(matches!(err, SettingsSyncError::WrongPassphrase), "{err:?}");
    assert_eq!(
        err.to_string(),
        "the passphrase does not unlock the settings sync key"
    );
    assert_eq!(folder.entries(), before, "folder unchanged");
    assert_eq!(folder.counters(), counters, "no write, no delete");
    // Empty folder and already set-up folder are typed errors too.
    let mut empty = MemoryTransport::new();
    assert!(matches!(
        join(&mut empty, &passphrase()).await,
        Err(SettingsSyncError::NotSetUp)
    ));
    let mut hlc = HlcClock::new(
        Arc::new(TestClock(AtomicI64::new(START_MS))) as Arc<dyn Clock>,
        DeviceId::random(),
    );
    assert!(matches!(
        setup(&mut t, &passphrase(), &mut hlc, factor()).await,
        Err(SettingsSyncError::AlreadySetUp)
    ));
    assert_eq!(folder.entries(), before);
}

#[tokio::test]
async fn changed_passphrase_rewraps_the_key_and_devices_keep_working() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    settle(&mut [&mut a, &mut b]).await;
    let new = Secret::new("a much better passphrase".to_owned());

    // Interrupted: the new key record is written, the old one not deleted yet.
    folder.fail_deletes_after(Some(0));
    let mut t = folder.clone();
    assert!(
        change_passphrase(&mut t, &a.key, &new, &mut a.hlc, factor())
            .await
            .is_err()
    );
    folder.fail_deletes_after(None);
    assert_eq!(
        folder
            .entries()
            .iter()
            .filter(|(_, e)| e.header.opaque_id.starts_with(KEY_RECORD_PREFIX))
            .count(),
        2
    );
    // Both passphrases unlock the same key meanwhile; the next pass removes the old record.
    let with_new = join(&mut t, &new).await.expect("new passphrase");
    let with_old = join(&mut t, &passphrase()).await.expect("old passphrase");
    assert_eq!(with_new.opaque_id(&theme()), with_old.opaque_id(&theme()));
    let report = b.pass().await;
    assert_eq!(report.deleted, 1);
    assert!(matches!(
        join(&mut t, &passphrase()).await,
        Err(SettingsSyncError::WrongPassphrase)
    ));
    let c = Device::joining_with(&folder, 3, START_MS, &new).await;
    assert_eq!(c.key.opaque_id(&theme()), a.key.opaque_id(&theme()));
    a.put(&theme(), json!("light"));
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(b.value(&theme()), Some(json!("light")));
}

#[tokio::test]
async fn reset_makes_other_devices_join_again_and_merge() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    b.put(&signature(1), json!({ "text": "from b" }));
    settle(&mut [&mut a, &mut b]).await;

    let fresh = Secret::new("new passphrase after forgetting".to_owned());
    let mut t = folder.clone();
    a.key = reset(&mut t, &fresh, &mut a.state, &mut a.hlc, factor())
        .await
        .expect("reset");
    a.pass().await;
    assert!(matches!(
        b.try_pass().await,
        Err(SettingsSyncError::KeyChanged)
    ));
    assert!(matches!(
        join(&mut t, &passphrase()).await,
        Err(SettingsSyncError::WrongPassphrase)
    ));
    b.key = join(&mut t, &fresh).await.expect("join after reset");
    b.state.forget_sync();
    b.put(&language(), json!("de"));
    settle(&mut [&mut b, &mut a]).await;
    assert_eq!(a.live(), b.live());
    assert_eq!(a.value(&language()), Some(json!("de")));
    assert_eq!(b.value(&theme()), Some(json!("dark")));
}

#[tokio::test]
async fn removed_account_is_asked_about_not_silently_deleted() {
    let (_, mut a, mut b) = pair().await;
    a.put_fields(&account(1), account_fields("ann@example.org", "Ann"));
    a.put_fields(&account(2), account_fields("bob@example.org", "Bob"));
    settle(&mut [&mut a, &mut b]).await;
    assert!(b.value(&account(1)).is_some());

    a.advance(10);
    a.delete(&account(1));
    a.delete(&account(2));
    a.pass().await;
    let report = b.pass().await;
    assert_eq!(report.questions.len(), 2);
    assert!(
        b.value(&account(1)).is_some(),
        "still there until the user decides"
    );
    // Asked again on the next pass, nothing changes meanwhile.
    let again = b.pass().await;
    assert_eq!(again.questions, report.questions);
    assert!(again.is_quiet());

    // Account 1: remove here too. Account 2: keep, which brings it back everywhere.
    let q1 = report
        .questions
        .iter()
        .find(|q| q.key() == &account(1))
        .expect("q1");
    let q2 = report
        .questions
        .iter()
        .find(|q| q.key() == &account(2))
        .expect("q2");
    b.state.accept_removal(q1);
    b.state.keep(q2, &mut b.hlc).expect("keep");
    settle(&mut [&mut b, &mut a]).await;
    for device in [&a, &b] {
        assert_eq!(device.value(&account(1)), None);
        assert_eq!(
            device
                .value(&account(2))
                .and_then(|v| v.get("email").cloned()),
            Some(json!("bob@example.org"))
        );
    }
}

#[tokio::test]
async fn account_fields_changed_on_two_devices_are_merged() {
    let (_, mut a, mut b) = pair().await;
    a.put_fields(&account(1), account_fields("ann@example.org", "Ann"));
    settle(&mut [&mut a, &mut b]).await;
    a.advance(5);
    a.put_fields(&account(1), json!({ "displayName": "Ann Example" }));
    b.advance(7);
    b.put_fields(&account(1), json!({ "color": 5 }));
    settle(&mut [&mut a, &mut b]).await;
    for device in [&a, &b] {
        let value = device.value(&account(1)).expect("account");
        assert_eq!(value["displayName"], json!("Ann Example"));
        assert_eq!(value["color"], json!(5));
    }
    assert_eq!(a.live(), b.live());
    let content = a.state.content(&account(1)).expect("content");
    let synced = SyncedAccount::from_content(content).expect("typed");
    assert_eq!(synced.display_name, "Ann Example");
}

#[tokio::test]
async fn snooze_and_pin_follow_the_message_not_local_ids() {
    let (_, mut a, mut b) = pair().await;
    let pin = RecordKey::mail(RecordKind::Pin, account_id(1), "<x@example.org>").expect("pin");
    a.put(&snooze(1), json!({ "untilMs": START_MS + DAY_MS }));
    a.put(&pin, json!({}));
    settle(&mut [&mut a, &mut b]).await;
    // B un-snoozes later than A re-snoozes: the newer wins.
    a.advance(10);
    a.put(&snooze(1), json!({ "untilMs": START_MS + 2 * DAY_MS }));
    b.advance(20);
    b.delete(&snooze(1));
    b.delete(&pin);
    settle(&mut [&mut a, &mut b]).await;
    assert_eq!(a.value(&snooze(1)), None);
    assert_eq!(a.value(&pin), None);
    assert_eq!(a.live(), b.live());
}

#[tokio::test]
async fn record_from_a_device_with_a_fast_clock_waits() {
    let (folder, mut a, mut b) = pair().await;
    // A's clock runs a day ahead.
    a.set_time(START_MS + DAY_MS);
    a.put(&theme(), json!("dark"));
    a.pass().await;
    let report = b.pass().await;
    assert!(
        matches!(report.notices.as_slice(), [Notice::Deferred { .. }]),
        "{report:?}"
    );
    assert_eq!(b.value(&theme()), None);
    assert_eq!(folder.len(), 2, "not deleted, not merged");
    // B's own change of the same setting waits as well instead of racing the deferred one.
    b.put(&theme(), json!("light"));
    let report = b.pass().await;
    assert_eq!(report.uploaded, 0);
    // Once B's clock is within the bound, the record is merged.
    b.set_time(START_MS + DAY_MS - MAX_CLOCK_DRIFT_MS + 1);
    settle(&mut [&mut b, &mut a]).await;
    assert_eq!(a.value(&theme()), b.value(&theme()));
}

#[tokio::test]
async fn records_of_a_newer_app_version_are_left_alone() {
    let (folder, mut a, mut b) = pair().await;
    a.put(&theme(), json!("dark"));
    a.pass().await;
    // A newer app writes a kind this version does not know.
    let header = RawHeader {
        opaque_id: "00112233445566778899aabbccddeeff".to_owned(),
        version: a.hlc.now().expect("now").to_string(),
    };
    let payload = json!({
        "format": 1, "opaqueId": header.opaque_id, "version": header.version,
        "kind": "calendar", "id": "x", "content": {}
    });
    let identity = a.key.export();
    let identity: age::x25519::Identity = identity.expose_secret().parse().expect("identity");
    let body = age::encrypt(&identity.to_public(), payload.to_string().as_bytes()).expect("enc");
    let handle = folder.insert_raw(header, body);
    let report = b.pass().await;
    assert!(report.notices.iter().any(|n| matches!(
        n,
        Notice::Rejected { handle: h, error: OpenError::Unsupported(_) } if *h == handle
    )));
    assert_eq!(b.value(&theme()), Some(json!("dark")));
    settle(&mut [&mut a, &mut b]).await;
    assert!(
        folder.entries().iter().any(|(h, _)| *h == handle),
        "kept on the server"
    );
}

#[tokio::test]
async fn unset_folder_and_foreign_key_are_typed_errors() {
    let (folder, mut a, _) = pair().await;
    // Every entry gone (folder deleted in another client).
    for (handle, _) in folder.entries() {
        folder.remove_raw(handle);
    }
    assert!(matches!(
        a.try_pass().await,
        Err(SettingsSyncError::NotSetUp)
    ));
    folder.set_offline(true);
    assert!(matches!(
        a.try_pass().await,
        Err(SettingsSyncError::Transport(TransportError::Unavailable(_)))
    ));
}

/// Passwords, tokens and API keys appear in no record: not in the content model, not in a
/// sealed record (header or ciphertext), not in the decrypted payload.
#[tokio::test]
async fn passwords_tokens_and_api_keys_appear_in_no_record() {
    const PASSWORD: &str = "imap-password-4711";
    const API_KEY: &str = "sk-test-api-key-0815";
    const MCP_TOKEN: &str = "mcp-client-token-2342";
    let password = Secret::new(PASSWORD.to_owned());

    // 1. The model refuses secrets wherever they are put.
    for (kind, value) in [
        (
            RecordKind::Account,
            json!({ "password": { "version": "x", "value": PASSWORD } }),
        ),
        (
            RecordKind::AiSettings,
            json!({ "provider": "openai", "apiKey": API_KEY }),
        ),
        (
            RecordKind::AiSettings,
            json!({ "mcpClients": [{ "name": "cli", "token": MCP_TOKEN }] }),
        ),
        (RecordKind::Identity, json!({ "smtpPassword": PASSWORD })),
        (
            RecordKind::Setting,
            json!({ "Authorization": format!("Bearer {MCP_TOKEN}") }),
        ),
    ] {
        assert!(SyncContent::new(kind, value).is_err(), "{kind}");
    }
    // A struct holding `Secret` cannot become content: `Secret` has no `Serialize` (this would
    // not compile: `SyncContent::from_serialize(RecordKind::Identity, &password)`).
    // The account model has no password field and refuses unknown fields.
    let typed = mail_model::ServerSettings {
        host: "imap.example.org".into(),
        port: 993,
        security: mail_model::Security::Tls,
        username: "ann".into(),
    };
    let synced = SyncedAccount {
        email: "ann@example.org".into(),
        display_name: "Ann".into(),
        color: mail_model::AccountColor::new(1).expect("color"),
        imap: typed.clone(),
        smtp: typed,
    };
    let values = synced.to_values().expect("values");
    assert_eq!(values.keys().map(String::as_str).collect::<Vec<_>>(), {
        let mut fields = ACCOUNT_FIELDS.to_vec();
        fields.sort_unstable();
        fields
    });

    // 2. Realistic records of every kind, sealed and opened again.
    let (folder, mut a, mut b) = pair().await;
    a.state
        .put_fields(account(1), values, &mut a.hlc)
        .expect("account");
    a.put(&theme(), json!("dark"));
    a.put(&signature(1), json!({ "html": "<p>Ann</p>" }));
    a.put(&snooze(1), json!({ "untilMs": START_MS }));
    for (kind, value) in [
        (
            RecordKind::Template,
            json!({ "name": "t", "body": "Hello" }),
        ),
        (
            RecordKind::Rule,
            json!({ "if": { "from": "x@example.org" }, "then": "archive" }),
        ),
        (RecordKind::SavedView, json!({ "query": "is:unread" })),
        (
            RecordKind::Identity,
            json!({ "email": "ann@example.org", "name": "Ann" }),
        ),
        (
            RecordKind::AiSettings,
            json!({ "provider": "openai-compatible", "baseUrl": "http://localhost:11434/v1",
                    "model": "llama", "maxTokens": 512, "mcp": { "access": "read" } }),
        ),
        (
            RecordKind::Device,
            json!({ "name": "Mac", "system": "macOS", "lastSeenMs": START_MS }),
        ),
    ] {
        let key = RecordKey::uuid(kind, uuid::Uuid::from_u128(77)).expect("key");
        a.put(&key, value);
    }
    a.pass().await;
    b.pass().await;
    assert_eq!(a.live(), b.live());
    assert_eq!(a.live().len(), 10);

    let forbidden = [
        PASSWORD.to_owned(),
        API_KEY.to_owned(),
        MCP_TOKEN.to_owned(),
        password.expose_secret().clone(),
        passphrase().expose_secret().clone(),
        a.key.export().expose_secret().clone(),
    ];
    for (handle, entry) in folder.entries() {
        let mut texts = vec![
            entry.header.opaque_id.clone(),
            entry.header.version.clone(),
            String::from_utf8_lossy(&entry.body).into_owned(),
        ];
        if let Some(header) = RecordHeader::parse(&entry.header)
            && !header.opaque_id.is_key_record()
        {
            let record = a.key.open(&header, &entry.body).expect("open");
            texts.push(format!("{record:?}"));
            if let Some(content) = record.content_ref() {
                texts.push(content.value().to_string());
            }
        }
        for text in &texts {
            for secret in &forbidden {
                assert!(!text.contains(secret.as_str()), "{handle:?} leaks a secret");
            }
        }
    }
}
