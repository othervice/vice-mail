//! Stage 20, task E (decision 0060): how full the mailbox on the server is. IMAP `QUOTA`
//! (RFC 9208, `GETQUOTAROOT INBOX`, the `STORAGE` resource) and JMAP quotas (RFC 9425, the
//! `octets` quota of the account for mail). What a server does not report is said, never
//! guessed.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::IpcError;

/// What a backend reports: bytes in use and the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageQuota {
    pub used_bytes: u64,
    pub limit_bytes: u64,
}

impl StorageQuota {
    /// Whole percent in use, 0 to 100 (more than the limit counts as 100).
    pub fn percent(self) -> u32 {
        if self.limit_bytes == 0 {
            return 100;
        }
        let percent = u128::from(self.used_bytes) * 100 / u128::from(self.limit_bytes);
        u32::try_from(percent.min(100)).unwrap_or(100)
    }
}

/// From this share on, the account shows a notice.
pub const QUOTA_WARN_PERCENT: u32 = 90;

/// How full an account's mailbox is (`getAccountQuota`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MailboxQuota {
    /// The server reports its storage.
    Reported {
        #[specta(type = Number)]
        used_bytes: u64,
        #[specta(type = Number)]
        limit_bytes: u64,
        /// 0 to 100.
        percent: u32,
        /// At least [`QUOTA_WARN_PERCENT`].
        nearly_full: bool,
    },
    /// The server does not report storage (no QUOTA extension, no quota for mail, or no
    /// storage limit).
    NotReported,
    /// Could not be asked now (offline, a refusal); `error` says why.
    Unknown { error: Option<IpcError> },
}

impl MailboxQuota {
    pub fn reported(quota: StorageQuota) -> Self {
        let percent = quota.percent();
        Self::Reported {
            used_bytes: quota.used_bytes,
            limit_bytes: quota.limit_bytes,
            percent,
            nearly_full: percent >= QUOTA_WARN_PERCENT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_the_notice() {
        let q = |used, limit| StorageQuota {
            used_bytes: used,
            limit_bytes: limit,
        };
        assert_eq!(q(0, 100).percent(), 0);
        assert_eq!(q(899, 1000).percent(), 89);
        assert_eq!(q(900, 1000).percent(), 90);
        assert_eq!(q(5000, 1000).percent(), 100);
        assert_eq!(q(1, 0).percent(), 100);
        assert!(matches!(
            MailboxQuota::reported(q(95, 100)),
            MailboxQuota::Reported {
                nearly_full: true,
                percent: 95,
                ..
            }
        ));
        assert!(matches!(
            MailboxQuota::reported(q(89, 100)),
            MailboxQuota::Reported {
                nearly_full: false,
                ..
            }
        ));
        assert_eq!(
            serde_json::to_value(MailboxQuota::reported(q(512, 1024))).expect("json"),
            serde_json::json!({ "kind": "reported", "usedBytes": 512, "limitBytes": 1024,
                                "percent": 50, "nearlyFull": false })
        );
    }
}
