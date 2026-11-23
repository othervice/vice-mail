import { describe, expect, it } from "vitest";
import { CoreError, mcpIpc, MIN_SYNC_PASSPHRASE_CHARS, SETTINGS_SYNC_FOLDER, settingsSyncIpc } from "../src/services/ipc";
import { makeAccount, mockCore } from "./support/core";
import { syncStatus } from "./support/mcpSync";

const ADA = makeAccount();

describe("stage 12 commands", () => {
  it("send the command names and argument names of the contract", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { enabled: true } } });

    await mcpIpc.getSettings();
    await mcpIpc.setEnabled(false);
    await mcpIpc.setAccountAccess(ADA.id, "read");
    const created = await mcpIpc.createClient("Cursor");
    await mcpIpc.revokeClient(created.client.clientId);
    await mcpIpc.listClients();
    await mcpIpc.getSetup();
    await mcpIpc.listAccess(null, 100);
    await mcpIpc.getStatus();
    await mcpIpc.testConnection();

    expect(core.calls).toEqual([
      { cmd: "get_mcp_settings", args: {} },
      { cmd: "set_mcp_enabled", args: { enabled: false } },
      { cmd: "set_mcp_account_access", args: { accountId: ADA.id, access: "read" } },
      { cmd: "create_mcp_client", args: { name: "Cursor" } },
      { cmd: "revoke_mcp_client", args: { clientId: created.client.clientId } },
      { cmd: "list_mcp_clients", args: {} },
      { cmd: "get_mcp_setup", args: {} },
      { cmd: "list_mcp_access", args: { before: null, limit: 100 } },
      { cmd: "get_mcp_status", args: {} },
      { cmd: "test_mcp_connection", args: {} },
    ]);
  });

  it("commands without a typed error still reject with a CoreError on a framework failure", async () => {
    const raw = "command get_mcp_setup not allowed by ACL";
    mockCore({ failures: { get_mcp_setup: raw, get_mcp_status: raw } });

    for (const run of [mcpIpc.getSetup, mcpIpc.getStatus]) {
      const error = await run().catch((e: unknown) => e);
      expect(error).toBeInstanceOf(CoreError);
      expect(error).toMatchObject({ code: "internal", detail: raw });
    }
  });
});

describe("stage 11 commands", () => {
  it("send the command names and argument names of the contract", async () => {
    const passphrase = "x".repeat(MIN_SYNC_PASSPHRASE_CHARS);
    const core = mockCore({
      accounts: [ADA],
      mcpSync: {
        sync: {
          status: syncStatus({ questions: [{ type: "accountRemoved", accountId: ADA.id, email: ADA.email }] }),
        },
      },
    });

    await settingsSyncIpc.getStatus();
    const on = await settingsSyncIpc.enable(ADA.id, passphrase);
    await settingsSyncIpc.previewJoin(ADA.id, passphrase);
    await settingsSyncIpc.join();
    await settingsSyncIpc.syncNow();
    await settingsSyncIpc.setDeviceName("Studio");
    await expect(settingsSyncIpc.changePassphrase(passphrase)).resolves.toBeUndefined();
    await settingsSyncIpc.reset(passphrase);
    await settingsSyncIpc.disable();

    expect(on.folder).toBe(SETTINGS_SYNC_FOLDER);
    expect(core.calls).toEqual([
      { cmd: "get_settings_sync_status", args: {} },
      { cmd: "enable_settings_sync", args: { accountId: ADA.id, passphrase } },
      { cmd: "preview_settings_sync_join", args: { accountId: ADA.id, passphrase } },
      { cmd: "join_settings_sync", args: {} },
      { cmd: "sync_settings_now", args: {} },
      { cmd: "set_sync_device_name", args: { name: "Studio" } },
      { cmd: "change_settings_sync_passphrase", args: { passphrase } },
      { cmd: "reset_settings_sync", args: { passphrase } },
      { cmd: "disable_settings_sync", args: {} },
    ]);
  });

  it("resolveSyncQuestion sends the account and the answer; the sync error codes arrive typed", async () => {
    const core = mockCore({
      accounts: [ADA],
      mcpSync: {
        sync: { status: syncStatus({ state: "active", accountId: ADA.id, questions: [{ type: "accountRemoved", accountId: ADA.id, email: ADA.email }] }) },
      },
    });
    await settingsSyncIpc.resolveQuestion(ADA.id, false);
    expect(core.callsOf("resolve_sync_question")).toEqual([{ accountId: ADA.id, remove: false }]);

    await expect(settingsSyncIpc.resolveQuestion(ADA.id, false)).rejects.toMatchObject({ name: "CoreError", code: "not_found" });
    await expect(settingsSyncIpc.previewJoin(ADA.id, "wrong passphrase")).rejects.toMatchObject({
      name: "CoreError",
      code: "sync_wrong_passphrase",
    });
  });
});
