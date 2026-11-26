import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanRecent, overridesFromCore, overridesToCore } from "../src/features/keyboard/uiPreferences";
import type { CoreWindowLayout, UiPreferences } from "../src/services/ipc";
import { useLayoutStore } from "../src/stores/layout";
import { usePaletteStore } from "../src/stores/palette";
import { boundShortcuts, useShortcutStore } from "../src/stores/shortcuts";
import { useUiStore } from "../src/stores/ui";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes, type Failure } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const NONE: UiPreferences = { shortcuts: { macos: [], windows: [] }, textSize: 100, recentCommands: [], aiPanel: "open", accentTheme: "blue", conversations: false, columnViews: [] };

function setup(options: { preferences?: UiPreferences; layout?: CoreWindowLayout; failures?: Record<string, Failure> } = {}) {
  const items = makeMessages(4);
  const core = mockCore({
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    ...(options.failures && { failures: options.failures }),
    ui: { ...(options.preferences && { preferences: options.preferences }), ...(options.layout && { layout: options.layout }) },
  });
  return { core, items, ...renderApp() };
}

async function openKeyboardSettings() {
  act(() => {
    useUiStore.getState().showSettings("keyboard");
  });
  return screen.findByRole("heading", { name: "Keyboard shortcuts" });
}

const local = (key: string, value: unknown) => {
  window.localStorage.setItem(`vice-mail.ui.${key}`, JSON.stringify(value));
};
const stored = (key: string) => window.localStorage.getItem(`vice-mail.ui.${key}`);

describe("UI preferences in the core (stage 8, decision 0025)", () => {
  it("converts the core's lists to the UI's map and back, keeping commands of a newer version", () => {
    const core = {
      macos: [
        { command: "archive", shortcuts: [{ key: "y", shift: false, mod: false }] },
        { command: "futureCommand", shortcuts: [{ key: "q", shift: true, mod: false }] },
      ],
      windows: [{ command: "reply", shortcuts: [] }],
    };
    const ui = overridesFromCore(core);
    expect(ui).toEqual({ macos: { archive: [{ key: "y" }] }, windows: { reply: [] } });
    const back = overridesToCore({ ...ui, macos: { ...ui.macos, delete: [{ key: "d", mod: true }] } }, core);
    expect(back.macos).toEqual([
      { command: "archive", shortcuts: [{ key: "y", shift: false, mod: false }] },
      { command: "delete", shortcuts: [{ key: "d", shift: false, mod: true }] },
      { command: "futureCommand", shortcuts: [{ key: "q", shift: true, mod: false }] },
    ]);
    expect(back.windows).toEqual([{ command: "reply", shortcuts: [] }]);
    expect(cleanRecent(["a", "b", "a", "", "x".repeat(201), "c", "d", "e", "f"])).toEqual(["a", "b", "c", "d", "e"]);
  });

  it("starts with the shortcuts, text size, recent commands and layout the core has", async () => {
    const { user } = setup({
      preferences: {
        shortcuts: { macos: [{ command: "archive", shortcuts: [{ key: "y", shift: false, mod: false }] }], windows: [] },
        textSize: 130,
        recentCommands: ["action:compose"],
        aiPanel: "open",
        accentTheme: "orange",
        conversations: false,
        columnViews: [],
      },
      layout: { readingPane: "bottom", density: "compact", sidebarExpanded: true, sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true },
    });
    // Unfolded: the accounts-and-folders column instead of the rail.
    await screen.findByRole("navigation", { name: "Accounts and folders" });
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "y" }]);
    expect(document.documentElement.dataset["textSize"]).toBe("130");
    expect(document.documentElement.dataset["density"]).toBe("compact");
    expect(useLayoutStore.getState().readingPane).toBe("bottom");
    expect(useUiStore.getState().sidebarExpanded).toBe(true);
    expect(usePaletteStore.getState().recent).toEqual(["action:compose"]);
    await user.keyboard("{Meta>}k{/Meta}");
    const first = within(await screen.findByRole("dialog", { name: "Command palette" })).getAllByRole("option")[0];
    expect(first?.textContent).toContain("New message");
  });

  it("saves a change optimistically and puts the previous value back when the core refuses it", async () => {
    const { core, user } = setup({ failures: { set_ui_preferences: ipcError("invalid_argument", "macos: shortcut 0: key longer than 20 characters") } });
    await openKeyboardSettings();
    const row = screen.getByTestId("shortcut-row-archive");
    await user.click(within(row).getByRole("button", { name: "Add shortcut for Archive" }));
    await user.keyboard("y");
    // The core refused: the standard assignment applies again and the reason is shown.
    const notice = await screen.findByTestId("shortcut-save-error");
    expect(notice.textContent).toContain("The change could not be saved.");
    expect(notice.textContent).toContain(ipcErrorTitle("invalid_argument"));
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "e" }]);
    expect(core.callsOf("set_ui_preferences")).toHaveLength(1);
    expect(core.ui.preferences).toEqual(NONE);
  });

  it("sends the whole object and keeps the parts it does not change", async () => {
    const { core, user } = setup({ preferences: { ...NONE, textSize: 115, recentCommands: ["action:reply"] } });
    await openKeyboardSettings();
    await user.click(within(screen.getByTestId("shortcut-row-archive")).getByRole("button", { name: "Add shortcut for Archive" }));
    await user.keyboard("y");
    await waitFor(() => {
      expect(core.callsOf("set_ui_preferences")).toHaveLength(1);
    });
    expect(core.callsOf("set_ui_preferences")[0]).toEqual({
      preferences: {
        shortcuts: { macos: [{ command: "archive", shortcuts: [{ key: "e", shift: false, mod: false }, { key: "y", shift: false, mod: false }] }], windows: [] },
        textSize: 115,
        recentCommands: ["action:reply"],
        aiPanel: "open",
        accentTheme: "blue",
        conversations: false,
        columnViews: [],
      },
    });
  });

  it("applies what another window or device saved (uiPreferencesChanged)", async () => {
    setup();
    await screen.findByRole("navigation", { name: "Views" });
    await emitCore({
      type: "uiPreferencesChanged",
      preferences: {
        shortcuts: { macos: [{ command: "reply", shortcuts: [{ key: "R", shift: true, mod: false }] }], windows: [] },
        textSize: 90,
        recentCommands: ["view:inbox"],
        aiPanel: "open",
        accentTheme: "orange",
        conversations: false,
        columnViews: [],
      },
    });
    await waitFor(() => {
      expect(boundShortcuts("reply", "macos")).toEqual([{ key: "R", shift: true }]);
    });
    expect(document.documentElement.dataset["textSize"]).toBe("90");
    expect(usePaletteStore.getState().recent).toEqual(["view:inbox"]);
  });

  it("stores the recently used palette commands in the core", async () => {
    const { core, user } = setup();
    await screen.findByRole("navigation", { name: "Views" });
    await user.keyboard("{Meta>}k{/Meta}");
    await user.keyboard("New message");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(core.ui.preferences.recentCommands).toHaveLength(1);
    });
    expect(core.ui.preferences.recentCommands).toEqual(usePaletteStore.getState().recent);
  });

  it("shows a load error with the reason and loads again on request", async () => {
    let fail = true;
    const items = makeMessages(1);
    mockCore({
      accounts: [makeAccount()],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      fail: (cmd) => (cmd === "get_ui_preferences" && fail ? ipcError("database") : undefined),
    });
    const { user } = renderApp();
    await openKeyboardSettings();
    const notice = await screen.findByTestId("shortcut-load-error");
    expect(notice.textContent).toContain("Your shortcuts could not be loaded.");
    expect(notice.textContent).toContain(ipcErrorTitle("database"));
    fail = false;
    await user.click(within(notice).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(screen.queryByTestId("shortcut-load-error")).toBeNull();
    });
  });

  it("puts the layout back and says why when the core refuses it", async () => {
    const { user } = setup({ failures: { set_window_layout: ipcError("invalid_argument", "a window without layout") } });
    act(() => {
      useUiStore.getState().showSettings("appearance");
    });
    const radios = await screen.findByRole("radiogroup", { name: "Reading pane" });
    await user.click(within(radios).getByRole("radio", { name: "Below" }));
    const notice = await screen.findByTestId("layout-save-error");
    expect(notice.textContent).toContain("The layout could not be saved.");
    await waitFor(() => {
      expect(useLayoutStore.getState().readingPane).toBe("right");
    });
  });
});

describe("moving the old local fallback to the core, once", () => {
  it("writes the local values to a core that has none, then removes them", async () => {
    local("shortcuts", { macos: { archive: [{ key: "y" }] }, windows: {} });
    local("paletteRecent", ["action:compose", "view:sent"]);
    local("layout.main", { readingPane: "bottom", density: "compact", textSize: 130, sidebarExpanded: true });
    const { core } = setup();
    await waitFor(() => {
      expect(stored("shortcuts")).toBeNull();
    });
    expect(core.ui.preferences).toEqual({
      shortcuts: { macos: [{ command: "archive", shortcuts: [{ key: "y", shift: false, mod: false }] }], windows: [] },
      textSize: 130,
      recentCommands: ["action:compose", "view:sent"],
      aiPanel: "open",
      accentTheme: "blue",
      conversations: false,
      columnViews: [],
    });
    expect(core.ui.layout).toEqual({ readingPane: "bottom", density: "compact", sidebarExpanded: true, sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: false });
    expect(stored("paletteRecent")).toBeNull();
    expect(stored("layout.main")).toBeNull();
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "y" }]);
    expect(useLayoutStore.getState()).toMatchObject({ readingPane: "bottom", density: "compact", textSize: 130 });
    expect(core.callsOf("set_ui_preferences")).toHaveLength(1);
  });

  it("leaves the core's values alone when it has some and still removes the local copy", async () => {
    local("shortcuts", { macos: { archive: [{ key: "y" }] }, windows: {} });
    local("layout.main", { readingPane: "bottom", density: "compact", textSize: 130, sidebarExpanded: false });
    const { core } = setup({
      preferences: { ...NONE, textSize: 90 },
      layout: { readingPane: "replace", density: "comfortable", sidebarExpanded: false, sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true },
    });
    await waitFor(() => {
      expect(stored("shortcuts")).toBeNull();
    });
    expect(stored("layout.main")).toBeNull();
    expect(core.callsOf("set_ui_preferences")).toHaveLength(0);
    expect(core.callsOf("set_window_layout")).toHaveLength(0);
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "e" }]);
  });

  it("keeps the local copy for the next start when the core refuses it, and says so", async () => {
    local("shortcuts", { macos: { archive: [{ key: "y" }] }, windows: {} });
    setup({ failures: { set_ui_preferences: ipcError("database") } });
    await openKeyboardSettings();
    const notice = await screen.findByTestId("shortcut-save-error");
    expect(notice.textContent).toContain(ipcErrorTitle("database"));
    expect(stored("shortcuts")).not.toBeNull();
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "e" }]);
  });

  it("does not migrate in a message or composer window", async () => {
    local("shortcuts", { macos: { archive: [{ key: "y" }] }, windows: {} });
    window.history.replaceState(null, "", "/?window=composer&draftId=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    const core = mockCore({ accounts: [makeAccount()] });
    renderApp();
    await waitFor(() => {
      expect(core.callsOf("get_window_layout")).toHaveLength(1);
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(core.callsOf("set_ui_preferences")).toHaveLength(0);
    expect(stored("shortcuts")).not.toBeNull();
    expect(useShortcutStore.getState().saveError).toBeNull();
  });
});

function ipcErrorTitle(code: "invalid_argument" | "database"): string {
  return {
    invalid_argument: "A value was not accepted.",
    database: "The local data could not be read or written.",
  }[code];
}
