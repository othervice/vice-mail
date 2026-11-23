// Stage 16 (decision 0033), lists and folders: the reading list, "Unread only" and "Unread first",
// favourite folders and folder colours, moving and copying between accounts, blocking a sender.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageListItem, ThemePreference } from "../src/services/ipc";
import { useDragStore } from "../src/stores/drag";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { optionName } from "./support/names";
import { toolbarAction } from "./support/reading";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: false });
const BOB_ID = "33333333-3333-4333-8333-333333333333";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

function setup(items: MessageListItem[] = makeMessages(5), extra: Options = {}) {
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

async function list(name: string | RegExp = /^Messages in/) {
  return screen.findByRole("listbox", { name });
}

function row(listbox: HTMLElement, subject: string): HTMLElement {
  const option = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(`, ${subject},`));
  if (!option) throw new Error(`no row ${subject}`);
  return option;
}

function subjects(listbox: HTMLElement): string[] {
  return within(listbox)
    .queryAllByRole("option")
    .map((option) => optionName(option));
}

function notices(): string[] {
  return screen.queryAllByTestId("notice").map((n) => n.textContent);
}

async function openFolders(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Folders" }));
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

describe("the reading list", () => {
  it("Q adds the message under the cursor, locally, and the row shows the mark", async () => {
    const items = makeMessages(3);
    const { core, user } = setup(items);
    const listbox = await list();
    await user.click(row(listbox, "Message 2"));
    await user.keyboard("q");
    await waitFor(() => {
      expect(core.callsOf("set_reading_list")).toEqual([{ messageIds: [items[1]?.id], on: true }]);
    });
    expect(core.parity.readingList).toEqual([items[1]?.id]);
    // Nothing went to the server: no action, no keyword.
    expect(core.callsOf("apply_action")).toEqual([]);
    await waitFor(() => {
      expect(notices()).toContain("Added to the reading list");
    });
    await waitFor(() => {
      expect(optionName(row(listbox, "Message 2"))).toContain("On the reading list");
    });
    expect(within(row(listbox, "Message 2")).getByTestId("row-mark-readingList")).toBeTruthy();
  });

  it("has its own view in the rail with the marked mail, and an empty state", async () => {
    const items = makeMessages(3);
    const { core, user } = setup(items, { parity: { readingList: [items[2]?.id ?? 0] } });
    await user.click(await screen.findByRole("button", { name: /^Reading list/ }));
    const listbox = await list(/^Messages in Reading list/);
    await waitFor(() => {
      expect(subjects(listbox)).toHaveLength(1);
    });
    expect(subjects(listbox)[0]).toContain("Message 3");
    expect(core.callsOf("list_reading_list")[0]).toEqual({ scope: { kind: "account", accountId: ADA.id } });
    // Removing it from the toolbar empties the view.
    await user.click(row(listbox, "Message 3"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Remove from reading list"));
    await waitFor(() => {
      expect(core.parity.readingList).toEqual([]);
    });
    expect(await screen.findByText("Nothing on your reading list")).toBeTruthy();
  });

  it("Cmd+9 opens the reading list", async () => {
    const { user } = setup();
    await list();
    await user.keyboard("{Meta>}9{/Meta}");
    await waitFor(() => {
      expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "readingList" });
    });
  });
});

describe("unread only and unread first", () => {
  it("the header switch shows only unread mail of the folder", async () => {
    const items = makeMessages(6); // every third one is unread
    const { core, user } = setup(items);
    await list();
    const toggle = screen.getByRole("switch", { name: "Unread only" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await user.click(toggle);
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    await waitFor(() => {
      expect(core.callsOf("list_inbox_filtered").at(-1)).toMatchObject({
        scope: { kind: "mailbox", mailboxId: 1 },
        filter: { kind: "unread" },
      });
    });
    const listbox = await list();
    await waitFor(() => {
      expect(subjects(listbox)).toHaveLength(2);
    });
    expect(subjects(listbox).every((name) => name.includes("Message 1") || name.includes("Message 4"))).toBe(true);
    await user.click(toggle);
    await waitFor(() => {
      expect(subjects(listbox).length).toBeGreaterThan(2);
    });
  });

  it("keeps the open message in place once it is read, until the selection moves on", async () => {
    const items = makeMessages(6);
    const { core, user } = setup(items, { accounts: [makeAccount({ writesEnabled: true })] });
    await list();
    await user.click(screen.getByRole("switch", { name: "Unread only" }));
    const listbox = await list();
    await waitFor(() => {
      expect(subjects(listbox)).toHaveLength(2);
    });
    await user.click(row(listbox, "Message 4"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Mark as read"));
    await waitFor(() => {
      expect(core.callsOf("apply_action").length).toBeGreaterThan(0);
    });
    const filtered = core.callsOf("list_inbox_filtered").length;
    // The core reports the change: the list asks again and no longer has it, but it stays shown.
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });
    await waitFor(() => {
      expect(core.callsOf("list_inbox_filtered").length).toBeGreaterThan(filtered);
    });
    expect(screen.getByRole("region", { name: "Message" })).toBe(pane);
    expect(subjects(listbox).some((name) => name.includes("Message 4"))).toBe(true);
    // Moving on lets it go.
    await user.click(row(listbox, "Message 1"));
    await waitFor(() => {
      expect(subjects(listbox).some((name) => name.includes("Message 4"))).toBe(false);
    });
  });

  it("says so when nothing is unread", async () => {
    const items = makeMessages(3).map((item) => ({ ...item, unread: false }));
    const { user } = setup(items);
    await list();
    await user.click(screen.getByRole("switch", { name: "Unread only" }));
    expect(await screen.findByText("No unread mail here")).toBeTruthy();
  });

  it("'Unread first' from the header menu groups the list and appears in Settings → Inbox", async () => {
    const items = makeMessages(6);
    const { core, user } = setup(items);
    await list();
    await user.click(screen.getByRole("button", { name: "List options" }));
    await user.click(screen.getByRole("menuitem", { name: "Show unread first" }));
    await waitFor(() => {
      expect(core.parity.unreadFirstViews).toEqual(["mailbox:1"]);
    });
    await waitFor(() => {
      expect(core.callsOf("list_unread_first").length).toBeGreaterThan(0);
    });
    const listbox = await list();
    await waitFor(() => {
      expect(screen.getByTestId("unread-group-unread")).toBeTruthy();
    });
    expect(screen.getByTestId("unread-group-read")).toBeTruthy();
    const names = subjects(listbox);
    expect(names[0]).toContain("Message 1");
    expect(names[1]).toContain("Message 4");
    // The menu now offers the way back.
    await user.click(screen.getByRole("button", { name: "List options" }));
    expect(screen.getByRole("menuitem", { name: "Stop showing unread first" })).toBeTruthy();
    await user.keyboard("{Escape}");

    act(() => {
      useUiStore.getState().showSettings("inbox");
    });
    const views = await screen.findByTestId("unread-first-views");
    expect(views.textContent).toContain("Inbox (Ada Lovelace)");
    await user.click(within(views).getByRole("button", { name: "Turn off “Unread first” for Inbox (Ada Lovelace)" }));
    await waitFor(() => {
      expect(core.parity.unreadFirstViews).toEqual([]);
    });
    expect(await screen.findByTestId("unread-first-none")).toBeTruthy();
  });
});

describe("favourite folders and folder colours", () => {
  it("a folder's menu adds it to the favourites at the top of the sidebar and to the palette", async () => {
    const { core, user } = setup();
    await list();
    await openFolders(user);
    await user.click(await screen.findByRole("button", { name: "Options for Projekte" }));
    await user.click(screen.getByRole("menuitem", { name: "Add to favourites" }));
    await waitFor(() => {
      expect(core.callsOf("set_mailbox_favorite")).toEqual([{ mailboxId: 5, favorite: true }]);
    });
    const favorites = await screen.findByRole("region", { name: "Favourites" });
    expect(within(favorites).getByRole("treeitem", { name: /^Projekte/ })).toBeTruthy();

    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("favourite");
    expect(within(palette).getByText("Open favourite folder Projekte")).toBeTruthy();
  });

  it("favourites move with Alt+arrow keys and their menu", async () => {
    const mailboxes = standardMailboxes().map((mailbox) =>
      mailbox.id === 5 ? { ...mailbox, favoritePosition: 0 } : mailbox.id === 2 ? { ...mailbox, favoritePosition: 1 } : mailbox,
    );
    const { core, user } = setup(makeMessages(2), { mailboxes });
    await list();
    await openFolders(user);
    const favorites = await screen.findByRole("region", { name: "Favourites" });
    // The favourites under the section's label row (level 1).
    const names = () =>
      within(favorites)
        .getAllByRole("treeitem")
        .filter((row) => row.getAttribute("aria-level") === "2")
        .map((b) => b.getAttribute("aria-label"));
    expect(names()).toEqual(["Projekte, 2 unread", "Sent"]);
    const projekte = within(favorites).getByRole("treeitem", { name: "Projekte, 2 unread" });
    projekte.focus();
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    await waitFor(() => {
      expect(core.callsOf("reorder_favorite_mailboxes")).toEqual([{ mailboxIds: [2, 5] }]);
    });
    await waitFor(() => {
      expect(names()).toEqual(["Sent", "Projekte, 2 unread"]);
    });
    expect(within(favorites).getByText("Projekte moved to position 2 of 2")).toBeTruthy();
    // The moved favourite keeps the focus, in its new place.
    expect(document.activeElement).toBe(within(favorites).getByRole("treeitem", { name: "Projekte, 2 unread" }));
    await user.click(within(favorites).getByRole("button", { name: "Options for Projekte" }));
    await user.click(screen.getByRole("menuitem", { name: "Move up" }));
    await waitFor(() => {
      expect(core.callsOf("reorder_favorite_mailboxes").at(-1)).toEqual({ mailboxIds: [5, 2] });
    });
  });

  it("a colour from the account palette is shown next to the name and named with it", async () => {
    const { core, user } = setup();
    await list();
    await openFolders(user);
    await user.click(await screen.findByRole("button", { name: "Options for Projekte" }));
    await user.click(screen.getByRole("menuitem", { name: "Folder colour…" }));
    const picker = await screen.findByRole("dialog", { name: "Colour of Projekte" });
    const group = within(picker).getByRole("radiogroup", { name: "Colour of Projekte" });
    expect(within(group).getAllByRole("radio")).toHaveLength(9);
    await user.click(within(group).getByRole("radio", { name: "Green" }));
    await waitFor(() => {
      expect(core.callsOf("set_mailbox_color")).toEqual([{ mailboxId: 5, color: 5 }]);
    });
    await waitFor(() => {
      expect(screen.getByRole("treeitem", { name: "Projekte, colour Green, 2 unread" })).toBeTruthy();
    });
    await user.click(within(group).getByRole("radio", { name: "No colour" }));
    await waitFor(() => {
      expect(core.callsOf("set_mailbox_color").at(-1)).toEqual({ mailboxId: 5, color: null });
    });
  });
});

describe("moving and copying into another account", () => {
  const bob = makeAccount({ id: BOB_ID, email: "bob@vicemail.test", displayName: "Bob Builder", color: 3, writesEnabled: true });
  const bobFolders = [
    makeMailbox({ id: 21, accountId: BOB_ID, path: "INBOX", role: "inbox", roleSource: "server" }),
    makeMailbox({ id: 22, accountId: BOB_ID, path: "Archiv", role: "custom" }),
  ];

  function twoAccounts(adaWrites: boolean, bobWrites = true) {
    const items = makeMessages(3);
    const ada = makeAccount({ writesEnabled: adaWrites });
    return {
      items,
      ...setup(items, { accounts: [ada, { ...bob, writesEnabled: bobWrites }], mailboxes: [...standardMailboxes(), ...bobFolders] }),
    };
  }

  it("'Move to …' offers the other account's folders and asks before copying", async () => {
    const { core, user, items } = twoAccounts(true);
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    await user.keyboard("v");
    const picker = await screen.findByTestId("move-picker");
    await user.keyboard("Archiv");
    const option = await within(picker).findByRole("option", { name: /Archiv/ });
    expect(option.textContent).toContain("Bob Builder");
    await user.click(option);
    const dialog = await screen.findByRole("alertdialog", { name: "Move or copy this message to Archiv in Bob Builder?" });
    expect(within(dialog).getByText(/removed from Ada Lovelace only after the copy has arrived/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(core.parity.transfers).toHaveLength(1);
    });
    expect(core.parity.transfers[0]).toMatchObject({
      messages: [{ mailboxId: 1, messageId: items[0]?.id }],
      targetMailboxId: 22,
      mode: "copy",
    });
    await waitFor(() => {
      expect(notices().some((text) => text.includes("Copying to Archiv (Bob Builder)"))).toBe(true);
    });
  });

  it("the target account needs changes allowed, and the notice says where", async () => {
    const { core, user } = twoAccounts(true, false);
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    await user.keyboard("v");
    await screen.findByTestId("move-picker");
    await user.keyboard("Archiv");
    await user.click(await within(screen.getByTestId("move-picker")).findByRole("option", { name: /Archiv/ }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(notices().some((text) => text.includes("Changes are turned off"))).toBe(true);
    });
    expect(core.callsOf("transfer_messages")).toEqual([]);
    expect(screen.getByTestId("notice-allow-changes")).toBeTruthy();
  });

  it("dropping rows onto a folder of another account asks first", async () => {
    // A read-only source can still copy: only the move needs changes in both accounts.
    const { core, user, items } = twoAccounts(false);
    const listbox = await list();
    const source = await waitFor(() => row(listbox, "Message 2"));
    const dataTransfer = { types: ["application/x-mail-message-refs"], setData: vi.fn(), setDragImage: vi.fn(), effectAllowed: "", dropEffect: "" };
    await openFolders(user);
    fireEvent.dragStart(source, { dataTransfer });
    expect(useDragStore.getState().session).not.toBeNull();
    const target = await screen.findByTestId("mailbox-22");
    fireEvent.dragEnter(target, { dataTransfer });
    fireEvent.dragOver(target, { dataTransfer });
    fireEvent.drop(target, { dataTransfer });
    const dialog = await screen.findByRole("alertdialog", { name: /Archiv in Bob Builder/ });
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(core.parity.transfers[0]).toMatchObject({ targetMailboxId: 22, mode: "copy", messages: [{ messageId: items[1]?.id }] });
    });
  });
});

describe("blocking a sender", () => {
  it("blocks the sender from the toolbar menu, notes it on the message and can undo", async () => {
    const items = makeMessages(2);
    const { core, user } = setup(items);
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Block sender"));
    await waitFor(() => {
      expect(core.callsOf("block_sender")).toEqual([{ address: "sender1@example.org" }]);
    });
    await waitFor(() => {
      expect(notices().some((text) => text.includes("sender1@example.org is blocked"))).toBe(true);
    });
    const note = await within(pane).findByTestId("blocked-note");
    expect(note.textContent).toContain("You blocked sender1@example.org");
    await user.click(within(note).getByRole("button", { name: "Unblock" }));
    await waitFor(() => {
      expect(core.parity.blockedSenders).toEqual([]);
    });
    await waitFor(() => {
      expect(within(pane).queryByTestId("blocked-note")).toBeNull();
    });
  });

  it("undo in the notice unblocks again", async () => {
    const items = makeMessages(2);
    const { core, user } = setup(items);
    const listbox = await list();
    await user.click(row(listbox, "Message 2"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Block sender"));
    await waitFor(() => {
      expect(core.parity.blockedSenders.map((b) => b.address)).toEqual(["sender2@example.org"]);
    });
    await user.click(await screen.findByTestId("notice-undo"));
    await waitFor(() => {
      expect(core.callsOf("unblock_sender")).toEqual([{ address: "sender2@example.org" }]);
    });
  });

  it("explains a refusal of the core", async () => {
    const items = [makeMessage({ id: 77, subject: "Odd", from: { name: "Odd", email: "odd@example.org" } })];
    const { user } = setup(items, { failures: { block_sender: { code: "invalid_argument", message: "x", detail: null } } });
    const listbox = await list();
    await user.click(row(listbox, "Odd"));
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(await toolbarAction(user, within(pane).getByRole("toolbar", { name: "Message actions" }), "Block sender"));
    await waitFor(() => {
      expect(notices().some((text) => text.includes("The sender could not be blocked."))).toBe(true);
    });
  });
});

describe.each(["light", "dark"] as const)("axe for the stage 16 lists and folders, %s theme", (theme: ThemePreference) => {
  it("finds nothing serious in the reading list, the unread header, favourites, colours and the transfer question", async () => {
    const items = makeMessages(4).map((item) => ({ ...item, readingList: true }));
    const bob = makeAccount({ id: BOB_ID, email: "bob@vicemail.test", displayName: "Bob Builder", color: 3, writesEnabled: true });
    const mailboxes = [
      ...standardMailboxes().map((mailbox) => (mailbox.id === 5 ? { ...mailbox, favoritePosition: 0, color: 5 } : mailbox)),
      makeMailbox({ id: 22, accountId: BOB_ID, path: "Archiv" }),
    ];
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [makeAccount({ writesEnabled: true }), bob],
      mailboxes,
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
      parity: { readingList: items.map((item) => item.id), unreadFirstViews: ["mailbox:1"] },
    });
    const { user } = renderApp();
    await list();
    await waitFor(() => {
      expect(screen.getByTestId("unread-group-unread")).toBeTruthy();
    });
    await openFolders(user);
    await screen.findByRole("region", { name: "Favourites" });
    expect(await seriousFindings()).toEqual([]);

    // In the column the views are rows of its "Views" tree.
    await user.click(screen.getByRole("treeitem", { name: "Reading list" }));
    await list(/^Messages in Reading list/);
    expect(await seriousFindings()).toEqual([]);

    const favorites = screen.getByRole("region", { name: "Favourites" });
    await user.click(within(favorites).getByRole("button", { name: "Options for Projekte" }));
    await user.click(screen.getByRole("menuitem", { name: "Folder colour…" }));
    await screen.findByRole("dialog", { name: "Colour of Projekte" });
    expect(await seriousFindings()).toEqual([]);
    await user.keyboard("{Escape}");

    act(() => {
      useUiStore.getState().showMailView("inbox");
    });
    const inbox = await list();
    await user.click(row(inbox, "Message 1"));
    await user.keyboard("v");
    await screen.findByTestId("move-picker");
    await user.keyboard("Archiv");
    await user.click(await within(screen.getByTestId("move-picker")).findByRole("option", { name: /Archiv/ }));
    await screen.findByRole("alertdialog");
    expect(await seriousFindings()).toEqual([]);
  });
});
