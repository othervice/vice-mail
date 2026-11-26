import { ArrowDown, ArrowUp, ChevronDown, ChevronRight, CircleExclamationPoint, CloudOff, Folder, HardDrive, Pencil, type LucideIcon } from "lucide-react";
import { useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { nearlyFullPercent, useAccountQuota } from "../accounts/quota";
import { ErrorNotice } from "../../components/ErrorNotice";
import { treeItemAria, useTreeNavigation, type TreeNavItem } from "../../components/useTreeNavigation";
import { ACCOUNT_NAME_MAX_CHARS, type Account, type Mailbox, type MailboxId } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { useAccountDragSource } from "../accounts/accountDrag";
import { movedOrder, useAccountArrange } from "../accounts/arrange";
import { useAccounts } from "../accounts/queries";
// Stage 16: folder colours and favourites.
import { FolderColorDot, FolderColorPicker, FolderMenu, RowMenuSlot, useColorName, useFolderName } from "../organize/folders";
import { accountFill } from "../accounts/accountColor";
import { indentClass, sidebarIconClass, sidebarRowClass } from "../layout/sidebarStyles";
import { useUiStore } from "../../stores/ui";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import { useOperations } from "../actions/queries";
import { useFolderCreateItem, useReloadFoldersItem } from "./FolderEdit";
import { accountReadScope, useMarkReadItem } from "./markRead";
import { useMailboxes } from "./queries";
import { buildMailboxTree, isRoleView, type MailboxNode } from "./tree";
import { DROP_REFUSED_CLASS, dropTargetState, useDropTarget } from "./useDropTarget";

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
  return accountReadScope(mailboxes).reduce((sum, mailbox) => sum + mailbox.unread, 0);
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
  // Rows dragged out of any list can be dropped here (stage 22, decision 0081); a closed folder
  // opens while they rest on it.
  const drop = useDropTarget(mailbox.selectable ? { kind: "folder", mailbox } : undefined, {
    id: `tree-${String(mailbox.id)}`,
    folder: name,
    onExpand:
      row.expanded === false
        ? () => {
            toggleMailbox(mailbox.id);
          }
        : undefined,
  });

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
      {...drop.handlers}
      data-testid={mailbox.selectable ? `mailbox-${String(mailbox.id)}` : undefined}
      data-folder-role={mailbox.role}
      data-drop-target={dropTargetState(drop)}
      className={sidebarRowClass(
        current,
        cx(
          "group/folder gap-1 pe-2",
          // Folders start one level below their account's row, without indentation of their own.
          indentClass(row.level - 1),
          !mailbox.selectable && !current && "text-tertiary",
          drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
          drop.refused && DROP_REFUSED_CLASS,
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
  // Stage 20 (decision 0060): a mailbox at least 90 % full shows on the account.
  const full = nearlyFullPercent(useAccountQuota(account.id).data);
  if (state !== "offline" && state !== "error") {
    if (full === null) return null;
    return (
      <HardDrive
        aria-hidden
        data-testid={`account-quota-mark-${account.id}`}
        className="size-(--ui-icon-small) shrink-0 text-warning"
        strokeWidth={1.75}
      />
    );
  }
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
export function useAccountRowLabel(): (account: Account, unread: number, fullPercent?: number | null) => string {
  const { t, i18n } = useTranslation();
  return (account, unread, fullPercent = null) => {
    const name = account.displayName || account.email;
    // The visible name and counter come first, together (WCAG 2.5.3, label in name), then the address.
    const shown = unread > 0 ? t("sidebar.folderUnread", { name, count: unread }) : name;
    const named = name === account.email ? shown : t("nav.account", { name: shown, email: account.email });
    // Stage 20 (decision 0060): "mailbox 95 % full".
    const label = fullPercent === null ? named : t("sidebar.quotaFull", { label: named, percent: fullPercent });
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
  /** The account's folders, for "Mark all as read". */
  mailboxes: readonly Mailbox[];
  item: TreeNavItem;
  unread: number;
  nav: ReturnType<typeof useTreeNavigation>;
  onToggle: () => void;
  /** Stage 22: the name is being edited in place. */
  editing: boolean;
  onRename: () => void;
  /** The edit ends: the new name, or null (cancelled). */
  onRenamed: (name: string | null) => void;
  onMove: (step: -1 | 1) => void;
}

interface AccountMenuProps {
  account: Account;
  mailboxes: readonly Mailbox[];
  returnFocus: () => void;
  onRename: () => void;
  onMove: (step: -1 | 1) => void;
}

/**
 * Stage 22 (decision 0078): the account's name in the lists, edited in place (double click, F2 or
 * "Rename" in its menu). Enter or leaving the field saves, Escape cancels; empty shows the address.
 */
function AccountNameField({ account, onDone }: { account: Account; onDone: (name: string | null) => void }) {
  const { t } = useTranslation();
  const [value, setValue] = useState(account.displayName);
  const field = useRef<HTMLInputElement>(null);
  const finished = useRef(false);
  const hintId = useId();
  useLayoutEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);
  const finish = (name: string | null) => {
    if (finished.current) return;
    finished.current = true;
    onDone(name);
  };
  return (
    <>
      <input
        ref={field}
        type="text"
        value={value}
        maxLength={ACCOUNT_NAME_MAX_CHARS}
        placeholder={account.email}
        aria-label={t("accountEdit.nameField", { email: account.email })}
        aria-describedby={hintId}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => {
          setValue(event.target.value);
        }}
        onKeyDown={(event) => {
          // The tree's keys (arrows, type-ahead) and the column's Escape stay out of the field.
          event.stopPropagation();
          if (event.key === "Enter") {
            event.preventDefault();
            finish(value);
          } else if (event.key === "Escape") {
            event.preventDefault();
            finish(null);
          }
        }}
        onBlur={() => {
          finish(value);
        }}
        onClick={(event) => {
          event.stopPropagation();
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
        }}
        data-testid={`account-name-field-${account.id}`}
        className="h-6 min-w-0 flex-1 rounded-control bg-surface px-1 text-13 font-semibold text-primary outline-2 -outline-offset-1 outline-accent"
      />
      <span id={hintId} className="sr-only">
        {t("accountEdit.nameHint")}
      </span>
    </>
  );
}

/**
 * Stage 18 (task D): the account's "…": "New folder…" at the top of the account and "Reload
 * folders from server". Out of the Tab order like a folder's (the row's context-menu key opens it).
 * Stage 21 (decision 0063): first "Mark all as read": the inbox and the own folders, what the
 * account's counter counts.
 */
function AccountMenu({ account, mailboxes, returnFocus, onRename, onMove }: AccountMenuProps) {
  const { t } = useTranslation();
  const name = account.displayName || account.email;
  const markItems = useMarkReadItem("account", [{ account, mailboxes: accountReadScope(mailboxes) }], name);
  // Stage 22 (decision 0078): renaming and moving the account, also for keyboard and screen readers.
  const accounts = useAccounts().data ?? [];
  const index = accounts.findIndex((a) => a.id === account.id);
  const arrangeItems: OverflowItem[] = [
    { id: "rename", label: t("accountEdit.rename"), icon: Pencil, keyshortcuts: "F2", onSelect: onRename, "data-testid": `account-rename-${account.id}` },
    ...(accounts.length > 1
      ? [
          {
            id: "moveUp",
            label: t("accountEdit.moveUp"),
            icon: ArrowUp,
            keyshortcuts: "Alt+ArrowUp",
            inert: index <= 0,
            onSelect: () => {
              if (index > 0) onMove(-1);
            },
            "data-testid": `account-move-up-${account.id}`,
          },
          {
            id: "moveDown",
            label: t("accountEdit.moveDown"),
            icon: ArrowDown,
            keyshortcuts: "Alt+ArrowDown",
            inert: index < 0 || index >= accounts.length - 1,
            onSelect: () => {
              if (index >= 0 && index < accounts.length - 1) onMove(1);
            },
            "data-testid": `account-move-down-${account.id}`,
          },
        ]
      : []),
  ];
  const items: OverflowItem[] = [...markItems, ...useFolderCreateItem(account, null, returnFocus), useReloadFoldersItem(account), ...arrangeItems];
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
function AccountRow({ account, mailboxes, item, unread, nav, onToggle, editing, onRename, onRenamed, onMove }: AccountRowProps) {
  const { t } = useTranslation();
  const full = nearlyFullPercent(useAccountQuota(account.id).data);
  const label = useAccountRowLabel()(account, unread, full);
  const Chevron = item.expanded ? ChevronDown : ChevronRight;
  const toggleAccount = useUiStore((state) => state.toggleAccount);
  // Stage 22 (decision 0078): the heading is dragged to put the account elsewhere.
  const dragSource = useAccountDragSource(account, !editing);
  // Stage 22 (decision 0081): a closed account opens while dragged mail rests on its heading.
  const expandOnDrag = useDropTarget(undefined, {
    id: `account-${account.id}`,
    folder: label,
    onExpand: item.expanded
      ? undefined
      : () => {
          toggleAccount(account.id);
        },
  });
  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(ACCOUNT_KEY)}
      aria-label={label}
      aria-description={editing ? undefined : t("accountEdit.dragHint")}
      aria-keyshortcuts="F2 Alt+ArrowUp Alt+ArrowDown"
      title={label}
      onClick={onToggle}
      onDoubleClick={(event) => {
        event.preventDefault();
        onRename();
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === "F2" && !event.altKey && !event.ctrlKey && !event.metaKey) {
          event.preventDefault();
          onRename();
          return;
        }
        if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
        event.preventDefault();
        onMove(event.key === "ArrowUp" ? -1 : 1);
      }}
      {...dragSource}
      {...expandOnDrag.handlers}
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
      {editing ? (
        <AccountNameField account={account} onDone={onRenamed} />
      ) : (
        <span className="truncate" data-testid={`sidebar-account-name-${account.id}`}>
          {account.displayName || account.email}
        </span>
      )}
      <AccountStatusMark account={account} />
      {/* A space before the counter: name and number are two words of the text, as in the name.
          Stage 18: it gives its place to the "…" like a folder's counter. */}
      {!editing && <UnreadCount count={unread} />}
      {!editing && (
        <RowMenuSlot>
          <AccountMenu
            account={account}
            mailboxes={mailboxes}
            returnFocus={() => {
              nav.focus(ACCOUNT_KEY);
            }}
            onRename={onRename}
            onMove={onMove}
          />
        </RowMenuSlot>
      )}
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
  const push = useNoticeStore((state) => state.push);
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
  // Stage 22 (decision 0078): renaming in place and moving the account, with what happened said.
  const [editing, setEditing] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const arrange = useAccountArrange();
  const accounts = useAccounts().data ?? [];
  const focusHeading = () => {
    // The account's tree moved in the column: its heading takes the focus again once it is there.
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-testid="sidebar-account-${account.id}"]`)?.focus();
    });
  };
  const move = (step: -1 | 1) => {
    const order = movedOrder(accounts, account.id, step);
    if (!order) return;
    void arrange.reorder(order).then((ok) => {
      if (!ok) return;
      setAnnouncement(t("accountEdit.moved", { name: account.displayName || account.email, position: order.indexOf(account.id) + 1, count: order.length }));
      focusHeading();
    });
    focusHeading();
  };
  const renamed = (name: string | null) => {
    setEditing(false);
    nav.focus(ACCOUNT_KEY);
    if (name === null || name.trim() === account.displayName) return;
    void arrange.rename(account.id, name).then((error) => {
      if (error) {
        push({ kind: "error", context: "accounts", error });
        return;
      }
      const shown = name.trim();
      setAnnouncement(shown ? t("accountEdit.renamed", { email: account.email, name: shown }) : t("accountEdit.renamedToAddress", { email: account.email }));
    });
  };
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
          mailboxes={mailboxes.data ?? []}
          item={accountItem}
          unread={accountUnread(mailboxes.data ?? [])}
          nav={nav}
          onToggle={() => {
            toggleAccount(account.id);
          }}
          editing={editing}
          onRename={() => {
            setEditing(true);
          }}
          onRenamed={renamed}
          onMove={move}
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
      <span aria-live="polite" className="sr-only" data-testid={`account-announce-${account.id}`}>
        {announcement}
      </span>
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
