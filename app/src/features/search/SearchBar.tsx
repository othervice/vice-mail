import { Search, X } from "lucide-react";
import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { splitHighlights } from "../../components/HighlightedText";
import type { SearchPage, SearchProblemCode, SearchTermInfo } from "../../services/ipc";
import { useSearchStore, type SearchScopeChoice } from "../../stores/search";
import { APP_COMMANDS } from "../actions/registry";
import { ariaShortcut, type ShortcutPlatform } from "../actions/shortcuts";
import { chipTerms, hasValue, rangeText, removeTerm, sortedProblems } from "./terms";

interface SearchBarProps {
  /** Scopes that make sense in the current view, narrowest first. */
  scopes: readonly SearchScopeChoice[];
  scope: SearchScopeChoice;
  /** The answer to the text in `searched`; problems and chips refer to that text. */
  page: SearchPage | undefined;
  searched: string | null;
  platform: ShortcutPlatform;
  /** Escape on an empty field: the search closes and the focus goes back to the list. */
  onClose: () => void;
}

/** One problem: the place in the query that it is about, and what is wrong there. */
function ProblemLine({ query, code, start, end }: { query: string; code: SearchProblemCode; start: number; end: number }) {
  const { t } = useTranslation();
  const problem = t(`search.problems.codes.${code}`);
  const text = rangeText(query, { start, end }).trim();
  return <li>{text ? t("search.problems.at", { text, problem }) : problem}</li>;
}

/** The searched text with the places of its problems underlined, right under the field. */
function MarkedQuery({ query, page }: { query: string; page: SearchPage }) {
  const pieces = splitHighlights(
    query,
    page.problems.map((problem) => problem.range),
  );
  return (
    <p aria-hidden className="truncate font-mono text-12 whitespace-pre text-secondary" data-testid="search-marked">
      {pieces.map((piece, index) =>
        piece.marked ? (
          <span key={index} className="text-danger underline decoration-danger decoration-wavy" data-testid="search-problem-mark">
            {piece.text}
          </span>
        ) : (
          <span key={index}>{piece.text}</span>
        ),
      )}
    </p>
  );
}

function Chip({ term, query, onRemove }: { term: SearchTermInfo; query: string; onRemove: () => void }) {
  const { t } = useTranslation();
  const kind = t(`search.chips.kinds.${term.kind}`);
  const value = rangeText(query, term.valueRange);
  let label = hasValue(term.kind) && value ? `${kind}: ${value}` : kind;
  if (term.negated) label = t("search.chips.negated", { term: label });
  if (term.alternative) label = t("search.chips.alternative", { term: label });
  return (
    <li
      className={cx(
        "flex h-6 items-center gap-1 rounded-control border ps-2 text-12",
        term.negated ? "border-danger text-danger" : "border-border-strong text-primary",
      )}
      data-testid="search-chip"
    >
      <span className="truncate">{label}</span>
      <button
        type="button"
        aria-label={t("search.chips.remove", { term: rangeText(query, term.range).trim() || label })}
        onClick={onRemove}
        className="flex size-6 items-center justify-center rounded-control text-tertiary hover:bg-hover hover:text-primary"
      >
        <X aria-hidden className="size-3" strokeWidth={2} />
      </button>
    </li>
  );
}

/**
 * The search field above the list (stage 7): results while typing, the scope, the problems of
 * the query at their place (underlined in a copy of the text and explained below it), the
 * filters as chips that can be removed, and the state of the index while it is being built.
 */
export function SearchBar({ scopes, scope, page, searched, platform, onClose }: SearchBarProps) {
  const { t } = useTranslation();
  const query = useSearchStore((state) => state.query);
  const focusSeq = useSearchStore((state) => state.focusSeq);
  const setQuery = useSearchStore((state) => state.setQuery);
  const submit = useSearchStore((state) => state.submit);
  const setScope = useSearchStore((state) => state.setScope);
  const inputRef = useRef<HTMLInputElement>(null);
  const problemsId = useId();
  const hintId = useId();
  const scopeId = useId();

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusSeq]);

  // Problems and chips belong to the text that was searched; while a newer one is on its way the
  // old marks would point at the wrong places.
  const current = page && searched !== null && searched === query ? page : undefined;
  const problems = current ? sortedProblems(current.problems) : [];
  const chips = current ? chipTerms(current.terms) : [];
  const index = current?.index;
  const shortcut = APP_COMMANDS.search.shortcuts[platform][0];

  return (
    <form
      role="search"
      aria-label={t("search.field")}
      className="flex shrink-0 flex-col gap-2 border-b border-border px-4 py-3"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
      data-testid="search-bar"
    >
      <div className="flex items-center gap-2">
        <div
          className={cx(
            "flex h-8 min-w-0 flex-1 items-center gap-2 rounded-control border bg-surface-raised px-2",
            problems.length > 0 ? "border-danger" : "border-border-strong",
          )}
        >
          <Search aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
          <input
            ref={inputRef}
            type="search"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
            }}
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              event.preventDefault();
              if (query) setQuery("");
              else onClose();
            }}
            aria-label={t("search.field")}
            aria-invalid={problems.length > 0 ? true : undefined}
            aria-describedby={[problems.length > 0 ? problemsId : null, query ? null : hintId].filter(Boolean).join(" ") || undefined}
            aria-keyshortcuts={shortcut ? ariaShortcut(shortcut, platform) : undefined}
            placeholder={t("search.placeholder")}
            autoComplete="off"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            className="h-full min-w-0 flex-1 bg-transparent text-14 text-primary outline-none select-text placeholder:text-tertiary"
            data-testid="search-input"
          />
          {query && (
            <button
              type="button"
              aria-label={t("search.clear")}
              onClick={() => {
                setQuery("");
                inputRef.current?.focus();
              }}
              className="flex size-6 shrink-0 items-center justify-center rounded-control text-tertiary hover:bg-hover hover:text-primary"
            >
              <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            </button>
          )}
        </div>
        <label htmlFor={scopeId} className="sr-only">
          {t("search.scope.label")}
        </label>
        <select
          id={scopeId}
          value={scope}
          onChange={(event) => {
            setScope(event.target.value as SearchScopeChoice);
          }}
          className="h-8 shrink-0 rounded-control border border-border-strong bg-surface-raised px-2 text-13 text-primary"
          data-testid="search-scope"
        >
          {scopes.map((choice) => (
            <option key={choice} value={choice}>
              {t(`search.scope.${choice}`)}
            </option>
          ))}
        </select>
        <button
          type="button"
          aria-label={t("search.close")}
          onClick={onClose}
          className="flex size-8 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          data-testid="search-close"
        >
          <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
        </button>
      </div>
      {!query && (
        <p id={hintId} className="text-12 text-secondary">
          {t("search.hint.body")}
        </p>
      )}
      {current && problems.length > 0 && (
        <div className="flex flex-col gap-1">
          <MarkedQuery query={query} page={current} />
          <ul
            id={problemsId}
            aria-label={t("search.problems.label")}
            aria-live="polite"
            className="flex flex-col gap-1 text-12 text-danger"
            data-testid="search-problems"
          >
            {problems.map((problem) => (
              <ProblemLine
                key={`${problem.code}-${String(problem.range.start)}-${String(problem.range.end)}`}
                query={query}
                code={problem.code}
                start={problem.range.start}
                end={problem.range.end}
              />
            ))}
          </ul>
        </div>
      )}
      {chips.length > 0 && (
        <ul aria-label={t("search.chips.label")} className="flex flex-wrap gap-1" data-testid="search-chips">
          {chips.map((term) => (
            <Chip
              key={`${term.kind}-${String(term.range.start)}`}
              term={term}
              query={query}
              onRemove={() => {
                setQuery(removeTerm(query, term.range));
                inputRef.current?.focus();
              }}
            />
          ))}
        </ul>
      )}
      {index && index.state !== "ready" && (
        <p role="status" className="text-12 text-secondary" data-testid="search-index">
          {t(`search.index.${index.state}`, { percent: index.percent })}
        </p>
      )}
    </form>
  );
}
