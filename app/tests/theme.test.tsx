import { act, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { mockCore } from "./support/core";
import { renderApp, rootTheme } from "./support/render";
import { setSystemDark } from "./support/system";

async function openAppearance(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Appearance" }));
}

describe("theme", () => {
  it("the switch sets dark and light at once and saves the choice through the core", async () => {
    const core = mockCore({ preferences: { theme: "light", language: "en" } });
    const { user } = renderApp();
    await openAppearance(user);
    expect(rootTheme()).toBe("light");

    await user.click(screen.getByRole("radio", { name: "Dark" }));
    expect(rootTheme()).toBe("dark");
    expect(core.preferences.theme).toBe("dark");

    await user.click(screen.getByRole("radio", { name: "Light" }));
    expect(rootTheme()).toBe("light");
    expect(core.preferences.theme).toBe("light");
  });

  it("'System' follows the operating system, also when it changes while the app runs", async () => {
    setSystemDark(true);
    mockCore({ preferences: { theme: "system", language: "en" } });
    renderApp();
    await screen.findByRole("main");
    expect(rootTheme()).toBe("dark");

    act(() => {
      setSystemDark(false);
    });
    expect(rootTheme()).toBe("light");
  });

  it("an explicit choice ignores the system setting", async () => {
    mockCore({ preferences: { theme: "dark", language: "en" } });
    renderApp();
    await screen.findByRole("main");

    act(() => {
      setSystemDark(false);
    });
    expect(rootTheme()).toBe("dark");
  });

  it("starts with the defaults and shows the error when the preferences cannot be loaded", async () => {
    setSystemDark(true);
    mockCore({ failures: { get_preferences: "command get_preferences not allowed by ACL" } });
    const { user } = renderApp();
    await openAppearance(user);

    expect(rootTheme()).toBe("dark");
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Your settings could not be loaded.");
    expect(alert.textContent).toContain("Something went wrong inside the app.");
    expect(screen.getByRole("radio", { name: "System" }).getAttribute("aria-checked")).toBe("true");
  });
});
