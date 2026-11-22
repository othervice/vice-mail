// Stage 3 acceptance in the real app: reading mail. Runs last, after account-sync.e2e.mjs, on the
// server wdio.conf.mjs seeded (10,000 messages for alice); the account is added through the core
// (the setup form is stage 2's test). Before that, the spec puts two folders on the server:
//
// - "Hostile": the 19 files of testmail/corpus/hostile/ unchanged (IMAP APPEND), plus a control
//   mail of another sender with external images. All their external references point at the
//   counter server (support/counter-server.mjs), which counts every TCP connection.
// - "Broken": server-side copies of every seed message with `X-Vicemail-Seed: broken/…`.
//
// 1. Opening a stored (prefetched) message: click on the row until head and body are in the
//    page, median and p95 over 40 messages of the inbox.
// 2. Every corpus mail in the reading pane: no script runs (markers in the app window and in the
//    frame, the command recorder of the e2e build), the counter sees no connection, the app does
//    not navigate, the main frame reports no CSP violation. Every link is clicked with a real
//    mouse click (`e2e_native_mouse`): no navigation, no connection, the link card shows the real
//    target. "Open" hands exactly that target to the recording opener of the e2e build.
// 3. "Always load from <sender>": the core loads that sender's images (the counter sees them,
//    and only them), another sender stays blocked; "Load once" loads once.
// 4. Attachments open only on click, executables after a warning; the copy is byte-identical.
// 5. Every broken seed message shows readable content or a clear notice; the app keeps running.
// 6. With the server stopped, prefetched messages open; one that is not stored says so.
// 7. Positive controls for every detector the checks above rely on.
//
// Screenshots (light and dark) go to tests/e2e/screenshots/reading-*.png, the facts of the run to
// tests/e2e/screenshots/reading-report.json.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, dataDir, invoke, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { counter, startCounter } from "./support/counter-server.mjs";
import { ALICE, REPO, SERVER, appendMessages, attachmentDigests, copySeedMessages, testmail } from "./support/testmail.mjs";

const report = { failures: [] };
/** Failures of the running test (collected, so one run reports every broken vector at once). */
const failures = [];
const fail = (where, message) => {
  failures.push(`${where}: ${message}`);
  report.failures.push(`${where}: ${message}`);
};

const HOSTILE = "Hostile";
const BROKEN = "Broken";
const CORPUS_DIR = path.join(REPO, "testmail", "corpus", "hostile");
const CORPUS_SENDER = "corpus@hostile.example";
const APP_TITLE = "Vice Mail";
/** Time for late loads (lazy images, preconnects) after a mail is shown or a link clicked. */
const SETTLE_MS = 1500;
const CLICK_SETTLE_MS = 800;
/** Opening must take less than this (docs/stages/03-nachricht-lesen.md, acceptance). */
const OPEN_LIMIT_MS = 100;
const TIMING_SAMPLES = 40;
const PLACEHOLDER = /^https:\/\/link\.invalid\/([0-9a-f]{32})\/(\d+)$/;

/** Real targets of links the corpus names, in document order (the core must show these). */
const EXPECTED_TARGETS = {
  "link-attributes.eml": [
    "http://127.0.0.1:18999/v/link-plain/a",
    "http://127.0.0.1:18999/v/link-target-top/a",
    "http://127.0.0.1:18999/v/link-target-blank/a",
    "http://127.0.0.1:18999/v/link-ping/a",
    "http://127.0.0.1:18999/v/link-download/a",
    "mailto:someone@example.test?subject=spike",
    // `vmlink:42` is no link any more: an unknown scheme becomes text (an <a> without href).
    "https://link.invalid/42",
    "http://127.0.0.1:18999/v/link-rel/a",
    "http://127.0.0.1:18999/v/link-nested/a",
  ],
  // The core shows the host as the network sees it (punycode), like the WHATWG URL parser.
  "deceptive-links.eml": [
    "https://evil.example/login",
    "https://xn--bnk-sna.example/",
    "https://bank.example@evil.example/",
    "http://192.0.2.7/login",
    "https://p\u0430ypal.example/",
    "https://xn--80ak6aa92e.com/",
  ].map((href) => new URL(href).href),
  "text-plain-markup.eml": ["https://example.com/ok", "mailto:bob@example.com"],
};
/** Links that are opened through "Open" and must reach the opener exactly like this. */
const OPEN_THROUGH_OPENER = {
  "link-attributes.eml": ["http://127.0.0.1:18999/v/link-plain/a", "mailto:someone@example.test?subject=spike"],
  "deceptive-links.eml": ["https://evil.example/login"],
  "text-plain-markup.eml": ["https://example.com/ok"],
};
/** Vectors of tracking-pixels.eml whose images the core may load once the sender is allowed. */
const TRACKING_IMAGE_VECTORS = ["body-background", "img-src", "img-srcset", "picture-source", "table-background", "td-background", "protocol-relative"];

const OTHER_SENDER = "news@other-sender.example";
const CONTROL_SUBJECT = "Control: images of another sender";
/** A mail of a sender the test never allows, with two external images. Written by the test. */
const CONTROL_MAIL = [
  "Date: Sun, 22 Nov 2026 13:00:00 +0000",
  `From: Other Sender <${OTHER_SENDER}>`,
  "To: Alice Archer <alice@vicemail.test>",
  `Subject: ${CONTROL_SUBJECT}`,
  "Message-ID: <control.other-sender@e2e.vicemail.test>",
  "MIME-Version: 1.0",
  "Content-Type: text/html; charset=utf-8",
  "",
  "<!DOCTYPE html><html><body><p>SAFE-CONTENT-VISIBLE</p>",
  '<p><img src="http://127.0.0.1:18999/v/other-sender/a.png" width="40" height="40" alt="a"></p>',
  '<table background="http://127.0.0.1:18999/v/other-sender/b.png"><tr><td>other sender</td></tr></table>',
  "</body></html>",
  "",
].join("\r\n");

// --- files and core ----------------------------------------------------------------------------

/** Header fields of an .eml file (unfolded), lower-case names. */
function headersOf(file) {
  const raw = fs.readFileSync(file, "latin1");
  const head = raw.split(/\r?\n\r?\n/, 1)[0].replace(/\r?\n[ \t]+/g, " ");
  const out = {};
  for (const line of head.split(/\r?\n/)) {
    const at = line.indexOf(":");
    if (at > 0) out[line.slice(0, at).toLowerCase()] ??= line.slice(at + 1).trim();
  }
  return out;
}

/** `X-Corpus-Check: links=6; remote=0; …` as an object of numbers. */
function corpusCheck(headers) {
  const out = {};
  for (const part of (headers["x-corpus-check"] ?? "").split(";")) {
    const [key, value] = part.split("=").map((s) => s.trim());
    if (key && value !== undefined) out[key] = /^\d+$/.test(value) ? Number(value) : value;
  }
  return out;
}

/** Names of every command the app registers (from the generated bindings) and the test commands. */
function knownCommands() {
  const bindings = fs.readFileSync(path.join(REPO, "app", "src", "generated", "bindings.ts"), "utf8");
  const names = new Set([...bindings.matchAll(/__TAURI_INVOKE\("([a-z0-9_]+)"/g)].map((m) => m[1]));
  for (const name of ["e2e_window_controls", "e2e_native_mouse", "e2e_opened", "e2e_invoked"]) names.add(name);
  return names;
}

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

async function mailboxes() {
  return coreCall("list_mailboxes", { accountId: (await onlyAccount()).id });
}

/** Every message of a mailbox, newest first, as the list shows them. */
async function allMessages(mailboxId) {
  const items = [];
  let cursor = null;
  do {
    const page = await coreCall("list_messages", { mailboxId, cursor, limit: 500 });
    items.push(...page.items);
    cursor = page.nextCursor;
  } while (cursor !== null);
  return items;
}

const opened = () => coreCall("e2e_opened");

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `theme ${theme} not applied`,
  });
}

/**
 * Screenshots of the current state in light and dark; ends in light. `probe` (a page function)
 * is evaluated right before each picture and returned, to show what the picture should contain.
 */
async function shootBothThemes(name, probe = null) {
  const seen = [];
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    if (probe) seen.push(await browser.execute(probe));
    report.screenshots.push(await screenshot(`reading-${name}-${theme}`));
  }
  await setTheme("light");
  return seen;
}

let cspSeen = 0;
async function newCspViolations() {
  const all = await cspViolations();
  const fresh = all.slice(cspSeen);
  cspSeen = all.length;
  return fresh;
}

/** A real (trusted) left click at a point of the main frame's viewport. */
async function nativeClick(point) {
  const viewportHeight = await browser.execute(() => innerHeight);
  return coreCall("e2e_native_mouse", { x: point.x, y: point.y, viewportHeight });
}

// --- page helpers ------------------------------------------------------------------------------

/**
 * Helpers in the app window, under `window.__e2e`. The reading pane is read from the app's own
 * document; the mail frame through `contentDocument` (same origin, `allow-same-origin`), which
 * gives the parent read access only: nothing runs in the frame (decision 0005).
 */
function installPageHelpers() {
  return browser.execute(() => {
    window.__e2eMessages = [];
    if (!window.__e2eMessageListener) {
      // A script in a mail frame would report through postMessage (the corpus vectors do).
      window.__e2eMessageListener = (event) => {
        let data;
        try {
          data = JSON.parse(JSON.stringify(event.data));
        } catch {
          data = String(event.data);
        }
        window.__e2eMessages.push({ data, origin: event.origin });
      };
      window.addEventListener("message", window.__e2eMessageListener);
    }
    const $ = (selector, root = document) => root.querySelector(selector);
    window.__e2e = {
      paneEl: () => $('[data-testid="reading-pane"]'),
      frame: () => $('[data-testid="reading-pane"] [data-testid="mail-frame"]'),
      row(id) {
        return [...document.querySelectorAll('[data-testid="message-list"] [role="option"]')].find((o) => o.id.endsWith(`-${id}`)) ?? null;
      },
      /** What the reading pane shows, as plain data. */
      pane() {
        const pane = this.paneEl();
        if (!pane) return null;
        const article = $("article", pane);
        const frame = this.frame();
        const doc = frame?.contentDocument ?? null;
        const textBody = $('[data-testid="text-body"]', pane);
        const header = $("article h2", pane);
        const unavailable = $('[data-testid="reading-unavailable"]', pane);
        const error = $('[data-testid="reading-error"], [data-testid="reading-fetch-failed"]', pane);
        let kind = null;
        if (frame) kind = "html";
        else if (textBody) kind = "text";
        else if (unavailable) kind = "unavailable";
        else if (error) kind = "error";
        return {
          subject: header?.textContent ?? null,
          from: $('[data-testid="reading-from"]', pane)?.textContent ?? null,
          busy: article?.getAttribute("aria-busy") === "true",
          // A view arrived, but its body is still being fetched (the skeleton under the head).
          loading: Boolean(header && $('[data-testid="reading-skeleton"]', pane)),
          kind,
          frameReady: Boolean(frame && doc && doc.URL === "about:srcdoc" && frame.style.height !== ""),
          frameHeight: frame ? Math.round(frame.getBoundingClientRect().height) : null,
          text: (frame ? doc?.body?.innerText : textBody?.textContent)?.trim() ?? "",
          unavailable: unavailable?.textContent ?? null,
          // Not `error`: webdriverio takes a returned object with `error` for a WebDriver error.
          errorText: error?.textContent ?? null,
          problems: [...pane.querySelectorAll('[data-testid="reading-problems"] li')].map((li) => li.textContent),
          remoteBar: $('[data-testid="remote-content-bar"] p', pane)?.textContent ?? null,
          attachments: pane.querySelectorAll('[data-testid="attachment"]').length,
        };
      },
      rendered(p) {
        if (!p || p.loading) return false;
        if (p.kind === "html") return p.frameReady;
        return p.kind !== null;
      },
      /**
       * Clicks the row of message `id` (a synthetic click, like the WebDriver's own) and waits
       * until the reading pane shows that message rendered. `ms`: click to rendered, measured in
       * the page (a MutationObserver and the frame's load event, so no polling interval).
       */
      async open(id, timeoutMs) {
        const row = this.row(id);
        if (!row) return { failure: "the row is not rendered" };
        row.scrollIntoView({ block: "nearest" });
        await new Promise((resolve) => setTimeout(resolve, 30));
        const old = this.paneEl();
        return new Promise((resolve) => {
          let sawLoading = false;
          let timer = null;
          let observer = null;
          const finish = (extra) => {
            observer?.disconnect();
            document.removeEventListener("load", check, true);
            clearTimeout(timer);
            resolve({ ...extra, sawLoading, pane: this.pane() });
          };
          const check = () => {
            const el = this.paneEl();
            if (!el || el === old) return;
            const p = this.pane();
            if (p.loading) sawLoading = true;
            if (this.rendered(p)) finish({ ms: performance.now() - t0 });
          };
          observer = new MutationObserver(check);
          observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
          document.addEventListener("load", check, true);
          timer = setTimeout(() => finish({ failure: "timeout" }), timeoutMs);
          const t0 = performance.now();
          this.row(id).click();
          check();
        });
      },
      /** Scrolls the list until the row of message `id` (at list position `position`) is rendered. */
      async reveal(id, position) {
        const list = $('[data-testid="message-list"]');
        for (let i = 0; i < 200 && !this.row(id); i += 1) {
          const options = [...list.querySelectorAll('[role="option"]')];
          const first = options[0];
          const last = options[options.length - 1];
          const firstPos = Number(first?.getAttribute("aria-posinset") ?? 1);
          const lastPos = Number(last?.getAttribute("aria-posinset") ?? 1);
          const height = first ? first.getBoundingClientRect().height : 44;
          if (position + 1 > lastPos) list.scrollTop += Math.min(list.clientHeight, (position + 1 - lastPos) * height + height);
          else if (position + 1 < firstPos) list.scrollTop -= Math.min(list.clientHeight, (firstPos - position - 1) * height + height);
          list.dispatchEvent(new Event("scroll"));
          await new Promise((resolve) => setTimeout(resolve, 40));
        }
        const row = this.row(id);
        row?.scrollIntoView({ block: "center" });
        return Boolean(row);
      },
      /** Script markers: in the app window, in the frame, the title, the URLs. */
      detectors() {
        const frame = this.frame();
        let frameMarker;
        let frameTauri = null;
        try {
          frameMarker = frame?.contentWindow?.__vmRan ?? null;
          frameTauri = typeof frame?.contentWindow?.__TAURI_INTERNALS__;
        } catch (e) {
          frameMarker = `unreadable: ${e}`;
        }
        return {
          messages: window.__e2eMessages.slice(),
          title: document.title,
          href: location.href,
          frameUrl: frame?.contentDocument?.URL ?? null,
          frameMarker,
          frameTauri,
          sandbox: frame?.getAttribute("sandbox") ?? null,
          appFrames: document.querySelectorAll("iframe").length,
        };
      },
      /** What survived the sanitizer in the frame. */
      frameContent() {
        const doc = this.frame()?.contentDocument;
        if (!doc) return null;
        const metas = [...doc.querySelectorAll("meta")].map((m) => ({ httpEquiv: m.getAttribute("http-equiv"), name: m.getAttribute("name"), content: m.getAttribute("content") }));
        return {
          metas,
          forbidden: [...doc.querySelectorAll("script, iframe, frame, object, embed, applet, link, base, form, input, button, select, textarea, svg, math, video, audio, portal")].map((e) => e.tagName.toLowerCase()),
          eventAttributes: [...doc.querySelectorAll("*")].flatMap((e) => [...e.attributes].filter((a) => /^on/i.test(a.name)).map((a) => `${e.tagName.toLowerCase()}[${a.name}]`)),
          anchors: [...doc.querySelectorAll("a")].map((a) => ({
            href: a.getAttribute("href"),
            attributes: [...a.attributes].map((x) => x.name),
            text: a.textContent.trim().slice(0, 60),
          })),
          // As the corpus counts them (crates/mail-content/tests/hostile_corpus.rs): every
          // data: image in the document, in <img> and in CSS, without the blocked-image pixel.
          dataImages:
            doc.documentElement.outerHTML.split("data:image/").length -
            doc.documentElement.outerHTML.split("data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7").length,
          images: [...doc.querySelectorAll("img")].map((img) => (img.getAttribute("src") ?? "").slice(0, 30)),
        };
      },
      /**
       * Where a real click on link `index` lands: in the frame (`kind` "frame", an `<a>`) or in a
       * plain-text body ("text", a link segment). Scrolls the pane so the link is in view and
       * checks with elementFromPoint that the point hits it.
       */
      async linkPoint(kind, index) {
        const frame = this.frame();
        const doc = kind === "frame" ? frame?.contentDocument : document;
        const el = kind === "frame" ? doc?.querySelectorAll("a")[index] : document.querySelectorAll('[data-testid="text-body"] [role="link"]')[index];
        if (!el) return { failure: "no such link" };
        const box = () => {
          const offset = kind === "frame" ? frame.getBoundingClientRect() : { left: 0, top: 0 };
          const rects = [...el.getClientRects(), ...[...el.querySelectorAll("*")].map((c) => c.getBoundingClientRect())];
          const r = rects.find((x) => x.width >= 1 && x.height >= 1);
          if (!r) return null;
          const local = { x: r.left + Math.min(r.width / 2, 10), y: r.top + r.height / 2 };
          return { x: offset.left + local.x, y: offset.top + local.y, local };
        };
        let b = box();
        if (!b) return { failure: "the link has no box" };
        const scroller = $('[data-testid="reading-scroller"]');
        const sr = scroller.getBoundingClientRect();
        if (b.y < sr.top + 10 || b.y > sr.bottom - 10) {
          scroller.scrollTop += b.y - (sr.top + sr.height / 2);
          await new Promise((resolve) => setTimeout(resolve, 60));
          b = box();
        }
        const hit = document.elementFromPoint(b.x, b.y);
        let hits;
        if (kind === "frame") {
          const inner = doc.elementFromPoint(b.local.x, b.local.y);
          hits = hit === frame && (inner === el || el.contains(inner));
        } else {
          hits = hit === el || el.contains(hit);
        }
        return { x: b.x, y: b.y, hits, hit: hit?.tagName ?? null };
      },
      /** The link card, as plain data. */
      linkCard() {
        const card = $('[data-testid="link-card"]');
        if (!card) return null;
        return {
          title: card.querySelector("p")?.textContent ?? null,
          target: $('[data-testid="link-target"]', card)?.textContent ?? null,
          host: $('[data-testid="link-host"]', card)?.textContent ?? null,
          warnings: [...card.querySelectorAll("[data-warning]")].map((w) => w.dataset.warning),
          open: $('[data-testid="link-open"]', card)?.textContent ?? null,
        };
      },
    };
  });
}

const pane = () => browser.execute(() => window.__e2e.pane());
const detectors = () => browser.execute(() => window.__e2e.detectors());
const frameContent = () => browser.execute(() => window.__e2e.frameContent());

/** Opens message `id` by a click on its row; waits up to `timeoutMs` for it to be rendered. */
async function openMessage(id, { position = null, timeoutMs = 20000 } = {}) {
  if (position !== null) {
    const shown = await browser.execute((m, p) => window.__e2e.reveal(m, p), id, position);
    assert.ok(shown, `the row of message ${id} (position ${position}) could not be scrolled into the list`);
  }
  const result = await browser.execute((m, t) => window.__e2e.open(m, t), id, timeoutMs);
  if (result.failure) throw new Error(`message ${id} did not open: ${result.failure}; pane: ${JSON.stringify(result.pane)}`);
  return result;
}

async function closeLinkCard() {
  const closed = await browser.execute(() => {
    const button = document.querySelector('[data-testid="link-card"] [data-testid="link-cancel"]');
    button?.click();
    return Boolean(button);
  });
  if (closed) await browser.waitUntil(async () => (await browser.execute(() => window.__e2e.linkCard())) === null, { timeout: 3000 });
}

/**
 * Native click on a link; waits for the link card with the checked target. With `expectCard`
 * false (an <a> the sanitizer left without href) it checks that no card appears.
 */
async function clickLink(kind, index, expectCard = true) {
  await closeLinkCard();
  const point = await browser.execute((k, i) => window.__e2e.linkPoint(k, i), kind, index);
  if (point.failure) return { skipped: point.failure };
  if (!point.hits) return { skipped: `the click point hits ${point.hit}, not the link` };
  const native = await nativeClick(point);
  if (!expectCard) {
    await browser.pause(CLICK_SETTLE_MS);
    const card = await browser.execute(() => window.__e2e.linkCard());
    return card === null ? { noCard: true, point } : { error: `a link card for text without a link: ${JSON.stringify(card)}` };
  }
  let card = null;
  try {
    await browser.waitUntil(
      async () => {
        card = await browser.execute(() => window.__e2e.linkCard());
        return card !== null && card.title !== "Checking the link…" && (card.target !== null || /could not|out of date/.test(card.title));
      },
      { timeout: 5000, interval: 50 },
    );
  } catch {
    return { error: `no link card after a real click (${native}); card: ${JSON.stringify(card)}` };
  }
  return { card, point };
}

async function showInbox() {
  await testId("rail-inbox").click();
  await browser.waitUntil(async () => (await testId("rail-inbox").getAttribute("aria-current")) === "page");
  await testId("message-list").waitForExist();
}

/** Opens a folder through the unfolded sidebar's tree, then folds the sidebar again. */
async function showMailbox(mailbox) {
  await testId("rail-folders").click();
  await testId(`mailbox-${mailbox.id}`).waitForExist();
  await testId(`mailbox-${mailbox.id}`).click();
  await browser.waitUntil(async () => (await browser.execute(() => document.querySelector("main header h1")?.textContent)) === mailbox.name, {
    timeoutMsg: `folder ${mailbox.path} did not open`,
  });
  await testId("rail-folders").click();
  await testId("message-list").waitForExist();
}

function stats(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)];
  const round = (v) => Math.round(v * 10) / 10;
  return { n: sorted.length, min: round(sorted[0]), median: round(at(0.5)), p95: round(at(0.95)), max: round(sorted.at(-1)) };
}

// --- the spec ----------------------------------------------------------------------------------

describe("reading mail (stage 3)", function () {
  let counterServer = null;
  let account;
  let hostile;
  let broken;
  let inbox;
  /** Corpus file name → its message in the Hostile folder and the facts from its headers. */
  const corpus = new Map();
  let known;
  let invokedBefore;
  let initialHref;

  before(async () => {
    report.screenshots = [];
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    await installPageHelpers();
    initialHref = (await detectors()).href;
    report.visibilityState = await browser.execute(() => document.visibilityState);
    known = knownCommands();
    counterServer = await startCounter();

    // The corpus and a control mail of another sender into "Hostile", the broken seed into "Broken".
    const controls = path.join(dataDir(), "controls");
    fs.mkdirSync(controls, { recursive: true });
    const controlFile = path.join(controls, "other-sender.eml");
    fs.writeFileSync(controlFile, CONTROL_MAIL);
    const files = fs.readdirSync(CORPUS_DIR).filter((f) => f.endsWith(".eml")).sort();
    assert.equal(files.length, 19, "the hostile corpus has 19 files");
    const appended = await appendMessages(HOSTILE, [...files.map((f) => path.join(CORPUS_DIR, f)), controlFile]);
    assert.equal(appended.messages, 20);
    const copied = await copySeedMessages("broken/", BROKEN);
    report.brokenSeed = copied.copied.map((c) => `${c.key} (${c.folder})`);
    assert.equal(copied.copied.length, 26, "the seed has 26 broken/* messages");

    await coreCall("add_account", {
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
        const a = await onlyAccount();
        if (a.status.state !== "idle" || a.status.lastSyncAt === null) return false;
        const boxes = await mailboxes();
        return boxes.find((m) => m.path === HOSTILE)?.total === 20 && boxes.find((m) => m.path === BROKEN)?.total === 26;
      },
      { timeout: 120000, interval: 200, timeoutMsg: "the account did not sync the test folders" },
    );
    account = await onlyAccount();
    assert.equal(account.writesEnabled, false, "new accounts are read-only");
    const boxes = await mailboxes();
    hostile = boxes.find((m) => m.path === HOSTILE);
    broken = boxes.find((m) => m.path === BROKEN);
    inbox = boxes.find((m) => m.role === "inbox");

    const items = await allMessages(hostile.id);
    for (const file of [...files, "other-sender.eml"]) {
      const headers = headersOf(file === "other-sender.eml" ? controlFile : path.join(CORPUS_DIR, file));
      const item = items.find((i) => i.subject === headers.subject);
      assert.ok(item, `${file}: no message with the subject "${headers.subject}" in ${HOSTILE}`);
      corpus.set(file, { item, position: items.indexOf(item), headers, check: corpusCheck(headers) });
    }

    // Prefetch (30 days of Inbox, Sent, Drafts) has to be complete for the timing and offline tests.
    const started = Date.now();
    let status;
    await browser.waitUntil(
      async () => {
        status = await coreCall("get_prefetch_status", { accountId: account.id });
        return status.state === "idle" && status.total > 0 && status.done === status.total;
      },
      { timeout: 180000, interval: 250, timeoutMsg: "the prefetch did not finish" },
    );
    report.prefetch = { ...status, waitedMs: Date.now() - started, settings: await coreCall("get_prefetch_settings") };
    invokedBefore = await coreCall("e2e_invoked");
    await newCspViolations();
    assert.deepEqual(await cspViolations(), [], "CSP violations before the reading tests");
  });

  beforeEach(() => {
    failures.length = 0;
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    await counterServer?.stop();
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "reading-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("opens a stored message in under 100 ms (click to head and body in the page)", async () => {
    await showInbox();
    const items = (await coreCall("list_messages", { mailboxId: inbox.id, cursor: null, limit: TIMING_SAMPLES + 20 })).items;
    // A preview exists once the body was parsed: these are stored (prefetched), none was opened.
    const local = items.slice(0, TIMING_SAMPLES).filter((i) => i.preview !== null);
    report.timingNotLocal = items.slice(0, TIMING_SAMPLES).filter((i) => i.preview === null).map((i) => i.subject);
    assert.ok(local.length >= 30, `only ${local.length} of the first ${TIMING_SAMPLES} inbox messages are stored`);
    const samples = [];
    for (const item of local) {
      const result = await openMessage(item.id, { position: items.indexOf(item), timeoutMs: 5000 });
      samples.push({
        ms: Math.round(result.ms * 10) / 10,
        kind: result.pane.kind,
        sawLoading: result.sawLoading,
        remoteBar: result.pane.remoteBar,
        subject: item.subject.slice(0, 50),
        id: item.id,
      });
    }
    const html = samples.filter((s) => s.kind === "html");
    report.timing = {
      what: "synthetic click on the row (in the page) until the pane shows head and rendered body: text in the DOM, or the frame loaded (about:srcdoc) and its height set; measured with performance.now() in the page, no paint",
      visibilityState: await browser.execute(() => document.visibilityState),
      all: stats(samples.map((s) => s.ms)),
      html: html.length ? stats(html.map((s) => s.ms)) : null,
      text: stats(samples.filter((s) => s.kind === "text").map((s) => s.ms)),
      first: samples[0],
      newsletters: html.filter((s) => s.remoteBar).map((s) => `${s.subject} (${s.ms} ms, ${s.remoteBar})`),
      samples,
    };
    assert.deepEqual(samples.filter((s) => s.sawLoading).map((s) => s.subject), [], "stored messages must not be fetched");
    assert.ok(html.length >= 3, `only ${html.length} HTML messages among the samples`);
    assert.ok(html.some((s) => s.remoteBar), "no HTML newsletter with blocked external images among the samples");
    assert.ok(report.timing.all.median < OPEN_LIMIT_MS, `median ${report.timing.all.median} ms`);
    assert.ok(report.timing.all.p95 < OPEN_LIMIT_MS, `p95 ${report.timing.all.p95} ms`);

    // Screenshots: an HTML newsletter with its blocked images, and a plain-text mail.
    const newsletter = html.find((s) => s.remoteBar);
    await openMessage(newsletter.id, { position: items.findIndex((i) => i.id === newsletter.id) });
    await shootBothThemes("html-newsletter");
    const text = samples.find((s) => s.kind === "text");
    await openMessage(text.id, { position: items.findIndex((i) => i.id === text.id) });
    await shootBothThemes("text-mail");
    assert.deepEqual(await newCspViolations(), [], "CSP violations while opening inbox messages");
  });

  it("shows every hostile corpus mail without a script, a connection or a navigation; real clicks show the real link target", async () => {
    await showMailbox(hostile);
    const results = {};
    for (const [file, entry] of corpus) {
      if (file === "other-sender.eml") continue;
      const where = file;
      await closeLinkCard();
      await browser.execute(() => {
        window.__e2eMessages.length = 0;
      });
      await counter.reset();
      const openedBefore = (await opened()).length;
      const shown = await openMessage(entry.item.id, { position: entry.position });
      await browser.pause(SETTLE_MS);
      const p = await pane();
      const d = await detectors();
      const content = p.kind === "html" ? await frameContent() : null;
      const hits = await counter.hits();
      const r = {
        kind: p.kind,
        fetched: shown.sawLoading,
        connections: hits.connections.length,
        requests: hits.requests.map((q) => `${q.port}${q.path}`),
        scriptMarkers: d.messages.length + (d.frameMarker === null ? 0 : 1) + (d.title === APP_TITLE ? 0 : 1),
        remoteBar: p.remoteBar,
        links: content ? content.anchors.length : null,
        clicks: [],
      };
      results[file] = r;

      // Rendered, and the expected content is there.
      if (!p.text.includes("SAFE-CONTENT-VISIBLE")) fail(where, `the safe content is not shown (${p.kind}): ${p.text.slice(0, 120)}`);
      // No script ran.
      if (d.messages.length) fail(where, `postMessage from the frame: ${JSON.stringify(d.messages)}`);
      if (d.frameMarker !== null) fail(where, `marker in the frame: ${d.frameMarker}`);
      if (d.title !== APP_TITLE) fail(where, `the app title changed to "${d.title}"`);
      if (d.href !== initialHref) fail(where, `the app navigated to ${d.href}`);
      // Nothing went out.
      if (hits.connections.length || hits.requests.length) fail(where, `counter: ${JSON.stringify(hits)}`);
      if ((await opened()).length !== openedBefore) fail(where, "something was opened without a click");
      if (p.kind === "html") {
        if (d.sandbox !== "allow-same-origin") fail(where, `sandbox="${d.sandbox}"`);
        if (d.frameUrl !== "about:srcdoc") fail(where, `the frame shows ${d.frameUrl}`);
        if (d.frameTauri !== "undefined") fail(where, `__TAURI_INTERNALS__ in the frame is ${d.frameTauri}`);
        const csp = content.metas.find((m) => m.httpEquiv?.toLowerCase() === "content-security-policy");
        if (!csp?.content?.startsWith("default-src 'none'")) fail(where, `no mandatory meta CSP: ${JSON.stringify(content.metas)}`);
        if (content.metas.length !== 2) fail(where, `metas besides CSP and referrer: ${JSON.stringify(content.metas)}`);
        if (content.forbidden.length) fail(where, `forbidden elements in the frame: ${content.forbidden.join(", ")}`);
        if (content.eventAttributes.length) fail(where, `event attributes in the frame: ${content.eventAttributes.join(", ")}`);
        const tokens = new Set();
        for (const a of content.anchors) {
          // No href: what the sanitizer made of a link it will not offer (unknown scheme).
          if (a.href === null) continue;
          const m = PLACEHOLDER.exec(a.href);
          if (!m) fail(where, `a link that is not a placeholder: ${a.href}`);
          else tokens.add(m[1]);
          const extra = a.attributes.filter((x) => x !== "href");
          if (extra.length) fail(where, `link attributes besides href: ${extra.join(", ")}`);
        }
        if (tokens.size > 1) fail(where, `links of ${tokens.size} renderings`);
        // `links=` of the corpus counts the core's link table, not <a> elements (an unknown
        // scheme keeps its entry but loses its href): reported, the clicks below check each one.
        r.linksInCore = entry.check.links ?? 0;
        if (entry.check["data-images"] !== undefined && content.dataImages !== entry.check["data-images"]) {
          fail(where, `${content.dataImages} data: images, the corpus says ${entry.check["data-images"]}`);
        }
        // The corpus counts distinct external addresses, the bar blocked elements (the same
        // pixel twice is two): both zero or both not.
        const blocked = Number(/^(\d+)/.exec(p.remoteBar ?? "")?.[1] ?? 0);
        r.blocked = blocked;
        if ((entry.check.remote ?? 0) > 0 !== blocked > 0) fail(where, `remote bar "${p.remoteBar}", the corpus lists ${entry.check.remote ?? 0} external addresses`);
      }
      if (entry.check.attachments !== undefined && p.attachments !== entry.check.attachments) {
        fail(where, `${p.attachments} attachments, the corpus says ${entry.check.attachments}`);
      }

      // Real clicks on every link: no navigation, no connection, the card with the real target.
      const linkCount = p.kind === "html" ? content.anchors.length : await browser.execute(() => document.querySelectorAll('[data-testid="text-body"] [role="link"]').length);
      if (p.kind === "text" && entry.check["text-links"] !== undefined && linkCount !== entry.check["text-links"]) {
        fail(where, `${linkCount} links in the text, the corpus says ${entry.check["text-links"]}`);
      }
      const targets = [];
      for (let i = 0; i < linkCount; i += 1) {
        await counter.reset();
        const textOnly = p.kind === "html" && content.anchors[i].href === null;
        const click = await clickLink(p.kind === "html" ? "frame" : "text", i, !textOnly);
        await browser.pause(CLICK_SETTLE_MS);
        const after = await detectors();
        const clickHits = await counter.hits();
        const line = { index: i };
        if (click.skipped) {
          line.skipped = click.skipped;
          r.clicks.push(line);
          continue;
        }
        if (click.error) {
          fail(where, `link ${i}: ${click.error}`);
          r.clicks.push({ ...line, error: click.error });
          continue;
        }
        if (click.noCard) {
          // Clicked, and nothing happened: no card, no navigation, no connection.
          line.text = content.anchors[i].text;
          line.noLink = true;
          if (clickHits.connections.length || clickHits.requests.length) fail(where, `link ${i}: counter ${JSON.stringify(clickHits)}`);
          if (after.href !== initialHref || after.frameUrl !== "about:srcdoc") fail(where, `link ${i}: navigated (${JSON.stringify(after)})`);
          r.clicks.push(line);
          continue;
        }
        line.target = click.card.target;
        line.warnings = click.card.warnings;
        line.open = click.card.open;
        targets.push(click.card.target);
        if (clickHits.connections.length || clickHits.requests.length) fail(where, `link ${i}: counter ${JSON.stringify(clickHits)}`);
        if (after.href !== initialHref) fail(where, `link ${i}: the app navigated to ${after.href}`);
        if (p.kind === "html" && after.frameUrl !== "about:srcdoc") fail(where, `link ${i}: the frame navigated to ${after.frameUrl}`);
        if (after.messages.length || after.frameMarker !== null || after.title !== APP_TITLE) fail(where, `link ${i}: a script ran (${JSON.stringify(after)})`);
        if (p.kind === "html") {
          // The card shows what the core knows for this placeholder: the real target.
          const m = PLACEHOLDER.exec(content.anchors[i].href);
          const info = await coreCall("describe_link", { renderToken: m[1], index: Number(m[2]) });
          if (info.target !== click.card.target) fail(where, `link ${i}: card shows ${click.card.target}, the core knows ${info.target}`);
          line.openable = info.openable;
        }
        if (file === "deceptive-links.eml" && i === 0) {
          const seen = await shootBothThemes("link-card-warning", () => ({
            card: window.__e2e.linkCard(),
            rect: document.querySelector('[data-testid="link-card"]')?.getBoundingClientRect().toJSON() ?? null,
            active: document.activeElement?.tagName ?? null,
          }));
          report.linkCardShot = { clicked: click.card, seen };
          if (seen.some((x) => x.card === null)) fail(where, `the link card was gone when photographed: ${JSON.stringify(seen)}`);
        }
        // "Open": only through the recording opener, with exactly the real target, nothing else.
        if ((OPEN_THROUGH_OPENER[file] ?? []).includes(click.card.target)) {
          const before = (await opened()).length;
          if (!click.card.open) fail(where, `link ${i}: no open button for ${click.card.target}`);
          else {
            await counter.reset();
            await browser.execute(() => document.querySelector('[data-testid="link-open"]').click());
            await browser.waitUntil(async () => (await opened()).length > before, { timeout: 5000, timeoutMsg: `${where}: "Open" handed nothing to the opener` });
            await browser.pause(CLICK_SETTLE_MS);
            const handed = (await opened()).slice(before);
            line.openedThrough = handed;
            if (handed.length !== 1 || handed[0] !== click.card.target) fail(where, `link ${i}: opener got ${JSON.stringify(handed)}, expected ${click.card.target}`);
            const openHits = await counter.hits();
            if (openHits.connections.length || openHits.requests.length) fail(where, `link ${i}: "Open" connected: ${JSON.stringify(openHits)}`);
            if (await browser.execute(() => document.querySelector('[data-testid="link-card"]') !== null)) fail(where, `link ${i}: the card stayed open after "Open"`);
          }
        }
        r.clicks.push(line);
      }
      await closeLinkCard();
      const expected = EXPECTED_TARGETS[file];
      if (expected && JSON.stringify(targets) !== JSON.stringify(expected)) fail(where, `link targets ${JSON.stringify(targets)}, expected ${JSON.stringify(expected)}`);
      if (file === "deceptive-links.eml") {
        const warned = r.clicks.filter((c) => c.warnings?.length).length;
        if (warned !== entry.check["warned-links"]) fail(where, `${warned} links with a warning, the corpus says ${entry.check["warned-links"]}`);
      }
      const csp = await newCspViolations();
      if (csp.length) fail(where, `CSP violations in the app window: ${JSON.stringify(csp)}`);
      r.cspViolations = csp.length;
    }
    report.corpus = results;
    // Commands that reached the app: only known ones, never the corpus's `sandbox_breach`.
    const invoked = await coreCall("e2e_invoked");
    report.unknownCommands = Object.keys(invoked).filter((name) => !known.has(name));
    if (report.unknownCommands.length) fail("commands", `unknown commands reached the app: ${report.unknownCommands.join(", ")}`);
    report.windows = (await browser.getWindowHandles()).length;
    if (report.windows !== 1) fail("windows", `${report.windows} windows`);
    assert.deepEqual(failures, [], failures.join("\n"));
  });

  it("loads the images of an allowed sender through the core, and only those; other senders stay blocked", async () => {
    const control = corpus.get("other-sender.eml");
    const tracking = corpus.get("tracking-pixels.eml");
    const css = corpus.get("css-url-import.eml");
    report.remote = {};

    // Another sender: blocked, nothing loads.
    await counter.reset();
    await openMessage(control.item.id, { position: control.position });
    await browser.pause(SETTLE_MS);
    let p = await pane();
    assert.equal(p.remoteBar, "2 external items blocked");
    assert.deepEqual(await counter.hits(), { connections: [], requests: [] }, "the other sender's mail loaded something");

    // The corpus sender: blocked until "Always load from corpus@hostile.example".
    await counter.reset();
    await openMessage(tracking.item.id, { position: tracking.position });
    await browser.pause(SETTLE_MS);
    p = await pane();
    assert.match(p.remoteBar ?? "", /^\d+ external items blocked$/);
    report.remote.trackingBlocked = p.remoteBar;
    assert.deepEqual(await counter.hits(), { connections: [], requests: [] });
    await shootBothThemes("remote-blocked");
    const label = await browser.execute(() => document.querySelector('[data-testid="remote-sender"]')?.textContent ?? null);
    assert.equal(label, `Always load from ${CORPUS_SENDER}`);
    await counter.reset();
    await browser.execute(() => document.querySelector('[data-testid="remote-sender"]').click());
    await browser.waitUntil(
      async () => {
        const now = await pane();
        return now.remoteBar === null && now.frameReady;
      },
      { timeout: 20000, interval: 100, timeoutMsg: "the images were not loaded after allowing the sender" },
    );
    await browser.pause(SETTLE_MS);
    let hits = await counter.hits();
    const content = await frameContent();
    const vectorOf = (q) => /^\/v\/([^/]+)\//.exec(q.path)?.[1] ?? q.path;
    report.remote.allowed = {
      connections: hits.connections.length,
      requests: hits.requests.map((q) => `${q.port}${q.path}`),
      userAgents: [...new Set(hits.requests.map((q) => q.userAgent))],
      referers: [...new Set(hits.requests.map((q) => q.referer))],
      cookies: [...new Set(hits.requests.map((q) => q.cookie))],
      dataImages: content.dataImages,
      senders: await coreCall("list_remote_content_senders", { accountId: account.id }),
    };
    assert.ok(hits.requests.length > 0, "positive control: the counter saw none of the allowed images");
    assert.ok(hits.connections.length > 0, "positive control: the counter saw no connection for the allowed images");
    assert.deepEqual(hits.requests.map(vectorOf).filter((v) => !TRACKING_IMAGE_VECTORS.includes(v)), [], "requests outside the mail's images");
    assert.ok(hits.requests.every((q) => q.port === 18999 && q.method === "GET"));
    assert.deepEqual(report.remote.allowed.referers, [null], "no referrer");
    assert.deepEqual(report.remote.allowed.cookies, [null], "no cookies");
    assert.deepEqual(report.remote.allowed.userAgents, ["Mozilla/5.0"], "the neutral user agent");
    assert.ok(content.dataImages > 0, "the frame shows no loaded image");
    assert.deepEqual(report.remote.allowed.senders, [CORPUS_SENDER]);
    await shootBothThemes("remote-allowed");

    // Another mail of the allowed sender loads its images by itself.
    await counter.reset();
    await openMessage(css.item.id, { position: css.position });
    await browser.pause(SETTLE_MS);
    hits = await counter.hits();
    p = await pane();
    report.remote.sameSenderOtherMail = { remoteBar: p.remoteBar, requests: hits.requests.map((q) => q.path) };
    assert.equal(p.remoteBar, null, "the allowed sender's other mail still blocks its images");
    assert.ok(hits.requests.length > 0, "the allowed sender's other mail loaded nothing");
    assert.ok(hits.requests.every((q) => q.path.startsWith("/v/css-")), JSON.stringify(report.remote.sameSenderOtherMail));

    // The other sender is still blocked; "Load once" loads its two images once.
    await counter.reset();
    await openMessage(control.item.id, { position: control.position });
    await browser.pause(SETTLE_MS);
    p = await pane();
    assert.equal(p.remoteBar, "2 external items blocked", "the other sender is no longer blocked");
    assert.deepEqual(await counter.hits(), { connections: [], requests: [] }, "the other sender's images loaded without a click");
    await browser.execute(() => document.querySelector('[data-testid="remote-once"]').click());
    await browser.waitUntil(async () => (await pane()).remoteBar === null, { timeout: 20000, timeoutMsg: "load once did nothing" });
    await browser.pause(SETTLE_MS);
    hits = await counter.hits();
    report.remote.once = hits.requests.map((q) => q.path).sort();
    assert.deepEqual(report.remote.once, ["/v/other-sender/a.png", "/v/other-sender/b.png"]);
    // "Once" stores nothing in the core: shown again, the UI shows its cached rendering (for
    // up to 30 s, without a new request), the core renders it blocked again.
    await openMessage(css.item.id, { position: css.position });
    await counter.reset();
    await openMessage(control.item.id, { position: control.position });
    await browser.pause(SETTLE_MS);
    report.remote.onceShownAgain = { remoteBar: (await pane()).remoteBar, hits: await counter.hits() };
    assert.deepEqual(report.remote.onceShownAgain.hits, { connections: [], requests: [] }, "shown again, the images were fetched again");
    const fresh = await coreCall("get_message", { messageId: control.item.id, format: "auto" });
    report.remote.onceCore = { blockedRemote: fresh.body.blockedRemote, remoteLoaded: fresh.body.remoteLoaded };
    assert.deepEqual(report.remote.onceCore, { blockedRemote: 2, remoteLoaded: false }, "load once was remembered by the core");
    assert.deepEqual(await counter.hits(), { connections: [], requests: [] });
    assert.deepEqual(await coreCall("list_remote_content_senders", { accountId: account.id }), [CORPUS_SENDER]);
    assert.deepEqual(await newCspViolations(), [], "CSP violations while loading external content");
  });

  it("opens attachments only on click, executables after a warning, as byte-identical copies", async () => {
    const entry = corpus.get("attachment-names.eml");
    const before = (await opened()).length;
    await openMessage(entry.item.id, { position: entry.position });
    await browser.pause(500);
    assert.equal((await opened()).length, before, "an attachment opened by itself");
    const list = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="attachment"]')].map((li) => ({
        index: Number(li.dataset.index),
        name: li.querySelector('[data-testid="attachment-name"]').textContent,
        executable: li.className.includes("border-warning"),
      })),
    );
    report.attachments = { names: list.map((a) => a.name), executable: list.filter((a) => a.executable).map((a) => a.name) };
    assert.equal(list.length, 27);
    const reserved = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i;
    for (const a of list) {
      const problems = [];
      if (!a.name) problems.push("empty");
      if (/[/\\:*?"<>|]/.test(a.name)) problems.push("separator or forbidden character");
      if (a.name.includes("..")) problems.push("..");
      // eslint-disable-next-line no-control-regex
      if (/[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u202a-\u202e\u2066-\u2069\ufeff]/.test(a.name)) problems.push("control, zero-width or bidi character");
      if (/[. ]$/.test(a.name) || /^[. ]/.test(a.name)) problems.push("leading or trailing dot or space");
      if (reserved.test(a.name)) problems.push("reserved device name");
      if (Buffer.byteLength(a.name) > 200) problems.push("longer than 200 bytes");
      if (a.name.normalize("NFC") !== a.name) problems.push("not NFC");
      if (problems.length) fail("attachment names", `${JSON.stringify(a.name)}: ${problems.join(", ")}`);
    }
    await shootBothThemes("attachments");

    const digests = new Set((await attachmentDigests(path.join(CORPUS_DIR, "attachment-names.eml"))).map((d) => d.sha256));
    const checkCopy = (file) => {
      const root = path.join(dataDir(), "cache", "attachments") + path.sep;
      assert.ok(file.startsWith(root), `opened ${file}, not a copy under ${root}`);
      const bytes = fs.readFileSync(file);
      const sha = crypto.createHash("sha256").update(bytes).digest("hex");
      assert.ok(digests.has(sha), `${file}: not byte-identical to any attachment of the original`);
      assert.equal(fs.statSync(file).mode & 0o222, 0, `${file} is writable`);
      return { file: path.relative(dataDir(), file), bytes: bytes.length, sha256: sha.slice(0, 16) };
    };
    const clickOpen = (index) =>
      browser.execute((i) => document.querySelectorAll(`[data-testid="attachment"][data-index="${i}"] button`)[1].click(), index);

    const plain = list.find((a) => !a.executable);
    await clickOpen(plain.index);
    await browser.waitUntil(async () => (await opened()).length === before + 1, { timeout: 10000, timeoutMsg: "the attachment did not open" });
    report.attachments.opened = checkCopy((await opened()).at(-1));

    const exe = list.find((a) => a.executable);
    assert.ok(exe, "the corpus has an executable attachment");
    await clickOpen(exe.index);
    await testId("attachment-warning").waitForDisplayed({ timeoutMsg: "no warning before an executable opens" });
    await browser.pause(500);
    assert.equal((await opened()).length, before + 1, "the executable opened before the confirmation");
    await shootBothThemes("attachment-warning");
    await browser.execute(() => document.querySelector('[data-testid="attachment-confirm"]').click());
    await browser.waitUntil(async () => (await opened()).length === before + 2, { timeout: 10000, timeoutMsg: "the confirmed executable did not open" });
    report.attachments.openedExecutable = checkCopy((await opened()).at(-1));
    assert.deepEqual(failures, [], failures.join("\n"));
  });

  it("shows every broken seed message readably or with a clear notice, and keeps running", async () => {
    await showMailbox(broken);
    const items = await allMessages(broken.id);
    assert.equal(items.length, 26);
    const rows = [];
    let shot = false;
    for (const [position, item] of items.entries()) {
      const result = await openMessage(item.id, { position });
      const p = result.pane;
      const row = {
        subject: item.subject.slice(0, 60),
        kind: p.kind,
        textLength: p.text.length,
        problems: p.problems,
        notice: p.unavailable ?? p.errorText ?? null,
        frameHeight: p.frameHeight,
        attachments: p.attachments,
      };
      rows.push(row);
      const where = `broken "${row.subject}"`;
      if (p.kind === "error") fail(where, `an error instead of the message: ${p.errorText}`);
      if (p.subject === null) fail(where, "no message head");
      // Never an empty view without a word: some text, or a notice that says why there is none.
      if (row.textLength === 0 && row.problems.length === 0 && row.notice === null) fail(where, "an empty view without a notice");
      if (!shot && p.problems.length > 0 && p.text.length > 0) {
        shot = true;
        await shootBothThemes("broken-notice");
      }
    }
    report.broken = rows;
    // Still alive: the core answers, the page is the app, one window.
    assert.equal((await onlyAccount()).email, ALICE.email);
    assert.equal((await detectors()).href, initialHref);
    assert.equal((await browser.getWindowHandles()).length, 1);
    const logs = fs.readdirSync(path.join(dataDir(), "logs")).map((f) => fs.readFileSync(path.join(dataDir(), "logs", f), "utf8")).join("");
    assert.doesNotMatch(logs, /panicked/, "a panic in the log");
    assert.ok(shot, "no broken message with a notice to photograph");
    assert.deepEqual(await newCspViolations(), [], "CSP violations while showing broken mail");
    assert.deepEqual(failures, [], failures.join("\n"));
  });

  it("opens prefetched messages while the server is stopped; one that is not stored says so", async () => {
    await showInbox();
    const page = await allMessages(inbox.id);
    // Stored by the prefetch and never opened in this run: after the timing samples.
    const stored = page.slice(TIMING_SAMPLES + 2, TIMING_SAMPLES + 40).filter((i) => i.preview !== null).slice(0, 6);
    const missing = page.slice(300).find((i) => i.preview === null);
    assert.equal(stored.length, 6, "not enough prefetched messages after the timing samples");
    assert.ok(missing, "no message without a stored body");

    await testmail("stop");
    try {
      await browser.waitUntil(async () => (await onlyAccount()).status.state === "offline", {
        timeout: 30000,
        interval: 100,
        timeoutMsg: "the account did not go offline",
      });
      const shown = [];
      for (const item of stored) {
        const result = await openMessage(item.id, { position: page.indexOf(item) });
        shown.push({ subject: item.subject.slice(0, 50), kind: result.pane.kind, ms: Math.round(result.ms), textLength: result.pane.text.length, fetched: result.sawLoading });
      }
      report.offline = { stored: shown };
      for (const s of shown) {
        assert.ok(s.kind === "html" || s.kind === "text", `offline: ${JSON.stringify(s)}`);
        assert.equal(s.fetched, false);
      }
      const result = await openMessage(missing.id, { position: page.indexOf(missing) });
      report.offline.missing = { subject: missing.subject.slice(0, 50), kind: result.pane.kind, notice: result.pane.unavailable };
      assert.equal(result.pane.kind, "unavailable");
      assert.match(result.pane.unavailable, /^This message is not on this computer yet/);
      assert.match(result.pane.unavailable, new RegExp(`as soon as Alice Archer is online again`));
      await shootBothThemes("offline-not-stored");
    } finally {
      await testmail("start");
    }
    // Back online, the waiting pane fetches the message by itself. The reading connection from
    // before the restart is dead and only checked after 60 s unused (reading/fetch.rs), so the
    // first fetch may fail with "connection lost"; then "Try again" (the user's way) fetches it.
    const restarted = Date.now();
    const shows = (p) => p !== null && (p.kind === "html" || p.kind === "text") && (p.kind !== "html" || p.frameReady);
    let last = null;
    const waitForBody = async (accept) => {
      try {
        await browser.waitUntil(
          async () => {
            last = { pane: await pane(), account: (await onlyAccount()).status.state };
            return accept(last.pane);
          },
          { timeout: 60000, interval: 250 },
        );
      } catch {
        throw new Error(`the message did not appear after the server came back: ${JSON.stringify(last)}`);
      }
    };
    await waitForBody((p) => shows(p) || p?.kind === "error");
    report.offline.afterRestart = { firstAttempt: last.pane.kind === "error" ? last.pane.errorText : last.pane.kind, ms: Date.now() - restarted };
    if (last.pane.kind === "error") {
      await shootBothThemes("fetch-failed-after-restart");
      await browser.execute(() => {
        const failed = document.querySelector('[data-testid="reading-fetch-failed"]');
        [...failed.querySelectorAll("button")].find((b) => b.textContent === "Try again").click();
      });
      await waitForBody(shows);
      report.offline.afterRestart.afterTryAgain = last.pane.kind;
    }
    assert.deepEqual(await newCspViolations(), [], "CSP violations offline");
  });

  it("has detectors that detect (positive controls)", async () => {
    report.controls = {};
    // 1. The counter sees a bare TCP connection the web view opens: WebKit preconnects for a
    //    <link rel=preconnect> even under a CSP (Spike D). The sanitizer removes <link> for that
    //    reason; here the test builds such a frame itself.
    await counter.reset();
    await browser.execute(() => {
      const frame = document.createElement("iframe");
      frame.setAttribute("sandbox", "allow-same-origin");
      frame.dataset.e2eControl = "preconnect";
      frame.srcdoc = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'"><link rel="preconnect" href="http://127.0.0.1:18997/">control`;
      document.body.append(frame);
    });
    await browser.waitUntil(async () => (await counter.hits()).connections.some((c) => c.port === 18997), {
      timeout: 5000,
      timeoutMsg: "the counter saw no connection for a preconnect in a frame: does it still count connections?",
    });
    report.controls.preconnect = (await counter.hits()).connections.length;
    await browser.execute(() => document.querySelector('[data-e2e-control="preconnect"]')?.remove());

    // 2. The postMessage detector receives messages.
    await browser.execute(() => {
      window.__e2eMessages.length = 0;
      window.postMessage({ vmRan: "control" }, "*");
    });
    await browser.waitUntil(async () => (await detectors()).messages.length === 1, { timeout: 2000, timeoutMsg: "the postMessage detector heard nothing" });

    // 3. The frame marker is readable from the app window: undefined above means no script set it.
    const entry = corpus.get("script-tags.eml");
    await showMailbox(hostile);
    await openMessage(entry.item.id, { position: entry.position });
    const marker = await browser.execute(() => {
      const win = window.__e2e.frame().contentWindow;
      win.__vmRan = "control";
      const seen = window.__e2e.detectors().frameMarker;
      delete win.__vmRan;
      return seen;
    });
    assert.equal(marker, "control", "the frame marker cannot be read from the app window");

    // 4. The command recorder sees a command nobody registered, as a script would send it.
    const breach = await invoke("sandbox_breach", { vector: "control" });
    const invoked = await coreCall("e2e_invoked");
    report.controls.sandboxBreach = { answer: breach, recorded: invoked.sandbox_breach ?? 0, before: invokedBefore.sandbox_breach ?? 0 };
    assert.ok(invoked.sandbox_breach >= 1, "the command recorder missed sandbox_breach");
    assert.ok("failure" in breach, "an unknown command was answered");

    // 5. The CSP recorder records a violation in the app window (an inline script).
    await browser.execute(() => {
      const s = document.createElement("script");
      s.textContent = "window.__e2eControlRan = true;";
      document.body.append(s);
      s.remove();
    });
    await browser.waitUntil(async () => (await cspViolations()).length > cspSeen, { timeout: 2000, timeoutMsg: "the CSP recorder saw nothing" });
    const csp = await newCspViolations();
    report.controls.csp = csp;
    assert.equal(csp.length, 1);
    assert.equal(await browser.execute(() => window.__e2eControlRan ?? false), false);
  });
});
