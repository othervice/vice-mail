import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Account } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const WRITER = makeAccount({ writesEnabled: true });

/** Standard folders plus `Projekte/2026` with four stored messages; the column open. */
function setup(account: Account = WRITER) {
  const mailboxes = standardMailboxes(account.id).map((m) => (m.id === 6 ? makeMailbox({ ...m, total: 4 }) : m));
  const core = mockCore({ accounts: [account], mailboxes, messages: { 1: [], 5: [], 6: [] } });
  return { core, ...renderApp() };
}

type User = ReturnType<typeof renderApp>["user"];

/** The accounts-and-folders column, opened with the rail's "Folders". */
async function openColumn(user: User) {
  await user.click(await screen.findByRole("button", { name: "Folders" }));
  await screen.findByTestId("sidebar-column");
}

async function menuOf(user: User, name: string) {
  if (!screen.queryByTestId("sidebar-column")) await openColumn(user);
  await user.click(await screen.findByRole("button", { name: `Options for ${name}` }));
  return screen.findByRole("menu");
}

function notices(): string[] {
  return screen.queryAllByTestId("notice").map((n) => n.textContent);
}

describe("renaming and deleting folders", () => {
  it("renames a folder of the user's own from its menu, through the outbox, and shows it on its way", async () => {
    const { core, user } = setup();
    const menu = await menuOf(user, "Projekte");
    await user.click(within(menu).getByRole("menuitem", { name: "Rename folder…" }));
    const panel = await screen.findByRole("dialog", { name: "Rename “Projekte”" });
    const field = within(panel).getByRole("textbox", { name: "New name" });
    await waitFor(() => {
      expect(document.activeElement).toBe(field);
    });
    expect((field as HTMLInputElement).value).toBe("Projekte");
    // A name with the delimiter is refused here already.
    await user.clear(field);
    await user.type(field, "Kunden/alt");
    await user.click(within(panel).getByRole("button", { name: "Rename" }));
    expect(panel.textContent).toContain("A folder name cannot contain “/”.");
    expect(core.callsOf("rename_mailbox")).toEqual([]);
    await user.clear(field);
    await user.type(field, "Kunden{Enter}");
    await waitFor(() => {
      expect(core.callsOf("rename_mailbox")).toEqual([{ accountId: WRITER.id, mailboxId: 5, name: "Kunden" }]);
    });
    expect(screen.queryByRole("dialog", { name: "Rename “Projekte”" })).toBeNull();
    expect(notices().some((n) => n.includes("Folder “Projekte” is being renamed"))).toBe(true);
    // The pending change shows at the folder until the server confirmed it.
    await emitCore({ type: "operationsChanged", accountId: WRITER.id, operations: [], removed: [] });
    const pending = await screen.findByTestId("folder-pending-5");
    expect(pending.textContent).toBe("being renamed");
    expect(screen.getByRole("treeitem", { name: /^Projekte, being renamed/ })).toBeTruthy();
  });

  it("asks before deleting and says how many messages the folder holds here; not with subfolders", async () => {
    const { core, user } = setup();
    let menu = await menuOf(user, "2026");
    await user.click(within(menu).getByRole("menuitem", { name: "Delete folder…" }));
    let dialog = await screen.findByRole("alertdialog", { name: "Delete “2026”?" });
    expect(dialog.textContent).toContain("the 4 messages stored in it are deleted on the server too");
    // Cancel first: nothing happens.
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(core.callsOf("delete_mailbox")).toEqual([]);
    menu = await menuOf(user, "2026");
    await user.click(within(menu).getByRole("menuitem", { name: "Delete folder…" }));
    dialog = await screen.findByRole("alertdialog", { name: "Delete “2026”?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete folder" }));
    await waitFor(() => {
      expect(core.callsOf("delete_mailbox")).toEqual([{ accountId: WRITER.id, mailboxId: 6 }]);
    });

    // A folder with subfolders: the dialog says to delete them first, and nothing is sent.
    menu = await menuOf(user, "Projekte");
    await user.click(within(menu).getByRole("menuitem", { name: "Delete folder…" }));
    dialog = await screen.findByRole("alertdialog", { name: "Delete “Projekte”?" });
    expect(dialog.textContent).toContain("Delete the subfolders of “Projekte” first.");
    await user.click(within(dialog).getByRole("button", { name: "Delete folder" }));
    expect(core.callsOf("delete_mailbox")).toHaveLength(1);
  });

  it("folders with a role have neither; a read-only or JMAP account says why nothing happens", async () => {
    const reader = makeAccount({ writesEnabled: false });
    const { core, user } = setup(reader);
    const sent = await menuOf(user, "Sent");
    expect(within(sent).queryByRole("menuitem", { name: "Rename folder…" })).toBeNull();
    expect(within(sent).queryByRole("menuitem", { name: "Delete folder…" })).toBeNull();
    await user.keyboard("{Escape}");

    const menu = await menuOf(user, "Projekte");
    await user.click(within(menu).getByRole("menuitem", { name: "Rename folder…" }));
    expect(screen.queryByRole("dialog", { name: "Rename “Projekte”" })).toBeNull();
    await waitFor(() => {
      expect(notices().some((n) => n.includes("Changes are turned off"))).toBe(true);
    });
    expect(core.callsOf("rename_mailbox")).toEqual([]);
  });

  it("a JMAP account's folders say that they cannot be changed from the app yet", async () => {
    const jmap = makeAccount({ writesEnabled: true, protocol: "jmap" });
    const { core, user } = setup(jmap);
    const menu = await menuOf(user, "Projekte");
    await user.click(within(menu).getByRole("menuitem", { name: "Delete folder…" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => {
      expect(notices().some((n) => n.includes("The folder could not be changed."))).toBe(true);
    });
    expect(core.callsOf("delete_mailbox")).toEqual([]);
  });

  it("the palette renames or deletes the open folder; on a read-only account it says why not", async () => {
    const { core, user } = setup();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      useUiStore.getState().showMailbox(WRITER.id, 5);
    });
    await user.keyboard("{Meta>}k{/Meta}");
    let palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("rename folder");
    const option = within(palette)
      .getAllByRole("option")
      .find((o) => o.textContent.includes("Rename folder “Projekte”…"));
    expect(option).toBeTruthy();
    if (option) await user.click(option);
    const panel = await screen.findByRole("dialog", { name: "Rename “Projekte”" });
    const field = within(panel).getByRole("textbox", { name: "New name" });
    await user.clear(field);
    await user.type(field, "Archiv 2025{Enter}");
    await waitFor(() => {
      expect(core.callsOf("rename_mailbox")).toEqual([{ accountId: WRITER.id, mailboxId: 5, name: "Archiv 2025" }]);
    });

    act(() => {
      useUiStore.getState().showMailbox(WRITER.id, 6);
    });
    await user.keyboard("{Meta>}k{/Meta}");
    palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("delete folder");
    const remove = within(palette)
      .getAllByRole("option")
      .find((o) => o.textContent.includes("Delete folder “2026”…"));
    expect(remove).toBeTruthy();
  });

  it("the palette offers nothing for a read-only account's folder but the reason", async () => {
    const reader = makeAccount({ writesEnabled: false });
    const { user } = setup(reader);
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      useUiStore.getState().showMailbox(reader.id, 5);
    });
    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("rename folder");
    const option = within(palette)
      .getAllByRole("option")
      .find((o) => o.textContent.includes("Rename folder “Projekte”…"));
    expect(option?.getAttribute("aria-disabled")).toBe("true");
    expect(option?.textContent).toContain("Changes are turned off for this account");
  });
});
