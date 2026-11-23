import { GROUP_ORDER, type PaletteEntry, type PaletteGroup } from "./entries";
import { fuzzyScore } from "./fuzzy";

export interface PaletteSection {
  /** "recent": the last used commands on top; "results": matches of a query, best first. */
  id: PaletteGroup | "recent" | "results";
  entries: PaletteEntry[];
}

/**
 * The order the palette shows: without a query the recently used commands first, then every
 * group in its order (entries that cannot run here at the end of their group); with a query
 * the matches by score, runnable ones first, and the hand-over to the search last.
 */
export function arrangeEntries(
  entries: readonly PaletteEntry[],
  query: string,
  recent: readonly string[],
  groupName: (group: PaletteGroup) => string,
): PaletteSection[] {
  const runnableFirst = (a: PaletteEntry, b: PaletteEntry) => Number(Boolean(a.disabled)) - Number(Boolean(b.disabled));
  if (!query.trim()) {
    const byKey = new Map(entries.map((entry) => [entry.key, entry]));
    const recentEntries = recent.flatMap((key) => {
      const entry = byKey.get(key);
      return entry && !entry.disabled ? [entry] : [];
    });
    const shown = new Set(recentEntries.map((entry) => entry.key));
    const sections: PaletteSection[] = recentEntries.length > 0 ? [{ id: "recent", entries: recentEntries }] : [];
    for (const group of GROUP_ORDER) {
      const inGroup = entries.filter((entry) => entry.group === group && !shown.has(entry.key));
      if (inGroup.length > 0) sections.push({ id: group, entries: [...inGroup].sort(runnableFirst) });
    }
    return sections;
  }
  const scored = entries.flatMap((entry, index) => {
    if (entry.group === "search") return [];
    const score = fuzzyScore(query, entry.label, [groupName(entry.group), entry.detail ?? "", ...(entry.keywords ?? [])]);
    return score === null ? [] : [{ entry, score, index }];
  });
  scored.sort((a, b) => runnableFirst(a.entry, b.entry) || b.score - a.score || a.index - b.index);
  const results = scored.map((item) => item.entry);
  const search = entries.filter((entry) => entry.group === "search");
  const sections: PaletteSection[] = [];
  if (results.length > 0) sections.push({ id: "results", entries: results });
  if (search.length > 0) sections.push({ id: "search", entries: search });
  return sections;
}
