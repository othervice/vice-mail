import type { TFunction } from "i18next";
import type { TextSpan } from "../../../components/HighlightedText";
import { PRODUCT_NAME } from "../../../config/product";
import { SETTINGS_SECTION_IDS, type SettingsSectionId } from "../../../stores/ui";
import { SETTINGS_INDEX, type SettingsIndexEntry } from "./settingsIndex";

/*
 * Stage 22, item 8 (decision 0073): the settings search itself, without React. Matching is on
 * the texts of the current language: every word of the query must occur in the title or the
 * description, ignoring case and accents ("uberfahren" finds "Überfahren"). Hits are grouped by
 * section in the order of the section list; within a section the best first (a title that starts
 * with the query, a word of the title that does, a title that holds it, the description), then
 * in the order of the page.
 */


/** A row of the index with its texts in the current language. */
export interface ResolvedEntry {
  /** Unique within the index: the section and the title's key. */
  id: string;
  section: SettingsSectionId;
  /** The section itself (its name in the list), not a row on its page. */
  isSection: boolean;
  title: string;
  /** The first description, shown under the title. */
  description: string | null;
  /** Every description (platform and state variants), searched. */
  descriptions: readonly string[];
  /** For the jump: the title's i18n key where the row is told apart by it (`data-setting-key`). */
  key: string | null;
  /** Position in the index, for a stable order. */
  order: number;
}

export interface SettingsHit {
  entry: ResolvedEntry;
  titleRanges: TextSpan[];
  descriptionRanges: TextSpan[];
  score: number;
}

export interface SettingsHitGroup {
  section: SettingsSectionId;
  hits: SettingsHit[];
}

/** `t` with free keys and the parameters every text of the settings may use. */
function translate(t: TFunction, key: string): string {
  return (t as unknown as (key: string, options: Record<string, unknown>) => string)(key, { product: PRODUCT_NAME });
}

function list(description: SettingsIndexEntry["description"]): readonly string[] {
  if (description === undefined) return [];
  return typeof description === "string" ? [description] : description;
}

/** The index (and every section's name) in the current language. */
export function resolveIndex(t: TFunction): ResolvedEntry[] {
  const sections: ResolvedEntry[] = SETTINGS_SECTION_IDS.map((section, order) => ({
    id: `${section}:section`,
    section,
    isSection: true,
    title: translate(t, `settings.${section}.title`),
    description: null,
    descriptions: [],
    key: null,
    order,
  }));
  const rows = SETTINGS_INDEX.map((entry, i): ResolvedEntry => {
    const descriptions = list(entry.description).map((key) => translate(t, key));
    return {
      id: `${entry.section}:${entry.title}`,
      section: entry.section,
      isSection: false,
      title: translate(t, entry.title),
      description: descriptions[0] ?? null,
      descriptions,
      key: entry.byKey ? entry.title : null,
      order: SETTINGS_SECTION_IDS.length + i,
    };
  });
  return [...sections, ...rows];
}

/**
 * Lower case without accents, one code unit per code unit of the text, so a place found in the
 * folded text is the same place in the shown one.
 */
export function fold(text: string): string {
  let out = "";
  for (let i = 0; i < text.length; i += 1) {
    const char = text.charAt(i);
    const folded = char.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase();
    out += folded.length === 1 ? folded : char.toLocaleLowerCase().length === 1 ? char.toLocaleLowerCase() : char;
  }
  return out;
}

function words(query: string): string[] {
  return fold(query.trim())
    .split(/\s+/)
    .filter((word) => word.length > 0);
}

function rangesOf(haystack: string, word: string): TextSpan[] {
  const out: TextSpan[] = [];
  let from = 0;
  for (;;) {
    const at = haystack.indexOf(word, from);
    if (at < 0) return out;
    out.push({ start: at, end: at + word.length });
    from = at + word.length;
  }
}

/** Ranges sorted and merged where they touch or overlap. */
function merge(ranges: readonly TextSpan[]): TextSpan[] {
  const sorted = [...ranges].sort((a, b) => a.start - b.start);
  const out: TextSpan[] = [];
  for (const range of sorted) {
    const last = out[out.length - 1];
    if (last && range.start <= last.end) last.end = Math.max(last.end, range.end);
    else out.push({ start: range.start, end: range.end });
  }
  return out;
}

const isWordStart = (text: string, at: number) => at === 0 || /[\s\p{P}]/u.test(text.charAt(at - 1));

function match(entry: ResolvedEntry, query: readonly string[]): SettingsHit | null {
  const title = fold(entry.title);
  const shown = entry.description === null ? "" : fold(entry.description);
  const others = entry.descriptions.slice(1).map(fold);
  let score = 0;
  const titleRanges: TextSpan[] = [];
  const descriptionRanges: TextSpan[] = [];
  for (const word of query) {
    const inTitle = rangesOf(title, word);
    const inShown = rangesOf(shown, word);
    if (inTitle.length > 0) {
      titleRanges.push(...inTitle);
      score += inTitle.some(({ start }) => start === 0) ? 6 : inTitle.some(({ start }) => isWordStart(title, start)) ? 4 : 2;
    } else if (inShown.length > 0) {
      score += 1;
    } else if (!others.some((text) => text.includes(word))) {
      return null;
    }
    descriptionRanges.push(...inShown);
  }
  // A section's name is the strongest hit of its group.
  if (entry.isSection) score += 1;
  return { entry, titleRanges: merge(titleRanges), descriptionRanges: merge(descriptionRanges), score };
}

/** The hits of `query`, grouped by section in the order of the section list; none for an empty query. */
export function searchSettings(index: readonly ResolvedEntry[], query: string): SettingsHitGroup[] {
  const terms = words(query);
  if (terms.length === 0) return [];
  const hits = index.map((entry) => match(entry, terms)).filter((hit): hit is SettingsHit => hit !== null);
  return SETTINGS_SECTION_IDS.map((section) => ({
    section,
    hits: hits.filter((hit) => hit.entry.section === section).sort((a, b) => b.score - a.score || a.entry.order - b.entry.order),
  })).filter((group) => group.hits.length > 0);
}
