import { Check, type LucideIcon } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { useMenuPlacement } from "../actions/useMenuPlacement";

/** One entry of a composer menu: a command, one choice of a group, or an option that is on or off. */
export type MenuEntry =
  | {
      kind: "action";
      id: string;
      label: string;
      icon?: LucideIcon | undefined;
      /** Shown at the end, e.g. a shortcut or a time; not part of the name. */
      detail?: string | undefined;
      /** `aria-keyshortcuts` ("Meta+Shift+Enter"). */
      keyshortcuts?: string | undefined;
      /** Shown, but does nothing now (no list to indent); says so to screen readers. */
      disabled?: boolean | undefined;
      onSelect: () => void;
      "data-testid"?: string | undefined;
    }
  | {
      kind: "radio" | "checkbox";
      id: string;
      label: string;
      checked: boolean;
      /** A second line under the name, also its description. */
      hint?: string | undefined;
      disabled?: boolean | undefined;
      /**
       * An option closes the menu too (a format of the text: the editor takes the focus back);
       * otherwise options keep it open for the next one.
       */
      closes?: boolean | undefined;
      onSelect: () => void;
      "data-testid"?: string | undefined;
    };

/** A group of entries, with a small label above it when it has one, and a line before the next. */
export interface MenuSection {
  id: string;
  label?: string | undefined;
  entries: readonly MenuEntry[];
}

interface ComposerMenuProps {
  /** The menu's name; the button's name and tooltip unless `name` says more. */
  label: string;
  /** The button's accessible name, where it says more than `label` (a reminder that is set). */
  name?: string | undefined;
  /** The entries, built when the menu opens (`openedAt`: the clock at the click, for times). */
  sections: readonly MenuSection[] | ((openedAt: number) => readonly MenuSection[]);
  /** The button's content: an icon, or a chip with text. */
  children: ReactNode;
  /** Classes of the button; `open` while its menu shows. */
  className: (open: boolean) => string;
  /** A line of text under the entries (a rule the choices follow); it also describes the menu. */
  note?: string | undefined;
  disabled?: boolean | undefined;
  /** Tooltip off where the button shows its text anyway. */
  tooltip?: boolean | undefined;
  align?: "start" | "end";
  /**
   * After a choice the focus goes back to the button (default), or stays where the choice put it
   * (the formatting menus: the editor takes it, to write on).
   */
  refocus?: boolean | undefined;
  /** Pressed look while an option of the menu is on (a reminder is set). */
  "data-active"?: boolean | undefined;
  "data-testid"?: string | undefined;
}

const ITEMS = '[role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"]';

/**
 * Stage 23 (decision 0086): the composer's menus (identity, paragraph style, more formatting,
 * "Send later", the reminder and "…"). A WAI-ARIA menu button like `OverflowMenu` and
 * `ChoiceMenu`, but with commands, choices and on/off options in one menu, and with a button the
 * composer draws itself (a chip, an icon, the arrow of the send button). Inline, not a popover
 * library (CSP). Enter, Space or ↓ open it on the checked choice (else the first entry), ↑ on the
 * last; arrows, Home and End move, Escape closes it and gives the focus back to the button (it
 * never reaches the composer, which would close), Tab closes it, a click outside too. A command
 * and a choice close the menu; an option stays open for the next one. It opens above its button
 * when there is no room below (`useMenuPlacement`): the menus at the foot of the composer do.
 */
export function ComposerMenu({
  label,
  name,
  sections,
  children,
  className,
  note,
  disabled = false,
  tooltip = true,
  align = "start",
  refocus = true,
  ...rest
}: ComposerMenuProps) {
  const [openedAt, setOpenedAt] = useState<number | null>(null);
  const open = openedAt !== null;
  const menuId = useId();
  const noteId = useId();
  const hintIds = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const focusOn = useRef<"checked" | "last" | null>(null);
  useMenuPlacement(open, wrapper, menu);

  useEffect(() => {
    if (!open) return undefined;
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapper.current?.contains(event.target as Node)) setOpenedAt(null);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  // Placed already (a layout effect before this one); the focus moves before the next key.
  useLayoutEffect(() => {
    if (!open || !focusOn.current) return;
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>(ITEMS) ?? [])];
    const target =
      focusOn.current === "last" ? items.at(-1) : (items.find((item) => item.getAttribute("aria-checked") === "true" && item.getAttribute("role") === "menuitemradio") ?? items[0]);
    target?.focus({ preventScroll: true });
    focusOn.current = null;
  }, [open]);

  const show = (at: "checked" | "last") => {
    focusOn.current = at;
    setOpenedAt(Date.now());
  };
  const close = (refocus: boolean) => {
    setOpenedAt(null);
    if (refocus) button.current?.focus();
  };

  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    // The menu's Escape, not the composer's (which would close the composer).
    if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      show(event.key === "ArrowDown" ? "checked" : "last");
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>(ITEMS) ?? [])];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const move = (next: number) => {
      event.preventDefault();
      event.stopPropagation();
      items[(next + items.length) % items.length]?.focus({ preventScroll: true });
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
        close(true);
        break;
      case "Tab":
        setOpenedAt(null);
        break;
      default:
    }
  };

  const list = openedAt === null ? [] : typeof sections === "function" ? sections(openedAt) : sections;
  const ownName = name ?? label;
  return (
    <div ref={wrapper} className="relative flex">
      <Tooltip label={ownName} disabled={open || !tooltip}>
        <button
          ref={button}
          type="button"
          aria-label={ownName}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? menuId : undefined}
          disabled={disabled}
          onClick={() => {
            if (open) close(false);
            else show("checked");
          }}
          onKeyDown={onButtonKeyDown}
          data-active={rest["data-active"] || undefined}
          data-testid={rest["data-testid"]}
          className={className(open)}
        >
          {children}
        </button>
      </Tooltip>
      {open && (
        <div
          ref={menu}
          id={menuId}
          role="menu"
          aria-label={label}
          aria-describedby={note ? noteId : undefined}
          onKeyDown={onMenuKeyDown}
          className={cx(
            "absolute z-30 flex max-h-(--ui-picker-list-height) w-max max-w-[min(var(--ui-composer-menu-max-width),calc(100vw-var(--ui-space-4)))] min-w-(--ui-reading-menu-width) flex-col overflow-y-auto rounded-card border border-border bg-surface-raised p-1 shadow-floating",
            "top-full mt-1 data-[side=above]:top-auto data-[side=above]:bottom-full data-[side=above]:mt-0 data-[side=above]:mb-1",
            align === "end" ? "end-0" : "start-0",
          )}
          data-testid={rest["data-testid"] && `${rest["data-testid"]}-menu`}
        >
          {list.map((section, sectionIndex) => {
            const labelId = `${menuId}-${section.id}`;
            return (
              <div
                key={section.id}
                role="group"
                aria-labelledby={section.label ? labelId : undefined}
                className={cx(sectionIndex > 0 && "mt-1 border-t border-border-subtle pt-1")}
              >
                {section.label && (
                  <div id={labelId} className="px-2 pt-1 pb-1 text-12 font-semibold text-tertiary">
                    {section.label}
                  </div>
                )}
                {section.entries.map((entry) => {
                  const hint = entry.kind !== "action" ? entry.hint : undefined;
                  const hintId = hint ? `${hintIds}-${section.id}-${entry.id}` : undefined;
                  const Icon = entry.kind === "action" ? entry.icon : undefined;
                  return (
                    <button
                      key={entry.id}
                      type="button"
                      role={entry.kind === "action" ? "menuitem" : entry.kind === "radio" ? "menuitemradio" : "menuitemcheckbox"}
                      aria-checked={entry.kind === "action" ? undefined : entry.checked}
                      aria-disabled={entry.disabled || undefined}
                      aria-describedby={hintId}
                      aria-keyshortcuts={entry.kind === "action" ? entry.keyshortcuts : undefined}
                      tabIndex={-1}
                      onClick={() => {
                        if (entry.disabled) return;
                        if (entry.kind !== "checkbox" || entry.closes) close(refocus);
                        entry.onSelect();
                      }}
                      data-testid={entry["data-testid"]}
                      className={cx(
                        "flex w-full shrink-0 gap-2 rounded-control px-2 text-start text-13 transition-colors hover:bg-hover focus-visible:bg-hover",
                        // Long names wrap instead of being cut off (German names are long).
                        "min-h-8 py-1",
                        hint ? "items-start" : "items-center",
                        entry.disabled ? "text-secondary" : "text-primary",
                      )}
                    >
                      <span aria-hidden className="flex h-6 w-(--ui-icon-small) shrink-0 items-center justify-center">
                        {entry.kind === "action"
                          ? Icon && <Icon className={cx("size-(--ui-icon-small)", entry.disabled ? "opacity-50" : "text-secondary")} strokeWidth={1.75} />
                          : entry.checked && <Check className={cx("size-(--ui-icon-small)", !entry.disabled && "text-accent")} strokeWidth={2} />}
                      </span>
                      {hint ? (
                        <span className="flex min-w-0 flex-1 flex-col">
                          <span>{entry.label}</span>
                          <span id={hintId} className="text-12 text-secondary">
                            {hint}
                          </span>
                        </span>
                      ) : (
                        <span className="min-w-0 flex-1">{entry.label}</span>
                      )}
                      {entry.kind === "action" && entry.detail && (
                        <span aria-hidden className="shrink-0 text-12 text-tertiary tabular-nums">
                          {entry.detail}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            );
          })}
          {note && (
            <p id={noteId} className="mt-1 border-t border-border-subtle px-2 pt-2 pb-1 text-12 text-secondary" data-testid={rest["data-testid"] && `${rest["data-testid"]}-note`}>
              {note}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
