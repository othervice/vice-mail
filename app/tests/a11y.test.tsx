import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThemePreference } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { SETTINGS_SECTION_IDS, useUiStore } from "../src/stores/ui";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { makeContact } from "./support/contacts";
import { makeDraft, makeOutgoing } from "./support/compose";
import { makeSignature, makeTemplate } from "./support/snippets";
// Stage 7: saved searches.
import { makeSavedSearch } from "./support/search";
import { renderApp } from "./support/render";
// Stage 13: AI in the app.
import { makeAiProfile, makeCloudProfile, makeHint, makeRequestEntry, makeTransfer } from "./support/ai";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  // jsdom has no canvas; axe only uses one for icon fonts, which the app does not have.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

/**
 * axe-core over the rendered app (docs/stages/08, "Automatische Prüfung"). jsdom has no layout
 * and loads no stylesheet, so axe cannot compute rendered colours: `color-contrast` is left to
 * tests/contrast.test.ts, which checks every text and control colour pair of tokens.css in both
 * themes (4.5:1 and 3:1, docs/SPEC.md). Everything else (names, roles, ARIA, structure) runs.
 */
async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    // The body of an HTML mail is the sender's content in a sandboxed frame, not the app's UI.
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map(
      (violation) =>
        `${violation.id} (${violation.impact ?? ""}): ${violation.help}\n` +
        violation.nodes.map((node) => `  ${node.target.join(" ")}: ${node.failureSummary ?? ""}`).join("\n"),
    );
}

function setup(theme: ThemePreference) {
  const items = makeMessages(8).map((item, index) => ({ ...item, flagged: index === 2, hasAttachments: index === 1 }));
  const core = mockCore({
    preferences: { theme, language: "system" },
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, items, ...renderApp() };
}

describe.each(["light", "dark"] as const)("axe, %s theme", (theme) => {
  it("finds nothing serious in the inbox", async () => {
    setup(theme);
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the reading pane", async () => {
    const { user } = setup(theme);
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Message" });
    await screen.findByRole("toolbar", { name: "Message actions" });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the composer", async () => {
    const { user } = setup(theme);
    await user.click(await screen.findByRole("button", { name: "New message" }));
    await screen.findByLabelText("Subject");
    expect(await seriousFindings()).toEqual([]);
  });

  // Stage 8 (decision 0025): the reduced shells of a message window and a composer window.
  it("finds nothing serious in a message window", async () => {
    const items = makeMessages(2);
    const account = makeAccount({ writesEnabled: true });
    window.history.replaceState(null, "", `/?window=message&accountId=${account.id}&messageId=${String(items[0]?.id)}`);
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [account],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
    });
    renderApp();
    await screen.findByRole("toolbar", { name: "Message actions" });
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in a composer window, while editing and once done", async () => {
    const draftId = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    window.history.replaceState(null, "", `/?window=composer&draftId=${draftId}`);
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true })],
      compose: { drafts: [makeDraft({ id: draftId, subject: "Plan" })] },
    });
    const { user } = renderApp();
    await screen.findByLabelText("Subject");
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByTestId("composer-close"));
    await screen.findByRole("heading", { name: "Done" });
    await screen.findByTestId("window-close-failed");
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the stage 10 composer: toolbar, inline image, signature, templates, send later, size", async () => {
    const account = makeAccount({ writesEnabled: true });
    const image = { id: "img-1", fileName: "logo.png", mime: "image/png", size: 4000, contentId: "logo@vicemail.test" };
    const draft = makeDraft({
      accountId: account.id,
      format: "html",
      to: [{ name: "Grace Hopper", email: "grace@example.org" }],
      bodyHtml: '<h2 style="text-align: center">Plan</h2><p>Logo <img src="cid:logo@vicemail.test" alt="" width="160"></p>',
      attachments: [image, { id: "a1", fileName: "Video.mov", mime: "video/quicktime", size: 30_000_000, contentId: null }],
    });
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [account],
      mailboxes: standardMailboxes(),
      compose: { drafts: [draft], sizeCheck: { size: 40_000_000, serverLimit: 26_214_400, limitKnown: true, exceedsServerLimit: true } },
      snippets: { signatures: [makeSignature()], templates: [makeTemplate()] },
    });
    const { user } = renderApp();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
    });
    await screen.findByTestId("composer-rich-body");
    await screen.findByTestId("composer-size-warning");
    await screen.findByRole("combobox", { name: "Signature" });
    await user.click(screen.getByRole("button", { name: "Template" }));
    await screen.findByTestId("composer-template-option");
    await user.click(screen.getByRole("button", { name: "Send later…" }));
    await screen.findByTestId("composer-send-later-picker");
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in any settings section", async () => {
    setup(theme);
    await screen.findByRole("listbox", { name: /^Messages in/ });
    const findings: string[] = [];
    for (const section of SETTINGS_SECTION_IDS) {
      act(() => {
        useUiStore.getState().showSettings(section);
      });
      await screen.findByRole("tab", { selected: true });
      // Let the section's own queries answer.
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
      findings.push(...(await seriousFindings()).map((finding) => `${section}: ${finding}`));
    }
    expect(findings).toEqual([]);
  });

  // Stage 10: the contact directory with merged addresses, duplicate suggestions and the merge question.
  it("finds nothing serious in the contacts with data, an open name editor and the merge question", async () => {
    const linus = makeContact({ email: "linus@example.org", name: "Linus", sent: 2, lastUsed: Date.UTC(2026, 10, 1) });
    const lists = makeContact({ email: "linus+lists@example.org", name: "Linus", received: 5 });
    const kim = makeContact({ email: "kim@example.org", name: "Kim Lee", received: 4 });
    const ada = {
      ...makeContact({ email: "ada@example.org", name: "Ada" }),
      addresses: [
        { email: "ada@example.org", sent: 4, received: 1, lastUsed: null },
        { email: "ada@old.example.org", sent: 1, received: 1, lastUsed: null },
      ],
    };
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      contacts: {
        contacts: [ada, linus, lists, kim],
        duplicates: [
          { reason: "plus_address", name: null, names: [], contacts: [linus, lists] },
          { reason: "names_differ", name: null, names: ["Kim Lee", "K. Lee"], contacts: [kim] },
        ],
      },
    });
    const { user } = renderApp();
    act(() => {
      useUiStore.getState().showSettings("contacts");
    });
    await screen.findByTestId("contacts-list");
    await screen.findByTestId("contact-duplicates");
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Edit name of linus@example.org" }));
    await screen.findByLabelText("Name");
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getAllByRole("button", { name: "Merge…" })[0] ?? document.body);
    await screen.findByRole("alertdialog", { name: "Merge these contacts?" });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in Settings → Writing with signatures, templates and both editors open", async () => {
    const image = { contentId: "logo@vicemail.test", fileName: "logo.png", mime: "image/png", size: 2048 };
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      snippets: {
        signatures: [makeSignature({ images: [image], html: '<p>Ada</p><p><img src="cid:logo@vicemail.test" alt=""></p>' })],
        templates: [makeTemplate({ subject: "Hi {{nickname}}" })],
        defaults: { 1: { newSignature: "sig-1", replySignature: null } },
      },
    });
    const { user } = renderApp();
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    await screen.findByTestId("signature-list");
    await screen.findByTestId("template-list");
    await screen.findByTestId("signature-defaults");
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Edit signature Work" }));
    await screen.findByRole("textbox", { name: "Signature" });
    await user.click(screen.getByRole("button", { name: "Edit template Thank you" }));
    await screen.findByRole("textbox", { name: "Text" });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the activity view, the outbox and with the sidebar unfolded", async () => {
    const { user } = setup(theme);
    await screen.findByRole("listbox", { name: /^Messages in/ });
    act(() => {
      useUiStore.getState().showActivity();
    });
    await screen.findByRole("heading", { level: 1, name: "Activity" });
    expect(await seriousFindings()).toEqual([]);
    act(() => {
      useUiStore.getState().showOutbox();
    });
    await screen.findByRole("heading", { level: 1, name: "Outbox" });
    expect(await seriousFindings()).toEqual([]);
    act(() => {
      useUiStore.getState().showMailView("inbox");
    });
    await user.click(await screen.findByTestId("rail-folders"));
    await screen.findByTestId("mailbox-5");
    expect(await seriousFindings()).toEqual([]);
  });

  // Stage 10: a send planned for later (with the time picker open) and one missed while the app was off.
  it("finds nothing serious in the outbox with planned and missed sends and the time picker", async () => {
    const plannedDraft = makeDraft({ id: "a11y-plan", state: "sending", outgoing: 80 });
    const missedDraft = makeDraft({ id: "a11y-missed", state: "sending", outgoing: 81 });
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      compose: {
        drafts: [plannedDraft, missedDraft],
        outgoing: [
          makeOutgoing({ operationId: 80, draftId: "a11y-plan", subject: "Planned", scheduled: true, sendAt: Date.now() + 7_200_000 }),
          makeOutgoing({
            operationId: 81,
            draftId: "a11y-missed",
            subject: "Missed",
            state: "missed",
            scheduled: true,
            sendAt: Date.now() - 7_200_000,
            error: { code: "send_missed", message: "missed", detail: null },
          }),
        ],
      },
    });
    const { user } = renderApp();
    // The question at the start (the notice) and the outbox rows.
    await screen.findByTestId("send-notice");
    act(() => {
      useUiStore.getState().showOutbox();
    });
    await screen.findByTestId("outbox-send-later-rule");
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    expect(await seriousFindings()).toEqual([]);
    await user.click(await screen.findByTestId("outgoing-change"));
    await screen.findByTestId("outgoing-picker");
    await user.click(screen.getByTestId("missed-reschedule"));
    await screen.findByTestId("missed-picker");
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the command palette and the shortcut overview", async () => {
    const { user } = setup(theme);
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await user.keyboard("{Meta>}k{/Meta}");
    await screen.findByRole("dialog", { name: "Command palette" });
    expect(await seriousFindings()).toEqual([]);
    await user.keyboard("{Escape}");
    await user.keyboard("?");
    await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
    expect(await seriousFindings()).toEqual([]);
  });

  // Stage 7: the recent searches under the field, "Save search", saved searches as views.
  it("finds nothing serious in the recent searches, the save form and the saved searches", async () => {
    const items = makeMessages(3);
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      search: {
        recentSearches: [{ query: "from:anna", usedAt: Date.now() }, { query: "rechnung", usedAt: Date.now() - 1000 }],
        savedSearches: [makeSavedSearch({ name: "Invoices", query: "rechnung" })],
      },
    });
    const { user } = renderApp();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await user.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByRole("listbox", { name: "Recent searches" });
    await user.keyboard("{ArrowDown}");
    expect(await seriousFindings()).toEqual([]);
    await user.type(screen.getByRole("searchbox", { name: "Search mail" }), "rechnung");
    await user.click(screen.getByRole("button", { name: "Save search" }));
    await screen.findByRole("group", { name: "Save this search" });
    expect(await seriousFindings()).toEqual([]);
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Close search" }));
    await user.click(screen.getByTestId("rail-folders"));
    await screen.findByRole("region", { name: "Saved searches" });
    await user.click(screen.getByRole("button", { name: /^Saved views/ }));
    await user.click(await screen.findByRole("button", { name: "Rename “Invoices”" }));
    expect(await seriousFindings()).toEqual([]);
    await user.keyboard("{Escape}");
    await user.click(within(screen.getByRole("region", { name: "Saved searches" })).getByRole("button", { name: "Invoices" }));
    await screen.findByRole("heading", { name: "Invoices" });
    expect(await seriousFindings()).toEqual([]);
  });
});

// ---- Stage 13: AI in the app (settings with providers, consent and log; panel; writing help) ----
describe.each(["light", "dark"] as const)("axe, %s theme, stage 13 AI", (theme) => {
  const local = makeAiProfile();
  const functions = { summary: local.id, reply_suggestion: local.id, writing_help: local.id, search: local.id };

  function setupAi() {
    const items = makeMessages(3);
    const core = mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
      ai: {
        enabled: true,
        profiles: [local, makeCloudProfile()],
        functions,
        log: [makeRequestEntry({ id: 2, outcome: "failed", error: "ai_busy" }), makeRequestEntry({ id: 1 })],
        hints: [makeHint({ messageId: items[0]?.id ?? 0 })],
        summary: (id) => ({
          keyPoints: [{ text: "Key point", sources: [id] }],
          openQuestions: [{ text: "Open question?", sources: [id] }],
          deadlines: [{ text: "Deadline", due: "Friday", sources: [id] }],
          dropped: 1,
          transfer: makeTransfer({ quotesRemoved: 2, attachmentNames: 1, trimming: { mailsLeftOut: 1, mailsShortened: 1, charsRemoved: 300 } }),
        }),
        reply: (id) => ({ text: "Suggested reply", inReplyTo: id, transfer: makeTransfer() }),
        writing: (passage) => ({ text: passage.toUpperCase(), transfer: makeTransfer({ mails: 0 }) }),
        answer: () => ({ found: true, statements: [{ text: "Answer", sources: [items[0]?.id ?? 0] }], query: "q", candidates: 1, excluded: 1, dropped: 0, transfers: [makeTransfer()] }),
      },
    });
    return { core, items, ...renderApp() };
  }

  async function themed() {
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
  }

  it("finds nothing serious in Settings → AI with providers, the form, the consent step and the request log", async () => {
    const { user } = setupAi();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showSettings("ai");
    });
    const panel = await screen.findByRole("tabpanel", { name: "AI in the app" });
    await within(panel).findByRole("region", { name: "Providers" });
    await themed();
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(panel).getByRole("button", { name: "Edit Ollama" }));
    await user.click(within(panel).getByRole("button", { name: "Read what is sent to OpenAI and agree" }));
    await within(panel).findByRole("group", { name: "Before the first request to OpenAI" });
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(panel).getByRole("tab", { name: "Request log" }));
    await within(panel).findByRole("table", { name: "AI requests, newest first" });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the AI panel with hints, summary, reply and answer cards", async () => {
    const { user } = setupAi();
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(panel).findByRole("region", { name: "Maybe still open" });
    await themed();
    expect(await seriousFindings()).toEqual([]);
    const options = await screen.findAllByRole("option");
    await user.click(options[0] as HTMLElement);
    await screen.findByTestId("open-reply-bar");
    await user.click(await within(panel).findByRole("button", { name: "Summarise" }));
    await within(panel).findByRole("article", { name: "Summary" });
    await user.click(within(panel).getByRole("button", { name: "Suggest a reply" }));
    await within(panel).findByRole("article", { name: "Reply suggestion" });
    await user.type(within(panel).getByRole("textbox", { name: "Question" }), "What?");
    await user.click(within(panel).getByRole("button", { name: "Ask" }));
    await within(panel).findByRole("article", { name: "Answer" });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in the writing help with its preview", async () => {
    const { user } = setupAi();
    await user.click(await screen.findByRole("button", { name: "New message" }));
    const composer = await screen.findByTestId("composer");
    const body = within(composer).getByLabelText<HTMLTextAreaElement>("Message");
    await user.type(body, "Hello world");
    body.setSelectionRange(0, 5);
    await user.click(within(composer).getByRole("button", { name: "Writing help" }));
    await user.click(within(composer).getByRole("button", { name: "Improve" }));
    await within(composer).findByRole("region", { name: "Suggestion" });
    await themed();
    expect(await seriousFindings()).toEqual([]);
  });
});
