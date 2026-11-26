// Stage 23, item 2, step 2 (decision 0085) in the real app against the local test server, in
// German: the sidebar of the approved draft with two accounts.
// 1. "Alle Konten": Posteingang (all inboxes, its unread count semibold), Später, Markiert,
//    Gesendet, "Mehr" closed; each account: its heading (a square in its colour), Posteingang and
//    Gesendet with icons in its colour, "Weitere Ordner" closed. Rows 28 px, section heads 11 px,
//    rows 13 px, the selected row on `rail-selected` without a bar.
// 2. "Mehr" and "Weitere Ordner" open, the core keeps that on the device (`sidebarFolds`).
// 3. The foot: "Abgeglichen um HH:MM" from the accounts' last pass; "Jetzt abgleichen" makes every
//    account sync again (their `lastSyncAt` moves on, and the foot with it).
// 4. A closed "Weitere Ordner" opens while dragged mail rests on it.
// 5. Gesendet of "Alle Konten" and of an account are each current only for themselves.
// 6. The narrow rail: grouped, the accounts' unread counts under their dots.
// Sets up and removes alice and bob itself and puts the sidebar's parts back to the default.
// Screenshots go to tests/e2e/screenshots/s23sidebar-*.png, the facts to s23-sidebar-report.json.
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
let foldsBefore = [];
let viewsBefore = [];

async function addAccount(who, name) {
  const account = await coreCall("add_account", {
    settings: {
      email: who.email,
      displayName: name,
      imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: who.email },
      smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: who.email },
    },
    password: who.password,
  });
  await coreCall("rename_account", { accountId: account.id, name });
  return account;
}

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "de" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeout: 10000,
    timeoutMsg: `no ${theme} theme`,
  });
}

/** The one-time update question (decision 0037) may come up at any time: "No, thanks". */
async function dismissUpdateQuestion() {
  if (await testId("update-consent-no").isExisting()) {
    await testId("update-consent-no").click();
    await testId("update-consent").waitForExist({ reverse: true });
  }
}

async function both(name) {
  await dismissUpdateQuestion();
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    await browser.execute(() => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    });
    await browser.pause(200);
    report.screenshots.push(await screenshot(`s23sidebar-${name}-${theme}`));
  }
  await setTheme("light");
}

async function setFolds(folds) {
  const ui = await coreCall("get_ui_preferences");
  await coreCall("set_ui_preferences", { preferences: { ...ui, sidebarFolds: folds } });
  await browser.waitUntil(async () => JSON.stringify((await coreCall("get_ui_preferences")).sidebarFolds) === JSON.stringify(folds), {
    timeout: 5000,
    timeoutMsg: "the sidebar's parts were not stored",
  });
  await browser.pause(300);
}

/** The rows of a tree of the column as a screen reader names them. */
const treeRows = (name) =>
  browser.execute((label) => {
    const tree = [...document.querySelectorAll('[data-testid="sidebar-column"] [role="tree"]')].find(
      (t) => (t.getAttribute("aria-label") ?? document.getElementById(t.getAttribute("aria-labelledby") ?? "")?.textContent) === label,
    );
    return tree ? [...tree.querySelectorAll('[role="treeitem"]')].map((row) => row.getAttribute("aria-label")) : null;
  }, name);

/** Size, font and fill of a row, and whether it draws a bar before it. */
const look = (selector) =>
  browser.execute((sel) => {
    const element = document.querySelector(sel);
    if (!element) return null;
    // A window the test drives gets no animation frames: colour transitions would stay at their start.
    for (const animation of document.getAnimations()) animation.finish();
    const style = getComputedStyle(element);
    const before = getComputedStyle(element, "::before");
    return {
      height: Math.round(element.getBoundingClientRect().height),
      fontSize: style.fontSize,
      fontWeight: style.fontWeight,
      background: style.backgroundColor,
      bar: before.content !== "none" && before.content !== "normal" && Number.parseFloat(before.width) > 0,
    };
  }, selector);

/** A token's colour as the browser computes it (`rgb(…)`). */
const tokenColour = (token) =>
  browser.execute((name) => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = `var(--ui-${name})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return value;
  }, token);

const footText = () => browser.execute(() => document.querySelector('[data-testid="sidebar-sync-status"]')?.textContent ?? null);

describe("stage 23: the sidebar of the approved draft", () => {
  before(async () => {
    await attach();
    await dismissUpdateQuestion();
    await browser.setWindowSize(1280, 800);
    await setTheme("light");
    // The default views (an earlier spec may have left a choice of its own), put back at the end.
    const preferences = await coreCall("get_ui_preferences");
    foldsBefore = preferences.sidebarFolds;
    viewsBefore = preferences.columnViews;
    await coreCall("set_ui_preferences", { preferences: { ...preferences, columnViews: [] } });
    alice = await addAccount(ALICE, "Arbeit");
    bob = await addAccount(BOB, "Privat");
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first syncs did not finish" },
    );
    await setFolds([]);
    await setSidebar(true);
    await testId("unified-all").waitForExist({ timeout: 20000 });
    await testId("unified-all").click();
    await testId("message-list").waitForExist({ timeout: 20000 });
  });

  after(async () => {
    await setFolds(foldsBefore).catch(() => undefined);
    const preferences = await coreCall("get_ui_preferences").catch(() => null);
    if (preferences) await coreCall("set_ui_preferences", { preferences: { ...preferences, columnViews: viewsBefore } }).catch(() => undefined);
    for (const account of [alice, bob]) if (account) await coreCall("remove_account", { accountId: account.id }).catch(() => undefined);
    await coreCall("set_preferences", { preferences: { theme: "system", language: "system" } }).catch(() => undefined);
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s23-sidebar-report.json"), JSON.stringify(report, null, 2));
  });

  it("shows Alle Konten, the favourites and one section per account, closed parts closed, in the draft's sizes", async () => {
    const all = await treeRows("Alle Konten");
    report.steps.allAccounts = all;
    assert.equal(all?.[0], "Alle Konten");
    assert.match(all?.[1] ?? "", /^Posteingang(, [\d.]+ ungelesen)?$/);
    assert.deepEqual(all?.slice(2), ["Später", "Markiert", "Gesendet", "Mehr"]);
    assert.equal(await testId("sidebar-more").getAttribute("aria-expanded"), "false");
    const arbeit = await treeRows("Ordner von Arbeit");
    report.steps.arbeit = arbeit;
    assert.match(arbeit?.[0] ?? "", /^Arbeit/);
    assert.match(arbeit?.[1] ?? "", /^Posteingang/);
    assert.equal(arbeit?.[2], "Gesendet");
    assert.match(arbeit?.[3] ?? "", /^Weitere Ordner/);
    assert.equal(arbeit?.length, 4, "More folders is closed");
    // Sizes of the draft: rows 28 px with 13 px text, heads 24 px with 11 px text.
    const inbox = await look('[data-testid="unified-all"]');
    const head = await look(`[data-testid="sidebar-account-${alice.id}"]`);
    report.steps.look = { inbox, head };
    assert.equal(inbox?.height, 28);
    assert.equal(inbox?.fontSize, "13px");
    assert.equal(head?.height, 24);
    assert.equal(head?.fontSize, "11px");
    // The row shown: the column's selected fill, semibold, no bar at its start.
    assert.equal(inbox?.background, await tokenColour("rail-selected"));
    assert.equal(inbox?.fontWeight, "600");
    assert.equal(inbox?.bar, false);
    // The account's icons in its colour, its heading with the square.
    report.steps.iconColour = await browser.execute((id) => {
      const icon = document.querySelector(`[data-testid="sidebar-account-section-${id}"] [data-folder-role="inbox"] svg`);
      return icon ? getComputedStyle(icon).color : null;
    }, alice.id);
    assert.ok(report.steps.iconColour, "no inbox icon in Arbeit's section");
    await both("closed");
  });

  it("opens Mehr and Weitere Ordner; the core keeps both on this device", async () => {
    await testId("sidebar-more").click();
    await testId(`sidebar-more-folders-${alice.id}`).click();
    await browser.waitUntil(
      async () => {
        const folds = (await coreCall("get_ui_preferences")).sidebarFolds;
        return folds.some((f) => f.key === "more" && f.open) && folds.some((f) => f.key === `folders:${alice.id}` && f.open);
      },
      { timeout: 5000, timeoutMsg: "the core did not keep More and More folders open" },
    );
    const all = await treeRows("Alle Konten");
    for (const name of ["Warten auf Antwort", "Aufgaben", "Kontakte", "Aktivität", "Ausgang"]) assert.ok(all?.includes(name), `${name} is not under Mehr`);
    const arbeit = await treeRows("Ordner von Arbeit");
    report.steps.arbeitOpen = arbeit;
    assert.ok((arbeit?.length ?? 0) > 4, "More folders shows no folder");
    await both("open");
    await setFolds([]);
  });

  it("says when the accounts last synced; Jetzt abgleichen makes them sync again", async () => {
    const accounts = await coreCall("list_accounts");
    const last = Math.min(...accounts.map((a) => a.status.lastSyncAt));
    const time = new Intl.DateTimeFormat("de", { timeStyle: "short" }).format(last);
    await browser.waitUntil(async () => (await footText()) === `Abgeglichen um ${time}`, { timeout: 10000, timeoutMsg: `the foot does not say ${time}` });
    report.steps.foot = await footText();
    await browser.pause(1100);
    await testId("sidebar-sync-now").click();
    await browser.waitUntil(
      async () => (await coreCall("list_accounts")).every((a) => a.status.state === "idle" && a.status.lastSyncAt > last),
      { timeout: 60000, interval: 300, timeoutMsg: "the accounts did not sync again" },
    );
    const after = await coreCall("list_accounts");
    const again = new Intl.DateTimeFormat("de", { timeStyle: "short" }).format(Math.min(...after.map((a) => a.status.lastSyncAt)));
    await browser.waitUntil(async () => (await footText()) === `Abgeglichen um ${again}`, { timeout: 10000, timeoutMsg: "the foot did not follow" });
    report.steps.footAfter = await footText();
  });

  it("opens a closed Weitere Ordner while dragged mail rests on it", async () => {
    await browser.waitUntil(async () => (await $('[data-testid="message-row"]').isExisting()), { timeout: 20000, timeoutMsg: "no row" });
    const more = `[data-testid="sidebar-more-folders-${alice.id}"]`;
    assert.equal(await $(more).getAttribute("aria-expanded"), "false");
    await browser.execute(() => {
      const row = document.querySelector('[data-testid="message-row"]');
      window.__e2eDrag = new DataTransfer();
      row.dispatchEvent(new DragEvent("dragstart", { bubbles: true, cancelable: true, dataTransfer: window.__e2eDrag }));
    });
    const started = Date.now();
    while (Date.now() - started < 1200) {
      await browser.execute((sel) => {
        const element = document.querySelector(sel);
        const box = element.getBoundingClientRect();
        element.dispatchEvent(
          new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: window.__e2eDrag, clientX: box.left + 20, clientY: box.top + box.height / 2 }),
        );
      }, more);
      await browser.pause(100);
    }
    report.steps.dragOpened = await $(more).getAttribute("aria-expanded");
    assert.equal(report.steps.dragOpened, "true");
    await browser.execute(() => {
      document.querySelector('[data-testid="message-row"]')?.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: window.__e2eDrag }));
    });
    await setFolds([]);
  });

  it("Gesendet of Alle Konten and of an account are each current only for themselves", async () => {
    await testId("sidebar-view-sent").click();
    await browser.waitUntil(async () => (await testId("sidebar-view-sent").getAttribute("aria-current")) === "page", { timeoutMsg: "Gesendet did not open" });
    // Decision 0087: Gesendet of Alle Konten is every account's Sent, as one mixed list.
    await testId("all-accounts-sent").waitForExist({ timeout: 20000 });
    assert.equal(await testId("view-scope").getText(), "Alle Konten");
    const sent = (await coreCall("list_mailboxes", { accountId: bob.id })).find((m) => m.role === "sent");
    assert.ok(sent, "bob has no Sent folder");
    assert.equal(await testId(`mailbox-${String(sent.id)}`).getAttribute("aria-current"), null);
    await testId(`mailbox-${String(sent.id)}`).click();
    await browser.waitUntil(async () => (await testId(`mailbox-${String(sent.id)}`).getAttribute("aria-current")) === "page", { timeoutMsg: "bob's Gesendet did not open" });
    assert.equal(await testId("sidebar-view-sent").getAttribute("aria-current"), null);
    await testId("unified-all").click();
  });

  it("the narrow rail is grouped like the column; the accounts' unread mail under their dots", async () => {
    await setSidebar(false);
    report.steps.railMain = await browser.execute(() => [...document.querySelectorAll('[data-testid="icon-rail-views-main"] button')].map((b) => b.dataset.testid));
    assert.deepEqual(report.steps.railMain, ["rail-inbox", "rail-later", "rail-flagged", "rail-sent"]);
    const inbox = (await coreCall("list_mailboxes", { accountId: alice.id })).find((m) => m.role === "inbox");
    report.steps.railLabel = await testId(`account-switch-${alice.id}`).getAttribute("aria-label");
    assert.match(report.steps.railLabel, /^Arbeit \(alice@vicemail\.test\), /);
    if (inbox.unread > 0) assert.match(report.steps.railLabel, / ungelesen$/);
    await both("rail");
    await setSidebar(true);
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
