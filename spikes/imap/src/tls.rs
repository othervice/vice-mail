//! rustls client configurations.
//!
//! Product idea: always the platform verifier (macOS Security.framework / Windows CryptoAPI),
//! optionally with *extra* roots. Production passes none; tests pass the test CA. The CA is
//! never written to the system keychain.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

pub fn load_pem(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let certs = CertificateDer::pem_file_iter(path)
        .with_context(|| format!("open {}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("parse {}", path.display()))?;
    anyhow::ensure!(!certs.is_empty(), "no certificate in {}", path.display());
    Ok(certs)
}

/// System trust store plus `extra_roots` (empty in production).
pub fn platform(extra_roots: Vec<CertificateDer<'static>>) -> Result<Arc<ClientConfig>> {
    let prov = provider();
    let verifier = if extra_roots.is_empty() {
        rustls_platform_verifier::Verifier::new(prov.clone())?
    } else {
        rustls_platform_verifier::Verifier::new_with_extra_roots(extra_roots, prov.clone())?
    };
    let cfg = ClientConfig::builder_with_provider(prov)
        .with_safe_default_protocol_versions()?
        .dangerous() // only the name: a custom verifier is plugged in, nothing is disabled
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// Alternative without the platform: webpki with exactly these roots.
pub fn webpki_only(roots: Vec<CertificateDer<'static>>) -> Result<Arc<ClientConfig>> {
    let mut store = rustls::RootCertStore::empty();
    for r in roots {
        store.add(r)?;
    }
    let cfg = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_root_certificates(store)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}
