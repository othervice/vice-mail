// Stage 22, item 6 (decision 0077): the learning spam filter in the real app against the local
// test server. alice's account is added through the core with writes on.
//
// 1. Settings → Inbox: the switch turns the filter on; 60 offers appended to the junk folder and
//    220 read letters to the inbox; "Learn now" learns them in the background with progress and
//    the filter is ready (at least 50 spam and 200 other mails).
// 2. A new offer that arrives in the inbox goes to the junk folder on the server by itself; in the
//    junk folder its row says "Spam filter" and the reading pane's note says so; "Not spam" in the
//    note moves it back to the inbox on the server.
// 3. With writes off, a new offer is only marked here ("Spam?" on the row, the note with "Not
//    spam"); nothing changes on the server; "Not spam" removes the mark.
//
// Screenshots (German, light and dark): tests/e2e/screenshots/spam-*.png; facts in
// spam-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, openView, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
let account;
let junk;
/** The theme and language before this spec, put back at its end. */
let preferencesBefore = null;

const WORDS = {
  offer: ["cheap", "pills", "pharmacy", "discount", "viagra", "cialis", "prescription", "winner", "prize", "casino", "bonus", "loan", "crypto", "investment", "guaranteed", "limited", "exclusive", "unsubscribe"],
  letter: ["meeting", "tomorrow", "project", "report", "budget", "review", "agenda", "lunch", "station", "weekend", "invoice", "draft", "minutes", "schedule", "holiday", "notes", "question", "thanks"],
};

function pick(list, n, seed) {
  return Array.from({ length: n }, (_, i) => list[(seed * 7 + i * 5) % list.length]);
}

function rfcDate(offsetMinutes) {
  return new Date(Date.now() - offsetMinutes * 60_000).toUTCString().replace("GMT", "+0000");
}

function offer(n, minutesAgo) {
  const words = pick(WORDS.offer, 10, n).join(" ");
  return [
    `From: Best Deals <deals${n % 7}@cheap-offers.example>`,
    "To: undisclosed-recipients:;",
    `Subject: ${pick(WORDS.offer, 3, n).join(" ").toUpperCase()} - ACT NOW ${RUN}-${n}`,
    `Date: ${rfcDate(minutesAgo)}`,
    `Message-ID: <offer-${RUN}-${n}@cheap-offers.example>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    `Dear friend, ${words}. Order now and save money! Click http://cheap-offers.example/deal?id=${n}`,
    "Limited offer, act now. Unsubscribe http://cheap-offers.example/remove",
    "",
  ];
}

function letter(n, minutesAgo) {
  const names = ["Bob Builder <bob@vicemail.test>", "Carol Chen <carol@work.example>", "Dan Diaz <dan@friends.example>", "Erin Ernst <erin@work.example>"];
  const words = pick(WORDS.letter, 8, n).join(" ");
  return [
    `From: ${names[n % names.length]}`,
    `To: Alice Archer <${ALICE.email}>`,
    `Subject: ${pick(WORDS.letter, 2, n).join(" and ")} (${RUN}-${n})`,
    `Date: ${rfcDate(minutesAgo)}`,
    `Message-ID: <letter-${RUN}-${n}@vicemail.test>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    `Hi Alice, about the ${words}: can we talk about it this week? I put my notes below.`,
    "Best regards",
    "",
  ];
}

function writeEmls(prefix, messages) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `vicemail-s22-${prefix}-`));
  return messages.map((lines, i) => {
    const file = path.join(dir, `${prefix}-${i}.eml`);
    fs.writeFileSync(file, lines.join("\r\n"));
    return file;
  });
}

async function status() {
  return coreCall("get_spam_filter");
}

async function setLanguageAndTheme(language, theme) {
  const prefs = await coreCall("get_preferences");
  await coreCall("set_preferences", { preferences: { ...prefs, language, theme } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === language, {
    timeout: 10000,
    timeoutMsg: `the UI did not switch to ${language}`,
  });
}

/** Both themes, German, as the stage's screenshots are asked for. */
async function shots(name) {
  for (const theme of ["light", "dark"]) {
    await setLanguageAndTheme("de", theme);
    await browser.pause(300);
    report.screenshots.push(await screenshot(`spam-${name}-${theme}`));
  }
  await setLanguageAndTheme("en", "light");
}

async function openRow(subject) {
  const row = await browser.$(`//*[@role="option" or @role="treeitem"][contains(., "${subject}")]`);
  await row.waitForExist({ timeout: 60000 });
  await row.click();
  await testId("reading-pane").waitForExist({ timeout: 20000 });
}

async function inServer(folder) {
  const folders = await serverFolders();
  return folders[folder]?.messages ?? 0;
}

describe("stage 22: the learning spam filter", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await setLanguageAndTheme("en", "light");
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
    const reading = await coreCall("get_reading_preferences");
    await coreCall("set_reading_preferences", { preferences: { ...reading, markReadOnOpen: "never" } });
    junk = (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "junk");
    assert.ok(junk, "the test server has a junk folder");
    report.steps.junkFolder = junk.path;
  });

  after(async () => {
    await coreCall("forget_spam_filter").catch(() => undefined);
    await coreCall("set_spam_filter", { settings: { enabled: false, excludedAccounts: [] } }).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "spam-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("1: turns on, learns from the junk folder and read inbox mail, with progress", async () => {
    await openSettings("inbox");
    const toggle = testId("spam-enabled");
    await toggle.scrollIntoView();
    assert.equal(await toggle.getAttribute("aria-checked"), "false", "off by default");
    await toggle.click();
    await browser.waitUntil(async () => (await status()).settings.enabled, { timeoutMsg: "the filter did not turn on" });

    const spam = writeEmls("offer", Array.from({ length: 60 }, (_, i) => offer(i, 60 + i)));
    const ham = writeEmls("letter", Array.from({ length: 220 }, (_, i) => letter(i, 30 + i)));
    await appendMessages(junk.path, spam);
    await appendMessages("INBOX", ham, { seen: true });
    await coreCall("reload_folders", { accountId: account.id });
    await browser.waitUntil(
      async () => {
        const page = await coreCall("list_messages", { mailboxId: junk.id, cursor: null, limit: 100 });
        return page.items.filter((m) => m.subject.includes(RUN)).length >= 60;
      },
      { timeout: 120000, interval: 500, timeoutMsg: "the offers did not reach the junk folder" },
    );

    const started = Date.now();
    await testId("spam-learn").click();
    await testId("spam-learning-state").waitForExist({ timeout: 20000 });
    report.steps.progressShown = await testId("spam-learning-state").getText();
    await browser.waitUntil(async () => (await status()).learning?.state === "done", {
      timeout: 300000,
      interval: 500,
      timeoutMsg: "learning did not finish",
    });
    const learned = await status();
    report.steps.learning = { ms: Date.now() - started, ...learned.learning, learnedSpam: learned.learnedSpam, learnedHam: learned.learnedHam, tokens: learned.tokens };
    assert.ok(learned.learnedSpam >= 50, `learned spam ${learned.learnedSpam}`);
    assert.ok(learned.learnedHam >= 200, `learned ham ${learned.learnedHam}`);
    assert.ok(learned.ready);
    await browser.waitUntil(async () => (await testId("spam-learning-state").getAttribute("data-state")) === "done", {
      timeoutMsg: "the settings did not show the end",
    });
    await testId("spam-settings").scrollIntoView();
    await shots("settings");
    assert.deepEqual(await cspViolations(), []);
  });

  it("2: moves a new offer to the junk folder; Not spam brings it back", async () => {
    await openView("inbox");
    const before = await inServer(junk.path);
    const fresh = writeEmls("new-offer", [offer(100, 0)]);
    const subjectPart = `${RUN}-100`;
    const started = Date.now();
    await appendMessages("INBOX", fresh);
    await browser.waitUntil(async () => (await inServer(junk.path)) > before, {
      timeout: 120000,
      interval: 200,
      timeoutMsg: "the new offer did not go to the junk folder on the server",
    });
    report.steps.movedAfterMs = Date.now() - started;
    // The notice, while it shows.
    const notice = $('[data-testid="notice"]');
    if (await notice.isExisting()) report.steps.notice = await notice.getText();

    await openView("junk");
    await openRow(subjectPart);
    const note = testId("spam-note");
    await note.waitForExist({ timeout: 20000 });
    assert.equal(await note.getAttribute("data-mark"), "moved");
    const mark = $('[data-testid="row-mark-spamMoved"]');
    assert.ok(await mark.isExisting(), "the row in the junk folder says why it is there");
    report.steps.movedNote = await note.getText();
    await shots("moved");

    const inboxBefore = await inServer("INBOX");
    await testId("spam-note-not-spam").click();
    await browser.waitUntil(async () => (await inServer("INBOX")) > inboxBefore, {
      timeout: 60000,
      interval: 300,
      timeoutMsg: "Not spam did not bring it back to the inbox on the server",
    });
    // Learned as not spam: back in the inbox it stays there.
    await browser.pause(3000);
    assert.equal(await inServer("INBOX"), inboxBefore + 1, "it stays in the inbox");
  });

  it("3: with writes off it only marks; Not spam removes the mark", async () => {
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: false });
    await openView("inbox");
    const junkBefore = await inServer(junk.path);
    const fresh = writeEmls("ro-offer", [offer(200, 0)]);
    await appendMessages("INBOX", fresh);
    const subjectPart = `${RUN}-200`;
    const row = await browser.$(`//*[@role="option" or @role="treeitem"][contains(., "${subjectPart}")]`);
    await row.waitForExist({ timeout: 120000 });
    await browser.waitUntil(async () => (await row.$('[data-testid="row-mark-spamSuspected"]').isExisting()), {
      timeout: 60000,
      interval: 300,
      timeoutMsg: "the row was not marked as suspected spam",
    });
    await openRow(subjectPart);
    const note = testId("spam-note");
    await note.waitForExist({ timeout: 20000 });
    assert.equal(await note.getAttribute("data-mark"), "suspected");
    assert.equal(await testId("spam-note-move").isExisting(), false, "no move without writes");
    report.steps.suspectedNote = await note.getText();
    await shots("suspected");
    assert.equal(await inServer(junk.path), junkBefore, "nothing moved on the server");

    await testId("spam-note-not-spam").click();
    await browser.waitUntil(async () => !(await testId("spam-note").isExisting()), {
      timeout: 20000,
      timeoutMsg: "the note did not go",
    });
    await browser.waitUntil(async () => !(await row.$('[data-testid="row-mark-spamSuspected"]').isExisting()), {
      timeout: 20000,
      timeoutMsg: "the mark did not go",
    });
    assert.deepEqual(await cspViolations(), []);
  });
});
