//! DNS SRV records for mail services (RFC 6186, with `_submissions` from RFC 8314).
//!
//! IMAP: `_imaps._tcp` (TLS) and `_imap._tcp` (STARTTLS); SMTP submission: `_submissions._tcp`
//! (TLS) and `_submission._tcp` (STARTTLS). The four names are queried at once.
//!
//! Choice per protocol, across both services: the lowest priority wins (RFC 6186 section 3.4);
//! on equal priority implicit TLS wins (RFC 8314 prefers it); then the highest weight (the
//! most likely pick of RFC 2782's weighted choice, made deterministic because the result fills
//! a form once); then the target name. A target of `.` means "service not offered" (RFC 2782)
//! and is skipped, as are port 0 and implausible host names. The login name is the full
//! address: SRV records do not say otherwise.

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::config::{
    ConnectionConfig, NameServerConfig, ResolveHosts, ResolverConfig, ResolverOpts,
};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::rr::RData;
use mail_model::{Security, ServerSettings};

use super::{EmailAddress, LookupError, Servers, error_chain, is_host_name};

/// Time for all four SRV queries together.
pub const SRV_TIMEOUT: Duration = Duration::from_secs(6);
/// Per query and name server, as configured in the resolver.
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const QUERY_ATTEMPTS: usize = 2;

/// One SRV record. `target` has no trailing dot; it is empty for the root (`.`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub target: String,
}

/// Looks up SRV records. Injectable so tests need no network.
pub trait SrvResolver: Send + Sync {
    /// Records for the fully qualified `name`; empty if the name has none (NXDOMAIN, no data).
    fn lookup_srv(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Vec<SrvRecord>, LookupError>> + Send;
}

/// Resolver with the system's DNS servers (macOS: SystemConfiguration and `/etc/resolv.conf`;
/// Windows: the network adapters' settings). Ignores the hosts file and caches nothing beyond
/// its own lifetime.
#[derive(Clone)]
pub struct SystemSrvResolver {
    resolver: Result<TokioResolver, LookupError>,
}

impl std::fmt::Debug for SystemSrvResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemSrvResolver")
            .field("ready", &self.resolver.is_ok())
            .finish()
    }
}

impl SystemSrvResolver {
    /// Reads the system's DNS configuration. Never fails: if it cannot be read, every lookup
    /// returns that error (and discovery falls back to the next step).
    pub fn new() -> Self {
        let resolver = TokioResolver::builder_tokio()
            .map_err(|err| LookupError(format!("system DNS configuration: {}", error_chain(&err))))
            .and_then(|mut builder| {
                configure(builder.options_mut());
                builder
                    .build()
                    .map_err(|err| LookupError(format!("DNS resolver: {}", error_chain(&err))))
            });
        Self { resolver }
    }

    /// A resolver that asks only the given name server over UDP (tests use a local one).
    pub fn with_name_server(addr: SocketAddr) -> Self {
        let mut udp = ConnectionConfig::udp();
        udp.port = addr.port();
        let server = NameServerConfig::new(addr.ip(), true, vec![udp]);
        let config = ResolverConfig::from_parts(None, Vec::new(), vec![server]);
        let mut builder =
            TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
        configure(builder.options_mut());
        let resolver = builder
            .build()
            .map_err(|err| LookupError(format!("DNS resolver: {}", error_chain(&err))));
        Self { resolver }
    }
}

impl Default for SystemSrvResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn configure(options: &mut ResolverOpts) {
    options.timeout = QUERY_TIMEOUT;
    options.attempts = QUERY_ATTEMPTS;
    options.use_hosts_file = ResolveHosts::Never;
}

impl SrvResolver for SystemSrvResolver {
    async fn lookup_srv(&self, name: &str) -> Result<Vec<SrvRecord>, LookupError> {
        let resolver = self.resolver.as_ref().map_err(Clone::clone)?;
        match resolver.srv_lookup(name).await {
            Ok(lookup) => Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::SRV(srv) => Some(SrvRecord {
                        priority: srv.priority,
                        weight: srv.weight,
                        port: srv.port,
                        target: srv
                            .target
                            .to_ascii()
                            .trim_end_matches('.')
                            .to_ascii_lowercase(),
                    }),
                    _ => None, // CNAMEs of the chain
                })
                .collect()),
            Err(err) if err.is_no_records_found() => Ok(Vec::new()),
            Err(err) => Err(LookupError(error_chain(&err))),
        }
    }
}

/// The four services, in query order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Imaps,
    Imap,
    Submissions,
    Submission,
}

impl Service {
    const fn label(self) -> &'static str {
        match self {
            Self::Imaps => "_imaps._tcp",
            Self::Imap => "_imap._tcp",
            Self::Submissions => "_submissions._tcp",
            Self::Submission => "_submission._tcp",
        }
    }

    const fn security(self) -> Security {
        match self {
            Self::Imaps | Self::Submissions => Security::Tls,
            Self::Imap | Self::Submission => Security::Starttls,
        }
    }
}

/// Queries the four SRV names of `domain` (ASCII) and picks one server per protocol.
/// Failures are logged and count as "no record".
pub async fn discover<R: SrvResolver>(
    resolver: &R,
    domain: &str,
    address: &EmailAddress,
) -> Servers {
    let query = |service: Service| async move {
        // Fully qualified, so no search domain of the system is appended.
        let name = format!("{}.{domain}.", service.label());
        match resolver.lookup_srv(&name).await {
            Ok(records) => records,
            Err(err) => {
                tracing::debug!(domain, service = service.label(), error = %err, "SRV lookup failed");
                Vec::new()
            }
        }
    };
    let all = async {
        tokio::join!(
            query(Service::Imaps),
            query(Service::Imap),
            query(Service::Submissions),
            query(Service::Submission),
        )
    };
    let Ok((imaps, imap, submissions, submission)) = tokio::time::timeout(SRV_TIMEOUT, all).await
    else {
        tracing::debug!(domain, "SRV lookups timed out");
        return Servers::default();
    };
    Servers {
        imap: choose([(Service::Imaps, imaps), (Service::Imap, imap)], address),
        smtp: choose(
            [
                (Service::Submissions, submissions),
                (Service::Submission, submission),
            ],
            address,
        ),
    }
}

/// The best usable record of the two services of one protocol (rules in the module docs).
fn choose(
    services: [(Service, Vec<SrvRecord>); 2],
    address: &EmailAddress,
) -> Option<ServerSettings> {
    let mut candidates: Vec<(Service, SrvRecord)> = services
        .into_iter()
        .flat_map(|(service, records)| records.into_iter().map(move |r| (service, r)))
        .filter(|(_, r)| r.port != 0 && !r.target.is_empty() && is_host_name(&r.target))
        .collect();
    candidates.sort_by(|(sa, a), (sb, b)| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| tls_first(sa.security()).cmp(&tls_first(sb.security())))
            .then_with(|| b.weight.cmp(&a.weight))
            .then_with(|| a.target.cmp(&b.target))
    });
    candidates
        .into_iter()
        .next()
        .map(|(service, record)| ServerSettings {
            host: record.target,
            port: record.port,
            security: service.security(),
            username: address.address().to_owned(),
        })
}

const fn tls_first(security: Security) -> u8 {
    match security {
        Security::Tls => 0,
        Security::Starttls => 1,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, PoisonError};

    use hickory_resolver::proto::op::{Message, MessageType, ResponseCode};
    use hickory_resolver::proto::rr::rdata::SRV;
    use hickory_resolver::proto::rr::{Name, RData, Record, RecordType};
    use tokio::net::UdpSocket;

    use super::*;

    fn record(priority: u16, weight: u16, port: u16, target: &str) -> SrvRecord {
        SrvRecord {
            priority,
            weight,
            port,
            target: target.into(),
        }
    }

    fn address() -> EmailAddress {
        EmailAddress::parse("alice@example.org").expect("address")
    }

    /// Answers from a table; names without an entry have no records. Records every query.
    #[derive(Default)]
    struct FakeResolver {
        answers: HashMap<String, Result<Vec<SrvRecord>, LookupError>>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeResolver {
        fn with(mut self, name: &str, answer: Result<Vec<SrvRecord>, LookupError>) -> Self {
            self.answers.insert(name.into(), answer);
            self
        }
    }

    impl SrvResolver for FakeResolver {
        async fn lookup_srv(&self, name: &str) -> Result<Vec<SrvRecord>, LookupError> {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(name.to_owned());
            self.answers.get(name).cloned().unwrap_or(Ok(Vec::new()))
        }
    }

    #[tokio::test]
    async fn queries_the_four_fully_qualified_names() {
        let resolver = FakeResolver::default();
        let found = discover(&resolver, "example.org", &address()).await;
        assert_eq!(found, Servers::default());
        let mut asked = resolver
            .asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        asked.sort();
        assert_eq!(
            asked,
            [
                "_imap._tcp.example.org.",
                "_imaps._tcp.example.org.",
                "_submission._tcp.example.org.",
                "_submissions._tcp.example.org.",
            ]
        );
    }

    #[tokio::test]
    async fn maps_services_to_security_and_uses_the_full_address() {
        let resolver = FakeResolver::default()
            .with(
                "_imaps._tcp.example.org.",
                Ok(vec![record(0, 1, 993, "imap.example.org")]),
            )
            .with(
                "_submission._tcp.example.org.",
                Ok(vec![record(0, 1, 587, "smtp.example.org")]),
            );
        let found = discover(&resolver, "example.org", &address()).await;
        assert_eq!(
            found.imap,
            Some(ServerSettings {
                host: "imap.example.org".into(),
                port: 993,
                security: Security::Tls,
                username: "alice@example.org".into(),
            })
        );
        let smtp = found.smtp.expect("smtp");
        assert_eq!(
            (smtp.host.as_str(), smtp.port, smtp.security),
            ("smtp.example.org", 587, Security::Starttls)
        );
    }

    #[test]
    fn priority_decides_across_services_then_tls_then_weight() {
        let a = address();
        let pick = |tls: Vec<SrvRecord>, starttls: Vec<SrvRecord>| {
            choose([(Service::Imaps, tls), (Service::Imap, starttls)], &a)
                .map(|s| (s.host, s.port, s.security))
        };
        // Lower priority wins even if it is STARTTLS.
        assert_eq!(
            pick(
                vec![record(20, 0, 993, "tls.example.org")],
                vec![record(10, 0, 143, "plain.example.org")]
            ),
            Some(("plain.example.org".into(), 143, Security::Starttls))
        );
        // Equal priority: implicit TLS.
        assert_eq!(
            pick(
                vec![record(10, 0, 993, "tls.example.org")],
                vec![record(10, 50, 143, "plain.example.org")]
            ),
            Some(("tls.example.org".into(), 993, Security::Tls))
        );
        // Same service and priority: highest weight, then name.
        assert_eq!(
            pick(
                vec![
                    record(5, 10, 993, "b.example.org"),
                    record(5, 60, 993, "c.example.org"),
                    record(1, 0, 993, "a.example.org"),
                ],
                vec![]
            ),
            Some(("a.example.org".into(), 993, Security::Tls))
        );
        assert_eq!(
            pick(
                vec![
                    record(5, 10, 993, "b.example.org"),
                    record(5, 60, 993, "c.example.org")
                ],
                vec![]
            ),
            Some(("c.example.org".into(), 993, Security::Tls))
        );
        assert_eq!(
            pick(
                vec![
                    record(5, 10, 993, "y.example.org"),
                    record(5, 10, 993, "x.example.org")
                ],
                vec![]
            ),
            Some(("x.example.org".into(), 993, Security::Tls))
        );
    }

    #[test]
    fn root_target_port_zero_and_bad_names_mean_not_offered() {
        let a = address();
        let pick = |tls: Vec<SrvRecord>, starttls: Vec<SrvRecord>| {
            choose([(Service::Imaps, tls), (Service::Imap, starttls)], &a).map(|s| s.host)
        };
        assert_eq!(pick(vec![record(0, 0, 0, "")], vec![]), None);
        assert_eq!(
            pick(
                vec![record(0, 0, 0, "")],
                vec![record(10, 0, 143, "imap.example.org")]
            ),
            Some("imap.example.org".into()),
            "'.' for _imaps does not hide _imap"
        );
        assert_eq!(
            pick(vec![record(0, 0, 0, "imap.example.org")], vec![]),
            None
        );
        assert_eq!(
            pick(vec![record(0, 0, 993, "bad host.example.org")], vec![]),
            None
        );
        assert_eq!(
            pick(vec![record(0, 0, 993, "evil.example.org/x")], vec![]),
            None
        );
    }

    #[tokio::test]
    async fn a_failing_lookup_counts_as_no_record() {
        let resolver = FakeResolver::default()
            .with(
                "_imaps._tcp.example.org.",
                Err(LookupError("timed out".into())),
            )
            .with(
                "_imap._tcp.example.org.",
                Ok(vec![record(0, 0, 143, "imap.example.org")]),
            );
        let found = discover(&resolver, "example.org", &address()).await;
        assert_eq!(found.imap.map(|s| s.port), Some(143));
        assert_eq!(found.smtp, None);
    }

    /// A tiny DNS server on 127.0.0.1: answers SRV queries from a table, NXDOMAIN otherwise.
    async fn dns_server(zone: HashMap<&'static str, Vec<SRV>>) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr = socket.local_addr().expect("addr");
        let zone = Arc::new(zone);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                    return;
                };
                let Some(request) = buf.get(..len).and_then(|b| Message::from_vec(b).ok()) else {
                    continue;
                };
                let mut response = Message::new(
                    request.metadata.id,
                    MessageType::Response,
                    request.metadata.op_code,
                );
                response.metadata.recursion_desired = request.metadata.recursion_desired;
                response.metadata.recursion_available = true;
                response.add_queries(request.queries.clone());
                let query = request.queries.first();
                let name = query.map(|q| q.name().to_ascii()).unwrap_or_default();
                match zone.get(name.as_str()) {
                    Some(records) if query.is_some_and(|q| q.query_type() == RecordType::SRV) => {
                        for srv in records {
                            let owner = Name::from_ascii(&name).expect("name");
                            response.add_answer(Record::from_rdata(
                                owner,
                                60,
                                RData::SRV(srv.clone()),
                            ));
                        }
                    }
                    _ => response.metadata.response_code = ResponseCode::NXDomain,
                }
                if let Ok(bytes) = response.to_vec() {
                    let _ = socket.send_to(&bytes, peer).await;
                }
            }
        });
        addr
    }

    /// The real resolver code against a local DNS server: records are mapped (trailing dot
    /// removed, lowercased), NXDOMAIN is "no records", a `.` target is skipped.
    #[tokio::test]
    async fn system_resolver_maps_real_dns_answers() {
        let name = |s: &str| Name::from_ascii(s).expect("name");
        let zone = HashMap::from([
            (
                "_imaps._tcp.example.org.",
                vec![
                    SRV::new(10, 5, 993, name("IMAP.Example.org.")),
                    SRV::new(20, 5, 993, name("backup.example.org.")),
                ],
            ),
            (
                "_submissions._tcp.example.org.",
                vec![SRV::new(0, 0, 0, Name::root())],
            ),
            (
                "_submission._tcp.example.org.",
                vec![SRV::new(0, 1, 587, name("smtp.example.org."))],
            ),
        ]);
        let resolver = SystemSrvResolver::with_name_server(dns_server(zone).await);

        let records = resolver
            .lookup_srv("_imaps._tcp.example.org.")
            .await
            .expect("lookup");
        assert_eq!(
            records,
            vec![
                record(10, 5, 993, "imap.example.org"),
                record(20, 5, 993, "backup.example.org")
            ]
        );
        assert_eq!(
            resolver
                .lookup_srv("_imap._tcp.example.org.")
                .await
                .expect("nxdomain"),
            vec![]
        );
        assert_eq!(
            resolver
                .lookup_srv("_submissions._tcp.example.org.")
                .await
                .expect("root"),
            vec![record(0, 0, 0, "")]
        );

        let found = discover(&resolver, "example.org", &address()).await;
        let imap = found.imap.expect("imap");
        assert_eq!(
            (imap.host.as_str(), imap.port, imap.security),
            ("imap.example.org", 993, Security::Tls)
        );
        let smtp = found.smtp.expect("smtp");
        assert_eq!(
            (smtp.host.as_str(), smtp.port, smtp.security),
            ("smtp.example.org", 587, Security::Starttls)
        );
    }

    /// A name server that never answers: discovery gives up within the time limit.
    #[tokio::test]
    async fn silent_name_server_ends_in_nothing_found() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let resolver = SystemSrvResolver::with_name_server(socket.local_addr().expect("addr"));
        let started = std::time::Instant::now();
        let found = discover(&resolver, "example.org", &address()).await;
        assert_eq!(found, Servers::default());
        assert!(
            started.elapsed() <= SRV_TIMEOUT + Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        drop(socket);
    }
}
