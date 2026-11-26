// Stage 23 integration (decision 0087): Later, Flagged, Sent and the other views of "All
// accounts" cover every visible account, like the unified inbox: one mixed list with the
// account's stripe, each row opened and acted on with its own account. Each account's Sent stays
// that account's folder; with an account chosen (the rail, the palette) the views show that one.
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { optionName } from "./support/names";
import { CONVERSATION_PREFERENCES, DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const now = Date.now();
const later = { until: now + 86_400_000, returned: false };
const ADA_LATER = makeMessage({ id: 3001, accountId: ADA.id, mailboxId: 1, subject: "Ada later", date: now - 60_000, snooze: later });
const WORK_LATER = makeMessage({ id: 2001, accountId: WORK.id, mailboxId: 20, subject: "Work later", date: now - 120_000, snooze: later });
const ADA_FLAG = makeMessage({ id: 3002, accountId: ADA.id, mailboxId: 1, subject: "Ada flagged", date: now - 180_000, flagged: true });
const WORK_FLAG = makeMessage({ id: 2002, accountId: WORK.id, mailboxId: 20, subject: "Work flagged", date: now - 240_000, flagged: true });
const ADA_SENT = makeMessage({ id: 3003, accountId: ADA.id, mailboxId: 2, subject: "Ada sent", date: now - 300_000 });
const WORK_SENT = makeMessage({ id: 2003, accountId: WORK.id, mailboxId: 22, subject: "Work sent", date: now - 360_000 });
const ALL = [ADA_LATER, WORK_LATER, ADA_FLAG, WORK_FLAG, ADA_SENT, WORK_SENT];

const LAYOUT = { readingPane: "right" as const, density: "comfortable" as const, sidebarExpanded: true, sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: false };

function setup({ sidebarExpanded = true, conversations = false }: { sidebarExpanded?: boolean; conversations?: boolean } = {}) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ADA, WORK],
    mailboxes: [
      ...standardMailboxes(),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      makeMailbox({ id: 22, accountId: WORK.id, path: "Gesendet", role: "sent", roleSource: "server" }),
    ],
    messages: { 1: [ADA_LATER, ADA_FLAG], 2: [ADA_SENT], 20: [WORK_LATER, WORK_FLAG], 22: [WORK_SENT] },
    views: Object.fromEntries(ALL.map((message) => [message.id, makeView(message)])),
    readingPreferences: { markReadOnOpen: "never" },
    ui: { preferences: conversations ? CONVERSATION_PREFERENCES : DEFAULT_UI_PREFERENCES, layout: { ...LAYOUT, sidebarExpanded } },
  });
  return { core, ...renderApp() };
}

const subjects = (list: HTMLElement) =>
  within(list)
    .getAllByRole("option")
    .map((option) => optionName(option).split(", ")[1]);

async function allAccounts() {
  const nav = await screen.findByRole("navigation", { name: "Accounts and folders" });
  return within(nav).findByRole("tree", { name: "All accounts" });
}

async function seriousFindings(): Promise<string[]> {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("the views of All accounts (decision 0087)", () => {
  it("Later mixes the snoozed mail of every account, each row with its account's stripe", async () => {
    const { core, user } = setup();
    const all = await allAccounts();
    await user.click(within(all).getByRole("treeitem", { name: "Later" }));
    const list = await screen.findByRole("listbox", { name: "Messages in Later" });
    await waitFor(() => {
      expect(subjects(list)).toEqual(["Ada later", "Work later"]);
    });
    expect(core.callsOf("list_snoozed").at(-1)).toEqual({ scope: { kind: "all" } });
    const rows = within(list).getAllByRole("option");
    expect(within(rows[1] as HTMLElement).getByTestId("row-account").title).toBe("Work");
    // The row of "All accounts" is the shown one; the toolbar names the scope, not one account.
    expect(within(all).getByRole("treeitem", { name: "Later" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByTestId("view-scope").textContent).toBe("All accounts");
    expect(screen.queryByTestId("view-account")).toBeNull();
    expect(await seriousFindings()).toEqual([]);
  });

  it("opens and acts on a row with its own account: read-only rules per account", async () => {
    const { user } = setup();
    const all = await allAccounts();
    await user.click(within(all).getByRole("treeitem", { name: "Flagged" }));
    const list = await screen.findByRole("listbox", { name: "Messages in Flagged" });
    await waitFor(() => {
      expect(subjects(list)).toEqual(["Ada flagged", "Work flagged"]);
    });
    // Work is read-only (new accounts are, CLAUDE.md): its row's actions say so.
    await user.click(within(list).getAllByRole("option")[1] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    await within(pane).findByTestId("reading-from");
    const toolbar = screen.getByRole("toolbar", { name: /actions/i });
    expect(within(toolbar).getByRole("button", { name: /^Archive/ }).hasAttribute("disabled") || within(toolbar).getByRole("button", { name: /^Archive/ }).getAttribute("aria-disabled") === "true").toBe(true);
    // Ada's row acts with Ada's (writable) account.
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    await waitFor(() => {
      const archive = within(screen.getByRole("toolbar", { name: /actions/i })).getByRole("button", { name: /^Archive/ });
      expect(archive.hasAttribute("disabled") || archive.getAttribute("aria-disabled") === "true").toBe(false);
    });
  });

  it("Sent mixes every account's Sent folder in the core; each account's Sent stays its own folder", async () => {
    const { core, user } = setup({ conversations: true });
    const all = await allAccounts();
    await user.click(within(all).getByRole("treeitem", { name: "Sent" }));
    // Conversations (decision 0039): a tree, from the core's conversation list of Sent.
    const list = await screen.findByRole("tree", { name: "Messages in Sent" });
    await waitFor(() => {
      expect(within(list).getAllByRole("treeitem").map((row) => optionName(row).split(", ")[1])).toEqual(["Ada sent", "Work sent"]);
    });
    expect(core.callsOf("list_unified_sent_threads").at(-1)).toMatchObject({ view: { kind: "all" } });
    expect(within(all).getByRole("treeitem", { name: "Sent" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByTestId("view-scope").textContent).toBe("All accounts");
    // No account's own Sent row is the shown one.
    const work = screen.getByRole("tree", { name: "Folders of Work" });
    expect(within(work).getByRole("treeitem", { name: "Sent" }).getAttribute("aria-current")).toBeNull();

    // Work's own Sent: that folder alone, named by its account.
    await user.click(within(work).getByRole("treeitem", { name: "Sent" }));
    await waitFor(() => {
      expect(useUiStore.getState().view).toEqual({ kind: "mailbox", accountId: WORK.id, mailboxId: 22 });
    });
    await waitFor(() => {
      const own = screen.getByRole("tree", { name: "Messages in Sent" });
      expect(within(own).getAllByRole("treeitem").map((row) => optionName(row).split(", ")[1])).toEqual(["Work sent"]);
    });
    expect(within(work).getByRole("treeitem", { name: "Sent" }).getAttribute("aria-current")).toBe("page");
    expect(within(all).getByRole("treeitem", { name: "Sent" }).getAttribute("aria-current")).toBeNull();
  });

  it("follows the conversation view in Sent: plain rows from the core's mixed page while it is off", async () => {
    const { core, user } = setup();
    const all = await allAccounts();
    await user.click(within(all).getByRole("treeitem", { name: "Sent" }));
    const list = await screen.findByRole("listbox", { name: "Messages in Sent" });
    await waitFor(() => {
      expect(subjects(list)).toEqual(["Ada sent", "Work sent"]);
    });
    expect(core.callsOf("list_unified_sent").at(-1)).toMatchObject({ view: { kind: "all" }, cursor: null });
    expect(core.callsOf("list_unified_sent_threads")).toEqual([]);
    // Sent shows the recipients, as an account's Sent folder does.
    expect(within(list).getAllByRole("option")[0]?.textContent).toContain("To");
  });

  it("in the rail: the views follow the scope; an account's dot narrows them to that account", async () => {
    const { core, user } = setup({ sidebarExpanded: false });
    await screen.findByTestId("icon-rail");
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    await user.click(screen.getByTestId("rail-later"));
    const list = await screen.findByRole("listbox", { name: "Messages in Later" });
    await waitFor(() => {
      expect(subjects(list)).toEqual(["Ada later", "Work later"]);
    });
    expect(core.callsOf("list_snoozed").at(-1)).toEqual({ scope: { kind: "all" } });
    await user.click(screen.getByTestId(`account-switch-${ADA.id}`));
    await waitFor(() => {
      expect(core.callsOf("list_snoozed").at(-1)).toEqual({ scope: { kind: "account", accountId: ADA.id } });
    });
    expect(screen.getByTestId("view-account").textContent).toContain("Ada Lovelace");
    // Sent of the rail with "All accounts" on: every account's.
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    await user.click(screen.getByTestId("rail-sent"));
    const sent = await screen.findByRole("listbox", { name: "Messages in Sent" });
    await waitFor(() => {
      expect(subjects(sent)).toEqual(["Ada sent", "Work sent"]);
    });
  });

  it("one account's Later has no row in All accounts", async () => {
    const { core } = setup();
    const all = await allAccounts();
    act(() => {
      useUiStore.getState().selectAccount(WORK.id);
      useUiStore.getState().showMailView("later");
    });
    await waitFor(() => {
      expect(core.callsOf("list_snoozed").at(-1)).toEqual({ scope: { kind: "account", accountId: WORK.id } });
    });
    expect(within(all).getByRole("treeitem", { name: "Later" }).getAttribute("aria-current")).toBeNull();
    expect(screen.getByTestId("view-account").textContent).toContain("Work");
  });
});
