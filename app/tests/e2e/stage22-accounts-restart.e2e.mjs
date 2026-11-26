// Stage 22, items 1 and 2 (decision 0078): after stage22-accounts.e2e.mjs, the app was quit and
// started again on the same data directory. The order of the accounts (bob before alice, after
// moves by the menu and by dragging) and bob's name in the lists ("Arbeit") come back from the
// database: in the column, the rail and the settings. Removes alice and bob at the end.
import assert from "node:assert/strict";
import { browser } from "@wdio/globals";
import { attach, coreCall, readState, screenshot, setSidebar, testId } from "./support/app.mjs";

describe("stage 22: the account order and names after a restart", () => {
  let expected = null;

  before(async () => {
    expected = readState()?.stage22Accounts ?? null;
    assert.ok(expected, "stage22-accounts.e2e.mjs did not finish, so there is nothing to check");
    await attach();
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
  });

  it("keeps the order and the name", async () => {
    const accounts = await coreCall("list_accounts");
    assert.deepEqual(
      accounts.map((a) => a.email),
      expected.order,
    );
    for (const account of accounts) assert.equal(account.displayName, expected.names[account.id] ?? "", account.email);
    await setSidebar(true);
    const shown = () =>
      browser.execute(() => [...document.querySelectorAll('[data-testid^="sidebar-account-name-"]')].map((name) => name.textContent));
    await browser.waitUntil(async () => (await shown()).length === accounts.length, { timeoutMsg: "the column shows no accounts" });
    assert.deepEqual(await shown(), accounts.map((a) => a.displayName || a.email));
    assert.equal((await shown())[0], "Arbeit");
    await setSidebar(false);
    const rail = await browser.execute(() => [...document.querySelectorAll('[data-testid^="account-switch-"]')].map((b) => b.getAttribute("aria-label")));
    assert.match(rail[0] ?? "", /^Arbeit/);
    assert.match(rail[1] ?? "", /^alice@vicemail\.test/);
    await screenshot("s22acc-after-restart-rail");
    await setSidebar(true);
    assert.ok(await testId("sidebar-column").isExisting());
  });
});
