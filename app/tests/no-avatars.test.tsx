import { screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { accountFill } from "../src/features/accounts/accountColor";
import { DEFAULT_LIST_TABLE } from "../src/features/views/table";
import type { Account, MessageListItem } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";

/*
 * Stage 22, item 3 (decision 0072): no pictures and no initials anywhere. Message rows start with
 * a narrow mark column (unread dot, the selection box on hover), in every presentation; lists that
 * mix accounts show the account as a dot in its colour before the sender; the narrow rail and the
 * account settings show accounts as dots in their colour with their name.
 */

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** The list measures its width through a ResizeObserver: report one at once. */
function listWidth(width: number) {
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
}

function inbox(items: MessageListItem[], extra: Parameters<typeof mockCore>[0] = {}) {
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

const classes = (element: Element | null | undefined) => (element?.getAttribute("class") ?? "").split(/\s+/);

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

/** The first thing a row draws (inside the option's drawn, aria-hidden part). */
function firstDrawn(option: HTMLElement): Element | null {
  const drawn = option.querySelector('[aria-hidden="true"].rounded-row');
  let element = drawn?.firstElementChild ?? null;
  // Descend through layout wrappers to the first leaf box.
  while (element && !element.hasAttribute("data-row-check") && element.firstElementChild && element.tagName === "SPAN") {
    element = element.firstElementChild;
  }
  return element;
}

describe("message rows without avatars", () => {
  it("start with the mark column: the unread dot, the box in its place on hover; read rows leave it empty", async () => {
    const items = makeMessages(3).map((item, i) => ({ ...item, unread: i === 0, from: { name: `Sender ${String(i)}`, email: `s${String(i)}@example.org` } }));
    inbox(items);
    const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
    const options = await within(listbox).findAllByRole("option");
    expect(options).toHaveLength(3);
    for (const option of options) {
      expect(within(option).queryByTestId("avatar")).toBeNull();
      expect(option.querySelector("img")).toBeNull();
      const mark = within(option).getByTestId("row-mark");
      // The row's first column, as wide as the selection box, after the row's lead.
      expect(firstDrawn(option)).toBe(mark);
      expect(classes(mark)).toEqual(expect.arrayContaining(["size-(--ui-row-mark)", "shrink-0", "relative"]));
      expect(classes(mark.closest(".rounded-row"))).toContain("ps-(--ui-row-lead)");
      // The box shows on hover (or in a multiple selection) where the dot is.
      expect(classes(within(option).getByTestId("row-check"))).toEqual(expect.arrayContaining(["hidden", "group-hover/row:flex"]));
      // A single account's list has no account dot.
      expect(within(option).queryByTestId("row-account-dot")).toBeNull();
    }
    const [unread, read] = options;
    const dot = within(unread as HTMLElement).getByTestId("row-unread-dot");
    expect(within(unread as HTMLElement).getByTestId("row-mark").contains(dot)).toBe(true);
    expect(classes(dot)).toContain("group-hover/row:hidden");
    // Unread is also bolder, so it never rests on the dot alone (stage 23, decision 0083: the
    // draft's weights, bold and semibold).
    expect(classes(within(unread as HTMLElement).getByTestId("row-sender"))).toContain("font-bold");
    expect(within(read as HTMLElement).queryByTestId("row-unread-dot")).toBeNull();
    expect(classes(within(read as HTMLElement).getByTestId("row-sender"))).toContain("font-semibold");
    expect(await seriousFindings()).toEqual([]);
  });

  for (const density of ["comfortable", "compact"] as const) {
    it(`puts the mark on the first line of narrow rows (${density}: ${density === "compact" ? "two" : "three"} lines)`, async () => {
      listWidth(400);
      const items = makeMessages(2).map((item) => ({ ...item, unread: true, preview: "The start of the text" }));
      inbox(items, { ui: { layout: { readingPane: "right", density, sidebarExpanded: false, sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true } } });
      const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
      await waitFor(() => {
        expect(document.documentElement.dataset["density"] ?? "comfortable").toBe(density);
        const first = within(listbox).getAllByRole("option")[0] as HTMLElement;
        // Compact: the preview follows the subject on its line; comfortable: a line of its own.
        const preview = within(first).getByTestId("row-preview");
        expect(preview.parentElement === within(first).getByTestId("row-subject").parentElement).toBe(density === "compact");
      });
      const option = within(listbox).getAllByRole("option")[0] as HTMLElement;
      const mark = within(option).getByTestId("row-mark");
      // The mark sits in a box as high as the first line (sender and time), beside the lines.
      expect(classes(mark.parentElement)).toEqual(expect.arrayContaining(["h-5", "items-center"]));
      expect(classes(mark.closest(".rounded-row"))).toEqual(expect.arrayContaining(["grid", "ps-(--ui-row-lead)"]));
      expect(within(option).queryByTestId("avatar")).toBeNull();
    });
  }

  it("table rows have the same mark column on the same lead, and the header keeps its place", async () => {
    inbox(makeMessages(2), { listViews: { table: { ...structuredClone(DEFAULT_LIST_TABLE), presentation: "table" } } });
    const header = await screen.findByRole("table", { name: /^Columns of Messages in/ });
    const listbox = screen.getByRole("listbox", { name: /^Messages in/ });
    await waitFor(() => {
      expect(within(listbox).getAllByTestId("table-row")).toHaveLength(2);
    });
    const row = within(listbox).getAllByTestId("table-row")[0] as HTMLElement;
    expect(classes(row)).toContain("ps-(--ui-row-lead)");
    expect(classes(within(row).getByTestId("row-mark"))).toContain("size-(--ui-row-mark)");
    const headerRow = within(header).getAllByRole("row")[0] as HTMLElement;
    expect(classes(headerRow)).toContain("ps-(--ui-row-lead)");
    expect(classes(headerRow.firstElementChild)).toContain("w-(--ui-row-mark)");
  });

  it("show the account with its colour's dot in lists that mix accounts, also while the quick actions show", async () => {
    const ada = makeMessages(1)[0] as MessageListItem;
    const work: MessageListItem = { ...ada, id: 900, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: ada.date - 60_000 };
    mockCore({
      accounts: [ADA, WORK],
      mailboxes: [...standardMailboxes(), makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" })],
      messages: { 1: [ada], 20: [work] },
      views: { [ada.id]: makeView(ada), [work.id]: makeView(work) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await user.click(await screen.findByRole("button", { name: /^Show the inbox of/ }));
    const choices = await screen.findByRole("list", { name: "Show the inbox of" });
    await user.click(within(choices).getByRole("button", { name: "All accounts" }));
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(2);
    });
    for (const option of within(list).getAllByRole("option")) {
      const account: Account = optionName(option).includes("Projekt") ? WORK : ADA;
      const name = account.displayName || account.email;
      const label = within(option).getByTestId("row-account");
      // Tooltip and the row's accessible name say the account in words (SPEC: never colour alone).
      expect(label.title).toBe(name);
      expect(optionName(option)).toContain(`Account: ${name}`);
      // Stage 23 (decision 0084): the stripe in the account's colour.
      expect(classes(label)).toContain(accountFill(account.color));
      // Not in the meta that gives way to the quick actions: it stays, before them.
      expect(within(option).getByTestId("row-meta").contains(label)).toBe(false);
      expect(within(option).getByTestId("row-meta-marks").contains(label)).toBe(false);
      expect(within(option).getByTestId("row-actions")).toBeTruthy();
      // Away from the unread dot: after the sender and the subject.
      expect(within(option).getByTestId("row-subject").compareDocumentPosition(label) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(within(option).queryByTestId("avatar")).toBeNull();
    }
    expect(await seriousFindings()).toEqual([]);
  });
});

describe("accounts without initials", () => {
  it("the narrow rail shows each account as a dot in its colour, named by its tooltip and accessible name", async () => {
    mockCore({ accounts: [ADA, WORK], mailboxes: standardMailboxes(), messages: { 1: makeMessages(1) } });
    const { user } = renderApp();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    for (const account of [ADA, WORK]) {
      const entry = screen.getByTestId(`account-switch-${account.id}`);
      // No initials: at most the account's unread count next to the dot (stage 23, decision 0085).
      expect(entry.textContent).toMatch(/^\d*$/);
      expect(within(entry).queryByTestId("avatar")).toBeNull();
      const dot = within(entry).getByTestId("account-dot");
      expect(dot.getAttribute("aria-hidden")).toBe("true");
      expect(classes(dot)).toEqual(expect.arrayContaining(["rounded-full", "size-(--ui-rail-account-dot)", accountFill(account.color)]));
      expect(entry.getAttribute("aria-label")).toContain(account.displayName || account.email);
    }
    // The tooltip says the name too.
    await user.hover(screen.getByTestId(`account-switch-${WORK.id}`));
    expect((await screen.findByRole("tooltip")).textContent).toContain("Work");
    expect(await seriousFindings()).toEqual([]);
  });

  it("Settings → Accounts lists accounts with their colour's dot and name, no initials", async () => {
    mockCore({ accounts: [ADA, WORK], mailboxes: standardMailboxes() });
    renderApp();
    useUiStore.getState().showSettings("accounts");
    const row = await screen.findByTestId(`account-row-${WORK.id}`);
    expect(within(row).queryByTestId("avatar")).toBeNull();
    expect(row.textContent).toContain("Work");
    expect(classes(within(row).getByTestId("account-dot"))).toContain(accountFill(WORK.color));
  });
});
