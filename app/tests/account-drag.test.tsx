// Stage 23: accounts dragged into a new order as in Thunderbird, in the left column and in the
// narrow rail: a line shows where the account goes (before or after by the pointer's half), the
// dragged account is dimmed, letting go puts it there (the core keeps the order), Escape cancels,
// a drop elsewhere does nothing, and only a drop that changes the order is offered at all.
import { act, createEvent, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { accountPlaceAt } from "../src/features/accounts/accountDrag";
import { useDragStore } from "../src/stores/drag";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5 });
const ACCOUNTS = [ADA, WORK, CLUB];

/** Each account of a list 100 px high, one below the other from 0 (jsdom lays nothing out). */
function layOut() {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const id = this.getAttribute("data-account-drop-item");
    const index = id === null ? -1 : ACCOUNTS.findIndex((account) => account.id === id);
    const items = id === null ? [] : [...(this.parentElement?.querySelectorAll("[data-account-drop-item]") ?? [])];
    const place = index < 0 ? 0 : items.indexOf(this);
    const top = place * 100;
    return { top, bottom: top + 100, left: 0, right: 200, width: 200, height: 100, x: 0, y: top, toJSON: () => ({}) };
  });
}

function setup() {
  layOut();
  const core = mockCore({
    accounts: ACCOUNTS,
    mailboxes: [
      ...standardMailboxes(),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
    ],
  });
  return { core, ...renderApp() };
}

afterEach(() => {
  act(() => {
    useDragStore.getState().endAccount();
  });
});

/** A DataTransfer stand-in (jsdom has none). */
function dataTransfer() {
  const data = new Map<string, string>();
  return {
    types: [] as string[],
    effectAllowed: "",
    dropEffect: "none",
    setData(type: string, value: string) {
      data.set(type, value);
      this.types.push(type);
    },
    getData: (type: string) => data.get(type) ?? "",
    setDragImage: vi.fn(),
  };
}

type Transfer = ReturnType<typeof dataTransfer>;

/** A drag event at a height; answers whether the page offered a drop there (default prevented). */
function dragAt(kind: "dragOver" | "drop", target: Element, transfer: Transfer, clientY: number): boolean {
  const event = createEvent[kind](target, { dataTransfer: transfer });
  Object.defineProperty(event, "clientY", { value: clientY });
  fireEvent(target, event);
  return event.defaultPrevented;
}

async function openColumn(user: ReturnType<typeof renderApp>["user"]) {
  if (!screen.queryByTestId("sidebar-column")) await user.click(await screen.findByRole("button", { name: "Folders" }));
  return screen.findByTestId("sidebar-accounts");
}

const columnOrder = () => screen.getAllByTestId(/^sidebar-account-name-/).map((name) => name.textContent);
const lines = (zone: HTMLElement) =>
  [...zone.querySelectorAll<HTMLElement>("[data-account-drop]")].map((item) => `${item.dataset["accountDrop"] ?? ""} ${item.getAttribute("data-account-drop-item") ?? ""}`);

describe("where a dragged account goes", () => {
  it("before the first account whose middle is below the pointer, else after the last; nothing where the order stays", () => {
    const boxes = ACCOUNTS.map((account, i) => ({ id: account.id, top: i * 100, bottom: i * 100 + 100 }));
    // Club over Ada's upper half: before Ada.
    expect(accountPlaceAt(20, boxes, ACCOUNTS, CLUB.id)).toEqual({ place: { target: ADA.id, side: "before" }, order: [CLUB.id, ADA.id, WORK.id] });
    // Over Ada's lower half: before Work.
    expect(accountPlaceAt(70, boxes, ACCOUNTS, CLUB.id)?.place).toEqual({ target: WORK.id, side: "before" });
    // Ada below Club's middle: after the last.
    expect(accountPlaceAt(260, boxes, ACCOUNTS, ADA.id)).toEqual({ place: { target: CLUB.id, side: "after" }, order: [WORK.id, CLUB.id, ADA.id] });
    // Over itself or right next to itself: the order stays, no line.
    expect(accountPlaceAt(20, boxes, ACCOUNTS, ADA.id)).toBeNull();
    expect(accountPlaceAt(70, boxes, ACCOUNTS, ADA.id)).toBeNull();
    expect(accountPlaceAt(260, boxes, ACCOUNTS, CLUB.id)).toBeNull();
    expect(accountPlaceAt(10, [], ACCOUNTS, CLUB.id)).toBeNull();
  });
});

describe("dragging accounts in the left column", () => {
  it("shows the line, dims the dragged account, and puts it where it is let go", async () => {
    const { core, user } = setup();
    const zone = await openColumn(user);
    await waitFor(() => {
      expect(columnOrder()).toEqual(["Ada Lovelace", "Work", "Club"]);
    });
    const transfer = dataTransfer();
    fireEvent.dragStart(screen.getByTestId(`sidebar-account-${ADA.id}`), { dataTransfer: transfer });
    expect(transfer.effectAllowed).toBe("move");
    const ada = screen.getByTestId(`sidebar-account-section-${ADA.id}`);
    expect(ada.dataset["accountDragged"]).toBe("true");
    expect(ada.className).toContain("opacity-50");

    // Over itself: no line, no drop offered.
    expect(dragAt("dragOver", ada, transfer, 30)).toBe(false);
    expect(lines(zone)).toEqual([]);
    // Over Work's lower half: before Club, offered as a move.
    const work = screen.getByTestId(`sidebar-account-section-${WORK.id}`);
    expect(dragAt("dragOver", work, transfer, 170)).toBe(true);
    expect(transfer.dropEffect).toBe("move");
    expect(lines(zone)).toEqual([`before ${CLUB.id}`]);
    // Below Club's middle: after Club, the last.
    const club = screen.getByTestId(`sidebar-account-section-${CLUB.id}`);
    expect(dragAt("dragOver", club, transfer, 280)).toBe(true);
    expect(lines(zone)).toEqual([`after ${CLUB.id}`]);
    // Outside every list of accounts the line goes.
    fireEvent.dragOver(screen.getByTestId("sidebar-views"), { dataTransfer: transfer });
    expect(lines(zone)).toEqual([]);

    // Let go after Club: Ada is last, also after a restart (the core keeps the order).
    expect(dragAt("drop", club, transfer, 280)).toBe(true);
    await waitFor(() => {
      expect(core.callsOf("reorder_accounts")).toEqual([{ accountIds: [WORK.id, CLUB.id, ADA.id] }]);
    });
    await waitFor(() => {
      expect(columnOrder()).toEqual(["Work", "Club", "Ada Lovelace"]);
    });
    fireEvent.dragEnd(screen.getByTestId(`sidebar-account-${ADA.id}`), { dataTransfer: transfer });
    expect(lines(zone)).toEqual([]);
    expect(screen.getByTestId(`sidebar-account-section-${ADA.id}`).dataset["accountDragged"]).toBeUndefined();
    expect(within(zone).getByText("Ada Lovelace is now at place 3 of 3.")).toBeTruthy();
  });

  it("Escape cancels; a drop elsewhere or after the drag ended does nothing", async () => {
    const { core, user } = setup();
    const zone = await openColumn(user);
    const transfer = dataTransfer();
    fireEvent.dragStart(screen.getByTestId(`sidebar-account-${CLUB.id}`), { dataTransfer: transfer });
    const ada = screen.getByTestId(`sidebar-account-section-${ADA.id}`);
    expect(dragAt("dragOver", ada, transfer, 10)).toBe(true);
    expect(lines(zone)).toEqual([`before ${ADA.id}`]);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(lines(zone)).toEqual([]);
    expect(screen.getByTestId(`sidebar-account-section-${CLUB.id}`).dataset["accountDragged"]).toBeUndefined();
    expect(dragAt("dragOver", ada, transfer, 10)).toBe(false);
    expect(dragAt("drop", ada, transfer, 10)).toBe(false);

    // A new drag, let go over the message list: nothing moves.
    fireEvent.dragStart(screen.getByTestId(`sidebar-account-${CLUB.id}`), { dataTransfer: transfer });
    expect(dragAt("dragOver", ada, transfer, 10)).toBe(true);
    const elsewhere = screen.getByTestId("sidebar-views");
    fireEvent.dragOver(elsewhere, { dataTransfer: transfer });
    expect(dragAt("drop", elsewhere, transfer, 10)).toBe(false);
    fireEvent.dragEnd(screen.getByTestId(`sidebar-account-${CLUB.id}`), { dataTransfer: transfer });
    expect(core.callsOf("reorder_accounts")).toEqual([]);
    expect(columnOrder()).toEqual(["Ada Lovelace", "Work", "Club"]);
  });

  it("files from outside are never taken for an account", async () => {
    const { user } = setup();
    const zone = await openColumn(user);
    const transfer = dataTransfer();
    fireEvent.dragStart(screen.getByTestId(`sidebar-account-${CLUB.id}`), { dataTransfer: transfer });
    const files = { ...dataTransfer(), types: ["Files"] };
    expect(dragAt("dragOver", screen.getByTestId(`sidebar-account-section-${ADA.id}`), files, 10)).toBe(false);
    expect(lines(zone)).toEqual([]);
  });
});

describe("dragging accounts in the narrow rail", () => {
  async function openRail(user: ReturnType<typeof renderApp>["user"]) {
    if (screen.queryByTestId("sidebar-column")) await user.click(screen.getByTestId("rail-folders"));
    return screen.findByTestId("rail-accounts");
  }

  it("a dot dragged up goes where the line shows", async () => {
    const { core, user } = setup();
    const rail = await openRail(user);
    const dot = await within(rail).findByTestId(`account-switch-${CLUB.id}`);
    expect(dot.getAttribute("draggable")).toBe("true");
    expect(dot.getAttribute("aria-description")).toBe("Drag to put the account elsewhere, or press Alt+Up or Alt+Down.");
    const transfer = dataTransfer();
    fireEvent.dragStart(dot, { dataTransfer: transfer });
    expect(transfer.types).toEqual(["application/x-mail-account"]);
    expect(dot.className).toContain("opacity-50");
    expect(dragAt("dragOver", rail, transfer, 40)).toBe(true);
    expect(transfer.dropEffect).toBe("move");
    expect(lines(rail)).toEqual([`before ${ADA.id}`]);
    expect(dragAt("drop", rail, transfer, 40)).toBe(true);
    fireEvent.dragEnd(dot, { dataTransfer: transfer });
    await waitFor(() => {
      expect(core.callsOf("reorder_accounts")).toEqual([{ accountIds: [CLUB.id, ADA.id, WORK.id] }]);
    });
    await waitFor(() => {
      expect(within(rail).getAllByTestId(/^account-switch-/).map((button) => button.dataset["testid"])).toEqual([
        `account-switch-${CLUB.id}`,
        `account-switch-${ADA.id}`,
        `account-switch-${WORK.id}`,
      ]);
    });
    expect(screen.getByTestId("rail-accounts-announce").textContent).toBe("Club is now at place 1 of 3.");
    expect(lines(rail)).toEqual([]);
  });

  it("Alt+Down and Alt+Up move the focused dot; the focus stays on it", async () => {
    const { core, user } = setup();
    const rail = await openRail(user);
    (await within(rail).findByTestId(`account-switch-${ADA.id}`)).focus();
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    await waitFor(() => {
      expect(core.callsOf("reorder_accounts")).toEqual([{ accountIds: [WORK.id, ADA.id, CLUB.id] }]);
    });
    await waitFor(() => {
      expect(document.activeElement).toBe(within(rail).getByTestId(`account-switch-${ADA.id}`));
    });
    expect(screen.getByTestId("rail-accounts-announce").textContent).toBe("Ada Lovelace is now at place 2 of 3.");
    // Up from the top does nothing.
    within(rail).getByTestId(`account-switch-${WORK.id}`).focus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(core.callsOf("reorder_accounts")).toHaveLength(1);
  });
});
