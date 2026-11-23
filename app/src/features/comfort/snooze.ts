import type { SnoozeChoice, SnoozeTimes } from "../../services/ipc";

/** The core's bounds for a time the user picks (decision 0020): a minute ahead, five years at most. */
export const MIN_SNOOZE_AHEAD_MS = 60_000;
export const MAX_SNOOZE_AHEAD_YEARS = 5;

export type PresetKind = Exclude<SnoozeChoice["kind"], "at">;

export interface SnoozePreset {
  kind: PresetKind;
  at: number;
}

/** The menu's presets in order; "later today" only while the core still offers it. */
export function snoozePresets(times: SnoozeTimes): SnoozePreset[] {
  const presets: SnoozePreset[] = [];
  if (times.laterToday !== null) presets.push({ kind: "laterToday", at: times.laterToday });
  presets.push({ kind: "tomorrow", at: times.tomorrow }, { kind: "weekend", at: times.weekend }, { kind: "nextWeek", at: times.nextWeek });
  return presets;
}

/**
 * The value of an `<input type="datetime-local">` ("2026-11-24T08:30") as Unix milliseconds in the
 * system's time zone, or null if it is incomplete. A local time that does not exist (the hour the
 * clock jumps forward) moves forward, as `Date` and the core (jiff "compatible") both do.
 */
export function parseLocalDateTime(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const [year, month, day, hour, minute] = match.slice(1).map(Number) as [number, number, number, number, number];
  const date = new Date(year, month - 1, day, hour, minute, 0, 0);
  // Reject what `Date` would roll over (31 February, hour 25).
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  return date.getTime();
}

/** The `datetime-local` value of a moment, in the system's time zone. */
export function toLocalDateTimeValue(at: number): string {
  const date = new Date(at);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export type SnoozeTimeProblem = "invalid" | "tooSoon" | "tooFar";

/** Why a picked time cannot be used, checked like the core does before asking it. */
export function snoozeTimeProblem(at: number | null, now: number): SnoozeTimeProblem | null {
  if (at === null || !Number.isFinite(at)) return "invalid";
  if (at < now + MIN_SNOOZE_AHEAD_MS) return "tooSoon";
  const limit = new Date(now);
  limit.setFullYear(limit.getFullYear() + MAX_SNOOZE_AHEAD_YEARS);
  if (at > limit.getTime()) return "tooFar";
  return null;
}

const formatters = new Map<string, Intl.DateTimeFormat>();

/** "Tue, 24 Nov, 08:00": weekday, day and time, the year only when it is not this year's. */
export function formatSnoozeTime(at: number, now: number, language: string): string {
  const withYear = new Date(at).getFullYear() !== new Date(now).getFullYear();
  const key = `${language}:${String(withYear)}`;
  let format = formatters.get(key);
  if (!format) {
    format = new Intl.DateTimeFormat(language, {
      weekday: "short",
      day: "numeric",
      month: "short",
      ...(withYear && { year: "numeric" }),
      hour: "2-digit",
      minute: "2-digit",
    });
    formatters.set(key, format);
  }
  return format.format(at);
}
