import { describe, expect, it } from "vitest";
import { buildInboxRows, inboxPageItems, sectionOfDate, type InboxListRow } from "../src/features/comfort/inboxRows";
import { minuteToTime, timeToMinute, withNotificationDefaults } from "../src/features/comfort/NotificationSettings";
import { rowMarks } from "../src/features/comfort/RowMarks";
import {
  formatSnoozeTime,
  parseLocalDateTime,
  snoozePresets,
  snoozeTimeProblem,
  toLocalDateTimeValue,
} from "../src/features/comfort/snooze";
import { de } from "../src/i18n/de";
import { en } from "../src/i18n/en";
import type {
  BundleSummary,
  CategoryReason,
  DateSection,
  InboxPage,
  MessageListItem,
  RuleSkipReason,
} from "../src/services/ipc";
import { makeMessage } from "./support/core";

const TODAY: DateSection = { kind: "today", start: new Date(2026, 10, 23).getTime() };
const YESTERDAY: DateSection = { kind: "yesterday", start: new Date(2026, 10, 22).getTime() };

function summary(category: "notification" | "newsletter", section: DateSection, count: number, unread = 0): BundleSummary {
  return {
    key: { category, section: section.start },
    count,
    unread,
    senders: [{ sender: { name: "GitHub", email: "notifications@github.com" }, count, unread }],
  };
}

const ids = (items: readonly MessageListItem[]) => new Set(items.map((item) => item.id));

function describeRows(rows: readonly InboxListRow[]): string[] {
  return rows.map((row) => {
    switch (row.kind) {
      case "block":
        return `[${row.block}]`;
      case "section":
        return `# ${row.section.kind}`;
      case "message":
        return `${row.item.subject}${row.bundle ? " (in bundle)" : ""}`;
      case "bundle":
        return `bundle ${row.bundle.category} ${String(row.bundle.summary.count)} ${row.expanded ? "open" : "closed"}`;
      case "showAll":
        return `show all ${String(row.total)}`;
    }
  });
}

describe("sorted inbox rows", () => {
  const pinned = makeMessage({ id: 900, subject: "Pinned one", pinned: true });
  const back = makeMessage({ id: 901, subject: "Back one", snooze: { until: 1, returned: true } });
  const vip = makeMessage({ id: 800, subject: "From the boss", important: true });
  const person = makeMessage({ id: 700, subject: "Lunch?" });
  const n1 = makeMessage({ id: 600, subject: "CI failed", category: "notification" });
  const n2 = makeMessage({ id: 599, subject: "CI passed", category: "notification" });
  const n3 = makeMessage({ id: 400, subject: "Review requested", category: "notification" });
  const news = makeMessage({ id: 500, subject: "Weekly digest", category: "newsletter" });

  const first: InboxPage = {
    rows: [
      { kind: "message", item: vip, section: TODAY },
      { kind: "message", item: person, section: TODAY },
      { kind: "bundle", summary: summary("notification", TODAY, 2), section: TODAY, members: [n1] },
      { kind: "bundle", summary: summary("newsletter", TODAY, 1), section: TODAY, members: [news] },
    ],
    nextCursor: { before: 1, bundles: "{}" },
    top: { pinned: [pinned], returned: [back], priority: [vip], priorityTotal: 5, priorityUnread: 2 },
    categories: true,
  };
  const second: InboxPage = {
    rows: [
      { kind: "more", summary: summary("notification", TODAY, 2, 1), members: [n2] },
      { kind: "bundle", summary: summary("notification", YESTERDAY, 1), section: YESTERDAY, members: [n3] },
    ],
    nextCursor: null,
    top: null,
    categories: true,
  };
  const everything = [...inboxPageItems(first), ...inboxPageItems(second)];

  it("puts pinned, returned and priority on top, each once, then the dated rows", () => {
    const rows = buildInboxRows([first, second], new Set(), ids(everything));
    expect(describeRows(rows)).toEqual([
      "[pinned]",
      "Pinned one",
      "[returned]",
      "Back one",
      "[priority]",
      "From the boss",
      "show all 5",
      "# today",
      // The boss is in the priority block; not repeated below.
      "Lunch?",
      // The second page's `more` extends the first page's bundle where it stands.
      "bundle notification 2 closed",
      // A bundle with a single message is a plain row.
      "Weekly digest",
      "# yesterday",
      "Review requested",
    ]);
  });

  it("an expanded bundle shows its members below it, from every page", () => {
    const rows = buildInboxRows([first, second], new Set([`notification-${String(TODAY.start)}`]), ids(everything));
    const names = describeRows(rows);
    const at = names.indexOf("bundle notification 2 open");
    expect(names.slice(at, at + 3)).toEqual(["bundle notification 2 open", "CI failed (in bundle)", "CI passed (in bundle)"]);
    // The summary of the later page replaced the first one.
    const bundle = rows.find((row) => row.kind === "bundle");
    expect(bundle?.kind === "bundle" && bundle.bundle.summary.unread).toBe(1);
  });

  it("leaves out messages an action is moving away, and a bundle with none left", () => {
    const visible = ids(everything.filter((item) => item.id !== n1.id && item.id !== n2.id && item.id !== pinned.id));
    const names = describeRows(buildInboxRows([first, second], new Set(), visible));
    expect(names).not.toContain("[pinned]");
    expect(names.some((name) => name.startsWith("bundle notification"))).toBe(false);
  });

  it("without categories the core sends plain rows and no blocks besides pins and returns", () => {
    const plain: InboxPage = {
      rows: [
        { kind: "message", item: person, section: TODAY },
        { kind: "message", item: n3, section: YESTERDAY },
      ],
      nextCursor: null,
      top: { pinned: [], returned: [], priority: [], priorityTotal: 0, priorityUnread: 0 },
      categories: false,
    };
    expect(describeRows(buildInboxRows([plain], new Set(), ids(inboxPageItems(plain))))).toEqual([
      "# today",
      "Lunch?",
      "# yesterday",
      "Review requested",
    ]);
  });

  it("lists every message of a page once for the list, blocks first", () => {
    expect(inboxPageItems(first).map((item) => item.id)).toEqual([900, 901, 800, 700, 600, 500]);
  });

  it("turns the core's month sections into the list's sections in the local zone", () => {
    expect(sectionOfDate({ kind: "month", start: new Date(2026, 2, 1).getTime() })).toEqual({ kind: "month", year: 2026, month: 2 });
    expect(sectionOfDate(TODAY)).toEqual({ kind: "today" });
  });
});

describe("snooze times", () => {
  const times = { laterToday: 10, tomorrow: 20, weekend: 30, nextWeek: 40 };

  it("offers later today only while the core does", () => {
    expect(snoozePresets(times).map((p) => p.kind)).toEqual(["laterToday", "tomorrow", "weekend", "nextWeek"]);
    expect(snoozePresets({ ...times, laterToday: null }).map((p) => p.kind)).toEqual(["tomorrow", "weekend", "nextWeek"]);
  });

  it("reads a local date and time and rejects incomplete or impossible ones", () => {
    expect(parseLocalDateTime("2026-11-24T08:30")).toBe(new Date(2026, 10, 24, 8, 30).getTime());
    expect(parseLocalDateTime("2026-11-24")).toBeNull();
    expect(parseLocalDateTime("2026-02-31T08:00")).toBeNull();
    expect(parseLocalDateTime("")).toBeNull();
    const at = new Date(2026, 6, 4, 23, 59).getTime();
    expect(parseLocalDateTime(toLocalDateTimeValue(at))).toBe(at);
  });

  it("checks the core's bounds: a minute ahead at least, five years at most", () => {
    const now = new Date(2026, 10, 23, 12, 0).getTime();
    expect(snoozeTimeProblem(null, now)).toBe("invalid");
    expect(snoozeTimeProblem(now + 30_000, now)).toBe("tooSoon");
    expect(snoozeTimeProblem(now - 60_000, now)).toBe("tooSoon");
    expect(snoozeTimeProblem(now + 60_000, now)).toBeNull();
    expect(snoozeTimeProblem(new Date(2031, 10, 23, 12, 0).getTime(), now)).toBeNull();
    expect(snoozeTimeProblem(new Date(2031, 10, 23, 12, 1).getTime(), now)).toBe("tooFar");
  });

  it("names the weekday and time, the year only when it differs", () => {
    const now = new Date(2026, 10, 23, 12, 0).getTime();
    const same = formatSnoozeTime(new Date(2026, 10, 24, 8, 0).getTime(), now, "en");
    expect(same).toContain("Tue");
    expect(same).not.toContain("2026");
    expect(formatSnoozeTime(new Date(2027, 0, 4, 8, 0).getTime(), now, "en")).toContain("2027");
  });
});

describe("row marks and notification settings", () => {
  it("marks a row important, pinned, snoozed or back, and as a task", () => {
    expect(rowMarks(makeMessage())).toEqual([]);
    expect(
      rowMarks(makeMessage({ important: true, pinned: true, snooze: { until: 1, returned: true }, task: { done: false } })),
    ).toEqual(["important", "pinned", "returned", "task"]);
    expect(rowMarks(makeMessage({ snooze: { until: 1, returned: false }, task: { done: true } }))).toEqual([
      "snoozed",
      "taskDone",
    ]);
  });

  it("fills fields an older store left out with the core's defaults", () => {
    const full = withNotificationDefaults({ enabled: false, categories: { newsletters: true } });
    expect(full.enabled).toBe(false);
    expect(full.categories).toEqual({ people: true, priority: true, notifications: false, newsletters: true });
    expect(full.badge).toBe("inboxUnread");
  });

  it("converts quiet hours between minutes and times", () => {
    expect(minuteToTime(22 * 60)).toBe("22:00");
    expect(minuteToTime(7 * 60 + 5)).toBe("07:05");
    expect(timeToMinute("23:59")).toBe(1439);
    expect(timeToMinute("24:00")).toBeNull();
    expect(timeToMinute("7:00")).toBeNull();
  });
});

describe("texts of stage 9", () => {
  // Every reason the core can give: a missing one fails to compile here.
  const REASONS: Record<Exclude<CategoryReason["kind"], "userRule">, true> = {
    autoSubmitted: true,
    autoReply: true,
    report: true,
    knownSystem: true,
    ownMessage: true,
    replyToOwn: true,
    discussionList: true,
    mailingList: true,
    precedence: true,
    campaign: true,
    correspondent: true,
    unsubscribe: true,
    mailingService: true,
    noReplyAddress: true,
    automatedAddress: true,
    newsletterAddress: true,
    noBulkSignals: true,
    localRule: true,
  };
  const SKIPS: Record<RuleSkipReason, true> = { read_only: true, missing_mailbox: true, no_message_id: true, failed: true };

  it("every category reason, sender key and skipped rule has its text in both languages", () => {
    for (const language of [en, de]) {
      const reasons = language.comfort.reasons as Record<string, unknown>;
      for (const kind of Object.keys(REASONS)) expect(typeof reasons[kind]).toBe("string");
      for (const key of ["address", "domain", "list_id"] as const) {
        expect(language.comfort.reasons.userRule[key]).toBeTruthy();
        expect(language.comfort.keys[key]).toBeTruthy();
      }
      for (const reason of Object.keys(SKIPS)) {
        expect((language.comfort.rules.skipped as Record<string, string>)[reason]).toBeTruthy();
      }
    }
  });

  it("the notification texts keep the core's {count} placeholder", () => {
    expect(en.comfort.notificationTexts.newMessages).toContain("{count}");
    expect(de.comfort.notificationTexts.newMessages).toContain("{count}");
  });
});
