import type { ReactNode } from "react";
import type { MessageListItem } from "../../services/ipc";

/**
 * Date sections of the list (docs/DESIGN.md): Today, Yesterday, This week, then one per month.
 * Messages dated in the future (a sender's wrong clock) go into the section of their month.
 */
export type Section =
  | { kind: "today" }
  | { kind: "yesterday" }
  | { kind: "thisWeek" }
  | { kind: "month"; year: number; month: number };

export type ListRow =
  | { kind: "section"; key: string; section: Section }
  /** `position` counts messages only (0-based), for `aria-posinset` and keyboard moves. */
  | { kind: "message"; key: string; item: MessageListItem; position: number }
  /**
   * Stage 9: a row the list only draws (block heading, bundle, "Show all"), with the height of a
   * section heading or of a message row. Not an option: the arrow keys skip it like a heading.
   */
  | { kind: "extra"; key: string; size: "section" | "row"; render: () => ReactNode };

/** The rows of one folder's pages: headings and messages, never drawn-only rows. */
export type FolderRow = Exclude<ListRow, { kind: "extra" }>;

/** Local day boundaries in Unix milliseconds, computed once per day. */
export interface DayBounds {
  /** Start of the current week (by the language's first day of the week). */
  week: number;
  yesterday: number;
  today: number;
  tomorrow: number;
  year: number;
}

function startOfDay(date: Date): Date {
  const day = new Date(date);
  day.setHours(0, 0, 0, 0);
  return day;
}

function addDays(date: Date, days: number): Date {
  const next = new Date(date);
  // setDate keeps local midnight across daylight-saving changes; adding 24 h would not.
  next.setDate(next.getDate() + days);
  return next;
}

/** `firstDayOfWeek`: 1 = Monday … 7 = Sunday, as in `Intl.Locale#getWeekInfo`. */
export function dayBounds(now: Date, firstDayOfWeek: number): DayBounds {
  const today = startOfDay(now);
  const offset = (today.getDay() - (firstDayOfWeek % 7) + 7) % 7;
  return {
    week: addDays(today, -offset).getTime(),
    yesterday: addDays(today, -1).getTime(),
    today: today.getTime(),
    tomorrow: addDays(today, 1).getTime(),
    year: today.getFullYear(),
  };
}

type LocaleWithWeekInfo = Intl.Locale & {
  getWeekInfo?: () => { firstDay: number };
  weekInfo?: { firstDay: number };
};

/** First day of the week for a UI language; Monday (ISO 8601) where the runtime cannot say. */
export function firstDayOfWeek(language: string): number {
  try {
    const locale = new Intl.Locale(language) as LocaleWithWeekInfo;
    const info = locale.getWeekInfo?.() ?? locale.weekInfo;
    return info?.firstDay ?? 1;
  } catch {
    return 1;
  }
}

export function sectionOf(date: number, bounds: DayBounds): Section {
  if (date >= bounds.today && date < bounds.tomorrow) return { kind: "today" };
  if (date >= bounds.yesterday && date < bounds.today) return { kind: "yesterday" };
  if (date >= bounds.week && date < bounds.yesterday) return { kind: "thisWeek" };
  const day = new Date(date);
  return { kind: "month", year: day.getFullYear(), month: day.getMonth() };
}

function sectionKey(section: Section): string {
  return section.kind === "month" ? `month-${section.year}-${section.month}` : section.kind;
}

/**
 * Flattens the messages (newest first) into list rows with a section row before each new
 * section. Duplicate ids (never expected from keyset pages) are dropped, so row keys stay unique.
 */
export function buildRows(items: readonly MessageListItem[], bounds: DayBounds): FolderRow[] {
  const rows: FolderRow[] = [];
  const seen = new Set<number>();
  let current = "";
  let position = 0;
  for (const item of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    const section = sectionOf(item.date, bounds);
    const key = sectionKey(section);
    if (key !== current) {
      // A section can only come back if the core's order were broken; keep its key unique.
      rows.push({ kind: "section", key: `section-${key}-${rows.length}`, section });
      current = key;
    }
    rows.push({ kind: "message", key: `message-${item.id}`, item, position });
    position += 1;
  }
  return rows;
}

const formatters = new Map<string, Intl.DateTimeFormat>();

function formatter(language: string, kind: "time" | "dayMonth" | "date" | "full" | "month" | "monthYear") {
  const key = `${language}:${kind}`;
  let format = formatters.get(key);
  if (!format) {
    const options: Intl.DateTimeFormatOptions = {
      time: { timeStyle: "short" } as const,
      dayMonth: { day: "numeric", month: "short" } as const,
      date: { dateStyle: "short" } as const,
      full: { dateStyle: "full", timeStyle: "short" } as const,
      month: { month: "long" } as const,
      monthYear: { month: "long", year: "numeric" } as const,
    }[kind];
    format = new Intl.DateTimeFormat(language, options);
    formatters.set(key, format);
  }
  return format;
}

/** Time column: today the time, this year day and month, before that the short date. */
export function formatListDate(date: number, bounds: DayBounds, language: string): string {
  if (date >= bounds.today && date < bounds.tomorrow) return formatter(language, "time").format(date);
  if (new Date(date).getFullYear() === bounds.year) return formatter(language, "dayMonth").format(date);
  return formatter(language, "date").format(date);
}

/** Full date and time, for the tooltip of the time column. */
export function formatFullDate(date: number, language: string): string {
  return formatter(language, "full").format(date);
}

/** Label of a month section; the year only when it is not the current one. */
export function formatMonthSection(section: { year: number; month: number }, bounds: DayBounds, language: string) {
  const date = new Date(section.year, section.month, 1);
  return formatter(language, section.year === bounds.year ? "month" : "monthYear").format(date);
}
