// Stage 22 (decision 0074) in the real app: contacts in the search and search inside words. In
// `specs` of wdio.conf.mjs after contacts.e2e.mjs (it appends four messages of its own to the
// inbox); sets up alice itself and removes her again.
//
// 1. Four mails about Max Mustermann (from him, to him, him in Cc) and one "Muster" mail from Jan
//    are appended to the inbox through IMAP; the addresses become contacts.
// 2. Typing the start of Max's address shows him on top of the search field (combobox); the arrow
//    key and Enter choose him: the field says `contact:<address>`, every account is searched, and
//    the results are the mail from, to and in copy of him, not Jan's.
// 3. `uster` finds the "Muster" mail and marks "uster" inside the word.
// 4. German, light and dark: the list of contacts as the user sees it (screenshots).
// 5. No CSP violation in the main frame during all of it.
//
// Screenshots go to tests/e2e/screenshots/search-contacts-*.png, the facts to
// search-contacts-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, SCREENSHOT_DIR, setConversations, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
const RUN = Date.now().toString(36);
const MAX = `max.mustermann-${RUN}@firma.example`;
const JAN = `jan-${RUN}@firma.example`;
let workDir;
/** Conversation view, theme and language as the previous specs left them; put back at the end. */
let conversations = true;
let preferences = { theme: "light", language: "en" };

function eml({ from, to = "Alice Archer <alice@vicemail.test>", cc = null, subject, id, minutes }) {
  return [
    `Date: Wed, 25 Nov 2026 10:${String(minutes).padStart(2, "0")}:00 +0000`,
    `From: ${from}`,
    `To: ${to}`,
    ...(cc ? [`Cc: ${cc}`] : []),
    `Subject: ${subject}`,
    `Message-ID: <${id}-${RUN}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    "Hallo zusammen.",
    "",
  ].join("\r\n");
}

/** The subjects of the search results, top to bottom, with the marked parts of each. */
const results = () =>
  browser.execute((run) => {
    const rows = [...document.querySelectorAll('[data-testid="message-row"]')];
    return rows
      .map((row) => ({
        subject: row.querySelector('[data-testid="row-subject"]')?.textContent ?? "",
        marks: [...row.querySelectorAll('[data-testid="highlight"]')].map((m) => m.textContent),
      }))
      .filter((r) => r.subject.includes(run));
  }, RUN);

const field = () =>
  browser.execute(() => {
    const input = document.querySelector('[data-testid="search-input"]');
    return input instanceof HTMLInputElement
      ? {
          value: input.value,
          role: input.getAttribute("role"),
          expanded: input.getAttribute("aria-expanded"),
          controls: input.getAttribute("aria-controls"),
          active: input.getAttribute("aria-activedescendant"),
          focused: document.activeElement === input,
        }
      : null;
  });

const suggestions = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="search-contact-option"]')].map((o) => ({
      id: o.id,
      label: o.getAttribute("aria-label"),
      selected: o.getAttribute("aria-selected"),
    })),
  );

/** Types into the search field (opening it with Cmd+F if needed), replacing what it holds. */
async function typeSearch(text) {
  if (!(await field())) await browser.keys(["Meta", "f"]);
  const input = await testId("search-input");
  await input.waitForExist({ timeout: 5000 });
  await input.click();
  await browser.execute(() => {
    const el = document.querySelector('[data-testid="search-input"]');
    if (el instanceof HTMLInputElement) el.select();
  });
  await browser.keys(["Backspace"]);
  await input.addValue(text);
}

/** A screenshot that keeps the focus in the field (the shared helper blurs it, closing the list). */
async function shot(name) {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  await browser.execute(() => {
    for (const animation of document.getAnimations()) animation.finish();
  });
  await browser.pause(150);
  const file = path.join(SCREENSHOT_DIR, `${name}.png`);
  await browser.saveScreenshot(file);
  report.screenshots.push(file);
}

describe("stage 22: contacts in the search, search inside words", () => {
  before(async () => {
    await attach();
    preferences = await coreCall("get_preferences");
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    const account = await coreCall("add_account", {
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
    workDir = fs.mkdtempSync(path.join(os.tmpdir(), "vicemail-search-contacts-"));
    const files = [
      eml({ from: `Max Mustermann <${MAX}>`, subject: `Projektplan ${RUN}`, id: "from-max", minutes: 1 }),
      eml({
        from: "Alice Archer <alice@vicemail.test>",
        to: `Max Mustermann <${MAX}>`,
        subject: `Re: Projektplan ${RUN}`,
        id: "to-max",
        minutes: 2,
      }),
      eml({ from: `Eve <eve-${RUN}@example.org>`, cc: `Max Mustermann <${MAX}>`, subject: `Rundschreiben ${RUN}`, id: "cc-max", minutes: 3 }),
      eml({ from: `Jan <${JAN}>`, subject: `Muster für die Werbung ${RUN}`, id: "jan", minutes: 4 }),
    ].map((text, i) => {
      const file = path.join(workDir, `mail-${String(i)}.eml`);
      fs.writeFileSync(file, text);
      return file;
    });
    await appendMessages("INBOX", files);
    await coreCall("retry_account", { accountId: account.id });
    // One row per message: the results are compared with the mails one by one.
    conversations = (await coreCall("get_ui_preferences")).conversations;
    await setConversations(false);
    await browser.waitUntil(
      async () => {
        const found = await coreCall("list_contacts", { query: `max.mustermann-${RUN}`, cursor: null, limit: 3, filter: "all" });
        return found.items.length === 1;
      },
      { timeout: 60000, interval: 500, timeoutMsg: "Max did not become a contact" },
    );
    await testId("message-list").waitForExist({ timeout: 20000 });
  });

  after(async () => {
    await coreCall("set_preferences", { preferences }).catch(() => {});
    await setConversations(conversations).catch(() => {});
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    if (workDir) fs.rmSync(workDir, { recursive: true, force: true });
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "search-contacts-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("offers the contact for a part of the address and shows all mail from and to them", async () => {
    // Only this folder: choosing a contact searches every account anyway.
    await browser.keys(["Meta", "f"]);
    await testId("search-scope").waitForExist({ timeout: 5000 });
    await testId("search-scope").selectByAttribute("value", "mailbox");
    await typeSearch(`max.mustermann-${RUN}@fir`);
    await browser.waitUntil(async () => (await suggestions()).length > 0, { timeout: 10000, timeoutMsg: "no contact offered" });
    const offered = await suggestions();
    report.steps.offered = offered;
    assert.equal(offered.length, 1, JSON.stringify(offered));
    assert.match(offered[0].label, new RegExp(MAX.replace(/[.]/g, "\\.")));
    const before = await field();
    report.steps.combobox = before;
    assert.equal(before.role, "combobox");
    assert.equal(before.expanded, "true");
    assert.ok(before.controls, "the field names its list");
    // The full-text results stay below while the list is open.
    await browser.waitUntil(async () => (await results()).length > 0, { timeout: 10000, timeoutMsg: "no full-text results below" });

    await browser.keys(["ArrowDown"]);
    await browser.waitUntil(async () => (await field()).active === offered[0].id, { timeout: 5000, timeoutMsg: "the arrow did not move" });
    await browser.keys(["Enter"]);
    await browser.waitUntil(async () => (await field()).value === `contact:${MAX}`, {
      timeout: 5000,
      timeoutMsg: `the field did not take the contact: ${JSON.stringify(await field())}`,
    });
    assert.equal(await testId("search-scope").getValue(), "all");
    await browser.waitUntil(async () => (await results()).length === 3, {
      timeout: 15000,
      timeoutMsg: `not the three mails with Max: ${JSON.stringify(await results())}`,
    });
    const found = await results();
    report.steps.contactResults = found;
    assert.deepEqual(
      found.map((r) => r.subject),
      [`Rundschreiben ${RUN}`, `Re: Projektplan ${RUN}`, `Projektplan ${RUN}`],
    );
    const chips = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="search-chip"]')].map((c) => c.firstChild?.textContent ?? ""),
    );
    assert.deepEqual(chips, [`Contact: ${MAX}`]);
    assert.equal((await field()).expanded, "false");
  });

  it("finds Muster for uster and marks it inside the word", async () => {
    await typeSearch(`uster ${RUN}`);
    await browser.keys(["Enter"]);
    await browser.waitUntil(async () => (await results()).some((r) => r.subject.startsWith("Muster")), {
      timeout: 15000,
      timeoutMsg: `the Muster mail was not found: ${JSON.stringify(await results())}`,
    });
    const found = await results();
    report.steps.insideWords = found;
    const muster = found.find((r) => r.subject.startsWith("Muster"));
    assert.ok(muster.marks.includes("uster"), JSON.stringify(muster));
    // The Mustermann mails match through their addresses too.
    assert.equal(found.length, 4, JSON.stringify(found));
    const core = await coreCall("search", {
      request: { query: `uster ${RUN}`, scope: { kind: "all" }, cursor: null, limit: 50, typing: false, monthFirst: false },
    });
    assert.equal(core.hits.length, 4);
    // "uster" inside "Muster", then the run's word.
    assert.deepEqual(core.hits.find((h) => h.message.subject.startsWith("Muster")).highlights.subject[0], { start: 1, end: 6 });
  });

  for (const theme of ["light", "dark"]) {
    it(`shows the contacts in German, ${theme}`, async () => {
      await coreCall("set_preferences", { preferences: { theme, language: "de" } });
      await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
        timeout: 5000,
      });
      await typeSearch(`muster`);
      await browser.waitUntil(async () => (await suggestions()).length > 0, { timeout: 10000, timeoutMsg: "no contact offered" });
      await browser.keys(["ArrowDown"]);
      const heading = await browser.execute(() => document.querySelector('[data-testid="search-contacts"] p')?.textContent ?? null);
      assert.equal(heading, "Kontakte");
      await shot(`search-contacts-de-${theme}`);
      await browser.keys(["Enter"]);
      await browser.waitUntil(async () => (await field()).value.startsWith("kontakt:"), { timeout: 5000, timeoutMsg: "no German filter" });
      await browser.waitUntil(async () => (await results()).length > 0, { timeout: 15000 });
      await shot(`search-contacts-de-${theme}-results`);
      await typeSearch(`uster ${RUN}`);
      await browser.keys(["Enter"]);
      await browser.waitUntil(async () => (await results()).some((r) => r.subject.startsWith("Muster")), { timeout: 15000 });
      await shot(`search-inside-words-de-${theme}`);
    });
  }

  it("has no CSP violation", async () => {
    const violations = await cspViolations();
    report.steps.csp = violations;
    assert.deepEqual(violations, []);
  });
});
