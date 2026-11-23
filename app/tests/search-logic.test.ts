import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi, type MockInstance } from "vitest";
import { splitHighlights } from "../src/components/HighlightedText";
import { handleSearchEvent } from "../src/features/search/events";
import { scopeOf, scopesFor } from "../src/features/search/SearchArea";
import { chipTerms, hasValue, monthFirstFor, rangeText, removeTerm, sortedProblems } from "../src/features/search/terms";
import { initialEntry } from "../src/features/threads/ThreadPane";
import { visibleAccounts } from "../src/features/unified/UnifiedInbox";
import type { SearchTermInfo, ThreadEntry } from "../src/services/ipc";
import { makeAccount, makeMessage } from "./support/core";

describe("search language helpers", () => {
  it.each([
    ["from:anna rechnung", { start: 0, end: 9 }, "rechnung"],
    ["rechnung from:anna", { start: 9, end: 18 }, "rechnung"],
    ["a  -has:attachment   b", { start: 3, end: 18 }, "a b"],
    ["only", { start: 0, end: 4 }, ""],
    ['subject:"zwei worte" x', { start: 0, end: 20 }, "x"],
  ])("removing a term from %j leaves %j", (query, range, expected) => {
    expect(removeTerm(query, range === undefined ? { start: 0, end: 0 } : range)).toBe(expected);
  });

  it("reads a range clipped to the text", () => {
    expect(rangeText("from:anna", { start: 5, end: 9 })).toBe("anna");
    expect(rangeText("abc", { start: 2, end: 99 })).toBe("c");
    expect(rangeText("abc", { start: -3, end: 1 })).toBe("a");
  });

  it("shows filters as chips, free text not", () => {
    const term = (kind: SearchTermInfo["kind"]): SearchTermInfo => ({
      kind,
      negated: false,
      range: { start: 0, end: 1 },
      valueRange: { start: 0, end: 1 },
      alternative: false,
    });
    expect(chipTerms([term("text"), term("from"), term("has_attachment")]).map((t) => t.kind)).toEqual([
      "from",
      "has_attachment",
    ]);
    expect(hasValue("from")).toBe(true);
    expect(hasValue("is_unread")).toBe(false);
  });

  it("sorts problems by place and drops repeats", () => {
    const problems = sortedProblems([
      { code: "invalid_date", range: { start: 10, end: 20 } },
      { code: "unknown_filter", range: { start: 0, end: 4 } },
      { code: "invalid_date", range: { start: 10, end: 20 } },
    ]);
    expect(problems.map((p) => p.code)).toEqual(["unknown_filter", "invalid_date"]);
  });

  it("knows which regions write the month first", () => {
    expect(monthFirstFor("en-US")).toBe(true);
    expect(monthFirstFor("de-DE")).toBe(false);
    expect(monthFirstFor("en-GB")).toBe(false);
    expect(monthFirstFor("not a locale ###")).toBe(false);
  });
});

describe("highlights", () => {
  it("splits text into marked and plain pieces", () => {
    expect(splitHighlights("Rechnung März", [{ start: 0, end: 8 }])).toEqual([
      { text: "Rechnung", marked: true },
      { text: " März", marked: false },
    ]);
  });

  it("clips, sorts and merges bad ranges and honours the trim offset", () => {
    expect(
      splitHighlights(
        "abcdef",
        [
          { start: 4, end: 99 },
          { start: 1, end: 3 },
          { start: 2, end: 2 },
          { start: 2, end: 5 },
        ],
        0,
      ),
    ).toEqual([
      { text: "a", marked: false },
      { text: "bc", marked: true },
      { text: "de", marked: true },
      { text: "f", marked: true },
    ]);
    // The text was trimmed by two characters: ranges into the raw text shift.
    expect(splitHighlights("Hallo", [{ start: 2, end: 4 }], 2)).toEqual([
      { text: "Ha", marked: true },
      { text: "llo", marked: false },
    ]);
  });
});

describe("search scopes", () => {
  it("offers what the view has, narrowest first, and falls back to all accounts", () => {
    const folder = { accountId: "a", mailboxId: 3, groupId: null };
    expect(scopesFor(folder)).toEqual(["mailbox", "account", "all"]);
    expect(scopeOf("mailbox", folder)).toEqual({ kind: "mailbox", mailboxId: 3 });
    expect(scopeOf("account", folder)).toEqual({ kind: "account", accountId: "a" });
    expect(scopeOf("group", folder)).toEqual({ kind: "all" });
    const group = { accountId: null, mailboxId: null, groupId: "g" };
    expect(scopesFor(group)).toEqual(["group", "all"]);
    expect(scopeOf("group", group)).toEqual({ kind: "group", groupId: "g" });
    expect(scopeOf("mailbox", group)).toEqual({ kind: "all" });
  });
});

describe("conversations", () => {
  const entry = (id: number, unread: boolean): ThreadEntry => ({ message: makeMessage({ id, unread }), mailboxIds: [1] });

  it("opens the oldest unread message first, else the newest", () => {
    expect(initialEntry([entry(1, false), entry(2, true), entry(3, true)], 9)).toBe(2);
    expect(initialEntry([entry(1, false), entry(2, false)], 9)).toBe(2);
    expect(initialEntry([], 9)).toBe(9);
  });
});

describe("unified views", () => {
  it("leave hidden accounts out, and accounts outside the group", () => {
    const a = makeAccount({ id: "a" });
    const b = makeAccount({ id: "b" });
    const c = makeAccount({ id: "c" });
    expect(visibleAccounts([a, b, c], ["b"], null).map((x) => x.id)).toEqual(["a", "c"]);
    expect(visibleAccounts([a, b, c], ["b"], ["b", "c"]).map((x) => x.id)).toEqual(["c"]);
    expect(visibleAccounts([a, b], ["a", "b"], null)).toEqual([]);
  });
});

describe("core events for stage 7 lists", () => {
  function clientWith(keys: readonly (readonly unknown[])[]) {
    const queryClient = new QueryClient();
    for (const key of keys) queryClient.setQueryData(key, { pages: [], pageParams: [] });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    return { queryClient, invalidate };
  }
  const invalidated = (invalidate: MockInstance<QueryClient["invalidateQueries"]>): string[] =>
    invalidate.mock.calls.map((call) => JSON.stringify(call[0]?.queryKey));

  it("messagesChanged reloads the folder's conversations, open threads, searches and the unified inbox", async () => {
    vi.useFakeTimers();
    const { queryClient, invalidate } = clientWith([
      ["threads", "acc", 1],
      ["threads", "acc", 2],
      ["thread", "acc", 5, 1],
      ["search", { query: "x" }],
      ["unified", { kind: "all" }, "inbox"],
    ]);
    handleSearchEvent(queryClient, { type: "messagesChanged", accountId: "acc", mailboxId: 1 });
    await vi.runAllTimersAsync();
    vi.useRealTimers();
    const keys = invalidated(invalidate);
    expect(keys).toContain(JSON.stringify(["threads", "acc", 1]));
    expect(keys).not.toContain(JSON.stringify(["threads", "acc", 2]));
    expect(keys).toContain(JSON.stringify(["thread", "acc", 5, 1]));
    expect(keys).toContain(JSON.stringify(["search", { query: "x" }]));
    expect(keys).toContain(JSON.stringify(["unified", { kind: "all" }, "inbox"]));
  });

  it("inboxChanged reloads the unified inbox (a sorted inbox too)", async () => {
    vi.useFakeTimers();
    const { queryClient, invalidate } = clientWith([["unified", { kind: "group", groupId: "g" }, "inbox"]]);
    handleSearchEvent(queryClient, { type: "inboxChanged", accountId: null });
    await vi.runAllTimersAsync();
    vi.useRealTimers();
    expect(invalidated(invalidate)).toContain(JSON.stringify(["unified", { kind: "group", groupId: "g" }, "inbox"]));
  });

  it("accountsChanged reloads groups, hidden accounts and identities", () => {
    const { queryClient, invalidate } = clientWith([]);
    handleSearchEvent(queryClient, { type: "accountsChanged" });
    const keys = invalidated(invalidate);
    for (const key of [["accountGroups"], ["hiddenAccounts"], ["identities"]]) expect(keys).toContain(JSON.stringify(key));
  });
});
