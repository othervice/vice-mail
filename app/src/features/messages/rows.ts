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

/**
 * Decision 0031: consecutive rows with the same card are drawn as one card: the priority block
 * (heading, its messages, "Show all") on its warm tint, an unfolded bundle with its members on a
 * raised surface.
 */
export type ListCard = "priority" | "bundle";

export type ListRow =
  | { kind: "section"; key: string; section: Section }
  /**
   * `position` counts messages only (0-based; in a conversation tree, child rows too), for
   * `aria-posinset` and keyboard moves. `level`: depth in a conversation tree (stage 17), 1 unless
   * the row is a member of an unfolded bundle (2).
   */
  | { kind: "message"; key: string; item: MessageListItem; position: number; card?: ListCard; level?: number }
  /**
   * Stage 17 (decision 0049): a message of an unfolded conversation, below the conversation's row
   * (`parent`: its identity, `parentItem`: its message), one level deeper. `last`: the last one,
   * where the conversation's line ends.
   */
  | {
      kind: "child";
      key: string;
      item: MessageListItem;
      parent: string;
      parentItem: MessageListItem;
      position: number;
      level: number;
      last: boolean;
      card?: ListCard;
    }
  /**
   * Stage 9: a row the list draws from its source (block heading, bundle, "Show all"), with the
   * height of a section heading or of a message row. Without `action` it is decoration that the
   * arrow keys skip like a heading. With one it is an `option` of the list (axe
   * `aria-required-children`: a listbox holds options, no buttons): the arrow keys reach it,
   * Enter, Space or a click run `activate`; it is never part of the selection. Its accessible name
   * comes from its content.
   */
  | {
      kind: "extra";
      key: string;
      /** `child`: the height of a child row (stage 17: loading, failed or cut conversations). */
      size: "section" | "row" | "child";
      render: () => ReactNode;
      card?: ListCard;
      action?: { activate: () => void };
      /** Depth in a conversation tree (default 1). */
      level?: number;
      /** A bundle in a conversation tree: whether it is unfolded (`aria-expanded`). */
      expanded?: boolean;
      /** Stage 17: the conversation a status row belongs to. */
      parent?: string;
    };

/** The rows of one folder's pages: headings and messages, never drawn-only rows. */
export type FolderRow = Extract<ListRow, { kind: "section" } | { kind: "message" }>;

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
 * Rows without date sections, for lists that are not a folder's timeline (search results): every
 * hit says its date in its row. Duplicate ids are dropped, as in `buildRows`.
 */
export function buildRowsWithoutSections(items: readonly MessageListItem[]): FolderRow[] {
  const rows: FolderRow[] = [];
  const seen = new Set<number>();
  for (const item of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    rows.push({ kind: "message", key: `message-${item.id}`, item, position: rows.length });
  }
  return rows;
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

function formatter(language: string, kind: "time" | "dayMonth" | "dayMonthYear" | "full" | "month" | "monthYear") {
  const key = `${language}:${kind}`;
  let format = formatters.get(key);
  if (!format) {
    const options: Intl.DateTimeFormatOptions = {
      time: { timeStyle: "short" } as const,
      dayMonth: { day: "numeric", month: "short" } as const,
      dayMonthYear: { day: "numeric", month: "short", year: "numeric" } as const,
      full: { dateStyle: "full", timeStyle: "short" } as const,
      month: { month: "long" } as const,
      monthYear: { month: "long", year: "numeric" } as const,
    }[kind];
    format = new Intl.DateTimeFormat(language, options);
    formatters.set(key, format);
  }
  return format;
}

/**
 * Time column: today the time, this year day and month ("Oct 9"), before that the same with the
 * year ("Sep 6, 2025"). One way of writing a date, also where no month headings stand above the
 * rows (search results).
 */
export function formatListDate(date: number, bounds: DayBounds, language: string): string {
  if (date >= bounds.today && date < bounds.tomorrow) return formatter(language, "time").format(date);
  if (new Date(date).getFullYear() === bounds.year) return formatter(language, "dayMonth").format(date);
  return formatter(language, "dayMonthYear").format(date);
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
