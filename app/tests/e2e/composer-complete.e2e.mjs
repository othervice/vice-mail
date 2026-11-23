// Stage 10 in the real app: the composer's new editing features against the local test server
// (in `specs` of wdio.conf.mjs after compose.e2e.mjs; the account stays read-only, drafts are local).
//
// 1. Rich text: a heading, centred, and struck-through text are saved as the HTML the core keeps
//    (`<h2 style="text-align: …">`, `<s>`), without CSP violations.
// 2. Pasting HTML with a script and an external image: the core cleans it (`cleanPastedHtml`),
//    the editor holds neither, and the hint about removed images shows.
// 3. A template made in the core is inserted from the composer's menu, filled for the recipient
//    (fallback of `{{first_name|…}}` without a name), and the subject is taken over.
// 4. "Forward as attachment" in the reading pane opens a draft with the message as `.eml`.
//
// Inline images need the system file dialog or a real file drop, which the embedded WebDriver
// cannot drive; they are covered by tests/composer-complete.test.tsx and the core's tests.
// Screenshots go to tests/e2e/screenshots/composer-complete-*.png, the facts to
// composer-complete-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openView, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
let account;

/**
 * Puts the focus back into the text after a click on the format bar. A real mouse press leaves it
 * in the editor (the buttons cancel mouse down); the embedded WebDriver's click focuses the button
 * it clicks, and TipTap moves the focus back only on the next animation frame, which a hidden
 * window does not run.
 */
const backToText = () => browser.execute(() => document.querySelector('[data-testid="composer-rich-body"]')?.focus());

/**
 * Picks a block style in the native select as a choice in its menu does (value, then `change`):
 * the driver's click on an `<option>` does not select it.
 */
const pickBlockStyle = (value) =>
  browser.execute((v) => {
    const select = document.querySelector('[data-testid="format-block"]');
    select.value = v;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  }, value);

async function openRichComposer(subject) {
  await $('button[aria-label="New message"]').click();
  await testId("composer-subject").waitForExist({ timeout: 20000 });
  const input = await $('[data-testid="composer-to"] [data-testid="recipient-input"]');
  await input.setValue(`${ALICE.email},`);
  await testId("composer-subject").setValue(subject);
  await testId("composer-format-html").click();
  await testId("composer-rich-body").waitForExist({ timeout: 20000 });
}

async function savedDraft(subject) {
  let found;
  await browser.waitUntil(
    async () => {
      const drafts = await coreCall("list_drafts", { accountId: account.id });
      const summary = drafts.find((d) => d.subject === subject);
      if (!summary) return false;
      found = await coreCall("get_draft", { draftId: summary.id });
      return true;
    },
    { timeout: 20000, interval: 500, timeoutMsg: `draft ${subject} not saved` },
  );
  return found;
}

describe("stage 10: the composer complete", () => {
  before(async () => {
    await attach();
    // The steps use the English names (labels such as "New message").
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
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    const templates = await coreCall("list_templates").catch(() => []);
    for (const t of templates) await coreCall("delete_template", { templateId: t.id }).catch(() => undefined);
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "composer-complete-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("saves a centred heading and struck-through text as the HTML the core keeps", async () => {
    const subject = `Format ${RUN}`;
    await openRichComposer(subject);
    const editor = await testId("composer-rich-body");
    await editor.click();
    await browser.keys([..."Plan"]);
    await pickBlockStyle("heading2");
    await backToText();
    await testId("format-align-center").click();
    await backToText();
    await browser.keys("Enter");
    await testId("format-strike").click();
    await backToText();
    await browser.keys([..."alt"]);
    report.screenshots.push(await screenshot("composer-complete-format"));
    await testId("composer-close").click();
    await testId("composer").waitForExist({ reverse: true });
    const draft = await savedDraft(subject);
    report.steps.format = { bodyHtml: draft.bodyHtml };
    assert.match(draft.bodyHtml, /<h2 style="text-align: ?center;?">Plan<\/h2>/);
    assert.match(draft.bodyHtml, /<s>alt<\/s>/);
    assert.deepEqual(await cspViolations(), [], "CSP violations while formatting");
  });

  it("cleans pasted HTML in the core before it enters the editor", async () => {
    await openRichComposer(`Paste ${RUN}`);
    const editor = await testId("composer-rich-body");
    await editor.click();
    await browser.execute((el) => {
      const data = new DataTransfer();
      data.setData("text/html", '<p style="color:red">Aus <b>Word</b><img src="https://tracker.example/p.gif"></p><script>window.pasted=1</script>');
      data.setData("text/plain", "Aus Word");
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
    }, editor);
    await browser.waitUntil(async () => (await editor.getText()).includes("Aus Word"), { timeoutMsg: "the paste never arrived" });
    const html = await browser.execute((el) => el.innerHTML, editor);
    report.steps.paste = { html };
    assert.ok(!/<script|<img|tracker\.example/i.test(html), `the editor kept unsafe content: ${html}`);
    await testId("composer-paste-note").waitForExist({ timeout: 5000 });
    assert.equal(await browser.execute(() => window.pasted ?? null), null, "the pasted script ran");
    report.screenshots.push(await screenshot("composer-complete-paste"));
    await testId("composer-close").click();
  });

  it("inserts a template filled for the recipient from the composer's menu", async () => {
    await coreCall("save_template", {
      template: { id: null, name: `Offer ${RUN}`, subject: "Angebot {{my_first_name}}", html: "<p>Hallo {{first_name|du}},</p>" },
    });
    const subject = "";
    await openRichComposer(subject);
    await testId("composer-template").click();
    const option = await $(`[data-testid="composer-template-option"]*=Offer ${RUN}`);
    await option.waitForExist({ timeout: 10000 });
    await option.click();
    const editor = await testId("composer-rich-body");
    await browser.waitUntil(async () => (await editor.getText()).includes("Hallo du,"), { timeoutMsg: "the template was not inserted" });
    assert.match(await testId("composer-subject").getValue(), /^Angebot /);
    report.steps.template = { subject: await testId("composer-subject").getValue(), text: await editor.getText() };
    report.screenshots.push(await screenshot("composer-complete-template"));
    await testId("composer-discard").click();
    await testId("composer-confirm-discard").waitForExist();
    // A CSS selector cannot hold WebdriverIO's text selector: the button by its text within the dialog.
    await testId("composer-confirm-discard").$("button=Discard").click();
    await testId("composer").waitForExist({ reverse: true });
  });

  it("forwards a message as an .eml attachment from the reading pane", async () => {
    await openView("inbox");
    const row = await $('[data-testid="message-list"] [role="option"]');
    await row.waitForExist({ timeout: 20000 });
    await row.click();
    const action = await $('button[aria-label="Forward as attachment"]');
    await action.waitForExist({ timeout: 20000 });
    await action.click();
    const attachment = await testId("composer-attachment");
    await attachment.waitForExist({ timeout: 20000 });
    const name = await attachment.getText();
    report.steps.forwardAsAttachment = { attachment: name };
    assert.match(name, /\.eml/);
    report.screenshots.push(await screenshot("composer-complete-forward"));
    await testId("composer-close").click();
  });
});
