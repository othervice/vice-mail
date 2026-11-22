import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountStatus } from "../src/services/ipc";
import { IDLE, ipcError, makeAccount, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ACCOUNT = makeAccount();

beforeEach(() => {
  // jsdom has no layout: give the message list a viewport so it renders rows.
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

function status(overrides: Partial<AccountStatus>): AccountStatus {
  return { ...IDLE, ...overrides };
}

function setup(accountStatus: AccountStatus, extra: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({
    accounts: [makeAccount({ status: accountStatus })],
    mailboxes: standardMailboxes(),
    messages: { 1: makeMessages(3) },
    ...extra,
  });
  return { core, ...renderApp() };
}

describe("sync status in the list header", () => {
  it("connecting", async () => {
    setup(status({ state: "connecting", lastSyncAt: null }));
    const indicator = await screen.findByTestId("sync-status");
    expect(within(indicator).getByRole("status").textContent).toBe("Connecting…");
  });

  it("syncing with progress; only the state word is announced", async () => {
    setup(status({ state: "syncing", progress: { mailboxId: 1, done: 4500, total: 10000 } }));
    const indicator = await screen.findByTestId("sync-status");
    expect(within(indicator).getByRole("status").textContent).toBe("Syncing…");
    expect(indicator.textContent).toContain("4,500 of 10,000");
  });

  it("idle shows nothing", async () => {
    setup(IDLE);
    const indicator = await screen.findByTestId("sync-status");
    expect(indicator.textContent).toBe("");
  });

  it("offline with the next attempt and a way to try now; the list stays usable", async () => {
    const retryAt = new Date(2026, 10, 22, 21, 30).getTime();
    const offline = status({ state: "offline", error: ipcError("connection_refused"), retryAt });
    // A second account is online, so the app as a whole is not offline.
    const { core, user } = setup(offline, {
      accounts: [
        makeAccount({ status: offline }),
        makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "b@example.org" }),
      ],
    });

    const indicator = await screen.findByTestId("sync-status");
    expect(within(indicator).getByRole("status").textContent).toBe("Offline");
    expect(indicator.textContent).toContain("Next attempt at 9:30 PM");
    expect(await screen.findByRole("listbox")).toBeTruthy();
    await user.click(within(indicator).getByRole("button", { name: "Try now" }));
    expect(core.callsOf("retry_account")).toEqual([{ accountId: ACCOUNT.id }]);
  });

  it("'offline' for the whole app when every account is offline", async () => {
    setup(status({ state: "offline", error: ipcError("dns_failed"), retryAt: null }));
    const indicator = await screen.findByTestId("sync-status");
    expect(within(indicator).getByRole("status").textContent).toBe("Offline. Your mail on this computer stays available.");
    // Local mail stays usable.
    expect(await screen.findAllByRole("option")).not.toHaveLength(0);
  });

  it("a status event updates the header without asking for the account list again", async () => {
    const { core } = setup(IDLE);
    const indicator = await screen.findByTestId("sync-status");
    const before = core.callsOf("list_accounts").length;

    await emitCore({
      type: "accountStatusChanged",
      accountId: ACCOUNT.id,
      status: status({ state: "syncing", progress: { mailboxId: 1, done: 10, total: 20 } }),
    });

    expect(indicator.textContent).toContain("10 of 20");
    expect(core.callsOf("list_accounts")).toHaveLength(before);
  });

  it("accountsChanged asks for the list again (e.g. an account added elsewhere)", async () => {
    const { core } = setup(IDLE);
    await screen.findByTestId("sync-status");
    const before = core.callsOf("list_accounts").length;

    await emitCore({ type: "accountsChanged" });

    await waitFor(() => {
      expect(core.callsOf("list_accounts").length).toBeGreaterThan(before);
    });
  });
});

describe("account errors", () => {
  it("a stopped account shows why where the mail is, with a retry and the way to fix it", async () => {
    const { core, user } = setup(status({ state: "error", error: ipcError("auth_failed", "NO [AUTHENTICATIONFAILED]") }));

    const banner = await screen.findByTestId("account-error");
    expect(banner.textContent).toContain("Ada Lovelace is not syncing.");
    expect(banner.textContent).toContain("The server did not accept the user name or password.");
    expect(banner.textContent).toContain("NO [AUTHENTICATIONFAILED]");
    expect(within(screen.getByTestId("sync-status")).getByRole("status").textContent).toBe("Not syncing");

    await user.click(within(banner).getByRole("button", { name: "Try again" }));
    expect(core.callsOf("retry_account")).toEqual([{ accountId: ACCOUNT.id }]);

    await user.click(within(banner).getByRole("button", { name: "Change password" }));
    expect(await screen.findByTestId("account-detail")).toBeTruthy();
    expect(screen.getByLabelText("New password")).toBeTruthy();
  });

  it("a certificate problem points to the account settings instead", async () => {
    setup(status({ state: "error", error: ipcError("cert_untrusted", "UnknownIssuer") }));
    const banner = await screen.findByTestId("account-error");
    expect(banner.textContent).toContain("The server's certificate is not trusted.");
    expect(within(banner).getByRole("button", { name: "Account settings" })).toBeTruthy();
  });

  it("the rail marks an account in trouble, in its name too, not only by a symbol", async () => {
    setup(status({ state: "error", error: ipcError("auth_failed") }));
    const switcher = await screen.findByRole("list", { name: "Accounts" });
    expect(await within(switcher).findByRole("button", { name: "Ada Lovelace (ada@vicemail.test), not syncing" })).toBeTruthy();
  });

  it("the account page shows the state and lets the user change the password", async () => {
    const { core, user } = setup(status({ state: "error", error: ipcError("auth_failed") }));
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${ACCOUNT.id}`));

    expect(screen.getByTestId("account-state").textContent).toBe("Not syncing until you act");
    await user.type(screen.getByLabelText("New password"), "new-secret");
    await user.click(screen.getByRole("button", { name: "Save password" }));

    expect(await screen.findByText("Password saved. The app reconnects with it.")).toBeTruthy();
    expect(core.callsOf("update_password")).toEqual([{ accountId: ACCOUNT.id, password: "new-secret" }]);
    expect(screen.getByLabelText<HTMLInputElement>("New password").value).toBe("");
  });

  it("a wrong new password is not saved and says so", async () => {
    const { user } = setup(IDLE, { failures: { update_password: ipcError("auth_failed") } });
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${ACCOUNT.id}`));
    await user.type(screen.getByLabelText("New password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Save password" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The password was not changed.");
    expect(alert.textContent).toContain("The server did not accept the user name or password.");
  });
});

describe("removing an account", () => {
  it("asks in place, explains what is deleted, and removes only after the second click", async () => {
    const { core, user } = setup(IDLE);
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${ACCOUNT.id}`));

    await user.click(screen.getByRole("button", { name: "Remove account…" }));
    const confirm = screen.getByTestId("remove-confirm");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(confirm.textContent).toContain("Remove ada@vicemail.test from this app?");
    expect(confirm.textContent).toContain("Your mail on the server stays as it is");
    expect(document.activeElement).toBe(confirm);
    expect(core.callsOf("remove_account")).toEqual([]);

    await user.click(within(confirm).getByRole("button", { name: "Keep account" }));
    expect(screen.queryByTestId("remove-confirm")).toBeNull();
    expect(core.callsOf("remove_account")).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Remove account…" }));
    await user.click(within(screen.getByTestId("remove-confirm")).getByRole("button", { name: "Remove" }));

    expect(core.callsOf("remove_account")).toEqual([{ accountId: ACCOUNT.id }]);
    expect(await screen.findByRole("heading", { name: "No accounts yet" })).toBeTruthy();
  });

  it("a failed removal keeps the account and shows the error", async () => {
    const { user } = setup(IDLE, { failures: { remove_account: ipcError("keychain", "errSecInteractionNotAllowed") } });
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${ACCOUNT.id}`));
    await user.click(screen.getByRole("button", { name: "Remove account…" }));
    await user.click(within(screen.getByTestId("remove-confirm")).getByRole("button", { name: "Remove" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The account could not be removed.");
    expect(alert.textContent).toContain("system keychain");
    expect(screen.getByTestId("account-detail")).toBeTruthy();
  });
});
