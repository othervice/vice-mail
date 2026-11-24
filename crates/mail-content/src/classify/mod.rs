//! Inbox categories without AI (stage 9, decision 0016): Person, Notification or Newsletter, from
//! the header data and a few local signals, with the reason for the UI ("Why here?").
//!
//! Lives here because the header parsing does; pure, no IO. The first rule that matches decides:
//!
//! 1. the user's correction for the sender address, then the list, then the domain
//!    ([`SenderRules`]; always first, also over every rule below)
//! 2. `Auto-Submitted` other than `no` → Notification
//! 3. automatic reply (`Precedence: auto_reply`, `X-Autoreply`, `X-Autorespond`, subject such as
//!    "Automatische Antwort:") → Notification
//! 4. report (`multipart/report`, null `Return-Path`, From `MAILER-DAEMON`/`postmaster`) →
//!    Notification
//! 5. header of a known system (`X-GitHub-Reason`, `X-GitLab-Project`, `X-JIRA-FingerPrint`, …)
//!    → Notification
//! 6. From is one of the user's own addresses → Person
//! 7. answers a message the user sent → Person
//! 8. mailing list or mass mail: `List-Post` (discussion list), `List-Id`,
//!    `Precedence: bulk/list/junk`, campaign headers or newsletter software → Newsletter
//! 9. the user has written to the sender before → Person
//! 10. `List-Unsubscribe` → Newsletter
//! 11. no-reply address → Notification
//! 12. address of a program (`notifications@`, `rechnung@`, …) → Notification
//! 13. newsletter address (`newsletter@`, `news@`, …) → Newsletter
//! 14. sent through a mailing service (`Feedback-ID`, `X-SES-Outgoing`, a bounce address) →
//!     Notification
//! 15. otherwise → Person
//!
//! Independently, a sender, domain or list the user marked important puts the message into the
//! "Priority" block ([`CategoryDecision::important`]).
//!
//! Store [`classify_automatic`]'s result with the message and apply the user's rules with
//! [`apply_user_rules`] (again when they change); [`classify`] does both.

mod headers;
pub(crate) mod patterns;
mod rules;
mod signals;

#[cfg(test)]
mod tests;

use mail_model::{CategoryDecision, CategoryReason, MessageCategory};

pub use headers::{CLASSIFY_HEADER_FIELDS, ClassifyHeaders};
pub use patterns::is_no_reply_address;
use patterns::{SenderPattern, sender_pattern};
pub use rules::{SenderIdentity, SenderRules, normalize_key};
use signals::short;

use crate::contacts::normalize_address;

/// What the app knows locally about a message's sender.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalSignals<'a> {
    /// The user's own addresses (all identities of all accounts), any spelling.
    pub own_addresses: &'a [String],
    /// How many messages the user has sent to the From address (contacts); 0 if none.
    pub sent_to_sender: u32,
    /// `In-Reply-To` or `References` names a message the user sent.
    pub replies_to_own: bool,
}

/// The category from the message and the local signals, without the user's rules.
pub fn classify_automatic(headers: &ClassifyHeaders, local: &LocalSignals<'_>) -> CategoryDecision {
    let (category, reason) = automatic(headers, local);
    CategoryDecision {
        category,
        reason,
        important: None,
    }
}

/// Applies the user's correction and important marks to an automatic decision.
pub fn apply_user_rules(
    decision: CategoryDecision,
    sender: &SenderIdentity,
    rules: &SenderRules,
) -> CategoryDecision {
    let important = rules.important(sender).cloned();
    match rules.correction(sender) {
        Some(correction) => CategoryDecision {
            category: correction.category,
            reason: CategoryReason::UserRule {
                key: correction.key.clone(),
            },
            important,
        },
        None => CategoryDecision {
            important,
            ..decision
        },
    }
}

/// [`classify_automatic`] and then [`apply_user_rules`].
pub fn classify(
    headers: &ClassifyHeaders,
    local: &LocalSignals<'_>,
    rules: &SenderRules,
) -> CategoryDecision {
    apply_user_rules(
        classify_automatic(headers, local),
        &SenderIdentity::of(headers),
        rules,
    )
}

fn automatic(
    headers: &ClassifyHeaders,
    local: &LocalSignals<'_>,
) -> (MessageCategory, CategoryReason) {
    use CategoryReason as R;
    use MessageCategory::{Newsletter, Notification, Person};

    if let Some(value) = headers
        .auto_submitted
        .as_deref()
        .filter(|v| !v.eq_ignore_ascii_case("no"))
    {
        return (
            Notification,
            R::AutoSubmitted {
                value: short(value),
            },
        );
    }
    if let Some(evidence) = signals::auto_reply(headers) {
        return (Notification, R::AutoReply { evidence });
    }
    if let Some(evidence) = signals::report(headers) {
        return (Notification, R::Report { evidence });
    }
    if let Some((system, header)) = signals::known_system(headers) {
        return (
            Notification,
            R::KnownSystem {
                system: system.to_owned(),
                header: header.to_owned(),
            },
        );
    }
    let from = headers.from_email();
    if from.is_some_and(|f| is_own(f, local.own_addresses)) {
        return (Person, R::OwnMessage);
    }
    if local.replies_to_own {
        return (Person, R::ReplyToOwn);
    }
    if let Some(post) = headers.list_post.as_deref()
        && !post.trim().eq_ignore_ascii_case("no")
    {
        return (
            Newsletter,
            R::DiscussionList {
                list_id: headers.list_id.as_deref().map(short),
            },
        );
    }
    if let Some(list_id) = headers.list_id.as_deref() {
        return (
            Newsletter,
            R::MailingList {
                list_id: short(list_id),
            },
        );
    }
    if let Some(value) = headers.precedence.as_deref().filter(|p| {
        ["bulk", "list", "junk"]
            .iter()
            .any(|bulk| p.eq_ignore_ascii_case(bulk))
    }) {
        return (
            Newsletter,
            R::Precedence {
                value: value.to_ascii_lowercase(),
            },
        );
    }
    if let Some(evidence) = signals::campaign(headers) {
        return (Newsletter, R::Campaign { evidence });
    }
    let pattern = from.and_then(sender_pattern);
    let no_reply = matches!(pattern, Some(SenderPattern::NoReply(_)));
    if local.sent_to_sender > 0 && !no_reply {
        return (
            Person,
            R::Correspondent {
                sent: local.sent_to_sender,
            },
        );
    }
    if headers.list_unsubscribe {
        return (Newsletter, R::Unsubscribe);
    }
    let address = || short(from.unwrap_or_default());
    match pattern {
        Some(SenderPattern::NoReply(_)) => {
            return (Notification, R::NoReplyAddress { address: address() });
        }
        Some(SenderPattern::Automated(p)) => {
            return (
                Notification,
                R::AutomatedAddress {
                    address: address(),
                    pattern: p.to_owned(),
                },
            );
        }
        Some(SenderPattern::Newsletter(p)) => {
            return (
                Newsletter,
                R::NewsletterAddress {
                    address: address(),
                    pattern: p.to_owned(),
                },
            );
        }
        None => {}
    }
    if let Some(evidence) = signals::mailing_service(headers) {
        return (Notification, R::MailingService { evidence });
    }
    (Person, R::NoBulkSignals)
}

/// Whether `email` is one of `own` (case ignored, IDNA, also as `name+tag@`).
fn is_own(email: &str, own: &[String]) -> bool {
    let Some(address) = normalize_address(email) else {
        return false;
    };
    let untagged = address.without_tag();
    own.iter()
        .filter_map(|o| normalize_address(o))
        .any(|o| o.key == address.key || untagged.as_deref() == Some(o.key.as_str()))
}
