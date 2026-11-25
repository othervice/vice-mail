import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Account } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

/*
 * Stage 21, item 2 (decision 0063): "Mark folder as read" in a folder's "…" menu and on its right
 * click, "Mark all as read" on the account (its inbox and own folders) and on the unified inbox
 * (the inboxes of its accounts). One call per account, one notice with "Undo"; nothing while
 * nothing is unread; on a read-only account unavailable, with the reason (axe: sidebar-menus).
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const ADA = makeAccount({ writesEnabled: true, displayName: "Ada" });
const GRACE = makeAccount({ id: "22222222-2222-4222-8222-222222222222", email: "grace@vicemail.test", writesEnabled: true, displayName: "Grace" });

function graceMailboxes() {
  return [
    makeMailbox({ id: 21, accountId: GRACE.id, path: "INBOX", role: "inbox", roleSource: "server", unread: 4 }),
    makeMailbox({ id: 22, accountId: GRACE.id, path: "Archiv", unread: 7 }),
  ];
}

function setup(accounts: Account[] = [ADA]) {
  const mailboxes = accounts.flatMap((account) => (account.id === GRACE.id ? graceMailboxes() : standardMailboxes(account.id)));
  // Sent has unread mail too: the account's "Mark all as read" leaves it alone.
  const sent = mailboxes.find((m) => m.id === 2);
  if (sent) sent.unread = 5;
  const core = mockCore({ accounts, mailboxes, messages: { 1: [], 5: [], 6: [], 21: [] } });
  return { core, ...renderApp() };
}

type User = ReturnType<typeof renderApp>["user"];

async function openColumn(user: User) {
  if (screen.queryByTestId("sidebar-column")) return;
  await user.click(await screen.findByRole("button", { name: "Folders" }));
  await screen.findByTestId("sidebar-column");
}

const notices = () => screen.queryAllByTestId("notice").map((n) => n.textContent);

describe("mark folder as read", () => {
  it("is the first entry of a folder's menu, also on a right click, and marks the folder with undo", async () => {
    const { core, user } = setup();
    await openColumn(user);
    await user.pointer({ keys: "[MouseRight]", target: await screen.findByTestId("mailbox-5") });
    const menu = await screen.findByRole("menu", { name: "Options for Projekte" });
    const items = within(menu).getAllByRole("menuitem");
    expect(items[0]?.textContent).toBe("Mark folder as read");
    expect(document.activeElement).toBe(items[0]);
    await user.click(within(menu).getByRole("menuitem", { name: "Mark folder as read" }));
    await waitFor(() => {
      expect(core.callsOf("mark_mailboxes_read")).toEqual([{ accountId: ADA.id, mailboxIds: [5], requestId: expect.any(String) as string }]);
    });
    expect(core.operations.map((op) => [op.action, op.kind, op.mailboxId, op.messageId, op.messageCount])).toEqual([
      ["mark_mailbox_read", "mark_mailbox", 5, null, 2],
    ]);
    await waitFor(() => {
      expect(notices()).toContain("2 messages in Projekte marked as readUndoZ");
    });
    // The counter drops with `mailboxesChanged`, as after any action.
    await emitCore({ type: "mailboxesChanged", accountId: ADA.id });
    await waitFor(() => {
      expect(screen.getByTestId("mailbox-5").getAttribute("aria-label")).toBe("Projekte");
    });

    await user.click(screen.getByTestId("notice-undo"));
    await waitFor(() => {
      expect(core.callsOf("undo_action")).toHaveLength(1);
    });
    expect(core.mailboxes.find((m) => m.id === 5)?.unread).toBe(2);
  });

  it("is not offered for a folder without unread mail", async () => {
    const { user } = setup();
    await openColumn(user);
    await user.click(await screen.findByRole("button", { name: "Options for Drafts" }));
    const menu = await screen.findByRole("menu", { name: "Options for Drafts" });
    expect(within(menu).queryByRole("menuitem", { name: "Mark folder as read" })).toBeNull();
  });

  it("stays in the menu of a read-only account, unavailable, with the reason, and sends nothing", async () => {
    const reader = makeAccount({ writesEnabled: false, displayName: "Leser" });
    const { core, user } = setup([reader]);
    await openColumn(user);
    act(() => {
      screen.getByTestId("mailbox-5").focus();
    });
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu", { name: "Options for Projekte" });
    const item = within(menu).getByRole("menuitem", { name: "Mark folder as read" });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(item.textContent).toContain("Changes are turned off for this account.");
    const described = document.getElementById(item.getAttribute("aria-describedby") ?? "");
    expect(described?.textContent).toBe("Changes are turned off for this account.");
    await user.click(item);
    await waitFor(() => {
      expect(notices().some((n) => n?.includes("Allow changes…"))).toBe(true);
    });
    expect(core.callsOf("mark_mailboxes_read")).toEqual([]);
  });

  it("marks the account's inbox and own folders from its row, not Sent", async () => {
    const { core, user } = setup();
    await openColumn(user);
    await user.click(await screen.findByRole("button", { name: "More for Ada" }));
    const menu = await screen.findByRole("menu");
    await user.click(within(menu).getByRole("menuitem", { name: "Mark all as read" }));
    await waitFor(() => {
      expect(core.callsOf("mark_mailboxes_read")).toEqual([{ accountId: ADA.id, mailboxIds: [1, 5, 6], requestId: expect.any(String) as string }]);
    });
    await waitFor(() => {
      expect(notices()).toContain("6 messages in Ada marked as readUndoZ");
    });
    expect(core.mailboxes.find((m) => m.id === 2)?.unread).toBe(5);
  });

  it("marks the inboxes of the unified inbox's accounts, one call each, undone together", async () => {
    const { core, user } = setup([ADA, GRACE]);
    await openColumn(user);
    await user.pointer({ keys: "[MouseRight]", target: await screen.findByTestId("unified-all") });
    const menu = await screen.findByRole("menu", { name: "Options for All inboxes" });
    await user.click(within(menu).getByRole("menuitem", { name: "Mark all as read" }));
    await waitFor(() => {
      expect(core.callsOf("mark_mailboxes_read").map((call) => [call["accountId"], call["mailboxIds"]])).toEqual([
        [ADA.id, [1]],
        [GRACE.id, [21]],
      ]);
    });
    await waitFor(() => {
      expect(notices()).toContain("7 messages in All inboxes marked as readUndoZ");
    });
    expect(core.mailboxes.find((m) => m.id === 22)?.unread).toBe(7);
    await user.click(screen.getByTestId("notice-undo"));
    await waitFor(() => {
      expect(core.callsOf("undo_action")).toHaveLength(2);
    });
    expect(core.mailboxes.filter((m) => m.role === "inbox").map((m) => m.unread)).toEqual([3, 4]);
  });

  it("shows the folder's messages in the activity view", async () => {
    const { core, user } = setup();
    await openColumn(user);
    await user.click(await screen.findByRole("button", { name: "Options for Projekte" }));
    await user.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Mark folder as read" }));
    await waitFor(() => {
      expect(notices().length).toBeGreaterThan(0);
    });
    await emitCore({ type: "operationsChanged", accountId: ADA.id, operations: core.operations, removed: [] });
    act(() => {
      useUiStore.getState().showActivity();
    });
    const batch = await screen.findByTestId(/^activity-batch-/);
    expect(batch.textContent).toContain("Mark folder as read");
    expect(batch.textContent).toContain("2 messages");
    expect(batch.textContent).toContain("Projekte");
  });
});
