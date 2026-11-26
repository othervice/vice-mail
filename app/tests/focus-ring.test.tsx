import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useComposerStore } from "../src/stores/composer";
import { SETTINGS_SECTION_IDS, useUiStore } from "../src/stores/ui";
import { makeAccount, makeAttachment, makeMessages, makeView, mockCore, renderToken, standardMailboxes, textBody } from "./support/core";
import { makeDraft } from "./support/compose";
import { makeAiProfile, makeCloudProfile, makeHint, makeRequestEntry, makeTransfer } from "./support/ai";
import { focusRingReport } from "./support/focusRing";
import { compiledCss } from "./support/styles";
import { renderApp } from "./support/render";
import appCss from "../src/styles/app.css?raw";
import composerCss from "../src/features/composer/composer.css?raw";
import tokensCss from "../src/styles/tokens.css?raw";

/*
 * Stage 8, "Fokus: sichtbarer Fokusring überall". Each test opens a main view and focuses every
 * element the keyboard can reach, one after another; the app's compiled stylesheet
 * (tests/support/styles.ts) must give it, or an element around it while it has the focus, a
 * solid outline of at least 2 px in the `focus-ring` token. The token's contrast against every
 * surface in both themes is checked in tests/contrast.test.ts (3:1).
 *
 * What jsdom cannot show: whether the outline is cut off by a scrolling parent, covered by a
 * neighbour or drawn outside the window (no layout), whether WebKit or WebView2 decide on
 * `:focus-visible` the same way as jsdom (jsdom matches it for every focus, the engines only
 * after keyboard use), and the HTML mail inside its sandboxed frame.
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

async function expectRings(minimum: number) {
  const report = await focusRingReport();
  expect(report.findings).toEqual([]);
  expect(report.checked).toBeGreaterThanOrEqual(minimum);
}

function setup() {
  const items = makeMessages(8).map((item, index) => ({ ...item, flagged: index === 2, hasAttachments: index === 1 }));
  const core = mockCore({
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, items, ...renderApp() };
}

describe("focus ring", () => {
  it("is in the base styles, from the token, in both themes", async () => {
    expect(appCss).toMatch(/:focus-visible\s*\{\s*outline:\s*2px solid var\(--ui-focus-ring\);/);
    expect(tokensCss).toMatch(/:root\s*\{[^}]*--ui-focus-ring:\s*#[0-9a-f]{6};/);
    expect(tokensCss).toMatch(/:root\[data-theme="dark"\]\s*\{[^}]*--ui-focus-ring:\s*#[0-9a-f]{6};/);
    // `outline-none` keeps `--tw-outline-style: none`, so a later `focus-visible:outline-2`
    // draws nothing: the reason the walk below resolves custom properties too.
    const css = await compiledCss(["outline-none", "focus-visible:outline-2"]);
    expect(css).toContain("--tw-outline-style: none;");
    expect(css).toContain("outline-style: var(--tw-outline-style);");
    // The editor's own stylesheet shows the ring instead of hiding it: on the text, or, with the
    // signature right below the text, around the whole writing area (and only there not on the text).
    expect(composerCss).toMatch(/\.composer-prose:focus-visible\s*\{\s*outline:\s*2px solid var\(--ui-focus-ring\);/);
    expect(composerCss).toMatch(/\.composer-writing:has\(\.composer-prose:focus-visible\)\s*\{\s*outline:\s*2px solid var\(--ui-focus-ring\);/);
    const hidden = [...composerCss.matchAll(/([^{}\n]+):focus-visible\s*\{\s*outline:\s*none/g)].map((match) => match[1]?.trim());
    expect(hidden).toEqual([".composer-with-footer .composer-prose"]);
  });

  it("shows on everything in the inbox: rail, header, search, list and its cursor row", async () => {
    const { user } = setup();
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await waitFor(() => {
      expect(list.getAttribute("aria-activedescendant")).toBeTruthy();
    });
    await expectRings(10);
    await user.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByRole("combobox", { name: "Search mail" });
    await expectRings(10);
  });

  it("shows on everything in the reading pane and its toolbar", async () => {
    const { user } = setup();
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("toolbar", { name: "Message actions" });
    await expectRings(15);
  });

  it("shows on the links of a plain-text mail, its attachments and the link check card", async () => {
    const [message] = makeMessages(1);
    if (!message) throw new Error("no message");
    const token = renderToken();
    const view = makeView(message, {
      formats: ["text"],
      attachments: [makeAttachment()],
      body: textBody([{ kind: "text", text: "See " }, { kind: "link", text: "https://example.org/docs", index: 0 }], token),
    });
    mockCore({
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: view },
      readingPreferences: { markReadOnOpen: "never" },
      links: {
        [`${token}/0`]: { target: "https://example.org/docs", scheme: "https", host: "example.org", text: "docs", warnings: [], openable: true, trackingRemoved: [] },
      },
    });
    const { user } = renderApp();
    await user.click((await screen.findAllByRole("option"))[0] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await within(pane).findByRole("link", { name: "https://example.org/docs" }));
    await within(pane).findByTestId("link-card");
    await expectRings(15);
  });

  it("shows on everything in the composer: recipients, subject, toolbar and both editors", async () => {
    const account = makeAccount({ writesEnabled: true });
    const draft = makeDraft({
      accountId: account.id,
      format: "html",
      to: [{ name: "Grace Hopper", email: "grace@example.org" }],
      bodyHtml: "<p>Plan</p>",
    });
    mockCore({ accounts: [account], mailboxes: standardMailboxes(), compose: { drafts: [draft] } });
    const { user } = renderApp();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
    });
    await screen.findByTestId("composer-rich-body");
    await expectRings(15);
    await user.click(screen.getByRole("button", { name: "Send later…" }));
    await screen.findByTestId("composer-send-later-picker");
    await expectRings(15);
    // The question before discarding (a dialog, the focus stays inside).
    await user.click(screen.getByTestId("composer-discard"));
    await screen.findByTestId("composer-confirm-discard");
    await expectRings(2);
  });

  it("shows on everything in the command palette and the shortcut overview", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("{ArrowDown}");
    await expectRings(3);
    expect(within(palette).getByRole("combobox").getAttribute("aria-activedescendant")).toBeTruthy();
    await user.keyboard("{Escape}");
    await user.keyboard("?");
    await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
    await expectRings(1);
  });

  it("shows on everything in every settings section", async () => {
    setup();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    const findings: string[] = [];
    let checked = 0;
    for (const section of SETTINGS_SECTION_IDS) {
      act(() => {
        useUiStore.getState().showSettings(section);
      });
      const navigation = await screen.findByRole("tablist", { name: "Settings sections" });
      await within(navigation).findByRole("tab", { selected: true });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
      // Only the open section: a focused tab of the navigation opens its own section.
      const panel = document.getElementById(navigation.querySelector('[aria-selected="true"]')?.getAttribute("aria-controls") ?? "");
      expect(panel, section).not.toBeNull();
      const report = await focusRingReport(panel ?? undefined);
      checked += report.checked;
      findings.push(...report.findings.map((finding) => `${section}: ${finding}`));
    }
    const navigation = screen.getByRole("tablist", { name: "Settings sections" });
    findings.push(...(await focusRingReport(navigation)).findings.map((finding) => `navigation: ${finding}`));
    expect(findings).toEqual([]);
    expect(checked).toBeGreaterThan(SETTINGS_SECTION_IDS.length * 2);
  });

  it("shows on everything in the activity view, the outbox and the unfolded sidebar", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    act(() => {
      useUiStore.getState().showActivity();
    });
    await screen.findByRole("heading", { level: 1, name: "Activity" });
    await expectRings(5);
    act(() => {
      useUiStore.getState().showOutbox();
    });
    await screen.findByRole("heading", { level: 1, name: "Outbox" });
    await expectRings(5);
    act(() => {
      useUiStore.getState().showMailView("inbox");
    });
    await user.click(await screen.findByTestId("rail-folders"));
    await screen.findByTestId("mailbox-5");
    await expectRings(10);
  });

  it("shows on everything in the AI panel with its cards", async () => {
    const local = makeAiProfile();
    const items = makeMessages(3);
    mockCore({
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
      ai: {
        enabled: true,
        profiles: [local, makeCloudProfile()],
        functions: { summary: local.id, reply_suggestion: local.id, writing_help: local.id, search: local.id },
        log: [makeRequestEntry({ id: 1 })],
        hints: [makeHint({ messageId: items[0]?.id ?? 0 })],
        summary: (id) => ({
          keyPoints: [{ text: "Key point", sources: [id] }],
          openQuestions: [],
          deadlines: [],
          dropped: 0,
          transfer: makeTransfer(),
        }),
        reply: (id) => ({ text: "Suggested reply", inReplyTo: id, transfer: makeTransfer() }),
      },
    });
    const { user } = renderApp();
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(panel).findByRole("region", { name: "Maybe still open" });
    const options = await screen.findAllByRole("option");
    await user.click(options[0] as HTMLElement);
    await user.click(await within(panel).findByRole("button", { name: "Summarise conversation" }));
    await within(panel).findByRole("article", { name: "Summary" });
    await user.click(within(panel).getByRole("button", { name: "Suggest a reply" }));
    await within(panel).findByRole("article", { name: "Reply suggestion" });
    await expectRings(15);
  });
});
