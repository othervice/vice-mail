// Stage 2 acceptance in the real app, against the local test server. wdio.conf.mjs resets the
// server and seeds 10,000 messages for alice (9,959 of them in INBOX) before the first app starts,
// and stops it when the run ends. Runs after frame.e2e.mjs and restart.e2e.mjs, on their data
// directory, which has no account yet.
//
// 1. Setup through Settings → Accounts with the form by hand (the test domain is unknown): a
//    wrong password first, then the right one. Folders with roles and counters and all messages
//    appear as on the server; times from the click on "Add account" are measured in the page.
// 2. The account is removed through the UI (no keychain entry left) and added again while the
//    server is slowed down, so the first sync takes long enough to scroll the list during it.
// 3. A message delivered over SMTP while the app runs appears by itself.
// 4. The server is stopped: the app shows "offline", the list stays usable. The server is
//    started: the sync recovers by itself, new mail arrives again.
// 5. The account is removed through the UI; the keychain keeps nothing.
//
// Screenshots of the main states in light and dark go to tests/e2e/screenshots/account-*.png.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, screenshot, SCREENSHOT_DIR, setPreference, testId } from "./support/app.mjs";
import { measureScroll } from "./support/scroll.mjs";
import {
  ALICE,
  KEYCHAIN_SERVICE,
  SERVER,
  deliverMail,
  keychainHasEntry,
  serverFolders,
  testmail,
  throttleServer,
} from "./support/testmail.mjs";

/** Facts collected along the way, printed at the end and written next to the screenshots. */
const report = {};

const ROLE_BY_FLAG = { "\\sent": "sent", "\\drafts": "drafts", "\\archive": "archive", "\\junk": "junk", "\\trash": "trash" };
const ROLE_NAMES = { inbox: "Inbox", sent: "Sent", drafts: "Drafts", archive: "Archive", trash: "Trash", junk: "Junk" };
const TOP_LEVEL_ORDER = ["Inbox", "Sent", "Drafts", "Archive", "Trash", "Junk", "Projects"];
const PROGRESS_TEXT = /^Syncing…[\d,]+ of [\d,]+$/;
/**
 * The slowed-down server: it runs 5 ms, then pauses 745 ms. The app, not the server, does most of
 * the work of a sync, so the server has to be slowed down this much for the first sync of 10,000
 * messages to take some 20 s instead of one second.
 */
const THROTTLE = { runMs: 5, pauseMs: 745 };

const numbers = new Intl.NumberFormat("en");
const roleOf = (path, info) => (path === "INBOX" ? "inbox" : (info.flags.map((f) => ROLE_BY_FLAG[f]).find(Boolean) ?? "custom"));

// --- page helpers ------------------------------------------------------------------------------

/**
 * Types into a field of the setup form through WebDriver and checks that the form (React state)
 * took the value, not only the DOM.
 */
async function fill(id, value) {
  const field = testId(id);
  await field.clearValue();
  await field.setValue(value);
  await browser.waitUntil(async () => (await field.getValue()) === value, {
    timeout: 2000,
    timeoutMsg: `${id} does not hold "${value}"`,
  });
}

/** Clicks the button with exactly this text, inside `scope` (a CSS selector) if given. */
async function clickButton(text, scope = "main") {
  const found = await browser.execute(
    (label, within) => {
      const root = document.querySelector(within);
      const button = [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
      if (!button || button.disabled) return false;
      button.click();
      return true;
    },
    text,
    scope,
  );
  assert.ok(found, `no enabled button "${text}" in ${scope}`);
}

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `theme ${theme} not applied`,
  });
}

/** Screenshots of the current state in light and dark; ends in dark. */
async function shootBothThemes(name) {
  await setTheme("light");
  await screenshot(`account-${name}-light`);
  await setTheme("dark");
  await screenshot(`account-${name}-dark`);
}

/**
 * Until stage 3 WebKit (macOS 27.2) reported one `style-src-attr` violation for every native
 * `<select>` (its own rendering of the control) under the app's `style-src 'self'`. The app's
 * CSP now allows inline styles, because the mail frame inherits it (decision 0005), so there is
 * no exception any more: every violation fails the run. The control test below shows that a
 * select causes none and that a violation the CSP still forbids is recorded on that page.
 */
let cspSeen = 0;
/** CSP violations since the last call. */
async function newCspViolations() {
  const all = await cspViolations();
  const fresh = all.slice(cspSeen);
  cspSeen = all.length;
  return fresh;
}

/**
 * The window shows the frame in place: the document itself never scrolls, only its panes do
 * (app.css, #root). Before that fix, scrolling the setup form to its error moved the whole frame
 * up by 360 px.
 */
async function assertFrameInPlace(where) {
  const doc = await browser.execute(() => ({
    scrollTop: document.scrollingElement.scrollTop,
    scrollHeight: document.scrollingElement.scrollHeight,
    height: innerHeight,
  }));
  assert.deepEqual(doc, { scrollTop: 0, scrollHeight: doc.height, height: doc.height }, `the document scrolled (${where})`);
}

const mainText = () => browser.execute(() => document.querySelector("main")?.textContent ?? "");

/** Waits until the view shows `text`; on timeout the error says what it shows instead. */
async function waitForMainText(text, timeout = 10000) {
  try {
    await browser.waitUntil(async () => (await mainText()).includes(text), { timeout, interval: 50 });
  } catch {
    throw new Error(`the view does not show "${text}" but: ${(await mainText()).slice(0, 400)}`);
  }
}

const syncText = () => browser.execute(() => document.querySelector('[data-testid="sync-status"]')?.textContent.trim() ?? null);

/** What the reading pane shows: subject, a rendered body, the "not on this computer" notice, an error. */
const paneState = () =>
  browser.execute(() => {
    const pane = document.querySelector('[data-testid="reading-pane"]');
    if (!pane) return null;
    return {
      subject: pane.querySelector("article h2")?.textContent ?? null,
      body: pane.querySelector('[data-testid="text-body"], [data-testid="mail-frame"]') !== null,
      unavailable: pane.querySelector('[data-testid="reading-unavailable"]')?.textContent ?? null,
      // Not `error`: webdriverio would take the returned object for a WebDriver error.
      failed: pane.querySelector('[data-testid="reading-error"], [data-testid="reading-fetch-failed"]') !== null,
    };
  });

/** Waits until the pane shows the message of the row labelled `rowLabel` with a body or a notice. */
async function waitForPane(rowLabel) {
  let last = null;
  try {
    await browser.waitUntil(
      async () => {
        last = await paneState();
        return last !== null && last.subject !== null && rowLabel.includes(last.subject) && (last.body || last.unavailable || last.failed);
      },
      { timeout: 10000, interval: 50 },
    );
  } catch {
    throw new Error(`the reading pane does not show the message "${rowLabel}": ${JSON.stringify(last)}`);
  }
  return last;
}

/**
 * Records, from now on, every change of what the frame shows (folder buttons, list rows, sync
 * status, view title) with page timestamps; `timeline()` reads it. A MutationObserver runs as a
 * microtask after each DOM change, so the times are exact to the change, not to a poll.
 */
function startTimeline() {
  return browser.execute(() => {
    window.__e2eTimeline?.observer.disconnect();
    const samples = [];
    const snapshot = () => {
      const list = document.querySelector('[data-testid="message-list"]');
      const first = list?.querySelector('[role="option"]');
      return {
        view: document.querySelector("main header h1")?.textContent ?? null,
        folders: document.querySelectorAll('[data-testid^="mailbox-"]').length,
        rows: first ? 1 : 0,
        setSize: Number(first?.getAttribute("aria-setsize") ?? 0),
        sync: document.querySelector('[data-testid="sync-status"]')?.textContent.trim() ?? null,
      };
    };
    let last = "";
    const record = () => {
      const s = snapshot();
      const key = JSON.stringify(s);
      if (key === last) return;
      last = key;
      samples.push({ t: performance.now(), wall: Date.now(), ...s });
    };
    const observer = new MutationObserver(record);
    observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["aria-setsize"] });
    window.__e2eTimeline = { samples, observer, marks: {} };
    record();
  });
}

/** Marks a point in the timeline (page time) and runs an optional click there. */
function mark(name, clickSelector = null) {
  return browser.execute(
    (label, selector) => {
      window.__e2eTimeline.marks[label] = { t: performance.now(), wall: Date.now() };
      if (selector) document.querySelector(selector).click();
    },
    name,
    clickSelector,
  );
}

async function timeline() {
  return browser.execute(() => ({ samples: window.__e2eTimeline.samples, marks: window.__e2eTimeline.marks }));
}

function stopTimeline() {
  return browser.execute(() => window.__e2eTimeline?.observer.disconnect());
}

/** Milliseconds from `from` (a mark) to the first sample matching `predicate`, or null. */
function firstAfter({ samples, marks }, from, predicate) {
  const start = marks[from];
  const hit = samples.find((s) => s.t >= start.t && predicate(s));
  return hit ? Math.round(hit.t - start.t) : null;
}

/** Scrolls the list to `top` and sends the scroll event a hidden window would not send. */
function scrollListTo(top) {
  return browser.execute((y) => {
    const list = document.querySelector('[data-testid="message-list"]');
    list.scrollTop = y;
    list.dispatchEvent(new Event("scroll"));
    return list.scrollTop;
  }, top);
}

/** The rows the list has in the DOM: position, label, selection. */
function renderedRows() {
  return browser.execute(() =>
    [...document.querySelectorAll('[data-testid="message-list"] [role="option"]')].map((el) => ({
      id: el.id,
      posinset: Number(el.getAttribute("aria-posinset")),
      setsize: Number(el.getAttribute("aria-setsize")),
      selected: el.getAttribute("aria-selected") === "true",
      label: el.getAttribute("aria-label"),
      top: el.getBoundingClientRect().top,
    })),
  );
}

/**
 * Scrolls to the end of the list again and again until every page is loaded. Returns the last
 * row's position and the list size; proves that every message of the folder can be reached.
 */
async function scrollToEnd() {
  const started = Date.now();
  for (let i = 0; i < 400; i += 1) {
    const state = await browser.execute(() => {
      const list = document.querySelector('[data-testid="message-list"]');
      list.scrollTop = list.scrollHeight;
      list.dispatchEvent(new Event("scroll"));
      const options = list.querySelectorAll('[role="option"]');
      const last = options[options.length - 1];
      return {
        lastPosinset: Number(last?.getAttribute("aria-posinset") ?? 0),
        setSize: Number(last?.getAttribute("aria-setsize") ?? 0),
        loadingMore: list.textContent.includes("Loading more messages"),
      };
    });
    if (!state.loadingMore && state.setSize > 0 && state.lastPosinset === state.setSize) {
      return { ...state, ms: Date.now() - started };
    }
    await browser.pause(30);
  }
  throw new Error("the list did not reach its end");
}

// --- core and server helpers -------------------------------------------------------------------

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

/** Waits until the account is idle after a complete pass and every folder matches the server. */
async function waitForSynced(expected, timeout = 120000) {
  let last;
  await browser.waitUntil(
    async () => {
      const account = await onlyAccount();
      if (account.status.state !== "idle" || account.status.lastSyncAt === null) return false;
      const mailboxes = await coreCall("list_mailboxes", { accountId: account.id });
      last = mailboxes.map((m) => `${m.path}:${m.total}/${m.unread}`);
      return Object.entries(expected).every(([p, info]) => {
        const local = mailboxes.find((m) => m.path === p);
        return local && local.total === info.messages && local.unread === info.unseen;
      });
    },
    { timeout, interval: 100, timeoutMsg: `not synced like the server: ${JSON.stringify(last)}` },
  );
}

// --- the setup form ----------------------------------------------------------------------------

async function openSetupForm() {
  await openSettings("accounts");
  const accounts = await coreCall("list_accounts");
  // Without accounts the list is an empty state whose action opens the form; otherwise a button.
  await clickButton("Add account");
  await testId("add-account").waitForDisplayed();
  assert.equal(accounts.length, 0, "the test starts without accounts");
}

/** Address, password and the servers by hand: the provider table does not know the test domain. */
async function fillSetupForm(password) {
  await fill("setup-email", ALICE.email);
  // The lookup in the provider table answers "unknown" and the form for the servers appears,
  // with the online search offered but not run (no network access without a click).
  await testId("online-search").waitForDisplayed({ timeoutMsg: "no hint that the domain is unknown" });
  await testId("server-imap").waitForDisplayed({ timeoutMsg: "no server form" });
  await fill("setup-password", password);
  await fill("imap-host", SERVER.host);
  await fill("imap-port", String(SERVER.imapsPort));
  assert.equal(await testId("imap-username").getValue(), ALICE.email, "user name prefilled with the address");
  await fill("smtp-host", SERVER.host);
  await fill("smtp-port", String(SERVER.smtpsPort));
}

/**
 * Settings → Accounts → the account → "Remove account…" → "Remove". Checks the CSP violations
 * of the way there: none, also on the account page with its native selects.
 */
async function removeAccountThroughUi(account, { shoot = null } = {}) {
  assert.deepEqual(await newCspViolations(), [], "CSP violations before the account page");
  await openSettings("accounts");
  await testId(`account-row-${account.id}`).click();
  await testId("account-detail").waitForDisplayed();
  await testId("special-folders").waitForDisplayed();
  const selects = await browser.execute(() => document.querySelectorAll("main select").length);
  if (shoot) {
    await shootBothThemes(shoot);
    // The special folders with their native selects further down the page.
    await browser.execute(() => document.querySelector('[data-testid="special-folders"]').scrollIntoView());
    await assertFrameInPlace("account page");
    await shootBothThemes(`${shoot}-folders`);
  }
  await clickButton("Remove account…");
  await testId("remove-confirm").waitForDisplayed();
  const started = Date.now();
  await clickButton("Remove", '[data-testid="remove-confirm"]');
  await browser.waitUntil(async () => (await coreCall("list_accounts")).length === 0, {
    timeoutMsg: "the account is still there",
  });
  const ms = Date.now() - started;
  // Back on the list, which is empty again.
  await waitForMainText("No accounts yet");
  const fresh = await newCspViolations();
  assert.equal(selects, 7, "one select per assignable role");
  assert.deepEqual(fresh, [], "CSP violations on the account page");
  return { ms, selects };
}

// --- the spec ----------------------------------------------------------------------------------

describe("account and read-only sync (stage 2)", function () {
  let expected;
  let stopThrottle = null;

  before(async () => {
    await attach();
    expected = await serverFolders();
    report.server = Object.fromEntries(Object.entries(expected).map(([p, i]) => [p, `${i.messages} (${i.unseen} unread)`]));
    report.serverTotal = Object.values(expected).reduce((sum, i) => sum + i.messages, 0);
    assert.equal(report.serverTotal, 10000, "wdio.conf.mjs seeds 10,000 messages");
    assert.equal(keychainHasEntry(), false, `a keychain entry of ${KEYCHAIN_SERVICE} is left from an earlier run; remove it with \`security delete-generic-password -s ${KEYCHAIN_SERVICE}\``);
    report.visibilityState = await browser.execute(() => document.visibilityState);
  });

  afterEach(async () => {
    // A failed measurement must not leave the server slowed down for the tests after it.
    if (stopThrottle) await stopThrottle();
    stopThrottle = null;
  });

  after(async () => {
    // Safety net when a test failed half-way: never leave an account (and its keychain entry).
    const left = await coreCall("list_accounts").catch(() => []);
    for (const account of left) {
      console.log(`cleanup: removing ${account.email} through the core`);
      await coreCall("remove_account", { accountId: account.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    }
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "account-sync-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("starts in English with no account", async () => {
    await setPreference("language", "en");
    assert.deepEqual(await coreCall("list_accounts"), []);
  });

  it("explains a wrong password in the setup form, without CSP violations", async () => {
    await openSetupForm();
    await fillSetupForm("wrong-password");
    const started = Date.now();
    await testId("setup-test").click();
    const error = testId("setup-error");
    await error.waitForDisplayed({ timeout: 30000, timeoutMsg: "no error for the wrong password" });
    report.wrongPasswordMs = Date.now() - started;
    assert.equal(await error.getAttribute("data-error-code"), "auth_failed");
    const text = await error.getText();
    assert.match(text, /The connection test failed\./);
    assert.match(text, /The server did not accept the user name or password\./);
    assert.deepEqual(await coreCall("list_accounts"), [], "a test stores nothing");
    assert.equal(keychainHasEntry(), false, "a test stores no password");
    await browser.execute(() => document.querySelector('[data-testid="setup-error"]').scrollIntoView({ block: "center" }));
    await shootBothThemes("setup-wrong-password");
    await assertFrameInPlace("setup form with its error in view");
    assert.deepEqual(await newCspViolations(), [], "CSP violations on the setup page");
  });

  it("tests the connection with the right password", async () => {
    await fill("setup-password", ALICE.password);
    // Any change clears the earlier result.
    assert.equal(await testId("setup-error").isExisting(), false);
    await testId("setup-test").click();
    const result = testId("test-result");
    await result.waitForDisplayed({ timeout: 30000, timeoutMsg: "the connection test did not succeed" });
    assert.match(await result.getText(), /the server supports IDLE/);
  });

  it("adds the account: folders and all 10,000 messages appear as on the server", async () => {
    // The unfolded sidebar shows the folder tree as soon as there is one.
    await testId("rail-folders").click();
    await startTimeline();
    await mark("add", '[data-testid="setup-submit"]');
    await waitForSynced(expected);
    await mark("synced");
    const account = await onlyAccount();
    assert.equal(account.writesEnabled, false, "new accounts are read-only");
    assert.equal(keychainHasEntry(), true, "the password is in the keychain");
    const tl = await timeline();
    await stopTimeline();
    const selectable = Object.keys(expected).length;
    report.firstAdd = {
      inboxViewMs: firstAfter(tl, "add", (s) => s.view === "Inbox"),
      allFoldersMs: firstAfter(tl, "add", (s) => s.folders >= selectable),
      firstRowsMs: firstAfter(tl, "add", (s) => s.rows > 0),
      inboxCompleteInListMs: firstAfter(tl, "add", (s) => s.setSize === expected.INBOX.messages),
      idleInUiMs: firstAfter(tl, "add", (s) => s.rows > 0 && s.sync === ""),
      coreSyncedMs: Math.round(tl.marks.synced.t - tl.marks.add.t),
      syncTexts: [...new Set(tl.samples.map((s) => s.sync).filter(Boolean))].slice(0, 8),
      samples: tl.samples.length,
    };
    for (const key of ["inboxViewMs", "allFoldersMs", "firstRowsMs", "idleInUiMs"]) {
      assert.notEqual(report.firstAdd[key], null, `${key}: never reached`);
    }
  });

  it("shows the folder tree with roles and unread counters from the server", async () => {
    const account = await onlyAccount();
    const mailboxes = await coreCall("list_mailboxes", { accountId: account.id });
    const tree = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid^="mailbox-"]')].map((b) => ({
        id: Number(b.dataset.testid.slice("mailbox-".length)),
        text: b.textContent,
        label: b.getAttribute("aria-label"),
        depth: (() => {
          let d = 0;
          for (let li = b.closest("li")?.parentElement?.closest("li"); li; li = li.parentElement?.closest("li")) d += 1;
          return d;
        })(),
      })),
    );
    const rows = [];
    let total = 0;
    for (const [p, info] of Object.entries(expected)) {
      const local = mailboxes.find((m) => m.path === p);
      assert.ok(local, `folder ${p} missing in the app`);
      const role = roleOf(p, info);
      assert.equal(local.role, role, `role of ${p}`);
      assert.equal(local.total, info.messages, `messages in ${p}`);
      assert.equal(local.unread, info.unseen, `unread in ${p}`);
      total += local.total;
      const name = role === "custom" ? p.split("/").at(-1) : ROLE_NAMES[role];
      const item = tree.find((t) => t.id === local.id);
      assert.ok(item, `${p} not in the folder tree`);
      const count = info.unseen > 0 ? numbers.format(info.unseen) : "";
      assert.equal(item.text, `${name}${count}`, `tree entry of ${p}`);
      assert.equal(item.label, info.unseen > 0 ? `${name}, ${info.unseen} unread` : name, `accessible name of ${p}`);
      assert.equal(item.depth, p.split("/").length - 1, `depth of ${p}`);
      rows.push(`${item.text.padEnd(18)} ${role} (${local.roleSource}) ${local.total}`);
    }
    assert.equal(total, 10000);
    assert.deepEqual(tree.filter((t) => t.depth === 0).map((t) => t.label.split(",")[0]), TOP_LEVEL_ORDER);
    report.tree = rows;
    await assertFrameInPlace("mail view");
    await scrollListTo(0);
    await shootBothThemes("folder-tree");
  });

  it("lists every message of the inbox, in the same number as the server", async () => {
    const rows = await renderedRows();
    assert.ok(rows.length > 0);
    assert.equal(rows[0].setsize, expected.INBOX.messages, "list size while pages are still unloaded");
    assert.equal(rows[0].posinset, 1);
    assert.ok(rows.every((r) => r.label && !r.label.startsWith("(Unknown sender)")), "rows show real senders");
    const end = await scrollToEnd();
    report.inboxListEnd = end;
    assert.equal(end.setSize, expected.INBOX.messages);
    assert.equal(end.lastPosinset, expected.INBOX.messages);
    await scrollListTo(0);
  });

  it("opens another folder from the tree", async () => {
    const account = await onlyAccount();
    const archive = (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "archive");
    await testId(`mailbox-${archive.id}`).click();
    await browser.waitUntil(async () => (await $("main header h1").getText()) === "Archive");
    await browser.waitUntil(async () => (await renderedRows()).length === expected.Archive.messages, {
      timeoutMsg: "the archive does not show its messages",
    });
    await testId("rail-inbox").click();
    await browser.waitUntil(async () => (await renderedRows()).length > 0);
  });

  it("removes the account through the UI, keychain entry included", async () => {
    const account = await onlyAccount();
    report.firstRemove = await removeAccountThroughUi(account, { shoot: "detail" });
    assert.equal(keychainHasEntry(), false, "the keychain entry is gone");
  });

  it("records CSP violations on the account page, and none for a native select (control)", async () => {
    await newCspViolations();
    // A bare select, added and removed again by the test: no violation since the app's CSP
    // allows inline styles (WebKit reported one per select before).
    await browser.execute(() => {
      const select = document.createElement("select");
      document.body.append(select);
      select.getBoundingClientRect();
      select.remove();
    });
    await browser.pause(300);
    assert.deepEqual(await newCspViolations(), [], "a native select causes a violation again");
    // What the CSP still forbids is recorded here as well: an image from outside the app
    // (`img-src 'self' data:`; a closed local port, and the CSP stops it before any request).
    await browser.execute(() => {
      const image = new Image();
      image.src = "http://127.0.0.1:9/e2e-csp-control.png";
    });
    await browser.waitUntil(async () => (await cspViolations()).length > cspSeen, {
      timeout: 2000,
      timeoutMsg: "the recorder saw no violation for the blocked image",
    });
    const fresh = await newCspViolations();
    report.cspControl = fresh;
    assert.equal(fresh.length, 1, JSON.stringify(fresh));
    assert.match(fresh[0].directive, /^img-src/);
    assert.equal(fresh[0].blocked, "http://127.0.0.1:9/e2e-csp-control.png");
  });

  it("scrolls the list smoothly while the first sync is running", async () => {
    await openSetupForm();
    await fillSetupForm(ALICE.password);
    stopThrottle = throttleServer(THROTTLE);
    await startTimeline();
    await mark("add", '[data-testid="setup-submit"]');
    // Wait until the list has a few pages to scroll through and the sync is still busy.
    await browser.waitUntil(
      async () => {
        const rows = await renderedRows();
        return rows.length > 0 && rows[0].setsize >= 600 && PROGRESS_TEXT.test((await syncText()) ?? "");
      },
      { timeout: 60000, interval: 50, timeoutMsg: "the slowed-down sync did not get going" },
    );
    const ready = await timeline();
    await stopTimeline();
    const before = await onlyAccount();
    const measured = await measureScroll({ durationMs: 8000, pxPerFrame: 80 });
    const after = await onlyAccount();
    assert.ok(!measured.error, measured.error);
    report.scroll = {
      throttle: THROTTLE,
      firstRowsAfterAddMs: firstAfter(ready, "add", (x) => x.rows > 0),
      measurementStartAfterAddMs: Math.round(ready.samples.at(-1).t - ready.marks.add.t),
      ...measured.summary,
      progressBefore: before.status.progress,
      progressAfter: after.status.progress,
      stateAfter: after.status.state,
      largestGaps: [...measured.gaps].sort((a, b) => b - a).slice(0, 8),
      blankFrameSamples: measured.blankFrames.slice(0, 5),
    };
    // The measurement counts only if the sync really ran all along and stored messages meanwhile.
    assert.equal(before.status.state, "syncing");
    assert.equal(after.status.state, "syncing", "the sync ended during the measurement; slow the server down more");
    assert.match(measured.summary.before.sync, PROGRESS_TEXT);
    assert.match(measured.summary.after.sync, PROGRESS_TEXT);
    assert.ok(measured.summary.after.setSize > measured.summary.before.setSize, "no messages arrived during the measurement");
    // Smoothness (docs/SPEC.md: 60 frames per second).
    const s = measured.summary;
    assert.ok(s.frames > 200, `only ${s.frames} frames`);
    assert.ok(s.frameMedianMs <= 17, `median frame ${s.frameMedianMs} ms`);
    assert.ok(s.frameMaxMs <= 50, `longest frame ${s.frameMaxMs} ms`);
    assert.ok(s.longestBlockingMs <= 50, `main thread blocked for ${s.longestBlockingMs} ms`);
    assert.equal(s.blankFrames, 0, `blank frames: ${JSON.stringify(measured.blankFrames.slice(0, 5))}`);
    // Still syncing: the list as it fills up, in both themes.
    await scrollListTo(0);
    await shootBothThemes("list-during-sync");
    report.syncTextAtScreenshot = await syncText();
    const syncStarted = ready.marks.add.wall;
    await waitForSynced(expected);
    report.scroll.throttledSyncTotalMs = Date.now() - syncStarted;
    await stopThrottle();
    stopThrottle = null;
  });

  it("shows a newly delivered message by itself within seconds", async () => {
    await scrollListTo(0);
    const subject = `Desktop test ${Date.now()}`;
    await browser.execute((wanted) => {
      window.__e2eArrival = null;
      const observer = new MutationObserver(() => {
        const row = [...document.querySelectorAll('[data-testid="message-list"] [role="option"]')].find((el) =>
          el.getAttribute("aria-label")?.includes(wanted),
        );
        if (row) {
          window.__e2eArrival = { wall: Date.now(), posinset: Number(row.getAttribute("aria-posinset")), label: row.getAttribute("aria-label") };
          observer.disconnect();
        }
      });
      observer.observe(document.body, { subtree: true, childList: true, attributes: true });
    }, subject);
    const sent = await deliverMail(subject);
    await browser.waitUntil(async () => (await browser.execute(() => window.__e2eArrival)) !== null, {
      timeout: 10000,
      timeoutMsg: "the new message did not appear within 10 s",
    });
    const arrival = await browser.execute(() => window.__e2eArrival);
    report.newMail = {
      smtpMs: sent.acceptedAt - sent.startedAt,
      visibleAfterAcceptedMs: arrival.wall - sent.acceptedAt,
      visibleAfterSendStartMs: arrival.wall - sent.startedAt,
      row: arrival.label,
    };
    assert.equal(arrival.posinset, 1, "the new message is the first row");
    assert.match(arrival.label, /^Unread, Bob Example, Desktop test \d+, /);
    assert.ok(report.newMail.visibleAfterSendStartMs < 5000, `${report.newMail.visibleAfterSendStartMs} ms`);
    expected = await serverFolders();
    await waitForSynced(expected, 10000);
  });

  it("shows offline when the server stops and keeps the list usable", async () => {
    await scrollListTo(0);
    await startTimeline();
    await mark("stop");
    await testmail("stop");
    await mark("stopped");
    await browser.waitUntil(async () => /^Offline/.test((await syncText()) ?? ""), {
      timeout: 30000,
      interval: 50,
      timeoutMsg: "no offline state after the server stopped",
    });
    const tl = await timeline();
    report.offline = {
      // testmail.sh stop sends SIGTERM at once and returns when the process has exited.
      stopCommandMs: Math.round(tl.marks.stopped.t - tl.marks.stop.t),
      offlineAfterStopCommandMs: firstAfter(tl, "stop", (s) => /^Offline/.test(s.sync ?? "")),
      text: await syncText(),
    };
    assert.ok(report.offline.offlineAfterStopCommandMs !== null);
    assert.equal(report.offline.text, "Offline. Your mail on this computer stays available.");
    const account = await onlyAccount();
    assert.equal(account.status.state, "offline");
    assert.ok(account.status.error, "offline carries the reason");
    report.offline.reason = account.status.error.code;
    // The unfolded sidebar marks the account too (its badge has no text; the account's
    // button in the folded rail names the state).

    // Scroll, click and use the keyboard in the list while offline.
    await scrollListTo(300 * 44);
    await browser.pause(100);
    const rows = await renderedRows();
    const target = rows.find((r) => r.posinset >= 300 && r.top > 150);
    assert.ok(target, `no row near 300 after scrolling: ${rows.map((r) => r.posinset).join(",")}`);
    await $(`[id="${target.id}"]`).click();
    await browser.waitUntil(async () => (await renderedRows()).find((r) => r.id === target.id)?.selected === true, {
      timeoutMsg: "the clicked row is not selected",
    });
    // Since stage 3 the click opens the message in the reading pane. Message 300 is older than
    // the prefetch period (30 days) and was never opened: offline the pane says so, naming the
    // account, and shows no error.
    const clicked = await waitForPane(target.label);
    await browser.keys(["ArrowDown"]);
    await browser.waitUntil(
      async () => (await renderedRows()).find((r) => r.selected)?.posinset === target.posinset + 1,
      { timeoutMsg: "ArrowDown did not move the selection" },
    );
    const active = await testId("message-list").getAttribute("aria-activedescendant");
    const next = (await renderedRows()).find((r) => r.selected);
    assert.equal(active, next.id);
    // The pane follows the selection.
    const following = await waitForPane(next.label);
    report.offline.listUsable = { clicked: target.posinset, afterArrowDown: target.posinset + 1 };
    report.offline.readingPane = { clicked, following };
    for (const pane of [clicked, following]) {
      assert.equal(pane.failed, false, `the reading pane shows an error offline: ${JSON.stringify(pane)}`);
      assert.ok(pane.body || pane.unavailable, `the reading pane is empty offline: ${JSON.stringify(pane)}`);
      if (pane.unavailable) {
        assert.match(pane.unavailable, /^This message is not on this computer yet/);
        assert.match(pane.unavailable, new RegExp(`as soon as ${ALICE.email} is online again`));
      }
    }
    await shootBothThemes("offline-reading-pane");
    await testId("message-list").click();
    await browser.keys(["Escape"]);
    await browser.waitUntil(async () => (await paneState()) === null, { timeoutMsg: "Escape did not close the reading pane" });
    await scrollListTo(0);
    await shootBothThemes("offline");
    // The folded rail: the account button names the state.
    await testId("rail-folders").click();
    report.offline.railLabel = await testId(`account-switch-${account.id}`).getAttribute("aria-label");
    assert.match(report.offline.railLabel, /, offline$/);
    await shootBothThemes("offline-rail");
    await testId("rail-folders").click();
    assert.deepEqual(await newCspViolations(), [], "CSP violations in the mail views");
  });

  it("recovers by itself when the server is back", async () => {
    await mark("start");
    await testmail("start");
    await mark("started");
    await browser.waitUntil(async () => (await onlyAccount()).status.state === "idle", {
      timeout: 150000,
      interval: 100,
      timeoutMsg: "the sync did not recover",
    });
    await browser.waitUntil(async () => (await syncText()) === "", { timeoutMsg: "the UI still shows a sync state" });
    const tl = await timeline();
    await stopTimeline();
    const states = tl.samples.filter((s) => s.t >= tl.marks.stop.t).map((s) => s.sync);
    report.recovery = {
      startCommandMs: Math.round(tl.marks.started.t - tl.marks.start.t),
      // The app may reconnect before testmail.sh returns, as soon as the ports are open.
      idleAfterStartCommandMs: firstAfter(tl, "start", (s) => s.sync === ""),
      offlineForMs: Math.round(
        tl.samples.find((s) => s.t >= tl.marks.start.t && s.sync === "").t - tl.samples.find((s) => /^Offline/.test(s.sync ?? "")).t,
      ),
      statesSeen: [...new Set(states)],
    };
    assert.ok(!states.some((s) => s === "Not syncing"), "never in the error state");
    // New mail arrives again.
    const subject = `Desktop test after recovery ${Date.now()}`;
    const sent = await deliverMail(subject);
    await browser.waitUntil(async () => (await renderedRows()).some((r) => r.label.includes(subject)), {
      timeout: 10000,
      interval: 50,
      timeoutMsg: "no new mail after the recovery",
    });
    report.recovery.newMailVisibleAfterSendStartMs = Date.now() - sent.startedAt;
    expected = await serverFolders();
    await waitForSynced(expected, 10000);
  });

  it("removes the account through the UI and leaves nothing in the keychain", async () => {
    const account = await onlyAccount();
    report.finalRemove = await removeAccountThroughUi(account);
    assert.equal(keychainHasEntry(), false, "the keychain entry is gone");
    await testId("rail-inbox").click();
    await waitForMainText("No account yet");
    assert.equal((await renderedRows()).length, 0);
    assert.deepEqual(await newCspViolations(), [], "CSP violations at the end");
  });
});
