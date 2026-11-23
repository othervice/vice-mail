import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { buildMailboxTree, type MailboxNode } from "../src/features/mailboxes/tree";
import { makeAccount, makeMailbox, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ACCOUNT = makeAccount();
const WORK = makeAccount({
  id: "33333333-3333-4333-8333-333333333333",
  email: "work@example.com",
  displayName: "Work",
  color: 3,
});

function shape(nodes: MailboxNode[]): unknown[] {
  return nodes.map((n) => (n.children.length ? [n.mailbox.path, shape(n.children)] : n.mailbox.path));
}

async function openFolders(user: ReturnType<typeof renderApp>["user"]) {
  const toggle = await screen.findByRole("button", { name: "Folders" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  await user.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
}

describe("folder tree", () => {
  it("puts roles first in a fixed order, then the own folders as a tree, sorted by name", () => {
    const tree = buildMailboxTree([
      makeMailbox({ id: 1, path: "Zettel" }),
      makeMailbox({ id: 2, path: "Trash", role: "trash" }),
      makeMailbox({ id: 3, path: "INBOX", role: "inbox" }),
      makeMailbox({ id: 4, path: "Archiv", role: "archive" }),
      makeMailbox({ id: 5, path: "Projekte" }),
      makeMailbox({ id: 6, path: "Projekte/2026", parentId: 5 }),
      makeMailbox({ id: 7, path: "Projekte/10", parentId: 5 }),
      makeMailbox({ id: 8, path: "Projekte/9", parentId: 5 }),
      makeMailbox({ id: 9, path: "Gesendet", role: "sent" }),
      makeMailbox({ id: 10, path: "INBOX/Rechnungen", parentId: 3 }),
    ]);
    expect(shape(tree)).toEqual([
      ["INBOX", ["INBOX/Rechnungen"]],
      "Gesendet",
      "Archiv",
      "Trash",
      ["Projekte", ["Projekte/9", "Projekte/10", "Projekte/2026"]],
      "Zettel",
    ]);
  });

  it("lifts Gmail's role folders out of the [Gmail] placeholder and hides the empty placeholder", () => {
    const tree = buildMailboxTree([
      makeMailbox({ id: 1, path: "INBOX", role: "inbox" }),
      makeMailbox({ id: 2, path: "[Gmail]", selectable: false }),
      makeMailbox({ id: 3, path: "[Gmail]/Sent Mail", parentId: 2, role: "sent" }),
      makeMailbox({ id: 4, path: "[Gmail]/All Mail", parentId: 2, role: "all" }),
      makeMailbox({ id: 5, path: "[Gmail]/Starred", parentId: 2, role: "flagged" }),
      makeMailbox({ id: 6, path: "Labels", selectable: false }),
      makeMailbox({ id: 7, path: "Labels/Reisen", parentId: 6 }),
    ]);
    expect(shape(tree)).toEqual([
      "INBOX",
      "[Gmail]/Sent Mail",
      "[Gmail]/Starred",
      "[Gmail]/All Mail",
      ["Labels", ["Labels/Reisen"]],
    ]);
  });

  it("survives a folder that is its own ancestor", () => {
    const tree = buildMailboxTree([
      makeMailbox({ id: 1, path: "A", parentId: 2 }),
      makeMailbox({ id: 2, path: "B", parentId: 1 }),
      makeMailbox({ id: 3, path: "C" }),
    ]);
    expect(shape(tree)).toEqual(["C"]);
  });

  it("'Folders' unfolds the sidebar with every account's folders and unread counters", async () => {
    mockCore({
      accounts: [ACCOUNT, WORK],
      mailboxes: [
        ...standardMailboxes(),
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server", unread: 7 }),
      ],
    });
    const { user } = renderApp();
    await openFolders(user);

    const ada = await screen.findByRole("tree", { name: "Folders of Ada Lovelace" });
    const names = within(ada)
      .getAllByTestId(/^mailbox-/)
      .map((button) => button.querySelector(".truncate")?.textContent);
    expect(names).toEqual(["Inbox", "Sent", "Drafts", "Trash", "Projekte", "2026"]);
    // The counter is read as "3 unread", not as a bare number.
    expect(within(ada).getByRole("treeitem", { name: "Inbox, 3 unread" })).toBeTruthy();
    expect(within(ada).getByRole("treeitem", { name: "Projekte, 2 unread" })).toBeTruthy();
    expect(within(ada).getByRole("treeitem", { name: "Sent" })).toBeTruthy();
    const work = await screen.findByRole("tree", { name: "Folders of Work" });
    expect(within(work).getByRole("treeitem", { name: "Inbox, 7 unread" })).toBeTruthy();
    // The account headers carry the name, never just the colour.
    expect(screen.getByRole("region", { name: "Work (work@example.com), up to date" })).toBeTruthy();
  });

  it("subfolders fold away and back", async () => {
    mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    await openFolders(user);
    await screen.findByText("2026");

    // The arrow before the name is for the mouse (the keyboard uses Right and Left), named by its tooltip.
    const toggle = screen.getByTestId("folder-toggle-5");
    expect(toggle.getAttribute("title")).toBe("Hide subfolders of Projekte");
    await user.click(toggle);
    expect(screen.queryByText("2026")).toBeNull();
    expect(screen.getByTestId("mailbox-5").getAttribute("aria-expanded")).toBe("false");
    expect(toggle.getAttribute("title")).toBe("Show subfolders of Projekte");
    await user.click(toggle);
    expect(screen.getByText("2026")).toBeTruthy();
    expect(screen.getByTestId("mailbox-5").getAttribute("aria-expanded")).toBe("true");
  });

  it("opens an own folder with its messages; role folders open the rail view", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: { 6: makeMessages(3, { mailboxId: 6 }) },
    });
    const { user } = renderApp();
    await openFolders(user);

    await user.click(await screen.findByTestId("mailbox-6"));
    expect(screen.getByRole("heading", { level: 1, name: "2026" })).toBeTruthy();
    expect(await screen.findByRole("listbox", { name: "Messages in 2026" })).toBeTruthy();
    expect(screen.getByTestId("mailbox-6").getAttribute("aria-current")).toBe("page");

    await user.click(screen.getByTestId("mailbox-2"));
    expect(screen.getByRole("heading", { level: 1, name: "Sent" })).toBeTruthy();
    expect(screen.getByTestId("rail-sent").getAttribute("aria-current")).toBe("page");
    expect(screen.getByTestId("mailbox-2").getAttribute("aria-current")).toBe("page");
  });

  it("counters follow mailboxesChanged", async () => {
    const core = mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    await openFolders(user);
    expect((await screen.findByTestId("mailbox-1")).getAttribute("aria-label")).toBe("Inbox, 3 unread");

    core.setMailboxes(standardMailboxes().map((m) => (m.id === 1 ? { ...m, unread: 12 } : m)));
    await emitCore({ type: "mailboxesChanged", accountId: ACCOUNT.id });

    await waitFor(() => {
      expect(screen.getByTestId("mailbox-1").getAttribute("aria-label")).toBe("Inbox, 12 unread");
    });
  });

  it("names a folder with its visible counter, in the same number format (label in name)", async () => {
    mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes().map((m) => (m.id === 1 ? { ...m, unread: 1296 } : m)) });
    const { user } = renderApp();
    await openFolders(user);
    const item = await screen.findByTestId("mailbox-1");
    expect(item.getAttribute("aria-label")).toBe("Inbox, 1,296 unread");
    // Name and counter are two words in the row's text, both part of its name.
    expect(item.textContent).toBe("Inbox 1,296");
    const words = (item.textContent ?? "").split(" ");
    for (const word of words) expect(item.getAttribute("aria-label")).toContain(word);
  });

  it("without an account the sidebar shows no folders", async () => {
    mockCore();
    const { user } = renderApp();
    await openFolders(user);
    expect(screen.queryByRole("tree", { name: /Folders of/ })).toBeNull();
    expect(screen.getByRole("navigation").textContent).not.toMatch(/\d/);
  });
});

describe("account switcher", () => {
  it("lists every account by name and state; the inbox follows the chosen account", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    mockCore({
      accounts: [ACCOUNT, WORK],
      mailboxes: [
        ...standardMailboxes(),
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      ],
      messages: { 1: makeMessages(2), 20: makeMessages(1, { mailboxId: 20 }) },
    });
    const { user } = renderApp();

    const switcher = await screen.findByRole("list", { name: "Accounts" });
    const buttons = await within(switcher).findAllByRole("button");
    expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
      "Ada Lovelace (ada@vicemail.test), up to date",
      "Work (work@example.com), up to date",
    ]);
    expect(buttons[0]?.getAttribute("aria-pressed")).toBe("true");
    expect(await screen.findByRole("listbox", { name: "Messages in Inbox" })).toBeTruthy();
    expect(screen.getAllByRole("option")).toHaveLength(2);

    await user.click(buttons[1] as HTMLElement);

    expect(buttons[1]?.getAttribute("aria-pressed")).toBe("true");
    expect(await within(screen.getByRole("banner")).findByText("Work (work@example.com)")).toBeTruthy();
    await waitFor(() => {
      expect(screen.getAllByRole("option")).toHaveLength(1);
    });
  });
});

describe("special folders in the account settings", () => {
  async function openAccount(user: ReturnType<typeof renderApp>["user"]) {
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${ACCOUNT.id}`));
    return screen.findByTestId("special-folders");
  }

  it("shows each role with its folder and where the assignment came from", async () => {
    mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    const folders = await openAccount(user);

    const sent = within(folders).getByLabelText<HTMLSelectElement>("Sent");
    expect(sent.selectedOptions[0]?.textContent).toBe("Sent");
    expect(sent.getAttribute("aria-describedby")).toBeTruthy();
    expect(folders.textContent).toContain("Marked by the server");
    expect(folders.textContent).toContain("Guessed from the folder name");
    const archive = within(folders).getByLabelText<HTMLSelectElement>("Archive");
    expect(archive.selectedOptions[0]?.textContent).toBe("None");
    // The inbox is fixed by the server and not offered.
    expect(within(folders).queryByLabelText("Inbox")).toBeNull();
    expect(within(archive).queryByRole("option", { name: "INBOX" })).toBeNull();
  });

  it("the user can give a folder a role, and take a role away", async () => {
    const core = mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    const folders = await openAccount(user);

    await user.selectOptions(within(folders).getByLabelText("Archive"), "Projekte");

    expect(core.callsOf("set_mailbox_role")).toEqual([{ mailboxId: 5, role: "archive" }]);
    await waitFor(() => {
      expect(within(folders).getByLabelText<HTMLSelectElement>("Archive").selectedOptions[0]?.textContent).toBe(
        "Projekte",
      );
    });
    expect(folders.textContent).toContain("Chosen by you");

    await user.selectOptions(within(folders).getByLabelText("Drafts"), "None");
    expect(core.callsOf("set_mailbox_role")[1]).toEqual({ mailboxId: 3, role: "custom" });
  });

  it("a refused change shows why", async () => {
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      failures: { set_mailbox_role: { code: "invalid_argument", message: "x", detail: "not selectable" } },
    });
    const { user } = renderApp();
    const folders = await openAccount(user);

    await user.selectOptions(within(folders).getByLabelText("Junk"), "Projekte");

    const alert = await within(folders).findByRole("alert");
    expect(alert.textContent).toContain("The folder could not be changed.");
    expect(alert.textContent).toContain("not selectable");
  });
});
