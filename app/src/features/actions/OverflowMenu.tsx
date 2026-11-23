import { MoreHorizontal, type LucideIcon } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";

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
  "data-testid"?: string;
}

/**
 * A "…" button that unfolds a menu below it (WAI-ARIA menu button). Inline, not a Radix popover:
 * the CSP rules those out (see InboxScopeSwitcher). Enter, Space or Arrow Down open it on the first
 * item, Arrow Up on the last; arrows, Home and End move, Escape and Tab close it, a click outside
 * too. Choosing an item closes the menu and gives the focus back to the button first.
 */
export function OverflowMenu({ label, items, tabIndex, onFocus, align = "end", returnFocus, ...rest }: OverflowMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [focusOn, setFocusOn] = useState<"first" | "last" | null>(null);

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
    (focusOn === "first" ? buttons[0] : buttons[buttons.length - 1])?.focus();
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
      buttons[(next + buttons.length) % buttons.length]?.focus();
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
    <div ref={wrapper} className="relative flex">
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
            "flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary",
            open && "bg-hover text-primary",
          )}
        >
          <MoreHorizontal aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
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
            "absolute top-full z-30 mt-1 flex max-h-(--ui-picker-list-height) min-w-(--ui-reading-menu-width) flex-col overflow-y-auto rounded-card border border-border bg-surface-raised p-1 shadow-floating",
            align === "end" ? "end-0" : "start-0",
          )}
          data-testid="overflow-menu"
        >
          {items.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                type="button"
                role="menuitem"
                tabIndex={-1}
                aria-label={item.label}
                aria-disabled={item.inert || undefined}
                aria-keyshortcuts={item.inert ? undefined : item.keyshortcuts}
                onClick={() => {
                  close(true);
                  item.onSelect();
                }}
                data-testid={item["data-testid"]}
                className={cx(
                  "flex h-8 w-full shrink-0 items-center gap-3 rounded-control px-2 text-start text-13 text-primary transition-colors hover:bg-hover focus-visible:bg-hover",
                  item.inert && "text-secondary",
                )}
              >
                <Icon aria-hidden className={cx("size-(--ui-icon-small) shrink-0", item.inert ? "opacity-50" : "text-secondary")} strokeWidth={1.75} />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
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
