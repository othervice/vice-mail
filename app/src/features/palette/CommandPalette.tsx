import { Search } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { usePaletteStore } from "../../stores/palette";
import { ariaShortcut, formatShortcut, type ShortcutPlatform } from "../actions/shortcuts";
import { arrangeEntries } from "./arrange";
import { usePaletteEntries, type PaletteEntry } from "./entries";

interface CommandPaletteProps {
  platform: ShortcutPlatform;
}

/** Cmd/Ctrl+K (docs/stages/08): every command of the app, found by typing, run with Enter. */
export function CommandPalette({ platform }: CommandPaletteProps) {
  const open = usePaletteStore((state) => state.open);
  return open ? <PalettePanel platform={platform} /> : null;
}

/**
 * Back to where the focus was when an overlay opened, unless something else took it meanwhile.
 * If that element is gone (the command went to another view), the main area takes the focus, so
 * the next Tab continues there instead of at the top of the window.
 */
export function useFocusReturn(): void {
  const [opener] = useState(() => (document.activeElement instanceof HTMLElement ? document.activeElement : null));
  useEffect(
    () => () => {
      const active = document.activeElement;
      if (active && active !== document.body) return;
      if (opener?.isConnected && opener !== document.body) opener.focus({ preventScroll: true });
      else document.querySelector<HTMLElement>("main")?.focus({ preventScroll: true });
    },
    [opener],
  );
}

function PalettePanel({ platform }: CommandPaletteProps) {
  const { t } = useTranslation();
  const id = useId();
  const close = usePaletteStore((state) => state.closePalette);
  const recent = usePaletteStore((state) => state.recent);
  // Stage 8: the core refused the recent list (it is shown as it was before).
  const recentError = usePaletteStore((state) => state.saveError);
  const noteUsed = usePaletteStore((state) => state.noteUsed);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [refused, setRefused] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const entries = usePaletteEntries(platform, query);
  const sections = arrangeEntries(entries, query, recent, (group) => t(`palette.groups.${group}`));
  const flat = sections.flatMap((section) => section.entries);
  const current = flat.length === 0 ? -1 : Math.min(active, flat.length - 1);
  const activeEntry = current >= 0 ? flat[current] : undefined;
  const listId = `${id}-list`;
  const optionId = (entry: PaletteEntry) => `${id}-${entry.key}`;
  useFocusReturn();

  const choose = (entry: PaletteEntry) => {
    if (entry.disabled) {
      // Said where the eyes and the screen reader are: under the field.
      setRefused(t("palette.refused", { label: entry.label, reason: entry.disabled }));
      return;
    }
    noteUsed(entry.key);
    close();
    entry.run();
  };

  const moveTo = (index: number) => {
    setActive(index);
    setRefused(null);
    const entry = flat[index];
    if (entry) document.getElementById(optionId(entry))?.scrollIntoView?.({ block: "nearest" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // Enter that confirms an input method's text (Japanese, Chinese) is not a choice. WebKit
    // reports that Enter only as keyCode 229.
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    const last = flat.length - 1;
    const moves: Record<string, number> = {
      ArrowDown: current >= last ? 0 : current + 1,
      ArrowUp: current <= 0 ? last : current - 1,
      PageDown: Math.min(last, current + 8),
      PageUp: Math.max(0, current - 8),
    };
    const move = moves[event.key];
    if (move !== undefined && last >= 0) {
      event.preventDefault();
      moveTo(move);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (activeEntry) choose(activeEntry);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    // The field is the only stop: Tab does not leave the palette.
    if (event.key === "Tab") event.preventDefault();
  };

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !panelRef.current?.contains(event.target)) close();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [close]);

  const count = flat.filter((entry) => !entry.disabled && entry.group !== "search").length;

  return (
    <>
      {/* Design pass 23.11.2026: a dimmed backdrop (no motion); a click on it closes like outside. */}
      <div aria-hidden className="fixed inset-0 z-40 bg-scrim" data-testid="palette-backdrop" />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("palette.title")}
        data-testid="command-palette"
        // Keys stay in the palette: Cmd+F or Cmd+1 from its field must not act on the view behind it.
        onKeyDown={(event) => {
          event.stopPropagation();
        }}
        // A click outside the field keeps the focus in it, so typing and Escape still work.
        onMouseDown={(event) => {
          if (event.target !== event.currentTarget.querySelector("input")) event.preventDefault();
        }}
        className="fixed top-(--ui-palette-top) left-1/2 z-50 flex max-h-[calc(100%-2*var(--ui-palette-top))] w-(--ui-palette-width) max-w-[calc(100%-2rem)] -translate-x-1/2 flex-col overflow-hidden rounded-panel border border-border bg-surface-raised shadow-panel"
      >
        {/* The field as a row with a search symbol; the ring sits on the row while the field has the
            keyboard focus, inside the panel instead of along its clipped edge. */}
        <div className="shrink-0 border-b border-border-subtle p-2">
          <div className="flex h-10 items-center gap-2 rounded-tile bg-track px-3 has-[input:focus-visible]:outline-2 has-[input:focus-visible]:outline-focus-ring">
            <Search aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
            <input
              type="text"
              role="combobox"
              aria-expanded
              aria-controls={listId}
              aria-autocomplete="list"
              aria-activedescendant={activeEntry ? optionId(activeEntry) : undefined}
              aria-label={t("palette.field")}
              aria-describedby={`${id}-status`}
              placeholder={t("palette.placeholder")}
              value={query}
              // The palette opens for typing; the field is its whole point.
              autoFocus
              autoComplete="off"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              onChange={(event) => {
                setQuery(event.target.value);
                setActive(0);
                setRefused(null);
              }}
              onKeyDown={onKeyDown}
              className="h-full min-w-0 flex-1 bg-transparent text-16 text-primary outline-none select-text placeholder:text-tertiary"
            />
          </div>
        </div>
        <p id={`${id}-status`} aria-live="polite" className={cx("px-4 text-12", refused ? "py-2 text-danger" : "sr-only")}>
          {refused ?? (query.trim() ? t("palette.results", { count }) : "")}
        </p>
        {recentError && (
          <div className="shrink-0 border-b border-border p-2">
            <ErrorNotice context={t("palette.recentFailed")} error={recentError} data-testid="palette-recent-error" />
          </div>
        )}
        <div id={listId} role="listbox" aria-label={t("palette.list")} className="min-h-0 flex-1 overflow-y-auto p-2">
          {flat.length === 0 && <p className="px-3 py-3 text-13 text-secondary">{t("palette.empty")}</p>}
          {sections.map((section) => (
            <div key={section.id} role="group" aria-labelledby={`${id}-group-${section.id}`} className="flex flex-col">
              <div
                id={`${id}-group-${section.id}`}
                role="presentation"
                className="px-3 pt-2 pb-1 text-12 font-medium text-secondary"
              >
                {t(`palette.groups.${section.id}`)}
              </div>
              {section.entries.map((entry) => {
                const index = flat.indexOf(entry);
                const Icon = entry.icon;
                const selected = index === current;
                return (
                  <div
                    key={entry.key}
                    id={optionId(entry)}
                    role="option"
                    aria-selected={selected}
                    aria-disabled={entry.disabled ? true : undefined}
                    aria-keyshortcuts={entry.shortcut ? ariaShortcut(entry.shortcut, platform) : undefined}
                    data-key={entry.key}
                    // Keeps the focus in the field.
                    onMouseDown={(event) => {
                      event.preventDefault();
                    }}
                    onClick={() => {
                      choose(entry);
                    }}
                    onMouseMove={() => {
                      if (!selected) setActive(index);
                    }}
                    className={cx(
                      "flex min-h-10 items-center gap-2 rounded-tile px-3 py-1 text-14",
                      selected ? "bg-selected" : "",
                      entry.disabled ? "text-tertiary" : "text-primary",
                    )}
                  >
                    <Icon aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
                    <span className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate">{entry.label}</span>
                      {(entry.disabled ?? entry.detail) && (
                        <span className="truncate text-12 text-secondary">{entry.disabled ?? entry.detail}</span>
                      )}
                    </span>
                    {entry.shortcut && (
                      <kbd aria-hidden className="shrink-0 rounded-control bg-badge px-2 font-sans text-12 text-secondary tabular-nums">
                        {formatShortcut(entry.shortcut, platform)}
                      </kbd>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
