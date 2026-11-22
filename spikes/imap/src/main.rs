//! Spike A (stage 0): async-imap 0.12 + Tokio + rustls against the local Stalwart test server.
//! Throwaway code. Usage: `cargo run --release -- <scenario>`; see README.md.

mod bench;
mod env;
mod errors;
mod mutf7;
mod net;
mod proxy;
mod raw;
mod report;
mod scenarios;
mod smtp;
mod timeout_io;
mod tls;

use std::process::ExitCode;

use scenarios::Ctx;

const ALL: &[&str] = &[
    "tls",
    "starttls",
    "caps",
    "folders",
    "select",
    "fetch",
    "condstore",
    "idle",
    "move",
    "append",
    "keywords",
    "reconnect",
    "smtp",
];

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let which = args.first().cloned().unwrap_or_else(|| "all".into());
    let cx = match Ctx::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("setup failed: {e:#} (is the test server running? scripts/testmail.sh start)");
            return ExitCode::from(2);
        }
    };
    let mut rep = report::Report::new();
    if which == "bench" {
        if let Err(e) = bench::run(&cx, &args[1..]).await {
            eprintln!("bench failed: {e:#}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    let list: Vec<&str> = if which == "all" {
        ALL.to_vec()
    } else {
        vec![which.as_str()]
    };
    for name in list {
        println!("\n== {name}");
        let res = match name {
            "tls" => scenarios::tls(&cx, &mut rep).await,
            "starttls" => scenarios::starttls(&cx, &mut rep).await,
            "caps" => scenarios::caps(&cx, &mut rep).await,
            "folders" => scenarios::folders(&cx, &mut rep).await,
            "select" => scenarios::select(&cx, &mut rep).await,
            "fetch" => scenarios::fetch(&cx, &mut rep).await,
            "condstore" => scenarios::condstore(&cx, &mut rep).await,
            "idle" => scenarios::idle(&cx, &mut rep).await,
            "move" => scenarios::mv(&cx, &mut rep).await,
            "append" => scenarios::append(&cx, &mut rep).await,
            "keywords" => scenarios::keywords(&cx, &mut rep).await,
            "reconnect" => scenarios::reconnect(&cx, &mut rep).await,
            "smtp" => smtp::run(&cx, &mut rep).await,
            "shutdown" => scenarios::shutdown(&cx, &mut rep).await,
            other => {
                eprintln!("unknown scenario {other}; known: all bench shutdown {}", ALL.join(" "));
                return ExitCode::from(2);
            }
        };
        if let Err(e) = res {
            rep.check(&format!("{name}.aborted"), false, format!("{e:#}"));
        }
    }
    rep.summary();
    if rep.failed() == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
