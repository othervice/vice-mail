// Stage 10 in the real app: "send later" against the local test server (not yet in `specs` of
// wdio.conf.mjs; written by the composer-complete UI session and NOT RUN yet).
//
// 1. A message planned a little more than a minute ahead shows in the outbox as "Planned" with
//    its time and the rule (sent only while the app runs and is online); nothing arrives before.
// 2. It arrives exactly once, close to the planned time.
// 3. A second planned message is moved with the outbox's "Send now": it goes out after the undo
//    delay, exactly once.
// 4. A third one is cancelled from the outbox: the composer opens it as a draft, nothing arrives.
//
// The "app was off" case (more and less than an hour) needs a restart with a moved clock; the
// core's tests cover it (decision 0023), the UI side is covered by tests/send-later.test.tsx.
//
// Screenshots go to tests/e2e/screenshots/send-later-*.png, the facts to send-later-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
let account;

const count = (folders, name) => folders[name]?.messages ?? 0;

async function plannedDraft(subject, sendAt) {
  const draft = await coreCall("create_draft", { request: { kind: "new", accountId: account.id, identityId: null, to: [{ name: null, email: ALICE.email }] } });
  await coreCall("save_draft", {
    draftId: draft.id,
    update: {
      identityId: draft.identityId,
      to: [{ name: null, email: ALICE.email }],
      cc: [],
      bcc: [],
      subject,
      format: "text",
      bodyHtml: "",
      bodyText: "Geplant.",
      includeQuote: false,
      quotePosition: "below",
      includeSignature: false,
      signatureId: null,
    },
  });
  return coreCall("schedule_send", { draftId: draft.id, requestId: crypto.randomUUID(), when: { sendAt } });
}

async function outboxRow(subject) {
  await testId("rail-outbox").click();
  await testId("outbox-view").waitForExist();
  return browser.waitUntil(
    async () =>
      browser.execute((wanted) => {
        const row = [...document.querySelectorAll('[data-testid="outgoing-row"]')].find((r) => r.textContent?.includes(wanted));
        return row ? { state: row.getAttribute("data-send-state"), text: row.textContent } : false;
      }, subject),
    { timeout: 20000, timeoutMsg: `no outbox row for ${subject}` },
  );
}

describe("stage 10: send later", () => {
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
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "send-later-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("a planned message waits in the outbox with its time and the rule, then arrives once on time", async () => {
    const subject = `Später ${RUN}`;
    const before = await serverFolders();
    const sendAt = Date.now() + 75_000;
    const mail = await plannedDraft(subject, sendAt);
    assert.equal(mail.scheduled, true);
    const row = await outboxRow(subject);
    assert.equal(row.state, "scheduled");
    assert.match(row.text, /Planned/);
    assert.ok(await testId("outbox-send-later-rule").isExisting(), "the outbox does not state the running-and-online rule");
    report.screenshots.push(await screenshot("send-later-outbox"));

    await browser.pause(30_000);
    assert.equal(count(await serverFolders(), "INBOX"), count(before, "INBOX"), "it arrived before the planned time");
    await browser.waitUntil(async () => count(await serverFolders(), "INBOX") >= count(before, "INBOX") + 1, {
      timeout: 120_000,
      interval: 1000,
      timeoutMsg: "the planned message did not arrive",
    });
    const arrivedAt = Date.now();
    await browser.pause(5000);
    assert.equal(count(await serverFolders(), "INBOX"), count(before, "INBOX") + 1, "it arrived more than once");
    report.steps.onTime = { sendAt, arrivedAt, lateBySeconds: Math.round((arrivedAt - sendAt) / 1000) };
  });

  it("“Send now” in the outbox sends a planned message after the undo delay", async () => {
    const subject = `Jetzt ${RUN}`;
    const before = await serverFolders();
    await plannedDraft(subject, Date.now() + 3_600_000);
    await outboxRow(subject);
    await browser.execute((wanted) => {
      const row = [...document.querySelectorAll('[data-testid="outgoing-row"]')].find((r) => r.textContent?.includes(wanted));
      row?.querySelector('[data-testid="outgoing-send-now"]')?.click();
    }, subject);
    await browser.waitUntil(async () => count(await serverFolders(), "INBOX") >= count(before, "INBOX") + 1, {
      timeout: 60_000,
      interval: 1000,
      timeoutMsg: "“Send now” did not send",
    });
    report.steps.sendNow = { inbox: count(await serverFolders(), "INBOX") };
  });

  it("cancelling a planned message opens it as a draft and sends nothing", async () => {
    const subject = `Abbruch ${RUN}`;
    const before = await serverFolders();
    await plannedDraft(subject, Date.now() + 3_600_000);
    await outboxRow(subject);
    await browser.execute((wanted) => {
      const row = [...document.querySelectorAll('[data-testid="outgoing-row"]')].find((r) => r.textContent?.includes(wanted));
      row?.querySelector('[data-testid="outgoing-cancel"]')?.click();
    }, subject);
    await testId("composer-subject").waitForExist({ timeout: 20000 });
    assert.equal(await testId("composer-subject").getValue(), subject);
    const scheduled = await coreCall("list_scheduled_sends");
    assert.ok(!scheduled.some((m) => m.subject === subject), "the cancelled send is still planned");
    await browser.pause(8000);
    assert.equal(count(await serverFolders(), "INBOX"), count(before, "INBOX"), "a cancelled send arrived");
    report.screenshots.push(await screenshot("send-later-cancelled"));
  });
});
