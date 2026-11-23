import type { SearchProblem, SearchTermInfo, SearchTermKind, TextRange } from "../../services/ipc";

/** Filters that are a chip without a value (`has:attachment`, `is:unread` …). */
const FLAG_KINDS: ReadonlySet<SearchTermKind> = new Set(["has_attachment", "is_unread", "is_read", "is_flagged", "is_snoozed"]);

/** Whether a chip of this kind shows a value after its name. */
export function hasValue(kind: SearchTermKind): boolean {
  return !FLAG_KINDS.has(kind);
}

/** The terms shown as chips: every filter; free text stays in the field only. */
export function chipTerms(terms: readonly SearchTermInfo[]): SearchTermInfo[] {
  return terms.filter((term) => term.kind !== "text");
}

/** The text of a range of the query (UTF-16, as the core sends it), clipped to the query. */
export function rangeText(query: string, range: TextRange): string {
  return query.slice(Math.max(0, range.start), Math.max(0, Math.min(query.length, range.end)));
}

/**
 * The query without one term (a chip's remove button): the term's range goes, and the white
 * space around it shrinks to one space, so the rest of the query keeps its meaning.
 */
export function removeTerm(query: string, range: TextRange): string {
  const before = query.slice(0, Math.max(0, range.start)).replace(/\s+$/u, "");
  const after = query.slice(Math.max(0, range.end)).replace(/^\s+/u, "");
  return before && after ? `${before} ${after}` : before || after;
}

/** Problems in order of their place, each once (the core sorts them; this keeps it robust). */
export function sortedProblems(problems: readonly SearchProblem[]): SearchProblem[] {
  const seen = new Set<string>();
  return [...problems]
    .sort((a, b) => a.range.start - b.range.start || a.range.end - b.range.end)
    .filter((problem) => {
      const key = `${problem.code}:${String(problem.range.start)}:${String(problem.range.end)}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
}

/**
 * Whether the user's region writes `01/02/2026` month first (United States and a few others).
 * Read from the date format of the region, not from the app's language: an English UI in
 * Germany still writes the day first.
 */
export function monthFirstFor(locale: string): boolean {
  try {
    const parts = new Intl.DateTimeFormat(locale, { year: "numeric", month: "2-digit", day: "2-digit" }).formatToParts(
      new Date(2026, 0, 2),
    );
    const month = parts.findIndex((part) => part.type === "month");
    const day = parts.findIndex((part) => part.type === "day");
    return month >= 0 && day >= 0 && month < day;
  } catch {
    return false;
  }
}
