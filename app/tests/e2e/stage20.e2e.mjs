// Stage 20 in the real app against the local test server, one `it` per task (decisions 0058 to
// 0060). alice's account is added through the core with writes on (she sends to herself).
//
// A. The attachment reminder: a text that says "attached" without a file asks "Send without an
//    attachment?"; "Keep writing" goes back, "Send anyway" sends.
// B. "Remind me if no reply": chosen in the composer, kept on the copy in Sent; with the app's
//    clock moved forward (`e2e_advance_clock`) it comes back above the inbox ("No reply since …");
//    bob's answer (In-Reply-To) ends it.
// C. The attachments view: a stored mail's attachments with type, size, sender; the image filter,
//    the image's preview (the core's re-encoding) and "Go to message".
// D. Trackers: a newsletter with a tracking pixel, an image of a known tracking service and a link
//    with utm parameters; the reading pane counts three blocked trackers and explains them, the
//    link opens without its tracking parameters (the recording opener sees the clean address).
// E. The quota: with a storage limit at 95 % of alice's use (management API of the test server),
//    account settings show used and free storage with the notice and the account shows its mark;
//    without a limit "The server does not report storage".
//
// Screenshots: tests/e2e/screenshots/stage20-*.png; facts in stage20-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, invoke, openSettings, openView, screenshot, SCREENSHOT_DIR, setComposerFormat, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, replyTo, setQuota } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
let account;
/** The theme and language before this spec, put back at its end. */
let preferencesBefore = null;

const PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEklEQVR4nGP4z8DAAMIM/4EAAB/uBfsL2WiLAAAAAElFTkSuQmCC";
const wrap = (base64) => base64.replace(/.{1,76}/g, "$&\r\n").trimEnd();

function writeEml(name, lines) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vicemail-s20-"));
  const file = path.join(dir, name);
  fs.writeFileSync(file, lines.join("\r\n"));
  return file;
}

async function openComposer() {
  await $('button[aria-label="New message"]').click();
  await testId("composer-subject").waitForExist({ timeout: 20000 });
  // Plain text, so the test types into a text field (stage 23: a choice of the "…" menu).
  await setComposerFormat("text");
  await testId("composer-text-body").waitForExist({ timeout: 10000 });
}

async function fill(subject, body) {
  const input = await $('[data-testid="composer-to"] [data-testid="recipient-input"]');
  await input.setValue(`${ALICE.email},`);
  await testId("composer-subject").setValue(subject);
  await testId("composer-text-body").setValue(body);
}

async function waitSent(subject) {
  await browser.waitUntil(
    async () => (await coreCall("list_outgoing", { accountId: account.id, before: null, limit: 50 })).some((m) => m.subject === subject && m.state === "sent"),
    { timeout: 60000, interval: 500, timeoutMsg: `${subject} was not sent` },
  );
}

async function openRow(subject) {
  const row = await browser.$(`//*[@role="option" or @role="treeitem"][contains(., "${subject}")]`);
  await row.waitForExist({ timeout: 60000 });
  await row.click();
  await testId("reading-pane").waitForExist({ timeout: 20000 });
}

describe("stage 20: reminders, attachments view, trackers, quota", () => {
  before(async () => {
    await attach();
    // English labels whatever the system's language (as the other specs do).
    preferencesBefore = await coreCall("get_preferences");
    await coreCall("set_preferences", { preferences: { ...preferencesBefore, theme: "light", language: "en" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
      timeout: 10000,
      timeoutMsg: "the UI did not switch to English",
    });
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
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never", blockTrackers: true, cleanLinks: true } });
    const settings = await coreCall("get_compose_settings");
    await coreCall("set_compose_settings", { settings: { ...settings, sendDelaySeconds: 0, attachmentReminder: true } });
    await openView("inbox");
  });

  after(async () => {
    await setQuota(null).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "stage20-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("A: asks before sending a text that speaks of a missing attachment", async () => {
    const subject = `Report ${RUN}`;
    await openComposer();
    await fill(subject, "Hi Alice,\n\nI attached the report for this week.\n\nBest");
    await testId("composer-send").click();
    const dialog = await testId("attachment-reminder");
    await dialog.waitForExist({ timeout: 20000 });
    const text = await dialog.getText();
    assert.match(text, /Send without an attachment\?/);
    assert.match(text, /“attached”/);
    report.screenshots.push(await screenshot("stage20-attachment-reminder"));
    // Keep writing: back in the composer, nothing sent.
    await testId("confirm-dialog-cancel").click();
    await dialog.waitForExist({ reverse: true });
    const before = await coreCall("list_outgoing", { accountId: account.id, before: null, limit: 50 });
    assert.ok(!before.some((m) => m.subject === subject), "nothing was sent");
    // Send anyway.
    await testId("composer-send").click();
    await testId("attachment-reminder").waitForExist({ timeout: 20000 });
    await testId("confirm-dialog-confirm").click();
    await waitSent(subject);
    report.steps.reminder = { asked: true, sent: subject };
  });

  it("B: reminds when no reply comes, with the clock moved forward, and an answer ends it", async () => {
    const subject = `Proposal ${RUN}`;
    await openComposer();
    await fill(subject, "Can you tell me by Friday?");
    // Stage 23 (decision 0086): the reminder is an icon of the send row with a menu of choices.
    const reminder = await testId("composer-follow-up");
    await reminder.click();
    await testId("composer-follow-up-tomorrow").waitForExist({ timeout: 10000, timeoutMsg: "no reminder times" });
    await testId("composer-follow-up-tomorrow").click();
    await browser.waitUntil(async () => (await reminder.getAttribute("data-active")) === "true", { timeoutMsg: "the choice did not stay" });
    await reminder.click();
    await testId("composer-follow-up-menu").waitForExist();
    assert.equal(await testId("composer-follow-up-tomorrow").getAttribute("aria-checked"), "true");
    report.screenshots.push(await screenshot("stage20-follow-up-choice"));
    await browser.keys("Escape");
    await testId("composer-send").click();
    await waitSent(subject);
    const waiting = await browser.waitUntil(
      async () => {
        const list = await coreCall("list_follow_ups", { scope: { kind: "all" } });
        return list.find((m) => m.subject === subject) ?? false;
      },
      { timeout: 60000, interval: 500, timeoutMsg: "the reminder is not on the copy in Sent" },
    );
    assert.equal(waiting.followUp.returned, false);
    // "Waiting for reply" lists it.
    await openView("waiting");
    await browser.$(`//*[@data-testid="message-list"]//*[contains(., "${subject}")]`).waitForExist({ timeout: 20000 });
    report.screenshots.push(await screenshot("stage20-waiting-view"));

    // Two days later: above the inbox with "No reply since …".
    const now = await invoke("e2e_advance_clock", { ms: 2 * 24 * 3600 * 1000 });
    assert.ok(now.ok, JSON.stringify(now));
    await openView("inbox");
    await testId("inbox-block-followUps").waitForExist({ timeout: 60000 });
    const mark = await testId("row-mark-noReply");
    await mark.waitForExist({ timeout: 20000 });
    assert.match(await mark.getText(), /^No reply since /);
    report.screenshots.push(await screenshot("stage20-no-reply"));

    // bob answers in the conversation: the reminder goes.
    const answer = await replyTo(subject);
    report.steps.followUp = { subject, answer };
    await browser.waitUntil(
      async () => !(await coreCall("list_follow_ups", { scope: { kind: "all" } })).some((m) => m.subject === subject),
      { timeout: 120000, interval: 1000, timeoutMsg: "the answer did not end the reminder" },
    );
    await testId("inbox-block-followUps").waitForExist({ reverse: true, timeout: 20000 });
  });

  it("C: lists the attachments of stored mail with filters, previews an image and goes to its message", async () => {
    const subject = `Files ${RUN}`;
    const boundary = `vice-s20-${RUN}`;
    const part = (type, name, base64) =>
      [`--${boundary}`, `Content-Type: ${type}; name="${name}"`, `Content-Disposition: attachment; filename="${name}"`, "Content-Transfer-Encoding: base64", "", wrap(base64)].join("\r\n");
    const file = writeEml("files.eml", [
      `Date: ${new Date().toUTCString()}`,
      "From: Bob Baker <bob@vicemail.test>",
      "To: Alice Archer <alice@vicemail.test>",
      `Subject: ${subject}`,
      `Message-ID: <files.${RUN}@e2e.vicemail.test>`,
      "MIME-Version: 1.0",
      `Content-Type: multipart/mixed; boundary="${boundary}"`,
      "",
      `--${boundary}`,
      "Content-Type: text/plain; charset=utf-8",
      "",
      "The photo and the figures.",
      part("image/png", `holiday-${RUN}.png`, PNG_BASE64),
      part("text/csv", `figures-${RUN}.csv`, Buffer.from("a,b\n1,2\n").toString("base64")),
      `--${boundary}--`,
      "",
    ]);
    await appendMessages("INBOX", [file]);
    await coreCall("retry_account", { accountId: account.id });
    await openView("inbox");
    await openRow(subject);
    await testId("attachments").waitForExist({ timeout: 20000 });

    await openView("attachments");
    const view = await testId("attachments-view");
    await view.waitForExist({ timeout: 20000 });
    const names = () => browser.execute(() => [...document.querySelectorAll('[data-testid="attachment-entry-name"]')].map((e) => e.textContent));
    await browser.waitUntil(async () => (await names()).includes(`holiday-${RUN}.png`), { timeout: 30000, timeoutMsg: "the image is not listed" });
    assert.ok((await names()).includes(`figures-${RUN}.csv`));
    report.screenshots.push(await screenshot("stage20-attachments-view"));
    await testId("attachments-kind-image").click();
    await browser.waitUntil(async () => {
      const shown = await names();
      return shown.includes(`holiday-${RUN}.png`) && !shown.includes(`figures-${RUN}.csv`);
    }, { timeoutMsg: "the image filter did not apply" });
    await testId("attachments-view-name").setValue(`holiday-${RUN}`);
    await browser.waitUntil(async () => (await names()).length === 1, { timeoutMsg: "the name search did not apply" });
    // Preview: the core's re-encoding as a data URI.
    const preview = await $(`button[aria-label="Preview holiday-${RUN}.png"]`);
    await preview.waitForExist({ timeout: 20000 });
    await preview.click();
    await browser.waitUntil(
      async () => browser.execute(() => /^data:image\/(png|jpeg);base64,/.test(document.querySelector('[role="dialog"] img')?.getAttribute("src") ?? "")),
      { timeout: 20000, timeoutMsg: "the preview did not show" },
    );
    await browser.keys("Escape");
    // Go to message: the mail's folder with the message selected.
    await $(`button[aria-label="Go to the message of holiday-${RUN}.png"]`).click();
    await testId("message-list").waitForExist({ timeout: 20000 });
    report.steps.attachments = { listed: await names().catch(() => []) };
  });

  it("D: counts blocked trackers with an explanation and opens links without tracking parameters", async () => {
    const subject = `Offers ${RUN}`;
    const file = writeEml("tracked.eml", [
      `Date: ${new Date().toUTCString()}`,
      "From: Shop News <news@shop.example>",
      "To: Alice Archer <alice@vicemail.test>",
      `Subject: ${subject}`,
      `Message-ID: <tracked.${RUN}@e2e.vicemail.test>`,
      "MIME-Version: 1.0",
      "Content-Type: text/html; charset=utf-8",
      "",
      `<p>Our offers this week. <a href="https://shop.example/offer?id=7&amp;utm_source=news&amp;utm_medium=email&amp;fbclid=x">See the offer</a></p>`,
      `<img src="https://tracker.example/open.gif?u=${RUN}" width="1" height="1" alt="">`,
      `<img src="https://us5.list-manage.com/track/open.php?u=${RUN}" alt="">`,
      `<img src="https://cdn.shop.example/banner.png?id=7&amp;utm_campaign=autumn" width="600" alt="Autumn">`,
      "",
    ]);
    await appendMessages("INBOX", [file]);
    await coreCall("retry_account", { accountId: account.id });
    await openView("inbox");
    await openRow(subject);
    const notice = await testId("tracker-notice");
    await notice.waitForExist({ timeout: 20000 });
    assert.equal(await notice.getAttribute("data-total"), "3");
    await testId("tracker-notice-toggle").click();
    const details = await testId("tracker-notice-details");
    await browser.waitUntil(async () => (await details.isDisplayed()), { timeoutMsg: "no explanation" });
    const text = await details.getText();
    assert.match(text, /1 tracking pixel/);
    assert.match(text, /known tracking service/);
    assert.match(text, /tracking parameters/);
    report.screenshots.push(await screenshot("stage20-trackers"));

    // The link: described and opened without its tracking parameters.
    const list = await coreCall("list_messages", { mailboxId: (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "inbox").id, cursor: null, limit: 50 });
    const message = list.items.find((m) => m.subject === subject);
    const view = await coreCall("get_message", { messageId: message.id, format: "auto" });
    assert.equal(view.body.kind, "html");
    assert.equal(view.body.trackers.pixels + view.body.trackers.knownHosts + view.body.trackers.parameters, 3);
    const link = await coreCall("describe_link", { renderToken: view.body.renderToken, index: 0 });
    assert.equal(link.target, "https://shop.example/offer?id=7");
    assert.deepEqual(link.trackingRemoved, ["utm_source", "utm_medium", "fbclid"]);
    await coreCall("open_link", { renderToken: view.body.renderToken, index: 0 });
    const opened = await invoke("e2e_opened");
    assert.ok(opened.ok.includes("https://shop.example/offer?id=7"), JSON.stringify(opened));
    report.steps.trackers = { summary: view.body.trackers, link };
  });

  it("E: shows the mailbox quota of the test server and the notice from 90 %", async () => {
    const set = await setQuota(95);
    report.steps.quota = { set };
    await openSettings("accounts");
    await testId(`account-row-${account.id}`).click();
    await testId("account-quota-block").waitForExist({ timeout: 20000 });
    await testId("account-quota-refresh").click();
    const quota = await testId("account-quota");
    await quota.waitForExist({ timeout: 30000 });
    const percent = Number(await quota.getAttribute("data-percent"));
    assert.ok(percent >= 90, `percent ${percent}`);
    await testId("account-quota-warning").waitForExist();
    report.screenshots.push(await screenshot("stage20-quota"));
    report.steps.quota.percent = percent;
    // The account in the column carries the mark (the column open, whatever earlier specs left).
    await openView("inbox");
    const sidebarWas = await setSidebar(true);
    await testId(`account-quota-mark-${account.id}`).waitForExist({ timeout: 20000 });
    await setSidebar(sidebarWas);

    // Without a limit the server reports nothing, and the app says so.
    await setQuota(null);
    await openSettings("accounts");
    await testId(`account-row-${account.id}`).click();
    await testId("account-quota-refresh").waitForExist({ timeout: 20000 });
    await testId("account-quota-refresh").click();
    const none = await testId("account-quota-none");
    await none.waitForExist({ timeout: 30000 });
    assert.equal(await none.getText(), "The server does not report storage.");
    assert.deepEqual(await cspViolations(), []);
  });
});
