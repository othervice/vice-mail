//! Search on the server (stage 7): IMAP `UID SEARCH` over each account's reading connection,
//! which is always read-only (EXAMINE; SEARCH is on the adapter's allowlist). The UIDs found are
//! mapped to stored messages (headers are always local) and filtered like a local search,
//! keeping only messages whose whole body is not stored (the local index covers the others).

use std::collections::BTreeSet;

use mail_model::search::{CompileOptions, CompiledQuery, ParsedQuery, ServerQuery};
use mail_model::{
    BackendError, CoreError, MailboxId, MailboxRole, MessageId, SearchProblem, SearchProblemCode,
    SearchRequest, ServerSearchResult, utf16_range,
};
use mail_store::{MailboxSyncState, MessageLocation, SearchFilter};

use super::{hit, resolve};
use crate::core::{AppCore, Connector};

/// Most server hits filtered locally per search (the newest).
const MAX_CANDIDATES: usize = 5_000;

/// Where to search when the query names no mailbox: every selectable mailbox but trash and
/// junk; with an "All Mail" folder only that one (it holds the others' messages).
fn default_mailboxes(states: &[MailboxSyncState]) -> Vec<&MailboxSyncState> {
    let usable: Vec<&MailboxSyncState> = states
        .iter()
        .filter(|s| s.selectable && s.uid_validity.is_some())
        .collect();
    if let Some(all) = usable.iter().find(|s| s.role == MailboxRole::All) {
        return vec![*all];
    }
    usable
        .into_iter()
        .filter(|s| !matches!(s.role, MailboxRole::Trash | MailboxRole::Junk))
        .collect()
}

pub(crate) async fn search<C: Connector>(
    core: &AppCore<C>,
    request: &SearchRequest,
    compiled: (ParsedQuery, CompiledQuery),
    server: ServerQuery,
) -> Result<ServerSearchResult, CoreError> {
    let store = core.store_ref()?;
    let reading = core.reading_ref()?;
    let options = CompileOptions::default();
    let plan = resolve::plan(store, request, compiled).await?;
    let input = request.query.as_str();
    let mut problems = plan.problems.clone();
    problems.extend(server.local_only.iter().map(|span| SearchProblem {
        code: SearchProblemCode::NotOnServer,
        range: utf16_range(*span, input),
    }));
    problems.sort_by_key(|p| (p.range.start, p.range.end));
    let mut result = ServerSearchResult {
        hits: Vec::new(),
        truncated: false,
        searched: Vec::new(),
        failed: Vec::new(),
        not_synced: 0,
        problems,
    };
    // Without a criterion the server would list every message.
    if plan.matches_nothing || server.is_empty() {
        return Ok(result);
    }

    // The mailboxes named by the scope or `in:`, else the default ones.
    let named: Option<BTreeSet<MailboxId>> = (!plan.filter.mailbox_sets.is_empty())
        .then(|| plan.filter.mailbox_sets.iter().flatten().copied().collect());
    let mut candidates: Vec<MessageId> = Vec::new();
    for account in &plan.accounts {
        let states = store.mailbox_sync_states(account.id).await?;
        let targets: Vec<&MailboxSyncState> = match &named {
            Some(named) => states
                .iter()
                .filter(|s| named.contains(&s.mailbox_id) && s.selectable)
                .collect(),
            None => default_mailboxes(&states),
        };
        if targets.is_empty() {
            continue;
        }
        let mut held = match reading.connections.hold(account).await {
            Ok(held) => held,
            Err(err) => {
                tracing::info!(account = %account.id, code = ?err.code(), "server search: account not reachable");
                result.failed.extend(targets.iter().map(|s| s.mailbox_id));
                continue;
            }
        };
        let mut lost = false;
        for state in targets {
            if lost {
                result.failed.push(state.mailbox_id);
                continue;
            }
            let location = MessageLocation {
                mailbox_id: state.mailbox_id,
                mailbox_name: state.name.clone(),
                role: state.role,
                selectable: state.selectable,
                uid_validity: state.uid_validity,
                uid: 0,
            };
            let found = async {
                let Some(selected) = held.open(&location).await? else {
                    return Ok(None);
                };
                let uids = held.search(&selected, &server.criteria).await?;
                Ok::<_, BackendError>(Some((selected.uid_validity, uids)))
            }
            .await;
            match found {
                Ok(Some((uid_validity, uids))) => {
                    let (ids, missing) = store
                        .messages_for_uids(state.mailbox_id, uid_validity, uids)
                        .await?;
                    candidates.extend(ids);
                    result.not_synced = result
                        .not_synced
                        .saturating_add(u32::try_from(missing).unwrap_or(u32::MAX));
                    result.searched.push(state.mailbox_id);
                }
                // Gone or UIDVALIDITY changed: the next sync catches up.
                Ok(None) => result.failed.push(state.mailbox_id),
                Err(err) => {
                    tracing::info!(mailbox = state.mailbox_id.0, error = %err, "server search failed in a mailbox");
                    result.failed.push(state.mailbox_id);
                    lost = !matches!(
                        err,
                        BackendError::Rejected(_) | BackendError::MailboxNotFound(_)
                    );
                }
            }
        }
    }
    candidates.sort_unstable_by(|a, b| b.cmp(a));
    candidates.dedup();
    if candidates.is_empty() {
        return Ok(result);
    }
    // The newest ones; the local filter runs over at most this many ids.
    if candidates.len() > MAX_CANDIDATES {
        candidates.truncate(MAX_CANDIDATES);
        result.truncated = true;
    }
    let filter = SearchFilter {
        only: Some(candidates),
        without_complete_body: true,
        ..plan.filter.clone()
    };
    let page = store.search(filter, request.cursor, request.limit).await?;
    let terms = plan.parsed.highlight_terms(options);
    result.truncated |= page.next_cursor.is_some();
    result.hits = page.hits.into_iter().map(|h| hit(h, &terms)).collect();
    Ok(result)
}
