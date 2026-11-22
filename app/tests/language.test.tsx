import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { resolveLanguage } from "../src/i18n";
import { mockCore } from "./support/core";
import { renderApp } from "./support/render";
import { setSystemLanguages } from "./support/system";

describe("language", () => {
  it("switching to Deutsch applies at once, without reload, and is saved through the core", async () => {
    const core = mockCore({ preferences: { theme: "system", language: "en" } });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("radio", { name: "Deutsch" }));

    // Same render tree, new texts: header, tabs and rail are German right away.
    expect(await screen.findByRole("heading", { level: 1, name: "Einstellungen" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: "Allgemein" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Posteingang" })).toBeTruthy();
    expect(document.documentElement.lang).toBe("de");
    expect(core.preferences).toEqual({ theme: "system", language: "de" });
    expect(screen.getByRole("radio", { name: "Deutsch" }).getAttribute("aria-checked")).toBe("true");

    await user.click(screen.getByRole("radio", { name: "English" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Settings" })).toBeTruthy();
    expect(core.preferences.language).toBe("en");
  });

  it("'System' follows the system language: German when the first supported one is de-*", async () => {
    setSystemLanguages(["fr-FR", "de-CH", "en-US"]);
    mockCore({ preferences: { theme: "system", language: "system" } });
    renderApp("en");

    expect(await screen.findByRole("heading", { name: "Noch kein Konto" })).toBeTruthy();
  });

  it("resolves the system language with English as fallback", () => {
    expect(resolveLanguage("system", ["de-DE", "en"])).toBe("de");
    expect(resolveLanguage("system", ["en-GB", "de-DE"])).toBe("en");
    expect(resolveLanguage("system", ["fr-FR", "DE"])).toBe("de");
    expect(resolveLanguage("system", ["fr-FR", "es"])).toBe("en");
    expect(resolveLanguage("system", [])).toBe("en");
    expect(resolveLanguage("de", ["en-US"])).toBe("de");
  });

  it("rolls the language back and shows the error when the core cannot save it", async () => {
    mockCore({
      preferences: { theme: "system", language: "en" },
      failures: { set_preferences: { code: "database", message: "write failed", detail: "SQLITE_FULL" } },
    });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("radio", { name: "Deutsch" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Your change could not be saved.");
    expect(alert.textContent).toContain("SQLITE_FULL");
    expect(screen.getByRole("heading", { level: 1, name: "Settings" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "English" }).getAttribute("aria-checked")).toBe("true");
  });
});
