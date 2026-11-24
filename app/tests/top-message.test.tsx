import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CoreWindowLayout, MessageListItem } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { DEFAULT_WINDOW_LAYOUT } from "./support/uiPrefs";

/*
 * Stage 18, task F (decision 0054): with the reading pane right of the list or below it, opening a
 * folder or a view shows its top message at once; the list keeps its reading size, also with
 * nothing selected (the pane's place says so quietly). A message shown this way is not marked
 * read on opening until the user selects it or works with it in the pane; the focus stays where
 * it was. In place of the list nothing opens by itself, and search results never jump.
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const WRITER = makeAccount({ writesEnabled: true });

function setup({ layout = {}, items = makeMessages(4).map((m) => ({ ...m, unread: true })), mode = "immediately" as const } = {}) {
  const core = mockCore({
    ui: { layout: { ...DEFAULT_WINDOW_LAYOUT, ...(layout as Partial<CoreWindowLayout>) } },
    accounts: [WRITER],
    mailboxes: [...standardMailboxes(WRITER.id), makeMailbox({ id: 9, accountId: WRITER.id, path: "Leer" })],
    messages: { 1: items, 9: [] },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: mode },
  });
  return { core, items, ...renderApp() };
}

const markReads = (core: ReturnType<typeof mockCore>) =>
  core.callsOf("apply_action").filter((call) => (call["action"] as { type: string }).type === "markRead") as { messages: { messageId: number }[] }[];

/** Lets timers and promises run (the mark-read delay, the core's answers). */
async function settle(ms = 50) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

describe("the top message when a folder opens", () => {
  it("shows the inbox's top message on the right, unread, without the focus", async () => {
    const { core, items } = setup();
    const top = items[0] as MessageListItem;
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(core.callsOf("get_message")).toEqual([{ messageId: top.id, format: "auto" }]);
    });
    expect(pane.textContent).toContain(top.subject);
    const option = within(list).getAllByRole("option")[0] as HTMLElement;
    expect(option.getAttribute("aria-selected")).toBe("true");
    // The focus did not move: not into the pane, not into the list.
    expect(pane.contains(document.activeElement)).toBe(false);
    expect(document.activeElement).not.toBe(list);
    // "Right away" does not count for a message the list opened by itself.
    await settle();
    expect(markReads(core)).toEqual([]);
    expect(option.getAttribute("aria-label") ?? option.textContent).toBeTruthy();
    // The list keeps its reading width beside the pane.
    expect(list.parentElement?.className).toContain("w-(--ui-list-width-reading)");
  });

  it("is marked read once the user selects it", async () => {
    const { core, items, user } = setup();
    const top = items[0] as MessageListItem;
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    await screen.findByRole("region", { name: "Message" });
    await settle();
    expect(markReads(core)).toEqual([]);
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    await waitFor(() => {
      expect(markReads(core).map((call) => call.messages[0]?.messageId)).toEqual([top.id]);
    });
  });

  it("is marked read once the user works with it in the pane, and so is the next one chosen", async () => {
    const { core, items, user } = setup();
    const [top, second] = items as [MessageListItem, MessageListItem];
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    const pane = await screen.findByRole("region", { name: "Message" });
    await settle();
    expect(markReads(core)).toEqual([]);
    fireEvent.pointerDown(within(pane).getAllByRole("heading")[0] as HTMLElement);
    await waitFor(() => {
      expect(markReads(core).map((call) => call.messages[0]?.messageId)).toEqual([top.id]);
    });
    // Moving on from the keyboard is the user's choice: marked as before.
    act(() => {
      list.focus();
    });
    await user.keyboard("{ArrowDown}");
    await waitFor(() => {
      expect(markReads(core).map((call) => call.messages[0]?.messageId)).toEqual([top.id, second.id]);
    });
  });

  it("Tab into the list keeps it unread; Enter opens it for the user", async () => {
    const { core, items, user } = setup();
    const top = items[0] as MessageListItem;
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    await screen.findByRole("region", { name: "Message" });
    act(() => {
      list.focus();
    });
    await settle();
    // The focus keeps the cursor where the list put it, still a preview.
    expect(markReads(core)).toEqual([]);
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(markReads(core).map((call) => call.messages[0]?.messageId)).toEqual([top.id]);
    });
  });

  it("with the pane below the list too; in place of the list nothing opens by itself", async () => {
    setup({ layout: { readingPane: "bottom" } });
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    await screen.findByRole("region", { name: "Message" });
    expect(list.parentElement?.className).toContain("h-(--ui-list-height-reading)");
  });

  it("in place of the list the list stays and nothing opens", async () => {
    const { core } = setup({ layout: { readingPane: "replace" } });
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    await within(list).findAllByRole("option");
    await settle();
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    expect(screen.queryByTestId("reading-empty")).toBeNull();
    expect(core.callsOf("get_message")).toEqual([]);
    expect(list.parentElement?.className).toContain("flex-1");
  });

  it("an empty folder keeps the list's reading width and says that no message is selected", async () => {
    setup();
    await screen.findByRole("region", { name: "Message" });
    act(() => {
      useUiStore.getState().showMailbox(WRITER.id, 9);
    });
    const empty = await screen.findByTestId("reading-empty");
    // The list column (the folder's empty state in it), first in the box of list and pane.
    const list = { parentElement: document.querySelector("[data-reading-pane]")?.firstElementChild ?? null };
    expect(empty.textContent).toContain("No message selected");
    expect(empty.textContent).toContain("Choose a message in the list to read it here.");
    expect(list.parentElement?.className).toContain("w-(--ui-list-width-reading)");
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    // The border between list and pane stays to drag.
    expect(screen.getByRole("separator", { name: "List width" })).toBeTruthy();
  });

  it("a cleared selection does not come back by itself", async () => {
    const { user } = setup();
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(within(pane).getByRole("button", { name: "Close message" }));
    await screen.findByTestId("reading-empty");
    await settle();
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
  });

  it("the unified inbox opens its top message too, unread", async () => {
    const work = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", writesEnabled: true });
    const items = makeMessages(2).map((m) => ({ ...m, unread: true }));
    const core = mockCore({
      accounts: [WRITER, work],
      mailboxes: [...standardMailboxes(WRITER.id), makeMailbox({ id: 20, accountId: work.id, path: "INBOX", role: "inbox", roleSource: "server" })],
      messages: { 1: items, 20: [] },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "immediately" },
    });
    renderApp();
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(pane.textContent).toContain(items[0]?.subject ?? "");
    await settle();
    expect(markReads(core)).toEqual([]);
  });
});
