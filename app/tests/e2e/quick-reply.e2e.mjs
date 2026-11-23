// Stage 16 in the real app: the inline quick reply under the open message, against the local test
// server (in `specs` of wdio.conf.mjs after send-later.e2e.mjs). Written in the stage 16 UI session,
// not yet run there: the main session runs the desktop suite.
//
// 1. Bob sends alice a message; it opens in the reading pane with the quick reply below it. No draft
//    exists before the first key; typing creates exactly one reply draft.
// 2. Escape leaves the field for the message (the reading pane stays open).
// 3. Cmd/Ctrl+Enter sends through the outbox with the undo countdown; the copy lands in alice's Sent
//    folder once, and the field is empty again.
// 4. "Expand" hands a second reply with its text to the full composer (same draft, no second one).
//
// Screenshots go to tests/e2e/screenshots/quick-reply-*.png, the facts to quick-reply-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, deliverMail, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const SUBJECT = `Schnellantwort ${RUN}`;
const MOD = process.platform === "darwin" ? "Meta" : "Control";
let account;

const count = (folders, name) => folders[name]?.messages ?? 0;
const sendState = () =>
  browser.execute(() => document.querySelector('[data-testid="send-notice"]')?.getAttribute("data-send-state") ?? null);
const replyDrafts = async () =>
  (await coreCall("list_drafts", { accountId: account.id })).filter((d) => d.subject.includes(SUBJECT));

/** Clicks the list row whose text names the subject (the newest message is at the top). */
async function openRow(subject) {
  await browser.waitUntil(
    () =>
      browser.execute((text) => {
        const row = [...document.querySelectorAll('[data-testid="message-list"] [role="option"]')].find((o) =>
          (o.textContent ?? "").includes(text),
        );
        row?.click();
        return Boolean(row);
      }, subject),
    { timeout: 60000, interval: 500, timeoutMsg: `no row for ${subject}` },
  );
  await testId("message-card").waitForExist({ timeout: 20000 });
}

describe("stage 16: inline quick reply", () => {
  before(async () => {
    await attach();
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
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    await coreCall("set_compose_settings", { settings: { sendDelaySeconds: 5 } });
    await deliverMail(SUBJECT);
    await testId("rail-inbox").click();
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "quick-reply-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("creates no draft before the first key and one reply draft after it", async () => {
    await openRow(SUBJECT);
    const field = await testId("inline-reply-field");
    await field.waitForExist({ timeout: 20000 });
    await field.click();
    assert.equal((await replyDrafts()).length, 0, "a draft exists before typing");
    await field.setValue("Danke, Bob! Umlaute: äöü ß.");
    await browser.waitUntil(async () => (await replyDrafts()).length === 1, { timeout: 15000, timeoutMsg: "no reply draft" });
    report.screenshots.push(await screenshot("quick-reply-typed"));
    report.steps.draft = (await replyDrafts()).map((d) => ({ id: d.id, kind: d.kind }));
  });

  it("Escape leaves the field for the message", async () => {
    await testId("inline-reply-field").click();
    await browser.keys(["Escape"]);
    const active = await browser.execute(() => document.activeElement?.tagName ?? null);
    assert.equal(active, "ARTICLE", "the focus did not go back to the message");
    assert.ok(await testId("reading-pane").isExisting(), "Escape closed the reading pane");
  });

  it("Cmd/Ctrl+Enter sends with the undo countdown; the copy is in Sent once", async () => {
    const before = await serverFolders();
    await testId("inline-reply-field").click();
    await browser.keys([MOD, "Enter"]);
    await browser.waitUntil(async () => (await sendState()) === "scheduled", { timeoutMsg: "no undo countdown" });
    report.screenshots.push(await screenshot("quick-reply-countdown"));
    await browser.waitUntil(async () => (await sendState()) === "sent", { timeout: 60000, timeoutMsg: "never sent" });
    let after;
    await browser.waitUntil(
      async () => {
        after = await serverFolders();
        return count(after, "Sent") >= count(before, "Sent") + 1;
      },
      { timeout: 60000, interval: 500, timeoutMsg: () => `no copy in Sent: ${JSON.stringify(after)}` },
    );
    await browser.pause(3000);
    const settled = await serverFolders();
    assert.equal(count(settled, "Sent"), count(before, "Sent") + 1, "the reply was filed more than once");
    assert.equal(await testId("inline-reply-field").getValue(), "", "the field kept the sent text");
    report.steps.sent = { sentBefore: count(before, "Sent"), sentAfter: count(settled, "Sent") };
  });

  it("Expand hands the reply with its text to the full composer", async () => {
    const field = await testId("inline-reply-field");
    await field.click();
    await field.setValue("Längere Antwort");
    await browser.waitUntil(async () => (await replyDrafts()).length === 1, { timeout: 15000, timeoutMsg: "no reply draft" });
    await testId("inline-reply-expand").click();
    await testId("composer-subject").waitForExist({ timeout: 20000 });
    const body = await testId("composer-text-body");
    assert.equal(await body.getValue(), "Längere Antwort", "the composer lost the text");
    assert.equal((await replyDrafts()).length, 1, "Expand made a second draft");
    report.screenshots.push(await screenshot("quick-reply-expanded"));
    await testId("composer-close").click();
    await testId("composer").waitForExist({ reverse: true });
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
