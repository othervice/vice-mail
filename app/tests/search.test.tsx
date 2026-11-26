import { screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { accountFill } from "../src/features/accounts/accountColor";
import { monthFirstFor } from "../src/features/search/terms";
import type { MessageListItem, SearchRequest, SearchTermInfo } from "../src/services/ipc";
import { ipcError, makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { makeContact } from "./support/contacts";
import { makeHit, searchPage } from "./support/search";
import { renderApp } from "./support/render";
import { showHeaderDetails } from "./support/reading";
import { optionName } from "./support/names";

const ACCOUNT = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });

const INVOICE = makeMessage({
  id: 9000,
  subject: "Rechnung März",
  preview: "Anbei die Rechnung für März",
  from: { name: "Anna Berg", email: "anna@example.org" },
  date: Date.now() - 3_600_000,
});
const LUNCH = makeMessage({ id: 8000, subject: "Lunch", preview: "Pizza?", date: Date.now() - 7_200_000 });
const WORK_INVOICE = makeMessage({
  id: 7000,
  accountId: WORK.id,
  mailboxId: 20,
  subject: "Rechnung Firma",
  date: Date.now() - 10_800_000,
});

function setup(options: Parameters<typeof mockCore>[0] = {}) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ACCOUNT, WORK],
    mailboxes: [
      ...standardMailboxes(),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
    ],
    messages: { 1: [INVOICE, LUNCH], 20: [WORK_INVOICE] },
    views: { [INVOICE.id]: makeView(INVOICE), [WORK_INVOICE.id]: makeView(WORK_INVOICE) },
    ...options,
  });
  return { core, ...renderApp() };
}

async function openSearch(user: ReturnType<typeof renderApp>["user"]) {
  await screen.findByRole("listbox", { name: "Messages in Inbox" });
  await user.click(screen.getByRole("button", { name: "Search" }));
  return screen.findByRole("combobox", { name: "Search mail" });
}

const lastRequest = (core: ReturnType<typeof mockCore>, cmd = "search") => {
  const calls = core.callsOf(cmd);
  return calls[calls.length - 1]?.["request"] as SearchRequest | undefined;
};

function term(kind: SearchTermInfo["kind"], start: number, end: number, valueStart: number, extra: Partial<SearchTermInfo> = {}): SearchTermInfo {
  return { kind, negated: false, range: { start, end }, valueRange: { start: valueStart, end }, alternative: false, ...extra };
}

describe("search field", () => {
  it("opens with Cmd+F and with /, with the focus in the field, and explains the language", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.keyboard("{Meta>}f{/Meta}");
    const input = await screen.findByRole("combobox", { name: "Search mail" });
    // The field takes the focus in an effect after it renders: wait for it (slow under load).
    await waitFor(() => {
      expect(document.activeElement).toBe(input);
    });
    // Empty: the folder stays, and a hint names the filters.
    expect(screen.getByRole("listbox", { name: "Messages in Inbox" })).toBeTruthy();
    expect(input.getAttribute("aria-describedby")).toBeTruthy();
    expect(within(screen.getByRole("search")).getByText(/from:, to:, cc:, subject:/)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Close search" }));
    expect(screen.queryByRole("combobox", { name: "Search mail" })).toBeNull();
    await user.click(screen.getByRole("listbox", { name: "Messages in Inbox" }));
    await user.keyboard("/");
    const reopened = await screen.findByRole("combobox", { name: "Search mail" });
    await waitFor(() => {
      expect(document.activeElement).toBe(reopened);
    });
  });

  it("searches while typing (prefix), then for the submitted text, and shows hits with their found places", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    await user.type(input, "rechnung");

    const results = await screen.findByRole("listbox", { name: "Messages in the search results for “rechnung”" });
    expect(lastRequest(core)).toEqual({
      query: "rechnung",
      scope: { kind: "all" },
      typing: true,
      monthFirst: monthFirstFor(navigator.language),
      cursor: null,
      limit: 100,
    });
    // Both accounts' invoices, each in its account's colour; the lunch is not a hit.
    const options = await within(results).findAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options.map((o) => optionName(o))).toEqual([
      expect.stringContaining("Rechnung März"),
      expect.stringContaining("Rechnung Firma"),
    ]);
    expect(within(options[1] as HTMLElement).getByTestId("row-account").className).toContain(accountFill(WORK.color));
    const marks = within(options[0] as HTMLElement).getAllByTestId("highlight").map((m) => m.textContent);
    expect(marks).toEqual(["Rechnung", "Rechnung"]);
    // One list: no date headings between hits (a folder's list has them).
    expect(within(results).queryAllByTestId("list-section")).toEqual([]);

    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(lastRequest(core)?.typing).toBe(false);
    });
  });

  it("searches the folder or the account when asked", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    await user.selectOptions(screen.getByRole("combobox", { name: "Search in" }), "This folder");
    await user.type(input, "rechnung");
    await waitFor(() => {
      expect(lastRequest(core)?.scope).toEqual({ kind: "mailbox", mailboxId: 1 });
    });
    const results = await screen.findByRole("listbox", { name: "Messages in the search results for “rechnung”" });
    expect(within(results).getAllByRole("option")).toHaveLength(1);
    await user.selectOptions(screen.getByRole("combobox", { name: "Search in" }), "This account");
    await waitFor(() => {
      expect(lastRequest(core)?.scope).toEqual({ kind: "account", accountId: ACCOUNT.id });
    });
  });

  it("opens no hit by itself while the query is typed (stage 18, task F)", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    const before = core.callsOf("get_message").length;
    await user.type(input, "fir");
    await user.type(input, "ma");
    const results = await screen.findByRole("listbox", { name: "Messages in the search results for “firma”" });
    await within(results).findByRole("option");
    for (const option of within(results).getAllByRole("option")) expect(option.getAttribute("aria-selected")).toBe("false");
    expect(await screen.findByTestId("reading-empty")).toBeTruthy();
    expect(core.callsOf("get_message")).toHaveLength(before);
    // The focus stays in the field.
    expect(document.activeElement).toBe(input);
  });

  it("opens a hit in the reading pane with its own account", async () => {
    const { core, user } = setup();
    const input = await openSearch(user);
    await user.type(input, "firma");
    const results = await screen.findByRole("listbox", { name: "Messages in the search results for “firma”" });
    await user.click(await within(results).findByRole("option"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await showHeaderDetails(user, pane);
    expect(within(pane).getByTestId("reading-account").textContent).toBe("Work (work@example.com)");
    // Stage 18 (task F): the folder under the search showed its top message before; the click
    // opened the hit.
    expect(core.callsOf("get_message").at(-1)).toEqual({ messageId: WORK_INVOICE.id, format: "auto", scheme: "light" });
    // The actions of the hit's own account and folder (read-only here: shown, but disabled).
    const toolbar = await screen.findByRole("toolbar", { name: "Message actions" });
    expect(within(toolbar).getByRole("button", { name: /^Archive/ }).getAttribute("aria-disabled")).toBe("true");
  });

  it("Escape clears the field, a second Escape closes the search and the folder is back", async () => {
    const { user } = setup();
    const input = await openSearch(user);
    await user.type(input, "rechnung");
    await screen.findByRole("listbox", { name: "Messages in the search results for “rechnung”" });
    await user.keyboard("{Escape}");
    expect((input as HTMLInputElement).value).toBe("");
    expect(await screen.findByRole("listbox", { name: "Messages in Inbox" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("search")).toBeNull();
  });
});

describe("problems, chips and empty states", () => {
  it("explains a problem at its place and marks the field invalid", async () => {
    const query = "before:32.13.2026 rechnung";
    const { user } = setup({
      search: {
        search: () =>
          searchPage({
            problems: [{ code: "invalid_date", range: { start: 7, end: 17 } }],
            terms: [term("before", 0, 17, 7), term("text", 18, 26, 18)],
          }),
      },
    });
    const input = await openSearch(user);
    await user.type(input, query);

    const problems = await screen.findByRole("list", { name: "Problems in the search" });
    expect(problems.textContent).toBe(
      "“32.13.2026”: Not a date. Write it like 22.11.2026, 2026-11-22 or “22 Nov 2026”.",
    );
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(input.getAttribute("aria-describedby")).toBe(problems.id);
    expect(screen.getByTestId("search-problem-mark").textContent).toBe("32.13.2026");
  });

  it("shows filters as chips; removing a chip removes its term from the query", async () => {
    const { core, user } = setup({
      search: {
        search: (request) =>
          request.query === "from:anna -has:attachment rechnung"
            ? searchPage({
                terms: [
                  term("from", 0, 9, 5),
                  term("has_attachment", 10, 25, 25, { negated: true }),
                  term("text", 26, 34, 26),
                ],
              })
            : searchPage(),
      },
    });
    const input = await openSearch(user);
    await user.type(input, "from:anna -has:attachment rechnung");
    const chips = await screen.findByRole("list", { name: "Filters of this search" });
    expect(within(chips).getAllByTestId("search-chip").map((c) => c.firstChild?.textContent)).toEqual([
      "From: anna",
      "not Has attachment",
    ]);
    await user.click(within(chips).getByRole("button", { name: "Remove from:anna" }));
    expect((input as HTMLInputElement).value).toBe("-has:attachment rechnung");
    expect(document.activeElement).toBe(input);
    await waitFor(() => {
      expect(lastRequest(core)?.query).toBe("-has:attachment rechnung");
    });
  });

  it("says when nothing was found and offers the server; says when the query cannot match", async () => {
    const { user } = setup({
      search: {
        search: (request) =>
          searchPage({ matchesNothing: request.query.includes("is:read"), problems: [] }),
      },
    });
    const input = await openSearch(user);
    await user.type(input, "zebra");
    expect(await screen.findByText("Nothing found")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Search on the server" })).toBeTruthy();
    await user.clear(input);
    await user.type(input, "is:unread is:read");
    expect(await screen.findByText("This search cannot find anything")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Search on the server" })).toBeNull();
  });

  it("shows a failed search where it happened, with a retry", async () => {
    const { core, user } = setup({ failures: { search: ipcError("database") } });
    const input = await openSearch(user);
    await user.type(input, "rechnung");
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The search failed")).toBeTruthy();
    const before = core.callsOf("search").length;
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("search").length).toBeGreaterThan(before);
    });
  });

  it("says while the index is being built that older mail may be missing", async () => {
    const { user } = setup({ search: { index: { state: "building", percent: 40 } } });
    const input = await openSearch(user);
    await user.type(input, "rechnung");
    expect((await screen.findByTestId("search-index")).textContent).toBe(
      "The search index is being built (40 %). Older messages may be missing from the results.",
    );
  });
});

describe("search on the server", () => {
  it("sends the same search, not as typing, and shows its hits and what could not be searched", async () => {
    const remote: MessageListItem = makeMessage({ id: 6000, subject: "Altes Angebot", date: Date.now() - 86_400_000 * 400 });
    const { core, user } = setup({
      search: {
        search: () => searchPage(),
        serverSearch: () => ({
          hits: [makeHit(remote, [], { bodyLocal: false })],
          truncated: true,
          searched: [1, 20],
          failed: [20],
          notSynced: 2,
          problems: [],
        }),
      },
    });
    const input = await openSearch(user);
    await user.type(input, "angebot");
    await user.click(await screen.findByRole("button", { name: "Search on the server" }));

    const list = await screen.findByRole("listbox", { name: "Messages in the results from the server" });
    expect(within(list).getAllByRole("option")).toHaveLength(1);
    expect(lastRequest(core, "search_on_server")).toMatchObject({ query: "angebot", typing: false, cursor: null });
    const status = screen.getByTestId("search-server-status");
    expect(status.textContent).toContain("1 folder could not be searched");
    expect(status.textContent).toContain("2 messages found on the server are not on this computer yet.");
    expect(status.textContent).toContain("The server found more messages than are shown.");
    // Its body is not stored here: the reading pane says that its text was not searched.
    await user.click(within(list).getByRole("option"));
    expect((await screen.findByTestId("search-headers-only")).textContent).toBe(
      "Only the headers of this message are stored here: its text was not searched.",
    );

    await user.click(screen.getByRole("button", { name: "Back to the results on this computer" }));
    expect(await screen.findByText("Nothing found")).toBeTruthy();
  });

  it("shows a failed server search with a retry", async () => {
    const { user } = setup({ search: { search: () => searchPage() }, failures: { search_on_server: ipcError("connection_lost") } });
    const input = await openSearch(user);
    await user.type(input, "angebot");
    await user.click(await screen.findByRole("button", { name: "Search on the server" }));
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The search on the server failed")).toBeTruthy();
    expect(alert.dataset["errorCode"]).toBe("connection_lost");
  });
});

// Stage 22 (decision 0074): contacts on top while typing.
const MAX = makeContact({
  email: "max.mustermann@firma.example",
  name: "Max Mustermann",
  addresses: [
    { email: "max.mustermann@firma.example", sent: 3, received: 5, seen: 0, lastUsed: null, blocked: false },
    { email: "max@privat.example", sent: 0, received: 2, seen: 0, lastUsed: null, blocked: false },
  ],
});
const DIRECTORY = [
  MAX,
  makeContact({ email: "maxi@laden.example", name: "Maxi Laden" }),
  makeContact({ email: "max.planck@physik.example", name: "Max Planck" }),
  makeContact({ email: "maxine@example.org", name: "Maxine" }),
  makeContact({ email: "anna@example.org", name: "Anna Berg" }),
];

function contactsSetup(options: Parameters<typeof mockCore>[0] = {}) {
  return setup({
    contacts: { contacts: DIRECTORY },
    search: {
      search: (request) =>
        request.query.includes(":")
          ? searchPage({ terms: [term("contact", 0, request.query.length, request.query.indexOf(":") + 1)] })
          : searchPage({ hits: [makeHit(INVOICE)] }),
    },
    ...options,
  });
}

async function axeFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

describe("contacts on top of the search (stage 22)", () => {
  it("offers up to three contacts while typing, chosen with the keyboard, over all accounts", async () => {
    const { core, user } = contactsSetup();
    const input = await openSearch(user);
    await user.selectOptions(screen.getByRole("combobox", { name: "Search in" }), "This folder");
    await user.type(input, "max");

    const contacts = await screen.findByRole("listbox", { name: "Contacts: all mail with the person" });
    const options = within(contacts).getAllByRole("option");
    expect(options.map((o) => o.getAttribute("aria-label"))).toEqual([
      "Max Mustermann, max.mustermann@firma.example: show all mail with this person",
      "Maxi Laden, maxi@laden.example: show all mail with this person",
      "Max Planck, max.planck@physik.example: show all mail with this person",
    ]);
    // A merged contact names its other addresses.
    expect(options[0]?.textContent).toContain("max.mustermann@firma.example and 1 more address");
    expect(core.callsOf("list_contacts").at(-1)).toEqual({ query: "max", cursor: null, limit: 3, filter: "all" });
    // Combobox: the field controls the list; the full-text results stay below.
    expect(input.getAttribute("aria-expanded")).toBe("true");
    expect(input.getAttribute("aria-controls")).toBe(contacts.id);
    expect(await screen.findByRole("listbox", { name: "Messages in the search results for “max”" })).toBeTruthy();

    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowUp}");
    expect(input.getAttribute("aria-activedescendant")).toBe(options[0]?.id);
    expect(options[0]?.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{ArrowUp}");
    expect(input.getAttribute("aria-activedescendant")).toBe(options[2]?.id);
    await user.keyboard("{ArrowDown}{Enter}");

    expect((input as HTMLInputElement).value).toBe("contact:max.mustermann@firma.example");
    expect(screen.queryByRole("listbox", { name: "Contacts: all mail with the person" })).toBeNull();
    expect(input.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByRole<HTMLSelectElement>("combobox", { name: "Search in" }).value).toBe("all");
    await waitFor(() => {
      expect(lastRequest(core)).toMatchObject({
        query: "contact:max.mustermann@firma.example",
        scope: { kind: "all" },
        typing: false,
      });
    });
    expect(core.callsOf("add_recent_search").at(-1)).toEqual({ query: "contact:max.mustermann@firma.example" });
    const chips = await screen.findByRole("list", { name: "Filters of this search" });
    expect(within(chips).getByTestId("search-chip").firstChild?.textContent).toBe(
      "Contact: max.mustermann@firma.example",
    );
  });

  it("keeps the filters, closes with Escape first, and answers a click", async () => {
    const { core, user } = contactsSetup();
    const input = await openSearch(user);
    await user.type(input, "has:attachment must");
    const contacts = await screen.findByRole("listbox", { name: "Contacts: all mail with the person" });
    expect(core.callsOf("list_contacts").at(-1)).toMatchObject({ query: "must" });
    expect(within(contacts).getAllByRole("option")).toHaveLength(1);

    // Escape closes the list and keeps the text; typing opens it again.
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox", { name: "Contacts: all mail with the person" })).toBeNull();
    expect((input as HTMLInputElement).value).toBe("has:attachment must");
    await user.type(input, "e");
    const again = await screen.findByRole("listbox", { name: "Contacts: all mail with the person" });
    // Enter without a chosen contact searches the text.
    await user.keyboard("{Enter}");
    expect(screen.queryByRole("listbox", { name: "Contacts: all mail with the person" })).toBeNull();
    await waitFor(() => {
      expect(lastRequest(core)).toMatchObject({ query: "has:attachment muste", typing: false });
    });
    await user.type(input, "r");
    await user.click(within(await screen.findByRole("listbox", { name: "Contacts: all mail with the person" })).getAllByRole("option")[0] as HTMLElement);
    expect(again).toBeTruthy();
    expect((input as HTMLInputElement).value).toBe("has:attachment contact:max.mustermann@firma.example");
    expect(document.activeElement).toBe(input);
    // Filters only, a phrase, nothing that fits: no list.
    await user.clear(input);
    await user.type(input, "\"max");
    await user.type(input, " zzz");
    expect(screen.queryByRole("listbox", { name: "Contacts: all mail with the person" })).toBeNull();
    await user.clear(input);
    await user.type(input, "zzz");
    await waitFor(() => {
      expect(core.callsOf("list_contacts").at(-1)).toMatchObject({ query: "zzz" });
    });
    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Contacts: all mail with the person" })).toBeNull();
    });
    expect(input.getAttribute("aria-expanded")).toBe("false");
  });

  it("writes the German filter in German", async () => {
    const { user } = contactsSetup({ preferences: { theme: "light", language: "de" } });
    await screen.findByRole("listbox", { name: "Nachrichten in Posteingang" });
    await user.click(screen.getByRole("button", { name: "Suchen" }));
    const input = await screen.findByRole("combobox", { name: "Mails durchsuchen" });
    await user.type(input, "planck");
    const contacts = await screen.findByRole("listbox", { name: "Kontakte: alle Mails mit der Person" });
    expect(within(screen.getByTestId("search-contacts")).getByText("Kontakte")).toBeTruthy();
    await user.click(within(contacts).getAllByRole("option")[0] as HTMLElement);
    expect((input as HTMLInputElement).value).toBe("kontakt:max.planck@physik.example");
  });

  it.each(["light", "dark"] as const)("has nothing serious for axe with the list open (%s)", async (theme) => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    const { user } = contactsSetup({ preferences: { theme, language: "system" } });
    const input = await openSearch(user);
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    await user.type(input, "max");
    await screen.findByRole("listbox", { name: "Contacts: all mail with the person" });
    await user.keyboard("{ArrowDown}");
    expect(await axeFindings()).toEqual([]);
  });
});

