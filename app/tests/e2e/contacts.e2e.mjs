// Stage 17, block B in the real app (decision 0050): the contacts view. In `specs` of
// wdio.conf.mjs after attachment-preview.e2e.mjs (it appends three messages of its own to the
// inbox, moves two of them to Junk and back).
//
// 1. Two mails from "Pushy <offers@spam.example>" and one from Grace with Dave in Cc and a
//    Reply-To are appended to the inbox through IMAP; every address becomes a contact.
// 2. Search: by name, by address and by domain, in the contacts view.
// 3. Picture: a PNG dropped on the window (the e2e hook records the drop and announces it as the
//    system does) opens the cropper with the image the helper encoded; the arrow keys move it,
//    plus zooms, "Save picture" stores the square. The picture shows in the contact's page, the
//    contact list and the inbox row of Grace's mail; every image is a JPEG `data:` URI the core
//    wrote, never the file.
// 4. Mark as spam: Pushy is blocked, the two messages go to Junk on the server through the
//    outbox; "Undo" unblocks and brings them back to the inbox.
// 5. No CSP violation in the main frame during all of it.
//
// Screenshots go to tests/e2e/screenshots/contacts-*.png, the facts to contacts-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, invoke, openView, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, serverFolders } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const SPAM_ADDRESS = `pushy-${RUN}@spam.example`;
const GRACE_ADDRESS = `grace-${RUN}@example.org`;
let account;
let workDir;

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let k = 0; k < 8; k += 1) crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/** An 8-bit RGB PNG of `width` × `height`: a diagonal gradient with a dark square in the middle. */
function png(width, height) {
  const rows = [];
  for (let y = 0; y < height; y += 1) {
    const row = Buffer.alloc(1 + width * 3);
    for (let x = 0; x < width; x += 1) {
      const inner = Math.abs(x - width / 2) < width / 6 && Math.abs(y - height / 2) < height / 6;
      row[1 + x * 3] = inner ? 40 : Math.round((255 * x) / width);
      row[2 + x * 3] = inner ? 40 : Math.round((255 * y) / height);
      row[3 + x * 3] = inner ? 60 : 180;
    }
    rows.push(row);
  }
  const chunk = (type, body) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(body.length);
    const typed = Buffer.concat([Buffer.from(type, "latin1"), body]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(typed));
    return Buffer.concat([length, typed, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 2;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
    chunk("IDAT", zlib.deflateSync(Buffer.concat(rows))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function eml({ from, subject, cc = null, replyTo = null, id }) {
  return [
    "Date: Tue, 24 Nov 2026 09:00:00 +0000",
    `From: ${from}`,
    "To: Alice Archer <alice@vicemail.test>",
    ...(cc ? [`Cc: ${cc}`] : []),
    ...(replyTo ? [`Reply-To: ${replyTo}`] : []),
    `Subject: ${subject}`,
    `Message-ID: <${id}-${RUN}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    "Hello Alice.",
    "",
  ].join("\r\n");
}

const viewState = () =>
  browser.execute(() => {
    const view = document.querySelector('[data-testid="contacts-view"]');
    if (!view) return null;
    const detail = view.querySelector('[data-testid="contact-detail"]');
    const pictureOf = (root) => root?.querySelector('[data-testid="avatar-picture"]')?.getAttribute("src")?.slice(0, 32) ?? null;
    return {
      rows: [...view.querySelectorAll('[data-testid="contact-row"]')].map((row) => ({
        key: row.getAttribute("data-contact"),
        name: row.querySelector('[data-testid="contact-name"]')?.textContent ?? null,
        picture: pictureOf(row),
      })),
      detail: detail
        ? {
            name: detail.querySelector('[data-testid="contact-detail-name"]')?.textContent ?? null,
            email: detail.querySelector('[data-testid="contact-detail-email"]')?.textContent ?? null,
            picture: pictureOf(detail.querySelector('[data-testid="contact-picture-area"]')),
            spam: Boolean(detail.querySelector('[data-testid="contact-spam"]')),
          }
        : null,
      cropper: view.querySelector('[data-testid="picture-crop-area"]')?.getAttribute("data-crop") ?? null,
      cropperImage: view.querySelector('[data-testid="picture-editor"] image')?.getAttribute("href")?.slice(0, 32) ?? null,
    };
  });

async function waitForView(check, message, timeout = 20000) {
  await browser.waitUntil(async () => check(await viewState()), {
    timeout,
    timeoutMsg: `${message}: ${JSON.stringify(await viewState())}`,
  });
  return viewState();
}

async function search(text) {
  const field = await testId("contacts-search");
  await field.click();
  await browser.execute(() => {
    const input = document.querySelector('[data-testid="contacts-search"]');
    if (input instanceof HTMLInputElement) input.select();
  });
  await browser.keys(["Backspace"]);
  if (text) await field.addValue(text);
}

async function inboxCount() {
  const folders = await serverFolders();
  return { inbox: folders.INBOX?.messages ?? 0, junk: Object.entries(folders).find(([name]) => /junk|spam/i.test(name))?.[1]?.messages ?? 0 };
}

/** The "Show lists and system addresses" box, on or off. */
async function showAutomated(on) {
  const box = await testId("contacts-show-all");
  if ((await box.isSelected()) !== on) await box.click();
  await browser.waitUntil(async () => (await box.isSelected()) === on, { timeoutMsg: "the filter did not change" });
}

describe("stage 17: contacts", () => {
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
    await browser.waitUntil(
      async () => {
        const [a] = await coreCall("list_accounts");
        return a?.status.state === "idle" && a.status.lastSyncAt !== null;
      },
      { timeout: 240000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    // "Mark as spam" moves mail only where the account may write.
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    workDir = fs.mkdtempSync(path.join(os.tmpdir(), "vicemail-contacts-"));
    const files = [
      eml({ from: `Pushy <${SPAM_ADDRESS}>`, subject: `Win a prize ${RUN}`, id: "spam-1" }),
      eml({ from: `Pushy <${SPAM_ADDRESS}>`, subject: `Win again ${RUN}`, id: "spam-2" }),
      eml({
        from: `Grace Hopper <${GRACE_ADDRESS}>`,
        subject: `Compilers ${RUN}`,
        cc: `Dave Delta <dave-${RUN}@example.org>`,
        replyTo: `Grace's team <team-${RUN}@example.org>`,
        id: "grace",
      }),
    ].map((text, i) => {
      const file = path.join(workDir, `mail-${String(i)}.eml`);
      fs.writeFileSync(file, text);
      return file;
    });
    report.steps.serverBefore = await inboxCount();
    await appendMessages("INBOX", files);
    await coreCall("retry_account", { accountId: account.id });
    // Every address becomes a contact (the directory reads new mail in the background).
    await browser.waitUntil(
      async () => {
        const found = await coreCall("list_contacts", { query: RUN, cursor: null, limit: 50, filter: "all" });
        return found.items.length >= 4;
      },
      { timeout: 60000, interval: 500, timeoutMsg: "the addresses did not become contacts" },
    );
    report.steps.contacts = (await coreCall("list_contacts", { query: RUN, cursor: null, limit: 50, filter: "all" })).items.map((c) => ({
      email: c.email,
      kind: c.kind,
    }));
    await openView("contacts");
    await testId("contacts-view").waitForExist({ timeout: 10000 });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    if (workDir) fs.rmSync(workDir, { recursive: true, force: true });
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "contacts-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("finds contacts by name, address and domain", async () => {
    await search(`grace-${RUN}`);
    const byAddress = await waitForView((s) => s?.rows.length === 1 && s.rows[0].key === GRACE_ADDRESS, "grace by address");
    assert.equal(byAddress.detail?.email, GRACE_ADDRESS);
    await search("Grace Hopper");
    await waitForView((s) => s?.rows.some((r) => r.key === GRACE_ADDRESS), "grace by name");
    // A sender only seen in mail that is not from a person may be a list address.
    await showAutomated(true);
    await search("spam.example");
    const byDomain = await waitForView((s) => s?.rows.some((r) => r.key === SPAM_ADDRESS), "pushy by domain");
    report.steps.search = { byAddress: byAddress.rows, byDomain: byDomain.rows };
    // Dave (Cc) and the Reply-To address are contacts too.
    await search(`dave-${RUN}`);
    await waitForView((s) => s?.rows.length === 1, "dave from Cc");
    await search(`team-${RUN}`);
    await waitForView((s) => s?.rows.length === 1, "the Reply-To address");
    report.screenshots.push(await screenshot("contacts-search"));
  });

  it("sets a picture from a dropped file; it shows in the page, the list and the inbox", async () => {
    await search(`grace-${RUN}`);
    await waitForView((s) => s?.detail?.email === GRACE_ADDRESS, "grace open");
    const file = path.join(workDir, "grace.png");
    fs.writeFileSync(file, png(900, 600));
    const dropped = await invoke("e2e_drop_files", { paths: [file] });
    assert.ok(!("failure" in dropped), JSON.stringify(dropped));
    const cropping = await waitForView((s) => s?.cropper !== null && s.cropperImage !== null, "the cropper did not open");
    assert.match(cropping.cropperImage, /^data:image\/jpeg;base64,/);
    assert.equal(cropping.cropper, "150,0,600");
    // Keyboard: focus the square, zoom in, move right and down.
    await testId("picture-crop-area").click();
    await browser.keys(["+"]);
    await browser.keys(["ArrowRight"]);
    await browser.keys(["ArrowDown"]);
    const moved = await waitForView((s) => s?.cropper !== "150,0,600", "the square did not move");
    report.steps.crop = moved.cropper;
    report.screenshots.push(await screenshot("contacts-cropper"));
    await testId("picture-save").click();
    const saved = await waitForView((s) => s?.cropper === null && s.detail?.picture !== null, "the picture did not show");
    assert.match(saved.detail.picture, /^data:image\/jpeg;base64,/);
    const listed = await waitForView((s) => s?.rows.find((r) => r.key === GRACE_ADDRESS)?.picture != null, "no picture in the list");
    report.steps.picture = { detail: saved.detail, row: listed.rows.find((r) => r.key === GRACE_ADDRESS) };
    report.screenshots.push(await screenshot("contacts-picture"));
    const stored = await coreCall("get_contact", { email: GRACE_ADDRESS });
    assert.ok(stored.picture !== null, "the core keeps the picture");

    // The inbox row of Grace's mail shows it too.
    await openView("inbox");
    const row = await $(`//*[@role="option" or @role="treeitem"][contains(., "Compilers ${RUN}")]`);
    await row.waitForExist({ timeout: 20000 });
    await browser.waitUntil(async () => (await row.$('[data-testid="avatar-picture"]').isExisting()), {
      timeout: 15000,
      timeoutMsg: "the inbox row shows no picture",
    });
    const src = await row.$('[data-testid="avatar-picture"]').getAttribute("src");
    assert.match(src, /^data:image\/jpeg;base64,/);
    report.screenshots.push(await screenshot("contacts-inbox-picture"));
    await openView("contacts");
    await testId("contacts-view").waitForExist({ timeout: 10000 });
  });

  it("marks a sender as spam, moves its mail to Junk and undoes both", async () => {
    await showAutomated(true);
    await search(SPAM_ADDRESS);
    await waitForView((s) => s?.detail?.email === SPAM_ADDRESS, "pushy open");
    const before = await inboxCount();
    await testId("contact-mark-spam").click();
    const dialog = await testId("contact-spam-dialog");
    await dialog.waitForExist({ timeout: 10000 });
    assert.match(await dialog.getText(), /Move 2 messages to Junk too\?/);
    report.screenshots.push(await screenshot("contacts-spam-question"));
    await testId("confirm-dialog-confirm").click();
    await waitForView((s) => s?.detail?.spam === true, "not marked");
    // The notice says what happened; with the focus on it, it stays until the server is done.
    const undo = await $('[data-testid="notice-undo"]');
    await undo.waitForExist({ timeout: 15000 });
    const noticeText = await browser.execute(() => {
      const button = document.querySelector('[data-testid="notice-undo"]');
      button?.focus();
      return button?.closest('[data-testid="notice"]')?.textContent ?? null;
    });
    assert.match(noticeText ?? "", /2 messages moved to Junk/);
    const blocked = await coreCall("list_blocked_senders");
    assert.ok(blocked.some((b) => b.address === SPAM_ADDRESS), JSON.stringify(blocked));
    await browser.waitUntil(async () => (await inboxCount()).inbox === before.inbox - 2, {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the two messages did not leave the inbox on the server",
    });
    const moved = await inboxCount();
    report.steps.spam = { before, moved, notice: noticeText };
    report.screenshots.push(await screenshot("contacts-spam-done"));

    // Undo: unblocked, and the mail comes back to the inbox.
    await undo.waitForClickable({ timeout: 10000 });
    await undo.click();
    await browser.waitUntil(async () => !(await coreCall("list_blocked_senders")).some((b) => b.address === SPAM_ADDRESS), {
      timeout: 20000,
      timeoutMsg: "still blocked after undo",
    });
    await browser.waitUntil(async () => (await inboxCount()).inbox === before.inbox, {
      timeout: 60000,
      interval: 500,
      timeoutMsg: "the messages did not come back to the inbox",
    });
    await waitForView((s) => s?.detail?.spam === false, "still shown as spam");
    report.steps.undone = await inboxCount();
    assert.deepEqual(await cspViolations(), []);
  });
});
