// Stage 16 in the real app: find in the open message (docs/stages/16, "Desktop-Tests für ... Suchen
// in der Mail"). Written for the desktop suite; add "./tests/e2e/find-in-message.e2e.mjs" to
// `specs` in wdio.conf.mjs after reading.e2e.mjs (it only appends two messages of its own to the
// inbox and reads them).
//
// 1. An HTML mail (two matches in the sandboxed frame, one in a hidden preheader) and a plain-text
//    mail are appended to the inbox through IMAP.
// 2. HTML mail: the focus in the reading pane, Cmd/Ctrl+F opens the find bar with the focus in its
//    field; typing finds the two visible matches ("1 of 2"), Enter moves on ("2 of 2"). The frame's
//    own highlight registry holds the current match (CSS Custom Highlight API in WebKit/WebView2),
//    its document got a style element and no script, the main frame reports no CSP violation.
// 3. Escape closes the bar, the message stays open, the frame's highlights are gone.
// 4. Plain-text mail: the same in the app's own document (subject and body).
// 5. Cmd/Ctrl+F with the focus in the message list opens the mail search, not the find bar.
//
// Screenshots go to tests/e2e/screenshots/find-*.png, the facts to find-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const HTML_SUBJECT = `Find HTML ${RUN}`;
const TEXT_SUBJECT = `Find text ${RUN}`;
const MOD = process.platform === "darwin" ? "Meta" : "Control";
let account;

function eml(subject, contentType, body) {
  return [
    "Date: Mon, 23 Nov 2026 10:00:00 +0000",
    "From: Bob Baker <bob@vicemail.test>",
    "To: Alice Archer <alice@vicemail.test>",
    `Subject: ${subject}`,
    `Message-ID: <${subject.replace(/\s+/g, ".")}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    `Content-Type: ${contentType}; charset=utf-8`,
    "",
    body,
    "",
  ].join("\r\n");
}

const HTML_BODY = [
  "<!DOCTYPE html><html><body>",
  '<div style="display:none">Quarterly preheader nobody sees</div>',
  "<p>The <b>Quarter</b>ly numbers are attached.</p>",
  "<p>Please check the quarterly forecast.</p>",
  "</body></html>",
].join("");
const TEXT_BODY = "Plain mail about the budget.\r\nThe budget meeting is on Friday.";

const active = () => browser.execute(() => document.activeElement?.closest("[data-testid]")?.getAttribute("data-testid") ?? null);
const count = () =>
  browser.execute(() => {
    const element = document.querySelector('[data-testid="find-count"]');
    return element ? { text: element.textContent, total: Number(element.getAttribute("data-total")) } : null;
  });

/** What the mail frame holds: highlights in its own registry, style and script elements. */
const frameState = () =>
  browser.execute(() => {
    const frame = document.querySelector('[data-testid="mail-frame"]');
    const doc = frame?.contentDocument;
    const registry = frame?.contentWindow?.CSS?.highlights;
    const size = (name) => {
      const highlight = registry?.get(name);
      return highlight ? highlight.size : 0;
    };
    return {
      api: Boolean(registry),
      current: size("vice-find-current"),
      others: size("vice-find"),
      currentText: (() => {
        const highlight = registry?.get("vice-find-current");
        const [range] = highlight ? [...highlight] : [];
        return range ? range.toString() : null;
      })(),
      styles: doc ? doc.querySelectorAll("style[data-vice-find]").length : -1,
      scripts: doc ? doc.querySelectorAll("script").length : -1,
    };
  });

async function openBySubject(subject) {
  const row = await browser.$(`//*[@role="option"][contains(., "${subject}")]`);
  await row.waitForExist({ timeout: 30000 });
  await row.click();
  await testId("message-card").waitForExist({ timeout: 20000 });
  await browser.waitUntil(async () => (await browser.execute(() => document.querySelector('[data-testid="reading-pane"]')?.textContent ?? "")).includes(subject), {
    timeoutMsg: `the reading pane does not show ${subject}`,
  });
  // The focus into the pane (not the frame: key events in the sandboxed frame never reach the app).
  await browser.execute(() => document.querySelector('[data-testid="reading-pane"] article')?.focus());
}

describe("stage 16: find in the open message", () => {
  before(async () => {
    await attach();
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
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vicemail-find-"));
    const html = path.join(dir, "find-html.eml");
    const text = path.join(dir, "find-text.eml");
    fs.writeFileSync(html, eml(HTML_SUBJECT, "text/html", HTML_BODY));
    fs.writeFileSync(text, eml(TEXT_SUBJECT, "text/plain", TEXT_BODY));
    await appendMessages("INBOX", [html, text]);
    await coreCall("retry_account", { accountId: account.id });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "find-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("finds in the HTML mail's frame without running anything in it", async () => {
    await openBySubject(HTML_SUBJECT);
    await browser.keys([MOD, "f"]);
    await testId("find-bar").waitForExist({ timeout: 5000 });
    assert.equal(await active(), "find-input", "the find field has the focus");
    await browser.keys([..."quarterly"]);
    await browser.waitUntil(async () => (await count())?.total === 2, { timeout: 5000, timeoutMsg: `count: ${JSON.stringify(await count())}` });
    assert.equal((await count()).text, "1 of 2");
    const first = await frameState();
    assert.ok(first.api, "the WebView offers the CSS Custom Highlight API");
    assert.equal(first.current, 1);
    assert.equal(first.others, 1);
    assert.equal(first.currentText?.toLowerCase(), "quarterly");
    assert.equal(first.styles, 1, "the highlight colours are in the frame");
    assert.equal(first.scripts, 0, "no script in the frame");
    report.screenshots.push(await screenshot("find-html"));
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await count())?.text === "2 of 2", { timeoutMsg: "Enter did not move to the next match" });
    report.steps.html = { first, second: await frameState() };
    assert.deepEqual(await cspViolations(), []);
  });

  it("Escape closes the bar, keeps the message open and removes the highlights", async () => {
    await browser.keys("Escape");
    await browser.waitUntil(async () => !(await testId("find-bar").isExisting()), { timeoutMsg: "the find bar is still open" });
    assert.ok(await testId("message-card").isExisting(), "the message is still open");
    const after = await frameState();
    assert.equal(after.current + after.others, 0);
    assert.equal(after.styles, 0);
    report.steps.closed = after;
  });

  it("finds in a plain-text mail in the app's own document", async () => {
    await openBySubject(TEXT_SUBJECT);
    await browser.keys([MOD, "f"]);
    await testId("find-bar").waitForExist({ timeout: 5000 });
    await browser.keys([..."budget"]);
    // The subject has no "budget"; the body has it twice.
    await browser.waitUntil(async () => (await count())?.total === 2, { timeout: 5000, timeoutMsg: `count: ${JSON.stringify(await count())}` });
    const highlighted = await browser.execute(() => {
      const registry = window.CSS?.highlights;
      return { current: registry?.get("vice-find-current")?.size ?? 0, others: registry?.get("vice-find")?.size ?? 0 };
    });
    assert.deepEqual(highlighted, { current: 1, others: 1 });
    report.screenshots.push(await screenshot("find-text"));
    await browser.keys("Escape");
  });

  it("Cmd/Ctrl+F in the message list stays the mail search", async () => {
    await browser.execute(() => document.querySelector('[data-testid="message-list"]')?.focus());
    await browser.keys([MOD, "f"]);
    await browser.waitUntil(async () => (await active()) === "search-input" || (await active())?.startsWith("search"), {
      timeout: 5000,
      timeoutMsg: `the search did not open: focus on ${await active()}`,
    });
    assert.ok(!(await testId("find-bar").isExisting()), "the find bar did not open");
    await browser.keys("Escape");
  });
});
