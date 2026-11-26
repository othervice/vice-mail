// Stage 21 (decisions 0069, 0070) in the real app on macOS: closing the main window keeps the app
// running; new mail still opens the popup; the window comes back through "Read", the tray menu
// and "New message"; the tray icon and its pause; "Start with the system" in the test's own
// launch-agent folder; the memory with and without the main window.
//
// 1. alice's account. The tray icon exists (menu bar, template image) with Open, New message,
//    Pause (1 h, until tomorrow, resume) and Quit.
// 2. The first close of the main window: the window stays and explains once; "Keep running"
//    closes it, the process goes on (the WebDriver still answers, no main window).
// 3. A new mail while no window is open: the popup appears. "Read" (a real click) builds the main
//    window again and shows the message there (the request waited for the new page).
// 4. The second close goes straight into the background (no note). From a message window, the
//    tray's "Open" and "New message" bring the main window back, the latter with a composer.
// 5. Pause for an hour from the tray: a new mail shows no popup; "Resume": the next one does.
// 6. Start with the system: on writes a launch agent with the executable and --background into
//    the test's folder (never ~/Library/LaunchAgents), off removes it.
// 7. Memory: footprint of the app and its WebKit processes 60 s after the window was open and
//    idle, and 60 s after it was closed into the background (support/perf.py).
//
// The facts go to tests/e2e/screenshots/background-report.json.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { browser } from "@wdio/globals";
import { attach, coreCall, invoke, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { DATA_DIR_ENV } from "./support/lifecycle.mjs";
import { ALICE, deliverMail, SERVER } from "./support/testmail.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const PERF_PY = path.join(here, "support", "perf.py");
const POPUP = "new-mail-popup";
const MAIN = "main";
const RUN = Date.now().toString(36);
const IDLE_MS = Number(process.env.BACKGROUND_IDLE_MS ?? 60000);
const report = { steps: {}, memory: {} };
let account;

const handles = () => browser.getWindowHandles();
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitHandles(predicate, message, timeout = 30000) {
  await browser.waitUntil(async () => predicate(await handles()), { timeout, interval: 250, timeoutMsg: message });
}

async function test(command, args = {}) {
  const result = await invoke(command, args);
  if ("failure" in result) throw new Error(`${command} failed: ${result.failure}`);
  return result.ok;
}

/** The pid of the test app: the process serving the WebDriver port. */
function appPid() {
  const pid = execFileSync("lsof", ["-ti", "tcp:4445", "-sTCP:LISTEN"], { encoding: "utf8" }).trim().split("\n")[0];
  return Number(pid);
}

function memory(pid) {
  const m = JSON.parse(execFileSync("python3", ["-I", PERF_PY, "memory", String(pid)], { encoding: "utf8" }));
  const mb = (bytes) => Math.round((bytes / 1048576) * 10) / 10;
  return {
    footprintMB: mb(m.footprintBytes),
    rssMB: mb(m.rssBytes),
    processes: m.processes.map((p) => ({ name: p.name, footprintMB: p.footprintBytes === null ? null : mb(p.footprintBytes), rssMB: mb(p.rssBytes) })),
  };
}

/** Cards shown in the popup (switch to it first). */
const cardSubjects = () =>
  browser.execute(() => [...document.querySelectorAll('[data-testid="popup-card"]')].map((c) => c.querySelector('[data-testid="popup-subject"]')?.textContent ?? null));

async function toPopupWith(subject) {
  await waitHandles((h) => h.includes(POPUP), `no popup for ${subject}`, 120000);
  await browser.switchToWindow(POPUP);
  await browser.waitUntil(async () => (await cardSubjects()).includes(subject), { timeout: 30000, timeoutMsg: `no card ${subject}` });
}

/** A real click into the popup (it never becomes key), as in new-mail-popup.e2e.mjs. */
async function nativeClick(subject, testid) {
  const rect = await browser.execute(
    (s, tid) => {
      const card = [...document.querySelectorAll('[data-testid="popup-card"]')].find((c) => c.querySelector('[data-testid="popup-subject"]')?.textContent === s);
      const element = card?.querySelector(`[data-testid="${tid}"]`);
      if (!element) return null;
      const r = element.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2, viewportHeight: innerHeight };
    },
    subject,
    testid,
  );
  assert.ok(rect, `no ${testid} for ${subject}`);
  try {
    await invoke("e2e_native_mouse", rect);
  } catch (e) {
    if (!/no such window|No window could be found|Channel closed/i.test(String(e.message ?? e))) throw e;
  }
}

async function readingSubject() {
  return browser.execute(() => document.querySelector('[data-testid="reading-subject"]')?.textContent ?? "");
}

describe("stage 21: running in the background", () => {
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
    await browser.waitUntil(
      async () => {
        const [a] = await coreCall("list_accounts");
        return a?.status.state === "idle" && a.status.lastSyncAt !== null;
      },
      { timeout: 240000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
  });

  after(async () => {
    // Leave the app with its main window and without the account, for the next spec.
    try {
      const open = await handles();
      if (!open.includes(MAIN) && open.length > 0) {
        await browser.switchToWindow(open[0]);
        await test("e2e_tray_action", { id: "open" });
        await waitHandles((h) => h.includes(MAIN), "main window not back");
      }
      await browser.switchToWindow(MAIN);
      await coreCall("pause_notifications", { choice: "resume" }).catch(() => undefined);
      await coreCall("set_autostart", { enabled: false }).catch(() => undefined);
      const left = await coreCall("list_accounts").catch(() => []);
      for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch(() => undefined);
    } catch (e) {
      console.log(`cleanup: ${e}`);
    }
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "background-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("has a tray icon in the menu bar with its menu", async () => {
    const tray = await test("e2e_tray");
    report.steps.tray = tray;
    assert.equal(tray.exists, true, "the menu-bar icon exists");
    const ids = tray.model.menu.filter((e) => e.kind !== "separator").map((e) => e.id);
    assert.deepEqual(ids, ["open", "compose", "pause", "quit"]);
    const pause = tray.model.menu.find((e) => e.id === "pause");
    assert.deepEqual(
      pause.items.filter((e) => e.kind !== "separator").map((e) => [e.id, e.enabled]),
      [
        ["pause-hour", true],
        ["pause-tomorrow", true],
        ["resume", false],
      ],
    );
    assert.equal(tray.model.menu[0].text, "Open Vice Mail");
  });

  it("measures the footprint with the main window open and idle", async () => {
    const pid = appPid();
    report.memory.pid = pid;
    await sleep(IDLE_MS);
    report.memory.windowOpen = memory(pid);
  });

  it("the first close explains once; Keep running closes the window and the app goes on", async () => {
    await browser.switchToWindow(MAIN);
    await test("e2e_close_window", { label: MAIN });
    const dialog = await testId("close-hint");
    await dialog.waitForExist({ timeout: 10000 });
    report.steps.note = await dialog.getText();
    assert.match(report.steps.note, /Vice Mail keeps running/);
    assert.ok((await handles()).includes(MAIN), "the window stays while the note shows");
    await (await testId("confirm-dialog-confirm")).click();
    await waitHandles((h) => !h.includes(MAIN), "the main window did not close", 15000);
    // The process is alive: the WebDriver answers and the core runs.
    const status = await fetch("http://127.0.0.1:4445/status").then((r) => r.ok);
    assert.equal(status, true, "the app still runs");
    report.steps.afterClose = { handles: await handles() };
  });

  it("measures the footprint with the window closed into the background", async () => {
    await sleep(IDLE_MS);
    report.memory.background = memory(report.memory.pid);
    report.memory.saved = Math.round((report.memory.windowOpen.footprintMB - report.memory.background.footprintMB) * 10) / 10;
  });

  it("a new mail without any window opens the popup; Read brings the main window back with it", async () => {
    const subject = `Background one ${RUN}`;
    await deliverMail(subject);
    await toPopupWith(subject);
    assert.ok(!(await handles()).includes(MAIN), "no main window yet");
    const tray = await test("e2e_tray");
    assert.ok(tray.model.count >= 1, `the tray counts unread mail: ${JSON.stringify(tray.model)}`);
    report.steps.trayWithMail = { title: tray.model.title, tooltip: tray.model.tooltip };
    await nativeClick(subject, "popup-read");
    await waitHandles((h) => h.includes(MAIN), "Read did not open the main window", 20000);
    await browser.switchToWindow(MAIN);
    await browser.waitUntil(async () => (await readingSubject()) === subject, {
      timeout: 30000,
      timeoutMsg: "the new main window does not show the message",
    });
    report.steps.readAfterBackground = subject;
  });

  it("the second close goes straight into the background; the tray opens the window and a composer", async () => {
    await browser.switchToWindow(MAIN);
    // A message window keeps a page to run the test's commands in.
    const inbox = (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "inbox");
    const page = await coreCall("list_messages", { mailboxId: inbox.id, cursor: null, limit: 1 });
    assert.ok(page.items[0], "a message to open");
    await coreCall("open_message_window", { accountId: account.id, messageId: page.items[0].id });
    const messageWindow = `message-${account.id.replaceAll("-", "")}-${page.items[0].id}`;
    await waitHandles((h) => h.includes(messageWindow), "no message window");

    // Closed from the message window's page (a page that closes itself loses the answer), once
    // that page has loaded.
    await browser.switchToWindow(messageWindow);
    await browser.waitUntil(
      () => browser.execute(() => document.readyState === "complete" && typeof window.__TAURI_INTERNALS__?.invoke === "function"),
      { timeout: 20000, timeoutMsg: "the message window did not load" },
    );
    await browser.waitUntil(async () => (await readingSubject()) !== "", { timeout: 20000, timeoutMsg: "the message window shows nothing" });
    await test("e2e_close_window", { label: MAIN });
    await waitHandles((h) => !h.includes(MAIN), "the second close did not go straight into the background", 15000);
    await test("e2e_tray_action", { id: "open" });
    await waitHandles((h) => h.includes(MAIN), "the tray's Open did not bring the window back", 20000);

    await test("e2e_close_window", { label: MAIN });
    await waitHandles((h) => !h.includes(MAIN), "closed again", 15000);
    await test("e2e_tray_action", { id: "compose" });
    await waitHandles((h) => h.includes(MAIN), "New message did not open the window", 20000);
    await browser.switchToWindow(MAIN);
    await testId("composer").waitForExist({ timeout: 30000 });
    report.steps.composeFromTray = true;
    await browser.switchToWindow(messageWindow);
    await browser.closeWindow();
    await browser.switchToWindow(MAIN);
    // Discard the empty draft.
    await browser.keys(["Escape"]);
  });

  it("pausing from the tray keeps new mail quiet; resuming lets the next one through", async () => {
    await browser.switchToWindow(MAIN);
    await test("e2e_tray_action", { id: "pause-hour" });
    await browser.waitUntil(async () => (await coreCall("get_notification_pause")).until !== null, { timeout: 10000, timeoutMsg: "not paused" });
    const tray = await test("e2e_tray");
    const pause = tray.model.menu.find((e) => e.id === "pause");
    const status = pause.items.find((e) => e.id === "paused");
    // The UI's language decides the clock: English here, so "Paused until 1:39 PM" (after 11 PM
    // the hour ends tomorrow: "Paused until tomorrow, 12:06 AM").
    assert.ok(status && /^Paused until (tomorrow, )?\d{1,2}:\d\d( [AP]M)?$/.test(status.text), JSON.stringify(pause));
    assert.equal(pause.items.find((e) => e.id === "resume").enabled, true);
    report.steps.pausedMenu = status.text;
    const quiet = `Background quiet ${RUN}`;
    await deliverMail(quiet);
    await browser.waitUntil(
      async () => (await coreCall("list_messages", { mailboxId: (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "inbox").id, cursor: null, limit: 20 })).items.some((i) => i.subject === quiet),
      { timeout: 90000, interval: 500, timeoutMsg: "the quiet mail did not arrive" },
    );
    await sleep(3000);
    assert.ok(!(await handles()).includes(POPUP), "no popup while paused");
    await test("e2e_tray_action", { id: "resume" });
    await browser.waitUntil(async () => (await coreCall("get_notification_pause")).until === null, { timeout: 10000, timeoutMsg: "not resumed" });
    const loud = `Background loud ${RUN}`;
    await deliverMail(loud);
    await toPopupWith(loud);
    assert.ok(!(await cardSubjects()).includes(quiet), "the mail of the pause is not announced later");
    // The popup closes with its last card; the answer to this script may not come back.
    await browser.execute(() => document.querySelector('[data-testid="popup-close"]')?.click()).catch(() => undefined);
    await waitHandles((h) => !h.includes(POPUP), "the popup did not close", 10000);
    report.steps.pause = { quiet, loud };
  });

  it("start with the system writes the launch agent into the test's folder and removes it", async () => {
    await browser.switchToWindow(MAIN);
    const plist = path.join(process.env[DATA_DIR_ENV], "launch-agents", "org.vicemail.desktop.e2e.plist");
    const before = await coreCall("get_autostart");
    assert.deepEqual(before, { enabled: false, available: true });
    const on = await coreCall("set_autostart", { enabled: true });
    assert.equal(on.enabled, true);
    const text = fs.readFileSync(plist, "utf8");
    assert.match(text, /<string>--background<\/string>/);
    assert.match(text, /<key>RunAtLoad<\/key>\s*<true\/>/);
    const lint = execFileSync("/usr/bin/plutil", ["-lint", plist], { encoding: "utf8" });
    report.steps.launchAgent = { path: plist, lint: lint.trim(), program: /<array>\s*<string>([^<]+)<\/string>/.exec(text.split("ProgramArguments")[1])?.[1] };
    assert.ok(!fs.existsSync(path.join(process.env.HOME, "Library", "LaunchAgents", "org.vicemail.desktop.e2e.plist")), "nothing in the user's LaunchAgents");
    const off = await coreCall("set_autostart", { enabled: false });
    assert.equal(off.enabled, false);
    assert.ok(!fs.existsSync(plist), "removed");
  });
});
