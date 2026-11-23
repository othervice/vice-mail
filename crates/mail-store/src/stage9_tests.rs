//! Stage 9 in the store: classification when stored, the copies of category and important
//! mark on the locations, sender rules (address before list before domain, subdomains), the
//! rules pass, snoozes, pins, tasks and their sync identity, local rules.

use std::sync::Arc;

use mail_model::{
    CategoryReason, MessageCategory, MessageHeader, MessageId, RuleAction, RuleCondition,
    RuleInput, SenderKey, TaskFilter, TextMatchOp,
};

use crate::test_support::{UV, fixture, header};
use crate::{
    Classified, Classifier, InboxListKind, InboxMailboxes, LocalFacts, MarkView, SenderKeys,
    SenderRuleChange,
};

/// Categories by the sender's local part, keys from the address; enough for the store's side.
struct ByLocalPart;

impl Classifier for ByLocalPart {
    fn sender_keys(&self, header: &MessageHeader) -> SenderKeys {
        let email = header.from.first().map(|a| a.email.to_lowercase());
        SenderKeys {
            domain: email
                .as_deref()
                .and_then(|e| e.split_once('@'))
                .map(|(_, d)| d.to_owned()),
            address: email,
            list_id: header.list_id.clone(),
        }
    }

    fn classify(&self, header: &MessageHeader, local: &LocalFacts<'_>) -> Classified {
        let email = header.from.first().map(|a| a.email.as_str()).unwrap_or("");
        let category = if local.sent_to_sender > 0 || !local.referenced_senders.is_empty() {
            MessageCategory::Person
        } else if email.starts_with("news") {
            MessageCategory::Newsletter
        } else if email.starts_with("noreply") {
            MessageCategory::Notification
        } else {
            MessageCategory::Person
        };
        Classified {
            category,
            reason: CategoryReason::NoBulkSignals,
            one_click: Some(email.starts_with("news")),
        }
    }
}

fn from(mut h: MessageHeader, email: &str) -> MessageHeader {
    h.from[0].email = email.into();
    h
}

const D: i64 = 1_764_000_000;

#[tokio::test]
async fn categories_are_stored_and_copied_onto_locations() {
    let f = fixture().await;
    f.store.set_classifier(Arc::new(ByLocalPart));
    let mut answer = from(header(3, D + 3, Some("3@x")), "carol@else.example");
    answer.in_reply_to = Some("1@x".into());
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![
                from(header(1, D + 1, Some("1@x")), "news@shop.example"),
                from(header(2, D + 2, Some("2@x")), "noreply@mail.shop.example"),
                answer,
            ],
            None,
        )
        .await
        .expect("store");
    // The copy in the archive (merge) takes the message's category through the trigger.
    f.store
        .store_headers(
            f.archive,
            UV,
            vec![from(header(1, D + 1, Some("1@x")), "news@shop.example")],
            None,
        )
        .await
        .expect("store archive");
    let items = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items;
    let cat = |subject: &str| {
        items
            .iter()
            .find(|i| i.subject == subject)
            .and_then(|i| i.category)
    };
    assert_eq!(cat("Subject 1"), Some(MessageCategory::Newsletter));
    assert_eq!(cat("Subject 2"), Some(MessageCategory::Notification));
    // Answers a stored message: a local fact the store looks up.
    assert_eq!(cat("Subject 3"), Some(MessageCategory::Person));
    let archived = f
        .store
        .inbox_list(
            InboxMailboxes::Mailbox(f.archive),
            InboxListKind::Category(MessageCategory::Newsletter),
            None,
            10,
        )
        .await
        .expect("archive");
    assert_eq!(archived.items.len(), 1);
    let stored = f
        .store
        .message_category(items[2].id)
        .await
        .expect("category")
        .expect("message");
    assert_eq!(stored.unsubscribe_one_click, Some(true));
    assert_eq!(stored.keys.domain.as_deref(), Some("shop.example"));

    // A domain rule covers subdomains; an address rule goes before it; the list views follow.
    let now = 1;
    f.store
        .put_sender_rule(
            SenderKey::domain("shop.example"),
            SenderRuleChange {
                category: Some(Some(MessageCategory::Person)),
                important: Some(true),
            },
            now,
        )
        .await
        .expect("rule");
    f.store
        .put_sender_rule(
            SenderKey::address("news@shop.example"),
            SenderRuleChange {
                category: Some(Some(MessageCategory::Notification)),
                important: None,
            },
            now,
        )
        .await
        .expect("rule");
    loop {
        let step = f.store.category_step().await.expect("step");
        if step.done && step.classified == 0 {
            break;
        }
    }
    let items = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items;
    let find = |subject: &str| items.iter().find(|i| i.subject == subject).expect("item");
    assert_eq!(
        find("Subject 1").category,
        Some(MessageCategory::Notification)
    );
    assert!(
        find("Subject 1").important,
        "the domain's important mark still applies"
    );
    assert_eq!(
        find("Subject 2").category,
        Some(MessageCategory::Person),
        "subdomain"
    );
    assert!(!find("Subject 3").important);
    let important = f
        .store
        .inbox_list(
            InboxMailboxes::Mailbox(f.inbox),
            InboxListKind::Important,
            None,
            10,
        )
        .await
        .expect("important");
    assert_eq!(important.items.len(), 2);
    assert_eq!(
        f.store
            .important_counts(InboxMailboxes::Mailbox(f.inbox))
            .await
            .expect("counts"),
        (2, 2)
    );
    // Removing the corrections returns to the automatic result.
    for rule in f.store.sender_rules().await.expect("rules") {
        assert!(f.store.delete_sender_rule(rule.id).await.expect("delete"));
    }
    loop {
        let step = f.store.category_step().await.expect("step");
        if step.done {
            break;
        }
    }
    let items = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items;
    assert!(items.iter().all(|i| !i.important));
    assert_eq!(
        items
            .iter()
            .find(|i| i.subject == "Subject 2")
            .and_then(|i| i.category),
        Some(MessageCategory::Notification)
    );
}

#[tokio::test]
async fn marks_follow_the_message_id_and_need_one() {
    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![header(1, D + 1, Some("1@x")), header(2, D + 2, None)],
            None,
        )
        .await
        .expect("store");
    let (with_id, without) = (MessageId(D + 1), MessageId(D + 2));
    let err = f
        .store
        .pin_message(without, 1)
        .await
        .expect_err("no Message-ID");
    assert!(matches!(err, crate::StoreError::InvalidArgument(_)));
    assert!(f.store.pin_message(with_id, 1).await.expect("pin"));
    assert!(
        !f.store.pin_message(with_id, 2).await.expect("again"),
        "idempotent"
    );

    // Snoozing again replaces the time; returned at the time; next due.
    f.store
        .snooze_message(with_id, 5_000, 1)
        .await
        .expect("snooze");
    let (stored, before) = f
        .store
        .snooze_message(with_id, 9_000, 2)
        .await
        .expect("again");
    assert_eq!(
        (stored.until, before.map(|b| b.until)),
        (9_000, Some(5_000))
    );
    assert_eq!(f.store.next_snooze_due().await.expect("next"), Some(9_000));
    assert!(
        f.store
            .return_due_snoozes(8_999)
            .await
            .expect("early")
            .is_empty()
    );
    let returned = f.store.return_due_snoozes(9_000).await.expect("due");
    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0].messages, vec![with_id]);
    assert_eq!(f.store.next_snooze_due().await.expect("next"), None);
    let item = &f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items[1];
    assert_eq!(item.id, with_id);
    assert!(item.pinned && item.snooze.is_some_and(|s| s.returned));

    // Rows of the inbox leave out pinned and snoozed messages; the top block has them.
    let rows = f
        .store
        .inbox_list(
            InboxMailboxes::Mailbox(f.inbox),
            InboxListKind::Rows,
            None,
            10,
        )
        .await
        .expect("rows");
    assert_eq!(
        rows.items.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![without]
    );
    let top = f
        .store
        .inbox_top(InboxMailboxes::Mailbox(f.inbox), false, 10)
        .await
        .expect("top");
    assert_eq!(top.iter().map(|i| i.id).collect::<Vec<_>>(), vec![with_id]);

    // Tasks: open, done, open again, removed.
    let (_, after) = f
        .store
        .set_task(with_id, Some(false), 10)
        .await
        .expect("task");
    assert_eq!(after.and_then(|t| t.done_at), None);
    let (_, after) = f
        .store
        .set_task(with_id, Some(true), 11)
        .await
        .expect("done");
    assert_eq!(after.and_then(|t| t.done_at), Some(11));
    let (_, after) = f
        .store
        .set_task(with_id, Some(true), 12)
        .await
        .expect("done again");
    assert_eq!(
        after.and_then(|t| t.done_at),
        Some(11),
        "first done time kept"
    );
    let accounts = vec![f.account];
    let view = |filter| {
        f.store
            .mark_view(MarkView::Tasks(filter), accounts.clone(), vec![f.inbox], 10)
    };
    assert_eq!(view(TaskFilter::Done).await.expect("done").len(), 1);
    assert!(view(TaskFilter::Open).await.expect("open").is_empty());
    f.store
        .set_task(with_id, Some(false), 13)
        .await
        .expect("reopen");
    assert_eq!(view(TaskFilter::Open).await.expect("open").len(), 1);
    f.store.set_task(with_id, None, 14).await.expect("remove");
    assert!(view(TaskFilter::All).await.expect("all").is_empty());

    // The marks survive a rebuilt mailbox (new local rows, same Message-ID).
    f.store
        .reset_mailbox(
            f.inbox,
            crate::MailboxBaseline {
                uid_validity: mail_model::UidValidity(2),
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    f.store
        .delete_orphan_messages(f.account)
        .await
        .expect("orphans");
    f.store
        .store_headers(
            f.inbox,
            mail_model::UidValidity(2),
            vec![header(7, D + 1, Some("1@x"))],
            None,
        )
        .await
        .expect("again");
    let pinned = f
        .store
        .mark_view(MarkView::Pinned, accounts, vec![f.inbox], 10)
        .await
        .expect("pinned");
    assert_eq!(pinned.len(), 1);
    assert!(pinned[0].pinned);
}

#[tokio::test]
async fn local_rules_are_checked_and_ordered() {
    let f = fixture().await;
    let input = |name: &str| RuleInput {
        name: name.into(),
        enabled: true,
        match_all: true,
        conditions: vec![RuleCondition::Subject {
            op: TextMatchOp::Contains,
            value: "x".into(),
        }],
        actions: vec![RuleAction::MarkRead],
        stop: false,
    };
    let a = f.store.create_rule(input("A"), 1).await.expect("a");
    let b = f.store.create_rule(input("B"), 1).await.expect("b");
    assert!(a.position < b.position);
    let mut no_action = input("C");
    no_action.actions.clear();
    assert!(f.store.create_rule(no_action, 1).await.is_err());
    let mut empty_value = input("D");
    empty_value.conditions = vec![RuleCondition::From {
        op: TextMatchOp::Is,
        value: " ".into(),
    }];
    assert!(f.store.create_rule(empty_value, 1).await.is_err());
    assert!(f.store.create_rule(input(" "), 1).await.is_err());
    let order = f
        .store
        .reorder_rules(vec![b.id.clone()], 2)
        .await
        .expect("reorder");
    assert_eq!(
        order.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["B", "A"]
    );
    let mut changed = input("A2");
    changed.enabled = false;
    let a2 = f
        .store
        .update_rule(a.id.clone(), changed, 3)
        .await
        .expect("update");
    assert!(!a2.enabled && a2.name == "A2");
    assert!(f.store.delete_rule(a.id).await.expect("delete"));
    assert_eq!(f.store.rules().await.expect("rules").len(), 1);
}
