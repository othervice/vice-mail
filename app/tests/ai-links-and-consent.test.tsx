// Decision 0028 in the UI: links in AI output go through the link check of a mail, the consent
// step shows the exact size from the core's dry run, and the AI panel's state is a UI
// preference of the core.
import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LinkInfo, MessageListItem } from "../src/services/ipc";
import { emitCore } from "./support/events";
import { CLOUD_PROFILE_ID, makeAiProfile, makeCloudProfile, makeTransfer, type AiFakeOptions, type SummaryFixture } from "./support/ai";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

const ADA = makeAccount();
const LOCAL = makeAiProfile();
const ALL_LOCAL = { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id };
const ALL_CLOUD = { summary: CLOUD_PROFILE_ID, reply_suggestion: CLOUD_PROFILE_ID, writing_help: CLOUD_PROFILE_ID, search: CLOUD_PROFILE_ID };
/** The token the core keeps the links of one result under. */
const TOKEN = "0123456789abcdef0123456789abcdef";
const OTHER_TOKEN = "fedcba9876543210fedcba9876543210";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

function mail(): MessageListItem {
  return makeMessage({ id: 5001, subject: "Budget 2027", from: { name: "Grace Hopper", email: "grace@example.org" } });
}

function linkInfo(overrides: Partial<LinkInfo> = {}): LinkInfo {
  return { target: "https://pay.example/x", scheme: "https", host: "pay.example", text: "https://pay.example/x", warnings: [], openable: true, ...overrides };
}

/** A summary whose key point has a link, as the core returns it. */
function summaryWithLink(messageId: number): SummaryFixture {
  return {
    keyPoints: [
      {
        text: "Pay at https://pay.example/x now.",
        segments: [
          { kind: "text", text: "Pay at " },
          { kind: "link", text: "https://pay.example/x", index: 0 },
          { kind: "text", text: " now." },
        ],
        sources: [messageId],
      },
    ],
    openQuestions: [],
    deadlines: [
      {
        text: "Sign at bank.example",
        segments: [
          { kind: "text", text: "Sign at " },
          { kind: "link", text: "bank.example", index: 1 },
        ],
        due: "Friday",
        sources: [messageId],
      },
    ],
    dropped: 0,
    transfer: makeTransfer(),
    linkToken: TOKEN,
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
  await user.click(await screen.findByRole("option"));
  await screen.findByRole("region", { name: "Message" });
  const panel = await screen.findByRole("complementary", { name: "AI assistant" });
  await within(panel).findByText("About: Budget 2027");
  return panel;
}

describe("links in AI output (decision 0028)", () => {
  it("open only through the same check as a link in a mail, never directly", async () => {
    const { core, user } = setup(
      { enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, summary: summaryWithLink },
      {
        links: {
          [`${TOKEN}/0`]: linkInfo({ text: "https://pay.example/x" }),
          [`${TOKEN}/1`]: linkInfo({
            target: "https://evil.example/sign",
            host: "evil.example",
            text: "bank.example",
            warnings: [{ kind: "domainMismatch", shownDomain: "bank.example", targetDomain: "evil.example" }],
          }),
        },
      },
    );
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const card = await within(panel).findByRole("article", { name: "Summary" });
    // Text with links as elements of their own: no anchor that could navigate, no image.
    expect(card.querySelector("a")).toBeNull();
    expect(card.querySelector("img")).toBeNull();
    expect(within(card).getByRole("region", { name: "Deadlines" }).textContent).toContain("Sign at bank.example (due: Friday)");
    const links = within(card).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(["https://pay.example/x", "bank.example"]);

    // The reading pane is not where this card goes: the panel shows it, with the core's answer.
    await user.click(links[1] as HTMLElement);
    const check = await within(panel).findByTestId("link-card");
    expect(core.callsOf("describe_link")).toEqual([{ renderToken: TOKEN, index: 1 }]);
    expect(within(check).getByTestId("link-target").textContent).toBe("https://evil.example/sign");
    expect(within(check).getByTestId("link-warnings").textContent).toContain("bank.example");
    expect(within(screen.getByTestId("reading-pane")).queryByTestId("link-card")).toBeNull();
    expect(core.callsOf("open_link")).toEqual([]);

    // Only "Open anyway" hands it to the core, which opens it in the browser.
    await user.click(within(check).getByRole("button", { name: "Open anyway" }));
    await waitFor(() => {
      expect(core.callsOf("open_link")).toEqual([{ renderToken: TOKEN, index: 1 }]);
    });
    await waitFor(() => {
      expect(within(panel).queryByTestId("link-card")).toBeNull();
    });

    // By keyboard as well; "Cancel" opens nothing.
    (links[0] as HTMLElement).focus();
    await user.keyboard("{Enter}");
    const plain = await within(panel).findByTestId("link-card");
    await within(plain).findByTestId("link-target");
    await user.click(within(plain).getByRole("button", { name: "Cancel" }));
    expect(core.callsOf("open_link")).toHaveLength(1);
  });

  it("says when the core no longer knows the links of a result, without a reload of a message", async () => {
    const { core, user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL, summary: summaryWithLink });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const card = await within(panel).findByRole("article", { name: "Summary" });
    await user.click(within(card).getAllByRole("link")[0] as HTMLElement);
    const check = await within(panel).findByTestId("link-card");
    expect(await within(check).findByText("The app no longer knows the links of this result. Ask again, then click the link once more.")).toBeTruthy();
    expect(within(check).queryByTestId("link-reload")).toBeNull();
    expect(core.callsOf("open_link")).toEqual([]);
  });

  it("lists the links of a reply suggestion below the text field, each through the check", async () => {
    const { core, user } = setup(
      {
        enabled: true,
        profiles: [LOCAL],
        functions: ALL_LOCAL,
        reply: (id) => ({
          text: "Hi Grace,\nsee https://example.org/terms\nAda",
          segments: [
            { kind: "text", text: "Hi Grace,\nsee " },
            { kind: "link", text: "https://example.org/terms", index: 0 },
            { kind: "text", text: "\nAda" },
          ],
          inReplyTo: id,
          transfer: makeTransfer({ function: "reply_suggestion" }),
          linkToken: OTHER_TOKEN,
        }),
      },
      { links: { [`${OTHER_TOKEN}/0`]: linkInfo({ target: "https://example.org/terms", host: "example.org", text: "https://example.org/terms" }) } },
    );
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Suggest a reply" }));
    const card = await within(panel).findByRole("article", { name: "Reply suggestion" });
    expect(within(card).getByTestId<HTMLTextAreaElement>("ai-reply-text").value).toContain("https://example.org/terms");
    const list = within(card).getByTestId("ai-link-list");
    await user.click(within(list).getByRole("link", { name: "https://example.org/terms" }));
    const check = await within(card).findByTestId("link-card");
    expect(within(check).getByTestId("link-target").textContent).toBe("https://example.org/terms");
    expect(core.callsOf("describe_link")).toEqual([{ renderToken: OTHER_TOKEN, index: 0 }]);
  });

  it("leaves text without links as text", async () => {
    const { user } = setup({
      enabled: true,
      profiles: [LOCAL],
      functions: ALL_LOCAL,
      summary: (id) => ({ keyPoints: [{ text: "Nothing to click.", sources: [id] }], openQuestions: [], deadlines: [], dropped: 0, transfer: makeTransfer() }),
    });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const card = await within(panel).findByRole("article", { name: "Summary" });
    expect(within(card).queryAllByRole("link")).toEqual([]);
  });
});

describe("the exact size before consent (decision 0028)", () => {
  it("shows what the request would send from the dry run, which reaches no provider", async () => {
    const { core, user, message } = setup({
      enabled: true,
      profiles: [makeCloudProfile()],
      functions: ALL_CLOUD,
      cloudAccounts: [ADA.id],
      summary: (id) => ({ ...summaryWithLink(id), linkToken: null }),
      dryRun: () =>
        makeTransfer({
          host: "api.openai.com:443",
          locality: "internet",
          model: "gpt-5",
          mails: 4,
          bytes: 20_480,
          trimming: { mailsLeftOut: 1, mailsShortened: 0, charsRemoved: 300 },
        }),
    });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const consent = await within(panel).findByRole("group", { name: "Before the first request to OpenAI" });
    const exact = await within(consent).findByTestId("ai-consent-exact");
    expect(exact.textContent).toContain("This request would send: 4 mails, 20 kB to api.openai.com:443 (gpt-5)");
    expect(exact.textContent).toContain("Shortened to fit: 1 mail was left out.");
    expect(within(exact).getByText("Cloud")).toBeTruthy();
    expect(core.ai.dryRuns).toEqual([{ function: "summary", messageId: message.id }]);
    expect(core.ai.providerCalls).toEqual([]);
    expect(core.callsOf("set_ai_consent")).toEqual([]);

    await user.click(within(consent).getByRole("button", { name: "Agree and send" }));
    await within(panel).findByRole("article", { name: "Summary" });
    expect(core.ai.providerCalls).toHaveLength(1);
  });

  it("says what follows for a question, and shows a failed dry run without blocking the choice", async () => {
    const { core, user } = setup({
      enabled: true,
      profiles: [makeCloudProfile()],
      functions: ALL_CLOUD,
      cloudAccounts: [ADA.id],
      answer: () => ({ found: false, statements: [], query: "x", candidates: 0, excluded: 0, dropped: 0, transfers: [] }),
      dryRun: (input) =>
        input.function === "search"
          ? makeTransfer({ function: "search", mails: 0, bytes: 900, host: "api.openai.com:443", locality: "internet", model: "gpt-5" })
          : // Tauri rejects with the serialised error value, not an Error.
            // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
            Promise.reject(ipcError("database")),
    });
    const panel = await openPanelOn(user);
    await user.type(within(panel).getByRole("textbox", { name: "Question" }), "When is the meeting?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    const consent = await within(panel).findByRole("group", { name: "Before the first request to OpenAI" });
    const exact = await within(consent).findByTestId("ai-consent-exact");
    expect(exact.textContent).toContain("This request would send: 900 bytes of text, no mail, to api.openai.com:443 (gpt-5)");
    expect(exact.textContent).toContain("a second request sends the matching mails");
    expect(core.ai.dryRuns).toEqual([{ function: "search", question: "When is the meeting?" }]);
    await user.click(within(consent).getByRole("button", { name: "Not now" }));

    await user.click(within(panel).getByRole("button", { name: "Summarise" }));
    const again = await within(panel).findByRole("group", { name: "Before the first request to OpenAI" });
    expect(await within(again).findByTestId("ai-consent-exact-error")).toBeTruthy();
    expect(within(again).getByRole("button", { name: "Agree and send" })).toBeTruthy();
    expect(core.ai.providerCalls).toEqual([]);
  });
});

describe("the AI panel's state in the core (decision 0028)", () => {
  it("stores folding the panel in the core, not in the web view, and follows other windows", async () => {
    const { core, user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Hide AI assistant" }));
    await waitFor(() => {
      expect(core.ui.preferences.aiPanel).toBe("folded");
    });
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    expect(window.localStorage.getItem("vice-mail.ui.layout.aiPanel")).toBeNull();

    // Another window unfolded it.
    await emitCore({ type: "uiPreferencesChanged", preferences: { ...DEFAULT_UI_PREFERENCES, aiPanel: "open" } });

    const shown = await screen.findByRole("complementary", { name: "AI assistant" });
    await user.click(within(shown).getByRole("button", { name: "Hide AI assistant" }));
    await waitFor(() => {
      expect(core.ui.preferences.aiPanel).toBe("folded");
    });
    // The header button unfolds it again.
    await user.click(screen.getByRole("button", { name: "Show AI assistant" }));
    await waitFor(() => {
      expect(core.ui.preferences.aiPanel).toBe("open");
    });
  });

  it("puts the panel back and says so when the core refuses to store it", async () => {
    const { user } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL }, { failures: { set_ui_preferences: ipcError("database") } });
    const panel = await openPanelOn(user);
    await user.click(within(panel).getByRole("button", { name: "Hide AI assistant" }));
    expect(await screen.findByText("Whether the AI panel is shown could not be saved.")).toBeTruthy();
    expect(await screen.findByRole("complementary", { name: "AI assistant" })).toBeTruthy();
  });

  it("moves a folded panel of the old local storage to the core once", async () => {
    window.localStorage.setItem("vice-mail.ui.layout.aiPanel", "false");
    const { core } = setup({ enabled: true, profiles: [LOCAL], functions: ALL_LOCAL });
    await waitFor(() => {
      expect(core.ui.preferences.aiPanel).toBe("folded");
    });
    expect(window.localStorage.getItem("vice-mail.ui.layout.aiPanel")).toBeNull();
  });
});
