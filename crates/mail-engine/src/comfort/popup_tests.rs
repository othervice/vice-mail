//! Stage 18 (decision 0051) through `AppCore` with the fake server: new mail becomes cards of
//! the app's popup instead of a system notification (by the same rules), with the sound of the
//! sync window and the snippet once the body is stored; "Read" asks the main window to show the
//! message, "Delete" goes through the outbox into the trash (refused for a read-only account)
//! and can be undone, "N more" opens the inbox; hidden content keeps nothing of the message.

use std::collections::BTreeMap;

use mail_model::{ErrorCode, IpcError, NewMailPopup, NewMailStyle};

use super::*;
use crate::sync::fake::FakeMailbox;

fn code(err: mail_model::CoreError) -> ErrorCode {
    IpcError::from(err).code
}

fn add_trash(server: &FakeServer) {
    server.with(|s| {
        s.mailboxes.insert(
            "Trash".to_owned(),
            FakeMailbox {
                uid_validity: 901,
                uid_next: 1,
                modseq: 1,
                attributes: vec!["\\Trash".to_owned()],
                messages: BTreeMap::new(),
                changes: 0,
            },
        );
    });
}

/// The last popup the sink received.
fn last(h: &Harness) -> Option<NewMailPopup> {
    h.sink.popups().last().map(|u| u.popup.clone())
}

fn subjects(popup: &NewMailPopup) -> Vec<String> {
    popup
        .cards
        .iter()
        .map(|c| {
            c.content
                .as_ref()
                .map(|c| c.subject.clone())
                .unwrap_or_default()
        })
        .collect()
}

fn mail(from: &str, subject: &str, text: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: alice@fake.test\r\nSubject: {subject}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n"
    )
    .into_bytes()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_mail_shows_as_popup_cards_by_the_same_rules() {
    let server = FakeServer::new();
    add_trash(&server);
    let mut h = Harness::new(server).await;
    // The fake server dates its messages in 2023: prefetch everything, so the body of the new
    // message is stored and its snippet arrives.
    h.core
        .set_prefetch_settings(mail_model::PrefetchSettings {
            period: mail_model::PrefetchPeriod::All,
            ..mail_model::PrefetchSettings::default()
        })
        .await
        .expect("prefetch");
    assert_eq!(
        h.core
            .get_notification_settings()
            .await
            .expect("settings")
            .new_mail_style,
        NewMailStyle::Popup,
        "the popup is the default"
    );
    // The first sync announced nothing.
    assert!(h.sink.popups().is_empty());

    // A person's mail: one card with sender and subject, the sound of the window, and no
    // system notification.
    let person = h.deliver(PERSON).await;
    eventually("a card for the person's mail", || async {
        last(&h).is_some_and(|p| subjects(&p) == ["Lunch tomorrow?"])
    })
    .await;
    let first = h.sink.popups();
    let arrival = first
        .iter()
        .find(|u| !u.popup.cards.is_empty())
        .expect("arrival");
    assert_eq!(arrival.sound.as_deref(), Some("Default"));
    let card = &arrival.popup.cards[0];
    assert_eq!((card.account_id, card.message_id), (h.account, person.id));
    assert_eq!(card.mailbox_id, h.mailbox(MailboxRole::Inbox).await);
    let content = card.content.as_ref().expect("content shown");
    assert_eq!(content.sender_name.as_deref(), Some("Bob Example"));
    assert_eq!(content.sender_email.as_deref(), Some("bob@friends.example"));
    assert_eq!(card.account, None, "one account: no account line");
    assert!(!card.writes_enabled, "new accounts are read-only");
    assert_eq!(arrival.popup.more, 0);
    assert!(
        h.sink
            .shown()
            .iter()
            .all(|n| n.kind != NotificationKind::NewMail),
        "no system notification for new mail"
    );
    // The snippet comes once the prefetch stored the body (or was there already).
    eventually("the snippet", || async {
        last(&h).is_some_and(|p| {
            p.cards
                .first()
                .and_then(|c| c.content.as_ref())
                .and_then(|c| c.snippet.as_deref())
                == Some("Hi Alice")
        })
    })
    .await;
    // The popup window asks for the same state.
    let asked = h.core.new_mail_popup().await;
    assert_eq!(Some(&asked), last(&h).as_ref());

    // A newsletter is not announced (categories), so no card and no sound.
    let updates = h.sink.popups().len();
    h.deliver(&newsletter("https://news.shop.example/u")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.sink.popups().len(), updates);

    // Read-only: "Delete" is refused by the outbox and nothing changes.
    assert_eq!(
        code(
            h.core
                .delete_from_popup(person.id)
                .await
                .expect_err("read-only")
        ),
        ErrorCode::ReadOnly
    );
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);

    // "Read": the card goes and the main window is asked to show the message in its inbox.
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    let account = h.account;
    let after = h.core.read_from_popup(person.id).await.expect("read");
    assert!(after.is_empty());
    assert!(last(&h).is_some_and(|p| p.is_empty()), "the shell hears it");
    h.event(|e| {
        matches!(e, CoreEvent::OpenMessageRequested { account_id, mailbox_id, message_id }
            if *account_id == account && *mailbox_id == Some(inbox) && *message_id == person.id)
    })
    .await;
    assert_eq!(
        code(h.core.read_from_popup(person.id).await.expect_err("gone")),
        ErrorCode::NotFound
    );

    // With writes on: "Delete" moves it to the trash on the server; "Undo" brings it back.
    h.writes(true).await;
    let urgent = h.deliver(URGENT).await;
    eventually("a card for the urgent mail", || async {
        last(&h).is_some_and(|p| subjects(&p) == ["URGENT: contract"])
    })
    .await;
    assert!(last(&h).is_some_and(|p| p.cards[0].writes_enabled));
    let deleted = h.core.delete_from_popup(urgent.id).await.expect("delete");
    assert!(deleted.cards[0].deleted, "the card says Deleted");
    eventually("in the trash on the server", || async {
        h.server.read(|s| {
            s.mailboxes["Trash"].messages.len() == 1 && s.mailboxes["INBOX"].messages.len() == 2
        })
    })
    .await;
    // Twice is still one deletion.
    assert!(
        h.core
            .delete_from_popup(urgent.id)
            .await
            .expect("again")
            .cards[0]
            .deleted
    );
    let undone = h.core.undo_popup_delete(urgent.id).await.expect("undo");
    assert!(!undone.cards[0].deleted);
    eventually("back in the inbox on the server", || async {
        h.server.read(|s| {
            s.mailboxes["Trash"].messages.is_empty() && s.mailboxes["INBOX"].messages.len() == 3
        })
    })
    .await;
    // Closing the card.
    assert!(h.core.dismiss_popup(Some(urgent.id)).await.is_empty());
    assert!(
        h.core.dismiss_popup(Some(urgent.id)).await.is_empty(),
        "gone is no error"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn several_messages_are_cards_up_to_three_and_n_more_opens_the_inbox() {
    let mut h = Harness::new(FakeServer::new()).await;
    for n in 1..=5 {
        h.server.deliver_raw(
            "INBOX",
            &mail(
                &format!("Friend {n} <friend{n}@friends.example>"),
                &format!("Note {n}"),
                "Hello",
            ),
        );
    }
    eventually("five cards", || async {
        last(&h).is_some_and(|p| p.cards.len() == 3 && p.more == 2)
    })
    .await;
    let popup = last(&h).expect("popup");
    assert_eq!(
        subjects(&popup),
        ["Note 5", "Note 4", "Note 3"],
        "newest on top"
    );
    assert!(popup.revision > 0);

    // Closing a card lets the next one move up.
    let top = popup.cards[0].message_id;
    let after = h.core.dismiss_popup(Some(top)).await;
    assert_eq!(subjects(&after), ["Note 4", "Note 3", "Note 2"]);
    assert_eq!(after.more, 1);
    assert!(after.revision > popup.revision);

    // "N more": everything goes, the main window shows this account's inbox.
    let account = h.account;
    assert!(h.core.open_inbox_from_popup().await.is_empty());
    h.event(|e| {
        matches!(e, CoreEvent::OpenInboxRequested { account_id } if *account_id == Some(account))
    })
    .await;
    assert!(last(&h).is_some_and(|p| p.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hidden_content_quiet_hours_and_the_system_style() {
    let mut h = Harness::new(FakeServer::new()).await;
    h.core
        .set_notification_settings(NotificationSettings {
            hide_content: true,
            ..NotificationSettings::default()
        })
        .await
        .expect("settings");
    let person = h.deliver(PERSON).await;
    eventually("a card without content", || async {
        last(&h).is_some_and(|p| {
            p.cards.len() == 1 && p.cards[0].message_id == person.id && p.cards[0].content.is_none()
        })
    })
    .await;
    // Not even once the body is stored.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(last(&h).is_some_and(|p| p.cards[0].content.is_none()));
    assert!(h.core.dismiss_popup(None).await.is_empty());

    // Quiet hours: no card, no sound.
    let minute = super::super::time::minute_of_day(h.clock.now_ms(), &jiff::tz::TimeZone::UTC);
    h.core
        .set_notification_settings(NotificationSettings {
            quiet_hours: mail_model::QuietHours {
                enabled: true,
                start_minute: minute.saturating_sub(5) % 1440,
                end_minute: (minute + 5) % 1440,
            },
            ..NotificationSettings::default()
        })
        .await
        .expect("settings");
    let updates = h.sink.popups().len();
    h.deliver(&mail("Eve <eve@x.example>", "At night", "x"))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.sink.popups().len(), updates);

    // The system style: a system notification and no card.
    h.core
        .set_notification_settings(NotificationSettings {
            new_mail_style: NewMailStyle::System,
            ..NotificationSettings::default()
        })
        .await
        .expect("settings");
    h.deliver(&mail("Dan <dan@x.example>", "By the system", "x"))
        .await;
    eventually("a system notification", || async {
        h.sink
            .shown()
            .iter()
            .any(|n| n.kind == NotificationKind::NewMail && n.body == "By the system")
    })
    .await;
    assert_eq!(h.sink.popups().len(), updates);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cards_of_a_removed_account_go() {
    let mut h = Harness::new(FakeServer::new()).await;
    h.deliver(PERSON).await;
    eventually("a card", || async {
        last(&h).is_some_and(|p| p.cards.len() == 1)
    })
    .await;
    h.core.remove_account(h.account).await.expect("remove");
    eventually("no card", || async {
        last(&h).is_some_and(|p| p.is_empty())
    })
    .await;
}
