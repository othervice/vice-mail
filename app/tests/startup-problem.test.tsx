import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { StartupProblem } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { mockCore } from "./support/core";
import { renderApp } from "./support/render";

const BACKUPS = "/Users/test/Library/Application Support/org.vicemail.desktop/backups";
const DATABASE = "/Users/test/Library/Application Support/org.vicemail.desktop/mail.sqlite3";

function problem(overrides: Partial<StartupProblem> = {}): StartupProblem {
  return {
    error: { code: "database", message: "database disk image is malformed", detail: "file is not a database" },
    databasePath: DATABASE,
    backupDir: BACKUPS,
    backups: ["mail-000001800000000000-v17.sqlite3", "mail-000001700000000000-v16.sqlite3"],
    ...overrides,
  };
}

describe("the database did not open at start", () => {
  it("says what happened, that the mail is on the server, the backups and how to go back, and leads to diagnostics", async () => {
    const core = mockCore({ startupProblem: problem() });
    const { user } = renderApp();
    const notice = await screen.findByTestId("startup-problem");
    expect(notice.getAttribute("role")).toBe("alert");
    expect(within(notice).getByRole("heading", { name: "The app could not open its local data" })).toBeTruthy();
    expect(notice.textContent).toContain("The local data could not be read or written.");
    expect(notice.textContent).toContain("Your mail is not lost: it stays on your mail server.");
    expect(within(notice).getByTestId("startup-problem-backups").textContent).toBe(
      `Before every update of its data the app keeps a copy. There are 2 in ${BACKUPS}; the newest is mail-000001800000000000-v17.sqlite3.`,
    );
    expect(notice.textContent).toContain(`move ${DATABASE} somewhere safe, put the copy in its place under the name mail.sqlite3`);
    expect(notice.textContent).toContain("export diagnostics in Settings → About");
    // The technical detail is there, behind the disclosure.
    expect(notice.textContent).toContain("file is not a database");
    expect(core.callsOf("get_startup_problem")).toHaveLength(1);

    await user.click(within(notice).getByRole("button", { name: "Open Settings → About" }));
    expect(useUiStore.getState().view).toEqual({ kind: "settings", section: "about" });
    expect(await screen.findByRole("button", { name: "Export diagnostics…" })).toBeTruthy();
    // It stays above every view until the next start.
    expect(screen.getByTestId("startup-problem")).toBeTruthy();
  });

  it("a database of a newer app version: nothing changed, update the app; no restore advice", async () => {
    mockCore({ startupProblem: problem({ error: { code: "database_too_new", message: "newer", detail: null } }) });
    renderApp();
    const notice = await screen.findByTestId("startup-problem");
    expect(notice.textContent).toContain("The local data was created by a newer version of the app.");
    expect(notice.textContent).toContain("Nothing was changed.");
    expect(within(notice).queryByTestId("startup-problem-backups")).toBeNull();
    expect(notice.textContent).not.toContain("go back to that copy");
  });

  it("without backups it says there are none", async () => {
    mockCore({ startupProblem: problem({ backups: [] }) });
    renderApp();
    const notice = await screen.findByTestId("startup-problem");
    expect(notice.textContent).toContain("There is no copy from an earlier update");
  });

  it("shows nothing when the database opened", async () => {
    const core = mockCore();
    renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    await waitFor(() => {
      expect(core.callsOf("get_startup_problem")).toHaveLength(1);
    });
    expect(screen.queryByTestId("startup-problem")).toBeNull();
  });
});
