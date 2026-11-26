import { Search, X } from "lucide-react";
import { useEffect, useId, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../../components/cx";
import { HighlightedText } from "../../../components/HighlightedText";
import { resolveIndex, searchSettings, type ResolvedEntry } from "./searchSettings";

export interface SettingsSearchHandle {
  /** Cmd/Ctrl+F in the settings: the focus into the field, its text selected. */
  focus: () => void;
}

interface SettingsSearchProps {
  /** A hit was chosen: open its section and go to the row. */
  onChoose: (entry: ResolvedEntry) => void;
  ref?: Ref<SettingsSearchHandle>;
}

/**
 * Stage 22, item 8 (decision 0073): the search field at the top of the settings' section list.
 * A combobox with a list of hits under it, grouped by section (ARIA combobox and listbox with
 * groups): the arrow keys move through the hits while the focus stays in the field, Enter or a
 * click goes to the row, Escape clears the field, Tab leaves it. A polite status says how many
 * settings were found. The list floats over the page, as wide as a hit needs, never wider than
 * the window.
 */
export function SettingsSearch({ onChoose, ref }: SettingsSearchProps) {
  const { t, i18n } = useTranslation();
  const id = useId();
  const listId = `${id}-list`;
  const statusId = `${id}-status`;
  const input = useRef<HTMLInputElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  // The texts of the current language; the index is small (about 130 rows).
  const language = i18n.language;
  const index = useMemo(() => resolveIndex(i18n.getFixedT(language)), [i18n, language]);
  const groups = useMemo(() => searchSettings(index, query), [index, query]);
  const flat = useMemo(() => groups.flatMap((group) => group.hits), [groups]);
  const typed = query.trim() !== "";
  const shown = open && typed;
  const current = flat.length > 0 ? Math.min(active, flat.length - 1) : -1;
  const optionId = (entry: ResolvedEntry) => `${id}-option-${entry.id}`;
  const activeHit = current >= 0 ? flat[current] : undefined;

  useImperativeHandle(ref, () => ({
    focus: () => {
      input.current?.focus();
      input.current?.select();
      setOpen(true);
    },
  }));

  // A click outside closes the list (the field keeps its text).
  useEffect(() => {
    if (!shown) return;
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [shown]);

  // The active hit stays in view while the arrow keys move through a long list.
  useEffect(() => {
    if (!shown || !activeHit) return;
    document.getElementById(optionId(activeHit.entry))?.scrollIntoView?.({ block: "nearest" });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the id follows the hit
  }, [shown, activeHit]);

  const choose = (entry: ResolvedEntry) => {
    setOpen(false);
    onChoose(entry);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        if (!typed) return;
        setOpen(true);
        if (flat.length === 0) return;
        const step = event.key === "ArrowDown" ? 1 : -1;
        setActive(shown ? (current + step + flat.length) % flat.length : 0);
        return;
      }
      case "Enter":
        if (shown && activeHit) {
          event.preventDefault();
          choose(activeHit.entry);
        }
        return;
      case "Escape":
        // Clears the field (and closes the list); nothing behind it reacts to this Escape.
        if (typed || open) {
          event.preventDefault();
          event.stopPropagation();
          setQuery("");
          setOpen(false);
          setActive(0);
        }
        return;
      case "Tab":
        setOpen(false);
        return;
    }
  };

  const count = flat.length;
  return (
    <div ref={root} className="relative" data-testid="settings-search">
      <div className="flex h-8 items-center gap-2 rounded-control border border-border bg-surface-raised ps-2 pe-1 has-[input:focus-visible]:outline-2 has-[input:focus-visible]:outline-focus-ring">
        <Search aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
        <input
          ref={input}
          type="text"
          role="combobox"
          aria-label={t("settings.search.label")}
          aria-expanded={shown}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={shown && activeHit ? optionId(activeHit.entry) : undefined}
          aria-describedby={statusId}
          placeholder={t("settings.search.placeholder")}
          value={query}
          autoComplete="off"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
            setOpen(true);
          }}
          onFocus={() => {
            setOpen(true);
          }}
          onKeyDown={onKeyDown}
          className="h-full min-w-0 flex-1 bg-transparent text-13 text-primary outline-none select-text placeholder:text-tertiary"
          data-testid="settings-search-field"
        />
        {typed && (
          <button
            type="button"
            aria-label={t("settings.search.clear")}
            // Keeps the focus in the field.
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              setQuery("");
              setActive(0);
              input.current?.focus();
            }}
            className="flex size-6 shrink-0 items-center justify-center rounded-control text-tertiary hover:bg-hover hover:text-primary"
            data-testid="settings-search-clear"
          >
            <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          </button>
        )}
      </div>
      <p id={statusId} role="status" className="sr-only" data-testid="settings-search-status">
        {typed ? (count > 0 ? t("settings.search.count", { count }) : t("settings.search.none", { query: query.trim() })) : ""}
      </p>
      {/* Always in the document, so `aria-controls` names an element; hidden while there is nothing to show. */}
      <div
        id={listId}
        role="listbox"
        aria-label={t("settings.search.results")}
        hidden={!shown || count === 0}
        className="absolute start-0 top-full z-30 mt-1 max-h-(--ui-settings-search-height) w-(--ui-settings-search-width) max-w-[calc(100vw-var(--ui-space-8))] overflow-y-auto rounded-card border border-border bg-surface-raised p-2 shadow-floating"
        data-testid="settings-search-results"
      >
        {shown &&
          groups.map((group) => (
            <div key={group.section} role="group" aria-labelledby={`${id}-group-${group.section}`} className="flex flex-col">
              <div id={`${id}-group-${group.section}`} role="presentation" className="px-2 pt-2 pb-1 text-12 font-medium text-secondary">
                {t(`settings.${group.section}.title`)}
              </div>
              {group.hits.map((hit) => {
                const selected = hit === activeHit;
                return (
                  <div
                    key={hit.entry.id}
                    id={optionId(hit.entry)}
                    role="option"
                    aria-selected={selected}
                    // Keeps the focus in the field.
                    onMouseDown={(event) => {
                      event.preventDefault();
                    }}
                    onClick={() => {
                      choose(hit.entry);
                    }}
                    onMouseMove={() => {
                      if (!selected) setActive(flat.indexOf(hit));
                    }}
                    className={cx("flex min-h-8 cursor-default flex-col justify-center rounded-row px-2 py-1", selected && "bg-selected")}
                    data-testid="settings-search-hit"
                    data-section={hit.entry.section}
                  >
                    <span className="truncate text-13 text-primary">
                      <HighlightedText text={hit.entry.title} ranges={hit.titleRanges} />
                    </span>
                    {(hit.entry.isSection || hit.entry.description) && (
                      <span className="line-clamp-2 text-12 text-secondary">
                        {hit.entry.isSection ? (
                          t("settings.search.section")
                        ) : (
                          <HighlightedText text={hit.entry.description ?? ""} ranges={hit.descriptionRanges} />
                        )}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
      </div>
    </div>
  );
}
