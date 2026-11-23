// Stage 9 in the real app (in `specs` of wdio.conf.mjs before actions.e2e.mjs, which changes the
// inbox on the server). Against the local test server with alice's seed; the account is added
// through the core and stays read-only.
//
// 1. The inbox is the sorted inbox: `listInbox` answers, bundles of notifications and newsletters
//    are single rows that unfold in place (a click on the bundle button).
// 2. Pin (P), snooze (B → Tomorrow) and task (T) on a read-only account: each works, the marks
//    show in the rows and the views Later, Pinned and Tasks, and nothing moves on the server
//    (folder counts before and after are equal, no operation in the outbox).
// 3. "Why here?" in the reading pane names a reason.
// 4. Settings → Inbox → "Sort the inbox" off: no bundle rows any more; on again.
// 5. No CSP violation on the way.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, openView, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { steps: {} };
let account;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1);
  return accounts[0];
}

function optionLabels() {
  return browser.execute(() =>
    [...document.querySelectorAll('[data-testid="message-list"] [role="option"]')].map((el) => el.querySelector('[data-testid="row-name"]')?.textContent ?? ""),
  );
}

async function focusFirstRow() {
  const first = await $('[data-testid="message-list"] [role="option"]');
  await first.click();
  await browser.waitUntil(async () => (await $('[data-testid="reading-pane"]')).isExisting(), { timeoutMsg: "no reading pane" });
  return browser.execute((el) => el.querySelector('[data-testid="row-name"]')?.textContent ?? "", first);
}

describe("stage 9: inbox comfort", () => {
  before(async () => {
    await attach();
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
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch(() => undefined);
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "comfort-report.json"), JSON.stringify(report, null, 2));
  });

  it("shows the sorted inbox with bundles that unfold in place", async () => {
    await browser.waitUntil(async () => (await optionLabels()).length > 0, { timeoutMsg: "no rows" });
    const bundles = await $$('[data-testid="bundle-row"]');
    report.steps.bundles = bundles.length;
    assert.ok(bundles.length > 0, "no bundle row in the sorted inbox (the seed has newsletters and notifications)");
    const bundle = bundles[0];
    assert.equal(await bundle.getAttribute("data-expanded"), "false");
    // The list is virtualized: the number of rendered rows stays about the same when a bundle
    // unfolds. What changes is the row right below the bundle (now its first member, a message
    // that had no row before) and the height of the list.
    const below = () =>
      browser.execute(() => {
        const list = document.querySelector('[data-testid="message-list"]');
        const bundleRow = list.querySelector('[data-testid="bundle-row"]').closest("[data-index]");
        const next = list.querySelector(`[data-index="${Number(bundleRow.dataset.index) + 1}"]`);
        return {
          bundle: list.querySelector('[data-testid="bundle-row"]').dataset.bundle,
          next: next?.getAttribute("role") === "option" ? next.id : null,
          ids: [...list.querySelectorAll('[role="option"]')].map((o) => o.id),
          height: list.scrollHeight,
        };
      });
    const before = await below();
    await bundle.click();
    let after;
    await browser
      .waitUntil(
        async () => {
          after = await below();
          return after.bundle === before.bundle && after.next !== null && !before.ids.includes(after.next) && after.height > before.height;
        },
        { timeoutMsg: "bundle did not unfold" },
      )
      .catch((e) => {
        const brief = (state) => state && { ...state, ids: state.ids.length };
        throw new Error(`${e.message}: ${JSON.stringify({ before: brief(before), after: brief(after) })}`);
      });
    assert.equal(await bundle.getAttribute("data-expanded"), "true");
    report.steps.unfolded = { bundle: before.bundle, firstMember: after.next, heightBefore: before.height, heightAfter: after.height };
  });

  it("pins, snoozes and marks tasks on a read-only account without moving anything on the server", async () => {
    const before = await serverFolders();
    const label = await focusFirstRow();
    report.steps.first = label;
    await browser.keys(["p"]);
    await browser.keys(["t"]);
    await browser.waitUntil(async () => (await coreCall("list_pinned", { scope: { kind: "all" } })).length === 1, {
      timeoutMsg: "not pinned",
    });
    await browser.waitUntil(async () => (await coreCall("list_tasks", { scope: { kind: "all" }, filter: "open" })).length === 1, {
      timeoutMsg: "no task",
    });
    await focusFirstRow();
    await browser.keys(["b"]);
    await (await testId("snooze-tomorrow")).click();
    await browser.waitUntil(async () => (await coreCall("list_snoozed", { scope: { kind: "all" } })).length === 1, {
      timeoutMsg: "not snoozed",
    });
    // Nothing moved: same counts per folder, and the read-only account queued no operation.
    const after = await serverFolders();
    assert.deepEqual(
      Object.fromEntries(Object.entries(after).map(([name, f]) => [name, f.messages])),
      Object.fromEntries(Object.entries(before).map(([name, f]) => [name, f.messages])),
    );
    const operations = await coreCall("list_operations", { accountId: account.id, before: null, limit: 50 });
    assert.equal(operations.length, 0, JSON.stringify(operations));
    report.steps.serverUnchanged = true;

    for (const view of ["later", "pinned", "tasks"]) {
      await openView(view);
      await browser.waitUntil(async () => (await optionLabels()).length === 1, { timeoutMsg: `${view} is not listed` });
    }
    await openView("inbox");
  });

  it("explains in the reading pane why a message is where it is", async () => {
    await browser.waitUntil(async () => (await optionLabels()).length > 0);
    await focusFirstRow();
    await (await testId("category-why")).click();
    const reason = await (await testId("category-reason")).getText();
    assert.ok(reason.length > 0);
    report.steps.reason = reason;
  });

  it("sorting off makes the inbox purely chronological", async () => {
    await openSettings("inbox");
    await (await testId("inbox-categories")).click();
    await openView("inbox");
    await browser.waitUntil(async () => (await $$('[data-testid="bundle-row"]')).length === 0, { timeoutMsg: "bundles stayed" });
    await openSettings("inbox");
    await (await testId("inbox-categories")).click();
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
