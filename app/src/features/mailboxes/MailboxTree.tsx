import { ChevronDown, ChevronRight, CircleExclamationPoint, CloudOff, Folder, type LucideIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { treeItemAria, useTreeNavigation, type TreeNavItem } from "../../components/useTreeNavigation";
import type { Account, Mailbox, MailboxId } from "../../services/ipc";
// Stage 16: folder colours and favourites.
import { FolderColorDot, FolderColorPicker, FolderMenu, RowMenuSlot, useColorName, useFolderName } from "../organize/folders";
import { accountFill } from "../accounts/accountColor";
import { indentClass, sidebarIconClass, sidebarRowClass } from "../layout/sidebarStyles";
import { useUiStore } from "../../stores/ui";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import { useOperations } from "../actions/queries";
import { useFolderCreateItem, useReloadFoldersItem } from "./FolderEdit";
import { useMailboxes } from "./queries";
import { buildMailboxTree, isRoleView, type MailboxNode } from "./tree";
import { useDropTarget } from "./useDropTarget";

interface MailboxTreeProps {
  account: Account;
  /** The folder the mail view shows right now, marked as current. */
  currentMailboxId: MailboxId | null;
}

/** Key of the account's own row, the root of its tree. Folder keys are their ids (digits only). */
const ACCOUNT_KEY = "account";

/**
 * The account's unread total in the column: its inbox and its own folders, where new mail lands.
 * Not junk, trash, sent or drafts, nor Gmail's "All mail" and "Flagged", which repeat the others.
 */
export function accountUnread(mailboxes: readonly Mailbox[]): number {
  return mailboxes.reduce((sum, mailbox) => (mailbox.role === "inbox" || mailbox.role === "custom" ? sum + mailbox.unread : sum), 0);
}

/**
 * Visible counter; screen readers get it through the row's name ("Inbox, 1,296 unread"). The
 * name holds the visible text (WCAG 2.5.3, label in name): the same number format, and a space
 * before the counter so name and number are two words (not rendered in the flex row). While the
 * row's "…" shows (hover, keyboard focus, open menu), it takes the counter's place.
 */
export function UnreadCount({ count, emphasis = false }: { count: number; emphasis?: boolean }) {
  const { i18n } = useTranslation();
  if (count <= 0) return null;
  return (
    <>
      {" "}
      <span
        aria-hidden
        className={cx(
          "ms-auto shrink-0 ps-2 text-12 tabular-nums group-hover/folder:invisible group-focus-visible/folder:invisible group-has-[[aria-expanded=true]]/folder:invisible",
          emphasis ? "font-semibold text-primary" : "font-medium text-secondary",
        )}
      >
        {new Intl.NumberFormat(i18n.language).format(count)}
      </span>
    </>
  );
}

function FolderIcon({ icon: Icon, current }: { icon: LucideIcon; current: boolean }) {
  return <Icon aria-hidden className={sidebarIconClass(current)} strokeWidth={1.75} />;
}

/** A visible row of the tree: the folder with its place, for the keyboard model and the aria tree. */
interface Row extends TreeNavItem {
  node: MailboxNode;
}

/** The rows shown, parents before their children; a closed folder hides everything below it. */
function visibleRows(nodes: readonly MailboxNode[], collapsed: ReadonlySet<MailboxId>, name: (mailbox: Mailbox) => string, level = 2): Row[] {
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
  /** Audit 24.11.2026: the account, for "Rename folder…" and "Delete folder…" in the menu. */
  account: Account;
  /** A rename or delete of the folder on its way to the server. */
  pending: "rename" | "delete" | null;
}

function MailboxItem({ row, current, nav, onActivate, onColor, account, pending }: ItemProps) {
  const { t } = useTranslation();
  const colorName = useColorName();
  const toggleMailbox = useUiStore((state) => state.toggleMailbox);
  const { mailbox } = row.node;
  const name = row.text;
  // Rows dragged out of the list can be dropped here (a move of the dragged messages).
  const drop = useDropTarget(mailbox);

  // Stage 16: the colour is named with the folder, never shown alone.
  const colored =
    mailbox.color === undefined || mailbox.color === null
      ? name
      : t("organize.favorites.withColor", { name, color: colorName(mailbox.color) });
  // Audit 24.11.2026: a rename or delete on its way says so, in the name too.
  const pendingText = pending === "rename" ? t("folderEdit.renaming") : pending === "delete" ? t("folderEdit.deleting") : null;
  const named = pendingText ? `${colored}, ${pendingText}` : colored;

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
      data-folder-role={mailbox.role}
      data-drop-target={drop.over ? "over" : drop.accepts ? "ready" : undefined}
      className={sidebarRowClass(
        current,
        cx(
          "group/folder gap-1 pe-2",
          // Folders start one level below their account's row, without indentation of their own.
          indentClass(row.level - 1),
          !mailbox.selectable && !current && "text-tertiary",
          drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
        ),
      )}
    >
      {row.expanded === undefined ? (
        <span aria-hidden className="w-5 shrink-0" />
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
          className="flex h-6 w-5 shrink-0 items-center justify-center rounded-control text-tertiary hover:text-primary"
        >
          {row.expanded ? (
            <ChevronDown className="size-(--ui-icon-small)" strokeWidth={1.75} />
          ) : (
            <ChevronRight className="size-(--ui-icon-small)" strokeWidth={1.75} />
          )}
        </span>
      )}
      <span className="flex min-w-0 flex-1 items-center gap-2">
        <FolderIcon icon={isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder} current={current} />
        <span className={cx("truncate", mailbox.unread > 0 && "text-primary")}>{name}</span>
        <FolderColorDot color={mailbox.color} />
        {pendingText && (
          <span aria-hidden className="shrink-0 text-12 text-tertiary" data-testid={`folder-pending-${String(mailbox.id)}`}>
            {pendingText}
          </span>
        )}
        <UnreadCount count={mailbox.unread} emphasis={mailbox.unread > 0 && mailbox.role === "inbox"} />
      </span>
      {mailbox.selectable && (
        <RowMenuSlot>
          <FolderMenu
            mailbox={mailbox}
            account={account}
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

/** Quiet mark next to an account that is offline or stopped; its reason is in the row's name. */
export function AccountStatusMark({ account }: { account: Account }) {
  const state = account.status.state;
  if (state !== "offline" && state !== "error") return null;
  const Icon = state === "error" ? CircleExclamationPoint : CloudOff;
  return (
    <Icon
      aria-hidden
      data-testid={`account-status-${account.id}`}
      className={cx("size-(--ui-icon-small) shrink-0", state === "error" ? "text-danger" : "text-tertiary")}
      strokeWidth={1.75}
    />
  );
}

/**
 * An account's name for the column (accessible name and tooltip): name and address, its unread
 * total as shown, and what is wrong while it is offline or stopped (with the reason, if known).
 */
export function useAccountRowLabel(): (account: Account, unread: number) => string {
  const { t, i18n } = useTranslation();
  return (account, unread) => {
    const name = account.displayName || account.email;
    // The visible name and counter come first, together (WCAG 2.5.3, label in name), then the address.
    const shown = unread > 0 ? t("sidebar.folderUnread", { name, count: unread }) : name;
    const label = name === account.email ? shown : t("nav.account", { name: shown, email: account.email });
    const { state, error } = account.status;
    if (state !== "offline" && state !== "error") return label;
    const reasonKey = error ? `errors.codes.${error.code}.title` : null;
    const reason = reasonKey && i18n.exists(reasonKey) ? t(reasonKey as "errors.codes.internal.title") : null;
    const status = t(`nav.accountState.${state}`);
    return t("sidebar.accountTrouble", { label, status: reason ? t("sidebar.statusWithReason", { status, reason }) : status });
  };
}

interface AccountRowProps {
  account: Account;
  item: TreeNavItem;
  unread: number;
  nav: ReturnType<typeof useTreeNavigation>;
  onToggle: () => void;
}

/**
 * Stage 18 (task D): the account's "…": "New folder…" at the top of the account and "Reload
 * folders from server". Out of the Tab order like a folder's (the row's context-menu key opens it).
 */
function AccountMenu({ account, returnFocus }: { account: Account; returnFocus: () => void }) {
  const { t } = useTranslation();
  const items: OverflowItem[] = [...useFolderCreateItem(account, null, returnFocus), useReloadFoldersItem(account)];
  return (
    <OverflowMenu
      label={t("folderEdit.accountMenu", { name: account.displayName || account.email })}
      items={items}
      tabIndex={-1}
      returnFocus={returnFocus}
      narrow
      data-testid={`account-menu-${account.id}`}
    />
  );
}

/** The account at the root of its tree: colour and name, its trouble, unread total; opens and closes. */
function AccountRow({ account, item, unread, nav, onToggle }: AccountRowProps) {
  const label = useAccountRowLabel()(account, unread);
  const Chevron = item.expanded ? ChevronDown : ChevronRight;
  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(ACCOUNT_KEY)}
      aria-label={label}
      title={label}
      onClick={onToggle}
      data-testid={`sidebar-account-${account.id}`}
      data-account-state={account.status.state}
      // `group/folder`: the "…" shows on hover and focus as on a folder's row (`RowMenuSlot`).
      className="group/account group/folder relative flex h-8 min-w-0 items-center gap-2 rounded-control ps-1 pe-2 text-13 font-semibold text-primary transition-colors hover:bg-rail-hover"
    >
      <span aria-hidden className="flex w-5 shrink-0 justify-center text-tertiary group-hover/account:text-primary">
        <Chevron className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </span>
      {/* The colour never stands alone: the name is next to it. */}
      <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
      <span className="truncate">{account.displayName || account.email}</span>
      <AccountStatusMark account={account} />
      {/* A space before the counter: name and number are two words of the text, as in the name.
          Stage 18: it gives its place to the "…" like a folder's counter. */}
      <UnreadCount count={unread} />
      <RowMenuSlot>
        <AccountMenu
          account={account}
          returnFocus={() => {
            nav.focus(ACCOUNT_KEY);
          }}
        />
      </RowMenuSlot>
    </li>
  );
}

/**
 * The folders of one account under its own row (docs/DESIGN.md, "Seitenspalte"): role folders
 * first in a fixed order, then the account's own folders as a tree, each with its unread counter.
 * The account's row opens and closes the whole account (remembered while the app runs). Kept
 * current by `mailboxesChanged`. One tree, one Tab stop.
 */
export function MailboxTree({ account, currentMailboxId }: MailboxTreeProps) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id);
  const folderName = useFolderName();
  const collapsed = useUiStore((state) => state.collapsedMailboxes);
  const toggleMailbox = useUiStore((state) => state.toggleMailbox);
  const accountOpen = useUiStore((state) => !state.collapsedAccounts.has(account.id));
  const toggleAccount = useUiStore((state) => state.toggleAccount);
  const open = useOpenMailbox(account);
  // A placeholder that holds no mail only opens and closes.
  const activate = (mailbox: Mailbox) => {
    if (mailbox.selectable) open(mailbox);
    else toggleMailbox(mailbox.id);
  };
  const tree = useMemo(() => buildMailboxTree(mailboxes.data ?? []), [mailboxes.data]);
  const rows = accountOpen ? visibleRows(tree, collapsed, folderName) : [];
  const accountItem: TreeNavItem = {
    key: ACCOUNT_KEY,
    level: 1,
    posinset: 1,
    setsize: 1,
    expanded: accountOpen,
    text: account.displayName || account.email,
  };
  const [coloring, setColoring] = useState<MailboxId | null>(null);
  // Audit 24.11.2026: renames and deletes on their way (only where the account may write).
  const operations = useOperations(account.id, account.writesEnabled && account.protocol !== "jmap").data;
  const pendingOf = (mailbox: Mailbox): "rename" | "delete" | null => {
    for (const page of operations?.pages ?? []) {
      for (const op of page) {
        if (op.mailboxId !== mailbox.id || (op.state !== "queued" && op.state !== "in_flight")) continue;
        if (op.kind === "rename_mailbox") return "rename";
        if (op.kind === "delete_mailbox") return "delete";
      }
    }
    return null;
  };
  const colored = coloring === null ? undefined : mailboxes.data?.find((mailbox) => mailbox.id === coloring);
  const nav = useTreeNavigation({
    items: [accountItem, ...rows],
    currentKey: currentMailboxId === null ? null : String(currentMailboxId),
    onOpen: (key) => {
      if (key === ACCOUNT_KEY) {
        toggleAccount(account.id);
        return;
      }
      const mailbox = rows.find((row) => row.key === key)?.node.mailbox;
      if (mailbox) activate(mailbox);
    },
    onExpand: (key, expanded) => {
      if (key === ACCOUNT_KEY) {
        if (accountOpen !== expanded) toggleAccount(account.id);
        return;
      }
      const id = Number(key);
      if (collapsed.has(id) === expanded) toggleMailbox(id);
    },
  });

  return (
    <>
      <ul {...nav.treeProps} aria-label={t("sidebar.folders", { account: account.displayName || account.email })} className="flex flex-col">
        <AccountRow
          account={account}
          item={accountItem}
          unread={accountUnread(mailboxes.data ?? [])}
          nav={nav}
          onToggle={() => {
            toggleAccount(account.id);
          }}
        />
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
            account={account}
            pending={pendingOf(row.node.mailbox)}
          />
        ))}
      </ul>
      {accountOpen && mailboxes.isError && (
        <ErrorNotice
          context={t("sidebar.loadFailed")}
          error={mailboxes.error}
          onRetry={() => {
            void mailboxes.refetch();
          }}
        />
      )}
      {accountOpen && mailboxes.isSuccess && tree.length === 0 && <p className="ps-8 pe-2 text-12 text-tertiary">{t("sidebar.noFolders")}</p>}
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
