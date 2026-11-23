import { QueryClient } from "@tanstack/react-query";
import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { handleComfortEvent } from "../src/features/comfort/events";
import type { AiAvailability, MessageListItem } from "../src/services/ipc";
import { useAiPanelStore } from "../src/stores/aiPanel";
import { makeAiProfile, makeTransfer, type AiFakeOptions } from "./support/ai";
import { makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

// Decision 0027: messages by id for the AI panel's sources, `aiSettingsChanged`, and the click on
// a system notification that opens the mail itself.

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const ADA = makeAccount();
const LOCAL = makeAiProfile();
const ALL_LOCAL = { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id };
const ON: AiAvailability = { enabled: true, summary: true, replySuggestion: true, writingHelp: true, search: true };
const OFF: AiAvailability = { enabled: false, summary: false, replySuggestion: false, writingHelp: false, search: false };

function mail(overrides: Partial<MessageListItem> = {}): MessageListItem {
  return makeMessage({
    id: 5001,
    subject: "Budget 2027",
    from: { name: "Grace Hopper", email: "grace@example.org" },
    date: new Date(2026, 10, 20, 14, 30).getTime(),
    ...overrides,
  });
}

function setup(ai: AiFakeOptions) {
  const message = mail();
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [message] },
    views: { [message.id]: makeView(message) },
    readingPreferences: { markReadOnOpen: "never" },
    ai,
  });
  return { core, message, ...renderApp() };
}

const answerWith = (sources: number[]): NonNullable<AiFakeOptions["answer"]> => () => ({
  found: true,
  statements: [{ text: "The budget is due on Friday.", sources }],
  query: "budget",
  candidates: 2,
  excluded: 0,
  dropped: 0,
  transfers: [makeTransfer({ function: "search", mails: 2 })],
});

async function ask(user: ReturnType<typeof renderApp>["user"]) {
  const panel = await screen.findByRole("complementary", { name: "AI assistant" });
  await user.type(within(panel).getByRole("textbox", { name: "Question" }), "When is the budget due?");
  await user.click(within(panel).getByRole("button", { name: "Ask" }));
  return within(panel).findByRole("article", { name: "Answer" });
}

describe("sources and messages by id (decision 0027)", () => {
  it("names the sources the core has and opens one the main window does not show in a window of its own", async () => {
    const { core, user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, answer: answerWith([5001, 4000]) });
    const answer = await ask(user);
    const named = await within(answer).findByRole("button", { name: /^Show source: Grace Hopper, / });
    expect(within(answer).getByRole("button", { name: "Show source: Mail 2" })).toBeTruthy();
    expect(core.callsOf("get_messages_by_ids")).toEqual([{ messageIds: [5001, 4000] }]);

    await user.click(named);
    await waitFor(() => {
      expect(core.callsOf("open_message_window")).toEqual([{ accountId: ADA.id, messageId: 5001 }]);
    });
    expect(core.ui.messageWindows).toEqual([`${ADA.id}/5001`]);
    expect(useAiPanelStore.getState().sourceRequest).toBeNull();

    // A source the core does not know (any more): a notice says so, no window opens.
    await user.click(within(answer).getByRole("button", { name: "Show source: Mail 2" }));
    expect(await screen.findByText("The message could not be opened in a new window.")).toBeTruthy();
    expect(core.callsOf("open_message_window")).toHaveLength(1);
  });

  it("shows a source in the main window when it is the selected message", async () => {
    const { core, user, message } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, answer: answerWith([5001]) });
    await user.click(await screen.findByRole("option"));
    await screen.findByRole("region", { name: "Message" });
    const answer = await ask(user);
    await user.click(await within(answer).findByRole("button", { name: /^Show source: Grace Hopper, / }));
    expect(useAiPanelStore.getState().sourceRequest?.messageId).toBe(message.id);
    expect(core.callsOf("open_message_window")).toEqual([]);
  });
});

describe("aiSettingsChanged (decision 0027)", () => {
  it("brings the AI panel and its commands in and out as the core reports what is usable", async () => {
    const { core } = setup({ enabled: false, profiles: [LOCAL], functions: ALL_LOCAL });
    await screen.findByRole("navigation", { name: "Views" });
    await waitFor(() => {
      expect(core.callsOf("get_ai_availability").length).toBeGreaterThan(0);
    });
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    const settingsLoads = core.callsOf("get_ai_settings").length;

    // Changed in another window (or the settings): this window follows without asking.
    await emitCore({ type: "aiSettingsChanged", availability: ON });
    expect(await screen.findByRole("complementary", { name: "AI assistant" })).toBeTruthy();
    await waitFor(() => {
      expect(core.callsOf("get_ai_settings").length).toBeGreaterThan(settingsLoads);
    });

    await emitCore({ type: "aiSettingsChanged", availability: OFF });
    await waitFor(() => {
      expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    });
  });
});

describe("a click on a system notification (decision 0027)", () => {
  it("shows the folder and opens the mail itself in a window of its own", async () => {
    const { core } = setup({ enabled: false });
    await screen.findByRole("navigation", { name: "Views" });
    await emitCore({ type: "openMessageRequested", accountId: ADA.id, mailboxId: 1, messageId: 5001 });
    await waitFor(() => {
      expect(core.callsOf("open_message_window")).toEqual([{ accountId: ADA.id, messageId: 5001 }]);
    });
    expect(within(screen.getByRole("banner")).getByText("Inbox")).toBeTruthy();
  });

  it("is answered by the main window only", () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), messages: { 1: [mail()] } });
    window.history.replaceState(null, "", `/?window=message&accountId=${ADA.id}&messageId=5001`);
    handleComfortEvent(new QueryClient(), { type: "openMessageRequested", accountId: ADA.id, mailboxId: 1, messageId: 5001 });
    expect(core.callsOf("open_message_window")).toEqual([]);
  });
});
