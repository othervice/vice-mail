//! Build script: Tauri's code generation plus two facts for `AppInfo`, the short git commit
//! (`MAIL_APP_COMMIT`, with `-dirty` when tracked files differ from it) and the build time
//! (`MAIL_APP_BUILD_DATE`, RFC 3339, UTC). Both stay unset when unknown; the app then reports
//! `null`.
//!
//! Cargo reruns this script only when HEAD moves, `SOURCE_DATE_EPOCH` changes or Tauri's config
//! changes, not on every edit. `scripts/build-macos.sh` and `scripts/build-windows.sh` therefore
//! set `SOURCE_DATE_EPOCH` for every build: the commit time for a clean tree, the current time for
//! a dirty one. That keeps both facts fresh in shipped builds; a plain `cargo build` may keep an
//! older value until one of the triggers fires.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    emit_git_commit();
    emit_build_date();
    build_tauri();
}

/// Tauri's code generation. On a Windows host the app manifest (`windows-app-manifest.xml`, the
/// Common Controls v6 dependency, the same file tauri-build ships) goes into every binary of this
/// crate through the linker: tauri-build embeds it in the app binary only, and the unit-test
/// binary then does not start (`STATUS_ENTRYPOINT_NOT_FOUND`, found by the first CI run on
/// Windows). This is the way tauri-build documents for `new_without_app_manifest`. Cross builds
/// from the Mac (cargo-xwin, lld-link) keep tauri-build's own embedding; their test binaries are
/// never run.
fn build_tauri() {
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows && cfg!(windows) {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        if let Err(err) =
            tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        {
            panic!("tauri-build failed: {err:#}");
        }
    } else {
        tauri_build::build();
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Whether tracked files differ from HEAD, staged or not (as `git describe --dirty` sees it;
/// untracked files do not count). `--no-optional-locks`: never rewrite the index, another git
/// process may hold it. If git cannot answer, the build counts as dirty: clean must be proven.
fn working_tree_dirty() -> bool {
    let out = Command::new("git")
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ])
        .output();
    match out {
        Ok(out) if out.status.success() => !out.stdout.is_empty(),
        Ok(out) => {
            println!(
                "cargo:warning=git status failed ({}); marking the commit as dirty",
                out.status
            );
            true
        }
        Err(err) => {
            println!("cargo:warning=git status failed ({err}); marking the commit as dirty");
            true
        }
    }
}

fn emit_git_commit() {
    if let Some(commit) = git(&["rev-parse", "--short=10", "HEAD"]) {
        let suffix = if working_tree_dirty() { "-dirty" } else { "" };
        println!("cargo:rustc-env=MAIL_APP_COMMIT={commit}{suffix}");
    }
    // Re-run when HEAD moves. Only existing files: a missing one would re-run on every build.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let mut watch = vec![format!("{git_dir}/HEAD"), format!("{git_dir}/packed-refs")];
        if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
            watch.push(format!("{git_dir}/{head_ref}"));
        }
        for path in watch.iter().filter(|p| Path::new(p).exists()) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn emit_build_date() {
    // Reproducible builds pin the time through SOURCE_DATE_EPOCH.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_secs()).ok())
        });
    if let Some(secs) = secs {
        println!("cargo:rustc-env=MAIL_APP_BUILD_DATE={}", rfc3339_utc(secs));
    }
}

/// Formats Unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (days-to-civil algorithm by Howard Hinnant).
fn rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}
