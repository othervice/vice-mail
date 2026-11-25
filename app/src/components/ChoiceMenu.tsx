import { Check, type LucideIcon } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "./cx";
import { Tooltip } from "./Tooltip";

/** One choice of a menu: a radio item of its group, or a checkbox item. */
export interface ChoiceItem {
  id: string;
  label: string;
  checked: boolean;
  /** Shown, but says it does nothing right now (the reason is in its label or the group's). */
  inert?: boolean;
  onSelect: () => void;
  "data-testid"?: string;
}

/** A group of choices: one of them (`radio`) or any of them (`checkbox`). */
export interface ChoiceGroup {
  id: string;
  /** The group's name, shown above it and naming it for screen readers. */
  label: string;
  kind: "radio" | "checkbox";
  items: readonly ChoiceItem[];
}

interface ChoiceMenuProps {
  /** The menu's name, and the button's name and tooltip unless `name` or `text` say more. */
  label: string;
  /** The button's accessible name where its visible text alone says too little (it contains it). */
  name?: string;
  groups: readonly ChoiceGroup[];
  /** The button's content: an icon, and a short text after it if any. */
  icon: LucideIcon;
  text?: ReactNode;
  /** Pressed look while the menu shows a choice other than the default (a filter is on). */
  active?: boolean;
  align?: "start" | "end";
  /** Radio items close the menu when chosen; checkboxes keep it open for more. */
  "data-testid"?: string;
  /** Roving focus of a surrounding toolbar. */
  tabIndex?: number;
  onFocus?: () => void;
  "data-roving-key"?: string;
  /** The button is a chip of the quick filter (rounded, with a hairline). */
  chip?: boolean;
  /** A menu of several groups may take most of the window's height before it scrolls. */
  tall?: boolean;
}

const ITEM_ROLES = '[role="menuitemradio"], [role="menuitemcheckbox"]';

/**
 * A button that unfolds a menu of choices (WAI-ARIA menu button with `menuitemradio` and
 * `menuitemcheckbox` items in labelled groups), inline like `OverflowMenu` (the CSP rules out
 * popover libraries). Enter, Space or ↓ open it on the checked item (else the first), ↑ on the
 * last; arrows, Home and End move, Escape and Tab close it, a click outside too. A radio item
 * closes the menu and gives the focus back to the button; a checkbox item stays open.
 */
export function ChoiceMenu({ label, name, groups, icon: Icon, text, active = false, align = "end", tabIndex, onFocus, chip = false, tall = false, ...rest }: ChoiceMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  /** Where the focus goes once the menu has opened. */
  const focusOn = useRef<"checked" | "last" | null>(null);

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
    if (!open || !focusOn.current) return;
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>(ITEM_ROLES) ?? [])];
    const target = focusOn.current === "last" ? items.at(-1) : (items.find((item) => item.getAttribute("aria-checked") === "true") ?? items[0]);
    target?.focus();
    focusOn.current = null;
  }, [open]);

  const close = () => {
    setOpen(false);
    button.current?.focus();
  };

  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      focusOn.current = event.key === "ArrowDown" ? "checked" : "last";
      setOpen(true);
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>(ITEM_ROLES) ?? [])];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const move = (next: number) => {
      event.preventDefault();
      event.stopPropagation();
      items[(next + items.length) % items.length]?.focus();
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
        move(items.length - 1);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        close();
        break;
      case "Tab":
        setOpen(false);
        break;
      default:
    }
  };

  return (
    <div ref={wrapper} className="relative flex">
      <Tooltip label={label} disabled={open || text !== undefined}>
        <button
          ref={button}
          type="button"
          aria-label={name ?? (text === undefined ? label : undefined)}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? menuId : undefined}
          tabIndex={tabIndex}
          onFocus={onFocus}
          data-roving-key={rest["data-roving-key"]}
          onClick={() => {
            focusOn.current = "checked";
            setOpen((value) => !value);
          }}
          onKeyDown={onButtonKeyDown}
          data-testid={rest["data-testid"]}
          className={cx(
            "flex items-center justify-center gap-1 transition-colors",
            chip ? "h-8 rounded-full border px-3 text-13" : "h-8 rounded-control px-2 text-13",
            chip
              ? active
                ? "border-accent bg-accent-soft font-medium text-accent"
                : "border-border text-secondary hover:bg-hover hover:text-primary"
              : cx("text-secondary hover:bg-hover hover:text-primary", (open || active) && "bg-hover text-primary"),
            text === undefined && !chip && "w-8 px-0",
          )}
        >
          <Icon aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
          {text !== undefined && <span className="max-w-(--ui-row-account-width) truncate">{text}</span>}
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
            "absolute top-full z-30 mt-1 flex min-w-(--ui-reading-menu-width) flex-col overflow-y-auto rounded-card border border-border bg-surface-raised p-1 shadow-floating",
            tall ? "max-h-[calc(100vh-var(--ui-space-12)*2)]" : "max-h-(--ui-picker-list-height)",
            align === "end" ? "end-0" : "start-0",
          )}
          data-testid={rest["data-testid"] && `${rest["data-testid"]}-menu`}
        >
          {groups.map((group, groupIndex) => {
            const labelId = `${menuId}-${group.id}`;
            return (
              <div key={group.id} role="group" aria-labelledby={labelId} className={cx(groupIndex > 0 && "mt-1 border-t border-border pt-1")}>
                <div id={labelId} className="px-2 pt-1 text-12 font-semibold text-tertiary">
                  {group.label}
                </div>
                {group.items.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    role={group.kind === "radio" ? "menuitemradio" : "menuitemcheckbox"}
                    aria-checked={item.checked}
                    aria-disabled={item.inert || undefined}
                    tabIndex={-1}
                    onClick={() => {
                      if (item.inert) return;
                      if (group.kind === "radio") close();
                      item.onSelect();
                    }}
                    data-testid={item["data-testid"]}
                    className={cx(
                      "flex h-8 w-full shrink-0 items-center gap-2 rounded-control px-2 text-start text-13 transition-colors hover:bg-hover focus-visible:bg-hover",
                      item.inert ? "text-secondary" : "text-primary",
                    )}
                  >
                    <span aria-hidden className="flex size-(--ui-icon-small) shrink-0 items-center justify-center">
                      {item.checked && <Check className={cx("size-(--ui-icon-small)", !item.inert && "text-accent")} strokeWidth={2} />}
                    </span>
                    <span className="min-w-0 flex-1 truncate">{item.label}</span>
                  </button>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
