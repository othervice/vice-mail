// Stage 18, task F (decision 0054) in the real app against the local test server:
// 1. Opening the inbox with the reading pane on the right shows its top message at once, without
//    the focus; with "Mark as read when opened: after 2 seconds" it stays unread, in the list and
//    on the server (IMAP \Seen), well after the delay.
// 2. Closing it leaves the list at its reading width and the pane's place says "No message
//    selected"; the list does not widen.
// 3. Selecting it with a click is the user's choice: then it is marked read after the delay (on
//    the server too). It is marked unread again at the end, so later specs see the seed as it was.
// Screenshots go to tests/e2e/screenshots/top-message-*.png, the facts to top-message-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openView, screenshot, SCREENSHOT_DIR, setConversations, testId } from "./support/app.mjs";
import { ALICE, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
let account;
let preferencesBefore = null;
let readingBefore = null;
let topMessage = null;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

/** The list, its top row, the pane and where the focus is. */
const state = () =>
  browser.execute(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, width: r.width, height: r.height };
    };
    const list = document.querySelector('[data-testid="message-list"]');
    // The first message row (a bundle row above it holds no message).
    const top = list?.querySelector('[data-testid="message-row"]');
    const pane = document.querySelector('[data-testid="reading-pane"]') ?? document.querySelector('[data-testid="thread-pane"]');
    return {
      list: box(list?.parentElement ?? null),
      topSelected: top?.getAttribute("aria-selected") ?? null,
      topName: top?.querySelector('[data-testid="row-name"]')?.textContent ?? "",
      paneSubject: pane?.querySelector("article h2")?.textContent ?? null,
      empty: document.querySelector('[data-testid="reading-empty"]')?.textContent ?? null,
      focusInPane: Boolean(pane && pane.contains(document.activeElement)),
      focus: document.activeElement?.closest("[data-testid]")?.getAttribute("data-testid") ?? document.activeElement?.tagName ?? null,
    };
  });

const inboxUnseen = async () => (await serverFolders()).INBOX.unseen;

describe("stage 18: opening a folder shows its top message, unread", () => {
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
    account = await coreCall("set_writes_enabled", { accountId: (await onlyAccount()).id, enabled: true });
    readingBefore = await coreCall("get_reading_preferences");
    await coreCall("set_reading_preferences", { preferences: { ...readingBefore, markReadOnOpen: "afterDelay" } });
    await setConversations(false);
    // The window reads the reading preferences when it starts (a change through the core sends
    // no event): it starts again, with "after 2 seconds".
    await browser.execute(() => {
      window.location.reload();
    });
    await browser.pause(2000);
    await attach();
  });

  after(async () => {
    // The top message unread again, as the seed has it.
    if (topMessage) {
      await coreCall("apply_action", {
        action: { type: "markUnread" },
        messages: [{ mailboxId: topMessage.mailboxId, messageId: topMessage.id }],
        requestId: `e2e-unread-${Date.now()}`,
      }).catch((e) => console.log(`unread again failed: ${e}`));
      await browser.waitUntil(async () => (await inboxUnseen()) === report.steps.unseenBefore, { timeout: 20000 }).catch(() => undefined);
    }
    if (readingBefore) await coreCall("set_reading_preferences", { preferences: readingBefore }).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "top-message-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("the inbox opens with its top message, which stays unread past the delay", async () => {
    // Away and back: the inbox opens anew.
    await openView("sent");
    report.steps.unseenBefore = await inboxUnseen();
    await openView("inbox");
    await testId("message-list").waitForExist({ timeout: 20000 });
    const inbox = (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "inbox");
    await browser.waitUntil(async () => (await state()).paneSubject !== null, { timeout: 15000, timeoutMsg: "no message shown when the inbox opened" });
    const shown = await state();
    report.steps.opened = shown;
    assert.equal(shown.topSelected, "true", "the top row is not the selected one");
    assert.ok(shown.topName.includes(shown.paneSubject), `the pane shows “${shown.paneSubject}”, the top row is “${shown.topName}”`);
    assert.equal(shown.focusInPane, false, "the focus moved into the pane");
    assert.ok(shown.topName.startsWith("Unread"), `the top message was not unread to begin with: ${shown.topName}`);
    report.screenshots.push(await screenshot("top-message-opened"));

    // Well past "after 2 seconds": still unread here and on the server.
    await browser.pause(3500);
    const later = await state();
    assert.ok(later.topName.startsWith("Unread"), `marked read in the list: ${later.topName}`);
    assert.equal(await inboxUnseen(), report.steps.unseenBefore, "marked read on the server");
    const listed = (await coreCall("list_messages", { mailboxId: inbox.id, cursor: null, limit: 5 })).items;
    topMessage = listed.find((m) => later.topName.includes(m.subject)) ?? null;
    report.steps.topMessage = topMessage && { id: topMessage.id, subject: topMessage.subject, unread: topMessage.unread };
    assert.ok(topMessage, "the top message is not among the inbox's newest");
    assert.equal(topMessage.unread, true, "the core has it read");
  });

  it("closing it keeps the list's width and says that no message is selected", async () => {
    const before = await state();
    await (await browser.$('[data-testid="reading-pane"] button[aria-label="Close message"]')).click();
    await testId("reading-empty").waitForExist({ timeout: 5000 });
    const closed = await state();
    report.steps.closed = closed;
    assert.ok(closed.empty?.includes("No message selected"), closed.empty);
    assert.ok(Math.abs(closed.list.width - before.list.width) <= 1, `the list went from ${String(before.list.width)} to ${String(closed.list.width)} px`);
    assert.ok(closed.list.width < 720, `the list spans ${String(closed.list.width)} px`);
    report.screenshots.push(await screenshot("top-message-none-selected"));
  });

  it("selected by the user it is marked read after the delay", async () => {
    await (await browser.$('[data-testid="message-list"] [data-testid="message-row"]')).click();
    await browser.waitUntil(async () => (await state()).paneSubject !== null, { timeout: 10000, timeoutMsg: "the click did not open it" });
    await browser.waitUntil(async () => (await inboxUnseen()) === report.steps.unseenBefore - 1, {
      timeout: 15000,
      interval: 500,
      timeoutMsg: "the chosen message was not marked read on the server",
    });
    const read = await state();
    assert.ok(!read.topName.startsWith("Unread"), `still unread in the list: ${read.topName}`);
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
