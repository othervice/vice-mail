import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiSummary, MessageListItem } from "../src/services/ipc";
import { useAiPanelStore } from "../src/stores/aiPanel";
import {
  AI_PROVIDER_COMMANDS,
  CLOUD_PROFILE_ID,
  makeAiProfile,
  makeCloudProfile,
  makeTransfer,
  type AiFakeOptions,
} from "./support/ai";
import { makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

const ADA = makeAccount();
const LOCAL = makeAiProfile();
const ALL_LOCAL = { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id };
const ALL_CLOUD = { summary: CLOUD_PROFILE_ID, reply_suggestion: CLOUD_PROFILE_ID, writing_help: CLOUD_PROFILE_ID, search: CLOUD_PROFILE_ID };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

function mail(overrides: Partial<MessageListItem> = {}): MessageListItem {
  return makeMessage({
    id: 5001,
    subject: "Budget 2027",
    from: { name: "Grace Hopper", email: "grace@example.org" },
    date: new Date(2026, 10, 20, 14, 30).getTime(),
    ...overrides,
  });
}

function summaryOf(messageId: number, overrides: Partial<AiSummary> = {}): AiSummary {
  return {
    keyPoints: [{ text: "The budget is due on Friday.", sources: [messageId] }],
    openQuestions: [{ text: "<img src=x onerror=alert(1)> Who signs it?", sources: [messageId] }],
    deadlines: [{ text: "Send the budget", due: "Friday", sources: [messageId] }],
    dropped: 1,
    transfer: makeTransfer({ attachmentNames: 1, quotesRemoved: 2, trimming: { mailsLeftOut: 2, mailsShortened: 1, charsRemoved: 900 } }),
    ...overrides,
  };
}

function setup(ai: AiFakeOptions, extra: Parameters<typeof mockCore>[0] = {}) {
  const message = mail();
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [message] },
    views: { [message.id]: makeView(message) },
    readingPreferences: { markReadOnOpen: "never" },
    ai,
    ...extra,
  });
  return { core, message, ...renderApp() };
}

async function openPanelOn(user: ReturnType<typeof renderApp>["user"]) {
  const option = await screen.findByRole("option");
  await user.click(option);
  await screen.findByRole("region", { name: "Message" });
  const panel = await screen.findByRole("complementary", { name: "AI assistant" });
  await within(panel).findByText("About: Budget 2027");
  return panel;
}

const providerCalls = (core: ReturnType<typeof mockCore>) => core.calls.filter((c) => AI_PROVIDER_COMMANDS.has(c.cmd));

describe("AI panel", () => {
  it("is not there while AI is off, even with a provider and functions set up", async () => {
    const { core, user } = setup({ enabled: false, profiles: [LOCAL], functions: ALL_LOCAL });
    await user.click(await screen.findByRole("option"));
    await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(core.callsOf("get_ai_availability").length).toBeGreaterThan(0);
    });
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Show AI assistant" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Hide AI assistant" })).toBeNull();
    expect(providerCalls(core)).toEqual([]);
  });

  it("is not there while AI is on but no function has a provider with a model", async () => {
    const { core, user } = setup({ enabled: true, profiles: [makeAiProfile({ model: "" })], functions: ALL_LOCAL });
    await user.click(await screen.findByRole("option"));
    await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(core.callsOf("get_ai_availability").length).toBeGreaterThan(0);
    });
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
  });

  it("summarises the selected thread: progress first, then cards whose statements link to their mail", async () => {
    const { core, user, message } = setup({
      enabled: true,
      profiles: [LOCAL],
      functions: ALL_LOCAL,
      summary: (id) => summaryOf(id),
    });
    const panel = await openPanelOn(user);
    expect(within(panel).queryByTestId("ai-no-thread")).toBeNull();

    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const card = await within(panel).findByRole("article", { name: "Summary" });
    expect(core.callsOf("ai_summarize_thread")).toEqual([{ requestId: expect.any(String) as string, messageId: message.id }]);

    const keyPoints = within(card).getByRole("region", { name: "Key points" });
    expect(keyPoints.textContent).toContain("The budget is due on Friday.");
    expect(within(card).getByRole("region", { name: "Deadlines" }).textContent).toContain("Send the budget (due: Friday)");
    // AI output is text only: markup stays visible text, nothing is parsed or loaded.
    const questions = within(card).getByRole("region", { name: "Open questions" });
    expect(questions.textContent).toContain("<img src=x onerror=alert(1)> Who signs it?");
    expect(panel.querySelector("img")).toBeNull();
    expect(within(card).getByTestId("ai-dropped").textContent).toBe("1 statement without a valid source was left out.");
    // What went where, as the core reported it.
    const sent = within(card).getByTestId("ai-transfer-summary").textContent;
    expect(sent).toContain("Sent: 3 mails");
    expect(sent).toContain("to localhost:11434 (llama3.2)");
    expect(within(card).getAllByText("Local").length).toBeGreaterThan(0);
    // Shortened content is said, never silent.
    const notes = within(card).getByTestId("ai-transfer-notes").textContent;
    expect(notes).toContain("Shortened to fit: 2 mails were left out.");
    expect(notes).toContain("Shortened to fit: 1 long mail was cut short.");
    expect(notes).toContain("Only the name of 1 attachment was sent, not its content.");
    expect(notes).toContain("2 passages quoted from earlier mails were left out.");

    // Every statement names its mail; a click asks the thread to show it.
    const sources = await within(keyPoints).findAllByRole("button", { name: /^Show source: Grace Hopper/ });
    expect(sources).toHaveLength(1);
    await user.click(sources[0] as HTMLElement);
    expect(useAiPanelStore.getState().sourceRequest?.messageId).toBe(message.id);
  });

  it("shows what is being sent while the request runs, and cancels it", async () => {
    const { core, user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, summary: (id) => summaryOf(id), hold: true });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const progress = await within(panel).findByTestId("ai-progress");
    await waitFor(() => {
      expect(within(progress).getByTestId("ai-progress-transfer").textContent).toContain("Sending 3 mails");
    });
    expect(within(progress).getByTestId("ai-progress-transfer").textContent).toContain("to localhost:11434 (llama3.2)");
    expect(within(panel).getByRole("button", { name: "Summarise" }).hasAttribute("disabled")).toBe(true);

    await user.click(within(progress).getByRole("button", { name: "Cancel" }));
    expect(await within(panel).findByTestId("ai-cancelled")).toBeTruthy();
    expect(core.callsOf("cancel_ai_request")).toHaveLength(1);
    expect(core.ai.cancelled).toHaveLength(1);
    expect(within(panel).queryByRole("article", { name: "Summary" })).toBeNull();
  });

  it("suggests a reply that only becomes a draft on “Open draft”, with the edited text", async () => {
    const { core, user, message } = setup(
      {
        enabled: true,
        profiles: [LOCAL],
        functions: ALL_LOCAL,
        reply: (id) => ({ text: "Hi Grace,\nthanks!", inReplyTo: id, transfer: makeTransfer({ function: "reply_suggestion" }) }),
      },
      { compose: { replies: { 5001: { subject: "Re: Budget 2027", to: [{ name: "Grace Hopper", email: "grace@example.org" }] } } } },
    );
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Suggest a reply" }));
    const card = await within(panel).findByRole("article", { name: "Reply suggestion" });
    const text = within(card).getByRole("textbox", { name: "You can change the text before you open it as a draft." });
    expect((text as HTMLTextAreaElement).value).toBe("Hi Grace,\nthanks!");
    // Nothing was created by the AI itself.
    expect(core.callsOf("ai_open_reply_draft")).toEqual([]);
    expect(core.callsOf("create_draft")).toEqual([]);

    await user.type(text, " Ada");
    await user.click(within(card).getByRole("button", { name: "Open draft" }));
    await waitFor(() => {
      expect(core.callsOf("ai_open_reply_draft")).toEqual([{ messageId: message.id, text: "Hi Grace,\nthanks! Ada" }]);
    });
    const composer = await screen.findByTestId("composer");
    expect(within(composer).getByTestId<HTMLTextAreaElement>("composer-text-body").value).toBe("Hi Grace,\nthanks! Ada");
  });

  it("asks before the first request to a cloud provider and sends nothing until the user agrees", async () => {
    const { core, user } = setup({
      enabled: true,
      profiles: [makeCloudProfile()],
      functions: ALL_CLOUD,
      cloudAccounts: [ADA.id],
      summary: (id) => summaryOf(id, { transfer: makeTransfer({ host: "api.openai.com:443", locality: "internet", model: "gpt-5" }) }),
    });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const consent = await within(panel).findByRole("group", { name: "Before the first request to OpenAI" });
    expect(within(consent).getByTestId("ai-consent-what").textContent).toContain("the text of the mails in this conversation");
    expect(within(consent).getByTestId("ai-consent-what").textContent).toContain("Attachments are not sent, only their names");
    expect(within(consent).getByTestId("ai-consent-to").textContent).toBe("To whom: OpenAI at api.openai.com:443, model gpt-5.");
    expect(consent.textContent).toContain("how many mails and how many kilobytes go out");
    expect(document.activeElement).toBe(consent);
    expect(core.callsOf("ai_summarize_thread")).toEqual([]);

    // "Not now" sends nothing and records nothing.
    await user.click(within(consent).getByRole("button", { name: "Not now" }));
    expect(within(panel).queryByRole("group", { name: "Before the first request to OpenAI" })).toBeNull();
    expect(core.callsOf("set_ai_consent")).toEqual([]);
    expect(providerCalls(core)).toEqual([]);

    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    await user.click(await within(panel).findByRole("button", { name: "Agree and send" }));
    await within(panel).findByRole("article", { name: "Summary" });
    expect(core.callsOf("set_ai_consent")).toEqual([{ profileId: CLOUD_PROFILE_ID, granted: true }]);
    expect(core.callsOf("ai_summarize_thread")).toHaveLength(1);
    expect(within(panel).getAllByText("Cloud").length).toBeGreaterThan(0);
  });

  it("shows the consent step when the core answers ai_consent_required", async () => {
    // The settings say consent was given, but the host changed in the meantime: the core refuses.
    const { core, user } = setup({
      enabled: true,
      profiles: [makeCloudProfile({ needsConsent: false, consentAt: 1 })],
      functions: ALL_CLOUD,
      cloudAccounts: [ADA.id],
      summary: (id) => summaryOf(id),
    }, {
      fail: (cmd) => (cmd === "ai_summarize_thread" ? { code: "ai_consent_required", message: "consent", detail: "OpenAI" } : undefined),
    });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    expect(await within(panel).findByTestId("ai-consent")).toBeTruthy();
    expect(within(panel).queryByRole("alert")).toBeNull();
    expect(core.ai.providerCalls).toEqual([]);
  });

  it("never asks a cloud provider about mail of an account without cloud permission", async () => {
    const { core, user } = setup({
      enabled: true,
      profiles: [makeCloudProfile({ needsConsent: false, consentAt: 1 })],
      functions: ALL_CLOUD,
      summary: (id) => summaryOf(id),
      reply: (id) => ({ text: "x", inReplyTo: id, transfer: makeTransfer() }),
    });
    const panel = await openPanelOn(user);
    expect(within(panel).getByTestId("ai-not-permitted").textContent).toContain("This account's mail does not go to cloud providers");
    const summarise = within(panel).getByRole("button", { name: "Summarise" });
    const reply = within(panel).getByRole("button", { name: "Suggest a reply" });
    expect(summarise.hasAttribute("disabled")).toBe(true);
    expect(reply.hasAttribute("disabled")).toBe(true);
    await user.click(summarise);
    await user.click(reply);
    // The question box would only send the question: with no permitted account it sends nothing.
    await user.type(within(panel).getByRole("textbox", { name: "Question" }), "When is the meeting?");
    expect(within(panel).getByRole("button", { name: "Ask" }).hasAttribute("disabled")).toBe(true);
    expect(within(panel).getByTestId("ai-ask-not-permitted")).toBeTruthy();
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    expect(providerCalls(core)).toEqual([]);
    expect(core.ai.providerCalls).toEqual([]);

    // The palette's commands go through the same check.
    useAiPanelStore.getState().request("summarize");
    useAiPanelStore.getState().request("suggestReply");
    await waitFor(() => {
      expect(useAiPanelStore.getState().command).toBeNull();
    });
    expect(providerCalls(core)).toEqual([]);
  });

  it("shows typed errors with title and hint and a way to the settings", async () => {
    const { user } = setup(
      { enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, summary: (id) => summaryOf(id) },
      { fail: (cmd) => (cmd === "ai_summarize_thread" ? { code: "ai_unreachable", message: "connect", detail: null } : undefined) },
    );
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const alert = await within(panel).findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("ai_unreachable");
    expect(within(alert).getByText("The summary did not work.")).toBeTruthy();
    expect(within(alert).getByText("The AI provider could not be reached.")).toBeTruthy();
    expect(within(alert).getByText(/Check the address in the settings/)).toBeTruthy();
  });

  it("answers a question with references, or says honestly that nothing was found", async () => {
    let found = true;
    const { core, user } = setup({
      enabled: true,
      profiles: [LOCAL],
      functions: ALL_LOCAL,
      answer: () =>
        found
          ? {
              found: true,
              statements: [{ text: "The meeting is on Monday at 10.", sources: [5001, 4000] }],
              query: "meeting",
              candidates: 4,
              excluded: 0,
              dropped: 0,
              transfers: [makeTransfer({ function: "search", mails: 0 }), makeTransfer({ function: "search", mails: 4 })],
            }
          : { found: false, statements: [], query: "holiday", candidates: 0, excluded: 0, dropped: 0, transfers: [makeTransfer({ function: "search", mails: 0 })] },
    });
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    expect(within(panel).getByTestId("ai-no-thread")).toBeTruthy();
    const question = within(panel).getByRole("textbox", { name: "Question" });
    await user.type(question, "When is the meeting?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    const answer = await within(panel).findByRole("article", { name: "Answer" });
    expect(core.callsOf("ai_ask")).toEqual([{ requestId: expect.any(String) as string, question: "When is the meeting?", accountIds: null }]);
    expect(answer.textContent).toContain("The meeting is on Monday at 10.");
    // Decision 0027: the core names the sources it has (`getMessagesByIds`); the others are numbered.
    await waitFor(() => {
      expect(within(answer).getAllByTestId("ai-source").map((b) => b.textContent)).toEqual([expect.stringMatching(/^Grace Hopper, /), "Mail 2"]);
    });
    expect(core.callsOf("get_messages_by_ids")).toEqual([{ messageIds: [5001, 4000] }]);
    expect(within(answer).getByText("Based on 4 mails the search found.")).toBeTruthy();
    expect(within(answer).getByTestId("ai-transfer-summary").textContent).toContain("of text, no mail, to localhost:11434");

    found = false;
    await user.clear(question);
    await user.type(question, "When is my holiday?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    expect(await within(panel).findByTestId("ai-nothing-found")).toBeTruthy();
    expect(within(panel).getByTestId("ai-nothing-found").textContent).toBe("Nothing found. None of your mails answers this question.");
  });

  it("asks a cloud provider only about accounts with cloud permission and says how many it left out", async () => {
    const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work" });
    const { core, user } = setup(
      {
        enabled: true,
        profiles: [makeCloudProfile({ needsConsent: false, consentAt: 1 })],
        functions: ALL_CLOUD,
        cloudAccounts: [WORK.id],
        answer: () => ({ found: false, statements: [], query: "x", candidates: 0, excluded: 0, dropped: 0, transfers: [] }),
      },
      { accounts: [ADA, WORK] },
    );
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("radio", { name: "All accounts" }));
    await user.type(within(panel).getByRole("textbox", { name: "Question" }), "Anything new?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    await within(panel).findByRole("article", { name: "Answer" });
    expect(core.callsOf("ai_ask")).toEqual([{ requestId: expect.any(String) as string, question: "Anything new?", accountIds: [WORK.id] }]);
    expect(within(panel).getByTestId("ai-excluded").textContent).toBe("1 was left out: no permission for cloud providers.");
  });

  it("folds in and out from the header, and the palette offers its commands", async () => {
    const { core, user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, summary: (id) => summaryOf(id) });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Hide AI assistant" }));
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    // Runs from the palette open the panel and run there, visibly.
    useAiPanelStore.getState().request("summarize");
    const again = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(again).findByRole("article", { name: "Summary" });
    expect(core.callsOf("ai_summarize_thread")).toHaveLength(1);
    await user.click(screen.getAllByRole("button", { name: "Hide AI assistant" })[0] as HTMLElement);
    await user.click(await screen.findByRole("button", { name: "Show AI assistant" }));
    expect(await screen.findByRole("complementary", { name: "AI assistant" })).toBeTruthy();
  });

  it("keeps the draft for later when Open draft fails, with the error", async () => {
    const { user } = setup(
      {
        enabled: true,
        profiles: [LOCAL],
        functions: ALL_LOCAL,
        reply: (id) => ({ text: "Hi", inReplyTo: id, transfer: makeTransfer() }),
      },
      { fail: (cmd) => (cmd === "ai_open_reply_draft" ? { code: "not_found", message: "gone", detail: null } : undefined) },
    );
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Suggest a reply" }));
    const card = await within(panel).findByRole("article", { name: "Reply suggestion" });
    await user.click(within(card).getByRole("button", { name: "Open draft" }));
    const alert = await within(card).findByRole("alert");
    expect(within(alert).getByText("The draft could not be opened.")).toBeTruthy();
    expect(screen.queryByTestId("composer")).toBeNull();
  });
});

describe("AI panel from the palette", () => {
  it("runs a summary asked for while the panel was folded in, once the settings are known", async () => {
    useAiPanelStore.setState({ open: false });
    const message = mail();
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message) },
      readingPreferences: { markReadOnOpen: "never" },
      ai: { enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, summary: (id) => summaryOf(id) },
    });
    const { user } = renderApp();
    await user.click(await screen.findByRole("option"));
    await screen.findByRole("region", { name: "Message" });
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    useAiPanelStore.getState().request("summarize");
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(panel).findByRole("article", { name: "Summary" });
    expect(core.callsOf("ai_summarize_thread")).toEqual([{ requestId: expect.any(String) as string, messageId: message.id }]);
  });
});

describe("AI panel in the conversation view", () => {
  it("names each source by sender and date and shows it in the open conversation on click", async () => {
    const now = Date.now();
    const first = makeMessage({ id: 1000, mailboxId: 5, subject: "Plan", from: { name: "Anna", email: "anna@example.org" }, date: now - 3 * 3_600_000 });
    const answer = makeMessage({ id: 1200, mailboxId: 5, subject: "Re: Plan", from: { name: "Bob", email: "bob@example.org" }, date: now - 3_600_000 });
    const thread = { threadId: 7, messages: [first, answer].map((message) => ({ message, mailboxIds: [5] })), total: 2 };
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [], 5: [answer, first] },
      views: Object.fromEntries([first, answer].map((m) => [m.id, makeView(m)])),
      readingPreferences: { markReadOnOpen: "never" },
      search: {
        threads: { 5: [{ message: answer, threadId: 7, messageCount: 2, unreadCount: 0, lastActivity: answer.date }] },
        threadViews: { [answer.id]: thread },
      },
      ai: {
        enabled: true,
        profiles: [LOCAL],
        functions: ALL_LOCAL,
        summary: () => summaryOf(first.id, { keyPoints: [{ text: "Anna proposes Monday.", sources: [first.id] }], openQuestions: [], deadlines: [] }),
      },
    });
    const { user } = renderApp();
    await user.click(await screen.findByTestId("rail-folders"));
    await user.click(await screen.findByTestId("mailbox-5"));
    await screen.findByRole("listbox", { name: "Messages in Projekte" });
    await user.click(screen.getByRole("button", { name: "Conversations" }));
    const list = await screen.findByRole("listbox", { name: "Messages in Projekte" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(1);
    });
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    const entries = await screen.findByRole("region", { name: "Conversation with 2 messages" });
    const buttons = within(entries).getAllByTestId("thread-entry");
    expect(buttons[1]?.getAttribute("aria-current")).toBe("true");

    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await user.click(await within(panel).findByRole("button", { name: "Summarise" }));
    const card = await within(panel).findByRole("article", { name: "Summary" });
    const source = await within(card).findByRole("button", { name: /^Show source: Anna, / });
    await user.click(source);
    await waitFor(() => {
      expect(buttons[0]?.getAttribute("aria-current")).toBe("true");
    });
    expect(core.callsOf("ai_summarize_thread")).toEqual([{ requestId: expect.any(String) as string, messageId: answer.id }]);
  });
});
