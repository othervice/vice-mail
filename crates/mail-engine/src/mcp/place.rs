//! Where the MCP endpoint and the helper program are (decision 0021).
//!
//! **Endpoint, macOS.** `<data dir>/mcp/mail-mcp.sock` (decision 0015). A Unix socket path
//! holds at most 103 bytes; the data directory contains the user name, so a long user name (or
//! a long test directory) can make it too long. Then the socket goes into the user's own
//! temporary directory, `$TMPDIR/mail-mcp-<16 hex digits of SHA-256(data dir)>/mail-mcp.sock`.
//! On macOS `$TMPDIR` is `/var/folders/<..>/<..>/T/` (about 50 bytes, owned by the user, mode
//! 0700), the same for every process of the user and stable across restarts, so the client
//! configurations keep working. The cache directory would not help: it also contains the user
//! name and is only 13 bytes shorter. The hash keeps two data directories (tests, a second app
//! build) apart. `mail_mcp::Endpoint` still makes the socket's own directory 0700 and refuses a
//! directory owned by someone else.
//!
//! **Endpoint, Windows.** A random pipe name, made once and stored (`mcp_state.endpoint`): the
//! client configurations contain it.
//!
//! **Helper.** Next to the app's executable: `Contents/MacOS/mail-mcp` in the macOS bundle,
//! `mail-mcp.exe` next to the app on Windows (Tauri `bundle.externalBin`), `target/<profile>/`
//! in a development build (after `cargo build -p mail-mcp`).

use std::path::{Path, PathBuf};

use mail_mcp::{Endpoint, EndpointError};
use mail_store::Store;
use sha2::{Digest, Sha256};

/// File name of the helper program.
pub(crate) const HELPER_FILE: &str = if cfg!(windows) {
    "mail-mcp.exe"
} else {
    "mail-mcp"
};

/// The endpoint for this data directory, or why there is none.
pub(crate) async fn resolve(store: &Store, data_dir: &Path, now: i64) -> Result<Endpoint, String> {
    if cfg!(windows) {
        let stored = store
            .mcp_state()
            .await
            .map_err(|err| err.to_string())?
            .endpoint;
        if let Some(stored) = stored {
            match Endpoint::new(stored) {
                Ok(endpoint) => return Ok(endpoint),
                Err(err) => {
                    tracing::warn!(error = %err, "stored MCP pipe name is not valid; a new one is made")
                }
            }
        }
        let endpoint = Endpoint::for_data_dir(data_dir).map_err(|err| err.to_string())?;
        store
            .set_mcp_endpoint(endpoint.as_str().to_owned(), now)
            .await
            .map_err(|err| err.to_string())?;
        return Ok(endpoint);
    }
    socket_for(data_dir, &std::env::temp_dir())
}

/// macOS (and other Unix systems): the socket in the data directory, or in `temp_dir` when that
/// path is too long.
pub(crate) fn socket_for(data_dir: &Path, temp_dir: &Path) -> Result<Endpoint, String> {
    match Endpoint::for_data_dir(data_dir) {
        Ok(endpoint) => Ok(endpoint),
        Err(EndpointError::TooLong(long)) => {
            let digest = Sha256::digest(data_dir.as_os_str().as_encoded_bytes());
            let short: String = digest
                .iter()
                .take(8)
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let path = temp_dir
                .join(format!("mail-mcp-{short}"))
                .join("mail-mcp.sock");
            let fallback = Endpoint::new(path.to_string_lossy().into_owned());
            match fallback {
                Ok(endpoint) => {
                    tracing::info!(
                        "the MCP socket path in the data directory is longer than 103 bytes; \
                         using the temporary directory"
                    );
                    Ok(endpoint)
                }
                Err(err) => Err(format!("{long} is too long, and so is the fallback: {err}")),
            }
        }
        Err(err) => Err(err.to_string()),
    }
}

/// The helper next to the running executable.
pub(crate) fn default_helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name(HELPER_FILE))
}

/// The app runs from a place that will not stay: a disk image (`/Volumes/...`) or macOS App
/// Translocation (a random read-only copy when the app was opened from Downloads).
pub(crate) fn unstable(path: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let text = path.to_string_lossy();
    text.starts_with("/Volumes/") || text.contains("/AppTranslocation/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn long_data_directories_put_the_socket_into_the_temporary_directory() {
        let short = Path::new("/Users/me/Library/Application Support/org.example.app");
        let endpoint = socket_for(short, Path::new("/var/folders/ab/xyz/T")).expect("short");
        assert_eq!(
            endpoint.as_str(),
            "/Users/me/Library/Application Support/org.example.app/mcp/mail-mcp.sock"
        );

        let long = PathBuf::from(format!(
            "/Users/{}/Library/Application Support/org.example.app",
            "a".repeat(60)
        ));
        let temp = Path::new("/var/folders/ab/cdefghijklmnopqrstuvwxyz0123/T");
        let first = socket_for(&long, temp).expect("fallback");
        assert!(
            first
                .as_str()
                .starts_with(&format!("{}/mail-mcp-", temp.display()))
        );
        assert!(first.as_str().ends_with("/mail-mcp.sock"));
        assert!(first.as_str().len() <= 103, "{}", first.as_str());
        // Stable for the same directory, different for another one.
        assert_eq!(socket_for(&long, temp).expect("again"), first);
        let other = long.join("other");
        assert_ne!(socket_for(&other, temp).expect("other"), first);

        let hopeless = Path::new("/").join("t".repeat(120));
        assert!(socket_for(&long, &hopeless).is_err());
    }

    #[test]
    fn unstable_locations() {
        if cfg!(target_os = "macos") {
            assert!(unstable(Path::new(
                "/Volumes/Mail/Mail.app/Contents/MacOS/mail-mcp"
            )));
            assert!(unstable(Path::new(
                "/private/var/folders/x/AppTranslocation/1/d/Mail.app/Contents/MacOS/mail-mcp"
            )));
        }
        assert!(!unstable(Path::new(
            "/Applications/Mail.app/Contents/MacOS/mail-mcp"
        )));
    }
}
