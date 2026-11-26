import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { makeSavedSearch } from "./support/search";
import { renderApp } from "./support/render";

/*
 * Stage 8, keyboard: every tree of the accounts-and-folders column ("All accounts" with "More",
 * favourites, each account with its folders) is one Tab stop with the WAI-ARIA tree view keys
 * inside, and the rail's two groups are one Tab stop each. Tab goes column, list in a handful of
 * presses. Stage 23 (decision 0085): an account's tree is its heading, Inbox, Sent and "More
 * folders" with the other folders below it (the fake core starts with it open).
 */

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const GROUP = { id: "55555555-5555-4555-8555-555555555555", name: "Freizeit", accountIds: [ADA.id, WORK.id], position: 0 };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

/** Ada's row, then Inbox, Sent, "More folders" with Drafts, Trash, Dokumente, Projekte (2026 below it). */
function setup(options: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: [...standardMailboxes(), makeMailbox({ id: 7, path: "Dokumente" })],
    messages: { 1: makeMessages(3), 5: makeMessages(2, { mailboxId: 5 }), 6: makeMessages(1, { mailboxId: 6 }) },
    ...options,
  });
  return { core, ...renderApp() };
}

async function unfold(user: ReturnType<typeof renderApp>["user"]) {
  await screen.findByRole("listbox", { name: /^Messages in/ });
  await user.click(screen.getByTestId("rail-folders"));
  return screen.findByRole("tree", { name: "Folders of Ada Lovelace" });
}

const ADA_ROW = `sidebar-account-${ADA.id}`;
const ADA_MORE = `sidebar-more-folders-${ADA.id}`;

const focused = () => document.activeElement;
// The accessible name as screen readers compute it (aria-hidden parts left out, stage 23: the
// toolbar's search is named by its text, not by its shortcut beside it).
function visibleText(node: Node): string {
  if (node instanceof Element && node.getAttribute("aria-hidden") === "true") return "";
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
  return [...node.childNodes].map(visibleText).join("");
}
const nameOf = (element: Element | null) => (element ? (element.getAttribute("aria-label") ?? visibleText(element)) : null);
const heading = () => screen.getByRole("heading", { level: 1 }).textContent;

/** The rows of a tree that Tab can reach: exactly one per tree. */
function tabStops(tree: HTMLElement) {
  return within(tree)
    .getAllByRole("treeitem")
    .filter((row) => row.tabIndex === 0)
    .map(nameOf);
}

/** Presses Tab from the start of the window until the message list has the focus; the stops on the way. */
async function tabToList(user: ReturnType<typeof renderApp>["user"]) {
  act(() => {
    (document.activeElement as HTMLElement | null)?.blur();
  });
  const stops: string[] = [];
  for (let i = 0; i < 60; i += 1) {
    await user.tab();
    const now = focused();
    if (now?.getAttribute("role") === "listbox") return stops;
    stops.push(`${now?.getAttribute("role") ?? now?.tagName.toLowerCase() ?? "?"}: ${nameOf(now) ?? ""}`);
  }
  throw new Error(`Tab never reached the list: ${stops.join(" | ")}`);
}

describe("folder tree: keyboard model", () => {
  it("is a tree with levels, positions and open states; the current folder is its one Tab stop", async () => {
    const { user } = setup();
    const tree = await unfold(user);
    const rows = within(tree).getAllByRole("treeitem");
    expect(rows.map(nameOf)).toEqual([
      "Ada Lovelace, 6 unread (ada@vicemail.test)",
      "Inbox, 3 unread",
      "Sent",
      "More folders",
      "Drafts",
      "Trash",
      "Dokumente",
      "Projekte, 2 unread",
      "2026, 1 unread",
    ]);
    // The account is the root of its tree: it opens and closes all of its folders.
    const account = screen.getByTestId(ADA_ROW);
    expect([account.getAttribute("aria-level"), account.getAttribute("aria-posinset"), account.getAttribute("aria-expanded")]).toEqual(["1", "1", "true"]);
    const inbox = screen.getByTestId("mailbox-1");
    expect([inbox.getAttribute("aria-level"), inbox.getAttribute("aria-posinset"), inbox.getAttribute("aria-setsize")]).toEqual(["2", "1", "3"]);
    expect(inbox.getAttribute("aria-current")).toBe("page");
    expect(inbox.hasAttribute("aria-expanded")).toBe(false);
    const more = screen.getByTestId(ADA_MORE);
    expect([more.getAttribute("aria-level"), more.getAttribute("aria-posinset"), more.getAttribute("aria-expanded")]).toEqual(["2", "3", "true"]);
    const projekte = screen.getByTestId("mailbox-5");
    expect([projekte.getAttribute("aria-level"), projekte.getAttribute("aria-posinset"), projekte.getAttribute("aria-setsize"), projekte.getAttribute("aria-expanded")]).toEqual([
      "3",
      "4",
      "4",
      "true",
    ]);
    const child = screen.getByTestId("mailbox-6");
    expect([child.getAttribute("aria-level"), child.getAttribute("aria-posinset"), child.getAttribute("aria-setsize")]).toEqual(["4", "1", "1"]);
    // Only the current folder is in the Tab order; the "…" of each row is not.
    expect(tabStops(tree)).toEqual(["Inbox, 3 unread"]);
    expect(screen.getByTestId("folder-menu-5").tabIndex).toBe(-1);
  });

  it("Up and Down move without opening; Tab leaves the tree and comes back in on the current folder", async () => {
    const { user } = setup();
    const tree = await unfold(user);
    act(() => {
      screen.getByTestId("mailbox-1").focus();
    });
    await user.keyboard("{ArrowUp}");
    expect(focused()).toBe(screen.getByTestId(ADA_ROW));
    // No wrap at the top.
    await user.keyboard("{ArrowUp}");
    expect(focused()).toBe(screen.getByTestId(ADA_ROW));
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(focused()).toBe(screen.getByTestId("mailbox-2"));
    expect(tabStops(tree)).toEqual(["Sent"]);
    // Moving opens nothing: the list still shows the inbox.
    expect(heading()).toBe("Inbox");
    await user.tab();
    expect(tree.contains(focused())).toBe(false);
    expect(tabStops(tree)).toEqual(["Inbox, 3 unread"]);
    await user.tab({ shift: true });
    expect(focused()).toBe(screen.getByTestId("mailbox-1"));
  });

  it("Right opens a folder or goes to its first child, Left closes it or goes to the parent, Home and End", async () => {
    const { user } = setup();
    const tree = await unfold(user);
    act(() => {
      screen.getByTestId("mailbox-1").focus();
    });
    await user.keyboard("{End}");
    expect(focused()).toBe(screen.getByTestId("mailbox-6"));
    // A folder without children: Right does nothing.
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe(screen.getByTestId("mailbox-6"));
    await user.keyboard("{ArrowLeft}");
    expect(focused()).toBe(screen.getByTestId("mailbox-5"));
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByTestId("mailbox-5").getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByTestId("mailbox-6")).toBeNull();
    expect(focused()).toBe(screen.getByTestId("mailbox-5"));
    // Left on a closed folder under "More folders" goes to that row; Left there closes it (the
    // folders below it go), Left again goes to the account's row, which closes the account; Right
    // opens it again.
    await user.keyboard("{ArrowLeft}");
    expect(focused()).toBe(screen.getByTestId(ADA_MORE));
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByTestId(ADA_MORE).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByTestId("mailbox-7")).toBeNull();
    await user.keyboard("{ArrowLeft}");
    expect(focused()).toBe(screen.getByTestId(ADA_ROW));
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByTestId(ADA_ROW).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByTestId("mailbox-1")).toBeNull();
    expect(tabStops(tree)).toEqual(["Ada Lovelace, 6 unread (ada@vicemail.test)"]);
    await user.keyboard("{ArrowRight}");
    expect(screen.getByTestId(ADA_ROW).getAttribute("aria-expanded")).toBe("true");
    expect(focused()).toBe(screen.getByTestId(ADA_ROW));
    await user.keyboard("{End}");
    expect(focused()).toBe(screen.getByTestId(ADA_MORE));
    await user.keyboard("{ArrowRight}");
    expect(screen.getByTestId(ADA_MORE).getAttribute("aria-expanded")).toBe("true");
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe(screen.getByTestId("mailbox-3"));
    await user.keyboard("{End}");
    expect(focused()).toBe(screen.getByTestId("mailbox-5"));
    await user.keyboard("{ArrowRight}");
    expect(screen.getByTestId("mailbox-5").getAttribute("aria-expanded")).toBe("true");
    expect(focused()).toBe(screen.getByTestId("mailbox-5"));
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe(screen.getByTestId("mailbox-6"));
    await user.keyboard("{Home}");
    expect(focused()).toBe(screen.getByTestId(ADA_ROW));
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe(screen.getByTestId("mailbox-1"));
    expect(tabStops(tree)).toEqual(["Inbox, 3 unread"]);
  });

  it("a letter goes to the next folder starting with it; letters are no app shortcuts there", async () => {
    const { user } = setup();
    await unfold(user);
    act(() => {
      screen.getByTestId("mailbox-1").focus();
    });
    await user.keyboard("s");
    expect(focused()).toBe(screen.getByTestId("mailbox-2"));
    // Typed right after: "sd" starts nothing, so "d" is a new search. The same letter again goes
    // on to the next folder with it (Drafts, then Dokumente).
    await user.keyboard("d");
    expect(focused()).toBe(screen.getByTestId("mailbox-3"));
    await user.keyboard("d");
    expect(focused()).toBe(screen.getByTestId("mailbox-7"));
    await user.keyboard("2");
    expect(focused()).toBe(screen.getByTestId("mailbox-6"));
    // From the end it starts over at the top; case and accents do not matter.
    await user.keyboard("I");
    expect(focused()).toBe(screen.getByTestId("mailbox-1"));
    await new Promise((resolve) => setTimeout(resolve, 600));
    // Letters typed quickly form one search.
    await user.keyboard("dr");
    expect(focused()).toBe(screen.getByTestId("mailbox-3"));
    // "c" writes a new message elsewhere; in the tree it is type-ahead and opens nothing.
    await user.keyboard("c");
    expect(screen.queryByTestId("composer")).toBeNull();
    expect(focused()).toBe(screen.getByTestId("mailbox-3"));
  });

  it("Enter and Space open the folder; a role folder opens its rail view; the opened one is the Tab stop", async () => {
    const { user } = setup();
    const tree = await unfold(user);
    act(() => {
      screen.getByTestId("mailbox-1").focus();
    });
    await user.keyboard("p");
    await user.keyboard("{Enter}");
    expect(heading()).toBe("Projekte");
    expect(screen.getByTestId("mailbox-5").getAttribute("aria-current")).toBe("page");
    expect(focused()).toBe(screen.getByTestId("mailbox-5"));
    await user.keyboard("{ArrowDown}");
    await user.keyboard(" ");
    expect(heading()).toBe("2026");
    expect(await screen.findByRole("listbox", { name: "Messages in 2026" })).toBeTruthy();
    expect(tabStops(tree)).toEqual(["2026, 1 unread"]);
    await user.keyboard("{Home}{ArrowDown}{ArrowDown}{Enter}");
    expect(heading()).toBe("Sent");
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "sent" });
    await user.tab();
    expect(tabStops(tree)).toEqual(["Sent"]);
  });

  it("Shift+F10, the context-menu key and a right click open the folder's menu; it closes back onto the row", async () => {
    const { user } = setup();
    await unfold(user);
    const projekte = screen.getByTestId("mailbox-5");
    act(() => {
      projekte.focus();
    });
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu", { name: "Options for Projekte" });
    // Stage 21: "Mark folder as read" comes first while the folder has unread mail.
    expect(focused()).toBe(within(menu).getByRole("menuitem", { name: "Mark folder as read" }));
    // Keys inside the menu stay in the menu (Enter there does not open the folder).
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(focused()).toBe(projekte);

    await user.keyboard("{ContextMenu}");
    expect(await screen.findByRole("menu", { name: "Options for Projekte" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(focused()).toBe(projekte);

    await user.pointer({ keys: "[MouseRight]", target: screen.getByTestId("mailbox-2") });
    const sent = await screen.findByRole("menu", { name: "Options for Sent" });
    await user.click(within(sent).getByRole("menuitem", { name: "Folder colour…" }));
    const picker = await screen.findByRole("dialog", { name: "Colour of Sent" });
    expect(heading()).toBe("Inbox");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(picker.isConnected).toBe(false);
    });
    // The colour picker gives the focus back to the folder's row.
    expect(focused()).toBe(screen.getByTestId("mailbox-2"));
  });
});

describe("the other sidebar trees", () => {
  function many() {
    const mailboxes = [
      ...standardMailboxes().map((mailbox) =>
        mailbox.id === 5 ? { ...mailbox, favoritePosition: 0 } : mailbox.id === 2 ? { ...mailbox, favoritePosition: 1 } : mailbox,
      ),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
    ];
    return setup({
      accounts: [ADA, WORK],
      mailboxes,
      messages: { 1: makeMessages(2), 5: makeMessages(1, { mailboxId: 5 }), 20: makeMessages(1, { mailboxId: 20 }) },
      search: {
        groups: [GROUP],
        savedSearches: [makeSavedSearch({ id: "5a5a5a5a-0000-4000-8000-000000000001", name: "Invoices" }), makeSavedSearch({ id: "5a5a5a5a-0000-4000-8000-000000000002", name: "Lunch", position: 1 })],
      },
    });
  }

  it("“All accounts” (with the groups and saved searches under “More”), the favourites and each account are trees of their own, one Tab stop each", async () => {
    const { user } = many();
    await unfold(user);
    await screen.findByRole("tree", { name: "Favourites" });
    await screen.findByRole("treeitem", { name: "Invoices" });
    const trees = screen.getAllByRole("tree");
    expect(trees.map((tree) => tree.getAttribute("aria-label") ?? document.getElementById(tree.getAttribute("aria-labelledby") ?? "")?.textContent)).toEqual([
      "All accounts",
      "Favourites",
      "Folders of Ada Lovelace",
      "Folders of Work",
    ]);
    for (const tree of trees) expect(tabStops(tree)).toHaveLength(1);
    const favorites = screen.getByRole("tree", { name: "Favourites" });
    const rows = within(favorites).getAllByRole("treeitem");
    // The section's label is the root row: it folds the section.
    expect(rows.map((row) => [row.getAttribute("aria-level"), row.getAttribute("aria-posinset"), row.getAttribute("aria-setsize")])).toEqual([
      ["1", "1", "1"],
      ["2", "1", "2"],
      ["2", "2", "2"],
    ]);
    expect(rows[0]?.getAttribute("aria-expanded")).toBe("true");
  });

  it("each opens its entry with Enter after the arrow keys", async () => {
    const { user } = many();
    await unfold(user);
    const favorites = await screen.findByRole("tree", { name: "Favourites" });
    act(() => {
      within(favorites).getByRole("treeitem", { name: /^Projekte/ }).focus();
    });
    await user.keyboard("{ArrowDown}");
    expect(nameOf(focused())).toBe("Sent in Ada Lovelace");
    await user.keyboard("{ArrowUp}{Enter}");
    expect(heading()).toBe("Projekte");
    expect(tabStops(favorites)).toEqual(["Projekte in Ada Lovelace, 2 unread"]);

    const inboxes = screen.getByRole("tree", { name: "All accounts" });
    act(() => {
      within(inboxes).getByRole("treeitem", { name: "Freizeit, 3 unread" }).focus();
    });
    await user.keyboard("{End}");
    expect(nameOf(focused())).toBe("Outbox");
    await user.keyboard("{Home}");
    expect(nameOf(focused())).toBe("All accounts");
    await user.keyboard("{ArrowDown}{Enter}");
    expect(await screen.findByRole("listbox", { name: "Messages in All inboxes" })).toBeTruthy();
    expect(within(inboxes).getByRole("treeitem", { name: "Inbox, 3 unread" }).getAttribute("aria-current")).toBe("page");

    const saved = inboxes;
    act(() => {
      within(saved).getByRole("treeitem", { name: "Invoices" }).focus();
    });
    await user.keyboard("l");
    expect(nameOf(focused())).toBe("Lunch");
    await user.keyboard("{Enter}");
    expect(within(saved).getByRole("treeitem", { name: "Lunch" }).getAttribute("aria-current")).toBe("page");
    expect(useUiStore.getState().view).toMatchObject({ kind: "mail", mailView: "savedViews", savedSearchId: "5a5a5a5a-0000-4000-8000-000000000002" });
  });
});

describe("the rail", () => {
  it("its views and its bottom group are one Tab stop each, with Up, Down, Home and End", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    const views = screen.getByRole("toolbar", { name: "Mail views" });
    expect(views.getAttribute("aria-orientation")).toBe("vertical");
    const reachable = (group: HTMLElement) => within(group).getAllByRole("button").filter((button) => button.tabIndex === 0);
    expect(reachable(views)).toEqual([screen.getByTestId("rail-inbox")]);
    act(() => {
      screen.getByTestId("rail-inbox").focus();
    });
    await user.keyboard("{ArrowDown}");
    expect(focused()).toBe(screen.getByTestId("rail-later"));
    // Stage 17: "Folders" is last; the chosen views stand between the inbox and Sent.
    await user.keyboard("{End}");
    expect(focused()).toBe(screen.getByTestId("rail-folders"));
    await user.keyboard("{Home}{ArrowUp}");
    expect(focused()).toBe(screen.getByTestId("rail-inbox"));
    // Enter is the button's own: it opens the view, which becomes the tab stop.
    await user.keyboard("{End}{ArrowUp}{Enter}");
    expect(heading()).toBe("Trash");
    expect(reachable(views)).toEqual([screen.getByTestId("rail-trash")]);

    const bottom = screen.getByRole("toolbar", { name: "Activity and settings" });
    // Folded, the account switcher is part of it; the selected account is the stop.
    expect(reachable(bottom)).toEqual([screen.getByTestId(`account-switch-${ADA.id}`)]);
    act(() => {
      screen.getByTestId(`account-switch-${ADA.id}`).focus();
    });
    await user.keyboard("{End}");
    expect(focused()).toBe(screen.getByTestId("rail-settings"));
    await user.keyboard("{ArrowUp}");
    expect(focused()).toBe(screen.getByTestId("rail-outbox"));
  });
});

describe("the Tab path to the message list", () => {
  it("is a handful of stops with the sidebar unfolded: rail, account, folder tree, rail, list header", async () => {
    const { user } = setup();
    await unfold(user);
    const stops = await tabToList(user);
    // Stage 23 (decision 0084): first the toolbar across the window (the column's toggle, New
    // mail, the list's controls, the open message's actions as one stop, search), then the column
    // (its edge, the views, the account with its folders as one tree, the foot's buttons as one
    // toolbar; decision 0085), then the list. The quick filter is folded behind its button.
    expect(stops).toEqual([
      "button: Hide sidebar",
      "button: New message",
      "button: Quick filter",
      "button: Sorted by Date, Newest first",
      "button: List options",
      "button: Conversations",
      "button: Reply",
      "button: Search",
      "separator: Sidebar width",
      "treeitem: Views",
      "treeitem: Inbox, 3 unread",
      "button: Sync now",
    ]);
  });

  it("stays short with two accounts, favourites, inboxes and saved searches", async () => {
    const { user } = setup({
      accounts: [ADA, WORK],
      mailboxes: [
        ...standardMailboxes().map((mailbox) => (mailbox.id === 5 ? { ...mailbox, favoritePosition: 0 } : mailbox)),
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
        makeMailbox({ id: 21, accountId: WORK.id, path: "Kunden" }),
        makeMailbox({ id: 22, accountId: WORK.id, path: "Kunden/Nord", parentId: 21 }),
      ],
      search: { groups: [GROUP], savedSearches: [makeSavedSearch({ name: "Invoices" })] },
    });
    await unfold(user);
    await screen.findByRole("treeitem", { name: "Invoices" });
    await screen.findByRole("tree", { name: "Folders of Work" });
    const stops = await tabToList(user);
    const sidebar = stops.slice(stops.indexOf("button: Search") + 1, stops.indexOf("button: Sync now"));
    // One stop per tree ("All accounts" with the groups and saved searches, favourites, each
    // account with its folders; decision 0085), plus the column's edge (its toggle is in the
    // toolbar since stage 23, decision 0084).
    expect(sidebar.filter((stop) => stop.startsWith("treeitem"))).toHaveLength(4);
    expect(sidebar).toHaveLength(5);
  });
});
