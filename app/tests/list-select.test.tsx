import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageListItem } from "../src/services/ipc";
import {
  LIST_SELECT_WAIT_MS,
  selectInList,
  selectWhenListed,
  useListSelectStore,
  type ListSelectAnswerer,
} from "../src/stores/listSelect";
import { useUiStore } from "../src/stores/ui";
import { makeAiProfile, makeTransfer, type AiFakeOptions } from "./support/ai";
import { makeAccount, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

// Stage 9, decision 0028: the message list selects a message by id, so a click on a system
// notification and a source of an AI answer show the mail in the list instead of a window.

/** jsdom has no `Element.scrollTo`; the virtualizer scrolls with it. Offsets asked for. */
const scrolls: number[] = [];

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  // The virtualizer scrolls at most to `scrollHeight - clientHeight` (0 in jsdom).
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(100_000);
  scrolls.length = 0;
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    writable: true,
    value(this: HTMLElement, options: ScrollToOptions) {
      if (options.top === undefined) return;
      scrolls.push(options.top);
      this.scrollTop = options.top;
    },
  });
});

afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
});

const ADA = makeAccount();
/** "Projekte", a plain folder (the inbox is the sorted inbox with bundles). */
const PROJECTS = 5;
const LOCAL = makeAiProfile();
const ALL_LOCAL = { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id };

function folderWith(count: number) {
  const messages = makeMessages(count, { mailboxId: PROJECTS });
  const mailboxes = standardMailboxes().map((m) => (m.id === PROJECTS ? { ...m, total: count } : m));
  return { messages, mailboxes };
}

function optionOf(list: HTMLElement, subject: string): HTMLElement {
  const option = within(list)
    .getAllByRole("option")
    .find((element) => within(element).queryByText(subject));
  if (!option) throw new Error(`no option for ${subject}`);
  return option;
}

describe("the message list selects a message by id", () => {
  it("selects a loaded message like a click, scrolls it into view and opens it; others are refused", async () => {
    const { messages, mailboxes } = folderWith(150);
    const target = messages[119] as MessageListItem;
    mockCore({ accounts: [ADA], mailboxes, messages: { [PROJECTS]: messages }, views: { [target.id]: makeView(target) } });
    useUiStore.getState().showMailbox(ADA.id, PROJECTS);
    renderApp();
    const list = await screen.findByRole("listbox", { name: "Messages in Projekte" });
    await within(list).findByText("Message 1");
    expect(within(list).queryByText("Message 120")).toBeNull();

    let selected = false;
    act(() => {
      selected = selectInList(target.id);
    });
    expect(selected).toBe(true);
    await waitFor(() => {
      expect(optionOf(list, "Message 120").getAttribute("aria-selected")).toBe("true");
    });
    // Far below the first screen: the list scrolled to it, and the reading pane shows it.
    expect(Math.max(...scrolls)).toBeGreaterThan(100 * 44);
    expect(await screen.findByRole("region", { name: "Message" })).toBeTruthy();

    act(() => {
      selected = selectInList(999_999);
    });
    expect(selected).toBe(false);
    expect(optionOf(list, "Message 120").getAttribute("aria-selected")).toBe("true");
  });

  it("selects a notification's mail in its folder once that list has loaded, without a window", async () => {
    const { messages, mailboxes } = folderWith(30);
    const target = messages[12] as MessageListItem;
    const inbox = makeMessages(3, { newest: Date.now() - 30 * 86_400_000 });
    const core = mockCore({ accounts: [ADA], mailboxes, messages: { 1: inbox, [PROJECTS]: messages }, views: { [target.id]: makeView(target) } });
    renderApp();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });

    // The inbox is on screen: the folder changes and its list loads after the event.
    await emitCore({ type: "openMessageRequested", accountId: ADA.id, mailboxId: PROJECTS, messageId: target.id });
    const list = await screen.findByRole("listbox", { name: "Messages in Projekte" });
    await waitFor(() => {
      expect(optionOf(list, "Message 13").getAttribute("aria-selected")).toBe("true");
    });
    expect(await screen.findByRole("region", { name: "Message" })).toBeTruthy();
    expect(core.callsOf("open_message_window")).toEqual([]);
    expect(useListSelectStore.getState().pending).toBeNull();
  });

  it("selects at once when the notification's folder is already shown", async () => {
    const { messages, mailboxes } = folderWith(30);
    const target = messages[3] as MessageListItem;
    const core = mockCore({ accounts: [ADA], mailboxes, messages: { [PROJECTS]: messages }, views: { [target.id]: makeView(target) } });
    useUiStore.getState().showMailbox(ADA.id, PROJECTS);
    renderApp();
    const list = await screen.findByRole("listbox", { name: "Messages in Projekte" });
    await within(list).findByText("Message 1");

    await emitCore({ type: "openMessageRequested", accountId: ADA.id, mailboxId: PROJECTS, messageId: target.id });
    expect(optionOf(list, "Message 4").getAttribute("aria-selected")).toBe("true");
    expect(core.callsOf("open_message_window")).toEqual([]);
  });

  it("still opens a window for a notification's mail the folder's list does not have", async () => {
    const elsewhere = makeMessage({ id: 7001, mailboxId: PROJECTS, subject: "Elsewhere" });
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: makeMessages(5), [PROJECTS]: [elsewhere] },
    });
    renderApp();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    // Said to be in the inbox, but not among its rows (moved meanwhile, or on no loaded page).
    await emitCore({ type: "openMessageRequested", accountId: ADA.id, mailboxId: 1, messageId: elsewhere.id });
    await waitFor(() => {
      expect(core.callsOf("open_message_window")).toEqual([{ accountId: ADA.id, messageId: elsewhere.id }]);
    });
  });
});

describe("an AI source in the main window (decision 0028)", () => {
  it("is selected in the list when the list has it, and opens in a window when it does not", async () => {
    const { messages, mailboxes } = folderWith(20);
    const listed = messages[7] as MessageListItem;
    const sent = makeMessage({ id: 8001, mailboxId: 2, subject: "Sent elsewhere", from: { name: "Ada", email: "ada@example.org" } });
    const ai: AiFakeOptions = {
      enabled: true,
      profiles: [LOCAL],
      functions: ALL_LOCAL,
      answer: () => ({
        found: true,
        statements: [{ text: "The plan was agreed.", sources: [listed.id, sent.id] }],
        query: "plan",
        candidates: 2,
        excluded: 0,
        dropped: 0,
        transfers: [makeTransfer({ function: "search", mails: 2 })],
      }),
    };
    const core = mockCore({
      accounts: [ADA],
      mailboxes,
      messages: { [PROJECTS]: messages, 2: [sent] },
      views: { [listed.id]: makeView(listed) },
      readingPreferences: { markReadOnOpen: "never" },
      ai,
    });
    useUiStore.getState().showMailbox(ADA.id, PROJECTS);
    const { user } = renderApp();
    const list = await screen.findByRole("listbox", { name: "Messages in Projekte" });
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await user.type(within(panel).getByRole("textbox", { name: "Question" }), "Was the plan agreed?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    const answer = await within(panel).findByRole("article", { name: "Answer" });
    const [first, second] = await within(answer).findAllByRole("button", { name: /^Show source: Sender 8, |^Show source: Ada, / });

    await user.click(first as HTMLElement);
    await waitFor(() => {
      expect(optionOf(list, "Message 8").getAttribute("aria-selected")).toBe("true");
    });
    expect(core.callsOf("open_message_window")).toEqual([]);

    await user.click(second as HTMLElement);
    await waitFor(() => {
      expect(core.callsOf("open_message_window")).toEqual([{ accountId: ADA.id, messageId: sent.id }]);
    });
  });
});

describe("requests that wait for a list", () => {
  function answerer(overrides: Partial<ListSelectAnswerer> = {}): ListSelectAnswerer {
    return {
      scope: { accountId: ADA.id, mailboxId: PROJECTS, inbox: false },
      loaded: () => true,
      settled: () => true,
      select: () => true,
      ...overrides,
    };
  }

  it("fall back once when no list of the folder shows in time, and a late list does not answer", () => {
    vi.useFakeTimers();
    const fallback = vi.fn();
    const select = vi.fn(() => true);
    selectWhenListed({ accountId: ADA.id, mailboxId: PROJECTS, messageId: 1, fallback });
    expect(useListSelectStore.getState().pending?.messageId).toBe(1);
    vi.advanceTimersByTime(LIST_SELECT_WAIT_MS - 1);
    expect(fallback).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fallback).toHaveBeenCalledTimes(1);
    expect(useListSelectStore.getState().pending).toBeNull();
    useListSelectStore.getState().register("late", answerer({ select }));
    vi.advanceTimersByTime(LIST_SELECT_WAIT_MS * 2);
    expect(select).not.toHaveBeenCalled();
    expect(fallback).toHaveBeenCalledTimes(1);
  });

  it("are answered at once by a settled list of their folder; other folders and mixed lists do not count", () => {
    vi.useFakeTimers();
    const store = useListSelectStore.getState();
    const other = vi.fn(() => true);
    const mixed = vi.fn(() => true);
    const busy = vi.fn(() => true);
    store.register("other", answerer({ scope: { accountId: ADA.id, mailboxId: 2, inbox: false }, select: other }));
    store.register("mixed", answerer({ scope: { accountId: null, mailboxId: null, inbox: false }, select: mixed }));
    store.register("busy", answerer({ settled: () => false, select: busy }));
    const fallback = vi.fn();
    selectWhenListed({ accountId: ADA.id, mailboxId: PROJECTS, messageId: 2, fallback });
    expect([other, mixed, busy].map((fn) => fn.mock.calls.length)).toEqual([0, 0, 0]);
    expect(useListSelectStore.getState().pending?.messageId).toBe(2);

    const select = vi.fn(() => false);
    useListSelectStore.getState().register("folder", answerer({ select }));
    selectWhenListed({ accountId: ADA.id, mailboxId: PROJECTS, messageId: 3, fallback });
    expect(select).toHaveBeenCalledWith(3);
    // It did not have the message: the fallback of this request runs, the replaced one never.
    expect(fallback).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(LIST_SELECT_WAIT_MS * 2);
    expect(fallback).toHaveBeenCalledTimes(1);
    expect(useListSelectStore.getState().pending).toBeNull();

    // Without a folder the account's inbox list is meant.
    const inbox = vi.fn(() => true);
    useListSelectStore.getState().register("inbox", answerer({ scope: { accountId: ADA.id, mailboxId: 1, inbox: true }, select: inbox }));
    selectWhenListed({ accountId: ADA.id, mailboxId: null, messageId: 4, fallback });
    expect(inbox).toHaveBeenCalledWith(4);
    // The most recently mounted list answers immediate requests (an AI source).
    expect(selectInList(5)).toBe(true);
    expect(inbox).toHaveBeenLastCalledWith(5);
  });
});
