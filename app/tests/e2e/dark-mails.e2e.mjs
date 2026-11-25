// Stage 21, items 8 and 9 (decisions 0066 and 0067) in the real app against the local test
// server. alice's account is added through the core with writes on (she sends to herself).
//
// A. A colourful newsletter in the dark theme: the core adapts it (frame `data-colors="adapted"`),
//    its canvas is the dark card, every marked text is readable on what lies behind it (at least
//    4.5:1, measured in the frame); "Original colours" shows the white page and back; the light
//    theme shows it as written.
// B. The seed's newsletter "Example News: issue #1" on a dark background with readable text.
// C. The composer in the dark theme: text pasted with the theme's colours is sent without any text
//    or background colour and with `color-scheme: light dark`; alice's light theme shows it with
//    the page's own dark text.
//
// Screenshots: tests/e2e/screenshots/stage21-*.png; facts in stage21-dark-mails-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, luminance, openView, screenshot, SCREENSHOT_DIR, setConversations, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const ROW = ':is([role="option"], [role="treeitem"])';
let account;
let preferencesBefore = null;
let conversationsBefore = null;

const NEWSLETTER_SUBJECT = `Autumn colours ${RUN}`;

/** A newsletter with the usual colours: grey page, white card, dark band, yellow note, blue button. */
const NEWSLETTER_HTML = `<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
.card{background-color:#ffffff;color:#222222;border:1px solid #e5e5e5}
.band{background:#1f2a44;color:#ffffff}
.note{background:#fff3cd;color:#664d03;border-left:4px solid #ffc107}
.button{background:#1a73e8;color:#ffffff;padding:10px 18px;text-decoration:none;border-radius:4px}
.footer{color:#888888;font-size:12px}
.soft{background:linear-gradient(#ffffff,#eef2ff);color:#333333}
</style></head>
<body bgcolor="#f4f4f4" style="margin:0;font-family:Arial,sans-serif">
<table width="600" align="center" cellpadding="0" cellspacing="0" class="card">
<tr><td class="band" style="padding:20px"><h1 id="band-title" style="margin:0">Autumn news</h1></td></tr>
<tr><td style="padding:20px">
<p id="intro">Here is what happened this week. <a id="read-more" href="https://news.example/read" style="color:#1a0dab">Read more</a></p>
<p class="note" id="note" style="padding:10px">Only this week: <font color="#c00000">20 % off</font> everything.</p>
<div class="soft" id="soft" style="padding:12px"><span id="soft-text">A soft card with a gradient.</span></div>
<p style="padding-top:16px"><a class="button" id="button" href="https://news.example/shop">Visit the shop</a></p>
<p class="footer" id="footer">You receive this mail because you subscribed.</p>
</td></tr></table></body></html>`;

function writeEml(name, lines) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vicemail-s21-"));
  const file = path.join(dir, name);
  fs.writeFileSync(file, lines.join("\r\n"));
  return file;
}

async function setTheme(theme) {
  const current = await coreCall("get_preferences");
  await coreCall("set_preferences", { preferences: { ...current, theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeout: 10000,
    timeoutMsg: `the theme did not switch to ${theme}`,
  });
}

async function openBySearch(query, subject) {
  await browser.keys(["Meta", "f"]);
  const input = await testId("search-input");
  await input.waitForExist({ timeout: 5000 });
  await input.setValue(query);
  let id = null;
  await browser.waitUntil(
    async () => {
      id = await browser.execute(
        (wanted, row) =>
          [...document.querySelectorAll(`[data-testid="message-list"] ${row}`)].find((el) => (el.textContent ?? "").includes(wanted))?.id ?? null,
        subject,
        ROW,
      );
      return Boolean(id);
    },
    { timeout: 60000, interval: 500, timeoutMsg: `no hit for ${subject}` },
  );
  await (await $(`[id="${id}"]`)).click();
  await testId("reading-pane").waitForExist({ timeout: 10000 });
  // Rows are `<prefix>-<message id>`.
  return Number(id.split("-").pop());
}

/**
 * Opens the row with `subject` in the open list (new mail is on top) and returns its message id.
 * Not through the search: new mail arriving while the search shows its hits refreshes them.
 */
async function openRow(subject) {
  const selector = `//*[@role="option" or @role="treeitem"][contains(., "${subject}")]`;
  // A newsletter of today shares today's "Newsletters" bundle with every other newsletter of
  // today (in the full run stage20.e2e.mjs leaves one from the same sender), and a bundle of
  // more than one member is a closed row: unfold closed bundles while waiting for the row.
  await browser.waitUntil(
    async () => {
      if (await (await browser.$(selector)).isExisting()) return true;
      await browser.execute(() => {
        for (const bundle of document.querySelectorAll('[data-testid="message-list"] [data-testid="bundle-row"][data-expanded="false"]')) bundle.click();
      });
      return false;
    },
    { timeout: 60000, interval: 500, timeoutMsg: `no row for "${subject}" in the list` },
  );
  const row = await browser.$(selector);
  const id = await row.getAttribute("id");
  await row.click();
  await testId("reading-pane").waitForExist({ timeout: 20000 });
  return Number(String(id).split("-").pop());
}

/** Answers the one-time "Check for updates automatically?" card so it covers no screenshot. */
async function dismissUpdateQuestion() {
  const no = await $("button=No, thanks");
  if (await no.isExisting()) await no.click();
}

/** The mail's frame at the top of the reading pane, with the switches above it. */
async function showMail() {
  await dismissUpdateQuestion();
  await browser.execute(() => {
    const anchor = document.querySelector('[data-testid="colors-original"]') ?? document.querySelector('[data-testid="mail-frame"]');
    anchor?.scrollIntoView({ block: "start" });
  });
  await browser.pause(200);
}

async function closeSearch() {
  const close = await testId("search-close");
  if (await close.isExisting()) await close.click();
}

async function frameColors(colors) {
  await browser.waitUntil(
    async () =>
      browser.execute((wanted) => {
        const frame = document.querySelector('[data-testid="mail-frame"]');
        const doc = frame?.contentDocument;
        if (frame?.dataset.colors !== wanted || doc?.readyState !== "complete" || frame.offsetHeight === 0) return false;
        // The loaded document, not the one before the new srcdoc: its scheme as the core set it.
        const dark = doc.defaultView.getComputedStyle(doc.documentElement).colorScheme.includes("dark");
        return dark === (wanted !== "original");
      }, colors),
    { timeout: 20000, timeoutMsg: `the frame does not show the colours ${colors}` },
  );
  await browser.pause(300);
}

/**
 * Inside the mail frame: the canvas, and for each element id its text colour and the first opaque
 * background behind it (its own or an ancestor's, the page's at the end), as computed. A gradient
 * counts with its first stop.
 */
async function measureFrame(ids) {
  let measured = null;
  await browser.waitUntil(
    async () => {
      measured = await measureOnce(ids);
      return measured !== null;
    },
    { timeout: 10000, timeoutMsg: "the mail frame could not be measured" },
  );
  return measured;
}

function measureOnce(ids) {
  return browser.execute((wanted) => {
    const frame = document.querySelector('[data-testid="mail-frame"]');
    const doc = frame?.contentDocument;
    if (!doc) return null;
    const view = doc.defaultView;
    const opaque = (color) => color && color !== "transparent" && !/rgba\([^)]*,\s*0\)$/.test(color);
    const behind = (el) => {
      for (let node = el; node; node = node.parentElement) {
        const style = view.getComputedStyle(node);
        if (opaque(style.backgroundColor)) return style.backgroundColor;
        const stop = /gradient\((?:[^,]*,)?\s*(rgba?\([^)]*\))/.exec(style.backgroundImage)?.[1];
        if (stop) return stop;
      }
      return view.getComputedStyle(doc.documentElement).backgroundColor;
    };
    const out = {
      canvas: view.getComputedStyle(doc.documentElement).backgroundColor,
      body: view.getComputedStyle(doc.body).backgroundColor,
      elements: {},
    };
    for (const id of wanted) {
      const el = doc.getElementById(id);
      if (!el) continue;
      out.elements[id] = { color: view.getComputedStyle(el).color, background: behind(el) };
    }
    return out;
  }, ids);
}

const contrast = (a, b) => {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
};

describe("stage 21: mails in the dark theme, no colours in sent mail", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await setTheme("dark");
    conversationsBefore = (await coreCall("get_ui_preferences")).conversations;
    await setConversations(false);
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
    await coreCall("set_update_auto_check", { enabled: false }).catch(() => undefined);
    const reading = await coreCall("get_reading_preferences");
    report.steps.defaultDarkMails = reading.darkMails;
    assert.equal(reading.darkMails, "adapted", "adapted is the default");
    await coreCall("set_reading_preferences", { preferences: { ...reading, markReadOnOpen: "never" } });
    const settings = await coreCall("get_compose_settings");
    await coreCall("set_compose_settings", { settings: { ...settings, sendDelaySeconds: 0, attachmentReminder: false } });
    const file = writeEml("autumn.eml", [
      `Date: ${new Date().toUTCString()}`,
      "From: Autumn Shop <news@shop.example>",
      "To: Alice Archer <alice@vicemail.test>",
      `Subject: ${NEWSLETTER_SUBJECT}`,
      `Message-ID: <autumn-${RUN}@shop.example>`,
      "MIME-Version: 1.0",
      "Content-Type: text/html; charset=utf-8",
      "",
      NEWSLETTER_HTML,
    ]);
    await appendMessages("INBOX", [file]);
    await openView("inbox");
  });

  after(async () => {
    await closeSearch().catch(() => undefined);
    if (conversationsBefore !== null) await setConversations(conversationsBefore).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "stage21-dark-mails-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("A: adapts a colourful newsletter in the dark theme; “Original colours” shows the white page", async () => {
    const ids = ["band-title", "intro", "read-more", "note", "soft-text", "button", "footer"];
    await openView("inbox");
    await openRow(NEWSLETTER_SUBJECT);
    await frameColors("adapted");
    const adapted = await measureFrame(ids);
    report.steps.adapted = adapted;
    // The page and the card are dark; the canvas is the dark card's colour.
    assert.ok(luminance(adapted.canvas) < 0.03, `canvas ${adapted.canvas}`);
    assert.ok(luminance(adapted.body) < 0.05, `page ${adapted.body}`);
    const ratios = {};
    for (const [id, { color, background }] of Object.entries(adapted.elements)) ratios[id] = Number(contrast(color, background).toFixed(2));
    report.steps.adaptedContrast = ratios;
    for (const id of ids.filter((i) => i !== "footer")) {
      assert.ok(ratios[id] >= 4.5, `${id}: ${String(ratios[id])} (${JSON.stringify(adapted.elements[id])})`);
    }
    // The footer was 3.5:1 on white; it may not get worse.
    assert.ok(ratios.footer >= 3.5, `footer: ${String(ratios.footer)}`);
    assert.equal(await testId("colors-original").getAttribute("aria-pressed"), "false");
    await showMail();
    report.screenshots.push(await screenshot("stage21-newsletter-dark-adapted"));

    await testId("colors-original").click();
    await frameColors("original");
    const original = await measureFrame(ids);
    report.steps.original = original;
    assert.ok(luminance(original.elements.intro.background) > 0.9, `original card ${original.elements.intro.background}`);
    assert.equal(await testId("colors-original").getAttribute("aria-pressed"), "true");
    await showMail();
    report.screenshots.push(await screenshot("stage21-newsletter-dark-original"));

    await testId("colors-original").click();
    await frameColors("adapted");

    // The light theme shows it as written, without the toggle.
    await setTheme("light");
    await frameColors("original");
    assert.equal(await testId("colors-original").isExisting(), false);
    await showMail();
    report.screenshots.push(await screenshot("stage21-newsletter-light"));
    await setTheme("dark");
    await frameColors("adapted");
    assert.deepEqual(await cspViolations(), [], "CSP violations while reading");
  });

  it("B: shows the seed's newsletter on a dark background with readable text", async () => {
    await openBySearch('subject:"Example News: issue #1"', "Example News: issue #1");
    await frameColors("adapted");
    const seed = await browser.execute(() => {
      const doc = document.querySelector('[data-testid="mail-frame"]')?.contentDocument;
      if (!doc) return null;
      const view = doc.defaultView;
      const table = doc.querySelector("table");
      const heading = doc.querySelector("h1");
      const footer = [...doc.querySelectorAll("p")].at(-1);
      return {
        page: view.getComputedStyle(doc.body).backgroundColor,
        card: table ? view.getComputedStyle(table).backgroundColor : null,
        heading: heading ? view.getComputedStyle(heading).color : null,
        footer: footer ? view.getComputedStyle(footer).color : null,
      };
    });
    report.steps.seedNewsletter = seed;
    assert.ok(luminance(seed.page) < 0.05 && luminance(seed.card) < 0.05, JSON.stringify(seed));
    assert.ok(contrast(seed.heading, seed.card) >= 7, JSON.stringify(seed));
    assert.ok(contrast(seed.footer, seed.card) >= 3.5, JSON.stringify(seed));
    await showMail();
    report.screenshots.push(await screenshot("stage21-seed-newsletter-dark"));
    await closeSearch();
  });

  it("C: sends text pasted with the dark theme's colours without any colour, declaring both schemes", async () => {
    const subject = `Pasted from dark ${RUN}`;
    await openView("inbox");
    await $('button[aria-label="New message"]').click();
    await testId("composer-subject").waitForExist({ timeout: 20000 });
    const to = await $('[data-testid="composer-to"] [data-testid="recipient-input"]');
    await to.setValue(`${ALICE.email},`);
    await testId("composer-subject").setValue(subject);
    const html = await testId("composer-format-html");
    if (await html.isExisting()) await html.click();
    const editor = await testId("composer-rich-body");
    await editor.waitForExist({ timeout: 20000 });
    await editor.click();
    // What WebKit puts on the clipboard when text is copied from the dark reading pane.
    await browser.execute((el) => {
      const data = new DataTransfer();
      data.setData(
        "text/html",
        '<p style="color: rgb(230, 237, 243); background-color: rgb(33, 40, 48);">Light text copied from the dark theme</p><pre style="color:#e6edf3"><code>let answer = 42;</code></pre>',
      );
      data.setData("text/plain", "Light text copied from the dark theme");
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
    }, editor);
    await browser.waitUntil(async () => (await editor.getText()).includes("Light text copied"), { timeoutMsg: "the paste never arrived" });
    await dismissUpdateQuestion();
    report.screenshots.push(await screenshot("stage21-composer-dark-pasted"));
    await testId("composer-send").click();
    await browser.waitUntil(
      async () => (await coreCall("list_outgoing", { accountId: account.id, before: null, limit: 50 })).some((m) => m.subject === subject && m.state === "sent"),
      { timeout: 60000, interval: 500, timeoutMsg: `${subject} was not sent` },
    );

    // alice receives it in her inbox: the HTML part as it went out.
    await openView("inbox");
    const messageId = await openRow(subject);
    // It declares both schemes: nothing is recoloured, it lies on the dark canvas.
    await frameColors("ownDark");
    const pane = await browser.execute(() => {
      const frame = document.querySelector('[data-testid="mail-frame"]');
      return { colors: frame?.dataset.colors ?? null };
    });
    report.steps.received = pane;
    assert.equal(pane.colors, "ownDark");
    const source = await coreCall("get_message_source", { messageId });
    // The HTML part as it went out (quoted-printable soft breaks joined).
    const text = source.text.replace(/=\r?\n/g, "").replace(/=3D/g, "=");
    const htmlPart = text.slice(text.toLowerCase().indexOf("<!doctype html"));
    report.steps.sentHtml = htmlPart.slice(0, 1500);
    assert.match(htmlPart, /<meta name="color-scheme" content="light dark">/);
    assert.match(htmlPart, /<meta name="supported-color-schemes" content="light dark">/);
    const withoutLines = htmlPart.replace(/border(?:-left)?:[^;"]*#cccccc/g, "");
    assert.ok(!/color\s*:|background|bgcolor|<font/i.test(withoutLines), `a colour in the sent HTML: ${htmlPart}`);
    await showMail();
    report.screenshots.push(await screenshot("stage21-composer-received-dark"));
    await setTheme("light");
    await frameColors("original");
    const light = await browser.execute(() => {
      const doc = document.querySelector('[data-testid="mail-frame"]')?.contentDocument;
      const p = doc?.querySelector("p");
      return p ? { color: doc.defaultView.getComputedStyle(p).color, page: doc.defaultView.getComputedStyle(doc.documentElement).backgroundColor } : null;
    });
    report.steps.receivedLight = light;
    // In the light theme the page's own dark text, never the light text it was copied in.
    assert.ok(light && luminance(light.color) < 0.1, JSON.stringify(light));
    await showMail();
    report.screenshots.push(await screenshot("stage21-composer-received-light"));
    await setTheme("dark");
  });
});
