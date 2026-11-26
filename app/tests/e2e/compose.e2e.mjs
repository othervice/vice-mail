// Stage 5 in the real app: writing, undo, sending and drafts against the local test server
// (in `specs` of wdio.conf.mjs after actions.e2e.mjs).
//
// 1. A new message to alice herself (umlauts and emoji in the subject): "Send" shows the undo
//    countdown; "Undo" brings the draft back and nothing reaches the server.
// 2. Sent again, it arrives exactly once in the inbox, and the notice says "Message sent".
// 3. A draft closed without sending is in the Drafts folder on the server, once; edited again
//    from there and closed, it is still there once (the newest version only).
// 4. The outbox view lists the sent message; the rich text editor runs without CSP violations
//    (TipTap's injected stylesheet is off).
//
// Screenshots go to tests/e2e/screenshots/compose-*.png, the facts to compose-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openStatusView, openView, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const SUBJECT = `Grüße 🧾 ${RUN}`;
let account;

const count = (folders, name) => folders[name]?.messages ?? 0;

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

async function openComposer() {
  await $('button[aria-label="New message"]').click();
  await testId("composer-subject").waitForExist({ timeout: 20000 });
}

async function fill({ to, subject, body }) {
  if (to) {
    const input = await $('[data-testid="composer-to"] [data-testid="recipient-input"]');
    await input.setValue(`${to},`);
  }
  await testId("composer-subject").setValue(subject);
  const text = await $('[data-testid="composer-text-body"]');
  if (await text.isExisting()) await text.setValue(body);
}

const sendState = () =>
  browser.execute(() => document.querySelector('[data-testid="send-notice"]')?.getAttribute("data-send-state") ?? null);

describe("stage 5: writing and sending", () => {
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
    assert.deepEqual(await cspViolations(), [], "CSP violations before composing");
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "compose-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("Undo within the delay sends nothing and brings the draft back", async () => {
    const before = await serverFolders();
    await openComposer();
    await fill({ to: ALICE.email, subject: SUBJECT, body: "Hallo Alice, Umlaute: äöü ß." });
    report.screenshots.push(await screenshot("compose-new"));
    await testId("composer-send").click();
    await browser.waitUntil(async () => (await sendState()) === "scheduled", { timeoutMsg: "no undo countdown" });
    await testId("send-undo").click();
    await testId("composer-subject").waitForExist();
    assert.equal(await testId("composer-subject").getValue(), SUBJECT);
    // Longer than the delay: nothing may arrive.
    await browser.pause(8000);
    const after = await serverFolders();
    assert.equal(count(after, "INBOX"), count(before, "INBOX"), "a cancelled send arrived");
    report.steps.undo = { before: count(before, "INBOX"), after: count(after, "INBOX") };
  });

  it("a sent message arrives exactly once and the notice says so", async () => {
    const before = await serverFolders();
    await testId("composer-send").click();
    await browser.waitUntil(async () => (await sendState()) === "sent", { timeout: 60000, timeoutMsg: "never sent" });
    const after = await waitForServer((f) => count(f, "INBOX") >= count(before, "INBOX") + 1, "did not arrive");
    await browser.pause(3000);
    const settled = await serverFolders();
    assert.equal(count(settled, "INBOX"), count(before, "INBOX") + 1, "the message arrived more than once");
    report.steps.sent = { inbox: count(after, "INBOX"), sent: count(settled, "Sent") };
    report.screenshots.push(await screenshot("compose-sent"));
  });

  it("a closed draft is on the server once, also after editing it again", async () => {
    const before = await serverFolders();
    await openComposer();
    await fill({ to: "", subject: `Entwurf ${RUN}`, body: "Erste Fassung" });
    await testId("composer-close").click();
    await testId("composer").waitForExist({ reverse: true });
    const drafts = await coreCall("list_drafts", { accountId: account.id });
    assert.ok(drafts.some((d) => d.subject === `Entwurf ${RUN}`), "the draft is not kept locally");
    await waitForServer((f) => count(f, "Drafts") === count(before, "Drafts") + 1, "the draft is not on the server");

    // Open it again from the Drafts folder and change it.
    await openView("drafts");
    await testId("message-list").waitForExist({ timeout: 20000 });
    // A double-click opens a draft in the composer (MessageList's onDoubleClick, like Enter).
    // The embedded WebDriver's pointer actions fire two clicks but no `dblclick` (its
    // pointer_input.rs), so the test clicks the row and dispatches the `dblclick` WebKit would.
    const row = await $('[data-testid="message-list"] [role="option"]');
    await row.click();
    await browser.execute((el) => {
      el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, detail: 2, view: window }));
    }, row);
    await testId("composer-subject").waitForExist();
    assert.equal(await testId("composer-subject").getValue(), `Entwurf ${RUN}`, "the composer shows another draft");
    await testId("composer-subject").setValue(`Entwurf ${RUN} v2`);
    await testId("composer-close").click();
    await waitForServer(
      (f) => count(f, "Drafts") === count(before, "Drafts") + 1,
      "more than the newest version in Drafts",
    );
    report.steps.drafts = { drafts: count(await serverFolders(), "Drafts") };
  });

  it("the outbox lists the send, the rich text editor causes no CSP violation", async () => {
    // Stage 23 (decision 0085): from the rail, or under the column's "More".
    await openStatusView("outbox");
    const states = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="outgoing-row"]')].map((r) => r.getAttribute("data-send-state")),
    );
    assert.ok(states.includes("sent"), JSON.stringify(states));
    report.screenshots.push(await screenshot("compose-outbox"));

    // "New message" is in the header of the mail views, not of the outbox.
    await openView("inbox");
    await openComposer();
    await testId("composer-format-html").click();
    await testId("composer-rich-body").click();
    await browser.keys(["F", "e", "t", "t"]);
    await testId("format-bold").click();
    report.screenshots.push(await screenshot("compose-rich"));
    assert.deepEqual(await cspViolations(), [], "CSP violations in the rich text editor");
    await testId("composer-discard").click();
    await testId("composer-confirm-discard").$("button=Discard").click();
  });
});
