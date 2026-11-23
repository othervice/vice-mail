// Decision 0040 (security review F1): server settings another device set for an account wait on
// the account's page; the user takes them (writing off, password asked for again) or keeps the
// current ones.
import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SyncServerChange } from "../src/services/ipc";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { syncStatus } from "./support/mcpSync";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const CHANGE: SyncServerChange = {
  accountId: ADA.id,
  imap: { host: "imap.elsewhere.example", port: 993, security: "tls", username: "ada" },
  smtp: { host: "smtp.elsewhere.example", port: 587, security: "starttls", username: "ada" },
};

function waiting() {
  return syncStatus({ state: "active", accountId: ADA.id, folder: "Settings-Sync", serverChanges: [CHANGE] });
}

async function accountDetail(user: ReturnType<typeof renderApp>["user"], id: string) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Accounts" }));
  await user.click(await screen.findByTestId(`account-row-${id}`));
  return screen.findByTestId("account-detail");
}

describe("Settings, accounts: server settings changed on another device", () => {
  it("shows the new servers on that account only, and the current ones stay in use", async () => {
    mockCore({ accounts: [ADA, WORK], mcpSync: { sync: { status: waiting() } } });
    const { user } = renderApp();
    const detail = await accountDetail(user, ADA.id);

    const notice = await within(detail).findByTestId("server-change");
    expect(notice.textContent).toContain("Server settings changed on another device");
    expect(notice.textContent).toContain("your password is not sent to the new ones");
    expect(notice.textContent).toContain("imap.elsewhere.example");
    expect(notice.textContent).toContain("smtp.elsewhere.example");
    expect(notice.textContent).toContain("enter the password for the new server afterwards");
    // The servers in use are the account's own.
    expect(detail.textContent).toContain("localhost");

    await user.click(within(detail).getByRole("button", { name: "All accounts" }));
    await user.click(await screen.findByTestId(`account-row-${WORK.id}`));
    const other = await screen.findByTestId("account-detail");
    expect(within(other).queryByTestId("server-change")).toBeNull();
  });

  it("keeps the current servers when asked to", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: waiting() } } });
    const { user } = renderApp();
    const detail = await accountDetail(user, ADA.id);
    const notice = await within(detail).findByTestId("server-change");

    await user.click(within(notice).getByRole("button", { name: "Keep the current servers" }));
    await waitFor(() => {
      expect(within(detail).queryByTestId("server-change")).toBeNull();
    });
    expect(core.callsOf("resolve_server_change")).toEqual([{ accountId: ADA.id, apply: false }]);
    expect(core.accounts[0]?.imap.host).toBe("localhost");
    expect(core.accounts[0]?.writesEnabled).toBe(true);
  });

  it("takes the new servers: writing switches off and the account is read-only", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: waiting() } } });
    const { user } = renderApp();
    const detail = await accountDetail(user, ADA.id);
    const notice = await within(detail).findByTestId("server-change");

    await user.click(within(notice).getByRole("button", { name: "Use the new servers" }));
    await waitFor(() => {
      expect(within(detail).queryByTestId("server-change")).toBeNull();
    });
    expect(core.callsOf("resolve_server_change")).toEqual([{ accountId: ADA.id, apply: true }]);
    await waitFor(() => {
      expect(within(screen.getByTestId("account-detail")).getByTestId("read-only-account")).toBeTruthy();
    });
    expect(screen.getByTestId("account-detail").textContent).toContain("imap.elsewhere.example");
  });

  it("shows a failure in place and keeps the question", async () => {
    const core = mockCore({
      accounts: [ADA],
      mcpSync: { sync: { status: waiting() } },
      failures: { resolve_server_change: ipcError("keychain", "denied") },
    });
    const { user } = renderApp();
    const detail = await accountDetail(user, ADA.id);
    const notice = await within(detail).findByTestId("server-change");

    await user.click(within(notice).getByRole("button", { name: "Use the new servers" }));
    expect(await within(notice).findByText("The server change could not be resolved.")).toBeTruthy();
    expect(within(detail).getByTestId("server-change")).toBeTruthy();
    expect(core.accounts[0]?.imap.host).toBe("localhost");
  });
});
