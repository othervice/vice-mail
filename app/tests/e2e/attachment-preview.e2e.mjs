// Stage 16 in the real app (decision 0036): the attachment preview (docs/stages/16, "Desktop-Tests
// für ... Anhangsvorschau"). In `specs` of wdio.conf.mjs after find-in-message.e2e.mjs (it only
// appends one message of its own to the inbox and reads it).
//
// 1. A mail with three attachments is appended to the inbox through IMAP: a PNG image, a PDF of
//    two pages and a plain-text file whose text looks like markup.
// 2. Image: "Preview photo.png" opens the dialog; the image is a PNG or JPEG `data:` URI the core
//    encoded (never the attachment's own bytes), its alt text is the name. Escape closes the
//    dialog and the focus returns to the button that opened it.
// 3. PDF: page 1 of 2, "Next page" shows page 2 (a new rasterised PNG), "Previous page" goes back.
// 4. Text: shown in a <pre> as text; no element of the text exists in the document.
// 5. No CSP violation in the main frame during all of it.
//
// Screenshots go to tests/e2e/screenshots/preview-*.png, the facts to preview-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, setConversations, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const SUBJECT = `Preview ${RUN}`;
const BOUNDARY = `vice-preview-${RUN}`;
let account;

/** A 2x2 PNG (red, green, blue, white). */
const PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEklEQVR4nGP4z8DAAMIM/4EAAB/uBfsL2WiLAAAAAElFTkSuQmCC";

/** A valid PDF of two pages, with a correct cross-reference table. */
function twoPagePdf() {
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 5 0 R /Resources << /Font << /F1 7 0 R >> >> >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 6 0 R /Resources << /Font << /F1 7 0 R >> >> >>",
  ];
  const page = (text) => {
    const stream = `BT /F1 24 Tf 40 100 Td (${text}) Tj ET`;
    return `<< /Length ${String(stream.length)} >>\nstream\n${stream}\nendstream`;
  };
  objects.push(page("Page one"), page("Page two"), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
  let body = "%PDF-1.4\n";
  const offsets = [];
  objects.forEach((object, i) => {
    offsets.push(Buffer.byteLength(body, "latin1"));
    body += `${String(i + 1)} 0 obj\n${object}\nendobj\n`;
  });
  const xref = Buffer.byteLength(body, "latin1");
  body += `xref\n0 ${String(objects.length + 1)}\n0000000000 65535 f \n`;
  for (const offset of offsets) body += `${String(offset).padStart(10, "0")} 00000 n \n`;
  body += `trailer\n<< /Size ${String(objects.length + 1)} /Root 1 0 R >>\nstartxref\n${String(xref)}\n%%EOF\n`;
  return Buffer.from(body, "latin1").toString("base64");
}

const TEXT = '<script>window.__previewRan = true</script><b id="preview-markup">not bold</b>\nSecond line of the notes.';

const wrap = (base64) => base64.replace(/.{1,76}/g, "$&\r\n").trimEnd();

function eml() {
  const part = (type, name, base64) =>
    [
      `--${BOUNDARY}`,
      `Content-Type: ${type}; name="${name}"`,
      `Content-Disposition: attachment; filename="${name}"`,
      "Content-Transfer-Encoding: base64",
      "",
      wrap(base64),
    ].join("\r\n");
  return [
    "Date: Mon, 23 Nov 2026 10:00:00 +0000",
    "From: Bob Baker <bob@vicemail.test>",
    "To: Alice Archer <alice@vicemail.test>",
    `Subject: ${SUBJECT}`,
    `Message-ID: <${SUBJECT.replace(/\s+/g, ".")}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    `Content-Type: multipart/mixed; boundary="${BOUNDARY}"`,
    "",
    `--${BOUNDARY}`,
    "Content-Type: text/plain; charset=utf-8",
    "",
    "Photos, the report and my notes are attached.",
    part("image/png", "photo.png", PNG_BASE64),
    part("application/pdf", "report.pdf", twoPagePdf()),
    part("text/plain; charset=utf-8", "notes.txt", Buffer.from(TEXT, "utf8").toString("base64")),
    `--${BOUNDARY}--`,
    "",
  ].join("\r\n");
}

const active = () =>
  browser.execute(() => {
    const element = document.activeElement;
    return element ? { label: element.getAttribute("aria-label"), testId: element.closest("[data-testid]")?.getAttribute("data-testid") ?? null } : null;
  });

/** What the open preview dialog shows. */
const dialogState = () =>
  browser.execute(() => {
    const dialog = document.querySelector('[data-testid="attachment-preview"]');
    if (!dialog) return null;
    const img = dialog.querySelector("img");
    return {
      title: dialog.querySelector("h2")?.textContent ?? null,
      busy: dialog.getAttribute("aria-busy"),
      img: img ? { alt: img.getAttribute("alt"), src: (img.getAttribute("src") ?? "").slice(0, 32), complete: img.complete, width: img.naturalWidth } : null,
      page: dialog.querySelector('[data-testid="preview-page"]')?.textContent ?? null,
      text: dialog.querySelector('[data-testid="preview-text"]')?.textContent ?? null,
      error: dialog.querySelector('[data-testid="preview-error"]')?.getAttribute("data-error-code") ?? null,
      markup: Boolean(document.getElementById("preview-markup")),
      ran: Boolean(window.__previewRan),
    };
  });

async function waitForDialog(check, message) {
  await browser.waitUntil(async () => check(await dialogState()), {
    timeout: 15000,
    timeoutMsg: `${message}: ${JSON.stringify(await dialogState())}`,
  });
  return dialogState();
}

async function clickButton(label) {
  const button = await browser.$(`//button[@aria-label="${label}" or normalize-space(.)="${label}"]`);
  // Stage 23 (decision 0084): an attachment card shows its buttons while the pointer is over it or
  // the focus in it. A real pointer over the card brings them out (`:focus-within` needs the
  // window to be the key window, which the test window is not).
  await button.waitForExist({ timeout: 10000 });
  const card = await browser.execute((el) => {
    const box = el.closest('[data-testid="attachment"]')?.getBoundingClientRect();
    return box ? { x: box.left + box.width / 2, y: box.top + box.height / 2, viewportHeight: innerHeight } : null;
  }, button);
  if (card) await coreCall("e2e_native_mouse_event", { kind: "move", x: card.x, y: card.y, viewportHeight: card.viewportHeight });
  await button.waitForClickable({ timeout: 10000 });
  await button.click();
}

describe("stage 16: attachment preview", () => {
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
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    // Message rows (as after account-sync.e2e.mjs), also when this spec runs on its own.
    await setConversations(false);
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vicemail-preview-"));
    const file = path.join(dir, "preview.eml");
    fs.writeFileSync(file, eml());
    await appendMessages("INBOX", [file]);
    await coreCall("retry_account", { accountId: account.id });

    const row = await browser.$(`//*[@role="option"][contains(., "${SUBJECT}")]`);
    await row.waitForExist({ timeout: 30000 });
    await row.click();
    await testId("attachments").waitForExist({ timeout: 20000 });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "preview-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("previews the image as a data URI the core encoded; Escape closes and the focus returns", async () => {
    await clickButton("Preview photo.png");
    const state = await waitForDialog((s) => s?.img?.complete && s.img.width > 0, "the image did not show");
    assert.equal(state.title, "Preview of photo.png");
    assert.equal(state.img.alt, "photo.png");
    assert.match(state.img.src, /^data:image\/(png|jpeg);base64,/);
    report.screenshots.push(await screenshot("preview-image"));
    report.steps.image = state;
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await dialogState()) === null, { timeoutMsg: "the dialog is still open" });
    assert.equal((await active())?.label, "Preview photo.png", "the focus is back on the button");
  });

  it("pages through the PDF, one rasterised page per step", async () => {
    await clickButton("Preview report.pdf");
    const first = await waitForDialog((s) => s?.page === "Page 1 of 2" && s.img?.complete, "page 1 did not show");
    assert.equal(first.img.alt, "report.pdf, page 1");
    assert.match(first.img.src, /^data:image\/png;base64,/);
    report.screenshots.push(await screenshot("preview-pdf-1"));
    await clickButton("Next page");
    const second = await waitForDialog((s) => s?.page === "Page 2 of 2" && s.img?.complete && s.busy !== "true", "page 2 did not show");
    assert.equal(second.img.alt, "report.pdf, page 2");
    report.screenshots.push(await screenshot("preview-pdf-2"));
    await clickButton("Previous page");
    await waitForDialog((s) => s?.page === "Page 1 of 2", "back to page 1 did not work");
    report.steps.pdf = { first, second };
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await dialogState()) === null, { timeoutMsg: "the dialog is still open" });
  });

  it("shows the text as text: nothing of it becomes an element or runs", async () => {
    await clickButton("Preview notes.txt");
    const state = await waitForDialog((s) => s?.text !== null && s?.text !== undefined, "the text did not show");
    assert.ok(state.text.includes('<b id="preview-markup">not bold</b>'), "the markup is shown as text");
    assert.equal(state.markup, false, "no element came from the text");
    assert.equal(state.ran, false, "no script of the text ran");
    report.screenshots.push(await screenshot("preview-text"));
    report.steps.text = state;
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await dialogState()) === null, { timeoutMsg: "the dialog is still open" });
    assert.deepEqual(await cspViolations(), []);
  });
});
