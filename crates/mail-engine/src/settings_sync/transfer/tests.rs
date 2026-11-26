//! The protocol of a password transfer without IO: entries, codes, sealing, and what a device
//! decides from a listing.

use mail_model::Security;

use super::*;
use crate::settings_sync::hlc::DeviceId;

const NOW: i64 = 1_764_000_000_000;

fn device(n: u128) -> DeviceId {
    DeviceId::from_uuid(uuid::Uuid::from_u128(n))
}

fn version(millis: i64, n: u128) -> Hlc {
    Hlc::new(millis, 0, device(n)).expect("version")
}

fn server(host: &str) -> ServerSettings {
    ServerSettings {
        host: host.into(),
        port: 993,
        security: Security::Tls,
        username: "alice".into(),
    }
}

fn wanted(n: u128) -> WantedAccount {
    WantedAccount {
        account: AccountId::from_uuid(uuid::Uuid::from_u128(0xacc0 + n)),
        sign_in: CredentialKind::Password,
        imap: server("imap.example.org"),
        smtp: server("smtp.example.org"),
    }
}

fn request(key: &RequestKey, id: &RequestId, from: DeviceId) -> TransferEntry {
    TransferEntry {
        request_id: id.clone(),
        device: from,
        expires_ms: NOW + TRANSFER_LIFETIME_MS,
        message: TransferMessage::Request {
            device_name: "New Mac".into(),
            commitment: commitment(id, key.public_key()),
            accounts: vec![wanted(1), wanted(2)],
        },
    }
}

fn opened(key: &SyncKey, entry: TransferEntry, handle: u64, at: i64) -> OpenedEntry {
    let v = Hlc::new(at, 0, entry.device).expect("version");
    let sealed = key.seal_transfer(&entry, v).expect("seal");
    let back = key
        .open_transfer(&sealed.header, &sealed.ciphertext)
        .expect("open");
    assert_eq!(back, entry);
    OpenedEntry {
        handle: EntryHandle(handle),
        header: sealed.header,
        entry: back,
    }
}

#[test]
fn entries_round_trip_and_are_bound_to_their_header() {
    let key = SyncKey::generate();
    let rk = RequestKey::generate();
    let id = RequestId::random();
    let entry = request(&rk, &id, device(1));
    let sealed = key.seal_transfer(&entry, version(NOW, 1)).expect("seal");
    assert!(sealed.header.opaque_id.is_transfer());
    assert!(sealed.header.opaque_id.as_str().starts_with("xfer-"));
    assert_eq!(
        OpaqueId::parse(sealed.header.opaque_id.as_str()),
        Some(sealed.header.opaque_id.clone())
    );
    let raw = String::from_utf8_lossy(&sealed.ciphertext);
    assert!(!raw.contains("New Mac") && !raw.contains("imap.example.org"));
    assert_eq!(
        key.open_transfer(&sealed.header, &sealed.ciphertext),
        Ok(entry.clone())
    );

    // Another key, another version, another id: rejected.
    assert_eq!(
        SyncKey::generate().open_transfer(&sealed.header, &sealed.ciphertext),
        Err(OpenError::WrongKey)
    );
    let later = RecordHeader {
        version: version(NOW + 1, 1),
        ..sealed.header.clone()
    };
    assert_eq!(
        key.open_transfer(&later, &sealed.ciphertext),
        Err(OpenError::HeaderMismatch("version"))
    );
    let challenge_id = RecordHeader {
        opaque_id: key.transfer_id(TransferKind::Challenge, &id, device(1)),
        ..sealed.header.clone()
    };
    assert_eq!(
        key.open_transfer(&challenge_id, &sealed.ciphertext),
        Err(OpenError::HeaderMismatch("opaque id"))
    );
    // A record header is not a transfer entry.
    let record = RecordHeader {
        opaque_id: OpaqueId::parse("00112233445566778899aabbccddeeff").expect("id"),
        version: version(NOW, 1),
    };
    assert!(key.open_transfer(&record, &sealed.ciphertext).is_err());
    // Ids differ per kind, request and device, and per key.
    let ids = [
        key.transfer_id(TransferKind::Request, &id, device(1)),
        key.transfer_id(TransferKind::Reveal, &id, device(1)),
        key.transfer_id(TransferKind::Request, &id, device(2)),
        key.transfer_id(TransferKind::Request, &RequestId::random(), device(1)),
        SyncKey::generate().transfer_id(TransferKind::Request, &id, device(1)),
    ];
    let distinct: BTreeSet<_> = ids.iter().collect();
    assert_eq!(distinct.len(), ids.len());
}

#[test]
fn malformed_entries_are_refused() {
    let key = SyncKey::generate();
    let rk = RequestKey::generate();
    let id = RequestId::random();
    let bad = [
        TransferMessage::Request {
            device_name: String::new(),
            commitment: commitment(&id, rk.public_key()),
            accounts: vec![wanted(1)],
        },
        TransferMessage::Request {
            device_name: "x".into(),
            commitment: "short".into(),
            accounts: vec![wanted(1)],
        },
        TransferMessage::Request {
            device_name: "x".into(),
            commitment: commitment(&id, rk.public_key()),
            accounts: vec![],
        },
        TransferMessage::Reveal {
            public_key: "age1notakey".into(),
            challenge_device: device(2).to_string(),
            challenge_nonce: nonce(),
        },
        TransferMessage::Response {
            device_name: "Old Mac".into(),
            declined: false,
            sealed: None,
        },
        TransferMessage::Response {
            device_name: "Old Mac".into(),
            declined: true,
            sealed: Some("x".into()),
        },
    ];
    for message in bad {
        let entry = TransferEntry {
            request_id: id.clone(),
            device: device(1),
            expires_ms: NOW,
            message,
        };
        let sealed = key.seal_transfer(&entry, version(NOW, 1)).expect("seal");
        assert!(
            matches!(
                key.open_transfer(&sealed.header, &sealed.ciphertext),
                Err(OpenError::Malformed(_))
            ),
            "{entry:?}"
        );
    }
}

#[test]
fn codes_are_six_digits_and_bind_key_and_nonce() {
    let id = RequestId::random();
    let a = RequestKey::generate();
    let b = RequestKey::generate();
    let n1 = nonce();
    let n2 = nonce();
    let c = code(&id, a.public_key(), &n1);
    assert_eq!(c.len(), 7);
    assert!(c.chars().enumerate().all(|(i, ch)| if i == 3 {
        ch == ' '
    } else {
        ch.is_ascii_digit()
    }));
    assert_eq!(c, code(&id, a.public_key(), &n1));
    // Another key or nonce gives another code (with overwhelming probability; fixed here).
    let others = [
        code(&id, b.public_key(), &n1),
        code(&id, a.public_key(), &n2),
        code(&RequestId::random(), a.public_key(), &n1),
    ];
    assert!(others.iter().filter(|o| **o == c).count() <= 1);
    assert_ne!(
        commitment(&id, a.public_key()),
        commitment(&id, b.public_key())
    );
    assert_eq!(nonce().len(), 64);
    assert_eq!(RequestId::random().as_str().len(), 32);
    assert!(RequestId::parse("ABCDEF0123456789abcdef0123456789").is_none());
}

#[test]
fn sealed_passwords_open_only_with_the_request_key_and_request() {
    let rk = RequestKey::generate();
    let id = RequestId::random();
    let item = GivenItem::new(
        wanted(1).account,
        CredentialKind::Password,
        server("imap.example.org"),
        server("smtp.example.org"),
        &Secret::new("correct horse 7%".into()),
    );
    let sealed = seal_items(rk.public_key(), &id, vec![item.clone()]).expect("seal");
    assert!(sealed.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
    assert!(!sealed.contains("correct horse"));
    let items = rk.open(&id, &sealed).expect("open");
    assert_eq!(items, vec![item.clone()]);
    assert_eq!(items[0].secret().expose_secret(), "correct horse 7%");
    assert!(!format!("{item:?}").contains("correct horse"));

    assert_eq!(
        RequestKey::generate().open(&id, &sealed),
        Err(UnsealError::WrongKey)
    );
    assert_eq!(
        rk.open(&RequestId::random(), &sealed),
        Err(UnsealError::OtherRequest)
    );
    let mut damaged = sealed.clone();
    damaged.insert_str(60, "AAAA");
    assert!(rk.open(&id, &damaged).is_err());
    assert!(format!("{rk:?}").contains("redacted"));
}

/// The whole exchange between a new device (1) and a giving device (2), as both see the folder.
#[test]
fn a_request_is_challenged_revealed_and_answered() {
    let key = SyncKey::generate();
    let rk = RequestKey::generate();
    let id = RequestId::random();
    let (new, old) = (device(1), device(2));
    let mut own = OwnRequest {
        request_id: id.clone(),
        expires_ms: NOW + TRANSFER_LIFETIME_MS,
        accounts: vec![wanted(1), wanted(2)],
        chosen: None,
        key: RequestKey::generate(),
    };
    own.key = rk;
    let mut folder = vec![opened(&key, request(&own.key, &id, new), 1, NOW)];
    let mut giving_new = BTreeMap::new();
    let mut giving_old = BTreeMap::new();

    // The new device sees only its own request; the old one sees it as a request.
    let plan = plan_transfers(&folder, NOW, new, Some(&mut own), &mut giving_new);
    assert!(plan.writes.is_empty() && plan.deletes.is_empty() && !plan.own_missing);
    assert!(plan.seen.is_empty());
    let plan = plan_transfers(&folder, NOW, old, None, &mut giving_old);
    assert_eq!(plan.seen.len(), 1);
    assert_eq!(plan.seen[0].device_name, "New Mac");
    assert_eq!(plan.seen[0].accounts, vec![wanted(1), wanted(2)]);

    // The old device challenges.
    let n = nonce();
    giving_old.insert(
        id.clone(),
        Giving {
            nonce: n.clone(),
            state: GivingState::Connecting,
        },
    );
    folder.push(opened(
        &key,
        TransferEntry {
            request_id: id.clone(),
            device: old,
            expires_ms: own.expires_ms,
            message: TransferMessage::Challenge {
                device_name: "Old Mac".into(),
                nonce: n.clone(),
            },
        },
        2,
        NOW + 1,
    ));

    // The new device chooses it and reveals its key.
    let plan = plan_transfers(&folder, NOW + 2, new, Some(&mut own), &mut giving_new);
    let chosen = own.chosen.clone().expect("chosen");
    assert_eq!(chosen.device, old);
    assert_eq!(chosen.device_name, "Old Mac");
    assert_eq!(plan.writes.len(), 1);
    let reveal = plan.writes[0].clone();
    assert!(
        matches!(&reveal.message, TransferMessage::Reveal { public_key, .. } if public_key == own.key.public_key())
    );
    folder.push(opened(&key, reveal, 3, NOW + 3));
    // Not written twice.
    assert!(
        plan_transfers(&folder, NOW + 3, new, Some(&mut own), &mut giving_new)
            .writes
            .is_empty()
    );

    // The old device checks the key against the commitment: the same code.
    plan_transfers(&folder, NOW + 4, old, None, &mut giving_old);
    let GivingState::Confirm { public_key, code } = &giving_old[&id].state else {
        panic!("{:?}", giving_old[&id]);
    };
    assert_eq!(public_key, own.key.public_key());
    assert_eq!(*code, chosen.code);

    // The response reaches the new device.
    let sealed = seal_items(public_key, &id, vec![]).expect("seal");
    folder.push(opened(
        &key,
        TransferEntry {
            request_id: id.clone(),
            device: old,
            expires_ms: own.expires_ms,
            message: TransferMessage::Response {
                device_name: "Old Mac".into(),
                declined: false,
                sealed: Some(sealed),
            },
        },
        4,
        NOW + 5,
    ));
    let plan = plan_transfers(&folder, NOW + 6, new, Some(&mut own), &mut giving_new);
    assert!(matches!(
        plan.response,
        Some(TransferMessage::Response {
            declined: false,
            ..
        })
    ));
}

#[test]
fn a_substituted_key_or_a_late_challenge_is_noticed() {
    let key = SyncKey::generate();
    let rk = RequestKey::generate();
    let id = RequestId::random();
    let (new, old, other) = (device(1), device(2), device(3));
    let request = opened(&key, request(&rk, &id, new), 1, NOW);
    let n = nonce();
    let challenge = |from: DeviceId, nonce: &str, handle: u64| {
        opened(
            &key,
            TransferEntry {
                request_id: id.clone(),
                device: from,
                expires_ms: NOW + TRANSFER_LIFETIME_MS,
                message: TransferMessage::Challenge {
                    device_name: "Mac".into(),
                    nonce: nonce.to_owned(),
                },
            },
            handle,
            NOW + i64::try_from(handle).expect("small"),
        )
    };
    let reveal = |public_key: &str, to: DeviceId, nonce: &str| {
        opened(
            &key,
            TransferEntry {
                request_id: id.clone(),
                device: new,
                expires_ms: NOW + TRANSFER_LIFETIME_MS,
                message: TransferMessage::Reveal {
                    public_key: public_key.to_owned(),
                    challenge_device: to.to_string(),
                    challenge_nonce: nonce.to_owned(),
                },
            },
            9,
            NOW + 9,
        )
    };
    let connecting = |nonce: &str| {
        BTreeMap::from([(
            id.clone(),
            Giving {
                nonce: nonce.to_owned(),
                state: GivingState::Connecting,
            },
        )])
    };

    // Someone in the folder reveals another key than the request committed to.
    let mut giving = connecting(&n);
    let folder = vec![
        request.clone(),
        challenge(old, &n, 2),
        reveal(RequestKey::generate().public_key(), old, &n),
    ];
    plan_transfers(&folder, NOW + 10, old, None, &mut giving);
    assert_eq!(giving[&id].state, GivingState::Failed);

    // The new device answered another device's challenge.
    let n3 = nonce();
    let mut giving = connecting(&n);
    let folder = vec![
        request.clone(),
        challenge(other, &n3, 2),
        challenge(old, &n, 3),
        reveal(rk.public_key(), other, &n3),
    ];
    plan_transfers(&folder, NOW + 10, old, None, &mut giving);
    assert_eq!(giving[&id].state, GivingState::OtherDevice);

    // The new device takes the first challenge only, and only that device's response.
    let mut own = OwnRequest {
        request_id: id.clone(),
        key: rk,
        expires_ms: NOW + TRANSFER_LIFETIME_MS,
        accounts: vec![wanted(1)],
        chosen: None,
    };
    let response = |from: DeviceId, handle: u64| {
        opened(
            &key,
            TransferEntry {
                request_id: id.clone(),
                device: from,
                expires_ms: NOW + TRANSFER_LIFETIME_MS,
                message: TransferMessage::Response {
                    device_name: "Mac".into(),
                    declined: true,
                    sealed: None,
                },
            },
            handle,
            NOW + 20,
        )
    };
    let folder = vec![
        request.clone(),
        challenge(other, &n3, 2),
        challenge(old, &n, 3),
        response(old, 4),
    ];
    let plan = plan_transfers(&folder, NOW + 21, new, Some(&mut own), &mut BTreeMap::new());
    assert_eq!(own.chosen.as_ref().map(|c| c.device), Some(other));
    assert!(plan.response.is_none(), "not from the chosen device");
    let mut folder = folder;
    folder.push(response(other, 5));
    let plan = plan_transfers(&folder, NOW + 22, new, Some(&mut own), &mut BTreeMap::new());
    assert!(plan.response.is_some());
}

#[test]
fn expired_stale_and_orphaned_entries_are_deleted() {
    let key = SyncKey::generate();
    let rk = RequestKey::generate();
    let id = RequestId::random();
    let (new, old) = (device(1), device(2));
    let live_request = opened(&key, request(&rk, &id, new), 1, NOW);
    let challenge = opened(
        &key,
        TransferEntry {
            request_id: id.clone(),
            device: old,
            expires_ms: NOW + TRANSFER_LIFETIME_MS,
            message: TransferMessage::Challenge {
                device_name: "Old".into(),
                nonce: nonce(),
            },
        },
        2,
        NOW,
    );
    // Before expiry plus grace nothing goes; after, everything of the request.
    let folder = vec![live_request.clone(), challenge.clone()];
    let at_expiry = NOW + TRANSFER_LIFETIME_MS;
    assert!(
        plan_transfers(&folder, at_expiry, old, None, &mut BTreeMap::new())
            .deletes
            .is_empty()
    );
    let plan = plan_transfers(
        &folder,
        at_expiry + TRANSFER_CLEANUP_GRACE_MS + 1,
        old,
        None,
        &mut BTreeMap::new(),
    );
    assert_eq!(plan.deletes, vec![EntryHandle(1), EntryHandle(2)]);
    assert!(plan.seen.is_empty());

    // The new device restarted: its old request has no key any more and goes at once.
    let plan = plan_transfers(&folder, NOW + 1, new, None, &mut BTreeMap::new());
    assert_eq!(plan.deletes, vec![EntryHandle(1), EntryHandle(2)]);

    // An answer whose request is gone is deleted by anyone.
    let plan = plan_transfers(&[challenge], NOW + 1, device(9), None, &mut BTreeMap::new());
    assert_eq!(plan.deletes, vec![EntryHandle(2)]);

    // Entries older than lifetime plus grace by their header go without being read.
    let listing: Vec<RemoteEntry> = [
        (
            1,
            NOW - TRANSFER_LIFETIME_MS - TRANSFER_CLEANUP_GRACE_MS - 1,
            100,
        ),
        (2, NOW - 1000, 100),
        (3, NOW, MAX_TRANSFER_ENTRY_BYTES + 1),
    ]
    .into_iter()
    .map(|(h, at, size)| RemoteEntry {
        handle: EntryHandle(h),
        header: RecordHeader {
            opaque_id: key.transfer_id(TransferKind::Request, &RequestId::random(), new),
            version: version(at, 1),
        }
        .to_raw(),
        size,
    })
    .collect();
    let sorted = transfer_listing(&listing, NOW);
    assert_eq!(sorted.expired, vec![EntryHandle(1)]);
    assert_eq!(
        sorted.fetch.iter().map(|(h, _)| *h).collect::<Vec<_>>(),
        vec![EntryHandle(2)]
    );
}
