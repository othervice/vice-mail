// Stage 16: accent themes (Settings → Appearance, per device until the core stores them) and the
// notification sounds (Settings → Notifications, from the core's fixed list of this system).
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ACCENT_THEMES, readStoredAccent, useAccentStore } from "../src/features/preferences/accent";
import type { NotificationSettings, ThemePreference } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, mockCore, standardMailboxes } from "./support/core";
import { focusRingReport } from "./support/focusRing";
import { WINDOWS_SOUNDS } from "./support/paritySettings";
import { renderApp } from "./support/render";

const STORAGE_KEY = "vice-mail.ui.accentTheme";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

afterEach(() => {
  window.localStorage.clear();
  act(() => {
    useAccentStore.setState({ accent: "teal", stored: true });
  });
  delete document.documentElement.dataset["accent"];
});

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.help}`);
}

function openSettings(section: "appearance" | "notifications") {
  act(() => {
    useUiStore.getState().showSettings(section);
  });
}

describe("accent themes", () => {
  it("offers six themes by name, the default marked, and applies a choice at once", async () => {
    mockCore();
    const { user } = renderApp();
    openSettings("appearance");

    const group = await screen.findByRole("radiogroup", { name: "Accent colour" });
    const radios = within(group).getAllByRole("radio");
    expect(radios.map((radio) => radio.textContent)).toEqual(["Teal (default)", "Blue", "Violet", "Rose", "Orange", "Green"]);
    expect(within(group).getByRole("radio", { name: "Teal (default)" }).getAttribute("aria-checked")).toBe("true");
    expect(document.documentElement.dataset["accent"]).toBeUndefined();

    await user.click(within(group).getByRole("radio", { name: "Orange" }));
    expect(document.documentElement.dataset["accent"]).toBe("orange");
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe('"orange"');
    expect(within(group).getByRole("radio", { name: "Orange" }).getAttribute("aria-checked")).toBe("true");

    // Back to the default: the attribute goes, the tokens' own values apply again.
    await user.click(within(group).getByRole("radio", { name: "Teal (default)" }));
    expect(document.documentElement.dataset["accent"]).toBeUndefined();
    // Says honestly that it is not synced yet.
    expect(screen.getByTestId("accent-device-note").textContent).toContain("not synced");
  });

  it("each swatch carries its own theme, so it shows that colour in the current mode", async () => {
    mockCore();
    renderApp();
    openSettings("appearance");
    for (const theme of ACCENT_THEMES) {
      const radio = await screen.findByTestId(`accent-${theme}`);
      expect(radio.querySelector(`[data-accent="${theme}"]`)).not.toBeNull();
    }
  });

  it("arrow keys move between the themes, Space picks one", async () => {
    mockCore();
    const { user } = renderApp();
    openSettings("appearance");
    const teal = await screen.findByRole("radio", { name: "Teal (default)" });
    act(() => {
      teal.focus();
    });
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toBe(screen.getByRole("radio", { name: "Blue" }));
    await user.keyboard(" ");
    expect(document.documentElement.dataset["accent"]).toBe("blue");
  });

  it("reads a stored choice back, ignores anything invalid, and follows another window", () => {
    window.localStorage.setItem(STORAGE_KEY, '"violet"');
    expect(readStoredAccent()).toBe("violet");
    window.localStorage.setItem(STORAGE_KEY, '"magenta"');
    expect(readStoredAccent()).toBe("teal");
    window.localStorage.setItem(STORAGE_KEY, "{broken");
    expect(readStoredAccent()).toBe("teal");

    mockCore();
    renderApp();
    window.localStorage.setItem(STORAGE_KEY, '"green"');
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: STORAGE_KEY }));
    });
    expect(document.documentElement.dataset["accent"]).toBe("green");
  });

  it("says so when this computer cannot store the choice", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    mockCore();
    const { user } = renderApp();
    openSettings("appearance");
    await user.click(await screen.findByRole("radio", { name: "Rose" }));
    expect(document.documentElement.dataset["accent"]).toBe("rose");
    expect(screen.getByTestId("accent-not-stored").textContent).toContain("until you quit");
  });

  describe.each(["light", "dark"] as const)("%s mode", (theme: ThemePreference) => {
    it.each(ACCENT_THEMES)("Appearance in the %s theme has no serious axe findings and focus rings", async (accent) => {
      mockCore({ preferences: { theme, language: "system" } });
      renderApp();
      act(() => {
        useAccentStore.getState().setAccent(accent);
      });
      openSettings("appearance");
      await screen.findByRole("radiogroup", { name: "Accent colour" });
      await waitFor(() => {
        expect(document.documentElement.dataset["theme"]).toBe(theme);
      });
      expect(await seriousFindings()).toEqual([]);
      const report = await focusRingReport(screen.getByTestId("accent-picker"));
      expect(report.findings).toEqual([]);
      expect(report.checked).toBeGreaterThanOrEqual(1);
    });
  });
});

const ADA = makeAccount({ writesEnabled: true });
const BOB = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "bob@vicemail.test", displayName: "Bob", color: 2 });

function notificationsSetup(options: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({ accounts: [ADA, BOB], mailboxes: standardMailboxes(), ...options });
  const rendered = renderApp();
  openSettings("notifications");
  return { core, ...rendered };
}

const lastSound = (core: ReturnType<typeof mockCore>) =>
  (core.callsOf("set_notification_settings").at(-1)?.["settings"] as NotificationSettings | undefined)?.sound;

describe("notification sounds", () => {
  it("macOS: picks a system sound for this platform only and sends the other back unchanged", async () => {
    const { core, user } = notificationsSetup({
      comfort: {
        notificationSettings: { sound: { enabled: true, macos: "Default", windows: "Reminder", silentAccounts: [] } },
      },
    });
    const select = await screen.findByRole("combobox", { name: "Sound" });
    const options = within(select).getAllByRole("option").map((option) => option.textContent);
    expect(options[0]).toBe("System default");
    expect(options).toContain("Glass");
    expect(options).toContain("Submarine");

    await user.selectOptions(select, "Glass");
    await waitFor(() => {
      expect(lastSound(core)).toEqual({ enabled: true, macos: "Glass", windows: "Reminder", silentAccounts: [] });
    });
    // No preview: the system plays a sound only with a notification.
    expect(screen.queryByRole("button", { name: /play/i })).toBeNull();
  });

  it("Windows: the toast sounds get names of their own and only the Windows field changes", async () => {
    const { core, user } = notificationsSetup({ paritySettings: { soundChoices: WINDOWS_SOUNDS } });
    const select = await screen.findByRole("combobox", { name: "Sound" });
    expect(within(select).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "System default",
      "Instant message",
      "Mail",
      "Reminder",
      "SMS",
    ]);
    await user.selectOptions(select, "IM");
    await waitFor(() => {
      expect(lastSound(core)).toEqual({ enabled: true, macos: "Default", windows: "IM", silentAccounts: [] });
    });
  });

  it("offers no choice on a system without a list", async () => {
    notificationsSetup({ paritySettings: { soundChoices: { platform: "other", sounds: [] } } });
    expect(await screen.findByRole("switch", { name: "Play a sound" })).toBeTruthy();
    expect(screen.queryByRole("combobox", { name: "Sound" })).toBeNull();
  });

  it("turns sound off, and keeps single accounts silent apart from muted ones", async () => {
    const { core, user } = notificationsSetup({
      comfort: { notificationSettings: { mutedAccounts: [BOB.id] } },
    });
    const sound = await screen.findByTestId("notify-sound");
    const ada = await within(sound).findByRole("switch", { name: "Sound for Ada Lovelace (ada@vicemail.test)" });
    const bob = within(sound).getByRole("switch", { name: "Sound for Bob (bob@vicemail.test)" });
    // Bob's notifications are muted altogether: nothing to make silent.
    expect(bob.hasAttribute("disabled")).toBe(true);
    expect(within(sound).getByText("No notifications for this account.")).toBeTruthy();

    await user.click(ada);
    await waitFor(() => {
      expect(lastSound(core)?.silentAccounts).toEqual([ADA.id]);
    });
    await waitFor(() => {
      expect(within(sound).getByRole("switch", { name: "Sound for Ada Lovelace (ada@vicemail.test)" }).getAttribute("aria-checked")).toBe("false");
    });

    await user.click(within(sound).getByRole("switch", { name: "Play a sound" }));
    await waitFor(() => {
      expect(lastSound(core)?.enabled).toBe(false);
    });
    await waitFor(() => {
      expect(within(sound).getByRole("combobox", { name: "Sound" }).hasAttribute("disabled")).toBe(true);
    });
  });

  it("says it in German", async () => {
    mockCore({ preferences: { theme: "system", language: "de" }, paritySettings: { soundChoices: WINDOWS_SOUNDS } });
    renderApp("de");
    openSettings("notifications");
    const select = await screen.findByRole("combobox", { name: "Ton" });
    expect(within(select).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Systemstandard",
      "Sofortnachricht",
      "Mail",
      "Erinnerung",
      "SMS",
    ]);
  });

  describe.each(["light", "dark"] as const)("%s theme", (theme) => {
    it("has no serious axe findings and focus rings", async () => {
      notificationsSetup({ preferences: { theme, language: "system" } });
      const sound = await screen.findByTestId("notify-sound");
      await within(sound).findByRole("combobox", { name: "Sound" });
      expect(await seriousFindings()).toEqual([]);
      const report = await focusRingReport(sound);
      expect(report.findings).toEqual([]);
      expect(report.checked).toBeGreaterThanOrEqual(3);
    });
  });
});
