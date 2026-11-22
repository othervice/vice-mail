import { act, screen } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { describe, expect, it } from "vitest";
import type { CoreEvent } from "../src/services/ipc";
import { mockCore } from "./support/core";
import { renderApp, rootTheme } from "./support/render";

describe("core events in the UI", () => {
  it("preferencesChanged from the core switches theme and language visibly", async () => {
    mockCore({ preferences: { theme: "light", language: "en" } });
    renderApp();
    expect(await screen.findByRole("heading", { name: "No account yet" })).toBeTruthy();
    expect(rootTheme()).toBe("light");

    // E.g. the user changed the settings in a second window; the core tells every window.
    const event: CoreEvent = { type: "preferencesChanged", preferences: { theme: "dark", language: "de" } };
    await act(async () => {
      await emit("core-event", event);
    });

    expect(await screen.findByRole("heading", { name: "Noch kein Konto" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "No account yet" })).toBeNull();
    expect(rootTheme()).toBe("dark");
    expect(document.documentElement.lang).toBe("de");
  });

  it("shows an error in the frame when the event stream cannot be subscribed", async () => {
    mockCore({
      mockEvents: false,
      failures: {
        "plugin:event|listen":
          "event.listen not allowed. Permissions associated with this command: core:event:allow-listen",
      },
    });
    renderApp();

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Live updates from the app core are not available.");
    expect(alert.textContent).toContain("core:event:allow-listen");
  });
});
