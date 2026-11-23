import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MailAction, MessageListItem, OperationInfo } from "../src/services/ipc";
import {
  ipcError,
  makeAccount,
  makeMailbox,
  makeMessages,
  makeView,
  mockCore,
  standardMailboxes,
} from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });
const READ_ONLY = makeAccount({ writesEnabled: false });

/** The list virtualizer needs a viewport height (jsdom has no layout). */
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

/** Inbox, Sent, Drafts, Trash, Projekte, Projekte/2026, and an archive and a junk folder. */
function mailboxes(options: { archive?: boolean; junk?: boolean } = {}) {
  return [
    ...standardMailboxes(),
    ...(options.archive === false ? [] : [makeMailbox({ id: 7, path: "Archive", role: "archive", roleSource: "server" })]),
    ...(options.junk === false ? [] : [makeMailbox({ id: 8, path: "Junk", role: "junk", roleSource: "server" })]),
  ];
}

interface SetupOptions {
  account?: typeof ADA;
  items?: MessageListItem[];
  /** Messages of other folders, by id. */
  others?: Record<number, MessageListItem[]>;
  extra?: Parameters<typeof mockCore>[0];
  archive?: boolean;
}

function setup({ account = ADA, items = makeMessages(10), others = {}, extra = {}, archive }: SetupOptions = {}) {
  const all = [...items, ...Object.values(others).flat()];
  const core = mockCore({
    accounts: [account],
    mailboxes: mailboxes({ ...(archive !== undefined && { archive }) }),
    messages: { 1: items, ...others },
    views: Object.fromEntries(all.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    ...extra,
  });
  const rendered = renderApp();
  return { core, ...rendered, items };
}

const actionCalls = (core: ReturnType<typeof mockCore>) =>
  core.callsOf("apply_action") as { action: MailAction; messages: { mailboxId: number; messageId: number }[]; requestId: string }[];

async function list() {
  return screen.findByRole("listbox", { name: /^Messages in/ });
}

function row(listbox: HTMLElement, subject: string): HTMLElement {
  const option = within(listbox)
    .getAllByRole("option")
    .find((o) => (o.getAttribute("aria-label") ?? "").includes(`, ${subject},`));
  if (!option) throw new Error(`no row ${subject}`);
  return option;
}

/** The row the list's keyboard cursor is on. */
function cursorRow(listbox: HTMLElement): HTMLElement | null {
  return document.getElementById(listbox.getAttribute("aria-activedescendant") ?? "");
}

function selectedRows(listbox: HTMLElement): string[] {
  return within(listbox)
    .getAllByRole("option")
    .filter((o) => o.getAttribute("aria-selected") === "true")
    .map((o) => /Message \d+(?=,)/.exec(o.getAttribute("aria-label") ?? "")?.[0] ?? "");
}

async function openFirst(user: ReturnType<typeof renderApp>["user"], subject = "Message 1") {
  const listbox = await list();
  await user.click(row(listbox, subject));
  const pane = await screen.findByRole("region", { name: "Message" });
  return { listbox, pane };
}

describe("actions from the keyboard", () => {
  it.each<[string, string, MailAction["type"], string]>([
    ["e", "e", "archive", "Archived"],
    ["#", "#", "delete", "Moved to the trash"],
    ["Shift+I", "{Shift>}I{/Shift}", "markRead", "Marked as read"],
    ["s", "s", "flag", "Flagged"],
    ["!", "!", "spam", "Moved to junk"],
  ])("%s runs %s on the selected message, with an undo notice", async (_name, keys, type, notice) => {
    // Message 1 is unread (every third one is), so marking it read changes something.
    const { core, user, items } = setup();
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard(keys);

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    const [call] = actionCalls(core);
    expect(call?.action.type).toBe(type);
    expect(call?.messages).toEqual([{ mailboxId: 1, messageId: items[0]?.id }]);
    // A fresh UUID per user action.
    expect(call?.requestId).toMatch(/^[0-9a-f-]{36}$/);
    const shown = await screen.findByTestId("notice");
    expect(shown.textContent).toContain(notice);
    expect(within(shown).getByRole("button", { name: /Undo/ })).toBeTruthy();
  });

  it("Shift+U marks unread, ⌫ deletes on macOS", async () => {
    const items = makeMessages(5).map((item) => ({ ...item, unread: false }));
    const { core, user } = setup({ items });
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("{Shift>}U{/Shift}");
    await waitFor(() => {
      expect(actionCalls(core).map((c) => c.action.type)).toEqual(["markUnread"]);
    });
    await user.keyboard("{Backspace}");
    await waitFor(() => {
      expect(actionCalls(core).map((c) => c.action.type)).toEqual(["markUnread", "delete"]);
    });
  });

  it("archiving puts the cursor and the focus on the next mail and opens it", async () => {
    const { core, user } = setup();
    const { listbox } = await openFirst(user, "Message 3");
    listbox.focus();

    await user.keyboard("e");

    await waitFor(() => {
      expect(within(listbox).queryByText("Message 3")).toBeNull();
    });
    expect(actionCalls(core)[0]?.messages).toHaveLength(1);
    expect(cursorRow(listbox)?.textContent).toContain("Message 4");
    expect(document.activeElement).toBe(listbox);
    const pane = screen.getByRole("region", { name: "Message" });
    expect(within(pane).getByRole("heading", { level: 2 }).textContent).toBe("Message 4");
    // The core moved it: it is in the archive now.
    expect(core.messagesIn(7).map((m) => m.subject)).toEqual(["Message 3"]);
  });

  it("shortcuts stay out of text fields", async () => {
    const { core, user } = setup();
    await openFirst(user);
    await user.keyboard("v");
    const field = await screen.findByRole("combobox", { name: "Filter folders" });

    await user.type(field, "e#s");

    expect(actionCalls(core)).toHaveLength(0);
    expect((field as HTMLInputElement).value).toBe("e#s");
  });
});

describe("actions from the toolbar and the rows", () => {
  it("the reading pane has a toolbar with the actions, their shortcuts and arrow-key navigation", async () => {
    const { core, user } = setup();
    const { pane } = await openFirst(user);

    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    const names = within(toolbar)
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label"));
    expect(names).toEqual(["Archive", "Delete", "Mark as read", "Flag", "Move to folder", "Tags", "Mark as junk"]);
    const archive = within(toolbar).getByRole("button", { name: "Archive" });
    expect(archive.getAttribute("aria-keyshortcuts")).toBe("E");
    // One tab stop; arrows move along.
    expect(archive.tabIndex).toBe(0);
    archive.focus();
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Delete");

    await user.click(within(toolbar).getByRole("button", { name: "Flag" }));
    await waitFor(() => {
      expect(actionCalls(core).map((c) => c.action)).toEqual([{ type: "flag" }]);
    });
    // The core changed its local state and says so; the row and the toolbar follow.
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });
    await waitFor(() => {
      expect(within(screen.getByRole("toolbar", { name: "Message actions" })).getByRole("button", { name: "Remove flag" })).toBeTruthy();
    });
  });

  it("a quick action acts on its row only and leaves the selection alone", async () => {
    const { core, user, items } = setup();
    const { listbox } = await openFirst(user);

    const second = row(listbox, "Message 2");
    // Hidden from screen readers (mouse shortcut), so found by test id.
    await user.click(within(second).getByTestId("quick-archive"));

    await waitFor(() => {
      expect(within(listbox).queryByText("Message 2")).toBeNull();
    });
    expect(actionCalls(core)[0]?.messages).toEqual([{ mailboxId: 1, messageId: items[1]?.id }]);
    expect(selectedRows(listbox)).toEqual(["Message 1"]);
    expect(within(second).queryByRole("button")).toBeNull();
  });

  it("quick actions are not focusable and not seen by screen readers; the row names the same actions as shortcuts", async () => {
    const { user } = setup();
    const listbox = await list();
    const first = row(listbox, "Message 1");
    const actions = within(first).getByTestId("row-actions");
    expect(actions.getAttribute("aria-hidden")).toBe("true");
    for (const button of within(actions).getAllByRole("button", { hidden: true })) {
      expect(button.tabIndex).toBe(-1);
    }
    expect(within(actions).getByTestId("quick-archive").getAttribute("title")).toBe("Archive (E)");
    void user;
  });

  it("Undo takes the action back through the core", async () => {
    const { core, user } = setup();
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("e");
    await waitFor(() => {
      expect(within(listbox).queryByText("Message 1")).toBeNull();
    });

    await user.click(within(await screen.findByTestId("notice")).getByRole("button", { name: /Undo/ }));

    await waitFor(() => {
      expect(core.callsOf("undo_action")).toEqual([{ batch: 1 }]);
    });
    expect(await within(listbox).findByText("Message 1")).toBeTruthy();
    expect((await screen.findByTestId("notice")).textContent).toContain("Undone.");
  });

  it("Z undoes the last action too; what cannot be undone says so", async () => {
    const { core, user } = setup({ extra: { fail: (cmd) => (cmd === "undo_action" ? ipcError("not_undoable") : undefined) } });
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("e");
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });

    await user.keyboard("z");

    await waitFor(() => {
      expect(core.callsOf("undo_action")).toHaveLength(1);
    });
    const notice = await screen.findByText("This action cannot be undone.");
    expect(notice).toBeTruthy();
  });

  it("notices are announced: calm ones in a status region, failures as alerts", async () => {
    const { user } = setup({ extra: { failures: { apply_action: ipcError("server_rejected") } } });
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("s");

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The action could not be carried out.");
    expect(alert.textContent).toContain("The server refused the request.");
    const region = screen.getByTestId("notices");
    expect(within(region).getByRole("status")).toBeTruthy();
  });

  it("a row whose change failed comes back into the list", async () => {
    const { core, user } = setup({ extra: { failures: { apply_action: ipcError("database") } } });
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("e");

    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(within(listbox).getByText("Message 1")).toBeTruthy();
    expect(core.messagesIn(1)).toHaveLength(10);
  });
});

describe("pending and failed changes", () => {
  it("rows show a symbol for a change on its way, a failed one and a conflict, also for screen readers", async () => {
    const [a, b, c, d] = makeMessages(4);
    const items = [
      { ...a, activity: "pending" as const },
      { ...b, activity: "failed" as const },
      { ...c, activity: "conflict" as const },
      d,
    ].filter((item): item is MessageListItem => item !== undefined);
    setup({ items });
    const listbox = await list();

    expect(within(row(listbox, "Message 1")).getByTestId("row-activity-pending")).toBeTruthy();
    expect(within(row(listbox, "Message 2")).getByTestId("row-activity-failed")).toBeTruthy();
    expect(within(row(listbox, "Message 3")).getByTestId("row-activity-conflict")).toBeTruthy();
    expect(within(row(listbox, "Message 4")).queryByTestId(/row-activity/)).toBeNull();
    expect(row(listbox, "Message 2").getAttribute("aria-label")).toContain("Change failed");
    expect(row(listbox, "Message 1").getAttribute("aria-label")).toContain("Change waiting for the server");
  });

  it("the rail counts what waits and what needs the user; a failure of an own action gets a notice", async () => {
    const { core, user } = setup();
    const activity = await screen.findByTestId("rail-activity");
    expect(activity.getAttribute("aria-label")).toBe("Activity");
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("e");
    await waitFor(() => {
      expect(core.operations).toHaveLength(1);
    });
    const [operation] = core.operations;
    await emitCore({ type: "operationsChanged", accountId: ADA.id, operations: core.operations, removed: [] });

    await waitFor(() => {
      expect(activity.getAttribute("aria-label")).toBe("Activity: 1 waiting, 0 need attention");
    });
    expect(within(activity).getByTestId("activity-badge").textContent).toBe("1");

    const failed = core.settle([operation?.id ?? 0], "failed", { error: ipcError("server_rejected") });
    await emitCore({ type: "operationsChanged", accountId: ADA.id, operations: failed, removed: [] });

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("A change could not be made on the server: Archive");
    expect(alert.textContent).toContain("The server refused the request.");
    await waitFor(() => {
      expect(activity.getAttribute("aria-label")).toBe("Activity: 0 waiting, 1 need attention");
    });
    expect(within(activity).getByTestId("activity-badge").dataset["attention"]).toBe("true");

    await user.click(within(alert).getByRole("button", { name: "Show activity" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Activity" })).toBeTruthy();
  });
});

function operation(overrides: Partial<OperationInfo>): OperationInfo {
  return {
    id: 1,
    batch: 1,
    accountId: ADA.id,
    action: "archive",
    kind: "move",
    state: "queued",
    mailboxId: 1,
    messageId: 100,
    targetMailboxId: 7,
    attempts: 1,
    error: null,
    conflict: null,
    notice: null,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

describe("activity view", () => {
  it("lists the operations per account with state, error and conflict; retry and discard go to the core", async () => {
    const core = mockCore({
      accounts: [ADA],
      mailboxes: mailboxes(),
      operations: [
        operation({ id: 5, batch: 3, action: "flag", kind: "set_flag", state: "queued", targetMailboxId: null }),
        operation({ id: 4, batch: 2, action: "move", state: "conflict", conflict: "gone", targetMailboxId: 5 }),
        operation({ id: 3, batch: 1, state: "failed", error: ipcError("server_rejected"), messageId: 101 }),
        operation({ id: 2, batch: 1, state: "confirmed", messageId: 102 }),
      ],
    });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Activity: 1 waiting, 2 need attention" }));

    const view = await screen.findByTestId(`activity-${ADA.id}`);
    expect(within(view).getByRole("heading", { level: 2 }).textContent).toBe("Ada Lovelace (ada@vicemail.test)");
    const archived = within(view).getByTestId("activity-batch-1");
    expect(archived.textContent).toContain("Archive");
    expect(archived.textContent).toContain("2 messages");
    expect(archived.textContent).toContain("Inbox → Archive");
    expect(within(archived).getByTestId("activity-states").textContent).toContain("1 Failed");
    expect(within(archived).getByTestId("activity-states").textContent).toContain("1 Done");
    expect(within(archived).getByTestId("activity-error").textContent).toContain("The server refused the request.");
    const moved = within(view).getByTestId("activity-batch-2");
    expect(within(moved).getByTestId("activity-conflict").textContent).toBe(
      "The message was moved or deleted in another app. The server's state was kept.",
    );
    // Waiting changes can be neither retried nor discarded (undo them instead).
    const flagged = within(view).getByTestId("activity-batch-3");
    expect(within(flagged).queryByRole("button")).toBeNull();

    await user.click(within(archived).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("retry_operation")).toEqual([{ operationId: 3 }]);
    });
    await user.click(within(moved).getByRole("button", { name: "Remove from list" }));
    await waitFor(() => {
      expect(core.callsOf("discard_operation")).toEqual([{ operationId: 4 }]);
    });
    await waitFor(() => {
      expect(within(view).queryByTestId("activity-batch-2")).toBeNull();
    });
  });

  it("reloads on operationsChanged and pages with the last id", async () => {
    const many = Array.from({ length: 100 }, (_, i) =>
      operation({ id: 200 - i, batch: 200 - i, state: "confirmed", messageId: 1000 + i }),
    );
    const core = mockCore({ accounts: [ADA], mailboxes: mailboxes(), operations: [...many, operation({ id: 50, batch: 50, state: "confirmed" })] });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Activity" }));
    const view = await screen.findByTestId(`activity-${ADA.id}`);
    await within(view).findByTestId("activity-batch-200");

    await user.click(within(view).getByRole("button", { name: "Show older" }));

    await waitFor(() => {
      expect(core.callsOf("list_operations").at(-1)).toEqual({ accountId: ADA.id, before: 101, limit: 100 });
    });
    expect(await within(view).findByTestId("activity-batch-50")).toBeTruthy();
    const before = core.callsOf("list_operations").length;
    await emitCore({ type: "operationsChanged", accountId: ADA.id, operations: [], removed: [200] });
    await waitFor(() => {
      expect(core.callsOf("list_operations").length).toBeGreaterThan(before);
    });
  });

  it("says when nothing happened", async () => {
    mockCore({ accounts: [ADA], mailboxes: mailboxes() });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Activity" }));
    expect(await screen.findByText("No changes in the last day.")).toBeTruthy();
  });
});

describe("read-only accounts", () => {
  it("show the actions disabled, say why and lead to the switch", async () => {
    const { core, user } = setup({ account: READ_ONLY });
    const { pane } = await openFirst(user);

    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    for (const button of within(toolbar).getAllByRole("button")) {
      expect(button.getAttribute("aria-disabled")).toBe("true");
    }
    const note = within(pane).getByTestId("read-only-actions");
    expect(note.textContent).toContain("Read-only");
    expect(note.textContent).toContain("Changes are turned off for this account, so the app only reads its mail.");
    expect(toolbar.getAttribute("aria-describedby")).toBeTruthy();

    // A click on a disabled action explains it instead of doing nothing.
    await user.click(within(toolbar).getByRole("button", { name: "Archive" }));
    const notice = await screen.findByTestId("notice");
    expect(notice.textContent).toContain("Changes are turned off for this account.");
    expect(actionCalls(core)).toHaveLength(0);

    await user.click(within(note).getByRole("button", { name: "Allow changes…" }));
    expect(await screen.findByTestId("account-detail")).toBeTruthy();
  });

  it("shortcuts show the same explanation", async () => {
    const { core, user } = setup({ account: READ_ONLY });
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("e");

    const notice = await screen.findByTestId("notice");
    expect(notice.textContent).toContain("Changes are turned off for this account.");
    expect(within(notice).getByRole("button", { name: "Allow changes…" })).toBeTruthy();
    expect(actionCalls(core)).toHaveLength(0);
    // Selecting still works.
    await user.keyboard("{Shift>}{ArrowDown}{/Shift}");
    expect(selectedRows(listbox)).toEqual(["Message 1", "Message 2"]);
  });

  it("quick actions are disabled with the reason in their tooltip", async () => {
    setup({ account: READ_ONLY });
    const listbox = await list();
    const archive = within(row(listbox, "Message 1")).getByTestId("quick-archive");
    expect(archive.getAttribute("aria-disabled")).toBe("true");
    expect(archive.getAttribute("title")).toBe("Archive: changes are turned off for this account");
  });

  it("the core's read_only answer is explained too (writes turned off elsewhere)", async () => {
    const { user } = setup({ extra: { failures: { apply_action: ipcError("read_only") } } });
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("e");

    const notice = await screen.findByTestId("notice");
    expect(notice.dataset["noticeKind"]).toBe("readOnly");
    expect(notice.textContent).toContain("Changes are turned off for this account.");
    expect(within(listbox).getByText("Message 1")).toBeTruthy();
  });

  it("opening a message marks nothing", async () => {
    const { core, user } = setup({ account: READ_ONLY, extra: { readingPreferences: { markReadOnOpen: "immediately" } } });
    await openFirst(user);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    expect(actionCalls(core)).toHaveLength(0);
  });
});

describe("the write switch", () => {
  it("is off by default, explained, and turns writes on for this account only", async () => {
    const core = mockCore({ accounts: [READ_ONLY], mailboxes: mailboxes() });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${READ_ONLY.id}`));

    const toggle = await screen.findByRole("switch", { name: "Allow changes" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    const description = document.getElementById(toggle.getAttribute("aria-describedby") ?? "");
    expect(description?.textContent).toContain("mark messages as read, flag and tag them, move them and delete them on the server");
    expect(description?.textContent).toContain("applies to this account only");
    expect(screen.getByTestId("read-only-account").textContent).toContain("Turn on “Allow changes” below");

    await user.click(toggle);

    expect(core.callsOf("set_writes_enabled")).toEqual([{ accountId: READ_ONLY.id, enabled: true }]);
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    expect(screen.getByTestId("writes-state").textContent).toBe("Changes are allowed for this account.");
    expect(screen.queryByTestId("read-only-account")).toBeNull();
  });

  it("the read-only hint in the error texts points to the switch", async () => {
    const { en } = await import("../src/i18n/en");
    expect(en.errors.codes.read_only.hint).toContain("Allow changes");
    expect(en.errors.codes.read_only.hint).not.toContain("later version");
    expect(en.settings.accounts.setup.readOnlyBody).not.toContain("later version");
  });
});

describe("multi-selection", () => {
  it("Shift-click selects a range, Cmd-click one more or one less; the bar counts and acts on all as one batch", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    expect(listbox.getAttribute("aria-multiselectable")).toBe("true");

    await user.click(row(listbox, "Message 2"));
    await user.keyboard("{Shift>}");
    await user.click(row(listbox, "Message 5"));
    await user.keyboard("{/Shift}");
    expect(selectedRows(listbox)).toEqual(["Message 2", "Message 3", "Message 4", "Message 5"]);

    await user.keyboard("{Meta>}");
    await user.click(row(listbox, "Message 8"));
    await user.click(row(listbox, "Message 3"));
    await user.keyboard("{/Meta}");
    expect(selectedRows(listbox)).toEqual(["Message 2", "Message 4", "Message 5", "Message 8"]);

    const bar = screen.getByRole("region", { name: "Selection" });
    expect(within(bar).getByTestId("selection-count").textContent).toBe("4 selected");
    // The pane says what is selected instead of showing one message.
    expect(screen.getByTestId("selection-summary").textContent).toContain("4 messages selected");
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();

    await user.click(within(within(bar).getByRole("toolbar")).getByRole("button", { name: "Archive" }));

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]?.messages.map((m) => m.messageId)).toEqual([1, 3, 4, 7].map((i) => items[i]?.id));
    const notice = await screen.findByTestId("notice");
    expect(notice.textContent).toContain("4 messages archived");
    await waitFor(() => {
      expect(within(listbox).getAllByRole("option")).toHaveLength(6);
    });
    // The next mail after the selection takes over.
    expect(cursorRow(listbox)?.getAttribute("aria-label")).toContain(", Message 9,");
  });

  it("the selection box at the start of a row toggles it, and shows in every row while several are selected", async () => {
    const { user } = setup();
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));

    await user.click(within(row(listbox, "Message 3")).getByTestId("row-check"));

    expect(selectedRows(listbox)).toEqual(["Message 1", "Message 3"]);
    expect(within(row(listbox, "Message 2")).getByTestId("row-check").className).toContain("flex");
    expect(within(row(listbox, "Message 2")).getByTestId("row-check").className).not.toContain("hidden");
  });

  it("Shift+arrows extend from the anchor; X and Space toggle the cursor row; Escape clears", async () => {
    const { user } = setup();
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));

    await user.keyboard("{Shift>}{ArrowDown}{ArrowDown}{/Shift}");
    expect(selectedRows(listbox)).toEqual(["Message 1", "Message 2", "Message 3"]);
    await user.keyboard("x");
    expect(selectedRows(listbox)).toEqual(["Message 1", "Message 2"]);
    await user.keyboard(" ");
    expect(selectedRows(listbox)).toEqual(["Message 1", "Message 2", "Message 3"]);
    await user.keyboard("{Escape}");
    expect(selectedRows(listbox)).toEqual([]);
  });

  it("Cmd+A selects the loaded messages and says honestly how many more the folder has", async () => {
    const items = makeMessages(250);
    const core = mockCore({
      accounts: [ADA],
      mailboxes: mailboxes().map((m) => (m.id === 1 ? { ...m, total: 250 } : m)),
      messages: { 1: items },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));

    await user.keyboard("{Meta>}a{/Meta}");

    const bar = screen.getByRole("region", { name: "Selection" });
    expect(within(bar).getByTestId("selection-count").textContent).toBe("200 selected");
    expect(within(bar).getByTestId("selection-not-loaded").textContent).toBe(
      "50 more messages in this folder are not loaded yet.",
    );

    await user.click(within(bar).getByRole("button", { name: "Select all in this folder" }));

    await waitFor(() => {
      expect(within(bar).getByTestId("selection-count").textContent).toBe("250 selected");
    });
    expect(core.callsOf("list_messages").length).toBeGreaterThanOrEqual(2);
    expect(within(bar).queryByTestId("selection-not-loaded")).toBeNull();
  });

  it("one action takes at most 10,000 messages; the bar says so", async () => {
    const items = makeMessages(10_050);
    mockCore({
      accounts: [ADA],
      mailboxes: mailboxes().map((m) => (m.id === 1 ? { ...m, total: 10_050 } : m)),
      messages: { 1: items },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    await user.keyboard("{Meta>}a{/Meta}");
    const bar = screen.getByRole("region", { name: "Selection" });

    await user.click(within(bar).getByRole("button", { name: "Select all in this folder" }));

    await waitFor(
      () => {
        expect(within(bar).getByTestId("selection-count").textContent).toBe("10,000 selected");
      },
      { timeout: 10_000 },
    );
    expect(within(bar).getByTestId("selection-limit").textContent).toBe(
      "One action works on at most 10,000 messages at a time; only the first 10,000 are selected.",
    );
  }, 20_000);
});

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

describe("drag and drop", () => {
  it("moves dragged rows onto a folder of the sidebar, which shows itself as the target", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    await user.keyboard("{Shift>}{ArrowDown}{/Shift}");
    await user.click(screen.getByRole("button", { name: "Folders" }));
    const target = await screen.findByTestId("mailbox-5");
    const transfer = dataTransfer();

    const source = row(listbox, "Message 2");
    fireEvent.dragStart(source, { dataTransfer: transfer });
    expect(target.dataset["dropTarget"]).toBe("ready");
    // Not the folder the rows come from.
    expect(screen.getByTestId("mailbox-1").dataset["dropTarget"]).toBeUndefined();
    fireEvent.dragEnter(target, { dataTransfer: transfer });
    fireEvent.dragOver(target, { dataTransfer: transfer });
    expect(target.dataset["dropTarget"]).toBe("over");
    expect(target.className).toContain("outline-accent");
    // The dragged rows leave the list at once; the drop itself ends the drag.
    fireEvent.drop(target, { dataTransfer: transfer });

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]?.action).toEqual({ type: "move", target: 5 });
    expect(actionCalls(core)[0]?.messages.map((m) => m.messageId)).toEqual([items[0]?.id, items[1]?.id]);
    expect(transfer.setDragImage).toHaveBeenCalled();
    expect((await screen.findByTestId("notice")).textContent).toContain("2 messages moved to Projekte");
    expect(target.dataset["dropTarget"]).toBeUndefined();
  });

  it("a row outside the selection is dragged alone; the rail's Archive takes it", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    await user.click(row(listbox, "Message 1"));
    const transfer = dataTransfer();
    const archive = screen.getByTestId("rail-archive");

    fireEvent.dragStart(row(listbox, "Message 4"), { dataTransfer: transfer });
    fireEvent.dragOver(archive, { dataTransfer: transfer });
    fireEvent.drop(archive, { dataTransfer: transfer });

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]).toMatchObject({ action: { type: "move", target: 7 }, messages: [{ mailboxId: 1, messageId: items[3]?.id }] });
  });
});

describe("move to folder", () => {
  it("V opens a folder list with a filter; Enter moves there", async () => {
    const { core, user, items } = setup();
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("v");

    const picker = await screen.findByRole("dialog", { name: "Move message to" });
    const field = within(picker).getByRole("combobox", { name: "Filter folders" });
    expect(document.activeElement).toBe(field);
    const options = within(picker).getAllByRole("option").map((o) => o.textContent);
    // Every other folder, roles first; not the inbox it is in.
    expect(options).toEqual(["Sent", "Drafts", "Archive", "Trash", "Junk", "Projekte", "2026Projekte/2026"]);

    await user.type(field, "proj");
    expect(within(picker).getAllByRole("option").map((o) => o.textContent)).toEqual(["Projekte", "2026Projekte/2026"]);
    await user.keyboard("{ArrowDown}{Enter}");

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]).toMatchObject({ action: { type: "move", target: 6 }, messages: [{ messageId: items[0]?.id }] });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect((await screen.findByTestId("notice")).textContent).toContain("Moved to 2026");
    expect(document.activeElement).toBe(listbox);
  });

  it("Escape closes the list without moving and gives the focus back", async () => {
    const { core, user } = setup();
    const { pane } = await openFirst(user);
    const button = within(within(pane).getByRole("toolbar")).getByRole("button", { name: "Move to folder" });
    await user.click(button);
    await screen.findByRole("dialog", { name: "Move message to" });

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(button);
    expect(actionCalls(core)).toHaveLength(0);
  });
});

describe("tags", () => {
  it("adds and removes tags, suggests the folder's existing ones, explains invalid characters at the field", async () => {
    const [first, second, ...rest] = makeMessages(5);
    const items = [
      { ...(first as MessageListItem), keywords: ["$Forwarded"] },
      { ...(second as MessageListItem), keywords: ["urgent", "$MDNSent"] },
      ...rest,
    ];
    const { core, user } = setup({ items });
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("l");

    const editor = await screen.findByRole("dialog", { name: "Tags of this message" });
    expect(within(editor).getByText("No tags yet.")).toBeTruthy();
    const suggestions = within(editor).getByRole("listbox", { name: "Existing tags" });
    expect(within(suggestions).getAllByRole("option").map((o) => o.textContent)).toEqual(["urgent"]);
    const field = within(editor).getByRole("combobox", { name: "Add a tag" });

    await user.type(field, "two words{Enter}");
    expect(field.getAttribute("aria-invalid")).toBe("true");
    const error = document.getElementById((field.getAttribute("aria-describedby") ?? "").split(" ").at(-1) ?? "");
    expect(error?.textContent).toContain("Tags cannot contain spaces");
    expect(actionCalls(core)).toHaveLength(0);

    await user.clear(field);
    await user.type(field, "project-x{Enter}");
    await waitFor(() => {
      expect(actionCalls(core).map((c) => c.action)).toEqual([{ type: "addKeyword", keyword: "project-x" }]);
    });
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });
    const chip = await within(editor).findByRole("button", { name: "Remove tag project-x" });
    expect((field as HTMLInputElement).value).toBe("");

    await user.click(chip);
    await waitFor(() => {
      expect(actionCalls(core).at(-1)?.action).toEqual({ type: "removeKeyword", keyword: "project-x" });
    });
  });

  it("the core's invalid_keyword answer shows at the field too", async () => {
    const { user } = setup({
      extra: {
        fail: (cmd, args) =>
          cmd === "apply_action" && (args["action"] as MailAction).type === "addKeyword" ? ipcError("invalid_keyword") : undefined,
      },
    });
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("l");
    const field = await screen.findByRole("combobox", { name: "Add a tag" });

    await user.type(field, "fine{Enter}");

    await waitFor(() => {
      expect(field.getAttribute("aria-invalid")).toBe("true");
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("a missing archive folder", () => {
  it("asks which folder to use; choosing one sets its role and archives", async () => {
    const { core, user, items } = setup({ archive: false });
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("e");

    const prompt = await screen.findByRole("region", { name: "No folder for “Archive”" });
    expect(prompt.textContent).toContain("No folder of Ada Lovelace is marked as “Archive”.");
    expect(document.activeElement).toBe(prompt);
    expect(within(listbox).getByText("Message 1")).toBeTruthy();
    const chooser = within(prompt).getByRole("combobox", { name: "Use an existing folder" });
    // Only ordinary folders: Sent or Trash do not become the archive by accident.
    expect(within(prompt).getAllByRole("option").map((o) => o.textContent)).toEqual(["Projekte", "2026Projekte/2026"]);

    await user.click(chooser);
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(core.callsOf("set_mailbox_role")).toEqual([{ mailboxId: 5, role: "archive" }]);
    });
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(2);
    });
    expect(actionCalls(core)[1]).toMatchObject({ action: { type: "archive" }, messages: [{ messageId: items[0]?.id }] });
    expect(screen.queryByRole("region", { name: "No folder for “Archive”" })).toBeNull();
    expect(core.messagesIn(5).map((m) => m.subject)).toEqual(["Message 1"]);
  });

  it("or creates one; the action runs once the server has it", async () => {
    const { core, user } = setup({ archive: false });
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("e");
    const prompt = await screen.findByRole("region", { name: "No folder for “Archive”" });
    expect(within(prompt).getByRole<HTMLInputElement>("textbox", { name: "Name of the new folder" }).value).toBe("Archive");

    await user.click(within(prompt).getByRole("button", { name: "Create and continue" }));

    await waitFor(() => {
      expect(core.callsOf("create_mailbox")).toEqual([{ accountId: ADA.id, path: "Archive", role: "archive" }]);
    });
    expect((await within(prompt).findByTestId("missing-creating")).textContent).toContain("Creating “Archive” on the server");
    expect(actionCalls(core)).toHaveLength(1);

    core.completeMailboxCreations();
    await emitCore({ type: "mailboxesChanged", accountId: ADA.id });

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(2);
    });
    expect(actionCalls(core)[1]?.action).toEqual({ type: "archive" });
    await waitFor(() => {
      expect(within(listbox).queryByText("Message 1")).toBeNull();
    });
  });

  it("Cancel leaves everything as it was", async () => {
    const { core, user } = setup({ archive: false });
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("e");
    const prompt = await screen.findByRole("region", { name: "No folder for “Archive”" });

    await user.click(within(prompt).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("region", { name: "No folder for “Archive”" })).toBeNull();
    expect(actionCalls(core)).toHaveLength(1);
    expect(document.activeElement).toBe(listbox);
  });
});

describe("deleting for good", () => {
  function trashSetup() {
    const trashed = makeMessages(3, { mailboxId: 4 });
    const result = setup({ items: [], others: { 4: trashed } });
    return { ...result, trashed };
  }

  it("in the trash asks in the one dialog: focus trapped, Escape cancels, focus returns", async () => {
    const { core, user } = trashSetup();
    await user.click(await screen.findByRole("button", { name: "Trash" }));
    const { pane } = await openFirst(user);
    const button = within(within(pane).getByRole("toolbar")).getByRole("button", { name: "Delete permanently" });

    await user.click(button);

    const dialog = await screen.findByRole("alertdialog", { name: "Delete this message permanently?" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(dialog.textContent).toContain("This cannot be undone.");
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    const confirm = within(dialog).getByRole("button", { name: "Delete permanently" });
    expect(document.activeElement).toBe(cancel);
    await user.tab();
    expect(document.activeElement).toBe(confirm);
    await user.tab();
    expect(document.activeElement).toBe(cancel);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(confirm);
    // The rest of the window is inert meanwhile.
    expect(screen.getByTestId("message-list").closest("[inert]")).toBeTruthy();

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(document.activeElement).toBe(button);
    expect(actionCalls(core)).toHaveLength(0);
  });

  it("Escape still cancels and the focus comes back when the dialog lost the focus", async () => {
    const { core, user } = trashSetup();
    await user.click(await screen.findByRole("button", { name: "Trash" }));
    const { pane } = await openFirst(user);
    await user.click(within(within(pane).getByRole("toolbar")).getByRole("button", { name: "Delete permanently" }));
    const dialog = await screen.findByRole("alertdialog");
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });

    // Focus moved out of the dialog (e.g. a re-render): it is pulled back in.
    (document.activeElement as HTMLElement | null)?.blur();
    document.body.focus();
    screen.getByTestId("message-list").dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(document.activeElement).toBe(cancel);

    // A key pressed while the page has the focus still reaches the dialog.
    (document.activeElement as HTMLElement | null)?.blur();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(actionCalls(core)).toHaveLength(0);
  });

  it("confirming deletes for good, without undo, and moves on to the next mail", async () => {
    const { core, user, trashed } = trashSetup();
    await user.click(await screen.findByRole("button", { name: "Trash" }));
    const { listbox } = await openFirst(user);
    listbox.focus();

    await user.keyboard("#");
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));

    await waitFor(() => {
      expect(actionCalls(core)).toEqual([
        expect.objectContaining({ action: { type: "deletePermanently" }, messages: [{ mailboxId: 4, messageId: trashed[0]?.id }] }),
      ]);
    });
    const notice = await screen.findByTestId("notice");
    expect(notice.textContent).toContain("Deleted permanently");
    expect(within(notice).queryByRole("button", { name: /Undo/ })).toBeNull();
    await waitFor(() => {
      expect(cursorRow(listbox)?.getAttribute("aria-label")).toContain(", Message 2,");
    });
    expect(document.activeElement).toBe(listbox);
  });

  it("the core's confirmation_required leads to the same question", async () => {
    const { core, user } = setup({
      extra: {
        fail: (cmd, args) =>
          cmd === "apply_action" && (args["action"] as MailAction).type === "delete" ? ipcError("confirmation_required") : undefined,
      },
    });
    const { listbox } = await openFirst(user);
    listbox.focus();
    await user.keyboard("#");

    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));

    await waitFor(() => {
      expect(actionCalls(core).map((c) => c.action.type)).toEqual(["delete", "deletePermanently"]);
    });
  });
});

describe("marking as read when opened", () => {
  const unreadItems = () => makeMessages(4).map((item) => ({ ...item, unread: true }));

  it("right away: an ordinary markRead, without an undo notice", async () => {
    const { core, user, items } = setup({ items: unreadItems(), extra: { readingPreferences: { markReadOnOpen: "immediately" } } });
    await openFirst(user);

    await waitFor(() => {
      expect(actionCalls(core)).toEqual([expect.objectContaining({ action: { type: "markRead" }, messages: [{ mailboxId: 1, messageId: items[0]?.id }] })]);
    });
    expect(screen.queryByTestId("notice")).toBeNull();
  });

  it("after two seconds, and not if another message is opened before", async () => {
    const { core, user, items } = setup({ items: unreadItems(), extra: { readingPreferences: { markReadOnOpen: "afterDelay" } } });
    const { listbox } = await openFirst(user);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });
    expect(actionCalls(core)).toHaveLength(0);

    await user.click(row(listbox, "Message 2"));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });
    // 2.4 s since the first was opened, but it was left after 1.2 s.
    expect(actionCalls(core)).toHaveLength(0);

    await waitFor(
      () => {
        expect(actionCalls(core)).toEqual([expect.objectContaining({ messages: [{ mailboxId: 1, messageId: items[1]?.id }] })]);
      },
      { timeout: 2000 },
    );
  });

  it("never: nothing is marked", async () => {
    const { core, user } = setup({ items: unreadItems(), extra: { readingPreferences: { markReadOnOpen: "never" } } });
    await openFirst(user);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 2300));
    });
    expect(actionCalls(core)).toHaveLength(0);
  });

  it("a message marked unread while it is open stays unread", async () => {
    const { core, user } = setup({ items: unreadItems(), extra: { readingPreferences: { markReadOnOpen: "immediately" } } });
    const { listbox } = await openFirst(user);
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });
    listbox.focus();

    await waitFor(() => {
      expect(row(listbox, "Message 1").getAttribute("aria-label")).not.toContain("Unread");
    });

    await user.keyboard("{Shift>}U{/Shift}");
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(2);
    });
    await emitCore({ type: "messagesChanged", accountId: ADA.id, mailboxId: 1 });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    expect(actionCalls(core).map((c) => c.action.type)).toEqual(["markRead", "markUnread"]);
  });

  it("is a setting under Reading with three choices", async () => {
    const core = mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Reading" }));

    const group = await screen.findByRole("radiogroup", { name: "Mark as read when opened" });
    expect(within(group).getAllByRole("radio").map((r) => r.textContent)).toEqual(["Right away", "After 2 seconds", "Never"]);
    expect(within(group).getByRole("radio", { name: "After 2 seconds" }).getAttribute("aria-checked")).toBe("true");

    await user.click(within(group).getByRole("radio", { name: "Never" }));

    expect(core.callsOf("set_reading_preferences")).toEqual([{ preferences: { markReadOnOpen: "never" } }]);
    expect(core.readingPreferences).toEqual({ markReadOnOpen: "never" });
  });
});

describe("every action from every surface", () => {
  it.each<[string, MailAction["type"]]>([
    ["quick-delete", "delete"],
    ["quick-archive", "archive"],
    ["quick-toggleRead", "markRead"],
    ["quick-toggleFlag", "flag"],
  ])("row quick action %s sends %s", async (testId, type) => {
    const { core, items } = setup();
    const listbox = await list();
    // Message 1 is unread and not flagged.
    fireEvent.click(within(row(listbox, "Message 1")).getByTestId(testId));
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]).toMatchObject({ action: { type }, messages: [{ mailboxId: 1, messageId: items[0]?.id }] });
  });

  it("the row's move quick action opens the folder list for that row", async () => {
    setup();
    const listbox = await list();
    fireEvent.click(within(row(listbox, "Message 2")).getByTestId("quick-move"));
    expect(await screen.findByRole("dialog", { name: "Move message to" })).toBeTruthy();
  });

  it.each<[string, MailAction["type"]]>([
    ["Archive", "archive"],
    ["Delete", "delete"],
    ["Mark as read", "markRead"],
    ["Flag", "flag"],
    ["Mark as junk", "spam"],
  ])("toolbar button %s sends %s", async (name, type) => {
    const { core, user, items } = setup();
    const { pane } = await openFirst(user);
    await user.click(within(within(pane).getByRole("toolbar")).getByRole("button", { name }));
    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    expect(actionCalls(core)[0]).toMatchObject({ action: { type }, messages: [{ messageId: items[0]?.id }] });
  });

  it("the toolbar's Tags button opens the tag editor", async () => {
    const { user } = setup();
    const { pane } = await openFirst(user);
    await user.click(within(within(pane).getByRole("toolbar")).getByRole("button", { name: "Tags" }));
    expect(await screen.findByRole("dialog", { name: "Tags of this message" })).toBeTruthy();
  });

  it("in the junk folder the toolbar and ! say “Not junk” and move to the inbox", async () => {
    const junk = makeMessages(2, { mailboxId: 8 });
    const { core, user } = setup({ items: [], others: { 8: junk } });
    await user.click(await screen.findByRole("button", { name: "Folders" }));
    await user.click(await screen.findByTestId("mailbox-8"));
    const { pane, listbox } = await openFirst(user);
    const toolbar = within(pane).getByRole("toolbar");
    expect(within(toolbar).queryByRole("button", { name: "Mark as junk" })).toBeNull();
    expect(within(toolbar).getByRole("button", { name: "Not junk" })).toBeTruthy();
    listbox.focus();

    await user.keyboard("!");

    await waitFor(() => {
      expect(actionCalls(core).map((c) => c.action)).toEqual([{ type: "notSpam" }]);
    });
    expect(core.messagesIn(1).map((m) => m.subject)).toEqual(["Message 1"]);
    await waitFor(() => {
      expect(within(listbox).queryByText("Message 1")).toBeNull();
    });
  });

  it("notices speak German with German numbers", async () => {
    const items = makeMessages(3);
    const core = mockCore({
      accounts: [ADA],
      mailboxes: mailboxes(),
      messages: { 1: items },
      readingPreferences: { markReadOnOpen: "never" },
      preferences: { theme: "light", language: "de" },
    });
    const { user } = renderApp("de");
    const listbox = await screen.findByRole("listbox", { name: "Nachrichten in Posteingang" });
    await user.click(row(listbox, "Message 1"));
    await user.keyboard("{Shift>}{ArrowDown}{/Shift}e");

    await waitFor(() => {
      expect(actionCalls(core)).toHaveLength(1);
    });
    const notice = await screen.findByTestId("notice");
    expect(notice.textContent).toContain("2 Mails archiviert");
    expect(within(notice).getByRole("button", { name: /Rückgängig/ })).toBeTruthy();
  });
});
