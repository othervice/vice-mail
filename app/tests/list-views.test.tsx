// Stage 19, task A (decisions 0061, 0062): sorting a view, the table presentation of the wide
// list and the quick filter, against the fake core; the keyboard and axe checks of all three.
import { screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ListTableSettings, MessageListItem, ThemePreference } from "../src/services/ipc";
import { DEFAULT_LIST_TABLE } from "../src/features/views/table";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: false });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

/** Six inbox messages: senders and subjects out of date order, sizes, flags, tags, an attachment. */
function messages(): MessageListItem[] {
  const senders = ["Zoë Ångström", "anna weber", "Bernd", "Älter Carl", "Dora", "Emil"];
  const subjects = ["Re: Zebra", "Angebot", "Mittag", "Bericht", "Urlaub", "Kaffee"];
  return makeMessages(6).map((item, i) => ({
    ...item,
    from: { name: senders[i] ?? "", email: `${String(i)}@example.org` },
    subject: subjects[i] ?? "",
    size: [5_000, 120_000, 800, 33_000, 2_048, 64_000][i] ?? 0,
    flagged: i === 1 || i === 4,
    hasAttachments: i === 3,
    keywords: i === 2 ? ["Projekt"] : i === 5 ? ["$Forwarded", "projekt"] : [],
    preview: i === 4 ? "Die Rechnung liegt bei" : null,
  }));
}

function setup(extra: Options = {}, items: MessageListItem[] = messages()) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    ...extra,
  });
  return { core, ...renderApp() };
}

async function list() {
  return screen.findByRole("listbox", { name: /^Messages in/ });
}

/**
 * The senders of the rows, in order (each option's name starts with its state and sender). The
 * list is asked for anew: a new order or filter may draw it anew.
 */
function senders(): string[] {
  const listbox = screen.queryByRole("listbox", { name: /^Messages in/ });
  if (!listbox) return [];
  return within(listbox)
    .queryAllByRole("option")
    .map((option) => optionName(option).replace(/^Unread, /, "").split(", ")[0] ?? "");
}

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

const table = (patch: Partial<ListTableSettings> = {}): ListTableSettings => ({ ...structuredClone(DEFAULT_LIST_TABLE), presentation: "table", ...patch });

describe("sorting a view", () => {
  it("the view menu sorts by sender A to Z, stores it for the view and turns the direction around", async () => {
    const { core, user } = setup();
    await list();
    expect(senders()[0]).toBe("Zoë Ångström");
    const menuButton = screen.getByRole("button", { name: "Sorted by Date, Newest first" });
    await user.click(menuButton);
    const menu = screen.getByRole("menu", { name: "Sort and view" });
    expect(within(menu).getByRole("menuitemradio", { name: "Date" }).getAttribute("aria-checked")).toBe("true");
    await user.click(within(menu).getByRole("menuitemradio", { name: "Sender" }));
    await waitFor(() => {
      expect(senders()).toEqual(["Älter Carl", "anna weber", "Bernd", "Dora", "Emil", "Zoë Ångström"]);
    });
    expect(core.listViews.orders).toEqual([{ view: "mailbox:1", order: { key: "sender", ascending: true } }]);
    expect(core.callsOf("list_view_messages").at(-1)).toMatchObject({
      request: { scope: { kind: "mailbox", mailboxId: 1 }, order: { key: "sender", ascending: true } },
    });
    // Sorted by another key, the list has no date sections; the button names the order.
    expect(screen.queryAllByTestId("list-section")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Sorted by Sender, A to Z" }));
    await user.click(screen.getByRole("menuitemradio", { name: "Z to A" }));
    await waitFor(() => {
      expect(senders()[0]).toBe("Zoë Ångström");
    });
    expect(core.listViews.orders[0]?.order).toEqual({ key: "sender", ascending: false });
    // Back to newest first: nothing is stored for the view.
    await user.click(screen.getByRole("button", { name: "Sorted by Sender, Z to A" }));
    await user.click(screen.getByRole("menuitemradio", { name: "Date" }));
    await waitFor(() => {
      expect(core.listViews.orders).toEqual([]);
    });
  });

  it("opens with the stored order and offers the recipient in Sent only", async () => {
    const { user } = setup({ listViews: { orders: [{ view: "mailbox:1", order: { key: "size", ascending: false } }] } });
    await list();
    await waitFor(() => {
      expect(senders()).toEqual(["anna weber", "Emil", "Älter Carl", "Zoë Ångström", "Dora", "Bernd"]);
    });
    await user.click(screen.getByRole("button", { name: "Sorted by Size, Largest first" }));
    expect(screen.queryByRole("menuitemradio", { name: "Recipient" })).toBeNull();
    // Keyboard: Escape closes the menu and gives the focus back to its button.
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Sorted by Size, Largest first" }));
  });

  it("'Unread first' waits while the view is sorted by another key", async () => {
    const { user } = setup({ listViews: { orders: [{ view: "mailbox:1", order: { key: "subject", ascending: true } }] } });
    await list();
    await user.click(screen.getByRole("button", { name: "List options" }));
    const item = screen.getByRole("menuitem", { name: "Show unread first (only with “Newest first”)" });
    expect(item.getAttribute("aria-disabled")).toBe("true");
  });
});

describe("the table", () => {
  it("draws the wide list as a table with sortable column headers, the list stays a listbox", async () => {
    const { core, user } = setup({ listViews: { table: table() } });
    const listbox = await list();
    const header = await screen.findByRole("table", { name: /^Columns of Messages in/ });
    const headers = within(header).getAllByRole("columnheader");
    expect(headers.map((h) => h.textContent)).toEqual(["Flagged", "Attachment", "Sender", "Subject", "Date", "Size"]);
    const date = within(header).getByRole("columnheader", { name: "Date" });
    expect(date.getAttribute("aria-sort")).toBe("descending");
    await waitFor(() => {
      expect(within(screen.getByRole("listbox", { name: /^Messages in/ })).getAllByTestId("table-row")).toHaveLength(6);
    });
    expect(within(listbox).getAllByRole("option")).toHaveLength(6);
    // A click on "Subject" sorts by it, A to Z; another click turns it around.
    await user.click(within(header).getByRole("button", { name: "Subject" }));
    await waitFor(() => {
      expect(screen.getByRole("columnheader", { name: "Subject" }).getAttribute("aria-sort")).toBe("ascending");
    });
    expect(screen.getByRole("columnheader", { name: "Date" }).getAttribute("aria-sort")).toBeNull();
    await waitFor(() => {
      expect(senders()[0]).toBe("anna weber");
    });
    await user.click(within(screen.getByRole("table")).getByRole("button", { name: "Subject" }));
    await waitFor(() => {
      expect(core.listViews.orders[0]?.order).toEqual({ key: "subject", ascending: false });
    });
    // Cells: the size in the UI language, marks as symbols.
    const first = within(screen.getByRole("listbox", { name: /^Messages in/ })).getAllByTestId("table-row")[0];
    expect(first && within(first).getByTestId("cell-size").textContent).not.toBe("");
  });

  it("columns are resized with the keyboard, chosen in the view menu and stored", async () => {
    const { core, user } = setup({ listViews: { table: table() } });
    await list();
    const handle = await screen.findByRole("separator", { name: "Width of the column Sender" });
    expect(handle.getAttribute("aria-valuenow")).toBe("200");
    handle.focus();
    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(handle.getAttribute("aria-valuenow")).toBe("232");
    await waitFor(() => {
      expect(core.listViews.table.columns.find((c) => c.id === "correspondent")?.width).toBe(232);
    });
    await user.click(screen.getByRole("button", { name: "Sorted by Date, Newest first" }));
    const size = screen.getByRole("menuitemcheckbox", { name: "Size" });
    expect(size.getAttribute("aria-checked")).toBe("true");
    await user.click(size);
    await waitFor(() => {
      expect(screen.queryByRole("columnheader", { name: "Size" })).toBeNull();
    });
    expect(core.listViews.table.columns.find((c) => c.id === "size")?.visible).toBe(false);
    // The subject always shows: it is not among the choices.
    expect(screen.queryByRole("menuitemcheckbox", { name: "Subject" })).toBeNull();
  });

  it("is chosen in the view menu and in Settings → Appearance", async () => {
    const { core, user } = setup();
    await list();
    expect(screen.queryByRole("table")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Sorted by Date, Newest first" }));
    await user.click(screen.getByRole("menuitemradio", { name: "Table" }));
    expect(await screen.findByRole("table", { name: /^Columns of/ })).toBeTruthy();
    await waitFor(() => {
      expect(core.listViews.table.presentation).toBe("table");
    });
  });
});

describe("the quick filter", () => {
  it("switches combine, the bar counts the matches and Reset clears everything", async () => {
    const { core, user } = setup();
    await list();
    const bar = screen.getByRole("region", { name: "Quick filter" });
    await user.click(within(bar).getByRole("button", { name: "Flagged" }));
    await waitFor(() => {
      expect(senders()).toEqual(["anna weber", "Dora"]);
    });
    expect(within(bar).getByRole("button", { name: "Flagged" }).getAttribute("aria-pressed")).toBe("true");
    expect(await within(bar).findByText("2 matches")).toBeTruthy();
    await user.click(within(bar).getByRole("button", { name: "Unread" }));
    // Nothing matches. The open message (the top one, opened by the list since stage 18 F) stays
    // in its place until the selection moves on, like stage 16's "Unread only".
    await waitFor(() => {
      expect(senders()).toEqual(["anna weber"]);
    });
    expect(await within(bar).findByText("0 matches")).toBeTruthy();
    expect(core.callsOf("count_list_view").at(-1)).toMatchObject({ request: { filter: { flagged: true, unread: true } } });
    // Without the open message, the list says that nothing matches.
    await user.keyboard("{Escape}");
    await user.click(within(bar).getByRole("button", { name: "Flagged" }));
    await user.click(within(bar).getByRole("button", { name: "Flagged" }));
    await waitFor(() => {
      expect(senders()).toEqual([]);
    });
    expect(await screen.findByText("No matching mail")).toBeTruthy();
    await user.click(within(bar).getByRole("button", { name: "Reset" }));
    await waitFor(() => {
      expect(senders()).toHaveLength(6);
    });
    expect(within(bar).queryByRole("button", { name: "Reset" })).toBeNull();
  });

  // Stage 21 (decision 0071): beside the reading pane (narrower than --ui-filter-compact-below)
  // the chips are their symbols, with the same names, so the bar stays one row.
  for (const width of [1000, 384]) {
    it(`shows the chips' names ${width < 800 ? "only to screen readers and as tooltips in a narrow bar" : "in a wide bar"}`, async () => {
      vi.stubGlobal(
        "ResizeObserver",
        class {
          constructor(private readonly callback: ResizeObserverCallback) {}
          observe() {
            this.callback([], this);
          }
          unobserve() {}
          disconnect() {}
        },
      );
      vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(width);
      const { user } = setup();
      await list();
      const bar = screen.getByRole("region", { name: "Quick filter" });
      const narrow = width < 800;
      expect(bar.hasAttribute("data-compact")).toBe(narrow);
      const flagged = within(bar).getByRole("button", { name: "Flagged" });
      expect(flagged.querySelector("span")?.classList.contains("sr-only")).toBe(narrow);
      expect(flagged.classList.contains("w-8")).toBe(narrow);
      const tag = within(bar).getByRole("button", { name: "Tag" });
      expect(tag.textContent).toBe(narrow ? "" : "Tag");
      expect(within(bar).getByRole("textbox").closest("label")?.classList.contains(narrow ? "min-w-(--ui-field-min-width)" : "min-w-(--ui-list-sender-width)")).toBe(true);
      // Hovered, a symbol-only chip names itself.
      await user.hover(flagged);
      if (narrow) expect((await screen.findByRole("tooltip")).textContent).toBe("Flagged");
      else expect(screen.queryByRole("tooltip")).toBeNull();
      vi.unstubAllGlobals();
    });
  }

  it("filters by text after a pause and by tag; Escape clears the text, then the filter", async () => {
    const { core, user } = setup();
    await list();
    const field = screen.getByRole("textbox", { name: "Filter by sender, subject and text" });
    await user.type(field, "rechn");
    await waitFor(() => {
      expect(senders()).toEqual(["Dora"]);
    });
    expect(core.callsOf("list_view_messages").at(-1)).toMatchObject({ request: { filter: { text: "rechn" } } });
    await user.keyboard("{Escape}");
    expect((field as HTMLInputElement).value).toBe("");
    await waitFor(() => {
      expect(senders()).toHaveLength(6);
    });
    const bar = screen.getByRole("region", { name: "Quick filter" });
    await user.click(within(bar).getByRole("button", { name: "Tag" }));
    const tags = await screen.findByRole("menu", { name: "Tag" });
    await waitFor(() => {
      expect(within(tags).getAllByRole("menuitemradio").map((item) => item.textContent)).toEqual(["No tag filter", "Projekt"]);
    });
    await user.click(within(tags).getByRole("menuitemradio", { name: "Projekt" }));
    await waitFor(() => {
      expect(senders()).toEqual(["Bernd", "Emil"]);
    });
    expect(within(bar).getByRole("button", { name: "Tag: Projekt" })).toBeTruthy();
    field.focus();
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(senders()).toHaveLength(6);
    });
  });

  it("is one toolbar for the keyboard and Cmd+Shift+K puts the focus into its text field", async () => {
    const { user } = setup();
    await list();
    const toolbar = screen.getByRole("toolbar", { name: "Show only" });
    const buttons = within(toolbar).getAllByRole("button");
    expect(buttons.filter((button) => button.tabIndex === 0)).toHaveLength(1);
    buttons[0]?.focus();
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toBe(within(toolbar).getByRole("button", { name: "Flagged" }));
    await user.keyboard("{End}");
    expect(document.activeElement).toBe(within(toolbar).getByRole("button", { name: "Tag" }));
    await user.keyboard("{Meta>}{Shift>}k{/Shift}{/Meta}");
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Filter by sender, subject and text" }));
  });

  it("filters the unified inbox over every account and reloads with the core's events", async () => {
    const items = messages();
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    await list();
    await user.click(within(screen.getByRole("region", { name: "Quick filter" })).getByRole("button", { name: "With attachment" }));
    await waitFor(() => {
      expect(senders()).toEqual(["Älter Carl"]);
    });
    const before = core.callsOf("list_view_messages").length;
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });
    await waitFor(() => {
      expect(core.callsOf("list_view_messages").length).toBeGreaterThan(before);
    });
  });
});

describe.each(["light", "dark"] as const)("axe for sorting, the table and the quick filter, %s theme", (theme: ThemePreference) => {
  it("finds nothing serious in the view menu, the table header, the rows and the filter bar", async () => {
    const { user } = setup({ preferences: { theme, language: "system" }, listViews: { table: table() } });
    await list();
    await screen.findByRole("table");
    await user.click(within(screen.getByRole("region", { name: "Quick filter" })).getByRole("button", { name: "Flagged" }));
    await screen.findByText("2 matches");
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Sorted by Date, Newest first" }));
    expect(await seriousFindings()).toEqual([]);
    await user.keyboard("{Escape}");
    await user.click(within(screen.getByRole("region", { name: "Quick filter" })).getByRole("button", { name: "Tag" }));
    await screen.findByRole("menu", { name: "Tag" });
    expect(await seriousFindings()).toEqual([]);
  });
});
