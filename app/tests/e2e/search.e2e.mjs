// Stage 7 in the real app: search, conversations and identities against the local test server
// (wdio.conf.mjs seeds 10,000 messages for alice). In `specs` before actions.e2e.mjs: it needs
// the seed's offsite thread in Archive as seeded.
//
// 1. Cmd+F opens the search field with the focus in it; typing a word of a real subject shows
//    hits with the word marked, and the core answers within 200 ms.
// 2. A broken date (`before:32.13.2026`) is explained under the field, at its place.
// 3. "Search on the server" asks the server and comes back with a list or the "nothing more" note.
// 4. The conversation view (decision 0039; account-sync.e2e.mjs turned it off) is offered in the
//    sorted inbox too; in Archive it lists threads with their size (the seed's offsite thread),
//    opening one shows its messages above the pane, and the inbox lists conversations as well.
// 5. Identities: the account's default identity is listed in its settings.
// 6. No CSP violation on the way.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, openView, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";

/** Stage 23: conversations on or off from the list's "…" menu in the toolbar. */
async function toggleConversations(on) {
  await testId("list-options").click();
  await testId("conversations-menu-toggle").click();
  await browser.waitUntil(async () => (await coreCall("get_ui_preferences")).conversations === on, {
    timeoutMsg: `conversations did not turn ${on ? "on" : "off"}`,
  });
}
import { ALICE, SERVER } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
let account;

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

const text = (id) => browser.execute((testid) => document.querySelector(`[data-testid="${testid}"]`)?.textContent ?? null, id);

describe("stage 7: search, conversations, identities", () => {
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
    await testId("message-list").waitForExist({ timeout: 20000 });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "search-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("finds a word of a real subject while typing, marked in the rows", async () => {
    const boxes = await coreCall("list_mailboxes", { accountId: account.id });
    const inbox = boxes.find((b) => b.role === "inbox");
    const page = await coreCall("list_messages", { mailboxId: inbox.id, cursor: null, limit: 20 });
    const word = page.items
      .flatMap((m) => m.subject.split(/\s+/))
      .find((w) => /^[\p{L}]{5,}$/u.test(w));
    assert.ok(word, "no usable word in the subjects");

    const started = Date.now();
    const direct = await coreCall("search", {
      request: { query: word, scope: { kind: "all" }, cursor: null, limit: 100, typing: false },
    });
    report.steps.searchMs = Date.now() - started;
    assert.ok(direct.hits.length > 0, `the core finds nothing for ${word}`);
    assert.ok(report.steps.searchMs < 200, `search took ${report.steps.searchMs} ms`);

    await browser.keys(["Meta", "f"]);
    const input = await testId("search-input");
    await input.waitForExist();
    assert.equal(await browser.execute(() => document.activeElement?.getAttribute("data-testid")), "search-input");
    await input.setValue(word);
    await browser.waitUntil(
      async () => (await browser.execute(() => document.querySelectorAll('[data-testid="highlight"]').length)) > 0,
      { timeout: 5000, timeoutMsg: "no marked hit" },
    );
    report.steps.word = word;
    report.screenshots.push(await screenshot("search-results"));
  });

  it("explains a broken date at its place", async () => {
    const input = await testId("search-input");
    await input.setValue("before:32.13.2026");
    await testId("search-problems").waitForExist({ timeout: 5000 });
    report.steps.problem = await text("search-problems");
    assert.match(report.steps.problem ?? "", /32\.13\.2026/);
    assert.equal(await text("search-problem-mark"), "32.13.2026");
    report.screenshots.push(await screenshot("search-problem"));
  });

  it("searches on the server when asked", async () => {
    const input = await testId("search-input");
    await input.setValue(report.steps.word);
    const button = await testId("search-server");
    await button.waitForExist({ timeout: 5000 });
    await button.click();
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => Boolean(document.querySelector('[data-testid="search-server-none"]')) || Boolean(document.querySelector('[data-testid="search-server-status"]')),
        )) === true,
      { timeout: 20000, timeoutMsg: "the server search did not answer" },
    );
    await testId("search-server-back").click();
    await testId("search-close").click();
  });

  it("lists conversations in a plain folder and in the sorted inbox, and opens one", async () => {
    // Decision 0039: offered in the sorted inbox too; off since account-sync.e2e.mjs.
    // Stage 23 (decision 0084): the switch is in the list's "…" menu (the toolbar's own button
    // shows in wide windows only).
    assert.equal((await coreCall("get_ui_preferences")).conversations, false, "conversations off in the inbox");
    // The seed's 60-message thread "Planning the 2026 offsite" lies in Archive, INBOX and Sent.
    await openView("archive");
    await testId("message-list").waitForExist({ timeout: 20000 });
    await toggleConversations(true);
    await testId("message-list").waitForExist({ timeout: 20000 });
    // Stage 17 (decision 0049): the list is a tree; a conversation row says "n replies".
    let first = null;
    await browser.waitUntil(
      async () => {
        first = await browser.execute(
          () => document.querySelector('[data-testid="row-replies"]')?.closest('[role="treeitem"]')?.id ?? null,
        );
        return first !== null;
      },
      { timeout: 20000, timeoutMsg: "no conversation row with replies" },
    );
    await (await browser.$(`[id="${first}"]`)).click();
    await testId("thread-pane").waitForExist({ timeout: 10000 });
    await testId("message-card").waitForExist({ timeout: 10000 });
    // One message on the right, not the stacked conversation.
    assert.deepEqual(
      await browser.execute(() => [document.querySelectorAll('[data-testid="message-card"]').length, document.querySelectorAll('[data-testid="thread-entries"]').length]),
      [1, 0],
    );
    report.steps.threadRows = await browser.execute(() => document.querySelectorAll('[data-testid="row-replies"]').length);
    report.screenshots.push(await screenshot("conversation"));
    // The offsite thread lies in INBOX as well: a conversation row of the sorted inbox.
    await openView("inbox");
    await browser.waitUntil(
      async () => (await browser.execute(() => document.querySelectorAll('[data-testid="row-replies"]').length)) > 0,
      { timeout: 20000, timeoutMsg: "no conversation row in the inbox" },
    );
    await toggleConversations(false);
  });

  it("lists the account's identities in its settings", async () => {
    const identities = await coreCall("list_identities", { accountId: account.id });
    report.steps.identities = identities.length;
    await openSettings("accounts");
    await testId(`account-row-${account.id}`).click();
    await testId("identities").waitForExist({ timeout: 10000 });
    if (identities.length > 0) await testId("identity-default").waitForExist();
    assert.deepEqual(await cspViolations(), [], "CSP violations during stage 7");
  });
});
