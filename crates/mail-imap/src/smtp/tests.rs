//! Protocol tests of the SMTP sender against a scripted server on 127.0.0.1 with TLS (a test
//! CA made at test time), so every outcome can be provoked exactly: refusals at each step, a
//! connection that ends before, during and after the message data, STARTTLS problems.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mail_model::{
    Access, BackendError, ConnectConfig, Envelope, ErrorCode, ExtraRootCa, Secret, Security,
    SendOutcome, ServerSettings, SmtpStep, Timeouts,
};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use super::SmtpSender;

// ---------------------------------------------------------------------------------------------
// Scripted server

#[derive(Debug, Clone)]
enum Step {
    /// Write this text.
    Send(&'static str),
    /// Read one line; it must start with this text.
    Expect(&'static str),
    /// TLS handshake (after a STARTTLS `220`).
    Tls,
    /// Read the message up to `.` alone on a line and keep it (dot-stuffing undone).
    Data,
    /// Read this many bytes of message data, then go on (to close in the middle).
    ReadBytes(usize),
    Pause(Duration),
    Close,
}

use Step::{Close, Data, Expect, Pause, ReadBytes, Send, Tls};

/// What the server received.
#[derive(Debug, Default)]
struct Seen {
    lines: Vec<String>,
    data: Vec<u8>,
}

trait Io: AsyncRead + AsyncWrite + Unpin + std::marker::Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + std::marker::Send> Io for T {}

struct Pki {
    ca_pem: String,
    acceptor: TlsAcceptor,
}

fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        i32::try_from(y).expect("year"),
        u8::try_from(m).expect("month"),
        u8::try_from(d).expect("day"),
    )
}

/// A test CA and a certificate for `localhost` and 127.0.0.1, valid from yesterday for 30 days
/// (macOS refuses server certificates valid for more than 825 days).
fn pki() -> Pki {
    let today = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs()
            / 86_400,
    )
    .expect("days");
    let day = |offset: i64| {
        let (y, m, d) = civil_from_days(today + offset);
        rcgen::date_time_ymd(y, m, d)
    };
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "SMTP test CA");
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    ca_params.not_before = day(-1);
    ca_params.not_after = day(30);
    let ca_key = KeyPair::generate().expect("key");
    let ca = ca_params.self_signed(&ca_key).expect("ca");
    let issuer = Issuer::new(ca_params, ca_key);

    let mut params = CertificateParams::new(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
        .expect("params");
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params.use_authority_key_identifier_extension = true;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = day(-1);
    params.not_after = day(30);
    let key = KeyPair::generate().expect("key");
    let cert = params.signed_by(&key, &issuer).expect("cert");

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone(), ca.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .expect("server config");
    Pki {
        ca_pem: ca.pem(),
        acceptor: TlsAcceptor::from(Arc::new(server)),
    }
}

struct Server {
    port: u16,
    ca_pem: String,
    task: JoinHandle<Result<Seen, String>>,
}

impl Server {
    /// Accepts one connection (TLS from the first byte with `implicit_tls`) and runs `script`.
    async fn start(implicit_tls: bool, script: Vec<Step>) -> Self {
        let pki = pki();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let acceptor = pki.acceptor.clone();
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let stream: Box<dyn Io> = if implicit_tls {
                Box::new(acceptor.accept(tcp).await.map_err(|e| e.to_string())?)
            } else {
                Box::new(tcp)
            };
            run(stream, acceptor, script).await
        });
        Self {
            port,
            ca_pem: pki.ca_pem,
            task,
        }
    }

    fn config(&self, security: Security) -> ConnectConfig {
        let server = ServerSettings {
            host: "127.0.0.1".into(),
            port: self.port,
            security,
            username: "alice@example.org".into(),
        };
        let mut config = ConnectConfig::new(
            &server,
            Secret::new("pw-alice-1234".to_owned()),
            Access::ReadWrite,
        );
        config.extra_root_ca = Some(ExtraRootCa::from_pem(self.ca_pem.clone().into_bytes()));
        config.timeouts = Timeouts {
            connect: Duration::from_secs(5),
            read: Duration::from_secs(5),
            write: Duration::from_secs(5),
        };
        config
    }

    async fn seen(self) -> Seen {
        tokio::time::timeout(Duration::from_secs(20), self.task)
            .await
            .expect("server finished")
            .expect("server task")
            .unwrap_or_else(|err| panic!("server script: {err}"))
    }
}

async fn run(
    stream: Box<dyn Io>,
    acceptor: TlsAcceptor,
    script: Vec<Step>,
) -> Result<Seen, String> {
    let mut seen = Seen::default();
    let mut stream = BufReader::new(stream);
    for step in script {
        match step {
            Send(text) => {
                stream
                    .get_mut()
                    .write_all(text.as_bytes())
                    .await
                    .map_err(|e| format!("send {text:?}: {e}"))?;
                stream.get_mut().flush().await.map_err(|e| e.to_string())?;
            }
            Expect(prefix) => {
                let mut line = String::new();
                stream
                    .read_line(&mut line)
                    .await
                    .map_err(|e| format!("expect {prefix:?}: {e}"))?;
                let line = line.trim_end().to_owned();
                if !line.starts_with(prefix) {
                    return Err(format!("expected {prefix:?}, got {line:?}"));
                }
                seen.lines.push(line);
            }
            Tls => {
                let tcp = stream.into_inner();
                let tls = acceptor.accept(tcp).await.map_err(|e| e.to_string())?;
                stream = BufReader::new(Box::new(tls));
            }
            Data => loop {
                let mut line = Vec::new();
                let n = stream
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("connection closed inside DATA".into());
                }
                if line == b".\r\n" {
                    break;
                }
                let unstuffed = line.strip_prefix(b".").unwrap_or(&line);
                seen.data.extend_from_slice(unstuffed);
            },
            ReadBytes(n) => {
                let mut buf = vec![0u8; n];
                stream
                    .read_exact(&mut buf)
                    .await
                    .map_err(|e| format!("read {n} bytes: {e}"))?;
            }
            Pause(d) => tokio::time::sleep(d).await,
            Close => return Ok(seen),
        }
    }
    // Keep reading until the client is done, so nothing it sends is lost.
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest)).await;
    for line in String::from_utf8_lossy(&rest).lines() {
        seen.lines.push(line.to_owned());
    }
    Ok(seen)
}

const EHLO_REPLY: &str = "250-test.example greets you\r\n250-PIPELINING\r\n250-8BITMIME\r\n\
                          250-AUTH PLAIN LOGIN\r\n250 ENHANCEDSTATUSCODES\r\n";

/// Greeting to MAIL FROM accepted, for TLS (`starttls = false`) or STARTTLS.
fn opening(starttls: bool) -> Vec<Step> {
    let mut steps = vec![Send("220 test.example ESMTP\r\n")];
    if starttls {
        steps.extend([
            Expect("EHLO [127.0.0.1]"),
            Send("250-test.example\r\n250-STARTTLS\r\n250 8BITMIME\r\n"),
            Expect("STARTTLS"),
            Send("220 2.0.0 go ahead\r\n"),
            Tls,
        ]);
    }
    steps.extend([
        Expect("EHLO [127.0.0.1]"),
        Send(EHLO_REPLY),
        Expect("AUTH PLAIN "),
        Send("235 2.7.0 Authentication successful\r\n"),
    ]);
    steps
}

fn script(starttls: bool, rest: impl IntoIterator<Item = Step>) -> Vec<Step> {
    let mut steps = opening(starttls);
    steps.extend(rest);
    steps
}

fn envelope(recipients: &[&str]) -> Envelope {
    Envelope {
        from: "alice@example.org".into(),
        recipients: recipients.iter().map(|r| (*r).to_owned()).collect(),
    }
}

const MESSAGE: &[u8] = b"From: alice@example.org\r\nTo: bob@example.net\r\nSubject: Hi\r\n\
Message-ID: <m1@example.org>\r\n\r\nHello.\r\n.leading dot\r\n.\r\nend\r\n";

// ---------------------------------------------------------------------------------------------
// Tests

#[tokio::test]
async fn accepted_over_tls_and_starttls() {
    for starttls in [false, true] {
        let server = Server::start(
            !starttls,
            script(
                starttls,
                [
                    Expect("MAIL FROM:<alice@example.org>"),
                    Send("250 2.1.0 ok\r\n"),
                    Expect("RCPT TO:<bob@example.net>"),
                    Send("250 2.1.5 ok\r\n"),
                    Expect("RCPT TO:<carol@example.net>"),
                    Send("250 2.1.5 ok\r\n"),
                    Expect("DATA"),
                    Send("354 go ahead\r\n"),
                    Data,
                    Send("250-2.0.0 queued as ABC123\r\n250 2.0.0 have a nice day\r\n"),
                    Expect("QUIT"),
                    Send("221 2.0.0 bye\r\n"),
                ],
            ),
        )
        .await;
        let security = if starttls {
            Security::Starttls
        } else {
            Security::Tls
        };
        let outcome = SmtpSender::new()
            .send(
                &server.config(security),
                &envelope(&["bob@example.net", "carol@example.net"]),
                MESSAGE,
            )
            .await
            .expect("sent");
        let SendOutcome::Accepted(reply) = outcome else {
            panic!("{security:?}: {outcome:?}");
        };
        assert_eq!(reply.code, 250);
        assert_eq!(reply.enhanced.as_deref(), Some("2.0.0"));
        assert_eq!(reply.text, "queued as ABC123 have a nice day");
        let seen = server.seen().await;
        assert_eq!(seen.data, MESSAGE, "dot-stuffing undone, message unchanged");
        let ehlos = seen.lines.iter().filter(|l| l.starts_with("EHLO")).count();
        assert_eq!(
            ehlos,
            if starttls { 2 } else { 1 },
            "EHLO again after STARTTLS"
        );
        assert!(
            seen.lines
                .iter()
                .all(|l| !l.contains("MAIL FROM") || !l.contains("BODY=")),
            "a 7-bit message needs no BODY parameter"
        );
    }
}

#[tokio::test]
async fn a_refused_recipient_abandons_the_message() {
    let server = Server::start(
        true,
        script(
            false,
            [
                Expect("MAIL FROM:"),
                Send("250 ok\r\n"),
                Expect("RCPT TO:<bob@example.net>"),
                Send("250 ok\r\n"),
                Expect("RCPT TO:<nobody@example.net>"),
                Send("550 5.1.1 <nobody@example.net>: no such user\r\n"),
                Expect("RCPT TO:<busy@example.net>"),
                Send("450 4.2.1 mailbox busy\r\n"),
                Expect("QUIT"),
                Send("221 bye\r\n"),
            ],
        ),
    )
    .await;
    let outcome = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["bob@example.net", "nobody@example.net", "busy@example.net"]),
            MESSAGE,
        )
        .await
        .expect("answered");
    let SendOutcome::Rejected(rejection) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(rejection.step, SmtpStep::Recipients);
    assert!(rejection.is_permanent());
    let refused: Vec<(&str, u16)> = rejection
        .recipients
        .iter()
        .map(|r| (r.address.as_str(), r.reply.code))
        .collect();
    assert_eq!(
        refused,
        [("nobody@example.net", 550), ("busy@example.net", 450)]
    );
    assert_eq!(rejection.reply.enhanced.as_deref(), Some("5.1.1"));
    assert_eq!(
        rejection.to_error().code(),
        ErrorCode::SendRejected,
        "for the UI"
    );
    let seen = server.seen().await;
    assert!(
        !seen.lines.iter().any(|l| l == "DATA"),
        "no DATA after a refusal"
    );
}

#[tokio::test]
async fn refusals_of_sender_and_content_are_typed() {
    // MAIL FROM refused.
    let server = Server::start(
        true,
        script(
            false,
            [
                Expect("MAIL FROM:"),
                Send("553 5.7.1 sender not allowed\r\n"),
                Expect("QUIT"),
                Send("221 bye\r\n"),
            ],
        ),
    )
    .await;
    let outcome = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["b@example.net"]),
            MESSAGE,
        )
        .await
        .expect("answered");
    assert!(
        matches!(&outcome, SendOutcome::Rejected(r) if r.step == SmtpStep::MailFrom && r.is_permanent()),
        "{outcome:?}"
    );
    server.seen().await;

    // Content refused at the end of the data, permanently and for now.
    for (reply, permanent) in [
        ("552 5.3.4 message too big\r\n", true),
        ("451 4.3.0 try again later\r\n", false),
    ] {
        let server = Server::start(
            true,
            script(
                false,
                [
                    Expect("MAIL FROM:"),
                    Send("250 ok\r\n"),
                    Expect("RCPT TO:"),
                    Send("250 ok\r\n"),
                    Expect("DATA"),
                    Send("354 go\r\n"),
                    Data,
                    Send(reply),
                    Expect("QUIT"),
                    Send("221 bye\r\n"),
                ],
            ),
        )
        .await;
        let outcome = SmtpSender::new()
            .send(
                &server.config(Security::Tls),
                &envelope(&["b@example.net"]),
                MESSAGE,
            )
            .await
            .expect("answered");
        let SendOutcome::Rejected(rejection) = &outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(rejection.step, SmtpStep::Message);
        assert_eq!(rejection.is_permanent(), permanent, "{reply}");
        assert_eq!(rejection.to_error().is_transient(), !permanent);
        server.seen().await;
    }
}

#[tokio::test]
async fn connection_lost_after_the_end_of_data_is_unclear() {
    // The server has the whole message but the reply never comes: cut, or silence.
    for (last, read_limit) in [
        (Close, Duration::from_secs(5)),
        (Pause(Duration::from_secs(4)), Duration::from_secs(1)),
    ] {
        let server = Server::start(
            true,
            script(
                false,
                [
                    Expect("MAIL FROM:"),
                    Send("250 ok\r\n"),
                    Expect("RCPT TO:"),
                    Send("250 ok\r\n"),
                    Expect("DATA"),
                    Send("354 go\r\n"),
                    Data,
                    last.clone(),
                    Close,
                ],
            ),
        )
        .await;
        let mut config = server.config(Security::Tls);
        config.timeouts.read = read_limit;
        let outcome = SmtpSender::new()
            .send(&config, &envelope(&["b@example.net"]), MESSAGE)
            .await
            .expect("an outcome, not an error");
        let SendOutcome::Unclear(detail) = &outcome else {
            panic!("{last:?}: {outcome:?}");
        };
        println!("{last:?}: unclear: {detail}");
        assert_eq!(
            outcome.error().map(|e| e.code()),
            Some(ErrorCode::SendUnclear)
        );
        let seen = server.seen().await;
        assert_eq!(seen.data, MESSAGE, "the server got everything");
    }
}

#[tokio::test]
async fn connection_lost_before_or_during_the_data_is_not_sent() {
    // Cut in the middle of a large message: the end marker never went out.
    let mut big = MESSAGE.to_vec();
    for n in 0..120_000 {
        big.extend_from_slice(format!("line {n:06} of a long message body.....\r\n").as_bytes());
    }
    let server = Server::start(
        true,
        script(
            false,
            [
                Expect("MAIL FROM:"),
                Send("250 ok\r\n"),
                Expect("RCPT TO:"),
                Send("250 ok\r\n"),
                Expect("DATA"),
                Send("354 go\r\n"),
                ReadBytes(100_000),
                Close,
            ],
        ),
    )
    .await;
    let err = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["b@example.net"]),
            &big,
        )
        .await
        .expect_err("not sent");
    assert!(
        matches!(err, BackendError::ConnectionLost(_)),
        "{err:?} ({} bytes)",
        big.len()
    );
    assert!(err.is_transient(), "safe to try again");
    server.seen().await;

    // Cut right after RCPT: nothing was sent either.
    let server = Server::start(
        true,
        script(
            false,
            [
                Expect("MAIL FROM:"),
                Send("250 ok\r\n"),
                Expect("RCPT TO:"),
                Close,
            ],
        ),
    )
    .await;
    let err = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["b@example.net"]),
            MESSAGE,
        )
        .await
        .expect_err("not sent");
    assert!(matches!(err, BackendError::ConnectionLost(_)), "{err:?}");
    server.seen().await;
}

#[tokio::test]
async fn login_and_connection_errors_are_typed() {
    // Wrong password: a permanent refusal; the password is not in the error.
    let server = Server::start(
        true,
        vec![
            Send("220 test.example ESMTP\r\n"),
            Expect("EHLO"),
            Send(EHLO_REPLY),
            Expect("AUTH PLAIN "),
            Send("535 5.7.8 Authentication credentials invalid\r\n"),
            Close,
        ],
    )
    .await;
    let err = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["b@example.net"]),
            MESSAGE,
        )
        .await
        .expect_err("refused");
    assert!(matches!(err, BackendError::AuthFailed(_)), "{err:?}");
    assert!(!err.is_transient());
    assert!(!format!("{err:?} {err}").contains("pw-alice"));
    server.seen().await;

    // Only LOGIN offered: used instead of PLAIN.
    let server = Server::start(
        true,
        vec![
            Send("220 test.example ESMTP\r\n"),
            Expect("EHLO"),
            Send("250-test.example\r\n250 AUTH LOGIN\r\n"),
            Expect("AUTH LOGIN"),
            Send("334 VXNlcm5hbWU6\r\n"),
            Expect("YWxpY2VAZXhhbXBsZS5vcmc="),
            Send("334 UGFzc3dvcmQ6\r\n"),
            Expect("cHctYWxpY2UtMTIzNA=="),
            Send("235 ok\r\n"),
            Expect("QUIT"),
            Send("221 bye\r\n"),
        ],
    )
    .await;
    SmtpSender::new()
        .verify(&server.config(Security::Tls))
        .await
        .expect("login");
    server.seen().await;

    // No usable AUTH mechanism.
    let server = Server::start(
        true,
        vec![
            Send("220 test.example ESMTP\r\n"),
            Expect("EHLO"),
            Send("250-test.example\r\n250 AUTH CRAM-MD5\r\n"),
            Close,
        ],
    )
    .await;
    let err = SmtpSender::new()
        .verify(&server.config(Security::Tls))
        .await
        .expect_err("no mechanism");
    assert!(matches!(err, BackendError::Unsupported(_)), "{err:?}");
    server.seen().await;

    // The server refuses the connection in its greeting.
    let server = Server::start(true, vec![Send("554 5.3.2 no service\r\n"), Close]).await;
    let err = SmtpSender::new()
        .verify(&server.config(Security::Tls))
        .await
        .expect_err("refused");
    assert!(matches!(err, BackendError::Rejected(_)), "{err:?}");
    server.seen().await;
}

#[tokio::test]
async fn starttls_is_required_and_checked() {
    // Not offered: the connection ends before anything else is sent.
    let server = Server::start(
        false,
        vec![
            Send("220 test.example ESMTP\r\n"),
            Expect("EHLO"),
            Send("250-test.example\r\n250 AUTH PLAIN\r\n"),
        ],
    )
    .await;
    let err = SmtpSender::new()
        .verify(&server.config(Security::Starttls))
        .await
        .expect_err("no STARTTLS");
    assert_eq!(err, BackendError::StarttlsUnavailable);
    let seen = server.seen().await;
    assert!(
        !seen.lines.iter().any(|l| l.starts_with("AUTH")),
        "no login in plain text: {:?}",
        seen.lines
    );

    // Bytes injected after the 220 to STARTTLS.
    let server = Server::start(
        false,
        vec![
            Send("220 test.example ESMTP\r\n"),
            Expect("EHLO"),
            Send("250-test.example\r\n250 STARTTLS\r\n"),
            Expect("STARTTLS"),
            Send("220 go ahead\r\n250 injected\r\n"),
        ],
    )
    .await;
    let err = SmtpSender::new()
        .verify(&server.config(Security::Starttls))
        .await
        .expect_err("injection");
    assert!(matches!(err, BackendError::Protocol(_)), "{err:?}");
    server.seen().await;
}

#[tokio::test]
async fn certificates_are_checked() {
    for (security, implicit) in [(Security::Tls, true), (Security::Starttls, false)] {
        let mut script = Vec::new();
        if !implicit {
            script.extend([
                Send("220 test.example ESMTP\r\n"),
                Expect("EHLO"),
                Send("250-test.example\r\n250 STARTTLS\r\n"),
                Expect("STARTTLS"),
                Send("220 go ahead\r\n"),
                Tls,
            ]);
        }
        // Without the test CA the handshake fails on the client side; the server just ends.
        let server = Server::start(implicit, script).await;
        let mut config = server.config(security);
        config.extra_root_ca = None;
        let err = SmtpSender::new()
            .verify(&config)
            .await
            .expect_err("untrusted");
        assert!(
            matches!(err, BackendError::CertUntrusted(_)),
            "{security:?}: {err:?}"
        );
        let _ = server.task.await;
    }
    let server = Server::start(true, Vec::new()).await;
    let err = SmtpSender::new()
        .with_tls_server_name("wrong.example")
        .verify(&server.config(Security::Tls))
        .await
        .expect_err("name");
    assert_eq!(err, BackendError::CertNameMismatch);
    let _ = server.task.await;
}

#[tokio::test]
async fn read_only_accounts_cannot_send() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let server = ServerSettings {
        host: "127.0.0.1".into(),
        port: listener.local_addr().expect("addr").port(),
        security: Security::Tls,
        username: "alice@example.org".into(),
    };
    let config = ConnectConfig::new(&server, Secret::new("pw".into()), Access::ReadOnly);
    let err = SmtpSender::new()
        .send(&config, &envelope(&["b@example.net"]), MESSAGE)
        .await
        .expect_err("read-only");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    let accepted = tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
    assert!(accepted.is_err(), "nothing connected");
}

#[tokio::test]
async fn non_ascii_needs_the_server_extensions() {
    // 8BITMIME offered (EHLO_REPLY), SMTPUTF8 not.
    let server = Server::start(
        true,
        script(
            false,
            [
                Expect("MAIL FROM:<alice@example.org> BODY=8BITMIME"),
                Send("250 ok\r\n"),
                Expect("RCPT TO:"),
                Send("250 ok\r\n"),
                Expect("DATA"),
                Send("354 go\r\n"),
                Data,
                Send("250 ok\r\n"),
                Expect("QUIT"),
                Send("221 bye\r\n"),
            ],
        ),
    )
    .await;
    let outcome = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["b@example.net"]),
            "Subject: Grüße\r\n\r\nGrüße\r\n".as_bytes(),
        )
        .await
        .expect("sent");
    assert!(matches!(outcome, SendOutcome::Accepted(_)), "{outcome:?}");
    server.seen().await;

    let server = Server::start(true, script(false, [])).await;
    let err = SmtpSender::new()
        .send(
            &server.config(Security::Tls),
            &envelope(&["jörg@example.net"]),
            MESSAGE,
        )
        .await
        .expect_err("needs SMTPUTF8");
    assert!(matches!(err, BackendError::Unsupported(_)), "{err:?}");
    let seen = server.seen().await;
    assert!(
        !seen.lines.iter().any(|l| l.starts_with("MAIL")),
        "{:?}",
        seen.lines
    );
}

#[tokio::test]
async fn envelope_problems_fail_before_connecting() {
    let server = ServerSettings {
        host: "127.0.0.1".into(),
        port: 9,
        security: Security::Tls,
        username: "a".into(),
    };
    let config = ConnectConfig::new(&server, Secret::new("pw".into()), Access::ReadWrite);
    let err = SmtpSender::new()
        .send(&config, &envelope(&[]), MESSAGE)
        .await
        .expect_err("no recipients");
    assert!(matches!(err, BackendError::InvalidState(_)), "{err:?}");
    let err = SmtpSender::new()
        .send(&config, &envelope(&["not an address"]), MESSAGE)
        .await
        .expect_err("bad address");
    assert!(matches!(err, BackendError::InvalidState(_)), "{err:?}");
}
