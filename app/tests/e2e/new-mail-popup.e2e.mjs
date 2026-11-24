// Stage 18, block C (decision 0051) in the real app: the popup for new mail. In `specs` of
// wdio.conf.mjs after contacts.e2e.mjs: it delivers messages of its own to alice's inbox, deletes
// one into the trash and removes its account at the end.
//
// 1. alice's account, read-only (as every new account). A message delivered on the test server
//    after the first sync opens the popup's window (`new-mail-popup`): sender and subject at
//    once, the snippet once the prefetch stored the body. "Delete" is disabled and says why. The
//    window never became key, the app's active state did not change, the window is floating, on
//    every Space, not in the Window menu, rounded, and at the bottom right of the screen's
//    visible frame. Its page may call its own commands only.
//    While the pointer is on it (a pointer event in its page), it does not hide itself.
// 2. "Read" (a real click into the non-key window) shows the message in the main window; the
//    popup closes.
// 3. With writes on: "Delete" moves the next message into the trash on the server, "Undo" brings
//    it back, "Delete" again.
// 4. Five messages: three cards, "2 more in the inbox", which opens the inbox.
// 5. Unattended, the popup hides itself after ten seconds on screen; a page the system hides
//    (lock screen of an unattended machine) waits, and its close button closes it.
// 6. With "System notification" no popup opens.
//
// Screenshots go to tests/e2e/screenshots/popup-*.png, the facts to popup-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, invoke, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, deliverMail, SERVER, serverFolders } from "./support/testmail.mjs";

const POPUP = "new-mail-popup";
const AXE_SOURCE = fs.readFileSync(createRequire(import.meta.url).resolve("axe-core/axe.min.js"), "utf8");
const RUN = Date.now().toString(36);
const report = { screenshots: [], steps: {} };
let main;
let account;

const handles = () => browser.getWindowHandles();

/** Waits for the popup's window and switches to it. */
async function toPopup(timeout = 90000) {
  await browser.waitUntil(async () => (await handles()).includes(POPUP), {
    timeout,
    interval: 250,
    timeoutMsg: "the popup did not open",
  });
  await browser.switchToWindow(POPUP);
  await testId("popup-card").waitForExist({ timeout: 20000 });
}

/**
 * The pointer enters or leaves the popup, as the mouse would (a pointer event in its page): the
 * popup does not hide itself while the pointer is on it. WebDriver cannot move the system's
 * pointer; the real clicks below are delivered to the web view directly and leave no "leave".
 */
async function pointer(onPopup) {
  await browser.execute((enter) => {
    document
      .querySelector('[data-testid="popup-root"]')
      ?.dispatchEvent(new PointerEvent(enter ? "pointerover" : "pointerout", { bubbles: true, pointerType: "mouse" }));
  }, onPopup);
  await browser.waitUntil(
    async () => (await browser.execute(() => document.querySelector('[data-testid="popup-root"]')?.hasAttribute("data-hovered") ?? null)) === onPopup,
    { timeout: 5000, timeoutMsg: `the popup did not see the pointer ${onPopup ? "enter" : "leave"}` },
  );
}

/** axe-core with every rule (contrast on the rendered colours) in the popup's page. */
async function axeFindings() {
  await browser.execute(`${AXE_SOURCE}\n;return typeof window.axe;`);
  const result = await browser.execute(async () => {
    try {
      const results = await window.axe.run(document, { resultTypes: ["violations"] });
      return { ok: results.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.map((n) => n.target.join(" ")) })) };
    } catch (e) {
      return { failure: String(e) };
    }
  });
  if ("failure" in result) throw new Error(`axe.run failed: ${result.failure}`);
  return result.ok;
}

async function toMain() {
  await browser.switchToWindow(main);
}

/** The cards as the popup shows them. */
const cardsShown = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="popup-card"]')].map((card) => ({
      id: Number(card.getAttribute("data-message-id")),
      deleted: card.hasAttribute("data-deleted"),
      sender: card.querySelector('[data-testid="popup-sender"]')?.textContent ?? null,
      subject: card.querySelector('[data-testid="popup-subject"]')?.textContent ?? null,
      snippet: card.querySelector('[data-testid="popup-snippet"]')?.textContent ?? null,
      deleteDisabled: card.querySelector('[data-testid="popup-delete"]')?.disabled ?? null,
      readOnly: card.querySelector('[data-testid="popup-read-only"]')?.textContent ?? null,
    })),
  );

/** The card of `subject` once it is shown. */
async function cardOf(subject, timeout = 30000) {
  let found = null;
  await browser.waitUntil(
    async () => {
      found = (await cardsShown()).find((c) => c.subject === subject) ?? null;
      return found !== null;
    },
    { timeout, interval: 250, timeoutMsg: `no card for ${subject}` },
  );
  return found;
}

/**
 * A real click (an NSEvent handed to the web view, as the mouse does) on the element of the card
 * of `messageId` in the popup's window, which never is key: the first click must count.
 */
async function nativeClick(messageId, testid, { closes = false } = {}) {
  const rect = await browser.execute(
    (id, tid) => {
      const element = document.querySelector(`[data-testid="popup-card"][data-message-id="${id}"] [data-testid="${tid}"]`);
      if (!element) return null;
      const r = element.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2, viewportHeight: innerHeight };
    },
    messageId,
    testid,
  );
  assert.ok(rect, `no ${testid} on card ${messageId}`);
  try {
    const result = await invoke("e2e_native_mouse", rect);
    assert.ok("ok" in result, `native click failed: ${JSON.stringify(result)}`);
    return result.ok;
  } catch (e) {
    // The click closed the popup (its last card went) before the answer came back.
    if (closes && /no such window|No window could be found|Channel closed/i.test(String(e.message ?? e))) return "the popup closed";
    throw e;
  }
}

async function windowState(label) {
  const result = await invoke("e2e_window_state", { label });
  if ("failure" in result) throw new Error(`e2e_window_state failed: ${result.failure}`);
  return result.ok;
}

/** Messages in alice's trash on the server (the folder with `\Trash`). */
async function trashOnServer() {
  const trash = Object.values(await serverFolders()).find((folder) => folder.flags.includes("\\trash"));
  assert.ok(trash, "the server has no trash folder");
  return trash.messages;
}

async function mailboxByRole(role) {
  const mailboxes = await coreCall("list_mailboxes", { accountId: account.id });
  const found = mailboxes.find((m) => m.role === role);
  assert.ok(found, `no ${role} folder`);
  return found;
}

async function subjectsIn(role) {
  const mailbox = await mailboxByRole(role);
  const page = await coreCall("list_messages", { mailboxId: mailbox.id, cursor: null, limit: 200 });
  return page.items.map((item) => item.subject);
}

/**
 * A screenshot of the popup's web content (WebDriver draws the page only: the window's rounded
 * corners and shadow come from the system and are checked through `e2e_window_state`).
 */
async function shot(name) {
  const file = await screenshot(name);
  report.screenshots.push(file);
  return file;
}

async function waitPopupGone(timeout, message) {
  await browser.waitUntil(async () => !(await handles()).includes(POPUP), { timeout, interval: 250, timeoutMsg: message });
}

describe("stage 18: the popup for new mail", () => {
  before(async () => {
    await attach();
    main = await browser.getWindowHandle();
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
    const settings = await coreCall("get_notification_settings");
    report.steps.defaultStyle = settings.newMailStyle;
    assert.equal(settings.newMailStyle, "popup", "the popup is the default");
    // The first sync announced nothing.
    assert.ok(!(await handles()).includes(POPUP), "no popup after the first sync");
  });

  after(async () => {
    await toMain().catch(() => undefined);
    await coreCall("set_notification_settings", { settings: { ...(await coreCall("get_notification_settings")), newMailStyle: "popup" } }).catch(
      () => undefined,
    );
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "popup-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("a new mail opens the popup with sender and snippet, without taking the focus; Delete is off for a read-only account", async () => {
    await toMain();
    const before = await windowState("main");
    report.steps.mainBefore = { key: before.key, appActive: before.appActive };
    const subject = `Popup one ${RUN}`;
    await deliverMail(subject);
    await toPopup();
    await pointer(true);
    report.steps.firstPage = await browser.execute(() => ({ visibility: document.visibilityState, focus: document.hasFocus() }));
    const card = await cardOf(subject);
    assert.equal(card.sender, "Bob Example");
    assert.equal(card.deleteDisabled, true, "Delete is disabled for a read-only account");
    assert.match(card.readOnly ?? "", /Read-only account/);
    // The snippet arrives once the prefetch stored the body.
    await browser.waitUntil(async () => ((await cardOf(subject)).snippet ?? "").includes("Delivered by the desktop test"), {
      timeout: 90000,
      interval: 500,
      timeoutMsg: "the snippet did not arrive",
    });
    report.steps.card = await cardOf(subject);

    // The window: never key, the app's state unchanged, floating, every Space, not in the menu,
    // rounded, at the bottom right of the visible frame (menu bar and Dock left out).
    const popup = await windowState(POPUP);
    const mainAfter = await windowState("main");
    report.steps.window = popup;
    report.steps.mainAfter = { key: mainAfter.key, appActive: mainAfter.appActive };
    assert.equal(popup.key, false, "the popup is not key");
    assert.equal(popup.canBecomeKey, false, "the popup cannot become key");
    assert.equal(popup.visible, true);
    assert.equal(mainAfter.appActive, before.appActive, "the app's active state did not change");
    assert.equal(mainAfter.key, before.key, "the main window kept (or did not get) the focus");
    assert.ok(popup.level >= 3, `floating level, got ${popup.level}`);
    const CAN_JOIN_ALL_SPACES = 1;
    assert.ok((popup.collectionBehavior & CAN_JOIN_ALL_SPACES) !== 0, "on every Space");
    assert.equal(popup.excludedFromWindowsMenu, true);
    assert.equal(popup.opaque, false);
    assert.equal(popup.hasShadow, true);
    assert.equal(popup.cornerRadius, 16);
    const [fx, fy, fw] = popup.frame;
    const [vx, vy, vw] = popup.visibleFrame;
    assert.equal(fw, 360, "width");
    assert.ok(Math.abs(vx + vw - (fx + fw) - 16) <= 1, `16 points from the right edge: ${JSON.stringify(popup)}`);
    assert.ok(Math.abs(fy - vy - 16) <= 1, `16 points above the Dock (bottom of the visible frame): ${JSON.stringify(popup)}`);

    // Its page may call its own commands only.
    const own = await invoke("get_new_mail_popup");
    assert.ok("ok" in own, JSON.stringify(own));
    for (const command of ["list_accounts", "get_message", "apply_action", "set_notification_settings"]) {
      const refused = await invoke(command, {});
      assert.ok("failure" in refused && refused.failure.includes("not allowed in the new-mail popup"), `${command}: ${JSON.stringify(refused)}`);
    }
    const close = await invoke("plugin:window|close", { label: POPUP });
    assert.ok("failure" in close, "no window permission");
    assert.deepEqual(await cspViolations(), [], "no CSP violation in the popup");
    const findings = await axeFindings();
    report.steps.axeLight = findings;
    assert.deepEqual(findings, [], "axe finds nothing in the popup (light)");
    await shot("popup-one");
    // The pointer has been on the popup longer than ten seconds: it is still there.
    await browser.pause(11000);
    assert.ok((await handles()).includes(POPUP), "the popup stays while the pointer is on it");
  });

  it("Read (a real first click) shows the message in the main window and closes the popup", async () => {
    await browser.switchToWindow(POPUP);
    const subject = `Popup one ${RUN}`;
    const card = await cardOf(subject);
    report.steps.readClick = await nativeClick(card.id, "popup-read", { closes: true });
    await waitPopupGone(10000, "the popup did not close after Read");
    await toMain();
    await browser.waitUntil(
      async () => (await browser.execute(() => document.querySelector('[data-testid="reading-subject"]')?.textContent ?? "")) === subject,
      { timeout: 20000, timeoutMsg: "the main window does not show the message" },
    );
    report.steps.readShown = subject;
  });

  it("Delete moves the message into the trash for an account with writes on; Undo brings it back", async () => {
    await toMain();
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    // The popup must not take the focus from the main window or the app in front (macOS does not
    // activate the app for the test's "Read", see decision 0051).
    const before = await windowState("main");
    const subject = `Popup delete ${RUN}`;
    await deliverMail(subject);
    await toPopup();
    await pointer(true);
    const after = await windowState("main");
    report.steps.focusWhileActive = { before: { key: before.key, appActive: before.appActive }, after: { key: after.key, appActive: after.appActive } };
    assert.equal(after.key, before.key, "the main window keeps the focus");
    assert.equal(after.appActive, before.appActive);
    assert.equal((await windowState(POPUP)).key, false);
    const card = await cardOf(subject);
    assert.equal(card.deleteDisabled, false);
    const trashBefore = await trashOnServer();
    await nativeClick(card.id, "popup-delete");
    await testId("popup-deleted").waitForExist({ timeout: 10000 });
    await toMain();
    await browser.waitUntil(async () => (await subjectsIn("trash")).includes(subject), {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the message is not in the trash",
    });
    await browser.waitUntil(async () => (await trashOnServer()) === trashBefore + 1, {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: "the server's trash did not get the message",
    });
    report.steps.deleted = { trashBefore, trashAfter: trashBefore + 1 };
    await browser.switchToWindow(POPUP);
    await shot("popup-deleted");
    await nativeClick(card.id, "popup-undo");
    await browser.waitUntil(async () => (await cardOf(subject)).deleted === false, { timeout: 10000, timeoutMsg: "Undo did not restore the card" });
    await toMain();
    await browser.waitUntil(async () => (await subjectsIn("inbox")).includes(subject), {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "Undo did not bring the message back to the inbox",
    });
    await browser.waitUntil(async () => (await trashOnServer()) === trashBefore, {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: "Undo did not take the message out of the server's trash",
    });
    report.steps.undone = true;
    await browser.switchToWindow(POPUP);
    await nativeClick(card.id, "popup-delete");
    await testId("popup-deleted").waitForExist({ timeout: 10000 });
    // The pointer leaves: on screen the deleted card goes after a few seconds, and the popup
    // with it; under the lock screen of an unattended machine it waits, and its close button
    // (a real click) closes it.
    await pointer(false);
    const visibility = await browser.execute(() => document.visibilityState);
    report.steps.deletedPage = visibility;
    if (visibility !== "visible") await nativeClick(card.id, "popup-close", { closes: true });
    await toMain();
    await browser.waitUntil(async () => (await trashOnServer()) === trashBefore + 1, {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: "the message is not in the server's trash again",
    });
    // The deleted card goes after a few seconds; the popup closes with it.
    await waitPopupGone(20000, "the popup did not close after the deleted card went");
  });

  it("several messages: three cards and “2 more in the inbox”, which opens the inbox", async () => {
    await toMain();
    assert.ok(!(await handles()).includes(POPUP), "no popup left over");
    // In the dark theme this time: the popup follows the app's theme.
    await coreCall("set_preferences", { preferences: { theme: "dark", language: "en" } });
    const subjects = [1, 2, 3, 4, 5].map((n) => `Popup many ${n} ${RUN}`);
    for (const subject of subjects) await deliverMail(subject);
    await toPopup();
    await pointer(true);
    await browser.waitUntil(
      async () => {
        const more = await $('[data-testid="popup-more"]');
        return (await more.isExisting()) && (await more.getText()) === "2 more in the inbox";
      },
      { timeout: 90000, interval: 500, timeoutMsg: "no “2 more” line" },
    );
    const shown = await cardsShown();
    report.steps.several = shown.map((c) => c.subject);
    assert.equal(shown.length, 3);
    assert.deepEqual(
      shown.map((c) => c.subject),
      [subjects[4], subjects[3], subjects[2]],
      "newest on top",
    );
    // Wait for the snippets, so the picture shows the cards as the user sees them.
    await browser
      .waitUntil(async () => (await cardsShown()).every((c) => c.snippet), { timeout: 60000, interval: 500 })
      .catch(() => undefined);
    assert.equal(await browser.execute(() => document.documentElement.dataset.theme), "dark");
    const findings = await axeFindings();
    report.steps.axeDark = findings;
    assert.deepEqual(findings, [], "axe finds nothing in the popup (dark)");
    await shot("popup-several-dark");
    await $('[data-testid="popup-more"]').click();
    await waitPopupGone(10000, "the popup did not close after “N more”");
    await toMain();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    await browser.waitUntil(async () => (await $$('[data-testid="message-list"] [data-testid="row-name"]')).length > 0, {
      timeout: 20000,
      timeoutMsg: "the inbox is not shown",
    });
  });

  it("hides itself after ten seconds on screen when left alone, and waits while it cannot be seen", async () => {
    await toMain();
    const subject = `Popup alone ${RUN}`;
    await deliverMail(subject);
    await toPopup();
    const opened = Date.now();
    const card = await cardOf(subject);
    // Only visible time counts (decision 0051). A Mac mini that nobody sits at shows the lock
    // screen over every window: WebKit then takes the page for hidden, and the popup waits.
    const page = await browser.execute(() => ({ visibility: document.visibilityState, focus: document.hasFocus() }));
    report.steps.alonePage = page;
    if (page.visibility === "visible") {
      await waitPopupGone(30000, "the popup did not hide itself");
      report.steps.hiddenAfterMs = Date.now() - opened;
      assert.ok(report.steps.hiddenAfterMs >= 8000, `hid too early: ${report.steps.hiddenAfterMs} ms`);
      return;
    }
    await browser.pause(15000);
    assert.ok((await handles()).includes(POPUP), "a popup nobody can see stays");
    report.steps.hiddenPageKept = `${String(Date.now() - opened)} ms, page ${page.visibility}`;
    // Closing the card (a real click) closes the popup with it.
    await nativeClick(card.id, "popup-close", { closes: true });
    await waitPopupGone(10000, "the close button did not close the popup");
  });

  it("with “System notification” no popup opens", async () => {
    await waitPopupGone(10000, "a popup is left over");
    await toMain();
    const settings = await coreCall("get_notification_settings");
    await coreCall("set_notification_settings", { settings: { ...settings, newMailStyle: "system" } });
    const subject = `Popup system ${RUN}`;
    await deliverMail(subject);
    await browser.waitUntil(async () => (await subjectsIn("inbox")).includes(subject), {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the message did not arrive",
    });
    await browser.pause(3000);
    assert.ok(!(await handles()).includes(POPUP), "no popup with the system's notifications");
    await coreCall("set_notification_settings", { settings: { ...settings, newMailStyle: "popup" } });
  });
});
