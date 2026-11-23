import { act, screen, waitFor } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThemePreference } from "../src/services/ipc";
import { SETTINGS_SECTION_IDS, useUiStore } from "../src/stores/ui";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

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
});
