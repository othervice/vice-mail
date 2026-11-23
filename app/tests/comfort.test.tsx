import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { InboxPage, MessageCategoryInfo, MessageListItem } from "../src/services/ipc";
import { inboxSection, SNOOZE_TIMES } from "./support/comfort";
import { ipcError, makeAccount, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";
import { toolbarAction } from "./support/reading";
import { optionName } from "./support/names";

/** Read-only on purpose: pins, snoozes and tasks are local and must work without writes. */
const ADA = makeAccount({ writesEnabled: false });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

function setup(items: MessageListItem[] = makeMessages(5), extra: Options = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    ...extra,
  });
  return { core, ...renderApp() };
}

async function list(name = /^Messages in/) {
  return screen.findByRole("listbox", { name });
}

function row(listbox: HTMLElement, subject: string): HTMLElement {
  const option = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(`, ${subject},`));
  if (!option) throw new Error(`no row ${subject}`);
  return option;
}

async function open(user: ReturnType<typeof renderApp>["user"], subject: string) {
  const listbox = await list();
  await user.click(row(listbox, subject));
  return screen.findByRole("region", { name: "Message" });
}

function notices(): string[] {
  return screen.queryAllByTestId("notice").map((n) => n.textContent);
}

describe("the sorted inbox", () => {
  const pinned = makeMessage({ id: 900, subject: "Pinned one", pinned: true });
  const back = makeMessage({ id: 901, subject: "Back one", snooze: { until: 1, returned: true } });
  const vip = makeMessage({ id: 800, subject: "From the boss", important: true });
  const person = makeMessage({ id: 700, subject: "Lunch?" });
  const n1 = makeMessage({ id: 600, subject: "CI failed", category: "notification", unread: true });
  const n2 = makeMessage({ id: 599, subject: "CI passed", category: "notification" });
  const section = inboxSection(Date.now());
  const page: InboxPage = {
    rows: [
      { kind: "message", item: vip, section },
      { kind: "message", item: person, section },
      {
        kind: "bundle",
        summary: {
          key: { category: "notification", section: section.start },
          count: 2,
          unread: 1,
          senders: [{ sender: { name: "GitHub", email: "notifications@github.com" }, count: 2, unread: 1 }],
        },
        section,
        members: [n1, n2],
      },
    ],
    nextCursor: null,
    top: { pinned: [pinned], returned: [back], priority: [vip], priorityTotal: 4, priorityUnread: 1 },
    categories: true,
    conversations: [],
  };
  const all = [pinned, back, vip, person, n1, n2];

  it("puts pinned, returned and priority on top and bundles notifications into one row with chips", async () => {
    const { core } = setup(all, { comfort: { inboxPages: { 1: [page] }, priority: { 1: [vip] } } });
    const listbox = await list();

    expect(core.callsOf("list_inbox")[0]).toEqual({ scope: { kind: "mailbox", mailboxId: 1 }, cursor: null, limit: 200 });
    expect(within(listbox).getByTestId("inbox-block-pinned").textContent).toBe("Pinned");
    expect(within(listbox).getByTestId("inbox-block-returned").textContent).toBe("Back from Later");
    expect(within(listbox).getByTestId("inbox-block-priority").textContent).toBe("Priority");
    // The returned snooze carries its mark, also in the row's name.
    const backRow = row(listbox, "Back one");
    expect(within(backRow).getByTestId("row-mark-returned").textContent).toBe("Back");
    expect(optionName(backRow)).toContain("Back from Later");
    expect(optionName(row(listbox, "Pinned one"))).toContain("Pinned");
    expect(optionName(row(listbox, "From the boss"))).toContain("Important sender");
    // The boss stands once (priority block), the bundle is one row until unfolded.
    const subjects = within(listbox)
      .getAllByRole("option")
      .map((o) => optionName(o));
    expect(subjects.filter((label) => label.includes("From the boss"))).toHaveLength(1);
    expect(subjects.some((label) => label.includes("CI failed"))).toBe(false);
    // Screen readers hear "n of m" over the entries they can reach (messages, the bundle and
    // "Show all"), not over the members of closed bundles.
    expect(within(listbox).getAllByRole("option")[0]?.getAttribute("aria-setsize")).toBe(String(subjects.length));
    expect(within(listbox).getAllByRole("option").map((o) => o.getAttribute("aria-posinset"))).toEqual(
      subjects.map((_, i) => String(i + 1)),
    );

    // Decision 0031 (axe aria-required-children): the bundle is an entry of the list, no button,
    // named by what it shows plus its folded state (WCAG 2.5.3).
    const bundle = within(listbox).getByRole("option", { name: "Notifications, 2 · 1 new, GitHub 2, collapsed" });
    expect(within(listbox).queryAllByRole("button")).toEqual([]);
    expect(bundle.hasAttribute("aria-label")).toBe(false);
    expect(within(bundle).getAllByTestId("bundle-chip-name").map((chip) => chip.textContent)).toEqual(["GitHub"]);
    expect(within(bundle).getByTestId("bundle-count").textContent).toBe("2 · 1 new");
    expect(within(listbox).getByRole("option", { name: "Show all (4, 1 unread)" })).toBeTruthy();
  });

  it("the arrow keys reach the bundle and “Show all” like messages; Space runs them, the selection stays", async () => {
    const { user } = setup(all, { comfort: { inboxPages: { 1: [page] }, priority: { 1: [vip] } } });
    const listbox = await list();
    const active = () => document.getElementById(listbox.getAttribute("aria-activedescendant") ?? "");

    await user.click(row(listbox, "From the boss"));
    await user.keyboard("{ArrowDown}");
    expect(optionName(active())).toBe("Show all (4, 1 unread)");
    // On "Show all" nothing new is selected: the message stays selected and open.
    expect(row(listbox, "From the boss").getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{ArrowDown}");
    expect(optionName(active())).toContain(", Lunch?,");
    expect(row(listbox, "Lunch?").getAttribute("aria-selected")).toBe("true");
    await user.keyboard("j");
    expect(optionName(active())).toBe("Notifications, 2 · 1 new, GitHub 2, collapsed");
    await user.keyboard(" ");
    expect(optionName(active())).toBe("Notifications, 2 · 1 new, GitHub 2, expanded");
    expect(row(listbox, "Lunch?").getAttribute("aria-selected")).toBe("true");
    expect(row(listbox, "CI failed").getAttribute("aria-selected")).toBe("false");
    await user.keyboard("{ArrowDown}");
    expect(optionName(active())).toContain(", CI failed,");
    await user.keyboard("{ArrowUp}{ArrowUp}");
    expect(optionName(active())).toContain(", Lunch?,");
  });

  it("a click or Enter unfolds a bundle in place, without opening or selecting anything", async () => {
    const { core, user } = setup(all, { comfort: { inboxPages: { 1: [page] } } });
    const listbox = await list();
    const bundle = () => within(listbox).getByRole("option", { name: /^Notifications/ });

    await user.click(within(listbox).getByTestId("bundle-row"));
    expect(optionName(bundle())).toMatch(/, expanded$/);
    expect(row(listbox, "CI failed")).toBeTruthy();
    expect(row(listbox, "CI passed")).toBeTruthy();

    // The click put the list's cursor on the bundle: Enter folds it again.
    expect(listbox.getAttribute("aria-activedescendant")).toBe(bundle().id);
    await user.keyboard("{Enter}");
    expect(optionName(bundle())).toMatch(/, collapsed$/);
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    expect(core.callsOf("get_message")).toHaveLength(0);
  });

  it("“Show all” lists the important senders' messages and leads back", async () => {
    const { core, user } = setup(all, { comfort: { inboxPages: { 1: [page] }, priority: { 1: [vip] } } });
    const listbox = await list();

    await user.click(within(listbox).getByTestId("priority-show-all"));

    await waitFor(() => {
      expect(core.callsOf("list_inbox_filtered")).toEqual([
        { scope: { kind: "mailbox", mailboxId: 1 }, filter: { kind: "priority" }, cursor: null, limit: 200 },
      ]);
    });
    const filtered = await list();
    await waitFor(() => {
      // The way back is an entry of the list too, then the one message.
      expect(within(filtered).getAllByRole("option").map((o) => optionName(o).split(", ")[0])).toEqual(["Priority", "Grace Hopper"]);
    });
    await user.click(within(filtered).getByTestId("priority-back"));
    expect(await within(await list()).findByTestId("bundle-row")).toBeTruthy();
  });

  it("reloads when the core says the inbox changed, and never on its own", async () => {
    const { core } = setup();
    await list();
    const before = core.callsOf("list_inbox").length;
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(core.callsOf("list_inbox")).toHaveLength(before);

    await emitCore({ type: "inboxChanged", accountId: null });

    await waitFor(() => {
      expect(core.callsOf("list_inbox").length).toBeGreaterThan(before);
    });
  });
});

describe("snooze", () => {
  it("offers the core's times; the choice snoozes, moves nothing on the server, and Undo brings it back", async () => {
    const items = makeMessages(3);
    const first = items[0] as MessageListItem;
    const { core, user } = setup(items);
    const pane = await open(user, "Message 1");

    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Snooze"));

    const picker = await screen.findByTestId("snooze-picker");
    expect(within(picker).getByRole("heading").textContent).toBe("Snooze message");
    expect(within(picker).getAllByRole("button", { name: /Later today|Tomorrow|This weekend|Next week/ })).toHaveLength(4);
    await user.click(within(picker).getByTestId("snooze-tomorrow"));

    await waitFor(() => {
      expect(core.callsOf("snooze_messages")).toEqual([{ messageIds: [first.id], choice: { kind: "tomorrow" } }]);
    });
    expect(screen.queryByTestId("snooze-picker")).toBeNull();
    await waitFor(() => {
      expect(notices().some((text) => text.startsWith("Snoozed until"))).toBe(true);
    });
    // Local only: no action for the server, no move.
    expect(core.callsOf("apply_action")).toHaveLength(0);

    await user.click(screen.getByTestId("notice-undo"));
    await waitFor(() => {
      expect(core.callsOf("unsnooze_messages")).toEqual([{ messageIds: [first.id] }]);
    });
  });

  it("without “later today” in the evening the menu leaves it out", async () => {
    const { user } = setup(makeMessages(2), { comfort: { snoozeTimes: { ...SNOOZE_TIMES, laterToday: null } } });
    await open(user, "Message 1");
    await user.keyboard("b");
    const picker = await screen.findByTestId("snooze-picker");
    await within(picker).findByTestId("snooze-tomorrow");
    expect(within(picker).queryByTestId("snooze-laterToday")).toBeNull();
  });

  it("a time of one's own is checked first, then sent as such", async () => {
    const items = makeMessages(2);
    const first = items[0] as MessageListItem;
    const { core, user } = setup(items);
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    listbox.focus();
    await user.keyboard("b");
    const picker = await screen.findByTestId("snooze-picker");
    const input = within(picker).getByTestId("snooze-custom");

    fireEvent.change(input, { target: { value: "2020-01-01T08:00" } });
    await user.click(within(picker).getByTestId("snooze-custom-submit"));
    expect(within(picker).getByTestId("snooze-problem").textContent).toBe("Pick a time at least one minute from now.");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(core.callsOf("snooze_messages")).toHaveLength(0);

    const next = new Date();
    next.setDate(next.getDate() + 2);
    next.setHours(9, 15, 0, 0);
    const value = `${String(next.getFullYear())}-${String(next.getMonth() + 1).padStart(2, "0")}-${String(next.getDate()).padStart(2, "0")}T09:15`;
    fireEvent.change(input, { target: { value } });
    await user.click(within(picker).getByTestId("snooze-custom-submit"));

    await waitFor(() => {
      expect(core.callsOf("snooze_messages")).toEqual([{ messageIds: [first.id], choice: { kind: "at", at: next.getTime() } }]);
    });
  });

  it("a message without Message-ID says why it cannot be snoozed", async () => {
    const { user } = setup(makeMessages(2), { failures: { snooze_messages: ipcError("invalid_argument") } });
    await open(user, "Message 1");
    await user.keyboard("b");
    await user.click(await screen.findByTestId("snooze-tomorrow"));
    await waitFor(() => {
      expect(notices().some((text) => text.includes("Snoozing did not work.") && text.includes("no Message-ID"))).toBe(true);
    });
  });
});

describe("pins and tasks", () => {
  it("work from the keyboard on a read-only account and never ask the server to move anything", async () => {
    const items = makeMessages(3);
    const first = items[0] as MessageListItem;
    const { core, user } = setup(items);
    await open(user, "Message 1");

    await user.keyboard("p");
    await waitFor(() => {
      expect(core.callsOf("set_pinned")).toEqual([{ messageIds: [first.id], pinned: true }]);
    });
    await user.keyboard("t");
    await waitFor(() => {
      expect(core.callsOf("set_task")).toEqual([{ messageIds: [first.id], mark: { done: false } }]);
    });
    await waitFor(() => {
      expect(notices()).toEqual(expect.arrayContaining(["Pinned", "Marked as a task"]));
    });
    expect(core.callsOf("apply_action")).toHaveLength(0);
    expect(core.comfort.pinned.has(first.id)).toBe(true);
    expect(core.comfort.tasks.get(first.id)).toEqual({ done: false });
  });

  it("the toolbar takes a mark back and ticks a task off", async () => {
    const items = [makeMessage({ id: 50, subject: "Invoice", pinned: true, task: { done: false } })];
    const { core, user } = setup(items);
    const pane = await open(user, "Invoice");
    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });

    await user.click(await toolbarAction(user, toolbar, "Unpin"));
    await user.click(await toolbarAction(user, toolbar, "Mark task as done"));

    await waitFor(() => {
      expect(core.callsOf("set_pinned")).toEqual([{ messageIds: [50], pinned: false }]);
      expect(core.callsOf("set_task")).toEqual([{ messageIds: [50], mark: { done: true } }]);
    });
  });
});

describe("Later, Pinned and Tasks", () => {
  it("say so when nothing is marked", async () => {
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: "Later" }));
    expect(await screen.findByRole("heading", { name: "Nothing snoozed" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Pinned" }));
    expect(await screen.findByRole("heading", { name: "Nothing pinned" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Tasks" }));
    expect(await screen.findByRole("heading", { name: "No tasks" })).toBeTruthy();
  });

  it("list the marked messages of the account and act on them", async () => {
    const items = [
      makeMessage({ id: 30, subject: "Waiting", snooze: { until: Date.now() + 3_600_000, returned: false } }),
      makeMessage({ id: 20, subject: "Keep on top", pinned: true }),
      makeMessage({ id: 10, subject: "Pay the bill", task: { done: false } }),
    ];
    const { core, user } = setup(items);

    await user.click(await screen.findByRole("button", { name: "Later" }));
    const later = await list(/^Messages in Later/);
    await waitFor(() => {
      expect(within(later).getAllByRole("option")).toHaveLength(1);
    });
    expect(optionName(row(later, "Waiting"))).toContain("Snoozed");
    expect(core.callsOf("list_snoozed")).toEqual([{ scope: { kind: "account", accountId: ADA.id } }]);

    await user.click(screen.getByRole("button", { name: "Pinned" }));
    const pinnedList = await list(/^Messages in Pinned/);
    await waitFor(() => {
      expect(row(pinnedList, "Keep on top")).toBeTruthy();
    });

    await user.click(screen.getByRole("button", { name: "Tasks" }));
    const tasks = await list(/^Messages in Tasks/);
    await waitFor(() => {
      expect(row(tasks, "Pay the bill")).toBeTruthy();
    });
    // Open tasks by default; the filter stays above the list, also when it is empty.
    expect(core.callsOf("list_tasks")).toEqual([{ scope: { kind: "account", accountId: ADA.id }, filter: "open" }]);
    await user.click(row(tasks, "Pay the bill"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Mark task as done"));
    await waitFor(() => {
      expect(core.callsOf("set_task")).toEqual([{ messageIds: [10], mark: { done: true } }]);
    });
    expect(await screen.findByRole("heading", { name: "No tasks" })).toBeTruthy();

    const filter = screen.getByRole("group", { name: "Show tasks" });
    expect(within(filter).getByRole("button", { name: "Open" }).getAttribute("aria-pressed")).toBe("true");
    await user.click(within(filter).getByRole("button", { name: "Done" }));
    const done = await list(/^Messages in Tasks/);
    await waitFor(() => {
      expect(optionName(row(done, "Pay the bill"))).toContain("Task done");
    });
    expect(core.callsOf("list_tasks").at(-1)).toEqual({ scope: { kind: "account", accountId: ADA.id }, filter: "done" });
  });
});

describe("why here", () => {
  const news = makeMessage({ id: 77, subject: "Weekly digest", category: "newsletter" });
  const info: MessageCategoryInfo = {
    messageId: 77,
    decision: { category: "newsletter", reason: { kind: "mailingList", listId: "news.example.org" }, important: null },
    address: { kind: "address", value: "news@example.org" },
    domain: { kind: "domain", value: "example.org" },
    list: { kind: "list_id", value: "news.example.org" },
  };

  it("explains the category and remembers “always as …” and “important” for the chosen key", async () => {
    const { core, user } = setup([news], { comfort: { categories: { 77: info } } });
    const pane = await open(user, "Weekly digest");

    expect((await within(pane).findByTestId("category-current")).textContent).toBe("In Newsletters");
    const why = within(pane).getByRole("button", { name: "Why here?" });
    expect(why.getAttribute("aria-expanded")).toBe("false");
    await user.click(why);
    expect(within(pane).getByTestId("category-reason").textContent).toBe("A mailing list (news.example.org).");

    await user.click(within(pane).getByTestId("category-always-person"));
    await waitFor(() => {
      expect(core.callsOf("set_sender_category")).toEqual([
        { key: { kind: "address", value: "news@example.org" }, category: "person" },
      ]);
    });

    await user.click(within(pane).getByRole("radio", { name: "The list news.example.org" }));
    await user.click(within(pane).getByRole("switch", { name: "Mark news.example.org as important" }));
    await waitFor(() => {
      expect(core.callsOf("set_sender_important")).toEqual([
        { key: { kind: "list_id", value: "news.example.org" }, important: true },
      ]);
    });
  });

  it("a correction of the user says so and can be taken back", async () => {
    const corrected: MessageCategoryInfo = {
      ...info,
      decision: { category: "person", reason: { kind: "userRule", key: { kind: "domain", value: "example.org" } }, important: null },
    };
    const { core, user } = setup([news], { comfort: { categories: { 77: corrected } } });
    const pane = await open(user, "Weekly digest");
    await user.click(await within(pane).findByRole("button", { name: "Why here?" }));

    expect(within(pane).getByTestId("category-reason").textContent).toBe(
      "Because you always sort mail from example.org this way.",
    );
    await user.click(within(pane).getByTestId("category-automatic"));
    await waitFor(() => {
      expect(core.callsOf("set_sender_category")).toEqual([{ key: { kind: "domain", value: "example.org" }, category: null }]);
    });
  });

  it("a failure to load says what failed, with a way to retry", async () => {
    const { user } = setup([news], { failures: { get_message_category: ipcError("database") } });
    const pane = await open(user, "Weekly digest");
    const alert = await within(pane).findByText("Why this message is here could not be loaded.");
    expect(alert.closest("[role=alert]")?.textContent).toContain("Try again");
  });
});

describe("settings", () => {
  it("Inbox: sorting off and on, bundles, and the user's choices per sender", async () => {
    const { core, user } = setup(makeMessages(1), {
      comfort: {
        senderRules: [
          {
            id: "00000000-0000-4000-8000-000000000001",
            key: { kind: "domain", value: "shop.example" },
            category: "newsletter",
            important: false,
            updatedAt: 1,
          },
        ],
      },
    });
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Inbox" }));

    const sorting = await screen.findByRole("switch", { name: "Sort the inbox" });
    await waitFor(() => {
      expect(sorting.getAttribute("aria-checked")).toBe("true");
    });
    await user.click(sorting);
    await waitFor(() => {
      expect(core.callsOf("set_inbox_settings")).toEqual([
        { settings: { categories: false, bundleNotifications: true, bundleNewsletters: true } },
      ]);
    });
    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "Bundle newsletters" }).hasAttribute("disabled")).toBe(true);
    });

    const rule = await screen.findByTestId("sender-rule");
    expect(rule.textContent).toContain("shop.example");
    expect(rule.textContent).toContain("always Newsletters");
    await user.click(within(rule).getByRole("button", { name: "Remove the choice for shop.example" }));
    await waitFor(() => {
      expect(core.callsOf("delete_sender_rule")).toEqual([{ id: "00000000-0000-4000-8000-000000000001" }]);
    });
    expect(await screen.findByTestId("sender-rules-empty")).toBeTruthy();
  });

  it("Notifications: categories, quiet hours, hidden content, muted accounts and the counter", async () => {
    const { core, user } = setup(makeMessages(1));
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Notifications" }));

    await user.click(await screen.findByRole("switch", { name: "Newsletters" }));
    await waitFor(() => {
      expect(core.comfort.notificationSettings.categories).toEqual({
        people: true,
        priority: true,
        notifications: false,
        newsletters: true,
      });
    });
    await user.click(screen.getByRole("switch", { name: "Quiet hours" }));
    await waitFor(() => {
      expect(core.comfort.notificationSettings.quietHours?.enabled).toBe(true);
    });
    fireEvent.change(screen.getByTestId("notify-quiet-start"), { target: { value: "21:30" } });
    await waitFor(() => {
      expect(core.comfort.notificationSettings.quietHours?.startMinute).toBe(21 * 60 + 30);
    });
    await user.click(screen.getByRole("switch", { name: "Ada Lovelace (ada@vicemail.test)" }));
    await waitFor(() => {
      expect(core.comfort.notificationSettings.mutedAccounts).toEqual([ADA.id]);
    });
    await user.click(screen.getByRole("radio", { name: "Unread from people" }));
    await waitFor(() => {
      expect(core.comfort.notificationSettings.badge).toBe("peopleUnread");
    });
    expect(screen.getByText(/Whether previews appear on the lock screen is decided by the system/)).toBeTruthy();
  });
});

describe("notifications and events", () => {
  it("sends the notification texts in the UI's language, again after a language change", async () => {
    const { core } = setup(makeMessages(1), { preferences: { theme: "system", language: "en" } });
    await waitFor(() => {
      expect(core.comfort.notificationTexts).toEqual({
        newMessage: "New message",
        newMessages: "{count} new messages",
        noSubject: "(no subject)",
        sendFailed: "Could not send",
        snoozeDue: "Back from Later",
      });
    });
    await emitCore({ type: "preferencesChanged", preferences: { theme: "system", language: "de" } });
    await waitFor(() => {
      expect(core.comfort.notificationTexts?.newMessages).toBe("{count} neue Nachrichten");
    });
  });

  it("a skipped local rule is reported with its reason", async () => {
    setup();
    await list();
    await emitCore({
      type: "ruleSkipped",
      accountId: ADA.id,
      ruleId: "r1",
      ruleName: "Receipts to folder",
      reason: "read_only",
      count: 3,
    });
    await waitFor(() => {
      expect(notices().some((text) => text.includes("Local rule “Receipts to folder” skipped 3 messages."))).toBe(true);
    });
    expect(notices().some((text) => text.includes("Moving and marking need changes to be allowed"))).toBe(true);
  });

  it("a click on a system notification brings the message's folder to the front", async () => {
    setup();
    await list();
    await emitCore({ type: "openMessageRequested", accountId: ADA.id, mailboxId: 5, messageId: 1 });
    expect(await screen.findByRole("heading", { name: "This folder is empty" })).toBeTruthy();
    expect(within(screen.getByRole("banner")).getByText("Projekte")).toBeTruthy();
  });
});
