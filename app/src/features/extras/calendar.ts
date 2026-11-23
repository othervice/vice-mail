import type { CalendarEvent, CalendarInvite, CalendarRecurrence, CalendarTime } from "../../services/ipc";

/** `t` for keys built at run time; tests/extras-calendar.test.ts checks that every key exists. */
export type Translate = (key: string, values?: Record<string, string | number>) => string;

const DAY_MS = 86_400_000;

/** `YYYY-MM-DD` as a UTC midnight, so formatting it with `timeZone: "UTC"` shows that very day. */
export function dayValue(date: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!match) return null;
  const value = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isNaN(value) ? null : value;
}

/** `YYYY-MM-DDTHH:MM[:SS]` (a wall-clock time) as if it were UTC, for formatting without a zone. */
function wallClockValue(local: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(local);
  if (!match) return null;
  return Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]), Number(match[4]), Number(match[5]));
}

const DAY_FORMAT: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "short", year: "numeric" };
const TIME_FORMAT: Intl.DateTimeFormatOptions = { ...DAY_FORMAT, hour: "2-digit", minute: "2-digit" };

export interface EventWhen {
  /** Start and end in the user's own time zone (or the day range of an all-day event). */
  text: string;
  allDay: boolean;
  /** The time as the file wrote it, when the user's zone shows another wall-clock time. */
  original: { time: string; zone: string } | null;
  /** No usable zone: the time is shown as written. */
  floating: boolean;
}

/**
 * When an event takes place, for the card: all-day events by day (the end is exclusive, so a
 * one-day event ends the next day and is shown as that one day), timed events in the user's
 * own zone from `utc`. `timeZone` is for tests; the app uses the system's zone.
 */
export function eventWhen(event: CalendarEvent, language: string, t: Translate, timeZone?: string): EventWhen | null {
  const { start, end } = event;
  if (!start) return null;
  if (start.kind === "date") {
    const first = dayValue(start.date);
    if (first === null) return null;
    const format = new Intl.DateTimeFormat(language, { ...DAY_FORMAT, timeZone: "UTC" });
    const endValue = end?.kind === "date" ? dayValue(end.date) : null;
    const last = endValue !== null && endValue - DAY_MS > first ? endValue - DAY_MS : first;
    const days = last > first ? format.formatRange(first, last) : format.format(first);
    return { text: t("extras.invite.allDay", { days }), allDay: true, original: null, floating: false };
  }
  const format = new Intl.DateTimeFormat(language, { ...TIME_FORMAT, ...(timeZone ? { timeZone } : {}) });
  const until = end?.kind === "dateTime" && end.utc > start.utc ? end.utc : null;
  const text = until !== null ? format.formatRange(start.utc, until) : format.format(start.utc);
  return { text, allDay: false, original: originalTime(start, language, timeZone), floating: start.floating };
}

/** Wall-clock time of an instant in the user's zone, `YYYY-MM-DDTHH:MM`. */
function userWallClock(utc: number, timeZone?: string): string {
  const parts = new Intl.DateTimeFormat("en-CA", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    ...(timeZone ? { timeZone } : {}),
  }).formatToParts(utc);
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((p) => p.type === type)?.value ?? "";
  return `${part("year")}-${part("month")}-${part("day")}T${part("hour")}:${part("minute")}`;
}

/**
 * The organizer's time, when it differs from what the user's zone shows ("16:00 in
 * America/New_York"). Compared by wall clock, not by zone name, so aliases of the user's own
 * zone add nothing.
 */
function originalTime(start: Extract<CalendarTime, { kind: "dateTime" }>, language: string, timeZone?: string) {
  if (start.floating || !start.zone) return null;
  if (start.local.slice(0, 16) === userWallClock(start.utc, timeZone)) return null;
  const wall = wallClockValue(start.local);
  if (wall === null) return null;
  const time = new Intl.DateTimeFormat(language, { ...TIME_FORMAT, timeZone: "UTC" }).format(wall);
  return { time, zone: start.zone };
}

/** A day of the file (`date`) or an instant (`dateTime`), as a date in the user's language. */
export function formatDay(time: CalendarTime, language: string, timeZone?: string): string | null {
  if (time.kind === "date") {
    const value = dayValue(time.date);
    return value === null ? null : new Intl.DateTimeFormat(language, { ...DAY_FORMAT, timeZone: "UTC" }).format(value);
  }
  return new Intl.DateTimeFormat(language, { ...DAY_FORMAT, ...(timeZone ? { timeZone } : {}) }).format(time.utc);
}

const WEEKDAYS = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"] as const;
/** 23 November 2026 is a Monday: the base for the weekday names of Intl. */
const A_MONDAY = Date.UTC(2026, 10, 23);
/** Ordinals the texts have words for ("second Tuesday", "last Sunday"). */
const ORDINALS = new Set([1, 2, 3, 4, 5, -1]);
/** RRULE parts the description cannot put into words; they are shown as written. */
const UNDESCRIBED = /(^|;)(BYWEEKNO|BYYEARDAY|BYHOUR|BYMINUTE|BYSECOND)=/i;

function weekdayName(code: string, language: string): string | null {
  const index = WEEKDAYS.indexOf(code as (typeof WEEKDAYS)[number]);
  if (index < 0) return null;
  return new Intl.DateTimeFormat(language, { weekday: "long", timeZone: "UTC" }).format(A_MONDAY + index * DAY_MS);
}

function monthName(month: number, language: string): string | null {
  if (!Number.isInteger(month) || month < 1 || month > 12) return null;
  return new Intl.DateTimeFormat(language, { month: "long", timeZone: "UTC" }).format(Date.UTC(2026, month - 1, 1));
}

function list(items: string[], language: string): string {
  return new Intl.ListFormat(language, { type: "conjunction" }).format(items);
}

/** `BYDAY` in words, or null when one entry has no words (then the rule is shown as written). */
function days(byDay: readonly string[], language: string, t: Translate): string | null {
  const names: string[] = [];
  for (const entry of byDay) {
    const match = /^([+-]?\d{1,2})?([A-Z]{2})$/.exec(entry.toUpperCase());
    const day = match?.[2] ? weekdayName(match[2], language) : null;
    if (!match || !day) return null;
    if (match[1] === undefined) {
      names.push(day);
      continue;
    }
    const ordinal = Number(match[1]);
    if (!ORDINALS.has(ordinal)) return null;
    names.push(t("extras.invite.recurrence.ordinalDay", { ordinal: t(`extras.invite.recurrence.ordinal.${String(ordinal)}`), day }));
  }
  return list(names, language);
}

function monthDays(byMonthDay: readonly number[], language: string, t: Translate): string | null {
  const names: string[] = [];
  for (const day of byMonthDay) {
    if (day === -1) names.push(t("extras.invite.recurrence.lastDay"));
    else if (day >= 1 && day <= 31) names.push(t("extras.invite.recurrence.dayNumber", { day }));
    else return null;
  }
  return list(names, language);
}

/**
 * A recurrence in words ("Every 2 weeks on Monday and Wednesday, until Thu, 31 Dec 2026").
 * Parts without words (BYSETPOS, BYWEEKNO, hourly rules, …) add the rule as written instead of
 * a guess. The core does not expand recurrences, and neither does this.
 */
export function describeRecurrence(recurrence: CalendarRecurrence, language: string, t: Translate, timeZone?: string): string {
  const { frequency, interval, count, until, byDay, byMonthDay, byMonth, bySetPos, rule } = recurrence;
  if (frequency === "other") return t("extras.invite.recurrence.rule", { rule });
  const parts: string[] = [t(`extras.invite.recurrence.${frequency}`, { count: Math.max(1, interval) })];
  let complete = bySetPos.length === 0 && !UNDESCRIBED.test(rule);
  if (byDay.length > 0) {
    const text = days(byDay, language, t);
    if (text) parts.push(t("extras.invite.recurrence.onDays", { days: text }));
    else complete = false;
  }
  if (byMonthDay.length > 0) {
    const text = monthDays(byMonthDay, language, t);
    if (text) parts.push(t("extras.invite.recurrence.onDays", { days: text }));
    else complete = false;
  }
  if (byMonth.length > 0) {
    const names = byMonth.map((month) => monthName(month, language));
    if (names.every((name): name is string => name !== null)) parts.push(t("extras.invite.recurrence.inMonths", { months: list(names, language) }));
    else complete = false;
  }
  let text = parts.join(" ");
  if (count !== null) text = t("extras.invite.recurrence.withCount", { text, count });
  const last = until ? formatDay(until, language, timeZone) : null;
  if (last) text = t("extras.invite.recurrence.withUntil", { text, date: last });
  return complete ? text : t("extras.invite.recurrence.withRule", { text, rule });
}

/** What kind of calendar message this is, for the card's label (iTIP method, RFC 5546). */
export type InviteKind = "request" | "update" | "cancel" | "reply" | "counter" | "publish" | "other";

export function inviteKind(invite: CalendarInvite, event: CalendarEvent | undefined): InviteKind {
  if (event?.cancelled || invite.method === "CANCEL") return "cancel";
  switch (invite.method) {
    case "REQUEST":
      return (event?.sequence ?? 0) > 0 ? "update" : "request";
    case "REPLY":
      return "reply";
    case "COUNTER":
      return "counter";
    case null:
    case "PUBLISH":
      return "publish";
    default:
      return "other";
  }
}
