//! Which accounts may send mail to a cloud provider (stage 13: "je Konto festlegen, ob
//! Cloud-Anbieter dessen Mails sehen dürfen"). The setting lives in the engine; this crate
//! checks it right before every request that carries mail content, so no code path can skip
//! it: a profile that is not local gets nothing from an account without permission.

use mail_model::AccountId;

use crate::AiError;
use crate::endpoint::Locality;

/// The user's per-account choice. Closures work: `&|account: &AccountId| allowed.contains(account)`.
pub trait CloudPolicy: Send + Sync {
    fn cloud_allowed(&self, account: &AccountId) -> bool;
}

impl<F> CloudPolicy for F
where
    F: Fn(&AccountId) -> bool + Send + Sync,
{
    fn cloud_allowed(&self, account: &AccountId) -> bool {
        self(account)
    }
}

/// Local providers may see every account; others only accounts the policy allows.
pub(crate) fn allowed(locality: Locality, account: &AccountId, policy: &dyn CloudPolicy) -> bool {
    locality.is_local() || policy.cloud_allowed(account)
}

/// Fails with the accounts that are not allowed, before anything is sent.
pub(crate) fn check(
    locality: Locality,
    accounts: &[AccountId],
    policy: &dyn CloudPolicy,
) -> Result<(), AiError> {
    let refused: Vec<AccountId> = accounts
        .iter()
        .filter(|account| !allowed(locality, account, policy))
        .copied()
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(AiError::NotPermitted { accounts: refused })
    }
}
