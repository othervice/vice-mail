// Stage 22 (decision 0078): the user's order of the accounts (left column by drag, the account's
// menu, Alt+arrows, Settings → Accounts) and their names in the lists (renamed in place, in the
// account's settings), the same in every list of accounts.
import { createEvent, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { droppedOrder, movedOrder } from "../src/features/accounts/arrange";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "", color: 3 });
const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5 });

function setup() {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ADA, WORK, CLUB],
    mailboxes: [
      ...standardMailboxes(),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
    ],
  });
  return { core, ...renderApp() };
}

type User = ReturnType<typeof renderApp>["user"];

async function openColumn(user: User) {
  if (screen.queryByTestId("sidebar-column")) return;
  await user.click(await screen.findByRole("button", { name: "Folders" }));
  await screen.findByTestId("sidebar-column");
}

/** The accounts of the left column, top to bottom, by their headings' text. */
function columnOrder(): string[] {
  return screen.getAllByTestId(/^sidebar-account-name-/).map((name) => name.textContent);
}

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
}

/** A DataTransfer stand-in (jsdom has none). */
function dataTransfer() {
  const data = new Map<string, string>();
  return {
    types: [] as string[],
    effectAllowed: "",
    dropEffect: "",
    setData(type: string, value: string) {
      data.set(type, value);
      this.types.push(type);
    },
    getData: (type: string) => data.get(type) ?? "",
    setDragImage: vi.fn(),
  };
}

/** A drag event at a height (jsdom puts every box at 0 with no height). */
function at(event: Event, clientY: number): Event {
  Object.defineProperty(event, "clientY", { value: clientY });
  return event;
}

describe("the order of the accounts", () => {
  it("moves an account with its menu and Alt+arrows; every list follows, and the place is said", async () => {
    const { core, user } = setup();
    await openColumn(user);
    // An account without a list name shows its address.
    await waitFor(() => {
      expect(columnOrder()).toEqual(["Ada Lovelace", "work@example.com", "Club"]);
    });
    await user.click(screen.getByRole("button", { name: "More for Ada Lovelace" }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByRole("menuitem", { name: "Move up" }).getAttribute("aria-disabled")).toBe("true");
    await user.click(within(menu).getByRole("menuitem", { name: "Move down" }));
    await waitFor(() => {
      expect(core.callsOf("reorder_accounts")).toEqual([{ accountIds: [WORK.id, ADA.id, CLUB.id] }]);
    });
    expect(columnOrder()).toEqual(["work@example.com", "Ada Lovelace", "Club"]);
    await waitFor(() => {
      expect(screen.getByTestId(`account-announce-${ADA.id}`).textContent).toBe("Ada Lovelace is now at place 2 of 3.");
    });

    // Alt+Down on the heading; the focus stays on it.
    const club = screen.getByTestId(`sidebar-account-${CLUB.id}`);
    club.focus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    await waitFor(() => {
      expect(columnOrder()).toEqual(["work@example.com", "Club", "Ada Lovelace"]);
    });
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    });
    expect(core.callsOf("reorder_accounts").at(-1)).toEqual({ accountIds: [WORK.id, CLUB.id, ADA.id] });

    // The same order in the settings and in the narrow rail's account switcher.
    useUiStore.getState().showSettings();
    useUiStore.getState().showAccountsPage({ kind: "list" });
    const list = await screen.findByRole("list", { name: "Accounts" });
    expect(within(list).getAllByTestId(/^account-row-/).map((row) => row.dataset["testid"])).toEqual([
      `account-row-${WORK.id}`,
      `account-row-${CLUB.id}`,
      `account-row-${ADA.id}`,
    ]);
    expect(await seriousFindings()).toEqual([]);
  });

  it("drags an account heading before or after another account", async () => {
    const { core, user } = setup();
    await openColumn(user);
    const transfer = dataTransfer();
    const heading = await screen.findByTestId(`sidebar-account-${CLUB.id}`);
    expect(heading.getAttribute("draggable")).toBe("true");
    fireEvent.dragStart(heading, { dataTransfer: transfer });
    expect(transfer.types).toEqual(["application/x-mail-account"]);
    // Folders never take an account.
    const folder = screen.getByTestId("mailbox-5");
    fireEvent.dragOver(folder, { dataTransfer: transfer });
    expect(folder.dataset["dropTarget"]).toBeUndefined();
    // Over Ada's upper half: before her, shown as a line.
    const ada = screen.getByTestId(`sidebar-account-section-${ADA.id}`);
    fireEvent(ada, at(createEvent.dragOver(ada, { dataTransfer: transfer }), -5));
    expect(ada.dataset["accountDrop"]).toBe("before");
    fireEvent(ada, at(createEvent.drop(ada, { dataTransfer: transfer }), -5));
    await waitFor(() => {
      expect(core.callsOf("reorder_accounts")).toEqual([{ accountIds: [CLUB.id, ADA.id, WORK.id] }]);
    });
    await waitFor(() => {
      expect(columnOrder()).toEqual(["Club", "Ada Lovelace", "work@example.com"]);
    });
    expect(ada.dataset["accountDrop"]).toBeUndefined();

    // The pure orders behind it.
    const accounts = [ADA, WORK, CLUB];
    expect(droppedOrder(accounts, CLUB.id, ADA.id, false)).toEqual([CLUB.id, ADA.id, WORK.id]);
    expect(droppedOrder(accounts, ADA.id, WORK.id, true)).toEqual([WORK.id, ADA.id, CLUB.id]);
    expect(droppedOrder(accounts, ADA.id, WORK.id, false)).toBeNull();
    expect(droppedOrder(accounts, ADA.id, ADA.id, true)).toBeNull();
    expect(movedOrder(accounts, ADA.id, -1)).toBeNull();
    expect(movedOrder(accounts, CLUB.id, 1)).toBeNull();
    expect(movedOrder(accounts, WORK.id, 1)).toEqual([ADA.id, CLUB.id, WORK.id]);
  });

  it("moves accounts in Settings → Accounts with buttons that keep the focus", async () => {
    const { core, user } = setup();
    useUiStore.getState().showSettings();
    useUiStore.getState().showAccountsPage({ kind: "list" });
    const up = await screen.findByRole("button", { name: "Move Ada Lovelace up" });
    expect((up as HTMLButtonElement).disabled).toBe(true);
    await user.click(screen.getByRole("button", { name: "Move work@example.com up" }));
    await waitFor(() => {
      expect(core.callsOf("reorder_accounts")).toEqual([{ accountIds: [WORK.id, ADA.id, CLUB.id] }]);
    });
    await waitFor(() => {
      expect(screen.getByTestId("accounts-moved").textContent).toBe("work@example.com is now at place 1 of 3.");
    });
    // At the top now: its "up" is off, the focus went to its "down".
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "Move work@example.com down" }));
    });
    expect(await seriousFindings()).toEqual([]);
  });
});

describe("the names of the accounts", () => {
  it("renames in place with a double click: Enter saves, Escape cancels, empty shows the address", async () => {
    const { core, user } = setup();
    await openColumn(user);
    const heading = await screen.findByTestId(`sidebar-account-${CLUB.id}`);
    await user.dblClick(within(heading).getByTestId(`sidebar-account-name-${CLUB.id}`));
    const field = await screen.findByRole("textbox", { name: "Name of club@example.net in the lists" });
    expect(document.activeElement).toBe(field);
    expect((field as HTMLInputElement).value).toBe("Club");
    expect(field.getAttribute("maxlength")).toBe("100");
    expect(await seriousFindings()).toEqual([]);
    await user.clear(field);
    await user.type(field, "Verein{Enter}");
    await waitFor(() => {
      expect(core.callsOf("rename_account")).toEqual([{ accountId: CLUB.id, name: "Verein" }]);
    });
    await waitFor(() => {
      expect(columnOrder()).toEqual(["Ada Lovelace", "work@example.com", "Verein"]);
    });
    expect(document.activeElement).toBe(screen.getByTestId(`sidebar-account-${CLUB.id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`account-announce-${CLUB.id}`).textContent).toBe("club@example.net is now listed as Verein.");
    });

    // F2 starts it too; Escape changes nothing.
    await user.keyboard("{F2}");
    const again = await screen.findByRole("textbox", { name: "Name of club@example.net in the lists" });
    await user.type(again, "XYZ{Escape}");
    expect(screen.queryByRole("textbox", { name: "Name of club@example.net in the lists" })).toBeNull();
    expect(core.callsOf("rename_account")).toHaveLength(1);

    // "Rename" in the menu; an empty name shows the address.
    await user.click(screen.getByRole("button", { name: "More for Verein" }));
    await user.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Rename" }));
    const third = await screen.findByRole("textbox", { name: "Name of club@example.net in the lists" });
    await user.clear(third);
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(columnOrder()).toEqual(["Ada Lovelace", "work@example.com", "club@example.net"]);
    });
    expect(core.callsOf("rename_account").at(-1)).toEqual({ accountId: CLUB.id, name: "" });
  });

  it("names the account in its settings; the sender name stays with the addresses", async () => {
    const { core, user } = setup();
    useUiStore.getState().showSettings();
    useUiStore.getState().showAccountsPage({ kind: "account", accountId: WORK.id });
    expect(await screen.findByRole("heading", { level: 3, name: "Name in the list" })).toBeTruthy();
    const field = screen.getByRole("textbox", { name: "Name" });
    expect(field.getAttribute("placeholder")).toBe("work@example.com");
    expect(screen.getByText(/People you write to never see it/)).toBeTruthy();
    await user.type(field, "Arbeit");
    await user.click(screen.getByRole("button", { name: "Save name" }));
    await waitFor(() => {
      expect(core.callsOf("rename_account")).toEqual([{ accountId: WORK.id, name: "Arbeit" }]);
    });
    expect(await screen.findByText("Saved.")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 2, name: "Arbeit" })).toBeTruthy();
    expect(core.callsOf("save_identity")).toEqual([]);
    expect(await seriousFindings()).toEqual([]);
  });
});
