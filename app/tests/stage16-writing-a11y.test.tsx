import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageListItem, ThemePreference } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeDraft } from "./support/compose";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { focusRingReport } from "./support/focusRing";
import { optionName } from "./support/names";
import { makeReceiptInfo } from "./support/parityCompose";
import { renderApp } from "./support/render";
import { makeTemplate } from "./support/snippets";

// Stage 16, writing: axe (light and dark) and the focus ring for the quick reply, the read
// receipt bar and list, the composer's send options and menu, and templates with recipients.

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  useComposerStore.setState({ session: null, openDraftId: null, sends: [], problems: [] });
});

/** Same rules as tests/a11y.test.tsx (colour contrast is tests/contrast.test.ts). */
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

const GRACE = { name: "Grace Hopper", email: "grace@example.org" };

function setup(theme: ThemePreference = "light") {
  const items = makeMessages(3);
  const first = items[0] as MessageListItem;
  const second = items[1] as MessageListItem;
  const draft = makeDraft({ kind: "reply", to: [GRACE], subject: "Re: Hello", originalMessageId: first.id });
  const core = mockCore({
    preferences: { theme, language: "system" },
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose: {
      drafts: [draft],
      replies: { [first.id]: { subject: `Re: ${first.subject}`, to: [GRACE], quoteText: "> Hello" } },
    },
    snippets: { templates: [makeTemplate({ to: [GRACE], cc: [{ name: null, email: "team@example.org" }] })] },
    receipts: {
      receipts: {
        [first.id]: makeReceiptInfo({ request: { to: "grace@example.org", returnPathDiffers: true, answer: "ask" } }),
        [second.id]: makeReceiptInfo({
          requested: true,
          received: [{ recipient: "bob@example.org", disposition: "displayed", receivedAt: Date.UTC(2026, 10, 22, 9) }],
        }),
      },
    },
  });
  return { core, first, second, draft, ...renderApp() };
}

async function openMessage(user: ReturnType<typeof setup>["user"], subject: string) {
  const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
  const row = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(subject));
  if (!row) throw new Error("no row");
  await user.click(row);
  await screen.findByTestId("message-card");
}

describe.each(["light", "dark"] as const)("stage 16 writing, axe, %s theme", (theme) => {
  it("finds nothing serious in the reading pane with a receipt request and an open quick reply", async () => {
    const { user, first } = setup(theme);
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    await openMessage(user, first.subject);
    await screen.findByTestId("read-receipt-request");
    await user.type(screen.getByRole("textbox", { name: /^Quick reply to/ }), "Thanks");
    await screen.findByRole("button", { name: /^Send reply/ });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious with receipts that came back", async () => {
    const { user, second } = setup(theme);
    await openMessage(user, second.subject);
    await screen.findByTestId("read-receipts-received");
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in a reply with the send options and the open send menu", async () => {
    const { user, draft } = setup(theme);
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
    });
    await screen.findByLabelText("Subject");
    await user.click(screen.getByRole("button", { name: "More ways to send" }));
    await screen.findByRole("menuitem", { name: /Send and archive/ });
    expect(await seriousFindings()).toEqual([]);
  });

  it("finds nothing serious in Settings → Writing with a template's recipients open", async () => {
    const { user } = setup(theme);
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    await screen.findByRole("combobox", { name: "Read receipt requests" });
    await user.click(await screen.findByRole("button", { name: "Edit template Thank you" }));
    await screen.findByRole("group", { name: "Recipients" });
    expect(await seriousFindings()).toEqual([]);
  });
});

describe("stage 16 writing, focus ring", () => {
  it("shows on the quick reply, its buttons and the receipt question", async () => {
    const { user, first } = setup();
    await openMessage(user, first.subject);
    await screen.findByTestId("read-receipt-request");
    await user.type(screen.getByRole("textbox", { name: /^Quick reply to/ }), "Thanks");
    const report = await focusRingReport(screen.getByTestId("message-card"));
    expect(report.findings).toEqual([]);
    // Field, Send, Expand, quote, discard, receipt buttons and more.
    expect(report.checked).toBeGreaterThanOrEqual(7);
  });

  it("shows on the composer's send options and the send menu", async () => {
    const { user, draft } = setup();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
    });
    await screen.findByLabelText("Subject");
    await user.click(screen.getByRole("button", { name: "More ways to send" }));
    await screen.findByRole("menuitem", { name: /Send and archive/ });
    // The menu button and its open item (the test id sits on the button).
    const menu = await focusRingReport(screen.getByTestId("composer-send-menu").parentElement ?? document.body);
    expect(menu.findings).toEqual([]);
    expect(menu.checked).toBeGreaterThanOrEqual(2);
    const options = await focusRingReport(screen.getByTestId("composer-send-options"));
    expect(options.findings).toEqual([]);
    // Stage 20: and the reminder's choice (decision 0059).
    expect(options.checked).toBe(3);
  });

  it("shows on the recipient fields of a template", async () => {
    const { user } = setup();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    await user.click(await screen.findByRole("button", { name: "Edit template Thank you" }));
    const report = await focusRingReport(await screen.findByTestId("template-recipients"));
    expect(report.findings).toEqual([]);
    // Three fields and the chips' remove buttons.
    expect(report.checked).toBeGreaterThanOrEqual(5);
  });
});
