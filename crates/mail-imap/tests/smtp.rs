//! SMTP against the local test server (Stalwart, `scripts/testmail.sh`): bob sends to alice over
//! submission with STARTTLS (1587) and implicit TLS (1465), alice's INBOX is read back over IMAP.
//! Ignored by default; run with the server started (a seed is not needed):
//!
//! ```text
//! scripts/testmail.sh lock NAME && scripts/testmail.sh start
//! cargo test -p mail-imap --test smtp -- --include-ignored
//! scripts/testmail.sh stop && scripts/testmail.sh unlock NAME
//! ```
//!
//! Every message has a Message-ID of its own: Stalwart silently drops a second delivery with
//! the same Message-ID (decision 0001), so a test could never see a double delivery otherwise.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mail_content::{
    Attachment, BuiltMessage, OutgoingMessage, attachment_bytes, build_message, new_message_id,
    parse,
};
use mail_imap::test_support::{Proxy, TestAccount, TestEnv};
use mail_imap::{ImapConnector, SmtpSender, WireTap};
use mail_model::{
    Access, Address, BackendError, MailBackend, MailConnector, Secret, Security, SendOutcome,
    SmtpStep, UidSet,
};

const SECURITIES: [Security; 2] = [Security::Starttls, Security::Tls];

/// 128 bits that differ per call and per test run (no randomness crate needed for tests).
fn unique() -> [u8; 16] {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&(nanos as u64).to_be_bytes());
    bytes[8..12].copy_from_slice(&std::process::id().to_be_bytes());
    bytes[12..].copy_from_slice(&(n as u32).to_be_bytes());
    bytes
}

fn named(name: &str, email: &str) -> Address {
    Address {
        name: Some(name.to_owned()),
        email: email.to_owned(),
    }
}

fn message(env: &TestEnv, subject: &str) -> OutgoingMessage {
    OutgoingMessage {
        from: named("Bob Bäcker 🍞", &env.bob.address),
        to: vec![named("Alice Ärger", &env.alice.address)],
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        subject: subject.to_owned(),
        text: None,
        html: Some("<p>Hallo <strong>Alice</strong>, schöne Grüße 👋</p>".to_owned()),
        attachments: Vec::new(),
        message_id: new_message_id(&env.bob.address, unique()),
        in_reply_to: None,
        references: Vec::new(),
        date: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
            .unwrap_or(0),
        user_agent: None,
    }
}

fn message_id(message: &OutgoingMessage) -> String {
    message.message_id.clone()
}

/// Raw copies of the message with `id` in `account`'s INBOX. Waits up to `wait` for at least
/// one, then a little longer for a possible second one.
async fn delivered(env: &TestEnv, account: &TestAccount, id: &str, wait: Duration) -> Vec<Vec<u8>> {
    let config = env.config(account, Security::Tls, Access::ReadOnly);
    let mut imap = ImapConnector::new()
        .connect(&config)
        .await
        .unwrap_or_else(|err| panic!("IMAP as {}: {err}", account.address));
    let start = Instant::now();
    let mut extra_wait = None;
    loop {
        let inbox = imap.open_mailbox("INBOX").await.expect("INBOX");
        let all = imap.search_uids(&inbox).await.expect("search");
        // The newest 200 messages are enough: the test's message just arrived.
        let window = UidSet::from_uids(all.iter().rev().take(200));
        let headers = if window.is_empty() {
            Vec::new()
        } else {
            imap.fetch_headers(&inbox, &window).await.expect("headers")
        };
        let matching = UidSet::from_uids(
            headers
                .iter()
                .filter(|h| h.message_id.as_deref() == Some(id))
                .map(|h| h.uid),
        );
        if !matching.is_empty() && extra_wait.is_none() {
            extra_wait = Some(Instant::now());
        }
        let done = extra_wait.is_some_and(|t| t.elapsed() > Duration::from_millis(500))
            || start.elapsed() > wait;
        if done {
            let raws = if matching.is_empty() {
                Vec::new()
            } else {
                imap.fetch_messages(&inbox, &matching)
                    .await
                    .expect("messages")
                    .into_iter()
                    .map(|m| m.data)
                    .collect()
            };
            imap.logout().await.ok();
            return raws;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn send(
    env: &TestEnv,
    security: Security,
    built: &BuiltMessage,
) -> Result<SendOutcome, BackendError> {
    SmtpSender::new()
        .send(
            &env.smtp_config(&env.bob, security),
            &built.envelope,
            &built.for_sending,
        )
        .await
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn arrives_with_attachment_umlauts_and_emoji_over_starttls_and_tls() {
    let env = TestEnv::require();
    let binary: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let text_file = "Prüfprotokoll\r\nZeile 2: ÄÖÜ äöü ß €\r\n"
        .as_bytes()
        .to_vec();
    for security in SECURITIES {
        let mut message = message(
            &env,
            &format!("Prüfbericht für Jörg 🧪 über {security:?} – „Anführung“ und Emoji 🎉"),
        );
        message.cc = vec![named("Zoë Ångström", &env.alice.address)];
        message.attachments = vec![
            Attachment {
                file_name: "Messwerte Größe 🧪 März.bin".to_owned(),
                mime: "application/octet-stream".to_owned(),
                bytes: binary.clone(),
            },
            Attachment {
                file_name: "Protokoll Übersicht.txt".to_owned(),
                mime: "text/plain".to_owned(),
                bytes: text_file.clone(),
            },
        ];
        let built = build_message(&message).expect("build");
        let outcome = send(&env, security, &built).await.expect("sent");
        let SendOutcome::Accepted(reply) = &outcome else {
            panic!("{security:?}: {outcome:?}");
        };
        println!("{security:?}: accepted: {reply}");

        let copies = delivered(
            &env,
            &env.alice,
            &message_id(&message),
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(copies.len(), 1, "{security:?}: exactly one delivery");
        let raw = &copies[0];
        let parsed = parse(raw);
        let h = &parsed.headers;
        assert_eq!(h.subject.as_deref(), Some(message.subject.as_str()));
        assert_eq!(h.from, vec![named("Bob Bäcker 🍞", &env.bob.address)]);
        assert_eq!(h.to, vec![named("Alice Ärger", &env.alice.address)]);
        assert!(h.cc.is_empty(), "alice once, as To");
        let html = parsed.html.as_deref().expect("html");
        assert!(html.contains("schöne Grüße 👋"), "{html}");
        assert!(
            parsed
                .text
                .as_deref()
                .is_some_and(|t| t.contains("schöne Grüße 👋"))
        );
        let names: Vec<&str> = parsed
            .attachments
            .iter()
            .map(|a| a.file_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Messwerte Größe 🧪 März.bin", "Protokoll Übersicht.txt"]
        );
        let got = attachment_bytes(raw, parsed.attachments[0].part_id).expect("bytes");
        assert_eq!(got.len(), binary.len());
        assert_eq!(fnv(&got), fnv(&binary), "same checksum");
        assert_eq!(got, binary);
        assert_eq!(
            attachment_bytes(raw, parsed.attachments[1].part_id).as_deref(),
            Some(text_file.as_slice())
        );
        // Nothing names the program.
        let text = String::from_utf8_lossy(raw).to_ascii_lowercase();
        let head = text.split("\r\n\r\n").next().unwrap_or_default();
        assert!(
            !head.contains("user-agent") && !head.contains("x-mailer"),
            "{head}"
        );
    }
}

/// FNV-1a 64 as a checksum for the log.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn bcc_recipient_gets_the_mail_and_stays_unnamed() {
    let env = TestEnv::require();
    let mut message = message(&env, "Blindkopie-Test");
    message.bcc = vec![named("Geheime Admina", &env.admin.address)];
    let built = build_message(&message).expect("build");
    assert!(
        built
            .envelope
            .recipients
            .iter()
            .any(|r| r == &env.admin.address)
    );
    let outcome = send(&env, Security::Tls, &built).await.expect("sent");
    assert!(matches!(outcome, SendOutcome::Accepted(_)), "{outcome:?}");

    let id = message_id(&message);
    for account in [&env.alice, &env.admin] {
        let copies = delivered(&env, account, &id, Duration::from_secs(10)).await;
        assert_eq!(copies.len(), 1, "{}: one delivery", account.address);
        let raw = String::from_utf8_lossy(&copies[0]).to_string();
        let head = raw.split("\r\n\r\n").next().unwrap_or_default().to_owned();
        let head_lower = head.to_ascii_lowercase();
        assert!(!head_lower.contains("bcc"), "{}: {head}", account.address);
        assert!(
            !head.contains("Geheime Admina"),
            "{}: {head}",
            account.address
        );
        // The server adds trace fields (Received, Delivered-To) for the mailbox it delivers to;
        // in alice's copy the blind recipient appears nowhere.
        if account.address == env.alice.address {
            assert!(!head_lower.contains(&env.admin.address), "{head}");
        }
    }
    // The Sent copy keeps the blind copy.
    assert!(
        parse(&built.for_sent_folder)
            .headers
            .bcc
            .iter()
            .any(|a| a.email == env.admin.address)
    );
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn wrong_password_and_missing_ca_are_typed() {
    let env = TestEnv::require();
    for security in SECURITIES {
        let mut config = env.smtp_config(&env.bob, security);
        config.password = Secret::new("wrong-password".to_owned());
        let err = SmtpSender::new()
            .verify(&config)
            .await
            .expect_err("wrong password");
        assert!(
            matches!(err, BackendError::AuthFailed(_)),
            "{security:?}: {err:?}"
        );
        assert!(!err.is_transient());
        assert!(!format!("{err:?}").contains("wrong-password"));

        let mut config = env.smtp_config(&env.bob, security);
        config.extra_root_ca = None;
        let err = SmtpSender::new()
            .verify(&config)
            .await
            .expect_err("untrusted");
        assert!(
            matches!(err, BackendError::CertUntrusted(_)),
            "{security:?}: {err:?}"
        );

        SmtpSender::new()
            .verify(&env.smtp_config(&env.bob, security))
            .await
            .unwrap_or_else(|err| panic!("{security:?}: {err}"));
    }
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn refused_recipient_sends_to_nobody() {
    let env = TestEnv::require();
    let mut message = message(&env, "Unbekannter Empfänger");
    message
        .to
        .push(named("Niemand", "nobody-here@vicemail.test"));
    let built = build_message(&message).expect("build");
    let outcome = send(&env, Security::Starttls, &built)
        .await
        .expect("answered");
    let SendOutcome::Rejected(rejection) = &outcome else {
        panic!("{outcome:?}");
    };
    println!("refused: {:?}", rejection);
    assert_eq!(rejection.step, SmtpStep::Recipients);
    assert_eq!(rejection.recipients.len(), 1);
    assert_eq!(rejection.recipients[0].address, "nobody-here@vicemail.test");
    assert!(rejection.is_permanent());
    let copies = delivered(
        &env,
        &env.alice,
        &message_id(&message),
        Duration::from_secs(2),
    )
    .await;
    assert!(
        copies.is_empty(),
        "the accepted recipient got nothing either"
    );
}

async fn proxy_to(env: &TestEnv, security: Security) -> Arc<Proxy> {
    let upstream: SocketAddr = format!("{}:{}", env.host, env.smtp_port(security))
        .parse()
        .expect("addr");
    Proxy::start(upstream).await.expect("proxy")
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn cut_after_the_end_of_data_is_unclear_and_never_doubled() {
    let env = TestEnv::require();
    for security in SECURITIES {
        let proxy = proxy_to(&env, security).await;
        let tap = WireTap::new();
        let cut = Arc::clone(&proxy);
        // The end-of-data marker goes out (dot-stuffing makes it unique); the server gets it,
        // its reply never reaches the client.
        tap.on_write("\r\n.\r\n", move || {
            cut.cut_responses(Duration::from_millis(500))
        });
        let mut message = message(&env, &format!("Abbruch nach dem Ende ({security:?})"));
        message.attachments = vec![Attachment {
            file_name: "daten.bin".into(),
            mime: "application/octet-stream".into(),
            bytes: vec![0x5a; 200_000],
        }];
        let built = build_message(&message).expect("build");
        let config = env.config_at(
            &env.bob,
            "127.0.0.1",
            proxy.port(),
            security,
            Access::ReadWrite,
        );
        let started = Instant::now();
        let outcome = SmtpSender::new()
            .with_wire_tap(tap)
            .send(&config, &built.envelope, &built.for_sending)
            .await
            .expect("an outcome");
        let SendOutcome::Unclear(detail) = &outcome else {
            panic!("{security:?}: {outcome:?}");
        };
        println!(
            "{security:?}: unclear after {:?}: {detail}",
            started.elapsed()
        );
        assert_eq!(proxy.cuts(), 1);
        assert_eq!(
            proxy.accepted(),
            1,
            "the sender never tries again by itself"
        );

        let copies = delivered(
            &env,
            &env.alice,
            &message_id(&message),
            Duration::from_secs(5),
        )
        .await;
        println!("{security:?}: {} message(s) in alice's INBOX", copies.len());
        assert!(copies.len() <= 1, "never two");
        assert_eq!(copies.len(), 1, "the server had the whole message");
    }
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn cut_during_the_data_is_not_sent() {
    let env = TestEnv::require();
    for security in SECURITIES {
        let proxy = proxy_to(&env, security).await;
        let tap = WireTap::new();
        let cut = Arc::clone(&proxy);
        let marker = format!("CUT-HERE-{:x}", fnv(&unique()));
        tap.on_write(&marker, move || cut.cut_responses(Duration::ZERO));
        let mut message = message(&env, &format!("Abbruch mitten in den Daten ({security:?})"));
        message.text = Some(format!("{marker}\nThe rest follows in the attachment."));
        message.html = None;
        message.attachments = vec![Attachment {
            file_name: "gross.bin".into(),
            mime: "application/octet-stream".into(),
            bytes: (0..6_000_000u32).map(|i| (i % 251) as u8).collect(),
        }];
        let built = build_message(&message).expect("build");
        let config = env.config_at(
            &env.bob,
            "127.0.0.1",
            proxy.port(),
            security,
            Access::ReadWrite,
        );
        let err = SmtpSender::new()
            .with_wire_tap(tap)
            .send(&config, &built.envelope, &built.for_sending)
            .await
            .expect_err("not sent");
        println!("{security:?}: {err:?}");
        assert!(
            matches!(err, BackendError::ConnectionLost(_)),
            "{security:?}: {err:?}"
        );
        assert!(err.is_transient());
        assert_eq!(proxy.accepted(), 1);
        let copies = delivered(
            &env,
            &env.alice,
            &message_id(&message),
            Duration::from_secs(3),
        )
        .await;
        assert!(copies.is_empty(), "{security:?}: nothing delivered");
    }
}
