// Stage 14, block D (decision 0041): a JMAP account through the setup form in the real app,
// against the local test server (Stalwart speaks JMAP on https://localhost:18443, trusted through
// the test CA like its IMAP ports). Bob's account, as seeded by wdio.conf.mjs.
//
// 1. Settings → Accounts → Add account: address and password, then JMAP in the server settings
//    (the test domain is unknown, so the form shows them), host and port by hand.
// 2. "Test connection" logs in over JMAP and says that new mail is pushed.
// 3. "Add account" stores the account (read-only) and the inbox shows bob's seeded messages.
// 4. The account page says JMAP; the account is removed again through the core.
import assert from "node:assert/strict";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, screenshot, testId } from "./support/app.mjs";

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
/** The test server's JMAP endpoint (testmail/accounts.env, TESTMAIL_HTTPS_PORT). */
const JMAP = { host: "localhost", port: "18443" };
/** A message the seed delivers to bob over SMTP (testmail/seed.py, smtp/alice-1). */
const BOB_SUBJECT = "Hello Bob from Alice";

async function fill(id, value) {
  const field = testId(id);
  await field.clearValue();
  await field.setValue(value);
  await browser.waitUntil(async () => (await field.getValue()) === value, {
    timeout: 2000,
    timeoutMsg: `${id} does not hold "${value}"`,
  });
}

/** Clicks the button (or radio) with exactly this text inside `scope`, once it is there. */
async function click(text, scope = "main") {
  const attempt = () =>
    browser.execute(
      (label, within) => {
        const root = document.querySelector(within);
        const target = [...(root?.querySelectorAll('button, [role="radio"]') ?? [])].find(
          (b) => b.textContent.trim() === label,
        );
        if (!target || target.disabled) return false;
        target.click();
        return true;
      },
      text,
      scope,
    );
  try {
    await browser.waitUntil(attempt, { timeout: 10000, interval: 100 });
  } catch {
    const seen = await browser.execute(
      (within) =>
        [...(document.querySelector(within)?.querySelectorAll('button, [role="radio"]') ?? [])].map((b) =>
          b.textContent.trim(),
        ),
      scope,
    );
    throw new Error(`no enabled "${text}" in ${scope}; buttons there: ${JSON.stringify(seen)}`);
  }
}

const mainText = () => browser.execute(() => document.querySelector("main")?.textContent ?? "");

async function waitForText(text, timeout = 20000) {
  try {
    await browser.waitUntil(async () => (await mainText()).includes(text), { timeout, interval: 100 });
  } catch {
    throw new Error(`the view does not show "${text}" but: ${(await mainText()).slice(0, 400)}`);
  }
}

describe("a JMAP account (stage 14, block D)", function () {
  let accountId = null;

  before(async () => {
    await attach();
    // The texts below are the English ones; the system language may be another.
    const preferences = await coreCall("get_preferences", {});
    await coreCall("set_preferences", { preferences: { ...preferences, language: "en" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
      timeoutMsg: "the app did not switch to English",
    });
  });

  after(async () => {
    if (accountId) await coreCall("remove_account", { accountId });
  });

  it("offers JMAP in the server settings and tests the login over JMAP", async () => {
    await openSettings("accounts");
    await click("Add account");
    await testId("add-account").waitForDisplayed({ timeoutMsg: "no setup form" });
    await fill("setup-email", BOB.email);
    await fill("setup-name", "Bob Baker");
    await testId("server-imap").waitForDisplayed({ timeoutMsg: "no server settings" });
    await fill("setup-password", BOB.password);
    await testId("setup-protocol").waitForDisplayed({ timeoutMsg: "no protocol choice" });
    await click("JMAP", '[data-testid="setup-protocol"]');
    await testId("server-jmap").waitForDisplayed({ timeoutMsg: "no JMAP server fields" });
    assert.equal(await testId("server-imap").isExisting(), false, "IMAP fields hidden");
    assert.equal(await testId("server-smtp").isExisting(), false, "SMTP fields hidden");
    assert.equal(await testId("jmap-username").getValue(), BOB.email, "user name prefilled");
    await fill("jmap-host", JMAP.host);
    await fill("jmap-port", JMAP.port);
    await click("Test connection");
    await testId("test-result").waitForDisplayed({ timeout: 20000, timeoutMsg: "no test result" });
    const result = await testId("test-result").getText();
    assert.match(result, /pushes changes/, result);
    await screenshot("jmap-setup-tested");
  });

  it("adds the account and shows its inbox", async () => {
    await click("Add account", '[data-testid="add-account"]');
    await waitForText(BOB_SUBJECT, 30000);
    const accounts = await coreCall("list_accounts", {});
    const bob = accounts.find((a) => a.email === BOB.email);
    assert.ok(bob, "the account is stored");
    accountId = bob.id;
    assert.equal(bob.protocol, "jmap");
    assert.equal(bob.writesEnabled, false, "read-only at first");
    assert.deepEqual(bob.imap, { host: JMAP.host, port: Number(JMAP.port), security: "tls", username: BOB.email });
    await screenshot("jmap-inbox");
    assert.deepEqual(await cspViolations(), [], "no CSP violations");
  });
});
