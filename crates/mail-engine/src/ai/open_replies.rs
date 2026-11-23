//! Open-reply hints without AI (stage 13): rules over the stored headers and previews.
//!
//! - **Awaiting a reply:** a message the user sent (it lies in Sent, or its sender is one of
//!   the account's addresses) with a question, at least `days` old, and no later message of
//!   someone else in its thread (same thread, or `In-Reply-To`/`References` naming it).
//! - **Unanswered question:** a message in the inbox addressed to the user directly (`To`, not
//!   only `Cc`), with a question, at least a day old, not answered (`\Answered`, or a later
//!   message of the user in its thread).
//! - **Never:** newsletters and lists (`List-Id`, `List-Unsubscribe`, category newsletter or
//!   notification, `Precedence: bulk/list/junk`), automatic mail (`Auto-Submitted` other than
//!   `no`, no-reply senders), messages only in Trash or Junk, dismissed hints, messages older
//!   than 30 days, and every message but the newest of its side in a thread.
//!
//! A hint is a suggestion ("maybe still open"), never a claim; the user dismisses it.

use std::collections::HashMap;

use mail_model::{
    AccountId, CoreError, MessageId, OpenReplyHint, OpenReplyKind, OpenReplySettings,
};
use mail_store::OpenReplyRow;

use crate::core::{AppCore, Connector};

const DAY: i64 = 24 * 60 * 60;
/// Messages older than this are not looked at.
const WINDOW_DAYS: i64 = 30;
/// A received question counts as unanswered after this long.
const RECEIVED_AFTER: i64 = DAY;
/// Most rows read, newest first.
const MAX_ROWS: u32 = 5_000;
/// Most hints returned.
const MAX_HINTS: usize = 50;

/// Local parts of senders that never expect an answer.
const NO_REPLY: &[&str] = &[
    "noreply",
    "no-reply",
    "no_reply",
    "donotreply",
    "do-not-reply",
    "do_not_reply",
    "mailer-daemon",
    "postmaster",
    "bounce",
    "bounces",
    "notification",
    "notifications",
];

/// A question mark in the text a person wrote: quoted lines (`>`) and links do not count.
pub(crate) fn has_question(text: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .flat_map(str::split_whitespace)
        .filter(|word| !word.contains("://") && !word.starts_with("www."))
        .any(|word| word.contains(['?', '？', '¿']))
}

fn local_part(email: &str) -> String {
    email
        .split('@')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn automatic(row: &OpenReplyRow) -> bool {
    let auto = row
        .auto_submitted
        .as_deref()
        .is_some_and(|value| !value.trim().eq_ignore_ascii_case("no"));
    let bulk = row.precedence.as_deref().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "bulk" | "list" | "junk"
        )
    });
    let no_reply = row.from.as_ref().is_some_and(|from| {
        let local = local_part(&from.email);
        NO_REPLY
            .iter()
            .any(|word| local == *word || local.starts_with(&format!("{word}+")))
            || local.contains("noreply")
            || local.contains("no-reply")
    });
    // 1 notification, 2 newsletter.
    let mass = matches!(row.category, Some(1 | 2));
    auto || bulk || no_reply || mass || row.list
}

/// `later` answers or continues `earlier` (same account): same thread, or it names it.
fn follows(later: &OpenReplyRow, earlier: &OpenReplyRow) -> bool {
    if later.account != earlier.account || later.id == earlier.id {
        return false;
    }
    let after =
        later.date > earlier.date || (later.date == earlier.date && later.id.0 > earlier.id.0);
    if !after {
        return false;
    }
    if let (Some(a), Some(b)) = (later.thread_id, earlier.thread_id)
        && a == b
    {
        return true;
    }
    earlier.message_id_hdr.as_ref().is_some_and(|id| {
        later.in_reply_to.as_deref() == Some(id.as_str()) || later.references.contains(id)
    })
}

fn text_of(row: &OpenReplyRow) -> String {
    format!(
        "{}\n{}",
        row.subject,
        row.preview.as_deref().unwrap_or_default()
    )
}

/// The hints for `rows` (newest first), pure.
pub(crate) fn hints(
    rows: &[OpenReplyRow],
    own: &HashMap<AccountId, Vec<String>>,
    now: i64,
    settings: OpenReplySettings,
) -> Vec<OpenReplyHint> {
    if !settings.enabled {
        return Vec::new();
    }
    let mine = |row: &OpenReplyRow, email: &str| {
        own.get(&row.account)
            .is_some_and(|list| list.iter().any(|own| own.eq_ignore_ascii_case(email)))
    };
    let from_me =
        |row: &OpenReplyRow| row.in_sent || row.from.as_ref().is_some_and(|f| mine(row, &f.email));
    let awaiting_after = i64::from(settings.days.clamp(1, 30)) * DAY;
    // Rows that may follow a row: same thread, or naming its Message-ID.
    let mut by_thread: HashMap<(AccountId, i64), Vec<&OpenReplyRow>> = HashMap::new();
    let mut by_reference: HashMap<(AccountId, &str), Vec<&OpenReplyRow>> = HashMap::new();
    for row in rows {
        if let Some(thread) = row.thread_id {
            by_thread
                .entry((row.account, thread))
                .or_default()
                .push(row);
        }
        for id in row.in_reply_to.iter().chain(&row.references) {
            by_reference
                .entry((row.account, id.as_str()))
                .or_default()
                .push(row);
        }
    }
    let later = |row: &OpenReplyRow| -> Vec<&OpenReplyRow> {
        let thread = row
            .thread_id
            .and_then(|t| by_thread.get(&(row.account, t)))
            .into_iter()
            .flatten();
        let named = row
            .message_id_hdr
            .as_deref()
            .and_then(|id| by_reference.get(&(row.account, id)))
            .into_iter()
            .flatten();
        thread
            .chain(named)
            .copied()
            .filter(|other| follows(other, row))
            .collect()
    };
    let mut out = Vec::new();
    for row in rows {
        if row.dismissed || row.only_trash || now - row.date > WINDOW_DAYS * DAY {
            continue;
        }
        let age = now - row.date;
        let later = later(row);
        let kind = if from_me(row) {
            let to_others = row.to.iter().chain(&row.cc).any(|a| !mine(row, &a.email));
            let answered = later.iter().any(|l| !from_me(l));
            let continued = later.iter().any(|l| from_me(l));
            if age < awaiting_after
                || !to_others
                || answered
                || continued
                || !has_question(&text_of(row))
            {
                continue;
            }
            OpenReplyKind::AwaitingReply
        } else {
            let direct = row.to.iter().any(|a| mine(row, &a.email));
            let answered = row.answered || later.iter().any(|l| from_me(l));
            let superseded = later.iter().any(|l| !from_me(l));
            if !row.in_inbox
                || !direct
                || age < RECEIVED_AFTER
                || answered
                || superseded
                || automatic(row)
                || !has_question(&text_of(row))
            {
                continue;
            }
            OpenReplyKind::UnansweredQuestion
        };
        let counterpart = match kind {
            OpenReplyKind::AwaitingReply => row
                .to
                .iter()
                .chain(&row.cc)
                .find(|a| !mine(row, &a.email))
                .cloned(),
            OpenReplyKind::UnansweredQuestion => row.from.clone(),
        };
        out.push(OpenReplyHint {
            message_id: row.id,
            account_id: row.account,
            kind,
            subject: row.subject.clone(),
            counterpart,
            date: row.date,
            days: u32::try_from(age.div_euclid(DAY)).unwrap_or(u32::MAX),
        });
        if out.len() >= MAX_HINTS {
            break;
        }
    }
    out
}

impl<C: Connector> AppCore<C> {
    /// Open-reply hints over every account, newest first (no AI involved). Empty when they
    /// are turned off.
    pub async fn list_open_reply_hints(&self) -> Result<Vec<OpenReplyHint>, CoreError> {
        let settings = super::settings::load(self.store_ref()?).await?.open_replies;
        if !settings.enabled {
            return Ok(Vec::new());
        }
        let store = self.store_ref()?;
        let now = self.now_ms().div_euclid(1000);
        let rows = store
            .open_reply_rows(now - WINDOW_DAYS * DAY, MAX_ROWS)
            .await?;
        let own: HashMap<AccountId, Vec<String>> = store
            .own_addresses_by_account()
            .await?
            .into_iter()
            .collect();
        Ok(hints(&rows, &own, now, settings))
    }

    /// Dismisses the hint of a message for good.
    pub async fn dismiss_open_reply_hint(&self, message: MessageId) -> Result<(), CoreError> {
        if self
            .store_ref()?
            .dismiss_open_reply(message, self.now_ms())
            .await?
        {
            Ok(())
        } else {
            Err(CoreError::NotFound(format!("message {}", message.0)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_model::Address;

    const NOW: i64 = 1_795_392_000;

    fn me() -> AccountId {
        "00000000-0000-4000-8000-000000000001".parse().expect("id")
    }

    fn own() -> HashMap<AccountId, Vec<String>> {
        HashMap::from([(me(), vec!["alice@example.org".to_owned()])])
    }

    fn addr(email: &str) -> Address {
        Address {
            name: None,
            email: email.into(),
        }
    }

    fn row(id: i64, days_ago: i64, from: &str, to: &str, text: &str) -> OpenReplyRow {
        OpenReplyRow {
            id: MessageId(id),
            account: me(),
            thread_id: Some(id),
            message_id_hdr: Some(format!("m{id}@x")),
            in_reply_to: None,
            references: Vec::new(),
            date: NOW - days_ago * DAY,
            subject: "Subject".into(),
            from: Some(addr(from)),
            to: vec![addr(to)],
            cc: Vec::new(),
            list: false,
            auto_submitted: None,
            precedence: None,
            category: Some(0),
            preview: Some(text.into()),
            in_sent: from == "alice@example.org",
            in_inbox: from != "alice@example.org",
            only_trash: false,
            answered: false,
            dismissed: false,
        }
    }

    fn reply_to(mut row: OpenReplyRow, to: &OpenReplyRow) -> OpenReplyRow {
        row.thread_id = to.thread_id;
        row.in_reply_to.clone_from(&to.message_id_hdr);
        row
    }

    fn kinds(rows: &[OpenReplyRow]) -> Vec<(i64, OpenReplyKind)> {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|r| std::cmp::Reverse((r.date, r.id.0)));
        hints(&rows, &own(), NOW, OpenReplySettings::default())
            .into_iter()
            .map(|h| (h.message_id.0, h.kind))
            .collect()
    }

    #[test]
    fn questions_are_found_in_text_people_wrote() {
        assert!(has_question("Can you send it by Friday?"));
        assert!(has_question("Kommst du？"));
        assert!(!has_question("See https://example.org/?a=1 for details."));
        assert!(!has_question("Thanks!\n> Can you send it?"));
        assert!(!has_question("All done."));
    }

    #[test]
    fn a_sent_question_without_answer_is_awaiting_after_some_days() {
        let sent = row(
            1,
            4,
            "alice@example.org",
            "bob@example.org",
            "Can you send the offer?",
        );
        assert_eq!(
            kinds(std::slice::from_ref(&sent)),
            vec![(1, OpenReplyKind::AwaitingReply)]
        );
        // Too fresh.
        let fresh = row(
            2,
            1,
            "alice@example.org",
            "bob@example.org",
            "Lunch tomorrow?",
        );
        assert!(kinds(&[fresh]).is_empty());
        // Answered by Bob.
        let answer = reply_to(
            row(3, 2, "bob@example.org", "alice@example.org", "Sure."),
            &sent,
        );
        assert!(kinds(&[sent.clone(), answer]).is_empty());
        // No question.
        let statement = row(
            4,
            5,
            "alice@example.org",
            "bob@example.org",
            "Here is the file.",
        );
        assert!(kinds(&[statement]).is_empty());
        // To myself only.
        let note = row(
            5,
            5,
            "alice@example.org",
            "alice@example.org",
            "Remember the milk?",
        );
        assert!(kinds(&[note]).is_empty());
        // Dismissed.
        let mut dismissed = sent;
        dismissed.dismissed = true;
        assert!(kinds(&[dismissed]).is_empty());
    }

    #[test]
    fn a_direct_question_to_me_is_unanswered_until_i_reply() {
        let asked = row(
            10,
            2,
            "bob@example.org",
            "alice@example.org",
            "Do you have time on Monday?",
        );
        assert_eq!(
            kinds(std::slice::from_ref(&asked)),
            vec![(10, OpenReplyKind::UnansweredQuestion)]
        );
        let mut flagged = asked.clone();
        flagged.answered = true;
        assert!(kinds(&[flagged]).is_empty());
        let mine = reply_to(
            row(11, 1, "alice@example.org", "bob@example.org", "Yes."),
            &asked,
        );
        assert!(kinds(&[asked.clone(), mine]).is_empty());
        // Only in Cc: not direct.
        let mut cc = asked.clone();
        cc.to = vec![addr("carol@example.org")];
        cc.cc = vec![addr("alice@example.org")];
        assert!(kinds(&[cc]).is_empty());
        // Today: not yet.
        let today = row(12, 0, "bob@example.org", "alice@example.org", "Ready?");
        assert!(kinds(&[today]).is_empty());
        // In the trash.
        let mut trashed = asked;
        trashed.only_trash = true;
        assert!(kinds(&[trashed]).is_empty());
    }

    #[test]
    fn newsletters_and_automatic_mail_never_get_a_hint() {
        let base = || {
            row(
                20,
                3,
                "news@shop.example",
                "alice@example.org",
                "Ready for the sale?",
            )
        };
        let mut list = base();
        list.list = true;
        let mut newsletter = base();
        newsletter.category = Some(2);
        let mut notification = base();
        notification.category = Some(1);
        let mut auto = base();
        auto.auto_submitted = Some("auto-replied".into());
        let mut bulk = base();
        bulk.precedence = Some("bulk".into());
        let noreply = row(
            21,
            3,
            "no-reply@service.example",
            "alice@example.org",
            "Did you know?",
        );
        for case in [list, newsletter, notification, auto, bulk, noreply] {
            assert!(kinds(std::slice::from_ref(&case)).is_empty(), "{case:?}");
        }
        let mut human = base();
        human.auto_submitted = Some("no".into());
        assert_eq!(kinds(&[human]).len(), 1);
    }

    #[test]
    fn only_the_newest_message_of_a_side_counts_and_off_is_off() {
        let first = row(
            30,
            6,
            "bob@example.org",
            "alice@example.org",
            "Question one?",
        );
        let second = reply_to(
            row(31, 5, "bob@example.org", "alice@example.org", "And two?"),
            &first,
        );
        assert_eq!(
            kinds(&[first.clone(), second.clone()]),
            vec![(31, OpenReplyKind::UnansweredQuestion)]
        );
        let off = OpenReplySettings {
            enabled: false,
            days: 3,
        };
        assert!(hints(&[first, second], &own(), NOW, off).is_empty());
    }
}
