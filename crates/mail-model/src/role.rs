//! Role detection for mailboxes. Pure logic: gets decoded paths and LIST attributes, returns a
//! role per mailbox. Decoding modified UTF-7 is the adapter's job.
//!
//! Order of precedence, each role at most once per account:
//! 1. `INBOX` (any case) is always the inbox and cannot be changed.
//! 2. A role the user chose (also "custom", which blocks automatic roles for that mailbox).
//! 3. SPECIAL-USE attributes (`\Sent`, `\Drafts`, `\Trash`, `\Junk`, `\Archive`, `\All`,
//!    `\Flagged`; RFC 6154).
//! 4. Well-known English and German names, only for top-level folders and direct children of
//!    `INBOX` (Courier-style `INBOX.Sent`) or `[Gmail]`/`[Google Mail]`. `All` and `Flagged`
//!    are only guessed inside the Gmail folder, where they are virtual folders.
//!
//! Non-selectable entries (`\Noselect`, `\NonExistent`) never get a role.

use std::collections::HashSet;

use crate::mailbox::{has_attribute, is_selectable};
use crate::{MailboxRole, RoleSource};

/// One mailbox as input for [`assign_roles`].
#[derive(Debug, Clone, Copy)]
pub struct RoleInput<'a> {
    /// Decoded path, for example `INBOX`, `Gesendet`, `[Gmail]/Sent Mail`.
    pub path: &'a str,
    pub delimiter: Option<char>,
    /// LIST attributes as sent by the server.
    pub attributes: &'a [String],
    /// Role the user set for this mailbox earlier, if any (stored with source `user`).
    pub user_role: Option<MailboxRole>,
}

/// Role of one mailbox and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleAssignment {
    pub role: MailboxRole,
    pub source: RoleSource,
}

const NONE: RoleAssignment = RoleAssignment {
    role: MailboxRole::Custom,
    source: RoleSource::None,
};

/// SPECIAL-USE attributes in the order they are tried when a mailbox has several.
const SPECIAL_USE: [(&str, MailboxRole); 7] = [
    ("Sent", MailboxRole::Sent),
    ("Drafts", MailboxRole::Drafts),
    ("Trash", MailboxRole::Trash),
    ("Junk", MailboxRole::Junk),
    ("Archive", MailboxRole::Archive),
    ("All", MailboxRole::All),
    ("Flagged", MailboxRole::Flagged),
];

/// Normalised names (lower case, umlauts composed, without spaces, hyphens and underscores)
/// with a rank: when several folders match the same role, the lowest rank wins.
const NAMES: &[(&str, MailboxRole, u8)] = &[
    // Sent
    ("sent", MailboxRole::Sent, 0),
    ("gesendet", MailboxRole::Sent, 0),
    ("sentitems", MailboxRole::Sent, 1),    // Outlook, Exchange
    ("sentmessages", MailboxRole::Sent, 1), // Apple
    ("sentmail", MailboxRole::Sent, 1),     // Gmail
    ("gesendeteobjekte", MailboxRole::Sent, 1), // Outlook (de)
    ("gesendeteelemente", MailboxRole::Sent, 1),
    ("gesendetenachrichten", MailboxRole::Sent, 1),
    // Drafts
    ("drafts", MailboxRole::Drafts, 0),
    ("entwürfe", MailboxRole::Drafts, 0),
    ("draft", MailboxRole::Drafts, 1),
    ("entwurf", MailboxRole::Drafts, 1),
    // Trash
    ("trash", MailboxRole::Trash, 0),
    ("papierkorb", MailboxRole::Trash, 0),
    ("deleteditems", MailboxRole::Trash, 1), // Outlook, Exchange
    ("deletedmessages", MailboxRole::Trash, 1), // Apple
    ("gelöschteelemente", MailboxRole::Trash, 1), // Outlook (de)
    ("gelöschteobjekte", MailboxRole::Trash, 1),
    ("gelöschtenachrichten", MailboxRole::Trash, 1),
    ("bin", MailboxRole::Trash, 2), // Gmail (UK)
    ("deleted", MailboxRole::Trash, 2),
    ("gelöscht", MailboxRole::Trash, 2),
    // Junk
    ("junk", MailboxRole::Junk, 0),
    ("spam", MailboxRole::Junk, 0),
    ("junkemail", MailboxRole::Junk, 1), // Outlook "Junk E-mail", "Junk-E-Mail" (de)
    ("junkmail", MailboxRole::Junk, 1),
    ("spamverdacht", MailboxRole::Junk, 1), // GMX
    ("unerwünscht", MailboxRole::Junk, 1),  // WEB.DE
    ("bulkmail", MailboxRole::Junk, 2),     // Yahoo
    // Archive
    ("archive", MailboxRole::Archive, 0),
    ("archiv", MailboxRole::Archive, 0),
    ("archives", MailboxRole::Archive, 1), // Thunderbird
];

/// Only inside `[Gmail]` / `[Google Mail]`.
const GMAIL_NAMES: &[(&str, MailboxRole, u8)] = &[
    ("allmail", MailboxRole::All, 0),
    ("allenachrichten", MailboxRole::All, 0),
    ("starred", MailboxRole::Flagged, 0),
    ("markiert", MailboxRole::Flagged, 0),
];

/// Assigns a role to every mailbox of one account. The result has the same order as `inputs`.
pub fn assign_roles(inputs: &[RoleInput<'_>]) -> Vec<RoleAssignment> {
    let mut out = vec![NONE; inputs.len()];
    let mut fixed = vec![false; inputs.len()];
    let mut taken: HashSet<MailboxRole> = HashSet::new();

    // Deterministic regardless of the server's LIST order.
    let mut order: Vec<usize> = (0..inputs.len()).collect();
    order.sort_by(|&a, &b| inputs[a].path.cmp(inputs[b].path).then(a.cmp(&b)));
    let entries: Vec<(usize, &RoleInput<'_>)> = order.iter().map(|&i| (i, &inputs[i])).collect();

    fn set(out: &mut [RoleAssignment], i: usize, role: MailboxRole, source: RoleSource) {
        if let Some(slot) = out.get_mut(i) {
            *slot = RoleAssignment { role, source };
        }
    }

    // 1. The inbox.
    for &(i, input) in &entries {
        if input.path.eq_ignore_ascii_case("INBOX") && !taken.contains(&MailboxRole::Inbox) {
            set(&mut out, i, MailboxRole::Inbox, RoleSource::Server);
            taken.insert(MailboxRole::Inbox);
            fixed[i] = true;
        }
    }

    // 2. The user's choices.
    for &(i, input) in &entries {
        if fixed[i] || !is_selectable(input.attributes) {
            continue;
        }
        match input.user_role {
            None | Some(MailboxRole::Inbox) => {}
            Some(MailboxRole::Custom) => {
                set(&mut out, i, MailboxRole::Custom, RoleSource::User);
                fixed[i] = true;
            }
            Some(role) => {
                let role = if taken.insert(role) {
                    role
                } else {
                    MailboxRole::Custom
                };
                set(&mut out, i, role, RoleSource::User);
                fixed[i] = true;
            }
        }
    }

    // 3. SPECIAL-USE.
    for &(i, input) in &entries {
        if fixed[i] || !is_selectable(input.attributes) {
            continue;
        }
        let special = SPECIAL_USE
            .iter()
            .filter(|(attr, _)| has_attribute(input.attributes, attr))
            .map(|(_, role)| *role)
            .find(|role| !taken.contains(role));
        if let Some(role) = special {
            taken.insert(role);
            set(&mut out, i, role, RoleSource::Server);
            fixed[i] = true;
        }
    }

    // 4. Names. Collect every candidate, then give each free role to the best one.
    let mut candidates: Vec<(MailboxRole, u8, &str, usize)> = Vec::new();
    for &(i, input) in &entries {
        if fixed[i] || !is_selectable(input.attributes) {
            continue;
        }
        if let Some((role, rank)) = guess_by_name(input.path, input.delimiter)
            && !taken.contains(&role)
        {
            candidates.push((role, rank, input.path, i));
        }
    }
    candidates.sort();
    for (role, _, _, i) in candidates {
        if !fixed[i] && taken.insert(role) {
            set(&mut out, i, role, RoleSource::Heuristic);
            fixed[i] = true;
        }
    }

    out
}

/// Role and rank suggested by the name alone, if the folder sits where well-known folders sit.
fn guess_by_name(path: &str, delimiter: Option<char>) -> Option<(MailboxRole, u8)> {
    let (parent, leaf) = match delimiter.and_then(|d| path.rsplit_once(d)) {
        Some((parent, leaf)) => (Some(parent), leaf),
        None => (None, path),
    };
    let gmail = parent.is_some_and(|p| {
        p.eq_ignore_ascii_case("[Gmail]") || p.eq_ignore_ascii_case("[Google Mail]")
    });
    let eligible = match parent {
        None => true,
        Some(p) => gmail || p.eq_ignore_ascii_case("INBOX"),
    };
    if !eligible {
        return None;
    }
    let key = normalize(leaf);
    let lookup = |table: &[(&str, MailboxRole, u8)]| {
        table
            .iter()
            .find(|(name, _, _)| *name == key)
            .map(|(_, role, rank)| (*role, *rank))
    };
    lookup(NAMES).or_else(|| if gmail { lookup(GMAIL_NAMES) } else { None })
}

/// Lower case, decomposed umlauts composed (`u` + U+0308 to `ü`, as macOS may write names),
/// spaces, hyphens and underscores removed.
fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        match c {
            ' ' | '-' | '_' => {}
            '\u{308}' => {
                let composed = match out.chars().last() {
                    Some('a') => Some('ä'),
                    Some('o') => Some('ö'),
                    Some('u') => Some('ü'),
                    _ => None,
                };
                match composed {
                    Some(ch) => {
                        out.pop();
                        out.push(ch);
                    }
                    None => out.push(c),
                }
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use MailboxRole as R;
    use RoleSource as S;

    struct Folder {
        path: &'static str,
        attributes: Vec<String>,
        user_role: Option<MailboxRole>,
        delimiter: Option<char>,
    }

    fn f(path: &'static str, attributes: &[&str]) -> Folder {
        Folder {
            path,
            attributes: attributes.iter().map(|a| (*a).to_owned()).collect(),
            user_role: None,
            delimiter: Some('/'),
        }
    }

    fn user(mut folder: Folder, role: MailboxRole) -> Folder {
        folder.user_role = Some(role);
        folder
    }

    fn assign(folders: &[Folder]) -> Vec<(&'static str, MailboxRole, RoleSource)> {
        let inputs: Vec<RoleInput<'_>> = folders
            .iter()
            .map(|f| RoleInput {
                path: f.path,
                delimiter: f.delimiter,
                attributes: &f.attributes,
                user_role: f.user_role,
            })
            .collect();
        assign_roles(&inputs)
            .into_iter()
            .zip(folders)
            .map(|(a, f)| (f.path, a.role, a.source))
            .collect()
    }

    fn role_of(
        result: &[(&str, MailboxRole, RoleSource)],
        path: &str,
    ) -> (MailboxRole, RoleSource) {
        result
            .iter()
            .find(|(p, _, _)| *p == path)
            .map(|(_, r, s)| (*r, *s))
            .unwrap_or((R::Custom, S::None))
    }

    #[test]
    fn special_use_wins_over_names() {
        let result = assign(&[
            f("INBOX", &[]),
            f("Sent", &["\\HasNoChildren"]),
            f("Sent Messages", &["\\Sent"]),
            f("Papierkorb", &[]),
            f("Deleted", &["\\trash"]),
            f("Archive", &["\\Archive"]),
        ]);
        assert_eq!(role_of(&result, "INBOX"), (R::Inbox, S::Server));
        assert_eq!(role_of(&result, "Sent Messages"), (R::Sent, S::Server));
        assert_eq!(role_of(&result, "Sent"), (R::Custom, S::None));
        assert_eq!(role_of(&result, "Deleted"), (R::Trash, S::Server));
        assert_eq!(role_of(&result, "Papierkorb"), (R::Custom, S::None));
        assert_eq!(role_of(&result, "Archive"), (R::Archive, S::Server));
    }

    #[test]
    fn german_names_without_special_use() {
        let result = assign(&[
            f("inbox", &[]),
            f("Gesendet", &[]),
            f("Entwürfe", &[]),
            f("Papierkorb", &[]),
            f("Spamverdacht", &[]),
            f("Archiv", &[]),
            f("Rechnungen", &[]),
        ]);
        assert_eq!(role_of(&result, "inbox"), (R::Inbox, S::Server));
        assert_eq!(role_of(&result, "Gesendet"), (R::Sent, S::Heuristic));
        assert_eq!(role_of(&result, "Entwürfe"), (R::Drafts, S::Heuristic));
        assert_eq!(role_of(&result, "Papierkorb"), (R::Trash, S::Heuristic));
        assert_eq!(role_of(&result, "Spamverdacht"), (R::Junk, S::Heuristic));
        assert_eq!(role_of(&result, "Archiv"), (R::Archive, S::Heuristic));
        assert_eq!(role_of(&result, "Rechnungen"), (R::Custom, S::None));
    }

    #[test]
    fn outlook_names_in_german_and_english() {
        let de = assign(&[
            f("INBOX", &[]),
            f("Gesendete Objekte", &[]),
            f("Gelöschte Elemente", &[]),
            f("Junk-E-Mail", &[]),
        ]);
        assert_eq!(role_of(&de, "Gesendete Objekte"), (R::Sent, S::Heuristic));
        assert_eq!(role_of(&de, "Gelöschte Elemente"), (R::Trash, S::Heuristic));
        assert_eq!(role_of(&de, "Junk-E-Mail"), (R::Junk, S::Heuristic));

        let en = assign(&[
            f("Sent Items", &[]),
            f("Deleted Items", &[]),
            f("Junk E-mail", &[]),
            f("Drafts", &[]),
        ]);
        assert_eq!(role_of(&en, "Sent Items"), (R::Sent, S::Heuristic));
        assert_eq!(role_of(&en, "Deleted Items"), (R::Trash, S::Heuristic));
        assert_eq!(role_of(&en, "Junk E-mail"), (R::Junk, S::Heuristic));
        assert_eq!(role_of(&en, "Drafts"), (R::Drafts, S::Heuristic));
    }

    #[test]
    fn gmail_folders_by_name_and_virtual_roles() {
        let result = assign(&[
            f("INBOX", &[]),
            f("[Gmail]", &["\\Noselect", "\\HasChildren"]),
            f("[Gmail]/Alle Nachrichten", &[]),
            f("[Gmail]/Gesendet", &[]),
            f("[Gmail]/Entwürfe", &[]),
            f("[Gmail]/Papierkorb", &[]),
            f("[Gmail]/Spam", &[]),
            f("[Gmail]/Markiert", &[]),
            f("[Gmail]/Wichtig", &[]),
        ]);
        assert_eq!(role_of(&result, "[Gmail]"), (R::Custom, S::None));
        assert_eq!(
            role_of(&result, "[Gmail]/Alle Nachrichten"),
            (R::All, S::Heuristic)
        );
        assert_eq!(
            role_of(&result, "[Gmail]/Gesendet"),
            (R::Sent, S::Heuristic)
        );
        assert_eq!(
            role_of(&result, "[Gmail]/Entwürfe"),
            (R::Drafts, S::Heuristic)
        );
        assert_eq!(
            role_of(&result, "[Gmail]/Papierkorb"),
            (R::Trash, S::Heuristic)
        );
        assert_eq!(role_of(&result, "[Gmail]/Spam"), (R::Junk, S::Heuristic));
        assert_eq!(
            role_of(&result, "[Gmail]/Markiert"),
            (R::Flagged, S::Heuristic)
        );
        assert_eq!(role_of(&result, "[Gmail]/Wichtig"), (R::Custom, S::None));

        let en = assign(&[
            f("[Google Mail]/All Mail", &[]),
            f("[Google Mail]/Sent Mail", &[]),
            f("[Google Mail]/Bin", &[]),
            f("[Google Mail]/Starred", &[]),
        ]);
        assert_eq!(
            role_of(&en, "[Google Mail]/All Mail"),
            (R::All, S::Heuristic)
        );
        assert_eq!(
            role_of(&en, "[Google Mail]/Sent Mail"),
            (R::Sent, S::Heuristic)
        );
        assert_eq!(role_of(&en, "[Google Mail]/Bin"), (R::Trash, S::Heuristic));
        assert_eq!(
            role_of(&en, "[Google Mail]/Starred"),
            (R::Flagged, S::Heuristic)
        );

        // Virtual roles are only guessed inside the Gmail folder.
        let plain = assign(&[f("All Mail", &[]), f("Starred", &[])]);
        assert_eq!(role_of(&plain, "All Mail"), (R::Custom, S::None));
        assert_eq!(role_of(&plain, "Starred"), (R::Custom, S::None));
    }

    #[test]
    fn courier_style_children_of_inbox() {
        let mut folders = vec![
            f("INBOX", &[]),
            f("INBOX.Sent", &[]),
            f("INBOX.Trash", &[]),
            f("INBOX.Projekte.Sent", &[]),
        ];
        for folder in &mut folders {
            folder.delimiter = Some('.');
        }
        let result = assign(&folders);
        assert_eq!(role_of(&result, "INBOX.Sent"), (R::Sent, S::Heuristic));
        assert_eq!(role_of(&result, "INBOX.Trash"), (R::Trash, S::Heuristic));
        assert_eq!(
            role_of(&result, "INBOX.Projekte.Sent"),
            (R::Custom, S::None)
        );
    }

    #[test]
    fn nested_folders_are_not_guessed() {
        let result = assign(&[f("Kunden/Sent", &[]), f("Kunden/Papierkorb", &[])]);
        assert!(
            result
                .iter()
                .all(|(_, r, s)| *r == R::Custom && *s == S::None)
        );
    }

    #[test]
    fn each_role_once_best_rank_wins() {
        let result = assign(&[
            f("Sent Items", &[]),
            f("Gesendet", &[]),
            f("Deleted", &[]),
            f("Deleted Items", &[]),
            f("Spam", &[]),
            f("Junk", &[]),
        ]);
        assert_eq!(role_of(&result, "Gesendet"), (R::Sent, S::Heuristic));
        assert_eq!(role_of(&result, "Sent Items"), (R::Custom, S::None));
        assert_eq!(role_of(&result, "Deleted Items"), (R::Trash, S::Heuristic));
        assert_eq!(role_of(&result, "Deleted"), (R::Custom, S::None));
        // Same rank: the lower path wins, so the result does not depend on LIST order.
        assert_eq!(role_of(&result, "Junk"), (R::Junk, S::Heuristic));
        assert_eq!(role_of(&result, "Spam"), (R::Custom, S::None));

        let reversed = assign(&[f("Spam", &[]), f("Junk", &[])]);
        assert_eq!(role_of(&reversed, "Junk"), (R::Junk, S::Heuristic));
    }

    #[test]
    fn user_roles_take_precedence_and_survive() {
        let result = assign(&[
            f("INBOX", &[]),
            user(f("Ausgang", &[]), R::Sent),
            f("Sent", &["\\Sent"]),
            user(f("Archiv", &[]), R::Custom),
            f("Archive", &[]),
            user(f("Altes", &[]), R::Inbox),
        ]);
        assert_eq!(role_of(&result, "Ausgang"), (R::Sent, S::User));
        assert_eq!(role_of(&result, "Sent"), (R::Custom, S::None));
        // "custom" chosen by the user blocks the guess for that folder only.
        assert_eq!(role_of(&result, "Archiv"), (R::Custom, S::User));
        assert_eq!(role_of(&result, "Archive"), (R::Archive, S::Heuristic));
        // The inbox role cannot be given to another folder.
        assert_eq!(role_of(&result, "Altes"), (R::Custom, S::None));
        assert_eq!(role_of(&result, "INBOX"), (R::Inbox, S::Server));

        let inbox_override = assign(&[user(f("INBOX", &[]), R::Trash)]);
        assert_eq!(role_of(&inbox_override, "INBOX"), (R::Inbox, S::Server));
    }

    #[test]
    fn non_selectable_folders_get_no_role() {
        let result = assign(&[
            f("Sent", &["\\Noselect"]),
            f("Trash", &["\\NonExistent", "\\Trash"]),
            user(f("Drafts", &["\\Noselect"]), R::Drafts),
        ]);
        assert!(
            result
                .iter()
                .all(|(_, r, s)| *r == R::Custom && *s == S::None)
        );
    }

    #[test]
    fn decomposed_umlauts_and_case() {
        // "Entwürfe" and "Gelöscht" as written by macOS (NFD), in odd case.
        let result = assign(&[f("ENTWU\u{308}RFE", &[]), f("gelo\u{308}scht", &[])]);
        assert_eq!(result[0].1, R::Drafts);
        assert_eq!(result[1].1, R::Trash);
        assert_eq!(normalize(" Junk-E-Mail "), "junkemail");
        assert_eq!(normalize("Unerwu\u{308}nscht"), "unerwünscht");
        assert_eq!(normalize("e\u{308}"), "e\u{308}");
    }

    #[test]
    fn several_special_use_attributes_on_one_folder() {
        let result = assign(&[f("Mail", &["\\Archive", "\\All"]), f("Alles", &["\\All"])]);
        assert_eq!(role_of(&result, "Mail"), (R::Archive, S::Server));
        assert_eq!(role_of(&result, "Alles"), (R::All, S::Server));
    }
}
