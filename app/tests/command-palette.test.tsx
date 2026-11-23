import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ACTION_IDS, ACTIONS, type ActionContext, type ActionHost, type ActionId } from "../src/features/actions/registry";
import { PALETTE_TOGGLES } from "../src/features/palette/entries";
import { en } from "../src/i18n/en";
import type { MailAction, MailboxRole, MessageListItem } from "../src/services/ipc";
import { useActionTargetStore } from "../src/stores/actionTarget";
import { useComposerStore } from "../src/stores/composer";
import { useSearchStore } from "../src/stores/search";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

type User = ReturnType<typeof renderApp>["user"];

function fakeHost() {
  return {
    apply: vi.fn<ActionHost["apply"]>(),
    confirmDeletePermanently: vi.fn<ActionHost["confirmDeletePermanently"]>(),
    openMove: vi.fn<ActionHost["openMove"]>(),
    openTags: vi.fn<ActionHost["openTags"]>(),
    undo: vi.fn<ActionHost["undo"]>(),
    toggleSelect: vi.fn<ActionHost["toggleSelect"]>(),
    selectAll: vi.fn<ActionHost["selectAll"]>(),
    setPinned: vi.fn<ActionHost["setPinned"]>(),
    openSnooze: vi.fn<ActionHost["openSnooze"]>(),
    unsnooze: vi.fn<ActionHost["unsnooze"]>(),
    setTask: vi.fn<ActionHost["setTask"]>(),
  } satisfies ActionHost;
}

/** A context in which the action can run: the folder and messages it needs. */
function contextFor(id: ActionId): ActionContext {
  const roles: Partial<Record<ActionId, MailboxRole>> = { deletePermanently: "trash", notSpam: "junk", editDraft: "drafts" };
  const role = roles[id] ?? "inbox";
  const items: Partial<Record<ActionId, Partial<MessageListItem>>> = {
    toggleRead: { unread: true },
    markRead: { unread: true },
    markUnread: { unread: false },
    flag: { flagged: false },
    unflag: { flagged: true },
    pin: { pinned: false },
    unpin: { pinned: true },
    unsnooze: { snooze: { until: Date.now() + 3_600_000, returned: false } },
    addTask: {},
    removeTask: { task: { done: false } },
    toggleTaskDone: { task: { done: false } },
    completeTask: { task: { done: false } },
    reopenTask: { task: { done: true } },
    editDraft: { draft: true },
  };
  return {
    account: makeAccount({ writesEnabled: true }),
    mailbox: makeMailbox({ id: 1, path: role === "inbox" ? "INBOX" : role, role }),
    items: [makeMessage({ id: 4242, subject: "Quarterly report", ...items[id] })],
    canUndo: id === "undo",
  };
}

/** What running the action must have done (the same as its shortcut or button). */
function expectRan(id: ActionId, host: ReturnType<typeof fakeHost>, ctx: ActionContext) {
  const apply = (type: MailAction["type"]) => {
    expect(host.apply).toHaveBeenCalledWith({ type }, ctx.items);
  };
  const composer = (kind: string) => {
    expect(useComposerStore.getState().session?.source).toMatchObject({ kind });
  };
  const checks: Record<ActionId, () => void> = {
    archive: () => {
      apply("archive");
    },
    delete: () => {
      apply("delete");
    },
    deletePermanently: () => expect(host.confirmDeletePermanently).toHaveBeenCalledWith(ctx.items),
    toggleRead: () => {
      apply("markRead");
    },
    markRead: () => {
      apply("markRead");
    },
    markUnread: () => {
      apply("markUnread");
    },
    toggleFlag: () => {
      apply("flag");
    },
    flag: () => {
      apply("flag");
    },
    unflag: () => {
      apply("unflag");
    },
    move: () => expect(host.openMove).toHaveBeenCalledWith(ctx.items),
    tags: () => expect(host.openTags).toHaveBeenCalledWith(ctx.items),
    spam: () => {
      apply("spam");
    },
    notSpam: () => {
      apply("notSpam");
    },
    undo: () => expect(host.undo).toHaveBeenCalled(),
    toggleSelect: () => expect(host.toggleSelect).toHaveBeenCalled(),
    selectAll: () => expect(host.selectAll).toHaveBeenCalled(),
    togglePin: () => expect(host.setPinned).toHaveBeenCalledWith(ctx.items, true),
    pin: () => expect(host.setPinned).toHaveBeenCalledWith(ctx.items, true),
    unpin: () => expect(host.setPinned).toHaveBeenCalledWith(ctx.items, false),
    snooze: () => expect(host.openSnooze).toHaveBeenCalledWith(ctx.items),
    unsnooze: () => expect(host.unsnooze).toHaveBeenCalledWith(ctx.items),
    toggleTask: () => expect(host.setTask).toHaveBeenCalledWith(ctx.items, { done: false }),
    addTask: () => expect(host.setTask).toHaveBeenCalledWith(ctx.items, { done: false }),
    removeTask: () => expect(host.setTask).toHaveBeenCalledWith(ctx.items, null),
    toggleTaskDone: () => expect(host.setTask).toHaveBeenCalledWith(ctx.items, { done: true }),
    completeTask: () => expect(host.setTask).toHaveBeenCalledWith(ctx.items, { done: true }),
    reopenTask: () => expect(host.setTask).toHaveBeenCalledWith(ctx.items, { done: false }),
    reply: () => {
      composer("reply");
    },
    replyAll: () => {
      composer("reply_all");
    },
    forward: () => {
      composer("forward");
    },
    editDraft: () => {
      composer("message");
    },
    compose: () => {
      composer("new");
    },
  };
  checks[id]();
}

function palette() {
  return screen.getByRole("dialog", { name: "Command palette" });
}

function field() {
  return within(palette()).getByRole("combobox", { name: "Command" });
}

function activeOption(): HTMLElement | null {
  return document.getElementById(field().getAttribute("aria-activedescendant") ?? "");
}

const optionLabel = (option: HTMLElement | null) => option?.querySelector("span > span")?.textContent ?? null;

async function openPalette(user: User) {
  await user.keyboard("{Meta>}k{/Meta}");
  await screen.findByRole("dialog", { name: "Command palette" });
  await waitFor(() => {
    expect(document.activeElement).toBe(field());
  });
}

/** Types the query and walks with the arrow keys to the entry with this label: keyboard only. */
async function chooseByKeyboard(user: User, query: string, label: string) {
  await user.keyboard(query);
  const options = within(palette()).getAllByRole("option");
  for (let step = 0; step <= options.length && optionLabel(activeOption()) !== label; step += 1) {
    await user.keyboard("{ArrowDown}");
  }
  expect(optionLabel(activeOption())).toBe(label);
  await user.keyboard("{Enter}");
}

describe("command palette", () => {
  it("every action of the directory can be found and run from the keyboard", async () => {
    mockCore();
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    for (const id of ACTION_IDS) {
      const ctx = contextFor(id);
      const host = fakeHost();
      useComposerStore.setState(useComposerStore.getInitialState(), true);
      act(() => {
        useActionTargetStore.getState().register("test", { ctx, host });
      });
      // A toggle is listed as the explicit action it runs, under the same label.
      const listed = PALETTE_TOGGLES.some(([toggle]) => toggle === id) ? ACTIONS[id].label(ctx) : id;
      const label = en.actions.labels[listed as keyof typeof en.actions.labels];
      await openPalette(user);
      await chooseByKeyboard(user, label, label);
      expect(screen.queryByRole("dialog", { name: "Command palette" }), id).toBeNull();
      expectRan(id, host, ctx);
    }
    // 33 rounds through the palette: more than the default 5 s on a busy machine.
  }, 30_000);

  it("shows actions that cannot run here, with the reason, and does not run them", async () => {
    mockCore();
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    const host = fakeHost();
    act(() => {
      useActionTargetStore.getState().register("test", { ctx: { ...contextFor("archive"), items: [] }, host });
    });
    await openPalette(user);
    await user.keyboard("Archive");
    const option = within(palette()).getAllByRole("option").find((o) => optionLabel(o) === "Archive");
    expect(option?.getAttribute("aria-disabled")).toBe("true");
    expect(option?.textContent).toContain("Select a message first");
    // Walk to it and press Enter: it stays open and says why.
    while (activeOption() !== option) await user.keyboard("{ArrowDown}");
    await user.keyboard("{Enter}");
    expect(palette()).toBeTruthy();
    expect(within(palette()).getByText("Archive: Select a message first")).toBeTruthy();
    expect(host.apply).not.toHaveBeenCalled();
  });

  it("outside a mail view names the reason; a new message works from the selected account", async () => {
    const account = makeAccount();
    mockCore({ accounts: [account], mailboxes: standardMailboxes(), messages: { 1: [] } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      useUiStore.getState().showSettings("general");
    });
    await openPalette(user);
    await user.keyboard("Archive");
    const archive = within(palette()).getAllByRole("option").find((o) => optionLabel(o) === "Archive");
    expect(archive?.textContent).toContain("Open a mail view first");
    await user.keyboard("{Escape}");
    await openPalette(user);
    await chooseByKeyboard(user, "new message", "New message");
    expect(useComposerStore.getState().session?.source).toEqual({ kind: "new", accountId: account.id });
  });

  it("runs an action on the open message of the real mail view and gives the focus back", async () => {
    const account = makeAccount({ writesEnabled: true });
    const items = makeMessages(5);
    const core = mockCore({
      accounts: [account],
      mailboxes: [...standardMailboxes(), makeMailbox({ id: 7, path: "Archive", role: "archive", roleSource: "server" })],
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    const first = within(list).getAllByRole("option")[0];
    if (!first) throw new Error("no rows");
    await user.click(first);
    await screen.findByRole("region", { name: "Message" });
    list.focus();

    await openPalette(user);
    await chooseByKeyboard(user, "arch", "Archive");

    await waitFor(() => {
      expect(core.callsOf("apply_action")).toHaveLength(1);
    });
    expect(core.callsOf("apply_action")[0]?.["action"]).toEqual({ type: "archive" });
    expect(core.callsOf("apply_action")[0]?.["messages"]).toEqual([{ mailboxId: 1, messageId: items[0]?.id }]);
    // The focus is back in the list (on the next message, as after the shortcut).
    expect(document.activeElement).toBe(list);
  });

  it("goes to views, folders, accounts and settings", async () => {
    const ada = makeAccount();
    const grace = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "grace@vicemail.test", displayName: "Grace" });
    mockCore({ accounts: [ada, grace], mailboxes: standardMailboxes(), messages: { 1: [], 5: [], 6: [] } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });

    await openPalette(user);
    await chooseByKeyboard(user, "sent", "Go to Sent");
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "sent" });
    // The opener was the page itself: the main area takes the focus, Tab goes on from there.
    expect(document.activeElement).toBe(screen.getByRole("main"));

    await openPalette(user);
    await chooseByKeyboard(user, "2026", "Open folder 2026");
    expect(useUiStore.getState().view).toEqual({ kind: "mailbox", accountId: ada.id, mailboxId: 6 });

    await openPalette(user);
    await chooseByKeyboard(user, "grace", "Switch to Grace (grace@vicemail.test)");
    expect(useUiStore.getState().selectedAccountId).toBe(grace.id);

    await openPalette(user);
    await chooseByKeyboard(user, "keyboard", "Settings: Keyboard");
    expect(useUiStore.getState().view).toEqual({ kind: "settings", section: "keyboard" });
    expect(await screen.findByRole("heading", { name: "Keyboard shortcuts" })).toBeTruthy();
  });

  it("hands the typed text to the search", async () => {
    mockCore({ accounts: [makeAccount()], mailboxes: standardMailboxes(), messages: { 1: [] } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    useUiStore.getState().showSettings("general");

    await openPalette(user);
    await chooseByKeyboard(user, "budget 2027", "Search mail for “budget 2027”");

    expect(useSearchStore.getState()).toMatchObject({ open: true, query: "budget 2027" });
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "inbox" });
  });

  it("runs the app commands of the directory: search and the shortcut overview", async () => {
    mockCore({ accounts: [makeAccount()], mailboxes: standardMailboxes(), messages: { 1: [] } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    await openPalette(user);
    // The search command names its shortcut.
    await user.keyboard("search");
    const option = within(palette()).getAllByRole("option").find((o) => optionLabel(o) === "Search");
    expect(option?.getAttribute("aria-keyshortcuts")).toBe("Meta+F");
    await user.keyboard("{Escape}");
    await openPalette(user);
    await chooseByKeyboard(user, "search", "Search");
    expect(useSearchStore.getState().open).toBe(true);
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole("searchbox", { name: "Search mail" }));
    });
  });

  it("lists the recently used commands on top", async () => {
    mockCore();
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    await openPalette(user);
    await chooseByKeyboard(user, "drafts", "Go to Drafts");
    await openPalette(user);
    const recent = within(palette()).getByRole("group", { name: "Recently used" });
    expect(within(recent).getAllByRole("option").map(optionLabel)).toEqual(["Go to Drafts"]);
    // The first entry is active, so Enter runs it again.
    expect(optionLabel(activeOption())).toBe("Go to Drafts");
  });

  it("is a combobox with a listbox; Escape closes it and gives the focus back, Tab stays inside", async () => {
    mockCore();
    const { user } = renderApp();
    const settings = await screen.findByRole("button", { name: "Settings" });
    settings.focus();
    await openPalette(user);
    const input = field();
    expect(input.getAttribute("aria-expanded")).toBe("true");
    const listbox = document.getElementById(input.getAttribute("aria-controls") ?? "");
    expect(listbox?.getAttribute("role")).toBe("listbox");
    expect(activeOption()?.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{Tab}");
    expect(document.activeElement).toBe(input);
    // The last entry wraps round to the first.
    await user.keyboard("{ArrowUp}");
    const last = within(palette()).getAllByRole("option").at(-1);
    expect(activeOption()).toBe(last);
    await user.keyboard("{ArrowDown}");
    expect(activeOption()).toBe(within(palette()).getAllByRole("option")[0]);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(document.activeElement).toBe(settings);
  });

  it("opens the views of the rail with Cmd+1 to 8, which the rail and the palette name", async () => {
    mockCore({ accounts: [makeAccount()], mailboxes: standardMailboxes(), messages: { 1: [] } });
    const { user } = renderApp();
    const later = await screen.findByTestId("rail-later");
    expect(later.getAttribute("aria-keyshortcuts")).toBe("Meta+2");
    expect(screen.getByTestId("rail-trash").getAttribute("aria-keyshortcuts")).toBe("Meta+8");
    expect(screen.getByTestId("rail-folders").getAttribute("aria-keyshortcuts")).toBeNull();

    await user.keyboard("{Meta>}2{/Meta}");
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "later" });
    // From the search field too (a shortcut with Cmd works while typing).
    act(() => {
      useSearchStore.getState().openSearch();
    });
    const field = await screen.findByRole("searchbox", { name: "Search mail" });
    field.focus();
    await user.keyboard("{Meta>}5{/Meta}");
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "sent" });

    await openPalette(user);
    await user.keyboard("go to drafts");
    const drafts = within(palette()).getAllByRole("option").find((o) => optionLabel(o) === "Go to Drafts");
    expect(drafts?.getAttribute("aria-keyshortcuts")).toBe("Meta+6");
  });

  it("shows the shortcut overview with ? and from the palette", async () => {
    mockCore();
    const { user } = renderApp();
    const settings = await screen.findByRole("button", { name: "Settings" });
    settings.focus();
    await user.keyboard("?");
    const overview = await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
    expect(within(overview).getByTestId("overview-archive").textContent).toContain("E");
    expect(within(overview).getByTestId("overview-palette").textContent).toContain("⌘K");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Keyboard shortcuts" })).toBeNull();
    expect(document.activeElement).toBe(settings);

    await openPalette(user);
    await chooseByKeyboard(user, "shortcuts", "Show keyboard shortcuts");
    expect(await screen.findByRole("dialog", { name: "Keyboard shortcuts" })).toBeTruthy();
  });
});
