//! Bulk load that mimics the initial sync: per account, per folder, newest to oldest, in windows
//! of `batch` mails per transaction. The second location of a mail is found via Message-ID.

use crate::datagen::{self, Content, Meta, World};
use crate::rng::hash64;
use crate::schema::{self, Tokenizer};
use rusqlite::{Connection, ErrorCode, Result, params};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum BodyMode {
    /// No body text in the index (headers only).
    None,
    /// Body text for the mails whose body is cached (last 30 days of Inbox/Sent/Drafts, flagged, 18% opened).
    Realistic,
    /// Body text for every mail, truncated to the first n words.
    AllShort(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct LoadCfg {
    pub fts: Option<Tokenizer>,
    pub body: BodyMode,
    pub batch: usize,
    pub sort_fts: bool,
    pub scale: f64,
}

#[derive(Default, Debug)]
pub struct LoadStats {
    pub messages: u64,
    pub locations: u64,
    pub attachments: u64,
    pub dedup_hits: u64,
    pub id_collisions: u64,
    pub body_docs: u64,
    pub db_time: Duration,
    pub gen_time: Duration,
    pub wall: Duration,
    pub text_bytes: [u64; 5],
    pub wal_max: u64,
    pub transactions: u64,
}

pub fn remove_db(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let p = format!("{}{}", path.display(), suffix);
        let _ = std::fs::remove_file(p);
    }
}

pub fn file_size(p: &str) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

pub fn wal_size(path: &Path) -> u64 {
    file_size(&format!("{}-wal", path.display()))
}

pub struct Item<'a> {
    pub meta: &'a Meta,
    pub loc: datagen::LocMeta,
    pub uid: i64,
    pub content: Content,
}

pub fn load(path: &Path, world: &World, cfg: LoadCfg) -> Result<LoadStats> {
    remove_db(path);
    let wall = Instant::now();
    let conn = schema::open_writer(path)?;
    conn.execute_batch(schema::BASE_SCHEMA)?;
    if let Some(t) = cfg.fts {
        conn.execute_batch(&schema::fts_schema(t))?;
        conn.execute_batch("CREATE VIRTUAL TABLE message_fts_vocab USING fts5vocab(message_fts, 'row');")?;
    }
    let mut st = LoadStats::default();
    for a in 0..datagen::ACCOUNT_SIZES.len() {
        load_account(&conn, world, cfg, a, &mut st)?;
    }
    st.wall = wall.elapsed();
    Ok(st)
}

fn load_account(conn: &Connection, world: &World, cfg: LoadCfg, a: usize, st: &mut LoadStats) -> Result<()> {
    let account_id = a as i64 + 1;
    let (name, addr) = World::account_address(a);
    conn.execute(
        "INSERT INTO account(id, uuid, address, display_name, writes_enabled) VALUES (?1, ?2, ?3, ?4, 0)",
        params![account_id, format!("00000000-0000-4000-8000-{:012}", a + 1), addr, name],
    )?;
    let defs = datagen::mailbox_defs(a);
    let mut mbx_ids = Vec::new();
    for d in &defs {
        conn.execute(
            "INSERT INTO mailbox(account_id, name, delimiter, role, uidvalidity) VALUES (?1, ?2, '/', ?3, 1)",
            params![account_id, d.name, d.role.as_str()],
        )?;
        mbx_ids.push(conn.last_insert_rowid());
    }
    let t = Instant::now();
    let metas = datagen::plan_account(a, &defs, cfg.scale);
    st.gen_time += t.elapsed();

    // Per folder: (date, seq, meta index, location slot), oldest first => UID 1..n.
    let mut per_mb: Vec<Vec<(i64, u64, usize, usize)>> = vec![Vec::new(); defs.len()];
    for (i, m) in metas.iter().enumerate() {
        for (slot, l) in m.locs.iter().enumerate() {
            if let Some(l) = l {
                per_mb[l.mailbox as usize].push((m.date, m.seq, i, slot));
            }
        }
    }
    for (mb, list) in per_mb.iter_mut().enumerate() {
        list.sort_unstable();
        let mbx_id = mbx_ids[mb];
        let n = list.len();
        // Sync newest first.
        let ordered: Vec<(usize, i64)> = list.iter().enumerate().rev().map(|(pos, x)| (x.2 * 2 + x.3, pos as i64 + 1)).collect();
        for chunk in ordered.chunks(cfg.batch) {
            let t = Instant::now();
            let items: Vec<Item> = chunk
                .iter()
                .map(|&(key, uid)| {
                    let meta = &metas[key / 2];
                    let loc = meta.locs[key % 2].unwrap();
                    Item { meta, loc, uid, content: world.content(meta, a) }
                })
                .collect();
            st.gen_time += t.elapsed();
            let t = Instant::now();
            insert_batch(conn, cfg, account_id, mbx_id, defs[mb].role == datagen::Role::Drafts, &items, st)?;
            st.db_time += t.elapsed();
            st.wal_max = st.wal_max.max(wal_size(Path::new(conn.path().unwrap_or(""))));
        }
        conn.execute(
            "UPDATE mailbox SET uidnext = ?1, highestmodseq = ?2 WHERE id = ?3",
            params![n as i64 + 1, n as i64 * 3, mbx_id],
        )?;
    }
    Ok(())
}

fn body_for<'a>(cfg: LoadCfg, meta: &Meta, c: &'a Content) -> &'a str {
    match cfg.body {
        BodyMode::None => "",
        BodyMode::Realistic => {
            if meta.has_body { &c.body } else { "" }
        }
        BodyMode::AllShort(n) => datagen::truncate_words(&c.body, n),
    }
}

/// Bits below the seconds in the time-ordered id (env SPIKE_ID_SHIFT, default 0 = plain seconds).
pub fn id_shift() -> u32 {
    std::env::var("SPIKE_ID_SHIFT").ok().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// First candidate for a time-ordered id; on a conflict the caller tries id + 1.
pub fn alloc_base_id(date: i64, _seq: u64) -> i64 {
    date.clamp(0, (1i64 << 33) - 1) << id_shift()
}

pub fn insert_batch(
    conn: &Connection,
    cfg: LoadCfg,
    account_id: i64,
    mbx_id: i64,
    is_drafts: bool,
    items: &[Item],
    st: &mut LoadStats,
) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let mut find = conn.prepare_cached("SELECT id FROM message WHERE account_id = ?1 AND message_id_hdr = ?2")?;
    let mut ins_msg = conn.prepare_cached(
        "INSERT INTO message(id, account_id, message_id_hdr, thread_id, date, subject, from_name, from_addr, \
         to_addrs, cc_addrs, size, preview, has_attachments, body_ref) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut ins_att = conn.prepare_cached(
        "INSERT INTO attachment(message_id, part, filename, mime_type, size) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut ins_loc = conn.prepare_cached(
        "INSERT INTO message_location(mailbox_id, message_id, uid, seen, flagged, answered, draft, deleted, keywords, modseq) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
    )?;
    let mut fts_rows: Vec<(i64, &Item)> = Vec::new();
    for it in items {
        let c = &it.content;
        let existing: Option<i64> = match find.query_row(params![account_id, c.message_id_hdr], |r| r.get(0)) {
            Ok(id) => Some(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };
        let id = match existing {
            Some(id) => {
                st.dedup_hits += 1;
                id
            }
            None => {
                let body = body_for(cfg, it.meta, c);
                let body_ref = if !body.is_empty() || (matches!(cfg.body, BodyMode::None) && it.meta.has_body) {
                    Some(format!("{:02x}/{:016x}.eml.zst", it.meta.seq & 0xff, it.meta.seq))
                } else {
                    None
                };
                let mut id = alloc_base_id(it.meta.date, it.meta.seq);
                loop {
                    match ins_msg.execute(params![
                        id,
                        account_id,
                        c.message_id_hdr,
                        c.thread_id,
                        it.meta.date,
                        c.subject,
                        c.from_name,
                        c.from_addr,
                        c.to,
                        c.cc,
                        c.size,
                        c.preview,
                        !c.attachments.is_empty(),
                        body_ref
                    ]) {
                        Ok(_) => break,
                        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::ConstraintViolation => {
                            st.id_collisions += 1;
                            id += 1;
                        }
                        Err(e) => return Err(e),
                    }
                }
                for (i, att) in c.attachments.iter().enumerate() {
                    ins_att.execute(params![id, format!("{}", i + 2), att.name, att.mime, att.size])?;
                    st.attachments += 1;
                }
                st.messages += 1;
                if cfg.fts.is_some() {
                    if cfg.sort_fts {
                        fts_rows.push((id, it));
                    } else {
                        insert_fts(conn, cfg, id, it, st)?;
                    }
                }
                id
            }
        };
        let keywords: Option<&str> = if hash64(it.meta.seq ^ 77).is_multiple_of(20) { Some("$Important") } else { None };
        ins_loc.execute(params![
            mbx_id,
            id,
            it.uid,
            it.loc.seen,
            it.loc.flagged,
            it.loc.answered,
            is_drafts,
            keywords,
            it.uid * 3
        ])?;
        st.locations += 1;
    }
    // FTS5 flushes its pending-terms buffer whenever a rowid is smaller than the previous one.
    // The sync runs newest -> oldest (descending ids), so sort each window ascending first.
    fts_rows.sort_unstable_by_key(|x| x.0);
    for (id, it) in fts_rows {
        insert_fts(conn, cfg, id, it, st)?;
    }
    drop((find, ins_msg, ins_att, ins_loc));
    conn.execute_batch("COMMIT")?;
    st.transactions += 1;
    Ok(())
}

fn insert_fts(conn: &Connection, cfg: LoadCfg, id: i64, it: &Item, st: &mut LoadStats) -> Result<()> {
    let c = &it.content;
    let body = body_for(cfg, it.meta, c);
    let sender = c.sender_text();
    let recipients = c.recipients_text();
    let atts = c.attachment_text();
    st.text_bytes[0] += c.subject.len() as u64;
    st.text_bytes[1] += sender.len() as u64;
    st.text_bytes[2] += recipients.len() as u64;
    st.text_bytes[3] += body.len() as u64;
    st.text_bytes[4] += atts.len() as u64;
    if !body.is_empty() {
        st.body_docs += 1;
    }
    let mut ins = conn.prepare_cached(
        "INSERT INTO message_fts(rowid, subject, sender, recipients, body, attachments) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    ins.execute(params![id, c.subject, sender, recipients, body, atts])?;
    Ok(())
}

/// Bytes per table / index via dbstat.
pub fn object_sizes(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut s = conn.prepare("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC")?;
    let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    rows.collect()
}
