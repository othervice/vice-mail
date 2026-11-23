// Stages 12 and 11 in the real app, against the local test server (last in `specs` of
// wdio.conf.mjs, after actions.e2e.mjs and compose.e2e.mjs, which leave no account behind).
//
// AI over MCP (stage 12):
// 1. MCP is off after setup; turning it on asks first and explains what a cloud client does.
// 2. The account is shared for reading; "Test connection" reaches the app through the helper.
// 3. A client is added: its token is shown once inside the configurations and gone after "Done".
// 4. The helper program is started with that configuration like an AI client would start it and
//    calls `list_accounts`: the call appears live in "Access". After "Revoke" the same token is
//    refused at once, and that refusal appears in "Access" too.
// 5. The emergency stop in "Access" turns MCP off; the core reports `stopped`.
//
// Settings sync (stage 11):
// 6. Turned on for alice with a passphrase: the state is "On", the server has the folder
//    Settings-Sync, and the app's folder list does not show it.
// 7. The device is renamed, "Sync now" runs, and the sync is turned off again.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, testId } from "./support/app.mjs";
import { ALICE, SERVER, serverFolders } from "./support/testmail.mjs";

const PASSPHRASE = "e2e passphrase for the settings sync";
let account;

/** Starts the helper as `config` (Claude Desktop JSON) says and makes one tool call over stdio. */
function callHelper(config, tool) {
  const server = Object.values(JSON.parse(config).mcpServers)[0];
  return new Promise((resolve, reject) => {
    const child = spawn(server.command, server.args ?? [], { env: { ...process.env, ...server.env }, stdio: ["pipe", "pipe", "pipe"] });
    const answers = new Map();
    let buffer = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`helper timed out: ${stderr}`));
    }, 20000);
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.stdout.on("data", (chunk) => {
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        const message = JSON.parse(line);
        if (message.id !== undefined) answers.set(message.id, message);
        if (message.id === 1) {
          send({ jsonrpc: "2.0", method: "notifications/initialized" });
          send({ jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: tool, arguments: {} } });
        }
        if (message.id === 2) {
          clearTimeout(timer);
          child.stdin.end();
          child.kill();
          resolve({ initialize: answers.get(1), call: message, stderr });
        }
      }
    });
    child.on("exit", (code) => {
      if (!answers.has(2)) {
        clearTimeout(timer);
        resolve({ initialize: answers.get(1) ?? null, call: null, exitCode: code, stderr });
      }
    });
    const send = (message) => child.stdin.write(`${JSON.stringify(message)}\n`);
    send({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } },
    });
  });
}

async function waitForCore(command, check, timeoutMsg) {
  await browser.waitUntil(async () => check(await coreCall(command)), { timeout: 20000, interval: 250, timeoutMsg });
}

describe("stage 12: AI over MCP", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    account = await coreCall("add_account", {
      settings: {
        email: ALICE.email,
        displayName: "Alice Archer",
        imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: ALICE.email },
        smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: ALICE.email },
      },
      password: ALICE.password,
    });
  });

  after(async () => {
    await coreCall("set_mcp_enabled", { enabled: false }).catch(() => undefined);
  });

  it("is off until turned on, and asks first", async () => {
    await openSettings("mcp");
    const toggle = testId("mcp-enabled");
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    assert.equal((await coreCall("get_mcp_status")).state, "stopped");
    await toggle.click();
    const ask = testId("mcp-enable-confirm");
    assert.match(await ask.getText(), /sends the mails it reads to its provider/);
    assert.equal((await coreCall("get_mcp_settings")).enabled, false, "nothing changed before the confirmation");
    await ask.$('[data-testid="inline-confirm"]').click();
    await waitForCore("get_mcp_status", (status) => status.state === "listening", "MCP did not start listening");
  });

  it("shares the account for reading and tests the connection", async () => {
    await testId(`mcp-share-${account.id}`).$('[role="radio"][value="read"]').click();
    await waitForCore(
      "get_mcp_settings",
      (settings) => settings.accounts.some((share) => share.accountId === account.id && share.access === "read"),
      "the share was not stored",
    );
    await testId("mcp-test").click();
    const ok = testId("mcp-test-ok");
    await ok.waitForDisplayed({ timeout: 25000 });
    assert.match(await ok.getText(), /AI clients see 1 account/);
  });

  it("shows a new client's token once, logs its access live and refuses it after revoking", async () => {
    await testId("mcp-client-name").setValue("e2e client");
    await testId("mcp-add-submit").click();
    const created = testId("mcp-created");
    await created.waitForDisplayed();
    const config = await testId("mcp-config-content-claude_desktop").getText();
    assert.match(config, /mcp1_[0-9a-f]{32}_/);

    await testId("mcp-created-done").click();
    await created.waitForDisplayed({ reverse: true });
    const pageText = await $("body").getText();
    assert.ok(!pageText.includes("mcp1_"), "the token is gone from the page");

    await testId("mcp-page-access").click();
    const answer = await callHelper(config, "list_accounts");
    assert.ok(answer.call && !answer.call.error, `list_accounts failed: ${JSON.stringify(answer)}`);
    await browser.waitUntil(async () => (await $("[data-testid='mcp-access-table']").getText()).includes("List accounts"), {
      timeout: 10000,
      timeoutMsg: "the access did not appear in Access",
    });

    await testId("mcp-page-settings").click();
    const client = (await coreCall("list_mcp_clients")).find((c) => c.name === "e2e client");
    await testId(`mcp-client-${client.clientId}`).$('[data-testid="mcp-revoke"]').click();
    await waitForCore("list_mcp_clients", (clients) => clients.some((c) => c.clientId === client.clientId && c.revokedAt), "not revoked");
    const refused = await callHelper(config, "list_accounts");
    assert.ok(!refused.call || refused.call.error || refused.call.result?.isError, `a revoked token still works: ${JSON.stringify(refused)}`);
    // The host reports the refusal after answering the helper, and a task writes the log in
    // batches (decision 0021): the entry comes shortly after the helper has its answer.
    let log;
    await browser
      .waitUntil(
        async () => {
          log = await coreCall("list_mcp_access", { before: null, limit: 10 });
          return log.entries[0]?.outcome === "refused";
        },
        { timeout: 10000, interval: 250, timeoutMsg: "the refusal is not in the access log" },
      )
      .catch((e) => {
        throw new Error(`${e.message}: ${JSON.stringify(log?.entries.slice(0, 3))}`);
      });
  });

  it("stops everything with the emergency stop in Access", async () => {
    await testId("mcp-page-access").click();
    await testId("mcp-stop").click();
    await waitForCore("get_mcp_status", (status) => status.state === "stopped", "MCP did not stop");
    await testId("mcp-access-off").waitForDisplayed();
    assert.deepEqual(await cspViolations(), []);
  });
});

describe("stage 11: settings sync", () => {
  after(async () => {
    await coreCall("disable_settings_sync").catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch(() => undefined);
  });

  it("turns on with a passphrase and writes only its own folder", async () => {
    await openSettings("sync");
    const form = testId("sync-enable-form");
    await form.waitForDisplayed();
    await testId("sync-enable-passphrase").setValue(PASSPHRASE);
    await testId("sync-enable-repeat").setValue(PASSPHRASE);
    await testId("sync-enable-submit").click();
    await browser.waitUntil(async () => (await testId("sync-state").getText()) === "On", {
      timeout: 60000,
      timeoutMsg: "the sync did not turn on",
    });
    const folders = await serverFolders();
    assert.ok("Settings-Sync" in folders, `no sync folder on the server: ${Object.keys(folders).join(", ")}`);
    const boxes = await coreCall("list_mailboxes", { accountId: account.id });
    assert.ok(!boxes.some((box) => box.path.includes("Settings-Sync")), "the sync folder shows in the folder list");
  });

  it("renames the device, syncs now and turns off", async () => {
    const field = testId("sync-device-name-field");
    await field.clearValue();
    await field.setValue("E2E Mac");
    await testId("sync-device-name").$('button[type="submit"]').click();
    await waitForCore(
      "get_settings_sync_status",
      (status) => status.devices.some((d) => d.current && d.name === "E2E Mac"),
      "the device name was not stored",
    );
    await testId("sync-now").click();
    await waitForCore("get_settings_sync_status", (status) => status.state === "active" && status.lastSyncAt !== null, "no pass");
    await testId("sync-disable-submit").click();
    await waitForCore("get_settings_sync_status", (status) => status.state === "off", "the sync did not turn off");
  });
});
