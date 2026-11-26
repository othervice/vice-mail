import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { useLayoutStore } from "../src/stores/layout";
import { SIDEBAR_WIDTH, useUiStore } from "../src/stores/ui";
import type { OperationInfo } from "../src/services/ipc";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { makeSavedSearch } from "./support/search";
import { renderApp } from "./support/render";

/*
 * docs/DESIGN.md, "Seitenspalte": the accounts-and-folders column replaces the icon rail while
 * it is open; a toggle, Cmd/Ctrl+Shift+S and the palette fold it. Its edge is a window splitter
 * (drag or arrow keys). Each account is the root of its folder tree, with its unread total and,
 * when something is wrong, the reason in its name. In a narrow window it opens over the content.
 */

const ADA = makeAccount({ writesEnabled: true });
const CLUB = makeAccount({
  id: "33333333-3333-4333-8333-333333333333",
  email: "board@club.example",
  displayName: "Club",
  color: 5,
  status: { state: "offline", progress: null, error: { code: "connection_lost", message: "reset", detail: null }, retryAt: null, lastSyncAt: null },
});

function setup(layout: { sidebarExpanded: boolean } = { sidebarExpanded: true }, operations: OperationInfo[] = []) {
  const core = mockCore({
    accounts: [ADA, CLUB],
    operations,
    mailboxes: [
      ...standardMailboxes().map((mailbox) => (mailbox.id === 5 ? { ...mailbox, favoritePosition: 0 } : mailbox)),
      makeMailbox({ id: 40, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server", unread: 4 }),
      makeMailbox({ id: 41, accountId: CLUB.id, path: "Turniere", unread: 1 }),
    ],
    ui: { layout: { readingPane: "right", density: "comfortable", sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true, ...layout } },
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
    const activity = within(nav).getByTestId("rail-activity");
    await waitFor(() => {
      expect(activity.getAttribute("aria-label")).toBe("Activity, 1 waiting, 0 need attention");
    });
    // The column shows the name and the count as two words ("Activity 1"), both in the name.
    expect(words(activity.textContent)).toBe("activity 1");
    expect(words(activity.getAttribute("aria-label"))).toContain(words(activity.textContent));
  });

  it("replaces the rail: views, unified inbox with its count, every account's tree, the bottom group", async () => {
    setup();
    const nav = await column();
    expect(screen.queryByTestId("icon-rail")).toBeNull();
    expect(await within(nav).findByRole("treeitem", { name: "All inboxes, 7 unread" })).toBeTruthy();
    const views = within(nav).getByRole("tree", { name: "Views" });
    expect(within(views).getAllByRole("treeitem").map((row) => row.textContent)).toEqual([
      "Views",
      "Later",
      "Waiting for reply",
      "Tasks",
      "Reading list",
      "Pinned attachments",
      "Attachments",
      "Flagged",
      "Pinned",
      "Saved views",
      // Stage 17 (decision 0050).
      "Contacts",
    ]);
    // Each account is the root of its own tree, named with address and unread total.
    const club = await within(nav).findByRole("tree", { name: "Folders of Club" });
    const root = within(club).getAllByRole("treeitem")[0];
    expect(root?.getAttribute("aria-level")).toBe("1");
    // Offline: a quiet mark, the state and its reason in the name (never the colour alone).
    expect(root?.getAttribute("aria-label")).toBe("Club, 5 unread (board@club.example), offline: The connection to the server was lost.");
    // Label in name (WCAG 2.5.3): the row shows "Club 5"; the name starts with it.
    expect(words(root?.textContent)).toBe("club 5");
    expect(words(root?.getAttribute("aria-label"))).toContain(words(root?.textContent));
    expect(screen.getByTestId(`account-status-${CLUB.id}`)).toBeTruthy();
    expect(within(nav).getByRole("toolbar", { name: "Activity and settings" })).toBeTruthy();
    expect(within(nav).getByTestId("rail-settings")).toBeTruthy();
  });

  it("finds nothing serious with every section, an offline account, folded parts and over the content", async () => {
    const { user } = setup();
    await column();
    await screen.findByRole("tree", { name: "Favourites" });
    await screen.findByRole("tree", { name: "Saved searches" });
    await screen.findByRole("treeitem", { name: "Turniere, 1 unread" });
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByTestId(`sidebar-account-${CLUB.id}`));
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
    expect(screen.getByTestId(`sidebar-account-${CLUB.id}`).getAttribute("aria-expanded")).toBe("false");
    await user.click(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    expect(within(club).getAllByRole("treeitem")).toHaveLength(3);
  });

  it("a section folds to its label with a click, Left or Enter, and opens with Right", async () => {
    const { user } = setup();
    await column();
    const views = screen.getByRole("tree", { name: "Views" });
    const label = within(views).getByRole("treeitem", { name: "Views" });
    expect(label.getAttribute("aria-expanded")).toBe("true");
    await user.click(label);
    expect(within(views).getAllByRole("treeitem")).toHaveLength(1);
    expect(label.getAttribute("aria-expanded")).toBe("false");
    act(() => {
      label.focus();
    });
    await user.keyboard("{ArrowRight}");
    expect(within(views).getAllByRole("treeitem")).toHaveLength(11);
    await user.keyboard("{ArrowDown}");
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
