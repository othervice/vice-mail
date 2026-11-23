import { screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it } from "vitest";
import type { UpdateInfo, UpdateStatus } from "../src/services/ipc";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ADA = makeAccount();

const SUPPORTED: Partial<UpdateInfo> = { supported: true };

/** Serious or critical axe findings in the rendered app (as `a11y.test.tsx`, without the mail frame). */
async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

const AVAILABLE: UpdateStatus = {
  state: "available",
  update: {
    version: "0.2.0",
    notes: "Faster search.\n<b>Not bold</b> <script>alert(1)</script>",
    pubDate: "2026-11-23T12:00:00Z",
  },
};

async function openAbout(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "About" }));
  const panel = screen.getByRole("tabpanel", { name: "About" });
  return within(panel).findByTestId("updates-panel");
}

describe("Settings → About, updates (decision 0037)", () => {
  it("shows the version and an off switch that says what a check reveals; nothing is checked by itself", async () => {
    const core = mockCore({ updates: { info: SUPPORTED } });
    const { user } = renderApp();
    const updates = await openAbout(user);

    const toggle = within(updates).getByRole("switch", { name: "Check for updates automatically" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    const description = toggle.getAttribute("aria-describedby");
    expect(description && document.getElementById(description)?.textContent).toMatch(
      /asks github\.com.*IP address.*no account data, no usage data and no identifier.*without your click/,
    );
    expect(within(updates).getByText("Not checked yet.")).toBeTruthy();
    expect(screen.getByRole("tabpanel", { name: "About" }).textContent).toContain("0.1.0");
    expect(core.callsOf("check_for_updates")).toEqual([]);

    await user.click(toggle);
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: true }]);
    expect(core.callsOf("check_for_updates")).toEqual([]);
  });

  it("checks on 'Check now' even with the switch off and reports the newest version", async () => {
    const core = mockCore({ updates: { info: SUPPORTED } });
    const { user } = renderApp();
    const updates = await openAbout(user);

    await user.click(within(updates).getByRole("button", { name: "Check now" }));
    expect(await within(updates).findByText(/You have the newest version\. Last checked/)).toBeTruthy();
    expect(core.callsOf("check_for_updates")).toHaveLength(1);
    expect(core.updates.info.autoCheck).toBe(false);
  });

  it("shows an available update with plain-text notes and installs only after both clicks", async () => {
    const core = mockCore({ updates: { info: SUPPORTED, checkResult: AVAILABLE } });
    const { user } = renderApp();
    const updates = await openAbout(user);

    await user.click(within(updates).getByRole("button", { name: "Check now" }));
    expect(await within(updates).findByText("Version 0.2.0 is available.")).toBeTruthy();
    const notes = within(updates).getByTestId("updates-notes");
    expect(notes.textContent).toBe("Faster search.\n<b>Not bold</b> <script>alert(1)</script>");
    expect(notes.querySelector("b, script")).toBeNull();
    expect(within(updates).getByText("Published November 23, 2026.")).toBeTruthy();
    expect(core.callsOf("download_update")).toEqual([]);
    expect(await seriousFindings()).toEqual([]);

    await user.click(within(updates).getByRole("button", { name: "Download update" }));
    expect(core.callsOf("download_update")).toHaveLength(1);
    core.updates.setStatus({ state: "downloading", version: "0.2.0", downloaded: 512, total: 2048 });
    await emitCore({ type: "updateStatusChanged", status: core.updates.info.status });
    const progress = await within(updates).findByRole("progressbar", { name: "Download progress" });
    expect(progress.getAttribute("value")).toBe("25");
    expect(await seriousFindings()).toEqual([]);
    expect(within(updates).getByText("Downloading version 0.2.0… 25%")).toBeTruthy();

    core.updates.setStatus({ state: "ready", version: "0.2.0" });
    await emitCore({ type: "updateStatusChanged", status: core.updates.info.status });
    expect(await within(updates).findByText("Version 0.2.0 is downloaded and its signature verified.")).toBeTruthy();
    expect(within(updates).getByText(/replaces itself with the new version and opens again/)).toBeTruthy();
    expect(core.callsOf("install_update")).toEqual([]);

    await user.click(within(updates).getByRole("button", { name: "Restart to install" }));
    expect(core.callsOf("install_update")).toHaveLength(1);
    expect(await within(updates).findByText("Installing version 0.2.0…")).toBeTruthy();
  });

  it("explains a rejected update with the code's title and hint", async () => {
    const core = mockCore({
      updates: {
        info: {
          ...SUPPORTED,
          status: { state: "failed", version: "0.2.0", error: ipcError("update_signature_invalid", "signature mismatch") },
        },
      },
    });
    const { user } = renderApp();
    const updates = await openAbout(user);

    const alert = within(updates).getByRole("alert");
    expect(alert.textContent).toContain("Updating to version 0.2.0 failed.");
    expect(alert.textContent).toContain("The update could not be verified and was discarded.");
    expect(alert.textContent).toContain("Nothing was installed.");
    expect(alert.textContent).toContain("signature mismatch");

    // "Try again" downloads the found update again; nothing is installed.
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    expect(core.callsOf("download_update")).toHaveLength(1);
    expect(core.callsOf("install_update")).toEqual([]);
  });

  it("speaks German with du", async () => {
    mockCore({ preferences: { theme: "system", language: "de" }, updates: { info: SUPPORTED } });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("button", { name: "Einstellungen" }));
    await user.click(screen.getByRole("tab", { name: "Über" }));
    const updates = await screen.findByTestId("updates-panel");
    const toggle = within(updates).getByRole("switch", { name: "Automatisch nach Updates suchen" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(within(updates).getByRole("button", { name: "Jetzt suchen" })).toBeTruthy();
    expect(updates.textContent).toContain("deine IP-Adresse");
  });

  it("offers nothing in a build that does not update itself", async () => {
    const core = mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const updates = await openAbout(user);
    expect(updates.textContent).toContain("This build does not update itself.");
    expect(within(updates).queryByRole("switch")).toBeNull();
    expect(within(updates).queryByRole("button", { name: "Check now" })).toBeNull();
    expect(screen.queryByTestId("update-consent")).toBeNull();
    expect(core.callsOf("check_for_updates")).toEqual([]);
  });
});

describe("the one-time update question (decision 0037)", () => {
  it("asks once after the first account, honestly, and 'No, thanks' leaves it off", async () => {
    const core = mockCore({ accounts: [ADA], updates: { info: { ...SUPPORTED, askConsent: true } } });
    const { user } = renderApp();

    const question = await screen.findByRole("region", { name: "Check for updates automatically?" });
    expect(question.textContent).toContain("github.com");
    expect(question.textContent).toContain("GitHub sees your IP address and the time of the request.");
    expect(question.textContent).toContain("no account data, no usage data and no identifier");
    expect(question.textContent).toContain("Nothing is downloaded or installed without your click.");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await seriousFindings()).toEqual([]);

    await user.click(within(question).getByRole("button", { name: "No, thanks" }));
    await waitFor(() => {
      expect(screen.queryByTestId("update-consent")).toBeNull();
    });
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: false }]);
    expect(core.updates.info.autoCheck).toBe(false);
    expect(core.callsOf("check_for_updates")).toEqual([]);
  });

  it("turns automatic checks on with 'Check automatically'", async () => {
    const core = mockCore({ accounts: [ADA], updates: { info: { ...SUPPORTED, askConsent: true } } });
    const { user } = renderApp();
    const question = await screen.findByTestId("update-consent");
    await user.click(within(question).getByRole("button", { name: "Check automatically" }));
    await waitFor(() => {
      expect(screen.queryByTestId("update-consent")).toBeNull();
    });
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: true }]);
  });

  it("is not asked when the core says so (answered, or no account yet)", async () => {
    mockCore({ updates: { info: { ...SUPPORTED, askConsent: false } } });
    renderApp();
    await screen.findByRole("button", { name: "Settings" });
    expect(screen.queryByTestId("update-consent")).toBeNull();
  });
});

describe("the quiet update note", () => {
  it("appears when an update is found and leads to About; 'Not now' hides it", async () => {
    const core = mockCore({ accounts: [ADA], updates: { info: { ...SUPPORTED, autoCheck: true } } });
    const { user } = renderApp();
    await screen.findByRole("button", { name: "Settings" });
    expect(screen.queryByTestId("update-notice")).toBeNull();

    core.updates.setStatus(AVAILABLE);
    await emitCore({ type: "updateStatusChanged", status: AVAILABLE });
    const note = await screen.findByRole("region", { name: "Update" });
    expect(note.textContent).toContain("Version 0.2.0 is available.");
    expect(core.callsOf("download_update")).toEqual([]);

    await user.click(within(note).getByRole("button", { name: "Not now" }));
    expect(screen.queryByTestId("update-notice")).toBeNull();

    core.updates.setStatus({ state: "ready", version: "0.2.0" });
    await emitCore({ type: "updateStatusChanged", status: { state: "ready", version: "0.2.0" } });
    const ready = await screen.findByTestId("update-notice");
    expect(ready.textContent).toContain("Version 0.2.0 is ready to install.");
    await user.click(within(ready).getByRole("button", { name: "Show" }));
    expect(await screen.findByTestId("updates-install")).toBeTruthy();
    expect(screen.queryByTestId("update-notice")).toBeNull();
    expect(core.callsOf("install_update")).toEqual([]);
  });
});
