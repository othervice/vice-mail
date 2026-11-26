import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { useLayoutStore } from "../src/stores/layout";
import { SIDEBAR_WIDTH, useUiStore } from "../src/stores/ui";
import type { OperationInfo } from "../src/services/ipc";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { makeSavedSearch } from "./support/search";
import { renderApp } from "./support/render";
import { DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

/*
 * docs/DESIGN.md, "Seitenspalte": the accounts-and-folders column replaces the icon rail while
 * it is open; a toggle, Cmd/Ctrl+Shift+S and the palette fold it. Its edge is a window splitter
 * (drag or arrow keys). Each account is the root of its folder tree, with its unread total and,
 * when something is wrong, the reason in its name. In a narrow window it opens over the content.
 * Stage 23 (decision 0085): "All accounts" (the unified inbox, Later, Flagged, Sent, "More"),
 * favourites, one section per account (inbox, Sent, "More folders"), a foot with the sync state.
 * The fake core starts with "More" and "More folders" open unless a test gives the core's default.
 */

const ADA = makeAccount({ writesEnabled: true });
const CLUB = makeAccount({
  id: "33333333-3333-4333-8333-333333333333",
  email: "board@club.example",
  displayName: "Club",
  color: 5,
  status: { state: "offline", progress: null, error: { code: "connection_lost", message: "reset", detail: null }, retryAt: null, lastSyncAt: null },
});

function setup(layout: { sidebarExpanded: boolean } = { sidebarExpanded: true }, operations: OperationInfo[] = [], coreDefault = false) {
  const core = mockCore({
    accounts: [ADA, CLUB],
    operations,
    mailboxes: [
      ...standardMailboxes().map((mailbox) => (mailbox.id === 5 ? { ...mailbox, favoritePosition: 0 } : mailbox)),
      makeMailbox({ id: 40, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server", unread: 4 }),
      makeMailbox({ id: 41, accountId: CLUB.id, path: "Turniere", unread: 1 }),
    ],
    ui: {
      layout: { readingPane: "right", density: "comfortable", sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true, ...layout },
      ...(coreDefault && { preferences: DEFAULT_UI_PREFERENCES }),
    },
    search: { savedSearches: [makeSavedSearch({ name: "Invoices" })] },
  });
  return { core, ...renderApp() };
}

const column = () => screen.findByRole("navigation", { name: "Accounts and folders" });

/** jsdom has no window size: a `max-width` query matches as if the window were narrow. */
function narrowWindow() {
  const original = window.matchMedia;
  vi.spyOn(window, "matchMedia").mockImplementation((query: string) => {
    const list = original(query);
    if (!query.includes("max-width")) return list;
    return { ...list, matches: true, media: query, addEventListener: () => undefined, removeEventListener: () => undefined };
  });
}

/** axe-core as in tests/a11y.test.tsx (colour contrast: tests/contrast.test.ts). */
async function seriousFindings(): Promise<string[]> {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}


/** Text as axe compares it for "label in name": lower case, words only. */
function words(text: string | null | undefined): string {
  return (text ?? "").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

describe("accounts-and-folders column", () => {
  it("names the activity count with what the column shows (label in name; the outbox count is drawn alike)", async () => {
    const waiting: OperationInfo = {
      id: 1,
      batch: 1,
      accountId: ADA.id,
      action: "archive",
      kind: "move",
      state: "queued",
      mailboxId: 1,
      messageId: 100,
      targetMailboxId: 7,
      messageCount: null,
      attempts: 1,
      error: null,
      conflict: null,
      notice: null,
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    setup({ sidebarExpanded: true }, [waiting]);
    const nav = await column();
    // Stage 23 (decision 0085): Activity is a row under "More".
    const activity = within(nav).getByTestId("sidebar-activity");
    await waitFor(() => {
      expect(activity.getAttribute("aria-label")).toBe("Activity, 1 waiting, 0 need attention");
    });
    // The column shows the name and the count as two words ("Activity 1"), both in the name.
    expect(words(activity.textContent)).toBe("activity 1");
    expect(words(activity.getAttribute("aria-label"))).toContain(words(activity.textContent));
  });

  it("replaces the rail: All accounts with the unified inbox and its count, More, every account's section, the foot", async () => {
    setup();
    const nav = await column();
    expect(screen.queryByTestId("icon-rail")).toBeNull();
    const all = await within(nav).findByRole("tree", { name: "All accounts" });
    expect(await within(all).findByRole("treeitem", { name: "Inbox, 7 unread" })).toBeTruthy();
    expect(within(all).getAllByRole("treeitem").map((row) => row.getAttribute("aria-label"))).toEqual([
      "All accounts",
      "Inbox, 7 unread",
      "Later",
      "Flagged",
      "Sent",
      "More",
      "Waiting for reply",
      "Tasks",
      "Reading list",
      "Pinned attachments",
      "Attachments",
      "Pinned",
      "Saved views",
      // Stage 17 (decision 0050).
      "Contacts",
      "Invoices",
      "Activity",
      "Outbox",
    ]);
    const levels = within(all)
      .getAllByRole("treeitem")
      .map((row) => row.getAttribute("aria-level"))
      .join("");
    expect(levels).toBe("12222233333333333");
    // Each account is the root of its own tree, named with address and unread total.
    const club = await within(nav).findByRole("tree", { name: "Folders of Club" });
    const root = within(club).getAllByRole("treeitem")[0];
    expect(root?.getAttribute("aria-level")).toBe("1");
    // Offline: a quiet mark, the state and its reason in the name (never the colour alone).
    expect(root?.getAttribute("aria-label")).toBe("Club, 5 unread (board@club.example), offline: The connection to the server was lost.");
    // Label in name (WCAG 2.5.3): the heading shows "Club" while open; the name starts with it.
    expect(words(root?.textContent)).toBe("club");
    expect(words(root?.getAttribute("aria-label"))).toContain(words(root?.textContent));
    expect(screen.getByTestId(`account-status-${CLUB.id}`)).toBeTruthy();
    expect(within(club).getAllByRole("treeitem").map((row) => row.getAttribute("aria-label"))).toEqual([
      root?.getAttribute("aria-label"),
      "Inbox, 4 unread",
      "More folders",
      "Turniere, 1 unread",
    ]);
    // The foot: what syncs (Club is offline), Sync now and Settings, one toolbar.
    expect(within(nav).getByTestId("sidebar-sync-status").textContent).toBe("Club: Offline");
    const foot = within(nav).getByRole("toolbar", { name: "Sync and settings" });
    expect(within(foot).getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["Sync now", "Settings"]);
    expect(within(nav).getByTestId("rail-settings")).toBeTruthy();
  });

  it("starts with More and More folders closed; both open and stay open on this device", async () => {
    const { core, user } = setup({ sidebarExpanded: true }, [], true);
    const nav = await column();
    const all = await within(nav).findByRole("tree", { name: "All accounts" });
    const more = within(all).getByRole("treeitem", { name: "More" });
    expect(more.getAttribute("aria-expanded")).toBe("false");
    expect(within(all).queryByRole("treeitem", { name: "Tasks" })).toBeNull();
    const ada = await within(nav).findByRole("tree", { name: "Folders of Ada Lovelace" });
    // Closed, "More folders" names the unread mail of the own folders it hides (Projekte 2, 2026 1).
    const folders = within(ada).getByRole("treeitem", { name: "More folders, 3 unread" });
    expect(folders.getAttribute("aria-expanded")).toBe("false");
    expect(words(folders.textContent)).toBe("more folders 3");
    expect(within(ada).getAllByRole("treeitem").map((row) => row.getAttribute("aria-label"))).toEqual([
      "Ada Lovelace, 6 unread (ada@vicemail.test)",
      "Inbox, 3 unread",
      "Sent",
      "More folders, 3 unread",
    ]);
    await user.click(more);
    expect(within(all).getByRole("treeitem", { name: "Tasks" })).toBeTruthy();
    await user.click(folders);
    expect(within(ada).getByRole("treeitem", { name: "Projekte, 2 unread" }).getAttribute("aria-level")).toBe("3");
    // The core keeps both on this device (UiPreferences.sidebarFolds).
    await waitFor(() => {
      expect(core.ui.preferences.sidebarFolds).toEqual([
        { key: "more", open: true },
        { key: `folders:${ADA.id}`, open: true },
      ]);
    });
    // Closing an account is kept too; opening it again leaves no entry (the default).
    await user.click(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    await waitFor(() => {
      expect(core.ui.preferences.sidebarFolds).toContainEqual({ key: `account:${CLUB.id}`, open: false });
    });
    await user.click(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    await waitFor(() => {
      expect(core.ui.preferences.sidebarFolds.map((fold) => fold.key)).toEqual(["more", `folders:${ADA.id}`]);
    });
  });

  it("marks the shown view under a closed More or More folders on that row, and Tab comes in there", async () => {
    const { user } = setup({ sidebarExpanded: true }, [], true);
    const nav = await column();
    act(() => {
      useUiStore.getState().showMailView("tasks");
    });
    const all = await within(nav).findByRole("tree", { name: "All accounts" });
    const more = within(all).getByRole("treeitem", { name: "More" });
    expect(more.className).toContain("bg-rail-selected");
    expect(more.tabIndex).toBe(0);
    expect(more.hasAttribute("aria-current")).toBe(false);
    act(() => {
      useUiStore.getState().showMailbox(ADA.id, 6);
    });
    const folders = screen.getByTestId(`sidebar-more-folders-${ADA.id}`);
    expect(folders.className).toContain("bg-rail-selected");
    expect(folders.tabIndex).toBe(0);
    await user.click(folders);
    expect(screen.getByTestId("mailbox-6").getAttribute("aria-current")).toBe("page");
    expect(screen.getByTestId(`sidebar-more-folders-${ADA.id}`).className).not.toContain("bg-rail-selected");
  });

  it("Sent of All accounts and an account's Sent are two rows: each is current only for itself", async () => {
    const { user } = setup();
    const nav = await column();
    const all = await within(nav).findByRole("tree", { name: "All accounts" });
    await user.click(within(all).getByRole("treeitem", { name: "Sent" }));
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "sent" });
    expect(within(all).getByRole("treeitem", { name: "Sent" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByTestId("mailbox-2").hasAttribute("aria-current")).toBe(false);
    // The account's Sent opens as its folder.
    await user.click(screen.getByTestId("mailbox-2"));
    expect(useUiStore.getState().view).toEqual({ kind: "mailbox", accountId: ADA.id, mailboxId: 2 });
    expect(screen.getByTestId("mailbox-2").getAttribute("aria-current")).toBe("page");
    expect(within(all).getByRole("treeitem", { name: "Sent" }).hasAttribute("aria-current")).toBe(false);
  });

  it("the foot says when the accounts last synced, or which one does not; Sync now asks every account for a pass", async () => {
    const { core, user } = setup();
    const nav = await column();
    const status = within(nav).getByTestId("sidebar-sync-status");
    await waitFor(() => {
      expect(status.getAttribute("data-tone")).toBe("offline");
    });
    await user.click(within(nav).getByRole("button", { name: "Sync now" }));
    await waitFor(() => {
      expect(core.callsOf("reload_folders")).toEqual([{ accountId: ADA.id }, { accountId: CLUB.id }]);
    });
  });

  it("finds nothing serious with every section, an offline account, folded parts and over the content", async () => {
    const { user } = setup();
    await column();
    await screen.findByRole("tree", { name: "Favourites" });
    await screen.findByRole("treeitem", { name: "Invoices" });
    await screen.findByRole("treeitem", { name: "Turniere, 1 unread" });
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    await user.click(screen.getByTestId("sidebar-more"));
    await user.click(screen.getByTestId(`sidebar-more-folders-${ADA.id}`));
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByTestId("sidebar-section-views"));
    expect(await seriousFindings()).toEqual([]);
  });

  it("folds into the rail with its toggle and Cmd+Shift+S, and stores that with the window", async () => {
    const { core, user } = setup();
    await column();
    await user.click(screen.getByRole("button", { name: "Hide sidebar" }));
    expect(await screen.findByTestId("icon-rail")).toBeTruthy();
    expect(screen.queryByTestId("sidebar-column")).toBeNull();
    await waitFor(() => {
      expect(core.ui.layout.sidebarExpanded).toBe(false);
    });
    await user.keyboard("{Meta>}{Shift>}s{/Shift}{/Meta}");
    expect(await column()).toBeTruthy();
    expect(useUiStore.getState().sidebarExpanded).toBe(true);
    // Stage 23 (decision 0084): the toggle stands in the toolbar and stays; from the keyboard the
    // focus stays on it, now named for unfolding.
    act(() => {
      screen.getByRole("button", { name: "Hide sidebar" }).focus();
    });
    await user.keyboard("{Enter}");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Show sidebar" }));
  });

  it("its edge is a splitter: arrow keys, Home and End change the width, Enter folds", async () => {
    const { user } = setup();
    const nav = await column();
    const splitter = screen.getByRole("separator", { name: "Sidebar width" });
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(SIDEBAR_WIDTH.default));
    expect(nav.style.getPropertyValue("--ui-sidebar-width")).toBe("15rem");
    act(() => {
      splitter.focus();
    });
    await user.keyboard("{ArrowRight}");
    expect(splitter.getAttribute("aria-valuenow")).toBe("256");
    expect(nav.style.getPropertyValue("--ui-sidebar-width")).toBe("16rem");
    await user.keyboard("{End}");
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(SIDEBAR_WIDTH.max));
    await user.keyboard("{ArrowRight}");
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(SIDEBAR_WIDTH.max));
    await user.keyboard("{Home}{ArrowLeft}");
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(SIDEBAR_WIDTH.min));
    await user.keyboard("{Enter}");
    expect(await screen.findByTestId("icon-rail")).toBeTruthy();
  });

  it("stores its width with the window once it rests, and opens at the stored width", async () => {
    const { core, user } = setup();
    await column();
    const splitter = screen.getByRole("separator", { name: "Sidebar width" });
    act(() => {
      splitter.focus();
    });
    // Three steps in a row are one save, after the width rests.
    await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
    expect(splitter.getAttribute("aria-valuenow")).toBe("288");
    await waitFor(() => {
      expect(core.ui.layout.sidebarWidth).toBe(288);
    });
    expect(core.callsOf("set_window_layout").filter((call) => (call as { layout: { sidebarWidth: number } }).layout.sidebarWidth !== 240)).toHaveLength(1);
    expect(useLayoutStore.getState().sidebarWidth).toBe(288);
    // Another window (or a refused save) puts a stored width in place: the column follows.
    act(() => {
      useLayoutStore.getState().applyStoredLayout({ ...core.ui.layout, sidebarWidth: 208 });
    });
    expect(splitter.getAttribute("aria-valuenow")).toBe("208");
    expect((await column()).style.getPropertyValue("--ui-sidebar-width")).toBe("13rem");
  });

  it("an account's row closes and opens all of its folders", async () => {
    const { user } = setup();
    await column();
    const club = await screen.findByRole("tree", { name: "Folders of Club" });
    await within(club).findByRole("treeitem", { name: "Turniere, 1 unread" });
    await user.click(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    expect(within(club).getAllByRole("treeitem")).toHaveLength(1);
    const heading = screen.getByTestId(`sidebar-account-${CLUB.id}`);
    expect(heading.getAttribute("aria-expanded")).toBe("false");
    // Closed, the heading shows the account's unread total.
    expect(words(heading.textContent)).toBe("club 5");
    await user.click(heading);
    expect(within(club).getAllByRole("treeitem")).toHaveLength(4);
  });

  it("a section folds to its label with a click, Left or Enter, and opens with Right", async () => {
    const { user } = setup();
    await column();
    const views = await screen.findByRole("tree", { name: "All accounts" });
    const label = within(views).getByRole("treeitem", { name: "All accounts" });
    expect(label.getAttribute("aria-expanded")).toBe("true");
    await user.click(label);
    expect(within(views).getAllByRole("treeitem")).toHaveLength(1);
    expect(label.getAttribute("aria-expanded")).toBe("false");
    act(() => {
      label.focus();
    });
    await user.keyboard("{ArrowRight}");
    expect(within(views).getAllByRole("treeitem")).toHaveLength(17);
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(document.activeElement?.textContent).toBe("Later");
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(document.activeElement).toBe(label);
    expect(label.getAttribute("aria-expanded")).toBe("false");
    await user.keyboard("{Enter}");
    expect(label.getAttribute("aria-expanded")).toBe("true");
  });

  it("in a narrow window it folds to the rail and opens over the content; Escape and a chosen folder close it", async () => {
    narrowWindow();
    const { user } = setup();
    expect(await screen.findByTestId("icon-rail")).toBeTruthy();
    expect(useUiStore.getState().sidebarNarrow).toBe(true);
    const folders = screen.getByRole("button", { name: "Folders" });
    await user.click(folders);
    const nav = await column();
    expect(nav.getAttribute("data-overlay")).toBe("true");
    expect(await seriousFindings()).toEqual([]);
    expect(folders.getAttribute("aria-expanded")).toBe("true");
    // The rail under it is out of reach; the stored state is untouched.
    expect(screen.getByTestId("icon-rail").hasAttribute("inert")).toBe(true);
    expect(useLayoutStore.getState().sidebarExpanded).toBe(true);
    // No splitter over the content.
    expect(screen.queryByRole("separator", { name: "Sidebar width" })).toBeNull();
    act(() => {
      within(nav).getByRole("treeitem", { name: "Later" }).focus();
    });
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("sidebar-column")).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Folders" }));

    await user.keyboard("{Enter}");
    await user.click(await screen.findByTestId("mailbox-5"));
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Projekte");
    expect(screen.queryByTestId("sidebar-column")).toBeNull();
  });
});
