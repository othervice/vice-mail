//! Stage 22 (decision 0077): the spam filter's data in the store: learning and unlearning by
//! the recorded tokens, undo by batch, verdicts on list rows, the messages to learn from, the
//! facts about a sender, pruning and forgetting.

use mail_model::{BatchId, MailboxId, MailboxRole, MessageId, SpamFilterMark};

use crate::test_support::{Fixture, UV, fixture, header, remote, seen};
use crate::{MailboxBaseline, SpamClass, SpamModelSize};

const D: i64 = 1_764_000_000;

async fn with_junk() -> (Fixture, MailboxId) {
    let f = fixture().await;
    f.store
        .sync_mailboxes(
            f.account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
                remote("Junk", &["\\Junk"]),
            ],
        )
        .await
        .expect("mailboxes");
    let junk = f
        .store
        .mailboxes(f.account)
        .await
        .expect("list")
        .into_iter()
        .find(|m| m.role == MailboxRole::Junk)
        .expect("junk")
        .id;
    f.store
        .reset_mailbox(
            junk,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    (f, junk)
}

async fn size(f: &Fixture) -> SpamModelSize {
    f.store.spam_model_size().await.expect("size")
}

#[tokio::test]
async fn learning_counts_tokens_once_and_unlearning_takes_back_exactly_them() {
    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, Some("a@x")), header(2, D + 1, Some("b@x"))],
            None,
        )
        .await
        .expect("store");
    let (a, b) = (MessageId(D), MessageId(D + 1));
    let learned = f
        .store
        .learn_spam(vec![(a, vec![1, 2, 3, 3])], SpamClass::Spam, None, 10)
        .await
        .expect("learn");
    assert_eq!(learned, [Some(crate::Learned { previous: None })]);
    f.store
        .learn_spam(vec![(b, vec![2, 3, u64::MAX])], SpamClass::Ham, None, 11)
        .await
        .expect("learn");
    assert_eq!(
        size(&f).await,
        SpamModelSize {
            spam: 1,
            ham: 1,
            tokens: 4
        }
    );
    let crate::SpamLookup {
        totals,
        tokens: counts,
    } = f
        .store
        .spam_counts(vec![1, 2, 5, u64::MAX])
        .await
        .expect("counts");
    assert_eq!(totals, (1, 1));
    assert_eq!(counts, [(1, 0), (1, 1), (0, 0), (0, 1)]);

    // A mail moved from spam to ham is unlearned as spam first.
    let again = f
        .store
        .learn_spam(vec![(a, vec![1, 9])], SpamClass::Ham, None, 12)
        .await
        .expect("relearn");
    assert_eq!(
        again,
        [Some(crate::Learned {
            previous: Some(SpamClass::Spam)
        })]
    );
    let crate::SpamLookup {
        totals,
        tokens: counts,
    } = f.store.spam_counts(vec![1, 2, 3, 9]).await.expect("counts");
    assert_eq!(totals, (0, 2));
    assert_eq!(counts, [(0, 1), (0, 1), (0, 1), (0, 1)]);
    assert_eq!(
        f.store.spam_learned_class(a).await.expect("class"),
        Some(SpamClass::Ham)
    );

    // Unlearning removes tokens nobody has any more.
    let gone = f.store.unlearn_spam(vec![a, b, a]).await.expect("unlearn");
    assert_eq!(gone, [Some(SpamClass::Ham), Some(SpamClass::Ham), None]);
    assert_eq!(size(&f).await, SpamModelSize::default());

    // A message that is gone is skipped.
    let skipped = f
        .store
        .learn_spam(vec![(MessageId(42), vec![1])], SpamClass::Spam, None, 13)
        .await
        .expect("skip");
    assert_eq!(skipped, [None]);
    assert_eq!(size(&f).await, SpamModelSize::default());
}

#[tokio::test]
async fn undo_unlearns_the_batch_and_deleted_mail_keeps_its_counts() {
    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, Some("a@x")), header(2, D + 1, Some("b@x"))],
            None,
        )
        .await
        .expect("store");
    let (a, b) = (MessageId(D), MessageId(D + 1));
    f.store
        .learn_spam(
            vec![(a, vec![1, 2]), (b, vec![2])],
            SpamClass::Spam,
            Some(BatchId(7)),
            10,
        )
        .await
        .expect("learn");
    let undone = f.store.unlearn_spam_batch(BatchId(7)).await.expect("undo");
    assert_eq!(undone.len(), 2);
    assert_eq!(size(&f).await, SpamModelSize::default());
    assert!(
        f.store
            .unlearn_spam_batch(BatchId(7))
            .await
            .expect("again")
            .is_empty()
    );

    // The record of a deleted message goes with it; what it taught stays.
    f.store
        .learn_spam(vec![(a, vec![5])], SpamClass::Spam, None, 11)
        .await
        .expect("learn");
    f.store
        .write(|tx| Ok(tx.execute("DELETE FROM message WHERE id = ?1", [D])?))
        .await
        .expect("delete");
    let records = crate::test_support::count(&f.store, "SELECT count(*) FROM spam_trained").await;
    assert_eq!(records, 0);
    assert_eq!(size(&f).await.spam, 1);
}

#[tokio::test]
async fn verdicts_mark_list_rows_moved_ones_only_in_junk() {
    let (f, junk) = with_junk().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, Some("a@x")), header(2, D + 1, Some("b@x"))],
            None,
        )
        .await
        .expect("inbox");
    f.store
        .store_headers(junk, UV, vec![header(1, D + 2, Some("c@x"))], None)
        .await
        .expect("junk");
    let (a, b, c) = (MessageId(D), MessageId(D + 1), MessageId(D + 2));
    f.store
        .set_spam_verdicts(vec![(a, 0.995)], SpamFilterMark::Suspected, None, 5)
        .await
        .expect("suspect");
    f.store
        .set_spam_verdicts(
            vec![(b, 0.999), (c, 1.0)],
            SpamFilterMark::Moved,
            Some(BatchId(3)),
            6,
        )
        .await
        .expect("moved");
    let inbox = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    let mark = |id: MessageId| {
        inbox
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.spam)
            .expect("row")
    };
    assert_eq!(mark(a), Some(SpamFilterMark::Suspected));
    assert_eq!(mark(b), None, "a moved mark shows only in the junk folder");
    let junk_rows = f.store.list_messages(junk, None, 10).await.expect("junk");
    assert_eq!(junk_rows.items[0].spam, Some(SpamFilterMark::Moved));

    let verdict = f
        .store
        .spam_verdict(a)
        .await
        .expect("verdict")
        .expect("some");
    assert_eq!(verdict.mark, SpamFilterMark::Suspected);
    assert!((verdict.score - 0.995).abs() < 1e-9);
    assert!(!verdict.can_move, "writes are off");
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes");
    assert!(
        f.store
            .spam_verdict(a)
            .await
            .expect("verdict")
            .expect("some")
            .can_move
    );
    let mut moved = f
        .store
        .spam_moved_in_batch(BatchId(3))
        .await
        .expect("batch");
    moved.sort();
    assert_eq!(moved, [b, c]);
    assert_eq!(
        f.store
            .clear_spam_verdicts(vec![a, a])
            .await
            .expect("clear"),
        [Some(SpamFilterMark::Suspected), None]
    );
    assert!(f.store.spam_verdict(a).await.expect("verdict").is_none());

    // Forgetting empties everything.
    f.store
        .learn_spam(vec![(a, vec![1])], SpamClass::Ham, None, 7)
        .await
        .expect("learn");
    f.store.reset_spam_filter().await.expect("reset");
    assert_eq!(size(&f).await, SpamModelSize::default());
    assert!(f.store.spam_verdict(b).await.expect("verdict").is_none());
}

#[tokio::test]
async fn candidates_are_junk_newest_first_and_replied_then_read_inbox_mail() {
    let (f, junk) = with_junk().await;
    let mut replied = seen(header(3, D + 3, Some("r@x")));
    replied.flags.answered = true;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![
                header(1, D + 1, Some("unread@x")),
                seen(header(2, D + 2, Some("read@x"))),
                replied,
                seen(header(4, D + 4, Some("read2@x"))),
                seen(header(5, D + 5, Some("suspected@x"))),
            ],
            None,
        )
        .await
        .expect("inbox");
    f.store
        .store_headers(
            junk,
            UV,
            (1..=4)
                .map(|i| header(i, D + 10 + i64::from(i), None))
                .collect(),
            None,
        )
        .await
        .expect("junk");
    f.store
        .set_spam_verdicts(
            vec![(MessageId(D + 5), 0.995)],
            SpamFilterMark::Suspected,
            None,
            1,
        )
        .await
        .expect("verdict");
    let accounts = vec![f.account];
    let spam = f
        .store
        .spam_learning_candidates(accounts.clone(), SpamClass::Spam, 3)
        .await
        .expect("spam");
    assert_eq!(
        spam,
        [MessageId(D + 14), MessageId(D + 13), MessageId(D + 12)]
    );
    let ham = f
        .store
        .spam_learning_candidates(accounts.clone(), SpamClass::Ham, 10)
        .await
        .expect("ham");
    assert_eq!(
        ham,
        [MessageId(D + 3), MessageId(D + 4), MessageId(D + 2)],
        "replied first, then read newest first; unread and suspected left out"
    );
    // Learned mail is not chosen again.
    f.store
        .learn_spam(vec![(MessageId(D + 14), vec![1])], SpamClass::Spam, None, 1)
        .await
        .expect("learn");
    let spam = f
        .store
        .spam_learning_candidates(accounts, SpamClass::Spam, 10)
        .await
        .expect("spam");
    assert_eq!(spam.len(), 3);
    assert!(!spam.contains(&MessageId(D + 14)));
}

#[tokio::test]
async fn sender_facts_name_replies_contacts_and_importance() {
    let f = fixture().await;
    let mut own = header(1, D, Some("mine@x"));
    own.from[0].email = "alice@example.org".into();
    let mut reply = header(2, D + 1, Some("reply@x"));
    reply.in_reply_to = Some("mine@x".into());
    reply.references = vec!["older@x".into(), "mine@x".into()];
    f.store
        .store_headers(f.sent, UV, vec![own], None)
        .await
        .expect("sent");
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![reply, header(3, D + 2, Some("other@x"))],
            None,
        )
        .await
        .expect("inbox");
    let facts = f
        .store
        .spam_sender_facts(MessageId(D + 1))
        .await
        .expect("facts")
        .expect("some");
    assert_eq!(facts.account, Some(f.account));
    assert_eq!(facts.from.as_deref(), Some("sender@example.org"));
    assert!(
        facts
            .referenced_senders
            .contains(&"alice@example.org".to_owned())
    );
    assert!(!facts.known_contact);
    assert!(
        f.store
            .spam_sender_facts(MessageId(99))
            .await
            .expect("facts")
            .is_none()
    );

    // A contact the user wrote to is known; one only seen is not.
    f.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET sender_key = 'sender@example.org' WHERE id IN (?1, ?2)",
                [D + 1, D + 2],
            )?;
            tx.execute(
                "INSERT INTO contact (address_key, local_part, domain, rank, search, updated_at,
                                      received)
                 VALUES ('sender@example.org', 'sender', 'example.org', 0, '', 0, 5)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("contact");
    let seen_only = f
        .store
        .spam_sender_facts(MessageId(D + 2))
        .await
        .expect("facts")
        .expect("some");
    assert!(!seen_only.known_contact);
    f.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE contact SET sent = 1 WHERE address_key = 'sender@example.org'",
                [],
            )?)
        })
        .await
        .expect("sent to");
    let known = f
        .store
        .spam_sender_facts(MessageId(D + 2))
        .await
        .expect("facts")
        .expect("some");
    assert!(known.known_contact);

    // The stored fields for tokens without a body.
    let fields = f
        .store
        .spam_fields(MessageId(D + 2))
        .await
        .expect("fields")
        .expect("some");
    assert_eq!(fields.subject, "Subject 3");
    assert_eq!(fields.recipients, 1);
    assert_eq!(
        fields.from.map(|a| a.email).as_deref(),
        Some("sender@example.org")
    );
}

#[tokio::test]
async fn a_large_model_drops_tokens_seen_once() {
    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D, Some("a@x")), header(2, D + 1, Some("b@x"))],
            None,
        )
        .await
        .expect("store");
    f.store
        .learn_spam(
            vec![
                (MessageId(D), vec![1, 2, 3]),
                (MessageId(D + 1), vec![1, 4]),
            ],
            SpamClass::Spam,
            None,
            1,
        )
        .await
        .expect("learn");
    let pruned = f
        .store
        .write(|tx| crate::spam::prune(tx, 3))
        .await
        .expect("prune");
    assert_eq!(pruned, 3, "2, 3 and 4 were seen once");
    let lookup = f.store.spam_counts(vec![1, 2]).await.expect("counts");
    assert_eq!(lookup.tokens, [(2, 0), (0, 0)]);
    // Unlearning a message whose tokens were pruned is harmless.
    f.store
        .unlearn_spam(vec![MessageId(D)])
        .await
        .expect("unlearn");
    let lookup = f.store.spam_counts(vec![1]).await.expect("counts");
    assert_eq!((lookup.totals, lookup.tokens), ((1, 0), vec![(1, 0)]));
}
