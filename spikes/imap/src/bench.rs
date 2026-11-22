//! Measurement: header data of the whole INBOX (~9959 messages after
//! `seed 10000 --filler-folder INBOX`) over TLS, window sizes, both fetch paths, flags only.

use std::time::{Duration, Instant};

use anyhow::Result;
use futures::TryStreamExt;

use crate::net::{Opts, Sess};
use crate::proxy::Proxy;
use crate::raw::{self, HeaderRec};
use crate::scenarios::{Ctx, HEADER_QUERY};

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output();
    out.ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

fn peak_rss_mb() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage writes into the provided struct.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    ru.ru_maxrss as f64 / 1024.0 / 1024.0 // bytes on macOS
}

fn loadavg() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

async fn fetch_all(s: &mut Sess, uid_next: u32, window: u32, checked: bool) -> Result<Vec<HeaderRec>> {
    let mut out = Vec::with_capacity(10_000);
    let mut hi = uid_next - 1;
    loop {
        let lo = hi.saturating_sub(window - 1).max(1);
        let set = format!("{lo}:{hi}");
        if checked {
            raw::fetch_checked(s, &set, HEADER_QUERY, &mut out).await?;
        } else {
            let mut st = s.uid_fetch(&set, HEADER_QUERY).await?;
            while let Some(f) = st.try_next().await? {
                out.push(HeaderRec::from_fetch(&f));
            }
        }
        if lo == 1 {
            return Ok(out);
        }
        hi = lo - 1;
    }
}

pub async fn run(cx: &Ctx, args: &[String]) -> Result<()> {
    let mut windows = vec![100u32, 500, 2000];
    let mut runs = 3;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--windows" => {
                windows = it
                    .next()
                    .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
                    .unwrap_or(windows)
            }
            "--runs" => runs = it.next().and_then(|v| v.parse().ok()).unwrap_or(runs),
            _ => {}
        }
    }
    println!("load average before: {}", loadavg());
    println!("RSS at start: {:.1} MB", rss_mb());

    let t = Instant::now();
    let (mut s, _) = cx.alice_via(&cx.opts()).await?;
    let mb = s.examine("INBOX").await?;
    println!(
        "connect + TLS + LOGIN + EXAMINE: {:.0} ms; INBOX exists {}",
        t.elapsed().as_secs_f64() * 1000.0,
        mb.exists
    );
    let uid_next = mb.uid_next.unwrap_or(1);

    println!("\npath      window  run  mails   seconds  mails/s  RSS MB");
    let mut summary = Vec::new();
    for &w in &windows {
        for checked in [false, true] {
            let mut times = Vec::new();
            for run in 1..=runs {
                let t = Instant::now();
                let recs = fetch_all(&mut s, uid_next, w, checked).await?;
                let el = t.elapsed();
                times.push(el);
                println!(
                    "{:<9} {w:>6} {run:>4} {:>6} {:>9.3} {:>8.0} {:>7.1}",
                    if checked { "checked" } else { "stream" },
                    recs.len(),
                    el.as_secs_f64(),
                    recs.len() as f64 / el.as_secs_f64(),
                    rss_mb()
                );
                drop(recs);
            }
            times.sort();
            summary.push((checked, w, times[0], times[times.len() / 2], times[times.len() - 1]));
        }
    }

    // Keep all records like a store would and look at memory.
    let before = rss_mb();
    let recs = fetch_all(&mut s, uid_next, 500, true).await?;
    let header_bytes: usize = recs
        .iter()
        .map(|r| r.header_fields.as_ref().map(|h| h.len()).unwrap_or(0))
        .sum();
    println!(
        "\n{} records held in memory: RSS {:.1} → {:.1} MB (header fields {:.1} MB)",
        recs.len(),
        before,
        rss_mb(),
        header_bytes as f64 / 1e6
    );
    drop(recs);

    println!("\nflags only (UID FLAGS) for the sync without CONDSTORE:");
    for (label, window) in [("1:* in one command", 0u32), ("windows of 2000", 2000)] {
        let mut times = Vec::new();
        let mut n = 0;
        for _ in 0..runs {
            let t = Instant::now();
            n = 0;
            if window == 0 {
                let mut st = s.uid_fetch("1:*", "(UID FLAGS)").await?;
                while let Some(f) = st.try_next().await? {
                    n += f.flags().count().min(1);
                }
            } else {
                let mut hi = uid_next - 1;
                loop {
                    let lo = hi.saturating_sub(window - 1).max(1);
                    let mut st = s.uid_fetch(format!("{lo}:{hi}"), "(UID FLAGS)").await?;
                    while let Some(f) = st.try_next().await? {
                        n += f.flags().count().min(1);
                    }
                    drop(st);
                    if lo == 1 {
                        break;
                    }
                    hi = lo - 1;
                }
            }
            times.push(t.elapsed());
        }
        times.sort();
        println!(
            "  {label:<20} {n} messages with flags: min {:.0} ms, median {:.0} ms, max {:.0} ms",
            ms(times[0]),
            ms(times[times.len() / 2]),
            ms(times[times.len() - 1])
        );
    }
    s.logout().await.ok();

    // Bytes on the wire for one header pass (through the counting proxy).
    let proxy = Proxy::start(cx.env.imaps_port).await?;
    let o = Opts {
        port: proxy.port,
        ..cx.opts()
    };
    let (mut ps, _) = cx.alice_via(&o).await?;
    ps.examine("INBOX").await?;
    let b0 = proxy.bytes_s2c.load(std::sync::atomic::Ordering::Relaxed);
    let recs = fetch_all(&mut ps, uid_next, 500, true).await?;
    let b1 = proxy.bytes_s2c.load(std::sync::atomic::Ordering::Relaxed);
    println!(
        "\nwire (TLS records, server→client) for {} headers: {:.1} MB ({:.0} bytes/message)",
        recs.len(),
        (b1 - b0) as f64 / 1e6,
        (b1 - b0) as f64 / recs.len() as f64
    );
    ps.logout().await.ok();

    println!("\nsummary (min / median / max seconds over {runs} runs):");
    for (checked, w, a, m, z) in summary {
        println!(
            "  {:<8} window {w:>5}: {:.3} / {:.3} / {:.3} s",
            if checked { "checked" } else { "stream" },
            a.as_secs_f64(),
            m.as_secs_f64(),
            z.as_secs_f64()
        );
    }
    println!("peak RSS of the process: {:.1} MB", peak_rss_mb());
    println!("load average after: {}", loadavg());
    Ok(())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
