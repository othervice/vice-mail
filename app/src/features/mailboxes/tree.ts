import type { Mailbox, MailboxRole } from "../../services/ipc";
import type { MailViewId } from "../../stores/ui";

/**
 * Roles in the order the folder tree shows them, before the ordinary folders (docs/DESIGN.md,
 * "Seitenspalte": Inbox, Drafts, Sent, Archive, Junk, Trash, as most mail apps order them).
 */
export const ROLE_ORDER = [
  "inbox",
  "drafts",
  "sent",
  "archive",
  "junk",
  "trash",
  "flagged",
  "all",
] as const satisfies readonly Exclude<MailboxRole, "custom">[];

/** Roles the user may assign. The inbox is fixed by the server (`INBOX`). */
export const ASSIGNABLE_ROLES = ROLE_ORDER.filter((role) => role !== "inbox");

/** Rail views that show the folder with the same role of the selected account. */
export const ROLE_VIEWS = ["inbox", "sent", "drafts", "archive", "trash"] as const satisfies readonly (MailViewId &
  MailboxRole)[];

export type RoleView = (typeof ROLE_VIEWS)[number];

export function isRoleView(view: MailViewId | MailboxRole): view is RoleView {
  return (ROLE_VIEWS as readonly string[]).includes(view);
}

export interface MailboxNode {
  mailbox: Mailbox;
  children: MailboxNode[];
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/**
 * Builds the folder tree: folders with a role first, in `ROLE_ORDER`, wherever they sit on the
 * server (Gmail keeps them under "[Gmail]"); then the ordinary folders as a tree by `parentId`,
 * sorted by name. Ordinary subfolders stay under their parent, also under a role folder.
 * Placeholders that cannot hold mail are left out when nothing visible is below them.
 */
export function buildMailboxTree(mailboxes: readonly Mailbox[]): MailboxNode[] {
  const ids = new Set(mailboxes.map((m) => m.id));
  const children = new Map<number, Mailbox[]>();
  for (const mailbox of mailboxes) {
    if (mailbox.role !== "custom" || mailbox.parentId === null || !ids.has(mailbox.parentId)) continue;
    const siblings = children.get(mailbox.parentId) ?? [];
    siblings.push(mailbox);
    children.set(mailbox.parentId, siblings);
  }

  const node = (mailbox: Mailbox, path: Set<number>): MailboxNode | null => {
    // A broken server answer could make a folder its own ancestor; never loop on it.
    if (path.has(mailbox.id)) return null;
    const inner = new Set(path).add(mailbox.id);
    const kids = (children.get(mailbox.id) ?? [])
      .slice()
      .sort((a, b) => collator.compare(a.name, b.name))
      .map((child) => node(child, inner))
      .filter((child): child is MailboxNode => child !== null);
    if (!mailbox.selectable && kids.length === 0) return null;
    return { mailbox, children: kids };
  };

  const roleRank = (role: MailboxRole) => ROLE_ORDER.indexOf(role as (typeof ROLE_ORDER)[number]);
  const withRole = mailboxes
    .filter((m) => m.role !== "custom")
    .sort((a, b) => roleRank(a.role) - roleRank(b.role) || collator.compare(a.path, b.path));
  const topLevel = mailboxes
    .filter((m) => m.role === "custom" && (m.parentId === null || !ids.has(m.parentId)))
    .sort((a, b) => collator.compare(a.name, b.name));

  return [...withRole, ...topLevel]
    .map((mailbox) => node(mailbox, new Set()))
    .filter((n): n is MailboxNode => n !== null);
}

/** The folder of an account that has `role`, if any (the core keeps at most one per role). */
export function mailboxWithRole(mailboxes: readonly Mailbox[], role: MailboxRole): Mailbox | undefined {
  return mailboxes.find((m) => m.role === role);
}

/** Stage 23 (decision 0085): an account's section: its inbox and Sent, and "More folders". */
export interface AccountTree {
  inbox: Mailbox | undefined;
  sent: Mailbox | undefined;
  /** Every other folder as a tree: roles first, then the own folders by name. */
  more: MailboxNode[];
}

/**
 * Splits an account's tree for its section in the column: the inbox and Sent stand on their own,
 * everything else goes under "More folders". Own folders below the inbox or Sent (servers that
 * keep every folder under `INBOX.`) move up there, among the other own folders by name; their
 * path stays in the row's tooltip.
 */
export function splitAccountTree(tree: readonly MailboxNode[]): AccountTree {
  const inbox = tree.find((node) => node.mailbox.role === "inbox");
  const sent = tree.find((node) => node.mailbox.role === "sent");
  const others = tree.filter((node) => node !== inbox && node !== sent);
  const own = [...others.filter((node) => node.mailbox.role === "custom"), ...(inbox?.children ?? []), ...(sent?.children ?? [])].sort((a, b) =>
    collator.compare(a.mailbox.name, b.mailbox.name),
  );
  return {
    inbox: inbox?.mailbox,
    sent: sent?.mailbox,
    more: [...others.filter((node) => node.mailbox.role !== "custom"), ...own],
  };
}
