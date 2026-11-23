//! Local rules (SPEC "Lokale Regeln", stage 9): which rules match a new message and what they
//! ask for. Pure; `comfort` carries the plan out (local marks at once, moving and marking as
//! read through the outbox).
//!
//! **Order.** Rules run by `position`. For each kind of action the first matching rule wins
//! (one move, one category, one notify decision); marking as read and pinning happen once if
//! any rule asks. A matching rule with `stop` ends the evaluation for that message. Disabled
//! rules are skipped. A rule without conditions matches every message.
//!
//! **Text** comparisons ignore case and accents (`mail_content::fold`). For addresses, `is`
//! compares the normalised address (IDNA, case), `domain` the domain or a subdomain of it,
//! `contains` searches name and address.

use mail_content::{fold, normalize_address};
use mail_model::{
    AccountId, Address, MailRule, MessageCategory, RuleAction, RuleCondition, RuleId, TextMatchOp,
};
use mail_store::ArrivedMessage;

/// What the rules ask for one message, with the rule behind each part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RulePlan {
    pub move_to: Option<(RuleRef, AccountId, String)>,
    pub mark_read: Option<RuleRef>,
    pub category: Option<(RuleRef, MessageCategory)>,
    pub pin: Option<RuleRef>,
    pub notify: Option<bool>,
}

impl RulePlan {
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleRef {
    pub id: RuleId,
    pub name: String,
}

fn text_matches(op: TextMatchOp, haystack: &str, needle: &str) -> bool {
    let (haystack, needle) = (fold(haystack), fold(needle.trim()));
    if needle.is_empty() {
        return false;
    }
    match op {
        TextMatchOp::Contains => haystack.contains(&needle),
        TextMatchOp::Is | TextMatchOp::Domain => haystack.trim() == needle,
    }
}

fn address_matches(op: TextMatchOp, address: &Address, value: &str) -> bool {
    match op {
        TextMatchOp::Contains => {
            let both = format!(
                "{} {}",
                address.name.as_deref().unwrap_or(""),
                address.email
            );
            text_matches(TextMatchOp::Contains, &both, value)
        }
        TextMatchOp::Is => match (normalize_address(&address.email), normalize_address(value)) {
            (Some(a), Some(b)) => a.key == b.key,
            _ => text_matches(TextMatchOp::Is, &address.email, value),
        },
        TextMatchOp::Domain => {
            let Some(a) = normalize_address(&address.email) else {
                return false;
            };
            let wanted = value.trim().trim_start_matches('@').to_ascii_lowercase();
            let wanted = mail_content::normalize_domain(&wanted).unwrap_or(wanted);
            !wanted.is_empty() && (a.domain == wanted || a.domain.ends_with(&format!(".{wanted}")))
        }
    }
}

fn condition_holds(condition: &RuleCondition, message: &ArrivedMessage) -> bool {
    match condition {
        RuleCondition::From { op, value } => message
            .from
            .as_ref()
            .is_some_and(|from| address_matches(*op, from, value)),
        RuleCondition::To { op, value } => message
            .to
            .iter()
            .chain(message.cc.iter())
            .any(|a| address_matches(*op, a, value)),
        RuleCondition::Subject { op, value } => text_matches(*op, &message.subject, value),
        RuleCondition::List { op, value } => message.list_id.as_deref().is_some_and(|list| {
            let value = value.trim().trim_start_matches('<').trim_end_matches('>');
            text_matches(*op, list, value)
        }),
        RuleCondition::Account { account_id } => *account_id == message.account_id,
    }
}

/// Whether `rule` matches `message` (enabled or not).
pub(crate) fn matches(rule: &MailRule, message: &ArrivedMessage) -> bool {
    if rule.conditions.is_empty() {
        return true;
    }
    if rule.match_all {
        rule.conditions.iter().all(|c| condition_holds(c, message))
    } else {
        rule.conditions.iter().any(|c| condition_holds(c, message))
    }
}

/// The plan of the enabled `rules` (in order) for `message`.
pub(crate) fn plan(rules: &[MailRule], message: &ArrivedMessage) -> RulePlan {
    let mut out = RulePlan::default();
    for rule in rules.iter().filter(|r| r.enabled) {
        if !matches(rule, message) {
            continue;
        }
        let at = || RuleRef {
            id: rule.id.clone(),
            name: rule.name.clone(),
        };
        for action in &rule.actions {
            match action {
                RuleAction::Move { account_id, path } => {
                    if out.move_to.is_none() {
                        out.move_to = Some((at(), *account_id, path.clone()));
                    }
                }
                RuleAction::MarkRead => {
                    if out.mark_read.is_none() {
                        out.mark_read = Some(at());
                    }
                }
                RuleAction::SetCategory { category } => {
                    if out.category.is_none() {
                        out.category = Some((at(), *category));
                    }
                }
                RuleAction::Pin => {
                    if out.pin.is_none() {
                        out.pin = Some(at());
                    }
                }
                RuleAction::Notify { enabled } => {
                    if out.notify.is_none() {
                        out.notify = Some(*enabled);
                    }
                }
            }
        }
        if rule.stop {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use mail_model::{MailboxId, MessageId};

    use super::*;

    fn account(n: u8) -> AccountId {
        format!("00000000-0000-4000-8000-0000000000{n:02}")
            .parse()
            .expect("uuid")
    }

    fn message() -> ArrivedMessage {
        ArrivedMessage {
            id: MessageId(1),
            mailbox_id: MailboxId(1),
            account_id: account(1),
            unread: true,
            subject: "Rechnung März für Müller".into(),
            from: Some(Address {
                name: Some("Stadtwerke Köln".into()),
                email: "Rechnung@Mail.Stadtwerke.Example".into(),
            }),
            to: vec![Address {
                name: None,
                email: "alice@example.org".into(),
            }],
            cc: vec![Address {
                name: Some("Bob".into()),
                email: "bob+family@example.net".into(),
            }],
            list_id: Some("news.stadtwerke.example".into()),
            category: Some(MessageCategory::Notification),
            important: false,
            message_id_hdr: Some("1@x".into()),
        }
    }

    fn rule(n: u8, conditions: Vec<RuleCondition>, actions: Vec<RuleAction>) -> MailRule {
        MailRule {
            id: RuleId(format!("rule-{n}")),
            name: format!("Rule {n}"),
            enabled: true,
            match_all: true,
            conditions,
            actions,
            stop: false,
            position: i64::from(n),
        }
    }

    fn holds(condition: RuleCondition) -> bool {
        matches(
            &rule(1, vec![condition], vec![RuleAction::MarkRead]),
            &message(),
        )
    }

    #[test]
    fn every_condition() {
        use RuleCondition as C;
        use TextMatchOp::{Contains, Domain, Is};
        let s = |v: &str| v.to_owned();
        // From: name or address, case and accents ignored; the whole address; the domain.
        assert!(holds(C::From {
            op: Contains,
            value: s("koln")
        }));
        assert!(holds(C::From {
            op: Contains,
            value: s("STADTWERKE.example")
        }));
        assert!(holds(C::From {
            op: Is,
            value: s("rechnung@mail.stadtwerke.example")
        }));
        assert!(!holds(C::From {
            op: Is,
            value: s("rechnung@stadtwerke.example")
        }));
        assert!(holds(C::From {
            op: Domain,
            value: s("stadtwerke.example")
        }));
        assert!(holds(C::From {
            op: Domain,
            value: s("@Mail.Stadtwerke.example")
        }));
        assert!(
            !holds(C::From {
                op: Domain,
                value: s("werke.example")
            }),
            "label boundary"
        );
        // To: To and Cc.
        assert!(holds(C::To {
            op: Is,
            value: s("ALICE@example.org")
        }));
        assert!(holds(C::To {
            op: Contains,
            value: s("bob")
        }));
        assert!(holds(C::To {
            op: Domain,
            value: s("example.net")
        }));
        assert!(
            !holds(C::To {
                op: Is,
                value: s("bob@example.net")
            }),
            "tags stay apart"
        );
        // Subject.
        assert!(holds(C::Subject {
            op: Contains,
            value: s("marz")
        }));
        assert!(holds(C::Subject {
            op: Contains,
            value: s("MÜLLER")
        }));
        assert!(!holds(C::Subject {
            op: Is,
            value: s("Rechnung")
        }));
        assert!(holds(C::Subject {
            op: Is,
            value: s("rechnung marz fur muller")
        }));
        assert!(
            !holds(C::Subject {
                op: Contains,
                value: s("  ")
            }),
            "empty never matches"
        );
        // List.
        assert!(holds(C::List {
            op: Is,
            value: s("<news.stadtwerke.example>")
        }));
        assert!(holds(C::List {
            op: Contains,
            value: s("stadtwerke")
        }));
        assert!(!holds(C::List {
            op: Is,
            value: s("other.example")
        }));
        // Account.
        assert!(holds(C::Account {
            account_id: account(1)
        }));
        assert!(!holds(C::Account {
            account_id: account(2)
        }));
    }

    #[test]
    fn all_or_any_and_empty() {
        let yes = RuleCondition::Subject {
            op: TextMatchOp::Contains,
            value: "rechnung".into(),
        };
        let no = RuleCondition::Subject {
            op: TextMatchOp::Contains,
            value: "urlaub".into(),
        };
        let mut r = rule(1, vec![yes.clone(), no.clone()], vec![RuleAction::MarkRead]);
        assert!(!matches(&r, &message()));
        r.match_all = false;
        assert!(matches(&r, &message()));
        r.conditions.clear();
        assert!(matches(&r, &message()), "no condition: every message");
    }

    #[test]
    fn every_action_and_the_order() {
        let target = (account(1), "Rechnungen".to_owned());
        let all = rule(
            1,
            vec![],
            vec![
                RuleAction::Move {
                    account_id: target.0,
                    path: target.1.clone(),
                },
                RuleAction::MarkRead,
                RuleAction::SetCategory {
                    category: MessageCategory::Person,
                },
                RuleAction::Pin,
                RuleAction::Notify { enabled: true },
            ],
        );
        let later = rule(
            2,
            vec![],
            vec![
                RuleAction::Move {
                    account_id: target.0,
                    path: "Other".into(),
                },
                RuleAction::SetCategory {
                    category: MessageCategory::Newsletter,
                },
                RuleAction::Notify { enabled: false },
            ],
        );
        let p = plan(&[all.clone(), later.clone()], &message());
        assert_eq!(
            p.move_to
                .as_ref()
                .map(|(r, _, path)| (r.id.0.as_str(), path.as_str())),
            Some(("rule-1", "Rechnungen"))
        );
        assert_eq!(
            p.mark_read.as_ref().map(|r| r.id.0.as_str()),
            Some("rule-1")
        );
        assert_eq!(
            p.category.as_ref().map(|(_, c)| *c),
            Some(MessageCategory::Person)
        );
        assert!(p.pin.is_some());
        assert_eq!(p.notify, Some(true));
        // The other order: the second rule's parts win where it has them.
        let p = plan(&[later.clone(), all.clone()], &message());
        assert_eq!(
            p.move_to.as_ref().map(|(_, _, path)| path.as_str()),
            Some("Other")
        );
        assert_eq!(
            p.category.as_ref().map(|(_, c)| *c),
            Some(MessageCategory::Newsletter)
        );
        assert_eq!(p.notify, Some(false));
        assert!(p.mark_read.is_some() && p.pin.is_some());
        // `stop` ends the evaluation; disabled rules are skipped.
        let mut stopper = later.clone();
        stopper.stop = true;
        let p = plan(&[stopper, all.clone()], &message());
        assert!(p.mark_read.is_none() && p.pin.is_none());
        let mut off = later;
        off.enabled = false;
        let p = plan(&[off, all], &message());
        assert_eq!(
            p.move_to.as_ref().map(|(_, _, path)| path.as_str()),
            Some("Rechnungen")
        );
        assert!(plan(&[], &message()).is_empty());
    }
}
