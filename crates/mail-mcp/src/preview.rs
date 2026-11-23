//! Attachment previews in a child process (finding C-2 of the stage 15 review, decision 0043).
//!
//! The decoders behind a preview (hayro for PDF, `image` for pictures) have no limits of their
//! own on memory and time: a PDF of 2 KB can expand to a gigabyte, and running out of memory
//! ends a process without a chance to recover. So the app does not decode these files itself.
//! It starts this helper as `mail-mcp --render-preview <kind> <page>`, writes the file to its
//! stdin and reads one JSON reply from its stdout (the PNG or JPEG as a data URL, or why there is
//! no preview). The parent ([`render_isolated`]) watches the child from outside:
//!
//! - **time:** after [`PreviewLimits::time`] the child is killed;
//! - **memory:** every [`PreviewLimits::poll`] the parent reads the child's resident memory
//!   (`/bin/ps -o rss=` on macOS and Linux, `tasklist` on Windows) and kills it above
//!   [`PreviewLimits::memory_bytes`]; between two looks a fast-growing child can pass the
//!   budget by what it allocates in that time (tens of MB);
//! - a killed child, a crash or a reply that is not one gives a typed [`PreviewError`]; the app
//!   goes on.
//!
//! Nothing here uses `unsafe` or a new dependency. The Windows path (`tasklist`, `mail-mcp.exe`)
//! is written for Windows but has not run there.

use std::path::Path;
use std::process::{ExitCode, Stdio};
use std::time::{Duration, Instant};

use mail_model::{AttachmentPreview, MAX_PREVIEW_INPUT_BYTES, PreviewError, PreviewKind};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// First argument of the preview mode.
pub const PREVIEW_ARG: &str = "--render-preview";

/// Largest reply read from the child: a data URL of a page at most 2048 px per side.
const MAX_REPLY_BYTES: u64 = 96 * 1024 * 1024;

/// Limits of one preview child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewLimits {
    /// Resident memory above which the child is killed.
    pub memory_bytes: u64,
    /// Wall-clock time after which the child is killed.
    pub time: Duration,
    /// How often the parent looks at the child's memory and the clock.
    pub poll: Duration,
}

impl Default for PreviewLimits {
    /// 512 MiB and 20 s. An image preview at its pixel limit needs about 400 MiB; a PDF page
    /// renders in well under a second.
    fn default() -> Self {
        Self {
            memory_bytes: 512 * 1024 * 1024,
            time: Duration::from_secs(20),
            poll: Duration::from_millis(50),
        }
    }
}

/// The child's one line of output.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Reply {
    Ok { preview: AttachmentPreview },
    Unsupported { detail: String },
    TooLarge { detail: String },
    Damaged { detail: String },
}

fn kind_name(kind: PreviewKind) -> &'static str {
    match kind {
        PreviewKind::Image => "image",
        PreviewKind::Pdf => "pdf",
        PreviewKind::Text => "text",
    }
}

fn kind_from(name: &str) -> Option<PreviewKind> {
    match name {
        "image" => Some(PreviewKind::Image),
        "pdf" => Some(PreviewKind::Pdf),
        "text" => Some(PreviewKind::Text),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Child

/// The preview mode: `args` are the arguments after [`PREVIEW_ARG`] (`<kind> <page>`). Reads the
/// file from stdin, renders it in this process and writes one JSON line to stdout.
pub fn run_child(args: &[String]) -> ExitCode {
    let (Some(kind), Some(page)) = (
        args.first().and_then(|a| kind_from(a)),
        args.get(1).and_then(|p| p.parse::<u32>().ok()),
    ) else {
        eprintln!("mail-mcp: usage: {PREVIEW_ARG} <image|pdf|text> <page>");
        return ExitCode::from(2);
    };
    let mut bytes = Vec::new();
    let limit = u64::from(MAX_PREVIEW_INPUT_BYTES);
    if let Err(err) = std::io::Read::read_to_end(
        &mut std::io::Read::take(std::io::stdin().lock(), limit + 1),
        &mut bytes,
    ) {
        eprintln!("mail-mcp: reading the file failed: {err}");
        return ExitCode::FAILURE;
    }
    let reply = match mail_content::render_preview(kind, &bytes, page) {
        Ok(preview) => Reply::Ok { preview },
        Err(PreviewError::Unsupported(detail)) => Reply::Unsupported { detail },
        Err(PreviewError::TooLarge(detail)) => Reply::TooLarge { detail },
        Err(PreviewError::Damaged(detail)) => Reply::Damaged { detail },
    };
    let mut out = std::io::stdout().lock();
    match serde_json::to_writer(&mut out, &reply) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mail-mcp: writing the preview failed: {err}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Parent

/// Renders a preview in the helper at `helper` within `limits` (see the module docs).
pub async fn render_isolated(
    helper: &Path,
    kind: PreviewKind,
    bytes: Vec<u8>,
    page: u32,
    limits: PreviewLimits,
) -> Result<AttachmentPreview, PreviewError> {
    let mut child = tokio::process::Command::new(helper)
        .arg(PREVIEW_ARG)
        .arg(kind_name(kind))
        .arg(page.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| {
            PreviewError::Unsupported(format!("the preview program could not start: {err}"))
        })?;
    let pid = child.id();
    let started = Instant::now();
    // Feeding and reading run beside the watchdog; a child that stops reading or writing
    // cannot block the parent past the time limit.
    let writer = child.stdin.take().map(|mut stdin| {
        tokio::spawn(async move {
            // A child that ends early closes the pipe; the exit status tells why.
            let _ = stdin.write_all(&bytes).await;
        })
    });
    let reader = child.stdout.take().map(|stdout| {
        tokio::spawn(async move {
            let mut reply = Vec::new();
            let _ = stdout.take(MAX_REPLY_BYTES).read_to_end(&mut reply).await;
            reply
        })
    });
    let stop = |reason: String| PreviewError::TooLarge(reason);
    let outcome = loop {
        tokio::select! {
            status = child.wait() => break Ok(status),
            () = tokio::time::sleep(limits.poll) => {
                if started.elapsed() > limits.time {
                    break Err(stop(format!(
                        "the preview took longer than {} s and was stopped",
                        limits.time.as_secs_f32()
                    )));
                }
                if let Some(pid) = pid
                    && let Some(used) = resident_bytes(pid).await
                    && used > limits.memory_bytes
                {
                    break Err(stop(format!(
                        "the preview needed more than {} MiB and was stopped",
                        limits.memory_bytes / (1024 * 1024)
                    )));
                }
            }
        }
    };
    let status = match outcome {
        Ok(status) => status,
        Err(err) => {
            let _ = child.kill().await;
            if let Some(writer) = writer {
                writer.abort();
            }
            if let Some(reader) = reader {
                reader.abort();
            }
            tracing::info!(error = %err, "preview child stopped");
            return Err(err);
        }
    };
    if let Some(writer) = writer {
        let _ = writer.await;
    }
    let reply = match reader {
        Some(reader) => reader.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let damaged = || PreviewError::Damaged("the preview program ended unexpectedly".into());
    if !matches!(status, Ok(s) if s.success()) {
        tracing::info!(?status, "preview child failed");
        return Err(damaged());
    }
    match serde_json::from_slice::<Reply>(&reply) {
        Ok(Reply::Ok { preview }) => Ok(preview),
        Ok(Reply::Unsupported { detail }) => Err(PreviewError::Unsupported(detail)),
        Ok(Reply::TooLarge { detail }) => Err(PreviewError::TooLarge(detail)),
        Ok(Reply::Damaged { detail }) => Err(PreviewError::Damaged(detail)),
        Err(_) => Err(damaged()),
    }
}

/// Resident memory of process `pid` in bytes, `None` if it cannot be read (the process ended,
/// or the system tool failed; the next look tries again).
pub async fn resident_bytes(pid: u32) -> Option<u64> {
    let output = if cfg!(windows) {
        // Untested on Windows: "image","pid","session","#","12.345 K" (separators per locale).
        tokio::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?
    } else {
        tokio::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?
    };
    if !output.status.success() {
        return None;
    }
    parse_resident_kib(&String::from_utf8_lossy(&output.stdout), cfg!(windows))
        .map(|kib| kib.saturating_mul(1024))
}

/// KiB from the output of `ps -o rss=` (a number) or `tasklist /FO CSV /NH` (the last field,
/// digits with locale separators and a unit).
fn parse_resident_kib(text: &str, tasklist: bool) -> Option<u64> {
    let field = if tasklist {
        text.lines()
            .find(|line| line.starts_with('"'))?
            .rsplit("\",\"")
            .next()?
    } else {
        text.trim()
    };
    let digits: String = field.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_figures_of_ps_and_tasklist_are_read() {
        assert_eq!(parse_resident_kib("  12345\n", false), Some(12_345));
        assert_eq!(parse_resident_kib("", false), None);
        assert_eq!(
            parse_resident_kib(
                "\"mail-mcp.exe\",\"4242\",\"Console\",\"1\",\"12.345 K\"\r\n",
                true
            ),
            Some(12_345)
        );
        assert_eq!(
            parse_resident_kib(
                "\"mail-mcp.exe\",\"4242\",\"Console\",\"1\",\"1,234,567 K\"",
                true
            ),
            Some(1_234_567)
        );
        // No such process: an information line without quotes.
        assert_eq!(
            parse_resident_kib(
                "INFO: No tasks are running which match the specified criteria.",
                true
            ),
            None
        );
    }

    #[test]
    fn replies_round_trip() {
        let reply = Reply::TooLarge { detail: "x".into() };
        let text = serde_json::to_string(&reply).expect("json");
        assert_eq!(text, r#"{"outcome":"tooLarge","detail":"x"}"#);
        assert!(matches!(
            serde_json::from_str::<Reply>(&text),
            Ok(Reply::TooLarge { .. })
        ));
    }
}
