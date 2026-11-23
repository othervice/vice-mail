import { ChevronDown, ChevronRight, Folder, type LucideIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { treeItemAria, useTreeNavigation, type TreeNavItem } from "../../components/useTreeNavigation";
import type { Account, Mailbox, MailboxId } from "../../services/ipc";
// Stage 16: folder colours and favourites.
import { FolderColorDot, FolderColorPicker, FolderMenu, RowMenuSlot, useColorName, useFolderName } from "../organize/folders";
import { useUiStore } from "../../stores/ui";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { useMailboxes } from "./queries";
import { buildMailboxTree, isRoleView, type MailboxNode } from "./tree";
import { useDropTarget } from "./useDropTarget";

interface MailboxTreeProps {
  account: Account;
  /** The folder the mail view shows right now, marked as current. */
  currentMailboxId: MailboxId | null;
}

/**
 * Visible counter; screen readers get it through the row's name ("Inbox, 1,296 unread"). The
 * name holds the visible text (WCAG 2.5.3, label in name): the same number format, and a space
 * before the counter so name and number are two words (not rendered in the flex row).
 */
function UnreadCount({ count }: { count: number }) {
  const { i18n } = useTranslation();
  if (count <= 0) return null;
  return (
    <>
      {" "}
      <span aria-hidden className="ms-auto shrink-0 text-12 font-medium text-secondary tabular-nums">
        {new Intl.NumberFormat(i18n.language).format(count)}
      </span>
    </>
  );
}

function FolderIcon({ icon: Icon }: { icon: LucideIcon }) {
  return <Icon aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />;
}

/** Indentation per level (the token grid has no inline styles): 16 px a level, flat from level 7. */
const INDENT = [
  "",
  "ms-4",
  "ms-8",
  "ms-12",
  "ms-[calc(var(--ui-space-4)*4)]",
  "ms-[calc(var(--ui-space-4)*5)]",
  "ms-[calc(var(--ui-space-4)*6)]",
] as const;

export function indentClass(level: number): string {
  return INDENT[Math.min(Math.max(level - 1, 0), INDENT.length - 1)] ?? "";
}

/** A visible row of the tree: the folder with its place, for the keyboard model and the aria tree. */
interface Row extends TreeNavItem {
  node: MailboxNode;
}

/** The rows shown, parents before their children; a closed folder hides everything below it. */
function visibleRows(nodes: readonly MailboxNode[], collapsed: ReadonlySet<MailboxId>, name: (mailbox: Mailbox) => string, level = 1): Row[] {
  return nodes.flatMap((node, index) => {
    const open = !collapsed.has(node.mailbox.id);
    const row: Row = {
      key: String(node.mailbox.id),
      level,
      posinset: index + 1,
      setsize: nodes.length,
      expanded: node.children.length > 0 ? open : undefined,
      text: name(node.mailbox),
      node,
    };
    return open ? [row, ...visibleRows(node.children, collapsed, name, level + 1)] : [row];
  });
}

interface ItemProps {
  row: Row;
  current: boolean;
  nav: ReturnType<typeof useTreeNavigation>;
  /** Enter, Space or a click: opens the folder, or opens and closes a placeholder. */
  onActivate: (mailbox: Mailbox) => void;
  /** Stage 16: "Folder colour…" of the folder's menu. */
  onColor: (mailbox: Mailbox) => void;
}

function MailboxItem({ row, current, nav, onActivate, onColor }: ItemProps) {
  const { t } = useTranslation();
  const colorName = useColorName();
  const toggleMailbox = useUiStore((state) => state.toggleMailbox);
  const { mailbox } = row.node;
  const name = row.text;
  // Rows dragged out of the list can be dropped here (a move of the dragged messages).
  const drop = useDropTarget(mailbox);

  // Stage 16: the colour is named with the folder, never shown alone.
  const named =
    mailbox.color === undefined || mailbox.color === null
      ? name
      : t("organize.favorites.withColor", { name, color: colorName(mailbox.color) });

  return (
    <li
      {...treeItemAria(row)}
      {...nav.itemProps(row.key)}
      aria-current={current ? "page" : undefined}
      aria-label={mailbox.unread > 0 ? t("sidebar.folderUnread", { name: named, count: mailbox.unread }) : named}
      title={mailbox.path}
      onClick={() => {
        onActivate(mailbox);
      }}
      {...(mailbox.selectable && drop.handlers)}
      data-testid={mailbox.selectable ? `mailbox-${String(mailbox.id)}` : undefined}
      data-drop-target={drop.over ? "over" : drop.accepts ? "ready" : undefined}
      className={cx(
        "group/folder flex h-8 min-w-0 items-center gap-1 rounded-control pe-1 text-13 transition-colors",
        indentClass(row.level),
        current ? "bg-selected font-medium text-primary" : mailbox.selectable ? "text-secondary hover:bg-rail-hover hover:text-primary" : "text-tertiary",
        drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
      )}
    >
      {row.expanded === undefined ? (
        <span aria-hidden className="size-6 shrink-0" />
      ) : (
        // For the mouse; the keyboard opens and closes with Right and Left (aria-expanded).
        <span
          aria-hidden
          title={row.expanded ? t("sidebar.collapse", { name }) : t("sidebar.expand", { name })}
          onClick={(event) => {
            event.stopPropagation();
            toggleMailbox(mailbox.id);
          }}
          data-testid={`folder-toggle-${String(mailbox.id)}`}
          className="flex size-6 shrink-0 items-center justify-center rounded-control text-tertiary hover:bg-rail-hover hover:text-primary"
        >
          {row.expanded ? (
            <ChevronDown className="size-(--ui-icon-small)" strokeWidth={1.75} />
          ) : (
            <ChevronRight className="size-(--ui-icon-small)" strokeWidth={1.75} />
          )}
        </span>
      )}
      <span className="flex min-w-0 flex-1 items-center gap-2 px-1">
        <FolderIcon icon={isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder} />
        <span className="truncate">{name}</span>
        <FolderColorDot color={mailbox.color} />
        <UnreadCount count={mailbox.unread} />
      </span>
      {mailbox.selectable && (
        <RowMenuSlot>
          <FolderMenu
            mailbox={mailbox}
            onColor={() => {
              onColor(mailbox);
            }}
            returnFocus={() => {
              nav.focus(row.key);
            }}
          />
        </RowMenuSlot>
      )}
    </li>
  );
}

/**
 * The folders of one account: role folders first in a fixed order, then the account's own
 * folders as a tree, each with its unread counter. Kept current by `mailboxesChanged`.
 */
export function MailboxTree({ account, currentMailboxId }: MailboxTreeProps) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id);
  const folderName = useFolderName();
  const collapsed = useUiStore((state) => state.collapsedMailboxes);
  const toggleMailbox = useUiStore((state) => state.toggleMailbox);
  const open = useOpenMailbox(account);
  // A placeholder that holds no mail only opens and closes.
  const activate = (mailbox: Mailbox) => {
    if (mailbox.selectable) open(mailbox);
    else toggleMailbox(mailbox.id);
  };
  const tree = useMemo(() => buildMailboxTree(mailboxes.data ?? []), [mailboxes.data]);
  const rows = visibleRows(tree, collapsed, folderName);
  const [coloring, setColoring] = useState<MailboxId | null>(null);
  const colored = coloring === null ? undefined : mailboxes.data?.find((mailbox) => mailbox.id === coloring);
  const nav = useTreeNavigation({
    items: rows,
    currentKey: currentMailboxId === null ? null : String(currentMailboxId),
    onOpen: (key) => {
      const mailbox = rows.find((row) => row.key === key)?.node.mailbox;
      if (mailbox) activate(mailbox);
    },
    onExpand: (key, expanded) => {
      const id = Number(key);
      if (collapsed.has(id) === expanded) toggleMailbox(id);
    },
  });

  if (mailboxes.isPending) return null;
  if (mailboxes.isError) {
    return (
      <ErrorNotice
        context={t("sidebar.loadFailed")}
        error={mailboxes.error}
        onRetry={() => {
          void mailboxes.refetch();
        }}
      />
    );
  }
  if (tree.length === 0) return <p className="px-2 text-12 text-tertiary">{t("sidebar.noFolders")}</p>;
  return (
    <>
      <ul {...nav.treeProps} aria-label={t("sidebar.folders", { account: account.displayName || account.email })} className="flex flex-col">
        {rows.map((row) => (
          <MailboxItem
            key={row.key}
            row={row}
            current={row.node.mailbox.id === currentMailboxId}
            nav={nav}
            onActivate={activate}
            onColor={(mailbox) => {
              setColoring(mailbox.id);
            }}
          />
        ))}
      </ul>
      {colored && (
        <FolderColorPicker
          mailbox={colored}
          onClose={() => {
            setColoring(null);
          }}
          restoreFocus={() => {
            nav.focus(String(colored.id));
          }}
        />
      )}
    </>
  );
}

/** Opens a folder: one with a view in the rail opens that view, so rail and tree agree. */
function useOpenMailbox(account: Account) {
  const showMailView = useUiStore((state) => state.showMailView);
  const showMailbox = useUiStore((state) => state.showMailbox);
  const selectAccount = useUiStore((state) => state.selectAccount);
  return (mailbox: Mailbox) => {
    if (isRoleView(mailbox.role)) {
      selectAccount(account.id);
      showMailView(mailbox.role);
    } else {
      showMailbox(account.id, mailbox.id);
    }
  };
}
