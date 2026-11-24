// Stage 17 (decision 0049): conversations unfold in the list like in Thunderbird. One row per
// conversation with "n replies" and a chevron; unfolded, every message of the conversation follows
// one level deeper, oldest first, from every folder. The reading pane shows one message. A tree
// for screen readers (level, place, set size, expanded); → ← ↑ ↓ like the sidebar's trees; actions
// on the conversation row act on the conversation, on a message row on that message alone.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { childKey, conversationIdent, treePlaces, withChildren } from "../src/features/threads/tree";
import type { ListRow } from "../src/features/messages/rows";
import type { MessageListItem, ThreadView } from "../src/services/ipc";
import { selectInList } from "../src/stores/listSelect";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";
import { CONVERSATION_PREFERENCES } from "./support/uiPrefs";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const ADA = makeAccount({ writesEnabled: true });
const now = Date.now();
const ANNA = { name: "Anna", email: "anna@example.org" };
const BOB = { name: "Bob", email: "bob@example.org" };
// A conversation over Inbox and Sent: Anna asks (unread), Ada answers from Sent, Bob answers.
const FIRST = makeMessage({ id: 1000, mailboxId: 1, subject: "Plan", from: ANNA, date: now - 3 * 3_600_000, unread: true, preview: "Montag?" });
const REPLY = makeMessage({ id: 1100, mailboxId: 2, subject: "Re: Plan", from: { name: "Ada Lovelace", email: "ada@vicemail.test" }, date: now - 2 * 3_600_000, preview: "Gern" });
const ANSWER = makeMessage({ id: 1200, mailboxId: 1, subject: "Re: Plan", from: BOB, date: now - 3_600_000, preview: "Montag passt" });
// A single message, newer than the conversation: it stands first.
const SOLO = makeMessage({ id: 1250, mailboxId: 1, subject: "Allein", from: { name: "Uma", email: "uma@example.org" }, date: now - 1_800_000 });
const thread = (...messages: MessageListItem[]): ThreadView => ({
  threadId: 7,
  messages: messages.map((message) => ({ message, mailboxIds: [message.mailboxId] })),
  total: messages.length,
});

function setup(options: Parameters<typeof mockCore>[0] = {}) {
  const threadViews: Record<number, ThreadView> = { [ANSWER.id]: thread(FIRST, REPLY, ANSWER) };
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [SOLO, ANSWER, FIRST], 2: [REPLY] },
    views: Object.fromEntries([FIRST, REPLY, ANSWER, SOLO].map((m) => [m.id, makeView(m)])),
    readingPreferences: { markReadOnOpen: "never" },
    // 1300: a reply that arrives later in one test.
    conversations: { threadOf: { [FIRST.id]: 7, [REPLY.id]: 7, [ANSWER.id]: 7, 1300: 7 } },
    search: { threadViews },
    ui: { preferences: CONVERSATION_PREFERENCES },
    ...options,
  });
  return { core, threadViews, ...renderApp() };
}

/** The inbox as a tree: the single message, then the conversation. */
async function inbox() {
  const list = await screen.findByRole("tree", { name: "Messages in Inbox" });
  await waitFor(() => {
    expect(within(list).getAllByRole("treeitem")).toHaveLength(2);
  });
  const conversation = () =>
    within(list)
      .getAllByRole("treeitem")
      .find((item) => item.getAttribute("aria-level") === "1" && item.hasAttribute("aria-expanded")) as HTMLElement;
  const children = () => within(list).queryAllByTestId("child-row");
  return { list, conversation, children };
}

/** Where the list's cursor is: the name of the tree item `aria-activedescendant` names. */
function cursorName(list: HTMLElement): string {
  const id = list.getAttribute("aria-activedescendant");
  return id ? optionName(document.getElementById(id)) : "";
}

/** Which row: the single message or the conversation, by its subject. */
const subject = (item: HTMLElement) => (optionName(item).includes("Allein") ? "Allein" : "Plan");

const places = (items: readonly HTMLElement[]) =>
  items.map((item) => [item.getAttribute("aria-level"), item.getAttribute("aria-posinset"), item.getAttribute("aria-setsize")]);

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

const actionCalls = (core: ReturnType<typeof mockCore>) =>
  core.callsOf("apply_action") as { action: { type: string }; messages: { mailboxId: number; messageId: number }[] }[];

describe("the tree (pure)", () => {
  const row = (id: number, level?: number): Extract<ListRow, { kind: "message" }> => ({
    kind: "message",
    key: `message-${String(id)}`,
    item: makeMessage({ id }),
    position: 0,
    ...(level !== undefined && { level }),
  });

  it("names a conversation by its thread, else its message", () => {
    expect(conversationIdent(makeMessage({ id: 5 }), { threadId: 9 })).toBe("t9");
    expect(conversationIdent(makeMessage({ id: 5 }), { threadId: null })).toBe("m5");
    expect(conversationIdent(makeMessage({ id: 5 }), undefined)).toBe("m5");
    expect(childKey("t9", 5)).toBe("t9/5");
  });

  it("puts an unfolded conversation's messages below its row and counts places per level", () => {
    const rows = withChildren([{ kind: "section", key: "s", section: { kind: "today" } }, row(1), row(2), row(3)], {
      expandable: (r) => (r.item.id === 2 ? "t2" : null),
      expanded: new Set(["t2"]),
      children: () => ({ kind: "loaded", messages: [makeMessage({ id: 20 }), makeMessage({ id: 21 })], total: 2 }),
      status: () => ({ render: () => null }),
    });
    expect(rows.map((r) => [r.kind, r.kind === "message" || r.kind === "child" ? r.position : null])).toEqual([
      ["section", null],
      ["message", 0],
      ["message", 1],
      ["child", 2],
      ["child", 3],
      ["message", 4],
    ]);
    const last = rows[4];
    expect(last?.kind === "child" && last.last).toBe(true);
    expect([...treePlaces(rows).entries()].sort(([a], [b]) => a - b)).toEqual([
      [1, { level: 1, posinset: 1, setsize: 3 }],
      [2, { level: 1, posinset: 2, setsize: 3 }],
      [3, { level: 2, posinset: 1, setsize: 2 }],
      [4, { level: 2, posinset: 2, setsize: 2 }],
      [5, { level: 1, posinset: 3, setsize: 3 }],
    ]);
  });

  it("shows loading, failed and cut conversations as rows of their own", () => {
    const status = vi.fn(() => ({ render: () => null, activate: () => undefined }));
    const loading = withChildren([row(1)], {
      expandable: () => "t1",
      expanded: new Set(["t1"]),
      children: () => ({ kind: "loading" }),
      status,
    });
    expect(loading.map((r) => (r.kind === "extra" ? [r.size, r.level, r.parent] : r.kind))).toEqual(["message", ["child", 2, "t1"]]);
    const cut = withChildren([row(1, 2)], {
      expandable: () => "t1",
      expanded: new Set(["t1"]),
      children: () => ({ kind: "loaded", messages: [makeMessage({ id: 30 })], total: 1500 }),
      status,
    });
    expect(cut.map((r) => [r.kind, r.kind === "message" ? r.level : r.kind === "child" || r.kind === "extra" ? r.level : null])).toEqual([
      ["message", 2],
      ["extra", 3],
      ["child", 3],
    ]);
    expect(status).toHaveBeenLastCalledWith({ kind: "truncated", shown: 1, total: 1500 }, expect.objectContaining({ kind: "message" }));
  });
});

describe("unfolding a conversation", () => {
  it("with the mouse: its messages below the row, oldest first, from every folder, one level deeper", async () => {
    const { core, user } = setup();
    const { list, conversation, children } = await inbox();
    expect(places([conversation()])).toEqual([["1", "2", "2"]]);
    expect(conversation().getAttribute("aria-expanded")).toBe("false");
    const solo = within(list).getAllByRole("treeitem")[0] as HTMLElement;
    expect(solo.hasAttribute("aria-expanded")).toBe(false);
    expect(core.callsOf("get_thread")).toEqual([]);

    const toggle = within(conversation()).getByTestId("row-replies");
    expect(toggle.textContent).toBe("2 replies");
    expect(toggle.getAttribute("title")).toBe("Show the replies");
    await user.click(toggle);

    // Loaded on unfolding, by the row's message in the row's folder; nothing is selected.
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    expect(core.callsOf("get_thread")).toEqual([{ messageId: ANSWER.id, mailboxId: 1 }]);
    expect(core.callsOf("get_message")).toEqual([]);
    expect(conversation().getAttribute("aria-expanded")).toBe("true");
    expect(within(conversation()).getByTestId("row-replies").getAttribute("title")).toBe("Hide the replies");
    expect(children().map(optionName)).toEqual([
      expect.stringMatching(/^Unread, Anna, Montag\?, /),
      expect.stringMatching(/^Ada Lovelace, in Sent, Gern, /),
      expect.stringMatching(/^Bob, Montag passt, /),
    ]);
    expect(places(children())).toEqual([
      ["2", "1", "3"],
      ["2", "2", "3"],
      ["2", "3", "3"],
    ]);
    expect(children().map((child) => child.getAttribute("role"))).toEqual(["treeitem", "treeitem", "treeitem"]);
    // Drawn: sender, folder where it lies elsewhere, the start of the text, the dotted line.
    const reply = children()[1] as HTMLElement;
    expect(within(reply).getByTestId("child-sender").textContent).toBe("Ada Lovelace");
    expect(within(reply).getByTestId("child-folder").textContent).toBe("Sent");
    expect(within(reply).getByTestId("child-text").textContent).toBe("Gern");
    expect(within(reply).getByTestId("tree-line")).toBeTruthy();
    expect(within(children()[0] as HTMLElement).getByTestId("row-unread-dot")).toBeTruthy();
    expect(within(children()[0] as HTMLElement).queryByTestId("child-folder")).toBeNull();
    expect(await seriousFindings()).toEqual([]);

    // Folded again with the same chevron.
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(0);
    });
    expect(conversation().getAttribute("aria-expanded")).toBe("false");
  });

  it("with the keyboard: → unfolds and enters, ↑ ↓ walk the visible rows, ← goes up and folds", async () => {
    const { user } = setup();
    const { list, conversation, children } = await inbox();
    list.focus();
    // The focus selects the first visible row, the single message.
    await waitFor(() => {
      expect(cursorName(list)).toMatch(/^Uma, Allein/);
    });
    await user.keyboard("{ArrowDown}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);
    // → on a single message does nothing.
    await user.keyboard("{ArrowRight}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);

    await user.keyboard("{ArrowRight}");
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    expect(conversation().getAttribute("aria-expanded")).toBe("true");
    await user.keyboard("{ArrowRight}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Montag\?/);
    expect(children()[0]?.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{ArrowDown}");
    expect(cursorName(list)).toMatch(/^Ada Lovelace, in Sent/);
    await user.keyboard("{ArrowDown}{ArrowDown}");
    // Past the last message: the next conversation (none here) or the end.
    expect(cursorName(list)).toMatch(/^Bob, Montag passt/);
    await user.keyboard("{ArrowUp}");
    expect(cursorName(list)).toMatch(/^Ada Lovelace, in Sent/);

    // ← from a message goes to its conversation's row, ← there folds it.
    await user.keyboard("{ArrowLeft}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);
    expect(conversation().getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{ArrowLeft}");
    await waitFor(() => {
      expect(children()).toHaveLength(0);
    });
    expect(conversation().getAttribute("aria-expanded")).toBe("false");
    await user.keyboard("{ArrowLeft}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);
    await user.keyboard("{ArrowUp}");
    expect(cursorName(list)).toMatch(/^Uma, Allein/);
  });

  it("folding with the cursor on one of its messages moves the cursor to the conversation", async () => {
    const { user } = setup();
    const { list, conversation, children } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    await user.click(children()[1] as HTMLElement);
    expect(cursorName(list)).toMatch(/^Ada Lovelace/);
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(0);
    });
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);
    expect(conversation().getAttribute("aria-selected")).toBe("true");
  });

  it("loads the messages when it unfolds and never blocks the list meanwhile", async () => {
    const { core, user } = setup({ hang: ["get_thread"] });
    const { list, conversation } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    const loading = await within(list).findByTestId("child-status-loading");
    expect(loading.textContent).toContain("Loading the conversation");
    expect(conversation().getAttribute("aria-busy")).toBe("true");
    expect(core.callsOf("get_thread")).toHaveLength(1);
    // The list goes on: the loading row is no stop for the keyboard.
    list.focus();
    await user.click(within(list).getAllByRole("treeitem")[0] as HTMLElement);
    await user.keyboard("{ArrowDown}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);
    await user.keyboard("{ArrowDown}");
    expect(cursorName(list)).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies/);
    await user.keyboard("{ArrowUp}");
    expect(cursorName(list)).toMatch(/^Uma, Allein/);
  });

  it("says in place when the messages cannot be loaded; Enter there tries again", async () => {
    let failing = true;
    const { core, user } = setup({ fail: (cmd) => (cmd === "get_thread" && failing ? ipcError("database") : undefined) });
    const { list, conversation, children } = await inbox();
    await user.click(conversation());
    await user.keyboard("{ArrowRight}");
    const failed = await within(list).findByRole("treeitem", { name: "The conversation could not be loaded. Try again" });
    expect(places([failed])).toEqual([["2", "1", "1"]]);
    failing = false;
    await user.keyboard("{ArrowDown}");
    expect(list.getAttribute("aria-activedescendant")).toBe(failed.id);
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    expect(core.callsOf("get_thread").length).toBeGreaterThanOrEqual(2);
  });

  it("stays unfolded, and open on the right, through a new reply that changes the row and the order", async () => {
    const { core, threadViews, user } = setup();
    const { list, conversation, children } = await inbox();
    expect(within(list).getAllByRole("treeitem").map(subject)).toEqual(["Allein", "Plan"]);
    await user.click(conversation());
    await waitFor(() => {
      expect(within(screen.getByRole("region", { name: "Message" })).getByTestId("reading-from").textContent).toContain(ANNA.email);
    });
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });

    // Bob answers again: the conversation's row is now his new message, and it moves up.
    const NEWER = makeMessage({ id: 1300, mailboxId: 1, subject: "Re: Plan", from: BOB, date: now - 60_000, unread: true, preview: "Oder Dienstag?" });
    threadViews[ANSWER.id] = thread(FIRST, REPLY, ANSWER, NEWER);
    threadViews[NEWER.id] = thread(FIRST, REPLY, ANSWER, NEWER);
    core.setMessages(1, [NEWER, SOLO, ANSWER, FIRST]);
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });

    await waitFor(() => {
      expect(children()).toHaveLength(4);
    });
    const rows = within(list).getAllByRole("treeitem").filter((item) => item.getAttribute("aria-level") === "1");
    expect(rows.map(subject)).toEqual(["Plan", "Allein"]);
    expect(rows[0]?.getAttribute("aria-expanded")).toBe("true");
    expect(within(rows[0] as HTMLElement).getByTestId("row-replies").textContent).toBe("3 replies");
    // The children follow their row; the selected conversation stays selected and its pane open.
    expect(children().map((child) => optionName(child).split(", ")[0] === "Unread" ? optionName(child).split(", ")[1] : optionName(child).split(", ")[0])).toEqual([
      "Anna",
      "Ada Lovelace",
      "Bob",
      "Bob",
    ]);
    expect(rows[0]?.getAttribute("aria-selected")).toBe("true");
    expect(within(screen.getByRole("region", { name: "Message" })).getByTestId("reading-from").textContent).toContain(ANNA.email);
    expect(core.callsOf("get_message").map((call) => call["messageId"])).not.toContain(NEWER.id);
  });
});

describe("the look (stage 17, after the screenshot pass)", () => {
  /** jsdom lays nothing out: the classes that make the layout, measured in conversations.e2e.mjs. */
  const classes = (element: Element | null | undefined) => (element?.getAttribute("class") ?? "").split(/\s+/);

  it("the replies pill gives way to the subject first, down to chevron and number", async () => {
    setup();
    const { conversation } = await inbox();
    const pill = within(conversation()).getByTestId("row-replies");
    expect(pill.textContent).toBe("2 replies");
    // Chevron, number and the word with its space in a column that may shrink to nothing.
    expect(classes(pill)).toEqual(expect.arrayContaining(["grid", "shrink-1000000", "grid-cols-[auto_auto_minmax(0,max-content)]"]));
    expect(classes(pill)).not.toContain("shrink-0");
    const word = within(pill).getByTestId("row-replies-word");
    expect(word.textContent).toBe(" replies");
    expect(classes(word)).toEqual(expect.arrayContaining(["min-w-0", "overflow-hidden", "text-ellipsis", "whitespace-pre"]));
    expect(pill.textContent?.replace(word.textContent ?? "", "")).toBe("2");
    // The subject shrinks only after it (shrink factor 1, no fixed width).
    const subject = within(conversation()).getByTestId("row-subject");
    expect(classes(subject)).toEqual(expect.arrayContaining(["min-w-0", "truncate"]));
    expect(classes(subject).some((c) => c.startsWith("shrink") || c.startsWith("w-"))).toBe(false);
  });

  it("child rows size the sender to its name; the text takes the rest", async () => {
    const { user } = setup();
    const { conversation, children } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    for (const child of children()) {
      const sender = within(child).getByTestId("child-sender");
      expect(classes(sender)).toEqual(expect.arrayContaining(["max-w-1/3", "shrink-0", "truncate"]));
      expect(classes(sender).some((c) => c.startsWith("w-"))).toBe(false);
      expect(classes(within(child).getByTestId("child-text"))).toEqual(expect.arrayContaining(["min-w-0", "flex-1", "truncate"]));
    }
  });

  for (const width of [1000, 400]) {
    it(`the line leaves from under the conversation's avatar (${width === 400 ? "narrow" : "wide"} list)`, async () => {
      // The list measures its width through a ResizeObserver: report one at once.
      vi.stubGlobal(
        "ResizeObserver",
        class {
          constructor(private readonly callback: ResizeObserverCallback) {}
          observe() {
            this.callback([], this);
          }
          unobserve() {}
          disconnect() {}
        },
      );
      vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(width);
      const { user } = setup();
      const { conversation, children } = await inbox();
      expect(within(conversation()).queryByTestId("tree-line")).toBeNull();
      await user.click(within(conversation()).getByTestId("row-replies"));
      await waitFor(() => {
        expect(children()).toHaveLength(3);
      });
      // Narrow rows have the subject on a line of their own.
      expect(conversation().querySelector(".flex-col") !== null).toBe(width === 400);
      const line = within(conversation()).getByTestId("tree-line");
      // From the row's middle (inside the avatar) to its bottom, under the avatar, which is
      // positioned after it and so covers its start.
      expect(classes(line)).toEqual(expect.arrayContaining(["absolute", "top-1/2", "bottom-0", "start-(--ui-tree-line-start)", "border-dotted"]));
      const avatar = within(conversation()).getByTestId("avatar").parentElement;
      expect(classes(avatar)).toContain("relative");
      expect(line.compareDocumentPosition(avatar as Node) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      // On through every message, ending in the middle of the last.
      const lines = children().map((child) => classes(within(child).getByTestId("tree-line")));
      expect(lines.map((c) => [c.includes("top-0"), c.includes("bottom-0"), c.includes("h-1/2")])).toEqual([
        [true, true, false],
        [true, true, false],
        [true, false, true],
      ]);
      vi.unstubAllGlobals();
    });
  }
});

describe("a message asked for by id (a notification, an AI source)", () => {
  it("unfolds its conversation and selects the message itself", async () => {
    const { core } = setup();
    const { list, conversation, children } = await inbox();
    let found = false;
    act(() => {
      found = selectInList(ANSWER.id);
    });
    expect(found).toBe(true);
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    await waitFor(() => {
      expect(cursorName(list)).toMatch(/^Bob, Montag passt/);
    });
    expect(conversation().getAttribute("aria-expanded")).toBe("true");
    expect(children()[2]?.getAttribute("aria-selected")).toBe("true");
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: ANSWER.id, format: "auto" });
    });
  });
});

describe("the reading pane shows one message", () => {
  it("the conversation row opens its oldest unread message, a message row that message", async () => {
    const { core, user } = setup();
    const { conversation, children } = await inbox();
    await user.click(conversation());
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(within(pane).getByTestId("reading-from").textContent).toContain(ANNA.email);
    });
    expect(within(pane).getAllByTestId("message-card")).toHaveLength(1);
    // The subject of the conversation is the title; the toolbar acts on the open message.
    expect(within(pane).getByRole("toolbar", { name: "Message actions" })).toBeTruthy();

    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    await user.click(children()[1] as HTMLElement);
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: REPLY.id, format: "auto" });
    });
    const replyPane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(within(replyPane).getByTestId("reading-from").textContent).toContain("ada@vicemail.test");
    });
    expect(within(replyPane).getAllByTestId("message-card")).toHaveLength(1);
    expect(await seriousFindings()).toEqual([]);
  });

  it("marking as read on open marks the message it shows, not the whole conversation", async () => {
    const { core, user } = setup({ readingPreferences: { markReadOnOpen: "immediately" } });
    const { conversation } = await inbox();
    await user.click(conversation());
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([{ action: { type: "markRead" }, messages: [{ mailboxId: 1, messageId: FIRST.id }], requestId: expect.any(String) as string }]);
    });
    expect(core.callsOf("conversation_targets")).toEqual([]);
  });
});

describe("actions: the conversation row acts on the conversation, a message row on that message", () => {
  it("a shortcut on a message row of an unfolded conversation deletes that message alone", async () => {
    const { core, user } = setup();
    const { list, conversation, children } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    await user.click(children()[1] as HTMLElement);
    list.focus();
    await user.keyboard("#");
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]?.messages).toEqual([{ mailboxId: 2, messageId: REPLY.id }]);
    expect(core.callsOf("conversation_targets")).toEqual([]);
    // The conversation stays in the list.
    expect(conversation()).toBeTruthy();

    // The conversation row: the conversation's messages in this folder, as the core says.
    await user.click(conversation());
    list.focus();
    await user.keyboard("#");
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(2);
    });
    expect(core.callsOf("conversation_targets")).toEqual([
      { rows: [{ mailboxId: 1, messageId: ANSWER.id }], intent: { kind: "action", action: { type: "delete" } } },
    ]);
  });

  it("a quick action on a message row acts on that message alone", async () => {
    const { core, user } = setup();
    const { conversation, children } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    await user.click(within(children()[0] as HTMLElement).getByTestId("quick-toggleRead"));
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]).toMatchObject({ action: { type: "markRead" }, messages: [{ mailboxId: 1, messageId: FIRST.id }] });
    expect(core.callsOf("conversation_targets")).toEqual([]);
  });

  it("a selection of conversations and messages acts on each as it was chosen", async () => {
    const { core, user } = setup();
    const { list, conversation, children } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    const solo = within(list).getAllByRole("treeitem")[0] as HTMLElement;
    await user.click(solo);
    await user.keyboard("{Meta>}");
    await user.click(children()[1] as HTMLElement);
    await user.keyboard("{/Meta}");
    const bar = screen.getByRole("region", { name: "Selection" });
    expect(within(bar).getByTestId("selection-count").textContent).toBe("2 selected");
    await user.click(within(within(bar).getByRole("toolbar")).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    // Only the single message's row asks the core; the message chosen in the conversation acts alone.
    expect(core.callsOf("conversation_targets")).toEqual([
      { rows: [{ mailboxId: 1, messageId: SOLO.id }], intent: { kind: "action", action: { type: "delete" } } },
    ]);
    expect(actionCalls(core)[0]?.messages).toEqual([
      { mailboxId: 2, messageId: REPLY.id },
      { mailboxId: 1, messageId: SOLO.id },
    ]);
  });

  it("Shift extends a range over conversation and message rows alike", async () => {
    const { user } = setup();
    const { list, conversation, children } = await inbox();
    await user.click(within(conversation()).getByTestId("row-replies"));
    await waitFor(() => {
      expect(children()).toHaveLength(3);
    });
    await user.click(conversation());
    fireEvent.keyDown(list, { key: "ArrowDown", shiftKey: true });
    fireEvent.keyDown(list, { key: "ArrowDown", shiftKey: true });
    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Selection" })).toBeTruthy();
    });
    expect(within(screen.getByRole("region", { name: "Selection" })).getByTestId("selection-count").textContent).toBe("3 selected");
    expect(children().map((child) => child.getAttribute("aria-selected"))).toEqual(["true", "true", "false"]);
  });
});
