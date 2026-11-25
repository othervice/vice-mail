import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Account } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

/*
 * Stage 18, task D: "New folder…" on the account's row and "New subfolder…" on a folder that may
 * hold folders, in their menus and the palette; the name is checked (empty, the server's
 * delimiter, taken) and the folder is created on the server through the outbox. "Reload folders
 * from server" on the account lists the server's folders again. Read-only accounts say why
 * nothing happens.
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const WRITER = makeAccount({ writesEnabled: true, displayName: "Ada" });

function setup(account: Account = WRITER) {
  const core = mockCore({ accounts: [account], mailboxes: standardMailboxes(account.id), messages: { 1: [], 5: [], 6: [] } });
  return { core, ...renderApp() };
}

type User = ReturnType<typeof renderApp>["user"];

async function openColumn(user: User) {
  if (screen.queryByTestId("sidebar-column")) return;
  await user.click(await screen.findByRole("button", { name: "Folders" }));
  await screen.findByTestId("sidebar-column");
}

async function accountMenu(user: User, account: Account = WRITER) {
  await openColumn(user);
  await user.click(await screen.findByRole("button", { name: `More for ${account.displayName || account.email}` }));
  return screen.findByRole("menu");
}

async function folderMenu(user: User, name: string) {
  await openColumn(user);
  await user.click(await screen.findByRole("button", { name: `Options for ${name}` }));
  return screen.findByRole("menu");
}

const notices = () => screen.queryAllByTestId("notice").map((n) => n.textContent);

describe("creating folders", () => {
  it("creates a folder at the top of the account from the account's menu, checked first", async () => {
    const { core, user } = setup();
    const menu = await accountMenu(user);
    expect(within(menu).getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Mark all as read",
      "New folder…",
      "Reload folders from server",
    ]);
    await user.click(within(menu).getByRole("menuitem", { name: "New folder…" }));
    const panel = await screen.findByRole("dialog", { name: "New folder in “Ada”" });
    const field = within(panel).getByRole("textbox", { name: "Name" });
    await waitFor(() => {
      expect(document.activeElement).toBe(field);
    });
    expect(panel.textContent).toContain("The folder is created on the server too.");

    // Empty, with the delimiter, taken: said in place, nothing sent.
    await user.click(within(panel).getByRole("button", { name: "Create" }));
    expect(panel.textContent).toContain("Enter a name.");
    await user.type(field, "Kunden/alt");
    await user.click(within(panel).getByRole("button", { name: "Create" }));
    expect(panel.textContent).toContain("A folder name cannot contain “/”.");
    await user.clear(field);
    await user.type(field, "Projekte");
    await user.click(within(panel).getByRole("button", { name: "Create" }));
    expect(panel.textContent).toContain("A folder “Projekte” exists here already.");
    await user.clear(field);
    await user.type(field, "inbox");
    await user.click(within(panel).getByRole("button", { name: "Create" }));
    expect(panel.textContent).toContain("A folder “inbox” exists here already.");
    expect(core.callsOf("create_folder")).toEqual([]);

    await user.clear(field);
    await user.type(field, " Kunden {Enter}");
    await waitFor(() => {
      expect(core.callsOf("create_folder")).toEqual([{ accountId: WRITER.id, parentId: null, name: "Kunden" }]);
    });
    expect(screen.queryByRole("dialog", { name: "New folder in “Ada”" })).toBeNull();
    expect(notices().some((n) => n.includes("Folder “Kunden” is being created"))).toBe(true);
    // The operation is visible in the activity view (the outbox), and the folder shows in the
    // tree once the server made it (`mailboxesChanged`).
    expect(core.operations.map((op) => [op.kind, op.state])).toEqual([["create_mailbox", "queued"]]);
    core.completeMailboxCreations();
    await emitCore({ type: "mailboxesChanged", accountId: WRITER.id });
    expect(await screen.findByRole("treeitem", { name: "Kunden" })).toBeTruthy();
  });

  it("creates a subfolder from a folder's menu, below that folder", async () => {
    const { core, user } = setup();
    const menu = await folderMenu(user, "Projekte");
    await user.click(within(menu).getByRole("menuitem", { name: "New subfolder…" }));
    const panel = await screen.findByRole("dialog", { name: "New folder in “Projekte”" });
    const field = within(panel).getByRole("textbox", { name: "Name" });
    // A sibling's name is taken below this folder only.
    await user.type(field, "2026{Enter}");
    expect(panel.textContent).toContain("A folder “2026” exists here already.");
    await user.clear(field);
    await user.type(field, "2027{Enter}");
    await waitFor(() => {
      expect(core.callsOf("create_folder")).toEqual([{ accountId: WRITER.id, parentId: 5, name: "2027" }]);
    });
    core.completeMailboxCreations();
    await emitCore({ type: "mailboxesChanged", accountId: WRITER.id });
    const created = await screen.findByRole("treeitem", { name: "2027" });
    expect(created.getAttribute("aria-level")).toBe("3");
    expect(created.getAttribute("title")).toBe("Projekte/2027");
  });

  it("a folder on a server without folders in folders offers no subfolder", async () => {
    const mailboxes = [
      ...standardMailboxes(WRITER.id).filter((m) => m.id !== 6),
      makeMailbox({ id: 9, accountId: WRITER.id, path: "Flach", delimiter: null }),
    ];
    mockCore({ accounts: [WRITER], mailboxes, messages: { 1: [] } });
    const { user } = renderApp();
    const flat = await folderMenu(user, "Flach");
    expect(within(flat).queryByRole("menuitem", { name: "New subfolder…" })).toBeNull();
    await user.keyboard("{Escape}");
    const own = await folderMenu(user, "Projekte");
    expect(within(own).getByRole("menuitem", { name: "New subfolder…" })).toBeTruthy();
  });

  it("a read-only account keeps the items and says why they do nothing; reloading still works", async () => {
    const reader = makeAccount({ writesEnabled: false, displayName: "Leser" });
    const { core, user } = setup(reader);
    let menu = await accountMenu(user, reader);
    const item = within(menu).getByRole("menuitem", { name: "New folder…" });
    await user.click(item);
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
      expect(notices().some((n) => n.includes("Changes are turned off"))).toBe(true);
    });
    menu = await folderMenu(user, "Projekte");
    await user.click(within(menu).getByRole("menuitem", { name: "New subfolder…" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(core.callsOf("create_folder")).toEqual([]);

    menu = await accountMenu(user, reader);
    await user.click(within(menu).getByRole("menuitem", { name: "Reload folders from server" }));
    await waitFor(() => {
      expect(core.callsOf("reload_folders")).toEqual([{ accountId: reader.id }]);
    });
    expect(notices().some((n) => n.includes("Loading the folders of Leser from the server"))).toBe(true);
  });

  it("the palette offers a new folder, a subfolder of the open folder and reloading", async () => {
    const { core, user } = setup();
    await openColumn(user);
    act(() => {
      useUiStore.getState().showMailbox(WRITER.id, 5);
    });
    await user.keyboard("{Meta>}k{/Meta}");
    await user.keyboard("subfolder in");
    const option = await screen.findByRole("option", { name: /New subfolder in “Projekte”/ });
    expect(option).toBeTruthy();
    await user.keyboard("{Enter}");
    const panel = await screen.findByRole("dialog", { name: "New folder in “Projekte”" });
    await user.type(within(panel).getByRole("textbox", { name: "Name" }), "Neu{Enter}");
    await waitFor(() => {
      expect(core.callsOf("create_folder")).toEqual([{ accountId: WRITER.id, parentId: 5, name: "Neu" }]);
    });
    await user.keyboard("{Meta>}k{/Meta}");
    await user.keyboard("reload folders");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(core.callsOf("reload_folders")).toEqual([{ accountId: WRITER.id }]);
    });
  });

  it("the account's menu and the panel have no serious accessibility findings", async () => {
    const { user } = setup();
    const menu = await accountMenu(user);
    await user.click(within(menu).getByRole("menuitem", { name: "New folder…" }));
    await screen.findByRole("dialog", { name: "New folder in “Ada”" });
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
    const serious = results.violations
      .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
      .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
    expect(serious).toEqual([]);
  });
});
