// Stage 22, items 1, 2 and 12 (decisions 0078, 0081) in the real app against the local test
// server, in German, light and dark:
// 1. New accounts show their address in the column. Alice moves down with "Nach unten" in her
//    menu, back up by dragging her heading before bob's (the line in the gap), then down again:
//    the order the restart spec (stage22-accounts-restart.e2e.mjs) expects.
// 2. Bob's account is renamed in place (double click, type, Enter): the name shows in the
//    column, the settings, the rail's account switcher and the rows of "Alle Posteingänge"; the
//    sender name of his identity stays.
// 3. "Alle Posteingänge": alice's row dragged onto bob's inbox (another account's folder): while
//    over it, the hint says "Nach Posteingang verschieben"; with bob read-only the folder refuses
//    and says why. Dropped, the mail ends in bob's inbox on the server and leaves alice's.
// 4. Settings → Konten with the move buttons, and bob's page with "Name in der Liste".
//
// The drags are DOM drag events in the web view (`new DataTransfer()`): WebDriver and the native
// mouse events of the test build start no AppKit drag session, so the system's drag gesture itself
// is not exercised here. Sets up alice and bob itself; the restart spec removes them.
// Screenshots go to tests/e2e/screenshots/s22acc-*.png, the facts to s22-accounts-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, readState, screenshot, SCREENSHOT_DIR, setSidebar, testId, writeState } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, appendToBob, bobMessage, folderCount } from "./support/testmail.mjs";

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const RUN = Date.now().toString(36);
const MARK = `S22A ${RUN}`;
const report = { screenshots: [], steps: {} };
let alice = null;
let bob = null;

function eml(subject, from, to, index) {
  return [
    `Date: ${new Date(Date.now() - index * 60000).toUTCString().replace("GMT", "+0000")}`,
    `From: ${from}`,
    `To: ${to}`,
    `Subject: ${subject}`,
    `Message-ID: <s22a-${RUN}-${String(index)}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    `Hallo, das ist ${subject}.`,
    "",
  ].join("\r\n");
}

function writeMessage(subject, from, to, index) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "s22-accounts-"));
  const file = path.join(dir, "m.eml");
  fs.writeFileSync(file, eml(subject, from, to, index));
  return file;
}

async function addAccount(who) {
  return coreCall("add_account", {
    settings: {
      email: who.email,
      displayName: who === ALICE ? "Alice Archer" : "Bob Baker",
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

/** Light and dark screenshots of what is on screen now (without blurring an open field). */
async function both(name, { keepFocus = false } = {}) {
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    await browser.pause(200);
    if (keepFocus) {
      fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
      await browser.execute(() => {
        for (const animation of document.getAnimations()) animation.finish();
      });
      const file = path.join(SCREENSHOT_DIR, `s22acc-${name}-${theme}.png`);
      await browser.saveScreenshot(file);
      report.screenshots.push(file);
    } else {
      report.screenshots.push(await screenshot(`s22acc-${name}-${theme}`));
    }
  }
  await setTheme("light");
}

/** The accounts of the column, top to bottom, as their headings show them. */
const columnOrder = () =>
  browser.execute(() => [...document.querySelectorAll('[data-testid^="sidebar-account-name-"]')].map((name) => name.textContent));

/** A click through the DOM (the account's "…" shows only on hover or focus). */
const domClick = (testid) =>
  browser.execute((id) => {
    const element = document.querySelector(`[data-testid="${id}"]`);
    if (!element) return false;
    element.click();
    return true;
  }, testid);

/** One DOM drag event on an element (the page keeps the DataTransfer of the running drag). */
const dragEvent = (selector, type, { clientY = null, altKey = false } = {}) =>
  browser.execute(
    (sel, kind, y, alt) => {
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
        altKey: alt,
      });
      element.dispatchEvent(event);
      return { prevented: event.defaultPrevented, dropEffect: window.__e2eDrag.dropEffect, types: [...window.__e2eDrag.types] };
    },
    selector,
    type,
    clientY,
    altKey,
  );

describe("stage 22: account order and names, mail dragged across accounts", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "de" } });
    alice = await addAccount(ALICE);
    bob = await addAccount(BOB);
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first syncs did not finish" },
    );
    if (await testId("update-consent-no").isExisting()) await testId("update-consent-no").click();
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    for (const account of [alice, bob]) await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    await appendMessages("INBOX", [writeMessage(`${MARK} drop`, "Grace Hopper <grace@example.org>", "Alice Archer <alice@vicemail.test>", 1)]);
    await appendToBob([writeMessage(`${MARK} bob`, "Grace Hopper <grace@example.org>", "Bob Baker <bob@vicemail.test>", 2)]);
    await setSidebar(true);
  });

  after(async () => {
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s22-accounts-report.json"), JSON.stringify(report, null, 2));
  });

  it("shows new accounts by their address and moves them by the menu and by dragging", async () => {
    // Decision 0078: the typed name became the sender name only.
    assert.deepEqual(await columnOrder(), [ALICE.email, BOB.email]);
    const identities = await coreCall("list_identities", { accountId: alice.id });
    report.steps.aliceSender = identities[0]?.name;
    assert.equal(identities[0]?.name, "Alice Archer");

    assert.ok(await domClick(`account-menu-${alice.id}`), "alice's menu");
    await testId(`account-move-down-${alice.id}`).waitForExist({ timeoutMsg: "no Nach unten" });
    assert.ok(await domClick(`account-move-down-${alice.id}`));
    await browser.waitUntil(async () => JSON.stringify(await columnOrder()) === JSON.stringify([BOB.email, ALICE.email]), {
      timeoutMsg: "alice did not move down",
    });
    await browser.waitUntil(async () => (await testId(`account-announce-${alice.id}`).getText()) !== "", { timeoutMsg: "nothing announced" });
    report.steps.menuAnnounced = await browser.execute((id) => document.querySelector(`[data-testid="account-announce-${id}"]`)?.textContent, alice.id);
    assert.equal(report.steps.menuAnnounced, "alice@vicemail.test steht jetzt an Stelle 2 von 2.");

    // Dragging alice's heading before bob's: the line shows in the gap above bob.
    const start = await dragEvent(`[data-testid="sidebar-account-${alice.id}"]`, "dragstart");
    assert.deepEqual(start.types, ["application/x-mail-account"]);
    const section = `[data-testid="sidebar-account-section-${bob.id}"]`;
    const top = await browser.execute((sel) => document.querySelector(sel).getBoundingClientRect().top + 4, section);
    await dragEvent(section, "dragenter", { clientY: top });
    const over = await dragEvent(section, "dragover", { clientY: top });
    assert.equal(over.prevented, true, "the section takes the account");
    assert.equal(await testId(`sidebar-account-section-${bob.id}`).getAttribute("data-account-drop"), "before");
    await both("order-drag-line");
    await dragEvent(section, "drop", { clientY: top });
    await dragEvent(`[data-testid="sidebar-account-${alice.id}"]`, "dragend");
    await browser.waitUntil(async () => JSON.stringify(await columnOrder()) === JSON.stringify([ALICE.email, BOB.email]), {
      timeoutMsg: "the drag did not put alice first",
    });
    // Down again with the menu: the order the restart has to keep.
    assert.ok(await domClick(`account-menu-${alice.id}`));
    await testId(`account-move-down-${alice.id}`).waitForExist();
    assert.ok(await domClick(`account-move-down-${alice.id}`));
    await browser.waitUntil(async () => JSON.stringify(await columnOrder()) === JSON.stringify([BOB.email, ALICE.email]), {
      timeoutMsg: "alice did not move down again",
    });
    assert.deepEqual(
      (await coreCall("list_accounts")).map((a) => a.email),
      [BOB.email, ALICE.email],
      "the core keeps the order",
    );
    report.steps.order = await columnOrder();
  });

  it("renames bob in place; the name shows in every list, the sender name stays", async () => {
    await browser.execute((id) => {
      document.querySelector(`[data-testid="sidebar-account-name-${id}"]`).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true }));
    }, bob.id);
    const field = testId(`account-name-field-${bob.id}`);
    await field.waitForExist({ timeoutMsg: "no name field" });
    assert.equal(await browser.execute((id) => document.activeElement?.dataset.testid === `account-name-field-${id}`, bob.id), true, "the field has the focus");
    await browser.keys("Arbeit");
    await both("rename-field", { keepFocus: true });
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await columnOrder())[0] === "Arbeit", { timeoutMsg: "the column did not show Arbeit" });
    assert.equal((await coreCall("list_accounts")).find((a) => a.id === bob.id).displayName, "Arbeit");
    assert.equal((await coreCall("list_identities", { accountId: bob.id }))[0]?.name, "Bob Baker", "the sender name stays");
    await browser.waitUntil(async () => (await testId(`account-announce-${bob.id}`).getText()) !== "");
    report.steps.renameAnnounced = await browser.execute((id) => document.querySelector(`[data-testid="account-announce-${id}"]`)?.textContent, bob.id);
    assert.equal(report.steps.renameAnnounced, "bob@vicemail.test steht jetzt als Arbeit in den Listen.");

    // "Alle Posteingänge": bob's row names its account "Arbeit".
    assert.ok(await domClick("unified-all"));
    await browser.waitUntil(
      async () => browser.execute((mark) => [...document.querySelectorAll('[data-testid="message-row"]')].some((row) => row.textContent.includes(`${mark} bob`)), MARK),
      { timeout: 30000, timeoutMsg: "bob's row is not in Alle Posteingänge" },
    );
    report.steps.unifiedLabel = await browser.execute(
      (mark) => [...document.querySelectorAll('[data-testid="message-row"]')].find((row) => row.textContent.includes(`${mark} bob`))?.querySelector('[data-testid="row-account"]')?.getAttribute("title"),
      MARK,
    );
    assert.equal(report.steps.unifiedLabel, "Arbeit");
    // The rail's account switcher and the settings follow.
    await setSidebar(false);
    report.steps.railLabel = await testId(`account-switch-${bob.id}`).getAttribute("aria-label");
    assert.match(report.steps.railLabel, /^Arbeit/);
    report.steps.railOrder = await browser.execute(() => [...document.querySelectorAll('[data-testid^="account-switch-"]')].map((b) => b.dataset.testid));
    assert.deepEqual(report.steps.railOrder, [`account-switch-${bob.id}`, `account-switch-${alice.id}`]);
    await setSidebar(true);
  });

  it("moves a mail from Alle Posteingänge into another account's folder; read-only refuses and says why", async () => {
    const bobInbox = (await coreCall("list_mailboxes", { accountId: bob.id })).find((m) => m.role === "inbox");
    const target = `[data-testid="mailbox-${String(bobInbox.id)}"]`;
    const row = () =>
      browser.execute((mark) => {
        const found = [...document.querySelectorAll('[data-testid="message-row"]')].find((r) => r.textContent.includes(`${mark} drop`));
        if (!found) return null;
        found.setAttribute("data-e2e-drag", "row");
        return true;
      }, MARK);
    await browser.waitUntil(async () => (await row()) === true, { timeout: 30000, timeoutMsg: "alice's row is not in Alle Posteingänge" });

    // Bob read-only: his inbox refuses, with the reason beside it and in the status region.
    await coreCall("set_writes_enabled", { accountId: bob.id, enabled: false });
    await browser.waitUntil(async () => !(await coreCall("list_accounts")).find((a) => a.id === bob.id).writesEnabled);
    await browser.pause(300);
    const started = await dragEvent('[data-e2e-drag="row"]', "dragstart");
    assert.deepEqual(started.types, ["application/x-mail-message-refs"]);
    await dragEvent(target, "dragenter");
    const refused = await dragEvent(target, "dragover");
    assert.equal(refused.prevented, false, "a read-only target takes nothing");
    assert.equal(await $(target).getAttribute("data-drop-target"), "refused");
    report.steps.refusedHint = await testId("drop-hint").getText();
    assert.equal(report.steps.refusedHint, "Nicht hierher: Für Arbeit sind Änderungen ausgeschaltet.");
    assert.equal(await browser.execute(() => document.querySelector('[data-testid="drop-hint-status"]')?.textContent), report.steps.refusedHint);
    await both("drop-refused");
    await dragEvent(target, "dragleave");
    await dragEvent('[data-e2e-drag="row"]', "dragend");

    // Writes on: the move goes through, no question.
    await coreCall("set_writes_enabled", { accountId: bob.id, enabled: true });
    await browser.waitUntil(async () => (await coreCall("list_accounts")).find((a) => a.id === bob.id).writesEnabled);
    await browser.pause(300);
    await row();
    await dragEvent('[data-e2e-drag="row"]', "dragstart");
    await dragEvent(target, "dragenter");
    const accepted = await dragEvent(target, "dragover");
    assert.equal(accepted.prevented, true);
    // WebKit keeps `dropEffect` of a page-made DataTransfer to itself; the hint says the mode.
    report.steps.moveHint = await testId("drop-hint").getText();
    assert.equal(report.steps.moveHint, "Nach Posteingang verschieben");
    await both("drop-move");
    // Option held: a copy (only looked at, not dropped).
    await dragEvent(target, "dragover", { altKey: true });
    report.steps.copyHint = await testId("drop-hint").getText();
    assert.equal(report.steps.copyHint, "Nach Posteingang kopieren");
    await dragEvent(target, "dragover");
    await dragEvent(target, "drop");
    await browser.waitUntil(async () => (await testId("notice").isExisting()) && (await testId("notice").getText()).includes("Posteingang"), {
      timeoutMsg: "no notice for the drop",
    });
    report.steps.notice = await testId("notice").getText();
    assert.match(report.steps.notice, /Wird nach Posteingang in Arbeit verschoben/);
    assert.equal(await $$('[role="alertdialog"]').length, 0, "no question before the move");

    // On the server: in bob's inbox, gone from alice's.
    await browser.waitUntil(
      async () => (await bobMessage(`${MARK} drop`)).found === true && (await folderCount("INBOX", `${MARK} drop`)).count === 0,
      { timeout: 60000, interval: 1000, timeoutMsg: "the mail did not move to bob's inbox on the server" },
    );
    report.steps.onServer = { bob: (await bobMessage(`${MARK} drop`)).count, alice: (await folderCount("INBOX", `${MARK} drop`)).count };
    assert.deepEqual(await cspViolations(), []);
  });

  it("offers the order and the list name in Settings → Konten", async () => {
    await openSettings("accounts");
    await testId(`account-up-${bob.id}`).waitForExist({ timeoutMsg: "no move buttons" });
    assert.equal(await testId(`account-up-${bob.id}`).isEnabled(), false, "the first account cannot move up");
    await both("settings-order");
    await testId(`account-row-${bob.id}`).click();
    await testId("account-list-name").waitForExist({ timeoutMsg: "no Name in der Liste" });
    assert.equal(await testId("account-list-name").getValue(), "Arbeit");
    await both("settings-name");
    writeState({ ...(readState() ?? {}), stage22Accounts: { order: [BOB.email, ALICE.email], names: { [bob.id]: "Arbeit", [alice.id]: "" } } });
  });
});
