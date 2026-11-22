//! Optional: lettre 0.11 with Tokio + rustls on 1587 (STARTTLS) with the same extra-root concept.

use std::time::Duration;

use anyhow::Result;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::report::Report;
use crate::scenarios::Ctx;

fn mailer(cx: &Ctx, with_ca: bool) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let mut p = TlsParameters::builder(cx.env.tls_name.clone());
    if with_ca {
        // With the `rustls-platform-verifier` feature this becomes Verifier::new_with_extra_roots.
        p = p.add_root_certificate(Certificate::from_pem(&std::fs::read(&cx.env.ca_file)?)?);
    }
    let tls = p.build_rustls()?;
    Ok(
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(cx.env.host.as_str()) // "dangerous" = no implicit TLS; STARTTLS is required below
            .port(cx.env.submission_port)
            .tls(Tls::Required(tls))
            .credentials(Credentials::new(cx.env.bob.0.clone(), cx.env.bob.1.clone()))
            .timeout(Some(Duration::from_secs(10)))
            .build(),
    )
}

pub async fn send_bob_to_alice(cx: &Ctx, subject: &str, with_ca: bool) -> Result<()> {
    let msg = Message::builder()
        .from(cx.env.bob.0.parse()?)
        .to(cx.env.alice.0.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(format!("spike A: {subject}\n"))?;
    mailer(cx, with_ca)?.send(msg).await?;
    Ok(())
}

pub async fn run(cx: &Ctx, r: &mut Report) -> Result<()> {
    let subject = format!("lettre-{}", std::process::id());
    let res = send_bob_to_alice(cx, &subject, true).await;
    r.check(
        "smtp.lettre_starttls_test_ca",
        res.is_ok(),
        format!("bob → alice over 1587 STARTTLS: {:?}", res.map_err(|e| e.to_string())),
    );

    let res = send_bob_to_alice(cx, "must-not-arrive", false).await;
    let err = res
        .as_ref()
        .err()
        .and_then(|e| e.downcast_ref::<lettre::transport::smtp::Error>());
    let is_tls = err.is_some_and(|e| e.is_tls());
    // lettre reports it as a connection error; the rustls error is in the source chain.
    let typed = err.and_then(|e| find_rustls(e)).map(crate::errors::from_tls);
    r.check(
        "smtp.lettre_untrusted_without_ca",
        matches!(typed, Some(crate::errors::NetError::CertUntrusted(_))),
        format!(
            "without the CA: {:?}; lettre is_tls() = {is_tls}; from source chain: {:?}",
            res.map_err(|e| e.to_string()),
            typed.map(|t| t.kind())
        ),
    );
    Ok(())
}

fn find_rustls<'a>(e: &'a (dyn std::error::Error + 'static)) -> Option<&'a rustls::Error> {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(c) = cur {
        if let Some(t) = c.downcast_ref::<rustls::Error>() {
            return Some(t);
        }
        if let Some(io) = c.downcast_ref::<std::io::Error>()
            && let Some(t) = io.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>())
        {
            return Some(t);
        }
        cur = c.source();
    }
    None
}
