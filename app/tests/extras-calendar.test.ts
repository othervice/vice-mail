import { describe, expect, it } from "vitest";
import { describeRecurrence, eventWhen, formatDay, inviteKind, type Translate } from "../src/features/extras/calendar";
import { mailtoNoticeText } from "../src/features/extras/MailtoNotices";
import { createI18n, type Language } from "../src/i18n";
import type { InviteKind } from "../src/features/extras/calendar";
import type { CalendarProblemCode, CalendarRecurrence, MailtoField, MailtoNotice, ParticipationStatus } from "../src/services/ipc";
import { makeCalendarEvent } from "./support/extras";

/** The app's own `t`, failing on keys that do not exist (i18next would return the key). */
function translator(language: Language): Translate {
  const i18n = createI18n(language);
  // Keys are built at run time here, which the typed `t` and `exists` cannot follow.
  const exists = i18n.exists.bind(i18n) as unknown as (key: string, values?: object) => boolean;
  const t = i18n.t.bind(i18n) as unknown as Translate;
  return (key, values) => {
    expect(exists(key, values), `missing text ${key} (${language})`).toBe(true);
    return t(key, values);
  };
}

const en = translator("en");
/** Intl puts thin and narrow no-break spaces into ranges and times; the tests read plain spaces. */
const plain = (text: string | null | undefined) => text?.replace(/[\u2009\u202f]/g, " ");
const de = translator("de");

function rule(overrides: Partial<CalendarRecurrence>): CalendarRecurrence {
  return {
    frequency: "weekly",
    interval: 1,
    count: null,
    until: null,
    byDay: [],
    byMonthDay: [],
    byMonth: [],
    bySetPos: [],
    rule: "FREQ=WEEKLY",
    ...overrides,
  };
}

describe("invitation times", () => {
  it("shows a one-day all-day event as that day (the end is exclusive)", () => {
    const event = makeCalendarEvent({ start: { kind: "date", date: "2026-12-24" }, end: { kind: "date", date: "2026-12-25" } });
    const when = eventWhen(event, "en", en, "Europe/Berlin");
    expect(when).toEqual({ text: "Thu, Dec 24, 2026, all day", allDay: true, original: null, floating: false });
    expect(eventWhen(event, "de", de, "Europe/Berlin")?.text).toBe("Do., 24. Dez. 2026, ganztägig");
  });

  it("shows several all-day days up to the day before the exclusive end", () => {
    const event = makeCalendarEvent({ start: { kind: "date", date: "2026-12-24" }, end: { kind: "date", date: "2026-12-27" } });
    expect(plain(eventWhen(event, "en", en, "America/Los_Angeles")?.text)).toBe("Thu, Dec 24 – Sat, Dec 26, 2026, all day");
  });

  it("keeps the day of an all-day event in every time zone (no shift to the day before)", () => {
    const event = makeCalendarEvent({ start: { kind: "date", date: "2027-01-01" }, end: null });
    expect(eventWhen(event, "en", en, "Pacific/Honolulu")?.text).toBe("Fri, Jan 1, 2027, all day");
    expect(eventWhen(event, "en", en, "Pacific/Kiritimati")?.text).toBe("Fri, Jan 1, 2027, all day");
  });

  it("shows a timed event in the user's own zone, from the instant", () => {
    const start = Date.UTC(2026, 10, 30, 15, 0);
    const event = makeCalendarEvent({
      start: { kind: "dateTime", utc: start, local: "2026-11-30T16:00:00", zone: "Europe/Berlin", floating: false },
      end: { kind: "dateTime", utc: start + 3_600_000, local: "2026-11-30T17:00:00", zone: "Europe/Berlin", floating: false },
    });
    const when = eventWhen(event, "de", de, "Europe/Berlin");
    expect(when?.text).toBe("Mo., 30. Nov. 2026, 16:00–17:00 Uhr");
    // Same zone as the user: no second time.
    expect(when?.original).toBeNull();
  });

  it("adds the organizer's time for a foreign zone", () => {
    const start = Date.UTC(2026, 10, 30, 15, 0); // 10:00 in New York, 16:00 in Berlin
    const event = makeCalendarEvent({
      start: { kind: "dateTime", utc: start, local: "2026-11-30T10:00:00", zone: "America/New_York", floating: false },
      end: null,
    });
    const when = eventWhen(event, "en", en, "Europe/Berlin");
    expect(when?.text).toBe("Mon, Nov 30, 2026, 04:00 PM");
    expect(when?.original).toEqual({ time: "Mon, Nov 30, 2026, 10:00 AM", zone: "America/New_York" });
  });

  it("finds no difference for an alias of the user's own zone", () => {
    const start = Date.UTC(2026, 6, 1, 6, 30); // 12:00 in Kolkata
    const event = makeCalendarEvent({
      start: { kind: "dateTime", utc: start, local: "2026-07-01T12:00:00", zone: "Asia/Calcutta", floating: false },
    });
    expect(eventWhen(event, "en", en, "Asia/Kolkata")?.original).toBeNull();
  });

  it("follows summer time: the same New York meeting moves by an hour in Berlin around the switch", () => {
    // 27 March 2026: New York already on summer time, Berlin not yet (five hours apart, not six).
    const march = makeCalendarEvent({
      start: { kind: "dateTime", utc: Date.UTC(2026, 2, 27, 14, 0), local: "2026-03-27T10:00:00", zone: "America/New_York", floating: false },
    });
    const april = makeCalendarEvent({
      start: { kind: "dateTime", utc: Date.UTC(2026, 3, 3, 14, 0), local: "2026-04-03T10:00:00", zone: "America/New_York", floating: false },
    });
    expect(eventWhen(march, "de", de, "Europe/Berlin")?.text).toBe("Fr., 27. März 2026, 15:00");
    expect(eventWhen(april, "de", de, "Europe/Berlin")?.text).toBe("Fr., 3. Apr. 2026, 16:00");
  });

  it("marks a floating time and shows it as written", () => {
    const event = makeCalendarEvent({
      start: { kind: "dateTime", utc: Date.UTC(2026, 10, 30, 9, 0), local: "2026-11-30T10:00:00", zone: null, floating: true },
    });
    const when = eventWhen(event, "en", en, "Europe/Berlin");
    expect(when?.floating).toBe(true);
    expect(when?.original).toBeNull();
    expect(when?.text).toBe("Mon, Nov 30, 2026, 10:00 AM");
  });

  it("has no time for an event without a start, and leaves out an end of another kind", () => {
    expect(eventWhen(makeCalendarEvent({ start: null }), "en", en)).toBeNull();
    const mixed = makeCalendarEvent({
      start: { kind: "dateTime", utc: Date.UTC(2026, 10, 30, 9, 0), local: "2026-11-30T09:00:00", zone: "UTC", floating: false },
      end: { kind: "date", date: "2026-12-01" },
    });
    expect(eventWhen(mixed, "en", en, "UTC")?.text).toBe("Mon, Nov 30, 2026, 09:00 AM");
    expect(eventWhen(makeCalendarEvent({ start: { kind: "date", date: "2026-02-30x" } }), "en", en)).toBeNull();
  });

  it("formats days of either kind", () => {
    expect(formatDay({ kind: "date", date: "2026-12-31" }, "en")).toBe("Thu, Dec 31, 2026");
    expect(
      formatDay({ kind: "dateTime", utc: Date.UTC(2026, 11, 31, 23, 30), local: "", zone: "UTC", floating: false }, "en", "Europe/Berlin"),
    ).toBe("Fri, Jan 1, 2027");
  });
});

describe("repetitions in words", () => {
  it("describes plain frequencies with their interval", () => {
    expect(describeRecurrence(rule({ frequency: "daily", rule: "FREQ=DAILY" }), "en", en)).toBe("Every day");
    expect(describeRecurrence(rule({ interval: 2 }), "en", en)).toBe("Every 2 weeks");
    expect(describeRecurrence(rule({ frequency: "yearly", rule: "FREQ=YEARLY" }), "de", de)).toBe("Jedes Jahr");
    expect(describeRecurrence(rule({ frequency: "monthly", interval: 3, rule: "FREQ=MONTHLY;INTERVAL=3" }), "de", de)).toBe(
      "Alle 3 Monate",
    );
  });

  it("names the weekdays, ordinal weekdays and days of the month", () => {
    expect(describeRecurrence(rule({ interval: 2, byDay: ["MO", "WE"] }), "en", en)).toBe("Every 2 weeks on Monday and Wednesday");
    expect(describeRecurrence(rule({ byDay: ["MO", "WE"] }), "de", de)).toBe("Jede Woche am Montag und Mittwoch");
    const monthly = rule({ frequency: "monthly", byDay: ["2TU"], rule: "FREQ=MONTHLY;BYDAY=2TU" });
    expect(describeRecurrence(monthly, "en", en)).toBe("Every month on the second Tuesday");
    expect(describeRecurrence(monthly, "de", de)).toBe("Jeden Monat am zweiten Dienstag");
    expect(describeRecurrence(rule({ frequency: "monthly", byDay: ["-1SU"] }), "en", en)).toBe("Every month on the last Sunday");
    expect(describeRecurrence(rule({ frequency: "monthly", byMonthDay: [15, -1] }), "en", en)).toBe(
      "Every month on day 15 and the last day",
    );
    expect(describeRecurrence(rule({ frequency: "monthly", byMonthDay: [15] }), "de", de)).toBe("Jeden Monat am 15.");
    expect(describeRecurrence(rule({ frequency: "yearly", byMonth: [1, 7] }), "de", de)).toBe("Jedes Jahr im Januar und Juli");
  });

  it("adds count and end date", () => {
    expect(describeRecurrence(rule({ count: 10 }), "en", en)).toBe("Every week, 10 times");
    expect(describeRecurrence(rule({ count: 1 }), "de", de)).toBe("Jede Woche, einmal");
    const until = rule({ byDay: ["FR"], until: { kind: "date", date: "2026-12-31" } });
    expect(describeRecurrence(until, "en", en)).toBe("Every week on Friday, until Thu, Dec 31, 2026");
    expect(describeRecurrence(until, "de", de)).toBe("Jede Woche am Freitag, bis Do., 31. Dez. 2026");
  });

  it("shows the rule as written where words would guess", () => {
    const setPos = rule({ frequency: "monthly", byDay: ["MO", "TU", "WE", "TH", "FR"], bySetPos: [-1], rule: "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1" });
    expect(describeRecurrence(setPos, "en", en)).toBe(
      "Every month on Monday, Tuesday, Wednesday, Thursday, and Friday (rule: FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1)",
    );
    expect(describeRecurrence(rule({ frequency: "other", rule: "FREQ=HOURLY;INTERVAL=4" }), "en", en)).toBe(
      "Repeats by the rule FREQ=HOURLY;INTERVAL=4",
    );
    expect(describeRecurrence(rule({ frequency: "monthly", byDay: ["7MO"], rule: "FREQ=MONTHLY;BYDAY=7MO" }), "en", en)).toBe(
      "Every month (rule: FREQ=MONTHLY;BYDAY=7MO)",
    );
    expect(describeRecurrence(rule({ rule: "FREQ=WEEKLY;BYHOUR=9" }), "de", de)).toBe("Jede Woche (Regel: FREQ=WEEKLY;BYHOUR=9)");
  });
});

describe("kinds of calendar messages", () => {
  const invite = (method: string | null) => ({ method, events: [], problems: [] });

  it("tells invitations, updates, cancellations, answers and plain calendar files apart", () => {
    expect(inviteKind(invite("REQUEST"), makeCalendarEvent())).toBe("request");
    expect(inviteKind(invite("REQUEST"), makeCalendarEvent({ sequence: 2 }))).toBe("update");
    expect(inviteKind(invite("CANCEL"), makeCalendarEvent())).toBe("cancel");
    expect(inviteKind(invite("REQUEST"), makeCalendarEvent({ cancelled: true }))).toBe("cancel");
    expect(inviteKind(invite("REPLY"), makeCalendarEvent())).toBe("reply");
    expect(inviteKind(invite("COUNTER"), undefined)).toBe("counter");
    expect(inviteKind(invite(null), undefined)).toBe("publish");
    expect(inviteKind(invite("PUBLISH"), undefined)).toBe("publish");
    expect(inviteKind(invite("REFRESH"), undefined)).toBe("other");
  });
});

describe("hints about mailto links", () => {
  const notices: MailtoNotice[] = [
    { kind: "bcc_present" },
    { kind: "header_ignored", name: "from" },
    { kind: "duplicate_ignored", field: "subject" },
    { kind: "address_invalid", value: "not-an-address" },
    { kind: "message_id_invalid", value: "<x" },
    { kind: "truncated", field: "body" },
    { kind: "invalid_encoding", field: "subject" },
    { kind: "control_characters_removed", field: "in_reply_to" },
    { kind: "notices_omitted", count: 3 },
  ];

  it("has a text for every kind in both languages", () => {
    expect(notices.map((notice) => mailtoNoticeText(notice, en))).toEqual([
      "The link adds blind copies (Bcc). Check them before you send.",
      "The link tried to set “from”. That was ignored.",
      "The link gave “Subject” more than once; the first one is used.",
      "“not-an-address” is not a valid address and was left out.",
      "The link refers to a message that is not valid (“<x”); that reference was left out.",
      "“Text” was too long and was shortened.",
      "“Subject” contained characters that could not be read; they were replaced.",
      "Control characters were removed from “In reply to”.",
      "3 more hints are not shown.",
    ]);
    for (const notice of notices) expect(mailtoNoticeText(notice, de)).not.toMatch(/extras\./);
    expect(mailtoNoticeText({ kind: "truncated", field: "to" }, de)).toBe("„An“ war zu lang und wurde gekürzt.");
  });
});

// Every value the core can send has a text in both languages (the lists fail to compile when the
// core adds a value, like ERROR_CODES in services/ipc.ts).
const PROBLEM_CODES = [
  "not_calendar",
  "no_event",
  "too_large",
  "too_many_events",
  "truncated",
  "broken_structure",
  "broken_line",
  "line_too_long",
  "malformed_text",
  "bad_date",
  "bad_duration",
  "bad_recurrence",
  "end_before_start",
  "missing_start",
  "unknown_time_zone",
  "internal",
] as const satisfies readonly CalendarProblemCode[];
const STATUSES = ["needs_action", "accepted", "declined", "tentative", "delegated"] as const satisfies readonly ParticipationStatus[];
const FIELDS = ["to", "cc", "bcc", "subject", "body", "in_reply_to"] as const satisfies readonly MailtoField[];
const KINDS = ["request", "update", "cancel", "reply", "counter", "publish", "other"] as const satisfies readonly InviteKind[];
type Complete<All, Listed> = [Exclude<All, Listed>] extends [never] ? true : false;
const complete: [
  Complete<CalendarProblemCode, (typeof PROBLEM_CODES)[number]>,
  Complete<ParticipationStatus, (typeof STATUSES)[number]>,
  Complete<MailtoField, (typeof FIELDS)[number]>,
] = [true, true, true];

describe("texts for every value of the core", () => {
  it("names every calendar problem, answer, mailto field and kind of calendar message", () => {
    expect(complete).toEqual([true, true, true]);
    for (const t of [en, de]) {
      for (const code of PROBLEM_CODES) expect(t(`extras.invite.problems.${code}`, { detail: "x" })).not.toBe("");
      for (const status of STATUSES) expect(t(`extras.invite.reply.${status}`, { name: "Lin" })).toContain("Lin");
      for (const field of FIELDS) expect(t(`extras.mailto.fields.${field}`)).not.toBe("");
      for (const kind of KINDS) expect(t(`extras.invite.kind.${kind}`)).not.toBe("");
    }
  });
});
