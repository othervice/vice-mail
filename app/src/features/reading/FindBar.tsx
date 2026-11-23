import { ChevronDown, ChevronUp, Search, X, type LucideIcon } from "lucide-react";
import { useEffect, useEffectEvent, useId, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "../../components/Tooltip";
import { boundShortcuts } from "../../stores/shortcuts";
import { matchesShortcut, shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
import { clearHighlights, collectSegments, findMatches, paintMatches, scrollToMatch, type FindMatch } from "./find";

/**
 * Stage 16: whether a key press means "find in this message". The same keys as the mail search
 * (`APP_COMMANDS.search`, as the user assigned it) with a modifier: Cmd/Ctrl+F by default. Inside
 * the reading pane they find in the message; everywhere else they stay the mail search.
 */
export function useFindShortcut(): (event: globalThis.KeyboardEvent) => boolean {
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  return (event) =>
    !event.repeat &&
    boundShortcuts("search", platform).some((shortcut) => shortcut.mod === true && matchesShortcut(event, shortcut, platform));
}

/** Content changes (a body loads, a frame finishes, the format switches) are searched again after this pause. */
const RESEARCH_DELAY_MS = 120;

interface FindBarProps {
  /** The pane's scrolling content: what is searched and scrolled. */
  scopeRef: RefObject<HTMLElement | null>;
  /** Grows with every find shortcut while the bar is open: the field takes the focus again. */
  focusSeq: number;
  /** Escape or the close button; the pane puts the focus back where it was. */
  onClose: () => void;
}

/**
 * The find bar at the top of the reading pane: a field, "2 of 5", previous and next. Enter and
 * Shift+Enter step through the matches, Escape closes. Searches the shown text, in the mail
 * frame too (features/reading/find.ts: read from here, nothing runs in the frame).
 */
export function FindBar({ scopeRef, focusSeq, onClose }: FindBarProps) {
  const { t, i18n } = useTranslation();
  const inputId = useId();
  const statusId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const docs = useRef(new Set<Document>());
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<readonly FindMatch[]>([]);
  const [index, setIndex] = useState(0);
  const numbers = new Intl.NumberFormat(i18n.language);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusSeq]);

  const search = useEffectEvent((keepIndex: boolean) => {
    const root = scopeRef.current;
    const found = root ? findMatches(collectSegments(root), query) : [];
    setMatches(found);
    setIndex((current) => (keepIndex ? Math.min(current, Math.max(0, found.length - 1)) : 0));
  });

  useEffect(() => {
    search(false);
  }, [query]);

  // The shown text changes under an open bar (body loaded, frame loaded, HTML/Text switched).
  useEffect(() => {
    const root = scopeRef.current;
    if (!root) return undefined;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const schedule = () => {
      timer ??= setTimeout(() => {
        timer = null;
        search(true);
      }, RESEARCH_DELAY_MS);
    };
    const observer = new MutationObserver(schedule);
    observer.observe(root, { childList: true, subtree: true, characterData: true });
    // `load` of a mail frame does not bubble; the capture phase still passes the pane.
    root.addEventListener("load", schedule, true);
    return () => {
      observer.disconnect();
      root.removeEventListener("load", schedule, true);
      if (timer !== null) clearTimeout(timer);
    };
  }, [scopeRef]);

  useEffect(() => {
    paintMatches(matches, index, docs.current);
    const current = matches[index];
    const scroller = scopeRef.current;
    if (current && scroller) scrollToMatch(current, scroller);
  }, [matches, index, scopeRef]);

  useEffect(() => {
    const touched = docs.current;
    return () => {
      clearHighlights(touched);
    };
  }, []);

  const step = (delta: number) => {
    if (matches.length === 0) return;
    setIndex((current) => (current + delta + matches.length) % matches.length);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      // Handled here: the pane's own Escape (back to the list) must not run as well.
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    }
  };

  const total = matches.length;
  const status =
    query.trim() === ""
      ? ""
      : total === 0
        ? t("find.none")
        : t("find.count", { current: numbers.format(index + 1), total: numbers.format(total) });

  return (
    <div
      role="search"
      aria-label={t("find.label")}
      className="flex shrink-0 items-center gap-2 border-b border-border-subtle bg-surface-list ps-3 pe-2 py-2"
      data-testid="find-bar"
      data-print-hide
    >
      <Search aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      <label htmlFor={inputId} className="sr-only">
        {t("find.field")}
      </label>
      <input
        ref={inputRef}
        id={inputId}
        type="text"
        value={query}
        placeholder={t("find.placeholder")}
        autoComplete="off"
        spellCheck={false}
        aria-describedby={statusId}
        onChange={(event) => {
          setQuery(event.target.value);
        }}
        onKeyDown={onKeyDown}
        data-testid="find-input"
        className="h-8 min-w-0 flex-1 rounded-control border border-border-strong bg-surface px-2 text-13 text-primary placeholder:text-tertiary focus-visible:outline-2 focus-visible:outline-focus-ring"
      />
      <span
        id={statusId}
        role="status"
        aria-live="polite"
        className="shrink-0 text-12 text-secondary tabular-nums"
        data-testid="find-count"
        data-total={total}
        data-current={total > 0 ? index + 1 : 0}
      >
        {status}
      </span>
      <FindButton icon={ChevronUp} label={t("find.previous")} disabled={total === 0} onClick={() => { step(-1); }} data-testid="find-previous" />
      <FindButton icon={ChevronDown} label={t("find.next")} disabled={total === 0} onClick={() => { step(1); }} data-testid="find-next" />
      <FindButton icon={X} label={t("find.close")} onClick={onClose} data-testid="find-close" />
    </div>
  );
}

function FindButton({
  icon: Icon,
  label,
  disabled = false,
  onClick,
  ...rest
}: {
  icon: LucideIcon;
  label: string;
  disabled?: boolean;
  onClick: () => void;
  "data-testid": string;
}) {
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        onClick={onClick}
        data-testid={rest["data-testid"]}
        className="flex size-8 shrink-0 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-secondary"
      >
        <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
