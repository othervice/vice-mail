import { Check, ChevronRight, type LucideIcon } from "lucide-react";
import {
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { cx } from "./cx";

/** One command of a menu (`menuitem`, or `menuitemcheckbox` when `checked` is given). */
export interface MenuItemEntry {
  kind: "item";
  id: string;
  label: string;
  icon?: LucideIcon | undefined;
  /** Shown at the end ("E", "⌘U"); not part of the name. */
  detail?: string | undefined;
  /** The shortcut for assistive technology (`aria-keyshortcuts`). */
  keyshortcuts?: string | undefined;
  /** A checkbox item: whether it is checked. Absent: a plain command. */
  checked?: boolean | undefined;
  /** Shown, but does nothing (`aria-disabled`); `describedBy` and `title` say why. */
  disabled?: boolean | undefined;
  /** Id of an element of the menu that describes the item (why it is disabled). */
  describedBy?: string | undefined;
  /** Native tooltip (the reason a disabled item does nothing). */
  title?: string | undefined;
  /**
   * A second line above the label, in small text (the read-only note), with this id so other
   * items can name it in `describedBy`. Hidden from the item's name; it describes the item.
   */
  note?: { id: string; text: string } | undefined;
  /** The item in the accent colour (a way out, like "Allow changes …"). */
  accent?: boolean | undefined;
  /** Depth in a tree (subfolders): the item starts further in, up to three steps. */
  indent?: number | undefined;
  onSelect: () => void;
  testId?: string | undefined;
}

/** An item that opens a menu of its own beside it. */
export interface SubmenuEntry {
  kind: "submenu";
  id: string;
  label: string;
  icon?: LucideIcon | undefined;
  entries: readonly MenuEntry[];
  testId?: string | undefined;
}

export interface SeparatorEntry {
  kind: "separator";
  id: string;
}

/** A small heading inside a menu (the other accounts in "Move to"); not focusable. */
export interface HeadingEntry {
  kind: "heading";
  id: string;
  label: string;
}

export type MenuEntry = MenuItemEntry | SubmenuEntry | SeparatorEntry | HeadingEntry;

/** Where a menu goes: at the pointer (the root), or beside the item that opened it. */
type Anchor = { kind: "point"; x: number; y: number } | { kind: "side"; left: number; right: number; top: number };

interface Box {
  width: number;
  height: number;
}

/** Start padding of items by tree depth (`indent`), 16 px a step. */
const INDENT = ["", "ps-6", "ps-10", "ps-14"] as const;

/** Room kept between a menu and the window's edges. */
export const MENU_MARGIN = 4;

/**
 * Where a menu of `size` goes in a viewport of `view`: at the pointer, to its right and below it,
 * flipped to the left or above where it would leave the window, else pushed in from the edge; a
 * submenu beside its item, on the left where the right has no room, its first item level with the
 * item. A menu taller than the window starts at the top margin (it scrolls inside).
 */
export function placeMenu(anchor: Anchor, size: Box, view: Box, margin = MENU_MARGIN): { x: number; y: number } {
  const fitsX = (x: number) => x >= margin && x + size.width <= view.width - margin;
  const fitsY = (y: number) => y >= margin && y + size.height <= view.height - margin;
  const clampX = (x: number) => Math.max(margin, Math.min(x, view.width - margin - size.width));
  const clampY = (y: number) => Math.max(margin, Math.min(y, view.height - margin - size.height));
  if (anchor.kind === "point") {
    const x = fitsX(anchor.x) ? anchor.x : fitsX(anchor.x - size.width) ? anchor.x - size.width : clampX(anchor.x);
    const y = fitsY(anchor.y) ? anchor.y : fitsY(anchor.y - size.height) ? anchor.y - size.height : clampY(anchor.y);
    return { x, y };
  }
  const x = fitsX(anchor.right) ? anchor.right : fitsX(anchor.left - size.width) ? anchor.left - size.width : clampX(anchor.right);
  return { x, y: clampY(anchor.top) };
}

/** Why a menu closed: `select` and `escape` give the focus back to where the menu came from. */
export type MenuCloseReason = "select" | "escape" | "tab" | "outside";

interface ContextMenuProps {
  /** The menu's accessible name. */
  label: string;
  /** Where it opens, in viewport pixels (the pointer, or the row's corner for the keyboard). */
  point: { x: number; y: number };
  /** A row of icon buttons on top (`group`), e.g. Reply, Archive, Spam, Delete. */
  iconRow?: { label: string; items: readonly MenuItemEntry[] } | undefined;
  entries: readonly MenuEntry[];
  onClose: (reason: MenuCloseReason) => void;
  "data-testid"?: string;
}

/** An open submenu: which entry of its parent opened it, and the parent item's box. */
interface OpenSubmenu {
  id: string;
  anchor: Extract<Anchor, { kind: "side" }>;
}

const ITEM = "[data-menu-item]";

function focusables(menu: HTMLElement | null): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>(ITEM) ?? [])];
}

/**
 * A context menu (WAI-ARIA menu) at the pointer: an optional row of icons on top, items,
 * separators and submenus beside their item. Rendered into `document.body` (above every pane,
 * never clipped), positioned through the CSSOM (the CSP allows no style attributes) and kept inside
 * the window: flipped at the right and bottom edges, a tall menu scrolls.
 *
 * Keyboard: the first item has the focus when it opens. ↓ ↑ move (the icon row is one stop, ← →
 * move within it), Home and End, a letter goes to the next item starting with it; → , Enter or
 * Space open a submenu on its first item, ← or Escape close it again onto its item; Escape on the
 * menu closes it, Tab too. Pointer: hovering an item focuses it (and opens its submenu), a click
 * chooses; a press outside closes the menu. Disabled items stay focusable and say why
 * (`aria-disabled`, `aria-describedby`), choosing them does nothing.
 */
export function ContextMenu({ label, point, iconRow, entries, onClose, ...rest }: ContextMenuProps) {
  const container = useRef<HTMLDivElement>(null);
  const [path, setPath] = useState<readonly OpenSubmenu[]>([]);
  const close = useEffectEvent((reason: MenuCloseReason) => {
    onClose(reason);
  });

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && container.current?.contains(event.target)) return;
      close("outside");
    };
    const onResize = () => {
      close("outside");
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("resize", onResize);
    window.addEventListener("blur", onResize);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("blur", onResize);
    };
  }, []);

  return createPortal(
    <div ref={container} data-testid={rest["data-testid"]} className="contents" data-context-menu>
      <MenuLayer
        depth={0}
        anchor={{ kind: "point", x: point.x, y: point.y }}
        label={label}
        iconRow={iconRow}
        entries={entries}
        path={path}
        setPath={setPath}
        focusOnOpen
        onClose={onClose}
      />
    </div>,
    document.body,
  );
}

interface MenuLayerProps {
  depth: number;
  anchor: Anchor;
  label: string;
  id?: string;
  iconRow?: { label: string; items: readonly MenuItemEntry[] } | undefined;
  entries: readonly MenuEntry[];
  path: readonly OpenSubmenu[];
  setPath: (update: (path: readonly OpenSubmenu[]) => readonly OpenSubmenu[]) => void;
  /** Take the focus onto the first item once placed (opened from the keyboard, or the root). */
  focusOnOpen: boolean;
  /** Closes this submenu and gives the focus back to its item (← and Escape in a submenu). */
  onBack?: () => void;
  onClose: (reason: MenuCloseReason) => void;
}

function MenuLayer({ depth, anchor, label, id, iconRow, entries, path, setPath, focusOnOpen, onBack, onClose }: MenuLayerProps) {
  const menu = useRef<HTMLDivElement>(null);
  const baseId = useId();
  const [placed, setPlaced] = useState(false);
  /** Typed letters within a short time form one search, as in the sidebar's trees. */
  const typed = useRef({ text: "", at: -Infinity });
  /** Submenu opened from the keyboard: its first item takes the focus. */
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const open = path[depth];
  const openEntry = open ? entries.find((entry): entry is SubmenuEntry => entry.kind === "submenu" && entry.id === open.id) : undefined;
  const anchorKey = anchor.kind === "point" ? `${String(anchor.x)},${String(anchor.y)}` : `${String(anchor.left)},${String(anchor.right)},${String(anchor.top)}`;

  const place = useEffectEvent(() => {
    const element = menu.current;
    if (!element) return;
    const box = element.getBoundingClientRect();
    const view = { width: window.innerWidth, height: window.innerHeight };
    const { x, y } = placeMenu(anchor, { width: box.width, height: Math.min(box.height, view.height - 2 * MENU_MARGIN) }, view);
    element.style.setProperty("--menu-x", `${String(x)}px`);
    element.style.setProperty("--menu-y", `${String(y)}px`);
    element.style.setProperty("--menu-max-height", `${String(Math.max(0, view.height - 2 * MENU_MARGIN))}px`);
    setPlaced(true);
  });
  // The anchor's numbers are its identity: a new object for the same place moves nothing.
  useLayoutEffect(() => {
    place();
  }, [anchorKey]);

  useEffect(() => {
    if (placed && focusOnOpen) focusables(menu.current)[0]?.focus({ preventScroll: true });
  }, [placed, focusOnOpen]);

  const subId = (entry: SubmenuEntry) => `${baseId}-${entry.id}`;

  /** Opens the submenu of `entry` beside its item `element`; from the keyboard onto its first item. */
  const openSubmenu = (entry: SubmenuEntry, element: HTMLElement, keyboard: boolean) => {
    const box = element.getBoundingClientRect();
    const parent = menu.current?.getBoundingClientRect();
    setKeyboardOpen(keyboard);
    setPath((current) => {
      if (current[depth]?.id === entry.id && !keyboard) return current;
      return [...current.slice(0, depth), { id: entry.id, anchor: { kind: "side", left: parent?.left ?? box.left, right: parent?.right ?? box.right, top: box.top - 4 } }];
    });
  };
  const closeSubmenus = () => {
    setPath((current) => (current.length > depth ? current.slice(0, depth) : current));
  };

  /** Closes the menu (the focus goes back first), then runs the item; a disabled one does nothing. */
  const choose = (entry: MenuItemEntry) => {
    if (entry.disabled) return;
    onClose("select");
    entry.onSelect();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = focusables(menu.current);
    const current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const index = current ? items.indexOf(current) : -1;
    const inRow = (element: HTMLElement | undefined) => element?.dataset["menuRow"] === "icons";
    const handled = () => {
      event.preventDefault();
      event.stopPropagation();
    };
    const focusAt = (next: number) => {
      items[(next + items.length) % items.length]?.focus({ preventScroll: true });
    };
    // The icon row is one stop for ↑ and ↓: the first icon stands for it.
    const rowStops = items.filter((item, i) => !inRow(item) || i === items.findIndex((other) => inRow(other)));
    const stopOf = (element: HTMLElement | null) => (element && inRow(element) ? items.find((item) => inRow(item)) : element);
    const stepRows = (direction: 1 | -1) => {
      const at = rowStops.indexOf(stopOf(current) ?? items[0] ?? document.body);
      const next = rowStops[(at + direction + rowStops.length) % rowStops.length];
      next?.focus({ preventScroll: true });
    };
    switch (event.key) {
      case "ArrowDown":
        handled();
        if (index === -1) focusAt(0);
        else stepRows(1);
        return;
      case "ArrowUp":
        handled();
        if (index === -1) focusAt(items.length - 1);
        else stepRows(-1);
        return;
      case "Home":
        handled();
        focusAt(0);
        return;
      case "End":
        handled();
        focusAt(items.length - 1);
        return;
      case "ArrowRight": {
        handled();
        if (current && inRow(current)) {
          const next = items[index + 1];
          if (inRow(next)) next?.focus({ preventScroll: true });
          return;
        }
        const entry = entries.find((candidate) => candidate.kind === "submenu" && candidate.id === current?.dataset["menuEntry"]);
        if (entry?.kind === "submenu" && current) openSubmenu(entry, current, true);
        return;
      }
      case "ArrowLeft":
        handled();
        if (current && inRow(current)) {
          const previous = items[index - 1];
          if (inRow(previous)) previous?.focus({ preventScroll: true });
          return;
        }
        onBack?.();
        return;
      case "Escape":
        handled();
        if (onBack) onBack();
        else onClose("escape");
        return;
      case "Tab":
        handled();
        onClose("tab");
        return;
      case "Enter":
      case " ": {
        handled();
        const id = current?.dataset["menuEntry"];
        const entry = [...(iconRow?.items ?? []), ...entries].find((candidate) => candidate.kind !== "separator" && candidate.kind !== "heading" && candidate.id === id);
        if (!entry || !current) return;
        if (entry.kind === "submenu") openSubmenu(entry, current, true);
        else if (entry.kind === "item") choose(entry);
        return;
      }
      default:
    }
    // Type-ahead: the next item whose label starts with what was typed.
    if (event.key.length === 1 && /\p{L}|\p{N}/u.test(event.key) && !event.altKey && !event.ctrlKey && !event.metaKey) {
      handled();
      const now = event.timeStamp;
      const text = now - typed.current.at < 500 ? typed.current.text + event.key : event.key;
      typed.current = { text, at: now };
      const labels = items.map((item) => item.dataset["menuLabel"] ?? "");
      const starts = (i: number) => labels[i]?.slice(0, text.length).localeCompare(text, undefined, { sensitivity: "base" }) === 0;
      const order = [...items.keys()].map((offset) => (index + (text.length > 1 ? 0 : 1) + offset) % items.length);
      const found = order.find((i) => starts(i));
      if (found !== undefined) focusAt(found);
    }
  };

  const itemClass = (entry: { disabled?: boolean | undefined }) =>
    cx(
      "flex h-8 w-full shrink-0 items-center gap-3 rounded-control px-2 text-start text-13 outline-none transition-colors",
      "focus:bg-hover focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus-ring",
      entry.disabled ? "cursor-default text-secondary" : "text-primary",
    );

  const renderEntry = (entry: MenuEntry): ReactNode => {
    switch (entry.kind) {
      case "separator":
        return <div key={entry.id} role="separator" className="mx-2 my-1 h-px shrink-0 bg-border-subtle" />;
      case "heading":
        return (
          <div key={entry.id} role="none" className="truncate px-2 pt-2 pb-1 text-12 font-semibold text-tertiary">
            {entry.label}
          </div>
        );
      case "submenu": {
        const Icon = entry.icon;
        const expanded = open?.id === entry.id;
        return (
          <button
            key={entry.id}
            type="button"
            role="menuitem"
            tabIndex={-1}
            aria-haspopup="menu"
            aria-expanded={expanded}
            aria-controls={expanded ? subId(entry) : undefined}
            data-menu-item
            data-menu-entry={entry.id}
            data-menu-label={entry.label}
            data-testid={entry.testId}
            onPointerEnter={(event) => {
              event.currentTarget.focus({ preventScroll: true });
              openSubmenu(entry, event.currentTarget, false);
            }}
            onClick={(event) => {
              openSubmenu(entry, event.currentTarget, event.detail === 0);
            }}
            className={cx(itemClass({}), expanded && "bg-hover")}
          >
            {Icon ? <Icon aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} /> : <span aria-hidden className="w-(--ui-icon-small) shrink-0" />}
            <span className="min-w-0 flex-1 truncate">{entry.label}</span>
            <ChevronRight aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary rtl:rotate-180" strokeWidth={1.75} />
          </button>
        );
      }
      case "item": {
        const Icon = entry.checked !== undefined ? (entry.checked ? Check : undefined) : entry.icon;
        return (
          <button
            key={entry.id}
            type="button"
            role={entry.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
            aria-checked={entry.checked}
            tabIndex={-1}
            aria-disabled={entry.disabled || undefined}
            aria-describedby={entry.describedBy}
            aria-keyshortcuts={entry.disabled ? undefined : entry.keyshortcuts}
            title={entry.title}
            data-menu-item
            data-menu-entry={entry.id}
            data-menu-label={entry.label}
            data-testid={entry.testId}
            onPointerEnter={(event) => {
              event.currentTarget.focus({ preventScroll: true });
              closeSubmenus();
            }}
            onClick={() => {
              choose(entry);
            }}
            className={cx(itemClass(entry), entry.note && "h-auto min-h-8 items-start py-1.5", INDENT[Math.min(entry.indent ?? 0, INDENT.length - 1)])}
          >
            {Icon ? (
              <Icon
                aria-hidden
                className={cx(
                  "size-(--ui-icon-small) shrink-0",
                  entry.note && "mt-0.5",
                  entry.checked ? "text-accent" : entry.disabled ? "opacity-50" : "text-secondary",
                )}
                strokeWidth={entry.checked ? 2.25 : 1.75}
              />
            ) : (
              <span aria-hidden className="w-(--ui-icon-small) shrink-0" />
            )}
            {entry.note ? (
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span id={entry.note.id} aria-hidden className="text-12 text-secondary">
                  {entry.note.text}
                </span>
                <span className={cx("font-medium", entry.accent && "text-accent")}>{entry.label}</span>
              </span>
            ) : (
              <span className={cx("min-w-0 flex-1 truncate", entry.accent && "font-medium text-accent")}>{entry.label}</span>
            )}
            {entry.detail && (
              <span aria-hidden className="shrink-0 text-12 text-tertiary">
                {entry.detail}
              </span>
            )}
          </button>
        );
      }
    }
  };

  return (
    <>
      <div
        ref={menu}
        id={id}
        role="menu"
        aria-label={label}
        aria-orientation="vertical"
        onKeyDown={onKeyDown}
        onContextMenu={(event) => {
          // Inside the menu the system's own menu never shows.
          event.preventDefault();
        }}
        data-menu-depth={depth}
        className={cx(
          "fixed top-(--menu-y) left-(--menu-x) z-50 flex max-h-(--menu-max-height) min-w-(--ui-context-menu-width) max-w-(--ui-context-menu-max-width) flex-col overflow-y-auto rounded-card border border-border bg-surface-raised p-1 shadow-floating",
          !placed && "invisible",
        )}
      >
        {iconRow && iconRow.items.length > 0 && (
          <div role="group" aria-label={iconRow.label} className="flex shrink-0 items-center gap-1 border-b border-border-subtle px-1 pb-1 mb-1">
            {iconRow.items.map((entry) => {
              const Icon = entry.icon;
              return (
                <button
                  key={entry.id}
                  type="button"
                  role="menuitem"
                  tabIndex={-1}
                  aria-label={entry.label}
                  aria-disabled={entry.disabled || undefined}
                  aria-describedby={entry.describedBy}
                  aria-keyshortcuts={entry.disabled ? undefined : entry.keyshortcuts}
                  title={entry.title ?? entry.label}
                  data-menu-item
                  data-menu-row="icons"
                  data-menu-entry={entry.id}
                  data-menu-label={entry.label}
                  data-testid={entry.testId}
                  onPointerEnter={(event) => {
                    event.currentTarget.focus({ preventScroll: true });
                    closeSubmenus();
                  }}
                  onClick={() => {
                    choose(entry);
                  }}
                  className={cx(
                    "flex size-8 shrink-0 items-center justify-center rounded-control outline-none transition-colors",
                    "focus:bg-hover focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus-ring",
                    entry.disabled ? "cursor-default text-secondary" : "text-secondary hover:text-primary focus:text-primary",
                  )}
                >
                  {Icon && <Icon aria-hidden className={cx("size-(--ui-icon)", entry.disabled && "opacity-50")} strokeWidth={1.75} />}
                </button>
              );
            })}
          </div>
        )}
        {entries.map(renderEntry)}
      </div>
      {openEntry && open && (
        <MenuLayer
          key={openEntry.id}
          depth={depth + 1}
          id={subId(openEntry)}
          anchor={open.anchor}
          label={openEntry.label}
          entries={openEntry.entries}
          path={path}
          setPath={setPath}
          focusOnOpen={keyboardOpen}
          onBack={() => {
            closeSubmenus();
            focusables(menu.current)
              .find((item) => item.dataset["menuEntry"] === openEntry.id)
              ?.focus({ preventScroll: true });
          }}
          onClose={onClose}
        />
      )}
    </>
  );
}
