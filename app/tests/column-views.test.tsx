import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import {
  COLUMN_VIEW_IDS,
  columnViewsToCore,
  DEFAULT_COLUMN_VIEWS,
  isDefaultColumnViews,
  moveColumnView,
  resolveColumnViews,
} from "../src/features/layout/columnViews";
import type { ColumnView } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { ipcError, makeAccount, mockCore, standardMailboxes, type Failure } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";
import { DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

/*
 * Stage 17, block C: "Edit views…" chooses which views the column's "Views" section and the rail
 * show, and in which order. The choice is a UI preference of the core (`columnViews`), synced like
 * the text size; hidden views stay in the command palette and keep their shortcuts.
 */

const ADA = makeAccount({ writesEnabled: true });

type User = ReturnType<typeof renderApp>["user"];

interface SetupOptions {
  columnViews?: ColumnView[];
  sidebarExpanded?: boolean;
  failures?: Record<string, Failure>;
  language?: "en" | "de";
}

function setup({ columnViews = [], sidebarExpanded = true, failures, language = "en" }: SetupOptions = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [] },
    ui: {
      preferences: { ...DEFAULT_UI_PREFERENCES, columnViews },
      layout: { readingPane: "right", density: "comfortable", sidebarWidth: 240, sidebarExpanded },
    },
    ...(failures && { failures }),
  });
  return { core, ...renderApp(language) };
}

const all = (shown: Record<string, boolean> = {}, order: readonly string[] = COLUMN_VIEW_IDS): ColumnView[] =>
  order.map((id) => ({ id, shown: shown[id] ?? true }));

async function viewsTree(): Promise<HTMLElement> {
  const column = await screen.findByRole("navigation", { name: "Accounts and folders" });
  return within(column).getByRole("tree", { name: "Views" });
}

/** The views the column shows, top to bottom (the first row is the section's label). */
async function columnIds(): Promise<string[]> {
  return within(await viewsTree())
    .getAllByRole("treeitem")
    .slice(1)
    .map((row) => row.dataset["testid"]?.replace("sidebar-view-", "") ?? "?");
}

/** The narrow rail's views, top to bottom. */
function railIds(): string[] {
  return within(screen.getByRole("toolbar", { name: "Mail views" }))
    .getAllByRole("button")
    .map((button) => button.dataset["testid"]?.replace("rail-", "") ?? "?");
}

/** The editor's views, top to bottom, with a "-" before a hidden one. */
function editorIds(): string[] {
  return within(screen.getByTestId("edit-views-list"))
    .getAllByRole("checkbox")
    .map((box) => `${(box as HTMLInputElement).checked ? "" : "-"}${box.dataset["testid"]?.replace("edit-views-check-", "") ?? "?"}`);
}

const dialog = () => screen.getByRole("dialog", { name: "Edit views" });

async function openFromColumn(user: User): Promise<HTMLElement> {
  await viewsTree();
  await user.click(screen.getByTestId("sidebar-views-menu"));
  await user.click(await screen.findByRole("menuitem", { name: "Edit views…" }));
  return screen.findByRole("dialog", { name: "Edit views" });
}

async function openFromPalette(user: User): Promise<HTMLElement> {
  await user.keyboard("{Meta>}k{/Meta}");
  await screen.findByRole("dialog", { name: "Command palette" });
  await user.keyboard("edit views");
  await user.keyboard("{Enter}");
  return screen.findByRole("dialog", { name: "Edit views" });
}

/** What the core stores, once the last save arrived. */
async function storedViews(core: ReturnType<typeof mockCore>, expected: ColumnView[]): Promise<void> {
  await waitFor(() => {
    expect(core.ui.preferences.columnViews).toEqual(expected);
  });
}

/** axe-core as in tests/a11y.test.tsx (colour contrast: tests/contrast.test.ts). */
async function seriousFindings(): Promise<string[]> {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

describe("the column's views as the core stores them", () => {
  it("is the default order, all shown, while nothing is stored", () => {
    expect(resolveColumnViews([])).toEqual(DEFAULT_COLUMN_VIEWS);
    expect(DEFAULT_COLUMN_VIEWS.map((view) => view.id)).toEqual([
      "later",
      // Stage 20 (decisions 0059, 0060).
      "waiting",
      "tasks",
      "readingList",
      "pinnedAttachments",
      "flagged",
      "pinned",
      "savedViews",
      "contacts",
    ]);
    expect(isDefaultColumnViews(DEFAULT_COLUMN_VIEWS)).toBe(true);
    // The default is stored as no choice, so a view a later version adds shows at its place.
    expect(columnViewsToCore(DEFAULT_COLUMN_VIEWS, [])).toEqual([]);
  });

  it("keeps the user's order; a view the list does not name comes after the one it follows by default", () => {
    const stored = all({ later: false }, ["pinned", "later", "tasks", "readingList", "pinnedAttachments", "flagged"]);
    expect(resolveColumnViews(stored)).toEqual([
      { id: "pinned", shown: true },
      // Not stored (new in this version): after "Pinned", its place by default, and shown.
      { id: "savedViews", shown: true },
      // Stage 17 (decision 0050): after "Saved views", its place by default.
      { id: "contacts", shown: true },
      { id: "later", shown: false },
      // Stage 20: new views after the one they follow by default, shown.
      { id: "waiting", shown: true },
      { id: "tasks", shown: true },
      { id: "readingList", shown: true },
      { id: "pinnedAttachments", shown: true },
      { id: "flagged", shown: true },
    ]);
    // Unknown ids (a newer device) and an id twice are left out of what the UI shows.
    expect(resolveColumnViews([{ id: "fromTheFuture", shown: true }, { id: "tasks", shown: false }, { id: "tasks", shown: true }])[2]).toEqual({
      id: "tasks",
      shown: false,
    });
  });

  it("saves every view in order and keeps the entries of views a newer version knows", () => {
    const choice = resolveColumnViews(all({ flagged: false }));
    const previous = [{ id: "fromTheFuture", shown: false }, { id: "later", shown: true }];
    expect(columnViewsToCore(choice, previous)).toEqual([...all({ flagged: false }), { id: "fromTheFuture", shown: false }]);
    // The default with a foreign entry is still stored in full, so the foreign entry stays.
    expect(columnViewsToCore(DEFAULT_COLUMN_VIEWS, previous)).toEqual([...all(), { id: "fromTheFuture", shown: false }]);
    expect(moveColumnView(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(moveColumnView(["a", "b", "c"], 0, 3)).toEqual(["a", "b", "c"]);
  });
});

describe("Edit views…", () => {
  it("opens from the Views label's menu and from the palette; Escape closes it and gives the focus back", async () => {
    const { user } = setup();
    expect(await columnIds()).toEqual([...COLUMN_VIEW_IDS]);
    const opened = await openFromColumn(user);
    expect(within(opened).getByRole("list", { name: "Views" })).toBeTruthy();
    expect(editorIds()).toEqual([...COLUMN_VIEW_IDS]);
    // The focus starts on the first view; the hint tells how to move one with the keyboard.
    const first = within(opened).getByRole("checkbox", { name: "Later" });
    expect(document.activeElement).toBe(first);
    expect(first.getAttribute("aria-keyshortcuts")).toBe("Alt+ArrowUp Alt+ArrowDown");
    expect(document.getElementById(first.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
      "Drag a view by its handle, or press ⌥↑ or ⌥↓ to move the view that has the focus.",
    );
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Edit views" })).toBeNull();
    // Back on the label of "Views" (the menu's own row).
    expect(document.activeElement).toBe(screen.getByTestId("sidebar-section-views"));

    // Shift+F10 on the label opens the same menu.
    await user.keyboard("{Shift>}{F10}{/Shift}");
    expect(await screen.findByRole("menuitem", { name: "Edit views…" })).toBeTruthy();
    await user.keyboard("{Escape}");

    // Escape closes it also when the focus was lost to the page.
    await openFromPalette(user);
    act(() => {
      (document.activeElement as HTMLElement | null)?.blur();
    });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Edit views" })).toBeNull();

    await openFromPalette(user);
    // Tab stays inside: from the last control back to the first, and Shift+Tab the other way.
    act(() => {
      within(dialog()).getByRole("button", { name: "Done" }).focus();
    });
    await user.tab();
    expect(document.activeElement).toBe(within(dialog()).getByRole("button", { name: "Close" }));
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(within(dialog()).getByRole("button", { name: "Done" }));
    // A key inside does not reach the shortcuts of the view behind it ("2" would open Later).
    await user.keyboard("{Meta>}2{/Meta}");
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "inbox" });
    await user.click(within(dialog()).getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog", { name: "Edit views" })).toBeNull();
  });

  it("hides and shows views: the column and the rail follow at once, the core stores the choice", async () => {
    const { core, user } = setup();
    await openFromColumn(user);
    await user.click(within(dialog()).getByRole("checkbox", { name: "Later" }));
    await user.click(within(dialog()).getByRole("checkbox", { name: "Pinned" }));
    // The column behind the dialog is the preview.
    expect(await columnIds()).toEqual(["waiting", "tasks", "readingList", "pinnedAttachments", "flagged", "savedViews", "contacts"]);
    expect(editorIds()).toEqual([
      "-later",
      "waiting",
      "tasks",
      "readingList",
      "pinnedAttachments",
      "flagged",
      "-pinned",
      "savedViews",
      "contacts",
    ]);
    await storedViews(core, all({ later: false, pinned: false }));
    await user.click(within(dialog()).getByRole("checkbox", { name: "Later" }));
    expect(await columnIds()).toEqual(["later", "waiting", "tasks", "readingList", "pinnedAttachments", "flagged", "savedViews", "contacts"]);
    await storedViews(core, all({ pinned: false }));
    await user.click(within(dialog()).getByRole("button", { name: "Done" }));

    // Folded to the rail: the same choice between the inbox and Sent, the fixed entries as they were.
    act(() => {
      useUiStore.getState().toggleSidebar();
    });
    await waitFor(() => {
      expect(railIds()).toEqual([
        "inbox",
        "later",
        "waiting",
        "tasks",
        "readingList",
        "pinnedAttachments",
        "flagged",
        "savedViews",
        "contacts",
        "sent",
        "drafts",
        "archive",
        "trash",
        "folders",
      ]);
    });
  });

  it("moves a view with Alt+arrow keys; the focus stays on it and the new place is announced", async () => {
    const { core, user } = setup();
    await openFromColumn(user);
    const flagged = within(dialog()).getByRole("checkbox", { name: "Flagged" });
    act(() => {
      flagged.focus();
    });
    await user.keyboard("{Alt>}{ArrowUp}{ArrowUp}{ArrowUp}{ArrowUp}{ArrowUp}{/Alt}");
    expect(editorIds()).toEqual([
      "flagged",
      "later",
      "waiting",
      "tasks",
      "readingList",
      "pinnedAttachments",
      "pinned",
      "savedViews",
      "contacts",
    ]);
    expect(document.activeElement).toBe(within(dialog()).getByRole("checkbox", { name: "Flagged" }));
    expect(screen.getByTestId("edit-views-announcement").textContent).toBe("Flagged, position 1 of 9");
    // At the top it stays there.
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(editorIds()[0]).toBe("flagged");
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    expect(editorIds().slice(0, 2)).toEqual(["later", "flagged"]);
    const moved = ["later", "flagged", "waiting", "tasks", "readingList", "pinnedAttachments", "pinned", "savedViews", "contacts"];
    expect(await columnIds()).toEqual(moved);
    await storedViews(core, all({}, moved));
    // Space shows and hides the view with the focus; Tab goes on to the next view.
    await user.keyboard(" ");
    expect(editorIds()[1]).toBe("-flagged");
    await user.tab();
    expect(document.activeElement).toBe(within(dialog()).getByRole("checkbox", { name: "Waiting for reply" }));
  });

  it("moves a view with the arrow buttons and by dragging its handle, previewed in the column while it is dragged", async () => {
    const { core, user } = setup();
    await openFromColumn(user);
    await user.click(within(dialog()).getByRole("button", { name: "Move “Saved views” up" }));
    expect(editorIds().slice(-3)).toEqual(["savedViews", "pinned", "contacts"]);
    await storedViews(
      core,
      all({}, ["later", "waiting", "tasks", "readingList", "pinnedAttachments", "flagged", "savedViews", "pinned", "contacts"]),
    );
    expect(within(dialog()).getByRole<HTMLButtonElement>("button", { name: "Move “Later” up" }).disabled).toBe(true);
    expect(within(dialog()).getByRole<HTMLButtonElement>("button", { name: "Move “Contacts” down" }).disabled).toBe(true);

    // jsdom has no layout: every row of the editor is 40 px high, the first at the top.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const row = this.closest<HTMLElement>("[data-view-row]");
      const index = row?.parentElement ? [...row.parentElement.children].indexOf(row) : 0;
      return new DOMRect(0, index * 40, 320, 40);
    });
    const saves = core.callsOf("set_ui_preferences").length;
    fireEvent.pointerDown(screen.getByTestId("edit-views-handle-flagged"), { button: 0, clientY: 220 });
    expect(screen.getByTestId("edit-views-row-flagged").dataset["dragging"]).toBe("true");
    act(() => {
      window.dispatchEvent(new PointerEvent("pointermove", { clientY: 50 }));
    });
    // While dragged: in its new place in the editor and in the column, not saved yet.
    expect(editorIds().slice(0, 3)).toEqual(["later", "flagged", "waiting"]);
    expect((await columnIds()).slice(0, 3)).toEqual(["later", "flagged", "waiting"]);
    expect(core.callsOf("set_ui_preferences")).toHaveLength(saves);
    act(() => {
      window.dispatchEvent(new PointerEvent("pointermove", { clientY: -20 }));
    });
    expect(editorIds()[0]).toBe("flagged");
    act(() => {
      window.dispatchEvent(new PointerEvent("pointerup", { clientY: -20 }));
    });
    await storedViews(
      core,
      all({}, ["flagged", "later", "waiting", "tasks", "readingList", "pinnedAttachments", "savedViews", "pinned", "contacts"]),
    );
    expect(screen.getByTestId("edit-views-announcement").textContent).toBe("Flagged, position 1 of 9");
    expect(screen.getByTestId("edit-views-row-flagged").dataset["dragging"]).toBeUndefined();

    // A cancelled drag (the system took the pointer) puts the order back as it was.
    fireEvent.pointerDown(screen.getByTestId("edit-views-handle-pinned"), { button: 0, clientY: 300 });
    act(() => {
      window.dispatchEvent(new PointerEvent("pointermove", { clientY: 10 }));
    });
    expect(editorIds()[0]).toBe("pinned");
    act(() => {
      window.dispatchEvent(new PointerEvent("pointercancel"));
    });
    expect(editorIds()[0]).toBe("flagged");
    expect((await columnIds())[0]).toBe("flagged");
  });

  it("“Reset to default” shows every view again in the default order and stores no choice", async () => {
    const { core, user } = setup({ columnViews: all({ tasks: false, savedViews: false }, ["pinned", "later", "tasks", "readingList", "pinnedAttachments", "flagged", "savedViews"]) });
    expect(await columnIds()).toEqual(["pinned", "later", "waiting", "readingList", "pinnedAttachments", "flagged", "contacts"]);
    await openFromColumn(user);
    const reset = within(dialog()).getByRole("button", { name: "Reset to default" });
    await user.click(reset);
    expect(editorIds()).toEqual([...COLUMN_VIEW_IDS]);
    expect(await columnIds()).toEqual([...COLUMN_VIEW_IDS]);
    await storedViews(core, []);
    expect((reset as HTMLButtonElement).disabled).toBe(true);
    // The disabled button gives the focus on to "Done"; the reset is announced.
    expect(document.activeElement).toBe(within(dialog()).getByRole("button", { name: "Done" }));
    expect(screen.getByTestId("edit-views-announcement").textContent).toBe("Every view is shown again, in the default order.");
  });

  it("keeps the choice over a restart: the core holds it, the next start shows it", async () => {
    const first = setup();
    await openFromColumn(first.user);
    await first.user.click(within(dialog()).getByRole("checkbox", { name: "Reading list" }));
    await first.user.click(within(dialog()).getByRole("button", { name: "Move “Saved views” up" }));
    const expected = all({ readingList: false }, [
      "later",
      "waiting",
      "tasks",
      "readingList",
      "pinnedAttachments",
      "flagged",
      "savedViews",
      "pinned",
      "contacts",
    ]);
    await storedViews(first.core, expected);

    // A new start against what the core stored.
    cleanup();
    useUiStore.setState(useUiStore.getInitialState(), true);
    setup({ columnViews: [...first.core.ui.preferences.columnViews] });
    expect(await columnIds()).toEqual(["later", "waiting", "tasks", "pinnedAttachments", "flagged", "savedViews", "pinned", "contacts"]);
  });

  it("applies a choice made on another device (uiPreferencesChanged)", async () => {
    setup();
    expect(await columnIds()).toEqual([...COLUMN_VIEW_IDS]);
    await emitCore({
      type: "uiPreferencesChanged",
      preferences: { ...DEFAULT_UI_PREFERENCES, columnViews: all({ later: false }, ["savedViews", ...COLUMN_VIEW_IDS.filter((id) => id !== "savedViews")]) },
    });
    await waitFor(async () => {
      expect(await columnIds()).toEqual(["savedViews", "waiting", "tasks", "readingList", "pinnedAttachments", "flagged", "pinned", "contacts"]);
    });
  });

  it("shows the core's refusal in the editor and puts the previous choice back", async () => {
    const { core, user } = setup({ failures: { set_ui_preferences: ipcError("invalid_argument", "column view 0: empty id") } });
    await openFromColumn(user);
    await user.click(within(dialog()).getByRole("checkbox", { name: "Tasks" }));
    const error = await within(dialog()).findByTestId("edit-views-error");
    expect(error.textContent).toContain("The views could not be saved.");
    await waitFor(async () => {
      expect(await columnIds()).toEqual([...COLUMN_VIEW_IDS]);
    });
    expect(editorIds()).toEqual([...COLUMN_VIEW_IDS]);
    expect(core.ui.preferences.columnViews).toEqual([]);
  });
});

describe("hidden views", () => {
  it("stay reachable through the palette and their shortcuts", async () => {
    const { user } = setup({ columnViews: all({ later: false, flagged: false }) });
    expect(await columnIds()).toEqual(["waiting", "tasks", "readingList", "pinnedAttachments", "pinned", "savedViews", "contacts"]);
    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("flagged");
    expect(within(palette).getByRole("option", { name: /Go to Flagged/ })).toBeTruthy();
    await user.click(within(palette).getByRole("option", { name: /Go to Flagged/ }));
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "flagged" });
    // Cmd+2 opens Later as before, hidden or not.
    await user.keyboard("{Meta>}2{/Meta}");
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "later" });
  });

  it("every view hidden: a quiet hint with “Edit views…” instead of an empty band; the rail keeps its fixed entries", async () => {
    const { user } = setup({ columnViews: all(Object.fromEntries(COLUMN_VIEW_IDS.map((id) => [id, false]))) });
    const tree = await viewsTree();
    expect(within(tree).getAllByRole("treeitem")).toHaveLength(1);
    const hint = screen.getByTestId("sidebar-views-empty");
    expect(hint.textContent).toContain("No views shown.");
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(hint).getByRole("button", { name: "Edit views…" }));
    await user.click(within(dialog()).getByRole("checkbox", { name: "Tasks" }));
    expect(await columnIds()).toEqual(["tasks"]);
    expect(screen.queryByTestId("sidebar-views-empty")).toBeNull();
    await user.click(within(dialog()).getByRole("checkbox", { name: "Tasks" }));
    await user.click(within(dialog()).getByRole("button", { name: "Done" }));
    act(() => {
      useUiStore.getState().toggleSidebar();
    });
    await waitFor(() => {
      expect(railIds()).toEqual(["inbox", "sent", "drafts", "archive", "trash", "folders"]);
    });
  });

  it("a view's own menu hides it; the focus goes on to the next view, the hiding is announced", async () => {
    const { core, user } = setup();
    const tree = await viewsTree();
    await user.pointer({ keys: "[MouseRight]", target: within(tree).getByRole("treeitem", { name: "Tasks" }) });
    await user.click(await screen.findByRole("menuitem", { name: "Hide from the sidebar" }));
    expect(await columnIds()).toEqual(["later", "waiting", "readingList", "pinnedAttachments", "flagged", "pinned", "savedViews", "contacts"]);
    expect(document.activeElement).toBe(within(tree).getByRole("treeitem", { name: "Reading list" }));
    expect(within(tree.closest("section") ?? tree).getByText("Tasks hidden. “Edit views…” shows it again.")).toBeTruthy();
    await storedViews(core, all({ tasks: false }));
    // The same menu leads to the editor.
    await user.keyboard("{Shift>}{F10}{/Shift}");
    await user.click(await screen.findByRole("menuitem", { name: "Edit views…" }));
    expect(await screen.findByRole("dialog", { name: "Edit views" })).toBeTruthy();
  });
});

describe("Edit views…, accessibility and languages", () => {
  it("finds nothing serious with the editor open, a view hidden and one dragged", async () => {
    const { user } = setup();
    await openFromColumn(user);
    await user.click(within(dialog()).getByRole("checkbox", { name: "Pinned" }));
    expect(await seriousFindings()).toEqual([]);
    // A dialog with a name and a description; the list and every control are named.
    expect(dialog().getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById(dialog().getAttribute("aria-describedby") ?? "")?.textContent).toContain("Hidden views stay in the command palette");
    for (const button of within(dialog()).getAllByRole("button")) expect(button.getAttribute("aria-label") ?? button.textContent).toBeTruthy();
  });

  it("speaks German (informal) on Windows", async () => {
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [] },
      preferences: { theme: "light", language: "de" },
      appInfo: { version: "0.1.0", commit: null, buildDate: null, platform: "windows" },
      ui: { layout: { readingPane: "right", density: "comfortable", sidebarWidth: 240, sidebarExpanded: true } },
    });
    const { user } = renderApp("de");
    const column = await screen.findByRole("navigation", { name: "Konten und Ordner" });
    await user.click(within(column).getByRole("button", { name: "Optionen für Ansichten" }));
    await user.click(await screen.findByRole("menuitem", { name: "Ansichten bearbeiten …" }));
    const editor = await screen.findByRole("dialog", { name: "Ansichten bearbeiten" });
    expect(within(editor).getByRole("checkbox", { name: "Später" })).toBeTruthy();
    expect(within(editor).getByRole("button", { name: "„Später“ nach unten" })).toBeTruthy();
    expect(within(editor).getByText("Zieh eine Ansicht am Griff oder drück Alt+↑ bzw. Alt+↓, um die Ansicht mit dem Fokus zu verschieben.")).toBeTruthy();
    expect(within(editor).getByRole("button", { name: "Auf Standard zurücksetzen" })).toBeTruthy();
    expect(within(editor).getByRole("button", { name: "Fertig" })).toBeTruthy();
    expect(await seriousFindings()).toEqual([]);
  });
});
