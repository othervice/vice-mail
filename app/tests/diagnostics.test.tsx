import { screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it } from "vitest";
import { ipcError, mockCore } from "./support/core";
import { renderApp } from "./support/render";

// Stage 15, "Diagnose": Settings → About → "Export diagnostics…". The core shows the save dialog
// and writes the file; the UI says what the file holds and what it never holds, and shows where
// it was saved or why it failed.

async function openPanel(user: ReturnType<typeof renderApp>["user"], names = { settings: "Settings", about: "About" }) {
  await user.click(await screen.findByRole("button", { name: names.settings }));
  await user.click(screen.getByRole("tab", { name: names.about }));
  const panel = screen.getByRole("tabpanel", { name: names.about });
  return within(panel).findByTestId("diagnostics-panel");
}

describe("Settings → About, diagnostics", () => {
  it("says what the file contains and never contains, and exports only on a click", async () => {
    const core = mockCore();
    const { user } = renderApp();
    const panel = await openPanel(user);

    expect(within(panel).getByRole("heading", { name: "Diagnostics" })).toBeTruthy();
    const button = within(panel).getByRole("button", { name: "Export diagnostics…" });
    const described = (button.getAttribute("aria-describedby") ?? "")
      .split(" ")
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ");
    expect(described).toMatch(/versions.*counts from the database.*kind and state of each account.*end of the recent log files/);
    expect(described).toMatch(/never contains passwords, keys or tokens, email addresses.*subjects, message text, attachment names/);
    expect(panel.textContent).toContain("The app never sends it anywhere by itself.");
    expect(core.callsOf("export_diagnostics")).toEqual([]);

    await user.click(button);
    const saved = await within(panel).findByTestId("diagnostics-saved");
    expect(saved.getAttribute("role")).toBe("status");
    expect(saved.textContent).toBe("Saved as /Users/test/Downloads/app-diagnostics-2026-11-23.txt");
    expect(core.callsOf("export_diagnostics")).toEqual([{ locale: navigator.language }]);
  });

  it("shows nothing when the save dialog is closed", async () => {
    const core = mockCore({ diagnostics: { kind: "cancelled" } });
    const { user } = renderApp();
    const panel = await openPanel(user);

    await user.click(within(panel).getByRole("button", { name: "Export diagnostics…" }));
    await waitFor(() => {
      expect(core.callsOf("export_diagnostics")).toHaveLength(1);
    });
    await waitFor(() => {
      expect(within(panel).getByRole("button", { name: "Export diagnostics…" }).hasAttribute("disabled")).toBe(false);
    });
    expect(within(panel).queryByTestId("diagnostics-saved")).toBeNull();
    expect(within(panel).queryByRole("alert")).toBeNull();
  });

  it("explains a file that could not be written and tries again", async () => {
    const core = mockCore({ failures: { export_diagnostics: ipcError("diagnostics_write_failed", "PermissionDenied") } });
    const { user } = renderApp();
    const panel = await openPanel(user);

    await user.click(within(panel).getByRole("button", { name: "Export diagnostics…" }));
    const alert = await within(panel).findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("diagnostics_write_failed");
    expect(alert.textContent).toContain("The diagnostics could not be exported.");
    expect(alert.textContent).toContain("The diagnostics file could not be saved.");
    expect(alert.textContent).toContain("Choose another folder, for example Downloads");
    expect(alert.textContent).toContain("PermissionDenied");
    expect(within(panel).queryByTestId("diagnostics-saved")).toBeNull();

    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("export_diagnostics")).toHaveLength(2);
    });
  });

  it("speaks German", async () => {
    mockCore({ preferences: { theme: "light", language: "de" } });
    const { user } = renderApp("de");
    const panel = await openPanel(user, { settings: "Einstellungen", about: "Über" });

    expect(within(panel).getByRole("heading", { name: "Diagnose" })).toBeTruthy();
    expect(panel.textContent).toContain("Sie enthält nie Passwörter");
    await user.click(within(panel).getByRole("button", { name: "Diagnose exportieren\u00a0…" }));
    expect((await within(panel).findByTestId("diagnostics-saved")).textContent).toBe(
      "Gespeichert als /Users/test/Downloads/app-diagnostics-2026-11-23.txt",
    );
  });

  it("has no serious axe findings", async () => {
    mockCore();
    const { user } = renderApp();
    const panel = await openPanel(user);
    await user.click(within(panel).getByRole("button", { name: "Export diagnostics…" }));
    await within(panel).findByTestId("diagnostics-saved");
    const results = await axe.run(document.body, {
      resultTypes: ["violations"],
      iframes: false,
      rules: { "color-contrast": { enabled: false } },
    });
    expect(
      results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id),
    ).toEqual([]);
  });
});
