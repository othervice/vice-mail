import { screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PRODUCT_NAME } from "../src/config/product";
import type { DiscoveryResult, UpdateInfo } from "../src/services/ipc";
import { ipcError, makeAccount, mockCore, standardMailboxes, type FakeCore } from "./support/core";
import { renderApp, rootTheme } from "./support/render";

/*
 * Stage 15 (decision 0044): the guided first start. Welcome, account, appearance, optional
 * extras, done; skippable on every step, shown once, never on an installation with an account,
 * and the update question of decision 0037 asked here instead of after the first account.
 */

const KNOWN: DiscoveryResult = {
  source: "knownProvider",
  imap: { host: "imap.vicemail.test", port: 993, security: "tls", username: "ada@vicemail.test" },
  smtp: { host: "smtp.vicemail.test", port: 465, security: "tls", username: "ada@vicemail.test" },
  providerHint: null,
};

const SUPPORTED: Partial<UpdateInfo> = { supported: true };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  // jsdom has no canvas; axe only uses one for icon fonts, which the app does not have.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

type User = ReturnType<typeof renderApp>["user"];

function fresh(options: Parameters<typeof mockCore>[0] = {}): FakeCore {
  return mockCore({ firstStart: { pending: true }, knownProviders: { "vicemail.test": KNOWN }, ...options });
}

const flow = () => screen.findByTestId("first-start");
const stepOf = async () => (await flow()).getAttribute("data-step");

/** The heading of the step that shows, once it has the focus. */
async function focusedHeading(name: string | RegExp): Promise<HTMLElement> {
  const heading = await screen.findByRole("heading", { level: 1, name });
  await waitFor(() => {
    expect(document.activeElement).toBe(heading);
  });
  return heading;
}

async function next(user: User) {
  await user.click(await screen.findByTestId("first-start-next"));
}

async function addAccount(user: User, email = "ada@vicemail.test") {
  await user.type(screen.getByLabelText("Email address"), email);
  await user.type(screen.getByLabelText("Password"), "app-password-123");
  await screen.findByTestId("server-summary");
  await user.click(screen.getByRole("button", { name: "Add account" }));
  await screen.findByTestId("first-start-account-added");
}

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

describe("guided first start (decision 0044)", () => {
  it("welcomes a new installation with what the app is and its four promises, the focus on the heading", async () => {
    fresh();
    renderApp();
    await focusedHeading("Welcome");
    const welcome = screen.getByTestId("first-start-welcome");
    expect(within(welcome).getByRole("img", { name: PRODUCT_NAME })).toBeTruthy();
    expect(welcome.textContent).toContain(`${PRODUCT_NAME} is an email client for your own accounts.`);
    const promises = within(welcome).getByRole("group", { name: "What you can rely on" });
    for (const text of ["only read at first", "No cloud service in between", "No telemetry", "system keychain"]) {
      expect(promises.textContent).toContain(text);
    }
    // The frame is not there yet: no rail, no "No account yet".
    expect(screen.queryByTestId("rail-settings")).toBeNull();
    expect(screen.queryByTestId("welcome")).toBeNull();
    const steps = screen.getByRole("navigation", { name: "Setup steps" });
    expect(within(steps).getAllByRole("listitem").map((item) => item.textContent)).toEqual([
      "Welcome",
      "Account",
      "Appearance",
      "Optional",
      "Done",
    ]);
    expect(within(steps).getByText("Welcome").closest("li")?.getAttribute("aria-current")).toBe("step");
    expect(screen.queryByTestId("first-start-back")).toBeNull();
  });

  it("walks through every step: account added read-only, theme and accent live, update question answered, done", async () => {
    // `askConsent` as the core has it once an account exists and nobody answered yet.
    const core = fresh({
      updates: { info: { supported: true, askConsent: true } },
      mailboxes: standardMailboxes("22222222-2222-4222-8222-222222222222"),
    });
    const { user } = renderApp();
    await focusedHeading("Welcome");
    await next(user);

    // 2. Account: the setup form in the step, without its own way back or "Cancel".
    await focusedHeading("Add your first account");
    const account = screen.getByTestId("first-start-account");
    expect(within(account).queryByRole("button", { name: "Back to accounts" })).toBeNull();
    expect(within(account).queryByRole("button", { name: "Cancel" })).toBeNull();
    expect(within(account).getByTestId("read-only-note")).toBeTruthy();
    await addAccount(user);
    expect(core.callsOf("add_account")).toHaveLength(1);
    const added = screen.getByTestId("first-start-account-added");
    expect(added.textContent).toContain("ada@vicemail.test is set up");
    expect(added.textContent).toContain("It only reads for now");
    await waitFor(() => {
      expect(document.activeElement).toBe(added);
    });
    // Still the first start: adding the account did not end it or open the inbox.
    expect(await stepOf()).toBe("account");
    expect(screen.queryByTestId("rail-settings")).toBeNull();
    await next(user);

    // 3. Appearance: applied at once and stored by the core.
    await focusedHeading("Choose your look");
    await user.click(screen.getByRole("radio", { name: "Dark" }));
    await waitFor(() => {
      expect(rootTheme()).toBe("dark");
    });
    expect(core.preferences.theme).toBe("dark");
    await user.click(screen.getByRole("radio", { name: "Violet" }));
    await waitFor(() => {
      expect(document.documentElement.dataset["accent"]).toBe("violet");
    });
    expect(core.ui.preferences.accentTheme).toBe("violet");
    await next(user);

    // 4. Extras: sync offered, AI explained, the update switch off.
    await focusedHeading("Optional extras");
    const extras = screen.getByTestId("first-start-extras-list");
    expect(within(extras).getByRole("button", { name: "Set up now" })).toBeTruthy();
    expect(extras.textContent).toContain("Settings → AI over MCP");
    const toggle = within(extras).getByRole("switch", { name: "Check for updates automatically" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(core.callsOf("set_update_auto_check")).toEqual([]);
    await next(user);
    // Leaving the step answers the question with what the switch showed.
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: false }]);

    // 5. Done: the account only reads, and where to allow changes.
    await focusedHeading("You’re all set");
    const note = screen.getByTestId("first-start-read-only");
    expect(note.textContent).toContain("ada@vicemail.test only reads for now");
    expect(note.textContent).toContain("Settings → Accounts");
    expect(note.textContent).toContain("“Allow changes”");
    expect(screen.queryByTestId("first-start-skip")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Open inbox" }));

    await screen.findByTestId("rail-settings");
    expect(screen.queryByTestId("first-start")).toBeNull();
    expect(core.firstStart.ends).toEqual(["completed"]);
    // Asked once: the separate question after the first account does not come.
    expect(core.updates.info.askConsent).toBe(false);
    expect(screen.queryByTestId("update-consent")).toBeNull();
    expect(core.updates.info.autoCheck).toBe(false);
    expect(rootTheme()).toBe("dark");
  });

  it("turning the update switch on in the flow stores it at once and keeps it on", async () => {
    const core = fresh({ updates: { info: SUPPORTED } });
    const { user } = renderApp();
    await next(user);
    await user.click(screen.getByTestId("first-start-later"));
    await next(user);
    await focusedHeading("Optional extras");
    const toggle = screen.getByRole("switch", { name: "Check for updates automatically" });
    await user.click(toggle);
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: true }]);
    await next(user);
    await focusedHeading("You’re all set");
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: true }, { enabled: true }]);
    expect(core.updates.info.autoCheck).toBe(true);
  });

  it.each([
    ["welcome", 0],
    ["account", 1],
    ["appearance", 2],
    ["extras", 3],
  ] as const)("can be skipped on the %s step and then shows the frame", async (step, forward) => {
    const core = fresh();
    const { user } = renderApp();
    await focusedHeading("Welcome");
    for (let i = 0; i < forward; i += 1) {
      const later = screen.queryByTestId("first-start-later");
      await user.click(later ?? screen.getByTestId("first-start-next"));
    }
    expect(await stepOf()).toBe(step);
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    await screen.findByTestId("welcome");
    expect(core.firstStart.ends).toEqual(["skipped"]);
    expect(core.firstStart.state).toEqual({ show: false, outcome: "skipped" });
  });

  it("is shown once: after it ended, the next start shows the frame", async () => {
    const core = fresh();
    const first = renderApp();
    await focusedHeading("Welcome");
    await first.user.click(screen.getByRole("button", { name: "Skip setup" }));
    await screen.findByTestId("welcome");
    first.unmount();

    renderApp();
    await screen.findByTestId("welcome");
    expect(screen.queryByTestId("first-start")).toBeNull();
    expect(core.callsOf("finish_first_start")).toHaveLength(1);
  });

  it("never shows on an installation that already has an account", async () => {
    const core = fresh({ accounts: [makeAccount()], mailboxes: standardMailboxes() });
    renderApp();
    await screen.findByTestId("rail-settings");
    expect(screen.queryByTestId("first-start")).toBeNull();
    expect(core.firstStart.state.outcome).toBe("notNeeded");
    expect(core.callsOf("finish_first_start")).toEqual([]);
  });

  it("skipped before the extras step, the update question still comes once, after the first account", async () => {
    // `askConsent` as the core has it once an account exists and nobody answered yet.
    const core = fresh({ updates: { info: { supported: true, askConsent: true } } });
    const { user } = renderApp();
    await next(user);
    await addAccount(user);
    // Not while the steps show: the first start replaces the frame and its notices.
    expect(screen.queryByTestId("update-consent")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    await screen.findByTestId("rail-settings");
    expect(core.callsOf("set_update_auto_check")).toEqual([]);
    const question = await screen.findByTestId("update-consent");
    await user.click(within(question).getByRole("button", { name: "No, thanks" }));
    await waitFor(() => {
      expect(screen.queryByTestId("update-consent")).toBeNull();
    });
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: false }]);
  });

  it("skipping on the extras step counts as an answer: it was asked there", async () => {
    const core = fresh({ updates: { info: SUPPORTED } });
    const { user } = renderApp();
    await next(user);
    await user.click(screen.getByTestId("first-start-later"));
    await next(user);
    await focusedHeading("Optional extras");
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    await screen.findByTestId("welcome");
    expect(core.callsOf("set_update_auto_check")).toEqual([{ enabled: false }]);
    expect(core.firstStart.ends).toEqual(["skipped"]);
  });

  it("asks nothing about updates where the build does not update itself", async () => {
    const core = fresh();
    const { user } = renderApp();
    await next(user);
    await user.click(screen.getByTestId("first-start-later"));
    await next(user);
    await focusedHeading("Optional extras");
    expect(screen.queryByRole("switch", { name: "Check for updates automatically" })).toBeNull();
    await next(user);
    await focusedHeading("You’re all set");
    expect(core.callsOf("set_update_auto_check")).toEqual([]);
  });

  it("keeps what was typed: Escape does nothing, and going on and back keeps the form", async () => {
    const core = fresh();
    const { user } = renderApp();
    await next(user);
    await focusedHeading("Add your first account");
    const email = screen.getByLabelText<HTMLInputElement>("Email address");
    await user.type(email, "ada@vicemail.test");
    await user.keyboard("{Escape}");
    expect(await stepOf()).toBe("account");
    expect(email.value).toBe("ada@vicemail.test");

    await user.click(screen.getByTestId("first-start-later"));
    await focusedHeading("Choose your look");
    await user.keyboard("{Escape}");
    expect(await stepOf()).toBe("appearance");
    await user.click(screen.getByTestId("first-start-back"));
    await focusedHeading("Add your first account");
    expect(screen.getByLabelText<HTMLInputElement>("Email address").value).toBe("ada@vicemail.test");
    expect(core.callsOf("finish_first_start")).toEqual([]);
  });

  it("works with the keyboard alone: Tab reaches the buttons, the focus lands on every heading", async () => {
    fresh();
    const { user } = renderApp();
    await focusedHeading("Welcome");
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Get started" }));
    await user.keyboard("{Enter}");
    await focusedHeading("Add your first account");
    await user.tab();
    expect(document.activeElement).toBe(screen.getByLabelText("Email address"));
    screen.getByTestId("first-start-later").focus();
    await user.keyboard("{Enter}");
    await focusedHeading("Choose your look");
    await user.tab();
    // The theme group is one stop; the arrow keys move (System → Light → Dark), Space picks.
    expect(document.activeElement).toBe(screen.getByRole("radio", { name: "System" }));
    await user.keyboard("{ArrowRight}{ArrowRight} ");
    await waitFor(() => {
      expect(rootTheme()).toBe("dark");
    });
    screen.getByTestId("first-start-next").focus();
    await user.keyboard("{Enter}");
    await focusedHeading("Optional extras");
    screen.getByTestId("first-start-next").focus();
    await user.keyboard("{Enter}");
    await focusedHeading("You’re all set");
    screen.getByTestId("first-start-finish").focus();
    await user.keyboard("{Enter}");
    await screen.findByTestId("welcome");
  });

  it("without an account, the last step says where to add one and the button starts the app", async () => {
    const core = fresh();
    const { user } = renderApp();
    await next(user);
    await user.click(screen.getByTestId("first-start-later"));
    await next(user);
    await next(user);
    await focusedHeading("You’re all set");
    expect(screen.getByTestId("first-start-no-account").textContent).toContain("Settings → Accounts");
    await user.click(screen.getByRole("button", { name: `Start using ${PRODUCT_NAME}` }));
    await screen.findByTestId("welcome");
    expect(core.firstStart.ends).toEqual(["completed"]);
  });

  it("opens the settings sync's own setup in the extras step and comes back to it", async () => {
    const core = fresh();
    const { user } = renderApp();
    await next(user);
    await addAccount(user);
    await next(user);
    await next(user);
    await focusedHeading("Optional extras");
    await user.click(screen.getByRole("button", { name: "Set up now" }));
    const page = await screen.findByTestId("first-start-sync-page");
    const heading = within(page).getByRole("heading", { level: 2, name: "Settings sync" });
    await waitFor(() => {
      expect(document.activeElement).toBe(heading);
    });
    expect(within(page).getByTestId("sync-enable-form")).toBeTruthy();
    await user.type(within(page).getByLabelText("Passphrase"), "correct horse battery staple");
    await user.type(within(page).getByLabelText("Repeat the passphrase"), "correct horse battery staple");
    await user.click(within(page).getByRole("button", { name: "Turn on settings sync" }));
    await waitFor(() => {
      expect(core.mcpSync.syncStatus.state).toBe("active");
    });

    await user.click(screen.getByRole("button", { name: "Back to setup" }));
    const row = await screen.findByRole("heading", { level: 2, name: "Settings sync" });
    await waitFor(() => {
      expect(document.activeElement).toBe(row);
    });
    expect(await screen.findByTestId("first-start-sync-on")).toBeTruthy();
    expect(await stepOf()).toBe("extras");
  });

  it("explains AI without turning anything on", async () => {
    const core = fresh();
    const { user } = renderApp();
    await next(user);
    await user.click(screen.getByTestId("first-start-later"));
    await next(user);
    await focusedHeading("Optional extras");
    expect(screen.getByTestId("first-start-extras-list").textContent).toContain("Optional and off.");
    await next(user);
    await user.click(screen.getByTestId("first-start-finish"));
    await screen.findByTestId("welcome");
    const changed = core.calls.filter((call) => /^(set_ai|save_ai|set_mcp|enable_settings_sync)/.test(call.cmd));
    expect(changed).toEqual([]);
  });

  it("shows why it could not end, and ends on the next try", async () => {
    let failures = 1;
    const core = fresh({
      fail: (cmd) => (cmd === "finish_first_start" && failures-- > 0 ? ipcError("database") : undefined),
    });
    const { user } = renderApp();
    await focusedHeading("Welcome");
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    const error = await screen.findByTestId("first-start-error");
    expect(error.textContent).toContain("The setup could not be finished.");
    expect(await stepOf()).toBe("welcome");
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    await screen.findByTestId("welcome");
    expect(core.firstStart.ends).toEqual(["skipped"]);
  });

  it("shows the frame if the core cannot say whether to show it", async () => {
    mockCore({ firstStart: { pending: true }, failures: { get_first_start: ipcError("database") } });
    renderApp();
    await screen.findByTestId("welcome");
    expect(screen.queryByTestId("first-start")).toBeNull();
  });

  it("speaks German completely", async () => {
    fresh({ preferences: { theme: "system", language: "de" }, updates: { info: SUPPORTED } });
    const { user } = renderApp("de");
    await focusedHeading("Willkommen");
    const texts: string[] = [];
    const collect = async () => {
      texts.push((await flow()).textContent ?? "");
    };
    await collect();
    await user.click(screen.getByRole("button", { name: "Los geht’s" }));
    await focusedHeading("Dein erstes Konto hinzufügen");
    await collect();
    await user.click(screen.getByRole("button", { name: "Später einrichten" }));
    await focusedHeading("Wähle dein Aussehen");
    await collect();
    await user.click(screen.getByRole("button", { name: "Weiter" }));
    await focusedHeading("Optionale Extras");
    expect(screen.getByRole("switch", { name: "Automatisch nach Updates suchen" })).toBeTruthy();
    await collect();
    await user.click(screen.getByRole("button", { name: "Weiter" }));
    await focusedHeading("Alles bereit");
    await collect();
    expect(screen.getByRole("button", { name: `${PRODUCT_NAME} verwenden` })).toBeTruthy();
    for (const text of texts) {
      // No key shows through and no English text is left.
      expect(text).not.toMatch(/firstStart\.|settings\.|updates\./);
      expect(text).not.toMatch(/\b(Continue|Back|Skip setup|Welcome|Optional extras|Choose your look)\b/);
    }
  });
});

describe.each(["light", "dark"] as const)("guided first start, axe, %s theme", (theme) => {
  it("finds nothing serious on any step", async () => {
    fresh({ preferences: { theme, language: "system" }, updates: { info: SUPPORTED } });
    const { user } = renderApp();
    await focusedHeading("Welcome");
    expect(rootTheme()).toBe(theme);
    expect(await seriousFindings()).toEqual([]);
    await next(user);
    await focusedHeading("Add your first account");
    expect(await seriousFindings()).toEqual([]);
    await addAccount(user);
    expect(await seriousFindings()).toEqual([]);
    await next(user);
    await focusedHeading("Choose your look");
    expect(await seriousFindings()).toEqual([]);
    await next(user);
    await focusedHeading("Optional extras");
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Set up now" }));
    await screen.findByTestId("first-start-sync-page");
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Back to setup" }));
    await next(user);
    await focusedHeading("You’re all set");
    expect(await seriousFindings()).toEqual([]);
  });
});
