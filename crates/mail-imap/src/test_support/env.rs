//! Test server settings from `testmail/accounts.env` (test values only, never real accounts).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use mail_model::{Access, ConnectConfig, ExtraRootCa, Secret, Security, ServerSettings, Timeouts};

/// One account on the test server.
#[derive(Debug, Clone)]
pub struct TestAccount {
    pub address: String,
    pub password: Secret<String>,
}

/// The local test server as described by `testmail/accounts.env`.
#[derive(Debug, Clone)]
pub struct TestEnv {
    pub repo: PathBuf,
    /// `127.0.0.1`; the certificate also names `localhost` and `::1`.
    pub host: String,
    /// Name in the server certificate (`localhost`).
    pub tls_name: String,
    /// IMAP with STARTTLS.
    pub imap_port: u16,
    /// IMAP with implicit TLS.
    pub imaps_port: u16,
    /// SMTP submission with STARTTLS and AUTH.
    pub submission_port: u16,
    /// SMTP submission with implicit TLS and AUTH.
    pub smtps_port: u16,
    pub alice: TestAccount,
    pub bob: TestAccount,
    /// The server administrator; it has a mailbox too (a third recipient, for Bcc tests).
    pub admin: TestAccount,
    /// PEM of the test CA (`testmail/.data/tls/ca.pem`, created by `scripts/testmail.sh`).
    pub ca_pem: Vec<u8>,
}

impl TestEnv {
    /// Loads the settings. The repository root is `VICEMAIL_REPO` or two levels above this
    /// crate. Fails with a hint when the test server was never set up.
    pub fn load() -> Result<Self, String> {
        let repo = std::env::var_os("VICEMAIL_REPO")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let path = repo.join("testmail/accounts.env");
        let text = std::fs::read_to_string(&path)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        let mut vars = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            if let Some((key, value)) = line.split_once('=') {
                vars.insert(
                    key.trim().to_owned(),
                    value.trim().trim_matches('"').to_owned(),
                );
            }
        }
        let get = |key: &str| {
            vars.get(key)
                .cloned()
                .ok_or_else(|| format!("{key} missing in {}", path.display()))
        };
        let port = |key: &str| -> Result<u16, String> {
            get(key)?
                .parse()
                .map_err(|err| format!("{key} in {}: {err}", path.display()))
        };
        let account = |prefix: &str| -> Result<TestAccount, String> {
            Ok(TestAccount {
                address: get(&format!("TESTMAIL_{prefix}_ADDRESS"))?,
                password: Secret::new(get(&format!("TESTMAIL_{prefix}_PASSWORD"))?),
            })
        };
        let ca_path = repo.join(get("TESTMAIL_CA_FILE")?);
        let ca_pem = std::fs::read(&ca_path).map_err(|err| {
            format!(
                "read {}: {err} (set up the test server: scripts/testmail.sh start)",
                ca_path.display()
            )
        })?;
        Ok(Self {
            host: get("TESTMAIL_HOST")?,
            tls_name: get("TESTMAIL_TLS_NAME")?,
            imap_port: port("TESTMAIL_IMAP_PORT")?,
            imaps_port: port("TESTMAIL_IMAPS_PORT")?,
            submission_port: port("TESTMAIL_SUBMISSION_PORT")?,
            smtps_port: port("TESTMAIL_SMTPS_PORT")?,
            alice: account("ALICE")?,
            bob: account("BOB")?,
            admin: account("ADMIN")?,
            ca_pem,
            repo,
        })
    }

    /// Like [`TestEnv::load`], but also checks that the server accepts connections; panics with
    /// the command to run otherwise (for `#[ignore]`d server tests).
    pub fn require() -> Self {
        let env = match Self::load() {
            Ok(env) => env,
            Err(err) => panic!("test server not set up: {err}"),
        };
        let addr = format!("{}:{}", env.host, env.imaps_port);
        if std::net::TcpStream::connect_timeout(
            &addr
                .parse()
                .unwrap_or_else(|_| panic!("bad address {addr}")),
            Duration::from_secs(2),
        )
        .is_err()
        {
            panic!(
                "test server not running on {addr}; run: scripts/testmail.sh reset && \
                 scripts/testmail.sh start && scripts/testmail.sh seed 1000"
            );
        }
        env
    }

    pub fn extra_root(&self) -> ExtraRootCa {
        ExtraRootCa::from_pem(self.ca_pem.clone())
    }

    /// Connection settings for `account` on the IMAP port of `security`, with the test CA.
    pub fn config(
        &self,
        account: &TestAccount,
        security: Security,
        access: Access,
    ) -> ConnectConfig {
        let port = match security {
            Security::Tls => self.imaps_port,
            Security::Starttls => self.imap_port,
        };
        self.config_at(account, &self.host, port, security, access)
    }

    /// SMTP settings for `account`: submission with STARTTLS (1587) or implicit TLS (1465),
    /// read-write, with the test CA.
    pub fn smtp_config(&self, account: &TestAccount, security: Security) -> ConnectConfig {
        self.config_at(
            account,
            &self.host,
            self.smtp_port(security),
            security,
            Access::ReadWrite,
        )
    }

    /// The SMTP submission port for `security`.
    pub fn smtp_port(&self, security: Security) -> u16 {
        match security {
            Security::Tls => self.smtps_port,
            Security::Starttls => self.submission_port,
        }
    }

    /// Like [`TestEnv::config`] for any host and port (for example a [`super::Proxy`]).
    pub fn config_at(
        &self,
        account: &TestAccount,
        host: &str,
        port: u16,
        security: Security,
        access: Access,
    ) -> ConnectConfig {
        let server = ServerSettings {
            host: host.to_owned(),
            port,
            security,
            username: account.address.clone(),
        };
        let mut config = ConnectConfig::new(&server, account.password.clone(), access);
        config.extra_root_ca = Some(self.extra_root());
        config.timeouts = Timeouts {
            connect: Duration::from_secs(10),
            read: Duration::from_secs(30),
            write: Duration::from_secs(30),
        };
        config
    }
}
