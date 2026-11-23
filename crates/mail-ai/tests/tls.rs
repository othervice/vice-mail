//! HTTPS uses exactly the TLS configuration the caller passes in (in the product
//! `mail_imap::tls::client_config`: the system's certificate check). A server whose
//! certificate that configuration does not trust gets no request.

mod support;

use std::sync::Arc;

use mail_ai::{
    AiError, CancelToken, ChatRequest, ProviderConfig, ProviderKind, build_provider, collect,
};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use support::{MockServer, openai_stream};

/// A test CA and a server certificate for `localhost` signed by it.
fn pki() -> (CertificateDer<'static>, rustls::ServerConfig) {
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "mail-ai test CA");
    let ca =
        CertifiedIssuer::self_signed(ca_params, KeyPair::generate().expect("key")).expect("ca");
    let key = KeyPair::generate().expect("key");
    let cert = CertificateParams::new(vec!["localhost".to_owned()])
        .expect("params")
        .signed_by(&key, &ca)
        .expect("cert");
    let server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .expect("server config");
    (ca.der().clone(), server)
}

fn provider(port: u16, tls: &rustls::ClientConfig) -> Box<dyn mail_ai::AiProvider> {
    let config = support::quick(ProviderConfig::new(
        ProviderKind::OpenAiCompatible,
        format!("https://localhost:{port}/v1"),
        None,
    ));
    build_provider(config, tls).expect("provider")
}

#[tokio::test]
async fn https_goes_through_the_injected_tls_configuration() {
    let (ca, server_config) = pki();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
    let server = MockServer::start_tls(vec![openai_stream("secure", 3)], acceptor).await;
    let request = ChatRequest::connection_test("test-model").expect("request");

    // A configuration that does not trust the test CA: the handshake fails, nothing is sent.
    let untrusting = provider(server.addr.port(), &support::tls());
    let err = untrusting
        .chat(&request, &CancelToken::new())
        .await
        .expect_err("untrusted certificate");
    assert!(matches!(err, AiError::Connect(_)), "{err:?}");
    assert!(server.requests().is_empty());

    // The same request with a configuration that trusts it.
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca).expect("root");
    let trusting = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let provider = provider(server.addr.port(), &trusting);
    let stream = provider
        .chat(&request, &CancelToken::new())
        .await
        .expect("stream");
    let text = collect(stream, |_| {}).await.expect("answer").into_text();
    assert_eq!(text.as_deref(), Ok("secure"));
    assert_eq!(server.requests().len(), 1);
}
