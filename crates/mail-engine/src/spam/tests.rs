//! Stage 22 (decision 0077): the learning spam filter through `AppCore` with the fake server
//! (a child module of the stage 9 tests, for their harness): learning from "Spam" and
//! "Not spam" and unlearning on undo, new spam moved to junk through the outbox without a
//! notification and learned as ham on undo, the rules that keep mail from ever being spam,
//! marks on read-only accounts, learning from existing mail, forgetting, and the setting.

use std::collections::BTreeMap;

use mail_model::{
    ActionResult, CoreEvent, ErrorCode, IpcError, MailAction, MailboxRole, MessageRef,
    SpamFilterMark, SpamFilterSettings, SpamFilterStatus, SpamLearningState,
};

use super::*;
use crate::sync::fake::FakeMailbox;

fn add_junk(server: &FakeServer) {
    server.with(|s| {
        s.mailboxes.insert(
            "Junk".to_owned(),
            FakeMailbox {
                uid_validity: 900,
                uid_next: 1,
                modseq: 1,
                attributes: vec!["\\Junk".to_owned()],
                messages: BTreeMap::new(),
                changes: 0,
            },
        );
    });
}

fn spam_mail(n: u32) -> Vec<u8> {
    format!(
        "From: Best Deals <deals{n}@cheap-pills.example>\r\nTo: undisclosed-recipients:;\r\n\
         Subject: CHEAP PILLS {n} - ORDER NOW!!!\r\nMessage-ID: <deal-{n}@cheap-pills.example>\r\n\
         Content-Type: text/plain\r\n\r\n\
         Discount pharmacy offer {n}: cheap pills, viagra, cialis without prescription. \
         Order now and save money! Click http://cheap-pills.example/order?id={n} \
         Limited offer, act now, unsubscribe http://cheap-pills.example/remove\r\n"
    )
    .into_bytes()
}

fn ham_mail(n: u32) -> Vec<u8> {
    format!(
        "From: Bob Example <bob@friends.example>\r\nTo: alice@fake.test\r\n\
         Subject: Hiking trip {n}\r\nMessage-ID: <hike-{n}@friends.example>\r\n\
         Content-Type: text/plain\r\n\r\n\
         Hi Alice, are you still coming along on Saturday? We meet at the station around \
         nine, bring boots and something warm. Maria brings sandwiches. Cheers, Bob {n}\r\n"
    )
    .into_bytes()
}

fn at(mailbox: MailboxId, item: &MessageListItem) -> MessageRef {
    MessageRef {
        mailbox_id: mailbox,
        message_id: item.id,
    }
}

async fn status(h: &Harness) -> SpamFilterStatus {
    h.core.spam_flush().await;
    h.core.get_spam_filter().await.expect("status")
}

async fn enable(h: &Harness) {
    h.core
        .set_spam_filter(SpamFilterSettings {
            enabled: true,
            excluded_accounts: Vec::new(),
        })
        .await
        .expect("on");
}

/// Teaches `spam` and `ham` messages (delivered to the inbox, their bodies stored as after
/// opening or prefetching) with the user's verbs.
async fn teach(h: &mut Harness, spam: std::ops::Range<u32>, ham: std::ops::Range<u32>) {
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    let mut refs = Vec::new();
    for n in spam {
        refs.push(at(inbox, &h.deliver(&spam_mail(n)).await));
    }
    let mut hams = Vec::new();
    for n in ham {
        hams.push(h.deliver(&ham_mail(n)).await.id);
    }
    for id in refs
        .iter()
        .map(|r| r.message_id)
        .chain(hams.iter().copied())
    {
        h.core
            .spam_tokens_of(id, crate::reading::SpamFetch::Notify)
            .await
            .expect("body");
    }
    if !refs.is_empty() {
        h.core
            .apply_action(MailAction::Spam, refs, uuid::Uuid::new_v4().to_string())
            .await
            .expect("spam");
    }
    if !hams.is_empty() {
        h.core
            .mark_spam_locally(hams, false)
            .await
            .expect("not spam");
    }
    h.core.spam_flush().await;
}

/// "Undo", once the move can be undone (the target folder was synced).
async fn undo(h: &Harness, batch: Option<mail_model::BatchId>) {
    let batch = batch.expect("batch");
    let deadline = Instant::now() + WAIT;
    loop {
        match h.core.undo_action(batch).await {
            Ok(_) => return,
            Err(err) if Instant::now() < deadline && err.code() == ErrorCode::NotUndoable => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(err) => panic!("undo: {err}"),
        }
    }
}

fn code(err: mail_model::CoreError) -> ErrorCode {
    IpcError::from(err).code
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spam_and_not_spam_teach_the_filter_and_undo_unlearns() {
    let server = FakeServer::new();
    add_junk(&server);
    let mut h = Harness::new(server).await;
    h.writes(true).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;

    // Off (the default): the actions teach nothing.
    assert!(!status(&h).await.settings.enabled);
    let first = h.deliver(&spam_mail(1)).await;
    h.core
        .apply_action(MailAction::Spam, vec![at(inbox, &first)], "s-off".into())
        .await
        .expect("spam");
    assert_eq!(status(&h).await.learned_spam, 0);

    enable(&h).await;
    let second = h.deliver(&spam_mail(2)).await;
    let ActionResult { batch, .. } = h
        .core
        .apply_action(MailAction::Spam, vec![at(inbox, &second)], "s-on".into())
        .await
        .expect("spam");
    let learned = status(&h).await;
    assert_eq!((learned.learned_spam, learned.learned_ham), (1, 0));
    // Learned from the stored body, or (not fetched yet) from the stored header data.
    assert!(learned.tokens > 10, "{} tokens", learned.tokens);

    // Undo takes it back exactly.
    undo(&h, batch).await;
    let undone = status(&h).await;
    assert_eq!((undone.learned_spam, undone.tokens), (0, 0));

    // Learned again, then "Not spam" in the junk folder: unlearned as spam, learned as ham.
    eventually("back in the inbox", || async {
        h.inbox_items().await.iter().any(|i| i.id == second.id)
    })
    .await;
    h.core
        .apply_action(MailAction::Spam, vec![at(inbox, &second)], "s-again".into())
        .await
        .expect("spam");
    let junk = h.mailbox(MailboxRole::Junk).await;
    eventually("in the junk folder", || async {
        h.core
            .list_messages(junk, None, 10)
            .await
            .expect("junk")
            .items
            .iter()
            .any(|i| i.id == second.id)
    })
    .await;
    assert_eq!(status(&h).await.learned_spam, 1);
    h.core
        .apply_action(
            MailAction::NotSpam,
            vec![MessageRef {
                mailbox_id: junk,
                message_id: second.id,
            }],
            "not-spam".into(),
        )
        .await
        .expect("not spam");
    let moved_back = status(&h).await;
    assert_eq!((moved_back.learned_spam, moved_back.learned_ham), (0, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_spam_goes_to_junk_unannounced_and_undo_learns_it_as_ham() {
    let server = FakeServer::new();
    add_junk(&server);
    let mut h = Harness::new(server).await;
    h.writes(true).await;
    h.core
        .set_notification_settings(system_style())
        .await
        .expect("system notifications");
    enable(&h).await;
    h.core.spam().set_minimums(3, 3);
    // More ham than spam, as the filter keeps it: one more ham mail then weighs little.
    teach(&mut h, 1..4, 10..22).await;
    let ready = status(&h).await;
    assert!(ready.ready, "{ready:?}");

    // A fourth offer goes to junk on the server, unannounced; a fourth letter stays.
    h.server.deliver_raw("INBOX", &spam_mail(4));
    let event = h
        .event(|e| matches!(e, CoreEvent::SpamFiltered { .. }))
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
    eventually("moved to junk on the server", || async {
        h.server.read(|s| {
            s.mailboxes["Junk"]
                .messages
                .values()
                .filter_map(|m| m.raw.as_deref())
                .any(|raw| String::from_utf8_lossy(raw).contains("PILLS 4"))
        })
    })
    .await;
    let letter = h.deliver(&ham_mail(4)).await;
    eventually("the letter announced", || async {
        h.sink.shown().iter().any(|n| n.body == "Hiking trip 4")
    })
    .await;
    assert!(
        !h.sink
            .shown()
            .iter()
            .any(|n| n.body.contains("CHEAP PILLS 4")),
        "spam is not announced"
    );
    assert!(letter.spam.is_none());
    // In the junk folder the row says why it is there, and so does the reading pane's note.
    let junk = h.mailbox(MailboxRole::Junk).await;
    let junk_offer = || async {
        h.core
            .list_messages(junk, None, 10)
            .await
            .expect("junk")
            .items
            .into_iter()
            .find(|i| i.subject.contains("PILLS 4"))
    };
    eventually("listed in junk with the mark", || async {
        junk_offer()
            .await
            .is_some_and(|o| o.spam == Some(SpamFilterMark::Moved))
    })
    .await;
    let offer = junk_offer().await.expect("offer");
    let verdict = h
        .core
        .get_spam_verdict(offer.id)
        .await
        .expect("verdict")
        .expect("some");
    assert_eq!(verdict.mark, SpamFilterMark::Moved);
    assert!(verdict.score >= mail_model::SPAM_THRESHOLD);

    // "Undo" on the notice: back in the inbox, learned as not spam, the mark gone.
    undo(&h, batch).await;
    let after = status(&h).await;
    assert_eq!(after.learned_ham, ready.learned_ham + 1);
    assert!(
        h.core
            .get_spam_verdict(offer.id)
            .await
            .expect("verdict")
            .is_none()
    );
    eventually("back in the inbox on the server", || async {
        h.server.read(|s| {
            s.mailboxes["INBOX"]
                .messages
                .values()
                .filter_map(|m| m.raw.as_deref())
                .any(|raw| String::from_utf8_lossy(raw).contains("PILLS 4"))
        })
    })
    .await;
    // Back in the inbox it arrives as new mail again: learned as ham, it is not judged again.
    eventually("listed in the inbox again", || async {
        h.inbox_items().await.iter().any(|i| i.id == offer.id)
    })
    .await;
    h.deliver(&ham_mail(5)).await;
    eventually("the next letter announced", || async {
        h.sink.shown().iter().any(|n| n.body == "Hiking trip 5")
    })
    .await;
    assert!(
        !h.sink
            .shown()
            .iter()
            .any(|n| n.body.contains("CHEAP PILLS 4")),
        "a mail coming back is not announced either way"
    );
    assert!(
        h.by_subject("CHEAP PILLS 4 - ORDER NOW!!!")
            .await
            .spam
            .is_none()
    );
    assert!(
        h.core
            .get_spam_verdict(offer.id)
            .await
            .expect("verdict")
            .is_none(),
        "not judged again"
    );
    assert!(
        h.server.read(|s| s.mailboxes["Junk"]
            .messages
            .values()
            .filter_map(|m| m.raw.as_deref())
            .all(|raw| !String::from_utf8_lossy(raw).contains("PILLS 4"))),
        "not moved again"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn own_mail_replies_and_trusted_senders_are_never_spam() {
    let server = FakeServer::new();
    add_junk(&server);
    // The user's own message, in Sent (the fake names it `<1@fake.test>`, its serial).
    server.deliver_raw(
        "Sent",
        b"From: Alice <alice@fake.test>\r\nTo: deals@cheap-pills.example\r\n\
          Subject: Question\r\nMessage-ID: <mine-1@fake.test>\r\n\r\nWhat is this?\r\n",
    );
    let mut h = Harness::new(server).await;
    h.writes(true).await;
    enable(&h).await;
    h.core.spam().set_minimums(3, 3);
    teach(&mut h, 1..4, 1..4).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;

    // A reply to the user's mail, the user's own address, and a sender whose images the user
    // allowed: each looks like the offers, none is touched.
    let reply = String::from_utf8(spam_mail(10))
        .expect("utf-8")
        .replace("Content-Type", "In-Reply-To: <1@fake.test>\r\nContent-Type");
    let own = String::from_utf8(spam_mail(11))
        .expect("utf-8")
        .replace("deals11@cheap-pills.example", "alice@fake.test");
    h.core
        .store_for_tests()
        .allow_remote_content(h.account, "deals12@cheap-pills.example")
        .await
        .expect("allow");
    for raw in [reply.as_bytes(), own.as_bytes(), &spam_mail(12)] {
        h.deliver(raw).await;
    }
    // A plain offer after them is moved: the filter did look at this window.
    h.server.deliver_raw("INBOX", &spam_mail(13));
    h.event(|e| matches!(e, CoreEvent::SpamFiltered { moved: 1, .. }))
        .await;
    let items = h.core.list_messages(inbox, None, 50).await.expect("inbox");
    for n in [10, 11, 12] {
        let subject = format!("CHEAP PILLS {n} - ORDER NOW!!!");
        let item = items
            .items
            .iter()
            .find(|i| i.subject == subject)
            .unwrap_or_else(|| panic!("{subject} left the inbox"));
        assert!(item.spam.is_none());
        assert!(
            h.core
                .get_spam_verdict(item.id)
                .await
                .expect("verdict")
                .is_none()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_only_accounts_get_a_local_mark_and_not_spam_removes_it() {
    let mut h = Harness::new(FakeServer::new()).await;
    h.core
        .set_notification_settings(system_style())
        .await
        .expect("system notifications");
    enable(&h).await;
    h.core.spam().set_minimums(3, 3);
    // Read-only: "Spam" is refused; the local verbs teach instead.
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    let first = h.deliver(&spam_mail(1)).await;
    let refused = h
        .core
        .apply_action(MailAction::Spam, vec![at(inbox, &first)], "ro".into())
        .await;
    assert_eq!(code(refused.expect_err("read-only")), ErrorCode::ReadOnly);
    let mut spam = vec![first.id];
    for n in 2..4 {
        spam.push(h.deliver(&spam_mail(n)).await.id);
    }
    for id in &spam {
        h.core
            .spam_tokens_of(*id, crate::reading::SpamFetch::Notify)
            .await
            .expect("body");
    }
    h.core
        .mark_spam_locally(spam, true)
        .await
        .expect("mark spam");
    teach(&mut h, 0..0, 1..4).await;
    assert!(status(&h).await.ready);

    h.server.deliver_raw("INBOX", &spam_mail(4));
    let event = h
        .event(|e| matches!(e, CoreEvent::SpamFiltered { .. }))
        .await;
    assert!(matches!(
        event,
        CoreEvent::SpamFiltered {
            moved: 0,
            marked: 1,
            batch: None,
            ..
        }
    ));
    let offer = h.by_subject("CHEAP PILLS 4 - ORDER NOW!!!").await;
    assert_eq!(offer.spam, Some(SpamFilterMark::Suspected));
    let verdict = h
        .core
        .get_spam_verdict(offer.id)
        .await
        .expect("verdict")
        .expect("some");
    assert!(!verdict.can_move, "the account may not write");
    assert_eq!(
        h.server.read(|s| s.writer_attempts),
        0,
        "nothing on the server"
    );
    assert!(
        !h.sink
            .shown()
            .iter()
            .any(|n| n.body.contains("CHEAP PILLS 4"))
    );

    // "Not spam": the mark goes, it is learned as ham; "Spam" again marks it.
    let before = status(&h).await;
    assert_eq!(
        h.core
            .mark_spam_locally(vec![offer.id], false)
            .await
            .expect("not spam"),
        1
    );
    assert!(
        h.by_subject("CHEAP PILLS 4 - ORDER NOW!!!")
            .await
            .spam
            .is_none()
    );
    assert_eq!(status(&h).await.learned_ham, before.learned_ham + 1);
    h.core
        .mark_spam_locally(vec![offer.id], true)
        .await
        .expect("spam");
    assert_eq!(
        h.by_subject("CHEAP PILLS 4 - ORDER NOW!!!").await.spam,
        Some(SpamFilterMark::Suspected)
    );
    let relearned = status(&h).await;
    assert_eq!(
        (relearned.learned_spam, relearned.learned_ham),
        (before.learned_spam + 1, before.learned_ham)
    );
    assert_eq!(
        code(
            h.core
                .mark_spam_locally(Vec::new(), true)
                .await
                .expect_err("none")
        ),
        ErrorCode::InvalidArgument
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn learning_from_existing_mail_takes_junk_and_read_inbox_mail() {
    let server = FakeServer::new();
    add_junk(&server);
    for n in 1..=4 {
        server.deliver_raw("Junk", &spam_mail(n));
    }
    for n in 1..=5 {
        let uid = server.deliver_raw("INBOX", &ham_mail(n));
        if n != 5 {
            server.with(|s| {
                if let Some(m) = s
                    .mailboxes
                    .get_mut("INBOX")
                    .and_then(|b| b.messages.get_mut(&uid))
                {
                    m.flags.seen = true;
                }
            });
        }
    }
    let mut h = Harness::new(server).await;
    // Off: refused.
    assert_eq!(
        code(h.core.learn_spam_from_mail().await.expect_err("off")),
        ErrorCode::InvalidArgument
    );
    enable(&h).await;
    h.core.learn_spam_from_mail().await.expect("learn");
    let done = h
        .event(|e| {
            matches!(e, CoreEvent::SpamFilterChanged { status }
                if status.learning.as_ref().is_some_and(|l| l.state != SpamLearningState::Running))
        })
        .await;
    let CoreEvent::SpamFilterChanged { status: done } = done else {
        unreachable!()
    };
    let learning = done.learning.expect("learning");
    assert_eq!(learning.state, SpamLearningState::Done);
    assert_eq!((learning.spam, learning.ham, learning.total), (4, 4, 8));
    assert_eq!((done.learned_spam, done.learned_ham), (4, 4));
    // The junk bodies were fetched to learn from (read-only): nothing became read.
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);
    // Again: everything is learned already.
    h.core.learn_spam_from_mail().await.expect("again");
    h.event(|e| {
        matches!(e, CoreEvent::SpamFilterChanged { status }
            if status.learning.as_ref().is_some_and(|l| l.state == SpamLearningState::Done && l.total == 0))
    })
    .await;

    // Forgetting empties the model; turning the filter off keeps it.
    let forgotten = h.core.forget_spam_filter().await.expect("forget");
    assert_eq!(
        (
            forgotten.learned_spam,
            forgotten.learned_ham,
            forgotten.tokens
        ),
        (0, 0, 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_setting_is_off_by_default_synchronised_and_per_account() {
    let h = Harness::new(FakeServer::new()).await;
    let status = h.core.get_spam_filter().await.expect("status");
    assert_eq!(status.settings, SpamFilterSettings::default());
    assert!(!status.ready);
    assert_eq!(
        (status.min_spam, status.min_ham),
        (mail_model::MIN_LEARNED_SPAM, mail_model::MIN_LEARNED_HAM)
    );
    assert!(crate::settings_sync::record::SYNCED_SETTINGS.contains(&crate::spam::SETTING_KEY));
    let stored = h
        .core
        .set_spam_filter(SpamFilterSettings {
            enabled: true,
            excluded_accounts: vec![h.account, h.account],
        })
        .await
        .expect("set");
    assert_eq!(stored.settings.excluded_accounts, [h.account]);
    assert!(!stored.settings.applies_to(h.account));
    let too_many = (0..=mail_model::MAX_SPAM_EXCLUDED_ACCOUNTS)
        .map(|i| {
            format!("00000000-0000-4000-8000-{i:012}")
                .parse()
                .expect("id")
        })
        .collect();
    assert_eq!(
        code(
            h.core
                .set_spam_filter(SpamFilterSettings {
                    enabled: true,
                    excluded_accounts: too_many,
                })
                .await
                .expect_err("too many")
        ),
        ErrorCode::InvalidArgument
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn learning_more_spam_than_half_the_ham_tops_up_ham_from_read_inbox_mail() {
    let server = FakeServer::new();
    add_junk(&server);
    // Ten read letters in the inbox, older than anything the test delivers.
    for n in 100..110 {
        let uid = server.deliver_raw("INBOX", &ham_mail(n));
        server.with(|s| {
            if let Some(m) = s
                .mailboxes
                .get_mut("INBOX")
                .and_then(|b| b.messages.get_mut(&uid))
            {
                m.flags.seen = true;
            }
        });
    }
    let mut h = Harness::new(server).await;
    h.writes(true).await;
    enable(&h).await;
    // Three offers as spam, nothing as ham yet: the filter learns twice as many read letters.
    teach(&mut h, 1..4, 0..0).await;
    let done = h
        .event(|e| {
            matches!(e, CoreEvent::SpamFilterChanged { status }
                if status.learning.as_ref().is_some_and(|l| l.state == SpamLearningState::Done))
        })
        .await;
    let CoreEvent::SpamFilterChanged { status: done } = done else {
        unreachable!()
    };
    assert_eq!((done.learned_spam, done.learned_ham), (3, 6));
    let learning = done.learning.expect("learning");
    assert_eq!((learning.spam, learning.ham), (0, 6));
}
