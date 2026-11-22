//! Benchmark, ignored by default: message bodies as files (the chosen design, decision 0009)
//! against zstd-compressed BLOBs in the database. Both through the async API of the store with
//! the same pragmas, the same compression and two hops (database thread, blocking pool).
//!
//! ```text
//! cargo test -p mail-store --release -- --ignored --nocapture bench_body
//! VICEMAIL_BODY_SAMPLES=<dir with small.eml newsletter.eml photo.eml large.eml> ...
//! ```
//!
//! Without `VICEMAIL_BODY_SAMPLES` the samples are generated (prose-like text, random
//! attachments in base64), which compress about like real mail.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mail_model::{MailboxId, MessageHeader, MessageId};

use crate::Store;
use crate::body_files;
use crate::test_support::{Fixture, UV, fixture, header, seen};

/// (name, target size, bodies of that size). 460 bodies, about 84 MB raw.
const CLASSES: [(&str, usize, usize); 4] = [
    ("small.eml", 5_000, 200),
    ("newsletter.eml", 50_000, 200),
    ("photo.eml", 500_000, 50),
    ("large.eml", 5_000_000, 10),
];
const HEADERS: u32 = 10_000;

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

const WORDS: &[&str] = &[
    "der", "die", "und", "in", "den", "von", "zu", "das", "mit", "sich", "des", "auf", "für",
    "ist", "im", "dem", "nicht", "ein", "Die", "eine", "als", "auch", "es", "an", "werden", "aus",
    "er", "hat", "dass", "sie", "nach", "wird", "bei", "einer", "Der", "um", "am", "sind", "noch",
    "wie", "einem", "über", "einen", "Das", "so", "Sie", "zum", "war", "haben", "nur", "oder",
    "aber", "vor", "zur", "bis", "mehr", "durch", "man", "sein", "wurde", "Rechnung", "Termin",
    "Projekt", "Woche", "Bericht", "Anhang", "Grüße", "Konto", "Stufe", "the", "of", "and", "to",
    "a", "in", "is", "that", "for", "it", "with", "as", "was", "on", "meeting", "invoice",
    "attached", "please", "thanks", "report", "update", "review",
];

fn prose(rng: &mut Rng, len: usize) -> String {
    let mut out = String::with_capacity(len + 16);
    while out.len() < len {
        let index = usize::try_from(rng.next() % WORDS.len() as u64).unwrap_or(0);
        out.push_str(WORDS[index]);
        out.push(if rng.next().is_multiple_of(12) {
            '\n'
        } else {
            ' '
        });
    }
    out
}

fn base64_random(rng: &mut Rng, bytes: usize) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes * 4 / 3 + bytes / 57 * 2);
    let mut line = 0;
    for _ in 0..bytes * 4 / 3 {
        let index = usize::try_from(rng.next() % 64).unwrap_or(0);
        out.push(char::from(ALPHABET[index]));
        line += 1;
        if line == 76 {
            out.push_str("\r\n");
            line = 0;
        }
    }
    out
}

/// A generated sample of about `size` bytes: text below 100 KB, text plus a random
/// attachment above.
fn generated(size: usize, seed: u64) -> Vec<u8> {
    let mut rng = Rng(seed);
    let mut mail = String::from(
        "From: Sender <sender@example.org>\r\nTo: alice@example.org\r\nSubject: Bench\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\n",
    );
    let text = if size < 100_000 { size } else { 20_000 };
    mail.push_str(&prose(&mut rng, text.saturating_sub(300)));
    if size >= 100_000 {
        mail.push_str(
            "\r\n--b\r\nContent-Type: application/pdf\r\nContent-Transfer-Encoding: base64\r\n\r\n",
        );
        mail.push_str(&base64_random(&mut rng, (size - text) * 3 / 4));
    }
    mail.push_str("\r\n--b--\r\n");
    mail.into_bytes()
}

/// (file name, content, bodies of that size).
type Sample = (String, Vec<u8>, usize);

fn samples() -> (Vec<Sample>, &'static str) {
    let dir = std::env::var_os("VICEMAIL_BODY_SAMPLES").map(PathBuf::from);
    let mut out = Vec::new();
    for (i, (name, size, count)) in CLASSES.iter().enumerate() {
        let data = match &dir {
            Some(dir) => std::fs::read(dir.join(name))
                .unwrap_or_else(|err| panic!("read sample {name}: {err}")),
            None => generated(*size, 42 + i as u64),
        };
        out.push(((*name).to_owned(), data, *count));
    }
    (
        out,
        if dir.is_some() {
            "samples from VICEMAIL_BODY_SAMPLES"
        } else {
            "generated samples"
        },
    )
}

/// A copy of `sample` that differs from every other copy (its own Message-ID line in front).
fn unique(sample: &[u8], n: usize) -> Vec<u8> {
    let mut data = format!("X-Bench-Copy: {n}\r\n").into_bytes();
    data.extend_from_slice(sample);
    data
}

async fn with_headers() -> (Fixture, Vec<MessageId>) {
    let f = fixture().await;
    let headers: Vec<MessageHeader> = (1..=HEADERS)
        .map(|uid| {
            seen(header(
                uid,
                1_700_000_000 + i64::from(uid) * 300,
                Some(&format!("{uid}@bench")),
            ))
        })
        .collect();
    for window in headers.chunks(500) {
        f.store
            .store_headers(f.inbox, UV, window.to_vec(), None)
            .await
            .expect("headers");
    }
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = f
            .store
            .list_messages(f.inbox, cursor, 500)
            .await
            .expect("list");
        ids.extend(page.items.iter().map(|i| i.id));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    (f, ids)
}

fn stats(mut times: Vec<Duration>) -> (f64, f64) {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    let p95 = times
        .get((times.len() * 95 / 100).min(times.len().saturating_sub(1)))
        .copied()
        .unwrap_or_default();
    (ms(median), ms(p95))
}

async fn list_page_ms(store: &Store, mailbox: MailboxId) -> f64 {
    let mut times = Vec::new();
    for _ in 0..300 {
        let started = Instant::now();
        let page = store.list_messages(mailbox, None, 100).await.expect("list");
        times.push(started.elapsed());
        assert_eq!(page.items.len(), 100);
    }
    stats(times).0
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn wal_len(db: &Path) -> u64 {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    file_len(Path::new(&wal))
}

fn dir_bytes(dir: &Path) -> (u64, usize) {
    let mut total = 0;
    let mut files = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let (b, n) = dir_bytes(&path);
                total += b;
                files += n;
            } else {
                total += file_len(&path);
                files += 1;
            }
        }
    }
    (total, files)
}

/// Moves the WAL into the database file, so file sizes compare.
fn checkpoint(db: &Path) {
    let conn = rusqlite::Connection::open(db).expect("open for checkpoint");
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .expect("checkpoint");
}

/// `VACUUM INTO` (the backup before every migration): time and size.
fn backup(db: &Path) -> (f64, u64) {
    let conn = rusqlite::Connection::open(db).expect("open for backup");
    let target = db.with_extension("bench-backup");
    let started = Instant::now();
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .expect("vacuum into");
    let ms = started.elapsed().as_secs_f64() * 1e3;
    let size = file_len(&target);
    std::fs::remove_file(&target).ok();
    (ms, size)
}

async fn blob_write(store: &Store, id: MessageId, raw: Vec<u8>) {
    let compressed = tokio::task::spawn_blocking(move || body_files::compress(&raw))
        .await
        .expect("join")
        .expect("compress");
    store
        .write(move |tx| {
            tx.prepare_cached(
                "INSERT OR REPLACE INTO body_blob (message_id, data) VALUES (?1, ?2)",
            )?
            .execute(rusqlite::params![id.0, compressed])?;
            Ok(())
        })
        .await
        .expect("insert blob");
}

async fn blob_read(store: &Store, id: MessageId) -> Vec<u8> {
    let compressed: Vec<u8> = store
        .read(move |conn| {
            Ok(conn
                .prepare_cached("SELECT data FROM body_blob WHERE message_id = ?1")?
                .query_row([id.0], |r| r.get(0))?)
        })
        .await
        .expect("select blob");
    tokio::task::spawn_blocking(move || body_files::decompress(&compressed))
        .await
        .expect("join")
        .expect("decompress")
}

/// Runs the futures concurrently and returns their outputs in order (no `futures` dependency).
async fn futures_join<F>(futures: impl Iterator<Item = F>) -> Vec<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handles: Vec<_> = futures.map(tokio::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        out.push(handle.await.expect("join"));
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "benchmark; run with --release --ignored --nocapture"]
async fn bench_body_files_against_blobs() {
    let (samples, source) = samples();
    println!("\n{source}");
    for (name, data, _) in &samples {
        let compressed = body_files::compress(data).expect("compress").len();
        println!(
            "  {name}: {} bytes, zstd {} bytes ({:.1} %)",
            data.len(),
            compressed,
            100.0 * compressed as f64 / data.len() as f64
        );
    }

    let (files, file_ids) = with_headers().await;
    let (blobs, blob_ids) = with_headers().await;
    blobs
        .store
        .write(|tx| {
            tx.execute_batch(
                "CREATE TABLE body_blob (
                   message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
                   data BLOB NOT NULL) STRICT;",
            )?;
            Ok(())
        })
        .await
        .expect("blob table");
    let list_before = (
        list_page_ms(&files.store, files.inbox).await,
        list_page_ms(&blobs.store, blobs.inbox).await,
    );
    checkpoint(files.store.path());
    checkpoint(blobs.store.path());
    let db_before = (file_len(files.store.path()), file_len(blobs.store.path()));

    println!("\n| size | variant | write median | write p95 | read median | read p95 |");
    println!("|---|---|---|---|---|---|");
    let mut next = 0;
    let mut wal_peak = (0u64, 0u64);
    for (name, sample, count) in &samples {
        let ids = next..next + count;
        next += count;
        let (mut fw, mut bw, mut fr, mut br) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for n in ids.clone() {
            let data = unique(sample, n);
            let started = Instant::now();
            files
                .store
                .store_body(file_ids[n], data.clone())
                .await
                .expect("store file");
            fw.push(started.elapsed());
            wal_peak.0 = wal_peak.0.max(wal_len(files.store.path()));

            let started = Instant::now();
            blob_write(&blobs.store, blob_ids[n], data).await;
            bw.push(started.elapsed());
            wal_peak.1 = wal_peak.1.max(wal_len(blobs.store.path()));
        }
        for n in ids {
            let started = Instant::now();
            let data = files
                .store
                .load_message(file_ids[n])
                .await
                .expect("load")
                .expect("stored");
            fr.push(started.elapsed());
            assert_eq!(data.len(), sample.len() + unique(b"", n).len());

            let started = Instant::now();
            let data = blob_read(&blobs.store, blob_ids[n]).await;
            br.push(started.elapsed());
            assert_eq!(data.len(), sample.len() + unique(b"", n).len());
        }
        for (variant, w, r) in [("file", fw, fr), ("BLOB", bw, br)] {
            let (wm, wp) = stats(w);
            let (rm, rp) = stats(r);
            println!(
                "| {name} ({} B) | {variant} | {wm:.2} ms | {wp:.2} ms | {rm:.2} ms | {rp:.2} ms |",
                sample.len()
            );
        }
    }

    let list_after = (
        list_page_ms(&files.store, files.inbox).await,
        list_page_ms(&blobs.store, blobs.inbox).await,
    );
    let (body_bytes, body_files) = dir_bytes(files.store.bodies_dir());
    let wal_after = (wal_len(files.store.path()), wal_len(blobs.store.path()));
    checkpoint(files.store.path());
    checkpoint(blobs.store.path());
    let db_after = (file_len(files.store.path()), file_len(blobs.store.path()));
    let backup_files = backup(files.store.path());
    let backup_blobs = backup(blobs.store.path());
    let raw_total: usize = samples.iter().map(|(_, d, c)| d.len() * c).sum();
    println!("\n{next} bodies, {raw_total} bytes raw, {HEADERS} headers in the mailbox");
    println!("| measure | files | BLOBs |");
    println!("|---|---|---|");
    println!(
        "| database file before bodies | {} B | {} B |",
        db_before.0, db_before.1
    );
    println!(
        "| database file after (checkpointed) | {} B | {} B |",
        db_after.0, db_after.1
    );
    println!("| body files | {body_bytes} B in {body_files} files | – |");
    println!(
        "| WAL largest while writing / after the writes | {} / {} B | {} / {} B |",
        wal_peak.0, wal_after.0, wal_peak.1, wal_after.1
    );
    println!(
        "| list page (100) before / after, median | {:.3} / {:.3} ms | {:.3} / {:.3} ms |",
        list_before.0, list_after.0, list_before.1, list_after.1
    );
    println!(
        "| backup before a migration (VACUUM INTO) | {:.1} ms, {} B | {:.1} ms, {} B |",
        backup_files.0, backup_files.1, backup_blobs.0, backup_blobs.1
    );

    // Deleting: orphan cleanup of 100 messages with bodies.
    let remove = |f: &Fixture| {
        let store = f.store.clone();
        let inbox = f.inbox;
        async move {
            store
                .remove_uids(inbox, UV, mail_model::UidSet::range(HEADERS - 99, HEADERS))
                .await
                .expect("remove");
            let started = Instant::now();
            let deleted = store
                .delete_orphan_messages(crate::test_support::account_id(1))
                .await
                .expect("orphans");
            assert_eq!(deleted, 100);
            started.elapsed().as_secs_f64() * 1e3
        }
    };
    println!(
        "| delete 100 messages with bodies (orphan cleanup) | {:.1} ms | {:.1} ms |",
        remove(&files).await,
        remove(&blobs).await
    );
    checkpoint(files.store.path());
    checkpoint(blobs.store.path());
    println!(
        "| database file after the delete (checkpointed) | {} B | {} B |",
        file_len(files.store.path()),
        file_len(blobs.store.path())
    );

    // Prefetch-like throughput: 200 newsletter-sized bodies, 8 writes in flight.
    let (_, newsletter, _) = &samples[1];
    let targets: Vec<(MessageId, MessageId, Vec<u8>)> = (0..200)
        .map(|n| {
            (
                file_ids[next + n],
                blob_ids[next + n],
                unique(newsletter, 10_000 + n),
            )
        })
        .collect();
    let started = Instant::now();
    for chunk in targets.chunks(8) {
        let writes = chunk.iter().map(|(id, _, data)| {
            let (store, id, data) = (files.store.clone(), *id, data.clone());
            async move { store.store_body(id, data).await }
        });
        for result in futures_join(writes).await {
            result.expect("store");
        }
    }
    let file_rate = started.elapsed();
    let started = Instant::now();
    for chunk in targets.chunks(8) {
        let writes = chunk.iter().map(|(_, id, data)| {
            let (store, id, data) = (blobs.store.clone(), *id, data.clone());
            async move { blob_write(&store, id, data).await }
        });
        futures_join(writes).await;
    }
    let blob_rate = started.elapsed();
    println!(
        "| 200 newsletter bodies, 8 writes in flight | {:.0} ms ({:.2} ms each) | {:.0} ms ({:.2} ms each) |",
        file_rate.as_secs_f64() * 1e3,
        file_rate.as_secs_f64() * 1e3 / 200.0,
        blob_rate.as_secs_f64() * 1e3,
        blob_rate.as_secs_f64() * 1e3 / 200.0
    );

    // Prefetch batches: 50 bodies per FETCH, packed into files (store_bodies) against one
    // transaction with 50 BLOBs.
    for (label, sample, offset) in [
        ("small", &samples[0].1, 20_000),
        ("newsletter", &samples[1].1, 30_000),
    ] {
        let base = next + 200 + if label == "small" { 0 } else { 200 };
        let batches: Vec<Vec<(usize, Vec<u8>)>> = (0..4)
            .map(|b| {
                (0..50)
                    .map(|i| {
                        let n = b * 50 + i;
                        (base + n, unique(sample, offset + n))
                    })
                    .collect()
            })
            .collect();
        let started = Instant::now();
        for batch in &batches {
            let bodies = batch
                .iter()
                .map(|(n, data)| (file_ids[*n], data.clone()))
                .collect();
            let stored = files.store.store_bodies(bodies).await.expect("batch");
            assert_eq!(stored.len(), 50);
        }
        let file_time = started.elapsed();
        let started = Instant::now();
        for batch in &batches {
            let rows: Vec<(i64, Vec<u8>)> = batch
                .iter()
                .map(|(n, data)| {
                    (
                        blob_ids[*n].0,
                        body_files::compress(data).expect("compress"),
                    )
                })
                .collect();
            blobs
                .store
                .write(move |tx| {
                    let mut stmt = tx.prepare_cached(
                        "INSERT OR REPLACE INTO body_blob (message_id, data) VALUES (?1, ?2)",
                    )?;
                    for (id, data) in &rows {
                        stmt.execute(rusqlite::params![id, data])?;
                    }
                    Ok(())
                })
                .await
                .expect("blob batch");
        }
        let blob_time = started.elapsed();
        println!(
            "| 200 {label} bodies in batches of 50 | {:.0} ms ({:.2} ms each) | {:.0} ms ({:.2} ms each) |",
            file_time.as_secs_f64() * 1e3,
            file_time.as_secs_f64() * 1e3 / 200.0,
            blob_time.as_secs_f64() * 1e3,
            blob_time.as_secs_f64() * 1e3 / 200.0
        );
    }
    let (packed_bytes, packed_files) = dir_bytes(files.store.bodies_dir());
    println!("| body files after the batches | {packed_bytes} B in {packed_files} files | – |");

    // The cost of durability: the same files without `sync_all`.
    let dir = tempfile::tempdir().expect("tempdir");
    println!("\n| size | write+rename | write+sync_all+rename |");
    println!("|---|---|---|");
    for (name, sample, _) in &samples {
        let compressed = body_files::compress(sample).expect("compress");
        let mut plain = Vec::new();
        let mut synced = Vec::new();
        for n in 0..20 {
            let temp = dir.path().join(format!("t-{n}"));
            let target = dir.path().join(format!("plain-{n}"));
            let started = Instant::now();
            std::fs::write(&temp, &compressed).expect("write");
            std::fs::rename(&temp, &target).expect("rename");
            plain.push(started.elapsed());
            let started = Instant::now();
            body_files::write_atomically(dir.path(), &format!("synced-{n}"), &compressed)
                .expect("atomic");
            synced.push(started.elapsed());
        }
        println!(
            "| {name} ({} B compressed) | {:.2} ms | {:.2} ms |",
            compressed.len(),
            stats(plain).0,
            stats(synced).0
        );
    }
}
