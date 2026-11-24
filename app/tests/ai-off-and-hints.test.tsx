import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AI_PROVIDER_COMMANDS,
  CLOUD_PROFILE_ID,
  makeAiProfile,
  makeCloudProfile,
  makeHint,
  makeTransfer,
  type AiFakeOptions,
} from "./support/ai";
import { makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { closeOpenedMessage, renderApp } from "./support/render";
import { optionName } from "./support/names";

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const LOCAL = makeAiProfile();
const ALL_LOCAL = { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id };
const ALL_CLOUD = { summary: CLOUD_PROFILE_ID, reply_suggestion: CLOUD_PROFILE_ID, writing_help: CLOUD_PROFILE_ID, search: CLOUD_PROFILE_ID };
const now = Date.now();
const ADA_MAIL = makeMessage({ id: 3000, accountId: ADA.id, mailboxId: 1, subject: "Privat", date: now - 60_000 });
const WORK_MAIL = makeMessage({ id: 2000, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: now - 120_000 });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const everything: AiFakeOptions = {
  summary: (id) => ({ keyPoints: [{ text: "x", sources: [id] }], openQuestions: [], deadlines: [], dropped: 0, transfer: makeTransfer() }),
  reply: (id) => ({ text: "x", inReplyTo: id, transfer: makeTransfer() }),
  writing: (passage) => ({ text: passage, transfer: makeTransfer() }),
  answer: () => ({ found: false, statements: [], query: "", candidates: 0, excluded: 0, dropped: 0, transfers: [] }),
};

function setup(ai: AiFakeOptions, extra: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({
    accounts: [ADA, WORK],
    mailboxes: [...standardMailboxes(), makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" })],
    messages: { 1: [ADA_MAIL], 20: [WORK_MAIL] },
    views: Object.fromEntries([ADA_MAIL, WORK_MAIL].map((m) => [m.id, makeView(m)])),
    readingPreferences: { markReadOnOpen: "never" },
    ai: { ...everything, ...ai },
    ...extra,
  });
  return { core, ...renderApp() };
}

const aiCalls = (core: ReturnType<typeof mockCore>) => core.calls.filter((c) => AI_PROVIDER_COMMANDS.has(c.cmd) || c.cmd === "ai_open_reply_draft");

async function paletteLabels(user: ReturnType<typeof renderApp>["user"], query: string) {
  await user.keyboard("{Meta>}k{/Meta}");
  const palette = await screen.findByRole("dialog", { name: "Command palette" });
  await user.keyboard(query);
  const labels = within(palette)
    .queryAllByRole("option")
    .map((option) => option.textContent);
  await user.keyboard("{Escape}");
  return labels;
}

describe("AI off: nothing reaches a provider", () => {
  it("calls no AI function whatever the user does, even with providers and functions set up", async () => {
    const { core, user } = setup({ enabled: false, profiles: [LOCAL, makeCloudProfile({ needsConsent: false, consentAt: 1 })], functions: ALL_LOCAL, cloudAccounts: [ADA.id, WORK.id] });
    // Read a message.
    await user.click(await screen.findByRole("option"));
    await screen.findByRole("region", { name: "Message" });
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    // The palette has no AI commands.
    const labels = await paletteLabels(user, "Summar");
    expect(labels.some((label) => label?.includes("Summarise conversation"))).toBe(false);
    expect((await paletteLabels(user, "Ask about")).some((label) => label?.includes("Ask about your mail"))).toBe(false);
    // Write: no writing help.
    await user.click(screen.getByRole("button", { name: "New message" }));
    const composer = await screen.findByTestId("composer");
    await user.type(within(composer).getByLabelText("Message"), "Some text to improve");
    expect(within(composer).queryByRole("button", { name: "Writing help" })).toBeNull();
    await user.click(within(composer).getByTestId("composer-close"));
    // Settings: the switch is off and nothing there can reach a provider.
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "AI in the app" }));
    const panel = screen.getByRole("tabpanel", { name: "AI in the app" });
    await within(panel).findByRole("switch", { name: "Use AI in the app" });
    expect(within(panel).queryByRole("button", { name: "Test connection" })).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Load models" })).toBeNull();
    await user.click(within(panel).getByRole("tab", { name: "Request log" }));
    await within(panel).findByText("No requests yet");

    expect(aiCalls(core)).toEqual([]);
    expect(core.ai.providerCalls).toEqual([]);
  });
});

describe("cloud permission per account", () => {
  it("sends nothing of an account without permission to a cloud provider, also from the unified inbox", async () => {
    const { core, user } = setup({
      enabled: true,
      profiles: [makeCloudProfile({ needsConsent: false, consentAt: 1 })],
      functions: ALL_CLOUD,
      cloudAccounts: [WORK.id],
    });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(await screen.findByRole("button", { name: /^Show the inbox of/ }));
    const choices = await screen.findByRole("list", { name: "Show the inbox of" });
    await user.click(within(choices).getByRole("button", { name: "All accounts" }));
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(2);
    });

    // Ada's mail: no permission, so the panel refuses before anything happens.
    const adaRow = within(list).getAllByRole("option").find((o) => optionName(o).includes("Privat"));
    await user.click(adaRow as HTMLElement);
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(panel).findByText("About: Privat");
    expect(within(panel).getByRole("button", { name: "Summarise conversation" }).hasAttribute("disabled")).toBe(true);
    expect(within(panel).getByRole("button", { name: "Suggest a reply" }).hasAttribute("disabled")).toBe(true);
    await user.click(within(panel).getByRole("button", { name: "Summarise conversation" }));
    // A question over all accounts only covers the permitted one.
    await user.click(within(panel).getByRole("radio", { name: "All accounts" }));
    await user.type(within(panel).getByRole("textbox", { name: "Question" }), "What is new?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    await within(panel).findByRole("article", { name: "Answer" });
    expect(core.callsOf("ai_ask")).toEqual([{ requestId: expect.any(String) as string, question: "What is new?", accountIds: [WORK.id] }]);
    expect(core.callsOf("ai_summarize_thread")).toEqual([]);
    expect(core.callsOf("ai_suggest_reply")).toEqual([]);

    // Work's mail: permitted, so the summary may go out.
    const workRow = within(list).getAllByRole("option").find((o) => optionName(o).includes("Projekt"));
    await user.click(workRow as HTMLElement);
    await within(panel).findByText("About: Projekt");
    await user.click(within(panel).getByRole("button", { name: "Summarise conversation" }));
    await within(panel).findByRole("article", { name: "Summary" });
    expect(core.callsOf("ai_summarize_thread")).toEqual([{ requestId: expect.any(String) as string, messageId: WORK_MAIL.id }]);
    expect(core.ai.providerCalls.map((c) => c.cmd)).toEqual(["ai_ask", "ai_summarize_thread"]);
  });
});

describe("open-reply hints", () => {
  it("shows a dismissable suggestion above the message, without AI", async () => {
    const { core, user } = setup({ enabled: false, hints: [makeHint({ messageId: ADA_MAIL.id, accountId: ADA.id })] });
    await user.click(await screen.findByRole("option"));
    const pane = await screen.findByRole("region", { name: "Message" });
    const bar = await within(pane).findByTestId("open-reply-bar");
    expect(bar.textContent).toContain("Maybe still open: you asked Grace Hopper 5 days ago and no answer has come yet.");
    expect(bar.textContent).toContain("A suggestion from simple rules, not a fact.");
    await user.click(within(bar).getByRole("button", { name: "Dismiss hint" }));
    await waitFor(() => {
      expect(within(pane).queryByTestId("open-reply-bar")).toBeNull();
    });
    expect(core.callsOf("dismiss_open_reply_hint")).toEqual([{ messageId: ADA_MAIL.id }]);
    expect(core.ai.providerCalls).toEqual([]);
  });

  it("lists the hints in the AI panel while no thread is chosen, and brings a failed dismissal back", async () => {
    const { core, user } = setup(
      {
        enabled: true,
        profiles: [LOCAL],
        functions: ALL_LOCAL,
        hints: [makeHint({ messageId: 77, kind: "unanswered_question", counterpart: { name: null, email: "bob@example.org" }, days: 1 })],
      },
      { fail: (cmd) => (cmd === "dismiss_open_reply_hint" ? { code: "database", message: "db", detail: null } : undefined) },
    );
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    // Stage 18 (task F): the inbox opened with its top message; the hints show without a thread.
    await closeOpenedMessage(user);
    const list = await within(panel).findByRole("region", { name: "Maybe still open" });
    const hint = within(list).getByTestId("open-reply-hint");
    expect(hint.textContent).toContain("Maybe still open: bob@example.org asked you something 1 day ago.");
    expect(within(hint).getByRole("button", { name: "Reply" })).toBeTruthy();
    await user.click(within(hint).getByRole("button", { name: "Dismiss hint" }));
    expect(await within(panel).findByText("The hint could not be dismissed.")).toBeTruthy();
    expect(within(panel).getAllByTestId("open-reply-hint")).toHaveLength(1);
    expect(core.callsOf("dismiss_open_reply_hint")).toEqual([{ messageId: 77 }]);
  });
});

describe("command palette", () => {
  it("offers the AI commands while AI is usable and runs them in the panel", async () => {
    const { core, user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL });
    await screen.findByRole("complementary", { name: "AI assistant" });
    // Stage 18 (task F): without the top message the inbox opened with, the command waits.
    await closeOpenedMessage(user);
    await user.keyboard("{Meta>}k{/Meta}");
    let palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("Summarise conv");
    const entry = await within(palette).findByRole("option", { name: /Summarise conversation/ });
    expect(entry.getAttribute("aria-disabled")).toBe("true");
    await user.keyboard("{Escape}");

    await user.click(await screen.findByRole("option", { name: /Privat/ }));
    await screen.findByRole("region", { name: "Message" });
    await user.keyboard("{Meta>}k{/Meta}");
    palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("Summarise conv");
    await within(palette).findByRole("option", { name: /Summarise conversation/ });
    await user.keyboard("{Enter}");
    const panel = screen.getByRole("complementary", { name: "AI assistant" });
    await within(panel).findByRole("article", { name: "Summary" });
    expect(core.callsOf("ai_summarize_thread")).toEqual([{ requestId: expect.any(String) as string, messageId: ADA_MAIL.id }]);
  });
});
