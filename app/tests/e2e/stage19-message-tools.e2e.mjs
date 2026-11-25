// Stage 19, part B (decision 0055) in the real app against the local test server.
//
// 1. A message appended to alice's INBOX opens; Cmd+U opens its source in a window of its own
//    (`?window=source`): the raw message as text (markup in it stays text), the window has no
//    event permission (it never listens), Escape closes it.
// 2. "Show all headers" lists every field under the message head.
// 3. "Redirect" to bob: the dialog, the undo countdown, "Message redirected"; bob's server stores
//    the message with Resent-From alice and the original's body.
// 4. "Edit as new" opens the composer with the subject and recipients of the message.
// 5. No CSP violation in the main frame.
//
// Screenshots: tests/e2e/screenshots/s19-source-window.png, s19-redirect-dialog.png; facts in
// s19-message-tools-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, bobMessage } from "./support/testmail.mjs";

const MOD = process.platform === "darwin" ? "Meta" : "Control";
const RUN = Date.now().toString(36);
const SUBJECT = `S19 tools ${RUN}`;
const report = { screenshots: [], steps: {} };
let main;

function eml() {
  return [
    `Date: ${new Date().toUTCString().replace("GMT", "+0000")}`,
    "From: Grace Hopper <grace@example.org>",
    "To: Alice Archer <alice@vicemail.test>",
    `Subject: ${SUBJECT}`,
    `Message-ID: <s19-${RUN}@e2e.vicemail.test>`,
    "X-S19-Marker: shown in all headers",
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    "Hello Alice, <b>this is not markup</b> and <script>alert(1)</script> neither.",
    "",
  ].join("\r\n");
}

/** Switches to the window whose page has `window=<kind>`; returns its handle (or null). */
async function switchToWindowOf(kind, timeout = 10000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      const search = await browser.execute(() => window.location.search);
      if (new URLSearchParams(search).get("window") === kind) return handle;
    }
    await browser.pause(250);
  }
  return null;
}

async function openMessage() {
  const row = await browser.$(`//*[@role="option" or @role="treeitem"][contains(., "${SUBJECT}")]`);
  await row.waitForExist({ timeout: 60000 });
  await row.click();
  await testId("message-card").waitForExist({ timeout: 20000 });
  await browser.execute(() => document.querySelector('[data-testid="reading-pane"] article')?.focus());
}

async function cardMenu(item) {
  await $('button[aria-label="More for this message"]').click();
  const entry = await testId(`card-menu-${item}`);
  await entry.waitForExist({ timeout: 5000 });
  await entry.click();
}

/** Answers the one-time "Check for updates automatically?" card so it covers no screenshot. */
async function dismissUpdateQuestion() {
  const no = await $("button=No, thanks");
  if (await no.isExisting()) await no.click();
}

const sendState = () =>
  browser.execute(() => document.querySelector('[data-testid="send-notice"]')?.getAttribute("data-send-state") ?? null);

describe("stage 19: source, all headers, redirect, edit as new", () => {
  before(async () => {
    await attach();
    main = await browser.getWindowHandle();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    const account = await coreCall("add_account", {
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
    await coreCall("set_compose_settings", { settings: { sendDelaySeconds: 4 } });
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    await dismissUpdateQuestion();
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "s19-tools-"));
    const file = path.join(dir, "tools.eml");
    fs.writeFileSync(file, eml());
    await appendMessages("INBOX", [file]);
    await coreCall("retry_account", { accountId: account.id });
  });

  after(async () => {
    for (const handle of await browser.getWindowHandles().catch(() => [])) {
      if (handle === main) continue;
      await browser.switchToWindow(handle).catch(() => undefined);
      await browser.closeWindow().catch(() => undefined);
    }
    await browser.switchToWindow(main).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s19-message-tools-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  afterEach(async () => {
    await browser.switchToWindow(main);
  });

  it("opens the source in a window of its own with Cmd+U, as text only", async () => {
    await openMessage();
    const before = (await browser.getWindowHandles()).length;
    await browser.keys([MOD, "u"]);
    const handle = await switchToWindowOf("source");
    assert.ok(handle, "a source window opened");
    const text = await testId("source-text");
    await text.waitForExist({ timeout: 20000 });
    const state = await browser.execute(() => {
      const pre = document.querySelector('[data-testid="source-text"]');
      return {
        text: pre?.textContent ?? "",
        elements: pre ? pre.querySelectorAll("*").length : -1,
        scripts: document.querySelectorAll("body script").length,
        bold: document.querySelectorAll("body b").length,
      };
    });
    assert.ok(state.text.includes(`Subject: ${SUBJECT}`), state.text.slice(0, 400));
    assert.ok(state.text.includes("<b>this is not markup</b>"), "markup stays text");
    assert.equal(state.elements, 0, "one text node");
    assert.equal(state.bold, 0);
    report.steps.source = { length: state.text.length, windows: before + 1 };
    report.screenshots.push(await screenshot("s19-source-window"));
    assert.deepEqual(await cspViolations(), [], "CSP violations in the source window");
    // The key closes the window it is sent to, so the driver may report the window as gone.
    await browser.keys(["Escape"]).catch((e) => {
      if (!/no such window|No window could be found|Channel closed/i.test(String(e))) throw e;
    });
    await browser.switchToWindow(main);
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === before, {
      timeout: 10000,
      timeoutMsg: "Escape did not close the source window",
    });
  });

  it("shows all headers under the head of the message", async () => {
    await openMessage();
    await cardMenu("allHeaders");
    const list = await testId("all-headers-list");
    await list.waitForExist({ timeout: 20000 });
    const text = await list.getText();
    assert.match(text, /X-S19-Marker/);
    assert.match(text, /shown in all headers/);
    report.steps.allHeaders = await browser.execute(() => document.querySelectorAll('[data-testid="all-headers-list"] dt').length);
  });

  it("redirects the message to bob with Resent fields after the undo delay", async () => {
    await openMessage();
    await cardMenu("redirect");
    const dialog = await testId("redirect-dialog");
    await dialog.waitForExist({ timeout: 5000 });
    const to = await $('[data-testid="redirect-to"] [data-testid="recipient-input"]');
    await to.setValue("bob@vicemail.test,");
    report.screenshots.push(await screenshot("s19-redirect-dialog"));
    await testId("redirect-submit").click();
    await browser.waitUntil(async () => (await sendState()) === "scheduled", { timeoutMsg: "no undo countdown" });
    await browser.waitUntil(async () => (await sendState()) === "sent", { timeout: 60000, timeoutMsg: "never redirected" });
    const notice = await testId("send-notice").getText();
    assert.match(notice, /Message redirected/);
    let arrived = { found: false };
    await browser.waitUntil(
      async () => {
        arrived = await bobMessage(SUBJECT);
        return arrived.found;
      },
      { timeout: 60000, interval: 1000, timeoutMsg: "the redirect did not arrive at bob" },
    );
    assert.match(arrived.resentFrom, /alice@vicemail\.test/);
    assert.match(arrived.resentTo, /bob@vicemail\.test/);
    const raw = Buffer.from(arrived.raw, "base64").toString("utf8");
    assert.ok(raw.includes("<b>this is not markup</b>"), "the original body arrived unchanged");
    assert.ok(raw.includes(`Message-ID: <s19-${RUN}@e2e.vicemail.test>`), "the original Message-ID stays");
    report.steps.redirect = { resentFrom: arrived.resentFrom, resentTo: arrived.resentTo, count: arrived.count };
  });

  it("edits the message as a new one", async () => {
    await openMessage();
    await cardMenu("editAsNew");
    const subject = await testId("composer-subject");
    await subject.waitForExist({ timeout: 20000 });
    await browser.waitUntil(async () => (await subject.getValue()) === SUBJECT, { timeoutMsg: "the subject was not taken over" });
    const chips = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="composer-to"] [data-testid="recipient-chip"]')].map((c) => c.textContent),
    );
    assert.ok(chips.some((c) => c.includes("Alice")), JSON.stringify(chips));
    report.steps.editAsNew = { subject: await subject.getValue(), to: chips };
    await testId("composer-discard").click();
    await testId("composer-confirm-discard").waitForExist();
    await testId("composer-confirm-discard").$("button=Discard").click();
    await testId("composer").waitForExist({ reverse: true });
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
