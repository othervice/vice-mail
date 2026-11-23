// Stage 8 in the real app: axe-core in the desktop tests (docs/stages/08, "Automatische Prüfung":
// axe finds nothing serious or critical in the main views, light and dark).
//
// Vitest runs axe in jsdom (tests/a11y.test.tsx), where nothing is laid out and no stylesheet
// applies, so `color-contrast` and every rule that needs rendered boxes is off there. Here axe-core
// (the dev dependency, the same version) is injected into the real web view with all rules on,
// including contrast on the rendered colours. The body of an HTML mail is the sender's content in
// a sandboxed frame, not the app's UI, so axe does not descend into frames (`iframes: false`).
//
// Views, each in the light and in the dark theme: the inbox, the reading pane with the newest
// message, the composer (a new, empty message) and its question before discarding it, every
// settings section and the command palette. Only reads mail; it adds the test account and removes it at the end.
// In `specs` of wdio.conf.mjs before layouts.e2e.mjs (it can run anywhere after account-sync.e2e.mjs).
//
// The findings (all impacts) go to tests/e2e/screenshots/a11y-report.json, a screenshot of each
// view to a11y-<theme>-<view>.png.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, openSettings, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const require = createRequire(import.meta.url);
const AXE_SOURCE = fs.readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const MOD = process.platform === "darwin" ? "Meta" : "Control";
const THEMES = ["light", "dark"];
// The ids of src/stores/ui.ts (SETTINGS_SECTION_IDS), which this plain .mjs file cannot import.
const SETTINGS_SECTIONS = [
  "general",
  "accounts",
  "reading",
  "compose",
  "contacts",
  "inbox",
  "notifications",
  "appearance",
  "keyboard",
  "sync",
  "ai",
  "mcp",
  "about",
];

const report = { axeVersion: null, views: {} };
let account;

/** Puts axe-core into the page unless it is there already (the app never reloads its page). */
async function injectAxe() {
  const present = await browser.execute(() => typeof window.axe?.run === "function");
  if (!present) {
    // The source runs as the WebDriver script itself (like every `browser.execute`), not through
    // `eval` in the page, so the app's CSP (script-src 'self', no 'unsafe-eval') does not block
    // loading it. axe sets `window.axe` itself.
    await browser.execute(`${AXE_SOURCE}\n;return typeof window.axe;`);
  }
  report.axeVersion = await browser.execute(() => window.axe.version);
}

/** axe over the whole document; every violation with its impact and up to ten nodes. */
async function runAxe() {
  await injectAxe();
  const result = await browser.execute(async () => {
    try {
      // A hidden test window runs no animation frames, so CSS transitions (theme colours,
      // hover) stay on their first frame and axe would read the colours of before. Finish them,
      // as the screenshots do (support/app.mjs).
      for (const animation of document.getAnimations()) animation.finish();
      const results = await window.axe.run(document, { resultTypes: ["violations"], iframes: false });
      return {
        violations: results.violations.map((violation) => ({
          id: violation.id,
          impact: violation.impact,
          help: violation.help,
          nodes: violation.nodes
            .slice(0, 10)
            .map((node) => ({ target: node.target.join(" "), summary: node.failureSummary, html: node.html.slice(0, 300) })),
        })),
      };
    } catch (e) {
      // Not `{ error }` (webdriverio would read it as a WebDriver error). If axe ever needs
      // `new Function` (its template engine), the CSP refuses it and this says so.
      return { failure: String(e) };
    }
  });
  if ("failure" in result) throw new Error(`axe.run failed in the web view: ${result.failure}`);
  return result.violations;
}

const serious = (violations) => violations.filter((v) => v.impact === "serious" || v.impact === "critical");

function describeFindings(violations) {
  return violations
    .map((v) => `${v.id} (${v.impact}): ${v.help}\n${v.nodes.map((n) => `  ${n.target}: ${n.summary}`).join("\n")}`)
    .join("\n");
}

/** Runs axe on the view as it is now, records it and fails on serious or critical findings. */
async function check(theme, view) {
  const violations = await runAxe();
  report.views[`${theme}/${view}`] = violations;
  // The screenshot takes the focus away (no focus ring in the picture); it goes back after, so
  // the next keys (Escape in the palette) reach the view as before.
  await browser.execute(() => {
    window.__e2eFocus = document.activeElement;
  });
  await screenshot(`a11y-${theme}-${view}`);
  await browser.execute(() => window.__e2eFocus?.focus());
  const bad = serious(violations);
  assert.deepEqual(bad, [], `axe in ${view} (${theme}):\n${describeFindings(bad)}`);
}

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `the ${theme} theme was not applied`,
  });
}

async function backToInbox() {
  await testId("rail-inbox").click();
  await testId("message-list").waitForExist({ timeout: 20000 });
  await browser.waitUntil(async () => (await $$('[data-testid="message-row"]').length) > 0, {
    timeout: 20000,
    timeoutMsg: "the inbox shows no rows",
  });
}

describe("stage 8: axe in the real web view, light and dark", () => {
  before(async () => {
    await attach();
    // The steps use the English names; an earlier spec may have left German.
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
    // Opening a message must not change the server; the composer needs a writable account
    // (the new message is discarded, nothing is sent).
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
  });

  after(async () => {
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "a11y-report.json"), JSON.stringify(report, null, 2));
    const counts = Object.fromEntries(Object.entries(report.views).map(([view, violations]) => [view, violations.map((v) => `${v.id}:${v.impact}`)]));
    console.log(`\nE2E REPORT ${JSON.stringify({ axeVersion: report.axeVersion, counts }, null, 2)}\n`);
  });

  for (const theme of THEMES) {
    describe(`${theme} theme`, () => {
      before(async () => {
        await setTheme(theme);
      });

      it("finds nothing serious in the inbox", async () => {
        await backToInbox();
        await check(theme, "inbox");
      });

      it("finds nothing serious in the reading pane", async () => {
        await backToInbox();
        await $('[data-testid="message-row"]').click();
        await testId("reading-pane").waitForExist({ timeout: 20000 });
        await $('[role="toolbar"]').waitForExist({ timeout: 20000 });
        // The body has arrived (text, or the frame of an HTML mail), not only the header.
        await browser.waitUntil(
          async () => browser.execute(() => document.querySelector('[data-testid="reading-pane"] article')?.getAttribute("aria-busy") !== "true"),
          { timeout: 20000, timeoutMsg: "the message body did not load" },
        );
        await check(theme, "reading");
      });

      it("finds nothing serious in the composer", async () => {
        await backToInbox();
        await $('button[aria-label="New message"]').click();
        await testId("composer-subject").waitForExist({ timeout: 20000 });
        await check(theme, "composer");
        // Discarding asks first (the one dialog of the composer): checked too, then confirmed.
        await testId("composer-discard").click();
        await testId("composer-confirm-discard").waitForExist();
        await check(theme, "composer-discard");
        await testId("composer-confirm-discard").$('[data-testid="confirm-dialog-confirm"]').click();
        await testId("composer").waitForExist({ reverse: true, timeout: 20000 });
      });

      it("finds nothing serious in any settings section", async () => {
        const findings = [];
        for (const section of SETTINGS_SECTIONS) {
          await openSettings(section);
          // Let the section's own data arrive.
          await browser.pause(300);
          const violations = await runAxe();
          report.views[`${theme}/settings-${section}`] = violations;
          const bad = serious(violations);
          if (bad.length > 0) findings.push(`${section}:\n${describeFindings(bad)}`);
        }
        await screenshot(`a11y-${theme}-settings`);
        assert.deepEqual(findings, [], findings.join("\n"));
      });

      it("finds nothing serious in the command palette", async () => {
        await backToInbox();
        await browser.keys([MOD, "k"]);
        await testId("command-palette").waitForExist();
        await browser.keys("ArrowDown");
        await check(theme, "palette");
        await browser.keys("Escape");
        await testId("command-palette").waitForExist({ reverse: true });
      });
    });
  }
});
