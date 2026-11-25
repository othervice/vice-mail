// Decision 0039: the conversation view as a stored UI preference, conversation rows in the sorted
// inbox, the unified inbox, the unread lists and search, the reading pane of a conversation row
// (one message since stage 17, decision 0049), and actions on conversation rows through the
// core's `conversationTargets`. Unfolding conversations in the list: `conversation-tree.test.tsx`.
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { rowSubject } from "../src/features/messages/MessageRow";
import {
  asConversation,
  conversationDecoration,
  conversationInboxPage,
  conversationKey,
  groupByThread,
  mergeInfo,
  type ConversationInfo,
} from "../src/features/threads/conversationRows";
import type { InboxPage, SearchRequest, ThreadView, UiPreferences } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAiProfile, makeTransfer } from "./support/ai";
import { inboxSection, messageRow } from "./support/comfort";
import { ipcError, makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";
import { makeHit, searchPage } from "./support/search";
import { CONVERSATION_PREFERENCES, DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const ADA = makeAccount({ writesEnabled: true });
const now = Date.now();
const ANNA = { name: "Anna", email: "anna@example.org" };
const BOB = { name: "Bob", email: "bob@example.org" };
// A conversation over Inbox and Sent: Anna asks, Ada answers from Sent, Bob answers.
const FIRST = makeMessage({ id: 1000, mailboxId: 1, subject: "Plan", from: ANNA, date: now - 3 * 3_600_000, unread: true, preview: "Montag?" });
const REPLY = makeMessage({ id: 1100, mailboxId: 2, subject: "Re: Plan", from: { name: "Ada Lovelace", email: "ada@vicemail.test" }, date: now - 2 * 3_600_000 });
const ANSWER = makeMessage({ id: 1200, mailboxId: 1, subject: "Re: Plan", from: BOB, date: now - 3_600_000, preview: "Montag passt" });
const SOLO = makeMessage({ id: 900, mailboxId: 1, subject: "Allein", date: now - 5 * 3_600_000 });
const THREAD_OF = { [FIRST.id]: 7, [REPLY.id]: 7, [ANSWER.id]: 7 };
const THREAD: ThreadView = {
  threadId: 7,
  messages: [FIRST, REPLY, ANSWER].map((message) => ({ message, mailboxIds: [message.mailboxId] })),
  total: 3,
};

function setup(options: Parameters<typeof mockCore>[0] = {}, preferences: UiPreferences = CONVERSATION_PREFERENCES) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [ANSWER, FIRST, SOLO], 2: [REPLY] },
    views: Object.fromEntries([FIRST, REPLY, ANSWER, SOLO].map((m) => [m.id, makeView(m)])),
    readingPreferences: { markReadOnOpen: "never" },
    conversations: { threadOf: THREAD_OF },
    search: { threadViews: { [ANSWER.id]: THREAD } },
    ui: { preferences },
    ...options,
  });
  return { core, ...renderApp() };
}

/** The inbox as conversations: a tree (stage 17) with the conversation and the single message. */
async function inboxList() {
  const list = await screen.findByRole("tree", { name: "Messages in Inbox" });
  await waitFor(() => {
    expect(within(list).getAllByRole("treeitem")).toHaveLength(2);
  });
  return list;
}

const savedConversations = (core: ReturnType<typeof mockCore>) =>
  core.callsOf("set_ui_preferences").map((call) => (call["preferences"] as UiPreferences).conversations);

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

describe("conversation rows (pure)", () => {
  const info = (overrides: Partial<ConversationInfo> = {}): ConversationInfo => ({
    threadId: 7,
    messageCount: 3,
    unreadCount: 1,
    participants: [ANNA, BOB],
    hasAttachments: true,
    flagged: true,
    subject: "Plan",
    ...overrides,
  });

  it("show the conversation's state on its row message", () => {
    const row = asConversation(ANSWER, info());
    expect([row.unread, row.flagged, row.hasAttachments]).toEqual([true, true, true]);
    // "Unread first" groups by the message's own state.
    expect(asConversation(ANSWER, info(), true).unread).toBe(false);
    expect(asConversation(ANSWER, undefined)).toBe(ANSWER);
  });

  it("swap every message of an inbox page and key rows by their thread", () => {
    const page: InboxPage = {
      rows: [messageRow(ANSWER)],
      nextCursor: null,
      top: { pinned: [], returned: [], priority: [FIRST], priorityTotal: 1, priorityUnread: 1 , followUps: []},
      categories: true,
      conversations: [
        { messageId: ANSWER.id, threadId: 7, messageCount: 3, unreadCount: 1, lastActivity: ANSWER.date, participants: [ANNA], hasAttachments: false, flagged: false, subject: "Plan" },
        { messageId: FIRST.id, threadId: 7, messageCount: 3, unreadCount: 1, lastActivity: ANSWER.date, participants: [ANNA], hasAttachments: false, flagged: false, subject: "Plan" },
      ],
    };
    const { page: swapped, info } = conversationInboxPage(page);
    const row = swapped.rows[0];
    expect(row?.kind === "message" && row.item.unread).toBe(true);
    const key = conversationKey(mergeInfo([{ info }]));
    expect(key(ANSWER)).toBe(key(FIRST));
    expect(swapped).toBeDefined();
    expect(key(SOLO)).toBe(`m${String(SOLO.id)}`);
  });

  it("group search hits: the newest hit of a thread is its row", () => {
    const hits = [{ id: 3, thread: 7 }, { id: 2, thread: null }, { id: 1, thread: 7 }];
    expect(groupByThread(hits, (hit) => hit.thread).map((hit) => hit.id)).toEqual([3, 2]);
  });
});

describe("the conversation view as a preference (decision 0039)", () => {
  it("is on by default: the core's default, and the UI's own when the core cannot answer", async () => {
    const { core } = setup();
    await inboxList();
    expect(core.callsOf("list_inbox_conversations")).toEqual([{ scope: { kind: "mailbox", mailboxId: 1 }, cursor: null, limit: 200 }]);
    expect(core.callsOf("list_inbox")).toEqual([]);
    expect(screen.getByRole("button", { name: "Conversations" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("starts on also when the stored preferences cannot be read", async () => {
    const { core } = setup({ failures: { get_ui_preferences: ipcError("database") } });
    await inboxList();
    expect(core.callsOf("list_inbox_conversations")).not.toHaveLength(0);
  });

  it("the header switch, the list menu, the palette and Settings → Reading change the stored value", async () => {
    const { core, user } = setup({}, DEFAULT_UI_PREFERENCES);
    const list = await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(3);
    });
    expect(core.callsOf("list_inbox_conversations")).toEqual([]);

    // Header switch.
    const toggle = screen.getByRole("button", { name: "Conversations" });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    await user.click(toggle);
    await waitFor(() => {
      expect(savedConversations(core)).toEqual([true]);
    });
    await inboxList();

    // The list's options menu.
    await user.click(screen.getByRole("button", { name: "List options" }));
    await user.click(await screen.findByRole("menuitem", { name: "Show single messages" }));
    await waitFor(() => {
      expect(savedConversations(core)).toEqual([true, false]);
    });
    expect(screen.getByRole("button", { name: "Conversations" }).getAttribute("aria-pressed")).toBe("false");

    // The command palette, keyboard only.
    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    const field = within(palette).getByRole("combobox", { name: "Command" });
    await waitFor(() => {
      expect(document.activeElement).toBe(field);
    });
    await user.keyboard("Show conversations");
    await waitFor(() => {
      expect(within(palette).getAllByRole("option")[0]?.textContent).toContain("Show conversations");
    });
    await user.keyboard("{Enter}");
    // The palette also saves its recent commands (with the value of the moment) before.
    await waitFor(() => {
      expect(core.ui.preferences.conversations).toBe(true);
    });
    expect(savedConversations(core).at(-1)).toBe(true);

    // Settings → Reading.
    act(() => {
      useUiStore.getState().showSettings("reading");
    });
    const group = await screen.findByRole("radiogroup", { name: "Lists show" });
    expect(within(group).getByRole("radio", { name: "One row per conversation" }).getAttribute("aria-checked")).toBe("true");
    await user.click(within(group).getByRole("radio", { name: "One row per message" }));
    await waitFor(() => {
      expect(core.ui.preferences.conversations).toBe(false);
    });
  });

  it("follows another device (uiPreferencesChanged) and puts a refused change back with the reason", async () => {
    const { core, user } = setup({ failures: { set_ui_preferences: ipcError("invalid_argument") } }, DEFAULT_UI_PREFERENCES);
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await emitCore({ type: "uiPreferencesChanged", preferences: CONVERSATION_PREFERENCES });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Conversations" }).getAttribute("aria-pressed")).toBe("true");
    });
    await waitFor(() => {
      expect(core.callsOf("list_inbox_conversations")).not.toHaveLength(0);
    });

    await user.click(screen.getByRole("button", { name: "Conversations" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Conversations" }).getAttribute("aria-pressed")).toBe("true");
    });
    act(() => {
      useUiStore.getState().showSettings("reading");
    });
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The conversation view could not be saved")).toBeTruthy();
  });
});

describe("conversation rows in the lists", () => {
  it("the sorted inbox shows one row per conversation: size, senders, unread when any message is", async () => {
    setup();
    const list = await inboxList();
    const [thread, single] = within(list).getAllByRole("treeitem");
    const name = optionName(thread);
    // Label in name: the visible senders, subject and replies, in reading order.
    // Stage 17: the conversation's subject, its oldest message's ("Plan"), not the newest one's.
    expect(name).toMatch(/^Unread, Anna, Ada Lovelace, Bob, Plan, 2 replies, /);
    expect(within(thread as HTMLElement).getByTestId("row-subject").textContent).toBe("Plan");
    expect(within(thread as HTMLElement).getByTestId("row-replies").textContent).toBe("2 replies");
    expect(within(thread as HTMLElement).getByTitle("Anna, Ada Lovelace, Bob")).toBeTruthy();
    expect(optionName(single)).not.toContain("repl");
  });

  it("shows a conversation of the priority block once", async () => {
    const vip = makeMessage({ id: 1300, mailboxId: 1, subject: "Wichtig", important: true, date: now - 600_000 });
    const reply = makeMessage({ id: 1400, mailboxId: 1, subject: "Re: Wichtig", date: now - 300_000 });
    const page: InboxPage = {
      rows: [messageRow(reply)],
      nextCursor: null,
      top: { pinned: [], returned: [], priority: [vip], priorityTotal: 1, priorityUnread: 0 , followUps: []},
      categories: true,
      conversations: [],
    };
    setup({
      messages: { 1: [reply, vip] },
      conversations: { threadOf: { [vip.id]: 9, [reply.id]: 9 } },
      comfort: { inboxPages: { 1: [page] } },
    });
    const list = await screen.findByRole("tree", { name: "Messages in Inbox" });
    await waitFor(() => {
      expect(within(list).getAllByRole("treeitem")).toHaveLength(1);
    });
    expect(optionName(within(list).getByRole("treeitem"))).toContain("Wichtig");
    expect(optionName(within(list).getByRole("treeitem"))).toContain("1 reply");
  });

  it("a bundle holds conversations: an unfolded member shows the size of its conversation", async () => {
    const failed = makeMessage({ id: 1500, mailboxId: 1, subject: "CI failed", category: "notification", date: now - 60_000 });
    const earlier = makeMessage({ id: 1450, mailboxId: 1, subject: "CI failed", category: "notification", date: now - 120_000 });
    const review = makeMessage({ id: 1400, mailboxId: 1, subject: "Review requested", category: "notification", date: now - 180_000 });
    const section = inboxSection(failed.date);
    const page: InboxPage = {
      rows: [
        {
          kind: "bundle",
          summary: { key: { category: "notification", section: section.start }, count: 2, unread: 0, senders: [] },
          section,
          members: [failed, earlier, review],
        },
      ],
      nextCursor: null,
      top: null,
      categories: true,
      conversations: [],
    };
    const { user } = setup({
      messages: { 1: [failed, earlier, review] },
      conversations: { threadOf: { [failed.id]: 4, [earlier.id]: 4 } },
      comfort: { inboxPages: { 1: [page] } },
    });
    const list = await screen.findByRole("tree", { name: "Messages in Inbox" });
    const bundle = await within(list).findByRole("treeitem", { name: /^Notifications/ });
    // In the tree the bundle says folded or unfolded itself, its members are one level deeper.
    expect(bundle.getAttribute("aria-expanded")).toBe("false");
    expect(optionName(bundle)).not.toContain("collapsed");
    await user.click(await within(list).findByTestId("bundle-row"));
    await waitFor(() => {
      expect(within(list).getAllByRole("treeitem")).toHaveLength(3);
    });
    expect(bundle.getAttribute("aria-expanded")).toBe("true");
    const members = within(list)
      .getAllByRole("treeitem")
      .filter((option) => optionName(option).includes("CI failed") || optionName(option).includes("Review"));
    expect(members.map((option) => optionName(option).includes("1 reply"))).toEqual([true, false]);
    expect(members.map((option) => [option.getAttribute("aria-level"), option.getAttribute("aria-posinset"), option.getAttribute("aria-setsize")])).toEqual([
      ["2", "1", "2"],
      ["2", "2", "2"],
    ]);
    // The member with a reply unfolds one level deeper still.
    expect(members[0]?.getAttribute("aria-expanded")).toBe("false");
  });

  it("Unread only and Unread first list conversations", async () => {
    const { core, user } = setup({ parity: {} });
    await inboxList();
    await user.click(screen.getByRole("button", { name: "Unread" }));
    await waitFor(() => {
      expect(core.callsOf("list_inbox_conversations_filtered")).toContainEqual({
        scope: { kind: "mailbox", mailboxId: 1 },
        filter: { kind: "unread" },
        cursor: null,
        limit: 200,
      });
    });
    const list = screen.getByRole("tree", { name: "Messages in Inbox" });
    await waitFor(() => {
      expect(within(list).getAllByRole("treeitem")).toHaveLength(1);
    });
    expect(optionName(within(list).getByRole("treeitem"))).toContain("2 replies");

    await user.click(screen.getByRole("button", { name: "Unread" }));
    await user.click(screen.getByRole("button", { name: "List options" }));
    await user.click(await screen.findByRole("menuitem", { name: "Show unread first" }));
    await waitFor(() => {
      expect(core.callsOf("list_unread_first_conversations")).toContainEqual({
        scope: { kind: "mailbox", mailboxId: 1 },
        cursor: null,
        limit: 200,
      });
    });
  });

  it("search results show one row per conversation with its size", async () => {
    const { user } = setup({
      search: {
        threadViews: { [ANSWER.id]: THREAD },
        search: (request: SearchRequest) =>
          request.query.includes("Plan")
            ? searchPage({
                hits: [
                  makeHit(ANSWER, ["Plan"], { threadId: 7, threadMessageCount: 3 }),
                  makeHit(FIRST, ["Plan"], { threadId: 7, threadMessageCount: 3 }),
                ],
              })
            : searchPage(),
      },
    });
    await inboxList();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.keyboard("Plan");
    const results = await screen.findByRole("tree", { name: "Messages in the search results for “Plan”" });
    await waitFor(() => {
      expect(within(results).getAllByRole("treeitem")).toHaveLength(1);
    });
    const row = within(results).getByRole("treeitem");
    expect(within(row).getByTestId("row-replies").textContent).toBe("2 replies");
    // Stage 17: the conversation's subject; the hit's highlight still marks the word in it.
    const subject = within(row).getByTestId("row-subject");
    expect(subject.textContent).toBe("Plan");
    expect(within(subject).getByTestId("highlight").textContent).toBe("Plan");
    expect(optionName(row)).toMatch(/, Plan, 2 replies, /);
    // One message on the right: the oldest unread one of the conversation.
    await user.click(row);
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(within(pane).getByTestId("reading-from").textContent).toContain(ANNA.email);
    });
    expect(screen.queryByTestId("thread-entries")).toBeNull();
    // Its replies unfold in the results, from every folder.
    await user.click(within(row).getByTestId("row-replies"));
    await waitFor(() => {
      expect(within(results).getAllByTestId("child-row")).toHaveLength(3);
    });
  });
});

describe("the subject of a conversation row (stage 17)", () => {
  it("is the conversation's, with the hit's highlights only where they fit", () => {
    const answer = makeMessage({ subject: "  Re: AW: Plan  " });
    expect(rowSubject(answer, undefined)).toEqual({ text: "Re: AW: Plan", offset: 2 });
    expect(rowSubject(answer, { subject: "Plan" })).toEqual({ text: "Plan", offset: 10 });
    // A subject changed in the conversation: the root's is shown, the hit's marks are not.
    const changed = makeMessage({ subject: "Re: Venue (was: Plan)" });
    expect(rowSubject(changed, { subject: "Plan" })).toEqual({ text: "Plan", offset: null });
    expect(rowSubject(changed, { subject: "" })).toEqual({ text: "", offset: null });
    // A message alone keeps its own subject, prefix included.
    expect(conversationDecoration({ threadId: 7, messageCount: 1, unreadCount: 0, participants: [], hasAttachments: false, flagged: false, subject: "x" })).toEqual({ threadId: 7 });
  });

  it("in search results drops the highlight when the hit's subject is another", async () => {
    const VENUE = makeMessage({ id: 1300, mailboxId: 1, subject: "Re: Venue (was: Plan)", from: BOB, date: now - 60_000 });
    const { user } = setup({
      search: {
        search: (request: SearchRequest) =>
          request.query.includes("Venue")
            ? searchPage({ hits: [makeHit(VENUE, ["Venue"], { threadId: 7, threadMessageCount: 4, conversationSubject: "Plan" })] })
            : searchPage(),
      },
    });
    await inboxList();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.keyboard("Venue");
    const results = await screen.findByRole("tree", { name: "Messages in the search results for “Venue”" });
    const row = await within(results).findByRole("treeitem");
    const subject = within(row).getByTestId("row-subject");
    expect(subject.textContent).toBe("Plan");
    expect(within(subject).queryByTestId("highlight")).toBeNull();
    expect(subject.getAttribute("title")).toBe("Plan");
  });
});

describe("opening a conversation", () => {
  it("in the inbox shows one message: the conversation's oldest unread one", async () => {
    const { core, user } = setup();
    const list = await inboxList();
    await user.click(within(list).getAllByRole("treeitem")[0] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(within(pane).getByTestId("reading-from").textContent).toContain(ANNA.email);
    });
    expect(core.callsOf("get_thread")).toEqual([{ messageId: ANSWER.id, mailboxId: 1 }]);
    await waitFor(() => {
      expect(core.callsOf("get_message")).toEqual([{ messageId: FIRST.id, format: "auto", scheme: "light" }]);
    });
    // No stacked conversation: the pane holds one message card.
    expect(screen.queryByTestId("thread-entries")).toBeNull();
    expect(within(pane).getAllByTestId("message-card")).toHaveLength(1);
    expect(await seriousFindings()).toEqual([]);
  });

  it("offers the AI chips under the message it opens, which is not the row's message", async () => {
    // The row stands for the newest message (Bob's answer); the pane opens the oldest unread one
    // (Anna's question). The AI panel works on the row's message, the whole conversation.
    const LOCAL = makeAiProfile();
    const { core, user } = setup({
      ai: {
        enabled: true,
        profiles: [LOCAL],
        functions: { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id },
        summary: () => ({ keyPoints: [], openQuestions: [], deadlines: [], dropped: 0, transfer: makeTransfer() }),
      },
    });
    const list = await inboxList();
    await user.click(within(list).getAllByRole("treeitem")[0] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(within(pane).getByTestId("reading-from").textContent).toContain(ANNA.email);
    });
    const chips = await within(pane).findByRole("group", { name: "AI for this message" });
    await user.click(within(chips).getByRole("button", { name: /^Summari[sz]e/ }));
    await waitFor(() => {
      expect(core.callsOf("ai_summarize_thread")).toEqual([expect.objectContaining({ messageId: ANSWER.id })]);
    });
  });

  it("in the unified inbox shows one message of the row's conversation, with the row's account", async () => {
    const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
    const WORK_MAIL = makeMessage({ id: 2000, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: now - 120_000 });
    const { core, user } = setup({
      accounts: [ADA, WORK],
      mailboxes: [...standardMailboxes(), makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" })],
      messages: { 1: [ANSWER, FIRST, SOLO], 2: [REPLY], 20: [WORK_MAIL] },
      views: Object.fromEntries([FIRST, REPLY, ANSWER, SOLO, WORK_MAIL].map((m) => [m.id, makeView(m)])),
    });
    await screen.findByRole("tree", { name: "Messages in Inbox" });
    await user.click(await screen.findByRole("button", { name: /^Show the inbox of/ }));
    const choices = await screen.findByRole("list", { name: "Show the inbox of" });
    await user.click(within(choices).getByRole("button", { name: "All accounts" }));
    const list = await screen.findByRole("tree", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("treeitem")).toHaveLength(3);
    });
    expect(core.callsOf("list_inbox_conversations")).toContainEqual({ scope: { kind: "unified", view: { kind: "all" } }, cursor: null, limit: 200 });
    const projekt = within(list)
      .getAllByRole("treeitem")
      .find((option) => optionName(option).includes("Projekt"));
    // Stage 17: the account label beside the names: its colour dot and name, the name as its
    // tooltip and in the row's name; it gives way to the names first (large shrink factor, the
    // dot as its smallest size; the layout itself is measured in conversations.e2e.mjs).
    const label = within(projekt as HTMLElement).getByTestId("row-account");
    expect(label.getAttribute("title")).toBe("Work");
    expect(within(label).getByTestId("row-account-name").textContent).toBe("Work");
    expect(within(label).getByTestId("row-account-dot").className).toContain("bg-account-4");
    expect(label.className).toMatch(/\bshrink-1000000\b/);
    expect(label.className).toContain("min-w-(--ui-dot)");
    expect(label.className).toContain("overflow-hidden");
    // The name whole or not at all: where it does not fit beside the dot it wraps out of sight.
    expect(label.className).toMatch(/\bflex-wrap\b/);
    expect(label.className).toMatch(/\bh-5\b/);
    expect(within(label).getByTestId("row-account-name").className).toMatch(/\btruncate\b/);
    expect(optionName(projekt)).toContain("Account: Work");
    await user.click(projekt as HTMLElement);
    await waitFor(() => {
      expect(core.callsOf("get_thread")).toContainEqual({ messageId: WORK_MAIL.id, mailboxId: 20 });
    });
    expect(await screen.findByTestId("thread-pane")).toBeTruthy();
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: WORK_MAIL.id, format: "auto", scheme: "light" });
    });
  });
});

describe("actions on conversation rows", () => {
  const actionCalls = (core: ReturnType<typeof mockCore>) =>
    core.callsOf("apply_action") as { action: { type: string }; messages: { mailboxId: number; messageId: number }[] }[];

  it("delete the whole conversation in this folder; the reading pane's toolbar acts on its one message", async () => {
    const { core, user } = setup();
    const list = await inboxList();
    await user.click(within(list).getAllByRole("treeitem")[0] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });

    // The toolbar: the open message only (the first unread one, Anna's question).
    await user.click(within(within(pane).getByRole("toolbar", { name: "Message actions" })).getByRole("button", { name: /^Flag/ }));
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]?.messages).toEqual([{ mailboxId: 1, messageId: FIRST.id }]);
    expect(core.callsOf("conversation_targets")).toEqual([]);

    // A shortcut on the row: the conversation's messages in the inbox (not the reply in Sent).
    list.focus();
    await user.keyboard("#");
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(2);
    });
    expect(core.callsOf("conversation_targets")).toEqual([
      { rows: [{ mailboxId: 1, messageId: ANSWER.id }], intent: { kind: "action", action: { type: "delete" } } },
    ]);
    expect(actionCalls(core)[1]?.messages).toEqual([
      { mailboxId: 1, messageId: ANSWER.id },
      { mailboxId: 1, messageId: FIRST.id },
    ]);
    // Both leave the list at once; the other conversation stays.
    await waitFor(() => {
      expect(within(list).getAllByRole("treeitem")).toHaveLength(1);
    });
  });

  it("pin the conversation and snooze it with its messages in the folder", async () => {
    const { core, user } = setup();
    const list = await inboxList();
    await user.click(within(list).getAllByRole("treeitem")[0] as HTMLElement);
    list.focus();
    await user.keyboard("p");
    await waitFor(() => {
      expect(core.callsOf("set_pinned")).toEqual([{ messageIds: [ANSWER.id], pinned: true }]);
    });
    expect(core.callsOf("conversation_targets")[0]).toEqual({
      rows: [{ mailboxId: 1, messageId: ANSWER.id }],
      intent: { kind: "pin", pinned: true },
    });

    list.focus();
    await user.keyboard("b");
    const picker = await screen.findByTestId("snooze-picker");
    await user.click(within(picker).getByTestId("snooze-tomorrow"));
    await waitFor(() => {
      expect(core.callsOf("snooze_messages")).toEqual([{ messageIds: [ANSWER.id, FIRST.id], choice: { kind: "tomorrow" } }]);
    });
  });
});
