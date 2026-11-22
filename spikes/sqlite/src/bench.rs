//! Query catalogue and timing harness.

use crate::datagen::{DAY, END};
use crate::report::Report;
use crate::schema::{self, Tokenizer};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, Result, params_from_iter};
use std::path::Path;
use std::time::{Duration, Instant};

pub struct Case {
    pub group: &'static str,
    pub name: String,
    pub sql: String,
    pub params: Vec<Value>,
}

pub struct Stat {
    pub median: f64,
    pub p95: f64,
    pub max: f64,
    pub n: usize,
    pub rows: usize,
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

pub fn stats(mut v: Vec<f64>, rows: usize) -> Stat {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Stat { median: percentile(&v, 0.5), p95: percentile(&v, 0.95), max: *v.last().unwrap_or(&f64::NAN), n: v.len(), rows }
}

/// Run a query and touch every column of every row, like the real list code would.
pub fn exec(conn: &Connection, sql: &str, params: &[Value]) -> Result<usize> {
    let mut st = conn.prepare_cached(sql)?;
    let ncol = st.column_count();
    let mut rows = st.query(params_from_iter(params.iter()))?;
    let mut n = 0;
    let mut sink = 0usize;
    while let Some(r) = rows.next()? {
        for i in 0..ncol {
            sink += match r.get_ref(i)? {
                ValueRef::Text(t) | ValueRef::Blob(t) => t.len(),
                ValueRef::Integer(x) => (x & 1) as usize,
                ValueRef::Real(_) => 1,
                ValueRef::Null => 0,
            };
        }
        n += 1;
    }
    std::hint::black_box(sink);
    Ok(n)
}

pub fn time_case(conn: &Connection, sql: &str, params: &[Value], max_iter: usize) -> Result<Stat> {
    let mut rows = 0;
    for _ in 0..3 {
        rows = exec(conn, sql, params)?;
    }
    let start = Instant::now();
    let mut v = Vec::new();
    while v.len() < max_iter {
        let t = Instant::now();
        rows = exec(conn, sql, params)?;
        v.push(ms(t.elapsed()));
        let el = start.elapsed();
        if (el > Duration::from_secs(2) && v.len() >= 10) || el > Duration::from_secs(8) {
            break;
        }
    }
    Ok(stats(v, rows))
}

pub fn plan(conn: &Connection, sql: &str, params: &[Value]) -> Result<String> {
    let mut st = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
    let mut rows = st.query(params_from_iter(params.iter()))?;
    let mut out = Vec::new();
    let mut depth: Vec<(i64, usize)> = Vec::new();
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let parent: i64 = r.get(1)?;
        let detail: String = r.get(3)?;
        while let Some(&(pid, _)) = depth.last() {
            if pid == parent {
                break;
            }
            depth.pop();
        }
        let d = depth.last().map(|x| x.1 + 1).unwrap_or(0);
        depth.push((id, d));
        out.push(format!("{}{}", "  ".repeat(d), detail));
    }
    Ok(out.join(" | "))
}

const LIST_COLS: &str = "l.message_id, l.uid, l.seen, l.flagged, l.answered, m.subject, m.from_name, \
                         m.from_addr, m.date, m.preview, m.has_attachments, m.thread_id, m.account_id";
const SEARCH_COLS: &str =
    "m.id, m.subject, m.from_name, m.from_addr, m.date, m.preview, m.has_attachments, m.account_id";

fn i(v: i64) -> Value {
    Value::Integer(v)
}
fn t(s: &str) -> Value {
    Value::Text(s.to_string())
}

pub struct Ctx {
    pub big_inbox: i64,
    pub big_inbox_count: i64,
    pub inboxes: Vec<i64>,
    pub unified_count: i64,
    pub rare_word: String,
    pub auszug_terms: Vec<String>,
}

fn key_at(conn: &Connection, mailbox: i64, offset: i64) -> Result<i64> {
    conn.query_row(
        "SELECT message_id FROM message_location WHERE mailbox_id = ?1 ORDER BY message_id DESC LIMIT 1 OFFSET ?2",
        [mailbox, offset],
        |r| r.get(0),
    )
}

fn unified_key_at(conn: &Connection, offset: i64) -> Result<i64> {
    conn.query_row(
        "SELECT message_id FROM message_location INDEXED BY loc_message \
         WHERE mailbox_id IN (SELECT id FROM mailbox WHERE role = 'inbox') ORDER BY message_id DESC LIMIT 1 OFFSET ?1",
        [offset],
        |r| r.get(0),
    )
}

pub fn context(conn: &Connection, rare_word: &str, tok: Option<Tokenizer>) -> Result<Ctx> {
    let big_inbox: i64 = conn.query_row("SELECT id FROM mailbox WHERE account_id = 1 AND role = 'inbox'", [], |r| r.get(0))?;
    let big_inbox_count: i64 =
        conn.query_row("SELECT count(*) FROM message_location WHERE mailbox_id = ?1", [big_inbox], |r| r.get(0))?;
    let mut s = conn.prepare("SELECT id FROM mailbox WHERE role = 'inbox' ORDER BY account_id")?;
    let inboxes: Vec<i64> = s.query_map([], |r| r.get(0))?.collect::<Result<_>>()?;
    let unified_count: i64 = conn.query_row(
        "SELECT count(*) FROM message_location WHERE mailbox_id IN (SELECT id FROM mailbox WHERE role = 'inbox')",
        [],
        |r| r.get(0),
    )?;
    let auszug_terms = if tok == Some(Tokenizer::Unicode61) {
        let mut s = conn.prepare("SELECT term FROM message_fts_vocab WHERE term LIKE '%auszug%'")?;
        s.query_map([], |r| r.get(0))?.collect::<Result<_>>()?
    } else {
        Vec::new()
    };
    Ok(Ctx { big_inbox, big_inbox_count, inboxes, unified_count, rare_word: rare_word.to_string(), auszug_terms })
}

fn unified_union_sql() -> String {
    let parts: Vec<String> = (1..=5)
        .map(|k| {
            format!(
                "SELECT * FROM (SELECT mailbox_id, message_id, uid, seen, flagged, answered FROM message_location \
                 WHERE mailbox_id = ?{k} AND message_id < ?6 ORDER BY message_id DESC LIMIT 100)"
            )
        })
        .collect();
    format!(
        "SELECT {LIST_COLS} FROM ({} ORDER BY message_id DESC LIMIT 100) l CROSS JOIN message m ON m.id = l.message_id \
         ORDER BY l.message_id DESC",
        parts.join(" UNION ALL ")
    )
}

pub fn list_cases(conn: &Connection, ctx: &Ctx) -> Result<Vec<Case>> {
    let mb = ctx.big_inbox;
    let n = ctx.big_inbox_count;
    let mid = key_at(conn, mb, n / 2)?;
    let end = key_at(conn, mb, n - 100)?;
    let page = format!(
        "SELECT {LIST_COLS} FROM message_location l CROSS JOIN message m ON m.id = l.message_id \
         WHERE l.mailbox_id = ?1 AND l.message_id < ?2 ORDER BY l.message_id DESC LIMIT 100"
    );
    let unread_page = format!(
        "SELECT {LIST_COLS} FROM message_location l CROSS JOIN message m ON m.id = l.message_id \
         WHERE l.mailbox_id = ?1 AND l.seen = 0 AND l.message_id < ?2 ORDER BY l.message_id DESC LIMIT 100"
    );
    let offset_page = format!(
        "SELECT {LIST_COLS} FROM message_location l CROSS JOIN message m ON m.id = l.message_id \
         WHERE l.mailbox_id = ?1 ORDER BY l.message_id DESC LIMIT 100 OFFSET ?2"
    );
    let un = ctx.unified_count;
    let umid = unified_key_at(conn, un / 2)?;
    let uend = unified_key_at(conn, un - 100)?;
    let union = unified_union_sql();
    let in_planner = format!(
        "SELECT {LIST_COLS} FROM message_location l JOIN message m ON m.id = l.message_id \
         WHERE l.mailbox_id IN (SELECT id FROM mailbox WHERE role = 'inbox') AND l.message_id < ?1 \
         ORDER BY l.message_id DESC LIMIT 100"
    );
    let in_idx = format!(
        "SELECT {LIST_COLS} FROM message_location l INDEXED BY loc_message CROSS JOIN message m ON m.id = l.message_id \
         WHERE l.mailbox_id IN (SELECT id FROM mailbox WHERE role = 'inbox') AND l.message_id < ?1 \
         ORDER BY l.message_id DESC LIMIT 100"
    );
    let in_sort = format!(
        "SELECT {LIST_COLS} FROM message_location l CROSS JOIN message m ON m.id = l.message_id \
         WHERE l.mailbox_id IN (SELECT id FROM mailbox WHERE role = 'inbox') AND +l.message_id < ?1 \
         ORDER BY +l.message_id DESC LIMIT 100"
    );
    let ib = &ctx.inboxes;
    let up = |key: i64| -> Vec<Value> {
        let mut v: Vec<Value> = ib.iter().map(|&x| i(x)).collect();
        v.push(i(key));
        v
    };
    let max = i64::MAX;
    Ok(vec![
        Case { group: "Liste Postfach", name: format!("Posteingang Konto 1 ({n} Mails), Seite 1"), sql: page.clone(), params: vec![i(mb), i(max)] },
        Case { group: "Liste Postfach", name: "… Keyset ab Mitte".into(), sql: page.clone(), params: vec![i(mb), i(mid)] },
        Case { group: "Liste Postfach", name: "… Keyset ab Ende (letzte Seite)".into(), sql: page.clone(), params: vec![i(mb), i(end)] },
        Case { group: "Liste Postfach", name: "… nur ungelesen, Seite 1".into(), sql: unread_page, params: vec![i(mb), i(max)] },
        Case { group: "Liste Postfach", name: "… zum Vergleich OFFSET Mitte".into(), sql: offset_page.clone(), params: vec![i(mb), i(n / 2)] },
        Case { group: "Liste Postfach", name: "… zum Vergleich OFFSET Ende".into(), sql: offset_page, params: vec![i(mb), i(n - 100)] },
        Case { group: "Vereinter Posteingang", name: format!("UNION ALL je Postfach ({un} Mails), Seite 1"), sql: union.clone(), params: up(max) },
        Case { group: "Vereinter Posteingang", name: "UNION ALL, Keyset ab Mitte".into(), sql: union.clone(), params: up(umid) },
        Case { group: "Vereinter Posteingang", name: "UNION ALL, Keyset ab Ende".into(), sql: union, params: up(uend) },
        Case { group: "Vereinter Posteingang", name: "IN (...) Planer-Wahl, Seite 1".into(), sql: in_planner.clone(), params: vec![i(max)] },
        Case { group: "Vereinter Posteingang", name: "IN (...) Planer-Wahl, ab Ende".into(), sql: in_planner, params: vec![i(uend)] },
        Case { group: "Vereinter Posteingang", name: "IN (...) über loc_message, Seite 1".into(), sql: in_idx.clone(), params: vec![i(max)] },
        Case { group: "Vereinter Posteingang", name: "IN (...) über loc_message, ab Ende".into(), sql: in_idx, params: vec![i(uend)] },
        Case { group: "Vereinter Posteingang", name: "IN (...) mit Sortierung (Gegenprobe)".into(), sql: in_sort, params: vec![i(max)] },
        Case { group: "Zähler", name: "ungelesen, größter Posteingang (Teilindex)".into(),
            sql: "SELECT count(*) FROM message_location WHERE mailbox_id = ?1 AND seen = 0".into(), params: vec![i(mb)] },
        Case { group: "Zähler", name: "ungelesen, größter Posteingang (ohne Teilindex)".into(),
            sql: "SELECT count(*) FROM message_location WHERE mailbox_id = ?1 AND +seen = 0".into(), params: vec![i(mb)] },
        Case { group: "Zähler", name: "ungelesen, alle Postfächer (Seitenleiste, Teilindex)".into(),
            sql: "SELECT mailbox_id, count(*) FROM message_location WHERE seen = 0 GROUP BY mailbox_id".into(), params: vec![] },
        Case { group: "Zähler", name: "ungelesen, alle Postfächer (ohne Teilindex)".into(),
            sql: "SELECT mailbox_id, count(*) FROM message_location WHERE +seen = 0 GROUP BY mailbox_id".into(), params: vec![] },
        Case { group: "Zähler", name: "Gesamtzahl größter Posteingang".into(),
            sql: "SELECT count(*) FROM message_location WHERE mailbox_id = ?1".into(), params: vec![i(mb)] },
    ])
}

pub fn search_cases(ctx: &Ctx, tok: Tokenizer) -> Vec<Case> {
    let newest = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 ORDER BY message_fts.rowid DESC LIMIT 50"
    );
    let rank = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 ORDER BY rank LIMIT 50"
    );
    let by_date = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 ORDER BY m.date DESC LIMIT 50"
    );
    let f_account = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 AND m.account_id = ?2 ORDER BY message_fts.rowid DESC LIMIT 50"
    );
    let f_date = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 AND message_fts.rowid >= ?2 ORDER BY message_fts.rowid DESC LIMIT 50"
    );
    let unread = "EXISTS (SELECT 1 FROM message_location l WHERE l.message_id = m.id AND l.seen = 0)";
    let f_unread = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 AND {unread} ORDER BY message_fts.rowid DESC LIMIT 50"
    );
    let f_all = format!(
        "SELECT {SEARCH_COLS} FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid \
         WHERE message_fts MATCH ?1 AND m.account_id = ?2 AND message_fts.rowid >= ?3 AND {unread} \
         ORDER BY message_fts.rowid DESC LIMIT 50"
    );
    let count = "SELECT count(*) FROM message_fts WHERE message_fts MATCH ?1".to_string();
    let year_ago = (END - 365 * DAY) << crate::load::id_shift();
    let tri = tok == Tokenizer::Trigram;
    let prefix = if tri { "rech" } else { "rech*" };
    let rare = ctx.rare_word.as_str();
    let mut v = vec![
        Case { group: "Suche neueste zuerst", name: "häufig: rechnung".into(), sql: newest.clone(), params: vec![t("rechnung")] },
        Case { group: "Suche neueste zuerst", name: "sehr häufig: und".into(), sql: newest.clone(), params: vec![t("und")] },
        Case { group: "Suche neueste zuerst", name: format!("selten: {rare}"), sql: newest.clone(), params: vec![t(rare)] },
        Case { group: "Suche neueste zuerst", name: format!("Präfix: {prefix}"), sql: newest.clone(), params: vec![t(prefix)] },
        Case { group: "Suche neueste zuerst", name: "Phrase: \"vielen dank\"".into(), sql: newest.clone(), params: vec![t("\"vielen dank\"")] },
        Case { group: "Suche neueste zuerst", name: "zwei Wörter: rechnung bestellung".into(), sql: newest.clone(), params: vec![t("rechnung bestellung")] },
        Case { group: "Suche neueste zuerst", name: "Umlaut: grüße".into(), sql: newest.clone(), params: vec![t("grüße")] },
        Case { group: "Suche neueste zuerst", name: "Spalte: sender:müller".into(), sql: newest.clone(), params: vec![t("sender:müller")] },
    ];
    if tri {
        v.push(Case { group: "Suche neueste zuerst", name: "Teilwort: auszug (Trigramm)".into(), sql: newest.clone(), params: vec![t("auszug")] });
        v.push(Case { group: "Suche neueste zuerst", name: "Teilwort: nummer (Trigramm)".into(), sql: newest.clone(), params: vec![t("nummer")] });
    } else {
        v.push(Case { group: "Suche neueste zuerst", name: "Teilwort Schritt 1: fts5vocab LIKE '%auszug%'".into(),
            sql: "SELECT term FROM message_fts_vocab WHERE term LIKE '%auszug%'".into(), params: vec![] });
        let q = ctx.auszug_terms.iter().map(|x| format!("\"{x}\"")).collect::<Vec<_>>().join(" OR ");
        v.push(Case { group: "Suche neueste zuerst", name: format!("Teilwort Schritt 2: OR über {} Terme", ctx.auszug_terms.len()), sql: newest.clone(), params: vec![t(&q)] });
    }
    v.extend([
        Case { group: "Suche mit Filter", name: "rechnung + Konto 4 (40k)".into(), sql: f_account.clone(), params: vec![t("rechnung"), i(4)] },
        Case { group: "Suche mit Filter", name: "rechnung + Konto 5 (10k)".into(), sql: f_account, params: vec![t("rechnung"), i(5)] },
        Case { group: "Suche mit Filter", name: "rechnung + letzte 365 Tage".into(), sql: f_date, params: vec![t("rechnung"), i(year_ago)] },
        Case { group: "Suche mit Filter", name: "rechnung + ungelesen".into(), sql: f_unread, params: vec![t("rechnung")] },
        Case { group: "Suche mit Filter", name: "rechnung + Konto 4 + 365 Tage + ungelesen".into(), sql: f_all, params: vec![t("rechnung"), i(4), i(year_ago)] },
        Case { group: "Suche andere Reihenfolge", name: "rechnung, ORDER BY rank (bm25)".into(), sql: rank.clone(), params: vec![t("rechnung")] },
        Case { group: "Suche andere Reihenfolge", name: "und, ORDER BY rank (bm25)".into(), sql: rank.clone(), params: vec![t("und")] },
        Case { group: "Suche andere Reihenfolge", name: format!("{rare}, ORDER BY rank (bm25)"), sql: rank, params: vec![t(rare)] },
        Case { group: "Suche andere Reihenfolge", name: "rechnung, ORDER BY m.date (Join)".into(), sql: by_date.clone(), params: vec![t("rechnung")] },
        Case { group: "Suche andere Reihenfolge", name: "und, ORDER BY m.date (Join)".into(), sql: by_date, params: vec![t("und")] },
        Case { group: "Trefferzahl", name: "count(*) rechnung".into(), sql: count.clone(), params: vec![t("rechnung")] },
        Case { group: "Trefferzahl", name: "count(*) und".into(), sql: count, params: vec![t("und")] },
    ]);
    v
}

/// Header-only infix fallback without FTS (what unicode61 users would get for "Teilwort" in subjects).
pub fn like_cases() -> Vec<Case> {
    vec![
        Case { group: "LIKE ohne FTS", name: "Betreff LIKE '%auszug%', neueste 50".into(),
            sql: format!("SELECT {SEARCH_COLS} FROM message m WHERE m.subject LIKE '%auszug%' ORDER BY m.id DESC LIMIT 50"), params: vec![] },
        Case { group: "LIKE ohne FTS", name: "Betreff/Absender LIKE '%müller%', neueste 50".into(),
            sql: format!("SELECT {SEARCH_COLS} FROM message m WHERE m.subject LIKE '%müller%' OR m.from_name LIKE '%müller%' ORDER BY m.id DESC LIMIT 50"), params: vec![] },
        Case { group: "LIKE ohne FTS", name: "Betreff/Absender LIKE '%laarzeso%' (selten, voller Scan)".into(),
            sql: format!("SELECT {SEARCH_COLS} FROM message m WHERE m.subject LIKE '%laarzeso%' OR m.from_name LIKE '%laarzeso%' ORDER BY m.id DESC LIMIT 50"), params: vec![] },
    ]
}

pub fn run_cases(path: &Path, cases: &[Case], rep: &mut Report, mmap: bool) -> Result<Vec<(String, Stat, f64)>> {
    let conn = schema::open_reader(path, mmap)?;
    let mut out = Vec::new();
    let mut group = "";
    for c in cases {
        if c.group != group {
            group = c.group;
            rep.line(&format!("\n### {group}\n"));
            rep.line("| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |");
            rep.line("|---|---:|---:|---:|---:|---:|---:|");
        }
        // Cold: fresh connection (empty page cache, OS cache warm), prepare + first run.
        let cold = {
            let c2 = schema::open_reader(path, mmap)?;
            let t0 = Instant::now();
            exec(&c2, &c.sql, &c.params)?;
            ms(t0.elapsed())
        };
        let s = time_case(&conn, &c.sql, &c.params, 200)?;
        rep.line(&format!(
            "| {} | {} | {:.3} | {:.3} | {:.3} | {} | {:.2} |",
            c.name, s.rows, s.median, s.p95, s.max, s.n, cold
        ));
        out.push((c.name.clone(), s, cold));
    }
    Ok(out)
}

pub fn print_plans(conn: &Connection, cases: &[Case], rep: &mut Report) -> Result<()> {
    rep.line("\n### EXPLAIN QUERY PLAN\n");
    for c in cases {
        let p = plan(conn, &c.sql, &c.params)?;
        let warn = if p.contains("TEMP B-TREE") { " **[TEMP B-TREE]**" } else { "" };
        rep.line(&format!("- {} / {}{}: `{}`", c.group, c.name, warn, p));
    }
    Ok(())
}
