import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ThreadListItem, ThreadView } from "../src/services/ipc";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { optionName } from "./support/names";

const ACCOUNT = makeAccount({ writesEnabled: true });
const now = Date.now();
const FIRST = makeMessage({ id: 1000, mailboxId: 5, subject: "Plan", from: { name: "Anna", email: "anna@example.org" }, date: now - 3 * 3_600_000, preview: "Wie wäre es mit Montag?" });
// The reply in Sent: part of the conversation, but not of the inbox.
const REPLY = makeMessage({ id: 1100, mailboxId: 2, subject: "Re: Plan", from: { name: "Ada Lovelace", email: "ada@vicemail.test" }, date: now - 2 * 3_600_000 });
const ANSWER = makeMessage({ id: 1200, mailboxId: 5, subject: "Re: Plan", from: { name: "Anna", email: "anna@example.org" }, date: now - 3_600_000, unread: true, preview: "Montag passt" });
const SINGLE = makeMessage({ id: 900, mailboxId: 5, subject: "Allein", date: now - 5 * 3_600_000 });

const THREAD: ThreadView = {
  threadId: 7,
  messages: [FIRST, REPLY, ANSWER].map((message) => ({ message, mailboxIds: [message.mailboxId] })),
  total: 3,
};
const THREADS: ThreadListItem[] = [
  { message: ANSWER, threadId: 7, messageCount: 3, unreadCount: 1, lastActivity: ANSWER.date },
  { message: SINGLE, threadId: 8, messageCount: 1, unreadCount: 0, lastActivity: SINGLE.date },
];

function setup(options: Parameters<typeof mockCore>[0] = {}) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ACCOUNT],
    mailboxes: standardMailboxes(),
    messages: { 1: [makeMessage({ id: 500, subject: "In the inbox", date: now })], 5: [ANSWER, FIRST, SINGLE], 2: [REPLY] },
    views: Object.fromEntries([FIRST, REPLY, ANSWER, SINGLE].map((m) => [m.id, makeView(m)])),
    search: { threads: { 5: THREADS }, threadViews: { [ANSWER.id]: THREAD } },
    ...options,
  });
  return { core, ...renderApp() };
}

/** Opens the folder "Projekte" (conversations are for plain folders, not the sorted inbox). */
async function conversations(user: ReturnType<typeof renderApp>["user"]) {
  await screen.findByRole("listbox", { name: "Messages in Inbox" });
  expect(screen.queryByRole("button", { name: "Conversations" })).toBeNull();
  await user.click(screen.getByTestId("rail-folders"));
  await user.click(await screen.findByTestId("mailbox-5"));
  await screen.findByRole("listbox", { name: "Messages in Projekte" });
  const toggle = screen.getByRole("button", { name: "Conversations" });
  expect(toggle.getAttribute("aria-pressed")).toBe("false");
  await user.click(toggle);
  expect(toggle.getAttribute("aria-pressed")).toBe("true");
  const list = await screen.findByRole("listbox", { name: "Messages in Projekte" });
  await waitFor(() => {
    expect(within(list).getAllByRole("option")).toHaveLength(2);
  });
  return list;
}

describe("conversation view", () => {
  it("shows one row per conversation with its size, from the core's thread list", async () => {
    const { core, user } = setup();
    const list = await conversations(user);
    expect(core.callsOf("list_threads")).toEqual([{ mailboxId: 5, cursor: null, limit: 200 }]);
    const [thread, single] = within(list).getAllByRole("option");
    expect(optionName(thread)).toContain("conversation with 3 messages");
    expect(within(thread as HTMLElement).getByTestId("row-thread-count").textContent).toBe("3");
    expect(optionName(single)).not.toContain("conversation");
    expect(within(single as HTMLElement).queryByTestId("row-thread-count")).toBeNull();

    // Off again: single messages.
    await user.click(screen.getByRole("button", { name: "Conversations" }));
    await waitFor(() => {
      expect(within(screen.getByRole("listbox", { name: "Messages in Projekte" })).getAllByRole("option")).toHaveLength(3);
    });
  });

  it("opens the conversation by message id, oldest first, the first unread message open", async () => {
    const { core, user } = setup();
    const list = await conversations(user);
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);

    const entries = await screen.findByRole("region", { name: "Conversation with 3 messages" });
    expect(core.callsOf("get_thread")).toEqual([{ messageId: ANSWER.id, mailboxId: 5 }]);
    const buttons = within(entries).getAllByTestId("thread-entry");
    expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
      expect.stringMatching(/^Show message from Anna, /),
      expect.stringMatching(/^Show message from Ada Lovelace, /),
      expect.stringMatching(/^Show message from Anna, .*, Unread$/),
    ]);
    // The unread answer is open below; the older ones are folded.
    expect(buttons[2]?.getAttribute("aria-current")).toBe("true");
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: ANSWER.id, format: "auto" });
    });
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(within(pane).getByRole("toolbar", { name: "Message actions" })).toBeTruthy();

    // An older one: it opens instead. The reply lies in Sent, so the inbox's actions do not apply.
    await user.click(buttons[1] as HTMLElement);
    expect(buttons[1]?.getAttribute("aria-current")).toBe("true");
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: REPLY.id, format: "auto" });
    });
    const replyPane = await screen.findByRole("region", { name: "Message" });
    expect(within(replyPane).queryByRole("toolbar", { name: "Message actions" })).toBeNull();
  });

  it("says when the conversation cannot be loaded and still shows the message", async () => {
    const { core, user } = setup({ failures: { get_thread: ipcError("database") } });
    const list = await conversations(user);
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The conversation could not be loaded")).toBeTruthy();
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: ANSWER.id, format: "auto" });
    });
  });

  it("names how much of a very long conversation it shows", async () => {
    const { user } = setup({
      search: { threads: { 5: THREADS }, threadViews: { [ANSWER.id]: { ...THREAD, total: 1500 } } },
    });
    const list = await conversations(user);
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    expect(await screen.findByText("Shows the newest 3 of 1500 messages.")).toBeTruthy();
  });
});
