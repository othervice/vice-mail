// Stage 22 (decision 0081): mail dragged from any list onto every folder of the left column, also
// rows of several accounts at once; dropping moves, Option or Ctrl copies, read-only accounts
// refuse visibly and say why, closed folders open while the rows rest on them.
import { act, createEvent, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { autoScrollSpeed } from "../src/features/mailboxes/useDragAutoScroll";
import { EXPAND_DELAY_MS } from "../src/features/mailboxes/useDropTarget";
import type { Account, Mailbox, MessageListItem } from "../src/services/ipc";
import { dropModeOf, dropVerdict, type DragSession } from "../src/stores/drag";
import { makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3, writesEnabled: true });
const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5, writesEnabled: false });
const now = Date.now();
const ADA_MAIL = makeMessage({ id: 3000, accountId: ADA.id, mailboxId: 1, subject: "Privat", date: now - 60_000 });
const WORK_MAIL = makeMessage({ id: 2000, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: now - 120_000 });
const CLUB_MAIL = makeMessage({ id: 1000, accountId: CLUB.id, mailboxId: 30, subject: "Training", date: now - 180_000 });

const MAILBOXES: Mailbox[] = [
  ...standardMailboxes(),
  makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
  makeMailbox({ id: 21, accountId: WORK.id, path: "Archive", role: "archive", roleSource: "server" }),
  makeMailbox({ id: 22, accountId: WORK.id, path: "Kunden" }),
  makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
];

function setup(accounts: Account[] = [ADA, WORK, CLUB]) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts,
    mailboxes: MAILBOXES,
    messages: { 1: [ADA_MAIL], 20: [WORK_MAIL], 30: [CLUB_MAIL] },
    views: Object.fromEntries([ADA_MAIL, WORK_MAIL, CLUB_MAIL].map((m) => [m.id, makeView(m)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, ...renderApp() };
}

/** A DataTransfer stand-in (jsdom has none): keeps what the list puts in. */
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

/** A drag event with a modifier held (jsdom has no DragEvent, so it is set by hand). */
function withKeys(event: Event, keys: { altKey?: boolean; ctrlKey?: boolean }): Event {
  for (const [key, value] of Object.entries(keys)) Object.defineProperty(event, key, { value });
  return event;
}

async function allInboxes(user: ReturnType<typeof renderApp>["user"], rows = 3) {
  await screen.findByRole("listbox", { name: "Messages in Inbox" });
  await user.click(await screen.findByRole("button", { name: /^Show the inbox of/ }));
  const choices = await screen.findByRole("list", { name: "Show the inbox of" });
  await user.click(within(choices).getByRole("button", { name: "All accounts" }));
  const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
  await waitFor(() => {
    expect(within(list).getAllByRole("option")).toHaveLength(rows);
  });
  return list;
}

function row(list: HTMLElement, subject: string): HTMLElement {
  const option = within(list)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(`, ${subject},`));
  if (!option) throw new Error(`no row ${subject}`);
  return option;
}

async function openColumn(user: ReturnType<typeof renderApp>["user"]) {
  if (screen.queryByTestId("sidebar-column")) return;
  await user.click(await screen.findByRole("button", { name: "Folders" }));
  await screen.findByTestId("sidebar-column");
}

const notices = () => screen.queryAllByTestId("notice").map((n) => n.textContent);

describe("dragging mail from any list", () => {
  it("drops rows of two accounts from All inboxes on another account's folder: one move, no question, one notice", async () => {
    const { core, user } = setup();
    const list = await allInboxes(user);
    await user.click(row(list, "Privat"));
    await user.keyboard("{Shift>}{ArrowDown}{/Shift}");
    await openColumn(user);
    const transfer = dataTransfer();
    fireEvent.dragStart(row(list, "Projekt"), { dataTransfer: transfer });
    // Dropping moves; a copy is allowed too. The drag image names the count.
    expect(transfer.effectAllowed).toBe("copyMove");
    expect(transfer.setDragImage).toHaveBeenCalled();
    // Folders of every account take the rows; the read-only one only says why when they are over it.
    const target = await screen.findByTestId("mailbox-22");
    expect(target.dataset["dropTarget"]).toBe("ready");
    expect(screen.getByTestId("mailbox-5").dataset["dropTarget"]).toBe("ready");
    expect(screen.getByTestId("mailbox-30").dataset["dropTarget"]).toBeUndefined();

    fireEvent.dragEnter(target, { dataTransfer: transfer });
    fireEvent.dragOver(target, { dataTransfer: transfer });
    expect(target.dataset["dropTarget"]).toBe("over");
    expect(transfer.dropEffect).toBe("move");
    expect(screen.getByTestId("drop-hint").textContent).toBe("Move 2 messages to Kunden");
    expect(screen.getByTestId("drop-hint-status").textContent).toBe("Move 2 messages to Kunden");
    fireEvent.drop(target, { dataTransfer: transfer });

    await waitFor(() => {
      expect(core.callsOf("drop_messages")).toHaveLength(1);
    });
    expect(core.callsOf("drop_messages")[0]).toMatchObject({
      targetMailboxId: 22,
      mode: "move",
      messages: [
        { mailboxId: 1, messageId: ADA_MAIL.id },
        { mailboxId: 20, messageId: WORK_MAIL.id },
      ],
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByTestId("drop-hint")).toBeNull();
    // Work's row moved within its account; Ada's goes the transfer's way (copy first).
    await waitFor(() => {
      expect(notices().some((text) => text.startsWith("Moving 2 messages to Kunden in Work"))).toBe(true);
    });
    expect(notices().find((text) => text.startsWith("Moving"))).toContain("Mail from another account leaves its folder once its copy has arrived.");
    expect(core.parity.transfers).toEqual([expect.objectContaining({ targetMailboxId: 22, mode: "move", messages: [{ mailboxId: 1, messageId: ADA_MAIL.id }] })]);

    // Undo takes back the move within the account; the copy already on the server stays.
    await user.click(screen.getByTestId("notice-undo"));
    await waitFor(() => {
      expect(notices()).toContain("Undone, except mail already copied into another account.");
    });
    expect(core.callsOf("undo_action")).toHaveLength(2);
  });

  it("a read-only account refuses as target, visibly and announced; Ctrl copies on Windows", async () => {
    const { core, user } = setup();
    const list = await allInboxes(user);
    await openColumn(user);
    const transfer = dataTransfer();
    fireEvent.dragStart(row(list, "Privat"), { dataTransfer: transfer });
    const club = screen.getByTestId("mailbox-30");
    fireEvent.dragEnter(club, { dataTransfer: transfer });
    fireEvent.dragOver(club, { dataTransfer: transfer });
    expect(club.dataset["dropTarget"]).toBe("refused");
    expect(club.className).toContain("outline-danger");
    expect(transfer.dropEffect).toBe("none");
    expect(screen.getByTestId("drop-hint").textContent).toBe("Not here: changes are turned off for Club.");
    expect(screen.getByTestId("drop-hint-status").textContent).toBe("Not here: changes are turned off for Club.");
    // Seen by screen readers and axe while it shows.
    const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
    fireEvent.drop(club, { dataTransfer: transfer });
    fireEvent.dragLeave(club, { dataTransfer: transfer });
    expect(screen.queryByTestId("drop-hint")).toBeNull();
    expect(core.callsOf("drop_messages")).toEqual([]);

    // Moving out of a read-only account is refused; the hint offers the copy.
    fireEvent.dragStart(row(list, "Training"), { dataTransfer: transfer });
    const kunden = screen.getByTestId("mailbox-22");
    fireEvent.dragOver(kunden, { dataTransfer: transfer });
    expect(kunden.dataset["dropTarget"]).toBe("refused");
    expect(screen.getByTestId("drop-hint").textContent).toBe("Not as a move: changes are turned off for Club. Hold Option to copy instead.");
    fireEvent(kunden, withKeys(createEvent.dragOver(kunden, { dataTransfer: transfer }), { altKey: true }));
    expect(kunden.dataset["dropTarget"]).toBe("over");
    expect(transfer.dropEffect).toBe("copy");
    expect(screen.getByTestId("drop-hint").textContent).toBe("Copy to Kunden");
    fireEvent(kunden, withKeys(createEvent.drop(kunden, { dataTransfer: transfer }), { altKey: true }));
    await waitFor(() => {
      expect(core.callsOf("drop_messages")).toEqual([
        expect.objectContaining({ targetMailboxId: 22, mode: "copy", messages: [{ mailboxId: 30, messageId: CLUB_MAIL.id }] }),
      ]);
    });
    await waitFor(() => {
      expect(notices().some((text) => text.startsWith("Copying to Kunden in Work"))).toBe(true);
    });

    // The modifier per system: Option on macOS, Ctrl on Windows.
    expect(dropModeOf({ altKey: true, ctrlKey: false }, "macos")).toBe("copy");
    expect(dropModeOf({ altKey: false, ctrlKey: true }, "macos")).toBe("move");
    expect(dropModeOf({ altKey: false, ctrlKey: true }, "windows")).toBe("copy");
    expect(dropModeOf({ altKey: true, ctrlKey: false }, "windows")).toBe("move");
  });

  it("opens a closed folder while the rows rest on it, and the column scrolls at its edges", async () => {
    const { user } = setup();
    const list = await allInboxes(user);
    await openColumn(user);
    await user.click(screen.getByTestId("folder-toggle-5"));
    expect(screen.queryByTestId("mailbox-6")).toBeNull();
    const transfer = dataTransfer();
    fireEvent.dragStart(row(list, "Privat"), { dataTransfer: transfer });
    const projekte = screen.getByTestId("mailbox-5");
    // `dragover` keeps coming while the pointer rests there.
    const started = Date.now();
    while (Date.now() - started < EXPAND_DELAY_MS + 300) {
      fireEvent.dragOver(projekte, { dataTransfer: transfer });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 100));
      });
    }
    expect(await screen.findByTestId("mailbox-6")).toBeTruthy();
    fireEvent.dragEnd(row(list, "Privat"), { dataTransfer: transfer });

    // Near the top edge up, near the bottom down, faster closer to the edge; nothing between.
    expect(autoScrollSpeed(102, 100, 600)).toBeLessThan(autoScrollSpeed(130, 100, 600));
    expect(autoScrollSpeed(130, 100, 600)).toBeLessThan(0);
    expect(autoScrollSpeed(350, 100, 600)).toBe(0);
    expect(autoScrollSpeed(598, 100, 600)).toBeGreaterThan(autoScrollSpeed(570, 100, 600));
    expect(autoScrollSpeed(570, 100, 600)).toBeGreaterThan(0);
  }, 15_000);

  it("decides each drop by the rows' accounts: target, sources, rows already there, the rail's roles", () => {
    const rows: MessageListItem[] = [ADA_MAIL, WORK_MAIL];
    const session: DragSession = { items: rows, count: 2, drop: vi.fn() };
    const context = {
      accounts: [ADA, WORK, CLUB],
      mailboxesOf: (accountId: string) => MAILBOXES.filter((m) => m.accountId === accountId),
    };
    const folder = (id: number) => ({ kind: "folder" as const, mailbox: MAILBOXES.find((m) => m.id === id) as Mailbox });
    expect(dropVerdict(session, folder(22), "move", context)).toEqual({ kind: "accept", mode: "move", count: 2 });
    // The one row already in the inbox stays; the other one counts.
    expect(dropVerdict(session, folder(20), "copy", context)).toEqual({ kind: "accept", mode: "copy", count: 1 });
    // Every row there already: not a target at all.
    expect(dropVerdict({ ...session, items: [WORK_MAIL] }, folder(20), "move", context)).toEqual({ kind: "none" });
    expect(dropVerdict(session, folder(30), "move", context)).toMatchObject({ kind: "refuse", reason: "targetReadOnly", account: { id: CLUB.id } });
    const fromClub: DragSession = { ...session, items: [CLUB_MAIL] };
    expect(dropVerdict(fromClub, folder(22), "move", context)).toMatchObject({ kind: "refuse", reason: "sourceReadOnly", account: { id: CLUB.id } });
    expect(dropVerdict(fromClub, folder(22), "copy", context)).toMatchObject({ kind: "accept" });
    // The rail: each row's own archive; Ada has none.
    expect(dropVerdict({ ...session, items: [WORK_MAIL] }, { kind: "role", role: "archive" }, "move", context)).toEqual({ kind: "accept", mode: "move", count: 1 });
    expect(dropVerdict(session, { kind: "role", role: "archive" }, "move", context)).toMatchObject({ kind: "refuse", reason: "noRoleFolder", role: "archive" });
    expect(dropVerdict(null, folder(22), "move", context)).toEqual({ kind: "none" });
  });

  it("the narrow rail's Trash takes each row into its own account's trash", async () => {
    const { core, user } = setup([ADA, WORK]);
    const list = await allInboxes(user, 2);
    const transfer = dataTransfer();
    fireEvent.dragStart(row(list, "Privat"), { dataTransfer: transfer });
    const trash = screen.getByTestId("rail-trash");
    fireEvent.dragOver(trash, { dataTransfer: transfer });
    expect(trash.dataset["dropTarget"]).toBe("over");
    fireEvent.drop(trash, { dataTransfer: transfer });
    await waitFor(() => {
      expect(core.callsOf("drop_messages")).toEqual([expect.objectContaining({ targetMailboxId: 4, mode: "move", messages: [{ mailboxId: 1, messageId: ADA_MAIL.id }] })]);
    });
    // Work has no trash folder here: its row is refused with the reason.
    fireEvent.dragStart(row(list, "Projekt"), { dataTransfer: transfer });
    fireEvent.dragOver(trash, { dataTransfer: transfer });
    expect(trash.dataset["dropTarget"]).toBe("refused");
    expect(screen.getByTestId("drop-hint").textContent).toBe("Not here: Work has no Trash folder.");
  });
});
