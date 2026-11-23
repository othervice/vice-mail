// Stage 9 part B in the real app (in `specs` of wdio.conf.mjs before actions.e2e.mjs, which
// changes the inbox on the server). Against the local test server with alice's seed; the account
// is added through the core and stays read-only.
//
// 1. The seed's calendar invitation (mime/calendar-invite, "Kick-off") shows as a card in the
//    reading pane: title, a time, the organizer, "Open in calendar" (not clicked: it would start
//    the system's calendar app).
// 2. A seed newsletter (html/newsletter-0, List-Unsubscribe with One-Click) shows "Unsubscribe";
//    opening the question and cancelling it sends nothing (no result, no operation in the outbox).
//    The core's own test counts connections on a counting server (decision 0020).
// 3. The print styles survived the build: a `@media print` rule hides everything but the message,
//    and the reading pane's article is the print root. The print dialog itself is not opened.
// 4. Settings → General shows whether the app is the default mail app (the button is not pressed:
//    it would change the test machine's default).
// 5. No CSP violation on the way.
//
// Not covered here, to be checked by hand (docs/stages/09, "Der Mensch prüft danach"): a mailto:
// link from the browser opening the composer, setting the default on macOS and Windows, printing
// and saving as PDF, `.eml` export through the save dialog.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
let account;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1);
  return accounts[0];
}

const text = (id) => browser.execute((testid) => document.querySelector(`[data-testid="${testid}"]`)?.textContent ?? null, id);

/** Searches for `query` and opens the hit whose row names `subject` in the reading pane. */
async function openBySearch(query, subject) {
  await browser.keys(["Meta", "f"]);
  const input = await testId("search-input");
  await input.waitForExist({ timeout: 5000 });
  await input.setValue(query);
  let id = null;
  await browser.waitUntil(
    async () => {
      id = await browser.execute(
        (wanted) => [...document.querySelectorAll('[role="option"]')].find((el) => (el.getAttribute("aria-label") ?? "").includes(wanted))?.id ?? null,
        subject,
      );
      return Boolean(id);
    },
    { timeout: 10000, timeoutMsg: `no hit for ${subject}` },
  );
  await (await $(`[id="${id}"]`)).click();
  await testId("reading-pane").waitForExist({ timeout: 10000 });
}

async function closeSearch() {
  const close = await testId("search-close");
  if (await close.isExisting()) await close.click();
}

describe("stage 9 part B: unsubscribe, invitations, print, default mail app", () => {
  before(async () => {
    await attach();
    // The steps use the English names (palette commands, labels); an earlier spec may have left German.
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
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
    account = await onlyAccount();
    assert.equal(account.writesEnabled, false, "new accounts are read-only");
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    await testId("message-list").waitForExist({ timeout: 20000 });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch(() => undefined);
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "extras-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("shows the seed's invitation as a card", async () => {
    await openBySearch("Kick", "Kick-off");
    // The body (and with it the calendar part) may still be on its way: the card follows it.
    await testId("invite-card").waitForExist({ timeout: 20000 });
    report.steps.inviteTitle = await text("invite-title");
    report.steps.inviteWhen = await text("invite-when");
    report.steps.inviteOrganizer = await text("invite-organizer");
    assert.equal(report.steps.inviteTitle, "Kick-off");
    assert.match(report.steps.inviteWhen ?? "", /2026/);
    assert.match(report.steps.inviteOrganizer ?? "", /carol@example\.org/);
    assert.ok(await testId("invite-open").isExisting(), "no “Open in calendar”");
    report.screenshots.push(await screenshot("extras-invite"));
    await closeSearch();
  });

  it("offers unsubscribing for a newsletter and sends nothing without the click", async () => {
    const before = await coreCall("get_outbox_counts", { accountId: account.id });
    // The subject as a phrase: "Example News" alone matches thousands of mails (every address
    // has "example" in it), and the hit would not be among the rendered rows.
    await openBySearch('subject:"Example News: issue #1"', "Example News: issue #1");
    await testId("unsubscribe-bar").waitForExist({ timeout: 10000 });
    await testId("unsubscribe-open").click();
    await testId("unsubscribe-ask").waitForExist({ timeout: 5000 });
    report.steps.unsubscribeWays = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid^="unsubscribe-way-"]')].map((el) => el.getAttribute("data-testid")),
    );
    assert.deepEqual(report.steps.unsubscribeWays, ["unsubscribe-way-oneClick", "unsubscribe-way-mailto", "unsubscribe-way-web"]);
    report.screenshots.push(await screenshot("extras-unsubscribe"));
    await testId("unsubscribe-cancel").click();
    assert.equal(await testId("unsubscribe-result").isExisting(), false, "a result without a click");
    const after = await coreCall("get_outbox_counts", { accountId: account.id });
    assert.deepEqual(after, before, "the outbox changed");
    await closeSearch();
  });

  it("has print styles that print only the message", async () => {
    const rules = await browser.execute(() => {
      const found = [];
      for (const sheet of document.styleSheets) {
        let list;
        try {
          list = sheet.cssRules;
        } catch {
          continue;
        }
        for (const rule of list) {
          if (rule instanceof CSSMediaRule && rule.media.mediaText.includes("print")) {
            for (const inner of rule.cssRules) if (inner.selectorText?.includes("data-print-root")) found.push(inner.selectorText);
          }
        }
      }
      return found;
    });
    report.steps.printRules = rules;
    assert.ok(rules.some((selector) => selector.includes(":has([data-print-root])")), "the print rules are missing from the build");
    // The search (and its reading pane) is closed: open the newest message of the inbox.
    await (await $('[data-testid="message-list"] [role="option"]')).click();
    await $("article[data-print-root]").waitForExist({ timeout: 10000, timeoutMsg: "no print root in the reading pane" });
  });

  it("shows the default mail app state in the settings", async () => {
    await openSettings("general");
    await testId("default-app-state").waitForExist({ timeout: 10000 });
    report.steps.defaultApp = await browser.execute(() => document.querySelector('[data-testid="default-app-state"]')?.getAttribute("data-state"));
    assert.ok(["yes", "no", "unknown"].includes(report.steps.defaultApp));
    report.screenshots.push(await screenshot("extras-default-app"));
  });

  it("caused no CSP violation", async () => {
    assert.deepEqual(await cspViolations(), []);
  });
});
