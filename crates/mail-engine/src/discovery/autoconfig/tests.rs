use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use super::*;

fn alice() -> EmailAddress {
    EmailAddress::parse("alice+test@example.org").expect("address")
}

// ---------------------------------------------------------------------------------------------
// Parser

const FULL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="example.org">
    <domain>example.org</domain>
    <displayName>Example Mail</displayName>
    <incomingServer type="pop3">
      <hostname>pop.example.org</hostname>
      <port>995</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>mail.%EMAILDOMAIN%</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILLOCALPART%</username>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>SMTP.Example.org.</hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILADDRESS%</username>
      <authentication>password-encrypted</authentication>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

#[test]
fn parses_servers_and_fills_placeholders() {
    let servers = parse(FULL.as_bytes(), &alice()).expect("parse");
    assert_eq!(
        servers.imap,
        Some(ServerSettings {
            host: "mail.example.org".into(),
            port: 993,
            security: Security::Tls,
            username: "alice+test".into(),
        })
    );
    assert_eq!(
        servers.smtp,
        Some(ServerSettings {
            host: "smtp.example.org".into(),
            port: 587,
            security: Security::Starttls,
            username: "alice+test@example.org".into(),
        })
    );

    // A byte order mark is fine; the domain placeholder in a host name is the ASCII form.
    let idn = EmailAddress::parse("max@Müller.example").expect("address");
    let servers = parse(format!("\u{feff}{FULL}").as_bytes(), &idn).expect("parse");
    assert_eq!(
        servers.imap.expect("imap").host,
        "mail.xn--mller-kva.example"
    );
}

fn single(server: &str) -> String {
    format!(
        "<clientConfig version=\"1.1\"><emailProvider id=\"x\">{server}</emailProvider></clientConfig>"
    )
}

#[test]
fn skips_servers_the_app_cannot_use_and_takes_the_next() {
    let xml = single(
        r#"
        <incomingServer type="imap">
          <hostname>plain.example.org</hostname><port>143</port><socketType>plain</socketType>
        </incomingServer>
        <incomingServer type="imap">
          <hostname>oauth.example.org</hostname><port>993</port><socketType>SSL</socketType>
          <authentication>OAuth2</authentication>
        </incomingServer>
        <incomingServer type="imap">
          <hostname>bad host</hostname><port>993</port><socketType>SSL</socketType>
        </incomingServer>
        <incomingServer type="imap">
          <hostname>port.example.org</hostname><port>99999</port><socketType>SSL</socketType>
        </incomingServer>
        <incomingServer type="imap">
          <hostname>%REALNAME%.example.org</hostname><port>993</port><socketType>SSL</socketType>
        </incomingServer>
        <incomingServer type="imap">
          <hostname>good.example.org</hostname><port>993</port><socketType>SSL</socketType>
          <authentication>OAuth2</authentication>
          <authentication>password-cleartext</authentication>
        </incomingServer>"#,
    );
    let servers = parse(xml.as_bytes(), &alice()).expect("parse");
    let imap = servers.imap.expect("imap");
    assert_eq!(imap.host, "good.example.org");
    assert_eq!(
        imap.username, "alice+test@example.org",
        "default login is the address"
    );
    assert_eq!(servers.smtp, None);
}

#[test]
fn infers_security_and_port_only_from_well_known_values() {
    let xml = single(
        r#"
        <incomingServer type="imap"><hostname>imap.example.org</hostname><port>143</port></incomingServer>
        <outgoingServer type="smtp"><hostname>smtp.example.org</hostname><socketType>SSL</socketType></outgoingServer>"#,
    );
    let servers = parse(xml.as_bytes(), &alice()).expect("parse");
    let imap = servers.imap.expect("imap");
    assert_eq!((imap.port, imap.security), (143, Security::Starttls));
    let smtp = servers.smtp.expect("smtp");
    assert_eq!((smtp.port, smtp.security), (465, Security::Tls));

    let xml = single(
        r#"<incomingServer type="imap"><hostname>imap.example.org</hostname><port>1993</port></incomingServer>"#,
    );
    assert_eq!(parse(xml.as_bytes(), &alice()).expect("parse").imap, None);
}

#[test]
fn rejects_hostile_and_broken_files() {
    let a = alice();
    let laughs = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">]>
<clientConfig><emailProvider><incomingServer type="imap"><hostname>&lol2;</hostname></incomingServer></emailProvider></clientConfig>"#;
    let external = r#"<?xml version="1.0"?>
<!DOCTYPE clientConfig [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<clientConfig><emailProvider><incomingServer type="imap"><hostname>&xxe;</hostname></incomingServer></emailProvider></clientConfig>"#;
    let many_nodes = single(&"<x/>".repeat(MAX_XML_NODES as usize + 10));
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("entity expansion", laughs.into()),
        ("external entity", external.into()),
        ("too many nodes", many_nodes.into_bytes()),
        ("too large", vec![b' '; MAX_BODY + 1]),
        ("not UTF-8", b"<clientConfig>\xff</clientConfig>".to_vec()),
        (
            "wrong root",
            b"<html><body>Not found</body></html>".to_vec(),
        ),
        ("no provider", b"<clientConfig version=\"1.1\"/>".to_vec()),
        ("not XML", b"{\"imap\": \"x\"}".to_vec()),
        ("unclosed", b"<clientConfig><emailProvider>".to_vec()),
    ];
    for (what, body) in cases {
        assert!(parse(&body, &a).is_err(), "{what} was accepted");
    }
}

#[test]
fn urls_are_the_two_places_of_the_domain() {
    let [first, second] = candidate_urls("example.org", &alice()).expect("urls");
    assert_eq!(
        first.as_str(),
        "https://autoconfig.example.org/mail/config-v1.1.xml?emailaddress=alice%2Btest%40example.org"
    );
    assert_eq!(
        second.as_str(),
        "https://example.org/.well-known/autoconfig/mail/config-v1.1.xml"
    );
}

#[test]
fn only_https_urls_within_the_domain_are_allowed() {
    let ok = |url: &str| allowed_url(&Url::parse(url).expect("url"), "example.org");
    assert!(ok("https://example.org/x"));
    assert!(ok("https://autoconfig.example.org/x"));
    assert!(ok("https://a.b.example.org:443/x"));
    assert!(!ok("http://example.org/x"), "plain http");
    assert!(!ok("https://evil.org/x"));
    assert!(!ok("https://notexample.org/x"), "suffix without a dot");
    assert!(!ok("https://example.org.evil.org/x"));
    assert!(!ok("https://example.org:8443/x"), "other port");
    assert!(
        !ok("https://user:pw@example.org/x"),
        "credentials in the URL"
    );
    assert!(!ok("https://192.0.2.1/x"));
}

// ---------------------------------------------------------------------------------------------
// HTTPS fetcher against local servers

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (H. Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        i32::try_from(year).expect("year"),
        u8::try_from(month).expect("month"),
        u8::try_from(day).expect("day"),
    )
}

/// A test CA and a server certificate for `names`, valid from yesterday for 30 days (macOS
/// rejects server certificates valid for more than 825 days).
struct Pki {
    ca: CertificateDer<'static>,
    server: Arc<rustls::ServerConfig>,
}

fn pki(names: &[&str]) -> Pki {
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
        .push(DnType::CommonName, "Discovery test CA");
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

    let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    let mut params = CertificateParams::new(names.clone()).expect("params");
    params.distinguished_name.push(
        DnType::CommonName,
        names.first().cloned().unwrap_or_default(),
    );
    params.use_authority_key_identifier_extension = true;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = day(-1);
    params.not_after = day(30);
    let key = KeyPair::generate().expect("key");
    let cert = params.signed_by(&key, &issuer).expect("cert");

    let server = rustls::ServerConfig::builder_with_provider(mail_imap::tls::provider())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone(), ca.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .expect("server config");
    Pki {
        ca: ca.der().clone(),
        server: Arc::new(server),
    }
}

/// A request as the test server saw it. Header names are lowercase.
#[derive(Debug, Clone)]
struct Seen {
    target: String,
    headers: HashMap<String, String>,
}

impl Seen {
    fn host(&self) -> &str {
        self.headers.get("host").map_or("", String::as_str)
    }
}

#[derive(Debug, Clone, Default)]
struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    /// Send the body chunked, without a length.
    chunked: bool,
    /// Never answer.
    stall: bool,
}

impl Reply {
    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            ..Self::default()
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }

    fn redirect(status: u16, location: &str) -> Self {
        Self {
            status,
            headers: vec![("location", location.to_owned())],
            ..Self::default()
        }
    }
}

type Handler = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

/// HTTPS server on 127.0.0.1: one request per connection, answered by `handler`.
/// Counts TCP connections (also those whose TLS handshake fails).
struct TestServer {
    addr: SocketAddr,
    connections: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TestServer {
    async fn start(tls: Arc<rustls::ServerConfig>, handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let acceptor = TlsAcceptor::from(tls);
        let (count, log) = (Arc::clone(&connections), Arc::clone(&seen));
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                count.fetch_add(1, Ordering::SeqCst);
                let (acceptor, handler, log) =
                    (acceptor.clone(), Arc::clone(&handler), Arc::clone(&log));
                tokio::spawn(async move {
                    let Ok(mut stream) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let Some(request) = read_head(&mut stream).await else {
                        return;
                    };
                    log.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(request.clone());
                    let reply = handler(&request);
                    if reply.stall {
                        std::future::pending::<()>().await;
                    }
                    let _ = stream.write_all(&encode(&reply)).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            addr,
            connections,
            seen,
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

async fn read_head<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 || buf.len() > 16 * 1024 {
            return None;
        }
        buf.extend_from_slice(chunk.get(..n)?);
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = text.split("\r\n");
    let target = lines.next()?.split(' ').nth(1)?.to_owned();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Some(Seen { target, headers })
}

fn encode(reply: &Reply) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {} X\r\nconnection: close\r\n", reply.status);
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    let mut out = Vec::new();
    if reply.chunked {
        head.push_str("transfer-encoding: chunked\r\n\r\n");
        out.extend_from_slice(head.as_bytes());
        for piece in reply.body.chunks(8 * 1024) {
            out.extend_from_slice(format!("{:x}\r\n", piece.len()).as_bytes());
            out.extend_from_slice(piece);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"0\r\n\r\n");
    } else {
        head.push_str(&format!("content-length: {}\r\n\r\n", reply.body.len()));
        out.extend_from_slice(head.as_bytes());
        out.extend_from_slice(&reply.body);
    }
    out
}

const NAMES: [&str; 3] = [
    "autoconfig.example.test",
    "example.test",
    "www.example.test",
];

/// The domain's server (all of [`NAMES`]) plus a foreign one (`evil.test`), and a fetcher
/// that trusts the test CA and resolves those names to the local servers.
struct Setup {
    server: TestServer,
    foreign: TestServer,
    fetcher: HttpsFetcher,
}

async fn setup(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Setup {
    let pki = pki(&[NAMES[0], NAMES[1], NAMES[2], "evil.test"]);
    let server = TestServer::start(Arc::clone(&pki.server), Arc::new(handler)).await;
    let foreign = TestServer::start(
        Arc::clone(&pki.server),
        Arc::new(|_: &Seen| Reply::ok(FULL)),
    )
    .await;
    let mut overrides: Vec<(&str, SocketAddr)> = NAMES.iter().map(|n| (*n, server.addr)).collect();
    overrides.push(("evil.test", foreign.addr));
    let fetcher = HttpsFetcher::for_tests(pki.ca, &overrides, Duration::from_secs(1));
    Setup {
        server,
        foreign,
        fetcher,
    }
}

fn url(text: &str) -> Url {
    Url::parse(text).expect("url")
}

#[tokio::test]
async fn fetches_over_https_with_neutral_headers_and_no_cookies() {
    let s = setup(|req| {
        if req.target.starts_with("/set-cookie") {
            Reply {
                headers: vec![("set-cookie", "session=abc123; Path=/".into())],
                ..Reply::ok("first")
            }
        } else {
            Reply::ok(FULL)
        }
    })
    .await;
    let first = s
        .fetcher
        .fetch(&url("https://example.test/set-cookie"), "example.test")
        .await
        .expect("fetch");
    assert_eq!(first.as_deref(), Some(&b"first"[..]));
    let body = s
        .fetcher
        .fetch(&url("https://example.test/again"), "example.test")
        .await
        .expect("fetch")
        .expect("body");
    assert_eq!(body, FULL.as_bytes());

    let seen = s.server.seen();
    assert_eq!(seen.len(), 2);
    for request in &seen {
        assert_eq!(
            request.headers.get("user-agent").map(String::as_str),
            Some(NEUTRAL_USER_AGENT)
        );
        assert!(!request.headers.contains_key("cookie"), "{request:?}");
        assert!(!request.headers.contains_key("referer"), "{request:?}");
        assert_eq!(request.host(), "example.test");
    }
}

#[tokio::test]
async fn missing_file_is_none() {
    let s = setup(|_| Reply::status(404)).await;
    let got = s
        .fetcher
        .fetch(
            &url("https://example.test/.well-known/autoconfig/mail/config-v1.1.xml"),
            "example.test",
        )
        .await
        .expect("fetch");
    assert_eq!(got, None);
}

#[tokio::test]
async fn follows_redirects_within_the_domain() {
    let s = setup(|req| match (req.host(), req.target.as_str()) {
        ("example.test", "/.well-known/autoconfig/mail/config-v1.1.xml") => {
            Reply::redirect(301, "https://www.example.test/auto.xml")
        }
        ("www.example.test", "/auto.xml") => Reply::redirect(307, "/final.xml"),
        ("www.example.test", "/final.xml") => Reply::ok(FULL),
        _ => Reply::status(404),
    })
    .await;
    let body = s
        .fetcher
        .fetch(
            &url("https://example.test/.well-known/autoconfig/mail/config-v1.1.xml"),
            "example.test",
        )
        .await
        .expect("fetch")
        .expect("body");
    assert_eq!(body, FULL.as_bytes());
    let hops: Vec<String> = s
        .server
        .seen()
        .iter()
        .map(|r| format!("{}{}", r.host(), r.target))
        .collect();
    assert_eq!(
        hops,
        [
            "example.test/.well-known/autoconfig/mail/config-v1.1.xml",
            "www.example.test/auto.xml",
            "www.example.test/final.xml"
        ]
    );
}

/// A redirect to another domain, to plain http or to another port is refused before any
/// connection is made. Positive control: the foreign server counts connections.
#[tokio::test]
async fn refuses_redirects_out_of_the_domain_without_connecting() {
    let s = setup(|req| match req.target.as_str() {
        "/foreign" => Reply::redirect(302, "https://evil.test/config.xml"),
        "/plain" => Reply::redirect(302, "http://www.example.test/config.xml"),
        "/port" => Reply::redirect(302, "https://www.example.test:8443/config.xml"),
        "/loop" => Reply::redirect(302, "/loop"),
        "/nowhere" => Reply::status(302),
        _ => Reply::status(404),
    })
    .await;
    for path in ["/foreign", "/plain", "/port", "/loop", "/nowhere"] {
        let got = s
            .fetcher
            .fetch(&url(&format!("https://example.test{path}")), "example.test")
            .await;
        assert!(got.is_err(), "{path}: {got:?}");
    }
    assert_eq!(s.foreign.connections(), 0, "foreign host contacted");
    assert_eq!(
        s.server
            .seen()
            .iter()
            .filter(|r| r.target == "/loop")
            .count(),
        MAX_REDIRECTS + 1
    );

    // Positive control: the counter does count.
    let body = s
        .fetcher
        .fetch(&url("https://evil.test/config.xml"), "evil.test")
        .await
        .expect("fetch");
    assert!(body.is_some());
    assert_eq!(s.foreign.connections(), 1);
}

#[tokio::test]
async fn enforces_the_size_limit() {
    let s = setup(|req| {
        let big = vec![b'a'; MAX_BODY + 1];
        match req.target.as_str() {
            "/length" => Reply::ok(big),
            "/chunked" => Reply {
                chunked: true,
                ..Reply::ok(big)
            },
            _ => Reply {
                chunked: true,
                ..Reply::ok(vec![b'a'; MAX_BODY])
            },
        }
    })
    .await;
    for path in ["/length", "/chunked"] {
        let got = s
            .fetcher
            .fetch(&url(&format!("https://example.test{path}")), "example.test")
            .await;
        let err = got.expect_err(path);
        assert!(err.0.contains("larger"), "{path}: {err}");
    }
    let exact = s
        .fetcher
        .fetch(&url("https://example.test/exact"), "example.test")
        .await
        .expect("fetch")
        .expect("body");
    assert_eq!(exact.len(), MAX_BODY);
}

#[tokio::test]
async fn a_silent_server_times_out() {
    let s = setup(|_| Reply {
        stall: true,
        ..Reply::default()
    })
    .await;
    let started = std::time::Instant::now();
    let got = s
        .fetcher
        .fetch(&url("https://example.test/slow"), "example.test")
        .await;
    assert!(got.is_err(), "{got:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// The certificate is checked: a server whose CA the client does not trust is refused
/// before any request is sent.
#[tokio::test]
async fn refuses_an_untrusted_certificate() {
    let s = setup(|_| Reply::ok(FULL)).await;
    let other_ca = pki(&["unrelated.test"]).ca;
    let overrides: Vec<(&str, SocketAddr)> = NAMES.iter().map(|n| (*n, s.server.addr)).collect();
    let distrusting = HttpsFetcher::for_tests(other_ca, &overrides, Duration::from_secs(1));
    let err = distrusting
        .fetch(&url("https://example.test/config.xml"), "example.test")
        .await
        .expect_err("untrusted");
    assert!(err.0.to_lowercase().contains("certificate"), "{err}");
    assert!(s.server.seen().is_empty(), "a request was sent");
    assert_eq!(s.server.connections(), 1);
}

/// Both places in order; the second is used when the first has nothing.
#[tokio::test]
async fn discover_tries_the_autoconfig_host_then_well_known() {
    let s = setup(|req| match (req.host(), req.target.as_str()) {
        ("example.test", "/.well-known/autoconfig/mail/config-v1.1.xml") => Reply::ok(FULL),
        _ => Reply::status(404),
    })
    .await;
    let address = EmailAddress::parse("bob@example.test").expect("address");
    let servers = discover(&s.fetcher, "example.test", &address)
        .await
        .expect("found");
    assert_eq!(servers.imap.expect("imap").host, "mail.example.test");
    let seen = s.server.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].host(), "autoconfig.example.test");
    assert_eq!(
        seen[0].target,
        "/mail/config-v1.1.xml?emailaddress=bob%40example.test"
    );
    assert_eq!(seen[1].host(), "example.test");
}

#[tokio::test]
async fn discover_ignores_unusable_files() {
    let s = setup(|req| match req.host() {
        "autoconfig.example.test" => Reply::ok("<html>parked domain</html>"),
        _ => Reply::ok(single(
            r#"<incomingServer type="imap"><hostname>x.example.test</hostname><socketType>plain</socketType></incomingServer>"#,
        )),
    })
    .await;
    let address = EmailAddress::parse("bob@example.test").expect("address");
    assert_eq!(discover(&s.fetcher, "example.test", &address).await, None);
    assert_eq!(s.server.seen().len(), 2);
}
