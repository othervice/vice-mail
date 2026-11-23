//! Structural guarantees (stage 13: "no code path turns model output into an action or a
//! network connection"):
//!
//! 1. The crate depends on no crate that could act: not on the engine, the store, the IMAP or
//!    SMTP adapter, the MCP server, the shell, or a crate that opens files, links or programs.
//! 2. Only the transport module builds HTTP requests; nothing in the crate touches the file
//!    system, starts processes or opens sockets of its own.
//! 3. Requests carry no tool or function definitions.

use std::path::Path;

const MANIFEST: &str = include_str!("../Cargo.toml");

/// Dependency names of one `[...dependencies]` table.
fn table(name: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut inside = false;
    for line in MANIFEST.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == format!("[{name}]");
            continue;
        }
        if inside
            && !line.starts_with('#')
            && let Some((key, _)) = line.split_once('=')
        {
            deps.push(key.trim().trim_end_matches(".workspace").to_owned());
        }
    }
    deps
}

#[test]
fn depends_only_on_model_content_and_plain_libraries() {
    let allowed = [
        "mail-model",
        "mail-content",
        "serde",
        "serde_json",
        "thiserror",
        "tokio",
        "reqwest",
        "rustls",
        "url",
        "getrandom",
    ];
    let deps = table("dependencies");
    assert!(
        deps.contains(&"mail-model".to_owned()),
        "parser broken? {deps:?}"
    );
    for dep in &deps {
        assert!(
            allowed.contains(&dep.as_str()),
            "unexpected dependency {dep}"
        );
    }
    for forbidden in [
        "mail-engine",
        "mail-store",
        "mail-imap",
        "mail-mcp",
        "mail-app",
        "tauri",
        "open",
        "rfd",
        "lettre",
        "async-imap",
        "rusqlite",
        "keyring-core",
    ] {
        assert!(
            !MANIFEST.lines().any(
                |line| line.trim_start().starts_with(&format!("{forbidden} "))
                    || line.trim_start().starts_with(&format!("{forbidden}."))
                    || line.trim_start().starts_with(&format!("{forbidden}="))
            ),
            "{forbidden} in the manifest"
        );
    }
    assert!(
        !MANIFEST.contains("[target."),
        "no platform-specific dependencies"
    );
}

/// The source without comment lines (docs may name other crates).
fn code_only(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every source file of the crate, without comments.
fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut stack = vec![dir];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let source = std::fs::read_to_string(&path).expect("source");
                files.push((name, code_only(&source)));
            }
        }
    }
    assert!(files.len() >= 10, "found only {} source files", files.len());
    files
}

#[test]
fn no_file_system_processes_or_own_sockets() {
    for (name, source) in sources() {
        for forbidden in [
            "std::fs",
            "std::process",
            "Command::new",
            "tokio::fs",
            "tokio::process",
            "TcpStream",
            "TcpListener",
            "UdpSocket",
            "std::net::Tcp",
            "open::",
            "mail_engine",
            "mail_store",
            "mail_imap",
            "unsafe ",
        ] {
            assert!(!source.contains(forbidden), "{name} uses {forbidden}");
        }
    }
}

#[test]
fn only_the_transport_builds_requests() {
    for (name, source) in sources() {
        let builds = ["reqwest::Client", ".post(", ".get(url", "lookup_host"]
            .iter()
            .any(|pattern| source.contains(pattern));
        if name != "http.rs" {
            assert!(!builds, "{name} builds HTTP requests itself");
        } else {
            assert!(builds, "the transport check does not find the transport");
        }
    }
}

#[test]
fn requests_have_no_tools() {
    for (name, source) in sources() {
        if name == "structure.rs" {
            continue;
        }
        // Request bodies are built with `json!`; none may name tools or functions.
        for field in [
            "\"tools\":",
            "\"functions\":",
            "\"tool_choice\":",
            "\"function_call\":",
        ] {
            assert!(!source.contains(field), "{name} sends {field}");
        }
    }
}
