import { ArrowDown, ArrowUp, Folder, Palette, Star, StarOff, type LucideIcon } from "lucide-react";
import { useId, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, AccountColor, Mailbox, MailboxId } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { accountShortNames } from "../accounts/accountName";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import { FloatingPanel } from "../actions/Pickers";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { isRoleView } from "../mailboxes/tree";
import { useDropTarget } from "../mailboxes/useDropTarget";
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
  /** In the favourites list: its order, for "Move up" and "Move down". */
  order?: readonly MailboxId[];
  onColor: () => void;
}

/** The "…" of a folder in the sidebar: favourite on or off, its place among the favourites, colour. */
export function FolderMenu({ mailbox, order, onColor }: FolderMenuProps) {
  const { t } = useTranslation();
  const name = useFolderName()(mailbox);
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
  return <OverflowMenu label={t("organize.favorites.menu", { name })} items={items} data-testid={`folder-menu-${String(mailbox.id)}`} />;
}

function folderIcon(mailbox: Mailbox): LucideIcon {
  return isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder;
}

function FolderGlyph({ icon: Icon }: { icon: LucideIcon }) {
  return <Icon aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />;
}

interface FavoriteRowProps {
  mailbox: Mailbox;
  order: readonly MailboxId[];
  accountName: string | undefined;
  current: boolean;
  onColor: (mailbox: Mailbox) => void;
}

function FavoriteRow({ mailbox, order, accountName, current, onColor }: FavoriteRowProps) {
  const { t, i18n } = useTranslation();
  const name = useFolderName()(mailbox);
  const colorName = useColorName();
  const showMailbox = useUiStore((state) => state.showMailbox);
  const change = useChangeFolder();
  const drop = useDropTarget(mailbox);
  const index = order.indexOf(mailbox.id);
  const [moved, setMoved] = useState<string | null>(null);
  const shown = accountName ? t("organize.favorites.inAccount", { name, account: accountName }) : name;
  const named = mailbox.color === undefined || mailbox.color === null ? shown : t("organize.favorites.withColor", { name: shown, color: colorName(mailbox.color) });
  const label = mailbox.unread > 0 ? t("sidebar.folderUnread", { name: named, count: mailbox.unread }) : named;

  // Alt+Arrow keys move the favourite (the menu offers the same).
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    const step = event.key === "ArrowUp" ? -1 : 1;
    const target = index + step;
    if (index < 0 || target < 0 || target >= order.length) return;
    event.preventDefault();
    const next = [...order];
    next.splice(index, 1);
    next.splice(target, 0, mailbox.id);
    change.mutate({ kind: "reorder", mailbox, order: next });
    setMoved(t("organize.favorites.moved", { name, position: target + 1, count: order.length }));
  };

  return (
    <li className="group/folder flex items-center gap-1">
      <button
        type="button"
        aria-current={current ? "page" : undefined}
        aria-label={label}
        aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
        title={mailbox.path}
        disabled={!mailbox.selectable}
        onClick={() => {
          showMailbox(mailbox.accountId, mailbox.id);
        }}
        onKeyDown={onKeyDown}
        {...drop.handlers}
        data-testid={`favorite-${String(mailbox.id)}`}
        data-drop-target={drop.over ? "over" : drop.accepts ? "ready" : undefined}
        className={cx(
          "flex h-8 min-w-0 flex-1 items-center gap-2 rounded-control px-2 text-13 transition-colors",
          current ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-rail-hover hover:text-primary",
          drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
        )}
      >
        <FolderGlyph icon={folderIcon(mailbox)} />
        <span className="truncate">{name}</span>
        <FolderColorDot color={mailbox.color} />
        {accountName && <span className="truncate text-12 text-tertiary">{accountName}</span>}
        {mailbox.unread > 0 && (
          <span aria-hidden className="ms-auto shrink-0 text-12 font-medium text-secondary tabular-nums">
            {new Intl.NumberFormat(i18n.language).format(mailbox.unread)}
          </span>
        )}
      </button>
      {/* Shown on hover and while focused, as in the folder tree; always reachable with Tab. */}
      <div className="flex opacity-0 group-hover/folder:opacity-100 focus-within:opacity-100">
        <FolderMenu
          mailbox={mailbox}
          order={order}
          onColor={() => {
            onColor(mailbox);
          }}
        />
      </div>
      <span aria-live="polite" className="sr-only">
        {moved}
      </span>
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
  const favorites = useFavoriteMailboxes();
  const [coloring, setColoring] = useState<Mailbox | null>(null);
  const names = accounts.length > 1 ? accountShortNames(accounts) : null;
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
  const list = (favorites.data ?? []).filter((mailbox) => accounts.some((account) => account.id === mailbox.accountId));
  if (list.length === 0) return null;
  const order = list.map((mailbox) => mailbox.id);
  const currentId = view.kind === "mailbox" ? view.mailboxId : null;
  return (
    <section aria-label={t("organize.favorites.title")} className="flex flex-col gap-1" data-testid="favorite-folders">
      <h2 className="flex h-8 items-center px-2 text-12 font-semibold text-secondary">{t("organize.favorites.title")}</h2>
      <ul className="flex flex-col">
        {list.map((mailbox) => (
          <FavoriteRow
            key={mailbox.id}
            mailbox={mailbox}
            order={order}
            accountName={names?.get(mailbox.accountId)}
            current={mailbox.id === currentId}
            onColor={setColoring}
          />
        ))}
      </ul>
      {coloring && (
        <FolderColorPicker
          mailbox={list.find((mailbox) => mailbox.id === coloring.id) ?? coloring}
          onClose={() => {
            setColoring(null);
          }}
          restoreFocus={() => undefined}
        />
      )}
    </section>
  );
}
