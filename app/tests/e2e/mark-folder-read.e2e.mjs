// Stage 21, items 2 and 3 (decision 0063) in the real app against the local test server:
// 1. A read-only account: the right click on a folder with unread mail opens its menu; "Mark folder
//    as read" is there, unavailable, with its reason; a click records nothing and the server keeps
//    the mail unread.
// 2. With changes on: right click, "Mark folder as read": the folder's counter drops at once, the
//    server has \Seen on every message (IMAP STATUS, UNSEEN 0); "Undo" makes them unread there
//    again. Shift+F10 on the row opens the same menu.
// 3. The column at its narrowest (200 px), German and English, light and dark: the folder's menu
//    lies inside the window and inside the column, no entry is cut off, the column never scrolls
//    sideways (scrollLeft 0, also after moving through the menu and after being pushed); a menu at
//    the bottom of the column stays inside the window too.
// The spec sets up alice itself and works in a folder of its own (removed at the end), so the
// seed stays as it is. Screenshots: tests/e2e/screenshots/mark-read-*.png; facts:
// mark-read-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSidebarParts, screenshot, SCREENSHOT_DIR, setSidebar, testId } from "./support/app.mjs";
import { ALICE, appendMessages, deleteServerFolders, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const stamp = String(Date.now()).slice(-6);
const FOLDER = `E2E Ungelesen ${stamp}`;
const COUNT = 4;
let account;
let folder;
let preferencesBefore = null;
let sidebarBefore = null;
let widthBefore = null;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

const mailboxes = () => coreCall("list_mailboxes", { accountId: account.id });
const localFolder = async () => (await mailboxes()).find((m) => m.path === FOLDER);
const serverUnseen = async () => (await serverFolders())[FOLDER]?.unseen;

const notices = () => browser.execute(() => [...document.querySelectorAll('[data-testid="notice"]')].map((n) => n.textContent ?? ""));

/** Messages of our own, unread, in a folder of our own on the server. */
function emlFiles() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vice-mark-read-"));
  return Array.from({ length: COUNT }, (_, i) => {
    const file = path.join(dir, `mark-${String(i)}.eml`);
    fs.writeFileSync(
      file,
      [
        "From: Grace Hopper <grace-mark@example.org>",
        `To: ${ALICE.email}`,
        `Subject: Mark read ${stamp} ${String(i + 1)}`,
        `Date: Wed, 25 Nov 2026 10:0${String(i)}:00 +0000`,
        `Message-ID: <mark-read-${stamp}-${String(i)}@example.org>`,
        "",
        `Body ${String(i + 1)}.`,
        "",
      ].join("\r\n"),
    );
    return file;
  });
}

/**
 * Opens the row's menu the way a right click does: the embedded WebDriver has no pointer actions
 * for the right button (and none for scrolling), so the row is scrolled into view in the page and
 * gets the `contextmenu` event the web view fires on a right click (the app keeps the web view's
 * own menu shut and opens the row's).
 */
async function rightClickMenu(rowTestId) {
  const prevented = await browser.execute((id) => {
    const target = document.querySelector(`[data-testid="${id}"]`);
    target.scrollIntoView({ block: "center" });
    const box = target.getBoundingClientRect();
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, buttons: 2, clientX: box.left + 24, clientY: box.top + box.height / 2 });
    target.dispatchEvent(event);
    return event.defaultPrevented;
  }, rowTestId);
  assert.equal(prevented, true, "the web view's own menu was not kept shut");
  await testId("overflow-menu").waitForExist({ timeout: 5000 });
}

/** Shift+F10 on the focused row (keys as the embedded WebDriver sends them, else the key event). */
async function shiftF10(rowTestId) {
  await browser.execute((id) => document.querySelector(`[data-testid="${id}"]`)?.focus(), rowTestId);
  await browser.keys(["Shift", "F10"]);
  if (await testId("overflow-menu").waitForExist({ timeout: 2000 }).catch(() => false)) return "keys";
  await browser.execute((id) => {
    document.querySelector(`[data-testid="${id}"]`)?.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true, cancelable: true }));
  }, rowTestId);
  await testId("overflow-menu").waitForExist({ timeout: 5000 });
  return "key event";
}

/** Escape in the open menu (its first entry takes the focus again: a screenshot blurs it). */
async function closeMenu() {
  if (!(await testId("overflow-menu").isExisting())) return;
  await browser.execute(() => document.querySelector('[data-testid="overflow-menu"] [role="menuitem"]')?.focus({ preventScroll: true }));
  await browser.keys("Escape");
  await testId("overflow-menu").waitForExist({ reverse: true, timeout: 5000 });
}

/** No update question and no notices over the column in a screenshot. */
async function clearOverlays() {
  if (await testId("update-consent-no").isExisting()) {
    await testId("update-consent-no").click();
    await testId("update-consent").waitForExist({ reverse: true, timeout: 5000 });
  }
  await browser.execute(() => {
    for (const close of document.querySelectorAll('[data-testid="notice"] button[aria-label]')) {
      if (close instanceof HTMLButtonElement && !close.dataset["testid"]) close.click();
    }
  });
}

/** Whether the open menu can be seen (its row's "…" slot is not transparent). */
const menuVisible = () =>
  browser.execute(() => {
    let node = document.querySelector('[data-testid="overflow-menu"]');
    for (; node; node = node.parentElement) if (Number(getComputedStyle(node).opacity) === 0) return false;
    return true;
  });

/** Where the open menu lies, against the window, the column and its scrolling area. */
const geometry = () =>
  browser.execute(() => {
    const menu = document.querySelector('[data-testid="overflow-menu"]');
    const scroll = document.querySelector('[data-testid="sidebar-scroll"]');
    const column = document.querySelector('[data-testid="sidebar-column"]');
    if (!menu || !scroll || !column) return null;
    const box = (el) => {
      const r = el.getBoundingClientRect();
      return { left: Math.round(r.left), right: Math.round(r.right), top: Math.round(r.top), bottom: Math.round(r.bottom), width: Math.round(r.width) };
    };
    const items = [...menu.querySelectorAll('[role="menuitem"]')].map((item) => ({
      label: item.getAttribute("aria-label"),
      height: Math.round(item.getBoundingClientRect().height),
      cut: [...item.querySelectorAll("span")].some((s) => s.scrollWidth > s.clientWidth + 1),
    }));
    return {
      menu: box(menu),
      column: box(column),
      visible: box(scroll),
      window: { width: window.innerWidth, height: window.innerHeight },
      side: menu.getAttribute("data-side"),
      menuScrolls: menu.scrollHeight > menu.clientHeight + 1,
      scrollLeft: scroll.scrollLeft,
      scrollWidth: scroll.scrollWidth,
      clientWidth: scroll.clientWidth,
      columnWidth: column.getBoundingClientRect().width,
      items,
    };
  });

function assertInside(g, what) {
  assert.ok(g, `${what}: no menu`);
  const { menu, column, window: w } = g;
  assert.ok(menu.left >= 0 && menu.right <= w.width && menu.top >= 0 && menu.bottom <= w.height, `${what}: outside the window ${JSON.stringify(g)}`);
  assert.ok(menu.left >= column.left && menu.right <= column.right, `${what}: wider than the column ${JSON.stringify(g)}`);
  assert.equal(g.scrollLeft, 0, `${what}: the column scrolled sideways`);
  assert.ok(g.scrollWidth <= g.clientWidth, `${what}: the column can scroll sideways ${JSON.stringify(g)}`);
  for (const item of g.items) {
    assert.equal(item.cut, false, `${what}: “${item.label}” is cut off`);
    // One line is 32 px; a wrapped name grows the entry with its padding (two lines: 44 px).
    assert.ok(item.height === 32 || item.height >= 44, `${what}: “${item.label}” is ${String(item.height)} px high`);
  }
}

async function setLanguage(language, theme) {
  const preferences = await coreCall("get_preferences");
  await coreCall("set_preferences", { preferences: { ...preferences, language, theme } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === language, {
    timeoutMsg: `the app did not switch to ${language}`,
  });
  await browser.pause(200);
}

const columnWidth = async () => Number(await testId("sidebar-resize").getAttribute("aria-valuenow"));

describe("stage 21: mark a folder as read from its menu; the column's menus fit", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await setLanguage("en", "light");
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
    sidebarBefore = await setSidebar(true);
    report.steps.appended = await appendMessages(FOLDER, emlFiles());
    await coreCall("reload_folders", { accountId: account.id });
    await browser.waitUntil(async () => (await localFolder())?.unread === COUNT, { timeout: 60000, interval: 500, timeoutMsg: "the new folder was not synced" });
    folder = await localFolder();
    // Stage 23 (decision 0085): the own folders are under the account's "More folders".
    await openSidebarParts();
    await testId(`mailbox-${String(folder.id)}`).waitForExist({ timeout: 10000 });
  });

  after(async () => {
    await closeMenu().catch(() => undefined);
    if (widthBefore !== null) {
      // Back to the width before: Home, then the steps of 16 px.
      await browser.execute(() => document.querySelector('[data-testid="sidebar-resize"]')?.focus());
      await browser.keys("Home");
      for (let w = 200; w < widthBefore; w += 16) await browser.keys("ArrowRight");
    }
    await deleteServerFolders([FOLDER]).catch((e) => console.log(`server cleanup failed: ${e}`));
    if (sidebarBefore !== null) await setSidebar(sidebarBefore).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "mark-read-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("a read-only account shows the entry unavailable with its reason and records nothing", async () => {
    assert.equal(account.writesEnabled, false, "new accounts start read-only");
    await rightClickMenu(`mailbox-${String(folder.id)}`);
    const item = testId("mark-read-folder");
    await item.waitForExist({ timeout: 5000 });
    report.steps.readOnly = { disabled: await item.getAttribute("aria-disabled"), text: await item.getText() };
    assert.equal(report.steps.readOnly.disabled, "true");
    assert.ok(report.steps.readOnly.text.includes("Changes are turned off for this account."), report.steps.readOnly.text);
    report.screenshots.push(await screenshot("mark-read-read-only"));
    await item.click();
    await browser.waitUntil(async () => (await notices()).some((t) => t.includes("Allow changes")), { timeout: 5000, timeoutMsg: "no notice says why" });
    const operations = await coreCall("list_operations", { accountId: account.id, before: null, limit: 50 });
    assert.equal(operations.filter((o) => o.kind === "mark_mailbox").length, 0, "something was recorded");
    assert.equal(await serverUnseen(), COUNT);
    assert.equal((await localFolder()).unread, COUNT);
  });

  it("marks the folder read on the server from its right click, and Undo makes it unread again", async () => {
    account = await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    await browser.waitUntil(async () => (await testId(`mailbox-${String(folder.id)}`).getAttribute("aria-label")).includes("unread"), {
      timeoutMsg: "the folder shows no unread mail",
    });
    await rightClickMenu(`mailbox-${String(folder.id)}`);
    const first = await browser.execute(() => document.querySelector('[data-testid="overflow-menu"] [role="menuitem"]')?.getAttribute("aria-label"));
    assert.equal(first, "Mark folder as read", "not the first entry");
    const started = Date.now();
    await testId("mark-read-folder").click();
    await browser.waitUntil(async () => !(await testId(`mailbox-${String(folder.id)}`).getAttribute("aria-label")).includes("unread"), {
      timeout: 5000,
      timeoutMsg: "the folder's counter did not drop",
    });
    report.steps.counterDroppedAfterMs = Date.now() - started;
    await browser.waitUntil(async () => (await serverUnseen()) === 0, { timeout: 30000, interval: 500, timeoutMsg: "the server still has unread mail in the folder" });
    report.steps.serverSeenAfterMs = Date.now() - started;
    const operations = await coreCall("list_operations", { accountId: account.id, before: null, limit: 50 });
    const op = operations.find((o) => o.kind === "mark_mailbox");
    report.steps.operation = op && { action: op.action, state: op.state, messageCount: op.messageCount };
    assert.equal(op?.messageCount, COUNT);
    report.steps.notices = await notices();
    assert.ok(report.steps.notices.some((t) => t.includes(`${String(COUNT)} messages in ${FOLDER} marked as read`)), JSON.stringify(report.steps.notices));
    report.screenshots.push(await screenshot("mark-read-done"));

    await testId("notice-undo").click();
    await browser.waitUntil(async () => (await serverUnseen()) === COUNT, { timeout: 30000, interval: 500, timeoutMsg: "Undo did not reach the server" });
    await browser.waitUntil(async () => (await localFolder()).unread === COUNT, { timeout: 15000, timeoutMsg: "the counter did not come back" });

    // Shift+F10 on the row opens the same menu.
    report.steps.shiftF10 = await shiftF10(`mailbox-${String(folder.id)}`);
    assert.equal(await testId("mark-read-folder").isExisting(), true);
    await closeMenu();
  });

  it("at the narrowest column, in German and English, light and dark, the menu fits and the column never scrolls sideways", async () => {
    widthBefore = await columnWidth();
    await browser.execute(() => document.querySelector('[data-testid="sidebar-resize"]')?.focus());
    await browser.keys("Home");
    await browser.waitUntil(async () => (await columnWidth()) === 200, { timeoutMsg: "the column did not get 200 px wide" });

    const inbox = (await mailboxes()).find((m) => m.role === "inbox");
    const cases = [
      ["de", "light", folder.id],
      ["de", "dark", folder.id],
      ["de", "light", inbox.id],
      ["en", "light", folder.id],
      ["en", "light", inbox.id],
    ];
    report.steps.narrow = [];
    for (const [language, theme, id] of cases) {
      await setLanguage(language, theme);
      await clearOverlays();
      await rightClickMenu(`mailbox-${String(id)}`);
      const g = await geometry();
      report.steps.narrow.push({ language, theme, folder: id === folder.id ? FOLDER : "INBOX", ...g });
      assertInside(g, `${language} ${theme} ${id === folder.id ? FOLDER : "INBOX"}`);
      assert.ok(g.columnWidth <= 201, `the column is ${String(g.columnWidth)} px wide`);
      if (id === folder.id && language === "de") {
        assert.equal(g.items[0]?.label, "Ordner als gelesen markieren");
        // Every entry shows without rolling the menu where the window has room for it.
        assert.equal(g.menuScrolls, false, `the menu rolls: ${JSON.stringify(g.menu)}`);
        report.screenshots.push(await screenshot(`mark-read-menu-de-200-${theme}`));
        // Without the focus in it (the screenshot blurred it), the open menu stays visible.
        assert.equal(await menuVisible(), true, "the open menu turned invisible");
        await browser.execute(() => document.querySelector('[data-testid="overflow-menu"] [role="menuitem"]')?.focus({ preventScroll: true }));
      }
      // Moving through the menu does not scroll the column sideways either.
      await browser.keys(["ArrowDown", "ArrowDown", "ArrowDown", "End", "Home"]);
      const moved = await geometry();
      assertInside(moved, `${language} ${theme} after moving`);
      await closeMenu();
    }

    // Pushed sideways from outside, the column goes back at once.
    // (No animation frames here: a window started from a terminal session is never key.)
    report.steps.pushedAtOnce = await browser.execute(() => {
      const scroll = document.querySelector('[data-testid="sidebar-scroll"]');
      scroll.scrollLeft = 60;
      return scroll.scrollLeft;
    });
    await browser.pause(300);
    const pushed = await browser.execute(() => document.querySelector('[data-testid="sidebar-scroll"]').scrollLeft);
    assert.equal(pushed, 0, "the column stayed scrolled sideways");

    // A menu at the bottom of the column stays inside the window (above its button if need be).
    const lastRow = await browser.execute(() => {
      const scroll = document.querySelector('[data-testid="sidebar-scroll"]');
      scroll.scrollTop = scroll.scrollHeight;
      const rows = [...scroll.querySelectorAll('[role="treeitem"][data-testid^="mailbox-"]')];
      return rows.at(-1)?.getAttribute("data-testid") ?? null;
    });
    assert.ok(lastRow, "no folder row in the column");
    await browser.pause(200);
    await rightClickMenu(lastRow);
    const bottom = await geometry();
    report.steps.bottom = { row: lastRow, ...bottom };
    assertInside(bottom, "the menu of the last folder");
    assert.ok(bottom.menu.bottom <= bottom.visible.bottom && bottom.menu.top >= bottom.visible.top, `outside the column's visible area ${JSON.stringify(bottom)}`);
    await closeMenu();
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
