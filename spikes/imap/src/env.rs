//! Test server settings from `testmail/accounts.env` (never real credentials).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Env {
    pub repo: PathBuf,
    pub host: String,
    pub tls_name: String,
    pub ca_file: PathBuf,
    pub imap_port: u16,
    pub imaps_port: u16,
    pub submission_port: u16,
    pub alice: (String, String),
    pub bob: (String, String),
}

impl Env {
    pub fn load() -> Result<Self> {
        let repo = std::env::var_os("VICEMAIL_REPO")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let repo = repo.canonicalize().context("repository root")?;
        let path = repo.join("testmail/accounts.env");
        let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut vars = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            if let Some((k, v)) = line.split_once('=') {
                vars.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
            }
        }
        let get = |k: &str| {
            vars.get(k)
                .cloned()
                .with_context(|| format!("{k} missing in accounts.env"))
        };
        let port = |k: &str| -> Result<u16> { Ok(get(k)?.parse()?) };
        Ok(Env {
            host: get("TESTMAIL_HOST")?,
            tls_name: get("TESTMAIL_TLS_NAME")?,
            ca_file: repo.join(get("TESTMAIL_CA_FILE")?),
            imap_port: port("TESTMAIL_IMAP_PORT")?,
            imaps_port: port("TESTMAIL_IMAPS_PORT")?,
            submission_port: port("TESTMAIL_SUBMISSION_PORT")?,
            alice: (get("TESTMAIL_ALICE_ADDRESS")?, get("TESTMAIL_ALICE_PASSWORD")?),
            bob: (get("TESTMAIL_BOB_ADDRESS")?, get("TESTMAIL_BOB_PASSWORD")?),
            repo,
        })
    }
}
