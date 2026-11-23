import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeAiProfile } from "./support/ai";
import { makeDraft } from "./support/compose";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

/*
 * Stage 8, "Zustände und Texte": the empty and error states the review found missing. Each says
 * what happened at its place and offers the way out (retry, add an account); nothing is made up.
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

/** Fails `command` until `recover()` is called, like a core that comes back. */
function failingUntilRecovered(command: string) {
  let failing = true;
  return {
    fail: (cmd: string) => (failing && cmd === command ? ipcError("database") : undefined),
    recover: () => {
      failing = false;
    },
  };
}

describe("states", () => {
  it("AI panel: says that its settings could not be loaded and loads them again on request", async () => {
    const local = makeAiProfile();
    const items = makeMessages(2);
    const failing = failingUntilRecovered("get_ai_settings");
    const core = mockCore({
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
      ai: { enabled: true, profiles: [local], functions: { summary: local.id, reply_suggestion: local.id, writing_help: local.id, search: local.id } },
      fail: failing.fail,
    });
    const { user } = renderApp();
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    const notice = await within(panel).findByTestId("ai-panel-settings-error");
    expect(notice.getAttribute("role")).toBe("alert");
    expect(notice.textContent).toContain("The AI settings could not be loaded");
    failing.recover();
    await user.click(within(notice).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(within(panel).queryByTestId("ai-panel-settings-error")).toBeNull();
    });
    expect(core.callsOf("get_ai_settings").length).toBeGreaterThanOrEqual(2);
  });

  it("composer: says that the sender addresses and the signatures could not be loaded, with a retry", async () => {
    const account = makeAccount({ writesEnabled: true });
    const draft = makeDraft({ accountId: account.id });
    const identities = failingUntilRecovered("list_compose_identities");
    const signatures = failingUntilRecovered("list_signatures");
    mockCore({
      accounts: [account],
      mailboxes: standardMailboxes(),
      compose: { drafts: [draft] },
      fail: (cmd) => identities.fail(cmd) ?? signatures.fail(cmd),
    });
    const { user } = renderApp();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
    });
    // The form replaces the frame that showed "Opening the draft…".
    await screen.findByLabelText("Subject");
    const composer = screen.getByTestId("composer");
    const fromError = await within(composer).findByTestId("composer-identity-error");
    expect(fromError.textContent).toContain("The sender addresses could not be loaded.");
    const signatureError = await within(composer).findByTestId("composer-signatures-error");
    expect(signatureError.textContent).toContain("The signatures could not be loaded.");
    identities.recover();
    signatures.recover();
    await user.click(within(fromError).getByRole("button", { name: "Try again" }));
    await user.click(within(signatureError).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(within(composer).queryByTestId("composer-identity-error")).toBeNull();
      expect(within(composer).queryByTestId("composer-signatures-error")).toBeNull();
    });
  });

  it("settings sync: offers a retry when the accounts could not be loaded", async () => {
    const failing = failingUntilRecovered("list_accounts");
    const core = mockCore({ fail: failing.fail });
    const { user } = renderApp();
    act(() => {
      useUiStore.getState().showSettings("sync");
    });
    const panel = await screen.findByRole("tabpanel", { name: "Settings sync" });
    const alerts = await within(panel).findAllByRole("alert");
    const notice = alerts.find((alert) => alert.textContent.includes("Try again"));
    expect(notice).toBeDefined();
    const before = core.callsOf("list_accounts").length;
    failing.recover();
    await user.click(within(notice as HTMLElement).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("list_accounts").length).toBeGreaterThan(before);
    });
  });

  it("without an account: the activity view offers to add one, the unfolded sidebar says why it is empty", async () => {
    mockCore({});
    const { user } = renderApp();
    await screen.findByRole("heading", { name: "No account yet" });
    act(() => {
      useUiStore.getState().showActivity();
    });
    await screen.findByRole("heading", { level: 2, name: "Nothing waiting" });
    expect(screen.getByText("Add an email account. Changes on their way to its server appear here.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Add account" }));
    await waitFor(() => {
      expect(useUiStore.getState().view).toMatchObject({ kind: "settings", section: "accounts" });
    });
    await user.click(screen.getByTestId("rail-folders"));
    expect((await screen.findByTestId("sidebar-no-accounts")).textContent).toBe("No account yet. Its folders appear here once you add one.");
  });

  it("speaks German with du in the new texts", async () => {
    mockCore({ preferences: { theme: "system", language: "de" } });
    const { user } = renderApp("de");
    await screen.findByRole("heading", { name: "Noch kein Konto" });
    act(() => {
      useUiStore.getState().showActivity();
    });
    await screen.findByText("Füge ein E-Mail-Konto hinzu. Änderungen auf dem Weg zu seinem Server erscheinen dann hier.");
    await user.click(screen.getByTestId("rail-folders"));
    expect((await screen.findByTestId("sidebar-no-accounts")).textContent).toBe("Noch kein Konto. Seine Ordner erscheinen hier, sobald du eins hinzufügst.");
  });
});
