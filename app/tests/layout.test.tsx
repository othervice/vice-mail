import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadLayout, useLayoutStore } from "../src/stores/layout";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

type User = ReturnType<typeof renderApp>["user"];

function setup() {
  const items = makeMessages(6);
  mockCore({
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { items, ...renderApp() };
}

async function chooseInAppearance(user: User, group: string, option: string) {
  act(() => {
    useUiStore.getState().showSettings("appearance");
  });
  const radios = await screen.findByRole("radiogroup", { name: group });
  await user.click(within(radios).getByRole("radio", { name: option }));
  act(() => {
    useUiStore.getState().showMailView("inbox");
  });
}

const panes = () => document.querySelector<HTMLElement>("[data-reading-pane]");
const cursorRow = (list: HTMLElement) => document.getElementById(list.getAttribute("aria-activedescendant") ?? "");

describe("layouts", () => {
  it("puts the reading pane below the list and stores the choice for the window", async () => {
    const { user } = setup();
    await chooseInAppearance(user, "Reading pane", "Below");
    expect(loadLayout("main").readingPane).toBe("bottom");

    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("{Enter}");
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(panes()?.dataset["readingPane"]).toBe("bottom");
    // List first, pane below it, in one column.
    expect(panes()?.className).toContain("flex-col");
    expect(list.compareDocumentPosition(pane) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await waitFor(() => {
      expect(pane.contains(document.activeElement)).toBe(true);
    });
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(list);
  });

  it("shows a message in place of the list only once it is opened; Escape brings the list back", async () => {
    const { user, items } = setup();
    await chooseInAppearance(user, "Reading pane", "Instead of the list");

    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("{ArrowDown}");
    // Moving through the list does not open anything.
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    const cursor = cursorRow(list);
    expect(cursor?.getAttribute("aria-label")).toContain(items[1]?.subject ?? "");

    await user.keyboard("{Enter}");
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(screen.queryByRole("listbox", { name: /^Messages in/ })).toBeNull();
    await waitFor(() => {
      expect(pane.contains(document.activeElement)).toBe(true);
    });

    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    });
    const back = screen.getByRole("listbox", { name: /^Messages in/ });
    expect(document.activeElement).toBe(back);
    // The cursor is where it was.
    expect(cursorRow(back)).toBe(cursor);
  });

  it("switches to compact rows at once and keeps them", async () => {
    const { user } = setup();
    expect(document.documentElement.dataset["density"]).not.toBe("compact");
    await chooseInAppearance(user, "Density", "Compact");
    expect(document.documentElement.dataset["density"]).toBe("compact");
    expect(loadLayout("main").density).toBe("compact");
    expect(await screen.findByRole("listbox", { name: /^Messages in/ })).toBeTruthy();
  });

  it("sets the text size from 80 to 150 percent and keeps it", async () => {
    const { user } = setup();
    expect(document.documentElement.dataset["textSize"]).toBeUndefined();
    await chooseInAppearance(user, "Text size", "150%");
    expect(document.documentElement.dataset["textSize"]).toBe("150");
    expect(loadLayout("main").textSize).toBe(150);
    await chooseInAppearance(user, "Text size", "80%");
    expect(document.documentElement.dataset["textSize"]).toBe("80");
    await chooseInAppearance(user, "Text size", "100%");
    expect(document.documentElement.dataset["textSize"]).toBeUndefined();
    // Stored values are checked: an unknown size falls back to 100.
    window.localStorage.setItem("vice-mail.ui.layout.main", JSON.stringify({ textSize: 400, readingPane: "left" }));
    expect(loadLayout("main")).toMatchObject({ textSize: 100, readingPane: "right" });
  });

  it("j and k move through the list like the arrow keys", async () => {
    const { user, items } = setup();
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("jj");
    expect(cursorRow(list)?.getAttribute("aria-label")).toContain(items[2]?.subject ?? "");
    await user.keyboard("k");
    expect(cursorRow(list)?.getAttribute("aria-label")).toContain(items[1]?.subject ?? "");
  });

  it("applies to the unified inbox too", async () => {
    const work = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work" });
    const ada = makeMessage({ id: 3000, subject: "Privat", date: Date.now() - 60_000 });
    const job = makeMessage({ id: 2000, accountId: work.id, mailboxId: 20, subject: "Projekt", date: Date.now() - 120_000 });
    mockCore({
      accounts: [makeAccount(), work],
      mailboxes: [...standardMailboxes(), makeMailbox({ id: 20, accountId: work.id, path: "INBOX", role: "inbox", roleSource: "server" })],
      messages: { 1: [ada], 20: [job] },
      views: { 3000: makeView(ada), 2000: makeView(job) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    await chooseInAppearance(user, "Reading pane", "Instead of the list");
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(2);
    });
    list.focus();
    await user.keyboard("j");
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    await user.keyboard("{Enter}");
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(pane.textContent).toContain("Projekt");
    expect(screen.queryByRole("listbox", { name: "Messages in All inboxes" })).toBeNull();
    await waitFor(() => {
      expect(pane.contains(document.activeElement)).toBe(true);
    });
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole("listbox", { name: "Messages in All inboxes" }));
    });
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
  });

  it("keeps the unfolded sidebar after a restart", async () => {
    const first = setup();
    await first.user.click(await screen.findByTestId("rail-folders"));
    expect(useUiStore.getState().sidebarExpanded).toBe(true);
    expect(loadLayout("main").sidebarExpanded).toBe(true);
    first.unmount();

    useUiStore.setState(useUiStore.getInitialState(), true);
    useLayoutStore.setState({ ...useLayoutStore.getInitialState(), ...loadLayout("main") }, true);
    renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    expect(useUiStore.getState().sidebarExpanded).toBe(true);
  });

  it("changes the layout from the command palette", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await user.keyboard("{Meta>}k{/Meta}");
    await user.keyboard("pane below");
    await user.keyboard("{Enter}");
    expect(useLayoutStore.getState().readingPane).toBe("bottom");
    await user.keyboard("{Meta>}k{/Meta}");
    await user.keyboard("compact rows");
    await user.keyboard("{Enter}");
    expect(useLayoutStore.getState().density).toBe("compact");
  });
});
