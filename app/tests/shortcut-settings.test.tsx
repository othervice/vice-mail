import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useShortcutStore } from "../src/stores/shortcuts";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { optionName } from "./support/names";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

type User = ReturnType<typeof renderApp>["user"];

function setup() {
  const items = makeMessages(6);
  const core = mockCore({
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: [...standardMailboxes(), makeMailbox({ id: 7, path: "Archive", role: "archive", roleSource: "server" })],
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, items };
}

async function openKeyboardSettings() {
  act(() => {
    useUiStore.getState().showSettings("keyboard");
  });
  return screen.findByRole("heading", { name: "Keyboard shortcuts" });
}

const row = (id: string) => screen.getByTestId(`shortcut-row-${id}`);
const chips = (id: string) =>
  within(within(row(id)).getByRole("list", { name: "Shortcuts" }))
    .queryAllByRole("listitem")
    .map((item) => item.querySelector("kbd")?.textContent);

/** Settings → Keyboard: "Add" for a command, then the key press. */
async function record(user: User, label: string, keys: string) {
  await user.click(screen.getByRole("button", { name: `Add shortcut for ${label}` }));
  const recorder = await screen.findByTestId("shortcut-recorder");
  expect(document.activeElement).toBe(recorder);
  await user.keyboard(keys);
}

/** The inbox with its first message open and the focus in the list. */
async function openInbox(user: User) {
  act(() => {
    useUiStore.getState().showMailView("inbox");
  });
  const list = await screen.findByRole("listbox", { name: /^Messages in/ });
  const first = within(list).getAllByRole("option")[0];
  if (!first) throw new Error("no rows");
  await user.click(first);
  await screen.findByRole("region", { name: "Message" });
  list.focus();
  return list;
}

describe("changeable shortcuts", () => {
  it("a new shortcut works at once and the old one keeps working until it is removed", async () => {
    const { core, items } = setup();
    const { user } = renderApp();
    await openKeyboardSettings();

    await record(user, "Archive", "y");
    expect(screen.getByTestId("shortcut-status").textContent).toBe("Y now runs Archive.");
    expect(chips("archive")).toEqual(["E", "Y"]);
    // The focus is back on "Add", where the recording started.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Add shortcut for Archive" }));
    expect(within(row("archive")).getByText("Changed")).toBeTruthy();

    await openInbox(user);
    await user.keyboard("y");
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toHaveLength(1);
    });
    expect(core.callsOf("apply_action")[0]).toMatchObject({
      action: { type: "archive" },
      messages: [{ mailboxId: 1, messageId: items[0]?.id }],
    });
    // The tooltip of the toolbar names the new assignment's first shortcut.
    const toolbar = screen.getByRole("toolbar", { name: "Message actions" });
    expect(within(toolbar).getByRole("button", { name: "Archive" }).getAttribute("aria-keyshortcuts")).toBe("E");

    // Removing "E" takes effect at once too.
    await openKeyboardSettings();
    await user.click(screen.getByRole("button", { name: "Remove E from Archive" }));
    expect(chips("archive")).toEqual(["Y"]);
    const list = await openInbox(user);
    await user.keyboard("e");
    expect(core.callsOf("apply_action")).toHaveLength(1);
    expect(document.activeElement).toBe(list);
    expect(within(screen.getByRole("toolbar", { name: "Message actions" })).getByRole("button", { name: "Archive" }).getAttribute("aria-keyshortcuts")).toBe("Y");
  });

  it("keeps the change after a restart", async () => {
    const { core } = setup();
    const first = renderApp();
    await openKeyboardSettings();
    await record(first.user, "Archive", "y");
    await first.user.click(screen.getByRole("button", { name: "Remove E from Archive" }));
    first.unmount();

    // A restart: the stores start empty, the app loads what the core stored and renders anew.
    await waitFor(() => {
      expect(core.ui.preferences.shortcuts.macos).toEqual([{ command: "archive", shortcuts: [{ key: "y", shift: false, mod: false }] }]);
    });
    useShortcutStore.setState(useShortcutStore.getInitialState(), true);
    useUiStore.setState(useUiStore.getInitialState(), true);
    const second = renderApp();
    await openKeyboardSettings();
    expect(chips("archive")).toEqual(["Y"]);

    await openInbox(second.user);
    await second.user.keyboard("e");
    expect(core.callsOf("apply_action")).toHaveLength(0);
    await second.user.keyboard("y");
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toHaveLength(1);
    });
    expect(core.callsOf("apply_action")[0]?.["action"]).toEqual({ type: "archive" });
  });

  it("refuses a shortcut another command has, system keys and the keys for moving around", async () => {
    setup();
    const { user } = renderApp();
    await openKeyboardSettings();

    await record(user, "Reply", "e");
    const status = screen.getByTestId("shortcut-status");
    expect(status.textContent).toBe("E is already assigned to “Archive”. Remove it there first.");
    expect(status.getAttribute("role")).toBe("status");
    // Still recording: the next key press is a new try. A system key is refused as well.
    await user.keyboard("{Meta>}q{/Meta}");
    expect(status.textContent).toBe("⌘Q belongs to the system or to text editing and cannot be assigned.");
    await user.keyboard("{ArrowDown}");
    expect(status.textContent).toBe("↓ is used for moving around in the app and cannot be assigned.");
    await user.keyboard("{Alt>}r{/Alt}");
    expect(status.textContent).toContain("Option and Alt combinations");
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("shortcut-recorder")).toBeNull();
    expect(chips("reply")).toEqual(["R"]);
    expect(chips("archive")).toEqual(["E"]);
    expect(within(row("reply")).queryByText("Changed")).toBeNull();

    // After freeing "e" there, it can be assigned here.
    await user.click(screen.getByRole("button", { name: "Remove E from Archive" }));
    await record(user, "Reply", "e");
    expect(chips("reply")).toEqual(["R", "E"]);
    // Reset all brings the standard back.
    await user.click(screen.getByRole("button", { name: "Reset all shortcuts" }));
    expect(chips("reply")).toEqual(["R"]);
    expect(chips("archive")).toEqual(["E"]);
    expect(screen.getByTestId("shortcut-status").textContent).toBe("All shortcuts are back to standard.");
  });

  it("a changed shortcut works in the unified inbox and in the palette too", async () => {
    const work = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", writesEnabled: true });
    const mine = makeMessages(2);
    const theirs = makeMessages(1, { mailboxId: 20, newest: Date.now() - 30_000 }).map((m) => ({ ...m, accountId: work.id, subject: "Projekt" }));
    const core = mockCore({
      accounts: [makeAccount({ writesEnabled: true }), work],
      mailboxes: [
        ...standardMailboxes(),
        makeMailbox({ id: 7, path: "Archive", role: "archive", roleSource: "server" }),
        makeMailbox({ id: 20, accountId: work.id, path: "INBOX", role: "inbox", roleSource: "server" }),
        makeMailbox({ id: 21, accountId: work.id, path: "Archive", role: "archive", roleSource: "server" }),
      ],
      messages: { 1: mine, 20: theirs },
      views: Object.fromEntries([...mine, ...theirs].map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    await openKeyboardSettings();
    await user.click(screen.getByRole("button", { name: "Remove E from Archive" }));
    await record(user, "Archive", "y");

    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(3);
    });
    const row = within(list).getAllByRole("option").find((o) => optionName(o).includes("Projekt"));
    if (!row) throw new Error("no row");
    await user.click(row);
    await screen.findByRole("region", { name: "Message" });
    list.focus();
    await user.keyboard("e");
    expect(core.callsOf("apply_action")).toHaveLength(0);
    await user.keyboard("y");
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toHaveLength(1);
    });
    expect(core.callsOf("apply_action")[0]).toMatchObject({ action: { type: "archive" }, messages: [{ mailboxId: 20 }] });
    // The palette names the new shortcut.
    await user.keyboard("{Meta>}k{/Meta}");
    await user.keyboard("archive");
    const option = within(screen.getByRole("dialog", { name: "Command palette" }))
      .getAllByRole("option")
      .find((o) => o.querySelector("span > span")?.textContent === "Archive");
    expect(option?.getAttribute("aria-keyshortcuts")).toBe("Y");
  });

  it("Tab and Shift+Tab leave the recorder; Shift with the moving keys is not assignable", async () => {
    setup();
    const { user } = renderApp();
    await openKeyboardSettings();
    await record(user, "Archive", "{Shift>}{ArrowDown}{/Shift}");
    expect(screen.getByTestId("shortcut-status").textContent).toBe(
      "⇧↓ is used for moving around in the app and cannot be assigned.",
    );
    await user.keyboard("{Shift>}{Tab}{/Shift}");
    expect(screen.queryByTestId("shortcut-recorder")).toBeNull();
    expect(chips("archive")).toEqual(["E"]);
    await record(user, "Archive", "{Tab}");
    expect(screen.queryByTestId("shortcut-recorder")).toBeNull();
    expect(chips("archive")).toEqual(["E"]);
  });

  it("does not reset a command whose standard key another command has now", async () => {
    setup();
    const { user } = renderApp();
    await openKeyboardSettings();
    await user.click(screen.getByRole("button", { name: "Remove E from Archive" }));
    await record(user, "Reply", "e");
    await user.click(screen.getByRole("button", { name: "Reset Archive" }));
    expect(screen.getByTestId("shortcut-status").textContent).toBe(
      "Archive cannot go back to standard: E is assigned to “Reply” now. Remove it there first.",
    );
    expect(chips("archive")).toEqual([]);
    expect(chips("reply")).toEqual(["R", "E"]);
  });

  it("changes the palette shortcut itself, which works at once", async () => {
    mockCore();
    const { user } = renderApp();
    await openKeyboardSettings();
    // Cmd+P prints (outside the directory), so it is taken.
    await record(user, "Command palette", "{Meta>}p{/Meta}");
    expect(screen.getByTestId("shortcut-status").textContent).toBe(
      "⌘P is used elsewhere in the app (printing, sending) and cannot be assigned.",
    );
    await user.keyboard("{Meta>}j{/Meta}");
    expect(chips("palette")).toEqual(["⌘K", "⌘J"]);
    await user.click(screen.getByRole("button", { name: "Remove ⌘K from Command palette" }));
    screen.getByRole("button", { name: "Add shortcut for Archive" }).focus();
    await user.keyboard("{Meta>}k{/Meta}");
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    await user.keyboard("{Meta>}j{/Meta}");
    expect(await screen.findByRole("dialog", { name: "Command palette" })).toBeTruthy();
  });
});
