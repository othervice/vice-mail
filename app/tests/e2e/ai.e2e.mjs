// Stage 13 in the real app (UNRUN: written by the UI session, to be added to `specs` of
// wdio.conf.mjs by the main session after mcp-sync.e2e.mjs). Uses the counting server of
// reading.e2e.mjs (support/counter-server.mjs) as the "provider": every connection to it is
// recorded, so "nothing leaves the device" is measured, not assumed.
//
// 1. AI is off after setup: the panel and the writing help are absent, and with a profile that
//    points at the counter, the model list fails with `ai_off` and the counter sees nothing.
// 2. Turned on: the model list is asked for only on "Load models" (one request to the counter).
// 3. The request log shows nothing for the model list (it carries no content).
import assert from "node:assert/strict";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, invoke, openSettings, testId } from "./support/app.mjs";
import { counter } from "./support/counter-server.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const PROVIDER = "http://127.0.0.1:18999/v1";
let account;
let profileId;

describe("stage 13: AI in the app", () => {
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
    const settings = await coreCall("save_ai_profile", {
      profile: {
        id: null,
        name: "Counter",
        kind: "openai_compatible",
        baseUrl: PROVIDER,
        model: "counted",
        structuredOutput: true,
        maxOutputTokens: null,
        key: { type: "keep" },
      },
    });
    profileId = settings.profiles[0].id;
    for (const aiFunction of ["summary", "reply_suggestion", "writing_help", "search"]) {
      await coreCall("set_ai_function", { aiFunction, profileId });
    }
  });

  after(async () => {
    await coreCall("set_ai_enabled", { enabled: false }).catch(() => undefined);
    if (profileId) await coreCall("delete_ai_profile", { profileId }).catch(() => undefined);
    if (account) await coreCall("remove_account", { accountId: account.id }).catch(() => undefined);
  });

  it("is off: no panel, no writing help, and nothing reaches the provider", async () => {
    await counter.reset();
    assert.equal((await coreCall("get_ai_availability")).enabled, false);
    await testId("rail-inbox").click().catch(() => undefined);
    await browser.pause(500);
    assert.equal(await testId("ai-panel").isExisting(), false, "the AI panel is hidden while AI is off");
    const models = await invoke("list_ai_models", { profileId });
    assert.ok("failure" in models && String(models.failure).includes("ai_off"), "the model list fails with ai_off");
    await openSettings("ai");
    assert.equal(await testId("ai-enabled").getAttribute("aria-checked"), "false");
    assert.equal(await testId("ai-test").isExisting(), false, "no connection test while AI is off");
    const hits = await counter.hits();
    assert.deepEqual(hits.connections, [], "no connection to the provider");
    assert.deepEqual(await cspViolations(), []);
  });

  it("asks the provider for its models only when the user wants them", async () => {
    await counter.reset();
    await testId("ai-enabled").click();
    await browser.waitUntil(async () => (await coreCall("get_ai_settings")).enabled, { timeoutMsg: "AI did not turn on" });
    assert.deepEqual((await counter.hits()).connections, [], "turning AI on sends nothing");
    await testId("ai-profile-edit").click();
    await testId("ai-load-models").click();
    await browser.waitUntil(async () => (await counter.hits()).requests.some((r) => r.path === "/v1/models"), {
      timeout: 10000,
      timeoutMsg: "the model list did not reach the provider",
    });
    await testId("ai-page-log").click();
    const log = await coreCall("list_ai_requests", { before: null, limit: 10 });
    assert.equal(log.entries.length, 0, "the model list is not logged (no content)");
    assert.ok((await $("body").getText()).includes("No requests yet"));
  });
});
