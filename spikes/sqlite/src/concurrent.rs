//! One writer thread syncing new mail and flag changes while a reader thread loads list pages and searches.

use crate::bench::{self, ms, stats};
use crate::datagen::{END, LocMeta, Meta, World};
use crate::load::{self, BodyMode, Item, LoadCfg, LoadStats};
use crate::report::Report;
use crate::rng::Rng;
use crate::schema::{self, Tokenizer};
use rusqlite::types::Value;
use rusqlite::{Result, params};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct ReaderQ {
    name: &'static str,
    sql: String,
    params: Vec<Value>,
}

fn reader_queries(path: &Path, tok: Option<Tokenizer>) -> Result<Vec<ReaderQ>> {
    let conn = schema::open_reader(path, true)?;
    let ctx = bench::context(&conn, "x", None)?;
    let lists = bench::list_cases(&conn, &ctx)?;
    let mut v = vec![
        ReaderQ { name: "Liste Posteingang Seite 1", sql: lists[0].sql.clone(), params: lists[0].params.clone() },
        ReaderQ { name: "Liste Posteingang ab Mitte", sql: lists[1].sql.clone(), params: lists[1].params.clone() },
        ReaderQ { name: "Vereinter Posteingang Seite 1", sql: lists[6].sql.clone(), params: lists[6].params.clone() },
        ReaderQ { name: "Ungelesen-Zähler Seitenleiste", sql: lists[16].sql.clone(), params: lists[16].params.clone() },
    ];
    if let Some(t) = tok {
        let s = bench::search_cases(&ctx, t);
        v.push(ReaderQ { name: "Suche rechnung neueste 50", sql: s[0].sql.clone(), params: s[0].params.clone() });
    }
    Ok(v)
}

fn read_loop(path: &Path, qs: &[ReaderQ], dur: Duration) -> Result<Vec<Vec<f64>>> {
    let conn = schema::open_reader(path, true)?;
    let mut lat = vec![Vec::new(); qs.len()];
    let start = Instant::now();
    while start.elapsed() < dur {
        for (k, q) in qs.iter().enumerate() {
            let t = Instant::now();
            bench::exec(&conn, &q.sql, &q.params)?;
            lat[k].push(ms(t.elapsed()));
        }
    }
    Ok(lat)
}

pub fn run(path: &Path, world: &World, tok: Option<Tokenizer>, secs: u64, rep: &mut Report) -> Result<()> {
    let qs = reader_queries(path, tok)?;
    let dur = Duration::from_secs(secs);
    rep.line(&format!("\n## Leser mit und ohne laufenden Schreiber ({secs} s je Phase)\n"));

    let alone = read_loop(path, &qs, dur)?;

    let stop = AtomicBool::new(false);
    let wal_max = AtomicU64::new(0);
    let (with_writer, writer_res) = std::thread::scope(|s| {
        let w = s.spawn(|| writer(path, world, tok, &stop, &wal_max));
        let r = read_loop(path, &qs, dur);
        stop.store(true, Ordering::SeqCst);
        (r, w.join().expect("writer thread"))
    });
    let with_writer = with_writer?;
    let (wst, tx_lat, flag_updates) = writer_res?;

    rep.line("| Abfrage | ohne Schreiber: n / Median / p95 / p99 / max ms | mit Schreiber: n / Median / p95 / p99 / max ms |");
    rep.line("|---|---|---|");
    for (k, q) in qs.iter().enumerate() {
        let f = |v: &Vec<f64>| {
            let mut s = v.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            format!(
                "{} / {:.2} / {:.2} / {:.2} / {:.2}",
                s.len(),
                bench::percentile(&s, 0.5),
                bench::percentile(&s, 0.95),
                bench::percentile(&s, 0.99),
                s.last().copied().unwrap_or(f64::NAN)
            )
        };
        rep.line(&format!("| {} | {} | {} |", q.name, f(&alone[k]), f(&with_writer[k])));
    }
    let txs = stats(tx_lat, 0);
    rep.line(&format!(
        "\nSchreiber: {} neue Mails (mit FTS, Body indexiert) und {} Flag-Änderungen in {} Transaktionen; \
         {:.0} Mails/s; Transaktion (100 Mails bzw. 200 Flags) Median {:.1} ms, p95 {:.1} ms, max {:.1} ms; \
         WAL maximal {:.1} MB",
        wst.messages,
        flag_updates,
        txs.n,
        wst.messages as f64 / dur.as_secs_f64(),
        txs.median,
        txs.p95,
        txs.max,
        wal_max.load(Ordering::SeqCst) as f64 / 1e6
    ));
    Ok(())
}

type WriterOut = (LoadStats, Vec<f64>, u64);

fn writer(path: &Path, world: &World, tok: Option<Tokenizer>, stop: &AtomicBool, wal_max: &AtomicU64) -> Result<WriterOut> {
    let conn = schema::open_writer(path)?;
    let inbox: i64 = conn.query_row("SELECT id FROM mailbox WHERE account_id = 1 AND role = 'inbox'", [], |r| r.get(0))?;
    let mut uid: i64 = conn.query_row("SELECT max(uid) FROM message_location WHERE mailbox_id = ?1", [inbox], |r| r.get(0))?;
    let max_uid = uid;
    let cfg = LoadCfg { fts: tok, body: BodyMode::Realistic, batch: 100, sort_fts: true, scale: 1.0 };
    let mut st = LoadStats::default();
    let mut tx_lat = Vec::new();
    let mut rng = Rng::new(99);
    let mut seq = 90_000_000u64;
    let mut date = END + 3_600;
    let mut flags = 0u64;
    while !stop.load(Ordering::SeqCst) {
        let metas: Vec<Meta> = (0..100)
            .map(|_| {
                seq += 1;
                date += 7;
                Meta {
                    seq,
                    date,
                    locs: [Some(LocMeta { mailbox: 0, seen: false, flagged: false, answered: false }), None],
                    has_body: true,
                    sent: false,
                }
            })
            .collect();
        let items: Vec<Item> = metas
            .iter()
            .map(|m| {
                uid += 1;
                Item { meta: m, loc: m.locs[0].unwrap(), uid, content: world.content(m, 0) }
            })
            .collect();
        let t = Instant::now();
        load::insert_batch(&conn, cfg, 1, inbox, false, &items, &mut st)?;
        tx_lat.push(ms(t.elapsed()));

        let t = Instant::now();
        conn.execute_batch("BEGIN IMMEDIATE")?;
        {
            let mut up = conn.prepare_cached(
                "UPDATE message_location SET seen = 1 - seen, modseq = modseq + 1 WHERE mailbox_id = ?1 AND uid = ?2",
            )?;
            for _ in 0..200 {
                up.execute(params![inbox, rng.range(1, max_uid as u64) as i64])?;
                flags += 1;
            }
        }
        conn.execute_batch("COMMIT")?;
        tx_lat.push(ms(t.elapsed()));
        wal_max.fetch_max(load::wal_size(path), Ordering::SeqCst);
    }
    Ok((st, tx_lat, flags))
}
