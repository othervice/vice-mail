import { act, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { describe, expect, it, vi } from "vitest";
import type { CoreEvent, SearchRequest } from "../src/services/ipc";
import { ipcError, makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { makeSavedSearch } from "./support/search";
import { renderApp } from "./support/render";

// Stage 7: recent searches under the search field, "Save search", saved searches as views.

const ACCOUNT = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const INVOICE = makeMessage({ id: 9000, subject: "Rechnung März", preview: "Anbei die Rechnung", date: Date.now() - 3_600_000 });
const LUNCH = makeMessage({ id: 8000, subject: "Lunch", preview: "Pizza?", date: Date.now() - 7_200_000 });
const WORK_INVOICE = makeMessage({ id: 7000, accountId: WORK.id, mailboxId: 20, subject: "Rechnung Firma", date: Date.now() - 10_800_000 });

type SearchOptions = NonNullable<Parameters<typeof mockCore>[0]>["search"];

function setup(search: SearchOptions = {}, extra: Parameters<typeof mockCore>[0] = {}) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ACCOUNT, WORK],
    mailboxes: [...standardMailboxes(), makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" })],
    messages: { 1: [INVOICE, LUNCH], 20: [WORK_INVOICE] },
    views: { [INVOICE.id]: makeView(INVOICE), [WORK_INVOICE.id]: makeView(WORK_INVOICE) },
    search,
    ...extra,
  });
  return { core, ...renderApp() };
}

async function openSearch(user: ReturnType<typeof renderApp>["user"]) {
  await screen.findByRole("listbox", { name: "Messages in Inbox" });
  await user.click(screen.getByRole("button", { name: "Search" }));
  return screen.findByRole("combobox", { name: "Search mail" });
}

const lastSearch = (core: ReturnType<typeof mockCore>) => {
  const calls = core.callsOf("search");
  return calls[calls.length - 1]?.["request"] as SearchRequest | undefined;
};

const recorded = (core: ReturnType<typeof mockCore>) => core.callsOf("add_recent_search").map((args) => args["query"]);

describe("recent searches", () => {
  it("remembers a submitted search and offers it under the empty field, chosen by keyboard", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    // Nothing remembered yet: no list.
    expect(screen.queryByRole("listbox", { name: "Recent searches" })).toBeNull();
    await user.type(input, "rechnung{Enter}");
    await waitFor(() => {
      expect(recorded(core)).toEqual(["rechnung"]);
    });

    await user.keyboard("{Escape}");
    expect((input as HTMLInputElement).value).toBe("");
    const list = await screen.findByRole("listbox", { name: "Recent searches" });
    expect(input.getAttribute("aria-controls")).toBe(list.id);
    const options = within(list).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["rechnung"]);
    // The focus stays in the field; the arrow keys point at an option.
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(input);
    expect(input.getAttribute("aria-activedescendant")).toBe(options[0]?.id);
    expect(options[0]?.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{Enter}");
    expect((input as HTMLInputElement).value).toBe("rechnung");
    await waitFor(() => {
      expect(lastSearch(core)).toMatchObject({ query: "rechnung", typing: false });
    });
    expect(screen.queryByRole("listbox", { name: "Recent searches" })).toBeNull();
    await screen.findByRole("listbox", { name: "Messages in the search results for “rechnung”" });
  });

  it("runs a recent search on a click, keeps the newest first and can be cleared", async () => {
    const { core, user } = setup({
      recentSearches: [
        { query: "from:anna", usedAt: Date.now() - 1000 },
        { query: "lunch", usedAt: Date.now() - 2000 },
      ],
    });
    const input = await openSearch(user);
    const list = await screen.findByRole("listbox", { name: "Recent searches" });
    expect(within(list).getAllByRole("option").map((o) => o.textContent)).toEqual(["from:anna", "lunch"]);
    await user.click(within(list).getByRole("option", { name: "lunch" }));
    expect((input as HTMLInputElement).value).toBe("lunch");
    expect(document.activeElement).toBe(input);
    await waitFor(() => {
      expect(lastSearch(core)).toMatchObject({ query: "lunch", typing: false });
    });
    await waitFor(() => {
      expect(recorded(core)).toEqual(["lunch"]);
    });

    await user.click(screen.getByRole("button", { name: "Clear search" }));
    const again = await screen.findByRole("listbox", { name: "Recent searches" });
    expect(within(again).getAllByRole("option").map((o) => o.textContent)).toEqual(["lunch", "from:anna"]);
    await user.click(screen.getByRole("button", { name: "Clear recent searches" }));
    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Recent searches" })).toBeNull();
    });
    expect(core.callsOf("clear_recent_searches")).toHaveLength(1);
    expect(core.search.recentSearches).toEqual([]);
  });

  it("closes the list with the first Escape and the search with the next", async () => {
    const { user } = setup({ recentSearches: [{ query: "lunch", usedAt: Date.now() }] });
    await openSearch(user);
    await screen.findByRole("listbox", { name: "Recent searches" });
    await user.keyboard("{ArrowDown}{Escape}");
    expect(screen.queryByRole("listbox", { name: "Recent searches" })).toBeNull();
    expect(screen.getByRole("combobox", { name: "Search mail" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("combobox", { name: "Search mail" })).toBeNull();
  });

  it("remembers a search once one of its hits is opened, once per search", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    await user.type(input, "rechnung");
    const results = await screen.findByRole("listbox", { name: "Messages in the search results for “rechnung”" });
    const hits = await within(results).findAllByRole("option");
    expect(recorded(core)).toEqual([]);
    await user.click(hits[0] as HTMLElement);
    await waitFor(() => {
      expect(recorded(core)).toEqual(["rechnung"]);
    });
    await user.click(hits[1] as HTMLElement);
    expect(recorded(core)).toEqual(["rechnung"]);
  });

  it("says so when the search could not be remembered", async () => {
    const { user } = setup({}, { failures: { add_recent_search: ipcError("internal") } });
    const input = await openSearch(user);
    await user.type(input, "lunch{Enter}");
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Could not remember this search");
  });
});

describe("saving a search", () => {
  it("saves the current search under a name and shows it in the sidebar", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    expect(screen.queryByRole("button", { name: "Save search" })).toBeNull();
    await user.type(input, "rechnung");
    await user.click(screen.getByRole("button", { name: "Save search" }));
    const form = screen.getByRole("group", { name: "Save this search" });
    const name = within(form).getByRole("textbox", { name: "Name" });
    expect(document.activeElement).toBe(name);
    expect((name as HTMLInputElement).value).toBe("rechnung");
    expect(within(form).getByText(/looks in all accounts/)).toBeTruthy();
    await user.clear(name);
    await user.type(name, "Invoices{Enter}");

    expect((await screen.findByTestId("search-saved")).textContent).toContain("Saved as “Invoices”");
    expect(core.callsOf("save_search")).toEqual([{ name: "Invoices", query: "rechnung" }]);
    expect(screen.queryByRole("group", { name: "Save this search" })).toBeNull();
    // Enter in the name field did not submit the search a second time as a recent one.
    expect(recorded(core)).toEqual([]);

    await user.click(screen.getByTestId("rail-folders"));
    // Stage 23 (decision 0085): the saved searches are under "More" of the first section.
    const nav = await screen.findByTestId("sidebar-views");
    expect(within(nav).getByRole("treeitem", { name: "Invoices" })).toBeTruthy();
  });

  it("explains a name that is taken and keeps the form open", async () => {
    const { core, user } = setup({ savedSearches: [makeSavedSearch({ name: "Invoices" })] });
    const input = await openSearch(user);
    await user.type(input, "rechnung");
    await user.click(screen.getByRole("button", { name: "Save search" }));
    const form = screen.getByRole("group", { name: "Save this search" });
    const name = within(form).getByRole("textbox", { name: "Name" });
    await user.clear(name);
    await user.type(name, "invoices");
    await user.click(within(form).getByRole("button", { name: "Save" }));
    const alert = await within(form).findByRole("alert");
    expect(alert.textContent).toContain("Could not save the search");
    expect(alert.textContent).toContain("A saved search with this name exists already.");
    expect(core.search.savedSearches).toHaveLength(1);
    // Cancel closes the form and gives the focus back to the button.
    await user.click(within(form).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: "Save this search" })).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Save search" }));
  });

  it("says when no more searches can be saved", async () => {
    const full = Array.from({ length: 100 }, (_, n) =>
      makeSavedSearch({ id: `5a5a5a5a-0000-4000-8000-${String(n).padStart(12, "0")}`, name: `Search ${String(n)}`, position: n }),
    );
    const { user } = setup({ savedSearches: full });
    const input = await openSearch(user);
    await user.type(input, "rechnung");
    await user.click(screen.getByRole("button", { name: "Save search" }));
    await user.keyboard("{Enter}");
    const alert = await within(screen.getByRole("group", { name: "Save this search" })).findByRole("alert");
    expect(alert.textContent).toContain("You have saved as many searches as possible.");
  });
});

describe("saved searches as views", () => {
  it("shows an empty state without saved searches and nothing in the sidebar", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByRole("button", { name: /^Saved views/ }));
    expect(await screen.findByRole("heading", { name: "No saved searches yet" })).toBeTruthy();
    await user.click(screen.getByTestId("rail-folders"));
    await screen.findByTestId("sidebar-views");
    expect(screen.queryByTestId("saved-search-nav-entry")).toBeNull();
  });

  it("opens a saved search from the sidebar with its results over all accounts", async () => {
    const { core, user } = setup({ savedSearches: [makeSavedSearch({ name: "Invoices", query: "rechnung" })] });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByTestId("rail-folders"));
    // Stage 23 (decision 0085): the saved searches are under "More" of the first section.
    const nav = await screen.findByTestId("sidebar-views");
    await user.click(within(nav).getByRole("treeitem", { name: "Invoices" }));

    expect(await screen.findByRole("heading", { name: "Invoices" })).toBeTruthy();
    expect(within(nav).getByRole("treeitem", { name: "Invoices" }).getAttribute("aria-current")).toBe("page");
    const results = await screen.findByRole("listbox", { name: "Messages in the search results for “rechnung”" });
    expect(await within(results).findAllByRole("option")).toHaveLength(2);
    expect(lastSearch(core)).toMatchObject({ query: "rechnung", scope: { kind: "all" }, typing: false });

    await user.click(screen.getByRole("button", { name: "All saved searches" }));
    const list = await screen.findByRole("list", { name: "Saved searches" });
    expect(within(list).getByText("Searches for: rechnung")).toBeTruthy();
  });

  it("renames inline and deletes after a confirmation", async () => {
    const { core, user } = setup({
      savedSearches: [
        makeSavedSearch({ name: "Invoices", query: "rechnung" }),
        makeSavedSearch({ id: "5a5a5a5a-0000-4000-8000-000000000002", name: "Lunch", query: "lunch", position: 1 }),
      ],
    });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByRole("button", { name: /^Saved views/ }));
    const list = await screen.findByRole("list", { name: "Saved searches" });
    expect(within(list).getAllByRole("listitem").map((row) => within(row).getAllByRole("button")[0]?.textContent)).toEqual([
      "Invoices",
      "Lunch",
    ]);

    // A taken name is explained at the field.
    await user.click(within(list).getByRole("button", { name: "Rename “Invoices”" }));
    const field = within(list).getByRole("textbox", { name: "New name for “Invoices”" });
    expect(document.activeElement).toBe(field);
    await user.clear(field);
    await user.type(field, "lunch{Enter}");
    expect((await within(list).findByRole("alert")).textContent).toContain("A saved search with this name exists already.");
    await user.clear(field);
    await user.type(field, "Bills{Enter}");
    await within(list).findByRole("button", { name: "Bills" });
    expect(core.search.savedSearches.map((s) => s.name)).toEqual(["Bills", "Lunch"]);
    expect(document.activeElement).toBe(within(list).getByRole("button", { name: "Rename “Bills”" }));
    // Escape cancels a rename.
    await user.click(within(list).getByRole("button", { name: "Rename “Lunch”" }));
    await user.keyboard("{Escape}");
    expect(within(list).getByRole("button", { name: "Lunch" })).toBeTruthy();

    await user.click(within(list).getByRole("button", { name: "Delete “Bills”" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete the saved search “Bills”?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(within(list).queryByRole("button", { name: "Bills" })).toBeNull();
    });
    expect(core.callsOf("delete_saved_search")).toEqual([{ savedSearchId: "5a5a5a5a-0000-4000-8000-000000000001" }]);
    expect(core.search.savedSearches.map((s) => s.name)).toEqual(["Lunch"]);
  });

  it("goes back to the list after deleting the open saved search, and explains one that is gone", async () => {
    const { core, user } = setup({ savedSearches: [makeSavedSearch({ name: "Invoices", query: "rechnung" })] });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByRole("button", { name: /^Saved views/ }));
    await user.click(await screen.findByRole("button", { name: "Invoices" }));
    expect(await screen.findByRole("heading", { name: "Invoices" })).toBeTruthy();

    // Deleted on another device: the sync applies it and the core says so.
    core.search.savedSearches.splice(0);
    const event: CoreEvent = { type: "savedSearchesChanged" };
    await act(async () => {
      await emit("core-event", event);
    });
    expect(await screen.findByRole("heading", { name: "This saved search no longer exists" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "All saved searches" }));
    expect(await screen.findByRole("heading", { name: "No saved searches yet" })).toBeTruthy();
  });

  it("deleting the open saved search returns to the list", async () => {
    const { user } = setup({ savedSearches: [makeSavedSearch({ name: "Invoices", query: "rechnung" })] });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByRole("button", { name: /^Saved views/ }));
    await user.click(await screen.findByRole("button", { name: "Invoices" }));
    await user.click(await screen.findByRole("button", { name: "Delete “Invoices”" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "No saved searches yet" })).toBeTruthy();
  });

  it("shows saved searches another device added (savedSearchesChanged)", async () => {
    const { core, user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByRole("button", { name: /^Saved views/ }));
    await screen.findByRole("heading", { name: "No saved searches yet" });
    core.search.savedSearches.push(makeSavedSearch({ name: "From Anna", query: "from:anna" }));
    const event: CoreEvent = { type: "savedSearchesChanged" };
    await act(async () => {
      await emit("core-event", event);
    });
    const list = await screen.findByRole("list", { name: "Saved searches" });
    expect(within(list).getByRole("button", { name: "From Anna" })).toBeTruthy();
  });

  it("shows the texts in German", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: { 1: [INVOICE] },
      preferences: { theme: "light", language: "de" },
      search: { recentSearches: [{ query: "lunch", usedAt: Date.now() }] },
    });
    const { user } = renderApp("de");
    await screen.findByRole("listbox", { name: /^Nachrichten in/ });
    await user.click(screen.getByRole("button", { name: "Suchen" }));
    expect(await screen.findByRole("listbox", { name: "Letzte Suchen" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Letzte Suchen löschen" })).toBeTruthy();
    await user.type(screen.getByRole("combobox", { name: "Mails durchsuchen" }), "rechnung");
    expect(screen.getByRole("button", { name: "Suche speichern" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /^Gespeicherte Ansichten/ }));
    expect(await screen.findByRole("heading", { name: "Noch keine gespeicherten Suchen" })).toBeTruthy();
  });
});
