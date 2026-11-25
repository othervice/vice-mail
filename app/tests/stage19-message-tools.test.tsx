// Stage 19 (decision 0055): view source (its own window, plain text), all headers, redirect and
// edit as new, through the real app against the fake core; plus the axe checks of these views.
import { screen, waitFor, within } from "@testing-library/react";
import { mockWindows } from "@tauri-apps/api/mocks";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { parseWindowRoute } from "../src/features/windows/route";
import type { MessageSource, ThemePreference } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

const ACCOUNT = makeAccount({ writesEnabled: true });

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

function setup(options: { writesEnabled?: boolean; theme?: ThemePreference; sources?: Record<number, MessageSource> } = {}) {
  const account = { ...ACCOUNT, writesEnabled: options.writesEnabled ?? true };
  const items = makeMessages(3);
  const core = mockCore({
    preferences: { theme: options.theme ?? "system", language: "system" },
    accounts: [account],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    stage19: {
      ...(options.sources && { sources: options.sources }),
      headers: {
        [items[0]?.id ?? 0]: [
          { name: "Received", value: "from mx.example.com by mx.example.org" },
          { name: "From", value: "Jörg <joerg@example.com>" },
          { name: "X-Trick", value: "a\u{fffd}b" },
        ],
      },
    },
  });
  return { core, items, account };
}

async function openFirst(user: ReturnType<typeof renderApp>["user"]) {
  const list = await screen.findByRole("listbox", { name: /^Messages in/ });
  const first = within(list).getAllByRole("option")[0];
  if (!first) throw new Error("no rows");
  await user.click(first);
  return screen.findByTestId("message-card");
}

async function cardMenu(user: ReturnType<typeof renderApp>["user"], name: string | RegExp) {
  await user.click(await screen.findByRole("button", { name: "More for this message" }));
  await user.click(await screen.findByRole("menuitem", { name }));
}

describe("view source and all headers", () => {
  it("opens the source in a window of its own from the card menu and with Cmd+U", async () => {
    const { core, items } = setup();
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "View source");
    await waitFor(() => {
      expect(core.stage19.sourceWindows).toEqual([`${ACCOUNT.id}/${String(items[0]?.id)}`]);
    });
    // The shortcut, with the focus on the message (not in a text field).
    screen.getByTestId("message-card").closest("article")?.focus();
    await user.keyboard("{Meta>}u{/Meta}");
    await waitFor(() => {
      expect(core.callsOf("open_source_window")).toHaveLength(2);
    });
  });

  it("lists every header field under the head and hides them again", async () => {
    setup();
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "Show all headers");
    const list = await screen.findByTestId("all-headers-list");
    const terms = within(list)
      .getAllByRole("term")
      .map((dt) => dt.textContent);
    expect(terms).toEqual(["Received", "From", "X-Trick"]);
    expect(within(list).getByText("Jörg <joerg@example.com>")).toBeTruthy();
    await cardMenu(user, "Hide all headers");
    expect(screen.queryByTestId("all-headers")).toBeNull();
  });

  it("reads the window route of a source window", () => {
    expect(parseWindowRoute(`?window=source&accountId=${ACCOUNT.id}&messageId=42`)).toEqual({
      kind: "source",
      accountId: ACCOUNT.id,
      messageId: 42,
    });
    expect(parseWindowRoute(`?window=source&accountId=${ACCOUNT.id}&messageId=x`)).toEqual({ kind: "invalid" });
  });

  it("shows the source as plain text, never as markup, without listening to core events", async () => {
    const text = 'Subject: Hi\n\n<p onclick="x()">Hello <script>alert(1)</script><img src=x onerror=alert(2)></p>\n';
    const { core, items } = setup({
      sources: {
        1: { accountId: ACCOUNT.id, messageId: 1, subject: "Hi", text, totalBytes: 3_000_000, shownBytes: 1_048_576, truncated: true },
      },
    });
    const id = items[0]?.id ?? 1;
    core.setView(id, makeView(items[0] ?? makeMessages(1)[0]!));
    window.history.replaceState(null, "", `/?window=source&accountId=${ACCOUNT.id}&messageId=1`);
    mockWindows("source-test");
    const { user } = renderApp();
    const pre = await screen.findByTestId("source-text");
    expect(pre.textContent).toBe(text);
    expect(pre.querySelector("p, script, img")).toBeNull();
    expect(document.querySelector("script[src='x'], img[src='x']")).toBeNull();
    // Cut, with the whole as .eml.
    expect(screen.getByTestId("source-truncated").textContent).toMatch(/Only the first 1 MB of 2.9 MB are shown/);
    await user.click(screen.getByTestId("source-save-whole"));
    await waitFor(() => {
      expect(core.callsOf("export_message")).toEqual([{ messageId: 1 }]);
    });
    // Wrapping is a switch of the view only.
    await user.click(screen.getByTestId("source-wrap"));
    expect(pre.className).toContain("whitespace-pre");
    expect(pre.className).not.toContain("whitespace-pre-wrap");
    // The window has no event permission: it never asks to listen.
    expect(core.calls.some((c) => c.cmd === "plugin:event|listen")).toBe(false);
    expect(await seriousFindings()).toEqual([]);
    // Escape closes the window (the shell may refuse; then it says how).
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(core.callsOf("plugin:window|close").length).toBeGreaterThan(0);
    });
  });
});

describe("redirect", () => {
  it("redirects from the card menu with the undo delay and can be undone", async () => {
    const { core, items } = setup();
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "Redirect");
    const dialog = await screen.findByRole("dialog", { name: "Redirect message" });
    expect(await seriousFindings()).toEqual([]);
    // Nothing to send to yet.
    await user.click(within(dialog).getByTestId("redirect-submit"));
    expect(within(dialog).getByTestId("redirect-hint").textContent).toBe("Add at least one recipient.");
    const to = within(within(dialog).getByTestId("redirect-to")).getByRole("combobox");
    await user.type(to, "carol@example.net,");
    await user.click(within(dialog).getByTestId("redirect-submit"));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Redirect message" })).toBeNull();
    });
    expect(core.stage19.redirects).toMatchObject([
      { messageId: items[0]?.id, to: [{ name: null, email: "carol@example.net" }], cc: [], bcc: [] },
    ]);
    const notice = await screen.findByTestId("send-notice");
    expect(notice.textContent).toContain(`Redirect: ${items[0]?.subject ?? ""}`);
    await user.click(within(notice).getByTestId("send-undo"));
    await waitFor(() => {
      expect(core.stage19.cancelled).toHaveLength(1);
    });
    expect(useComposerStore.getState().sends).toEqual([]);
    expect(useComposerStore.getState().session, "no draft to go back to").toBeNull();
  });

  it("says why it cannot redirect on a read-only account", async () => {
    const { core } = setup({ writesEnabled: false });
    const { user } = renderApp();
    await openFirst(user);
    await user.click(await screen.findByRole("button", { name: "More for this message" }));
    const item = await screen.findByRole("menuitem", { name: /Redirect: changes are turned off/ });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    await user.click(item);
    expect(screen.queryByRole("dialog", { name: "Redirect message" })).toBeNull();
    // The same explanation as a disabled action: why, and where to change it.
    expect((await screen.findAllByText(/Changes are turned off for this account/)).length).toBeGreaterThan(0);
    expect(core.stage19.redirects).toEqual([]);
  });

  it("closes with Escape and sends nothing", async () => {
    const { core } = setup();
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "Redirect");
    await screen.findByRole("dialog", { name: "Redirect message" });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Redirect message" })).toBeNull();
    expect(core.stage19.redirects).toEqual([]);
  });

  it("shows a sent redirect as redirected", async () => {
    const { core } = setup();
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "Redirect");
    const dialog = await screen.findByRole("dialog", { name: "Redirect message" });
    await user.type(within(within(dialog).getByTestId("redirect-to")).getByRole("combobox"), "carol@example.net,");
    await user.click(within(dialog).getByTestId("redirect-submit"));
    const notice = await screen.findByTestId("send-notice");
    const mail = useComposerStore.getState().sends[0]?.mail;
    if (!mail) throw new Error("no send");
    await emitCore({ type: "outgoingChanged", accountId: mail.accountId, mails: [{ ...mail, state: "sent" }] });
    await waitFor(() => {
      expect(notice.textContent).toContain("Message redirected");
    });
    expect(core.stage19.redirects).toHaveLength(1);
  });
});

describe("edit as new", () => {
  it("opens a new draft made from the message", async () => {
    const { core, items } = setup();
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "Edit as new");
    expect(useComposerStore.getState().session?.source).toEqual({ kind: "edit_as_new", messageId: items[0]?.id });
    await waitFor(() => {
      expect(core.callsOf("create_draft").at(-1)).toEqual({ request: { kind: "edit_as_new", messageId: items[0]?.id } });
    });
  });
});

describe.each(["light", "dark"] as const)("axe, %s theme, stage 19", (theme) => {
  it("the all-headers list and the card menu have no serious findings", async () => {
    setup({ theme });
    const { user } = renderApp();
    await openFirst(user);
    await cardMenu(user, "Show all headers");
    await screen.findByTestId("all-headers-list");
    expect(await seriousFindings()).toEqual([]);
  });
});
