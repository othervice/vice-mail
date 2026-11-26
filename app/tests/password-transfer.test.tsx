// Stage 22 (decision 0076): passwords from another device. The new device asks and shows the
// code; the device that has the passwords answers, compares the code and chooses the accounts.
// Both sides with the keyboard, and axe in both themes.
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PasswordRequest, SettingsSyncStatus, ThemePreference } from "../src/services/ipc";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { emitCore } from "./support/events";
import { makeDevice, makePasswordRequest, syncStatus } from "./support/mcpSync";
import { renderApp } from "./support/render";
import { REQUEST_POLL_MS } from "../src/features/settings-sync/PasswordTransfer";

const ADA = makeAccount();
const BOB = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "bob@example.com", displayName: "Bob", color: 3 });
const CAROL = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "carol@example.com", displayName: "Carol", color: 4 });
const EXPIRES = Date.UTC(2026, 10, 26, 10, 15);

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

/** Same rules as tests/a11y.test.tsx (colour contrast is tests/contrast.test.ts). */
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

function active(overrides: Partial<SettingsSyncStatus> = {}): SettingsSyncStatus {
  return syncStatus({
    state: "active",
    accountId: ADA.id,
    folder: "Settings-Sync",
    lastSyncAt: Date.UTC(2026, 10, 26, 9, 30),
    devices: [makeDevice()],
    ...overrides,
  });
}

async function openSync(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Settings sync" }));
  const panel = screen.getByRole("tabpanel", { name: "Settings sync" });
  await within(panel).findByText(/Keeps settings, signatures/);
  return panel;
}

/** Emits a new status as a pass of the core would. */
async function pass(core: ReturnType<typeof mockCore>, next: SettingsSyncStatus) {
  core.mcpSync.setSyncStatus(next);
  await emitCore({ type: "settingsSyncChanged", status: next });
}

describe("Settings sync, the new device: take over passwords", () => {
  it("asks, shows the code once a device answered, and what arrived", async () => {
    const core = mockCore({
      accounts: [ADA, BOB, CAROL],
      mcpSync: { sync: { status: active({ accountsNeedingPassword: [BOB.id, CAROL.id] }) } },
    });
    const { user } = renderApp();
    const panel = await openSync(user);
    const block = await within(panel).findByTestId("sync-passwords");
    expect(block.textContent).toContain("take them over from another device, or enter each one here");
    expect(within(block).getByTestId("transfer-offer").textContent).toContain("Give passwords to a new device");

    // Keyboard: Tab to the button, Enter.
    const start = within(block).getByRole("button", { name: "Take over passwords from another device" });
    start.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(core.callsOf("start_password_transfer")).toEqual([{}]);
    });
    const waiting = await within(block).findByTestId("transfer-waiting");
    expect(within(waiting).getByRole("status").textContent).toContain("The request runs until");

    // Another device answered: the code.
    const transfer = core.mcpSync.syncStatus.passwordTransfer;
    expect(transfer?.accounts).toEqual([BOB.id, CAROL.id]);
    await pass(core, {
      ...core.mcpSync.syncStatus,
      passwordTransfer: { ...transfer!, state: "confirm", code: "123 456", otherDevice: "Old Mac" },
    });
    const confirm = await within(block).findByTestId("transfer-confirm");
    expect(within(confirm).getByTestId("transfer-code").textContent).toBe("123 456");
    expect(within(confirm).getByRole("img", { name: "Code: 1 2 3 4 5 6" })).toBeTruthy();
    expect(confirm.textContent).toContain("Old Mac shows a code too. If both are the same, confirm there.");

    // The passwords arrived: one stored, one not (other servers there).
    await pass(core, {
      ...core.mcpSync.syncStatus,
      accountsNeedingPassword: [CAROL.id],
      passwordTransfer: { ...transfer!, state: "done", otherDevice: "Old Mac", stored: [BOB.id], skipped: [CAROL.id] },
    });
    const done = await within(block).findByTestId("transfer-done");
    expect(done.textContent).toContain("One password taken over");
    expect(done.textContent).toContain("Writing stays off for these accounts");
    expect(within(done).getByTestId("transfer-skipped").textContent).toBe(CAROL.email);
    expect(within(block).getByTestId(`sync-password-${CAROL.id}`)).toBeTruthy();

    await user.click(within(done).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(core.callsOf("cancel_password_transfer")).toEqual([{}]);
    });
    expect(await within(block).findByTestId("transfer-offer")).toBeTruthy();
  });

  it("cancels a waiting request, and starts over after a decline or a failure", async () => {
    const core = mockCore({
      accounts: [ADA, BOB],
      mcpSync: { sync: { status: active({ accountsNeedingPassword: [BOB.id] }) } },
    });
    const { user } = renderApp();
    const panel = await openSync(user);
    const block = await within(panel).findByTestId("sync-passwords");
    await user.click(within(block).getByRole("button", { name: "Take over passwords from another device" }));
    const waiting = await within(block).findByTestId("transfer-waiting");
    await user.click(within(waiting).getByRole("button", { name: "Cancel" }));
    await within(block).findByTestId("transfer-offer");
    expect(core.callsOf("cancel_password_transfer")).toEqual([{}]);

    const ended = (state: "declined" | "expired" | "failed") => ({
      state,
      code: null,
      otherDevice: "Old Mac",
      expiresAt: EXPIRES,
      accounts: [BOB.id],
      stored: [],
      skipped: [],
    });
    await pass(core, { ...core.mcpSync.syncStatus, passwordTransfer: ended("declined") });
    expect((await within(block).findByTestId("transfer-declined")).textContent).toContain("Old Mac declined");
    await pass(core, { ...core.mcpSync.syncStatus, passwordTransfer: ended("expired") });
    expect((await within(block).findByTestId("transfer-expired")).textContent).toContain("A request runs for 15 minutes.");
    await pass(core, { ...core.mcpSync.syncStatus, passwordTransfer: ended("failed") });
    const failed = await within(block).findByTestId("transfer-failed");
    expect(failed.textContent).toContain("Nothing was stored.");
    await user.click(within(failed).getByRole("button", { name: "Start over" }));
    await waitFor(() => {
      expect(core.callsOf("start_password_transfer")).toHaveLength(2);
    });
  });

  it("points to the sync from the page of an account that waits for its password", async () => {
    mockCore({ accounts: [ADA, BOB], mcpSync: { sync: { status: active({ accountsNeedingPassword: [BOB.id] }) } } });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${BOB.id}`));
    const detail = await screen.findByTestId("account-detail");
    const notice = await within(detail).findByTestId("password-from-other-device");
    await user.click(within(notice).getByRole("button", { name: "Open settings sync" }));
    expect(await screen.findByRole("tabpanel", { name: "Settings sync" })).toBeTruthy();

    // Not on an account that has its password.
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${ADA.id}`));
    const other = await screen.findByTestId("account-detail");
    expect(within(other).queryByTestId("password-from-other-device")).toBeNull();
  });
});

describe("Settings sync, the device that has the passwords: give them", () => {
  function request(overrides: Partial<PasswordRequest> = {}): PasswordRequest {
    return makePasswordRequest({
      expiresAt: EXPIRES,
      accounts: [
        { accountId: BOB.id, state: "ready" },
        { accountId: CAROL.id, state: "serversDiffer" },
      ],
      ...overrides,
    });
  }

  it("looks for requests while open, compares the code and gives the chosen passwords", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const core = mockCore({ accounts: [ADA, BOB, CAROL], mcpSync: { sync: { status: active() } } });
      const { user } = renderApp();
      const panel = await openSync(user);
      const block = await within(panel).findByTestId("sync-give");
      await user.click(within(block).getByRole("button", { name: "Give passwords to a new device" }));
      const open = await within(block).findByTestId("give-panel");
      expect(within(open).getByRole("status").textContent).toContain("On the new device, open Settings");
      // A pass at once, then every few seconds while nothing came.
      await waitFor(() => {
        expect(core.callsOf("sync_settings_now").length).toBeGreaterThanOrEqual(1);
      });
      const before = core.callsOf("sync_settings_now").length;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(REQUEST_POLL_MS + 10);
      });
      expect(core.callsOf("sync_settings_now").length).toBeGreaterThan(before);

      await pass(core, { ...core.mcpSync.syncStatus, passwordRequests: [request()] });
      const card = await within(open).findByTestId("give-request-open");
      expect(card.textContent).toContain("Request from New Mac");
      const polled = core.callsOf("sync_settings_now").length;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3 * REQUEST_POLL_MS);
      });
      expect(core.callsOf("sync_settings_now")).toHaveLength(polled);

      // Keyboard: the focused "Continue" answers the request.
      within(card).getByRole("button", { name: "Continue" }).focus();
      await user.keyboard("{Enter}");
      await waitFor(() => {
        expect(core.callsOf("open_password_request")).toEqual([{ requestId: request().requestId }]);
      });
      expect((await within(open).findByTestId("give-connecting")).textContent).toContain("Connecting to New Mac");

      // The new device sent its key: the code and the accounts.
      await pass(core, { ...core.mcpSync.syncStatus, passwordRequests: [request({ state: "confirm", code: "654 321" })] });
      const confirm = await within(open).findByTestId("give-request-confirm");
      expect(within(confirm).getByTestId("transfer-code").textContent).toBe("654 321");
      expect(within(confirm).getByRole("status").textContent).toBe("Does New Mac show this code?");
      const accounts = within(confirm).getByRole("group", { name: "Passwords to give" });
      const bob = within(accounts).getByRole("checkbox", { name: /bob@example.com/ });
      const carol = within(accounts).getByRole("checkbox", { name: /carol@example.com/ });
      expect((bob as HTMLInputElement).checked).toBe(true);
      expect((carol as HTMLInputElement).disabled).toBe(true);
      expect(accounts.textContent).toContain("servers differ on the two devices, not sent");

      // Nothing chosen: the send button waits.
      await user.click(bob);
      expect(within(confirm).getByRole("button", { name: "Codes match, send passwords" })).toHaveProperty("disabled", true);
      expect(confirm.textContent).toContain("Choose at least one account.");
      await user.click(bob);
      await user.click(within(confirm).getByRole("button", { name: "Codes match, send passwords" }));
      await waitFor(() => {
        expect(core.callsOf("give_passwords")).toEqual([{ requestId: request().requestId, accountIds: [BOB.id] }]);
      });
      expect((await within(open).findByTestId("give-request-given")).textContent).toContain("New Mac stores them in its keychain");
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows a request a pass found, declines it, and warns about a key that does not fit", async () => {
    const core = mockCore({
      accounts: [ADA, BOB],
      mcpSync: { sync: { status: active({ passwordRequests: [request()] }) } },
    });
    const { user } = renderApp();
    const panel = await openSync(user);
    const block = await within(panel).findByTestId("sync-give");
    const notice = within(block).getByTestId("give-notice");
    expect(notice.textContent).toContain("New Mac asks for passwords");
    await user.click(within(notice).getByRole("button", { name: "Show request" }));
    const card = await within(block).findByTestId("give-request-open");
    await user.click(within(card).getByRole("button", { name: "Decline" }));
    await waitFor(() => {
      expect(core.callsOf("decline_password_request")).toEqual([{ requestId: request().requestId }]);
    });
    expect((await within(block).findByTestId("give-request-declined")).textContent).toContain("Declined.");

    await pass(core, { ...core.mcpSync.syncStatus, passwordRequests: [request({ state: "failed" })] });
    const failed = await within(block).findByTestId("give-request-failed");
    expect(failed.textContent).toContain("The key of the new device does not fit its request");
    expect(failed.textContent).toContain("Nothing was sent.");
    await pass(core, { ...core.mcpSync.syncStatus, passwordRequests: [request({ state: "otherDevice" })] });
    expect((await within(block).findByTestId("give-request-otherDevice")).textContent).toContain("Another device answered");
  });

  it("shows the error when giving fails", async () => {
    mockCore({
      accounts: [ADA, BOB],
      mcpSync: { sync: { status: active({ passwordRequests: [request({ state: "confirm", code: "111 222" })] }) } },
      failures: { give_passwords: ipcError("keychain", "denied") },
    });
    const { user } = renderApp();
    const panel = await openSync(user);
    const block = await within(panel).findByTestId("sync-give");
    await user.click(within(block).getByRole("button", { name: "Show request" }));
    const confirm = await within(block).findByTestId("give-request-confirm");
    await user.click(within(confirm).getByRole("button", { name: "Codes match, send passwords" }));
    expect((await within(confirm).findByRole("alert")).textContent).toContain("The passwords could not be sent.");
  });
});

describe.each(["light", "dark"] as const)("axe, %s theme, password transfer", (theme: ThemePreference) => {
  it("finds nothing serious on either side", async () => {
    const core = mockCore({
      preferences: { theme, language: "system" },
      accounts: [ADA, BOB, CAROL],
      mcpSync: {
        sync: {
          status: active({
            accountsNeedingPassword: [CAROL.id],
            passwordTransfer: {
              state: "confirm",
              code: "123 456",
              otherDevice: "Old Mac",
              expiresAt: EXPIRES,
              accounts: [CAROL.id],
              stored: [],
              skipped: [],
            },
            passwordRequests: [
              makePasswordRequest({
                expiresAt: EXPIRES,
                state: "confirm",
                code: "654 321",
                accounts: [
                  { accountId: BOB.id, state: "ready" },
                  { accountId: CAROL.id, state: "noPassword" },
                ],
              }),
            ],
          }),
        },
      },
    });
    const { user } = renderApp();
    const panel = await openSync(user);
    await within(panel).findByTestId("transfer-confirm");
    await user.click(within(panel).getByRole("button", { name: "Show request" }));
    await within(panel).findByTestId("give-request-confirm");
    expect(await seriousFindings()).toEqual([]);
    expect(core.callsOf("sync_settings_now").length).toBeGreaterThanOrEqual(1);
  }, 15_000);
});
