import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  buildRows,
  dayBounds,
  firstDayOfWeek,
  formatListDate,
  sectionOf,
} from "../src/features/messages/rows";
import { INBOX_PAGE_SIZE } from "../src/features/comfort/listSources";
import { IDLE, makeAccount, makeMailbox, makeMessage, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { emitCore, emitCoreBurst } from "./support/events";
import { renderApp } from "./support/render";

const ACCOUNT = makeAccount();
/** Stage 9: the inbox is the sorted inbox (`listInbox`), paged like a folder. */
const INBOX_SCOPE = { kind: "mailbox", mailboxId: 1 } as const;
const PAGE = INBOX_PAGE_SIZE;
/** Height of the list's viewport in these tests (jsdom has no layout). */
const VIEWPORT = 600;

function withViewport() {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(VIEWPORT);
}

async function scrollTo(list: HTMLElement, top: number) {
  list.scrollTop = top;
  fireEvent.scroll(list);
  // The virtualizer re-renders in a layout effect; give React a turn.
  await waitFor(() => {
    expect(list.scrollTop).toBe(top);
  });
}

function bigInbox(count: number) {
  const mailboxes = standardMailboxes().map((m) => (m.id === 1 ? { ...m, total: count } : m));
  return { accounts: [ACCOUNT], mailboxes, messages: { 1: makeMessages(count) } };
}

describe("message list", () => {
  it("renders only the visible rows of 10,000 messages and tells screen readers the total", async () => {
    withViewport();
    const core = mockCore(bigInbox(10_000));
    renderApp();

    const list = await screen.findByRole("listbox", { name: "Messages in Inbox" });
    const options = within(list).getAllByRole("option");
    // 600 px of 44 px rows plus overscan, not 200 (one page) and never 10,000.
    expect(options.length).toBeGreaterThan(10);
    expect(options.length).toBeLessThan(40);
    expect(options[0]?.getAttribute("aria-setsize")).toBe("10000");
    expect(options[0]?.getAttribute("aria-posinset")).toBe("1");
    // One page was asked for, from the start.
    expect(core.callsOf("list_inbox")).toEqual([{ scope: INBOX_SCOPE, cursor: null, limit: PAGE }]);
  });

  it("loads the next page with the cursor of the last one when scrolling near the end", async () => {
    withViewport();
    const inbox = bigInbox(10_000);
    const core = mockCore(inbox);
    // The served messages themselves: a second `makeMessages` could have other (clock) ids.
    const messages = inbox.messages[1];
    renderApp();
    const list = await screen.findByRole("listbox");

    await scrollTo(list, PAGE * 44 - VIEWPORT);

    await waitFor(() => {
      expect(core.callsOf("list_inbox")).toHaveLength(2);
    });
    const lastOfFirstPage = messages[PAGE - 1];
    expect(core.callsOf("list_inbox")[1]).toEqual({
      scope: INBOX_SCOPE,
      cursor: { before: lastOfFirstPage?.id, bundles: null },
      limit: PAGE,
    });
    // Message 201 is now part of the list (rendered once scrolled to).
    await scrollTo(list, PAGE * 44);
    expect(await within(list).findByText("Message 201")).toBeTruthy();
  });

  it("reloads when the core says the messages changed, and never on its own", async () => {
    withViewport();
    const core = mockCore(bigInbox(50));
    renderApp();
    await screen.findByText("Message 1");
    const before = core.callsOf("list_inbox").length;

    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(core.callsOf("list_inbox")).toHaveLength(before);

    core.setMessages(1, [makeMessage({ subject: "Fresh from the server", unread: true }), ...makeMessages(50)]);
    await emitCore({ type: "messagesChanged", accountId: ACCOUNT.id, mailboxId: 1 });

    expect(await screen.findByText("Fresh from the server")).toBeTruthy();
    expect(core.callsOf("list_inbox").at(-1)).toEqual({ scope: INBOX_SCOPE, cursor: null, limit: PAGE });
  });

  it("merges a burst of events into one reload", async () => {
    withViewport();
    const core = mockCore(bigInbox(50));
    renderApp();
    await screen.findByText("Message 1");
    const before = core.callsOf("list_inbox").length;

    await emitCoreBurst([
      { type: "messagesChanged", accountId: ACCOUNT.id, mailboxId: 1 },
      { type: "mailboxesChanged", accountId: ACCOUNT.id },
      { type: "messagesChanged", accountId: ACCOUNT.id, mailboxId: 1 },
    ]);

    await waitFor(() => {
      expect(core.callsOf("list_inbox").length).toBeGreaterThan(before);
    });
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(core.callsOf("list_inbox").length - before).toBe(1);
  });

  it("ignores events for other folders", async () => {
    withViewport();
    const core = mockCore(bigInbox(10));
    renderApp();
    await screen.findByText("Message 1");
    const before = core.callsOf("list_inbox").length;

    await emitCore({ type: "messagesChanged", accountId: ACCOUNT.id, mailboxId: 5 });
    await new Promise((resolve) => setTimeout(resolve, 100));

    expect(core.callsOf("list_inbox")).toHaveLength(before);
  });

  it("keeps the visible rows in place when new mail arrives above them", async () => {
    withViewport();
    const inbox = bigInbox(100);
    const core = mockCore(inbox);
    renderApp();
    const list = await screen.findByRole("listbox");
    // 8 px padding, 32 px "Today", then 44 px rows: 440 is 4 px into message 10.
    await scrollTo(list, 440);

    // The same messages again, under two new ones. Not a second `makeMessages(100)`: its ids
    // come from the clock (sort date in seconds), so whenever a second boundary passed between
    // the two calls (more often the slower the machine) every message got a new id, the rows
    // new keys, and the row to keep in place was gone.
    core.setMessages(1, [makeMessage({ subject: "New A" }), makeMessage({ subject: "New B" }), ...inbox.messages[1]]);
    await emitCore({ type: "messagesChanged", accountId: ACCOUNT.id, mailboxId: 1 });

    await waitFor(() => {
      expect(list.scrollTop).toBe(440 + 2 * 44);
    });
  });

  it("shows a row as DESIGN.md says: unread mark, sender, subject, attachment, flag, time", async () => {
    withViewport();
    const today = new Date();
    today.setHours(9, 5, 0, 0);
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: {
        1: [
          makeMessage({
            date: today.getTime(),
            from: { name: "Grace Hopper", email: "grace@example.org" },
            subject: "Compiler notes",
            unread: true,
            flagged: true,
            hasAttachments: true,
          }),
          makeMessage({ date: today.getTime() - 60_000, from: null, subject: "", unread: false }),
        ],
      },
    });
    renderApp();

    const [first, second] = await screen.findAllByRole("option");
    const full = new Intl.DateTimeFormat("en", { dateStyle: "full", timeStyle: "short" }).format(today);
    // Screen readers hear what the row shows, in reading order, with the full date.
    expect(first?.getAttribute("aria-label")).toBe(`Unread, Grace Hopper, Compiler notes, Has attachment, Flagged, ${full}`);
    expect(within(first as HTMLElement).getByText("Grace Hopper").className).toContain("font-semibold");
    expect(within(first as HTMLElement).getByText("Compiler notes")).toBeTruthy();
    expect(within(first as HTMLElement).getByText("9:05 AM").tagName).toBe("TIME");
    expect(first?.querySelector(".lucide-paperclip")).toBeTruthy();
    expect(first?.querySelector(".lucide-flag")).toBeTruthy();
    expect(second?.textContent).toContain("(Unknown sender)");
    expect(second?.textContent).toContain("(No subject)");
    expect(second?.getAttribute("aria-label")).not.toContain("Unread");
    expect(within(second as HTMLElement).getByText("(Unknown sender)").className).toContain("font-regular");
  });

  it("shows the preview muted after the subject and lets it describe the row", async () => {
    withViewport();
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: {
        1: [
          makeMessage({ subject: "Lunch", preview: "  Shall we meet on Friday at noon?  " }),
          makeMessage({ subject: "Not parsed yet", preview: null }),
        ],
      },
    });
    renderApp();

    const [first, second] = await screen.findAllByRole("option");
    const preview = within(first as HTMLElement).getByTestId("row-preview");
    expect(preview.textContent).toBe("Shall we meet on Friday at noon?");
    expect(preview.className).toContain("text-tertiary");
    expect(preview.className).toContain("truncate");
    expect(first?.getAttribute("aria-describedby")).toBe(preview.id);
    // The name stays short; the preview is read after it.
    expect(first?.getAttribute("aria-label")).not.toContain("Friday");
    expect(within(second as HTMLElement).queryByTestId("row-preview")).toBeNull();
    expect(second?.hasAttribute("aria-describedby")).toBe(false);
  });

  it("Sent shows the recipients instead of the sender", async () => {
    withViewport();
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: {
        2: [makeMessage({ mailboxId: 2, to: [{ name: null, email: "team@example.org" }], subject: "Status" })],
      },
    });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Sent" }));

    const option = await screen.findByRole("option");
    expect(option.textContent).toContain("To: team@example.org");
  });

  it("arrow keys move the selection, the focus stays visible on it", async () => {
    withViewport();
    mockCore(bigInbox(30));
    const { user } = renderApp();
    const list = await screen.findByRole("listbox");

    await user.tab();
    await user.tab();
    list.focus();
    await waitFor(() => {
      expect(list.getAttribute("aria-activedescendant")).toBeTruthy();
    });
    const selected = () => document.getElementById(list.getAttribute("aria-activedescendant") ?? "");
    expect(selected()?.textContent).toContain("Message 1");
    expect(selected()?.getAttribute("aria-selected")).toBe("true");

    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(selected()?.textContent).toContain("Message 3");
    await user.keyboard("{ArrowUp}");
    expect(selected()?.textContent).toContain("Message 2");
    await user.keyboard("{End}");
    expect(selected()?.textContent).toContain("Message 30");
    await user.keyboard("{Home}");
    expect(selected()?.textContent).toContain("Message 1");
    expect(within(list).getAllByRole("option").filter((o) => o.getAttribute("aria-selected") === "true")).toHaveLength(1);
  });

  it("a click selects the clicked row", async () => {
    withViewport();
    mockCore(bigInbox(30));
    const { user } = renderApp();
    const list = await screen.findByRole("listbox");

    await user.click(await within(list).findByText("Message 4"));

    const active = document.getElementById(list.getAttribute("aria-activedescendant") ?? "");
    expect(active?.textContent).toContain("Message 4");
    expect(document.activeElement).toBe(list);
  });

  it("shows a skeleton only while the first page loads", async () => {
    mockCore({ ...bigInbox(10), hang: ["list_inbox"] });
    renderApp();

    const skeleton = await screen.findByTestId("list-skeleton");
    expect(within(skeleton).getByRole("status").textContent).toBe("Loading messages");
    expect(screen.queryByRole("option")).toBeNull();
  });

  it("an empty folder says so; during the first sync it says that instead", async () => {
    mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes(), messages: {} });
    const { unmount } = renderApp();
    expect(await screen.findByRole("heading", { name: "This folder is empty" })).toBeTruthy();
    unmount();

    mockCore({
      accounts: [makeAccount({ status: { state: "syncing", progress: null, error: null, retryAt: null, lastSyncAt: null } })],
      mailboxes: standardMailboxes(),
      messages: {},
    });
    renderApp();
    expect(await screen.findByRole("heading", { name: "Loading your mail" })).toBeTruthy();
  });

  it("a failed load shows the error with a way to retry", async () => {
    mockCore({ ...bigInbox(10), failures: { list_inbox: { code: "database", message: "x", detail: "SQLITE_BUSY" } } });
    renderApp();

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The messages of this folder could not be loaded.");
    expect(alert.textContent).toContain("SQLITE_BUSY");
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("groups by date with section labels", async () => {
    withViewport();
    const now = new Date();
    const yesterday = new Date(now);
    yesterday.setDate(now.getDate() - 1);
    yesterday.setHours(12, 0, 0, 0);
    const lastYear = new Date(now.getFullYear() - 1, 2, 15, 12);
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: {
        1: [
          makeMessage({ date: now.getTime(), subject: "Today's" }),
          makeMessage({ date: yesterday.getTime(), subject: "Yesterday's" }),
          makeMessage({ date: lastYear.getTime(), subject: "Old" }),
        ],
      },
    });
    renderApp();

    await screen.findByText("Old");
    const labels = screen.getAllByTestId("list-section").map((s) => s.textContent);
    expect(labels).toEqual(["Today", "Yesterday", `March ${String(now.getFullYear() - 1)}`]);
  });
});

describe("date sections and the time column", () => {
  // Wednesday, 18 November 2026, 15:00 local time.
  const now = new Date(2026, 10, 18, 15, 0);
  const at = (day: number, hour = 10) => new Date(2026, 10, day, hour).getTime();

  it("sorts into Today, Yesterday, This week and months (week from Monday)", () => {
    const bounds = dayBounds(now, 1);
    expect(sectionOf(at(18, 0), bounds)).toEqual({ kind: "today" });
    expect(sectionOf(at(17, 23), bounds)).toEqual({ kind: "yesterday" });
    expect(sectionOf(at(16), bounds)).toEqual({ kind: "thisWeek" });
    expect(sectionOf(at(15), bounds)).toEqual({ kind: "month", year: 2026, month: 10 });
    expect(sectionOf(new Date(2026, 9, 31).getTime(), bounds)).toEqual({ kind: "month", year: 2026, month: 9 });
    // A sender's clock in the future: its own month, never "Today".
    expect(sectionOf(new Date(2026, 11, 2).getTime(), bounds)).toEqual({ kind: "month", year: 2026, month: 11 });
  });

  it("a week starting on Sunday moves the boundary", () => {
    const bounds = dayBounds(now, 7);
    expect(sectionOf(at(15), bounds)).toEqual({ kind: "thisWeek" });
    expect(sectionOf(at(14), bounds)).toEqual({ kind: "month", year: 2026, month: 10 });
  });

  it("knows the first day of the week per language, Monday when unsure", () => {
    expect(firstDayOfWeek("de")).toBe(1);
    expect([1, 7]).toContain(firstDayOfWeek("en"));
    expect(firstDayOfWeek("not a locale!")).toBe(1);
  });

  it("inserts a section row before each new section", () => {
    const bounds = dayBounds(now, 1);
    const rows = buildRows(
      [
        makeMessage({ id: 5, date: at(18) }),
        makeMessage({ id: 4, date: at(18, 9) }),
        makeMessage({ id: 3, date: at(17) }),
        makeMessage({ id: 2, date: at(2) }),
        makeMessage({ id: 2, date: at(2) }),
      ],
      bounds,
    );
    expect(rows.map((r) => (r.kind === "section" ? r.section.kind : r.item.id))).toEqual([
      "today",
      5,
      4,
      "yesterday",
      3,
      "month",
      2,
    ]);
    expect(rows.filter((r) => r.kind === "message").map((r) => r.position)).toEqual([0, 1, 2, 3]);
  });

  it("time column: today the time, this year day and month, earlier the date", () => {
    const bounds = dayBounds(now, 1);
    expect(formatListDate(at(18, 9), bounds, "en")).toBe("9:00 AM");
    expect(formatListDate(at(18, 9), bounds, "de")).toBe("09:00");
    expect(formatListDate(at(3), bounds, "en")).toBe("Nov 3");
    expect(formatListDate(at(3), bounds, "de")).toBe("3. Nov.");
    expect(formatListDate(new Date(2025, 0, 9).getTime(), bounds, "de")).toBe("09.01.25");
  });
});

describe("folders in the list header", () => {
  it("the header names the folder and its account", async () => {
    withViewport();
    mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes(), messages: {} });
    renderApp();

    expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeTruthy();
    expect(await within(screen.getByRole("banner")).findByText("Ada Lovelace (ada@vicemail.test)")).toBeTruthy();
  });

  it("a rail view whose folder the account lacks says so and points to the settings", async () => {
    mockCore({
      accounts: [{ ...ACCOUNT, status: IDLE }],
      mailboxes: [makeMailbox({ id: 1, path: "INBOX", role: "inbox", roleSource: "server" })],
    });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Archive" }));

    expect(await screen.findByRole("heading", { name: "No folder for “Archive”" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Open account settings" }));
    expect(await screen.findByTestId("account-detail")).toBeTruthy();
  });

  it("saved views without saved searches say how to make one instead of showing anything made up", async () => {
    mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes() });
    const { user } = renderApp();

    // Stage 9 gave Later, Tasks and Pinned their lists; stage 7 gave saved views theirs.
    await user.click(await screen.findByRole("button", { name: "Saved views" }));

    expect(await screen.findByRole("heading", { name: "No saved searches yet" })).toBeTruthy();
    expect(screen.getByText(/choose “Save search”/)).toBeTruthy();
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
