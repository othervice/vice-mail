import { MoreHorizontal, type LucideIcon } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { useMenuPlacement } from "./useMenuPlacement";

export interface OverflowItem {
  id: string;
  label: string;
  icon: LucideIcon;
  /** Shown at the end of the item, e.g. the shortcut ("⌘P", "E"); not part of its name. */
  detail?: string;
  /** The shortcut for assistive technology (`aria-keyshortcuts`, "Meta+P"). */
  keyshortcuts?: string;
  /** The item stays in the menu and says why it does nothing (read-only account). */
  inert?: boolean;
  /** Stage 21: a second line under the name, also its description (why it is unavailable). */
  hint?: string;
  onSelect: () => void;
  "data-testid"?: string;
}

interface OverflowMenuProps {
  /** The button's name and tooltip ("More actions"). */
  label: string;
  items: readonly OverflowItem[];
  /** Roving focus of a surrounding toolbar: its tab stop and the focus report. */
  tabIndex?: number;
  onFocus?: () => void;
  /** Where the menu unfolds, under the button: from its end (default) or from its start. */
  align?: "end" | "start";
  /**
   * Where the focus goes back to when the menu closes, instead of the button: the row of a tree
   * whose menu this is (the button itself is not in the Tab order there).
   */
  returnFocus?: () => void;
  /**
   * A menu for a row of the accounts-and-folders column (stage 21): as wide as the column allows
   * (at most 16 rem), never wider, with names that wrap instead of being cut off (it may be only
   * 200 px wide, and German names are long).
   */
  narrow?: boolean;
  /** Stage 17: a 24 px button for a 24 px row (a section label of the column). */
  small?: boolean;
  /**
   * Stage 23, step 3 (decision 0084): a button of the toolbar's joined groups (34 × 28, square
   * corners, the group draws the frame) or a plain 28 px button of the toolbar or a message's head.
   */
  look?: "joined" | "joinedAlone" | "toolbar";
  /** Classes of the wrapper, e.g. the line before a joined button. */
  className?: string;
  "data-testid"?: string;
}

/**
 * A "…" button that unfolds a menu below it (WAI-ARIA menu button). Inline, not a Radix popover:
 * the CSP rules those out (see InboxScopeSwitcher). Enter, Space or Arrow Down open it on the first
 * item, Arrow Up on the last; arrows, Home and End move, Escape and Tab close it, a click outside
 * too. Choosing an item closes the menu and gives the focus back to the button first. Stage 21: it
 * stays inside what can be seen (`useMenuPlacement`): above the button when there is no room
 * below, moved back in at the sides.
 */
export function OverflowMenu({
  label,
  items,
  tabIndex,
  onFocus,
  align = "end",
  returnFocus,
  narrow = false,
  small = false,
  look,
  className,
  ...rest
}: OverflowMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [focusOn, setFocusOn] = useState<"first" | "last" | null>(null);
  useMenuPlacement(open, wrapper, menu);
  const hintIds = useId();

  useEffect(() => {
    if (!open) return undefined;
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapper.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  useEffect(() => {
    if (!open || !focusOn) return;
    const buttons = menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
    if (!buttons || buttons.length === 0) return;
    // Placed already (a layout effect); focusing must not scroll a column sideways.
    (focusOn === "first" ? buttons[0] : buttons[buttons.length - 1])?.focus({ preventScroll: true });
    setFocusOn(null);
  }, [open, focusOn]);

  const close = (refocus: boolean) => {
    setOpen(false);
    if (!refocus) return;
    if (returnFocus) returnFocus();
    else button.current?.focus();
  };

  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      setOpen(true);
      setFocusOn(event.key === "ArrowDown" ? "first" : "last");
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const buttons = [...(menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const move = (next: number) => {
      event.preventDefault();
      event.stopPropagation();
      buttons[(next + buttons.length) % buttons.length]?.focus({ preventScroll: true });
    };
    switch (event.key) {
      case "ArrowDown":
        move(index + 1);
        break;
      case "ArrowUp":
        move(index - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(buttons.length - 1);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case "Tab":
        setOpen(false);
        break;
      default:
    }
  };

  if (items.length === 0) return null;
  return (
    <div ref={wrapper} className={cx("relative flex", className)}>
      <Tooltip label={label} disabled={open}>
        <button
          ref={button}
          type="button"
          aria-label={label}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? menuId : undefined}
          tabIndex={tabIndex}
          onFocus={onFocus}
          onClick={() => {
            setOpen((value) => !value);
            setFocusOn("first");
          }}
          onKeyDown={onButtonKeyDown}
          data-testid={rest["data-testid"]}
          className={cx(
            "flex items-center justify-center text-secondary transition-colors hover:bg-hover hover:text-primary",
            look === "joined" || look === "joinedAlone"
              ? cx("h-(--ui-toolbar-control) w-(--ui-toolbar-button)", look === "joined" ? "rounded-e-control" : "rounded-control")
              : look === "toolbar"
                ? "size-(--ui-toolbar-control) rounded-control"
                : cx("rounded-control", small ? "size-6" : "size-8"),
            open && "bg-hover text-primary",
          )}
        >
          <MoreHorizontal aria-hidden className={small || look ? "size-(--ui-icon-small)" : "size-(--ui-icon)"} strokeWidth={1.75} />
        </button>
      </Tooltip>
      {open && (
        <div
          ref={menu}
          id={menuId}
          role="menu"
          aria-label={label}
          onKeyDown={onMenuKeyDown}
          className={cx(
            "absolute z-30 flex flex-col overflow-y-auto rounded-card border border-border bg-surface-raised p-1 shadow-floating",
            // Below the button; above it when `useMenuPlacement` finds no room below.
            "top-full mt-1 data-[side=above]:top-auto data-[side=above]:bottom-full data-[side=above]:mt-0 data-[side=above]:mb-1",
            narrow
              ? // The column sets --ui-sidebar-width on itself: the menu never gets wider than its rows.
                // Wrapped names make it higher: all of it shows where there is room (else the
                // placement gives it the room there is, and it rolls).
                "max-h-[calc(100vh-var(--ui-space-8))] w-[min(var(--ui-sidebar-menu-width),calc(var(--ui-sidebar-width)-var(--ui-space-4)))]"
              : "max-h-(--ui-picker-list-height) max-w-[calc(100vw-var(--ui-space-4))] min-w-(--ui-reading-menu-width)",
            align === "end" ? "end-0" : "start-0",
          )}
          data-testid="overflow-menu"
        >
          {items.map((item, index) => {
            const Icon = item.icon;
            const hintId = item.hint ? `${hintIds}-${String(index)}` : undefined;
            return (
              <button
                key={item.id}
                type="button"
                role="menuitem"
                tabIndex={-1}
                aria-label={item.label}
                aria-describedby={hintId}
                aria-disabled={item.inert || undefined}
                aria-keyshortcuts={item.inert ? undefined : item.keyshortcuts}
                onClick={() => {
                  close(true);
                  item.onSelect();
                }}
                data-testid={item["data-testid"]}
                className={cx(
                  "flex w-full shrink-0 gap-2 rounded-control px-2 text-start text-13 text-primary transition-colors hover:bg-hover focus-visible:bg-hover",
                  // Wrapping names: as high as they need (32 px for one line, on the 4 px grid).
                  narrow || item.hint ? "min-h-8 items-center py-1" : "h-8 items-center",
                  item.inert && "text-secondary",
                )}
              >
                <Icon
                  aria-hidden
                  className={cx("size-(--ui-icon-small) shrink-0", item.inert ? "opacity-50" : "text-secondary")}
                  strokeWidth={1.75}
                />
                {narrow || item.hint ? (
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className={narrow ? "break-words" : "truncate"}>{item.label}</span>
                    {item.hint && (
                      <span id={hintId} className="text-12 text-secondary">
                        {item.hint}
                      </span>
                    )}
                  </span>
                ) : (
                  <span className="min-w-0 flex-1 truncate">{item.label}</span>
                )}
                {item.detail && (
                  <span aria-hidden className="shrink-0 text-12 text-tertiary">
                    {item.detail}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
