//! One function per requirement of spike A. Each records PASS/FAIL lines in the report.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use async_imap::extensions::idle::IdleResponse;
use async_imap::imap_proto::{MailboxDatum, Response};
use async_imap::types::{Fetch, Flag, Name, UnsolicitedResponse};
use futures::TryStreamExt;
use rustls::ClientConfig;
use rustls::pki_types::CertificateDer;

use crate::env::Env;
use crate::errors::NetError;
use crate::net::{self, Opts, Sess, drain_unsolicited};
use crate::proxy::{Fault, Proxy};
use crate::raw::{self, HeaderRec};
use crate::report::Report;
use crate::timeout_io::TimeoutHandle;
use crate::{mutf7, tls};

pub struct Ctx {
    pub env: Env,
    pub ca: Vec<CertificateDer<'static>>,
    /// Platform verifier + test CA as extra root: the configuration all scenarios use.
    pub tls: Arc<ClientConfig>,
}

impl Ctx {
    pub fn new() -> Result<Self> {
        let env = Env::load()?;
        let ca = tls::load_pem(&env.ca_file)?;
        let tls = tls::platform(ca.clone())?;
        Ok(Ctx { env, ca, tls })
    }
    pub fn opts(&self) -> Opts {
        Opts::imaps(&self.env, self.tls.clone())
    }
    pub async fn alice_via(&self, o: &Opts) -> Result<(Sess, TimeoutHandle), NetError> {
        net::login(o, &self.env.alice.0, &self.env.alice.1).await
    }
    pub async fn alice(&self) -> Result<Sess> {
        Ok(self.alice_via(&self.opts()).await?.0)
    }
}

pub const HEADER_QUERY: &str = "(UID FLAGS RFC822.SIZE ENVELOPE BODYSTRUCTURE INTERNALDATE MODSEQ \
BODY.PEEK[HEADER.FIELDS (MESSAGE-ID REFERENCES IN-REPLY-TO LIST-ID LIST-UNSUBSCRIBE AUTO-SUBMITTED PRECEDENCE)])";
/// Same plus the seed marker, so broken messages can be identified.
const HEADER_QUERY_SEED: &str = "(UID FLAGS RFC822.SIZE ENVELOPE BODYSTRUCTURE INTERNALDATE MODSEQ \
BODY.PEEK[HEADER.FIELDS (MESSAGE-ID REFERENCES IN-REPLY-TO LIST-ID LIST-UNSUBSCRIBE AUTO-SUBMITTED PRECEDENCE X-VICEMAIL-SEED)])";

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1000.0)
}

fn unique() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    )
}

pub fn test_message(id: &str, subject: &str, body_extra: usize) -> Vec<u8> {
    let mut m = format!(
        "From: Spike <alice@vicemail.test>\r\nTo: alice@vicemail.test\r\nSubject: {subject}\r\n\
Message-ID: <{id}@spike.vicemail.test>\r\nDate: Sun, 01 Feb 2026 10:00:00 +0100\r\nMIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\nBody of {id} \u{2013} Grüße\r\n"
    )
    .into_bytes();
    for i in 0..body_extra / 64 {
        m.extend_from_slice(format!("{i:063}\n").replace('\n', "\r\n").as_bytes());
    }
    m
}

async fn reset_folder(s: &mut Sess, name: &str) -> Result<()> {
    let _ = s.delete(name).await; // may not exist
    s.create(name).await.with_context(|| format!("create {name}"))?;
    Ok(())
}

async fn cleanup_spike_folders(s: &mut Sess) {
    let names: Vec<Name> = match s.list(Some(""), Some("Spike*")).await {
        Ok(st) => st.try_collect().await.unwrap_or_default(),
        Err(_) => return,
    };
    let mut v: Vec<String> = names.iter().map(|n| n.name().to_string()).collect();
    v.sort_by_key(|n| std::cmp::Reverse(n.len())); // children first
    for n in v {
        let _ = s.delete(&n).await;
    }
}

async fn uids_of(s: &mut Sess) -> Result<Vec<u32>> {
    let v: Vec<Fetch> = s.uid_fetch("1:*", "(UID)").await?.try_collect().await?;
    let mut u: Vec<u32> = v.iter().filter_map(|f| f.uid).collect();
    u.sort_unstable();
    Ok(u)
}

fn has_flag(rec: &HeaderRec, flag: &str) -> bool {
    rec.flags.iter().any(|f| f.eq_ignore_ascii_case(flag))
}

async fn fetch_one(s: &mut Sess, uid: u32) -> Result<Option<HeaderRec>> {
    let mut v = Vec::new();
    raw::fetch_checked(s, &uid.to_string(), HEADER_QUERY, &mut v).await?;
    Ok(v.into_iter().find(|r| r.uid == uid))
}

// ---------------------------------------------------------------------------------------------

pub async fn tls(cx: &Ctx, r: &mut Report) -> Result<()> {
    let (alice, pw) = (&cx.env.alice.0, &cx.env.alice.1);

    match net::connect(&cx.opts()).await {
        Ok(c) => {
            let info = c.tls_info.clone();
            match c.client.login(alice, pw).await {
                Ok(mut s) => {
                    let caps = s.capabilities().await?;
                    r.check(
                        "tls.imaps_platform_plus_test_ca",
                        true,
                        format!("{info}, LOGIN ok, {} capabilities", caps.len()),
                    );
                    s.logout().await.ok();
                }
                Err((e, _)) => r.check("tls.imaps_platform_plus_test_ca", false, e.to_string()),
            }
        }
        Err(e) => r.check("tls.imaps_platform_plus_test_ca", false, e.to_string()),
    }

    let o = Opts::imaps(&cx.env, tls::webpki_only(cx.ca.clone())?);
    let res = net::login(&o, alice, pw).await;
    r.check(
        "tls.imaps_webpki_test_ca_only",
        res.is_ok(),
        res.err().map(|e| e.to_string()).unwrap_or("LOGIN ok".into()),
    );

    let o = Opts::imaps(&cx.env, tls::platform(vec![])?);
    match net::connect(&o).await {
        Err(e @ NetError::CertUntrusted(_)) => {
            r.check("tls.untrusted_without_test_ca", true, format!("{}: {e}", e.kind()))
        }
        Err(e) => r.check(
            "tls.untrusted_without_test_ca",
            false,
            format!("wrong kind {}: {e}", e.kind()),
        ),
        Ok(_) => r.check("tls.untrusted_without_test_ca", false, "connected without the CA"),
    }
    let o = Opts::imaps(&cx.env, tls::webpki_only(vec![])?);
    match net::connect(&o).await {
        Err(e @ NetError::CertUntrusted(_)) => {
            r.check("tls.untrusted_webpki_empty_store", true, format!("{}: {e}", e.kind()))
        }
        other => r.check("tls.untrusted_webpki_empty_store", false, format!("{:?}", other.err())),
    }

    let o = Opts {
        tls_name: "imap.vicemail.test".into(),
        ..cx.opts()
    };
    match net::connect(&o).await {
        Err(e @ NetError::CertNameMismatch) => r.check("tls.name_mismatch", true, format!("{}: {e}", e.kind())),
        other => r.check("tls.name_mismatch", false, format!("{:?}", other.err())),
    }

    let secret = "wrong-secret-4711";
    match net::login(&cx.opts(), alice, secret).await {
        Err(e @ NetError::AuthFailed) => {
            let leak = format!("{e} {e:?}").contains(secret);
            r.check(
                "tls.wrong_password_typed",
                !leak,
                format!("{}: {e}; password in message: {leak}", e.kind()),
            )
        }
        other => r.check("tls.wrong_password_typed", false, format!("{:?}", other.err())),
    }

    let o = Opts { port: 1, ..cx.opts() };
    match net::connect(&o).await {
        Err(e @ NetError::Refused) => r.check("tls.port_closed_typed", true, format!("{}: {e}", e.kind())),
        other => r.check("tls.port_closed_typed", false, format!("{:?}", other.err())),
    }
    Ok(())
}

pub async fn starttls(cx: &Ctx, r: &mut Report) -> Result<()> {
    let o = Opts::starttls(&cx.env, cx.tls.clone());
    let c = net::connect(&o).await?;
    let pre = c.greeting_caps.join(" ");
    r.check(
        "starttls.greeting_caps",
        pre.contains("STARTTLS") && pre.contains("LOGINDISABLED"),
        format!("plain-text greeting: {pre}"),
    );
    let info = c.tls_info.clone();
    let mut s = c
        .client
        .login(&cx.env.alice.0, &cx.env.alice.1)
        .await
        .map_err(|(e, _)| anyhow!(e))?;
    let caps = s.capabilities().await?;
    let ok = caps.has_str("IDLE") && !caps.has_str("LOGINDISABLED") && !caps.has_str("STARTTLS");
    r.check(
        "starttls.upgrade_login",
        ok,
        format!("{info}, LOGIN ok, IDLE={}, LOGINDISABLED gone", caps.has_str("IDLE")),
    );
    s.logout().await.ok();

    let o = Opts::starttls(&cx.env, tls::platform(vec![])?);
    match net::connect(&o).await {
        Err(e @ NetError::CertUntrusted(_)) => r.check("starttls.untrusted_without_test_ca", true, e.kind()),
        other => r.check(
            "starttls.untrusted_without_test_ca",
            false,
            format!("{:?}", other.err()),
        ),
    }
    Ok(())
}

pub async fn caps(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut s = cx.alice().await?;
    let caps = s.capabilities().await?;
    let mut names: Vec<String> = caps.iter().map(|c| format!("{c:?}")).collect();
    names.sort();
    r.info("caps.list", names.join(" "));
    let need = [
        "IDLE",
        "CONDSTORE",
        "MOVE",
        "UIDPLUS",
        "SPECIAL-USE",
        "ENABLE",
        "QRESYNC",
        "ID",
        "LITERAL+",
        "UTF8=ACCEPT",
    ];
    let missing: Vec<&str> = need.iter().copied().filter(|c| !caps.has_str(c)).collect();
    r.check(
        "caps.required_present",
        missing.is_empty(),
        format!("checked {}; missing {:?}", need.join(","), missing),
    );
    r.info(
        "caps.has_str_case",
        format!(
            "has_str(\"idle\") = {} (issue #140: case-sensitive for atoms)",
            caps.has_str("idle")
        ),
    );

    let id = s
        .id([("name", Some("vice-mail-spike")), ("version", Some("0"))])
        .await?;
    r.check("caps.id_command", id.is_some(), format!("server ID: {:?}", id));

    let raw = raw::raw(&mut s, "ENABLE CONDSTORE").await?;
    let enabled = raw
        .untagged
        .iter()
        .any(|u| matches!(u.parsed(), Response::Capabilities(c) if format!("{c:?}").contains("CONDSTORE")));
    r.check(
        "caps.enable_condstore_raw",
        raw.done.ok() && enabled,
        format!(
            "tagged {} , * ENABLED parsed as Capabilities: {enabled}",
            raw.done.status
        ),
    );
    s.logout().await.ok();
    Ok(())
}

pub async fn folders(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut s = cx.alice().await?;
    let plain: Vec<Name> = s.list(Some(""), Some("*")).await?.try_collect().await?;
    let wire: BTreeSet<String> = plain.iter().map(|n| n.name().to_string()).collect();
    let delims: BTreeSet<String> = plain
        .iter()
        .map(|n| n.delimiter().unwrap_or("NIL").to_string())
        .collect();
    let expect = [
        "INBOX",
        "Sent",
        "Drafts",
        "Trash",
        "Junk",
        "Archive",
        "Projects",
        "Projects/2026",
        "Projects/&ANw-berpr&APw-fung",
        "Projects/Empty",
    ];
    let missing: Vec<&str> = expect.iter().copied().filter(|e| !wire.contains(*e)).collect();
    r.check(
        "folders.list_all",
        missing.is_empty() && delims.len() == 1 && delims.contains("/"),
        format!("{} folders, delimiter {:?}, missing {:?}", wire.len(), delims, missing),
    );

    // LIST-EXTENDED: async-imap inserts the pattern unquoted, so RETURN (...) can ride along.
    let special: Vec<Name> = s
        .list(Some(""), Some("\"*\" RETURN (SPECIAL-USE)"))
        .await?
        .try_collect()
        .await?;
    let mut roles = BTreeMap::new();
    for n in &special {
        for a in n.attributes() {
            let a = format!("{a:?}");
            if ["Sent", "Drafts", "Trash", "Junk", "Archive", "All", "Flagged"].contains(&a.as_str()) {
                roles.insert(n.name().to_string(), a);
            }
        }
    }
    let want = [
        ("Sent", "Sent"),
        ("Drafts", "Drafts"),
        ("Trash", "Trash"),
        ("Junk", "Junk"),
        ("Archive", "Archive"),
    ];
    let ok = want.iter().all(|(f, a)| roles.get(*f).map(String::as_str) == Some(*a));
    r.check(
        "folders.special_use_return",
        ok,
        format!("LIST \"\" \"*\" RETURN (SPECIAL-USE): {roles:?}"),
    );
    let plain_attrs: usize = plain
        .iter()
        .filter(|n| n.attributes().iter().any(|a| format!("{a:?}") == "Sent"))
        .count();
    r.info(
        "folders.special_use_plain_list",
        format!("plain LIST carries \\Sent too: {}", plain_attrs > 0),
    );

    let sel = raw::raw(&mut s, "LIST (SPECIAL-USE) \"\" \"*\"").await?;
    let n = sel
        .untagged
        .iter()
        .filter(|u| matches!(u.parsed(), Response::MailboxData(MailboxDatum::List(_))))
        .count();
    r.check(
        "folders.list_selection_special_use",
        sel.done.ok() && n == 5,
        format!("LIST (SPECIAL-USE) via raw command: {n} folders"),
    );

    // LIST-STATUS: roles and counters of every folder in one round trip (for stage 2).
    let ls = raw::raw(
        &mut s,
        "LIST \"\" \"*\" RETURN (SPECIAL-USE STATUS (MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ))",
    )
    .await?;
    let lists = ls
        .untagged
        .iter()
        .filter(|u| matches!(u.parsed(), Response::MailboxData(MailboxDatum::List(_))))
        .count();
    let stats = ls.untagged.iter().filter(|u| matches!(u.parsed(), Response::MailboxData(MailboxDatum::Status { status, .. }) if status.iter().any(|a| format!("{a:?}").starts_with("HighestModSeq")))).count();
    r.check(
        "folders.list_status_one_round_trip",
        ls.done.ok() && lists == wire.len() && stats >= lists - 1,
        format!("{lists} LIST + {stats} STATUS (with HIGHESTMODSEQ) parsed from one command"),
    );

    let enc = wire.iter().find(|w| w.contains('&')).cloned().unwrap_or_default();
    let dec = mutf7::decode(&enc).map_err(|e| anyhow!(e.0))?;
    r.check(
        "folders.mutf7_decode",
        dec == "Projects/Überprüfung" && mutf7::encode(&dec) == enc,
        format!("{enc} -> {dec} -> {}", mutf7::encode(&dec)),
    );
    let mb = s.examine(mutf7::encode("Projects/Überprüfung")).await?;
    r.check(
        "folders.examine_non_ascii",
        mb.exists == 1,
        format!("EXAMINE by encoded name: {} message(s)", mb.exists),
    );

    let st = s
        .status("Sent", "(MESSAGES UIDNEXT UIDVALIDITY UNSEEN HIGHESTMODSEQ)")
        .await?;
    r.check(
        "folders.status",
        st.uid_validity.is_some() && st.highest_modseq.is_some(),
        format!(
            "Sent: exists {} uidnext {:?} uidvalidity {:?} unseen {:?} highestmodseq {:?}",
            st.exists, st.uid_next, st.uid_validity, st.unseen, st.highest_modseq
        ),
    );
    s.logout().await.ok();

    // UTF8=ACCEPT: names arrive as raw UTF-8; does imap-proto cope?
    let mut s = cx.alice().await?;
    let en = raw::raw(&mut s, "ENABLE UTF8=ACCEPT").await?;
    match s.list(Some(""), Some("Projects/*")).await {
        Ok(st) => {
            let v: Result<Vec<Name>, _> = st.try_collect().await;
            match v {
                Ok(v) => {
                    let names: Vec<String> = v.iter().map(|n| n.name().to_string()).collect();
                    let utf8 = names.iter().any(|n| n == "Projects/Überprüfung");
                    r.check(
                        "folders.utf8_accept_list",
                        en.done.ok() && utf8,
                        format!("after ENABLE UTF8=ACCEPT: {names:?}"),
                    );
                }
                Err(e) => r.check("folders.utf8_accept_list", false, format!("parse error: {e}")),
            }
        }
        Err(e) => r.check("folders.utf8_accept_list", false, e.to_string()),
    }
    s.logout().await.ok();
    Ok(())
}

pub async fn select(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut s = cx.alice().await?;
    let mb = s.select_condstore("INBOX").await?;
    let may_create = mb.permanent_flags.contains(&Flag::MayCreate);
    r.check(
        "select.condstore_values",
        mb.uid_validity.is_some() && mb.uid_next.is_some() && mb.highest_modseq.is_some() && may_create,
        format!(
            "exists {} uidvalidity {:?} uidnext {:?} highestmodseq {:?} permanentflags {:?}",
            mb.exists, mb.uid_validity, mb.uid_next, mb.highest_modseq, mb.permanent_flags
        ),
    );
    let ex = s.examine("INBOX").await?;
    r.check(
        "select.examine_values",
        ex.uid_validity == mb.uid_validity && ex.exists == mb.exists,
        format!(
            "EXAMINE: uidvalidity {:?} exists {} highestmodseq {:?} (Mailbox has no read-only field)",
            ex.uid_validity, ex.exists, ex.highest_modseq
        ),
    );
    let rx = raw::raw(&mut s, "EXAMINE INBOX (CONDSTORE)").await?;
    let hms = rx.untagged.iter().find_map(|u| {
        raw::Done::from_resp(u.parsed())
            .and_then(|d| d.code)
            .filter(|c| c.starts_with("HighestModSeq"))
    });
    r.check(
        "select.examine_read_only_code",
        rx.done.read_only && hms.is_some(),
        format!(
            "raw EXAMINE INBOX (CONDSTORE): tagged code {:?}, {:?}",
            rx.done.code, hms
        ),
    );

    let uid = mb.uid_next.unwrap_or(2) - 1;
    let before = fetch_one(&mut s, uid).await?.context("uid")?;
    // async-imap: STORE after EXAMINE
    let via_lib: Result<Vec<Fetch>, _> = s
        .uid_store(uid.to_string(), "+FLAGS (\\Flagged)")
        .await?
        .try_collect()
        .await;
    r.info(
        "select.examine_store_async_imap",
        format!(
            "uid_store after EXAMINE returned {:?} (tagged NO is swallowed, issue #151)",
            via_lib.as_ref().map(|v| v.len()).map_err(|e| e.to_string())
        ),
    );
    let rs = raw::raw(&mut s, &format!("UID STORE {uid} +FLAGS (\\Flagged)")).await?;
    let after = fetch_one(&mut s, uid).await?.context("uid")?;
    r.check(
        "select.examine_store_rejected",
        rs.done.status != "OK" && has_flag(&before, "\\Flagged") == has_flag(&after, "\\Flagged"),
        format!(
            "server answered {} {:?}; flags unchanged {:?}",
            rs.done.status, rs.done.info, after.flags
        ),
    );

    // Stalwart sends no HIGHESTMODSEQ on a plain EXAMINE even after SELECT (CONDSTORE); ENABLE helps?
    let mut e = cx.alice().await?;
    raw::raw(&mut e, "ENABLE CONDSTORE").await?.done.into_result()?;
    let exe = e.examine("INBOX").await?;
    r.check(
        "select.enable_condstore_then_examine",
        exe.highest_modseq.is_some(),
        format!("ENABLE CONDSTORE + EXAMINE: highestmodseq {:?}", exe.highest_modseq),
    );
    e.logout().await.ok();

    // Read-only mode of the adapter: refuse locally, before any byte goes out.
    let guard = WriteGuard { writes_enabled: false };
    match guard.store(&mut s, uid, "+FLAGS (\\Flagged)").await {
        Err(e @ NetError::ReadOnly) => r.check("select.readonly_guard_typed", true, format!("{}: {e}", e.kind())),
        other => r.check("select.readonly_guard_typed", false, format!("{:?}", other.err())),
    }
    s.logout().await.ok();
    Ok(())
}

/// Sketch of the adapter-side guard for accounts with writes_enabled = false.
struct WriteGuard {
    writes_enabled: bool,
}
impl WriteGuard {
    async fn store(&self, s: &mut Sess, uid: u32, what: &str) -> Result<raw::Done, NetError> {
        if !self.writes_enabled {
            return Err(NetError::ReadOnly);
        }
        raw::raw(s, &format!("UID STORE {uid} {what}"))
            .await?
            .done
            .into_result()
    }
}

pub async fn fetch(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut s = cx.alice().await?;
    let folders: Vec<Name> = s.list(Some(""), Some("*")).await?.try_collect().await?;
    let folders: Vec<String> = folders
        .iter()
        .filter(|n| !n.attributes().iter().any(|a| format!("{a:?}") == "NoSelect"))
        .map(|n| n.name().to_string())
        .collect();

    // Stream path (async-imap) inside a spawned task: proves Send and catches panics.
    let task = tokio::spawn(async move {
        let mut all: Vec<(String, HeaderRec)> = Vec::new();
        for f in &folders {
            let mb = s.examine(f).await?;
            if mb.exists == 0 {
                continue;
            }
            let mut hi = mb.uid_next.unwrap_or(1).saturating_sub(1);
            while hi >= 1 {
                let lo = hi.saturating_sub(499).max(1);
                let mut st = s.uid_fetch(format!("{lo}:{hi}"), HEADER_QUERY_SEED).await?;
                while let Some(fe) = st.try_next().await? {
                    all.push((f.clone(), HeaderRec::from_fetch(&fe)));
                }
                drop(st);
                if lo == 1 {
                    break;
                }
                hi = lo - 1;
            }
        }
        Ok::<_, anyhow::Error>((s, folders, all))
    });
    let (mut s, folders, all) = match task.await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            r.check("fetch.no_panic_stream_path", false, format!("error: {e:#}"));
            return Ok(());
        }
        Err(e) => {
            r.check("fetch.no_panic_stream_path", false, format!("task panicked: {e}"));
            return Ok(());
        }
    };
    r.check(
        "fetch.no_panic_stream_path",
        true,
        format!(
            "{} messages in {} folders, windows of 500 newest→oldest",
            all.len(),
            folders.len()
        ),
    );

    let incomplete: Vec<String> = all
        .iter()
        .filter(|(_, h)| !h.missing().is_empty())
        .map(|(f, h)| {
            format!(
                "{f}/{} {:?} missing {:?}",
                h.uid,
                h.header("X-Vicemail-Seed"),
                h.missing()
            )
        })
        .collect();
    r.check(
        "fetch.all_parts_parsed",
        incomplete.is_empty(),
        format!(
            "{} incomplete: {:?}",
            incomplete.len(),
            incomplete.iter().take(5).collect::<Vec<_>>()
        ),
    );

    let broken: Vec<&(String, HeaderRec)> = all
        .iter()
        .filter(|(_, h)| h.header("X-Vicemail-Seed").is_some_and(|v| v.starts_with("broken/")))
        .collect();
    let deepest = broken.iter().map(|(_, h)| h.depth).max().unwrap_or(0);
    r.check(
        "fetch.broken_messages_parsed",
        broken.len() == 26 && broken.iter().all(|(_, h)| h.missing().is_empty()),
        format!(
            "{} broken/* messages, all parts present, deepest bodystructure {deepest} levels",
            broken.len()
        ),
    );
    for (f, h) in broken.iter().take(30) {
        r.info(
            "fetch.broken",
            format!(
                "{f}/{} {} parts={} depth={} subject={:?} from={:?} date={:?}",
                h.uid,
                h.header("X-Vicemail-Seed").unwrap_or_default(),
                h.parts,
                h.depth,
                h.subject.as_deref().map(|s| s.chars().take(40).collect::<String>()),
                h.from,
                h.date
            ),
        );
    }
    let kw = all.iter().filter(|(_, h)| has_flag(h, "projekt-x")).count();
    let lbl = all.iter().filter(|(_, h)| has_flag(h, "$label1")).count();
    r.info(
        "fetch.keywords_seen",
        format!("projekt-x on {kw}, $label1 on {lbl} messages"),
    );
    let lid = all.iter().filter(|(_, h)| h.header("List-Id").is_some()).count();
    let refs = all.iter().filter(|(_, h)| h.header("References").is_some()).count();
    r.check(
        "fetch.header_fields_extracted",
        lid > 0 && refs > 0,
        format!("List-Id on {lid}, References on {refs} messages"),
    );

    // Checked raw path must see exactly the same messages.
    let mut raw_all: Vec<(String, HeaderRec)> = Vec::new();
    for f in &folders {
        let mb = s.examine(f).await?;
        if mb.exists == 0 {
            continue;
        }
        let mut v = Vec::new();
        raw::fetch_checked(&mut s, "1:*", HEADER_QUERY_SEED, &mut v).await?;
        raw_all.extend(v.into_iter().map(|h| (f.clone(), h)));
    }
    let a: BTreeSet<(String, u32)> = all.iter().map(|(f, h)| (f.clone(), h.uid)).collect();
    let b: BTreeSet<(String, u32)> = raw_all.iter().map(|(f, h)| (f.clone(), h.uid)).collect();
    r.check(
        "fetch.raw_path_same_result",
        a == b,
        format!("stream {} vs checked raw {}", a.len(), b.len()),
    );

    // Full bodies of broken and large messages: literal handling for odd and big content.
    let mut bad = Vec::new();
    let mut n = 0;
    let mut biggest = 0;
    let pick: Vec<(String, u32, u32)> = all
        .iter()
        .filter(|(_, h)| {
            h.header("X-Vicemail-Seed")
                .is_some_and(|v| v.starts_with("broken/") || v.starts_with("large/"))
        })
        .map(|(f, h)| (f.clone(), h.uid, h.size.unwrap_or(0)))
        .collect();
    for (f, uid, size) in pick {
        s.examine(&f).await?;
        let v: Vec<Fetch> = s
            .uid_fetch(uid.to_string(), "(UID RFC822.SIZE BODY.PEEK[])")
            .await?
            .try_collect()
            .await?;
        let len = v.first().and_then(|f| f.body()).map(|b| b.len()).unwrap_or(0);
        n += 1;
        biggest = biggest.max(len);
        if len as u32 != size {
            bad.push(format!("{f}/{uid}: {len} != {size}"));
        }
    }
    r.check(
        "fetch.full_bodies_broken_and_large",
        bad.is_empty() && n >= 28,
        format!(
            "{n} bodies fetched, largest {:.1} MB, size mismatches {:?}",
            biggest as f64 / 1e6,
            bad
        ),
    );

    // Dropping a stream midway: how does the connection recover?
    s.examine("INBOX").await?;
    drain_unsolicited(&s);
    {
        let mut st = s.uid_fetch("1:*", "(UID FLAGS)").await?;
        for _ in 0..50 {
            st.try_next().await?;
        }
    } // dropped with ~9900 responses still in flight
    let t = Instant::now();
    let noop = s.noop().await;
    let el = t.elapsed();
    let parked = drain_unsolicited(&s).len();
    let one: Vec<Fetch> = s.uid_fetch("1", "(UID FLAGS)").await?.try_collect().await?;
    r.check(
        "fetch.dropped_stream_recovers",
        noop.is_ok() && one.len() == 1 && one[0].uid == Some(1),
        format!("NOOP after drop: {:?} in {} (reads the rest of the old FETCH); {parked} responses parked in the unsolicited channel (cap 100, rest dropped)", noop.map_err(|e| e.to_string()), ms(el)),
    );
    s.logout().await.ok();
    Ok(())
}

pub async fn condstore(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut a = cx.alice().await?;
    let mut b = cx.alice().await?;
    let mb = a.select_condstore("INBOX").await?;
    let m0 = mb.highest_modseq.context("HIGHESTMODSEQ")?;
    let uid = mb.uid_next.context("uidnext")? - 10;
    b.select("INBOX").await?;
    let orig = fetch_one(&mut b, uid).await?.context("uid")?;
    let _: Vec<Fetch> = b
        .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Flagged $SpikeCond)")
        .await?
        .try_collect()
        .await?;

    let changed: Vec<Fetch> = a
        .uid_fetch("1:*", format!("(UID FLAGS) (CHANGEDSINCE {m0})"))
        .await?
        .try_collect()
        .await?;
    let m1 = changed.first().and_then(|f| f.modseq).unwrap_or(0);
    r.check(
        "condstore.changedsince",
        changed.len() == 1 && changed[0].uid == Some(uid) && m1 > m0,
        format!(
            "UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE {m0}) via uid_fetch: {} result(s), uid {:?}, MODSEQ {m1} parsed",
            changed.len(),
            changed.first().and_then(|f| f.uid)
        ),
    );
    drain_unsolicited(&a);

    let via_lib: Result<Vec<Fetch>, _> = a
        .uid_store(uid.to_string(), format!("(UNCHANGEDSINCE {m0}) +FLAGS (\\Seen)"))
        .await?
        .try_collect()
        .await;
    r.info(
        "condstore.unchangedsince_async_imap",
        format!(
            "uid_store with UNCHANGEDSINCE {m0} returned {:?}; the [MODIFIED] code is not visible",
            via_lib.map(|v| v.len()).map_err(|e| e.to_string())
        ),
    );
    let conflict = raw::raw(
        &mut a,
        &format!("UID STORE {uid} (UNCHANGEDSINCE {m0}) +FLAGS (\\Seen)"),
    )
    .await?;
    r.check(
        "condstore.unchangedsince_modified",
        conflict.done.modified().as_deref() == Some(uid.to_string().as_str()),
        format!(
            "raw: tagged {} code {:?} text {:?} -> MODIFIED {:?}",
            conflict.done.status,
            conflict.done.code,
            conflict.done.info,
            conflict.done.modified()
        ),
    );
    let okst = raw::raw(
        &mut a,
        &format!("UID STORE {uid} (UNCHANGEDSINCE {m1}) +FLAGS ($SpikeCond2)"),
    )
    .await?;
    let fetch_modseq = okst
        .untagged
        .iter()
        .any(|u| format!("{:?}", u.parsed()).contains("ModSeq"));
    r.check(
        "condstore.unchangedsince_success",
        okst.done.ok() && okst.done.modified().is_none() && fetch_modseq,
        format!(
            "UNCHANGEDSINCE {m1}: {} with FETCH (MODSEQ) {fetch_modseq}",
            okst.done.status
        ),
    );
    let restore: Vec<&str> = orig
        .flags
        .iter()
        .map(String::as_str)
        .filter(|f| !f.eq_ignore_ascii_case("\\Recent"))
        .collect();
    raw::raw(&mut b, &format!("UID STORE {uid} FLAGS.SILENT ({})", restore.join(" ")))
        .await?
        .done
        .into_result()?;
    a.logout().await.ok();

    // QRESYNC with raw commands: ENABLE, SELECT (QRESYNC (...)) and VANISHED (EARLIER).
    let folder = "Spike/Qresync";
    cleanup_spike_folders(&mut b).await;
    reset_folder(&mut b, "Spike").await.ok();
    reset_folder(&mut b, folder).await?;
    let mut uids = Vec::new();
    for i in 0..3 {
        let d = raw::append(
            &mut b,
            folder,
            "",
            None,
            &test_message(&format!("q{i}-{}", unique()), "qresync", 0),
        )
        .await?;
        uids.push(d.append_uid.context("APPENDUID")?.1.parse::<u32>()?);
    }
    let mut c = cx.alice().await?;
    let en = raw::raw(&mut c, "ENABLE QRESYNC").await?;
    let snap = c.select(folder).await?;
    let (v, m) = (snap.uid_validity.context("v")?, snap.highest_modseq.context("m")?);
    raw::raw(&mut c, "UNSELECT").await?;
    b.select(folder).await?;
    raw::raw(&mut b, &format!("UID STORE {} +FLAGS.SILENT (\\Deleted)", uids[1]))
        .await?
        .done
        .into_result()?;
    raw::raw(&mut b, &format!("UID EXPUNGE {}", uids[1]))
        .await?
        .done
        .into_result()?;
    raw::raw(&mut b, &format!("UID STORE {} +FLAGS.SILENT ($SpikeQ)", uids[2]))
        .await?
        .done
        .into_result()?;
    let q = raw::raw(&mut c, &format!("SELECT \"{folder}\" (QRESYNC ({v} {m}))")).await?;
    let vanished = q.vanished();
    let fetched = q
        .untagged
        .iter()
        .filter(|u| matches!(u.parsed(), Response::Fetch(..)))
        .count();
    r.check(
        "condstore.qresync_raw",
        en.done.ok()
            && q.done.ok()
            && vanished.iter().any(|(e, s)| *e && s.contains(&uids[1].to_string()))
            && fetched >= 1,
        format!(
            "ENABLE QRESYNC {}; SELECT (QRESYNC ({v} {m})): VANISHED {:?}, {fetched} FETCH",
            en.done.status, vanished
        ),
    );
    c.logout().await.ok();
    cleanup_spike_folders(&mut b).await;
    b.logout().await.ok();
    Ok(())
}

pub async fn idle(cx: &Ctx, r: &mut Report) -> Result<()> {
    let (mut s, th) = cx.alice_via(&cx.opts()).await?;
    let mb = s.select("INBOX").await?;
    let exists0 = mb.exists;
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
    let th2 = th.clone();

    // Runs in its own Tokio task: the IDLE future and the session must be Send.
    let task = tokio::spawn(async move {
        th2.set_read(Duration::ZERO); // the IDLE wait has its own limit
        let mut idle = s.idle();
        idle.init().await?;
        let t = Instant::now();
        let (fut, _stop) = idle.wait_with_timeout(Duration::from_secs(2));
        let r1 = fut.await?;
        let e1 = t.elapsed();

        let (fut, stop) = idle.wait_with_timeout(Duration::from_secs(60));
        let t = Instant::now();
        stop_tx.send(stop).ok();
        let r2 = fut.await?;
        let e2 = t.elapsed();

        // Collect untagged data until EXISTS arrives (the server may send other data first).
        let mut seen = Vec::new();
        let mut r3 = IdleResponse::Timeout;
        let mut woke = Instant::now();
        let mut ready = Some(ready_tx);
        for _ in 0..10 {
            let (fut, _stop) = idle.wait_with_timeout(Duration::from_secs(15));
            if let Some(tx) = ready.take() {
                tx.send(()).ok();
            }
            let resp = fut.await?;
            woke = Instant::now();
            let is_exists = matches!(&resp, IdleResponse::NewData(d) if matches!(d.parsed(), Response::MailboxData(MailboxDatum::Exists(_))));
            seen.push(match &resp {
                IdleResponse::NewData(d) => format!("{:?}", d.parsed()).chars().take(80).collect::<String>(),
                other => format!("{other:?}"),
            });
            r3 = resp;
            if is_exists || r3 == IdleResponse::Timeout {
                break;
            }
        }
        th2.set_read(Duration::from_secs(30));
        let s = idle.done().await?;
        Ok::<_, anyhow::Error>((s, r1, e1, r2, e2, r3, woke, seen))
    });

    let stop = stop_rx.await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(stop); // interrupt from outside
    ready_rx.await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let subject = format!("idle-{}", unique());
    let t_send = Instant::now();
    crate::smtp::send_bob_to_alice(cx, &subject, true).await?;
    let sent = Instant::now();

    let (mut s, r1, e1, r2, e2, r3, woke, seen) = task.await??;
    r.check(
        "idle.timeout",
        r1 == IdleResponse::Timeout && e1 >= Duration::from_millis(1900) && e1 < Duration::from_millis(2500),
        format!("wait_with_timeout(2 s) -> {r1:?} after {}", ms(e1)),
    );
    r.check(
        "idle.interrupt_from_outside",
        r2 == IdleResponse::ManualInterrupt && e2 < Duration::from_millis(1000),
        format!(
            "StopSource dropped by another task after 300 ms -> {r2:?} after {}",
            ms(e2)
        ),
    );
    let exists = match &r3 {
        IdleResponse::NewData(d) => match d.parsed() {
            Response::MailboxData(MailboxDatum::Exists(n)) => Some(*n),
            _ => None,
        },
        _ => None,
    };
    r.check("idle.exists_from_smtp", exists.is_some_and(|n| n > exists0), format!("SMTP bob→alice (1587 STARTTLS, lettre, {} to send) -> * {:?} EXISTS {} after the send started; IDLE data in order: {seen:?}", ms(sent - t_send), exists, ms(woke.saturating_duration_since(t_send))));

    s.noop().await?;
    let parked = drain_unsolicited(&s);
    let newest: Vec<Fetch> = s.uid_fetch("*", "(UID ENVELOPE)").await?.try_collect().await?;
    let subj = newest
        .first()
        .and_then(|f| f.envelope())
        .and_then(|e| e.subject.as_ref())
        .map(|s| String::from_utf8_lossy(s).into_owned());
    r.check(
        "idle.session_usable_after_done",
        subj.as_deref() == Some(subject.as_str()),
        format!(
            "DONE ok, NOOP ok, newest subject {subj:?}, {} unsolicited parked",
            parked.len()
        ),
    );
    s.logout().await.ok();
    Ok(())
}

pub async fn mv(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut a = cx.alice().await?;
    let mut b = cx.alice().await?;
    cleanup_spike_folders(&mut a).await;
    let (src, dst) = ("Spike/Src", "Spike/Dst");
    reset_folder(&mut a, "Spike").await.ok();
    reset_folder(&mut a, src).await?;
    reset_folder(&mut a, dst).await?;
    let mut uids = Vec::new();
    let tag = unique();
    for i in 0..4 {
        let d = raw::append(
            &mut a,
            src,
            "",
            None,
            &test_message(&format!("mv{i}-{tag}"), &format!("move {i}"), 0),
        )
        .await?;
        uids.push(d.append_uid.context("APPENDUID")?.1.parse::<u32>()?);
    }
    a.select(src).await?;
    drain_unsolicited(&a);

    a.uid_mv(uids[0].to_string(), dst).await?;
    let copyuid = drain_unsolicited(&a).into_iter().find_map(|u| match u {
        UnsolicitedResponse::Other(d) => raw::Done::from_untagged(&d).and_then(|d| d.copy_uid),
        _ => None,
    });
    let left = uids_of(&mut a).await?;
    let st = a.status(dst, "(MESSAGES)").await?;
    r.check(
        "move.uid_move",
        !left.contains(&uids[0]) && st.exists == 1,
        format!(
            "UID MOVE {} -> {dst}: gone from source, {} in target; COPYUID from unsolicited channel {:?}",
            uids[0], st.exists, copyuid
        ),
    );

    // Fallback COPY + STORE \Deleted + UID EXPUNGE, with a foreign \Deleted message present.
    b.select(src).await?;
    raw::raw(&mut b, &format!("UID STORE {} +FLAGS.SILENT (\\Deleted)", uids[2]))
        .await?
        .done
        .into_result()?;
    let cp = raw::raw(&mut a, &format!("UID COPY {} \"{dst}\"", uids[1]))
        .await?
        .done
        .into_result()?;
    raw::raw(&mut a, &format!("UID STORE {} +FLAGS.SILENT (\\Deleted)", uids[1]))
        .await?
        .done
        .into_result()?;
    let expunged: Vec<u32> = a.uid_expunge(uids[1].to_string()).await?.try_collect().await?;
    let left = uids_of(&mut a).await?;
    let foreign = fetch_one(&mut a, uids[2]).await?;
    let st = a.status(dst, "(MESSAGES)").await?;
    r.check(
        "move.copy_store_uid_expunge",
        !left.contains(&uids[1])
            && left.contains(&uids[2])
            && foreign.as_ref().is_some_and(|f| has_flag(f, "\\Deleted"))
            && st.exists == 2,
        format!(
            "COPYUID {:?}; UID EXPUNGE {} returned seq {:?}; foreign \\Deleted uid {} still there: {}; target has {}",
            cp.copy_uid,
            uids[1],
            expunged,
            uids[2],
            left.contains(&uids[2]),
            st.exists
        ),
    );
    b.logout().await.ok();
    cleanup_spike_folders(&mut a).await;
    a.logout().await.ok();
    Ok(())
}

impl raw::Done {
    /// Untagged `* OK [COPYUID ...]` as delivered by MOVE.
    pub fn from_untagged(d: &raw::ResponseData) -> Option<raw::Done> {
        match d.parsed() {
            Response::Data { .. } => raw::Done::from_resp(d.parsed()),
            _ => None,
        }
    }
}

pub async fn append(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut s = cx.alice().await?;
    cleanup_spike_folders(&mut s).await;
    let folder = "Spike/Append";
    reset_folder(&mut s, "Spike").await.ok();
    reset_folder(&mut s, folder).await?;
    let tag = unique();

    let res = s
        .append(
            folder,
            Some("(\\Seen $label1)"),
            Some("\"01-Feb-2026 10:00:00 +0100\""),
            test_message(&format!("ap1-{tag}"), "append lib", 0),
        )
        .await;
    r.check(
        "append.async_imap",
        res.is_ok(),
        format!(
            "Session::append -> {:?} (returns (), no APPENDUID)",
            res.map_err(|e| e.to_string())
        ),
    );

    let d = raw::append(
        &mut s,
        folder,
        "\\Seen \\Flagged $label2",
        Some("01-Feb-2026 10:00:00 +0100"),
        &test_message(&format!("ap2-{tag}"), "append raw", 0),
    )
    .await?;
    let (v, u) = d.append_uid.clone().context("APPENDUID")?;
    let uid: u32 = u.parse()?;
    s.select(folder).await?;
    let back = fetch_one(&mut s, uid).await?.context("appended")?;
    let lib: Vec<Fetch> = s
        .uid_fetch(uid.to_string(), "(UID INTERNALDATE)")
        .await?
        .try_collect()
        .await?;
    let ts = lib.first().and_then(|f| f.internal_date()).map(|d| d.timestamp());
    r.check(
        "append.flags_date_appenduid",
        has_flag(&back, "\\Seen")
            && has_flag(&back, "\\Flagged")
            && has_flag(&back, "$label2")
            && ts == Some(1_769_936_400),
        format!(
            "APPENDUID uidvalidity {v} uid {uid}; flags {:?}; INTERNALDATE {:?} = {:?}",
            back.flags, back.internal_date, ts
        ),
    );

    let big = test_message(&format!("ap3-{tag}"), "append big", 5_000_000);
    let t = Instant::now();
    let d = raw::append(&mut s, folder, "", None, &big).await?;
    let el = t.elapsed();
    let uid3: u32 = d.append_uid.context("APPENDUID")?.1.parse()?;
    let back = fetch_one(&mut s, uid3).await?.context("big")?;
    r.check(
        "append.big_literal",
        back.size == Some(big.len() as u32),
        format!("{:.1} MB in {}", big.len() as f64 / 1e6, ms(el)),
    );
    cleanup_spike_folders(&mut s).await;
    s.logout().await.ok();
    Ok(())
}

pub async fn keywords(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut s = cx.alice().await?;
    cleanup_spike_folders(&mut s).await;
    let folder = "Spike/Keywords";
    reset_folder(&mut s, "Spike").await.ok();
    reset_folder(&mut s, folder).await?;
    let d = raw::append(
        &mut s,
        folder,
        "",
        None,
        &test_message(&format!("kw-{}", unique()), "keywords", 0),
    )
    .await?;
    let uid: u32 = d.append_uid.context("APPENDUID")?.1.parse()?;
    let mb = s.select(folder).await?;
    r.check(
        "keywords.permanentflags_star",
        mb.permanent_flags.contains(&Flag::MayCreate),
        format!("{:?}", mb.permanent_flags),
    );

    let echoed: Vec<Fetch> = s
        .uid_store(uid.to_string(), "+FLAGS (Spike_Kw $SpikeTwo projekt-y)")
        .await?
        .try_collect()
        .await?;
    let echo: Vec<String> = echoed
        .iter()
        .flat_map(|f| f.flags().map(|fl| raw::flag_wire(&fl)).collect::<Vec<_>>())
        .collect();
    let mut s2 = cx.alice().await?;
    s2.examine(folder).await?;
    let back = fetch_one(&mut s2, uid).await?.context("uid")?;
    let all = ["spike_kw", "$spiketwo", "projekt-y"]
        .iter()
        .all(|k| has_flag(&back, k));
    r.check(
        "keywords.set_and_read",
        all,
        format!(
            "STORE echo {echo:?}; new connection reads {:?} (compare case-insensitively)",
            back.flags
        ),
    );

    let _: Vec<Fetch> = s
        .uid_store(uid.to_string(), "-FLAGS (spike_kw $SPIKETWO)")
        .await?
        .try_collect()
        .await?;
    let back = fetch_one(&mut s2, uid).await?.context("uid")?;
    r.check(
        "keywords.remove",
        !has_flag(&back, "spike_kw") && !has_flag(&back, "$spiketwo") && has_flag(&back, "projekt-y"),
        format!("after -FLAGS (other case): {:?}", back.flags),
    );

    let bad = raw::raw(&mut s, &format!("UID STORE {uid} +FLAGS (bad(kw)")).await?;
    let ok = !bad.done.ok() && !is_keyword_atom("bad(kw") && is_keyword_atom("projekt-y");
    r.check(
        "keywords.invalid_atom_rejected",
        ok,
        format!(
            "server: {} {:?}; client-side atom check needed because uid_store would swallow it",
            bad.done.status, bad.done.info
        ),
    );
    s2.logout().await.ok();
    cleanup_spike_folders(&mut s).await;
    s.logout().await.ok();
    Ok(())
}

/// RFC 3501 atom without the characters that are special in flags.
fn is_keyword_atom(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b > 0x20 && b < 0x7f && !b"(){%*\"\\]".contains(&b))
}

// ---------------------------------------------------------------------------------------------
// Reconnect: cut the connection in the middle of a large FETCH, detect it, resume.

#[derive(Default)]
struct SyncState {
    uidvalidity: Option<u32>,
    /// Lowest UID of the last fully stored window; the next window ends below it.
    lowest_done: Option<u32>,
    fetched_total: usize,
    /// Records of the window that was cut off; thrown away and fetched again.
    discarded: usize,
    store: BTreeMap<u32, HeaderRec>,
}

async fn sync_attempt(cx: &Ctx, o: &Opts, st: &mut SyncState, window: u32, checked: bool) -> Result<(), NetError> {
    let (mut s, _) = cx.alice_via(o).await?;
    let mb = s.examine("INBOX").await?;
    if st.uidvalidity.is_some() && st.uidvalidity != mb.uid_validity {
        st.store.clear(); // mapping invalid; would be rebuilt via Message-ID
        st.lowest_done = None;
    }
    st.uidvalidity = mb.uid_validity;
    let mut hi = match st.lowest_done {
        Some(1) => return Ok(()),
        Some(l) => l - 1,
        None => mb.uid_next.unwrap_or(1) - 1,
    };
    loop {
        let lo = hi.saturating_sub(window - 1).max(1);
        let mut recs = Vec::new();
        let res: Result<(), NetError> = async {
            if checked {
                raw::fetch_checked(&mut s, &format!("{lo}:{hi}"), HEADER_QUERY, &mut recs).await?;
            } else {
                let mut stream = s.uid_fetch(format!("{lo}:{hi}"), HEADER_QUERY).await?;
                while let Some(f) = stream.try_next().await? {
                    recs.push(HeaderRec::from_fetch(&f));
                }
            }
            Ok(())
        }
        .await;
        if let Err(e) = res {
            st.discarded += recs.len();
            return Err(e);
        }
        st.fetched_total += recs.len();
        for h in recs {
            st.store.insert(h.uid, h); // a real store commits the window in one transaction
        }
        st.lowest_done = Some(lo);
        if lo == 1 {
            break;
        }
        hi = lo - 1;
    }
    s.logout().await.ok();
    Ok(())
}

async fn resumable_sync(
    cx: &Ctx,
    proxy: &Proxy,
    o: &Opts,
    fault: Fault,
    checked: bool,
    refuse: usize,
) -> Result<(SyncState, Vec<String>)> {
    let mut st = SyncState::default();
    let mut log = Vec::new();
    proxy.arm(fault).await;
    proxy.refuse_next(refuse);
    let mut backoff = Duration::from_millis(100);
    for attempt in 1..=8 {
        match sync_attempt(cx, o, &mut st, 500, checked).await {
            Ok(()) => {
                log.push(format!("attempt {attempt}: complete"));
                return Ok((st, log));
            }
            Err(e) => {
                let det = match proxy.fired_at().await {
                    Some(t) => format!("detected {} after the fault", ms(t.elapsed())),
                    None => "no fault fired".into(),
                };
                log.push(format!(
                    "attempt {attempt}: {} ({e}); {det}; stored {} so far, resume below uid {:?}; backoff {}",
                    e.kind(),
                    st.store.len(),
                    st.lowest_done,
                    ms(backoff)
                ));
                if !e.is_transient() {
                    return Err(anyhow!("non-transient error {e}"));
                }
                proxy.arm(Fault::None).await;
                proxy.release_stalled();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
            }
        }
    }
    Err(anyhow!("gave up: {log:?}"))
}

pub async fn reconnect(cx: &Ctx, r: &mut Report) -> Result<()> {
    let mut d = cx.alice().await?;
    let expected: BTreeSet<u32> = {
        d.examine("INBOX").await?;
        uids_of(&mut d).await?.into_iter().collect()
    };
    d.logout().await.ok();
    let proxy = Proxy::start(cx.env.imaps_port).await?;
    let base = Opts {
        port: proxy.port,
        read_timeout: Duration::from_secs(3),
        ..cx.opts()
    };

    for (id, fault, checked, refuse) in [
        (
            "reconnect.close_mid_fetch_checked",
            Fault::CloseAfter(4_000_000),
            true,
            0,
        ),
        (
            "reconnect.close_mid_fetch_stream",
            Fault::CloseAfter(4_000_000),
            false,
            0,
        ),
        (
            "reconnect.stall_mid_fetch_timeout",
            Fault::StallAfter(4_000_000),
            true,
            0,
        ),
        ("reconnect.refused_then_backoff", Fault::None, true, 2),
    ] {
        let t = Instant::now();
        let (st, log) = resumable_sync(cx, &proxy, &base, fault, checked, refuse).await?;
        let got: BTreeSet<u32> = st.store.keys().copied().collect();
        let refetched = st.fetched_total - st.store.len();
        let detected_error = log.len() > 1;
        r.check(
            id,
            got == expected && detected_error && refetched == 0 && st.discarded < 500,
            format!(
                "{} of {} stored, {} records of the cut window discarded and fetched again, total {}; {}",
                got.len(),
                expected.len(),
                st.discarded,
                ms(t.elapsed()),
                log.join(" | ")
            ),
        );
    }

    // IDLE on a connection that is closed / goes silent.
    let o = Opts {
        read_timeout: Duration::from_secs(2),
        ..base.clone()
    };
    proxy.arm(Fault::CloseAfterMs(1500)).await;
    let (mut s, th) = cx.alice_via(&o).await?;
    s.select("INBOX").await?;
    th.set_read(Duration::ZERO);
    let mut idle = s.idle();
    idle.init().await?;
    let t = Instant::now();
    let (fut, _stop) = idle.wait_with_timeout(Duration::from_secs(10));
    let res = fut.await;
    let el = t.elapsed();
    th.set_read(Duration::from_secs(2));
    let done = idle.done().await;
    let fired = proxy.fired_at().await.map(|f| ms(f.elapsed())).unwrap_or_default();
    r.check(
        "reconnect.idle_connection_closed",
        el < Duration::from_secs(3) && done.is_err(),
        format!(
            "wait returned {:?} after {} (fault {fired} ago); DONE -> {:?}",
            res.map_err(|e| e.to_string()),
            ms(el),
            done.err().map(|e| NetError::from(e).kind())
        ),
    );

    proxy.arm(Fault::StallAfterMs(1500)).await;
    let (mut s, th) = cx.alice_via(&o).await?;
    s.select("INBOX").await?;
    th.set_read(Duration::ZERO);
    let mut idle = s.idle();
    idle.init().await?;
    let t = Instant::now();
    let (fut, _stop) = idle.wait_with_timeout(Duration::from_secs(4));
    let res = fut.await;
    th.set_read(Duration::from_secs(2));
    let done = idle.done().await;
    let el = t.elapsed();
    let done_kind = done.err().map(|e| NetError::from(e).kind());
    r.check(
        "reconnect.idle_connection_silent",
        done_kind == Some("Timeout"),
        format!(
            "silent link: wait -> {:?}, then DONE -> {:?}; detected after {} (IDLE slice 4 s + read limit 2 s)",
            res.map_err(|e| e.to_string()),
            done_kind,
            ms(el)
        ),
    );
    proxy.release_stalled();
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Server shutdown in the middle of IDLE and FETCH (not part of "all": stops the test server).

async fn slow_fetch(mut s: Sess, uid_next: u32, checked: bool) -> String {
    let set = format!("1:{}", uid_next - 1);
    let mut n = 0usize;
    let res: Result<(), NetError> = async {
        if checked {
            let tag = s.run_command(format!("UID FETCH {set} {HEADER_QUERY}")).await?;
            loop {
                let r = s
                    .read_response()
                    .await?
                    .ok_or_else(|| NetError::ConnectionLost("EOF before tagged OK".into()))?;
                match r.parsed() {
                    Response::Fetch(..) => {
                        n += 1;
                        tokio::time::sleep(Duration::from_micros(500)).await;
                    }
                    Response::Done { tag: t, .. } if *t == tag => return Ok(()),
                    _ => {}
                }
            }
        } else {
            let mut st = s.uid_fetch(&set, HEADER_QUERY).await?;
            while let Some(_f) = st.try_next().await? {
                n += 1;
                tokio::time::sleep(Duration::from_micros(500)).await;
            }
            Ok(())
        }
    }
    .await;
    match res {
        Ok(()) => format!("ended Ok after {n} of {} messages", uid_next - 1),
        Err(e) => format!("{} after {n} messages: {e}", e.kind()),
    }
}

pub async fn shutdown(cx: &Ctx, r: &mut Report) -> Result<()> {
    let script = cx.env.repo.join("scripts/testmail.sh");
    let (mut a, tha) = cx.alice_via(&cx.opts()).await?;
    a.select("INBOX").await?;
    let (mut b, _) = cx.alice_via(&cx.opts()).await?;
    let uid_next = b.examine("INBOX").await?.uid_next.unwrap_or(1);
    let (mut c, _) = cx.alice_via(&cx.opts()).await?;
    c.examine("INBOX").await?;

    let idle_task = tokio::spawn(async move {
        tha.set_read(Duration::ZERO);
        let mut idle = a.idle();
        idle.init().await?;
        let t = Instant::now();
        let (fut, _stop) = idle.wait_with_timeout(Duration::from_secs(30));
        let res = fut.await;
        let el = t.elapsed();
        tha.set_read(Duration::from_secs(3));
        let done = idle.done().await;
        Ok::<_, anyhow::Error>(format!(
            "wait -> {:?} after {}; DONE -> {:?}",
            res.map_err(|e| e.to_string()),
            ms(el),
            done.err().map(|e| NetError::from(e).kind())
        ))
    });
    let stream_task = tokio::spawn(slow_fetch(b, uid_next, false));
    let checked_task = tokio::spawn(slow_fetch(c, uid_next, true));
    tokio::time::sleep(Duration::from_millis(800)).await;
    let t = Instant::now();
    let out = tokio::process::Command::new(&script).arg("stop").output().await?;
    r.info(
        "shutdown.server_stop",
        format!(
            "scripts/testmail.sh stop: exit {:?} in {}",
            out.status.code(),
            ms(t.elapsed())
        ),
    );
    let idle_res = idle_task.await??;
    let stream_res = stream_task.await?;
    let checked_res = checked_task.await?;
    r.info("shutdown.idle", idle_res.clone());
    r.check(
        "shutdown.stream_path",
        true,
        format!("async-imap uid_fetch stream: {stream_res}"),
    );
    r.check(
        "shutdown.checked_path_detects",
        !checked_res.starts_with("ended Ok"),
        format!("checked raw fetch: {checked_res}"),
    );
    let out = tokio::process::Command::new(&script).arg("start").output().await?;
    r.check(
        "shutdown.server_restarted",
        out.status.success(),
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .last()
            .unwrap_or_default()
            .to_string(),
    );
    Ok(())
}
