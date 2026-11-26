import { ArrowDown, ArrowUp, Folder, Palette, Star, StarOff, type LucideIcon } from "lucide-react";
import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { treeItemAria, useTreeNavigation, type TreeNavItem } from "../../components/useTreeNavigation";
import type { Account, AccountColor, Mailbox, MailboxId } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { accountFill, accountText } from "../accounts/accountColor";
import { accountShortNames } from "../accounts/accountName";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import { FloatingPanel } from "../actions/Pickers";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { useFolderCreateItem, useFolderEditItems } from "../mailboxes/FolderEdit";
import { useMarkReadItem } from "../mailboxes/markRead";
import { useMboxItems } from "../mbox/MboxJobs";
import { isRoleView } from "../mailboxes/tree";
import { DROP_REFUSED_CLASS, dropTargetState, useDropTarget } from "../mailboxes/useDropTarget";
import { SECTION_KEY, SectionRow, useSection } from "../layout/SectionRow";
import { sidebarIconClass, sidebarRowClass } from "../layout/sidebarStyles";
import { isFavorite, useChangeFolder, useFavoriteMailboxes } from "./queries";

/** The eight colours of the account palette (decision 0033: folders use the same). */
export const FOLDER_COLORS: readonly AccountColor[] = [0, 1, 2, 3, 4, 5, 6, 7];

/** The name of a palette colour ("Blue"), so a colour never stands alone. */
const COLOR_KEYS = ["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7"] as const;

export function useColorName(): (color: AccountColor) => string {
  const { t } = useTranslation();
  return (color) => t(`organize.colors.${COLOR_KEYS[((Math.trunc(color) % 8) + 8) % 8] ?? "c0"}`);
}

/** A folder's name as the UI shows it: the translated role, or its own name. */
export function useFolderName(): (mailbox: Mailbox) => string {
  const { t } = useTranslation();
  return (mailbox) => (mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`));
}

/** The folder's colour as a small dot next to its name (hidden: the name carries the meaning). */
export function FolderColorDot({ color }: { color: AccountColor | null | undefined }) {
  if (color === undefined || color === null) return null;
  return <span aria-hidden data-testid="folder-color" className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(color))} />;
}

interface ColorPickerProps {
  mailbox: Mailbox;
  onClose: () => void;
  restoreFocus: () => void;
}

/**
 * "Folder colour…": no colour or one of the eight account colours, each with its name (a radio
 * group: arrow keys move and choose, as native radios do). The choice applies at once.
 */
export function FolderColorPicker({ mailbox, onClose, restoreFocus }: ColorPickerProps) {
  const { t } = useTranslation();
  const name = useFolderName()(mailbox);
  const colorName = useColorName();
  const change = useChangeFolder();
  const groupName = useId();
  const hintId = useId();
  const current = mailbox.color ?? null;
  const choices: (AccountColor | null)[] = [null, ...FOLDER_COLORS];
  return (
    <FloatingPanel title={t("organize.favorites.colorTitle", { name })} onClose={onClose} restoreFocus={restoreFocus} data-testid="folder-color-picker">
      <p id={hintId} className="text-12 text-secondary">
        {t("organize.favorites.colorHint")}
      </p>
      <div role="radiogroup" aria-label={t("organize.favorites.colorTitle", { name })} aria-describedby={hintId} className="grid grid-cols-3 gap-1">
        {choices.map((color) => {
          const label = color === null ? t("organize.favorites.noColor") : colorName(color);
          const checked = current === color;
          return (
            <label
              key={color ?? "none"}
              className={cx(
                "flex h-8 cursor-default items-center gap-2 rounded-control px-2 text-13 transition-colors hover:bg-hover has-focus-visible:outline-2 has-focus-visible:outline-focus-ring",
                checked ? "bg-selected font-medium text-primary" : "text-secondary",
              )}
            >
              <input
                type="radio"
                name={groupName}
                checked={checked}
                // The focus starts on the chosen colour, as in a native radio group.
                autoFocus={checked}
                onChange={() => {
                  change.mutate({ kind: "color", mailbox, color });
                }}
                className="sr-only"
                data-testid={`folder-color-${color === null ? "none" : String(color)}`}
              />
              {color === null ? (
                <span aria-hidden className="size-(--ui-dot) shrink-0 rounded-full border border-border-strong" />
              ) : (
                <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(color))} />
              )}
              <span className="truncate">{label}</span>
            </label>
          );
        })}
      </div>
    </FloatingPanel>
  );
}

interface FolderMenuProps {
  mailbox: Mailbox;
  /** Audit 24.11.2026: in the account's tree, "Rename folder…" and "Delete folder…" too. */
  account?: Account;
  /**
   * Stage 21: the folder's account where `account` is not given (the favourites), for "Mark
   * folder as read" alone.
   */
  owner?: Account | undefined;
  /** In the favourites list: its order, for "Move up" and "Move down". */
  order?: readonly MailboxId[];
  onColor: () => void;
  /**
   * The folder's row in its tree: the focus goes back there when the menu closes. The "…" button
   * is then out of the Tab order (Shift+F10 or the context-menu key on the row opens the menu).
   */
  returnFocus?: () => void;
}

/**
 * The "…" of a folder in the sidebar (also its right click, Shift+F10 and the context-menu key):
 * "Mark folder as read" (stage 21), favourite on or off, its place among the favourites, colour,
 * and in the account's tree new subfolder, rename, delete, mbox.
 */
export function FolderMenu({ mailbox, account, owner, order, onColor, returnFocus }: FolderMenuProps) {
  const { t } = useTranslation();
  const editItems = useFolderEditItems(account, mailbox, returnFocus);
  // Stage 18: "New subfolder…" in the account's tree.
  const createItems = useFolderCreateItem(account, mailbox, returnFocus);
  // Stage 19 (decision 0057): the folder as an mbox file, an mbox file into the folder.
  const mbox = useMboxItems(account, mailbox);
  const name = useFolderName()(mailbox);
  const markAccount = account ?? owner;
  const markItems = useMarkReadItem("folder", markAccount ? [{ account: markAccount, mailboxes: [mailbox] }] : [], name);
  const change = useChangeFolder();
  const favorite = isFavorite(mailbox);
  const index = order?.indexOf(mailbox.id) ?? -1;
  const move = (step: -1 | 1) => {
    if (!order || index < 0) return;
    const next = [...order];
    const [moved] = next.splice(index, 1);
    if (moved === undefined) return;
    next.splice(index + step, 0, moved);
    change.mutate({ kind: "reorder", mailbox, order: next });
  };
  const items: OverflowItem[] = [
    ...markItems,
    favorite
      ? {
          id: "favorite-remove",
          label: t("organize.favorites.remove"),
          icon: StarOff,
          onSelect: () => {
            change.mutate({ kind: "favorite", mailbox, favorite: false });
          },
          "data-testid": "folder-favorite-remove",
        }
      : {
          id: "favorite-add",
          label: t("organize.favorites.add"),
          icon: Star,
          onSelect: () => {
            change.mutate({ kind: "favorite", mailbox, favorite: true });
          },
          "data-testid": "folder-favorite-add",
        },
  ];
  if (order && index > 0) {
    items.push({
      id: "favorite-up",
      label: t("organize.favorites.moveUp"),
      icon: ArrowUp,
      detail: "⌥↑",
      onSelect: () => {
        move(-1);
      },
      "data-testid": "folder-favorite-up",
    });
  }
  if (order && index >= 0 && index < order.length - 1) {
    items.push({
      id: "favorite-down",
      label: t("organize.favorites.moveDown"),
      icon: ArrowDown,
      detail: "⌥↓",
      onSelect: () => {
        move(1);
      },
      "data-testid": "folder-favorite-down",
    });
  }
  items.push({ id: "color", label: t("organize.favorites.color"), icon: Palette, onSelect: onColor, "data-testid": "folder-color-open" });
  items.push(...createItems, ...editItems);
  items.push(...mbox.items);
  return (
    <OverflowMenu
      label={t("organize.favorites.menu", { name })}
      items={items}
      {...(returnFocus && { tabIndex: -1, returnFocus })}
      narrow
      data-testid={`folder-menu-${String(mailbox.id)}`}
    />
  );
}

/**
 * The "…" of a row in a sidebar tree: shown on hover, while the row has the keyboard focus and
 * while its menu is open, over the end of the row where the unread counter is (which hides
 * meanwhile), so it takes no width from the name. Its clicks stay here, so they do not also open
 * the folder.
 */
export function RowMenuSlot({ children }: { children: ReactNode }) {
  return (
    <div
      onClick={(event) => {
        event.stopPropagation();
      }}
      // Stage 21: also while its menu is open, wherever the focus is (a click on the menu's edge
      // moves it to the page in WebKit; the open menu must not turn invisible).
      className="absolute inset-y-0 end-0 flex items-center opacity-0 group-hover/folder:opacity-100 group-focus-visible/folder:opacity-100 focus-within:opacity-100 has-[[aria-expanded=true]]:opacity-100"
    >
      {children}
    </div>
  );
}

function folderIcon(mailbox: Mailbox): LucideIcon {
  return isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder;
}

function FolderGlyph({ icon: Icon, current, tone }: { icon: LucideIcon; current: boolean; tone: string | undefined }) {
  return <Icon aria-hidden className={sidebarIconClass(current, tone)} strokeWidth={1.75} />;
}

interface FavoriteRowProps {
  item: TreeNavItem;
  mailbox: Mailbox;
  order: readonly MailboxId[];
  /** Stage 21: the folder's account ("Mark folder as read"). */
  account: Account | undefined;
  accountName: string | undefined;
  /**
   * Stage 23 (decision 0085): the account's short name shows only where another favourite has the
   * same name (the icon's colour tells the accounts apart otherwise, the name says it always).
   */
  showAccount: boolean;
  current: boolean;
  nav: ReturnType<typeof useTreeNavigation>;
  onColor: (mailbox: Mailbox) => void;
  /** Announces a move made with Alt+arrow keys. */
  onMoved: (text: string) => void;
}

function FavoriteRow({ item, mailbox, order, account, accountName, showAccount, current, nav, onColor, onMoved }: FavoriteRowProps) {
  const { t, i18n } = useTranslation();
  const name = useFolderName()(mailbox);
  const colorName = useColorName();
  const showMailbox = useUiStore((state) => state.showMailbox);
  const change = useChangeFolder();
  // Stage 22 (decision 0081): rows of any list, also of other accounts, move here.
  const drop = useDropTarget(mailbox.selectable ? { kind: "folder", mailbox } : undefined, { id: `favorite-${String(mailbox.id)}`, folder: name });
  const index = order.indexOf(mailbox.id);
  const shown = accountName ? t("organize.favorites.inAccount", { name, account: accountName }) : name;
  const named = mailbox.color === undefined || mailbox.color === null ? shown : t("organize.favorites.withColor", { name: shown, color: colorName(mailbox.color) });
  const label = mailbox.unread > 0 ? t("sidebar.folderUnread", { name: named, count: mailbox.unread }) : named;

  // Alt+Arrow keys move the favourite (the menu offers the same); the focus stays on it.
  const onKeyDown = (event: KeyboardEvent<HTMLLIElement>) => {
    if (event.target !== event.currentTarget) return;
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    const step = event.key === "ArrowUp" ? -1 : 1;
    const target = index + step;
    if (index < 0 || target < 0 || target >= order.length) return;
    event.preventDefault();
    const next = [...order];
    next.splice(index, 1);
    next.splice(target, 0, mailbox.id);
    nav.keepFocus(item.key);
    change.mutate({ kind: "reorder", mailbox, order: next });
    onMoved(t("organize.favorites.moved", { name, position: target + 1, count: order.length }));
  };

  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(item.key)}
      aria-current={current ? "page" : undefined}
      aria-label={label}
      aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
      aria-disabled={mailbox.selectable ? undefined : true}
      title={mailbox.path}
      onClick={() => {
        if (mailbox.selectable) showMailbox(mailbox.accountId, mailbox.id);
      }}
      onKeyDown={onKeyDown}
      {...drop.handlers}
      data-testid={`favorite-${String(mailbox.id)}`}
      data-drop-target={dropTargetState(drop)}
      className={sidebarRowClass(
        current,
        cx(
          "group/folder px-2",
          drop.over && "bg-accent-soft outline-2 -outline-offset-2 outline-accent",
          drop.refused && DROP_REFUSED_CLASS,
        ),
      )}
    >
      <span className="flex min-w-0 flex-1 items-center gap-2">
        {/* Stage 23 (decision 0085): the icon in its account's colour. */}
        <FolderGlyph icon={folderIcon(mailbox)} current={current} tone={account && accountText(account.color)} />
        <span className="truncate">{name}</span>
        <FolderColorDot color={mailbox.color} />
        {/* The folder's name keeps its room; the account's short name gives way first. */}
        {showAccount && accountName && <span className="shrink-[4] truncate text-12 text-tertiary">{accountName}</span>}
        {mailbox.unread > 0 && (
          <span
            aria-hidden
            className={cx(
              "ms-auto shrink-0 ps-2 text-12 tabular-nums group-hover/folder:invisible group-focus-visible/folder:invisible group-has-[[aria-expanded=true]]/folder:invisible",
              mailbox.role === "inbox" ? "font-semibold text-secondary" : "text-tertiary",
            )}
          >
            {new Intl.NumberFormat(i18n.language).format(mailbox.unread)}
          </span>
        )}
      </span>
      <RowMenuSlot>
        <FolderMenu
          mailbox={mailbox}
          owner={account}
          order={order}
          onColor={() => {
            onColor(mailbox);
          }}
          returnFocus={() => {
            nav.focus(item.key);
          }}
        />
      </RowMenuSlot>
    </li>
  );
}

/**
 * Top of the unfolded sidebar (stage 16): the favourite folders of every account in their order,
 * each with its colour next to its name and, with several accounts, the account's short name.
 * Nothing while there are none (a folder's "…" menu adds one).
 */
export function FavoriteFolders({ accounts }: { accounts: readonly Account[] }) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showMailbox = useUiStore((state) => state.showMailbox);
  const favorites = useFavoriteMailboxes();
  const folderName = useFolderName();
  const [coloring, setColoring] = useState<Mailbox | null>(null);
  const [moved, setMoved] = useState<string | null>(null);
  const headingId = useId();
  const names = accounts.length > 1 ? accountShortNames(accounts) : null;
  const section = useSection("favorites", t("organize.favorites.title"));
  const list = (favorites.data ?? []).filter((mailbox) => accounts.some((account) => account.id === mailbox.accountId));
  const items: TreeNavItem[] = list.map((mailbox, index) => ({
    key: String(mailbox.id),
    level: 2,
    posinset: index + 1,
    setsize: list.length,
    text: folderName(mailbox),
  }));
  const currentId = view.kind === "mailbox" ? view.mailboxId : null;
  const nav = useTreeNavigation({
    items: [section.item, ...(section.open ? items : [])],
    currentKey: currentId === null ? null : String(currentId),
    onOpen: (key) => {
      if (key === SECTION_KEY) section.toggle();
      const mailbox = list.find((entry) => String(entry.id) === key);
      if (mailbox?.selectable) showMailbox(mailbox.accountId, mailbox.id);
    },
    onExpand: section.onExpand,
  });
  if (favorites.isError) {
    return (
      <ErrorNotice
        context={t("organize.favorites.loadFailed")}
        error={favorites.error}
        onRetry={() => {
          void favorites.refetch();
        }}
      />
    );
  }
  if (list.length === 0) return null;
  const order = list.map((mailbox) => mailbox.id);
  return (
    <section aria-label={t("organize.favorites.title")} className="flex flex-col" data-testid="favorite-folders">
      <h2 id={headingId} className="sr-only">
        {t("organize.favorites.title")}
      </h2>
      <ul {...nav.treeProps} aria-labelledby={headingId} className="flex flex-col">
        <SectionRow section={section} nav={nav} data-testid="sidebar-section-favorites" />
        {(section.open ? list : []).map((mailbox, index) => {
          const item = items[index];
          if (!item) return null;
          const shownName = folderName(mailbox);
          return (
            <FavoriteRow
              key={mailbox.id}
              item={item}
              mailbox={mailbox}
              order={order}
              account={accounts.find((candidate) => candidate.id === mailbox.accountId)}
              accountName={names?.get(mailbox.accountId)}
              showAccount={list.some((other) => other.id !== mailbox.id && folderName(other) === shownName)}
              current={mailbox.id === currentId}
              nav={nav}
              onColor={setColoring}
              onMoved={setMoved}
            />
          );
        })}
      </ul>
      <span aria-live="polite" className="sr-only">
        {moved}
      </span>
      {coloring && (
        <FolderColorPicker
          mailbox={list.find((mailbox) => mailbox.id === coloring.id) ?? coloring}
          onClose={() => {
            setColoring(null);
          }}
          restoreFocus={() => {
            nav.focus(String(coloring.id));
          }}
        />
      )}
    </section>
  );
}
