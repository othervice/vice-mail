// Stage 23, item 1 (decision 0082) in the real app against the local test server, in German:
// 1. Accounts dragged into a new order in the column: while bob's heading is over alice's upper
//    half, a 4 px line shows in the gap before her and bob's account is dimmed; let go, bob is
//    first (the core's order). Escape cancels a drag: the line goes, a drop does nothing.
// 2. The same in the narrow rail with the account dots.
// 3. Files dragged onto the window: without a composer a drop is refused and nothing opens; with
//    a rich text composer, a PNG goes into the text and a 5 MB file (three pieces of 2 MiB) is
//    attached, both handed over as bytes through the real IPC bridge, never as a path.
//
// The drags are DOM drag events in the web view (`new DataTransfer()`, `new File()`): macOS
// refuses synthetic system mouse events to processes without the Accessibility permission (see
// decision 0082), so the system's drag gesture itself is checked by hand (docs/testing). Sets up
// alice and bob itself and removes them at the end.
// Screenshots go to tests/e2e/screenshots/s23drag-*.png, the facts to s23-drag-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const report = { screenshots: [], steps: {} };
let alice = null;
let bob = null;

async function addAccount(who, name) {
  return coreCall("add_account", {
    settings: {
      email: who.email,
      displayName: name,
      imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: who.email },
      smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: who.email },
    },
    password: who.password,
  });
}

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "de" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeout: 10000,
    timeoutMsg: `no ${theme} theme`,
  });
}

/** Light and dark screenshots of what is on screen now (a running drag stays as it is). */
async function both(name) {
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    await browser.pause(150);
    report.screenshots.push(await screenshot(`s23drag-${name}-${theme}`));
  }
  await setTheme("light");
}

/** One DOM drag event on an element; the running drag's DataTransfer stays on the page. */
const dragEvent = (selector, type, { clientY = null } = {}) =>
  browser.execute(
    (sel, kind, y) => {
      const element = document.querySelector(sel);
      if (!element) return null;
      if (kind === "dragstart") window.__e2eDrag = new DataTransfer();
      const box = element.getBoundingClientRect();
      const event = new DragEvent(kind, {
        bubbles: true,
        cancelable: true,
        dataTransfer: window.__e2eDrag,
        clientX: box.left + box.width / 2,
        clientY: y ?? box.top + box.height / 2,
      });
      element.dispatchEvent(event);
      return { prevented: event.defaultPrevented, dropEffect: window.__e2eDrag.dropEffect };
    },
    selector,
    type,
    clientY,
  );

/** The top of an element (for a pointer in its upper half). */
const topOf = (selector) => browser.execute((sel) => document.querySelector(sel)?.getBoundingClientRect().top ?? null, selector);

/** The lines shown in a list of accounts, and how one is drawn (pseudo element, 4 px, accent). */
const linesIn = (zone) =>
  browser.execute((sel) => {
    const items = [...(document.querySelector(sel)?.querySelectorAll("[data-account-drop]") ?? [])];
    return items.map((item) => {
      const side = item.getAttribute("data-account-drop");
      const style = getComputedStyle(item, side === "before" ? "::before" : "::after");
      return { side, id: item.getAttribute("data-account-drop-item"), content: style.content, height: style.height, color: style.backgroundColor };
    });
  }, zone);

/**
 * The opacity an element ends at (transitions finished: a window started from a terminal session
 * gets no animation frames, see `screenshot`).
 */
const opacityOf = (selector) =>
  browser.execute((sel) => {
    for (const animation of document.getAnimations()) animation.finish();
    return getComputedStyle(document.querySelector(sel)).opacity;
  }, selector);

const order = async () => (await coreCall("list_accounts")).map((a) => a.email);

/** Files made in the page and dragged over `selector` from outside: enter and over. */
const dragFiles = (selector, files) =>
  browser.execute(
    (sel, specs) => {
      const transfer = new DataTransfer();
      for (const spec of specs) {
        const bytes = spec.base64 ? Uint8Array.from(atob(spec.base64), (c) => c.charCodeAt(0)) : new Uint8Array(spec.size).map((_, i) => i % 251);
        transfer.items.add(new File([bytes], spec.name, { type: spec.type }));
      }
      window.__e2eFiles = transfer;
      const target = document.querySelector(sel);
      const fire = (kind) => {
        const event = new DragEvent(kind, { bubbles: true, cancelable: true, dataTransfer: transfer, clientX: 400, clientY: 300 });
        target.dispatchEvent(event);
        return event.defaultPrevented;
      };
      return { enter: fire("dragenter"), over: fire("dragover"), dropEffect: transfer.dropEffect, types: [...transfer.types] };
    },
    selector,
    files,
  );

const dropFiles = (selector) =>
  browser.execute((sel) => {
    const event = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: window.__e2eFiles, clientX: 400, clientY: 300 });
    document.querySelector(sel).dispatchEvent(event);
    return { prevented: event.defaultPrevented, location: window.location.pathname };
  }, selector);

/** A 2×2 PNG. */
const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP8z8DAwMDAxMDAwMDAAAANHQEDasKb6QAAAABJRU5ErkJggg==";

describe("stage 23: dragging inside the app, files from outside as bytes", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "de" } });
    alice = await addAccount(ALICE, "Alice Archer");
    bob = await addAccount(BOB, "Bob Baker");
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first syncs did not finish" },
    );
    if (await testId("update-consent-no").isExisting()) await testId("update-consent-no").click();
    for (const account of [alice, bob]) await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    await setSidebar(true);
  });

  after(async () => {
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s23-drag-report.json"), JSON.stringify(report, null, 2));
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
  });

  it("drags an account in the column: line in the gap, dimmed while dragged, the new order kept", async () => {
    assert.deepEqual(await order(), [ALICE.email, BOB.email]);
    const zone = '[data-testid="sidebar-accounts"]';
    const aliceSection = `[data-testid="sidebar-account-section-${alice.id}"]`;
    await $(aliceSection).waitForExist({ timeout: 10000 });
    await dragEvent(`[data-testid="sidebar-account-${bob.id}"]`, "dragstart");
    const top = await topOf(aliceSection);
    const over = await dragEvent(aliceSection, "dragover", { clientY: top + 4 });
    report.steps.columnOver = over;
    // (WebKit keeps `dropEffect` of a script-made DataTransfer at "none"; Vitest checks it.)
    assert.equal(over.prevented, true, "the drop is offered");
    const lines = await linesIn(zone);
    report.steps.columnLine = lines;
    assert.equal(lines.length, 1);
    assert.equal(lines[0].side, "before");
    assert.equal(lines[0].id, alice.id);
    assert.equal(lines[0].height, "4px");
    const dimmed = await opacityOf(`[data-testid="sidebar-account-section-${bob.id}"]`);
    assert.equal(dimmed, "0.5");
    await both("column-line");
    const dropped = await dragEvent(aliceSection, "drop", { clientY: top + 4 });
    await dragEvent(`[data-testid="sidebar-account-${bob.id}"]`, "dragend");
    assert.equal(dropped.prevented, true);
    await browser.waitUntil(async () => (await order())[0] === BOB.email, { timeoutMsg: "the drag did not put bob first" });
    assert.deepEqual(await linesIn(zone), []);

    // Escape cancels: the line goes, and a drop afterwards does nothing.
    await dragEvent(`[data-testid="sidebar-account-${alice.id}"]`, "dragstart");
    const bobTop = await topOf(`[data-testid="sidebar-account-section-${bob.id}"]`);
    await dragEvent(`[data-testid="sidebar-account-section-${bob.id}"]`, "dragover", { clientY: bobTop + 4 });
    assert.equal((await linesIn(zone)).length, 1);
    await browser.keys("Escape");
    assert.deepEqual(await linesIn(zone), []);
    const late = await dragEvent(`[data-testid="sidebar-account-section-${bob.id}"]`, "drop", { clientY: bobTop + 4 });
    await dragEvent(`[data-testid="sidebar-account-${alice.id}"]`, "dragend");
    assert.equal(late.prevented, false);
    await browser.pause(500);
    assert.deepEqual(await order(), [BOB.email, ALICE.email]);
  });

  it("drags an account dot in the narrow rail", async () => {
    await setSidebar(false);
    // The second account goes before the first, whatever the order is now.
    const [first, second] = (await coreCall("list_accounts")).map((a) => a.id);
    const zone = '[data-testid="rail-accounts"]';
    const firstDot = `[data-account-drop-item="${first}"]`;
    await $(firstDot).waitForExist({ timeout: 10000 });
    await dragEvent(`[data-testid="account-switch-${second}"]`, "dragstart");
    const dimmed = await opacityOf(`[data-testid="account-switch-${second}"]`);
    assert.equal(dimmed, "0.5");
    const top = await topOf(firstDot);
    const over = await dragEvent(firstDot, "dragover", { clientY: top + 2 });
    assert.equal(over.prevented, true);
    const lines = await linesIn(zone);
    report.steps.railLine = lines;
    assert.deepEqual(
      lines.map((l) => [l.side, l.id, l.height]),
      [["before", first, "4px"]],
    );
    await both("rail-line");
    await dragEvent(firstDot, "drop", { clientY: top + 2 });
    await dragEvent(`[data-testid="account-switch-${second}"]`, "dragend");
    await browser.waitUntil(async () => (await coreCall("list_accounts"))[0]?.id === second, { timeoutMsg: "the rail drag did not move the account" });
    const rail = await browser.execute(() => [...document.querySelectorAll('[data-testid^="account-switch-"]')].map((b) => b.dataset.testid));
    assert.deepEqual(rail, [`account-switch-${second}`, `account-switch-${first}`]);
    assert.deepEqual(await linesIn(zone), []);
  });

  it("takes files dropped on a rich text composer as bytes: an image into the text, a 5 MB file attached", async () => {
    await setSidebar(true);
    // Without a composer: refused, handled, nothing opens.
    const before = await browser.execute(() => window.location.href);
    const refused = await dragFiles("#root", [{ name: "notes.txt", type: "text/plain", size: 10 }]);
    assert.equal(refused.over, true, "the page handles the drag");
    const ignored = await dropFiles("#root");
    assert.equal(ignored.prevented, true);
    await browser.pause(300);
    assert.equal(await browser.execute(() => window.location.href), before);
    assert.equal(await testId("composer").isExisting(), false);

    await $('button[aria-label="Neue E-Mail"]').click();
    await testId("composer").waitForExist({ timeout: 10000 });
    if (await testId("composer-format-html").isExisting()) await testId("composer-format-html").click();
    const big = 5 * 1024 * 1024 + 7;
    const over = await dragFiles('[data-testid="composer"]', [
      { name: "Diagramm.png", type: "image/png", base64: PNG },
      { name: "Großer Bericht.bin", type: "application/octet-stream", size: big },
    ]);
    report.steps.composerOver = over;
    assert.equal(over.over, true);
    assert.ok(over.types.includes("Files"), JSON.stringify(over.types));
    await testId("composer-drop-hint").waitForExist({ timeout: 5000 });
    report.screenshots.push(await screenshot("s23drag-composer-hint"));
    const started = Date.now();
    const dropped = await dropFiles('[data-testid="composer"]');
    assert.equal(dropped.prevented, true);
    assert.equal(await browser.execute(() => window.location.href), before);
    let draft = null;
    await browser.waitUntil(
      async () => {
        const drafts = await coreCall("list_drafts", { accountId: null });
        const newest = drafts.sort((a, b) => b.updatedAt - a.updatedAt)[0];
        if (!newest) return false;
        draft = await coreCall("get_draft", { draftId: newest.id });
        return draft.attachments.length >= 2;
      },
      { timeout: 60000, interval: 300, timeoutMsg: "the dropped files did not arrive" },
    );
    report.steps.uploadMs = Date.now() - started;
    report.steps.attachments = draft.attachments;
    const image = draft.attachments.find((a) => a.fileName === "Diagramm.png");
    const file = draft.attachments.find((a) => a.fileName === "Großer Bericht.bin");
    assert.ok(image && image.contentId, `the PNG went into the text: ${JSON.stringify(draft.attachments)}`);
    assert.equal(image.mime, "image/png");
    assert.ok(file && file.contentId === null, "the file is an attachment");
    assert.equal(file.size, big);
    await testId("composer-attachment").waitForExist({ timeout: 5000 });
    report.screenshots.push(await screenshot("s23drag-composer-dropped"));
    await browser.waitUntil(async () => !(await testId("composer-drop-hint").isExisting()), { timeoutMsg: "the hint stayed" });
    await coreCall("delete_draft", { draftId: draft.id });
    assert.deepEqual(await cspViolations(), []);
  });
});
