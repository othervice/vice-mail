//! Stage 22 (decision 0077) against the local test server (Stalwart). Ignored by default; needs
//! the server started:
//!
//! ```text
//! scripts/testmail.sh lock spam
//! scripts/testmail.sh start
//! cargo test -p mail-engine --lib spam::server_tests -- --ignored --nocapture --test-threads 1
//! scripts/testmail.sh stop; scripts/testmail.sh unlock spam
//! ```
//!
//! Alice's account (writes on) learns three offers from the admin as spam ("Spam" moves them to
//! the junk folder on the server) and three letters from Bob as ham. A fourth offer that arrives
//! by SMTP goes to the junk folder through the outbox, as one batch; "Undo" brings it back to the
//! INBOX on the server and teaches the filter that it was not spam. Every subject carries a
//! tag of this run, so mail of earlier runs does not count.

use std::sync::Arc;
use std::time::Duration;

use mail_imap::ImapConnector;
use mail_imap::test_support::{TestEnv, send_mail};
use mail_model::{
    AccountSettings, AccountState, CoreEvent, MailAction, MailboxId, MailboxRole, MessageId,
    MessageRef, Security, ServerSettings, SpamFilterMark, SpamFilterSettings,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::sync::SyncConfig;
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, ReadingServices, RecordingOpener};

const WAIT: Duration = Duration::from_secs(120);

async fn wait_event(
    events: &mut broadcast::Receiver<CoreEvent>,
    pred: impl Fn(&CoreEvent) -> bool,
) -> CoreEvent {
    let deadline = Instant::now() + WAIT;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) if pred(&event) => return event,
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(err)) => panic!("event stream: {err}"),
            Err(_) => panic!("no matching event within {WAIT:?}"),
        }
    }
}

/// The message with this subject in `mailbox`, once the sync stored it.
async fn arrived<C: crate::core::Connector>(
    core: &AppCore<C>,
    mailbox: MailboxId,
    subject: &str,
) -> MessageId {
    let deadline = Instant::now() + WAIT;
    loop {
        let found = core
            .list_messages(mailbox, None, 200)
            .await
            .expect("list")
            .items
            .into_iter()
            .find(|i| i.subject == subject);
        if let Some(item) = found {
            return item.id;
        }
        assert!(Instant::now() < deadline, "{subject:?} never arrived");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn offer(tag: &str, n: u32) -> (String, String) {
    (
        format!("CHEAP PILLS {tag} {n} - ORDER NOW"),
        format!(
            "Discount pharmacy offer {n}: cheap pills, viagra, cialis without prescription.\n\
             Order now and save money! Click http://cheap-pills.example/order?id={n}\n\
             Limited offer, act now, unsubscribe http://cheap-pills.example/remove\n"
        ),
    )
}

fn letter(tag: &str, n: u32) -> (String, String) {
    (
        format!("Hiking trip {tag} {n}"),
        format!(
            "Hi Alice, are you still coming along on Saturday? We meet at the station around\n\
             nine, bring boots and something warm. Maria brings sandwiches. Cheers, Bob {n}\n"
        ),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the test server: see the module docs"]
async fn new_spam_goes_to_junk_on_the_test_server_and_undo_brings_it_back() {
    let env = TestEnv::require();
    let dir = TempDir::new().expect("temp dir");
    let mut services = CoreServices::with_connector(
        Credentials::new(MemorySecretStore::new()),
        ImapConnector::new(),
    );
    services.sync = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        extra_root_ca: Some(env.extra_root()),
        ..SyncConfig::default()
    };
    services.reading = ReadingServices {
        opener: Arc::new(RecordingOpener::default()),
        open_dir: Some(dir.path().join("open")),
        ..ReadingServices::default()
    };
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    let mut events = core.subscribe();
    let server = |port, security| ServerSettings {
        host: env.host.clone(),
        port,
        security,
        username: env.alice.address.clone(),
    };
    let account = core
        .add_account(
            AccountSettings {
                credential: Default::default(),
                protocol: Default::default(),
                email: env.alice.address.clone(),
                display_name: "Alice".into(),
                imap: server(env.imaps_port, Security::Tls),
                smtp: server(env.submission_port, Security::Starttls),
            },
            env.alice.password.clone(),
        )
        .await
        .expect("add alice")
        .id;
    wait_event(&mut events, |e| {
        matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
    })
    .await;
    core.set_writes_enabled(account, true)
        .await
        .expect("writes");
    let mailboxes = core.list_mailboxes(account).await.expect("mailboxes");
    let role = |role| {
        mailboxes
            .iter()
            .find(|m| m.role == role)
            .map(|m| m.id)
            .unwrap_or_else(|| panic!("no {role:?} folder"))
    };
    let (inbox, junk) = (role(MailboxRole::Inbox), role(MailboxRole::Junk));
    core.set_spam_filter(SpamFilterSettings {
        enabled: true,
        excluded_accounts: Vec::new(),
    })
    .await
    .expect("on");
    core.spam().set_minimums(3, 3);
    let tag = format!("{:x}", uuid::Uuid::new_v4().as_u128() & 0xffff_ffff);

    // Three offers and three letters arrive; the user teaches them.
    let mut offers = Vec::new();
    let mut letters = Vec::new();
    for n in 1..=3 {
        let (subject, body) = offer(&tag, n);
        send_mail(&env, &env.admin, &env.alice.address, &subject, &body)
            .await
            .expect("send offer");
        offers.push(arrived(&core, inbox, &subject).await);
        let (subject, body) = letter(&tag, n);
        send_mail(&env, &env.bob, &env.alice.address, &subject, &body)
            .await
            .expect("send letter");
        letters.push(arrived(&core, inbox, &subject).await);
    }
    let refs = offers
        .iter()
        .map(|id| MessageRef {
            mailbox_id: inbox,
            message_id: *id,
        })
        .collect();
    core.apply_action(MailAction::Spam, refs, uuid::Uuid::new_v4().to_string())
        .await
        .expect("spam");
    core.mark_spam_locally(letters, false)
        .await
        .expect("not spam");
    core.spam_flush().await;
    let status = core.get_spam_filter().await.expect("status");
    println!(
        "learned: {} spam, {} ham, {} tokens",
        status.learned_spam, status.learned_ham, status.tokens
    );
    assert!(status.ready, "{status:?}");

    // A fourth offer goes to the junk folder on the server, as one batch.
    let (subject, body) = offer(&tag, 4);
    let started = Instant::now();
    send_mail(&env, &env.admin, &env.alice.address, &subject, &body)
        .await
        .expect("send offer");
    let event = wait_event(
        &mut events,
        |e| matches!(e, CoreEvent::SpamFiltered { account_id, .. } if *account_id == account),
    )
    .await;
    let CoreEvent::SpamFiltered {
        moved,
        marked,
        batch,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!((moved, marked), (1, 0));
    let in_junk = arrived(&core, junk, &subject).await;
    println!(
        "arrived and in Junk on the server after {} ms",
        started.elapsed().as_millis()
    );
    let row = core
        .list_messages(junk, None, 200)
        .await
        .expect("junk")
        .items
        .into_iter()
        .find(|i| i.id == in_junk)
        .expect("row");
    assert_eq!(row.spam, Some(SpamFilterMark::Moved));
    assert!(
        !core
            .list_messages(inbox, None, 200)
            .await
            .expect("inbox")
            .items
            .iter()
            .any(|i| i.subject == subject),
        "not in the inbox"
    );

    // "Undo" brings it back on the server and teaches the filter it is ham.
    let batch = batch.expect("batch");
    let deadline = Instant::now() + WAIT;
    loop {
        match core.undo_action(batch).await {
            Ok(_) => break,
            Err(err) if Instant::now() < deadline => {
                tracing::debug!(code = ?err.code(), "undo not yet");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(err) => panic!("undo: {err}"),
        }
    }
    arrived(&core, inbox, &subject).await;
    core.spam_flush().await;
    assert_eq!(
        core.store_for_tests()
            .spam_learned_class(in_junk)
            .await
            .expect("class"),
        Some(mail_store::SpamClass::Ham),
        "learned as ham on undo"
    );
    assert!(
        core.get_spam_verdict(in_junk)
            .await
            .expect("verdict")
            .is_none()
    );
    // Learning three spam mails topped up ham from read inbox mail (two per spam), if there was any.
    let after = core.get_spam_filter().await.expect("status");
    println!(
        "after undo: {} spam, {} ham learned",
        after.learned_spam, after.learned_ham
    );
    core.shutdown().await;
}
