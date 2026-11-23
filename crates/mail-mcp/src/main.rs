//! `mail-mcp`: the helper an AI client starts as a local MCP server (stdio). See
//! `mail_mcp::helper` and decision 0015.

use std::process::ExitCode;

use mail_mcp::helper::{ENV_ENDPOINT, ENV_TOKEN, HelperConfig, run_stdio};

const USAGE: &str = "mail-mcp: local MCP server of the mail app (stdio).

An AI client starts this program and talks MCP with it on stdin and stdout. It needs two
environment variables, both shown in the app's settings under AI access:

  MAIL_MCP_ENDPOINT  where the app listens (socket path or pipe name)
  MAIL_MCP_TOKEN     the token of this client (mcp1_...)

Options: --help, --version. Log messages go to stderr (more with MAIL_MCP_LOG=debug).";

fn main() -> ExitCode {
    // Finding C-2 (decision 0043): the app renders attachment previews in this program, so a
    // decoder that runs out of memory or time ends this process, not the app.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(mail_mcp::preview::PREVIEW_ARG) {
        return mail_mcp::preview::run_child(&args[2..]);
    }
    if let Some(arg) = std::env::args().nth(1) {
        return match arg.as_str() {
            "--version" | "-V" => {
                println!("mail-mcp {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                ExitCode::SUCCESS
            }
            _ => {
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        };
    }
    // stdout carries MCP only; logs go to stderr, which clients collect.
    let level = std::env::var("MAIL_MCP_LOG")
        .ok()
        .and_then(|level| level.parse::<tracing::Level>().ok())
        .unwrap_or(tracing::Level::WARN);
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_max_level(level)
        .init();

    let config = HelperConfig::from_env();
    if let Err(problem) = &config {
        tracing::warn!(
            "{problem}; set {ENV_ENDPOINT} and {ENV_TOKEN} as shown in the mail app's settings"
        );
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("mail-mcp: cannot start: {err}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run_stdio(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "stdio failed");
            ExitCode::FAILURE
        }
    }
}
