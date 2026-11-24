import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ThreadListItem, ThreadView } from "../src/services/ipc";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { optionName } from "./support/names";
import { singleThread } from "./support/search";

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
  singleThread(ANSWER, { threadId: 7, messageCount: 3, unreadCount: 1 }),
  singleThread(SINGLE, { threadId: 8 }),
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

/**
 * Opens the folder "Projekte" and turns the conversation view on (the fake core starts with it
 * off; the real default is on, decision 0039). The sorted inbox offers it too. Since stage 17 the
 * list of conversations is a tree.
 */
async function conversations(user: ReturnType<typeof renderApp>["user"]) {
  await screen.findByRole("listbox", { name: "Messages in Inbox" });
  expect(screen.getByRole("button", { name: "Conversations" })).toBeTruthy();
  await user.click(screen.getByTestId("rail-folders"));
  await user.click(await screen.findByTestId("mailbox-5"));
  await screen.findByRole("listbox", { name: "Messages in Projekte" });
  const toggle = screen.getByRole("button", { name: "Conversations" });
  expect(toggle.getAttribute("aria-pressed")).toBe("false");
  await user.click(toggle);
  expect(toggle.getAttribute("aria-pressed")).toBe("true");
  const list = await screen.findByRole("tree", { name: "Messages in Projekte" });
  await waitFor(() => {
    expect(within(list).getAllByRole("treeitem")).toHaveLength(2);
  });
  return list;
}

describe("conversation view", () => {
  it("shows one row per conversation with its replies, from the core's thread list", async () => {
    const { core, user } = setup();
    const list = await conversations(user);
    expect(core.callsOf("list_threads")).toEqual([{ mailboxId: 5, cursor: null, limit: 200 }]);
    const [thread, single] = within(list).getAllByRole("treeitem");
    expect(optionName(thread)).toContain("2 replies");
    expect(within(thread as HTMLElement).getByTestId("row-replies").textContent).toBe("2 replies");
    expect(thread?.getAttribute("aria-expanded")).toBe("false");
    expect(optionName(single)).not.toContain("repl");
    expect(within(single as HTMLElement).queryByTestId("row-replies")).toBeNull();
    expect(single?.hasAttribute("aria-expanded")).toBe(false);

    // Off again: single messages in a list box.
    await user.click(screen.getByRole("button", { name: "Conversations" }));
    await waitFor(() => {
      expect(within(screen.getByRole("listbox", { name: "Messages in Projekte" })).getAllByRole("option")).toHaveLength(3);
    });
  });

  it("opens one message of the conversation, the first unread; the rest unfold below the row", async () => {
    const { core, user } = setup();
    const list = await conversations(user);
    const row = within(list).getAllByRole("treeitem")[0] as HTMLElement;
    await user.click(row);

    // The conversation is opened by message id; the pane shows its unread answer alone.
    await waitFor(() => {
      expect(core.callsOf("get_thread")).toEqual([{ messageId: ANSWER.id, mailboxId: 5 }]);
    });
    await waitFor(() => {
      expect(core.callsOf("get_message")).toEqual([{ messageId: ANSWER.id, format: "auto" }]);
    });
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(within(pane).getByRole("toolbar", { name: "Message actions" })).toBeTruthy();
    expect(screen.queryByTestId("thread-entries")).toBeNull();

    // Unfolded: every message, oldest first, the answer from Sent named by its folder.
    await user.click(within(row).getByTestId("row-replies"));
    await waitFor(() => {
      expect(within(list).getAllByTestId("child-row")).toHaveLength(3);
    });
    const children = within(list).getAllByTestId("child-row");
    expect(children.map((child) => optionName(child))).toEqual([
      expect.stringMatching(/^Anna, Wie wäre es mit Montag\?, /),
      expect.stringMatching(/^Ada Lovelace, in Sent, /),
      expect.stringMatching(/^Unread, Anna, Montag passt, /),
    ]);
    expect(within(children[1] as HTMLElement).getByTestId("child-folder").textContent).toBe("Sent");

    // A child row opens that message alone, with the actions of its own folder.
    await user.click(children[1] as HTMLElement);
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: REPLY.id, format: "auto" });
    });
    const replyPane = await screen.findByRole("region", { name: "Message" });
    expect(within(replyPane).getByRole("toolbar", { name: "Message actions" })).toBeTruthy();
  });

  it("says when the conversation cannot be loaded and still shows the row's message", async () => {
    const { core, user } = setup({ failures: { get_thread: ipcError("database") } });
    const list = await conversations(user);
    await user.click(within(list).getAllByRole("treeitem")[0] as HTMLElement);
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The conversation could not be loaded")).toBeTruthy();
    await waitFor(() => {
      expect(core.callsOf("get_message")).toContainEqual({ messageId: ANSWER.id, format: "auto" });
    });
  });

  it("names how much of a very long conversation it unfolds", async () => {
    const { user } = setup({
      search: { threads: { 5: THREADS }, threadViews: { [ANSWER.id]: { ...THREAD, total: 1500 } } },
    });
    const list = await conversations(user);
    await user.click(within(within(list).getAllByRole("treeitem")[0] as HTMLElement).getByTestId("row-replies"));
    expect(await within(list).findByText("Shows the newest 3 of 1500 messages.")).toBeTruthy();
  });
});
