//! From a search request to the store's [`SearchFilter`]: parse and compile with the user's
//! time zone, resolve the scope, `account:` and `in:`, collect problems and chips.

use std::collections::{BTreeSet, HashSet};

use mail_model::search::{
    Clause, CompileOptions, CompiledQuery, Filter, MailboxSpec, ParsedQuery, SearchContext,
    TimeZone, parse,
};
use mail_model::{
    AccountId, CoreError, MailboxId, SearchProblem, SearchProblemCode, SearchRequest, SearchScope,
    SearchTermInfo, utf16_range,
};
use mail_store::{MailboxName, SearchFilter, Store, StoredAccount};

/// The system's time zone (jiff: `/etc/localtime` and the zoneinfo files on macOS, the
/// Windows zone with a bundled tzdb on Windows). UTC if the system's zone cannot be read.
pub struct LocalZone(jiff::tz::TimeZone);

impl LocalZone {
    pub fn system() -> Self {
        Self(jiff::tz::TimeZone::system())
    }

    /// A named zone, for tests (`Europe/Berlin`).
    pub fn named(name: &str) -> Option<Self> {
        jiff::tz::TimeZone::get(name).ok().map(Self)
    }
}

impl TimeZone for LocalZone {
    fn utc_offset(&self, utc: i64) -> i32 {
        jiff::Timestamp::from_second(utc)
            .map(|ts| self.0.to_offset(ts).seconds())
            .unwrap_or(0)
    }
}

/// Everything a search needs, resolved.
pub(crate) struct Plan {
    pub(crate) parsed: ParsedQuery,
    pub(crate) filter: SearchFilter,
    pub(crate) problems: Vec<SearchProblem>,
    pub(crate) chips: Vec<SearchTermInfo>,
    /// Nothing can match: a contradiction, an unknown account or mailbox, `is:snoozed`.
    pub(crate) matches_nothing: bool,
    /// No condition at all (empty input).
    pub(crate) empty: bool,
    /// The accounts searched, in account order.
    pub(crate) accounts: Vec<StoredAccount>,
}

fn problem(code: SearchProblemCode, span: mail_model::search::Span, input: &str) -> SearchProblem {
    SearchProblem {
        code,
        range: utf16_range(span, input),
    }
}

/// Whether `value` (as typed after `account:`) names `account`: its address, its name or its
/// UUID (ignoring case), or the domain of its address (`account:example.org`).
fn names_account(value: &str, account: &StoredAccount) -> bool {
    let value = value.trim();
    let email = account.email.to_lowercase();
    let v = value.to_lowercase();
    email == v
        || account.display_name.to_lowercase() == v
        || account.id.to_string() == v
        || email
            .rsplit_once('@')
            .is_some_and(|(_, domain)| domain == v.trim_start_matches('@'))
}

/// Mailboxes `spec` names: those with its role, else those whose full path or last part is
/// the name (ignoring case).
fn names_mailboxes(spec: &MailboxSpec, mailboxes: &[MailboxName]) -> Vec<MailboxId> {
    if let Some(role) = spec.role {
        let by_role: Vec<MailboxId> = mailboxes
            .iter()
            .filter(|m| m.role == role)
            .map(|m| m.id)
            .collect();
        if !by_role.is_empty() {
            return by_role;
        }
    }
    let name = spec.name.to_lowercase();
    mailboxes
        .iter()
        .filter(|m| m.path.to_lowercase() == name || m.name.to_lowercase() == name)
        .map(|m| m.id)
        .collect()
}

/// Parses and compiles the query (before any `.await`: the context borrows a time zone that
/// is not `Sync`).
pub(crate) fn compile(
    request: &SearchRequest,
    context: &SearchContext<'_>,
    options: CompileOptions,
) -> (ParsedQuery, CompiledQuery) {
    let parsed = parse(&request.query, context);
    let compiled = parsed.compile(context, options);
    (parsed, compiled)
}

pub(crate) async fn plan(
    store: &Store,
    request: &SearchRequest,
    (parsed, compiled): (ParsedQuery, CompiledQuery),
) -> Result<Plan, CoreError> {
    let input = request.query.as_str();
    let mut problems: Vec<SearchProblem> = parsed
        .errors
        .iter()
        .map(|e| problem(e.kind.into(), e.span, input))
        .collect();
    problems.extend(
        compiled
            .conflicts
            .iter()
            .map(|span| problem(SearchProblemCode::Conflict, *span, input)),
    );
    let chips: Vec<SearchTermInfo> = parsed
        .clauses
        .iter()
        .flat_map(|clause| match clause {
            Clause::Term(term) => vec![SearchTermInfo::from_term(term, input, false)],
            Clause::AnyOf(terms) => terms
                .iter()
                .map(|t| SearchTermInfo::from_term(t, input, true))
                .collect(),
        })
        .collect();

    // The scope.
    let all = store.accounts().await?;
    let hidden = store.hidden_accounts().await?;
    let mut scope_mailbox = None;
    let base: Vec<AccountId> = match &request.scope {
        SearchScope::All => all
            .iter()
            .map(|a| a.id)
            .filter(|id| !hidden.contains(id))
            .collect(),
        SearchScope::Account { account_id } => {
            if !all.iter().any(|a| a.id == *account_id) {
                return Err(CoreError::NotFound(format!("account {account_id}")));
            }
            vec![*account_id]
        }
        SearchScope::Mailbox { mailbox_id } => {
            let owner = store.mailbox_owner(*mailbox_id).await?;
            scope_mailbox = Some(*mailbox_id);
            vec![owner]
        }
        SearchScope::Group { group_id } => store.visible_accounts(Some(group_id.clone())).await?,
    };

    // `account:`: every positive value narrows, every negative one removes.
    let mut matches_nothing = compiled.matches_nothing();
    let mut allowed: BTreeSet<AccountId> = base.iter().copied().collect();
    for term in parsed.terms() {
        let Filter::Account(value) = &term.filter else {
            continue;
        };
        let named: HashSet<AccountId> = all
            .iter()
            .filter(|a| names_account(value, a))
            .map(|a| a.id)
            .collect();
        if named.is_empty() {
            problems.push(problem(SearchProblemCode::UnknownAccount, term.span, input));
            if !term.negated {
                matches_nothing = true;
            }
            continue;
        }
        if term.negated {
            allowed.retain(|a| !named.contains(a));
        } else {
            allowed.retain(|a| named.contains(a));
        }
    }
    let accounts: Vec<StoredAccount> = all
        .iter()
        .filter(|a| allowed.contains(&a.id))
        .cloned()
        .collect();
    // Mailbox names (without counters) only when `in:` needs them.
    let mut searched: Vec<MailboxName> = Vec::new();
    if parsed.terms().any(|t| matches!(t.filter, Filter::In(_))) {
        for account in &accounts {
            searched.extend(store.mailbox_names(account.id).await?);
        }
    }

    // `in:` and `-in:`.
    let mut mailbox_sets: Vec<Vec<MailboxId>> =
        scope_mailbox.into_iter().map(|m| vec![m]).collect();
    let mut excluded = Vec::new();
    for term in parsed.terms() {
        let Filter::In(spec) = &term.filter else {
            continue;
        };
        let named = names_mailboxes(spec, &searched);
        if named.is_empty() {
            problems.push(problem(SearchProblemCode::UnknownMailbox, term.span, input));
            if !term.negated {
                matches_nothing = true;
            }
            continue;
        }
        if term.negated {
            excluded.extend(named);
        } else {
            mailbox_sets.push(named);
        }
    }
    // Nothing is snoozed before stage 9.
    if compiled.snoozed == Some(true) {
        matches_nothing = true;
    }
    if allowed.is_empty() {
        matches_nothing = true;
    }
    problems.sort_by_key(|p| (p.range.start, p.range.end));

    let every_account = allowed.len() == all.len();
    let filter = SearchFilter {
        text: compiled.text.clone(),
        dates: compiled.dates,
        accounts: (!every_account).then(|| allowed.iter().copied().collect()),
        mailbox_sets,
        excluded_mailboxes: excluded,
        unread: compiled.unread,
        flagged: compiled.flagged,
        has_attachment: compiled.has_attachment,
        only: None,
        without_complete_body: false,
    };
    let empty = compiled.is_unrestricted();
    Ok(Plan {
        parsed,
        filter,
        problems,
        chips,
        matches_nothing,
        empty,
        accounts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_zone_knows_daylight_saving() {
        let berlin = LocalZone::named("Europe/Berlin").expect("tzdb has Berlin");
        // 2026-01-15 and 2026-07-15, noon UTC.
        assert_eq!(berlin.utc_offset(1_768_478_400), 3600);
        assert_eq!(berlin.utc_offset(1_784_116_800), 7200);
        // The system zone answers something sensible.
        let offset = LocalZone::system().utc_offset(1_784_116_800);
        assert!((-14 * 3600..=14 * 3600).contains(&offset));
    }

    #[test]
    fn account_and_mailbox_names() {
        let account = StoredAccount {
            id: "00000000-0000-4000-8000-000000000001".parse().expect("id"),
            email: "Alice@Example.org".into(),
            display_name: "Alice Work".into(),
            color: mail_model::AccountColor::new(0).expect("color"),
            imap: mail_model::ServerSettings {
                host: "h".into(),
                port: 993,
                security: mail_model::Security::Tls,
                username: "u".into(),
            },
            smtp: mail_model::ServerSettings {
                host: "h".into(),
                port: 465,
                security: mail_model::Security::Tls,
                username: "u".into(),
            },
            credential_ref: "r".into(),
            writes_enabled: false,
            created_at: 0,
        };
        for yes in [
            "alice@example.org",
            "ALICE WORK",
            "example.org",
            "@example.org",
            "00000000-0000-4000-8000-000000000001",
        ] {
            assert!(names_account(yes, &account), "{yes}");
        }
        for no in ["alice", "example", "bob@example.org"] {
            assert!(!names_account(no, &account), "{no}");
        }
        let mailbox = |id: i64, path: &str, role| MailboxName {
            id: MailboxId(id),
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or(path).into(),
            role,
        };
        let boxes = [
            mailbox(1, "INBOX", mail_model::MailboxRole::Inbox),
            mailbox(2, "Archiv", mail_model::MailboxRole::Archive),
            mailbox(3, "Projekte/Archive", mail_model::MailboxRole::Custom),
        ];
        let spec = |name: &str, role| MailboxSpec {
            name: name.into(),
            role,
        };
        // Role before name.
        assert_eq!(
            names_mailboxes(
                &spec("archive", Some(mail_model::MailboxRole::Archive)),
                &boxes
            ),
            [MailboxId(2)]
        );
        assert_eq!(
            names_mailboxes(&spec("projekte/archive", None), &boxes),
            [MailboxId(3)]
        );
        assert_eq!(
            names_mailboxes(&spec("ARCHIV", None), &boxes),
            [MailboxId(2)]
        );
        assert!(names_mailboxes(&spec("nope", None), &boxes).is_empty());
    }
}
