// Stage 8 in the real app: the whole round without the mouse (docs/stages/08, "Desktop-Test").
// In `specs` of wdio.conf.mjs before actions.e2e.mjs (it archives one message it delivered
// itself, so the seeded inbox stays as the later specs expect).
//
// Only keys drive the UI (`browser.keys`, and `pressTab` for Tab, whose focus move the embedded
// WebDriver does not perform); `coreCall` sets up the account and reads the results back.
// 1. A message from bob arrives. Tab reaches the message list, Home and Enter open the newest
//    message (the delivered one) in the reading pane.
// 2. "r" opens the reply with the focus in the text, typing writes the answer, Cmd+Enter sends it.
// 3. Escape goes back to the list, "e" archives the message; the focus is on the next message.
// 4. Cmd+K, "Go to Archive", Enter: the archived message is there. On the server the reply was
//    sent once and the message is in the Archive folder.
//
// Screenshots go to tests/e2e/screenshots/keyboard-*.png, the facts to keyboard-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, pressTab, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, deliverMail, SERVER, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const SUBJECT = `Tastatur ${RUN}`;
const MOD = process.platform === "darwin" ? "Meta" : "Control";
let account;

const active = () =>
  browser.execute(() => {
    const element = document.activeElement;
    return {
      testId: element?.closest("[data-testid]")?.getAttribute("data-testid") ?? null,
      role: element?.getAttribute("role") ?? null,
      label: element?.getAttribute("aria-label") ?? null,
      // Decision 0031: options are named by their content (one visually hidden text), not aria-label.
      activeOption:
        document.getElementById(element?.getAttribute("aria-activedescendant") ?? "")?.querySelector('[data-testid="row-name"]')?.textContent ?? null,
    };
  });

/**
 * A screenshot without losing the focus: `screenshot` blurs the focused element (no focus ring in
 * the picture), which would send the next keys to the page body instead of where the user is.
 */
async function shot(name) {
  await browser.execute(() => {
    window.__e2eFocus = document.activeElement;
  });
  const file = await screenshot(name);
  await browser.execute(() => window.__e2eFocus?.focus());
  return file;
}

/**
 * Presses Tab until the message list has the focus (at most `limit` times). `pressTab` moves the
 * focus as the browser would: the embedded WebDriver sends Tab's key events only.
 */
async function tabToList(limit = 40) {
  for (let i = 0; i < limit; i += 1) {
    const now = await active();
    if (now.role === "listbox" && now.testId === "message-list") return now;
    await pressTab();
  }
  throw new Error("Tab never reached the message list");
}

async function mailboxByRole(role) {
  const mailboxes = await coreCall("list_mailboxes", { accountId: account.id });
  const found = mailboxes.find((m) => m.role === role);
  assert.ok(found, `no ${role} folder`);
  return found;
}

async function subjectsIn(role) {
  const mailbox = await mailboxByRole(role);
  const page = await coreCall("list_messages", { mailboxId: mailbox.id, cursor: null, limit: 200 });
  return page.items.map((item) => item.subject);
}

describe("stage 8: open, reply, send, archive without the mouse", () => {
  before(async () => {
    await attach();
    // The steps use the English names (palette commands, labels); an earlier spec may have left German.
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
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    await coreCall("set_compose_settings", { settings: { sendDelaySeconds: 5 } });
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    await deliverMail(SUBJECT);
    await browser.waitUntil(async () => (await subjectsIn("inbox")).includes(SUBJECT), {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the delivered message did not reach the inbox",
    });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "keyboard-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("Tab reaches the list, Home and Enter open the newest message", async () => {
    await tabToList();
    await browser.keys("Home");
    const listFocus = await active();
    assert.ok(listFocus.activeOption?.includes(SUBJECT), `cursor on ${listFocus.activeOption}`);
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await active()).testId !== "message-list", { timeoutMsg: "Enter did not move the focus" });
    const pane = await browser.execute(() => document.querySelector('[data-testid="reading-pane"]')?.textContent ?? "");
    assert.ok(pane.includes(SUBJECT), "the reading pane does not show the message");
    report.steps.open = { focus: await active() };
    report.screenshots.push(await shot("keyboard-open"));
  });

  it("r replies, typing writes the answer, Cmd/Ctrl+Enter sends it", async () => {
    const before = await serverFolders();
    await browser.keys("r");
    await testId("composer").waitForExist({ timeout: 20000 });
    let focus;
    await browser
      .waitUntil(
        async () => {
          focus = await active();
          return focus.testId?.startsWith("composer") ?? false;
        },
        { timeoutMsg: "the composer did not take the focus" },
      )
      .catch(async (e) => {
        const page = await browser.execute(() => ({
          visibility: document.visibilityState,
          hasFocus: document.hasFocus(),
          active: document.activeElement?.tagName ?? null,
          textBody: Boolean(document.querySelector('[data-testid="composer-text-body"]')),
          richBody: Boolean(document.querySelector('[data-testid="composer-rich-body"]')),
          loading: Boolean(document.querySelector('[data-testid="composer-loading"]')),
        }));
        throw new Error(`${e.message}: ${JSON.stringify({ focus, page })}`);
      });
    // The reply opens with the focus in the text; the recipient and subject are filled in.
    await browser.keys([..."Danke, angekommen."]);
    report.screenshots.push(await shot("keyboard-reply"));
    await browser.execute(() => {
      window.__e2eKeys = [];
      document.addEventListener(
        "keydown",
        (event) => {
          window.__e2eKeys.push({ key: event.key, meta: event.metaKey, ctrl: event.ctrlKey, target: event.target?.getAttribute?.("data-testid") ?? event.target?.tagName });
        },
        true,
      );
    });
    await browser.keys([MOD, "Enter"]);
    await browser
      .waitUntil(
        () => browser.execute(() => document.querySelector('[data-testid="send-notice"]')?.getAttribute("data-send-state") === "sent"),
        { timeout: 60000, interval: 500, timeoutMsg: "the reply was never sent" },
      )
      .catch(async (e) => {
        const page = await browser.execute(() => ({
          keys: window.__e2eKeys,
          composer: Boolean(document.querySelector('[data-testid="composer"]')),
          notices: [...document.querySelectorAll('[data-testid="send-notice"]')].map((n) => n.getAttribute("data-send-state")),
          errors: [...document.querySelectorAll('[data-testid="composer"] [role="alert"]')].map((n) => n.textContent),
        }));
        throw new Error(`${e.message}: ${JSON.stringify(page)}`);
      });
    await browser.waitUntil(async () => ((await serverFolders()).Sent?.messages ?? 0) === (before.Sent?.messages ?? 0) + 1, {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the reply is not in Sent once",
    });
    report.steps.reply = { sentBefore: before.Sent?.messages ?? 0 };
  });

  it("Escape goes back to the list, e archives, the focus is on the next message", async () => {
    // After sending, the focus is back where it was before the composer: the reading pane.
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await active()).testId === "message-list", { timeoutMsg: "Escape did not reach the list" });
    const before = await active();
    assert.ok(before.activeOption?.includes(SUBJECT), `cursor on ${before.activeOption}`);
    await browser.keys("e");
    await browser.waitUntil(async () => !((await active()).activeOption ?? "").includes(SUBJECT), {
      timeoutMsg: "the cursor stayed on the archived message",
    });
    const after = await active();
    assert.equal(after.testId, "message-list", "the focus left the list");
    await browser.waitUntil(async () => (await subjectsIn("archive")).includes(SUBJECT), {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the message is not in the archive",
    });
    report.steps.archive = { next: after.activeOption };
    report.screenshots.push(await shot("keyboard-archived"));
  });

  it("the command palette goes to the archive, where the message is", async () => {
    await browser.keys([MOD, "k"]);
    await testId("command-palette").waitForExist();
    await browser.keys([..."Go to Archive"]);
    await browser.keys("Enter");
    await testId("command-palette").waitForExist({ reverse: true });
    await tabToList();
    const list = await browser.execute(() => document.querySelector('[data-testid="message-list"]')?.textContent ?? "");
    assert.ok(list.includes(SUBJECT), "the archive view does not list the message");
    report.screenshots.push(await shot("keyboard-archive-view"));
  });
});
