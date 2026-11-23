// Stage 4 in the real app: actions and the outbox against the local test server (wdio.conf.mjs
// seeds 10,000 messages for alice). The account is added through the core and starts read-only.
//
// 1. Read-only: the toolbar's actions are disabled and say why, a shortcut explains it as well,
//    and nothing reaches the core's outbox.
// 2. Writes are turned on in the UI (Settings, Accounts, the switch); the core stores it.
// 3. E archives the open message: it leaves the list at once, the next one is shown, and the
//    server has it in Archive. "Undo" in the notice brings it back, on the server too.
// 4. With the server stopped, E archives another one: the list changes at once, the rail counts
//    one waiting change and the activity view shows it. After the server is started again the
//    outbox runs it by itself.
// 5. Shift+arrows select several messages (selection bar), # in the trash asks in the dialog.
// 6. No CSP violation on the way (no injected styles).
//
// Screenshots (light and dark) go to tests/e2e/screenshots/actions-*.png, the facts of the run to
// tests/e2e/screenshots/actions-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, serverFolders, testmail } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
let account;
let boxes;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `theme ${theme} not applied`,
  });
}

async function shootBothThemes(name) {
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    report.screenshots.push(await screenshot(`actions-${name}-${theme}`));
  }
  await setTheme("light");
}

/** The rendered rows: id, accessible name, selected state. */
function rows() {
  return browser.execute(() =>
    [...document.querySelectorAll('[data-testid="message-list"] [role="option"]')]
      .map((el) => ({
        id: el.id,
        label: el.getAttribute("aria-label") ?? "",
        selected: el.getAttribute("aria-selected") === "true",
        top: el.getBoundingClientRect().top,
      }))
      .sort((a, b) => a.top - b.top),
  );
}

async function showRole(role) {
  await testId(`rail-${role}`).click();
  await browser.waitUntil(async () => (await testId(`rail-${role}`).getAttribute("aria-current")) === "page");
  await testId("message-list").waitForExist({ timeout: 20000 });
}

/** Clicks a row and waits until the reading pane shows it. Returns the row. */
async function openRow(index = 0) {
  const all = await rows();
  const target = all[index];
  assert.ok(target, `no row ${index}`);
  await $(`[id="${target.id}"]`).click();
  await browser.waitUntil(async () => (await rows()).find((r) => r.id === target.id)?.selected === true, {
    timeoutMsg: "the clicked row is not selected",
  });
  await testId("reading-pane").waitForExist();
  await testId("action-toolbar").waitForExist();
  return target;
}

const noticeText = () => browser.execute(() => [...document.querySelectorAll('[data-testid="notice"]')].map((n) => n.textContent));

const pendingBadge = () => browser.execute(() => document.querySelector('[data-testid="activity-badge"]')?.textContent ?? null);

/** Waits until the server's folder counts satisfy `check`; returns them. */
async function waitForServer(check, timeoutMsg, timeout = 60000) {
  let folders;
  await browser.waitUntil(
    async () => {
      folders = await serverFolders();
      return check(folders);
    },
    { timeout, interval: 500, timeoutMsg: () => `${timeoutMsg}: ${JSON.stringify(folders)}` },
  );
  return folders;
}

const count = (folders, name) => folders[name]?.messages ?? 0;

describe("stage 4: actions and the outbox", () => {
  before(async () => {
    await attach();
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
    assert.equal(account.writesEnabled, false, "new accounts are read-only");
    boxes = await coreCall("list_mailboxes", { accountId: account.id });
    // Marking read on open would add operations of its own; this run looks at the actions alone.
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    assert.deepEqual(await cspViolations(), [], "CSP violations before the actions");
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "actions-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("explains why a read-only account changes nothing", async () => {
    await showRole("inbox");
    await openRow(0);
    const disabled = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="action-toolbar"] button')].map((b) => b.getAttribute("aria-disabled")),
    );
    assert.ok(disabled.length >= 6 && disabled.every((d) => d === "true"), `toolbar not disabled: ${disabled}`);
    const note = await testId("read-only-actions").getText();
    assert.match(note, /Read-only/);
    await browser.keys(["e"]);
    await browser.waitUntil(async () => (await noticeText()).some((t) => t.includes("Changes are turned off for this account.")), {
      timeoutMsg: "the shortcut did not explain the read-only account",
    });
    const invoked = await coreCall("e2e_invoked");
    assert.equal(invoked.apply_action ?? 0, 0, "a read-only account sent an action to the core");
    report.steps.readOnly = { note, notices: await noticeText() };
    await shootBothThemes("read-only");
  });

  it("turns writes on with the switch in the account settings", async () => {
    await testId("allow-changes").click();
    await testId("account-detail").waitForExist();
    const toggle = testId("writes-switch");
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    await toggle.click();
    await browser.waitUntil(async () => (await toggle.getAttribute("aria-checked")) === "true", { timeoutMsg: "the switch did not turn on" });
    await browser.waitUntil(async () => (await onlyAccount()).writesEnabled === true, { timeoutMsg: "the core did not store it" });
    report.steps.writes = { state: await testId("writes-state").getText() };
    await shootBothThemes("writes-switch");
  });

  it("archives with E at once, on the server too, and Undo brings the message back", async () => {
    const before = await serverFolders();
    report.steps.serverBefore = { INBOX: count(before, "INBOX"), Archive: count(before, "Archive") };
    await showRole("inbox");
    const target = await openRow(0);
    const next = (await rows())[1];

    await browser.keys(["e"]);

    await browser.waitUntil(async () => !(await rows()).some((r) => r.id === target.id), { timeout: 2000, timeoutMsg: "the row did not leave the list" });
    await browser.waitUntil(async () => (await rows()).find((r) => r.selected)?.id === next.id, { timeoutMsg: "the next mail is not selected" });
    await browser.waitUntil(async () => (await noticeText()).some((t) => t.includes("Archived")), { timeoutMsg: "no undo notice" });
    report.screenshots.push(await screenshot("actions-undo-notice-light"));
    const archived = await waitForServer(
      (f) => count(f, "Archive") === count(before, "Archive") + 1 && count(f, "INBOX") === count(before, "INBOX") - 1,
      "the server did not archive the message",
    );
    report.steps.archived = { INBOX: count(archived, "INBOX"), Archive: count(archived, "Archive") };

    await testId("notice-undo").click();

    await browser.waitUntil(async () => (await rows()).some((r) => r.label === target.label), { timeout: 10000, timeoutMsg: "the row did not come back" });
    const undone = await waitForServer(
      (f) => count(f, "Archive") === count(before, "Archive") && count(f, "INBOX") === count(before, "INBOX"),
      "the server did not take the archive back",
    );
    report.steps.undone = { INBOX: count(undone, "INBOX"), Archive: count(undone, "Archive") };
  });

  it("queues an action while the server is down and runs it once it is back", async () => {
    const before = await serverFolders();
    await testmail("stop");
    let target;
    try {
      await browser.waitUntil(async () => (await onlyAccount()).status.state === "offline", {
        timeout: 30000,
        interval: 100,
        timeoutMsg: "the account did not go offline",
      });
      await showRole("inbox");
      target = await openRow(0);
      await browser.keys(["e"]);
      await browser.waitUntil(async () => !(await rows()).some((r) => r.id === target.id), { timeout: 2000, timeoutMsg: "offline: the row did not leave the list" });
      await browser.waitUntil(async () => (await pendingBadge()) === "1", { timeout: 5000, timeoutMsg: "the rail does not count the waiting change" });
      report.steps.offline = { badge: await pendingBadge(), label: await testId("rail-activity").getAttribute("aria-label") };
      await shootBothThemes("offline-pending");
      await testId("rail-activity").click();
      await testId("activity-view").waitForExist();
      await browser.waitUntil(async () => (await testId("activity-view").getText()).includes("Waiting"), { timeoutMsg: "the activity view does not show the waiting change" });
      report.steps.activityOffline = (await testId("activity-view").getText()).slice(0, 400);
      await shootBothThemes("activity");
    } finally {
      await testmail("start");
    }
    const after = await waitForServer(
      (f) => count(f, "Archive") === count(before, "Archive") + 1 && count(f, "INBOX") === count(before, "INBOX") - 1,
      "the queued archive did not reach the server after the restart",
      120000,
    );
    report.steps.afterRestart = { INBOX: count(after, "INBOX"), Archive: count(after, "Archive") };
    await browser.waitUntil(async () => (await pendingBadge()) === null, { timeout: 30000, timeoutMsg: "the waiting change is still counted" });
    await browser.waitUntil(async () => (await testId("activity-view").getText()).includes("Done"), { timeoutMsg: "the activity view does not show it done" });
  });

  it("selects several messages with Shift and asks before deleting for good in the trash", async () => {
    await showRole("inbox");
    await openRow(0);
    await browser.keys(["Shift", "ArrowDown", "ArrowDown"]);
    await browser.keys(["Shift"]);
    await testId("selection-bar").waitForExist({ timeout: 5000 });
    const countText = await testId("selection-count").getText();
    assert.equal(countText, "3 selected");
    report.steps.selection = countText;
    await shootBothThemes("selection");
    await browser.keys(["Escape"]);

    // One into the trash, then delete it there for good: the dialog.
    await openRow(0);
    await browser.keys(["#"]);
    await browser.waitUntil(async () => (await noticeText()).some((t) => t.includes("Moved to the trash")), { timeoutMsg: "no delete notice" });
    await showRole("trash");
    await openRow(0);
    await browser.keys(["#"]);
    const dialog = testId("confirm-delete");
    await dialog.waitForExist({ timeout: 5000 });
    const focused = await browser.execute(() => document.activeElement?.getAttribute("data-testid"));
    assert.equal(focused, "confirm-dialog-cancel", "the dialog does not start on Cancel");
    report.steps.dialog = await dialog.getText();
    await shootBothThemes("delete-dialog");
    await browser.keys(["Escape"]);
    await dialog.waitForExist({ reverse: true, timeout: 5000 });
    assert.deepEqual(await cspViolations(), [], "CSP violations during the actions");
    report.steps.boxes = boxes.map((m) => `${m.path}:${m.role}`);
  });
});
