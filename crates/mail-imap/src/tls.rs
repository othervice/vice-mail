//! The app's TLS setup, for every connection it makes (decision 0002): IMAP and SMTP here,
//! HTTPS for server discovery and external images in `mail-engine` (through reqwest), and the
//! TLS servers of the tests. The crypto provider is chosen in [`provider`] and nowhere else.
//!
//! Certificates are always checked by the operating system (`rustls-platform-verifier`:
//! Security.framework on macOS, CryptoAPI on Windows), so a self-signed server certificate works
//! once the user trusts it in the system. Tests add the test server's CA as an *extra* root
//! ([`ExtraRootCa`]); it is never written to a system store.
//!
//! Why here: `mail-imap` holds the TLS connections and maps their errors, `mail-engine` already
//! depends on it, and `mail-model` stays free of IO and of rustls. A crate of its own for these
//! few functions would add a crate and buy no boundary.

use std::sync::Arc;

use mail_model::{BackendError, ExtraRootCa};
use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

/// The crypto provider of every TLS client and test server, and the random source of render
/// tokens: `aws-lc-rs`, the rustls default (decision 0002, addendum of 23.11.2026).
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// Client configuration: system trust store, plus `extra` (tests only).
pub fn client_config(extra: Option<&ExtraRootCa>) -> Result<Arc<ClientConfig>, BackendError> {
    let roots = match extra {
        None => Vec::new(),
        Some(ca) => parse_pem(ca.pem())?,
    };
    client_config_with_roots(roots)
}

/// [`client_config`] with extra roots that are already certificates (tests only; empty in
/// the product).
pub fn client_config_with_roots(
    extra: Vec<CertificateDer<'static>>,
) -> Result<Arc<ClientConfig>, BackendError> {
    let provider = provider();
    let verifier = if extra.is_empty() {
        rustls_platform_verifier::Verifier::new(provider.clone())
    } else {
        rustls_platform_verifier::Verifier::new_with_extra_roots(extra, provider.clone())
    }
    .map_err(|err| BackendError::Tls(format!("certificate verifier: {err}")))?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|err| BackendError::Tls(format!("protocol versions: {err}")))?
        // Only the name is "dangerous": a verifier is plugged in, nothing is switched off.
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn parse_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, BackendError> {
    let certs = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            BackendError::InvalidState(format!("extra root CA is not valid PEM: {err}"))
        })?;
    if certs.is_empty() {
        return Err(BackendError::InvalidState(
            "extra root CA contains no certificate".into(),
        ));
    }
    Ok(certs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_config_builds_and_bad_extra_roots_are_rejected() {
        assert!(client_config(None).is_ok());
        let empty = ExtraRootCa::from_pem(b"no certificate here".to_vec());
        assert!(matches!(
            client_config(Some(&empty)),
            Err(BackendError::InvalidState(_))
        ));
        let broken = ExtraRootCa::from_pem(
            b"-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n".to_vec(),
        );
        assert!(matches!(
            client_config(Some(&broken)),
            Err(BackendError::InvalidState(_))
        ));
    }
}
