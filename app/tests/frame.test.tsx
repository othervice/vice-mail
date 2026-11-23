import { act, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { mockCore } from "./support/core";
import { renderApp } from "./support/render";

describe("app frame without an account", () => {
  it("shows the inbox with the empty state and no invented content", async () => {
    mockCore();
    renderApp();

    expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeTruthy();
    // The account list comes from the core; without an account the empty state says what to do.
    expect(await screen.findByRole("heading", { name: "No account yet" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add account" })).toBeTruthy();
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.queryByTestId("sync-status")).toBeNull();
    // No mail rows, no counters: the only list in the main area would be mail, and there is none.
    const main = screen.getByRole("main");
    expect(within(main).queryAllByRole("listitem")).toHaveLength(0);
    expect(main.textContent).not.toMatch(/\d/);
  });

  it("has every rail entry from DESIGN.md, an empty account switcher and settings", async () => {
    mockCore();
    renderApp();

    const rail = await screen.findByRole("navigation", { name: "Views" });
    const names = within(rail)
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label"));
    expect(names).toEqual([
      "Inbox",
      "Later",
      "Tasks",
      // Stage 16
      "Reading list",
      "Pinned attachments",
      "Pinned",
      "Sent",
      "Drafts",
      "Archive",
      "Trash",
      "Folders",
      "Saved views",
      "Activity",
      "Outbox",
      "Settings",
    ]);
    expect(within(rail).getByRole("list", { name: "Accounts" }).children).toHaveLength(0);
    expect(within(rail).getByRole("button", { name: "Inbox" }).getAttribute("aria-current")).toBe("page");
  });

  it("switches views from the rail; every mail view shows the same empty state", async () => {
    mockCore();
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Sent" }));

    expect(screen.getByRole("heading", { level: 1, name: "Sent" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "No account yet" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sent" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("button", { name: "Inbox" }).getAttribute("aria-current")).toBeNull();
  });

  it("'Add account' opens the account setup as a view of Settings → Accounts, not a dialog", async () => {
    mockCore();
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Add account" }));

    expect(screen.getByRole("heading", { level: 1, name: "Settings" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: "Accounts" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("heading", { level: 2, name: "Add account" })).toBeTruthy();
    expect(screen.getByLabelText("Email address")).toBeTruthy();
    expect(screen.getByLabelText("Password")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("Settings → Accounts without accounts offers to add one instead of an invented list", async () => {
    mockCore();
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));

    expect(await screen.findByRole("heading", { name: "No accounts yet" })).toBeTruthy();
    expect(screen.getByRole("tabpanel").textContent).not.toMatch(/@/);
    await user.click(screen.getByRole("button", { name: "Add account" }));
    expect(screen.getByRole("heading", { level: 2, name: "Add account" })).toBeTruthy();
  });

  it("settings is a view with General, Accounts, Reading, Appearance and About, not a dialog", async () => {
    mockCore();
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));

    const tabs = screen.getByRole("tablist", { name: "Settings sections" });
    expect(within(tabs).getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "General",
      "Accounts",
      "Reading",
      "Writing",
      "Contacts",
      "Inbox",
      "Notifications",
      "Appearance",
      "Keyboard",
      "Settings sync",
      // Stage 13: AI in the app, next to AI over MCP.
      "AI in the app",
      "AI over MCP",
      "About",
    ]);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("About shows product name, version, commit, build date, platform and license from the core", async () => {
    mockCore();
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "About" }));

    const panel = screen.getByRole("tabpanel");
    const text = panel.textContent;
    expect(text).toContain("Vice Mail");
    expect(text).toContain("0.1.0");
    expect(text).toContain("1e235a5");
    expect(text).toContain("2026");
    expect(text).toContain("macOS");
    expect(text).toContain("GPL-3.0-or-later");
  });

  it("About says 'Unknown' for a missing commit and build date", async () => {
    mockCore({ appInfo: { version: "0.1.0", commit: null, buildDate: null, platform: "windows" } });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "About" }));

    const panel = screen.getByRole("tabpanel");
    expect(within(panel).getAllByText("Unknown")).toHaveLength(2);
    expect(panel.textContent).toContain("Windows");
  });

  it("shows a core error where it happened, with a text for its code and the detail", async () => {
    mockCore({
      failures: {
        get_app_info: { code: "database", message: "Could not open the database.", detail: "disk I/O error" },
      },
    });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "About" }));

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("App information could not be loaded.");
    expect(alert.textContent).toContain("The local data could not be read or written.");
    expect(within(alert).getByText("Technical details")).toBeTruthy();
    expect(alert.textContent).toContain("disk I/O error");
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("marks <html> with the platform so macOS gets room for the window buttons", async () => {
    mockCore();
    renderApp();
    await screen.findByRole("main");
    expect(document.documentElement.dataset["platform"]).toBe("macos");
    expect(document.querySelector("header")?.getAttribute("data-tauri-drag-region")).toBe("deep");
  });

  it("adds no drag region on Windows, which keeps its native title bar", async () => {
    mockCore({ appInfo: { version: "0.1.0", commit: null, buildDate: null, platform: "windows" } });
    renderApp();
    await screen.findByRole("main");
    expect(document.documentElement.dataset["platform"]).toBe("windows");
    expect(document.querySelector("[data-tauri-drag-region]")).toBeNull();
  });

  it("folding the sidebar opens no tooltip of a button that had the focus while it was unfolded", async () => {
    mockCore();
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Folders" }));
    // Unfolded, the rail shows its labels and no tooltips. Keyboard focus passes over Inbox.
    const inbox = screen.getByRole("button", { name: "Inbox" });
    act(() => {
      inbox.focus();
    });
    expect(screen.queryByRole("tooltip")).toBeNull();
    act(() => {
      inbox.blur();
    });

    await user.click(screen.getByRole("button", { name: "Folders" }));
    expect(screen.getByRole("button", { name: "Folders" }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("tooltip")).toBeNull();
  });
});
