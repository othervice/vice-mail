//! Spike B: SQLite schema, FTS5 tokenizer choice and query latency with 500,000 generated mails.
//! Throwaway code, never imported by product code. See README.md.

mod bench;
mod concurrent;
mod datagen;
mod load;
mod report;
mod rng;
mod schema;

use datagen::World;
use load::{BodyMode, LoadCfg};
use report::Report;
use rusqlite::{Result, params};
use schema::Tokenizer;
use std::path::PathBuf;
use std::time::Instant;

fn variant(name: &str) -> (Option<Tokenizer>, BodyMode) {
    match name {
        "base" => (None, BodyMode::None),
        "u61" => (Some(Tokenizer::Unicode61), BodyMode::Realistic),
        "tri" => (Some(Tokenizer::Trigram), BodyMode::Realistic),
        "u61all" => (Some(Tokenizer::Unicode61), BodyMode::AllShort(120)),
        "triall" => (Some(Tokenizer::Trigram), BodyMode::AllShort(120)),
        "u61hdr" => (Some(Tokenizer::Unicode61), BodyMode::None),
        "trihdr" => (Some(Tokenizer::Trigram), BodyMode::None),
        _ => panic!("unknown variant {name}; use base|u61|tri|u61all|triall|u61hdr|trihdr"),
    }
}

fn db_path(name: &str) -> PathBuf {
    std::fs::create_dir_all(".data").expect("create .data/");
    let file = std::env::var("SPIKE_DB_NAME").unwrap_or_else(|_| name.to_string());
    PathBuf::from(format!(".data/{file}.db"))
}

fn mb(b: impl Into<f64>) -> String {
    format!("{:.1} MB", b.into() / 1e6)
}

fn hw(rep: &mut Report) {
    let out = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    rep.line(&format!(
        "Rechner: {} / {} Kerne / {} GB RAM / {} / SQLite {} / Last: {}",
        out("sysctl", &["-n", "machdep.cpu.brand_string"]),
        out("sysctl", &["-n", "hw.ncpu"]),
        out("sysctl", &["-n", "hw.memsize"]).parse::<u64>().unwrap_or(0) / (1 << 30),
        out("sw_vers", &["-productVersion"]),
        rusqlite::version(),
        out("sysctl", &["-n", "vm.loadavg"]),
    ));
}

fn build(name: &str, scale: f64, batch: usize, sort_fts: bool, rep: &mut Report) -> Result<()> {
    let (fts, body) = variant(name);
    let path = db_path(name);
    let world = World::new();
    rep.line(&format!(
        "\n## Aufbau `{name}` (FTS {:?}, Body {:?}, Batch {batch}, FTS sortiert {sort_fts}, Skala {scale}, id = Sekunden << {}, Cache {} MB)\n",
        fts,
        body,
        load::id_shift(),
        std::env::var("SPIKE_CACHE_MB").unwrap_or_else(|_| "64".into())
    ));
    hw(rep);
    let st = load::load(&path, &world, LoadCfg { fts, body, batch, sort_fts, scale })?;
    let conn = schema::open_writer(&path)?;
    let wal_end = load::wal_size(&path);
    let t = Instant::now();
    conn.execute_batch("ANALYZE")?;
    let analyze = t.elapsed();
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    let db_size = load::file_size(&path.display().to_string());
    let db_s = st.db_time.as_secs_f64();
    rep.line(&format!(
        "- Mails {} / Orte {} (davon {} per Message-ID zusammengeführt) / Anhänge {} / Bodies im Index {} / Transaktionen {} / ID-Kollisionen {}",
        st.messages, st.locations, st.dedup_hits, st.attachments, st.body_docs, st.transactions, st.id_collisions
    ));
    rep.line(&format!(
        "- Einfügerate (nur DB-Zeit {:.1} s): **{:.0} Mails/s**, {:.0} Orte/s; Erzeugung {:.1} s; gesamt {:.1} s",
        db_s,
        st.messages as f64 / db_s,
        st.locations as f64 / db_s,
        st.gen_time.as_secs_f64(),
        st.wall.as_secs_f64()
    ));
    rep.line(&format!(
        "- WAL maximal während des Ladens {} / am Ende {}; ANALYZE {:.1} s; Datei nach Checkpoint **{}**",
        mb(st.wal_max as f64),
        mb(wal_end as f64),
        analyze.as_secs_f64(),
        mb(db_size as f64)
    ));
    if fts.is_some() {
        let names = ["subject", "sender", "recipients", "body", "attachments"];
        let total: u64 = st.text_bytes.iter().sum();
        let parts: Vec<String> = names.iter().zip(st.text_bytes).map(|(n, b)| format!("{n} {}", mb(b as f64))).collect();
        rep.line(&format!("- Indexierter Text {} ({})", mb(total as f64), parts.join(", ")));
    }
    let unread: i64 = conn.query_row("SELECT count(*) FROM message_location WHERE seen = 0", [], |r| r.get(0))?;
    rep.line(&format!("- Ungelesene Orte gesamt: {unread}"));
    let sizes = load::object_sizes(&conn)?;
    let fts_total: i64 = sizes.iter().filter(|(n, _)| n.starts_with("message_fts")).map(|x| x.1).sum();
    let all: i64 = sizes.iter().map(|x| x.1).sum();
    rep.line(&format!("- Größe ohne FTS {} / FTS {}", mb((all - fts_total) as f64), mb(fts_total as f64)));
    let list: Vec<String> = sizes.iter().filter(|x| x.1 > 1_000_000).map(|(n, b)| format!("{n} {}", mb(*b as f64))).collect();
    rep.line(&format!("- Objekte: {}", list.join(", ")));
    Ok(())
}

fn rare_word(world: &World) -> String {
    world.vocab[40_000].clone()
}

fn bench_cmd(name: &str, mmap: bool, only: &str, rep: &mut Report) -> Result<()> {
    let (fts, _) = variant(name);
    let path = db_path(name);
    let world = World::new();
    let conn = schema::open_reader(&path, mmap)?;
    let ctx = bench::context(&conn, &rare_word(&world), fts)?;
    rep.line(&format!("\n## Messung `{name}` (mmap {mmap})\n"));
    hw(rep);
    let mut cases = Vec::new();
    if only != "search" {
        cases.extend(bench::list_cases(&conn, &ctx)?);
        cases.extend(bench::like_cases());
    }
    if let Some(t) = fts
        && only != "list"
    {
        let rare_docs: i64 =
            conn.query_row("SELECT count(*) FROM message_fts WHERE message_fts MATCH ?1", [&ctx.rare_word], |r| r.get(0))?;
        rep.line(&format!("Seltenes Wort `{}` steht in {rare_docs} Mails.", ctx.rare_word));
        for (a, b) in [("über", "uber"), ("grüße", "gruße"), ("grüße", "grusse"), ("müller", "mueller")] {
            let c = |q: &str| -> Result<i64> {
                conn.query_row("SELECT count(*) FROM message_fts WHERE message_fts MATCH ?1", [format!("\"{q}\"")], |r| r.get(0))
            };
            rep.line(&format!("Diakritika: `{a}` {} Treffer, `{b}` {} Treffer", c(a)?, c(b)?));
        }
        cases.extend(bench::search_cases(&ctx, t));
    }
    bench::print_plans(&conn, &cases, rep)?;
    drop(conn);
    bench::run_cases(&path, &cases, rep, mmap)?;
    let rss = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    rep.line(&format!(
        "\nRSS des Messprozesses am Ende: {} MB (Leser-Cache {} MB, mmap {mmap})",
        rss.parse::<f64>().unwrap_or(0.0) / 1024.0,
        std::env::var("SPIKE_READER_CACHE_MB").unwrap_or_else(|_| "32".into())
    ));
    Ok(())
}

fn reindex(name: &str, n: usize, rep: &mut Report) -> Result<()> {
    let (fts, _) = variant(name);
    let fts = fts.expect("needs FTS");
    let path = db_path(name);
    let world = World::new();
    let conn = schema::open_writer(&path)?;
    // Mails without cached body, newest first: they get their body when opened / prefetched.
    let mut s = conn.prepare(
        "SELECT m.id, m.account_id, m.message_id_hdr, m.date, mb.role FROM message m \
         JOIN message_location l ON l.message_id = m.id JOIN mailbox mb ON mb.id = l.mailbox_id \
         WHERE m.body_ref IS NULL GROUP BY m.id ORDER BY m.id DESC LIMIT ?1",
    )?;
    let rows: Vec<(i64, i64, String, i64, Option<String>)> = s
        .query_map([n as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_>>()?;
    drop(s);
    let mut gen_items = Vec::new();
    for (id, acc, hdr, date, role) in rows {
        let seq = u64::from_str_radix(&hdr[1..17], 16).expect("seq in Message-ID");
        let sent = matches!(role.as_deref(), Some("sent") | Some("drafts"));
        let meta = datagen::Meta { seq, date, locs: [None, None], has_body: true, sent };
        gen_items.push((id, world.content(&meta, acc as usize - 1)));
    }
    let t = Instant::now();
    for chunk in gen_items.chunks(500) {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        {
            let mut ins = conn.prepare_cached(
                "INSERT OR REPLACE INTO message_fts(rowid, subject, sender, recipients, body, attachments) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut up = conn.prepare_cached("UPDATE message SET body_ref = ?2 WHERE id = ?1")?;
            for (id, c) in chunk {
                ins.execute(params![id, c.subject, c.sender_text(), c.recipients_text(), c.body, c.attachment_text()])?;
                up.execute(params![id, format!("{id:x}.eml.zst")])?;
            }
        }
        conn.execute_batch("COMMIT")?;
    }
    let d = t.elapsed().as_secs_f64();
    rep.line(&format!(
        "\n## Body nachträglich indexieren `{name}` ({fts:?})\n\n- {} Mails per INSERT OR REPLACE in 500er-Transaktionen: {:.2} s, **{:.0} Mails/s**",
        gen_items.len(),
        d,
        gen_items.len() as f64 / d
    ));
    // Correctness: the re-indexed body is searchable, the old header terms still are.
    let (id, c) = &gen_items[0];
    let word = c.body.split_whitespace().rev().find(|w| w.chars().count() > 6 && w.chars().all(char::is_alphabetic)).unwrap_or("dank");
    let hit: i64 = conn.query_row(
        "SELECT count(*) FROM message_fts WHERE message_fts MATCH ?1 AND rowid = ?2",
        params![format!("body:\"{word}\""), id],
        |r| r.get(0),
    )?;
    rep.line(&format!("- Probe: Body-Wort `{word}` findet die Mail {id}: {}", hit == 1));
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(())
}

fn optimize(name: &str, rep: &mut Report) -> Result<()> {
    let path = db_path(name);
    let conn = schema::open_writer(&path)?;
    let before = load::object_sizes(&conn)?.iter().filter(|(n, _)| n.starts_with("message_fts")).map(|x| x.1).sum::<i64>();
    let segs_before: i64 = conn.query_row("SELECT count(*) FROM message_fts_data", [], |r| r.get(0))?;
    let t = Instant::now();
    conn.execute_batch("INSERT INTO message_fts(message_fts) VALUES('optimize')")?;
    let d = t.elapsed().as_secs_f64();
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    let wal_peak_note = "WAL nach Checkpoint abgeschnitten";
    let after = load::object_sizes(&conn)?.iter().filter(|(n, _)| n.starts_with("message_fts")).map(|x| x.1).sum::<i64>();
    let segs_after: i64 = conn.query_row("SELECT count(*) FROM message_fts_data", [], |r| r.get(0))?;
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let db_size = load::file_size(&path.display().to_string());
    rep.line(&format!(
        "\n## FTS optimize `{name}`\n\n- Dauer {d:.1} s; FTS {} -> {}; Zeilen in message_fts_data {segs_before} -> {segs_after}; Datei {} davon frei {} (VACUUM würde sie freigeben); {wal_peak_note}",
        mb(before as f64),
        mb(after as f64),
        mb(db_size as f64),
        mb(free as f64 * 4096.0)
    ));
    Ok(())
}

fn micro(rep: &mut Report) -> Result<()> {
    rep.line("\n## Einfügen: Batchgröße und FTS-Reihenfolge (Skala 0.1 = 50.000 Mails)\n");
    hw(rep);
    for (name, batch, sorted) in [
        ("base", 100, true),
        ("base", 500, true),
        ("base", 2000, true),
        ("u61", 100, true),
        ("u61", 500, true),
        ("u61", 2000, true),
        ("u61", 500, false),
        ("tri", 500, true),
    ] {
        let (fts, body) = variant(name);
        let path = db_path("micro");
        let world = World::new();
        let st = load::load(&path, &world, LoadCfg { fts, body, batch, sort_fts: sorted, scale: 0.1 })?;
        let size = load::file_size(&path.display().to_string()) + load::wal_size(&path);
        rep.line(&format!(
            "- {name}, Batch {batch}, FTS sortiert {sorted}: **{:.0} Mails/s** (DB {:.2} s), Datei+WAL {}",
            st.messages as f64 / st.db_time.as_secs_f64(),
            st.db_time.as_secs_f64(),
            mb(size as f64)
        ));
        load::remove_db(&path);
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("help");
    let name = args.get(2).map(String::as_str).unwrap_or("u61");
    let report_name = std::env::var("SPIKE_DB_NAME").unwrap_or_else(|_| name.to_string());
    let mut rep = Report::new(if cmd == "micro" { "micro" } else { &report_name });
    match cmd {
        "build" => {
            let scale = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            build(name, scale, 500, true, &mut rep)?
        }
        "bench" => {
            let mmap = args.get(3).map(String::as_str) != Some("nommap");
            let only = args.get(4).map(String::as_str).unwrap_or("all");
            bench_cmd(name, mmap, only, &mut rep)?
        }
        "concurrent" => {
            let secs = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15);
            let (fts, _) = variant(name);
            concurrent::run(&db_path(name), &World::new(), fts, secs, &mut rep)?
        }
        "reindex" => reindex(name, args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20_000), &mut rep)?,
        "optimize" => optimize(name, &mut rep)?,
        "micro" => micro(&mut rep)?,
        "sample" => {
            let world = World::new();
            let defs = datagen::mailbox_defs(0);
            let metas = datagen::plan_account(0, &defs, 0.001);
            for m in metas.iter().take(3) {
                let c = world.content(m, 0);
                println!("{} | {} <{}> | {} | {}\n{}\n---", c.subject, c.from_name, c.from_addr, c.to, c.message_id_hdr, c.body);
            }
            println!("rare word: {} / vocab {}", rare_word(&world), world.vocab.len());
        }
        _ => eprintln!("usage: spike-sqlite build|bench|concurrent|reindex|optimize|micro|sample <variant> [args]"),
    }
    Ok(())
}
