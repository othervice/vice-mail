//! rustls client configuration (decision 0002).
//!
//! Certificates are always checked by the operating system (`rustls-platform-verifier`:
//! Security.framework on macOS, CryptoAPI on Windows), so a self-signed server certificate works
//! once the user trusts it in the system. Tests add the test server's CA as an *extra* root
//! ([`ExtraRootCa`]); it is never written to a system store. The crypto provider is `ring`,
//! chosen here and nowhere else.

use std::sync::Arc;

use mail_model::{BackendError, ExtraRootCa};
use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Client configuration: system trust store, plus `extra` (tests only).
pub(crate) fn client_config(
    extra: Option<&ExtraRootCa>,
) -> Result<Arc<ClientConfig>, BackendError> {
    let provider = provider();
    let verifier = match extra {
        None => rustls_platform_verifier::Verifier::new(provider.clone()),
        Some(ca) => {
            let roots = parse_pem(ca.pem())?;
            rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider.clone())
        }
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
