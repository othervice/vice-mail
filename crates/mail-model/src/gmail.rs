//! Stage 14, block A (decision 0034): Gmail over IMAP. Gmail has labels instead of folders;
//! IMAP shows every label as a folder, so one message lies in several mailboxes at once (the
//! inbox, each label, "All Mail"). With `X-GM-EXT-1` the server says which locations are the
//! same message (`X-GM-MSGID`) and which labels it carries (`X-GM-LABELS`).
//!
//! Labels are kept decoded (modified UTF-7 is the adapter's business). System labels keep
//! their backslash form as Gmail writes them (`\Inbox`, `\Important`, ...); everything else is
//! a user label, named like its folder (`Work/Projects`).

use crate::MailboxRole;

/// What Gmail says about one message besides plain IMAP (fetched with the headers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailInfo {
    /// `X-GM-MSGID`: one number for the message in every label it shows up in.
    pub msgid: u64,
    /// `X-GM-LABELS`, decoded and normalised ([`normalize_labels`]).
    pub labels: Vec<String>,
}

/// The inbox label. Archiving on Gmail removes it (decision 0034).
pub const INBOX_LABEL: &str = "\\Inbox";

/// System labels as `X-GM-LABELS` writes them. Sent without quotes; everything else is quoted.
pub const SYSTEM_LABELS: [&str; 7] = [
    "\\Inbox",
    "\\Important",
    "\\Starred",
    "\\Sent",
    "\\Draft",
    "\\Spam",
    "\\Trash",
];

/// Longest label the app sets (Gmail's own limit is 225 characters).
pub const MAX_LABEL_LEN: usize = 225;

/// Folder prefixes of Gmail's own folders (`[Gmail]/All Mail`; `[Google Mail]` in some
/// countries). They are not user labels.
const SYSTEM_FOLDER_PREFIXES: [&str; 2] = ["[Gmail]", "[Google Mail]"];

/// Whether an IMAP or SMTP host is Gmail's (`imap.gmail.com`, also for Google Workspace
/// domains, and `*.googlemail.com`).
pub fn is_gmail_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    ["gmail.com", "googlemail.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// The system label `name` stands for, in Gmail's spelling (ASCII case ignored), if any.
pub fn system_label(name: &str) -> Option<&'static str> {
    SYSTEM_LABELS
        .into_iter()
        .find(|label| label.eq_ignore_ascii_case(name))
}

/// Whether `name` can be set as a label: a system label, or a user label that is not empty,
/// has no control characters, no backslash at the start, no blank at either end, and at most
/// [`MAX_LABEL_LEN`] characters. Gmail's own folders (`[Gmail]/...`) are not labels.
pub fn is_valid_label(name: &str) -> bool {
    if system_label(name).is_some() {
        return true;
    }
    !name.is_empty()
        && name.chars().count() <= MAX_LABEL_LEN
        && !name.starts_with('\\')
        && name.trim() == name
        && !name.chars().any(char::is_control)
        && !is_system_folder(name)
}

/// Whether `path` is one of Gmail's own folders or below one (`[Gmail]/Sent Mail`).
pub fn is_system_folder(path: &str) -> bool {
    SYSTEM_FOLDER_PREFIXES.iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// System labels in Gmail's spelling, then the rest; sorted (system labels first, in the order
/// of [`SYSTEM_LABELS`]), without duplicates or empty names.
pub fn normalize_labels<I, S>(labels: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = labels
        .into_iter()
        .filter_map(|label| {
            let label = label.as_ref();
            match system_label(label) {
                Some(system) => Some(system.to_owned()),
                None if label.is_empty() => None,
                None => Some(label.to_owned()),
            }
        })
        .collect();
    let rank = |label: &str| {
        SYSTEM_LABELS
            .iter()
            .position(|s| *s == label)
            .unwrap_or(SYSTEM_LABELS.len())
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    out.dedup();
    out
}

/// The label a message carries because it lies in this mailbox (`path` decoded, with `/` as
/// Gmail's delimiter). `None` for "All Mail", for Gmail's other own folders and for an archive
/// role the user chose. Used to complete `X-GM-LABELS`, which leaves out the label of the
/// mailbox the message was fetched from on some Gmail servers.
pub fn label_of_mailbox(role: MailboxRole, path: &str) -> Option<String> {
    let system = match role {
        MailboxRole::Inbox => "\\Inbox",
        MailboxRole::Sent => "\\Sent",
        MailboxRole::Drafts => "\\Draft",
        MailboxRole::Junk => "\\Spam",
        MailboxRole::Trash => "\\Trash",
        MailboxRole::Flagged => "\\Starred",
        MailboxRole::All | MailboxRole::Archive => return None,
        MailboxRole::Custom => {
            if path.eq_ignore_ascii_case("INBOX") {
                return Some(INBOX_LABEL.to_owned());
            }
            if is_system_folder(path) {
                return path
                    .rsplit('/')
                    .next()
                    .filter(|leaf| leaf.eq_ignore_ascii_case("Important"))
                    .map(|_| "\\Important".to_owned());
            }
            return (!path.is_empty()).then(|| path.to_owned());
        }
    };
    Some(system.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_hosts() {
        assert!(is_gmail_host("imap.gmail.com"));
        assert!(is_gmail_host("IMAP.googlemail.com."));
        assert!(!is_gmail_host("imap.example.org"));
        assert!(!is_gmail_host("notgmail.com"));
    }

    #[test]
    fn system_labels_keep_gmails_spelling() {
        assert_eq!(system_label("\\inbox"), Some("\\Inbox"));
        assert_eq!(system_label("\\IMPORTANT"), Some("\\Important"));
        assert_eq!(system_label("Inbox"), None);
        assert_eq!(
            normalize_labels(["Work", "\\important", "", "\\Inbox", "Work", "Ärger"]),
            vec!["\\Inbox", "\\Important", "Work", "Ärger"]
        );
    }

    #[test]
    fn label_validity() {
        for ok in [
            "Work",
            "Work/Projects",
            "Muy Importante",
            "Ärger",
            "\\Inbox",
            "\\starred",
        ] {
            assert!(is_valid_label(ok), "{ok:?}");
        }
        let long = "x".repeat(MAX_LABEL_LEN + 1);
        for bad in [
            "",
            " Work",
            "Work ",
            "\\Unknown",
            "a\r\nb",
            "tab\there",
            "[Gmail]",
            "[Gmail]/All Mail",
            "[Google Mail]/Spam",
            long.as_str(),
        ] {
            assert!(!is_valid_label(bad), "{bad:?}");
        }
        assert!(
            is_valid_label("[Gmail]x"),
            "only the folder itself and below"
        );
    }

    #[test]
    fn mailboxes_imply_labels() {
        assert_eq!(
            label_of_mailbox(MailboxRole::Inbox, "INBOX").as_deref(),
            Some("\\Inbox")
        );
        assert_eq!(
            label_of_mailbox(MailboxRole::Sent, "[Gmail]/Sent Mail").as_deref(),
            Some("\\Sent")
        );
        assert_eq!(
            label_of_mailbox(MailboxRole::Flagged, "[Gmail]/Starred").as_deref(),
            Some("\\Starred")
        );
        assert_eq!(label_of_mailbox(MailboxRole::All, "[Gmail]/All Mail"), None);
        assert_eq!(label_of_mailbox(MailboxRole::Archive, "Old"), None);
        assert_eq!(
            label_of_mailbox(MailboxRole::Custom, "[Gmail]/Important").as_deref(),
            Some("\\Important")
        );
        assert_eq!(label_of_mailbox(MailboxRole::Custom, "[Gmail]/Chats"), None);
        assert_eq!(
            label_of_mailbox(MailboxRole::Custom, "Work/Projects").as_deref(),
            Some("Work/Projects")
        );
    }
}
