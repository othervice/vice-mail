//! Convergence: random changes, passes, crashes, replays and clock jumps on two and three
//! devices with fixed seeds. Afterwards every device holds the same records (same versions,
//! same content) and the folder holds exactly one entry per record.

use serde_json::json;

use super::*;

/// SplitMix64: a small deterministic generator for test data (not for anything secret).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = usize::try_from(self.below(items.len() as u64)).unwrap_or(0);
        &items[i]
    }
}

fn pool() -> Vec<RecordKey> {
    vec![
        theme(),
        language(),
        signature(1),
        signature(2),
        signature(3),
        snooze(1),
        snooze(2),
        account(1),
        account(2),
    ]
}

fn change(device: &mut Device, key: &RecordKey, rng: &mut Rng) {
    let n = rng.below(1000);
    match key.kind() {
        RecordKind::Setting if key == &theme() => {
            device.put(key, json!(rng.pick(&["dark", "light", "system"])));
        }
        RecordKind::Setting => device.put(key, json!(rng.pick(&["en", "de"]))),
        RecordKind::Signature => device.put(key, json!({ "text": format!("s{n}") })),
        RecordKind::Snooze => device.put(
            key,
            json!({ "untilMs": START_MS + i64::try_from(n).unwrap_or(0) }),
        ),
        RecordKind::Account => {
            if device.value(key).is_none() {
                device.put_fields(key, account_fields(&format!("{n}@example.org"), "New"));
            } else if rng.below(2) == 0 {
                device.put_fields(key, json!({ "displayName": format!("N{n}") }));
            } else {
                device.put_fields(key, json!({ "color": n % 8 }));
            }
        }
        other => panic!("not in the pool: {other}"),
    }
}

/// What the random histories exercised.
#[derive(Debug, Default)]
struct Stats {
    passes: usize,
    uploads: usize,
    deletions: usize,
    failed_writes: usize,
    questions: usize,
    replays: usize,
    long_breaks: usize,
    worst_rounds: usize,
}

/// Runs one random history and checks convergence.
async fn run(seed: u64, device_count: u128, steps: usize, stats: &mut Stats) {
    let folder = MemoryTransport::new();
    let mut devices = vec![Device::first(&folder, 1).await];
    for id in 2..=device_count {
        devices.push(Device::joining(&folder, id, START_MS).await);
    }
    let mut rng = Rng(seed);
    // Clocks differ by up to ±2 s between devices.
    let skews: Vec<i64> = devices
        .iter()
        .map(|_| i64::try_from(rng.below(4_001)).unwrap_or(0) - 2_000)
        .collect();
    let mut time = START_MS;
    let mut history: Vec<StoredEntry> = Vec::new();
    let keys = pool();
    for _ in 0..steps {
        let i = usize::try_from(rng.below(devices.len() as u64)).unwrap_or(0);
        let device = &mut devices[i];
        match rng.below(100) {
            0..=34 => {
                let key = rng.pick(&keys).clone();
                change(device, &key, &mut rng);
            }
            35..=44 => {
                let key = rng.pick(&keys).clone();
                device.delete(&key);
            }
            45..=79 => {
                // Sometimes the connection drops after a few writes or deletes.
                match rng.below(10) {
                    0 => folder.fail_writes_after(Some(rng.below(2))),
                    1 => folder.fail_deletes_after(Some(rng.below(2))),
                    _ => {}
                }
                let report = device.pass().await;
                stats.passes += 1;
                stats.uploads += report.uploaded;
                stats.deletions += report.deleted;
                stats.failed_writes += report.failed.len();
                stats.questions += report.questions.len();
                folder.fail_writes_after(None);
                folder.fail_deletes_after(None);
                for question in &report.questions {
                    if rng.below(2) == 0 {
                        device.state.accept_removal(question);
                    } else {
                        device.state.keep(question, &mut device.hlc).expect("keep");
                    }
                }
                for (_, entry) in folder.entries() {
                    if history.len() < 300 && !entry.header.opaque_id.starts_with(KEY_RECORD_PREFIX)
                    {
                        history.push(entry);
                    }
                }
            }
            80..=82 if !history.is_empty() => {
                // The server plays an old entry in again.
                let old = rng.pick(&history).clone();
                folder.insert_raw(old.header, old.body);
                stats.replays += 1;
            }
            83..=97 => time += i64::try_from(rng.below(3_000)).unwrap_or(0),
            // Rarely, everyone is away for up to 250 days: tombstones expire meanwhile.
            _ => {
                time += i64::try_from(rng.below(250)).unwrap_or(0) * DAY_MS;
                stats.long_breaks += 1;
            }
        }
        for (device, skew) in devices.iter().zip(&skews) {
            device.set_time(time + skew);
        }
    }

    time += 10_000;
    for (device, skew) in devices.iter().zip(&skews) {
        device.set_time(time + skew);
    }
    let rounds = {
        let mut all: Vec<&mut Device> = devices.iter_mut().collect();
        settle(&mut all).await
    };
    stats.worst_rounds = stats.worst_rounds.max(rounds);
    let expected = devices[0].live();
    for (n, device) in devices.iter().enumerate().skip(1) {
        assert_eq!(
            device.live(),
            expected,
            "seed {seed}: device {} differs",
            n + 1
        );
    }
    let per_record = entries_per_record(&folder);
    assert!(
        per_record.values().all(|n| *n == 1),
        "seed {seed}: duplicates left {per_record:?}"
    );
    // Every live record is on the server under its version.
    let on_server: Vec<(String, String)> = folder
        .entries()
        .into_iter()
        .map(|(_, e)| (e.header.opaque_id, e.header.version))
        .collect();
    for (key, (version, _)) in &expected {
        let id = devices[0].key.opaque_id(key).to_string();
        assert!(
            on_server.contains(&(id, version.to_string())),
            "seed {seed}: {key} missing on the server"
        );
    }
}

/// Seeds per test; `SETTINGS_SYNC_SEEDS=500` searches deeper.
fn seeds() -> u64 {
    std::env::var("SETTINGS_SYNC_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12)
}

#[tokio::test]
async fn two_devices_converge_after_random_histories() {
    let mut stats = Stats::default();
    for seed in 0..seeds() {
        run(seed, 2, 160, &mut stats).await;
    }
    eprintln!("two devices: {stats:?}");
    assert!(stats.questions > 0 && stats.replays > 0 && stats.long_breaks > 0);
}

#[tokio::test]
async fn three_devices_converge_after_random_histories() {
    let mut stats = Stats::default();
    for seed in 100..100 + seeds() {
        run(seed, 3, 220, &mut stats).await;
    }
    eprintln!("three devices: {stats:?}");
    assert!(stats.failed_writes > 0 && stats.questions > 0 && stats.long_breaks > 0);
}
