// Stage 18, task D in the real app against the local test server (docs/stages/18, acceptance
// "Ordner anlegen per Desktop-Test"):
// 1. A read-only account offers "New folder…" and "New subfolder…" but says why they do nothing;
//    nothing is created.
// 2. With changes on: "New folder…" on the account's row creates a folder at the top of the
//    account, "New subfolder…" one below it; both exist on the server (IMAP LIST), show in the
//    tree under their parent, and the new folder takes mail (a move, seen on the server).
// 3. A folder created directly on the server (another program) shows after "Reload folders from
//    server", without a restart.
// The folders are removed on the server again at the end; the moved message goes back first.
// Screenshots go to tests/e2e/screenshots/folders-*.png, the facts to folders-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, setConversations, setSidebar, testId } from "./support/app.mjs";
import { ALICE, createServerFolder, deleteServerFolders, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const stamp = String(Date.now()).slice(-6);
const TOP = `E2E Kunden ${stamp}`;
const SUB = "Archiv 2026";
const OTHER = `E2E Elsewhere ${stamp}`;
let account;
let preferencesBefore = null;
let sidebarBefore = null;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

const mailboxes = () => coreCall("list_mailboxes", { accountId: account.id });

/** The tree item of the folder with this path in the account's tree, as shown. */
const folderRow = (folderPath) =>
  browser.execute((p) => {
    const row = [...document.querySelectorAll('[data-testid="sidebar-column"] [role="treeitem"]')].find((r) => r.getAttribute("title") === p);
    return row ? { level: row.getAttribute("aria-level"), name: row.getAttribute("aria-label"), testId: row.getAttribute("data-testid") } : null;
  }, folderPath);

/** Opens the "…" of the account's row (or of a folder's) and clicks the item with `itemId`. */
async function menuItem(menuTestId, itemId) {
  await testId(menuTestId).click();
  const item = testId(itemId);
  await item.waitForExist({ timeout: 5000 });
  const state = { inert: (await item.getAttribute("aria-disabled")) === "true", label: await item.getText() };
  await item.click();
  return state;
}

/** Types a name into the open "New folder" panel and confirms. */
async function createInPanel(name) {
  const field = testId("folder-create-name");
  await field.waitForExist({ timeout: 5000 });
  await field.setValue(name);
  await testId("folder-create-confirm").click();
  await testId("folder-create-panel").waitForExist({ reverse: true, timeout: 5000 });
}

async function waitServerFolder(folderPath, timeoutMsg) {
  let folders = {};
  await browser.waitUntil(
    async () => {
      folders = await serverFolders();
      return folderPath in folders;
    },
    { timeout: 20000, interval: 500, timeoutMsg },
  );
  return folders[folderPath];
}

describe("stage 18: folders created in the app exist on the server; the server's show without a restart", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await coreCall("set_preferences", { preferences: { ...preferencesBefore, language: "en", theme: "light" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
      timeoutMsg: "the app did not switch to English",
    });
    await browser.setWindowSize(1280, 800);
    await coreCall("add_account", {
      settings: {
        email: ALICE.email,
        displayName: "Alice Archer",
        imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: ALICE.email },
        smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: ALICE.email },
      },
      password: ALICE.password,
    });
    await browser.waitUntil(
      async () => {
        const a = await onlyAccount();
        return a.status.state === "idle" && a.status.lastSyncAt !== null;
      },
      { timeout: 240000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    account = await onlyAccount();
    await setConversations(false);
    sidebarBefore = await setSidebar(true);
    await testId(`account-menu-${account.id}`).waitForExist({ timeout: 10000 });
  });

  after(async () => {
    await deleteServerFolders([TOP, `${TOP}/${SUB}`, OTHER]).catch((e) => console.log(`server cleanup failed: ${e}`));
    if (sidebarBefore !== null) await setSidebar(sidebarBefore).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "folders-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("a read-only account keeps the actions but creates nothing and says why", async () => {
    assert.equal(account.writesEnabled, false, "new accounts start read-only");
    const state = await menuItem(`account-menu-${account.id}`, "folder-create");
    report.steps.readOnly = state;
    assert.equal(state.inert, true, "New folder… is not marked as unavailable");
    assert.equal(await testId("folder-create-panel").isExisting(), false, "a panel opened on a read-only account");
    await browser.waitUntil(
      async () => (await browser.execute(() => [...document.querySelectorAll('[data-testid="notice"]')].map((n) => n.textContent))).some((t) => t.includes("Changes are turned off")),
      { timeout: 5000, timeoutMsg: "no notice says why" },
    );
    const inbox = (await mailboxes()).find((m) => m.role === "inbox");
    const sub = await menuItem(`folder-menu-${String(inbox.id)}`, "folder-create-sub");
    assert.equal(sub.inert, true, "New subfolder… is not marked as unavailable");
    assert.equal(await testId("folder-create-panel").isExisting(), false);
    const operations = await coreCall("list_operations", { accountId: account.id, before: null, limit: 50 });
    assert.equal(operations.filter((o) => o.kind === "create_mailbox").length, 0, "something was recorded");
    report.screenshots.push(await screenshot("folders-read-only"));
  });

  it("creates a folder and a subfolder on the server; they show in the tree and take mail", async () => {
    account = await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    assert.equal(account.writesEnabled, true);
    // At the top of the account.
    await menuItem(`account-menu-${account.id}`, "folder-create");
    report.screenshots.push(await screenshot("folders-new-panel"));
    await createInPanel(TOP);
    report.steps.top = await waitServerFolder(TOP, `“${TOP}” was not created on the server`);
    await browser.waitUntil(async () => (await folderRow(TOP)) !== null, { timeout: 15000, timeoutMsg: "the new folder is not in the tree" });
    const top = (await mailboxes()).find((m) => m.path === TOP);
    assert.ok(top, "the core does not list the new folder");
    assert.equal(top.parentId, null);

    // Below it, from its own menu.
    await menuItem(`folder-menu-${String(top.id)}`, "folder-create-sub");
    await createInPanel(SUB);
    const subPath = `${TOP}/${SUB}`;
    report.steps.sub = await waitServerFolder(subPath, `“${subPath}” was not created on the server`);
    await browser.waitUntil(async () => (await folderRow(subPath)) !== null, { timeout: 15000, timeoutMsg: "the subfolder is not in the tree" });
    const sub = (await mailboxes()).find((m) => m.path === subPath);
    assert.equal(sub.parentId, top.id, "the subfolder is not below its parent");
    report.steps.tree = { top: await folderRow(TOP), sub: await folderRow(subPath) };
    assert.equal(Number(report.steps.tree.sub.level), Number(report.steps.tree.top.level) + 1, "the subfolder is not one level deeper");
    report.screenshots.push(await screenshot("folders-created"));

    // A taken name is refused in place, nothing is sent.
    await menuItem(`account-menu-${account.id}`, "folder-create");
    await testId("folder-create-name").setValue(TOP);
    await testId("folder-create-confirm").click();
    const problem = await browser.execute(() => document.querySelector('[data-testid="folder-create-panel"]')?.textContent ?? "");
    assert.ok(problem.includes("exists here already"), problem);
    await browser.keys("Escape");
    await testId("folder-create-panel").waitForExist({ reverse: true, timeout: 5000 });

    // The new folder takes mail: a move into it reaches the server, and back.
    const inbox = (await mailboxes()).find((m) => m.role === "inbox");
    const [first] = (await coreCall("list_messages", { mailboxId: inbox.id, cursor: null, limit: 1 })).items;
    const requestId = () => `e2e-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    await coreCall("apply_action", { action: { type: "move", target: sub.id }, messages: [{ mailboxId: inbox.id, messageId: first.id }], requestId: requestId() });
    await browser.waitUntil(async () => (await serverFolders())[subPath]?.messages === 1, { timeout: 20000, timeoutMsg: "the move did not reach the server" });
    report.steps.moved = (await serverFolders())[subPath];
    let moved = [];
    await browser.waitUntil(
      async () => {
        moved = (await coreCall("list_messages", { mailboxId: sub.id, cursor: null, limit: 5 })).items;
        return moved.length === 1;
      },
      { timeout: 20000, timeoutMsg: "the moved message is not listed in the new folder" },
    );
    await coreCall("apply_action", { action: { type: "move", target: inbox.id }, messages: [{ mailboxId: sub.id, messageId: moved[0].id }], requestId: requestId() });
    await browser.waitUntil(async () => (await serverFolders())[subPath]?.messages === 0, { timeout: 20000, timeoutMsg: "the message did not go back" });
  });

  it("a folder created on the server shows after “Reload folders from server”, without a restart", async () => {
    await createServerFolder(OTHER);
    // Not before the next full pass (every 3 minutes): the reload brings it at once.
    const before = (await mailboxes()).some((m) => m.path === OTHER);
    report.steps.otherBeforeReload = before;
    const state = await menuItem(`account-menu-${account.id}`, "folders-reload");
    assert.equal(state.inert, false);
    const started = Date.now();
    await browser.waitUntil(async () => (await folderRow(OTHER)) !== null, { timeout: 30000, timeoutMsg: "the server's folder did not show after the reload" });
    report.steps.otherShownAfterMs = Date.now() - started;
    report.screenshots.push(await screenshot("folders-reloaded"));
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
