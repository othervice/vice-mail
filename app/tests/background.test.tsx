// Stage 21 (decisions 0069, 0070): running in the background (Settings → General), pausing
// notifications (Settings → Notifications, also the tray menu), the one-time note when the main
// window is first closed, the tray menu's texts and the main window's "ready" signal.
import { act, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PRODUCT_NAME } from "../src/config/product";
import type { ThemePreference } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { APP_INFO, mockCore } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.help}`);
}

function openSettings(section: "general" | "notifications") {
  act(() => {
    useUiStore.getState().showSettings(section);
  });
}

/** The shell asks the page to explain running in the background (first close of the window). */
async function closeRequested() {
  await act(async () => {
    await emit("close-to-background-asked", null);
  });
}

describe("Settings → General: in the background", () => {
  it("macOS: keep running, the menu-bar icon and starting with the system", async () => {
    const core = mockCore();
    const { user } = renderApp();
    openSettings("general");
    const section = await screen.findByTestId("background-settings");
    const keep = within(section).getByRole("switch", { name: "Keep running when the window is closed" });
    await waitFor(() => {
      expect(keep.getAttribute("aria-checked")).toBe("true");
    });
    expect(section.textContent).toContain(`${PRODUCT_NAME} stays in the Dock; quit it with ⌘Q`);
    const icon = within(section).getByRole("switch", { name: "Show the icon in the menu bar" });
    expect(icon.getAttribute("aria-checked")).toBe("true");
    const autostart = within(section).getByRole("switch", { name: "Start with the system" });
    await waitFor(() => {
      expect(autostart.hasAttribute("disabled")).toBe(false);
    });
    expect(autostart.getAttribute("aria-checked")).toBe("false");

    await user.click(keep);
    await waitFor(() => {
      expect(core.desktop.background.keepRunning).toBe(false);
    });
    expect(core.callsOf("set_background_settings").at(-1)).toEqual({ settings: { keepRunning: false, menuBarIcon: true } });
    await user.click(icon);
    await waitFor(() => {
      expect(core.desktop.background).toEqual({ keepRunning: false, menuBarIcon: false });
    });
    await user.click(autostart);
    await waitFor(() => {
      expect(autostart.getAttribute("aria-checked")).toBe("true");
    });
    expect(core.callsOf("set_autostart")).toEqual([{ enabled: true }]);
  });

  it("Windows: the icon in the notification area is always there, so a note instead of a switch", async () => {
    mockCore({ appInfo: { ...APP_INFO, platform: "windows" } });
    renderApp();
    openSettings("general");
    const section = await screen.findByTestId("background-settings");
    await within(section).findByTestId("background-tray-note");
    expect(within(section).queryByRole("switch", { name: "Show the icon in the menu bar" })).toBeNull();
    expect(section.textContent).toContain(`${PRODUCT_NAME} stays in the notification area`);
  });

  it("a development build cannot start with the system and says so; a refusal shows an error", async () => {
    mockCore({ desktop: { autostart: { available: false } } });
    renderApp();
    openSettings("general");
    const section = await screen.findByTestId("background-settings");
    await waitFor(() => {
      expect(section.textContent).toContain("Only the installed app can start with the system.");
    });
    expect(within(section).getByRole("switch", { name: "Start with the system" }).hasAttribute("disabled")).toBe(true);
  });

  it("the system refusing the login item is shown, and the switch stays as the system has it", async () => {
    mockCore({ desktop: { autostartFails: "internal" } });
    const { user } = renderApp();
    openSettings("general");
    const section = await screen.findByTestId("background-settings");
    const autostart = within(section).getByRole("switch", { name: "Start with the system" });
    await waitFor(() => {
      expect(autostart.hasAttribute("disabled")).toBe(false);
    });
    await user.click(autostart);
    await within(section).findByText(`Could not change whether ${PRODUCT_NAME} starts with the system.`);
    expect(autostart.getAttribute("aria-checked")).toBe("false");
  });
});

describe("Settings → Notifications: pause", () => {
  it("pauses for an hour, says until when, resumes, and follows the tray menu", async () => {
    const core = mockCore();
    const { user } = renderApp();
    openSettings("notifications");
    const pause = await screen.findByTestId("notification-pause");
    expect(within(pause).getByRole("heading", { name: "Pause notifications" })).toBeTruthy();
    expect(within(pause).queryByTestId("pause-state")).toBeNull();
    expect(within(pause).queryByRole("button", { name: "Resume" })).toBeNull();

    await user.click(within(pause).getByRole("button", { name: "For 1 hour" }));
    const state = await within(pause).findByTestId("pause-state");
    expect(core.callsOf("pause_notifications")).toEqual([{ choice: "oneHour" }]);
    const until = core.desktop.pause.until ?? 0;
    const time = new Intl.DateTimeFormat("en", { hour: "numeric", minute: "2-digit" }).format(until);
    expect(state.textContent).toBe(`Paused until ${time}`);
    expect(state.getAttribute("role")).toBe("status");

    await user.click(within(pause).getByRole("button", { name: "Resume" }));
    await waitFor(() => {
      expect(within(pause).queryByTestId("pause-state")).toBeNull();
    });

    // "Until tomorrow" chosen in the tray menu: the core's event updates the page.
    const tomorrow = new Date();
    tomorrow.setDate(tomorrow.getDate() + 1);
    tomorrow.setHours(8, 0, 0, 0);
    await emitCore({
      type: "notificationPauseChanged",
      pause: { until: tomorrow.getTime(), localTime: { hour: 8, minute: 0, laterDay: true } },
    });
    const later = await within(pause).findByTestId("pause-state");
    expect(later.textContent).toMatch(/^Paused until tomorrow, 8:00\sAM$/);
  });

  it("German: the same, in the 24-hour clock", async () => {
    mockCore({
      preferences: { theme: "system", language: "de" },
      desktop: { pause: { until: new Date(2030, 0, 2, 14, 30).getTime(), localTime: { hour: 14, minute: 30, laterDay: false } } },
    });
    renderApp();
    openSettings("notifications");
    const pause = await screen.findByTestId("notification-pause");
    const state = await within(pause).findByTestId("pause-state");
    expect(state.textContent).toBe("Pausiert bis 14:30");
    expect(within(pause).getByRole("button", { name: "Fortsetzen" })).toBeTruthy();
    expect(within(pause).getByRole("button", { name: "Bis morgen früh" })).toBeTruthy();
  });
});

describe("closing the main window the first time", () => {
  it("explains running in the background; Keep running closes into the background", async () => {
    const core = mockCore();
    const { user } = renderApp();
    await screen.findByTestId("rail-settings");
    await closeRequested();
    const dialog = await screen.findByRole("alertdialog", { name: `${PRODUCT_NAME} keeps running` });
    expect(dialog.textContent).toContain("To quit, press ⌘Q");
    expect(dialog.textContent).toContain("Settings → General");
    // The harmless choice has the focus: Cancel.
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Cancel" }));
    await user.click(within(dialog).getByRole("button", { name: "Keep running" }));
    await waitFor(() => {
      expect(core.desktop.closes).toEqual([false]);
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("Quit ends the app; Escape leaves the window open", async () => {
    const core = mockCore({ appInfo: { ...APP_INFO, platform: "windows" } });
    const { user } = renderApp();
    await screen.findByTestId("rail-settings");
    await closeRequested();
    let dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain("right-click that icon and choose “Quit”");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(core.desktop.closes).toEqual([]);

    await closeRequested();
    dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: `Quit ${PRODUCT_NAME}` }));
    await waitFor(() => {
      expect(core.desktop.closes).toEqual([true]);
    });
  });
});

describe("closing during the guided first start", () => {
  it("the note shows there too, so the window can always be closed", async () => {
    const core = mockCore({ firstStart: { pending: true } });
    const { user } = renderApp();
    await screen.findByTestId("first-start");
    await closeRequested();
    const dialog = await screen.findByRole("alertdialog", { name: `${PRODUCT_NAME} keeps running` });
    await user.click(within(dialog).getByRole("button", { name: "Keep running" }));
    await waitFor(() => {
      expect(core.desktop.closes).toEqual([false]);
    });
  });
});

describe("tray texts and the ready signal", () => {
  it("sends the tray menu's texts in the UI's language and says once that the page listens", async () => {
    const core = mockCore();
    renderApp();
    await screen.findByTestId("rail-settings");
    await waitFor(() => {
      expect(core.desktop.texts[0]?.quit).toBe("Quit {product}");
    });
    expect(core.desktop.texts[0]).toMatchObject({
      open: "Open {product}",
      tooltipUnread: "{product}: {count} unread",
      pausedUntil: "Paused until {time}",
    });
    await waitFor(() => {
      expect(core.desktop.readyCalls).toBe(1);
    });
  });

  it("German texts keep the placeholders the shell fills in", async () => {
    const core = mockCore({ preferences: { theme: "system", language: "de" } });
    renderApp();
    await screen.findByTestId("rail-settings");
    await waitFor(() => {
      expect(core.desktop.texts[0]?.quit).toBe("{product} beenden");
    });
    expect(core.desktop.texts[0]).toMatchObject({ newMessage: "Neue Nachricht", tooltipUnread: "{product}: {count} ungelesen", hour12: false });
  });
});

describe("accessibility in light and dark", () => {
  describe.each(["light", "dark"] as const)("%s mode", (theme: ThemePreference) => {
    it("General, Notifications and the close note have no serious axe findings", async () => {
      mockCore({
        preferences: { theme, language: "system" },
        desktop: { pause: { until: Date.now() + 3_600_000, localTime: { hour: 14, minute: 30, laterDay: false } } },
      });
      renderApp();
      openSettings("general");
      await screen.findByTestId("background-settings");
      await waitFor(() => {
        expect(document.documentElement.dataset["theme"]).toBe(theme);
      });
      expect(await seriousFindings()).toEqual([]);
      openSettings("notifications");
      await screen.findByTestId("pause-state");
      expect(await seriousFindings()).toEqual([]);
      await closeRequested();
      await screen.findByRole("alertdialog");
      expect(await seriousFindings()).toEqual([]);
    });
  });
});
